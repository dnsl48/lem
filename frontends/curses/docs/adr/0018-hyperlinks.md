# 0018. Web addresses as terminal hyperlinks

**Status:** Accepted — September 2026
**Depends on:** [0017](0017-font-styles-and-links-in-lem-attributes.md),
which gives Lem's attribute a `link`.

## Context

Most current terminals make hyperlinks of OSC 8
(`ESC ] 8 ; params ; URL ESC \` … `ESC ] 8 ; ; ESC \`): the text shows the
URL on hover, can be copied as a link, and opens with the terminal's own
opener on the terminal's own machine. ADR 0015 and 0016 deferred this: a
hyperlink is a URL with the text, not a style.

What reaches a frontend is drawing objects: text and an attribute, with
no buffer position and no text properties. So the relay cannot look a
URL up when it draws. It needs the URL to arrive in the attribute, which
0017 makes possible: Lem's attribute has a `link` slot, merged, compared
and hashed like its colours.

Lem has links of its own, but not everywhere. `link-mode` (`src/ext/link.lisp`),
which puts a `:link` text property on each address and makes it
clickable, is enabled per buffer: by lisp-mode, shell-mode, and the
buffer current at startup. Markdown puts `:link` only when tree-sitter is
not there to highlight it. Reading `:link` would link almost nothing.

Overlays are merged over a line's attributes when it is drawn.

## Decision

**Carry the URL in an invisible overlay's attribute; the protocol carries
it with the text; the display writes OSC 8 where the terminal makes
hyperlinks.**

- **Finding addresses** (`lem-ratatui`, `lisp/links.lisp`). A function on
  `lem:after-syntax-scan-hook`, globally, finds `http://` and `https://`
  addresses in the lines Lem has just scanned, in every buffer. It trims
  trailing punctuation and unmatched closing brackets, so
  `[text](https://x.org)` and `(see https://x.org).` link `https://x.org`.
  Lem scans what is shown, when it is shown or edited, so this costs
  what the lines on screen cost. `lem-ratatui/links:*links*` turns it off.
- **Carrying them.** Over each address it puts an overlay whose
  attribute sets only the link, `(make-attribute :link url)`, replacing
  the ones on the rescanned lines. Lem merges it over the text's own
  attributes, so the text looks as it did, in every theme, and linked
  text is never merged into its unlinked neighbours.
- **Schema:** `Put` gains `optional string link = 7`. It is an addition
  within v1. It belongs to the text, not to its `Style`: styles are
  interned for the session, and a URL is not a look. Modeline runs have
  none.
- **Relay:** `relayed-link` reads the link of the drawn object's
  attribute. A URL that is empty, longer than 2048 characters, or holds
  anything but printable ASCII is no link: it comes from buffer text, and
  an escape or a bell in it would end the sequence and reach the
  terminal as a command. The display checks the same again before
  writing it (`paint::link_of`).
- **Display:** a `Layer` keeps a link per cell beside its underline
  styles (`paint::Grid`), and compositing, clearing and repainting keep
  them in step. A border or separator drawn over linked text is not
  linked. The presenter redraws a cell whose link alone changed, opens a
  link where a linked run starts, closes it where it ends and at the end
  of every frame, so nothing else is ever linked. The `id` parameter is
  the URL's hash, so an address wrapped over two rows is one link to the
  terminal.
- **Terminal support** (`support::hyperlinks`), as for underline styles:
  by identity, with an override. kitty, WezTerm, foot, Alacritty, Ghostty,
  Contour, iTerm2, VTE 0.50 and later, Konsole 20.12 and later, and
  Windows Terminal. tmux and screen count as unsupported: tmux 3.4 passes
  links on only with its `hyperlinks` feature. `LEM_RATATUI_HYPERLINKS=1`
  or `0` settles it either way. Where unsupported, nothing is written and
  the text is drawn as before.

## Consequences

- Web addresses in any buffer are hyperlinks in terminals that make them.
  The mouse is captured, so a plain click still goes to Lem, which opens
  a `link-mode` link itself; the terminal's opener takes its bypass
  modifier, usually Shift (Ctrl in VTE).
- The link text of a Markdown `[text](url)` is not linked, only the
  address: tree-sitter's highlighting does not say what a link's text
  points at. File links (`path:line`) are left to Lem's own click, which
  opens them in Lem.
- Overlays are buffer points, which Lem moves on every edit. A buffer
  showing thousands of addresses would pay for that; the workload
  measurement is unchanged (ADR 0011).
- Tested without a terminal: the relay's reading and rejection of URLs,
  the codec leaving `link` unset (Lem's merging and comparing of links is
  tested in `tests/attribute.lisp`); the
  display's escape bytes, a change of link alone, the fallback, a
  wrapped address keeping one id, borders never linked; a link in the
  golden session. Acceptance check 13 runs a real session both ways.
