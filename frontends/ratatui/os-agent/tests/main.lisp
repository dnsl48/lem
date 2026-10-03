(defpackage :lem-os-agent/tests
  (:use :cl :rove)
  (:local-nicknames (:agent :lem-os-agent)
                    (:ipc :lem-ipc-channels)
                    (:pb :cl-protobufs.lem.os.v1)
                    (:proto :cl-protobufs)))
(in-package :lem-os-agent/tests)

(defun temporary-socket-path ()
  (format nil "~Alem-os-agent-test-~36R.sock"
          (namestring (uiop:temporary-directory))
          (random (expt 36 8) (make-random-state t))))

(defun to-editor (seq &rest message)
  (apply #'pb:make-to-editor :seq seq :time-us 0 message))

(defun welcome (reply-to &key (clipboard t) (version 1))
  (to-editor 1 :welcome (pb:make-welcome :protocol-version version
                                         :session-id "test"
                                         :reply-to reply-to
                                         :capabilities (pb:make-capabilities
                                                        :clipboard clipboard))))

(defun call-with-fake-agent (respond function)
  "Call FUNCTION with the path of a socket where a fake agent listens.
RESPOND is called with each `ToAgent' it receives and returns the
`ToEditor' messages to answer with, or :hang-up to close the connection.
The fake serves one connection."
  (let* ((path (temporary-socket-path))
         (listener (make-instance 'sb-bsd-sockets:local-socket :type :stream)))
    (sb-bsd-sockets:socket-bind listener path)
    (sb-bsd-sockets:socket-listen listener 1)
    (bt2:make-thread
     (lambda ()
       (let* ((peer (sb-bsd-sockets:socket-accept listener))
              (stream (sb-bsd-sockets:socket-make-stream peer :input t :output t
                                                              :element-type '(unsigned-byte 8)
                                                              :buffering :full)))
         (unwind-protect
              (loop :for body := (ipc:read-delimited stream)
                    :while body
                    :do (let ((answer (funcall respond
                                               (proto:deserialize-from-bytes 'pb:to-agent body))))
                          (when (eq answer :hang-up)
                            (return))
                          (dolist (message answer)
                            (ipc:write-delimited stream (proto:serialize-to-bytes message)))))
           (sb-bsd-sockets:socket-close peer))))
     :name "fake lem-os-agent")
    ;; The fake's thread is not joined: one whose client never connected
    ;; stays in accept, harmlessly, until the test process ends.
    (unwind-protect (funcall function path)
      (sb-bsd-sockets:socket-close listener)
      (ignore-errors (delete-file path)))))

(defmacro with-fake-agent ((path respond) &body body)
  `(call-with-fake-agent ,respond (lambda (,path) ,@body)))

(defun clipboard-agent (&key (text "from the agent") (clipboard t) set)
  "A RESPOND for a fake agent whose clipboard holds TEXT (NIL for nothing
readable). SET, when given, is called with each text Lem copies."
  (lambda (message)
    (let ((seq (pb:to-agent.seq message)))
      (case (pb:to-agent.message-case message)
        (pb:hello (list (welcome seq :clipboard clipboard)))
        (pb:clipboard-request
         (list (to-editor 2 :clipboard-reply
                          (apply #'pb:make-clipboard-reply :reply-to seq
                                 (and text (list :text text))))))
        (pb:set-clipboard
         (when set
           (funcall set (pb:set-clipboard.text (pb:to-agent.set-clipboard message))))
         '())))))

(defun welcome-only (message)
  "A RESPOND that welcomes, then never answers."
  (when (eq 'pb:hello (pb:to-agent.message-case message))
    (list (welcome (pb:to-agent.seq message)))))

;;; Connecting

(deftest connecting-learns-what-the-agent-can-do
  (with-fake-agent (path (clipboard-agent))
    (let ((agent (agent:connect path "test")))
      (ok agent)
      (ok (agent:connected-p agent))
      (ok (agent:capable-p :clipboard agent))
      (ok (not (agent:capable-p :tray agent)) "nothing it did not announce")
      (agent:disconnect agent))))

(deftest no-agent-is-nil
  (ok (null (agent:connect (temporary-socket-path) "test" :timeout 0.1))))

(deftest an-agent-that-never-welcomes-is-no-agent
  (with-fake-agent (path (lambda (message) (declare (ignore message)) '()))
    (ok (null (agent:connect path "test" :timeout 0.2)))))

(deftest a-welcome-without-a-protocol-version-is-no-agent
  (with-fake-agent (path (lambda (message)
                           (list (welcome (pb:to-agent.seq message) :version 0))))
    (ok (null (agent:connect path "test")))))

(deftest without-a-socket-start-does-nothing
  (ok (null (agent:start :path nil)))
  (ok (null (agent:start :path ""))))

;;; The clipboard

(deftest an-agent-without-a-clipboard-is-not-asked
  (with-fake-agent (path (clipboard-agent :clipboard nil))
    (let ((agent (agent:connect path "test")))
      (ok (not (agent:capable-p :clipboard agent)))
      (ok (null (agent:clipboard-text agent)))
      (ok (null (agent:set-clipboard-text "x" agent)))
      (agent:disconnect agent))))

(deftest the-clipboard-comes-from-the-agent
  (with-fake-agent (path (clipboard-agent :text "from the agent"))
    (let ((agent (agent:connect path "test")))
      (ok (equal "from the agent" (agent:clipboard-text agent)))
      (agent:disconnect agent))))

(deftest nothing-on-the-clipboard-is-nil
  (with-fake-agent (path (clipboard-agent :text nil))
    (let ((agent (agent:connect path "test")))
      (ok (null (agent:clipboard-text agent)))
      (agent:disconnect agent))))

(deftest copying-puts-text-on-the-agents-clipboard
  (let ((copied nil)
        (arrived (bt2:make-semaphore)))
    (with-fake-agent (path (clipboard-agent :set (lambda (text)
                                                   (setf copied text)
                                                   (bt2:signal-semaphore arrived))))
      (let ((agent (agent:connect path "test")))
        (ok (agent:set-clipboard-text "copied" agent))
        (ok (bt2:wait-on-semaphore arrived :timeout 2))
        (ok (equal "copied" copied))
        (agent:disconnect agent)))))

(deftest a-silent-agent-costs-only-the-request-timeout
  (with-fake-agent (path #'welcome-only)
    (let ((agent (agent:connect path "test"))
          (start (get-internal-real-time)))
      (ok (null (agent:clipboard-text agent)))
      (ok (< (/ (- (get-internal-real-time) start) internal-time-units-per-second) 1)
          "bounded by *request-timeout*")
      (agent:disconnect agent))))

(deftest when-the-agent-hangs-up-it-can-do-nothing
  (with-fake-agent (path (lambda (message)
                           (if (eq 'pb:hello (pb:to-agent.message-case message))
                               (list (welcome (pb:to-agent.seq message)))
                               :hang-up)))
    (let ((agent (agent:connect path "test")))
      (ok (agent:capable-p :clipboard agent))
      (ok (null (agent:clipboard-text agent)) "the request it hung up on")
      (sleep 0.2)
      (ok (not (agent:connected-p agent)))
      (ok (not (agent:capable-p :clipboard agent)))
      (ok (null (agent:set-clipboard-text "x" agent)) "and nothing is sent"))))
