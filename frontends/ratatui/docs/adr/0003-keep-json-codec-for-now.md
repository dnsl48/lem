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

## Measurements — September 2026

Taken with the instrumentation added in Task 9, which is the condition
this record was accepted on.

**Release build, one realistic session** — open a file, type ~135
characters, move the cursor, shrink to 60x20 and grow back to 100x30,
save, quit:

```
960 frames, 3,006,287 B total, avg 3,131 B, max 17,234 B, avg decode 68us
```

**First frame alone**, 80x24, headless: 7,061 B, decoding in 124us
(release) or 414us (debug).

### What this says

**Decoding is not the bottleneck.** 68us per frame is 0.4% of a 16.7ms
frame budget; the whole session spent about 65ms decoding. A faster codec
would be optimising something that costs nothing. The decision to keep
JSON stands, and now stands on numbers.

**Frame *amplification* is the more interesting result.** An earlier
version of this section read 38 frames/sec out of the number above and
attributed it to cursor blink and a modeline clock. That was wrong — it
divided one mixed workload by its wall time. Measured directly:

```
idle, clean buffer     ~0.0 frames/sec   (Lem is genuinely quiet)
idle, modified buffer  ~3.2 frames/sec
typing                 ~13 frames per keystroke
```

So the frames follow activity, not idleness, and the cost is one
keystroke producing a dozen frames. `lem-if:render-line-on-modeline`
(`frontends/server/main.lisp:737`) is a large part of it: every frame it
emits a full-width blank `modeline-put` followed by every modeline
object, unconditionally and with no caching, which is exactly the
12-instruction frames seen when the editor is doing nothing of substance.

If anything here deserves attention before the codec it is that — and
unlike the levers listed below, it does not need `lem-server` to change:
`render-line-on-modeline` and `update-display` are generics, and a method
specialised on our own implementation class is strictly more specific
than `lem-server`'s. See `../poc-plan.md`.

### What was *not* measured

This instruments the **Rust decode** side only. The Lisp **encode** side
is untouched, and that is where the two upstream perf commits cited above
actually were. A conclusion about YASON's cost needs separate
instrumentation inside `lem-server`; nothing here licenses one.
