(defsystem "lem-ratatui"
  :description "Terminal frontend driving a Rust/Ratatui display process over the lem-server JSON-RPC protocol."
  :depends-on ("lem-server")
  :serial t
  :pathname "lisp/"
  :components ((:file "implementation")
               (:file "main")))
