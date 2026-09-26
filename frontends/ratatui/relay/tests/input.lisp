(defpackage :lem-relay/tests/input
  (:use :cl :rove :lem-relay/relay :lem-relay/input))
(in-package :lem-relay/tests/input)

(defclass input-relay (relay) ()
  (:documentation "A relay for input tests: nothing is drawn, so it needs
no implementation around it."))

(defun drain ()
  "Empty Lem's event queue, so each test sees only what it queued.

`lem:receive-event' also runs what it takes off the queue, and a :resize
lays out windows, which needs an editor these tests do not start. It
dequeues first, so ignoring the error still discards the event."
  (loop :while (plusp (lem:event-queue-length))
        :do (ignore-errors (lem:receive-event 0))))

(defun press-clicks-of (relay)
  (lem-relay/input::press-clicks (relay-last-press relay)))

(defun next-event ()
  (lem:receive-event 0.5))

(defun queued-after (function)
  "How many events FUNCTION adds to Lem's queue."
  (drain)
  (let ((before (lem:event-queue-length)))
    (funcall function)
    (prog1 (- (lem:event-queue-length) before)
      (drain))))

;;; Keys

(deftest a-key-becomes-a-lem-key
  (let ((key (key-event (make-key-input "x" :ctrl t))))
    (ok (equal "x" (lem:key-sym key)))
    (ok (lem:key-ctrl key))
    (ng (lem:key-meta key))))

(deftest a-space-is-named-space
  (ok (equal "Space" (lem:key-sym (key-event (make-key-input " "))))))

(deftest shift-is-dropped-from-self-inserting-keys
  ;; The character already carries it: shift+a arrives as "A".
  (let ((key (key-event (make-key-input "A" :shift t))))
    (ok (equal "A" (lem:key-sym key)))
    (ng (lem:key-shift key))))

(deftest shift-stays-on-other-keys
  (ok (lem:key-shift (key-event (make-key-input "Up" :shift t)))))

(deftest meta-shift-letter-is-the-capital
  ;; M-S-a is Lem's M-A.
  (let ((key (key-event (make-key-input "a" :meta t :shift t))))
    (ok (equal "A" (lem:key-sym key)))
    (ok (lem:key-meta key))))

(deftest a-delivered-key-reaches-the-editor-queue
  (drain)
  (deliver (make-instance 'input-relay) (make-key-input "q"))
  (let ((event (next-event)))
    (ok (equal "q" (lem:key-sym event)))))

;;; C-] interrupts

(deftest c-right-bracket-interrupts-rather-than-queueing
  (drain)
  (let* ((relay (make-instance 'input-relay))
         (ready (bt2:make-semaphore))
         (thread (bt2:make-thread
                  (lambda ()
                    (handler-case (progn (bt2:signal-semaphore ready) (sleep 5) :finished)
                      (lem:editor-interrupt () :interrupted))))))
    (setf (relay-editor-thread relay) thread)
    (bt2:wait-on-semaphore ready)
    (deliver relay (make-key-input "]" :ctrl t :seq 6))
    (ok (eq :interrupted (bt2:join-thread thread)))
    (ok (zerop (lem:event-queue-length)) "no key queued")
    (ok (= 6 (relay-input-seq relay)) "still counted as delivered")))

(deftest c-g-stays-a-key
  (ok (= 1 (queued-after (lambda ()
                           (deliver (make-instance 'input-relay)
                                    (make-key-input "g" :ctrl t)))))))

;;; Mouse

(deftest a-click-is-a-mouse-button-down
  (drain)
  (let ((relay (make-instance 'input-relay)))
    (deliver relay (make-mouse-input :down 12 3 :button :left))
    (let ((event (next-event)))
      (ok (string= "MOUSE-BUTTON-DOWN" (symbol-name (type-of event)))))
    (ok (= 12 (relay-mouse-x relay)) "and Lem can ask where the mouse is")
    (ok (= 3 (nth-value 1 (lem-if:get-mouse-position relay))))))

(deftest each-mouse-action-is-one-event
  (let ((relay (make-instance 'input-relay)))
    (dolist (input (list (make-mouse-input :up 0 0 :button :right)
                         (make-mouse-input :move 1 1)
                         (make-mouse-input :wheel 1 1 :wheel-y -1)))
      (ok (= 1 (queued-after (lambda () (deliver relay input))))
          (string-downcase (symbol-name (lem-relay/input::mouse-input-action input)))))))

(deftest a-release-with-no-button-is-ignored
  (ok (zerop (queued-after (lambda ()
                             (deliver (make-instance 'input-relay)
                                      (make-mouse-input :up 0 0)))))))

;;; Counting clicks (ADR 0013): the display reports presses, the relay counts

(defun at (seconds)
  "SECONDS as microseconds, the unit of input times and `count-click'."
  (round (* seconds 1000000)))

(defun clicks-for (presses)
  "The click count of each press in PRESSES, (button x y seconds) each."
  (let ((relay (make-instance 'input-relay)))
    (mapcar (lambda (press)
              (destructuring-bind (button x y seconds) press
                (count-click relay button x y (at seconds))))
            presses)))

(deftest quick-presses-on-one-cell-count-up
  (ok (equal '(1 2 3) (clicks-for '((:button-1 5 5 0) (:button-1 5 5 0.2) (:button-1 5 5 0.4))))))

(deftest a-slow-second-press-starts-again
  (ok (equal '(1 1) (clicks-for '((:button-1 5 5 0) (:button-1 5 5 0.9))))))

(deftest a-press-elsewhere-starts-again
  (ok (equal '(1 1) (clicks-for '((:button-1 5 5 0) (:button-1 6 5 0.1))))))

(deftest another-button-starts-again
  (ok (equal '(1 1) (clicks-for '((:button-1 5 5 0) (:button-3 5 5 0.1))))))

(deftest the-interval-is-configurable
  (let ((*click-interval* 1.0))
    (ok (equal '(1 2) (clicks-for '((:button-1 5 5 0) (:button-1 5 5 0.9))))))
  (let ((*click-interval* 0.1))
    (ok (equal '(1 1) (clicks-for '((:button-1 5 5 0) (:button-1 5 5 0.2)))))))

(deftest a-delivered-double-click-reaches-lem-as-one
  ;; Delivered back to back, well inside the interval.
  (drain)
  (let ((relay (make-instance 'input-relay)))
    (deliver relay (make-mouse-input :down 3 3 :button :left))
    (deliver relay (make-mouse-input :down 3 3 :button :left))
    (ok (= 2 (press-clicks-of relay)))))

(deftest clicks-are-counted-by-when-the-display-saw-them
  ;; Arrival is irrelevant when presses carry their own time: here they
  ;; arrive far apart but were made close together, and vice versa.
  (let ((relay (make-instance 'input-relay)))
    (deliver relay (make-mouse-input :down 3 3 :button :left :time (at 10)))
    (sleep 0.6)
    (deliver relay (make-mouse-input :down 3 3 :button :left :time (at 10.2)))
    (ok (= 2 (press-clicks-of relay)) "slow to arrive, quick to make: a double click"))
  (let ((relay (make-instance 'input-relay)))
    (deliver relay (make-mouse-input :down 3 3 :button :left :time (at 10)))
    (deliver relay (make-mouse-input :down 3 3 :button :left :time (at 11)))
    (ok (= 1 (press-clicks-of relay)) "quick to arrive, slow to make: two singles"))
  (drain))

;;; Everything else

(deftest a-resize-records-the-size-and-tells-the-editor
  (let ((relay (make-instance 'input-relay)))
    (ok (= 1 (queued-after (lambda () (deliver relay (make-resize-input 120 40))))))
    (ok (= 120 (lem-if:display-width relay)))
    (ok (= 40 (lem-if:display-height relay)))))

(deftest a-paste-is-queued-for-the-editor
  (ok (= 1 (queued-after (lambda ()
                           (deliver (make-instance 'input-relay)
                                    (make-paste-input "pasted")))))))

(deftest a-clipboard-reply-answers-the-waiting-paste
  (let ((relay (make-instance 'input-relay)))
    ;; The request goes out as message 8; the reply names it.
    (setf (relay-sink relay) (lambda (message) (declare (ignore message)) 8))
    (deliver relay (make-clipboard-reply "from the display" :reply-to 8))
    (ok (equal "from the display" (lem-if:clipboard-paste relay)))))

(deftest a-reply-to-another-request-is-not-taken
  (let ((relay (make-instance 'input-relay)))
    (setf (relay-sink relay) (lambda (message) (declare (ignore message)) 8))
    (deliver relay (make-clipboard-reply "stale" :reply-to 5))
    (ok (null (lem-if:clipboard-paste relay)))))

(deftest delivering-an-input-records-its-seq
  (drain)
  (let ((relay (make-instance 'input-relay)))
    (deliver relay (make-key-input "a" :seq 3))
    (deliver relay (make-key-input "b"))
    (ok (= 3 (relay-input-seq relay)) "an input without a seq leaves it be")
    (deliver relay (make-key-input "c" :seq 4))
    (ok (= 4 (relay-input-seq relay))))
  (drain))

(deftest abort-interrupts-the-editor-thread
  (let* ((relay (make-instance 'input-relay))
         (ready (bt2:make-semaphore))
         (thread (bt2:make-thread
                  (lambda ()
                    (handler-case (progn (bt2:signal-semaphore ready)
                                         (sleep 5)
                                         :finished)
                      (lem:editor-interrupt () :interrupted)))
                  :name "stand-in editor thread")))
    (setf (relay-editor-thread relay) thread)
    (bt2:wait-on-semaphore ready)
    (deliver relay (make-abort-input))
    (ok (eq :interrupted (bt2:join-thread thread)))))

(deftest abort-before-the-editor-starts-is-harmless
  (ok (null (deliver (make-instance 'input-relay) (make-abort-input)))))
