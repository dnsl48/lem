(defpackage :lem-relay/input
  (:use :cl :lem-relay/relay)
  (:export :key-input
           :make-key-input
           :abort-input
           :make-abort-input
           :paste-input
           :make-paste-input
           :mouse-input
           :make-mouse-input
           :resize-input
           :make-resize-input
           :clipboard-reply
           :make-clipboard-reply
           :key-event
           :deliver))
(in-package :lem-relay/input)

;;; What the display sends (ADR 0011), as Lisp data a codec decodes into.

(defstruct (key-input (:constructor make-key-input
                          (name &key ctrl meta shift super)))
  "A key: NAME as Lem names keys (\"a\", \"Return\", \"F5\", \" \"...)
plus modifiers."
  name ctrl meta shift super)

(defstruct (abort-input (:constructor make-abort-input ()))
  "Stop what the editor is doing (C-g). A message of its own, not a key:
a queued key would wait behind the very work it is meant to stop.")

(defstruct (paste-input (:constructor make-paste-input (text)))
  "TEXT pasted into the terminal, inserted as the major mode pastes." text)

(defstruct (mouse-input (:constructor make-mouse-input
                            (action x y &key button (clicks 1) (wheel-x 0) (wheel-y 0))))
  "A mouse event at cell X, Y. ACTION is :down, :up, :move or :wheel;
BUTTON is :left, :middle, :right or NIL."
  action x y button clicks wheel-x wheel-y)

(defstruct (resize-input (:constructor make-resize-input (width height)))
  "The display is now WIDTH by HEIGHT cells." width height)

(defstruct (clipboard-reply (:constructor make-clipboard-reply (id text)))
  "The display's clipboard, answering request ID." id text)

;;; Delivery. Ported from lem-server's `input-callback'
;;; (frontends/server/main.lisp:792).

(defun key-event (input)
  "The Lem key for INPUT.

Shift is dropped from keys that insert themselves, since the character
already carries it, except with meta: M-S-a is Lem's M-A."
  (let* ((name (key-input-name input))
         (name (if (string= name " ") "Space" name))
         (inserts (lem:insertion-key-sym-p name)))
    (lem:make-key :ctrl (key-input-ctrl input)
                  :meta (key-input-meta input)
                  :super (key-input-super input)
                  :shift (and (not inserts) (key-input-shift input))
                  :sym (if (and inserts (key-input-shift input) (key-input-meta input))
                           (string-upcase name)
                           name))))

(defun mouse-button (button)
  (ecase button
    (:left :button-1)
    (:middle :button-2)
    (:right :button-3)
    ((nil) nil)))

(defgeneric deliver (relay input)
  (:documentation "Hand INPUT from the display to Lem.

Called on the thread reading the display, never the editor's. Everything
reaches the editor through its event queue, except the two that must
not wait in it: abort, which interrupts the editor thread, and a
clipboard reply, which the editor thread is blocked waiting for.")
  (:method ((relay relay) (input key-input))
    (lem:send-event (key-event input)))
  (:method ((relay relay) (input abort-input))
    (declare (ignore input))
    (alexandria:when-let ((thread (relay-editor-thread relay)))
      (lem:send-abort-event thread nil)))
  (:method ((relay relay) (input paste-input))
    (let ((text (paste-input-text input)))
      (lem:send-event
       (lambda ()
         (lem:paste-using-mode (lem:ensure-mode-object
                                (lem:current-major-mode-at-point (lem:current-point)))
                               text)))))
  (:method ((relay relay) (input mouse-input))
    ;; Lem's receive-mouse-* queue the event themselves. lem-server wraps
    ;; them in a send-event lambda, which queues them a second time, behind
    ;; anything that arrived in between; called directly they keep their
    ;; place. A cell is 1x1 "pixels" (lem-if:get-char-width), so pixel
    ;; coordinates are the cell's.
    (let ((x (mouse-input-x input))
          (y (mouse-input-y input))
          (button (mouse-button (mouse-input-button input))))
      (setf (relay-mouse-x relay) x
            (relay-mouse-y relay) y)
      (ecase (mouse-input-action input)
        (:down (when button
                 (lem:receive-mouse-button-down x y x y button (mouse-input-clicks input))))
        (:up (when button
               (lem:receive-mouse-button-up x y x y button)))
        (:move (lem:receive-mouse-motion x y x y button))
        (:wheel (lem:receive-mouse-wheel x y x y
                                         (mouse-input-wheel-x input)
                                         (mouse-input-wheel-y input))))))
  (:method ((relay relay) (input resize-input))
    (set-display-size relay (resize-input-width input) (resize-input-height input))
    (lem:send-event :resize))
  (:method ((relay relay) (input clipboard-reply))
    (clipboard-replied relay (clipboard-reply-id input) (clipboard-reply-text input))))
