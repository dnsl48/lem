(defsystem "lem-ratatui"
  :description "Terminal frontend driving a Rust/Ratatui display process over the lem-server JSON-RPC protocol."
  :depends-on ("lem-server"
               "jsonrpc"
               "jsonrpc/transport/stdio"
               "babel"
               "yason")
  :serial t
  :pathname "lisp/"
  :components ((:file "implementation")
               (:file "transport")
               (:file "jsonrpc-stdio-fixes")
               (:file "main")))
