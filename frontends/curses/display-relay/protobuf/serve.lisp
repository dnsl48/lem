(defpackage :lem-display-relay/protobuf/serve
  (:use :cl)
  (:local-nicknames (:ipc :lem-ipc-channels)
                    (:codec :lem-display-relay/protobuf/codec)
                    (:relay :lem-display-relay/relay)
                    (:input :lem-display-relay/input))
  (:export :serve))
(in-package :lem-display-relay/protobuf/serve)

(defun unpack-color (rgb)
  (and rgb (lem:make-color (ldb (byte 8 16) rgb) (ldb (byte 8 8) rgb) (ldb (byte 8 0) rgb))))

(defun hello (relay channel ready version session-id width height foreground background
              capabilities)
  "Take the display's `Hello': its size, colours and capabilities, then `Welcome', then
let the editor start (see `serve'). Return NIL when the display cannot
be served, having told it why with `Exit'."
  (when (zerop version)
    (ipc:channel-send channel
                      (lambda (seq)
                        (codec:exit-message seq "Hello carried no protocol_version.")))
    (return-from hello nil))
  (log:info "lem-display-relay/protobuf: session ~A, display protocol revision ~D, terminal colours ~A on ~A"
            session-id version
            (if foreground (format nil "#~6,'0X" foreground) "unknown")
            (if background (format nil "#~6,'0X" background) "unknown"))
  (when (and (plusp width) (plusp height))
    (relay:set-display-size relay width height))
  ;; A reader-only slot: the codec sets handshake state before the editor
  ;; starts. User configuration observes an immutable value after this.
  (reinitialize-instance relay :terminal-capabilities capabilities)
  ;; The terminal's own colours: what Lem judges light or dark mode by
  ;; until a theme sets its own.
  (alexandria:when-let ((color (unpack-color foreground)))
    (setf (relay:relay-foreground relay) color))
  (alexandria:when-let ((color (unpack-color background)))
    (setf (relay:relay-background relay) color))
  (ipc:channel-send channel (lambda (seq) (codec:welcome seq session-id)))
  (bt2:signal-semaphore ready)
  t)

(defun serve (relay function &key input output)
  "Run Lem with RELAY, speaking lem.display.v1 (display.proto) on the octet
streams INPUT and OUTPUT, until the editor exits or the display hangs up.
The body of an implementation's `lem-if:invoke'; FUNCTION is what Lem
hands that. Returns the editor's crash report, if it left one.

As ncurses does (frontends/ncurses/mainloop.lisp): this thread reads,
the editor runs on its own thread, and the editor's finalize callback
ends the loop. The editor waits for `Hello', which carries the screen's
size, and sends its first frame unasked: there is no `redraw' step
(ADR 0011). When the editor exits, the display gets `Exit' before the
stream ends. Writes from both threads go through one channel, which
serialises and numbers them (ADR 0019)."
  (let ((channel (ipc:make-channel input output))
        (ready (bt2:make-semaphore :name "lem-display-relay/protobuf hello"))
        (state (codec:make-codec-state))
        (reader (bt2:current-thread)))
    (setf (relay:relay-sink relay)
          (lambda (message)
            (ipc:channel-send channel (lambda (seq) (codec:encode message state seq)))))
    (catch 'editor-exited
      (setf (relay:relay-editor-thread relay)
            (funcall function
                     (lambda () (bt2:wait-on-semaphore ready))
                     (lambda (report)
                       (ignore-errors
                        (ipc:channel-send channel
                                          (lambda (seq)
                                            (codec:exit-message seq (or report "")))))
                       (bt2:interrupt-thread reader
                                             (lambda () (throw 'editor-exited report))))))
      (loop :for body := (ipc:channel-receive channel)
            :while body
            :do (handler-case
                    (let ((request (codec:decode body)))
                      (ecase (first request)
                        (:hello
                         (unless (apply #'hello relay channel ready (cddr request))
                           (return)))
                        (:input (input:deliver relay (second request)))
                        (:ignored
                         (log:info "lem-display-relay/protobuf: ignoring a message this revision does not know"))))
                  (error (condition)
                    (log:error "lem-display-relay/protobuf: failed to handle a message: ~A" condition))))
      nil)))
