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
`ratatui-core` for `Buffer`/`Cell`/`Style` and its diff, and nothing
else. The display writes to the terminal itself, with crossterm, because
Ratatui's own output cannot draw curly or dotted underlines. Calling it a
crossterm frontend that borrows Ratatui's cell buffer is more honest.

See [`docs/adr/0002`](docs/adr/0002-ratatui-core-over-full-ratatui.md) and
[`0016`](docs/adr/0016-underline-styles.md).

## Layout

```
Makefile                     build, run, test, dist (see Building)
dist/                        every build output (gitignored): the Lisp
                             image, the dev launcher, the bundled binary
lem-ratatui.asd              ASDF system; :pathname "lisp/"
lisp/
  implementation.lisp        the `ratatui' class: relay + capability flags
  transport.lisp             stdin/stdout as the wire, everything else muffled
  links.lisp                 web addresses as hyperlinks (ADR 0018)
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
    lem-ratatui/             the display: transport, compositing, input,
                             and its own terminal output (present.rs)
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
make -C frontends/ratatui test     # Rust and lem-relay suites (Lisp needs Roswell)
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

At startup the display asks the terminal for its default colours
(OSC 10/11, through `terminal-colorsaurus`) and passes them on in
`Hello`: Lem judges light or dark mode by them for any theme that does
not choose one, and falls back to them for attributes without colours.
A terminal that does not support the query answers the DA1 request sent
with it, so nothing waits; the relay logs what it got, or `unknown`.

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
python3 scripts/acceptance.py      # needs `make` to have run; 13 checks
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
- **Light terminals with the default theme.** The display tells Lem the
  terminal's own colours, but `lem-default` pins
  `:display-background-mode :dark` (`src/ext/themes.lisp:40`), so on a
  light terminal it still picks dark-mode colours. That is Lem's theme
  to change, upstream.

## Keys and mouse

**Font styles.** Markdown emphasis is drawn in italic. Lem's attributes
can be italic, struck through or dim, and have an underline style
([ADR 0017](docs/adr/0017-font-styles-and-links-in-lem-attributes.md), a
small patch to Lem core carried here until upstream takes it). Give them
in an attribute's definition or in a theme; to change one defined
elsewhere, redefine it in `init.lisp`, which a theme load keeps:

```lisp
(lem:define-attribute lem:syntax-comment-attribute
  (:light :foreground "#cd0000" :italic t)
  (:dark :foreground "chocolate1" :italic t))
```

**Underline styles.** LSP diagnostics and Lisp compiler notes get a curly
underline, in terminals that draw one: kitty, WezTerm, foot, Alacritty,
Ghostty, iTerm2 and VTE-based ones (GNOME Terminal and others). Elsewhere,
and inside tmux or screen, they are underlined straight, as before.
`LEM_RATATUI_UNDERCURL=1` or `0` overrides the detection. Any attribute can
ask for `:underline-style :curly`, `:dotted`, `:dashed` or `:double`
([ADR 0016](docs/adr/0016-underline-styles.md)).

**Hyperlinks.** Web addresses in any buffer are terminal hyperlinks (OSC 8)
in terminals that make them: kitty, WezTerm, foot, Alacritty, Ghostty,
iTerm2, VTE-based ones, Konsole and Windows Terminal. Hovering shows the
address; the terminal opens it with its bypass modifier, usually
Shift+click (Ctrl+click in VTE), since a plain click goes to Lem. Inside
tmux or screen, and elsewhere, the text is drawn as before.
`LEM_RATATUI_HYPERLINKS=1` or `0` overrides the detection (tmux 3.4 passes
links on with its `hyperlinks` feature), and
`(setf lem-ratatui/links:*links* nil)` in `init.lisp` turns them off
([ADR 0018](docs/adr/0018-hyperlinks.md)).

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

**Enhanced keyboard input.** On startup the display queries the controlling
terminal for the kitty keyboard protocol. When available, it requests
escape-code disambiguation and alternate-key reporting, then confirms the
active flags. Detection and confirmation share a one-second deadline.
Unsupported terminals, failed queries and missing confirmations use legacy
input. `LEM_RATATUI_KEYBOARD=legacy` skips negotiation.

The confirmed keyboard capabilities travel to the relay in `Hello`.
`M-x describe-terminal-capabilities` reports them; an older display that
does not send capabilities is reported as unknown. The capability to report
keypad identity does not guarantee that every keypad key is distinguishable
in every terminal or Num Lock state: each key carries its own provenance.

Modified keypad digits and arithmetic keys can be bound separately, for
example `C-Keypad4` or `C-M-Keypad5`. Ordinary keypad digits still insert
text, and keypad Enter and navigation retain their usual behaviour.
Shifted printable chords use Lem's character convention: Ctrl+Shift+G is
`C-G`, distinct from `C-g`; Alt+Shift+P is `M-P`. For named keys, use
`Shift-Tab` or `C-Shift-Return`. In Lem key specifications `S-` means Super,
not Shift. Terminal or desktop shortcuts may still intercept a gesture
before it reaches Lem.

The Rust workspace carries a narrowly patched crossterm 0.29.0 under
`rust/vendor/`. Its terminal query writes only to `/dev/tty`, waits within
the supplied deadline and preserves input queued during detection. It also
decodes the raw C0 bytes 0x1C–0x1F by their ASCII names, `C-\`, `C-]`,
`C-^` and `C-_`. Enhanced Ctrl+4 through Ctrl+7 therefore remain distinct
from those control keys, including when legacy input was queued during
negotiation. The local patch record documents the upstream source and fixes.

Keyboard flags are restored once on exit or panic, before leaving the
alternate screen. A terminal that cannot report enhanced keys continues to
support ordinary Emacs chords; personal configurations should retain those
as alternatives.

Focused keyboard checks, from this frontend directory:

```sh
cargo build --manifest-path rust/Cargo.toml -p lem-ratatui
python3 scripts/keyboard-acceptance.py
cargo test --manifest-path rust/vendor/crossterm/Cargo.toml --lib
python3 rust/vendor/crossterm/tests/keyboard_probe_pty.py
```

These use isolated PTYs and protocol pipes. They verify negotiation,
fallback, queued keys and restoration without opening Lem or changing a
personal configuration. Physical gestures still need checking in the
terminal and keyboard layout in use.

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
