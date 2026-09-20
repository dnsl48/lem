# 0001. Reuse the `lem-server` JSON-RPC protocol

**Status:** Accepted — September 2026

## Context

A terminal display half needs some way to talk to Lem. Three layers are
separable, and conflating them is the usual source of bad arguments here:

| Layer | Today |
|---|---|
| **Schema** — method names and argument shapes | 26 notifications out, 5 methods in |
| **Codec** — objects to bytes | JSON, via YASON |
| **Transport** — bytes to the other process | stdio (`Content-Length` framing), websocket, unix socket |

`frontends/server/` already implements all three, and the schema is
already the schema we want: cell-coordinate, attribute-tagged, batched
per frame. That is not a coincidence — the browser display half is not
doing layout either, because Lem resolves layout before emitting
anything (see [protocol-notes](../protocol-notes.md) sections 3 and 4).

The alternatives considered were a purpose-built protocol and gRPC.

**gRPC** fails on two independent grounds. There is no mature gRPC server
for Common Lisp: `cl-protobufs` covers encoding, but gRPC is protobuf
*over HTTP/2*, so adopting it means implementing HTTP/2 framing, flow
control and stream multiplexing in Lisp, or running a sidecar. And it
would buy nothing structural — gRPC exists for typed request/response
across many services and languages with codegen and discovery, whereas
this is 26 fire-and-forget notifications flowing one way between exactly
two processes on one machine. The one thing worth wanting, compact fast
encoding, is available without HTTP/2 or a codegen step.

**A purpose-built protocol** would mean re-deriving the same 26 methods
and then maintaining a fork forever, with every upstream protocol change
needing to be mirrored by hand.

## Decision

Reuse all three layers unchanged. Connect over **stdio**
(`lem-server --mode=stdio`) for the PoC; the unix-socket transport is
there if stdio proves awkward.

Do not fork the schema. Protocol changes we want are additive and
negotiated, never divergent.

## Consequences

- No protocol fork, and the browser and webview display halves keep
  working untouched. Upstream protocol improvements are inherited.
- The schema carries browser-shaped fields a terminal ignores —
  `pixelX`/`pixelY`/`pixelWidth`/`pixelHeight`, `font`, `load-css`,
  `js-eval`. They are genuinely optional, so ignoring them is free;
  suppressing them server-side is a later optimisation via capability
  negotiation at `login`.
- It is **not** zero Lisp changes, as first assumed. `lem-server`
  declares `:no-force-needed t`, which suppresses the force-redraw of
  windows underneath a dismissed floating window. A terminal has one
  grid and no occlusion repair, so it needs an implementation subclass
  with terminal-appropriate capabilities (`:no-force-needed nil`,
  `:support-pixel-positioning nil`). That subclass is the entire content
  of the `lisp/` half — on the order of tens of lines.
- The codec decision is deferred rather than settled; see
  [0003](0003-keep-json-codec-for-now.md).
