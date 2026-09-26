# lem-ratatui

A terminal frontend for Lem built on `crossterm`. Lem's side is
`lem-relay`, our own implementation of the `lem-if` protocol, and the two
halves speak `lem.relay.v1`, a protobuf protocol defined in
[`proto/`](proto/lem/relay/v1/relay.proto) (see
[`docs/relay-plan.md`](docs/relay-plan.md)).

> **Status: working PoC.** Opens files, edits them, renders syntax
> colours, reflows on resize and leaves the terminal clean on exit — all
> the acceptance checks from [`docs/poc-plan.md`](docs/poc-plan.md) pass.
> Splits, popups, tabs (the frame multiplexer), the clipboard and the
> mouse work; images do not. See **Next steps** and **Known gaps** below.

## Shape

Two halves, split by toolchain rather than by role:

```
relay/    lem-relay: every lem-if:* method, building frames from what
          Lem draws, and the codec that puts them on the wire
lisp/     the ratatui implementation class and the process entry point
rust/     the launcher, which starts both halves and connects them, and
          the display, which owns the terminal: paints protocol frames
          into a cell buffer, sends key and mouse events back
docs/     the investigation, and the decisions it produced
```

Lem and the display run as **siblings** under a launcher, which joins
their stdio with a pipe each way. Lem's stdout carries protocol traffic,
so it must not be a TTY; the display's does too, so it draws on
`/dev/tty` instead. Neither knows how the other was started.

```
                  ┌──────────────────────┐
                  │ lem-ratatui-launcher │  finds, unpacks, starts,
                  └──────────────────────┘  logs, reports exits
                    spawns │       │ spawns
               ┌───────────┘       └───────────┐
               ▼                               ▼
┌──────────────────────────┐  frames  ┌─────────────────┐
│ lem --interface RATATUI  │ ───────► │  lem-ratatui    │
│ (lem-relay over stdio)   │ ◄─────── │  (owns the TTY) │
└──────────────────────────┘  input   └─────────────────┘
```

See [`docs/adr/0008`](docs/adr/0008-a-launcher-owns-the-processes.md).

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
Makefile                     build, run, test, dist (see Building)
dist/                        every build output (gitignored): the Lisp
                             image, the dev launcher, the bundled binary
lem-ratatui.asd              ASDF system; :pathname "lisp/"
lisp/
  implementation.lisp        the `ratatui' class: relay + capability flags
  transport.lisp             stdin/stdout as the wire, everything else muffled
  main.lisp                  entry point, and lem-if:invoke
relay/                       lem-relay (ADR 0009, 0012)
  lem-relay.asd              lem-relay, lem-relay/protobuf, lem-relay/tests
  frame.lisp                 the frame model; suppression of no-op frames
  view.lisp, draw.lisp       drawing objects to ops, ported from lem-server
  relay.lisp                 the lem-if methods
  input.lisp                 what the display sends, delivered to Lem
  protobuf/                  lem.relay.v1: framing, codec, the event loop
  tests/                     Rove tests, run by `make test`
proto/lem/relay/v1/
  relay.proto                the protocol's one schema (ADR 0010-0014)
proto/fixtures/              golden messages both halves decode, each with
                             a text twin: relay-session (Lisp → Rust) and
                             display-inputs (Rust → Lisp)
scripts/
  acceptance.py              the acceptance checks, driven through a pty
  workload.py                the fixed workload's frames and bytes
  capture-relay-v1.lisp      regenerates proto/fixtures/relay-session.v1.*
rust/
  Cargo.toml                 workspace
  crates/
    lem-protocol/            wire types + codec; no TTY, unit-testable
    lem-ratatui/             the display: transport, compositing, input
    lem-ratatui-launcher/    the OS-facing binary: starts and connects
                             both halves; embeds them for `make dist`
docs/
  relay-plan.md              the plan lem-relay is being built to
  protocol.md                how lem.relay.v1 behaves: order, guarantees
  protocol-notes.md          what lem-server sent, before lem-relay (history)
  adr/                       decisions and the arguments behind them
```

## Building

```bash
make -C frontends/ratatui          # everything + the dist/lem-ratatui-dev script
make -C frontends/ratatui run      # launch against LEM_HOME=/tmp/lem-scratch/
make -C frontends/ratatui test     # Rust (70 tests) and lem-relay (77; needs Roswell)
make -C frontends/ratatui dist     # one self-contained binary: dist/lem-ratatui
```

**Prerequisites** beyond SBCL, qlot and Rust: a C++17 compiler, CMake,
zlib and git. The first build runs `make toolchain`, which builds
protobuf's `protoc` (v36.2) and cl-protobufs' `protoc-gen-cl-pb` into
`toolchain/` (about three minutes, once). Every Lisp build puts
`toolchain/bin` first on `PATH`, because cl-protobufs compiles `.proto`
files as it loads, its own included. The Rust side needs none of this.
`PROTOC_PREFIX=/usr/local` uses an installed protobuf instead, headers
and CMake files included. `make distclean` removes `toolchain/`, and
`make clean` leaves it. See
[`docs/adr/0010`](docs/adr/0010-protobuf-for-schema-and-codec.md).

`make dist` embeds the Lisp image and the display binary, zstd-compressed,
in the launcher (`bundle` feature, ~27 MB against the image's ~125 MB).
They can't be run from memory — an SBCL executable locates its core via
`/proc/self/exe`, which a memfd can't satisfy — so the first launch
unpacks them to `$XDG_CACHE_HOME/lem-ratatui/<hash>/` (default
`~/.cache`) and later launches reuse them; copies from other builds are
removed at that point. `LEM_RATATUI_LISP` and `LEM_RATATUI_TERMINAL`
still override either one. See
[`docs/adr/0007`](docs/adr/0007-one-binary-by-embedding-the-image.md) and
[`0008`](docs/adr/0008-a-launcher-owns-the-processes.md).

Every build output lands in `frontends/ratatui/dist/`, except the Rust
binaries of a development build, which stay in `rust/target/release/`.
The script `dist/lem-ratatui-dev` runs the launcher from there against the
image beside it, so it can be symlinked onto `PATH`; the launcher finds
the display beside itself. The Lisp image is rebuilt only when a
Lisp source under `src/`, `extensions/` or this frontend changes;
`make -B` forces it.

By hand, from the repo root:

```bash
qlot install
sbcl --load .qlot/setup.lisp --load frontends/ratatui/build.lisp
(cd frontends/ratatui/rust && cargo build --release)
LEM_RATATUI_LISP=frontends/ratatui/dist/lem-ratatui-lisp frontends/ratatui/rust/target/release/lem-ratatui-launcher
```

Point `LEM_HOME` at a scratch directory when driving it by hand — note
the **trailing slash**, which `merge-pathnames` requires. Every
argument is passed through to Lem, so `lem-ratatui README.md` opens the
file just as `lem README.md` would. `LEM_RATATUI_LISP` picks the Lisp
image and `LEM_RATATUI_TERMINAL` the display binary; `make dist`'s binary
falls back to the ones embedded in it.

Set `LEM_RATATUI_DEBUG=1` to turn off frame suppression, so every
frame and modeline Lem draws is sent (ADR 0012), and
`LEM_RATATUI_BACKTRACE=6` to dump every thread's backtrace after six
seconds when the editor appears stuck. The relay's own warnings go to
Lem's log, `<lem-home>/debug.log`. Lem's stderr goes to
`/tmp/lem-ratatui.log`; if Lem dies or exits non-zero the launcher says
so, names that log, and exits non-zero.

There is deliberately no `make ratatui` target in the root `Makefile`
and no `lem.asd` registration yet.

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
| Killing Lem | the terminal is restored, and the launcher reports it and exits 1 |
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

Encoding and decoding were never the cost. In the PoC, on `lem-server`'s
JSON, one realistic session decoded in 68 µs a frame, 0.4% of a 16.7 ms
frame budget ([`docs/adr/0003`](docs/adr/0003-keep-json-codec-for-now.md)).
Protobuf was chosen for the schema, not for speed
([`docs/adr/0010`](docs/adr/0010-protobuf-for-schema-and-codec.md)).

The notable number was frame *amplification*, not idle churn:

```
idle, clean buffer     ~0.0 frames/sec
idle, modified buffer  ~3.2 frames/sec
typing                 ~13 frames per keystroke
```

Lem is quiet when idle. One keystroke producing a dozen frames is the
cost worth attacking, and `render-line-on-modeline` repainting the whole
modeline unconditionally every frame was a large part of it.

Two rules keep that down, and both belong to the relay's frame model
([`docs/adr/0012`](docs/adr/0012-the-relay-frame-model.md)), below any
codec:

- a modeline is sent only when it differs from the last one sent for its
  view; `lem-server` repaints it in full every frame;
- a frame that changes nothing is not sent. Most were `clear-eob`
  re-blanking an already-blank region, which `redraw-lines` emits on
  every redraw where the buffer does not fill the window.

Before them, on `lem-server`, a fixed workload took 535 frames and
1,635,547 bytes; with them, 130 and 639,265 (−76%, −61%). That workload
is now scripted (`scripts/workload.py`, five runs, median). Moving from
`lem-server` to `lem-relay`, then from JSON to `lem.relay.v1`:

| fixed workload | `lem-server` + our overrides | `lem-relay/json` | `lem-relay/protobuf` |
|---|---|---|---|
| frames | 186 (176–189) | 190 (186–192) | 175 (170–184) |
| total bytes | 315,475 | 265,007 (−16%) | **59,069** (−81%) |

On JSON the relay sent a few more frames than `lem-server`'s overrides:
cursor-only ones. The relay remembers one last cursor, not one per view,
so the cursor coming back to where it was in another view is sent; with
per-view memory the median was 177. The display now moves the terminal's
own cursor, and needs them. The bytes fall mostly because styles are
defined once and referred to by id, and because nothing browser-only is
on the wire.

## Acceptance

```bash
python3 scripts/acceptance.py      # needs `make` to have run
```

## Next steps

Nothing here is required for the PoC; each is its own piece of work.

- **Phase 2 of [`docs/relay-plan.md`](docs/relay-plan.md)**: the
  protobuf protocol, and the display half of ADR 0012 (the terminal's own
  cursor with its shape, theme colours, stacking order).
- **Report the jsonrpc stdio defects upstream** (protocol-notes section
  13). We no longer depend on them, but `lem-server --mode stdio` does.
- **Images.** A terminal draws none.
- **No tabbar.** Lem's lives in `lem-server` and is html-only, so it is
  not loaded here; a terminal-native one would be a feature, not a port.
- **Find more geometry bugs by reconstructing the screen.** The modeline
  rendered in the wrong row for six tasks because acceptance only checked
  that its text was present. Replaying the escape stream into a virtual
  screen and reading it row by row catches what substring checks cannot.
- **Terminal colour detection.** `Hello` can carry the terminal's own
  default colours, for Lem's light/dark choice before a theme sets its
  own; the display does not query them yet (OSC 10/11).

## Keys and mouse

**C-]** interrupts the editor when it is busy (`abort`), as in the ncurses
and browser frontends. C-g is an ordinary key, so `keyboard-quit` works as
always. The display describes keys neutrally and the relay gives them
Lem's names ([ADR 0014](docs/adr/0014-the-display-describes-keys.md)).

The **mouse** is captured: click, drag, and the wheel reach Lem, and
double and triple clicks select an expression and a form. The relay counts
them, within `lem-relay/input:*click-interval*` (0.5 s, settable from
`init.lisp`). Capturing takes the terminal's own text selection away;
most terminals keep it on **Shift+drag**. Pasting from the terminal
(bracketed paste) inserts the text as the major mode pastes.

crossterm decodes the C0 controls 0x1C–0x1F as `Ctrl+'4'` through
`Ctrl+'7'`, but ASCII and Lem both name those bytes `C-\`, `C-]`, `C-^`
and `C-_`. They are translated back, so `C-\` reaches Lem as `C-\`
rather than reporting `Key not found: C-4`, and `C-_` runs `redo`.

The two readings are indistinguishable on the wire — a terminal sends
0x1C for both `Ctrl+4` and `C-\` — so the ASCII one wins because it is
what Lem binds. Enabling the kitty keyboard protocol would separate them
and this would need revisiting.

## Tabbar

`lem-server` enables a tabbar after init, implemented as an **html**
header window, which a terminal cannot paint. It came with `lem-server`,
so since the move to `lem-relay` it is simply not loaded. Setting
`lem/tabbar:*enable-tabbar-on-startup*` in an init file is harmless and
does nothing here.

## An upstream bug worth reporting

`lem/multi-column-list` crashes any frontend that declares
`:underline-color-support t` while the active theme leaves `:foreground`
NIL — which `lem-default`, the fallback theme, does on purpose so the
frontend can supply its own:

```lisp
;; src/ext/multi-column-list.lisp:214
:underline (if (underline-color-support-p (implementation))
               (darken-color (foreground-color) :factor 0.6)   ; NIL -> type error
               t)
```

`C-x C-b` dies with *"The value NIL is not of type
LEM/COMMON/COLOR:COLOR"*. `lem-server` (so webview and the browser) and
`lem-sdl2` both set that flag, and both read the same `foreground-color`,
so the same code path applies to them on the default theme.

The obvious fix is for that call site to fall back to
`lem-if:get-foreground-color`, which is the colour the frontend actually
reported at login, rather than the theme's deliberately-empty one.

Until then this frontend leaves the flag off, matching ncurses. Nothing
is lost visually: an attribute naming an underline colour still renders
one, because the display half maps `Underline::Color` regardless of the
flag. The flag has exactly one call site in Lem, and it is the crash.

## Known gaps

- `:underline-color-support` is off (see above) and needs real
  terminal-capability detection before it could be turned on.
- No protocol version exchange at `login`. The launcher ships both halves
  together, so they cannot drift today; phase 2's `Hello` carries one.
- A clipboard reply answers the latest request: today's wire has no
  request ids. Phase 2's protocol does.
