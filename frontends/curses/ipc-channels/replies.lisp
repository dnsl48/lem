(defpackage :lem-ipc-channels/replies
  (:use :cl)
  (:export :replies
           :make-replies
           :deliver-reply
           :await-reply))
(in-package :lem-ipc-channels/replies)

(defstruct (replies (:constructor make-replies ()))
  "Replies to requests sent on a channel, handed from the thread that
reads the channel to the one waiting. One waiter at a time: Lem's editor
thread, which asks and blocks until the answer or a deadline.

A reply names its request by the request's seq (ADR 0013). One that
arrives for a request whose waiter has given up is discarded by the next
wait rather than mistaken for that wait's answer."
  (lock (bt2:make-lock :name "lem-ipc-channels replies"))
  (arrived (bt2:make-condition-variable :name "lem-ipc-channels reply arrived"))
  (pending '()))

(defun deliver-reply (replies id value)
  "Hand VALUE, the reply to the request sent as ID, to whoever awaits it."
  (bt2:with-lock-held ((replies-lock replies))
    (setf (replies-pending replies)
          (nconc (replies-pending replies) (list (cons id value))))
    (bt2:condition-broadcast (replies-arrived replies))))

(defun take-reply (replies id)
  "With the lock held: pop pending replies up to the one for ID, discarding
the rest. (values VALUE T) when it was there, NIL otherwise."
  (loop :for (reply-id . value) := (pop (replies-pending replies))
        :while reply-id
        :when (eql reply-id id)
          :do (return (values value t))))

(defun await-reply (replies id timeout)
  "The reply to the request sent as ID, and T; or NIL and NIL when none
arrives within TIMEOUT seconds."
  (let ((deadline (+ (get-internal-real-time)
                     (* timeout internal-time-units-per-second))))
    (bt2:with-lock-held ((replies-lock replies))
      (loop
        (multiple-value-bind (value found) (take-reply replies id)
          (when found
            (return (values value t))))
        (let ((remaining (/ (- deadline (get-internal-real-time))
                            internal-time-units-per-second)))
          (unless (plusp remaining)
            (return (values nil nil)))
          (bt2:condition-wait (replies-arrived replies) (replies-lock replies)
                              :timeout (coerce remaining 'double-float)))))))
