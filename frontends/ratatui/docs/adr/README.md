# Architecture Decision Records

Decisions taken while building the Ratatui frontend, with the reasoning
that produced them. The point of keeping these is that the reasoning
usually outlives the decision: when one of these is revisited, the
argument to beat is written down rather than reconstructed.

Format is [Michael Nygard's][nygard]: **Context** (the forces in play),
**Decision** (what we chose), **Consequences** (what we now live with,
good and bad). Status is one of `Proposed`, `Accepted`, `Superseded
by NNNN`.

Supporting research lives in [`../protocol-notes.md`](../protocol-notes.md).

[nygard]: https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions

| # | Title | Status |
|---|---|---|
| [0001](0001-reuse-lem-server-jsonrpc.md) | Reuse the `lem-server` JSON-RPC protocol | Accepted |
| [0002](0002-ratatui-core-over-full-ratatui.md) | Depend on `ratatui-core`, not full Ratatui | Accepted |
| [0003](0003-keep-json-codec-for-now.md) | Keep the JSON codec until measurement says otherwise | Accepted |
| [0004](0004-two-processes-not-embedded-lisp.md) | Two OS processes, not an embedded Lisp runtime | Accepted |
| [0005](0005-stdio-as-the-default-transport.md) | stdio as the default transport | Accepted |
| [0006](0006-stay-on-lem-server-for-now.md) | Stay on `lem-server` for now (revisits 0001) | Accepted |

## Adding one

Copy the structure of an existing record, take the next number, and add a
row above. Records are immutable once accepted — to change a decision,
write a new record and mark the old one superseded.
