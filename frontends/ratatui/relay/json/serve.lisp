(defpackage :lem-relay/json/serve
  (:use :cl)
  (:local-nicknames (:framing :lem-relay/json/framing)
                    (:codec :lem-relay/json/codec)
                    (:relay :lem-relay/relay)
                    (:input :lem-relay/input))
  (:export :serve))
(in-package :lem-relay/json/serve)

(defun write-bodies (output lock bodies)
  (bt2:with-lock-held (lock)
    (dolist (body bodies)
      (framing:write-message output body))))

(defun login (relay output lock ready id width height foreground background)
  "Answer the display's `login': take its size and colours, answer it, and
let the editor thread start (it waits for this; see `serve')."
  (when (and width height)
    (relay:set-display-size relay width height))
  ;; The terminal's own colours: what Lem judges light or dark mode by
  ;; until a theme sets its own.
  (alexandria:when-let ((color (and foreground (lem:parse-color foreground))))
    (setf (relay:relay-foreground relay) color))
  (alexandria:when-let ((color (and background (lem:parse-color background))))
    (setf (relay:relay-background relay) color))
  (write-bodies output lock
                (list (codec:response
                       id
                       (let ((result (make-hash-table :test 'equal))
                             (size (make-hash-table :test 'equal)))
                         (setf (gethash "width" size) (relay:relay-display-width relay)
                               (gethash "height" size) (relay:relay-display-height relay)
                               (gethash "views" result) (vector)
                               (gethash "foreground" result) foreground
                               (gethash "background" result) background
                               (gethash "size" result) size)
                         result))))
  (bt2:signal-semaphore ready))

(defun handle (relay output lock ready state message)
  (let ((request (codec:decode message state)))
    (ecase (first request)
      (:login
       (destructuring-bind (id width height foreground background) (rest request)
         (login relay output lock ready id width height foreground background)))
      (:redraw
       ;; lem-server's `redraw' resizes and forces a full repaint; a
       ;; :resize does both (`lem:update-on-display-resized').
       (destructuring-bind (width height) (rest request)
         (input:deliver relay (input:make-resize-input
                               (or width (relay:relay-display-width relay))
                               (or height (relay:relay-display-height relay))))))
      (:input
       (dolist (input (rest request))
         (input:deliver relay input)))
      (:ignored
       (log:info "lem-relay/json: ignoring ~S" (second request))))))

(defun serve (relay function &key input output)
  "Run Lem with RELAY, speaking today's JSON protocol on the octet streams
INPUT and OUTPUT, until the editor exits or the display hangs up. The
body of the implementation's `lem-if:invoke'; FUNCTION is what Lem hands
that. Returns the editor's crash report, if it left one.

As ncurses does (frontends/ncurses/mainloop.lisp), this thread reads
input and the editor runs on its own thread, and the editor's finalize
callback ends the loop here. The editor thread waits for the display's
`login' before starting, as lem-server's does: until then Lem does not
know the screen's size.

Writes come from both threads (frames from the editor's, the login
answer from this one), so they are serialised."
  (let ((lock (bt2:make-lock :name "lem-relay/json output"))
        (ready (bt2:make-semaphore :name "lem-relay/json login"))
        (state (codec:make-codec-state))
        (reader (bt2:current-thread)))
    (setf (relay:relay-sink relay)
          (lambda (message)
            (write-bodies output lock (codec:encode message state))))
    (catch 'editor-exited
      (setf (relay:relay-editor-thread relay)
            (funcall function
                     (lambda () (bt2:wait-on-semaphore ready))
                     (lambda (report)
                       (bt2:interrupt-thread reader
                                             (lambda () (throw 'editor-exited report))))))
      (loop :for body := (framing:read-message input)
            :while body
            :do (handler-case (handle relay output lock ready state (codec:parse body))
                  (error (condition)
                    (log:error "lem-relay/json: failed to handle a message: ~A" condition))))
      nil)))
