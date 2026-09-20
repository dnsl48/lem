# Protocol notes: what `lem-server` actually sends

Research notes taken while scoping the Ratatui frontend (September 2026).
Everything here was read out of the tree at `f5ba7d83`; line references are
to that revision. The purpose is to record *why* the design decisions in
`adr/` are what they are, so they can be re-argued later against facts
rather than memory.

## 1. There is no "webview frontend"

`lem-webview` is roughly 100 lines of glue, not a frontend:

```lisp
;; frontends/webview/main.lisp:14
(defclass webview (lem-server:jsonrpc lem-core:implementation) ())
```

It inherits every `lem-if:*` method from `lem-server`. Its whole job
(`frontends/webview/main.lisp:67`) is to pick a random port, start
`lem-server:run-websocket-server` on a thread, and point a native webview
window at `http://127.0.0.1:<port>`.

So the family has exactly two real halves:

| Half | Lives in | Role |
|---|---|---|
| Lisp | `frontends/server/` | implements `lem-if:*`, emits notifications |
| Display | `frontends/server/frontend/` (`editor.js`) | paints what it is told, sends input back |

The Ratatui frontend is a third implementation of the *display* half.

## 2. The protocol

JSON-RPC 2.0. Three transports are already implemented: websocket, stdio,
and (non-Windows) unix domain socket, selected by `--mode=`.

**Server to display: notifications only.** 26 of them, registered on the
JS side at `frontends/server/frontend/editor.js:1165`:

```
update-foreground  update-background  make-view      delete-view
resize-view        move-view          redraw-view-after
clear              clear-eol          clear-eob
put                modeline-put       update-display move-cursor
change-view        resize-display     bulk           exit
get-clipboard-text set-clipboard-text js-eval        set-font
get-font           get-display-size   load-css       update-cursor-shape
```

**Display to server: five exposed methods** (`frontends/server/main.lisp:215`):
`login`, `input`, `redraw`, `got-clipboard-text`, `invoke`.

stdio framing is LSP-style `Content-Length: N\r\n\r\n<json>`, implemented by
a local monkey-patch over the `jsonrpc` library in
`frontends/server/jsonrpc-stdio-patch.lisp`.

## 3. Lem does 100% of the layout, in character cells

The display half never computes a layout. `make-view` arrives fully
resolved (`editor.js:630`):

```
{ id, x, y, width, height, useModeline,
  kind: 'tile' | 'header' | 'floating',
  type, border, borderShape,
  pixelX?, pixelY?, pixelWidth?, pixelHeight? }
```

`x/y/width/height` are **character cells**. The window tree, splits,
modeline placement and floating-window geometry are all resolved in Lisp
before anything is sent.

## 4. There are no widgets

The DOM container is a single `<div id="lem-editor">` (`editor.js:92`).
Each view appends its *own* `<canvas>` to it, absolutely positioned
(`CanvasSurface`, `editor.js:365`).

`BaseSurface` (`editor.js:282`) exposes exactly two drawing primitives
(`editor.js:353`):

```js
drawBlock(x, y, width, height, color)
drawText(x, y, text, textWidth, attribute)
```

Cell coordinates, multiplied by `fontWidth`/`fontHeight` at the last
moment. Completion popups, the minibuffer, the modeline, floating windows
— all of them are Lem windows drawn with those two calls. (`HTMLSurface`
exists but serves only `html-buffer`.)

The one piece of chrome the display half owns is window separators and
floating-window frames: `VerticalBorder` / `HorizontalBorder`.

## 5. Payload anatomy

`put` is emitted by `frontends/server/main.lisp:637`:

```json
{"method":"put","argument":{
  "viewInfo":{"id":3}, "x":0, "y":5,
  "text":"defun", "textWidth":5,
  "attribute":{"foreground":"#AABBCC","background":"#111111",
               "reverse":false,"bold":true,"underline":null,"cursor":false},
  "font":null}}
```

Roughly 200 bytes, of which the attribute is ~120. Note `textWidth` is
precomputed server-side, so the display half never needs its own
string-width calculation.

**Batching.** `notify*` enqueues; `lem-if:update-display`
(`frontends/server/main.lisp:411`) flushes the whole frame as a single
`bulk` notification carrying an array of `{method, argument}`
(`frontends/server/main.lisp:157`). One message per frame, not one per
glyph.

Rough sizes (estimates, not yet measured): a full 80x24 syntax-highlighted
redraw is ~190 puts, call it ~40KB; a single-line edit is under 2KB.
Measuring this for real is the first instrumentation task of the PoC.

## 6. Lem already diffs, at line granularity

This is the fact that most shapes the design. `redraw-buffer` for a
text buffer (`src/display/physical-line.lisp:821`) consults a per-window
**line fingerprint cache** (`src/display/physical-line.lisp:429`).
Unchanged screen rows never reach `lem-if:render-line`, and therefore
never become `put` notifications.

The `bulk` array is **already a diff**. A cell-level diff on the display
side is a second diff over an already-diffed stream, and should not be
expected to pay for itself in bytes.

## 7. Two things the browser gets for free that a terminal does not

**Compositing.** Each view is its own canvas with a z-index; the browser
composites overlapping views. A terminal has exactly one grid and
composites nothing. The display half must blit per-view buffers in
z-order (tiles, header, floating).

**Occlusion repair.** When a floating window is deleted, the browser
removes a canvas and the canvases underneath are still painted. A terminal
must repaint what was underneath — and **Lem will not resend it**, because
`lem-server` declares `:no-force-needed t` (`frontends/server/main.lisp:120`),
which tells `redraw-display` (`src/display/base.lisp:39`) to skip
force-redrawing lower windows. See `src/interface.lisp:30` for the flag's
own documentation, which calls out ncurses as the case that needs `force`.

A terminal display half therefore needs its own implementation subclass
with terminal-appropriate capabilities rather than using
`lem-server:jsonrpc` directly.

## 8. Fields a terminal ignores

`pixelX`, `pixelY`, `pixelWidth`, `pixelHeight`, `font`, `load-css`,
`js-eval`, and the icon-font machinery. The JS side already falls back
cleanly when pixel fields are absent
(`(pixelX != null) ? ... : x * fontWidth`), so they are genuinely
optional.

Skipping them server-side is worth doing eventually via capability
negotiation at `login`, which is additive and backward compatible. It is
not required to get a PoC running.

## 9. Serialization has been a measured hotspot

Not speculation — there is upstream history:

- `9fd84616` *"perf(server): reduce websocket serialization overhead"* —
  send view IDs instead of 14-field view objects, memoize attribute JSON,
  precompute `object-width`.
- `4c93eaf3` *"perf: fix terminal wakeup lock contention and cache webview
  view-id hash"*.

Note this cost is **already paid today** by every webview user; a second
display half does not add to it. See `adr/0003` for the ranked list of
levers if it ever needs attacking.

## 10. stdio hygiene: stdout is protocol

Running with `--mode=stdio` makes `*standard-output*` part of the wire.
Any stray `(format t ...)`, compiler note, or library that prints to
standard output corrupts the frame stream — the same failure mode that
bites LSP servers.

There is precedent for the mitigation in the tree. `run-websocket-server`
already muffles two of the three streams while the server runs
(`frontends/server/main.lisp`, `server-listen` on
`websocket-server-runner`):

```lisp
(let* ((null-stream (make-broadcast-stream))
       (*trace-output* null-stream)
       (*error-output* null-stream))
  ...)
```

The stdio path needs the same treatment extended to `*standard-output*`,
bound before any editor code runs.

Separately, the **child's stderr must be redirected to a log file** by
the Rust parent. If it is inherited, a Lisp backtrace lands on the TTY
and shreds the display. Keeping stderr as a real log is also what makes
the two-process split debuggable
([adr/0004](adr/0004-two-processes-not-embedded-lisp.md)).

## 11. The startup handshake

Established by running a real editor, not by reading the JS. Two things
are mandatory and neither is obvious from the message list.

**`login` must carry non-nil `foreground` and `background`.** The
`jsonrpc` implementation class declares those two slots with **no
`:initform`** (`frontends/server/main.lisp:106-107`), and `handle-login`
only assigns them when the client sends parseable colours. Omit them and
the slots stay unbound; the first `lem-if:update-display` then signals
`UNBOUND-SLOT`, which `with-error-handler` swallows. The editor keeps
running and **silently emits nothing forever**. The browser client never
hits this because it always sends its own option colours.

**The client must send a `redraw` notification after the login
response.** `login` alone produces no frame. `editor.js:1285` does
exactly this, and `redraw` (`frontends/server/main.lisp:190`) is what
calls `lem:send-event` with a forced `redraw-display`.

So the minimal working sequence is:

```
--> {"id":1,"method":"login","params":{"size":{"width":80,"height":24},
                                       "foreground":"#DDDDDD","background":"#111111"}}
<-- {"id":1,"result":{"views":[],"foreground":...,"background":...,"size":{...}}}
--> {"method":"redraw","params":{"size":{"width":80,"height":24}}}
<-- {"method":"resize-display",...}
<-- {"method":"bulk","params":[ ... make-view, put, clear-eol, ... ]}
```

A capture of that sequence lives in
`../rust/crates/lem-protocol/tests/fixtures/frame.jsonl`.

## 12. Where errors actually go

Anything signalled during a redraw is caught by `with-display-error` /
`with-error-handler` and logged through log4cl to **`<lem-home>/debug.log`**,
not to stderr. When the display half goes quiet, that file is the first
place to look — the process will appear healthy and keep running.

`lem-home` (`src/config.lisp:6`) resolves `LEM_HOME`, then `~/.lem/`,
then `$XDG_CONFIG_HOME/lem/`. It is used with `merge-pathnames`, so
**`LEM_HOME` needs a trailing slash** or its last component is treated as
a filename.

Worth setting to a scratch directory when capturing fixtures: with a real
profile, `attempt-automigrate-config-file` (`src/config.lisp:33`) can
block startup on an interactive `y/n` prompt if a legacy `config.lisp`
exists, and the editor will sit in `prompt-for-y-or-n-p` forever.

## 13. Defects in jsonrpc's stdio server transport

Server-side stdio is not exercised by anything else in this ecosystem,
and it carries three independent defects. All are worked around in
`../lisp/jsonrpc-stdio-fixes.lisp`; each is worth reporting upstream.

1. **Notifications never leave the process.** `jsonrpc/server:broadcast`
   walks `server-client-connections`, populated by `on-open-connection`.
   The tcp, websocket and local-domain-socket transports all call it;
   **stdio never does**, so the list stays empty and every notification is
   dropped. Request/response still works, which is why `login` succeeds
   while no frame arrives.
2. **The framing counts characters, not bytes.** `write-message` sends
   `(length json)` and `read-message` reads into `(make-string length)`,
   so one non-ASCII character desynchronises everything after it.
3. **`lem-server`'s own fix for (2) is dead code.** Its
   `jsonrpc-stdio-patch.lisp` is written against an older API:
   `CONNECTION-SOCKET`, `READ-HEADERS` and `PARSE-MESSAGE` are all
   interned-but-undefined against the pinned version, so both of its
   methods signal `undefined-function` on first use. Verified by loading
   `lem-server` and checking `fboundp` on each.

## 14. One `bulk` is exactly one frame

Every `bulk` notification ends with an `update-display` instruction.
That is the frame boundary, and it is the signal to composite and draw.

From the committed capture, a fresh 80x24 editor after login + redraw:

```
0: <login response>
1: resize-display
2: bulk  n=18  make-view, resize-view, move-view, put, modeline-put,
               clear-eol, clear-eob, change-view, move-cursor,
               redraw-view-after, update-display
3: bulk  n=12
4: bulk  n=54
5: bulk  n=13
6..8: bulk n=12 each
```

Two consequences for the display half.

**The first `bulk` is the whole initial paint** — 18 instructions
carrying `make-view` through `update-display`. There is no partial
startup state to accumulate.

**The editor keeps emitting frames forever**, even with no input: the
trailing 12-instruction bulks are cursor blink and modeline updates. Any
loop that waits for a fixed number of messages will either hang or cut a
frame in half. Read until `update-display`, then draw.
