# Plan: from `lem-server` to `lem-relay`

This implements [0009](adr/0009-our-own-relay.md) (our own relay),
[0010](adr/0010-protobuf-for-schema-and-codec.md) (protobuf) and
[0011](adr/0011-a-frame-oriented-protocol.md) (the protocol redesign)
in two phases. Each phase ends with a working editor.

The split exists so that a regression always has one suspect. Phase 1
changes who produces the frames and leaves the bytes the same. Phase 2
changes the bytes and leaves the producer the same. Doing both at once
would leave any breakage ambiguous.

**Phase 0, done:** the ADRs above, the
[protobuf spike](protobuf-spike.md), and a baseline: 9/9 acceptance,
`cargo test` green, and the workload scripted (`scripts/workload.py`).

## Invariants for every step

- `python3 frontends/ratatui/scripts/acceptance.py` passes 9/9.
- `C-x C-c` exits 0 with the terminal restored.
- No new `lem-server::`, `lem-core::` or `lem::` references (the
  `internal_symbol_rule` in `contract.yml`).
- Nothing outside `frontends/ratatui/` changes: not `src/`,
  `extensions/`, `qlfile` or `lem.asd`. If a step seems to need
  a core change, stop and write that down. [0009](adr/0009-our-own-relay.md)
  says none is required, so needing one means something was missed.

## Layout

```
frontends/ratatui/
├── proto/lem/relay/v1/relay.proto    phase 2: the one schema
├── toolchain/                        phase 2, gitignored: cl-protobufs, protoc, plugin
├── relay/
│   ├── lem-relay.asd                 lem-relay, lem-relay/json, lem-relay/protobuf
│   ├── relay.lisp                    the relay mixin: capability-free lem-if methods
│   ├── draw.lisp                     draw-object, ported from lem-server
│   ├── frame.lisp                    frame model, attribute table, suppression
│   ├── input.lisp                    input model → lem:send-event
│   ├── json/                         phase 1 codec + Content-Length framing
│   └── protobuf/                     phase 2 codec + length-delimited framing
├── lisp/                             lem-ratatui: the ratatui class and main
└── rust/crates/lem-protocol/         phase 2: generated types + framing
```

## Phase 1 — `lem-relay` speaking today's JSON

The goal is to replace `lem-server` while the Rust side stays exactly
as it is.

**The one real design choice:** the relay's internal frame model is
**shaped by [0011](adr/0011-a-frame-oriented-protocol.md) from day one**:

- frames as a unit;
- styles as normalised values;
- default colours, cursor shape and stacking as state;
- the cursor as frame state;
- `Hello`/`Welcome` semantics.

`lem-relay/json` expands that model back into today's messages: it
turns styles into attribute objects, emits `bulk` with a trailing
`update-display`, and answers `login` then waits for `redraw`. Model
state with a `lem-server` message maps onto it: the cursor becomes
`move-cursor` and `update-cursor-shape`, and the default colours
`update-foreground` / `update-background`. Today's display ignores
those, which is harmless. `views-stacked` has no equivalent and is not
emitted. The display-side work of
[0012](adr/0012-the-relay-frame-model.md) (hardware cursor, default
colours, fitting runs to `width`, explicit stacking) lands with the
display rewrite in phase 2, so in phase 1 the Rust side stays
untouched. Phase 1 therefore also
proves the model against the acceptance script, and phase 2 becomes a
codec swap rather than a second redesign. Porting `lem-server` verbatim
first would be less work now, but we would pay for it again in phase 2.

1. **Scaffold `relay/`.** Create `lem-relay.asd` with `lem-relay` and
   `lem-relay/json`, packages per `defpackage_rule`, and an empty relay
   mixin. Make `lem-ratatui` depend on it alongside `lem-server` for
   now. It should build and change nothing.
2. **Frame model (`frame.lisp`),** per [0012](adr/0012-the-relay-frame-model.md):
   inline styles, ops, the cursor and default colours as frame state,
   stacking, and suppression in the session. Pin each rule with a Rove
   test.
3. **Port the drawing (`draw.lisp`).** Bring across `draw-object` for
   every drawing-object class in `src/display/physical-line.lisp`,
   including `set-last-print-cursor`. Add a catch-all method that logs
   an unknown class and skips it. Port it, don't rewrite it
   ([0009](adr/0009-our-own-relay.md)).
4. **The `lem-if` methods (`relay.lisp`).** Implement views, render
   line, modeline, clear, update display, colours, cursor shape and
   clipboard, all building into the frame model. Absorb what
   `lisp/frame.lisp` (empty-frame suppression, with the `clear-eob`
   rule) and `lisp/modeline.lisp` (memoisation and its invalidation)
   do today. Use `lem:update-on-display-resized` on resize, not
   `lem-core::adjust-all-window-size`.
5. **Input (`input.lisp`).** Convert key, abort, paste, mouse, resize
   and clipboard reply into `lem:send-event` / `lem:send-abort-event`,
   ported from `input-callback` (`frontends/server/main.lisp:792`).
   Abort must interrupt the editor thread, not enqueue.
6. **`lem-relay/json`.** Write `Content-Length` framing over stdio in
   bytes, not characters ([protocol-notes](protocol-notes.md) §13).
   Encode the frame model into today's methods. Implement the
   `login`/`redraw` handshake as the display half performs it now. Do
   not use the `jsonrpc` library.
7. **Switch over.** Make `ratatui` inherit the relay mixin and
   `lem-core:implementation` instead of `lem-server:jsonrpc`. Delete
   `lisp/transport.lisp`, `lisp/jsonrpc-stdio-fixes.lisp`,
   `lisp/frame.lisp` and `lisp/modeline.lisp`. Drop `lem-server`,
   `jsonrpc` and `jsonrpc/transport/stdio` from `lem-ratatui.asd`, and
   `frontends/server` from the Makefile's `LISP_SOURCES`.

**Done when:**

- the invariants hold;
- `cargo test` passes against the existing `frame.jsonl` fixture;
- `C-x C-b` still works (the `86106f75` regression);
- `grep -r "lem-server" frontends/ratatui/lisp frontends/ratatui/relay`
  is empty;
- `python3 frontends/ratatui/scripts/workload.py 5` is no worse than
  the baseline beyond run-to-run spread. The baseline was measured at
  `9f1a483c` over 5 runs: median 186 frames / 315,475 B (range 176–189
  frames, 303,729–316,941 B). This is 0003's workload (open a file,
  type 40 characters, save, quit), now scripted. 0003's own 130 frames /
  639,265 B came from a manual run before the launcher refactor, so it
  is not comparable.

**Phase 1 is done** (September 2026). The frontend runs on `lem-relay`,
with no `lem-server`, no `jsonrpc` in our code, and no Lem internals.

- Acceptance passes 9/9 and `cargo test` passes, including
  `relay_fixture.rs`, which decodes the relay's real output
  (`scripts/capture-relay-json.lisp`) with the display's own types.
- The workload moved from 186 frames / 315,475 B to 190 / 265,007 B
  (medians of five). Bytes fell 16%. The extra frames are cursor-only:
  with per-view cursor memory the median is 177. Phase 2's hardware
  cursor needs them, so the single-cursor rule of ADR 0012 stays.
- Where step 7 differed from the list above:
  - `lisp/transport.lisp` was trimmed, not deleted: the stream hygiene
    (stdout kept for the wire, the debugger off, the backtrace watchdog)
    belongs to the process, not to the `lem-server` runner it also held.
    Its streams are now octet streams.
  - `lem-ratatui` now depends on `lem/extensions` itself. `lem-server` had
    been bringing in every mode and extension.
  - `lem-server`'s start-up hook also switched the frame multiplexer off.
    That is kept, through the exported `toggle-frame-multiplexer`,
    because today's display would composite a stale virtual frame over
    the live one.

## Phase 2 — `lem-relay/protobuf`

The goal is the redesigned protocol, generated on both sides, with JSON
gone.

1. **Toolchain.** Add a `make toolchain` target that fills the
   gitignored `toolchain/` ([0010](adr/0010-protobuf-for-schema-and-codec.md)):
   - clone `cl-protobufs` at a pinned commit into `toolchain/cl-protobufs`,
     where qlot's project searcher finds it;
   - build protobuf v36.2 under `toolchain/.build/`, and the plugin
     in-source in a copy there ([spike](protobuf-spike.md));
   - install both into `toolchain/bin`;
   - honour `PROTOC_PREFIX`.

   Make `toolchain` a prerequisite of the Lisp build, and have the build
   prepend `toolchain/bin` to `PATH`. `make clean` keeps `toolchain/`;
   a new `make distclean` removes it. Document the prerequisites in the
   README: a C++17 compiler, CMake, zlib. **No change to the root
   `qlfile`.**

   *Done.* 2m38s from scratch on 12 cores, a no-op after. `toolchain/bin/protoc`
   is a wrapper script rather than a symlink: make judges a symlink by
   its target's mtime, which predates the stamp, and protoc finds its
   bundled `.proto` imports relative to its real location. The protobuf
   install lives under `.build/`, out of qlot's searcher's way.
   `PROTOC_PREFIX` was tried against an existing install: 7.5s, plugin only.
2. **Write `relay.proto`** to [0011](adr/0011-a-frame-oriented-protocol.md).
   Its open questions are answered: character width by
   [0012](adr/0012-the-relay-frame-model.md), icon glyphs and the mouse
   by [0013](adr/0013-icons-and-mouse-input.md), which also moves click
   counting into the relay (done, with `*click-interval*`).

   *Done:* `proto/lem/relay/v1/relay.proto`, reviewed, with
   [0014](adr/0014-the-display-describes-keys.md) moving key naming into
   the relay. `protoc`, `protox`/`prost` and cl-protobufs all compile
   it, and both envelopes round-trip in each language. The root
   `.gitignore` ignores anything named `lem`, so the frontend's own
   re-includes `proto/lem/`. Review the schema against 0011 before any code uses it.
3. **`lem-relay/protobuf`.** Add the schema as a `:protobuf-source-file`
   component. Encode the frame model into generated messages,
   interning styles as it goes ([0012](adr/0012-the-relay-frame-model.md)). Use
   varint length-delimited framing. Implement the `Hello`/`Welcome`/`Exit`
   handshake. The frame model should not need to change; if it does,
   phase 1 got the model wrong, so fix it there.

   *Done:* `relay/protobuf/` (framing, codec, `serve`), 16 Rove tests.
   The frame model did not need to change. `Key` names are mapped here
   ([0014](adr/0014-the-display-describes-keys.md)); a `Hello` without a
   `protocol_version` gets `Exit` with the reason; the editor exiting
   sends `Exit` before the stream ends. `lem-ratatui` does not use it
   yet: it switches over once the display speaks it (step 4).
4. **`lem-protocol` rewrite.** Its `build.rs` uses `protox` and `prost-build`.
   Framing uses `decode_length_delimited`. Remove `serde` and `serde_json`.
   Then move `lem-ratatui` (`views.rs`, `paint.rs`, `input.rs`,
   `transport.rs`) onto the generated types. The interned style table
   lives display-side.

   *Done*, in two commits. `lem-protocol` gained `v1` alongside the JSON
   types. Then the display moved onto it and `lem-ratatui` switched to
   `lem-relay/protobuf` in one change, since the wire changes for both
   halves at once. The display now puts the terminal's cursor where Lem's
   is, shaped; fills unset colours from the theme's defaults when
   compositing; fits each run to its `width`; composites in
   `views-stacked` order, showing only listed views; captures the mouse
   and turns on bracketed paste; and sends keys neutrally (ADR 0014). The
   relay turns C-] into abort, as ncurses and the browser client do. The
   frame multiplexer is back on: its tab bar shows on row 0, as in
   ncurses. Acceptance 9/9. The workload, medians of five: 175 frames,
   59,069 B, against 190 / 265,007 B on `lem-relay/json` (−78%) and the
   original baseline's 186 / 315,475 B (−81%). Implement [0012](adr/0012-the-relay-frame-model.md)'s
   display half: the hardware cursor with its shape and visibility,
   default colours, fitting each run to its `width`, and compositing
   in `views-stacked` order. Then remove `keep-frame-multiplexer-off`
   from `lisp/main.lisp`, so the frame multiplexer works as it does in
   ncurses.
5. **Cross-language golden tests.** Lisp writes fixture frames, in
   binary plus text format for review, and a Rust test decodes them.
   Rust writes fixture inputs and a Rove test decodes them. This
   replaces `frame.jsonl` and catches schema drift that compiles on
   both sides.

   *Done.* Both fixtures live in `proto/fixtures/`, beside the schema,
   each with a text twin for review:
   - `relay-session.v1.bin` is written by `scripts/capture-relay-v1.lisp`
     from the real editor through the relay, and decoded by
     `lem-protocol/tests/relay_v1_fixture.rs`. That test checks wire
     order, style definitions, text, stacking and frame state.
   - `display-inputs.v1.bin` is built by `lem-ratatui`'s own key and
     mouse conversions (`src/golden.rs`, which fails if they change;
     `LEM_UPDATE_FIXTURES=1` rewrites it), and decoded by
     `relay/tests/golden.lisp`. That test checks what Lem receives: key
     names, modifiers, and a double click from the display's timestamps.
6. **Retire JSON.** Remove `lem-relay/json` and the `yason` dependency.
   Rewrite [protocol-notes](protocol-notes.md) around the new protocol:
   §11–§14 describe a wire that no longer exists and become history.

   *Done.* `lem-relay/json`, `capture-relay-json.lisp`, the JSON half of
   `lem-protocol` (its types, framing, JSON-RPC envelopes, both `.jsonl`
   fixtures and their tests) are gone, and with them `yason`, `serde` and
   `serde_json`. The ADRs cite `protocol-notes.md` by section, so it was
   not rewritten: it is marked as history, and
   [`protocol.md`](protocol.md) describes `lem.relay.v1` as it behaves.

**Done when:**

- the invariants hold;
- the golden tests pass in both directions;
- `make dist` produces a working single binary;
- a fresh clone builds with only `make toolchain && make` beyond the
  documented prerequisites;
- `workload.py` is measured and recorded in 0011 next to the phase 1
  numbers.

**Phase 2 is done** (September 2026). The frontend speaks `lem.relay.v1`
end to end, generated on both sides from one schema, and JSON is gone.

- Acceptance passes 9/9, and the golden fixtures decode in both
  directions.
- `make dist` builds the single binary, and it runs from its embedded
  halves.
- A clean copy of the tree, with no `.qlot/`, `toolchain/` or build
  outputs, built with `make toolchain` (2m40s) then `make` (48s), and
  passed `make test` and acceptance. qlot's and cargo's download caches
  were warm, so a new machine spends longer fetching, but nothing relied
  on a working tree's build state.
- The workload is recorded in [0011](adr/0011-a-frame-oriented-protocol.md):
  175 frames and 59,069 B, against 315,475 B at the start.

## Upstream reports, whenever convenient

- `cxxxr/jsonrpc`: the four stdio-transport defects in
  [protocol-notes](protocol-notes.md) §13. We stop needing the fixes in
  phase 1, but `lem-server --mode stdio` still has them.
- `qitab/cl-protobufs`: `protoc-gen-cl-pb` CMake fails out-of-source.
