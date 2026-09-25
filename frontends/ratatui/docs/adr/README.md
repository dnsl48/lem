# Architecture Decision Records

Decisions taken while building the Ratatui frontend, with the reasoning
that produced them. The point of keeping these is that the reasoning
usually outlives the decision: when one of these is revisited, the
argument to beat is written down rather than reconstructed.

Format is [Michael Nygard's][nygard]: **Context** (the forces in play),
**Decision** (what we chose), **Consequences** (what we now live with,
good and bad). Status is one of `Proposed`, `Accepted`, `Superseded
by NNNN`.

Supporting research lives in [`../protocol-notes.md`](../protocol-notes.md)
and [`../protobuf-spike.md`](../protobuf-spike.md); the plan that
implements 0009–0011 is [`../relay-plan.md`](../relay-plan.md).

[nygard]: https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions

| # | Title | Status |
|---|---|---|
| [0001](0001-reuse-lem-server-jsonrpc.md) | Reuse the `lem-server` JSON-RPC protocol | Superseded by 0009 |
| [0002](0002-ratatui-core-over-full-ratatui.md) | Depend on `ratatui-core`, not full Ratatui | Accepted |
| [0003](0003-keep-json-codec-for-now.md) | Keep the JSON codec until measurement says otherwise | Superseded by 0010 |
| [0004](0004-two-processes-not-embedded-lisp.md) | Two OS processes, not an embedded Lisp runtime | Accepted; who spawns whom superseded by 0008 |
| [0005](0005-stdio-as-the-default-transport.md) | stdio as the default transport | Accepted |
| [0006](0006-stay-on-lem-server-for-now.md) | Stay on `lem-server` for now (revisits 0001) | Superseded by 0009 |
| [0007](0007-one-binary-by-embedding-the-image.md) | One binary by embedding the Lisp image, not the Lisp runtime | Superseded by 0008 |
| [0008](0008-a-launcher-owns-the-processes.md) | A launcher owns the processes; the display only speaks the protocol | Accepted |
| [0009](0009-our-own-relay.md) | Our own relay, `lem-relay`, instead of `lem-server` | Accepted |
| [0010](0010-protobuf-for-schema-and-codec.md) | Protobuf for the schema and the codec | Accepted |
| [0011](0011-a-frame-oriented-protocol.md) | A frame-oriented protocol designed for a terminal | Accepted (principles; schema in phase 2) |

## Adding one

Copy the structure of an existing record, take the next number, and add a
row above. Records are immutable once accepted — to change a decision,
write a new record and mark the old one superseded.
