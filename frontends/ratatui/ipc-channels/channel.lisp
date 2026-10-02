(defpackage :lem-ipc-channels/channel
  (:use :cl)
  (:local-nicknames (:framing :lem-ipc-channels/framing))
  (:export :channel
           :make-channel
           :channel-send
           :channel-receive
           :close-channel
           :start-reader
           :connect-local
           :now-microseconds))
(in-package :lem-ipc-channels/channel)

(defclass channel ()
  ((input :initarg :input :reader channel-input)
   (output :initarg :output :reader channel-output)
   (lock :initform (bt2:make-lock :name "lem-ipc-channels output") :reader channel-lock)
   (seq :initform 0 :accessor channel-seq)
   (closer :initarg :closer :initform nil :reader channel-closer))
  (:documentation "One peer process over a pair of octet streams. Messages
are framed length-delimited and numbered from 1 in wire order, whichever
thread sends (ADR 0013). What a message is belongs to the link: a channel
moves octet vectors."))

(defun make-channel (input output &key closer)
  "A channel reading the octet stream INPUT and writing OUTPUT. CLOSER, a
function of no arguments, is what `close-channel' calls to release what
the streams were made from; without one, closing does nothing, as for
stdio, which the process owns."
  (make-instance 'channel :input input :output output :closer closer))

(defun channel-send (channel encode)
  "Number the next message, write what ENCODE, a function of that number,
makes of it, and return the number. ENCODE runs under the channel's lock,
so it may keep state that must follow wire order."
  (bt2:with-lock-held ((channel-lock channel))
    (let ((seq (incf (channel-seq channel))))
      (framing:write-delimited (channel-output channel) (funcall encode seq))
      seq)))

(defun channel-receive (channel)
  "The next message from the peer, an octet vector, or NIL when it hung up."
  (framing:read-delimited (channel-input channel)))

(defun close-channel (channel)
  "End CHANNEL: a reader blocked on it sees the end, and later sends fail."
  (alexandria:when-let ((closer (channel-closer channel)))
    (ignore-errors (funcall closer))))

(defun start-reader (channel handler &key (name "lem-ipc-channels reader") on-close)
  "Read CHANNEL on a new thread named NAME, calling HANDLER with each
message, until the peer hangs up or the stream fails; then call ON-CLOSE.
An error in HANDLER is logged and reading goes on: one bad message does
not end the link. Returns the thread."
  (bt2:make-thread
   (lambda ()
     (unwind-protect
          (handler-case
              (loop :for body := (channel-receive channel)
                    :while body
                    :do (handler-case (funcall handler body)
                          (error (condition)
                            (log:error "~A: failed to handle a message: ~A" name condition))))
            (error (condition)
              (log:info "~A: the stream failed: ~A" name condition)))
       (when on-close
         (funcall on-close))))
   :name name))

(defun now-microseconds ()
  "Microseconds on this process's monotonic clock (ADR 0013)."
  (floor (* (get-internal-real-time) 1000000) internal-time-units-per-second))

(defun socket-channel (socket)
  "A channel on the connected local SOCKET. One fd-stream each way on the
socket's descriptor, so the reader and the writers never share a stream;
neither closes the descriptor, which the closer does with the socket."
  (let ((fd (sb-bsd-sockets:socket-file-descriptor socket)))
    (make-channel (sb-sys:make-fd-stream fd :input t :auto-close nil
                                            :element-type '(unsigned-byte 8)
                                            :buffering :full)
                  (sb-sys:make-fd-stream fd :output t :auto-close nil
                                            :element-type '(unsigned-byte 8)
                                            :buffering :full)
                  :closer (lambda ()
                            ;; Shutting down first wakes a reader blocked in
                            ;; read(), which closing alone does not. A beat
                            ;; afterwards lets that wakeup actually run its
                            ;; read() before the descriptor goes away under
                            ;; it: closing right on shutdown's heels can
                            ;; still win the race against a reader that
                            ;; hasn't reached its own read() yet, which some
                            ;; runtimes answer with EBADF instead of the
                            ;; clean end shutdown promised.
                            (ignore-errors (sb-bsd-sockets:socket-shutdown socket :direction :io))
                            (sleep 0.001)
                            (sb-bsd-sockets:socket-close socket)))))

(defun connect-local (path &key (timeout 1) (interval 0.02))
  "A channel to the peer listening on the local socket at PATH, or NIL when
none accepts within TIMEOUT seconds. Tried every INTERVAL seconds, since
the peer may still be starting."
  (loop :with deadline := (+ (get-internal-real-time)
                             (* timeout internal-time-units-per-second))
        :do (let ((socket (make-instance 'sb-bsd-sockets:local-socket :type :stream)))
              (handler-case
                  (progn
                    (sb-bsd-sockets:socket-connect socket (namestring path))
                    (return (socket-channel socket)))
                (sb-bsd-sockets:socket-error ()
                  (sb-bsd-sockets:socket-close socket)
                  (when (<= deadline (get-internal-real-time))
                    (return nil))
                  (sleep interval))))))
