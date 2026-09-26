(uiop:define-package :lem-ratatui
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui)
  (:import-from :lem-ratatui/transport
   :*protocol-input*
   :*protocol-output*
   :*log-stream*
   :call-with-protocol-streams)
  (:export :main))
(in-package :lem-ratatui)

(defun keep-frame-multiplexer-off ()
  "Undo the frame multiplexer's own after-init hook.

Switching virtual frames leaves the other frame's views alive, and until
the display composites by `views-stacked' (ADR 0012, phase 2 of
relay-plan.md) it paints every live view in creation order, so switching
back would show the wrong frame. lem-server turned it off for its own
reasons.

Its enabling function is internal; the command that toggles it is not.
Run with weight -1, this comes after that hook (weight 0), so it always
finds the multiplexer freshly switched on."
  (lem/frame-multiplexer:toggle-frame-multiplexer))

(defmethod lem-if:invoke ((ratatui ratatui) function)
  "Run the editor, relaying it over the protocol streams in today's JSON
protocol. A crash report the editor leaves goes to the log: stdout is
the wire."
  (alexandria:when-let ((report (lem-relay/json/serve:serve ratatui function
                                                            :input *protocol-input*
                                                            :output *protocol-output*)))
    (format *log-stream* "~&~A~%" report)
    (force-output *log-stream*)))

(defun main (&optional (args (uiop:command-line-arguments)))
  "Run Lem with the Ratatui frontend, the protocol on stdin and stdout.

Started by the launcher, which connects those to the Rust display
process: this process's stdout carries the protocol, not output for a
human, so it must never be attached to a TTY."
  (call-with-protocol-streams
   (lambda ()
     (lem:add-hook lem:*after-init-hook* 'keep-frame-multiplexer-off -1)
     (apply #'lem:lem (append args (list "--interface" "RATATUI"))))))
