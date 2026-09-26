# 0017. Font styles and links in Lem's attributes

**Status:** Accepted — September 2026
**Supersedes:** [0015](0015-font-styles-without-touching-lem.md)
(property-list keys and `*attribute-font-styles*`).
**Revises:** [0016](0016-underline-styles.md) (where the underline style
comes from).

## Context

0015 kept font styles out of Lem core by carrying them in an attribute's
property list, and 0016 added the underline style the same way; a first
draft of hyperlinks carried the URL there too. That rested on an accident: the property list survives
merging, and Lem ignores it when comparing attributes. It turned out Lem
ignores it in three places that decide what is drawn:

- `attribute-equal`, which decides whether neighbouring drawing objects
  merge (`drawing-object-mergable-p`): an italic run beside plain text of
  its colour merged into it and lost its italic; a link merged into
  look-alike text lost its URL, or lent it to the neighbour;
- the same, deciding whether a line changed since it was drawn
  (`drawing-object-equal`);
- `item-content-hash`, the line fingerprint cache: a line whose only
  change was a style or a link was not redrawn.

That draft worked around the first two with `:around` methods on two
unexported Lem generics. The third was never covered. Every new property
would need the same care, invisibly to anyone else reading Lem.

We now accept carrying a patch to Lem core, provided it is small, in its
own commits, and written to go upstream: it is a feature any frontend
can use, not a ratatui workaround.

## Decision

**Font styles and the link are slots of Lem's attribute.**

- **Core** (`src/attribute.lisp`): `attribute` gains `underline-style`
  (NIL or `:straight`, `:curly`, `:dotted`, `:dashed`, `:double`),
  `italic`, `strikethrough`, `dim` and `link` (a URL). Each has a reader
  and a `set-attribute-…` setter, is accepted by `make-attribute` and
  `define-attribute`, is merged by `merge-attribute` (over wins), and is
  compared by `attribute-equal` and hashed by `item-content-hash`. So
  merging, the line cache and the fingerprint all see them, and nothing
  of Lem's drawing needs to know which frontend reads what.
- **`set-attribute`**, which color themes use, sets the new slots only
  when given. `apply-theme` rebuilds every attribute from its definition
  first, so a theme that names only an attribute's colours keeps the font
  styles its definition gives it, and switching themes drops a style the
  previous theme set.
- **Defaults in core:** `document-italic-attribute` is italic;
  `compiler-note-attribute` and lsp-mode's four diagnostic attributes
  have a curly underline. Frontends that cannot draw a style ignore it.
- **Two commits, each upstreamable:** font styles, then links, with
  tests in `tests/attribute.lisp`.
- **The relay** reads the slots (`attribute-style`, `relayed-link`), and
  has no property-list keys and no `mark-font-styles`. It keeps its own check on URLs before they reach the wire.
- **`lem-ratatui`** loses `font-styles.lisp` and
  `*attribute-font-styles*`. `links.lisp` puts overlays whose attribute
  is `(make-attribute :link url)`.

## Consequences

- One thing to know for italic comments or similar in `init.lisp`:
  redefine the attribute, `(define-attribute lem:syntax-comment-attribute
  (:dark :foreground "chocolate1" :italic t))`, or give the style in a
  theme. A bare `set-attribute-italic` is undone by the next theme load,
  as `set-attribute-bold` always was.
- We carry a patch to four of Lem's files: `src/attribute.lisp`,
  `src/internal-packages.lisp`, `src/display/physical-line.lisp` and
  `extensions/lsp-mode/lsp-mode.lisp`, plus a new
  `tests/attribute.lisp`. In the last year upstream committed to them 2,
  29, 7 and 5 times. `internal-packages.lisp` is the busy one, but our
  lines there sit inside the attribute exports, which rarely change.
  Until upstream takes the patch, a pull can conflict in these files; the
  conflicts would be small and local.
- The relay no longer names anything Lem does not export.
- Other frontends can now draw italic and the rest (ncurses has
  `A_ITALIC`, the browser has CSS); none does yet.
- The protocol and the display are unchanged: `Style` and `Put.link`
  carry the same fields as before.
