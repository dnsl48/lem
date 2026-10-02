(defpackage :lem-ipc-channels/tests
  (:use :cl :rove)
  (:local-nicknames (:ipc :lem-ipc-channels)))
(in-package :lem-ipc-channels/tests)

(defun octets (&rest bytes)
  (coerce bytes '(vector (unsigned-byte 8))))

(defun delimited (&rest bodies)
  (let ((out (flexi-streams:make-in-memory-output-stream)))
    (dolist (body bodies)
      (ipc:write-delimited out body))
    (flexi-streams:get-output-stream-sequence out)))

(defun undelimited (octets)
  (let ((in (flexi-streams:make-in-memory-input-stream octets)))
    (loop :for body := (ipc:read-delimited in)
          :while body
          :collect body)))

(defun temporary-socket-path ()
  (format nil "~Alem-ipc-test-~36R.sock"
          (namestring (uiop:temporary-directory))
          (random (expt 36 8) (make-random-state t))))

(defun listen-local (path)
  (let ((listener (make-instance 'sb-bsd-sockets:local-socket :type :stream)))
    (sb-bsd-sockets:socket-bind listener path)
    (sb-bsd-sockets:socket-listen listener 1)
    listener))

(defun accepted-stream (listener)
  (sb-bsd-sockets:socket-make-stream (sb-bsd-sockets:socket-accept listener)
                                     :input t :output t
                                     :element-type '(unsigned-byte 8)
                                     :buffering :full))

;;; Framing

(deftest lengths-are-varints
  (ok (equalp (octets 3 1 2 3) (delimited (octets 1 2 3))) "one byte up to 127")
  (let ((body (make-array 300 :element-type '(unsigned-byte 8) :initial-element 7)))
    (ok (equalp (octets #xAC #x02) (subseq (delimited body) 0 2)) "300 is AC 02")
    (ok (equalp (list body) (undelimited (delimited body))))))

(deftest messages-round-trip-back-to-back
  (let ((a (octets 1)) (b (octets)) (c (octets 9 9)))
    (ok (equalp (list a b c) (undelimited (delimited a b c))) "the empty one included")))

(deftest the-end-between-messages-is-not-an-error
  (ok (null (ipc:read-delimited (flexi-streams:make-in-memory-input-stream (octets))))))

(deftest the-end-inside-a-prefix-or-message-is
  (ok (signals (ipc:read-delimited (flexi-streams:make-in-memory-input-stream (octets #x80)))))
  (ok (signals (ipc:read-delimited (flexi-streams:make-in-memory-input-stream (octets 5 1))))))

;;; Replies

(deftest a-reply-delivered-before-the-wait-is-found
  (let ((replies (ipc:make-replies)))
    (ipc:deliver-reply replies 3 "three")
    (ok (equal '("three" t) (multiple-value-list (ipc:await-reply replies 3 0.1))))))

(deftest no-reply-is-nil-after-the-timeout
  (let ((replies (ipc:make-replies))
        (start (get-internal-real-time)))
    (ok (equal '(nil nil) (multiple-value-list (ipc:await-reply replies 1 0.05))))
    (ok (<= 0.05 (/ (- (get-internal-real-time) start) internal-time-units-per-second))
        "it waited out the timeout")))

(deftest a-reply-from-another-thread-wakes-the-waiter
  (let ((replies (ipc:make-replies)))
    (bt2:make-thread (lambda ()
                       (sleep 0.05)
                       (ipc:deliver-reply replies 7 :in-time)))
    (ok (eq :in-time (ipc:await-reply replies 7 2)))))

(deftest a-reply-to-an-abandoned-request-is-discarded
  (let ((replies (ipc:make-replies)))
    (ipc:deliver-reply replies 1 "stale")
    (ipc:deliver-reply replies 2 "fresh")
    (ok (equal "fresh" (ipc:await-reply replies 2 0.1)))
    (ok (null (nth-value 1 (ipc:await-reply replies 1 0.01))) "not kept for later")))

;;; Channels

(defun in-memory-channel (&optional (input (octets)))
  (let ((out (flexi-streams:make-in-memory-output-stream)))
    (values (ipc:make-channel (flexi-streams:make-in-memory-input-stream input) out)
            out)))

(deftest the-first-send-is-numbered-1
  (let ((channel (in-memory-channel)))
    (ok (= 1 (ipc:channel-send channel (lambda (seq) (octets seq)))))
    (ok (= 2 (ipc:channel-send channel (lambda (seq) (octets seq)))))))

(deftest sends-are-numbered-in-wire-order-across-threads
  (multiple-value-bind (channel out) (in-memory-channel)
    (let ((threads (loop :repeat 4
                         :collect (bt2:make-thread
                                   (lambda ()
                                     (loop :repeat 50
                                           :do (ipc:channel-send
                                                channel
                                                (lambda (seq)
                                                  (flexi-streams:string-to-octets
                                                   (princ-to-string seq))))))))))
      (mapc #'bt2:join-thread threads)
      (ok (equal (alexandria:iota 200 :start 1)
                 (mapcar (lambda (body)
                           (parse-integer (flexi-streams:octets-to-string body)))
                         (undelimited (flexi-streams:get-output-stream-sequence out))))))))

(deftest the-reader-hands-over-each-message-then-says-it-closed
  (let ((received '())
        (closed (bt2:make-semaphore))
        (channel (in-memory-channel (delimited (octets 1) (octets 2)))))
    (ipc:start-reader channel (lambda (body) (push body received))
                      :on-close (lambda () (bt2:signal-semaphore closed)))
    (ok (bt2:wait-on-semaphore closed :timeout 2))
    (ok (equalp (list (octets 1) (octets 2)) (reverse received)))))

(deftest a-handler-error-does-not-end-the-reader
  (let ((received '())
        (closed (bt2:make-semaphore))
        (channel (in-memory-channel (delimited (octets 1) (octets 2)))))
    (ipc:start-reader channel
                      (lambda (body)
                        (when (equalp body (octets 1))
                          (error "a bad message"))
                        (push body received))
                      :on-close (lambda () (bt2:signal-semaphore closed)))
    (ok (bt2:wait-on-semaphore closed :timeout 2))
    (ok (equalp (list (octets 2)) received))))

(deftest nothing-listening-is-nil-not-an-error
  (ok (null (ipc:connect-local (temporary-socket-path) :timeout 0.1))))

(deftest a-local-socket-carries-messages-both-ways
  (let* ((path (temporary-socket-path))
         (listener (listen-local path)))
    (unwind-protect
         (let ((echo (bt2:make-thread
                      (lambda ()
                        (let ((stream (accepted-stream listener)))
                          (alexandria:when-let ((body (ipc:read-delimited stream)))
                            (ipc:write-delimited stream (reverse body)))
                          (close stream)))))
               (channel (ipc:connect-local path :timeout 1)))
           (ok channel)
           (ipc:channel-send channel (lambda (seq) (declare (ignore seq)) (octets 1 2 3)))
           (ok (equalp (octets 3 2 1) (ipc:channel-receive channel)))
           (ipc:close-channel channel)
           (bt2:join-thread echo))
      (sb-bsd-sockets:socket-close listener)
      (ignore-errors (delete-file path)))))

(deftest closing-a-socket-channel-ends-its-reader
  (let* ((path (temporary-socket-path))
         (listener (listen-local path)))
    (unwind-protect
         (let ((peer nil)
               (accepted (bt2:make-semaphore))
               (closed (bt2:make-semaphore)))
           (bt2:make-thread (lambda ()
                              (setf peer (accepted-stream listener))
                              (bt2:signal-semaphore accepted)))
           (let ((channel (ipc:connect-local path :timeout 1)))
             (bt2:wait-on-semaphore accepted :timeout 2)
             (ipc:start-reader channel #'identity
                               :on-close (lambda () (bt2:signal-semaphore closed)))
             (ipc:close-channel channel)
             (ok (bt2:wait-on-semaphore closed :timeout 2)
                 "a reader blocked in read sees the end"))
           (when peer (close peer)))
      (sb-bsd-sockets:socket-close listener)
      (ignore-errors (delete-file path)))))
