(defpackage :lem-relay/tests/protobuf
  (:use :cl :rove :lem-relay/frame :lem-relay/input)
  (:local-nicknames (:pb :cl-protobufs.lem.relay.v1)
                    (:proto :cl-protobufs)
                    (:framing :lem-relay/protobuf/framing)
                    (:codec :lem-relay/protobuf/codec)
                    (:serve :lem-relay/protobuf/serve)
                    (:relay :lem-relay/relay)))
(in-package :lem-relay/tests/protobuf)

;;; Framing

(defun delimited (&rest bodies)
  (let ((out (flexi-streams:make-in-memory-output-stream)))
    (dolist (body bodies)
      (framing:write-delimited out body))
    (flexi-streams:get-output-stream-sequence out)))

(defun undelimited (octets)
  (let ((in (flexi-streams:make-in-memory-input-stream octets)))
    (loop :for body := (framing:read-delimited in)
          :while body
          :collect body)))

(defun octets (&rest bytes)
  (coerce bytes '(vector (unsigned-byte 8))))

(deftest lengths-are-varints
  (ok (equalp (octets 3 1 2 3) (delimited (octets 1 2 3))) "one byte up to 127")
  (let ((body (make-array 300 :element-type '(unsigned-byte 8) :initial-element 7)))
    (ok (equalp (octets #xAC #x02) (subseq (delimited body) 0 2)) "300 is AC 02")
    (ok (equalp (list body) (undelimited (delimited body))))))

(deftest messages-round-trip-back-to-back
  (let ((a (octets 1)) (b (octets)) (c (octets 9 9)))
    (ok (equalp (list a b c) (undelimited (delimited a b c))) "the empty one included")))

(deftest the-end-between-messages-is-not-an-error
  (ok (null (framing:read-delimited (flexi-streams:make-in-memory-input-stream (octets))))))

(deftest the-end-inside-a-prefix-or-message-is
  (ok (signals (framing:read-delimited (flexi-streams:make-in-memory-input-stream (octets #x80)))))
  (ok (signals (framing:read-delimited (flexi-streams:make-in-memory-input-stream (octets 5 1))))))

;;; Encoding

(defun to-display (bytes)
  (proto:deserialize-from-bytes 'pb:to-display bytes))

(defun sample-frame (session &key (style (make-style :foreground #xAABBCC :bold t)))
  (add-op session (make-view-created :view 1 :x 0 :y 0 :width 80 :height 23
                                     :kind :floating :modeline-p t :border 1
                                     :border-shape :drop-curtain))
  (add-op session (make-text-put :view 1 :x 0 :y 0 :text "日本" :width 4 :style style))
  (add-op session (make-text-put :view 1 :x 4 :y 0 :text "x" :width 1 :style nil))
  (add-op session (make-modeline-painted
                   :view 1 :puts (list (make-text-put :view 1 :x 0 :y 0 :text "L1"
                                                      :width 2 :style style))))
  (add-op session (make-views-stacked :views '(1)))
  (finish-frame session (make-cursor :view 1 :x 4 :y 0 :shape :bar) 12))

(deftest a-frame-carries-its-envelope-and-contents
  (let* ((state (codec:make-codec-state))
         (session (make-session))
         (frame (sample-frame session))
         (message (to-display (codec:encode frame state 7)))
         (pb-frame (pb:to-display.frame message)))
    (ok (= 7 (pb:to-display.seq message)))
    (ok (= (frame-time frame) (pb:to-display.time-us message)) "timed when it closed")
    (ok (= 12 (pb:frame.input-seq pb-frame)))
    (ok (equal '(pb:view-created pb:put pb:put pb:modeline-painted pb:views-stacked)
               (mapcar #'pb:op.op-case (pb:frame.ops pb-frame))))
    (let ((view (pb:op.view-created (first (pb:frame.ops pb-frame)))))
      (ok (eq :view-kind-floating (pb:view-created.kind view)))
      (ok (eq :border-shape-drop-curtain (pb:view-created.border-shape view))))
    (let ((cursor (pb:frame.cursor pb-frame)))
      (ok (eq :cursor-shape-bar (pb:cursor.shape cursor)))
      (ng (pb:cursor.hidden cursor)))
    (ok (pb:frame.has-defaults pb-frame) "the first frame's default colours")))

(deftest a-style-is-defined-once-and-referred-to-by-id
  (let* ((state (codec:make-codec-state))
         (session (make-session))
         (first-frame (pb:to-display.frame (to-display (codec:encode (sample-frame session) state 1))))
         (styles (pb:frame.styles first-frame))
         (puts (mapcar #'pb:op.put (remove 'pb:put (pb:frame.ops first-frame)
                                           :key #'pb:op.op-case :test-not #'eq))))
    (ok (= 1 (length styles)) "one distinct style, though used twice")
    (let ((style (first styles)))
      (ok (= 1 (pb:style.id style)))
      (ok (= #xAABBCC (pb:style.foreground style)))
      (ng (pb:style.has-background style) "an absent colour is unset, not 0")
      (ok (pb:style.bold style)))
    (ok (equal '(1 0) (mapcar #'pb:put.style puts)) "no style is 0")
    (let ((run (first (pb:modeline-painted.runs
                       (pb:op.modeline-painted (fourth (pb:frame.ops first-frame)))))))
      (ok (= 1 (pb:run.style run)) "modeline runs share the table"))
    (add-op session (make-text-put :view 1 :x 0 :y 1 :text "again" :width 5
                                   :style (make-style :foreground #xAABBCC :bold t)))
    (let ((second-frame (pb:to-display.frame
                         (to-display (codec:encode (finish-frame session nil) state 2)))))
      (ok (null (pb:frame.styles second-frame)) "already defined")
      (ok (= 1 (pb:put.style (pb:op.put (first (pb:frame.ops second-frame)))))))))

(deftest a-coloured-underline-keeps-its-colour
  (let* ((state (codec:make-codec-state))
         (session (make-session))
         (style (first (pb:frame.styles
                        (pb:to-display.frame
                         (to-display (codec:encode (sample-frame session
                                                                 :style (make-style :underline #xFF0000))
                                                   state 1)))))))
    (ok (pb:style.underline style))
    (ok (= #xFF0000 (pb:style.underline-color style)))))

(deftest the-other-messages-encode
  (let ((state (codec:make-codec-state)))
    (ok (equal "copied" (pb:set-clipboard.text
                         (pb:to-display.set-clipboard
                          (to-display (codec:encode (relay::make-set-clipboard "copied") state 1))))))
    (ok (eq 'pb:clipboard-request
            (pb:to-display.message-case
             (to-display (codec:encode (relay:make-clipboard-request) state 2)))))
    (let ((welcome (pb:to-display.welcome (to-display (codec:welcome 3 "session-x")))))
      (ok (= codec:+protocol-version+ (pb:welcome.protocol-version welcome)))
      (ok (equal "session-x" (pb:welcome.session-id welcome))))
    (ok (equal "why" (pb:exit.reason (pb:to-display.exit (to-display (codec:exit-message 4 "why"))))))))

;;; Decoding

(defun to-editor (&rest args)
  (proto:serialize-to-bytes (apply #'pb:make-to-editor args)))

(defun decoded-input (&rest args)
  (let ((request (codec:decode (apply #'to-editor args))))
    (ok (eq :input (first request)))
    (second request)))

(deftest keys-get-lems-names
  (flet ((name (key) (key-input-name (decoded-input :key key))))
    (ok (equal "a" (name (pb:make-key :text "a"))))
    (ok (equal "Return" (name (pb:make-key :named :named-key-enter))))
    (ok (equal "PageUp" (name (pb:make-key :named :named-key-page-up))))
    (ok (equal "F5" (name (pb:make-key :function 5))))
    (ok (equal "Space" (lem:key-sym (key-event (decoded-input :key (pb:make-key :text " ")))))
        "the relay's own rule names a space")))

(deftest modifiers-and-the-envelope-reach-the-input
  (let ((input (decoded-input :seq 9 :time-us 12345
                              :key (pb:make-key :text "x"
                                                :modifiers '(:modifier-ctrl :modifier-meta)))))
    (ok (key-input-ctrl input))
    (ok (key-input-meta input))
    (ng (key-input-shift input))
    (ok (= 9 (input-seq input)))
    (ok (= 12345 (input-time input)))))

(deftest mouse-events-decode
  (let ((press (decoded-input :mouse (pb:make-mouse :x 3 :y 4
                                                    :press (pb:make-press :button :button-left))))
        (move (decoded-input :mouse (pb:make-mouse :x 3 :y 4 :move (pb:make-move))))
        (drag (decoded-input :mouse (pb:make-mouse :x 3 :y 4
                                                   :move (pb:make-move :button :button-right))))
        (wheel (decoded-input :mouse (pb:make-mouse :x 3 :y 4 :wheel (pb:make-wheel :dy -1)))))
    (ok (eq :down (lem-relay/input::mouse-input-action press)))
    (ok (eq :left (lem-relay/input::mouse-input-button press)))
    (ok (null (lem-relay/input::mouse-input-button move)) "a plain move has no button")
    (ok (eq :right (lem-relay/input::mouse-input-button drag)) "a drag does")
    (ok (= -1 (lem-relay/input::mouse-input-wheel-y wheel)))))

(deftest a-clipboard-reply-may-have-no-text
  (let ((with (decoded-input :clipboard-reply (pb:make-clipboard-reply :reply-to 4 :text "hi")))
        (without (decoded-input :clipboard-reply (pb:make-clipboard-reply :reply-to 5))))
    (ok (equal "hi" (lem-relay/input::clipboard-reply-text with)))
    (ok (= 4 (lem-relay/input::clipboard-reply-reply-to with)))
    (ok (null (lem-relay/input::clipboard-reply-text without)))))

(deftest a-hello-decodes
  (ok (equal '(:hello 1 1 "s" 100 30 nil #x101010)
             (codec:decode (to-editor :seq 1 :hello (pb:make-hello :protocol-version 1
                                                                   :session-id "s"
                                                                   :width 100 :height 30
                                                                   :background #x101010))))))

;;; Serving

(defclass serve-relay (relay:relay) ())

(defun written (output)
  (mapcar #'to-display (undelimited (flexi-streams:get-output-stream-sequence output))))

(deftest serve-greets-then-starts-the-editor
  (let* ((relay (make-instance 'serve-relay))
         (input (flexi-streams:make-in-memory-input-stream
                 (delimited (to-editor :seq 1 :hello (pb:make-hello :protocol-version 1
                                                                    :session-id "s-1"
                                                                    :width 100 :height 30
                                                                    :background #x101010))
                            (to-editor :seq 2 :time-us 50 :key (pb:make-key :text "q")))))
         (output (flexi-streams:make-in-memory-output-stream))
         (editor nil))
    (ok (null (serve:serve relay
                           (lambda (initialize finalize)
                             (declare (ignore finalize))
                             (setf editor (bt2:make-thread
                                           (lambda () (funcall initialize) :started))))
                           :input input :output output))
        "returns when the display hangs up")
    (ok (eq :started (bt2:join-thread editor)) "after the Hello")
    (ok (= 100 (lem-if:display-width relay)))
    (ok (eql #x101010 (pack-color (lem-if:get-background-color relay))))
    (let ((welcome (first (written output))))
      (ok (equal "s-1" (pb:welcome.session-id (pb:to-display.welcome welcome))) "echoed")
      (ok (= 1 (pb:to-display.seq welcome)) "the relay's first message"))
    (ok (= 2 (relay:relay-input-seq relay)) "the key was delivered, as display message 2")
    (let ((event (lem:receive-event 0.5)))
      (ok (equal "q" (lem:key-sym event))))))

(deftest a-hello-without-a-version-is-refused-out-loud
  (let* ((relay (make-instance 'serve-relay))
         (output (flexi-streams:make-in-memory-output-stream)))
    (serve:serve relay
                 (lambda (initialize finalize)
                   (declare (ignore initialize finalize))
                   (bt2:make-thread (lambda ())))
                 :input (flexi-streams:make-in-memory-input-stream
                         (delimited (to-editor :seq 1 :hello (pb:make-hello :width 80 :height 24))))
                 :output output)
    (let ((answer (first (written output))))
      (ok (eq 'pb:exit (pb:to-display.message-case answer)))
      (ok (search "protocol_version" (pb:exit.reason (pb:to-display.exit answer)))))))

(defclass endless-input (trivial-gray-streams:fundamental-binary-input-stream) ()
  (:documentation "A display that stays connected and never says anything."))

(defmethod trivial-gray-streams:stream-read-byte ((stream endless-input))
  (loop (sleep 60)))

(deftest the-editor-exiting-says-exit-and-ends-serve
  (let* ((relay (make-instance 'serve-relay))
         (output (flexi-streams:make-in-memory-output-stream)))
    (ok (equal "report"
               (serve:serve relay
                            (lambda (initialize finalize)
                              (declare (ignore initialize))
                              (bt2:make-thread (lambda () (sleep 0.2) (funcall finalize "report"))))
                            :input (make-instance 'endless-input)
                            :output output)))
    (let ((last (car (last (written output)))))
      (ok (eq 'pb:exit (pb:to-display.message-case last)))
      (ok (equal "report" (pb:exit.reason (pb:to-display.exit last)))))))
