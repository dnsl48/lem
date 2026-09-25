# 0009. Our own relay, `lem-relay`, instead of `lem-server`

**Status:** Accepted — September 2026
**Supersedes:** [0001](0001-reuse-lem-server-jsonrpc.md), [0006](0006-stay-on-lem-server-for-now.md)

## Context

[0006](0006-stay-on-lem-server-for-now.md) kept `lem-server` "for now"
and named the trigger for reconsidering: the PoC reaching its
acceptance criteria, at which point "the mapping is understood and the
fork is a mechanical exercise". The PoC passed 8 of 8. The trigger has
fired.

What we have built on top of `lem-server` since then mostly works
around it:

- `lisp/transport.lisp` and `lisp/main.lisp` subclass the internal
  `lem-server::server-runner`, bind `lem-server::*server-runner*` and
  call `lem-server::init`, because `run-stdio-server` hardcodes
  `--interface JSONRPC`.
- `lisp/frame.lisp` specialises the internal `lem-server::notify-all`
  and reads `jsonrpc-message-queue` to suppress no-op frames.
- `lisp/modeline.lisp` specialises the internal `lem-server::notify*`
  to memoise the modeline.
- `lisp/jsonrpc-stdio-fixes.lisp` replaces methods in the `jsonrpc`
  library with identical specialisers, over `lem-server`'s own patch,
  which is dead code ([protocol-notes](../protocol-notes.md) §13).
- `6a78daa2` removes the html tabbar that `lem-server` enables at
  startup, which reserved two blank rows in a terminal.

Everything that uses `lem-server::` internals is a coupling that
upstream never agreed to.

Meanwhile the part of `lem-server` we actually depend on is small. The
`lem-if:*` protocol is defined by Lem core (`src/interface.lisp`), is
public, and is enough by itself: ncurses and SDL2 implement it with no
server at all. `lem-server` reaches into core internals in only three
places: `lem-core::adjust-all-window-size` in `redraw`, and
`lem-core::foreground-color` / `background-color` in `login`. The
first has a public equivalent, `lem:update-on-display-resized`
(`src/window/window.lisp:1056`), which is what core's own `:resize`
event calls. The other two are our own state in a relay that owns
its colour slots. **Owning the relay needs no change to Lem core.**

Of `frontends/server`'s ~4.8k lines, a terminal needs roughly 400–500
lines of `main.lisp`:

- the `lem-if` methods;
- `draw-object` / `put`, including `set-last-print-cursor`, the only
  mechanism by which the cursor position is tracked;
- key and mouse conversion;
- the login handshake;
- clipboard.

It does not need:

- `icon.lisp` (3.2k lines);
- `tabbar.lisp`, `color-picker.lisp`;
- websocket, clack and static-file serving;
- `js-eval`, `load-css`, `html-buffer` rendering;
- the `*-pixels` view methods.

## Decision

**Write our own relay, `lem-relay`, inside `frontends/ratatui`,
implementing `lem-if:*` directly. Stop depending on `lem-server`.**

- **Scope: terminal only.** No HTTP, WebSocket, HTML, CSS, JS, pixel
  positioning, fonts or tabbar. Any of these that Lem core offers stays
  switched off by the capability flags, as `ratatui` already does
  (`:html-support nil`, `:support-pixel-positioning nil`).
- **Port, don't rewrite, the drawing-object mapping.** The
  `draw-object` methods and the cursor tracking come across from
  `frontends/server/main.lisp` largely verbatim: [0006](0006-stay-on-lem-server-for-now.md)
  identified them as the code most likely to be subtly wrong if
  re-derived. Everything else is written for purpose.
- **Split by codec, as ASDF systems:**

  | System | Role |
  |---|---|
  | `lem-relay` | The `lem-if` methods and frame building. Produces frames as Lisp data and consumes inputs as Lisp data. Knows nothing about bytes. |
  | `lem-relay/json` | Today's wire: JSON-RPC 2.0 with `Content-Length` framing, the protocol today's `lem-protocol` crate decodes. Transitional ([plan](../relay-plan.md), phase 1). |
  | `lem-relay/protobuf` | The redesigned wire ([0010](0010-protobuf-for-schema-and-codec.md), [0011](0011-a-frame-oriented-protocol.md)). Phase 2. |
  | `lem-ratatui` | The entry point and the `ratatui` implementation class. Picks a codec. |

- **The relay class is a mixin, not a direct subclass of
  `implementation`.** `get-default-implementation` lists every direct
  subclass of `implementation` as a selectable interface. A `relay`
  class there would show up as an interface nobody can run. `ratatui`
  keeps inheriting `lem-core:implementation` directly, for the reason
  already given in `lisp/implementation.lisp`.
- **No `jsonrpc` library, even in phase 1.** `Content-Length` framing
  plus a JSON-RPC envelope is a few dozen lines to write ourselves. All
  four defects in [protocol-notes](../protocol-notes.md) §13 are in the
  library's stdio transport, not in the format. Writing it ourselves
  removes `jsonrpc-stdio-fixes.lisp` entirely.
- **What `ratatui` already layers on top moves into the relay as design,
  not as overrides:** frame suppression (`frame.lisp`), modeline
  memoisation (`modeline.lisp`), and the absence of the tabbar.

## Consequences

- The ratatui Lisp code stops touching `lem-server::` and `lem-core::`
  internals. The only core API it depends on is `lem-if:*` and the
  exported `lem`, `lem-core` and `lem/display` symbols.
- **Drawing objects are core's internal classes, so we now follow them
  ourselves.** [0006](0006-stay-on-lem-server-for-now.md) measured 2
  commits touching both `src/display/` and `frontends/server/main.lisp`
  in twenty months. When `src/display/physical-line.lisp` gains a
  drawing-object class, `lem-relay` needs a `draw-object` method for
  it. A catch-all method that logs and skips unknown objects makes
  that a visible gap rather than a crash.
- We no longer inherit drawing-pipeline fixes from `lem-server`. That
  is the same annual sync, in the other direction.
- `lem-server`, `webview` and the browser client are untouched.
  `lem.asd` still depends on `lem-server` for the combined `lem`
  executable. Nothing upstream changes.
- `lisp/transport.lisp`, `lisp/jsonrpc-stdio-fixes.lisp`,
  `lisp/frame.lisp` and `lisp/modeline.lisp` are deleted or absorbed.
  `Makefile`'s `LISP_SOURCES` drops `frontends/server`.
- Phase 1 must speak exactly the protocol today's Rust side decodes:
  the same methods, fields and handshake, and the same frame suppression
  and modeline memoisation `ratatui` already applies. The 8 acceptance
  checks and the captured `frame.jsonl` fixture are the test that the
  port is faithful before anything else changes.
