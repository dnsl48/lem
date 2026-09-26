# 0014. The display describes keys; the relay names them

**Status:** Accepted — September 2026

## Context

Lem names keys its own way: `"Return"` not Enter, `"Space"` for the space
bar, `"F5"`, `"PageUp"`, and a key that inserts a character is named by
that character (`src/key.lisp`). Shift has rules of its own: it is
dropped from self-inserting keys, except that meta+shift+a is `M-A`.

Today the display does the naming. `rust/crates/lem-ratatui/src/input.rs`
maps crossterm's key codes to Lem's names and sends them, and the relay
applies the shift rules (`lem-relay/input:key-event`). So two halves in
two languages each hold part of Lem's key conventions.

[0013](0013-icons-and-mouse-input.md) made the relay the adapter between
a display and Lem for mouse input: the display reports what happened,
and the relay turns it into what Lem consumes. The same reasoning
applies to keys.

## Decision

**The schema's `Key` describes the key neutrally, and the relay maps it
to Lem's name.**

```proto
message Key {
  oneof code {
    string text = 1;        // the character typed: "a", "A", "é", " "
    NamedKey named = 2;     // ENTER, TAB, BACKSPACE, the arrows, HOME, ...
    uint32 function = 3;    // F0 to F24, as Lem has them
  }
  repeated Modifier modifiers = 4;
}
```

`NamedKey` has exactly Lem's non-character keys (`src/key.lisp:4`), except
`NopKey`, which is Lem-internal. Space is text `" "`, which the relay
already names `Space`. Shift+Tab is `TAB` with shift. The relay's table
from `NamedKey` to Lem's names sits beside its shift rules, in the
protobuf codec's decoding.

**Decoding the terminal stays in the display.** crossterm reports the
control bytes 0x1C-0x1F as Ctrl+4..7 where the terminal meant `C-\`,
`C-]`, `C-^` and `C-_`. Knowing what bytes a terminal sent is the
display's job, so it corrects that before describing the key, whatever
the protocol says about names.

## Consequences

- Lem's key conventions live in one place, in Lisp beside the rest of
  Lem. A new display only has to say which key was pressed.
- The display's `input.rs` loses its name table in phase 2 and maps
  crossterm's codes to `Key` instead: a closer, simpler mapping.
- Today's JSON wire carries Lem's names, so `lem-relay/json` is
  unchanged. The mapping belongs to `lem-relay/protobuf`.
- Modifiers are a repeated enum rather than booleans. That avoids a
  field named `super`, which is a Rust keyword, and leaves room for
  Hyper, which Lem's SDL2 frontend supports.
