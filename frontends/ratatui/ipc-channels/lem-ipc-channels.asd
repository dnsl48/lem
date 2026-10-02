(defsystem "lem-ipc-channels"
  :description "Channels between Lem and its peer processes: length-delimited
framing, numbering, replies matched to requests, a reader thread, and
connecting to a local socket. ADR 0019, frontends/ratatui/docs/adr/."
  :depends-on ("alexandria" "bordeaux-threads" "log4cl" (:require "sb-bsd-sockets"))
  :serial t
  :components ((:file "framing")
               (:file "replies")
               (:file "channel")
               (:file "main"))
  :in-order-to ((test-op (test-op "lem-ipc-channels/tests"))))

(defsystem "lem-ipc-channels/tests"
  :depends-on ("lem-ipc-channels" "rove" "flexi-streams")
  :pathname "tests/"
  :components ((:file "main"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
