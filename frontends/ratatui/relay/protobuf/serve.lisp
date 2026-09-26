(defpackage :lem-relay/protobuf/serve
  (:use :cl)
  (:local-nicknames (:framing :lem-relay/protobuf/framing)
                    (:codec :lem-relay/protobuf/codec)
                    (:relay :lem-relay/relay)
                    (:input :lem-relay/input))
  (:export :serve))
(in-package :lem-relay/protobuf/serve)

(defun send-numbered (output lock state encode)
  "Number the next outgoing message, write what ENCODE makes of that
number, and return it. Both under LOCK, so numbers follow wire order
whichever thread sends (ADR 0013)."
  (bt2:with-lock-held (lock)
    (let ((seq (codec:next-seq state)))
      (framing:write-delimited output (funcall encode seq))
      seq)))

(defun unpack-color (rgb)
  (and rgb (lem:make-color (ldb (byte 8 16) rgb) (ldb (byte 8 8) rgb) (ldb (byte 8 0) rgb))))

(defun hello (relay output lock state ready version session-id width height foreground background)
  "Take the display's `Hello': its size and colours, then `Welcome', then
let the editor start (see `serve'). Return NIL when the display cannot
be served, having told it why with `Exit'."
  (when (zerop version)
    (send-numbered output lock state
                   (lambda (seq)
                     (codec:exit-message seq "Hello carried no protocol_version.")))
    (return-from hello nil))
  (log:info "lem-relay/protobuf: session ~A, display protocol revision ~D"
            session-id version)
  (when (and (plusp width) (plusp height))
    (relay:set-display-size relay width height))
  ;; The terminal's own colours: what Lem judges light or dark mode by
  ;; until a theme sets its own.
  (alexandria:when-let ((color (unpack-color foreground)))
    (setf (relay:relay-foreground relay) color))
  (alexandria:when-let ((color (unpack-color background)))
    (setf (relay:relay-background relay) color))
  (send-numbered output lock state (lambda (seq) (codec:welcome seq session-id)))
  (bt2:signal-semaphore ready)
  t)

(defun serve (relay function &key input output)
  "Run Lem with RELAY, speaking lem.relay.v1 (relay.proto) on the octet
streams INPUT and OUTPUT, until the editor exits or the display hangs up.
The body of an implementation's `lem-if:invoke'; FUNCTION is what Lem
hands that. Returns the editor's crash report, if it left one.

As ncurses does (frontends/ncurses/mainloop.lisp): this thread reads,
the editor runs on its own thread, and the editor's finalize callback
ends the loop. The editor waits for `Hello', which carries the screen's
size, and sends its first frame unasked: there is no `redraw' step
(ADR 0011). When the editor exits, the display gets `Exit' before the
stream ends. Writes from both threads are serialised and numbered."
  (let ((lock (bt2:make-lock :name "lem-relay/protobuf output"))
        (ready (bt2:make-semaphore :name "lem-relay/protobuf hello"))
        (state (codec:make-codec-state))
        (reader (bt2:current-thread)))
    (setf (relay:relay-sink relay)
          (lambda (message)
            (send-numbered output lock state
                           (lambda (seq) (codec:encode message state seq)))))
    (catch 'editor-exited
      (setf (relay:relay-editor-thread relay)
            (funcall function
                     (lambda () (bt2:wait-on-semaphore ready))
                     (lambda (report)
                       (ignore-errors
                        (send-numbered output lock state
                                       (lambda (seq)
                                         (codec:exit-message seq (or report "")))))
                       (bt2:interrupt-thread reader
                                             (lambda () (throw 'editor-exited report))))))
      (loop :for body := (framing:read-delimited input)
            :while body
            :do (handler-case
                    (let ((request (codec:decode body)))
                      (ecase (first request)
                        (:hello
                         (unless (apply #'hello relay output lock state ready (cddr request))
                           (return)))
                        (:input (input:deliver relay (second request)))
                        (:ignored
                         (log:info "lem-relay/protobuf: ignoring a message this revision does not know"))))
                  (error (condition)
                    (log:error "lem-relay/protobuf: failed to handle a message: ~A" condition))))
      nil)))
