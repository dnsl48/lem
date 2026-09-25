# 0004. Two OS processes, not an embedded Lisp runtime

**Status:** Accepted — September 2026. The parent/child arrangement is
superseded by [0008](0008-a-launcher-owns-the-processes.md); the two
processes are not.

## Context

If the display half and the editor are going to talk, they could be two
processes exchanging messages, or one process with Lisp embedded in the
Rust executable so the same RPC runs in-memory.

The second framing contains a trap worth stating plainly: **embedding
while keeping RPC is the worst of both worlds.** The pipe was never the
cost. A 2KB frame over a local pipe is a syscall and a memcpy — single
digit microseconds. The expensive part is YASON turning drawing objects
into bytes ([0003](0003-keep-json-codec-for-now.md)), and that is
unchanged by sharing an address space. Embedding to remove the pipe pays
the entire build-complexity bill to delete a rounding error.

So a single process only pays if RPC is *also* replaced by a direct call
boundary. That is a different architecture, not a transport tweak.

And if we ever go there, the direction is the reverse of the question as
posed. Embedding SBCL in a Rust executable requires SBCL built with
`sb-linkable-runtime`, which distributions do not ship:

```
$ sbcl --version                     SBCL 2.6.8-1.fc44   (stock Fedora)
  linkable-runtime:   NIL            <- cannot be embedded as built
  load-shared-object: WORKS          <- can load a .so today, no special build
```

Every contributor and packager would need a bespoke SBCL. Meanwhile SBCL
dlopening a Rust `cdylib` through CFFI needs nothing special and is how
Lem's existing frontends already work — ncurses via `cl-charms`, SDL2 via
the `sdl2` bindings. There are also the hazards of putting SBCL's GC and
signal machinery (`SIGSEGV` write barriers, signals for thread
interrupts) in one process with crossterm/mio's signal handling, and of
registering Rust-spawned threads with SBCL before they can call in.

Latency is not an argument for either side. The IPC hop is microseconds
against a frame budget measured in milliseconds — roughly 1-2%, far below
the ~10-20ms where typing latency becomes perceptible. Lem's own redraw
computation and the terminal's escape-sequence processing dominate.

What two processes actively buy:

- **Terminal restoration on crash**, the strongest of them. When Lem dies
  the Rust parent sees EOF and cleanly drops raw mode and leaves the
  alternate screen. In-process, a hard Lisp abort skips `Drop` handlers
  and leaves the user's terminal wrecked — and SBCL falling into LDB
  would fight the TUI for the same terminal.
- **SLIME against a live editor** — recompile the Lisp half while the TUI
  keeps running. Not a small thing for a Lisp project.
- **Fixture capture and replay** — run the editor by hand, capture
  frames, replay them into the Rust side with no editor and no TTY. This
  is what makes `lem-protocol` testable and why it is its own crate.
- **Independent debugging** of each half.

What one process would honestly buy, stated fairly: no serialization at
all, a single artifact to install, no child lifecycle or orphan handling,
and no protocol version skew between separately installed binaries.

## Decision

Two OS processes. The Rust binary is the parent and owns the terminal;
Lem is spawned as a child and speaks JSON-RPC over its stdio.

## Consequences

- Version skew between independently installed binaries is a real cost
  that one process would not have. It needs a protocol version exchange
  at `login` before either half ships to users. Not required for the PoC.
- The child's stderr must be redirected to a log file. Inherited from the
  Rust parent it lands on the TTY and corrupts the display. See
  [0005](0005-stdio-as-the-default-transport.md) for the matching stdout
  hazard.
- The escape hatch, if serialization ever measures as the bottleneck, is
  a Rust `cdylib` loaded by Lisp through CFFI — *not* SBCL embedded in
  Rust. Going that way means implementing all ~50 `lem-if:*` generics
  directly instead of inheriting them from `lem-server:jsonrpc`.
- That escape hatch is cheap to keep open: the compositing and rendering
  code in `lem-ratatui` is unchanged by such a move, since only the point
  where frames enter the crate would differ. This is the seam
  `lem-protocol` already draws.
