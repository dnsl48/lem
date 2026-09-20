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
