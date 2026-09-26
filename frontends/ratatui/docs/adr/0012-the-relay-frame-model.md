# 0012. The relay's frame model

**Status:** Accepted — September 2026
**Refines:** [0011](0011-a-frame-oriented-protocol.md). It settles 0011's
open question on character width, and moves attribute interning from the
model to the phase 2 codec.

## Context

[0009](0009-our-own-relay.md) splits `lem-relay` into a codec-independent
core and codecs (`lem-relay/json` now, `lem-relay/protobuf` later). The
core's product is a **frame model**: the Lisp data the `lem-if` methods
build and a codec turns into bytes. Everything in phase 1 is built into
it, and phase 2 must be able to encode it unchanged
([plan](../relay-plan.md)). So its shape is the decision that is
expensive to revisit.

0011 set principles for the wire. A model can follow them closely, or
it can stay close to what Lem's existing frontends do and leave
optimisation to the codec. Where the two pull apart, **this model stays
close to the roots.** The baseline should be something `lem-server` and
ncurses already prove. Optimisations belong in a codec, where they can
be added and measured without disturbing what the `lem-if` methods
build.

How the existing frontends handle the points in question:

- **Attributes.** `lem-server` sends the full attribute with every
  `put`, with colours normalised to `"#RRGGBB"`. Its only cache is by
  identity, within one frame, to skip re-serialising
  (`frontends/server/main.lisp:609`). ncurses does intern by content, for
  the session, but only locally: a terminal has numbered colour pairs,
  and `get-color-pair` (`frontends/ncurses/term.lisp:361`) assigns one to
  each distinct `(fg . bg)` the first time it is seen. Ids on the wire
  are 0011's idea, not something either frontend does.
- **The cursor.** ncurses both paints the cursor cell (its attribute
  reverses it) and moves the terminal's own cursor there, hiding it for
  views marked cursor-invisible (`frontends/ncurses/view.lisp:271`).
  Today's ratatui display hides the terminal cursor and relies on the
  painted cell alone. It also drops `lem-if:update-cursor-shape`.
- **Default colours.** Lem calls `lem-if:update-foreground` and
  `update-background` when a theme sets them. `lem-server` forwards
  both; our display ignores them. Text drawn with no attribute shows the
  terminal's colours, not the theme's.
- **Stacking.** Neither protocol states the order views composite in.
  The ratatui display sorts by kind (tile, header, floating) and keeps
  creation order within a kind. Lem's actual order is
  `frame-floating-windows`, and the two can differ.

## Decision

**The model mirrors Lem's calls; it does not keep a screen.** Ops
correspond to `lem-if` calls:

- the view lifecycle: `view-created`, `view-deleted`, `view-moved`,
  `view-resized`, `view-cleared`;
- `views-stacked`;
- painting: `text-put`, `line-cleared` (clear to end of line),
  `rest-cleared` (clear to bottom of view), `modeline-painted`.

The display applies ops to its own buffers. A cell grid per view in the
relay, sending only changed cells, would make suppression exact, but it
duplicates the diffing Ratatui already does and puts per-cell work on
Lem's editor thread. It is the recorded fallback if the suppression
rules below ever prove fragile.

**Styles are inline values, compared by content.** Every `text-put`
carries its `style`, or NIL for the default colours, as `lem-server`
carries the attribute. A style is:

- foreground and background: packed `#xRRGGBB` integers, or NIL;
- bold and reverse;
- underline: NIL, T, or a colour;
- cursor: this cell is painted as a cursor.

Every colour spelling Lem accepts becomes one integer, as `lem-server`
makes them all hex strings. **Reverse stays a flag**, not a swap done in
the relay: when a colour is NIL, only the display knows what it resolves
to. **There is no interning in the model.** 0011's wire-level interning
(define each style once, then refer to it by id) is done by the phase 2
encoder while encoding, on the ncurses colour-pair pattern. It is a
codec optimisation there, not a property of the model.

**Default colours are frame state.** The session holds the colours Lem
last set. A frame carries them when they changed since the last frame
sent, and always in the first frame. A NIL style colour means "the
default"; a NIL default means "the terminal's own".

**The cursor is frame state, and the display uses the terminal's real
cursor too (ncurses parity).** A frame ends with the primary cursor:
view, x, y, visible, and shape (`:box`, `:bar`, `:underline`, from
`lem-if:update-cursor-shape`). The display moves the terminal's cursor
there, shaped and shown or hidden accordingly. The cell underneath is
still painted with a `cursor` style, so the theme's cursor colour is
unchanged. That brings the cursor shape, blinking, and the IME and
accessibility behaviour that come with a real cursor.

**The relay decides character width.** A `text-put` carries the `width`
Lem laid the text out as occupying (`lem:string-width`). The display
places the run at its `x` and fits it to exactly `width` cells, clipping
or padding. A width disagreement for an emoji or an ambiguous-width
character then stays inside that run instead of shifting the rest of
the line. This settles 0011's first open question.

**Stacking order is explicit.** `views-stacked` lists view ids bottom
first. The relay builds it from Lem's window lists at each update, and
the session sends it only when it changed. The display stops guessing
from kinds and creation order.

**A frame is one `lem-if:update-display`:** the ops since the last one,
in order, then the cursor and, if they changed, the default colours.
View lifecycle calls that Lem makes outside a redraw queue up and ride
in the next frame. Envelope messages are not ops: the handshake, input,
clipboard and exit travel alongside frames (0011).

**Suppression lives in the session, below every codec.** The session
decides:

- **A frame is dropped** when every op in it is a redundant
  `rest-cleared` (the region is already blank), and the cursor and the
  default colours are as last sent.
- **A modeline or stacking order identical to the last one sent** is
  not queued at all. Creating, resizing or deleting a view forgets its
  modeline.
- **Paint invalidates what the session remembers as blank,** before the
  frame's own `rest-cleared` is recorded, because Lem draws lines and
  blanks the remainder after.

These are the rules `lisp/frame.lisp` and `lisp/modeline.lisp` apply
today as overrides, with one simplification: one last cursor instead of
one per view. A session made with `:suppress nil` sends everything. When the relay's
`lem-if` methods land ([plan](../relay-plan.md) step 4), they create
such a session under `LEM_RATATUI_DEBUG`, for diagnosis.

**One session per connection, used only from Lem's editor thread,**
where every `lem-if` drawing call runs. It is not thread-safe, and
nothing outside the editor thread touches it. In particular, a clipboard
reply arrives on the relay's reader thread and goes straight to the
`lem-if:clipboard-paste` call waiting for it, as `lem-server` does with
`*clipboard-wait-queue*`. It cannot go through `lem:send-event`: the
editor thread is blocked inside that very call and would not process
its event queue until the wait timed out.

## Consequences

- The JSON codec is a direct mapping: a style becomes the attribute
  object `lem-server` sends, with no table to consult.
- The phase 2 encoder owns interning, and a session-long style table,
  unbounded as ncurses' pair table effectively is. If distinct styles
  ever grow without limit (colour previews, rainbow modes), a
  reset-the-table message is additive, not a `v2`.
- The display gains work it skips today: honouring default colours, the
  hardware cursor and its shape, fitting runs to `width`, and ordering
  by `views-stacked`. Each is small, and each is a visible fidelity gain
  over the PoC.
- `relay/frame.lisp` implements this model, and its 19 Rove tests pin
  each rule above.

## Follow-ups outside this plan

- **Italic.** Ratatui renders it, but Lem's attribute has no italic slot
  (`src/attribute.lisp:5`). `document-italic-attribute` is defined only
  by a colour for that reason. Real italic needs a core change: a slot
  or a documented `:italic` plist key, plus `define-attribute` support
  and themes that use it. That benefits SDL2 and webview too, so it
  belongs upstream. The relay side is one more style flag.
