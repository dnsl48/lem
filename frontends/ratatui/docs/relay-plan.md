# Plan: from `lem-server` to `lem-relay`

This implements [0009](adr/0009-our-own-relay.md) (our own relay),
[0010](adr/0010-protobuf-for-schema-and-codec.md) (protobuf) and
[0011](adr/0011-a-frame-oriented-protocol.md) (the protocol redesign)
in two phases. Each phase ends with a working editor.

The split exists so that a regression always has one suspect. Phase 1
changes who produces the frames and leaves the bytes the same. Phase 2
changes the bytes and leaves the producer the same. Doing both at once
would leave any breakage ambiguous.

**Phase 0, done:** the ADRs above and the
[protobuf spike](protobuf-spike.md).

## Invariants for every step

- `python3 frontends/ratatui/scripts/acceptance.py` passes 8/8.
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
- interned attributes;
- the cursor as frame state;
- `Hello`/`Welcome` semantics.

`lem-relay/json` expands that model back into today's messages: it
de-interns attributes, emits `bulk` with a trailing `update-display`,
and answers `login` then waits for `redraw`. Phase 1 therefore also
proves the model against the acceptance script, and phase 2 becomes a
codec swap rather than a second redesign. Porting `lem-server` verbatim
first would be less work now, but we would pay for it again in phase 2.

1. **Scaffold `relay/`.** Create `lem-relay.asd` with `lem-relay` and
   `lem-relay/json`, packages per `defpackage_rule`, and an empty relay
   mixin. Make `lem-ratatui` depend on it alongside `lem-server` for
   now. It should build and change nothing.
2. **Frame model and attribute table (`frame.lisp`).** Model a frame,
   its ops, the session-scoped attribute interning and the cursor
   state. Add Rove tests: the same attribute interns to the same id,
   and an empty frame is detected as empty.
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
- the fixed workload from [0003](adr/0003-keep-json-codec-for-now.md)
  (open a file, type 40 characters, save, quit) is no worse than 130
  frames / 639,265 bytes.

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
2. **Write `relay.proto`** to [0011](adr/0011-a-frame-oriented-protocol.md).
   Resolve its open questions first: who owns character width, icon
   glyphs, the mouse shape. Record the answers in 0011's follow-up or in
   a new ADR. Review the schema against 0011 before any code uses it.
3. **`lem-relay/protobuf`.** Add the schema as a `:protobuf-source-file`
   component. Encode the frame model into generated messages. Use
   varint length-delimited framing. Implement the `Hello`/`Welcome`/`Exit`
   handshake. The frame model should not need to change; if it does,
   phase 1 got the model wrong, so fix it there.
4. **`lem-protocol` rewrite.** Its `build.rs` uses `protox` and `prost-build`.
   Framing uses `decode_length_delimited`. Remove `serde` and `serde_json`.
   Then move `lem-ratatui` (`views.rs`, `paint.rs`, `input.rs`,
   `transport.rs`) onto the generated types. The interned attribute
   table lives display-side.
5. **Cross-language golden tests.** Lisp writes fixture frames, in
   binary plus text format for review, and a Rust test decodes them.
   Rust writes fixture inputs and a Rove test decodes them. This
   replaces `frame.jsonl` and catches schema drift that compiles on
   both sides.
6. **Retire JSON.** Remove `lem-relay/json` and the `yason` dependency.
   Rewrite [protocol-notes](protocol-notes.md) around the new protocol:
   §11–§14 describe a wire that no longer exists and become history.

**Done when:**

- the invariants hold;
- the golden tests pass in both directions;
- `make dist` produces a working single binary;
- a fresh clone builds with only `make toolchain && make` beyond the
  documented prerequisites;
- the fixed workload is measured and recorded in 0011 next to the
  phase 1 numbers.

## Upstream reports, whenever convenient

- `cxxxr/jsonrpc`: the four stdio-transport defects in
  [protocol-notes](protocol-notes.md) §13. We stop needing the fixes in
  phase 1, but `lem-server --mode stdio` still has them.
- `qitab/cl-protobufs`: `protoc-gen-cl-pb` CMake fails out-of-source.
