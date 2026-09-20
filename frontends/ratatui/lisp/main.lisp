(uiop:define-package :lem-ratatui
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui)
  (:export :main))
(in-package :lem-ratatui)

(defun main (&optional (args (uiop:command-line-arguments)))
  "Run Lem with the Ratatui frontend, serving JSON-RPC over stdio.

Intended to be spawned as a child process by the Rust display binary,
which owns the terminal: this process's stdout carries protocol traffic,
not output for a human, so it must never be attached to a TTY.

Because stdout is the wire, `*standard-output*' must be rebound away from
it before any editor code runs, or a stray format call corrupts the frame
stream. `lem-server' already muffles `*trace-output*' and `*error-output*'
for the websocket runner; the same is needed here and is not yet done.

`lem-server' internals are used here because the exported
`lem-server:run-stdio-server' hardcodes --interface JSONRPC and so cannot
select another implementation. Teaching it an :interface argument is the
right fix and should be proposed upstream; until then this duplicates its
three lines."
  (let ((lem-server::*server-runner*
          (make-instance 'lem-server::stdio-server-runner)))
    (lem-server::init)
    (apply #'lem:lem (append args (list "--interface" "RATATUI")))))
