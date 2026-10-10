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
    (force-output *log-stream*)
    report))

(defun main (&optional (args (uiop:command-line-arguments)))
  "Run Lem with the Ratatui frontend, the protocol on stdin and stdout.

Started by the launcher, which connects those to the Rust display
process: this process's stdout carries the protocol, not output for a
human, so it must never be attached to a TTY."
  ;; CLI output and errors precede protocol setup. Parsing alone has no
  ;; startup side effects, and malformed arguments must never start a
  ;; session or enter an interactive debugger.
  (multiple-value-bind (parsed parse-error) (ignore-errors (lem:parse-args args))
    (when parse-error
      (format *error-output* "lem-ratatui: ~A~%" parse-error)
      (finish-output *error-output*)
      (uiop:quit 2))
    (when (and parsed
               (or (lem:command-line-arguments-help parsed)
                   (lem:command-line-arguments-version parsed)))
      ;; The launcher sets this marker for protocol sessions and clears
      ;; inherited markers for direct CLI calls. If its parser misses an
      ;; informational option, keep human output out of the protocol pipe.
      (when (uiop:getenv "LEM_SESSION_ID")
        (format *error-output*
                "lem-ratatui: LEM_SESSION_ID is set, so this is treated as a protocol session; ~
                 run the lem-ratatui launcher for --help or --version~%")
        (finish-output *error-output*)
        (uiop:quit 2))
      (return-from main (apply #'lem:lem args)))
    ;; A launcher/parser disagreement must not write protobuf to the
    ;; user's terminal. Inspect the actual wire descriptor, not a Lisp
    ;; stream which may have been rebound.
    (unless (zerop (sb-unix:unix-isatty 1))
      (format *error-output* "lem-ratatui: protocol stdout must not be a TTY~%")
      (finish-output *error-output*)
      (uiop:quit 2))
    (let ((report (call-with-protocol-streams
                   (lambda ()
                     (apply #'lem:lem (append args (list "--interface" "RATATUI")))))))
      (when (and report (plusp (length report)))
        (uiop:quit 1)))))
