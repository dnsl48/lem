(defpackage :lem-relay/tests/json
  (:use :cl :rove :lem-relay/frame :lem-relay/input)
  (:local-nicknames (:framing :lem-relay/json/framing)
                    (:codec :lem-relay/json/codec)
                    (:serve :lem-relay/json/serve)
                    (:relay :lem-relay/relay)))
(in-package :lem-relay/tests/json)

(defun octets (string)
  (babel:string-to-octets string :encoding :utf-8))

(defun text (octets)
  (babel:octets-to-string octets :encoding :utf-8))

(defun framed (&rest bodies)
  "BODIES (strings) framed back to back, as an octet vector."
  (let ((out (flexi-streams:make-in-memory-output-stream)))
    (dolist (body bodies)
      (framing:write-message out (octets body)))
    (flexi-streams:get-output-stream-sequence out)))

(defun unframed (octets)
  "Every message body in OCTETS, as strings."
  (let ((in (flexi-streams:make-in-memory-input-stream octets)))
    (loop :for body := (framing:read-message in)
          :while body
          :collect (text body))))

;;; Framing

(deftest messages-round-trip-back-to-back
  (ok (equal '("{\"t\":\"héllo wörld\"}" "{\"next\":true}")
             (unframed (framed "{\"t\":\"héllo wörld\"}" "{\"next\":true}")))))

(deftest the-length-counts-bytes-not-characters
  (let ((wire (text (framed "{\"a\":\"é\"}"))))
    (ok (search "Content-Length: 10" wire) "9 characters, 10 bytes")))

(deftest the-end-between-messages-is-not-an-error
  (ok (null (framing:read-message (flexi-streams:make-in-memory-input-stream #())))))

(deftest the-end-inside-a-header-is
  (ok (signals (framing:read-message
                (flexi-streams:make-in-memory-input-stream (octets "Content-Len"))))))

;;; Encoding frames

(defun parsed (bodies)
  (mapcar (lambda (body) (codec:parse body)) bodies))

(defun method-of (message) (gethash "method" message))

(defun bulk-of (messages)
  (gethash "params" (find "bulk" messages :key #'method-of :test #'equal)))

(defun instruction-methods (bulk)
  (map 'list (lambda (instruction) (gethash "method" instruction)) bulk))

(defun sample-frame (&key defaults (shape :box))
  (let ((session (make-session)))
    (finish-frame session nil)          ; the first frame, with its defaults
    (when defaults
      (setf (session-defaults session) defaults))
    (add-op session (make-view-created :view 1 :x 0 :y 0 :width 80 :height 23
                                       :kind :tile :modeline-p t))
    (add-op session (make-text-put :view 1 :x 0 :y 0 :text "defun" :width 5
                                   :style (make-style :foreground #xAABBCC :bold t)))
    (add-op session (make-modeline-painted
                     :view 1 :puts (list (make-text-put :view 1 :x 0 :y 0 :text "L1"
                                                        :width 2 :style nil))))
    (add-op session (make-views-stacked :views '(1)))
    (finish-frame session (make-cursor :view 1 :x 5 :y 0 :shape shape))))

(deftest a-frame-is-one-bulk-ending-in-update-display
  (let ((messages (parsed (codec:encode (sample-frame) (codec:make-codec-state)))))
    (ok (equal '("make-view" "put" "modeline-put" "move-cursor" "update-display")
               (instruction-methods (bulk-of messages)))
        "views-stacked has no equivalent and is left out")))

(deftest a-put-carries-lem-servers-attribute
  (let* ((wire (text (car (last (codec:encode (sample-frame) (codec:make-codec-state))))))
         (put (elt (bulk-of (list (codec:parse (octets wire)))) 1))
         (attribute (gethash "attribute" (gethash "argument" put))))
    (ok (equal "#AABBCC" (gethash "foreground" attribute)))
    (ok (eq t (gethash "bold" attribute)))
    (ok (search "\"reverse\":false" wire) "false, not null: lem-protocol reads a bool")
    (ok (= 5 (gethash "textWidth" (gethash "argument" put))))))

(deftest a-view-carries-what-lem-protocol-reads
  (let* ((session (make-session)))
    (add-op session (make-view-created :view 4 :x 1 :y 2 :width 30 :height 8
                                       :kind :floating :border 1
                                       :border-shape :drop-curtain))
    (let* ((bulk (bulk-of (parsed (codec:encode (finish-frame session nil)
                                                (codec:make-codec-state)))))
           (view (gethash "argument" (elt bulk 0))))
      (ok (equal "floating" (gethash "kind" view)))
      (ok (equal "drop-curtain" (gethash "border_shape" view)))
      (ok (equal "editor" (gethash "type" view)))
      (ok (= 1 (gethash "border" view))))))

(deftest changed-default-colours-come-before-the-bulk
  (let ((messages (parsed (codec:encode (sample-frame :defaults (make-defaults :background #x224466))
                                        (codec:make-codec-state)))))
    (ok (equal "update-background" (method-of (first messages))))
    (ok (equal "#224466" (gethash "params" (first messages))))))

(deftest the-cursor-shape-is-announced-when-it-changes
  (let ((state (codec:make-codec-state)))
    (flet ((shape-notices (shape)
             (count "update-cursor-shape"
                    (parsed (codec:encode (sample-frame :shape shape) state))
                    :key #'method-of :test #'equal)))
      (ok (= 1 (shape-notices :bar)))
      (ok (= 0 (shape-notices :bar)))
      (ok (= 1 (shape-notices :box))))))

;;; Clipboard

(deftest the-clipboard-messages-are-lem-servers
  (let ((state (codec:make-codec-state)))
    (ok (equal "set-clipboard-text"
               (method-of (first (parsed (codec:encode (relay::make-set-clipboard "x") state))))))
    (ok (equal "get-clipboard-text"
               (method-of (first (parsed (codec:encode (relay::make-clipboard-request 7) state))))))
    (let ((reply (second (codec:decode (codec:parse (octets "{\"jsonrpc\":\"2.0\",\"method\":\"got-clipboard-text\",\"params\":{\"text\":\"hi\"}}"))
                                       state))))
      (ok (typep reply 'clipboard-reply))
      (ok (= 7 (lem-relay/input::clipboard-reply-id reply)) "answering the latest request"))))

;;; Decoding

(defun decoded (json)
  (codec:decode (codec:parse (octets json)) (codec:make-codec-state)))

(deftest a-login-is-understood
  (ok (equal '(:login 1 100 30 "#DDDDDD" "#111111")
             (decoded "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"login\",\"params\":{\"size\":{\"width\":100,\"height\":30},\"foreground\":\"#DDDDDD\",\"background\":\"#111111\"}}"))))

(deftest a-key-is-a-key-input
  (let ((input (second (decoded "{\"jsonrpc\":\"2.0\",\"method\":\"input\",\"params\":{\"kind\":\"key\",\"value\":{\"key\":\"x\",\"ctrl\":true,\"meta\":false,\"shift\":false,\"super\":false}}}"))))
    (ok (typep input 'key-input))
    (ok (equal "x" (lem:key-sym (key-event input))))
    (ok (lem:key-ctrl (key-event input)))))

(deftest an-input-string-is-one-key-per-character
  (ok (= 3 (length (rest (decoded "{\"jsonrpc\":\"2.0\",\"method\":\"input\",\"params\":{\"kind\":\"input-string\",\"value\":\"abc\"}}"))))))

(deftest a-redraw-without-a-size-is-still-a-redraw
  (ok (equal '(:redraw nil nil) (decoded "{\"jsonrpc\":\"2.0\",\"method\":\"redraw\",\"params\":{}}"))))

;;; Serving

(defclass serve-relay (relay:relay) ())

(defun login-json (&key (width 100) (height 30))
  (format nil "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"login\",\"params\":{\"size\":{\"width\":~D,\"height\":~D},\"foreground\":\"#DDDDDD\",\"background\":\"#101010\"}}"
          width height))

(deftest serve-logs-in-then-starts-the-editor
  (let* ((relay (make-instance 'serve-relay))
         (input (flexi-streams:make-in-memory-input-stream
                 (framed (login-json)
                         "{\"jsonrpc\":\"2.0\",\"method\":\"input\",\"params\":{\"kind\":\"key\",\"value\":{\"key\":\"q\"}}}")))
         (output (flexi-streams:make-in-memory-output-stream))
         (editor nil))
    (ok (null (serve:serve relay
                           (lambda (initialize finalize)
                             (declare (ignore finalize))
                             (setf editor (bt2:make-thread
                                           (lambda () (funcall initialize) :started))))
                           :input input :output output))
        "returns when the display hangs up")
    (ok (eq :started (bt2:join-thread editor)) "the editor started after the login")
    (ok (= 100 (lem-if:display-width relay)))
    (ok (eql #x101010 (pack-color (lem-if:get-background-color relay)))
        "the terminal's colours answer Lem until a theme sets its own")
    (let ((answer (codec:parse (octets (first (unframed (flexi-streams:get-output-stream-sequence output)))))))
      (ok (eql 1 (gethash "id" answer)) "the login is answered"))
    (let ((event (lem:receive-event 0.5)))
      (ok (equal "q" (lem:key-sym event)) "the key reached the editor"))))

(defclass endless-input (trivial-gray-streams:fundamental-binary-input-stream) ()
  (:documentation "A display that stays connected and never says anything."))

(defmethod trivial-gray-streams:stream-read-byte ((stream endless-input))
  (loop (sleep 60)))

(deftest the-editor-exiting-ends-serve-with-its-report
  (let ((relay (make-instance 'serve-relay)))
    (ok (equal "crash report"
               (serve:serve relay
                            (lambda (initialize finalize)
                              (declare (ignore initialize))
                              (bt2:make-thread
                               (lambda () (sleep 0.2) (funcall finalize "crash report"))))
                            :input (make-instance 'endless-input)
                            :output (flexi-streams:make-in-memory-output-stream))))))
