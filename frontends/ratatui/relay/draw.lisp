(defpackage :lem-relay/draw
  (:use :cl :lem-relay/frame :lem-relay/view)
  (:local-nicknames (:display :lem-core/display))
  (:export :object-width
           :object-puts
           :render-line
           :render-line-on-modeline
           :clear-to-end-of-window))
(in-package :lem-relay/draw)

(defvar *unknown-classes* (make-hash-table :test 'eq)
  "Drawing-object classes already reported as unknown, so each is logged
once rather than on every redraw.")

;;; What is ported from where.
;;;
;;; The mapping from drawing objects to puts is `lem-server''s
;;; (frontends/server/main.lisp, `draw-object'), as ADR 0009 requires:
;;; port it, do not re-derive it. Three corrections come from ncurses
;;; (frontends/ncurses/render.lisp), the terminal frontend, where the two
;;; disagree and a terminal needs ncurses' answer. Each is marked
;;; "ncurses:" below.

(defgeneric object-width (object)
  (:documentation "The cells OBJECT occupies on its line.")
  (:method (object)
    (declare (ignore object))
    0)
  (:method ((object display:text-object))
    (lem:string-width (display:text-object-string object))))

(defun window-background ()
  "The background of the window being drawn, packed, or NIL.

Lem binds `lem-if:*background-color-of-drawing-window*' while drawing a
floating or inactive window whose background differs."
  (pack-color lem-if:*background-color-of-drawing-window*))

(defun resolve-style (attribute)
  "The style ATTRIBUTE (an attribute or an attribute name) draws with.

ncurses: a missing background takes the drawing window's, so text in a
popup sits on the popup's colour. `lem-server' does that only when there
is no attribute at all, which suits a browser that paints each view's
background itself, but not a terminal."
  (let ((style (attribute-style (lem:ensure-attribute attribute nil)))
        (background (window-background)))
    (cond ((null background) style)
          ((null style) (make-style :background background))
          ((style-background style) style)
          (t (make-style :foreground (style-foreground style)
                         :background background
                         :bold (style-bold style)
                         :reverse (style-reverse style)
                         :underline (style-underline style)
                         :cursor (style-cursor style))))))

(defun put (view x y text style &optional (width (lem:string-width text)))
  (make-text-put :view (view-id view) :x x :y y :text text :width width :style style))

(defun note-cursor (view x y)
  "Tell Lem the cursor was drawn at X, Y of VIEW. The relay reads it back
from `lem:last-print-cursor-x'/`-y' when it closes the frame."
  (when (view-window view)
    (lem-core:set-last-print-cursor (view-window view) x y)))

(defgeneric object-puts (object x y view)
  (:documentation "The text-put ops that draw OBJECT at X, Y of VIEW, and as
a second value whether OBJECT is where the cursor is.")
  (:method (object x y view)
    (declare (ignore x y view))
    (let ((class (class-name (class-of object))))
      (unless (gethash class *unknown-classes*)
        (setf (gethash class *unknown-classes*) t)
        (log:warn "lem-relay: not drawing unknown drawing object ~S" class)))
    '())
  (:method ((object display:void-object) x y view)
    (declare (ignore x y view))
    '())
  (:method ((object display:image-object) x y view)
    ;; A terminal has no image support; lem-server and ncurses skip it too.
    (declare (ignore x y view))
    '())
  (:method ((object display:text-object) x y view)
    ;; Also icon-, folder-, emoji- and control-character-objects: all
    ;; text. lem-server additionally sends an icon's font, which a
    ;; terminal cannot choose, so icons need no method of their own.
    (let ((attribute (display:text-object-attribute object)))
      (values (list (put view x y (display:text-object-string object)
                         (resolve-style attribute)
                         (object-width object)))
              (and attribute (lem:cursor-attribute-p attribute) t))))
  (:method ((object display:line-end-object) x y view)
    (list (put view (+ x (display:line-end-object-offset object)) y
               (display:text-object-string object)
               (resolve-style (display:text-object-attribute object))
               (object-width object))))
  (:method ((object display:eol-cursor-object) x y view)
    ;; ncurses: only the true cursor is the cursor. With multiple cursors
    ;; the others are eol-cursor-objects too, and lem-server, noting every
    ;; one, lets whichever is drawn last claim the cursor position.
    (values (list (put view x y " "
                       (make-style :background
                                   (pack-color (display:eol-cursor-object-color object))
                                   :cursor t)
                       1))
            (and (display:eol-cursor-object-true-cursor-p object) t)))
  (:method ((object display:extend-to-eol-object) x y view)
    (let ((width (view-width view)))
      (when (< x width)
        (list (put view x y (make-string (- width x) :initial-element #\Space)
                   (make-style :background
                               (pack-color (display:extend-to-eol-object-color object)))
                   (- width x)))))))

(defun draw-at (object x y view)
  "OBJECT's puts at X, Y of VIEW, telling Lem when the cursor is there."
  (multiple-value-bind (puts cursor-here) (object-puts object x y view)
    (when cursor-here
      (note-cursor view x y))
    puts))

(defun line-puts (view x y objects)
  (loop :for object :in objects
        :append (draw-at object x y view)
        :do (incf x (object-width object))))

(defun line-puts-from-behind (view y objects)
  "Puts for OBJECTS laid out leftwards from VIEW's right edge, as Lem lays
out the right-hand side of a modeline."
  (loop :with x := (view-width view)
        :for object :in objects
        :do (decf x (object-width object))
        :append (draw-at object x y view)))

(defun render-line (session view x y objects)
  "Queue row Y of VIEW: blanked from column X, then OBJECTS drawn from X."
  (add-op session (make-line-cleared :view (view-id view) :x x :y y))
  (dolist (put (line-puts view x y objects))
    (add-op session put)))

(defun render-line-on-modeline (session view left-objects right-objects default-attribute)
  "Queue VIEW's modeline as one op: blanked across the view's width in
DEFAULT-ATTRIBUTE, LEFT-OBJECTS from the left edge, RIGHT-OBJECTS against
the right."
  (let ((width (view-width view)))
    (add-op session
            (make-modeline-painted
             :view (view-id view)
             :puts (append (list (put view 0 0 (make-string width :initial-element #\Space)
                                      (attribute-style
                                       (lem:ensure-attribute default-attribute nil))
                                      width))
                           (line-puts view 0 0 left-objects)
                           (line-puts-from-behind view 0 right-objects))))))

(defun clear-to-end-of-window (session view y)
  "Queue VIEW blanked from row Y down.

ncurses: nothing when Y is already past the bottom. lem-server sends it
regardless, which the display then has to ignore."
  (when (< y (view-height view))
    (add-op session (make-rest-cleared :view (view-id view) :y y))))
