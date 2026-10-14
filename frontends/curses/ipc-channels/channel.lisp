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
   (shutdown :initarg :shutdown :initform nil :reader channel-shutdown)
   (closer :initarg :closer :initform nil :reader channel-closer)
   (reader-started-p :initform nil :accessor channel-reader-started-p)
   (closed-p :initform nil :accessor channel-closed-p))
  (:documentation "One peer process over a pair of octet streams. Messages
are framed length-delimited and numbered from 1 in wire order, whichever
thread sends (ADR 0013). What a message is belongs to the link: a channel
moves octet vectors.

Closing is split into waking (SHUTDOWN) and releasing (CLOSER) so that
the descriptor is never closed while a reader thread might still be
polling it. Whether `close-channel' or the reader started by
`start-reader' is the one that releases it is decided once, atomically
under the channel's lock, by CLOSED-P and READER-STARTED-P together:
see those two functions."))

(defun make-channel (input output &key closer shutdown)
  "A channel reading the octet stream INPUT and writing OUTPUT. CLOSER, a
function of no arguments, is what releases what the streams were made
from; without one, closing does nothing, as for stdio, which the process
owns. SHUTDOWN, also a function of no arguments, wakes a reader blocked
on INPUT without releasing anything; without one, `close-channel' on a
channel with a running reader (`start-reader') just waits for that
reader to notice the stream has failed on its own."
  (make-instance 'channel :input input :output output :closer closer :shutdown shutdown))

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

(defun release-channel (channel)
  "Free what CHANNEL's streams were made from, via its CLOSER. A channel
with none (stdio, which the process owns, or an in-memory one, as in
tests) does nothing. Called exactly once per channel, by whichever of
`close-channel' or a reader thread started by `start-reader' the two
functions agreed, atomically, would be the one to call it."
  (alexandria:when-let ((closer (channel-closer channel)))
    (ignore-errors (funcall closer))))

(defun close-channel (channel)
  "End CHANNEL: a reader blocked on it sees the end, and later sends
fail. Closing an already-closed channel, or racing a `start-reader'
call on the same channel from another thread, is safe: the decision —
mark CHANNEL closed, wake it with SHUTDOWN, and release it immediately
unless a reader was already started on it — is made once, atomically
under CHANNEL's lock. A second close does nothing further; when a
reader was started first, that reader releases CHANNEL itself once its
own loop ends, never this function, so the descriptor is never closed
while that thread might still be polling it."
  (bt2:with-lock-held ((channel-lock channel))
    (unless (channel-closed-p channel)
      (setf (channel-closed-p channel) t)
      (alexandria:when-let ((shutdown (channel-shutdown channel)))
        (ignore-errors (funcall shutdown)))
      (unless (channel-reader-started-p channel)
        (release-channel channel)))))

(defun start-reader (channel handler &key (name "lem-ipc-channels reader") on-close)
  "Read CHANNEL on a new thread named NAME, calling HANDLER with each
message, until the peer hangs up or the stream fails; then release
CHANNEL (see `close-channel') and call ON-CLOSE. An error in HANDLER is
logged and reading goes on: one bad message does not end the link.

Whether CHANNEL is already closed and, if not, marking it as having a
reader are decided together, atomically under CHANNEL's lock, with
`close-channel' marking it closed: whichever of the two runs first
wins, so the two can never both think the other is responsible for
releasing CHANNEL. If CHANNEL is already closed, no thread is spawned —
ON-CLOSE is still called, synchronously, so a caller waiting on it is
not left hanging — and this returns NIL rather than a thread."
  (if (bt2:with-lock-held ((channel-lock channel))
        (if (channel-closed-p channel)
            t
            (progn (setf (channel-reader-started-p channel) t)
                   nil)))
      (progn
        (when on-close
          (funcall on-close))
        nil)
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
           (release-channel channel)
           (when on-close
             (funcall on-close))))
       :name name)))

(defun now-microseconds ()
  "Microseconds on this process's monotonic clock (ADR 0013)."
  (floor (* (get-internal-real-time) 1000000) internal-time-units-per-second))

(defun socket-channel (socket)
  "A channel on the connected local SOCKET. One fd-stream each way on the
socket's descriptor, so the reader and the writers never share a stream;
neither closes the descriptor. Waking and releasing SOCKET are kept
separate (see `close-channel'): shutting down wakes a reader blocked in
read() with a clean end, without touching the descriptor a poll of that
same read() may still be about to look at; closing frees it, and is left
to that reader to do once its own loop has actually ended, never to a
shutdown racing against it."
  (let ((fd (sb-bsd-sockets:socket-file-descriptor socket)))
    (make-channel (sb-sys:make-fd-stream fd :input t :auto-close nil
                                            :element-type '(unsigned-byte 8)
                                            :buffering :full)
                  (sb-sys:make-fd-stream fd :output t :auto-close nil
                                            :element-type '(unsigned-byte 8)
                                            :buffering :full)
                  :shutdown (lambda ()
                              (sb-bsd-sockets:socket-shutdown socket :direction :io))
                  :closer (lambda ()
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
