(defsystem "lem-display-relay"
  :description "Lem's lem-if protocol relayed to a separate display process
as frames. Codec-independent; lem-display-relay/protobuf puts it on the wire.
ADRs 0009 and 0019, frontends/ratatui/docs/adr/."
  :depends-on ("lem/core" "lem-ipc-channels")
  :serial t
  :components ((:file "frame")
               (:file "view")
               (:file "draw")
               (:file "relay")
               (:file "input"))
  :in-order-to ((test-op (test-op "lem-display-relay/tests"))))

(defsystem "lem-display-relay/protobuf"
  :description "lem-display-relay speaking lem.display.v1, the protobuf protocol of
ADRs 0010-0014, defined in frontends/ratatui/proto. Needs `make
toolchain`: loading compiles the schema with protoc and cl-protobufs'
plugin, which the Makefile puts on PATH."
  :defsystem-depends-on ("cl-protobufs.asdf")
  :depends-on ("lem-display-relay" "cl-protobufs")
  :pathname "protobuf/"
  :serial t
  :components ((:protobuf-source-file "display"
                :proto-pathname "../../proto/lem/display/v1/display.proto")
               (:file "codec")
               (:file "serve")))

(defsystem "lem-display-relay/tests"
  :depends-on ("lem-display-relay" "lem-display-relay/protobuf" "rove" "flexi-streams")
  :pathname "tests/"
  :components ((:file "frame")
               (:file "draw")
               (:file "relay")
               (:file "input")
               (:file "protobuf")
               (:file "golden"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
