(defpackage :lem-relay/json/codec
  (:use :cl :lem-relay/frame :lem-relay/input)
  (:import-from :lem-relay/relay
   :set-clipboard
   :set-clipboard-text
   :clipboard-request
   :clipboard-request-id)
  (:export :codec-state
           :make-codec-state
           :encode
           :notification
           :response
           :parse
           :decode))
(in-package :lem-relay/json/codec)

;;; Today's wire, as lem-server writes it and lem-protocol reads it:
;;; JSON-RPC 2.0 notifications, a frame as one `bulk' whose last
;;; instruction is `update-display'. ADR 0012 says which parts of the
;;; model this wire can express; `views-stacked' has no equivalent and is
;;; not sent.

(defstruct (codec-state (:constructor make-codec-state ()))
  "What the JSON codec remembers across messages: the cursor shape last
announced, and the clipboard request last sent. Today's wire has no
request ids, so a reply answers the latest request."
  (cursor-shape nil)
  (clipboard-request-id nil))

;;; JSON values

(defun obj (&rest keys-and-values)
  (let ((table (make-hash-table :test 'equal)))
    (loop :for (key value) :on keys-and-values :by #'cddr
          :do (setf (gethash key table) value))
    table))

(defun bool (value)
  (if value 'yason:true 'yason:false))

(defun hex (color)
  "A packed colour as \"#RRGGBB\", or NIL (JSON null)."
  (and color (format nil "#~6,'0X" color)))

(defun to-octets (value)
  (babel:string-to-octets (with-output-to-string (stream) (yason:encode value stream))
                          :encoding :utf-8))

(defun notification (method params)
  "A JSON-RPC notification, as octets."
  (to-octets (obj "jsonrpc" "2.0" "method" method "params" params)))

(defun response (id result)
  "A JSON-RPC response to request ID, as octets."
  (to-octets (obj "jsonrpc" "2.0" "id" id "result" result)))

;;; The model, as lem-server's instructions

(defun view-info (view)
  (obj "id" view))

(defun attribute (style)
  (and style
       (obj "foreground" (hex (style-foreground style))
            "background" (hex (style-background style))
            "reverse" (bool (style-reverse style))
            "bold" (bool (style-bold style))
            "underline" (let ((underline (style-underline style)))
                          (if (eq underline t) 'yason:true (hex underline)))
            "cursor" (bool (style-cursor style)))))

(defun instruction (method argument)
  (obj "method" method "argument" argument))

(defun put-instruction (method put)
  (instruction method (obj "viewInfo" (view-info (text-put-view put))
                           "x" (text-put-x put)
                           "y" (text-put-y put)
                           "text" (text-put-text put)
                           "textWidth" (text-put-width put)
                           "attribute" (attribute (text-put-style put)))))

(defun op-instructions (op)
  "The instructions standing for OP, in order."
  (etypecase op
    (view-created
     (list (instruction "make-view"
                        (obj "id" (view-created-view op)
                             "x" (view-created-x op)
                             "y" (view-created-y op)
                             "width" (view-created-width op)
                             "height" (view-created-height op)
                             "use_modeline" (bool (view-created-modeline-p op))
                             "kind" (string-downcase (view-created-kind op))
                             "type" "editor"
                             "border" (view-created-border op)
                             "border_shape" (and (view-created-border-shape op)
                                                 (string-downcase
                                                  (view-created-border-shape op)))))))
    (view-deleted
     (list (instruction "delete-view" (obj "viewInfo" (view-info (op-view op))))))
    (view-moved
     (list (instruction "move-view" (obj "viewInfo" (view-info (op-view op))
                                         "x" (view-moved-x op)
                                         "y" (view-moved-y op)))))
    (view-resized
     (list (instruction "resize-view" (obj "viewInfo" (view-info (op-view op))
                                           "width" (view-resized-width op)
                                           "height" (view-resized-height op)))))
    (view-cleared
     (list (instruction "clear" (obj "viewInfo" (view-info (op-view op))))))
    (views-stacked '())
    (text-put
     (list (put-instruction "put" op)))
    (line-cleared
     (list (instruction "clear-eol" (obj "viewInfo" (view-info (op-view op))
                                         "x" (line-cleared-x op)
                                         "y" (line-cleared-y op)))))
    (rest-cleared
     (list (instruction "clear-eob" (obj "viewInfo" (view-info (op-view op))
                                         "x" 0
                                         "y" (rest-cleared-y op)))))
    (modeline-painted
     (mapcar (lambda (put) (put-instruction "modeline-put" put))
             (modeline-painted-puts op)))))

(defun frame-notifications (frame state)
  "FRAME as today's notifications: default colours and cursor shape when
they changed, as lem-server sends them, then one `bulk'."
  (let ((notifications '())
        (cursor (frame-cursor frame)))
    (alexandria:when-let ((defaults (frame-defaults frame)))
      (alexandria:when-let ((foreground (hex (defaults-foreground defaults))))
        (push (notification "update-foreground" foreground) notifications))
      (alexandria:when-let ((background (hex (defaults-background defaults))))
        (push (notification "update-background" background) notifications)))
    (when (and cursor (not (eq (cursor-shape cursor) (codec-state-cursor-shape state))))
      (setf (codec-state-cursor-shape state) (cursor-shape cursor))
      (push (notification "update-cursor-shape"
                          (obj "cursorType" (string-downcase (cursor-shape cursor))))
            notifications))
    (push (notification
           "bulk"
           (coerce (append (mapcan #'op-instructions (frame-ops frame))
                           (when cursor
                             (list (instruction "move-cursor"
                                                (obj "viewInfo" (view-info (cursor-view cursor))
                                                     "x" (cursor-x cursor)
                                                     "y" (cursor-y cursor)))))
                           (list (instruction "update-display" nil)))
                   'vector))
          notifications)
    (nreverse notifications)))

(defun encode (message state)
  "The framed-message bodies (octet vectors) standing for MESSAGE, a
`frame' or an envelope message from the relay, in the order to send them."
  (etypecase message
    (frame (frame-notifications message state))
    (set-clipboard
     (list (notification "set-clipboard-text" (obj "text" (set-clipboard-text message)))))
    (clipboard-request
     (setf (codec-state-clipboard-request-id state) (clipboard-request-id message))
     (list (notification "get-clipboard-text" (obj))))))

;;; What the display sends

(defun parse (body)
  "BODY, an octet vector holding one JSON value, as Lisp data: objects as
EQUAL hash tables."
  (yason:parse (babel:octets-to-string body :encoding :utf-8)))

(defun mouse-button (number)
  "lem-server's numbering, the browser's: 0 left, 1 middle, 2 right."
  (case number
    (0 :left)
    (1 :middle)
    (2 :right)))

(defun key-from (value)
  (make-key-input (gethash "key" value)
                  :ctrl (gethash "ctrl" value)
                  :meta (gethash "meta" value)
                  :shift (gethash "shift" value)
                  :super (gethash "super" value)))

(defun mouse-from (action value)
  (make-mouse-input action (gethash "x" value) (gethash "y" value)
                    :button (mouse-button (gethash "button" value))
                    :clicks (or (gethash "clicks" value) 1)
                    :wheel-x (or (gethash "wheelX" value) 0)
                    :wheel-y (or (gethash "wheelY" value) 0)))

(defun input-from (params)
  "The inputs an `input' notification's PARAMS stand for, as
lem-server's `input-callback' reads them."
  (let ((kind (gethash "kind" params))
        (value (gethash "value" params)))
    (alexandria:switch (kind :test #'equal)
      ("key" (and value (list (key-from value))))
      ("abort" (list (make-abort-input)))
      ("resize" (list (make-resize-input (gethash "width" value) (gethash "height" value))))
      ("mousedown" (list (mouse-from :down value)))
      ("mouseup" (list (mouse-from :up value)))
      ("mousemove" (list (mouse-from :move value)))
      ("wheel" (list (mouse-from :wheel value)))
      ("input-string" (map 'list (lambda (char) (make-key-input (string char))) value))
      (t (log:warn "lem-relay/json: ignoring input of kind ~S" kind)
         '()))))

(defun decode (message state)
  "What MESSAGE, a parsed JSON-RPC message from the display, asks for:

  (:login ID WIDTH HEIGHT FOREGROUND BACKGROUND)
  (:redraw WIDTH HEIGHT)   ; either may be NIL: redraw at the current size
  (:input INPUT...)
  (:ignored METHOD)"
  (let ((method (gethash "method" message))
        (params (gethash "params" message)))
    (flet ((size (key)
             (let ((size (and (hash-table-p params) (gethash "size" params))))
               (and (hash-table-p size) (gethash key size)))))
      (alexandria:switch (method :test #'equal)
        ("login" (list :login (gethash "id" message) (size "width") (size "height")
                       (gethash "foreground" params) (gethash "background" params)))
        ("redraw" (list :redraw (size "width") (size "height")))
        ("input" (cons :input (input-from params)))
        ("got-clipboard-text"
         (list :input (make-clipboard-reply (codec-state-clipboard-request-id state)
                                            (gethash "text" params))))
        (t (list :ignored method))))))
