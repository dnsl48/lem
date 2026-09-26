(defpackage :lem-relay/protobuf/codec
  (:use :cl :lem-relay/frame :lem-relay/input)
  (:local-nicknames (:pb :cl-protobufs.lem.relay.v1)
                    (:proto :cl-protobufs))
  (:import-from :lem-relay/relay
   :set-clipboard
   :set-clipboard-text
   :clipboard-request)
  (:export :+protocol-version+
           :codec-state
           :make-codec-state
           :next-seq
           :now-microseconds
           :encode
           :welcome
           :exit-message
           :decode
           :key-name))
(in-package :lem-relay/protobuf/codec)

(defconstant +protocol-version+ 1
  "The revision of lem.relay.v1 this codec speaks (relay.proto, `Hello').")

(defparameter *named-keys*
  '((:named-key-enter . "Return")
    (:named-key-tab . "Tab")
    (:named-key-backspace . "Backspace")
    (:named-key-escape . "Escape")
    (:named-key-insert . "Insert")
    (:named-key-delete . "Delete")
    (:named-key-up . "Up")
    (:named-key-down . "Down")
    (:named-key-left . "Left")
    (:named-key-right . "Right")
    (:named-key-home . "Home")
    (:named-key-end . "End")
    (:named-key-page-up . "PageUp")
    (:named-key-page-down . "PageDown")
    (:named-key-context-menu . "ContextMenu"))
  "Lem's name for each `NamedKey' (src/key.lisp). The display describes a
key and the relay names it (ADR 0014).")

(defstruct (codec-state (:constructor make-codec-state ()))
  "What the codec remembers across messages: the `seq' last given to an
outgoing message, and the styles interned so far.

Style ids are the codec's (ADR 0012): the model carries styles as values,
and the codec gives each distinct one an id the first time a frame uses
it, sending its definition in that frame. Ids are stable for the
session; 0 is no style."
  (seq 0)
  (style-ids (make-hash-table :test 'equal))
  (last-style-id 0))

(defun next-seq (state)
  "Number the next outgoing message. Call under the output lock, so
numbers follow wire order (ADR 0013)."
  (incf (codec-state-seq state)))

(defun now-microseconds ()
  "Microseconds on the relay's monotonic clock (ADR 0013)."
  (floor (* (get-internal-real-time) 1000000) internal-time-units-per-second))

;;; Encoding

(defun optionals (&rest keys-and-values)
  "KEYS-AND-VALUES without the pairs whose value is NIL: an optional field
is left unset by not passing it."
  (loop :for (key value) :on keys-and-values :by #'cddr
        :when value :append (list key value)))

(defun pb-style (id style)
  (let ((underline (style-underline style)))
    (apply #'pb:make-style
           :id id
           :bold (style-bold style)
           :reverse (style-reverse style)
           :underline (and underline t)
           :cursor (style-cursor style)
           :italic (style-italic style)
           :strikethrough (style-strikethrough style)
           :dim (style-dim style)
           (optionals :foreground (style-foreground style)
                      :background (style-background style)
                      :underline-color (and (integerp underline) underline)))))

(defun style-id (state style new-styles)
  "STYLE's id, interning it if it is new, when it is pushed onto the cell
NEW-STYLES for the frame to define."
  (if (null style)
      0
      (let ((key (list (style-foreground style) (style-background style)
                       (style-bold style) (style-reverse style)
                       (style-underline style) (style-cursor style)
                       (style-italic style) (style-strikethrough style) (style-dim style))))
        (or (gethash key (codec-state-style-ids state))
            (let ((id (incf (codec-state-last-style-id state))))
              (push (pb-style id style) (car new-styles))
              (setf (gethash key (codec-state-style-ids state)) id))))))

(defun view-kind (kind)
  (ecase kind
    (:tile :view-kind-tile)
    (:header :view-kind-header)
    (:floating :view-kind-floating)))

(defun border-shape (shape)
  (ecase shape
    ((nil) :border-shape-none)
    (:drop-curtain :border-shape-drop-curtain)
    (:left-border :border-shape-left-border)))

(defun cursor-shape-value (shape)
  (ecase shape
    (:box :cursor-shape-box)
    (:bar :cursor-shape-bar)
    (:underline :cursor-shape-underline)))

(defun pb-op (op state new-styles)
  (flet ((style (style) (style-id state style new-styles)))
    (etypecase op
      (view-created
       (pb:make-op :view-created
                   (pb:make-view-created :view (view-created-view op)
                                         :x (view-created-x op)
                                         :y (view-created-y op)
                                         :width (view-created-width op)
                                         :height (view-created-height op)
                                         :kind (view-kind (view-created-kind op))
                                         :modeline (view-created-modeline-p op)
                                         :border (or (view-created-border op) 0)
                                         :border-shape (border-shape
                                                        (view-created-border-shape op)))))
      (view-deleted
       (pb:make-op :view-deleted (pb:make-view-deleted :view (op-view op))))
      (view-moved
       (pb:make-op :view-moved (pb:make-view-moved :view (op-view op)
                                                   :x (view-moved-x op)
                                                   :y (view-moved-y op))))
      (view-resized
       (pb:make-op :view-resized (pb:make-view-resized :view (op-view op)
                                                       :width (view-resized-width op)
                                                       :height (view-resized-height op))))
      (view-cleared
       (pb:make-op :view-cleared (pb:make-view-cleared :view (op-view op))))
      (views-stacked
       (pb:make-op :views-stacked (pb:make-views-stacked :views (views-stacked-views op))))
      (text-put
       (pb:make-op :put (pb:make-put :view (text-put-view op)
                                     :x (text-put-x op)
                                     :y (text-put-y op)
                                     :text (text-put-text op)
                                     :width (text-put-width op)
                                     :style (style (text-put-style op)))))
      (line-cleared
       (pb:make-op :line-cleared (pb:make-line-cleared :view (op-view op)
                                                       :x (line-cleared-x op)
                                                       :y (line-cleared-y op))))
      (rest-cleared
       (pb:make-op :rest-cleared (pb:make-rest-cleared :view (op-view op)
                                                       :y (rest-cleared-y op))))
      (modeline-painted
       (pb:make-op :modeline-painted
                   (pb:make-modeline-painted
                    :view (op-view op)
                    :runs (mapcar (lambda (put)
                                    (pb:make-run :x (text-put-x put)
                                                 :text (text-put-text put)
                                                 :width (text-put-width put)
                                                 :style (style (text-put-style put))))
                                  (modeline-painted-puts op))))))))

(defun pb-frame (frame state)
  (let* ((new-styles (list '()))
         (ops (mapcar (lambda (op) (pb-op op state new-styles)) (frame-ops frame)))
         (cursor (frame-cursor frame))
         (defaults (frame-defaults frame)))
    (apply #'pb:make-frame
           :styles (reverse (car new-styles))
           :ops ops
           (optionals
            :input-seq (frame-input-seq frame)
            :cursor (and cursor
                         (pb:make-cursor :view (cursor-view cursor)
                                         :x (cursor-x cursor)
                                         :y (cursor-y cursor)
                                         :hidden (not (cursor-visible cursor))
                                         :shape (cursor-shape-value (cursor-shape cursor))))
            :defaults (and defaults
                           (apply #'pb:make-defaults
                                  (optionals :foreground (defaults-foreground defaults)
                                             :background (defaults-background defaults))))))))

(defun envelope (seq time &rest message)
  (proto:serialize-to-bytes (apply #'pb:make-to-display :seq seq :time-us time message)))

(defun encode (message state seq)
  "MESSAGE, a `frame' or an envelope message from the relay, as one
serialised `ToDisplay' numbered SEQ. A frame is timed when it closed;
anything else, now."
  (etypecase message
    (frame (envelope seq (frame-time message) :frame (pb-frame message state)))
    (set-clipboard
     (envelope seq (now-microseconds)
               :set-clipboard (pb:make-set-clipboard :text (set-clipboard-text message))))
    (clipboard-request
     (envelope seq (now-microseconds) :clipboard-request (pb:make-clipboard-request)))))

(defun welcome (seq session-id)
  "A serialised `Welcome' numbered SEQ, echoing SESSION-ID."
  (envelope seq (now-microseconds)
            :welcome (pb:make-welcome :protocol-version +protocol-version+
                                      :session-id session-id)))

(defun exit-message (seq reason)
  "A serialised `Exit' numbered SEQ; REASON is empty for a normal exit."
  (envelope seq (now-microseconds) :exit (pb:make-exit :reason reason)))

;;; Decoding

(defun key-name (key)
  "Lem's name for a `Key' (ADR 0014)."
  (ecase (pb:key.code-case key)
    (pb:text (pb:key.text key))
    (pb:named (or (cdr (assoc (pb:key.named key) *named-keys*))
                  (error "No Lem name for ~S." (pb:key.named key))))
    (pb:function (format nil "F~D" (pb:key.function key)))))

(defun key-input-from (key seq time)
  (let ((modifiers (pb:key.modifiers key)))
    (make-key-input (key-name key)
                    :ctrl (and (member :modifier-ctrl modifiers) t)
                    :meta (and (member :modifier-meta modifiers) t)
                    :shift (and (member :modifier-shift modifiers) t)
                    :super (and (member :modifier-super modifiers) t)
                    :time time :seq seq)))

(defun button (value)
  (ecase value
    (:button-unspecified nil)
    (:button-left :left)
    (:button-middle :middle)
    (:button-right :right)))

(defun mouse-input-from (mouse seq time)
  (let ((x (pb:mouse.x mouse))
        (y (pb:mouse.y mouse)))
    (ecase (pb:mouse.action-case mouse)
      (pb:press
       (make-mouse-input :down x y :button (button (pb:press.button (pb:mouse.press mouse)))
                                   :time time :seq seq))
      (pb:release
       (make-mouse-input :up x y :button (button (pb:release.button (pb:mouse.release mouse)))
                                 :time time :seq seq))
      (pb:move
       (let ((move (pb:mouse.move mouse)))
         (make-mouse-input :move x y :button (and (pb:move.has-button move)
                                                  (button (pb:move.button move)))
                                     :time time :seq seq)))
      (pb:wheel
       (let ((wheel (pb:mouse.wheel mouse)))
         (make-mouse-input :wheel x y :wheel-x (pb:wheel.dx wheel) :wheel-y (pb:wheel.dy wheel)
                                      :time time :seq seq))))))

(defun decode (body)
  "What BODY, one serialised `ToEditor', asks for:

  (:hello SEQ PROTOCOL-VERSION SESSION-ID WIDTH HEIGHT FOREGROUND BACKGROUND)
  (:input INPUT)
  (:ignored)          ; a message this revision does not know"
  (let* ((message (proto:deserialize-from-bytes 'pb:to-editor body))
         (seq (pb:to-editor.seq message))
         (time (pb:to-editor.time-us message)))
    (case (pb:to-editor.message-case message)
      (pb:hello
       (let ((hello (pb:to-editor.hello message)))
         (list :hello seq
               (pb:hello.protocol-version hello)
               (pb:hello.session-id hello)
               (pb:hello.width hello)
               (pb:hello.height hello)
               (and (pb:hello.has-foreground hello) (pb:hello.foreground hello))
               (and (pb:hello.has-background hello) (pb:hello.background hello)))))
      (pb:key
       (list :input (key-input-from (pb:to-editor.key message) seq time)))
      (pb:abort
       (list :input (make-abort-input :time time :seq seq)))
      (pb:paste
       (list :input (make-paste-input (pb:paste.text (pb:to-editor.paste message))
                                      :time time :seq seq)))
      (pb:mouse
       (list :input (mouse-input-from (pb:to-editor.mouse message) seq time)))
      (pb:resize
       (let ((resize (pb:to-editor.resize message)))
         (list :input (make-resize-input (pb:resize.width resize) (pb:resize.height resize)
                                         :time time :seq seq))))
      (pb:clipboard-reply
       (let ((reply (pb:to-editor.clipboard-reply message)))
         (list :input (make-clipboard-reply (and (pb:clipboard-reply.has-text reply)
                                                 (pb:clipboard-reply.text reply))
                                            :reply-to (pb:clipboard-reply.reply-to reply)
                                            :time time :seq seq))))
      (t (list :ignored)))))
