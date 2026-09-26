(defsystem "lem-relay"
  :description "Lem's lem-if protocol relayed to a separate display process
as frames. Codec-independent; lem-relay/protobuf puts it on the wire.
ADR 0009, frontends/ratatui/docs/adr/."
  :depends-on ("lem/core")
  :serial t
  :components ((:file "frame")
               (:file "view")
               (:file "draw")
               (:file "relay")
               (:file "input"))
  :in-order-to ((test-op (test-op "lem-relay/tests"))))

(defsystem "lem-relay/protobuf"
  :description "lem-relay speaking lem.relay.v1, the protobuf protocol of
ADRs 0010-0014, defined in frontends/ratatui/proto. Needs `make
toolchain`: loading compiles the schema with protoc and cl-protobufs'
plugin, which the Makefile puts on PATH."
  :defsystem-depends-on ("cl-protobufs.asdf")
  :depends-on ("lem-relay" "cl-protobufs")
  :pathname "protobuf/"
  :serial t
  :components ((:protobuf-source-file "relay"
                :proto-pathname "../../proto/lem/relay/v1/relay.proto")
               (:file "framing")
               (:file "codec")
               (:file "serve")))

(defsystem "lem-relay/tests"
  :depends-on ("lem-relay" "lem-relay/protobuf" "rove" "flexi-streams")
  :pathname "tests/"
  :components ((:file "frame")
               (:file "draw")
               (:file "relay")
               (:file "input")
               (:file "protobuf")
               (:file "golden"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
