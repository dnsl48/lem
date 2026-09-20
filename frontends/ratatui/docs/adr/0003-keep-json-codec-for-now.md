# 0003. Keep the JSON codec until measurement says otherwise

**Status:** Accepted — September 2026

## Context

[0001](0001-reuse-lem-server-jsonrpc.md) separates schema, codec and
transport, and reuses all three. The codec is the one with a live
question attached: JSON encoding on the Lisp side has a track record of
being hot. Upstream has already attacked it twice — `9fd84616`
("reduce websocket serialization overhead": send view IDs rather than
14-field view objects, memoize attribute JSON, precompute
`object-width`) and `4c93eaf3` (more view-id-hash caching).

The tempting move is MessagePack. It does not survive inspection.

**The Common Lisp library is not there.** `cl-messagepack` is the main
implementation: ~31 stars, ~54 commits, a README still describing it as
"first draft encoder and decoder implemented", floats supported only on
SBCL, and ext-type encoding capped at the `#xC7` byte giving a 255-byte
limit on byte arrays. Against that, `yason` is mature and already a
transitive dependency. The swap trades a proven codec for a draft one.

**The payload is the wrong shape for it.** MessagePack's wins come from
numbers and structural overhead. This frame is dominated by strings — the
text content, a `"#RRGGBB"` pair in every attribute, method names. A
string costs about the same either way. Realistically ~25-30% off the
wire, and the wire was never the constraint: this is a local pipe and a
full redraw is on the order of 40KB.

**It would break every existing display half at once**, which is a
materially worse risk profile than anything that preserves the wire
format.

There are better levers, and they are better precisely because the
biggest is not a codec change at all. `attribute-to-hash` memoizes the
hash table but still serialises the full attribute into every single
`put` — roughly 120 of a ~200 byte message.

## Decision

Keep JSON and YASON. Do not adopt MessagePack.

Revisit only on measurement, and in this order:

1. **Intern attributes in the protocol.** Emit each distinct attribute
   once per frame with an id, then reference `"attr": 7`. Cuts a `put`
   to roughly 90 bytes — a >50% payload cut, dwarfing any codec change,
   and negotiable per client.
2. **Capability negotiation at `login`** to suppress pixel, font and
   html fields for terminal clients. Perhaps another 15%.
3. **Encode straight to octets.** `jsonrpc-stdio-patch.lisp` currently
   does `(babel:string-to-octets (with-output-to-string (s) (yason:encode ...)))`
   — building the whole frame as a Lisp string and then re-encoding it.
   Two full passes and two whole-payload allocations, for pure CPU
   savings and no protocol change at all.
4. **Swap the JSON encoder, not the format** — `com.inuoe.jzon` or
   `shasht`. Same wire format, so every existing display half keeps
   working. Avoid `jonathan` despite its reputation: it degrades badly on
   nested objects, which is exactly this payload's shape.

## Consequences

- The PoC carries no codec risk and needs no protocol negotiation to
  start: `serde_json` against the existing stream.
- The PoC should instrument bytes and encode time per frame, so this
  record can be revisited against numbers instead of estimates. That
  instrumentation is the deliverable that makes this decision reviewable.
- If a profile ever does demand more, levers 1 and 3 are available
  without breaking a single existing client, and both are larger wins
  than MessagePack would have been.
