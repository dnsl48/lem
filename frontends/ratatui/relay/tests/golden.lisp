(defpackage :lem-relay/tests/golden
  (:use :cl :rove :lem-relay/input)
  (:local-nicknames (:framing :lem-relay/protobuf/framing)
                    (:codec :lem-relay/protobuf/codec)
                    (:relay :lem-relay/relay)))
(in-package :lem-relay/tests/golden)

;;; The Rust → Lisp golden test. proto/fixtures/display-inputs.v1.bin is
;;; what the display sends for a fixed session, built by lem-ratatui's own
;;; conversions from crossterm events (rust/crates/lem-ratatui/src/golden.rs).
;;; Here it is decoded, and checked against what Lem receives.

(defun fixture-requests ()
  "Every message in the fixture, decoded, in order."
  (with-open-file (in (asdf:system-relative-pathname
                       "lem-relay" "../proto/fixtures/display-inputs.v1.bin")
                      :element-type '(unsigned-byte 8))
    (loop :for body := (framing:read-delimited in)
          :while body
          :collect (codec:decode body))))

(defun inputs ()
  (mapcar #'second (remove :hello (fixture-requests) :key #'first)))

(defun lem-key (input)
  (let ((key (key-event input)))
    (list (lem:key-sym key)
          (and (lem:key-ctrl key) :ctrl)
          (and (lem:key-meta key) :meta)
          (and (lem:key-shift key) :shift))))

(deftest the-hello-carries-the-display
  (ok (equal '(:hello 1 1 "01920000-0000-7000-8000-000000000000" 100 30 nil #x101010)
             (first (fixture-requests)))))

(deftest keys-reach-lem-by-lems-names
  (let ((keys (remove-if-not (lambda (input) (typep input 'key-input)) (inputs))))
    (ok (equal '(("x" :ctrl nil nil)             ; Ctrl+x
                 ("Return" nil nil nil)          ; Enter
                 ("F5" nil nil nil)
                 ("A" nil :meta nil)             ; Alt+Shift+a: M-A, shift folded in
                 ("\\" :ctrl nil nil)            ; the byte crossterm calls Ctrl+4
                 ("Space" nil nil nil)
                 ("Tab" nil nil :shift)          ; BackTab
                 ("é" nil nil nil))
               (mapcar #'lem-key keys)))))

(deftest every-input-carries-the-displays-seq-and-time
  (let ((inputs (inputs)))
    (ok (equal (loop :for seq :from 2 :to (1+ (length inputs)) :collect seq)
               (mapcar #'input-seq inputs))
        "numbered from 2, after the Hello")
    (ok (every #'integerp (mapcar #'input-time inputs)))))

(defclass golden-relay (relay:relay) ())

(deftest two-presses-200-ms-apart-are-a-double-click
  ;; Delivered back to back here, but made 200 ms apart by the display's
  ;; clock: the relay counts by the display's time (ADR 0013).
  (let ((relay (make-instance 'golden-relay))
        (presses (remove-if-not (lambda (input)
                                  (and (typep input 'mouse-input)
                                       (eq :down (lem-relay/input::mouse-input-action input))))
                                (inputs))))
    (ok (= 2 (length presses)))
    (dolist (press presses)
      (deliver relay press))
    (ok (= 2 (lem-relay/input::press-clicks (relay:relay-last-press relay))))
    (loop :while (plusp (lem:event-queue-length))
          :do (ignore-errors (lem:receive-event 0)))))

(deftest the-rest-of-the-mouse-decodes
  (let ((mouse (remove-if-not (lambda (input) (typep input 'mouse-input)) (inputs))))
    (flet ((action (input) (lem-relay/input::mouse-input-action input)))
      (let ((drag (find :move mouse :key #'action))
            (wheel (find :wheel mouse :key #'action)))
        (ok (eq :right (lem-relay/input::mouse-input-button drag)) "a drag holds its button")
        (ok (= 1 (lem-relay/input::mouse-input-wheel-y wheel)) "positive is up")
        (ok (equal '(7 3) (list (lem-relay/input::mouse-input-x wheel)
                                (lem-relay/input::mouse-input-y wheel))))))))

(deftest paste-resize-and-clipboard-replies
  (let ((inputs (inputs)))
    (ok (equal "pasted" (lem-relay/input::paste-input-text
                         (find-if (lambda (i) (typep i 'paste-input)) inputs))))
    (let ((resize (find-if (lambda (i) (typep i 'resize-input)) inputs)))
      (ok (equal '(120 40) (list (lem-relay/input::resize-input-width resize)
                                 (lem-relay/input::resize-input-height resize)))))
    (let ((replies (remove-if-not (lambda (i) (typep i 'clipboard-reply)) inputs)))
      (ok (equal '((7 "from the display") (8 nil))
                 (mapcar (lambda (reply)
                           (list (lem-relay/input::clipboard-reply-reply-to reply)
                                 (lem-relay/input::clipboard-reply-text reply)))
                         replies))
          "an absent text is no text, not an empty one"))))
