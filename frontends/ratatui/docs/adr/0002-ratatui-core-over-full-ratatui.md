# 0002. Depend on `ratatui-core`, not full Ratatui

**Status:** Accepted — September 2026

## Context

The obvious framing — "build a Ratatui frontend" — does not survive
contact with how Lem draws.

Lem owns its own window tree and its own layout, and resolves both before
emitting a single notification. It then hands each physical line to
`lem-if:render-line` as a list of drawing objects. The display half
receives absolutely-positioned paint commands in cell coordinates. **Lem
would use none of Ratatui's widget library and none of its layout
solver.** They are dead weight.

That invites the opposite conclusion — use bare `crossterm` — which is
also wrong, because three things still need a cell grid, none of which
are diffing:

1. **Compositing.** The browser composites overlapping per-view canvases
   by z-index. A terminal has one grid and composites nothing. Per-view
   buffers blitted in z-order is exactly `Buffer` + `Rect`.
2. **Style-run coalescing.** Naively mapping each `put` to
   `MoveTo`/`SetColors`/`Print` emits a colour-change pair per fragment.
   A cell grid lets style changes be emitted only where the style
   actually changes.
3. **Correctness around wide characters and clipping**, which `Buffer`
   already handles.

Diffing specifically is *not* on that list. Lem already diffs at line
granularity via a per-window fingerprint cache, so the incoming stream is
already a diff ([protocol-notes](../protocol-notes.md) section 6). A
cell-level diff is a second, finer pass over it — not useless, but not
the reason to take the dependency.

Ratatui split itself into separate crates in 0.30. `ratatui-core`
provides `backend`, `buffer`, `layout`, `style`, `symbols`, `terminal`,
`text`, and a `widgets` module holding **only** the `Widget` and
`StatefulWidget` traits. The widget library proper lives in
`ratatui-widgets`, which we simply do not list.

Alternatives weighed:

- **`crossterm` alone** — hand-roll the cell grid, diff and style
  coalescing. 300-500 lines of exactly the code `ratatui-core` has
  already tested.
- **`termwiz::surface::Surface`** — a cell surface with built-in change
  diffing, written for WezTerm, so it solves precisely this problem
  rather than a superset. Thinner docs and a smaller community. Worth
  revisiting if the escape-sequence layer turns out to be where the pain
  is.

## Decision

Depend on **`ratatui-core` + `ratatui-crossterm`**. Do not depend on
`ratatui` or `ratatui-widgets`.

## Consequences

- The name "Ratatui frontend" is a mild misnomer, and we should say so
  plainly rather than let reviewers discover it: this is a crossterm
  frontend that borrows Ratatui's cell buffer. It is not built out of
  Ratatui widgets and never will be by default.
- We get `Buffer`, `Cell`, `Style`, the `Backend` trait and `Terminal`'s
  diff-and-flush for roughly the cost of the crossterm dependency.
- The door stays open. If we later want certain views reinterpreted as
  native widgets — rendering a floating completion view as a real list
  with borders instead of painting the cells Lem sent — adding
  `ratatui-widgets` is a one-line change. That idea needs protocol work
  first, though: `make-view` says `kind: 'floating'` and carries
  `borderShape`, but never says *what* the floating window is for.
- If the escape-sequence emission layer becomes the bottleneck, `termwiz`
  is the first alternative to evaluate, not bare `crossterm`.
