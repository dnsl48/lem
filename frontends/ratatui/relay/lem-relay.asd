(defsystem "lem-relay"
  :description "Lem's lem-if protocol relayed to a separate display process
as frames. Codec-independent: see lem-relay/json and, later,
lem-relay/protobuf. ADR 0009, frontends/ratatui/docs/adr/."
  :depends-on ("lem/core")
  :serial t
  :components ((:file "frame")
               (:file "view")
               (:file "draw")
               (:file "relay"))
  :in-order-to ((test-op (test-op "lem-relay/tests"))))

(defsystem "lem-relay/tests"
  :depends-on ("lem-relay" "rove")
  :pathname "tests/"
  :components ((:file "frame")
               (:file "draw")
               (:file "relay"))
  :perform (test-op (o c) (symbol-call :rove '#:run c)))
