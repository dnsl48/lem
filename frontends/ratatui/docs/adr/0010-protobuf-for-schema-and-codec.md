# 0010. Protobuf for the schema and the codec

**Status:** Accepted — September 2026
**Supersedes:** [0003](0003-keep-json-codec-for-now.md)

## Context

[0003](0003-keep-json-codec-for-now.md) kept JSON on two grounds:

- the obvious binary alternative, MessagePack, had no mature Common
  Lisp library;
- changing the codec would break every existing display half.

Both grounds have moved. [0009](0009-our-own-relay.md) gives us our own
relay. There is one display half, ours, and we are redesigning the
protocol anyway ([0011](0011-a-frame-oriented-protocol.md)). So the
break happens regardless, and the only question is what it breaks to.

What we lack today is not speed. 0003's own measurements settle that:
Rust decode costs 36–68 µs per frame, under 0.5% of the frame budget.
What we lack is **a schema**. The protocol exists twice, as the
hand-built hash tables in `frontends/server/main.lisp` and as the
hand-written serde types in `lem-protocol`. Nothing checks that they
agree. Every finding in [protocol-notes](../protocol-notes.md) §11,
such as the unbound colour slots and the mandatory `redraw` after
`login`, was discovered by running the editor, because no document
described the protocol.

The options, judged on whether both halves are generated from one
definition:

- **Protobuf.** A `.proto` file is the definition, and both halves are
  generated from it. The spike ([protobuf-spike](../protobuf-spike.md))
  sent frames both ways between `cl-protobufs` and `prost`: UTF-8 text,
  `oneof`, `repeated` and proto3 `optional` all survived. Field numbers
  give a real evolution story.
- **JSON with JSON Schema.** Rust can generate types from a schema
  (`typify`), but nothing mature does that for Common Lisp. The Lisp
  half would stay hand-written and only validated in tests. That is
  better than today, but it is a check, not generation.
- **MessagePack, FlatBuffers, Cap'n Proto.** 0003 already found the
  MessagePack library immature. FlatBuffers and Cap'n Proto have weaker
  Common Lisp support than protobuf, and their zero-copy advantage buys
  nothing on a local pipe carrying a few kilobytes per frame.
- **A hand-written protobuf codec in Lisp.** The wire format is simple
  enough to hand-roll. But a hand-written codec is exactly the kind of
  mirror we are trying to get rid of. If generated protobuf were not
  viable, we would rather fall back to JSON with JSON Schema than
  maintain one.

The cost of protobuf is on the Lisp side, and the spike measured it.
`cl-protobufs` is maintained by `qitab`, the Google Common Lisp org,
with its last commit two days before the spike. It generates code with
`protoc` and a C++ plugin, `protoc-gen-cl-pb`. Its own `.asd` compiles
the protobuf well-known types through that plugin, so **every machine
that compiles `lem-ratatui` needs `protoc` and the plugin on `PATH`**,
not just those changing the schema. Fedora 44's packaged protobuf
(3.19.6) is too old to build the plugin against. Building protobuf
v36.2 and the plugin from source took 2m24s and needed only a C++17
compiler, CMake and zlib. The Rust half needs none of this: `protox`
compiles `.proto` in pure Rust.

## Decision

**Protobuf is the schema and the codec of `lem-relay/protobuf`.**

- **One schema, in the repository:** `frontends/ratatui/proto/lem/relay/v1/relay.proto`,
  proto3, package `lem.relay.v1`. Both halves are generated from this
  file and neither hand-writes message types. [0011](0011-a-frame-oriented-protocol.md)
  sets what goes in it.
- **Everything lives inside `frontends/ratatui`; no root file changes.**
  `make toolchain` populates a gitignored `frontends/ratatui/toolchain/`:

  ```
  toolchain/
  ├── cl-protobufs/   git checkout at a pinned commit
  ├── bin/            protoc, protoc-gen-cl-pb
  └── .build/         protobuf and plugin sources and build trees
  ```

- **Lisp: `cl-protobufs`, found without touching `qlfile`.** qlot's
  project searcher (`.qlot/local-init/qlot-99-setup.lisp`) finds any
  `.asd` in any subdirectory of the repository whose name does not start
  with a dot. So `toolchain/cl-protobufs/cl-protobufs.asd` is found like
  any system of Lem's own. Its Lisp dependencies are already in Lem's
  pinned Quicklisp dist. Verified: with an empty fasl cache, `asdf:find-system`
  resolved both `cl-protobufs` and `cl-protobufs.asdf` from there and
  `asdf:load-system` succeeded. The schema is an ASDF
  `:protobuf-source-file` component of `lem-relay/protobuf`, so it is
  regenerated whenever it changes.
- **Lisp toolchain: built by us, pinned, local.** The same `make toolchain`
  builds protobuf (v36.2 today) and `protoc-gen-cl-pb` into
  `toolchain/bin/`. The plugin comes from the same `cl-protobufs`
  checkout, so the generator and the runtime cannot come from different
  commits. It is built once, in about 2.5 minutes. The Lisp build
  prepends `toolchain/bin` to `PATH`, and setting `PROTOC_PREFIX` points
  at an existing install instead. We do not rely on distro packages.
  The 257 MB of C++ sources sit under `.build/` because qlot's searcher
  walks every non-dot directory whenever it looks up a system.
- **Rust: `prost` for the types, `protox` to compile the schema,** in
  `lem-protocol`'s `build.rs`. The Rust build needs no `protoc` and no
  C++. If `protox` ever falls behind, `prost-build` can call `protoc`
  from `toolchain/bin` instead; that is a `build.rs` change only.
- **One codec, not two.** `lem-relay/json` exists only to carry
  [phase 1](../relay-plan.md). It is removed once the protobuf wire is
  proven. Readable dumps for debugging and fixtures come from
  protobuf's text format (`cl-protobufs:print-text-format`, and
  `prost`'s `Debug`), not from a second wire format.
- **Fallback, if the toolchain proves untenable:** JSON with a JSON
  Schema checked on both sides, not a hand-written protobuf codec.

## Consequences

- The protocol has one definition, and a mismatch between the halves
  becomes a build error or a test failure instead of a silent frame
  drop.
- **Anyone building the Lisp half needs a C++17 compiler and CMake,**
  plus about 2.5 minutes the first time. Users of `make dist` need
  nothing: the saved image carries the generated code, and nothing runs
  `protoc` at run time.
- `lem-ratatui` does not become part of upstream Lem's build. The
  `lem` system does not depend on it, so Lem's CI and `qlot install`
  need no toolchain. No file outside `frontends/ratatui` changes: not
  `qlfile`, not `.gitmodules`, not `lem.asd`.
- **The price of avoiding `qlfile`:** `cl-protobufs` is pinned by our
  Makefile rather than by `qlfile.lock`. Loading `lem-ratatui` before
  `make toolchain` has run fails with a missing-system error, not an
  automatic fetch. The Makefile makes `toolchain` a prerequisite of the
  Lisp build, so this only affects someone loading the system by hand
  in a REPL.
- A cold Lisp build is about a minute slower (the spike measured 62s
  to compile `cl-protobufs` the first time). Cached builds are
  unaffected.
- Schema evolution follows protobuf's rules: never reuse or renumber a
  field, `reserved` what is removed. Launcher and display ship together
  in one binary ([0008](0008-a-launcher-owns-the-processes.md)), so
  versions differ only during development. The rules still cost nothing
  to follow.
- Upstream papercut to report: `protoc-gen-cl-pb`'s CMake only builds
  in-source ([spike](../protobuf-spike.md)). `make toolchain` builds in
  a copy until that is fixed.
