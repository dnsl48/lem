# 0011. A frame-oriented protocol designed for a terminal

**Status:** Accepted — September 2026. These are principles; the
concrete `relay.proto` is written in [phase 2](../relay-plan.md) and
reviewed against this record.

## Context

The protocol we speak today was designed for a browser, and we adopted
it whole ([0001](0001-reuse-lem-server-jsonrpc.md)). Running it produced
a list of costs, each measured or found by running the editor:

1. **Attributes are repeated in every `put`.** Roughly 120 of a ~200
   byte `put` is the attribute. The committed fixture has 80 `put`s but
   only 10 distinct attributes ([0003](0003-keep-json-codec-for-now.md)).
2. **44% of the fixture is browser-only:** `pixelX/Y/W/H`, `font`,
   `content`, `border`, `border_shape`, `type`
   ([0006](0006-stay-on-lem-server-for-now.md)).
3. **There is no frame message.** A frame is a `bulk` of loose
   notifications, and its end is recognised by finding
   `update-display` inside it ([protocol-notes](../protocol-notes.md) §14).
4. **The handshake has hidden requirements.** `login` must carry
   colours, or the editor emits nothing forever, and the client must
   send `redraw` after `login` or no frame arrives (§11).
5. **Most frames were empty.** 352 of 494 frames were exactly
   `clear-eob redraw-view-after move-cursor update-display`, and the
   modeline was re-sent in full every frame. Both are fixed today by
   overrides in `lisp/frame.lisp` and `lisp/modeline.lisp`, which is
   where the −76% frames and −61% bytes of 0003 came from.
6. **Colours are strings.** Every attribute carries `"#RRGGBB"` for
   foreground and background, parsed on every decode.
7. **Clipboard reads block on a round trip** built from two unrelated
   notifications (`get-clipboard-text`, then `got-clipboard-text`) and
   a queue.

We are breaking the wire anyway ([0010](0010-protobuf-for-schema-and-codec.md)),
so each of these can be fixed in the design instead of worked around.

## Decision

**Envelopes.** Two message streams on the existing stdio transport
([0005](0005-stdio-as-the-default-transport.md)): `ToDisplay` from the
editor and `ToEditor` from the display. Each is a `oneof` envelope, and
each message is framed with protobuf's standard length-delimited format:
a varint length, then the message. This is what `prost` reads with
`decode_length_delimited`. There is no JSON-RPC and no generic RPC
layer.

**Handshake.** The display sends `Hello`: protocol version, screen
size, the terminal's default colours if known, and capabilities. The
editor answers `Welcome` and then its first `Frame`, with no separate
`redraw`. On a version mismatch the editor sends `Exit` with a reason
instead of going quiet. There are no required fields whose absence
fails silently.

**The frame is the unit.** One `Frame` per `lem-if:update-display`,
covering everything since `will-update-display`, with a sequence
number. **A frame that would change nothing on screen is not sent.**
That makes what `frame.lisp` does today a guarantee of the protocol.
The modeline is sent only when it changed, which does the same for
`modeline.lisp`.

**Attributes are interned per session.** An `Attribute` has a `uint32`
id and is defined in the first frame that uses it. After that, ops
refer to it by id. Ids are stable for the session, and the display
keeps the table. Id `0` is the terminal default.

**Colours are packed `0xRRGGBB` integers,** optional, where absent
means the terminal default. They are never strings.

**Cells, never pixels.** There are no pixel, font, CSS, HTML or JS
fields, not even optional ones. Lem does all layout in cells
([protocol-notes](../protocol-notes.md) §3), and the ops say so:
views, rows, columns, text runs.

**Views and ops.** A view has a `uint32` id and a kind: tile, header,
or floating with its border. The ops cover the view lifecycle (create,
move, resize, delete) and painting (put a text run at a cell, clear to
end of line, clear to end of view, modeline content). The compositing
order is stated in the frame, so the display does not have to infer it.

**The cursor is frame state, not an op.** Each frame ends with where
the cursor is (view, x, y), its shape, and whether it is visible. The
relay derives it from the drawing objects, as `set-last-print-cursor`
does today.

**Correlated requests where the editor needs an answer.** Clipboard
paste is `ClipboardRequest { id }` answered by `ClipboardReply { id, text }`.
Copy is a one-way `SetClipboard`. Anything else that needs a reply
later follows the same pattern.

**Input** covers what `lem-server` accepts today
(`frontends/server/main.lisp:792`), typed:

- `Key`: name plus modifiers.
- `Abort`: a dedicated message, not a key. It becomes
  `lem:send-abort-event`, which interrupts a busy editor thread; a
  queued `C-g` would wait behind the very work it is meant to stop.
- `Paste`: text inserted through the major mode's paste, replacing
  `clipboard-paste` and `input-string`.
- `Mouse`: button down, button up, move, wheel.
- `Resize`.
- `ClipboardReply`.

Focus events and the like are added when something uses them.

**Evolution.** Package `lem.relay.v1`. Field numbers are never reused.
Removed fields are `reserved`. An incompatible change is `v2`, not an
edit to `v1`.

## Consequences

- The overrides in `frame.lisp` and `modeline.lisp` become the relay's
  normal behaviour, and the protocol states what they guaranteed only
  by convention.
- The display half gets simpler: no `bulk` scanning, no string colours,
  no fields to ignore, no handshake quirks. `lem-protocol` becomes
  generated types plus framing.
- We give up any possibility of the browser client speaking this
  protocol. That is intended: `lem-server` remains the browser's
  protocol, and this one is the terminal's.
- The relay now owns correctness properties the display relies on, the
  empty-frame rule and modeline memoisation in particular. Their
  invalidation rules (resize, view creation, `clear`) are specified
  with the schema and tested in Lisp, not just observed from the Rust
  side.

## Open questions for phase 2

- **Who owns character width?** Today Lisp decides widths
  (`lem-if:object-width`) and the display trusts `x`. A wide character
  whose width the two halves disagree on shifts the rest of the line.
  Either `Put` carries the cell width the relay assumed, or the relay
  pads so that disagreement cannot drift.
- **Icon glyphs.** `icon-object`s still arrive. Pass them through for
  Nerd Font terminals, substitute them, or make it a capability in
  `Hello`.
- **The mouse event shape.** Settle it against what crossterm reports
  and what `lem:receive-mouse-button-down` / `-up` and friends expect.
  The browser sends pixel coordinates alongside cells, and we will not.
