# 0016. Underline styles, and our own terminal output

**Status:** Accepted — September 2026; revised by
[0017](0017-font-styles-and-links-in-lem-attributes.md): the underline
style is a slot of Lem's attribute, not a property-list key.
**Revises:** [0002](0002-ratatui-core-over-full-ratatui.md): `ratatui-core`
stays; `ratatui-crossterm` and Ratatui's `Terminal` go.

## Context

Lem underlines LSP diagnostics: `diagnostic-error-attribute` and its three
siblings are a foreground colour with `:underline t`
(`extensions/lsp-mode/lsp-mode.lisp:677`). Lisp mode's compiler notes are
a red underline (`compiler-note-attribute`, `src/attribute.lisp:209`). Most
editors draw diagnostics with a curly underline, and most current
terminals can: `SGR 4:3`, with `4:2` double, `4:4` dotted and `4:5`
dashed. Lem's attributes have no underline style, but they carry a
property list that survives merging and theme loads
([0015](0015-font-styles-without-touching-lem.md)).

Ratatui cannot draw them. Its cell has one `UNDERLINED` flag. Its
`Terminal` redraws only cells whose `Cell` changed, so a cell going from a
straight underline to a curly one would never be redrawn. crossterm can
write the styles. Encoding them in Ratatui flags we do not otherwise use
(blink, hidden) was rejected: it makes those flags mean something they do
not, for anyone reading the code later.

Terminal support is uneven. kitty, WezTerm, foot, Alacritty, Ghostty,
iTerm2 and VTE-based terminals (0.52 and later) draw the styles. The Linux
console and older terminals do not, and may drop the underline
altogether. tmux passes them through only when configured to.

## Decision

**The protocol carries the style; the display decides what the terminal
can show.**

- **Schema:** `Style` gains `UnderlineStyle underline_style`, with
  `STRAIGHT` (0, the default), `CURLY`, `DOTTED`, `DASHED` and `DOUBLE`.
  It is an addition within v1. It states what Lem wants.
- **Relay:** reads `:underline-style` from the attribute's property list
  (`lem-relay/frame:*underline-styles*`). `mark-font-styles` takes
  `(attribute (:underline-style :curly))`. `lem-ratatui` marks
  `lem:compiler-note-attribute` and lsp-mode's four diagnostic attributes
  curly by default.
- **The display keeps the underline style beside Ratatui's buffer, not
  in it.** A `Layer` (`paint.rs`) is a Ratatui `Buffer` plus a grid of
  underline styles the same size, and painting, clearing and compositing
  keep the two in step.
- **The display writes to the terminal itself** (`present.rs`), in place
  of Ratatui's `Terminal` and `ratatui-crossterm`'s backend. A cell is
  redrawn when Ratatui's `Buffer::diff` reports it *or* its underline
  style changed. A cell covered by a wide character is never a position
  of its own, so it cannot split that character. The writing follows
  `ratatui-crossterm`'s: move only when needed, change only what
  differs. The display now tracks the screen size from resize events,
  which `Terminal` used to do.
- **Terminal support is decided by identity, with an override**
  (`support.rs`). It uses `TERM`, `TERM_PROGRAM` and `VTE_VERSION`
  against the terminals above. tmux and screen count as unsupported.
  `LEM_RATATUI_UNDERCURL=1` or `0` settles it either way. Where
  unsupported, every style is drawn straight, which is what Lem looked
  like before.

## Consequences

- LSP diagnostics and compiler notes are curly in terminals that draw it,
  and underlined as before elsewhere. Themes and users can use every
  style through `:plist '(:underline-style :dotted)` or the
  `*attribute-font-styles*` list.
- **lsp-mode's diagnostic attributes are not exported.** `lem-ratatui`
  finds them by name when the theme loads (`lsp-diagnostic-entries`), so
  a build without lsp-mode, or a rename upstream, skips them rather than
  failing to load. It is the one place this frontend names a symbol that
  another system does not export. That is deliberate and confined to
  this default.
- We own about 150 lines of terminal output that Ratatui used to supply.
  They are tested without a terminal: the exact bytes of every underline
  style and the straight fallback, a style-only change being redrawn,
  unchanged frames writing no cells, wide characters kept whole, cursor
  placement and shape, and a cleared screen on resize. Acceptance check
  12 runs a real session both ways.
- Ratatui's buffer, with its grapheme, width and clipping handling, and
  its diff, stay. If Ratatui gains underline styles, `present.rs` and
  the grid can go, and `Terminal` can come back.
- Hyperlinks (OSC 8) stay deferred (0015). They would ride on the same
  presenter, but need a URL with the text, not a style. (Done: 0018.)
