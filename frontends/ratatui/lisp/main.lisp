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

(defmethod lem-if:invoke ((ratatui ratatui) function)
  "Run the editor, relaying it over the protocol streams in lem.display.v1,
with the OS agent connected in the background when the launcher started
one (ADR 0019). A crash report the editor leaves also reaches the
display, in `Exit'; here it goes to the log, since stdout is the wire."
  (lem-os-agent:start)
  (alexandria:when-let ((report (lem-display-relay/protobuf/serve:serve ratatui function
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
     (apply #'lem:lem (append args (list "--interface" "RATATUI"))))))
