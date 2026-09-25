# 0008. A launcher owns the processes; the display only speaks the protocol

**Status:** Accepted — September 2026

## Context

[0004](0004-two-processes-not-embedded-lisp.md) made the Rust display
binary the parent: it found the Lisp image, spawned it, redirected its
stderr to a log, and killed it on exit. [0007](0007-one-binary-by-embedding-the-image.md)
then taught the same binary to carry the image and unpack it. So one
crate held two unrelated jobs: painting protocol frames, and being the
program the operating system runs.

The second job is about to grow. System tray presence, collecting and
reporting errors, possibly telemetry — none of it has anything to do with
cells and escape sequences, and all of it would otherwise land in the
crate that should only care about the protocol. Two symptoms had already
shown up: the display's command line had to be moved into an environment
variable so it could be passed on to Lem, and Lem's exit status was never
looked at, which hid a crash on every `C-x C-c`.

For the display to stop spawning Lem, something else has to connect
them, and the display's stdio is the obvious channel — but it also owns
the terminal. On Unix crossterm already reads keys, sets raw mode and
measures the window through `/dev/tty` whenever stdin is not a TTY, so
only drawing has to be pointed at `/dev/tty` explicitly.

## Decision

A third binary, `lem-ratatui-launcher`, starts both halves as siblings
and cross-wires two anonymous pipes between them — Lem's stdout to the
display's stdin and back — using `duct`. It depends on neither
`lem-ratatui` nor `lem-protocol`.

- `lem-ratatui` speaks the protocol on its own stdin/stdout and draws on
  the controlling terminal. With no controlling terminal it runs
  headless, as before.
- The launcher owns everything about the operating system: finding both
  programs (`LEM_RATATUI_LISP`, `LEM_RATATUI_TERMINAL`, the bundle, or
  the display beside itself), Lem's log file, passing the command line to
  Lem, and turning how each half ended into an exit code and a message.
- The `bundle` feature moves to the launcher and embeds **both** the Lisp
  image and the display binary, unpacked together under one hash.
  `make dist`'s single file is now the launcher.

## Consequences

- 0004's reasons for two processes all stand, including terminal
  restoration: the launcher keeps no pipe end open, so either half
  exiting is still EOF to the other. What changes is only who is the
  parent.
- The launcher sees both exit statuses. A Lem that dies of a signal or
  exits non-zero is now reported with the log's path, and the launcher
  exits non-zero. The first thing this caught was a Lisp-side crash on
  every `C-x C-c` — jsonrpc's stdio `start-server` cleanup destroying a
  thread that had already exited — now fixed in
  `lisp/jsonrpc-stdio-fixes.lisp`. Acceptance check 7 guards it.
- A display that is killed outright (SIGKILL) still can't restore the
  terminal, as before. The launcher is now the natural place to do that
  on its behalf, since it survives both halves.
- Three processes instead of two, and one more binary in a development
  build. The dev launcher finds the display in `target/release/` beside
  itself, so nothing extra has to be configured.
- `/dev/tty` is Unix. On Windows the display opens `CONOUT$`, which is
  untested, as is Windows generally.
- Stdout of the display is protocol: a stray `println!` there corrupts the
  stream exactly as a stray `format t` does on the Lisp side
  ([0005](0005-stdio-as-the-default-transport.md)).
