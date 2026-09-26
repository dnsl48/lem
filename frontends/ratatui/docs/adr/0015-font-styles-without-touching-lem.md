# 0015. Font styles without touching Lem: italic, strikethrough, dim

**Status:** Accepted — September 2026
**Supersedes:** [0012](0012-the-relay-frame-model.md)'s follow-up, "Italic
… needs a core change".

## Context

0012 recorded that italic needs a change to Lem core: its attribute class
has slots for foreground, background, reverse, bold and underline only
(`src/attribute.lisp:5`). We do not want to carry patches to Lem's own
files. Every pull from upstream would risk a merge conflict, and upstream
patches are not a priority now.

A closer look at the class shows a way round:

- **Every attribute has a free-form property list** (`plist`), read and
  written with the exported `lem:attribute-value` and its `setf`. The
  cursor flag already rides in it.
- **It survives merging.** `merge-attribute` appends both attributes'
  property lists (`src/attribute.lisp:78`), so a syntax attribute's flag
  outlives being merged with the region's, say.
- **It survives the colours of a theme load.** `apply-theme` clears the
  attribute objects Lem caches, and `set-attribute` then overwrites only
  the five slots (`src/attribute.lisp:97`). The objects are rebuilt from
  their `define-attribute` definitions, so a flag set *on an object*
  does not survive; a flag *in the definition* (`:plist '(:italic t)`)
  does.
- **Lem runs `lem:*after-load-theme-hook*`** after each theme load,
  including the one at startup. It is exported, and vi-mode uses it.

Which styles, beyond italic? What our stack can draw and what Lem asks
for:

| | Ratatui | Lem today |
|---|---|---|
| italic | `ITALIC` | `document-italic-attribute` (Markdown emphasis, tree-sitter's `markup.italic`), drawn as a colour only |
| strikethrough | `CROSSED_OUT` | nothing yet; LSP defines deprecated tags meant to be struck through, which Lem's client does not use |
| dim | `DIM` | nothing; themes use colours |
| curly, dotted, dashed, double underline | no: `ratatui-core` has one underline flag; crossterm could, bypassing Ratatui | LSP diagnostics' coloured underlines |
| hyperlinks (OSC 8) | no | `document-link-attribute`, colour only; would need the URL with the text |

## Decision

**Italic, strikethrough and dim, carried in the attribute's property
list, all within `frontends/ratatui`.**

- **The keys are `:italic`, `:strikethrough` and `:dim`** in a Lem
  attribute's property list (`lem-relay/frame:*font-style-keys*`). The
  relay reads them into the style. The schema's `Style` gains
  `italic`, `strikethrough` and `dim`, an addition within v1 that an older
  display ignores. The display maps them to Ratatui's modifiers.
- **Two ways to set them:**
  - **In an attribute's definition:** `(define-attribute my-attribute (t
    :foreground "…" :plist '(:italic t)))`. It survives everything,
    works in any frontend's init file, and other frontends ignore the key.
  - **For attributes defined elsewhere:**
    `lem-ratatui/font-styles:*attribute-font-styles*`, a list of
    `(attribute-name key…)`. `lem-ratatui` marks those attributes from
    `lem:*after-load-theme-hook*`, so each theme load re-marks what it
    rebuilt. The default is `((lem:document-italic-attribute :italic))`,
    the one attribute that exists for a font style. The marking itself,
    `lem-relay/frame:mark-font-styles`, is in the relay and tested there.
- **After marking, the hook forces a redraw.** Lem's line cache compares
  attributes with `attribute-equal`, which ignores property lists, so
  lines already on screen would keep their old look.
- **`#+lem-ratatui`** is on `*features*` in our image, so an `init.lisp`
  shared with other frontends can add to the list without naming a
  package they do not have.
- **Underline styles and hyperlinks are deferred.** Both need display
  work outside Ratatui's model, not a flag. Both can join v1 later without
  breaking anything: an optional underline style on `Style`, an optional
  URL on `Put`.

## Consequences

- No file outside `frontends/ratatui` changes, and no Lem function is
  replaced. We rely only on exported API: `attribute-value`,
  `*after-load-theme-hook*`, `define-attribute`'s `:plist`. Pulling
  upstream stays free of conflicts. The tests would catch a change to
  those APIs.
- Markdown emphasis is italic out of the box. Acceptance check 11 opens a
  Markdown file and, replayed through a terminal emulator, "emphasis" is
  italic cell by cell and the text around it is not.
- If an extension re-evaluates a `define-attribute` after the theme has
  loaded, that attribute loses a mark from the list until the next theme
  load. Marks in a definition's `:plist` are not affected.
- Lem's built-in terminal emulator (`extensions/terminal`) reads italic,
  strikethrough and more from the programs inside it, but keeps only
  bold, underline and reverse when it builds attributes
  (`extensions/terminal/terminal.lisp:336`). Passing them on is a change
  to that extension, upstream; the relay would carry them unchanged.
