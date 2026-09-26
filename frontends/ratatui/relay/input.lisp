(defpackage :lem-relay/input
  (:use :cl :lem-relay/relay)
  (:export :input
           :input-time
           :input-seq
           :key-input
           :make-key-input
           :key-input-name
           :key-input-ctrl
           :key-input-meta
           :key-input-shift
           :key-input-super
           :key-input-keypad
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
           :*click-interval*
           :count-click
           :deliver))
(in-package :lem-relay/input)

(defparameter *click-interval* 0.5
  "Seconds within which another press of the same button on the same cell
counts as a double, then triple, click. Set it in init.lisp:

  (setf lem-relay/input:*click-interval* 0.4)

The display reports every press as it happens and the relay counts them
(ADR 0013), since Lem acts on the count: two selects an expression,
three a form.")

;;; Frontend-local extensions to Lem's key vocabulary. Core needs no
;;; knowledge of the terminal protocol or of keypad provenance.
(lem:define-named-key "Keypad0")
(lem:define-named-key "Keypad1")
(lem:define-named-key "Keypad2")
(lem:define-named-key "Keypad3")
(lem:define-named-key "Keypad4")
(lem:define-named-key "Keypad5")
(lem:define-named-key "Keypad6")
(lem:define-named-key "Keypad7")
(lem:define-named-key "Keypad8")
(lem:define-named-key "Keypad9")
(lem:define-named-key "KeypadAdd")
(lem:define-named-key "KeypadSubtract")
(lem:define-named-key "KeypadMultiply")
(lem:define-named-key "KeypadDivide")
(lem:define-named-key "KeypadDecimal")

(defparameter *keypad-names*
  '(("0" . "Keypad0") ("1" . "Keypad1") ("2" . "Keypad2")
    ("3" . "Keypad3") ("4" . "Keypad4") ("5" . "Keypad5")
    ("6" . "Keypad6") ("7" . "Keypad7") ("8" . "Keypad8")
    ("9" . "Keypad9") ("+" . "KeypadAdd") ("-" . "KeypadSubtract")
    ("*" . "KeypadMultiply") ("/" . "KeypadDivide") ("." . "KeypadDecimal"))
  "Distinct names for modified keypad characters. Enter and navigation
keep their existing Lem names, and unmodified characters insert normally.")

;;; What the display sends (ADR 0011), as Lisp data a codec decodes into.

(defstruct (input (:constructor nil))
  "What every input has (ADR 0013). lem.relay.v1 carries both on every
message; either is NIL only for an input made in Lisp rather than
decoded, as tests make them:

TIME  when the display saw it, in microseconds on the display's monotonic
      clock; compared only with other display times.
SEQ   the display's number for the message it came in, counting from 1
      per session; with the session id it identifies the input."
  (time nil)
  (seq nil))

(defstruct (key-input (:include input)
                      (:constructor make-key-input
                          (name &key ctrl meta shift super keypad time seq)))
  "A key: NAME as Lem names keys (\"a\", \"Return\", \"F5\", \" \"...)
plus modifiers. KEYPAD records this event's provenance independently of
the terminal's advertised capabilities. Naming modified keypad keys is
the relay's responsibility, just as shift normalisation is."
  name ctrl meta shift super keypad)

(defstruct (abort-input (:include input)
                        (:constructor make-abort-input (&key time seq)))
  "Stop what the editor is doing (C-g). A message of its own, not a key:
a queued key would wait behind the very work it is meant to stop.")

(defstruct (paste-input (:include input)
                        (:constructor make-paste-input (text &key time seq)))
  "TEXT pasted into the terminal, inserted as the major mode pastes." text)

(defstruct (mouse-input (:include input)
                        (:constructor make-mouse-input
                            (action x y &key button (wheel-x 0) (wheel-y 0) time seq)))
  "A mouse event at screen cell X, Y. ACTION is :down, :up, :move or
:wheel; BUTTON is :left, :middle, :right or NIL. Wheel deltas are lines,
positive up and left, as Lem reads them. A press carries no click count:
the relay counts (`count-click')."
  action x y button wheel-x wheel-y)

(defstruct (press (:constructor make-press (button x y time clicks)))
  button x y time clicks)

(defstruct (resize-input (:include input)
                         (:constructor make-resize-input (width height &key time seq)))
  "The display is now WIDTH by HEIGHT cells." width height)

(defstruct (clipboard-reply (:include input)
                            (:constructor make-clipboard-reply
                                (text &key reply-to time seq)))
  "The display's clipboard TEXT, answering the request the relay sent as
REPLY-TO." text reply-to)

;;; Delivery. Ported from lem-server's `input-callback'
;;; (frontends/server/main.lisp:792).

(defun key-event (input)
  "The Lem key for INPUT.

Shift is folded into printable Ctrl/Meta shortcuts: C-S-g and C-G both
become C-G, distinct from C-g. Plain text keeps its received character;
named keys retain Shift. Modified keypad characters get their own names
from this event's provenance, without consulting session capabilities."
  (let* ((name (key-input-name input))
         (name (if (and (key-input-keypad input)
                        (or (key-input-ctrl input) (key-input-meta input)
                            (key-input-shift input) (key-input-super input)))
                   (or (cdr (assoc name *keypad-names* :test #'equal)) name)
                   name))
         (name (if (string= name " ") "Space" name))
         (inserts (lem:insertion-key-sym-p name)))
    (lem:make-key :ctrl (key-input-ctrl input)
                  :meta (key-input-meta input)
                  :super (key-input-super input)
                  :shift (and (not inserts) (key-input-shift input))
                  :sym (if (and inserts (key-input-shift input)
                                (or (key-input-ctrl input) (key-input-meta input)))
                           (string-upcase name)
                           name))))

(defun mouse-button (button)
  (ecase button
    (:left :button-1)
    (:middle :button-2)
    (:right :button-3)
    ((nil) nil)))

(defun now-microseconds ()
  (floor (* (get-internal-real-time) 1000000) internal-time-units-per-second))

(defun count-click (relay button x y time)
  "How many clicks a press of BUTTON at X, Y at TIME (microseconds) makes:
one more than the last press if that was the same button, on the same
cell, within `*click-interval*'; otherwise one."
  (let* ((last (relay-last-press relay))
         (clicks (if (and last
                          (eq button (press-button last))
                          (eql x (press-x last))
                          (eql y (press-y last))
                          (<= (- time (press-time last))
                              (* *click-interval* 1000000)))
                     (1+ (press-clicks last))
                     1)))
    (setf (relay-last-press relay) (make-press button x y time clicks))
    clicks))

(defun abort-key-p (input)
  "True for C-] and nothing else."
  (and (equal "]" (key-input-name input))
       (key-input-ctrl input)
       (not (or (key-input-meta input) (key-input-shift input) (key-input-super input)))))

(defgeneric deliver (relay input)
  (:documentation "Hand INPUT from the display to Lem.

Called on the thread reading the display, never the editor's. Everything
reaches the editor through its event queue, except the two that must
not wait in it: abort, which interrupts the editor thread, and a
clipboard reply, which the editor thread is blocked waiting for.")
  (:method ((relay relay) (input key-input))
    ;; C-] interrupts, as in ncurses and the browser client: the key for
    ;; an editor too busy to read its keys. C-g stays a key, so
    ;; keyboard-quit works as ever.
    (if (abort-key-p input)
        (deliver relay (make-abort-input :time (input-time input) :seq (input-seq input)))
        (lem:send-event (key-event input))))
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
                 (lem:receive-mouse-button-down
                  x y x y button
                  ;; When the display saw the press, not when it arrived
                  ;; (ADR 0013). An input without one falls back to arrival,
                  ;; which a local pipe makes nearly the same.
                  (count-click relay button x y
                               (or (input-time input) (now-microseconds))))))
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
    (clipboard-replied relay (clipboard-reply-reply-to input) (clipboard-reply-text input)))
  (:method :after ((relay relay) (input input))
    ;; What the next frame can reflect (ADR 0013). A plain assignment:
    ;; inputs are delivered in wire order, from the one reading thread.
    (alexandria:when-let ((seq (input-seq input)))
      (setf (relay-input-seq relay) seq))))
