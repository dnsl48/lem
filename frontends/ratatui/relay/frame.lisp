(defpackage :lem-relay/frame
  (:use :cl)
  (:export
   ;; Styles: a Lem attribute as the display will show it.
   :style
   :make-style
   :style-foreground
   :style-background
   :style-bold
   :style-reverse
   :style-underline
   :style-cursor
   :style-italic
   :style-strikethrough
   :style-dim
   :style-underline-style
   :*underline-styles*
   :style-with-background
   :attribute-style
   :pack-color
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
   :views-stacked
   :make-views-stacked
   :views-stacked-views
   :text-put
   :make-text-put
   :text-put-view
   :text-put-x
   :text-put-y
   :text-put-text
   :text-put-width
   :text-put-style
   :text-put-link
   :relayed-link
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
   ;; Frame state and frames.
   :cursor
   :make-cursor
   :cursor-view
   :cursor-x
   :cursor-y
   :cursor-visible
   :cursor-shape
   :defaults
   :make-defaults
   :defaults-foreground
   :defaults-background
   :frame
   :frame-time
   :frame-input-seq
   :frame-ops
   :frame-cursor
   :frame-defaults
   ;; The session that builds frames.
   :session
   :make-session
   :session-defaults
   :session-sent-count
   :session-dropped-count
   :add-op
   :finish-frame))
(in-package :lem-relay/frame)

(defparameter *underline-styles* '(:curly :dotted :dashed :double)
  "The values of a Lem attribute's underline style the display draws
(ADR 0016, 0017). NIL, :straight or anything else is a straight
underline.")

;;; Styles

(defstruct (style (:constructor make-style
                      (&key foreground background bold reverse underline cursor
                         italic strikethrough dim underline-style)))
  "A Lem attribute reduced to what the display shows (ADR 0012).

Colours are packed #xRRGGBB integers, or NIL for the default colour (see
`defaults'). UNDERLINE is NIL, T, or a packed colour. CURSOR marks a cell
painted as a cursor. ITALIC, STRIKETHROUGH, DIM and UNDERLINE-STYLE are
the attribute's own (ADR 0017); UNDERLINE-STYLE is one of
`*underline-styles*', or NIL for straight. Styles compare by content:
equal slots are the same style, however many Lem attribute objects
produced them."
  (foreground nil :read-only t)
  (background nil :read-only t)
  (bold nil :read-only t)
  (reverse nil :read-only t)
  (underline nil :read-only t)
  (cursor nil :read-only t)
  (italic nil :read-only t)
  (strikethrough nil :read-only t)
  (dim nil :read-only t)
  (underline-style nil :read-only t))

(defun style-with-background (style background)
  "STYLE with BACKGROUND in place of its own, every other slot kept."
  (make-style :foreground (style-foreground style)
              :background background
              :bold (style-bold style)
              :reverse (style-reverse style)
              :underline (style-underline style)
              :cursor (style-cursor style)
              :italic (style-italic style)
              :strikethrough (style-strikethrough style)
              :dim (style-dim style)
              :underline-style (style-underline-style style)))

(defun pack-color (color)
  "COLOR, a Lem colour struct or anything `lem:parse-color' reads, as
#xRRGGBB; NIL when it is neither."
  (let ((color (if (stringp color) (lem:parse-color color) color)))
    (when (typep color 'lem:color)
      (logior (ash (lem:color-red color) 16)
              (ash (lem:color-green color) 8)
              (lem:color-blue color)))))

(defun attribute-style (attribute)
  "The style a Lem ATTRIBUTE draws with; NIL when ATTRIBUTE is not one.

Every colour spelling Lem accepts becomes one packed integer, as
`lem-server' turns them all into \"#RRGGBB\"."
  (when (lem:attribute-p attribute)
    (let ((underline (lem:attribute-underline attribute)))
      (make-style :foreground (pack-color (lem:attribute-foreground attribute))
                  :background (pack-color (lem:attribute-background attribute))
                  :bold (and (lem:attribute-bold attribute) t)
                  :reverse (and (lem:attribute-reverse attribute) t)
                  :underline (and underline (or (pack-color underline) t))
                  :cursor (and (lem:cursor-attribute-p attribute) t)
                  :italic (and (lem:attribute-italic attribute) t)
                  :strikethrough (and (lem:attribute-strikethrough attribute) t)
                  :dim (and (lem:attribute-dim attribute) t)
                  :underline-style (find (lem:attribute-underline-style attribute)
                                         *underline-styles*)))))

;; A URL is passed on only if it is plain printable ASCII, as OSC 8 needs,
;; and not absurdly long; anything else could carry a terminal escape.
(defconstant +link-length-limit+ 2048
  "The longest URL passed on as a link, in characters.")

(defun relayed-link (attribute)
  "The URL a Lem ATTRIBUTE links its text to (`lem:attribute-link'), or
NIL. A URL with anything but printable ASCII in it, or longer than
`+link-length-limit+', is no link (ADR 0018)."
  (let ((url (and (lem:attribute-p attribute)
                  (lem:attribute-link attribute))))
    (when (and (stringp url)
               (< 0 (length url) (1+ +link-length-limit+))
               (every (lambda (c) (char<= #\Space c #\~)) url))
      url)))

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

(defstruct (views-stacked (:constructor make-views-stacked (&key views)))
  "The order views composite in, bottom first: a list of view ids."
  views)

(defstruct (text-put (:constructor make-text-put (&key view x y text width style link)))
  "TEXT drawn at cell X, Y of VIEW with STYLE (a `style', or NIL for the
default colours). WIDTH is the cells Lem laid it out as occupying; the
display fits TEXT to exactly that many (ADR 0012). LINK is the URL the
text links to, or NIL (ADR 0018)."
  view x y text width style link)

(defstruct (line-cleared (:constructor make-line-cleared (&key view x y)))
  "Row Y of VIEW blanked from column X to its end." view x y)

(defstruct (rest-cleared (:constructor make-rest-cleared (&key view y)))
  "VIEW blanked from row Y to its bottom." view y)

(defstruct (modeline-painted (:constructor make-modeline-painted (&key view puts)))
  "VIEW's modeline repainted in full: blanked, then PUTS (text-put ops,
with Y relative to the modeline row)."
  view puts)

(defun op-view (op)
  "The view OP addresses; NIL for an op about no single view."
  (etypecase op
    (view-created (view-created-view op))
    (view-deleted (view-deleted-view op))
    (view-moved (view-moved-view op))
    (view-resized (view-resized-view op))
    (view-cleared (view-cleared-view op))
    (views-stacked nil)
    (text-put (text-put-view op))
    (line-cleared (line-cleared-view op))
    (rest-cleared (rest-cleared-view op))
    (modeline-painted (modeline-painted-view op))))

;;; Frame state and frames

(defstruct (cursor (:constructor make-cursor (&key view x y (visible t) (shape :box))))
  "The primary cursor: a cell of a view, whether it shows, and its SHAPE
(:box, :bar or :underline). The display puts the terminal's own cursor
here, over the cell painted with a cursor style (ADR 0012)."
  view x y visible shape)

(defstruct (defaults (:constructor make-defaults (&key foreground background)))
  "The default colours Lem last set, as packed integers or NIL. A style
colour of NIL means these; a NIL here means the terminal's own."
  foreground background)

(defstruct (frame (:constructor %make-frame (time input-seq ops cursor defaults)))
  "One display update: OPS in order, then the cursor.

TIME is when the relay closed it, in microseconds on the relay's
monotonic clock. INPUT-SEQ is the `seq' of the last display message the
relay had handed to Lem by then, or NIL if none: what this frame can
reflect, for tracing and replay (ADR 0013). A frame's own identity is
its envelope's `seq', assigned as it is sent. DEFAULTS is a `defaults'
when they changed since the last frame sent, and NIL when they did not."
  time input-seq ops cursor defaults)

(defun now-microseconds ()
  (floor (* (get-internal-real-time) 1000000) internal-time-units-per-second))

(defstruct (session (:constructor make-session (&key (suppress t))))
  "What the relay has told the display so far, and the frame being built.

With SUPPRESS on, frames and repaints that would change nothing on screen
are not sent (ADR 0011, 0012). That needs a memory of the screen: where
each view was last blanked from, the cursor, each view's last modeline,
the stacking order and the default colours.

Used only from Lem's editor thread, where every lem-if drawing call runs."
  (suppress t)
  (pending '())
  (cleared-from (make-hash-table :test 'eql))
  (last-cursor nil)
  (last-modeline (make-hash-table :test 'eql))
  (last-stacking :none)
  (defaults (make-defaults))
  (sent-defaults nil)
  (sent-count 0)
  (dropped-count 0))

(defun unchanged-repaint-p (session op)
  "True when OP repeats a modeline or a stacking order already sent,
recording it when it does not.

A new or resized view starts with a blank modeline and a deleted view has
none, so all three forget the one remembered."
  (let ((last-modeline (session-last-modeline session)))
    (typecase op
      ((or view-created view-resized view-deleted)
       (remhash (op-view op) last-modeline)
       nil)
      (modeline-painted
       (let ((view (modeline-painted-view op)))
         (or (equalp (modeline-painted-puts op) (gethash view last-modeline))
             (progn (setf (gethash view last-modeline) (modeline-painted-puts op))
                    nil))))
      (views-stacked
       (or (equal (views-stacked-views op) (session-last-stacking session))
           (progn (setf (session-last-stacking session) (views-stacked-views op))
                  nil))))))

(defun add-op (session op)
  "Queue OP for the frame being built in SESSION and return it, or return
NIL when suppression drops it as a repeat."
  (when (and (unchanged-repaint-p session op) (session-suppress session))
    (return-from add-op nil))
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
        (view-deleted
         (remhash (op-view op) cleared-from)
         (remhash (op-view op) (session-last-modeline session)))
        (rest-cleared)
        (t (when (op-view op) (remhash (op-view op) cleared-from)))))
    (dolist (op ops)
      (when (typep op 'rest-cleared)
        (setf (gethash (rest-cleared-view op) cleared-from) (rest-cleared-y op))))))

(defun finish-frame (session cursor &optional input-seq)
  "Close the frame being built with CURSOR (a `cursor', or NIL) and
INPUT-SEQ (see `frame') and return it, or NIL when suppression finds it would change nothing on screen:
every op redundant, the cursor as it was, the default colours as they
were. Either way the next frame starts empty."
  (let* ((ops (reverse (session-pending session)))
         (defaults (session-defaults session))
         (defaults-changed (not (equalp defaults (session-sent-defaults session)))))
    (setf (session-pending session) '())
    (cond ((and (session-suppress session)
                (every (lambda (op) (redundant-p session op)) ops)
                (equalp cursor (session-last-cursor session))
                (not defaults-changed))
           (incf (session-dropped-count session))
           nil)
          (t
           (remember session ops)
           (setf (session-last-cursor session) cursor
                 (session-sent-defaults session) (copy-defaults defaults))
           (incf (session-sent-count session))
           (%make-frame (now-microseconds)
                        input-seq
                        ops
                        cursor
                        (and defaults-changed (copy-defaults defaults)))))))
