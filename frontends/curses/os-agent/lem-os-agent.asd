(defsystem "lem-os-agent"
  :description "Lem's side of the link to the OS agent (ADR 0019): lem.os.v1
over a local socket, and the API Lem, extensions and init files use. Needs
`make toolchain', as lem-display-relay/protobuf does: loading compiles the
schema with protoc and cl-protobufs' plugin."
  :defsystem-depends-on ("cl-protobufs.asdf")
  :depends-on ("lem-ipc-channels" "cl-protobufs" "alexandria" "bordeaux-threads" "log4cl")
  :serial t
  :components ((:protobuf-source-file "os"
                :proto-pathname "../proto/lem/os/v1/os.proto")
               (:file "codec")
               (:file "main"))
  :in-order-to ((test-op (test-op "lem-os-agent/tests"))))

(defsystem "lem-os-agent/tests"
  :depends-on ("lem-os-agent" "rove")
  :pathname "tests/"
  :components ((:file "main"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
