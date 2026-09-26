(defpackage :lem-relay/relay
  (:use :cl :lem-relay/frame :lem-relay/view)
  (:local-nicknames (:draw :lem-relay/draw)
                    (:queue :lem/common/queue))
  (:export :relay
           :relay-session
           :relay-sink
           :relay-display-width
           :relay-display-height
           :relay-foreground
           :relay-background
           :relay-terminal-capabilities
           :terminal-capabilities
           :make-terminal-capabilities
           :terminal-capabilities-keyboard-disambiguation
           :terminal-capabilities-alternate-key-reporting
           :terminal-capabilities-keypad-identity
           :describe-terminal-capabilities
           :relay-editor-thread
           :relay-last-press
           :relay-mouse-x
           :relay-mouse-y
           :set-display-size
           :set-clipboard
           :set-clipboard-text
           :clipboard-request
           :make-clipboard-request
           :clipboard-replied
           :relay-input-seq))
(in-package :lem-relay/relay)

(defparameter *clipboard-timeout* 0.1
  "Seconds `lem-if:clipboard-paste' waits for the display's clipboard, as
`lem-server' does. Lem asks on every yank, so a display without a
clipboard must not stall the editor.")

;;; Messages sent outside frames (ADR 0011: envelopes, not ops).

(defstruct (set-clipboard (:constructor make-set-clipboard (text)))
  "Put TEXT on the display's clipboard." text)

(defstruct (clipboard-request (:constructor make-clipboard-request ()))
  "Ask the display for its clipboard. The answer names this request by
its `seq', as `reply_to' (ADR 0013), and arrives as a
`clipboard-replied' call.")

(defstruct (terminal-capabilities (:copier nil))
  "The display's confirmed keyboard features, immutable for this Hello.
An instance with all flags NIL is a known legacy session; a missing
instance means an older peer or unknown capabilities. Keypad identity
means the protocol can report provenance, not that every key/lock state
is distinguishable. Each key event remains authoritative."
  (keyboard-disambiguation nil :type boolean :read-only t)
  (alternate-key-reporting nil :type boolean :read-only t)
  (keypad-identity nil :type boolean :read-only t))

;;; The relay

(defclass relay ()
  ((session
    :initform (make-session :suppress (not (uiop:getenv "LEM_RATATUI_DEBUG")))
    :reader relay-session
    :documentation "The frame session. Suppression is off under
LEM_RATATUI_DEBUG (ADR 0012). Read when the implementation is made, at
run time, never baked into a saved image.")
   (sink
    :initarg :sink
    :initform nil
    :accessor relay-sink
    :documentation "Function of one argument, called on Lem's editor thread
with each `frame' and each envelope message for the display. A codec
supplies it, and returns the `seq' it gave the message: messages are
numbered as they are written, in wire order (ADR 0013).")
   (display-width :initform 80 :accessor relay-display-width)
   (display-height :initform 24 :accessor relay-display-height)
   (terminal-capabilities
    :initarg :terminal-capabilities
    :initform nil
    :reader relay-terminal-capabilities
    :documentation "The immutable terminal-capabilities reported by Hello,
or NIL for an older peer or unknown capabilities. A present all-false
value is a known legacy session. Read after the handshake, including in
init.lisp, without depending on the protobuf codec.")
   (foreground
    :initform (lem:make-color #xdd #xdd #xdd)
    :accessor relay-foreground
    :documentation "Lem's default foreground as a colour, never NIL: Lem
falls back to it for attributes without one.")
   (background
    :initform (lem:make-color #x11 #x11 #x11)
    :accessor relay-background
    :documentation "Lem's default background as a colour, never NIL: Lem
decides light or dark theme mode from it.")
   (cursor-shape :initform :box :accessor relay-cursor-shape)
   (editor-thread
    :initform nil
    :accessor relay-editor-thread
    :documentation "Lem's editor thread, recorded by the codec's
`lem-if:invoke' when it starts it; abort interrupts it.")
   (last-press
    :initform nil
    :accessor relay-last-press
    :documentation "The last mouse press, for counting clicks: see
`lem-relay/input:count-click'.")
   (mouse-x :initform -1 :accessor relay-mouse-x)
   (mouse-y :initform -1 :accessor relay-mouse-y)
   (painted
    :initform '()
    :accessor relay-painted
    :documentation "View ids in the order Lem painted them this frame,
latest first. `redraw-display' visits every window each time, in
painter's order, so this is the stacking order (ADR 0012).")
   (last-view-id :initform 0 :accessor relay-last-view-id)
   (input-seq
    :initform nil
    :accessor relay-input-seq
    :documentation "The `seq' of the last display message handed to Lem,
recorded by `lem-relay/input:deliver'; each frame carries it.")
   (clipboard-replies :initform (queue:make-concurrent-queue)
                      :reader relay-clipboard-replies))
  (:documentation "The relay's half of a Lem implementation: every `lem-if'
drawing method, building frames with a `session'.

A mixin, not a subclass of `lem:implementation': Lem offers each direct
subclass of that as an interface to run, and this one cannot run alone.
The concrete class combines it with `lem:implementation', and a codec
gives it a sink and an event loop (`lem-if:invoke')."))

(lem:define-command describe-terminal-capabilities () ()
  "Show the display's confirmed keyboard capabilities for this session.
Unknown means the display did not advertise capabilities. A legacy
session advertises all features as unavailable; keypad provenance still
comes from individual key events. This command never runs at startup."
  (let* ((implementation (lem:implementation))
         (capabilities (and (typep implementation 'relay)
                            (relay-terminal-capabilities implementation))))
    (lem:display-popup-message
     (if capabilities
         (format nil "Terminal keyboard capabilities~%~%Disambiguation: ~:[no~;yes~]~%Alternate keys: ~:[no~;yes~]~%Keypad identity: ~:[no~;yes~]~%~%Keypad identity describes protocol support; each key event is authoritative."
                 (terminal-capabilities-keyboard-disambiguation capabilities)
                 (terminal-capabilities-alternate-key-reporting capabilities)
                 (terminal-capabilities-keypad-identity capabilities))
         "Terminal keyboard capabilities: unknown (not advertised by this display)."))))

(defun send (relay message)
  "Hand MESSAGE to the sink; return the `seq' it was sent as, or NIL when
there is no sink."
  (alexandria:when-let ((sink (relay-sink relay)))
    (funcall sink message)))

(defun set-display-size (relay width height)
  "Record the display's size in cells. The caller then tells Lem, with
`(lem:send-event :resize)', so the windows are laid out again."
  (setf (relay-display-width relay) width
        (relay-display-height relay) height))

;;; Display-wide

(defmethod lem-if:display-width ((relay relay))
  (relay-display-width relay))

(defmethod lem-if:display-height ((relay relay))
  (relay-display-height relay))

(defmethod lem-if:display-title ((relay relay))
  nil)

(defmethod lem-if:set-display-title ((relay relay) title)
  (declare (ignore title)))

(defmethod lem-if:display-fullscreen-p ((relay relay))
  nil)

(defmethod lem-if:set-display-fullscreen-p ((relay relay) fullscreen-p)
  (declare (ignore fullscreen-p)))

(defmethod lem-if:get-char-width ((relay relay))
  1)

(defmethod lem-if:get-char-height ((relay relay))
  1)

(defmethod lem-if:get-foreground-color ((relay relay))
  (relay-foreground relay))

(defmethod lem-if:get-background-color ((relay relay))
  (relay-background relay))

(defmethod lem-if:get-mouse-position ((relay relay))
  (values (relay-mouse-x relay) (relay-mouse-y relay)))

(defmethod lem-if:update-foreground ((relay relay) color-name)
  ;; As ncurses: the new default is what later questions get answered
  ;; with, and it becomes frame state for the display (ADR 0012).
  (alexandria:when-let ((color (lem:parse-color color-name)))
    (setf (relay-foreground relay) color
          (defaults-foreground (session-defaults (relay-session relay))) (pack-color color))))

(defmethod lem-if:update-background ((relay relay) color-name)
  (alexandria:when-let ((color (lem:parse-color color-name)))
    (setf (relay-background relay) color
          (defaults-background (session-defaults (relay-session relay))) (pack-color color))))

(defmethod lem-if:update-cursor-shape ((relay relay) cursor-type)
  (setf (relay-cursor-shape relay) cursor-type))

;;; Views

(defun window-kind (window)
  (cond ((or (lem:floating-window-p window) (lem:attached-window-p window)) :floating)
        ((lem:header-window-p window) :header)
        (t :tile)))

(defmethod lem-if:make-view ((relay relay) window x y width height use-modeline)
  (let ((view (make-view :id (incf (relay-last-view-id relay))
                         :window window
                         :x x :y y :width width :height height
                         :kind (window-kind window)
                         :modeline-p (and use-modeline t)
                         :border (lem:window-border window)
                         :border-shape (and (lem:floating-window-p window)
                                            (lem:floating-window-border-shape window)))))
    (add-op (relay-session relay)
            (make-view-created :view (view-id view)
                               :x x :y y :width width :height height
                               :kind (view-kind view)
                               :modeline-p (view-modeline-p view)
                               :border (view-border view)
                               :border-shape (view-border-shape view)))
    view))

(defmethod lem-if:delete-view ((relay relay) view)
  (add-op (relay-session relay) (make-view-deleted :view (view-id view))))

(defmethod lem-if:view-width ((relay relay) view)
  (view-width view))

(defmethod lem-if:view-height ((relay relay) view)
  (view-height view))

(defmethod lem-if:set-view-size ((relay relay) view width height)
  (setf (view-width view) width
        (view-height view) height)
  (add-op (relay-session relay)
          (make-view-resized :view (view-id view) :width width :height height)))

(defmethod lem-if:set-view-pos ((relay relay) view x y)
  (setf (view-x view) x
        (view-y view) y)
  (add-op (relay-session relay) (make-view-moved :view (view-id view) :x x :y y)))

(defmethod lem-if:clear ((relay relay) view)
  (add-op (relay-session relay) (make-view-cleared :view (view-id view))))

;;; Drawing

(defmethod lem-if:redraw-view-before ((relay relay) view)
  (push (view-id view) (relay-painted relay)))

(defmethod lem-if:render-line ((relay relay) view x y objects height)
  (declare (ignore height))
  (draw:render-line (relay-session relay) view x y objects))

(defmethod lem-if:render-line-on-modeline ((relay relay) view left-objects right-objects
                                           default-attribute height)
  (declare (ignore height))
  (draw:render-line-on-modeline (relay-session relay) view
                                left-objects right-objects default-attribute))

(defmethod lem-if:clear-to-end-of-window ((relay relay) view y)
  (draw:clear-to-end-of-window (relay-session relay) view y))

(defmethod lem-if:object-width ((relay relay) drawing-object)
  (draw:object-width drawing-object))

(defmethod lem-if:object-height ((relay relay) drawing-object)
  (declare (ignore drawing-object))
  1)

;;; Frames

(defun current-cursor (relay)
  "The primary cursor, where the drawing noted it (`lem-relay/draw')."
  (let* ((window (lem:current-window))
         (view (lem:window-view window)))
    (when view
      (make-cursor :view (view-id view)
                   :x (lem:last-print-cursor-x window)
                   :y (lem:last-print-cursor-y window)
                   :visible (not (lem:window-cursor-invisible-p window))
                   :shape (relay-cursor-shape relay)))))

(defmethod lem-if:update-display ((relay relay))
  (let ((session (relay-session relay)))
    (add-op session (make-views-stacked
                     :views (remove-duplicates (reverse (relay-painted relay))
                                               :from-end t)))
    (setf (relay-painted relay) '())
    (alexandria:when-let ((frame (finish-frame session (current-cursor relay)
                                               (relay-input-seq relay))))
      (send relay frame))))

;;; Clipboard

(defmethod lem-if:clipboard-copy ((relay relay) text)
  (send relay (make-set-clipboard text)))

(defmethod lem-if:clipboard-paste ((relay relay))
  (alexandria:when-let ((seq (send relay (make-clipboard-request))))
    (await-clipboard relay seq)))

(defun await-clipboard (relay id)
  "The text replied to the request sent as ID, or NIL after
`*clipboard-timeout*'.
A late reply to an earlier request is discarded, not mistaken for this
one's."
  (loop :with deadline := (+ (get-internal-real-time)
                             (* *clipboard-timeout* internal-time-units-per-second))
        :for remaining := (/ (- deadline (get-internal-real-time))
                             internal-time-units-per-second)
        :while (plusp remaining)
        :do (let ((reply (queue:dequeue (relay-clipboard-replies relay)
                                        :timeout remaining :timeout-value nil)))
              (when (and reply (eql id (car reply)))
                (return (cdr reply))))))

(defun clipboard-replied (relay reply-to text)
  "Hand the display's clipboard TEXT, answering the request sent as
REPLY-TO, to the `lem-if:clipboard-paste' waiting for it.

Called from the thread reading the display, never the editor's: the
editor thread is blocked in `lem-if:clipboard-paste' waiting for exactly
this, so the reply cannot go through `lem:send-event' (ADR 0012)."
  (queue:enqueue (relay-clipboard-replies relay) (cons reply-to text)))
