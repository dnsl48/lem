(defpackage :lem-relay/tests/relay
  (:use :cl :rove :lem-relay/frame :lem-relay/relay))
(in-package :lem-relay/tests/relay)

(defclass test-relay (relay lem:implementation) ()
  (:default-initargs
   :name :relay-test
   :redraw-after-modifying-floating-window t
   :no-force-needed nil
   :window-left-margin 1
   :window-bottom-margin 1)
  (:documentation "The relay as a runnable implementation, for driving the
real editor in-process, the way Lem's own tests use lem-fake-interface."))

(defvar *sent* '()
  "Everything the relay under test sent, latest first.")

(defun sent-frames ()
  (reverse (remove-if-not (lambda (message) (typep message 'frame)) *sent*)))

(defun last-frame ()
  (find-if (lambda (message) (typep message 'frame)) *sent*))

(defun call-with-editor (function)
  (let ((*sent* '())
        (relay (make-instance 'test-relay)))
    (setf (relay-sink relay) (lambda (message) (push message *sent*) (length *sent*)))
    (lem:with-current-buffers ()
      (lem:with-implementation relay
        (lem:setup-first-frame)
        (funcall function relay)))))

(defmacro with-editor ((relay) &body body)
  `(call-with-editor (lambda (,relay) (declare (ignorable ,relay)) ,@body)))

(defun redraw ()
  "Repaint everything, as Lem does on startup and after a resize."
  (lem:redraw-display :force t))

(defun redraw-after-command ()
  "Repaint as Lem does after an ordinary command: only what changed. A
forced redraw repaints every line, and the relay, mirroring Lem's calls
rather than keeping a grid (ADR 0012), sends that repaint."
  (lem:redraw-display))

(defun type-text (string)
  "Insert STRING where the current window's cursor is.

Into the window's own buffer, not `lem:current-buffer': when several
editors have run in one process, as tests do, the two can be different
*tmp* buffers."
  (lem:insert-string (lem:buffer-point (lem:window-buffer (lem:current-window))) string))

(defun ops-of-type (type frame)
  (remove-if-not (lambda (op) (typep op type)) (frame-ops frame)))

(defun painted-text (frame)
  (apply #'concatenate 'string (mapcar #'text-put-text (ops-of-type 'text-put frame))))

;;; The first frame

(deftest the-first-frame-paints-the-editor
  (with-editor (relay)
    (redraw)
    (let ((frames (sent-frames)))
      (ok frames "at least one frame")
      (let ((ops (mapcan #'frame-ops frames)))
        (ok (find-if (lambda (op) (and (typep op 'view-created)
                                       (eq :tile (view-created-kind op))
                                       (view-created-modeline-p op)))
                     ops)
            "a tile with a modeline")
        (ok (find-if (lambda (op) (typep op 'modeline-painted)) ops) "its modeline")
        (ok (find-if (lambda (op) (typep op 'views-stacked)) ops) "a stacking order"))
      (ok (frame-defaults (first frames)) "the default colours")
      (ok (frame-cursor (car (last frames))) "a cursor"))))

(deftest an-idle-redraw-sends-nothing
  (with-editor (relay)
    (redraw)
    (let ((before (length (sent-frames))))
      (redraw-after-command)
      (ok (= before (length (sent-frames)))))))

;;; Editing

(deftest typed-text-is-painted-and-moves-the-cursor
  (with-editor (relay)
    (redraw)
    (type-text "hello")
    (redraw-after-command)
    (let ((frame (last-frame)))
      (ok (search "hello" (painted-text frame)))
      (ok (= 5 (cursor-x (frame-cursor frame)))))))

(deftest splitting-a-window-stacks-both-views
  (with-editor (relay)
    (redraw)
    (lem:split-window-vertically (lem:current-window))
    (redraw)
    (let* ((created (ops-of-type 'view-created (last-frame)))
           (stacked (views-stacked-views (first (ops-of-type 'views-stacked (last-frame))))))
      (ok (= 1 (length created)) "one new view")
      (ok (member (view-created-view (first created)) stacked) "which is stacked")
      (ok (<= 2 (length stacked)) "with the one it was split from"))))

(deftest a-popup-stacks-above-the-window-it-covers
  (with-editor (relay)
    (redraw)
    (lem/common/timer:with-timer-manager (make-instance 'lem/common/timer:timer-manager)
      (lem:display-popup-message "hi there"))
    (redraw)
    (let* ((frame (last-frame))
           (popup (find :floating (ops-of-type 'view-created frame) :key #'view-created-kind))
           (stacked (views-stacked-views (first (ops-of-type 'views-stacked frame)))))
      (ok popup "the popup is a floating view")
      (ok (view-created-border popup) "with its border")
      (ok (search "hi there" (painted-text frame)))
      (ok (eql (view-created-view popup) (car (last stacked))) "on top"))))

(lem:define-attribute test-italic-attribute
  (t :foreground "#123456"))

(deftest marking-an-attribute-makes-its-text-italic
  (with-editor (relay)
    (redraw)
    (ok (= 1 (mark-font-styles '((test-italic-attribute :italic :dim)
                                 (no-such-attribute :italic)))))
    (let ((put (first (lem-relay/draw:object-puts
                       (make-instance 'lem-core/display:text-object
                                      :string "x" :attribute 'test-italic-attribute :type nil)
                       0 0 (lem-relay/view:make-view :id 1 :width 10 :height 1)))))
      (ok (style-italic (text-put-style put)))
      (ok (style-dim (text-put-style put))))))

(deftest a-theme-load-drops-the-marks-so-they-are-reapplied
  ;; Why lem-ratatui marks from lem:*after-load-theme-hook*: a theme load
  ;; rebuilds every attribute object from its definition.
  (with-editor (relay)
    (mark-font-styles '((test-italic-attribute :italic)))
    (ok (lem:attribute-value (lem:ensure-attribute 'test-italic-attribute) :italic))
    (lem:load-theme "lem-default" nil)
    (ng (lem:attribute-value (lem:ensure-attribute 'test-italic-attribute) :italic))
    (mark-font-styles '((test-italic-attribute :italic)))
    (ok (lem:attribute-value (lem:ensure-attribute 'test-italic-attribute) :italic))))

(deftest only-font-styles-can-be-marked
  (ok (signals (mark-font-styles '((test-italic-attribute :bold)))))
  (ok (signals (mark-font-styles '((test-italic-attribute (:underline-style :wavy)))))))

(deftest an-underline-style-can-be-marked
  (with-editor (relay)
    (mark-font-styles '((test-italic-attribute (:underline-style :curly))))
    (ok (eq :curly (lem:attribute-value (lem:ensure-attribute 'test-italic-attribute)
                                         :underline-style)))))

;;; Frame state

(deftest a-theme-background-reaches-lem-and-the-display
  (with-editor (relay)
    (redraw)
    (lem:set-background "#224466")
    (ok (eql #x224466 (pack-color (lem-if:get-background-color relay)))
        "Lem is answered with the new default")
    (redraw)
    (let ((defaults (frame-defaults (last-frame))))
      (ok defaults "the next frame carries it")
      (ok (eql #x224466 (defaults-background defaults))))))

(deftest a-frame-carries-the-last-input-delivered
  (with-editor (relay)
    (lem-relay/input:deliver relay (lem-relay/input:make-resize-input 80 24 :seq 17))
    (redraw)
    (ok (= 17 (frame-input-seq (last-frame))))))

(deftest a-cursor-shape-change-is-a-frame
  (with-editor (relay)
    (redraw)
    (lem-if:update-cursor-shape relay :bar)
    (redraw)
    (ok (eq :bar (cursor-shape (frame-cursor (last-frame)))))))

(deftest the-default-colours-are-never-nil
  ;; Lem falls back to them for attributes without a colour.
  (let ((relay (make-instance 'test-relay)))
    (ok (typep (lem-if:get-foreground-color relay) 'lem:color))
    (ok (typep (lem-if:get-background-color relay) 'lem:color))))

;;; Clipboard

(defun replying-relay (reply)
  "A relay whose display answers clipboard requests with REPLY, or not at
all when REPLY is NIL. Its sink numbers messages as a codec does."
  (let ((relay (make-instance 'test-relay))
        (seq 0))
    (setf (relay-sink relay)
          (lambda (message)
            (push message *sent*)
            (incf seq)
            (when (and reply (typep message 'clipboard-request))
              (clipboard-replied relay seq reply))
            seq))
    relay))

(deftest pasting-asks-the-display
  (let ((*sent* '()))
    (ok (equal "from the display" (lem-if:clipboard-paste (replying-relay "from the display"))))))

(deftest pasting-gives-up-when-the-display-does-not-answer
  (let ((*sent* '()))
    (ok (null (lem-if:clipboard-paste (replying-relay nil))))))

(deftest a-late-reply-is-not-taken-for-the-next-paste
  (let ((*sent* '())
        (relay (replying-relay "fresh")))
    (clipboard-replied relay 999 "stale")
    (ok (equal "fresh" (lem-if:clipboard-paste relay)))))

(deftest copying-puts-text-on-the-display-clipboard
  (let ((*sent* '())
        (relay (replying-relay nil)))
    (lem-if:clipboard-copy relay "copied")
    (ok (equal "copied" (set-clipboard-text (first *sent*))))))
