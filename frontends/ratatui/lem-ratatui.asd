(defsystem "lem-ratatui"
  :description "Terminal frontend: Lem relayed by lem-relay to a Rust/Ratatui display process."
  :depends-on ("lem/core"
               ;; As lem-ncurses: the modes and extensions. lem-server used
               ;; to bring these in; nothing else here does.
               (:feature (:not :lem-minimal-build) "lem/extensions")
               "cl-ppcre"
               "lem-relay"
               "lem-relay/protobuf")
  :serial t
  :pathname "lisp/"
  :components ((:file "implementation")
               (:file "transport")
               (:file "links")
               (:file "main")))
