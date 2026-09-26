(defsystem "lem-relay"
  :description "Lem's lem-if protocol relayed to a separate display process
as frames. Codec-independent: see lem-relay/json and, later,
lem-relay/protobuf. ADR 0009, frontends/ratatui/docs/adr/."
  :depends-on ("lem/core")
  :serial t
  :components ((:file "frame")
               (:file "view")
               (:file "draw")
               (:file "relay")
               (:file "input"))
  :in-order-to ((test-op (test-op "lem-relay/tests"))))

(defsystem "lem-relay/json"
  :description "lem-relay speaking today's wire: JSON-RPC 2.0 notifications
with Content-Length framing, as lem-server writes it and the display's
lem-protocol crate reads it. Transitional (relay-plan.md, phase 1)."
  :depends-on ("lem-relay" "yason" "babel")
  :pathname "json/"
  :serial t
  :components ((:file "framing")
               (:file "codec")
               (:file "serve")))

(defsystem "lem-relay/tests"
  :depends-on ("lem-relay" "lem-relay/json" "rove" "flexi-streams")
  :pathname "tests/"
  :components ((:file "frame")
               (:file "draw")
               (:file "relay")
               (:file "input")
               (:file "json"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
