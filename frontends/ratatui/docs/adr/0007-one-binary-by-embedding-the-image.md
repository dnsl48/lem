# 0007. One binary by embedding the Lisp image, not the Lisp runtime

**Status:** Superseded by [0008](0008-a-launcher-owns-the-processes.md) —
the image is still embedded, but in the launcher, beside the display
binary.

## Context

[0004](0004-two-processes-not-embedded-lisp.md) chose two processes and
conceded what one process would buy, including "a single artifact to
install" and "no protocol version skew between separately installed
binaries". A build produces three files: the Rust binary, the saved Lisp
image (`lem-ratatui-lisp`, ~125 MB) and a launcher script that runs the
first against the second.

Both of those concessions are about *packaging*, not about process
count. They can be had while keeping two processes, if the image travels
inside the Rust binary.

The obvious way to run an embedded image doesn't work. Writing it to a
memfd and spawning `/proc/self/fd/N` fails:

```
fatal error encountered in SBCL pid 14274 tid 14274:
Can't find sbcl.core
```

An SBCL executable finds its own core by opening `/proc/self/exe`, and a
memfd's link there (`/memfd:... (deleted)`) is not a path it can open.
The image has to exist as a real file.

Separately, SBCL can compress a saved core itself (`:compression t`,
available in the stock Fedora 2.6.8). That shrinks the file but pays the
decompression on every launch.

## Decision

Behind a `bundle` Cargo feature, `build.rs` compresses the image with
zstd and the binary embeds it with `include_bytes!`. When no image path
is given on the command line, the binary unpacks it to
`$XDG_CACHE_HOME/lem-ratatui/<hash>/lem-ratatui-lisp` (default
`~/.cache`) and spawns it from there, exactly as before. The hash is of
the image, so an unchanged image is unpacked once and reused, and a new
one never runs a stale copy. Images left by other builds are pruned when
a new one is unpacked.

Built by `make dist`, under its own Cargo profile so it doesn't replace
the development binary.

## Consequences

- One file to install: ~27 MB, against ~125 MB for the image alone. The
  two halves can no longer be installed at different versions, which
  closes the skew 0004 identified for the bundled build — the protocol
  version exchange is still needed for anyone running the halves
  separately.
- The first launch unpacks ~125 MB (measured 0.28s to first paint,
  against 0.14s once cached). Later launches cost nothing extra.
- The cache holds one copy of the image. Two different dist builds used
  alternately evict each other and re-unpack each time.
- Without the feature nothing changes: the development flow, SLIME
  against a live editor and fixture capture all keep the separate image.
- Nothing here touches 0004's escape hatch. If Lisp ever loads Rust as a
  `cdylib`, the image *is* the artifact and this goes away.
