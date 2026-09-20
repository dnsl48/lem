(uiop:define-package :lem-ratatui
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui)
  (:import-from :lem-ratatui/transport
   :stdio-runner
   :call-with-protocol-streams)
  (:export :main))
(in-package :lem-ratatui)

(defun main (&optional (args (uiop:command-line-arguments)))
  "Run Lem with the Ratatui frontend, serving JSON-RPC over stdio.

Intended to be spawned as a child process by the Rust display binary,
which owns the terminal: this process's stdout carries protocol traffic,
not output for a human, so it must never be attached to a TTY.

`lem-server' internals are used here because the exported
`lem-server:run-stdio-server' hardcodes --interface JSONRPC and so cannot
select another implementation, and because the runner protocol it
dispatches on is internal. Teaching it an :interface argument and an
explicit output stream is the right fix and should be proposed upstream;
until then this duplicates its three lines."
  (call-with-protocol-streams
   (lambda ()
     (let ((lem-server::*server-runner* (make-instance 'stdio-runner)))
       (lem-server::init)
       (apply #'lem:lem (append args (list "--interface" "RATATUI")))))))
