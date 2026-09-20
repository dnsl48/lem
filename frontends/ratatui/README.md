# lem-ratatui

A terminal frontend for Lem built on `crossterm`, reusing the JSON-RPC
display protocol that already drives the browser and webview frontends.

> **Status: working PoC.** Opens files, edits them, renders syntax
> colours, reflows on resize and leaves the terminal clean on exit — all
> eight acceptance checks in [`docs/poc-plan.md`](docs/poc-plan.md) pass.
> Single window only: no popups, mouse, clipboard or images. See
> **Known gaps** below for what is deliberately absent.

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
cd rust && cargo test                    # Rust half (53 tests)

# Lisp half: build, then drive it by hand
qlot install
sbcl --load .qlot/setup.lisp --load frontends/ratatui/build.lisp
```

Point `LEM_HOME` at a scratch directory when driving it by hand — note
the **trailing slash**, which `merge-pathnames` requires:

```bash
LEM_HOME=/tmp/lem-scratch/ ./frontends/ratatui/lem-ratatui-lisp
```

Set `LEM_RATATUI_DEBUG=1` for transport logging on stderr, and
`LEM_RATATUI_BACKTRACE=6` to dump every thread's backtrace after six
seconds when the editor appears stuck.

There is deliberately no `make ratatui` target and no `lem.asd`
registration yet — neither is useful until the display half does
something.

## What it does

Verified against a real editor under a pty:

| | |
|---|---|
| Startup screen and modeline | renders |
| `C-x C-f` | opens a file; the minibuffer prompt composites over the buffer |
| Syntax colours | 13 distinct truecolor foregrounds on Lisp source |
| Cursor keys | `abc`, Left, Left, `X` writes `aXbc` |
| Typing and undo | text inserts; `C-x u` reverts it |
| Resize | 60→100 columns paints to 100, 100→60 paints to 60 |
| `C-x C-c` | exits 0, raw mode and the alternate screen released |
| Killing Lem | the display half exits 0 and still restores the terminal |
| Splits | `C-x 3` and `C-x 2` render with `│` separators, nested |
| UTF-8 input | `café naïve 日本語 🔥 żółć` round-trips byte-exact |
| Floating windows | ringed by a rounded box, `drop-curtain` joining a prompt above |
| Clipboard | `M-w` copies to the system clipboard, `C-y` pastes from it |

Window splits get a `│` separator in the column Lem reserves through
`:window-left-margin`, spanning the modeline row. Horizontal splits need
none: each pane's modeline already delimits it.

Floating windows are ringed by a rounded box in the same characters
`lem-ncurses/style` uses, drawn *outside* the view as ncurses places it —
a view at (x, y) of w by h is ringed at (x-1, y-1) of w+2 by h+2. The
`drop-curtain` shape tees its top corners into whatever sits above, which
is how a completion list joins the prompt it belongs to.

One consequence worth knowing: Lem grows the `C-x C-f` prompt as you type,
and it can end up wider than the terminal — at 120 columns a long path
gave x=20, width=110. The right edge then falls off-screen and the box
looks open on that side. That is Lem's layout rather than a drawing bug;
ncurses fares worse there, since `newwin` fails outright when a window
does not fit and no border is drawn at all. A popup that fits is closed on
all four sides.

Wide characters work — Lem's startup modeline contains U+1F512, and
`ratatui-core`'s buffer advances by display width.

## Clipboard

`M-w` and `C-y` use the system clipboard through `arboard`. Lem waits only
0.1s for a paste to be answered, so the read happens inline rather than on
a thread.

Two caveats, neither ours to fix:

- On X11 the clipboard is served by the process that owns it, so text
  copied out of Lem is gone once Lem exits. That is how X11 works.
- With no display server — SSH without forwarding, a container —
  initialisation fails, a line is logged, and copy and paste do nothing
  rather than erroring.

`cargo run -p lem-ratatui --example clip -- get|hold TEXT` is a helper for
testing either direction.

## Performance

One realistic release-build session (open, type, move, resize twice,
save, quit):

```
960 frames, 3,006,287 B total, avg 3,131 B, max 17,234 B, avg decode 68us
```

Decoding is 0.4% of a 16.7ms frame budget, which is why
[`docs/adr/0003`](docs/adr/0003-keep-json-codec-for-now.md) keeps JSON.

The notable number is frame *amplification*, not idle churn:

```
idle, clean buffer     ~0.0 frames/sec
idle, modified buffer  ~3.2 frames/sec
typing                 ~13 frames per keystroke
```

Lem is quiet when idle. One keystroke producing a dozen frames is the
cost worth attacking, and `render-line-on-modeline` repainting the whole
modeline unconditionally every frame was a large part of it.

Two optimisations live in `lisp/`, both specialising on our own
implementation class — no change to `lem-server`, no patching:

- `modeline.lisp` sends the modeline only when it differs from the last
  one sent. `lem-server` repaints it in full every frame with no caching.
- `frame.lisp` drops a frame that changes nothing. Most frames were
  `clear-eob` re-blanking an already-blank region, which `redraw-lines`
  emits on every redraw where the buffer does not fill the window.

| fixed workload | baseline | both |
|---|---|---|
| frames | 535 | **130** (−76%) |
| total bytes | 1,635,547 | **639,265** (−61%) |

## Acceptance

```bash
python3 scripts/acceptance.py      # the 8-point script, needs both halves built
```

## Next steps

Nothing here is required for the PoC; each is its own piece of work.

- **Reconsider ownership of the Lisp half** —
  [`docs/adr/0006`](docs/adr/0006-stay-on-lem-server-for-now.md) names
  PoC completion as an explicit trigger to revisit.
- **Report the three jsonrpc stdio defects upstream**, and the two
  `lem-server` handshake traps.
- **Mouse, images, popup menus** — stubbed `lem-if` methods. Mouse
  capture being off means terminal-native selection still works.
- **`update-cursor-shape`** — no bar or underline cursor styles.
- **Find more geometry bugs by reconstructing the screen.** The modeline
  rendered in the wrong row for six tasks because acceptance only checked
  that its text was present. Replaying the escape stream into a virtual
  screen and reading it row by row catches what substring checks cannot.
- **Cursor shape and position** — `move-cursor` is currently ignored; the
  cursor renders only as Lem's own reverse-video cell.

## Keys

crossterm decodes the C0 controls 0x1C–0x1F as `Ctrl+'4'` through
`Ctrl+'7'`, but ASCII and Lem both name those bytes `C-\`, `C-]`, `C-^`
and `C-_`. They are translated back, so `C-\` reaches Lem as `C-\`
rather than reporting `Key not found: C-4`, and `C-_` runs `redo`.

The two readings are indistinguishable on the wire — a terminal sends
0x1C for both `Ctrl+4` and `C-\` — so the ASCII one wins because it is
what Lem binds. Enabling the kitty keyboard protocol would separate them
and this would need revisiting.

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
- `lisp/jsonrpc-stdio-fixes.lisp` works around three defects in jsonrpc's
  stdio server transport, one of which (`lem-server`'s own
  `jsonrpc-stdio-patch.lisp` referencing three undefined functions) is a
  live bug in the tree. All three deserve upstream reports; see
  [`docs/protocol-notes.md`](docs/protocol-notes.md) section 13.
- The display half must send non-nil `foreground`/`background` in `login`,
  or the editor silently stops emitting. That is a robustness bug in
  `lem-server` — the slots have no `:initform` — that we work around
  rather than fix. Section 11 of the protocol notes has the detail.
