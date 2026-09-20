# lem-ratatui

A terminal frontend for Lem built on `crossterm`, reusing the JSON-RPC
display protocol that already drives the browser and webview frontends.

> **Status: scaffold.** The design is settled and written down; nothing is
> implemented. `cargo test` passes and the Lisp half is a capability
> subclass plus an entry point. There is no working editor here yet.

## Shape

Two halves, split by toolchain rather than by role:

```
lisp/     implements lem-if:* by inheriting lem-server:jsonrpc,
          overriding only the capability flags a terminal changes
rust/     owns the terminal: paints protocol frames into a cell buffer,
          sends key and mouse events back
docs/     the investigation, and the decisions it produced
```

Lem runs as a **child** of the Rust process. That direction is forced:
with `--mode=stdio` Lem's stdout carries protocol traffic, so it must not
be a TTY.

```
┌─────────────────┐  spawns   ┌──────────────────────────┐
│  lem-ratatui    │ ────────► │ lem --interface RATATUI  │
│  (owns the TTY) │ ◄──────── │ (stdio JSON-RPC)         │
└─────────────────┘  frames   └──────────────────────────┘
```

## Why not "a Ratatui frontend"

Lem resolves its own window tree and layout before emitting anything, and
sends absolutely-positioned paint commands in cell coordinates. So
Ratatui's widget library and layout solver are unused — this depends on
`ratatui-core` for `Buffer`/`Cell`/`Style` and `ratatui-crossterm` for the
backend, and nothing else. Calling it a crossterm frontend that borrows
Ratatui's cell buffer is more honest.

See [`docs/adr/0002`](docs/adr/0002-ratatui-core-over-full-ratatui.md).

## Layout

```
lem-ratatui.asd              ASDF system; :pathname "lisp/"
lisp/
  implementation.lisp        the `ratatui' implementation class
  main.lisp                  entry point: serve JSON-RPC over stdio
rust/
  Cargo.toml                 workspace
  crates/
    lem-protocol/            wire types + codec; no TTY, unit-testable
    lem-ratatui/             the binary: transport, compositing, input
docs/
  protocol-notes.md          what lem-server actually sends, with refs
  adr/                       decisions and the arguments behind them
```

## Building

```bash
cd rust && cargo test        # works today
```

There is deliberately no `make ratatui` target and no `lem.asd`
registration yet — neither is useful until the display half does
something.

## Next steps

The PoC's success criterion: *open a file, move the cursor, type, see
syntax colours, resize the terminal.* Single window; no popups, images,
mouse or clipboard — all of those are `lem-if` methods that can stay
stubbed.

1. stdio transport: spawn Lem, read `Content-Length`-framed JSON-RPC
2. `login` with terminal size and colours; handle `bulk`
3. one `Buffer` per view, composited in z-order on `update-display`
4. crossterm events translated to Lem key encodings, sent as `input`
5. instrument bytes and encode time per frame, so
   [`docs/adr/0003`](docs/adr/0003-keep-json-codec-for-now.md) can be
   revisited against numbers rather than estimates

Step 3 has no equivalent in the browser half and is where the real
unknowns are: a terminal has one grid, composites nothing, and gets no
occlusion repair for free. See
[`docs/protocol-notes.md`](docs/protocol-notes.md) section 7.

## Known gaps in the scaffold

- `lisp/main.lisp` reaches into `lem-server::` internals because the
  exported `run-stdio-server` hardcodes `--interface JSONRPC`. Teaching it
  an `:interface` argument is a small upstream change worth proposing.
- `:underline-color-support t` is set deliberately but is unverified
  end-to-end; it needs real terminal-capability detection.
- `*standard-output*` is not yet muffled, and stdout is the wire under
  `--mode=stdio`. A stray `format` corrupts the frame stream; the Rust
  side must also redirect the child's stderr to a log file. See
  [`docs/adr/0005`](docs/adr/0005-stdio-as-the-default-transport.md).
- No protocol version exchange at `login`. Two separately installed
  binaries can drift; needed before this ships to users, not for the PoC.
