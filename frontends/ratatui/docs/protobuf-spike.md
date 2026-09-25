# Spike: protobuf on both halves

September 2026. Answers one question for [ADR 0010](adr/0010-protobuf-for-schema-and-codec.md):
can Lisp and Rust exchange protobuf messages generated from one `.proto`,
using maintained libraries, without a hand-written codec on either side?

**Yes.** A frame encoded by `cl-protobufs` decodes in `prost`, and an
input encoded by `prost` decodes in `cl-protobufs`. That covers UTF-8
text, `oneof`, `repeated` and proto3 `optional`. The cost sits entirely
in the Lisp build toolchain. It is real but bounded, and the rest of
this note is mostly about it.

## What was tested

A cut-down draft of the frame protocol, chosen to exercise the shapes
the real schema will use:

```proto
syntax = "proto3";
package lem.relay.spike;

message Frame     { uint64 seq = 1; repeated Attribute attributes = 2; repeated Op ops = 3; }
message Attribute { uint32 id = 1; optional Rgb fg = 2; optional Rgb bg = 3;
                    bool bold = 4; bool reverse = 5; optional Rgb underline = 6; }
message Rgb       { uint32 rgb = 1; }
message Op        { oneof op { MakeView make_view = 1; Put put = 2; ClearEol clear_eol = 3; } }
message Put       { uint32 view = 1; uint32 x = 2; uint32 y = 3; string text = 4; uint32 attribute = 5; }
message Input     { oneof event { Key key = 1; Resize resize = 2; } }
message Key       { string name = 1; bool ctrl = 2; bool meta = 3; bool shift = 4; bool super = 5; }
// MakeView, ClearEol, Resize elided
```

- **Lisp → Rust:** Lisp built a `Frame` (one attribute; `make-view`, a
  `put` of `"(defun héllo () 'λ)"`, `clear-eol`) and wrote 70 bytes. Rust
  decoded it field for field. The unset `optional underline` came back
  as `None`, not as a zero value.
- **Rust → Lisp:** Rust wrote a 7-byte `Input` carrying `C-x`. Lisp
  decoded it as `key { name: "x" ctrl: true }`, and `input.event-case`
  returned `KEY`.

## Versions

| Piece | Version |
|---|---|
| SBCL | 2.6.8 |
| cl-protobufs | git `b15c26db` (2026-09-23) |
| protobuf (`protoc`, `libprotoc`) | v36.2, built from source |
| protoc-gen-cl-pb | from the same cl-protobufs commit |
| prost / prost-build | 0.14.4 |
| protox | 0.9.1 |

## Lisp side: `cl-protobufs`

**Maintenance.** Maintained under `qitab`, the Google Common Lisp org:
41 commits since January 2025, the last one two days before this spike.
It is in Quicklisp (2025-06-22 release). Every Lisp dependency it needs
(`closer-mop`, `alexandria`, `trivial-garbage`, `cl-base64`,
`local-time`, `float-features`) was already in our `.qlot`.

**How it generates code.** A `.proto` becomes Lisp via `protoc` plus
the C++ plugin `protoc-gen-cl-pb`. `cl-protobufs.asdf` wraps that in an
ASDF component, `:protobuf-source-file`, which shells out to `protoc`
at compile time and loads the result. The output is readable Lisp, one
`define-message` per message (182 lines for the schema above), in a
package named after the proto package: `CL-PROTOBUFS.LEM.RELAY.SPIKE`.
The API uses dotted accessors: `pb:make-put`, `pb:input.key`,
`pb:input.event-case`.

**The finding that matters most: loading `cl-protobufs` itself needs
the toolchain.** Its own `.asd` declares the protobuf well-known types
(`descriptor`, `any`, `timestamp`, …) as `:protobuf-source-file`
components (`cl-protobufs.asd:80-104`). So `protoc` and
`protoc-gen-cl-pb` must be on `PATH` on every machine that compiles
`lem-ratatui`, not only when our `.proto` changes. Committing our own
generated Lisp would not avoid this. The saved image, and therefore
`make dist` and its users, need nothing at run time.

**Getting the toolchain.**

- Distro packages are not an option on Fedora 44. It ships protobuf
  3.19.6 (2022), which predates the Abseil-based API that
  `protoc-gen-cl-pb` builds against. Other distributions will vary, so
  we should not rely on them.
- Building from source works without root:

  ```sh
  git clone --depth 1 --branch v36.2 https://github.com/protocolbuffers/protobuf.git
  cmake -S protobuf -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=17 \
        -Dprotobuf_BUILD_TESTS=OFF -Dprotobuf_ABSL_PROVIDER=module \
        -DABSL_PROPAGATE_CXX_STD=ON -DCMAKE_INSTALL_PREFIX=$PREFIX
  cmake --build build --parallel && cmake --install build        # 2m24s on 12 cores

  cd cl-protobufs/protoc                                           # in-source, see below
  cmake . -DCMAKE_CXX_STANDARD=17 -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PREFIX_PATH=$PREFIX -DCMAKE_INSTALL_PREFIX=$PREFIX
  cmake --build . --parallel && cmake --install .                  # 6s
  ```

  Abseil is fetched by protobuf's CMake (`ABSL_PROVIDER=module`), so
  the only system requirements are a C++17 compiler, CMake and zlib.
  The source tree is 257 MB and the installed prefix 65 MB. The
  resulting `protoc-gen-cl-pb` links statically against protobuf and
  Abseil, and dynamically only against libc, libstdc++ and zlib.

- **Papercut: the plugin's CMake only builds in-source.** The generated
  `proto2-descriptor-extensions.pb.h` lands in the build directory, but
  the include path points at the source directory. An out-of-source
  build (`cmake -B elsewhere`) fails with `fatal error:
  proto2-descriptor-extensions.pb.h: No such file or directory`. The
  README does say `cmake .`, so this is documented behaviour, but it is
  worth reporting upstream. Until then we build in a copy of the plugin
  directory.

**Build time.** A cold `asdf:load-system` of `cl-protobufs` plus the
spike schema took 62s, almost all of it compiling `cl-protobufs`. With
fasls cached it took 2.5s. That cost lands once per fresh Lisp build.

## Rust side: `prost` + `protox`

**`prost`** is the de facto standard: tokio-rs, 4.8k stars, about 600M
downloads, a release in June 2026.

**`protox`** compiles `.proto` files in pure Rust into the same
`FileDescriptorSet` that `protoc` produces, and hands it to
`prost_build::Config::compile_fds`. The Rust build therefore needs no
`protoc` and no C++ at all. 0.9.1 (Dec 2025), 15.7M downloads, 137
reverse dependencies including `prometheus-client`, Temporal's SDK
protos and Slint. The whole `build.rs`:

```rust
fn main() {
    let fds = protox::compile(["relay.proto"], ["../proto"]).expect("protox compile");
    prost_build::Config::new().compile_fds(fds).expect("prost-build");
    println!("cargo:rerun-if-changed=../proto/relay.proto");
}
```

It generated all 10 message types. There was one surprise: a field named
`super` becomes `super_`, because it is a Rust keyword.

**Fallback.** If `protox` ever stalls, `prost-build` can call `protoc`
directly. The Lisp toolchain above already builds one, and
`protoc-bin-vendored` (3.2.0, July 2025) is the other option. Switching
is a `build.rs` change only.

**Not chosen: Google's official `protobuf` crate** (4.36.x, on the same
release train as protobuf itself). It is newer and heavier: it builds
upb C sources and needs `protoc`. It gives us nothing over `prost` for
two processes exchanging plain messages.

## Encode cost (Lisp)

The same 24-line, 80-column screen encoded 2000 times, SBCL, warm:

| | bytes | µs / frame |
|---|---|---|
| protobuf, interned attributes | 2,198 | 40.5 |
| JSON via YASON, `lem-server` shape (full attribute per `put`) | 5,847 | 253.0 |

This compares **the new protocol with the old**, not the codec alone.
Most of the byte saving comes from attribute interning, which a JSON
protocol could also do. It is here to show that the Lisp encode side,
which [ADR 0003](adr/0003-keep-json-codec-for-now.md) listed as never
measured, is not a concern with protobuf. It was never the reason to
switch.

## Consequences for the plan

1. The toolchain has to be provisioned, not assumed. A `make toolchain`
   target builds the pinned protobuf and plugin into a local prefix, and
   the Lisp build prepends it to `PATH`. See
   [ADR 0010](adr/0010-protobuf-for-schema-and-codec.md).
2. Pin `cl-protobufs` by git commit, but not in the root `qlfile`. A
   checkout anywhere under `frontends/ratatui/` in a non-dot directory
   is found by qlot's project searcher. This was verified with a copy at
   `frontends/ratatui/toolchain/cl-protobufs/`: `find-system` resolved
   both `cl-protobufs` and `cl-protobufs.asdf` there, and a cold load
   (empty fasl cache, 59s) succeeded. Using the same checkout for the
   plugin build means the Lisp runtime and the plugin always come from
   the same commit.
3. Upstream Lem CI is unaffected as long as the `lem` system does not
   depend on `lem-ratatui`, which it does not today.
