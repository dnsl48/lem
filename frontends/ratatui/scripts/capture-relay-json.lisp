;;;; Capture what lem-relay/json sends for a scripted session in the real
;;;; editor, one JSON message per line, as a fixture for lem-protocol's
;;;; relay_fixture.rs: the Rust half decoding the Lisp half's output.
;;;;
;;;; Regenerate from the repository root after changing the relay or the
;;;; JSON codec:
;;;;
;;;;   sbcl --non-interactive --load .qlot/setup.lisp \
;;;;        --load frontends/ratatui/scripts/capture-relay-json.lisp

(defpackage :capture-relay-json
  (:use :cl))
(in-package :capture-relay-json)

(handler-bind ((warning #'muffle-warning))
  (asdf:load-system "lem-relay/json"))

(defclass capture-relay (lem-relay/relay:relay lem:implementation) ()
  (:default-initargs
   :name :capture-relay
   :redraw-after-modifying-floating-window t
   :no-force-needed nil
   :window-left-margin 1
   :window-bottom-margin 1))

(defparameter *fixture*
  (asdf:system-relative-pathname
   "lem-relay" "../rust/crates/lem-protocol/tests/fixtures/relay-frames.jsonl"))

(defun capture ()
  (let ((relay (make-instance 'capture-relay))
        (state (lem-relay/json/codec:make-codec-state))
        (lines '()))
    (setf (lem-relay/relay:relay-sink relay)
          (lambda (message)
            (dolist (body (lem-relay/json/codec:encode message state))
              (push (babel:octets-to-string body :encoding :utf-8) lines))))
    (lem:with-current-buffers ()
      (lem:with-implementation relay
        (lem:setup-first-frame)
        (flet ((redraw () (lem:redraw-display :force t))
               (type-text (string)
                 (lem:insert-string
                  (lem:buffer-point (lem:window-buffer (lem:current-window))) string)))
          (redraw)
          (type-text (format nil "(defun héllo () 'λ)~%;; 日本語"))
          (lem:redraw-display)
          (lem:split-window-vertically (lem:current-window))
          (redraw)
          (lem/common/timer:with-timer-manager
              (make-instance 'lem/common/timer:timer-manager)
            (lem:display-popup-message "a popup"))
          (redraw)
          (lem:set-background "#1c1c1c")
          (lem-if:update-cursor-shape relay :bar)
          (lem:redraw-display)
          (lem-if:clipboard-copy relay "copied"))))
    (with-open-file (out *fixture* :direction :output :if-exists :supersede
                                   :external-format :utf-8)
      (dolist (line (reverse lines))
        (write-line line out)))
    (format t "~&Wrote ~D messages to ~A~%" (length lines) *fixture*)))

(capture)
