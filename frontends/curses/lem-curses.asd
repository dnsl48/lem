(defsystem "lem-curses"
  :description "Curses terminal frontend: Lem relayed by lem-display-relay to a Rust display process."
  :depends-on ("lem/core"
               ;; As lem-ncurses: the modes and extensions. lem-server used
               ;; to bring these in; nothing else here does.
               (:feature (:not :lem-minimal-build) "lem/extensions")
               "cl-ppcre"
               "lem-display-relay"
               "lem-display-relay/protobuf"
               "lem-os-agent")
  :serial t
  :pathname "lisp/"
  :components ((:file "implementation")
               (:file "clipboard")
               (:file "transport")
               (:file "links")
               (:file "main"))
  :in-order-to ((test-op (test-op "lem-curses/tests"))))

(defsystem "lem-curses/tests"
  :depends-on ("lem-curses" "rove")
  :pathname "lisp/tests/"
  :components ((:file "main"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
