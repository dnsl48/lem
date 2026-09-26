(defpackage :lem-relay/frame
  (:use :cl)
  (:export
   ;; Styles: a Lem attribute as the display will show it, interned.
   :style
   :make-style
   :style-foreground
   :style-background
   :style-bold
   :style-reverse
   :style-underline
   :style-cursor
   :attribute-style
   :style-table
   :make-style-table
   :intern-style
   :style-by-id
   ;; Ops: what a frame asks the display to do.
   :view-created
   :make-view-created
   :view-created-view
   :view-created-x
   :view-created-y
   :view-created-width
   :view-created-height
   :view-created-kind
   :view-created-modeline-p
   :view-created-border
   :view-created-border-shape
   :view-deleted
   :make-view-deleted
   :view-deleted-view
   :view-moved
   :make-view-moved
   :view-moved-view
   :view-moved-x
   :view-moved-y
   :view-resized
   :make-view-resized
   :view-resized-view
   :view-resized-width
   :view-resized-height
   :view-cleared
   :make-view-cleared
   :view-cleared-view
   :text-put
   :make-text-put
   :text-put-view
   :text-put-x
   :text-put-y
   :text-put-text
   :text-put-width
   :text-put-style
   :line-cleared
   :make-line-cleared
   :line-cleared-view
   :line-cleared-x
   :line-cleared-y
   :rest-cleared
   :make-rest-cleared
   :rest-cleared-view
   :rest-cleared-y
   :modeline-painted
   :make-modeline-painted
   :modeline-painted-view
   :modeline-painted-puts
   :op-view
   ;; Frames and the session that builds them.
   :cursor
   :make-cursor
   :cursor-view
   :cursor-x
   :cursor-y
   :frame
   :frame-seq
   :frame-ops
   :frame-new-styles
   :frame-cursor
   :session
   :make-session
   :session-styles
   :session-sent-count
   :session-dropped-count
   :add-op
   :finish-frame))
(in-package :lem-relay/frame)

;;; Styles

(defstruct (style (:constructor make-style
                      (&key foreground background bold reverse underline cursor)))
  "A Lem attribute reduced to what the display shows. This is the wire's
`Attribute' (ADR 0011).

Colours are packed #xRRGGBB integers, or NIL for the terminal default.
UNDERLINE is NIL, T, or a packed colour. Two styles with equal slots are
the same style, however many Lem attribute objects produced them."
  (foreground nil :read-only t)
  (background nil :read-only t)
  (bold nil :read-only t)
  (reverse nil :read-only t)
  (underline nil :read-only t)
  (cursor nil :read-only t))

(defstruct (style-table (:constructor make-style-table ()))
  "Styles interned for one session. Id 0 is the terminal default and is
never stored; ids from 1 are assigned in order of first use and are stable
for the session."
  (ids (make-hash-table :test 'equal))
  (styles (make-array 16 :adjustable t :fill-pointer 1 :initial-element nil))
  ;; Ids whose definition has already gone out in a frame.
  (sent (make-hash-table :test 'eql)))

(defun pack-color (color)
  "COLOR, a Lem colour struct or a colour string, as #xRRGGBB, or NIL."
  (let ((color (if (stringp color) (lem:parse-color color) color)))
    (when (typep color 'lem:color)
      (logior (ash (lem:color-red color) 16)
              (ash (lem:color-green color) 8)
              (lem:color-blue color)))))

(defun attribute-style (attribute)
  "The style a Lem ATTRIBUTE draws with, or NIL for the terminal default.

Anything Lem accepts as a colour is normalised to one packed integer, so
\"#FF0000\" and a colour struct of the same value produce equal styles."
  (when (lem:attribute-p attribute)
    (let ((underline (lem:attribute-underline attribute)))
      (make-style :foreground (pack-color (lem:attribute-foreground attribute))
                  :background (pack-color (lem:attribute-background attribute))
                  :bold (and (lem:attribute-bold attribute) t)
                  :reverse (and (lem:attribute-reverse attribute) t)
                  :underline (and underline (or (pack-color underline) t))
                  :cursor (and (lem:cursor-attribute-p attribute) t)))))

(defun style-key (style)
  (list (style-foreground style) (style-background style) (style-bold style)
        (style-reverse style) (style-underline style) (style-cursor style)))

(defun intern-style (table style)
  "The id of STYLE in TABLE, assigning the next one if it is new.
NIL, the terminal default, is always 0."
  (if (null style)
      0
      (let ((key (style-key style)))
        (or (gethash key (style-table-ids table))
            (setf (gethash key (style-table-ids table))
                  (vector-push-extend style (style-table-styles table)))))))

(defun style-by-id (table id)
  "The style interned as ID in TABLE; NIL for 0, the terminal default."
  (aref (style-table-styles table) id))

;;; Ops

(defstruct (view-created (:constructor make-view-created
                             (&key view x y width height kind modeline-p
                                border border-shape)))
  "A view appeared. KIND is :tile, :header or :floating."
  view x y width height kind modeline-p border border-shape)

(defstruct (view-deleted (:constructor make-view-deleted (&key view)))
  "A view went away." view)

(defstruct (view-moved (:constructor make-view-moved (&key view x y)))
  "A view's top-left corner moved, in screen cells." view x y)

(defstruct (view-resized (:constructor make-view-resized (&key view width height)))
  "A view changed size, in cells." view width height)

(defstruct (view-cleared (:constructor make-view-cleared (&key view)))
  "Everything in a view was blanked." view)

(defstruct (text-put (:constructor make-text-put (&key view x y text width style)))
  "TEXT drawn at cell X, Y of VIEW. WIDTH is the cells the relay assumed it
takes; STYLE is an interned style id."
  view x y text width style)

(defstruct (line-cleared (:constructor make-line-cleared (&key view x y)))
  "Row Y of VIEW blanked from column X to its end." view x y)

(defstruct (rest-cleared (:constructor make-rest-cleared (&key view y)))
  "VIEW blanked from row Y to its bottom." view y)

(defstruct (modeline-painted (:constructor make-modeline-painted (&key view puts)))
  "VIEW's modeline repainted in full: blanked, then PUTS (text-put ops,
with Y relative to the modeline row)."
  view puts)

(defun op-view (op)
  "The view OP addresses."
  (etypecase op
    (view-created (view-created-view op))
    (view-deleted (view-deleted-view op))
    (view-moved (view-moved-view op))
    (view-resized (view-resized-view op))
    (view-cleared (view-cleared-view op))
    (text-put (text-put-view op))
    (line-cleared (line-cleared-view op))
    (rest-cleared (rest-cleared-view op))
    (modeline-painted (modeline-painted-view op))))

;;; Frames

(defstruct (cursor (:constructor make-cursor (&key view x y)))
  "Where the cursor is: a cell of a view." view x y)

(defstruct (frame (:constructor %make-frame (seq ops new-styles cursor)))
  "One display update: OPS in order, then the cursor.

NEW-STYLES pairs each style id that OPS use for the first time on the
wire with its style, so a display that keeps a style table can decode
every id it sees."
  seq ops new-styles cursor)

(defstruct (session (:constructor make-session ()))
  "What the relay has told the display so far, and the frame being built.

The relay suppresses frames that would change nothing on screen (ADR
0011), so it has to remember enough of the screen to know: where each
view was last blanked from, where the cursor was, and each view's last
modeline."
  (styles (make-style-table))
  (pending '())
  (seq 0)
  (cleared-from (make-hash-table :test 'eql))
  (last-cursor nil)
  (last-modeline (make-hash-table :test 'eql))
  (sent-count 0)
  (dropped-count 0))

(defun forget-view (session view)
  (remhash view (session-cleared-from session))
  (remhash view (session-last-modeline session)))

(defun add-op (session op)
  "Queue OP for the frame being built in SESSION.

A modeline identical to the one last sent for its view is dropped here.
A new or resized view starts with a blank modeline and a deleted view
has none, so all three forget the one remembered."
  (typecase op
    ((or view-created view-resized view-deleted)
     (remhash (op-view op) (session-last-modeline session)))
    (modeline-painted
     (let ((view (modeline-painted-view op))
           (last (session-last-modeline session)))
       (when (equalp (modeline-painted-puts op) (gethash view last))
         (return-from add-op nil))
       (setf (gethash view last) (modeline-painted-puts op)))))
  (push op (session-pending session))
  op)

(defun redundant-p (session op)
  "True when OP would change nothing on screen.

Only a `rest-cleared' can be: Lem blanks what a short buffer leaves of
its window on every redraw, and re-blanking an already blank region is
a no-op."
  (and (typep op 'rest-cleared)
       (eql (rest-cleared-y op)
            (gethash (rest-cleared-view op) (session-cleared-from session)))))

(defun remember (session ops)
  "Record what OPS leave on screen. Any paint into a view makes where it
was last blanked from stale; this is done before recording the frame's
own `rest-cleared', because Lem draws lines first and blanks the rest
after."
  (let ((cleared-from (session-cleared-from session)))
    (dolist (op ops)
      (typecase op
        (view-deleted (forget-view session (op-view op)))
        (rest-cleared)
        (t (remhash (op-view op) cleared-from))))
    (dolist (op ops)
      (when (typep op 'rest-cleared)
        (setf (gethash (rest-cleared-view op) cleared-from) (rest-cleared-y op))))))

(defun op-style-ids (op)
  (typecase op
    (text-put (list (text-put-style op)))
    (modeline-painted (mapcar #'text-put-style (modeline-painted-puts op)))))

(defun new-styles (session ops)
  "Pairs (id . style) for the styles OPS use that no sent frame defined,
marking them sent."
  (let* ((table (session-styles session))
         (sent (style-table-sent table))
         (new '()))
    (dolist (op ops)
      (dolist (id (op-style-ids op))
        (unless (or (zerop id) (gethash id sent))
          (setf (gethash id sent) t)
          (push (cons id (style-by-id table id)) new))))
    (nreverse new)))

(defun finish-frame (session cursor)
  "Close the frame being built with CURSOR (a `cursor', or NIL) and return
it, or NIL when it would change nothing on screen: every op redundant and
the cursor where it already was. Either way the next frame starts empty."
  (let ((ops (reverse (session-pending session))))
    (setf (session-pending session) '())
    (cond ((and (every (lambda (op) (redundant-p session op)) ops)
                (equalp cursor (session-last-cursor session)))
           (incf (session-dropped-count session))
           nil)
          (t
           (remember session ops)
           (setf (session-last-cursor session) cursor)
           (incf (session-sent-count session))
           (%make-frame (incf (session-seq session))
                        ops
                        (new-styles session ops)
                        cursor)))))
