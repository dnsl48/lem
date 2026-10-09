# termina Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement ADR 0021: the display reads the terminal with termina instead of a vendored, patched crossterm, and writes every output byte itself.

**Architecture:** Output first, input second, deletion last. Task 1 moves every escape sequence the display writes into a module of our own (`ansi.rs`), byte-identical to crossterm 0.29, proven by a temporary test that compares the two. Task 2 swaps input: termina provides raw mode, size, the panic-hook restore and events; keyboard negotiation sends its own query (`probe.rs`) and reads every event itself, setting aside non-replies for the main loop (`events::Input`), with C0 control keys decoded by the encoding in force when they were read (`input::legacy_control`). Task 3 deletes `rust/vendor/crossterm`, adds the two acceptance checks that prove what the patch fixed stays fixed, and updates the docs.

**Tech Stack:** Rust 2024 (stable toolchain), termina 0.4 (default features), ratatui-core 0.1, prost; Python 3 standard library for the PTY acceptance harness.

**Spec:** [`adr/0021-termina-instead-of-a-patched-crossterm.md`](adr/0021-termina-instead-of-a-patched-crossterm.md). Read it before starting; the plan argues from it.

## Global Constraints

- Nothing outside `frontends/ratatui/` changes.
- termina is `termina = "0.4"` with default features: no `windows-legacy`, no `event-stream`.
- Every byte the display writes stays exactly what crossterm 0.29 wrote. The presenter, terminal and keyboard tests pin those bytes and must pass **unchanged**; `proto/fixtures/display-inputs.v1.bin` must not change (never run with `LEM_UPDATE_FIXTURES`).
- Keyboard negotiation keeps its one-second budget (`keyboard::BUDGET`), its confirmation query and its push/pop ownership rules.
- Supported platforms: Linux, macOS, Windows through WSL. Native Windows only has to keep compiling in principle; there is no Windows CI and no Windows-only code is added.
- Commit messages follow the repository's style: `feat(ratatui): …`, `refactor(ratatui): …`, `docs(ratatui): …`. Commits are signed by the local git configuration; pass no signing flags.
- Every existing check keeps passing: `make test`, `scripts/acceptance.py`, `scripts/keyboard-acceptance.py` (8 checks, 10 after Task 3).
- Rust: `cargo clippy --all-targets` adds no warnings; `cargo fmt` on every file you touch.

## How to run things

Run from `frontends/ratatui/` unless stated.

- Display tests: `(cd rust && cargo test -p lem-ratatui)`; one test: `(cd rust && cargo test -p lem-ratatui probe::)`.
- Lint and format: `(cd rust && cargo clippy --all-targets && cargo fmt)`.
- Keyboard acceptance (builds nothing itself): `(cd rust && cargo build -p lem-ratatui) && python3 scripts/keyboard-acceptance.py`.
- All unit tests: `make test`.
- Full build, then full acceptance. **`sbcl` is not on PATH on this machine**; use Roswell's:
  `SB=~/.roswell/impls/x86-64/linux/sbcl-bin/2.6.8; PATH="$SB/bin:$PATH" SBCL_HOME="$SB/lib/sbcl" make all && python3 scripts/acceptance.py`
- Task 2 fetches termina 0.4.0 from crates.io, so it needs network access once.

## Before you start

- You are on `feat/ratatui/261010`, and `git status --short` prints nothing. (When this plan was written, two uncommitted edits to `rust/vendor/crossterm` — an `unused_parens` fix and its note in `LEM-PATCH.md` — were in the tree. They belong to the patch Task 3 deletes; their owner commits or drops them before Task 1.)
- ADR 0021 and this plan are committed.

## Review Focus

1. **The terminal hangs up** (window closed, SSH dropped, `tmux kill-session`), with SIGHUP not ending the process. The display must exit within seconds, not spin at 100% CPU. Test: Task 3, keyboard acceptance `hangup`.
2. **Input that never completes an event** (a lone `ESC [`). Lem's messages must still be served at once, not wait for the next keystroke. Test: Task 3, keyboard acceptance `stall`.
3. **Keys typed during keyboard negotiation.** They must arrive in order, with raw C0 bytes read before the push named `C-\` … `C-_`, and CSI-u Ctrl+digits read during confirmation kept as digits. Tests: Task 2, `probe::tests::everything_else_is_set_aside_in_order_and_nothing_after_da1_is_read`, `keyboard::tests::set_aside_keys_are_decoded_by_when_they_were_read`, keyboard acceptance `enhanced with queued keys`.
4. **Undo and redo in a terminal without the kitty protocol** (VTE, default xterm, tmux): `C-\` (0x1C) and `C-_` (0x1F) must reach Lem by those names, after a fallback or an explicit `LEM_RATATUI_KEYBOARD=legacy`. Tests: Task 2, `events::tests::under_legacy_encoding_live_c0_keys_are_named`, keyboard acceptance `legacy` and `override`.
5. **The screen looks exactly as before.** Every colour, attribute, cursor and mode sequence must be byte-identical to crossterm's. Tests: Task 1, `ansi::same_as_crossterm` (temporary) and `ansi::tests` (permanent); the unchanged presenter tests.

---

## File map

```
frontends/ratatui/
├── rust/Cargo.toml                         Task 2: drop [patch.crates-io]; Task 3: drop exclude
├── rust/Cargo.lock                         Task 2: regenerated by cargo
├── rust/vendor/crossterm/                  Task 3: deleted
├── rust/crates/lem-ratatui/
│   ├── Cargo.toml                          Task 2: crossterm → termina
│   └── src/
│       ├── ansi.rs                         NEW (Task 1): every byte the display writes
│       ├── present.rs                      Task 1: writes through ansi
│       ├── term.rs                         Task 1: writes through ansi; Task 2: Guard over termina
│       ├── keyboard.rs                     Task 1: writes through ansi; Task 2: negotiation over probe
│       ├── input.rs                        Task 2: termina types, Encoding, legacy_control, decode
│       ├── probe.rs                        NEW (Task 2): the keyboard query, read on the input stream
│       ├── events.rs                       NEW (Task 2): Events, Input, test Script
│       ├── golden.rs                       Task 2: termina types
│       └── main.rs                         Task 1: mod ansi; Task 2: mods, input loop, size
├── scripts/keyboard-acceptance.py          Task 2: one byte sequence; Task 3: hangup, stall
├── lisp/implementation.lisp                Task 3: comment
├── display-relay/tests/golden.lisp         Task 3: comment
└── README.md                               Task 3: crossterm → termina
```

---

### Task 1: The display writes its own escape sequences

**Files:**
- Create: `rust/crates/lem-ratatui/src/ansi.rs`
- Modify: `rust/crates/lem-ratatui/src/main.rs` (module list), `src/present.rs`, `src/term.rs`, `src/keyboard.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces (`crate::ansi`), used by Tasks 2 and 3:
  - `pub const ENTER: &str`, `pub const LEAVE: &str`, `pub const POP_KEYBOARD: &str`, `pub const HIDE_CURSOR: &str`, `pub const SHOW_CURSOR: &str`, `pub const CLEAR: &str`
  - `pub fn push_keyboard(flags: u8) -> String`
  - `pub fn move_to(out: &mut impl Write, x: u16, y: u16) -> io::Result<()>`
  - `pub fn cursor_shape(shape: CursorShape) -> &'static str`
  - `pub enum Sgr { Reset, Bold, Dim, Italic, Underlined, DoubleUnderlined, Undercurled, Underdotted, Underdashed, SlowBlink, RapidBlink, Reverse, Hidden, CrossedOut, NormalIntensity, NoItalic, NoUnderline, NoBlink, NoReverse, NoHidden, NotCrossedOut }` and `pub fn sgr(out: &mut impl Write, attribute: Sgr) -> io::Result<()>`
  - `pub fn foreground / background / underline_color(out: &mut impl Write, color: ratatui_core::style::Color) -> io::Result<()>`

- [ ] **Step 1: Write `ansi.rs` with its tests**

Create `rust/crates/lem-ratatui/src/ansi.rs`:

````rust
//! Every byte the display writes to the terminal (ADR 0016, 0021).
//!
//! These are exactly the sequences crossterm 0.29 wrote, so leaving
//! crossterm changes nothing on the screen. Reading the terminal's
//! replies is `probe.rs`'s job, not this module's.

use std::io::{self, Write};

use lem_protocols::display::v1::CursorShape;
use ratatui_core::style::Color;

/// Take the screen: the alternate screen, the cursor hidden, the mouse
/// reported with SGR coordinates (ADR 0013), pastes bracketed.
pub const ENTER: &str = concat!(
    "\x1b[?1049h",
    "\x1b[?25l",
    "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h",
    "\x1b[?2004h",
);

/// Give the screen back: [`ENTER`] undone in reverse, with the user's own
/// cursor shape.
pub const LEAVE: &str = concat!(
    "\x1b[?2004l",
    "\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l",
    "\x1b[0 q",
    "\x1b[?25h",
    "\x1b[?1049l",
);

/// Pop the one entry [`push_keyboard`] pushed onto the terminal's stack.
pub const POP_KEYBOARD: &str = "\x1b[<1u";

pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const CLEAR: &str = "\x1b[2J";

/// Push kitty keyboard `flags` (their bits) onto the terminal's stack.
pub fn push_keyboard(flags: u8) -> String {
    format!("\x1b[>{flags}u")
}

/// Move the cursor to column `x`, row `y`, both counted from 0.
pub fn move_to(out: &mut impl Write, x: u16, y: u16) -> io::Result<()> {
    write!(out, "\x1b[{};{}H", u32::from(y) + 1, u32::from(x) + 1)
}

/// The steady cursor shape for Lem's.
pub fn cursor_shape(shape: CursorShape) -> &'static str {
    match shape {
        CursorShape::Bar => "\x1b[6 q",
        CursorShape::Underline => "\x1b[4 q",
        CursorShape::Box | CursorShape::Unspecified => "\x1b[2 q",
    }
}

/// SGR attributes, named as crossterm named them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sgr {
    Reset,
    Bold,
    Dim,
    Italic,
    Underlined,
    DoubleUnderlined,
    Undercurled,
    Underdotted,
    Underdashed,
    SlowBlink,
    RapidBlink,
    Reverse,
    Hidden,
    CrossedOut,
    NormalIntensity,
    NoItalic,
    NoUnderline,
    NoBlink,
    NoReverse,
    NoHidden,
    NotCrossedOut,
}

impl Sgr {
    fn code(self) -> &'static str {
        match self {
            Self::Reset => "0",
            Self::Bold => "1",
            Self::Dim => "2",
            Self::Italic => "3",
            Self::Underlined => "4",
            Self::DoubleUnderlined => "4:2",
            Self::Undercurled => "4:3",
            Self::Underdotted => "4:4",
            Self::Underdashed => "4:5",
            Self::SlowBlink => "5",
            Self::RapidBlink => "6",
            Self::Reverse => "7",
            Self::Hidden => "8",
            Self::CrossedOut => "9",
            Self::NormalIntensity => "22",
            Self::NoItalic => "23",
            Self::NoUnderline => "24",
            Self::NoBlink => "25",
            Self::NoReverse => "27",
            Self::NoHidden => "28",
            Self::NotCrossedOut => "29",
        }
    }
}

pub fn sgr(out: &mut impl Write, attribute: Sgr) -> io::Result<()> {
    write!(out, "\x1b[{}m", attribute.code())
}

pub fn foreground(out: &mut impl Write, color: Color) -> io::Result<()> {
    set_color(out, 38, color)
}

pub fn background(out: &mut impl Write, color: Color) -> io::Result<()> {
    set_color(out, 48, color)
}

pub fn underline_color(out: &mut impl Write, color: Color) -> io::Result<()> {
    set_color(out, 58, color)
}

/// SGR `set` with `color`, or `set + 1`, its reset. Named colours are
/// palette indices, as `ratatui-crossterm` mapped them: `Red` is ANSI
/// red, `LightRed` bright red.
fn set_color(out: &mut impl Write, set: u8, color: Color) -> io::Result<()> {
    let index = match color {
        Color::Reset => return write!(out, "\x1b[{}m", set + 1),
        Color::Rgb(r, g, b) => return write!(out, "\x1b[{set};2;{r};{g};{b}m"),
        Color::Indexed(index) => index,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    write!(out, "\x1b[{set};5;{index}m")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(write: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut out = Vec::new();
        write(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn named_colours_are_palette_indices() {
        for (color, expected) in [
            (Color::Black, "\x1b[38;5;0m"),
            (Color::Red, "\x1b[38;5;1m"),
            (Color::Gray, "\x1b[38;5;7m"),
            (Color::DarkGray, "\x1b[38;5;8m"),
            (Color::LightRed, "\x1b[38;5;9m"),
            (Color::White, "\x1b[38;5;15m"),
            (Color::Indexed(200), "\x1b[38;5;200m"),
            (Color::Rgb(1, 2, 3), "\x1b[38;2;1;2;3m"),
            (Color::Reset, "\x1b[39m"),
        ] {
            assert_eq!(written(|out| foreground(out, color)), expected, "{color:?}");
        }
    }

    #[test]
    fn background_and_underline_colours_have_their_own_codes() {
        assert_eq!(written(|out| background(out, Color::Blue)), "\x1b[48;5;4m");
        assert_eq!(written(|out| background(out, Color::Reset)), "\x1b[49m");
        assert_eq!(
            written(|out| underline_color(out, Color::Rgb(9, 8, 7))),
            "\x1b[58;2;9;8;7m"
        );
        assert_eq!(written(|out| underline_color(out, Color::Reset)), "\x1b[59m");
    }

    #[test]
    fn underline_styles_use_the_colon_form() {
        for (attribute, expected) in [
            (Sgr::Underlined, "\x1b[4m"),
            (Sgr::DoubleUnderlined, "\x1b[4:2m"),
            (Sgr::Undercurled, "\x1b[4:3m"),
            (Sgr::Underdotted, "\x1b[4:4m"),
            (Sgr::Underdashed, "\x1b[4:5m"),
            (Sgr::NoUnderline, "\x1b[24m"),
        ] {
            assert_eq!(written(|out| sgr(out, attribute)), expected, "{attribute:?}");
        }
    }

    #[test]
    fn positions_count_from_one_and_never_overflow() {
        assert_eq!(written(|out| move_to(out, 0, 0)), "\x1b[1;1H");
        assert_eq!(written(|out| move_to(out, 2, 1)), "\x1b[2;3H");
        assert_eq!(
            written(|out| move_to(out, u16::MAX, u16::MAX)),
            "\x1b[65536;65536H"
        );
    }

    #[test]
    fn the_keyboard_stack_is_pushed_with_the_flag_bits() {
        assert_eq!(push_keyboard(5), "\x1b[>5u");
    }
}

/// Deleted with the crossterm dependency (docs/termina-plan.md, Task 2).
/// Until then, the proof that this module writes what crossterm wrote.
#[cfg(test)]
mod same_as_crossterm {
    use super::*;
    use crossterm::Command;
    use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
    use crossterm::event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    };
    use crossterm::style::{
        Attribute, Color as C, Colored, SetAttribute, SetBackgroundColor, SetForegroundColor,
        SetUnderlineColor,
    };
    use crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};

    fn theirs(command: impl Command) -> String {
        let mut out = String::new();
        command.write_ansi(&mut out).unwrap();
        out
    }

    fn ours(write: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut out = Vec::new();
        write(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// present.rs's mapping before this module replaced it.
    fn crossterm_color(color: Color) -> C {
        match color {
            Color::Reset => C::Reset,
            Color::Black => C::Black,
            Color::Red => C::DarkRed,
            Color::Green => C::DarkGreen,
            Color::Yellow => C::DarkYellow,
            Color::Blue => C::DarkBlue,
            Color::Magenta => C::DarkMagenta,
            Color::Cyan => C::DarkCyan,
            Color::Gray => C::Grey,
            Color::DarkGray => C::DarkGrey,
            Color::LightRed => C::Red,
            Color::LightGreen => C::Green,
            Color::LightBlue => C::Blue,
            Color::LightYellow => C::Yellow,
            Color::LightMagenta => C::Magenta,
            Color::LightCyan => C::Cyan,
            Color::White => C::White,
            Color::Indexed(i) => C::AnsiValue(i),
            Color::Rgb(r, g, b) => C::Rgb { r, g, b },
        }
    }

    #[test]
    fn colours() {
        // crossterm writes nothing for a colour when NO_COLOR is set.
        Colored::set_ansi_color_disabled(false);
        for color in [
            Color::Reset,
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
            Color::Indexed(0),
            Color::Indexed(255),
            Color::Rgb(0, 128, 255),
        ] {
            let c = crossterm_color(color);
            assert_eq!(ours(|out| foreground(out, color)), theirs(SetForegroundColor(c)));
            assert_eq!(ours(|out| background(out, color)), theirs(SetBackgroundColor(c)));
            assert_eq!(
                ours(|out| underline_color(out, color)),
                theirs(SetUnderlineColor(c))
            );
        }
    }

    #[test]
    fn attributes() {
        for (ours_, theirs_) in [
            (Sgr::Reset, Attribute::Reset),
            (Sgr::Bold, Attribute::Bold),
            (Sgr::Dim, Attribute::Dim),
            (Sgr::Italic, Attribute::Italic),
            (Sgr::Underlined, Attribute::Underlined),
            (Sgr::DoubleUnderlined, Attribute::DoubleUnderlined),
            (Sgr::Undercurled, Attribute::Undercurled),
            (Sgr::Underdotted, Attribute::Underdotted),
            (Sgr::Underdashed, Attribute::Underdashed),
            (Sgr::SlowBlink, Attribute::SlowBlink),
            (Sgr::RapidBlink, Attribute::RapidBlink),
            (Sgr::Reverse, Attribute::Reverse),
            (Sgr::Hidden, Attribute::Hidden),
            (Sgr::CrossedOut, Attribute::CrossedOut),
            (Sgr::NormalIntensity, Attribute::NormalIntensity),
            (Sgr::NoItalic, Attribute::NoItalic),
            (Sgr::NoUnderline, Attribute::NoUnderline),
            (Sgr::NoBlink, Attribute::NoBlink),
            (Sgr::NoReverse, Attribute::NoReverse),
            (Sgr::NoHidden, Attribute::NoHidden),
            (Sgr::NotCrossedOut, Attribute::NotCrossedOut),
        ] {
            assert_eq!(
                ours(|out| sgr(out, ours_)),
                theirs(SetAttribute(theirs_)),
                "{ours_:?}"
            );
        }
    }

    #[test]
    fn screen_cursor_and_keyboard() {
        let enter = [
            theirs(EnterAlternateScreen),
            theirs(Hide),
            theirs(EnableMouseCapture),
            theirs(EnableBracketedPaste),
        ];
        assert_eq!(ENTER, enter.concat());
        let leave = [
            theirs(DisableBracketedPaste),
            theirs(DisableMouseCapture),
            theirs(SetCursorStyle::DefaultUserShape),
            theirs(Show),
            theirs(LeaveAlternateScreen),
        ];
        assert_eq!(LEAVE, leave.concat());
        assert_eq!(HIDE_CURSOR, theirs(Hide));
        assert_eq!(SHOW_CURSOR, theirs(Show));
        assert_eq!(CLEAR, theirs(Clear(ClearType::All)));
        for (x, y) in [(0, 0), (2, 1), (79, 23), (1000, 500)] {
            assert_eq!(ours(|out| move_to(out, x, y)), theirs(MoveTo(x, y)));
        }
        for (shape, style) in [
            (CursorShape::Bar, SetCursorStyle::SteadyBar),
            (CursorShape::Underline, SetCursorStyle::SteadyUnderScore),
            (CursorShape::Box, SetCursorStyle::SteadyBlock),
            (CursorShape::Unspecified, SetCursorStyle::SteadyBlock),
        ] {
            assert_eq!(cursor_shape(shape), theirs(style));
        }
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
        assert_eq!(
            push_keyboard(flags.bits()),
            theirs(PushKeyboardEnhancementFlags(flags))
        );
        assert_eq!(POP_KEYBOARD, theirs(PopKeyboardEnhancementFlags));
    }
}
````

In `src/main.rs`, add `mod ansi;` as the first line of the module list (before `mod colors;`).

- [ ] **Step 2: Run the new tests**

Run: `(cd rust && cargo test -p lem-ratatui ansi::)`
Expected: PASS, 8 tests (5 in `ansi::tests`, 3 in `ansi::same_as_crossterm`). The module is not used yet, so `cargo build` warns about dead code until Step 3; that is expected for this step only.

- [ ] **Step 3: The presenter writes through `ansi`**

In `src/present.rs`, replace the `use` block (lines 17–34) with:

```rust
use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write};

use lem_protocols::display::v1::{CursorShape, UnderlineStyle};
use ratatui_core::buffer::Cell;
use ratatui_core::layout::Position;
use ratatui_core::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::ansi::{self, Sgr};
use crate::paint::{Layer, Link};
```

Replace `present` with:

```rust
    pub fn present(
        &mut self,
        frame: &Layer,
        cursor: Option<Position>,
        shape: Option<CursorShape>,
    ) -> io::Result<()> {
        self.out.write_all(ansi::HIDE_CURSOR.as_bytes())?;
        let previous = match self.previous.take() {
            Some(previous) if previous.area() == frame.area() => previous,
            // First frame, or a new size: start from a cleared screen,
            // which is what an empty layer describes.
            _ => {
                self.out.write_all(ansi::CLEAR.as_bytes())?;
                Layer::new(frame.area().width, frame.area().height)
            }
        };
        let positions = changed(&previous, frame);
        self.write_cells(frame, &positions)?;

        if shape != self.shape {
            if let Some(shape) = shape {
                self.out.write_all(ansi::cursor_shape(shape).as_bytes())?;
            }
            self.shape = shape;
        }
        if let Some(position) = cursor {
            ansi::move_to(&mut self.out, position.x, position.y)?;
            self.out.write_all(ansi::SHOW_CURSOR.as_bytes())?;
        }
        self.out.flush()?;
        self.previous = Some(frame.clone());
        Ok(())
    }
```

Replace `write_cells` and `underline_attribute` with:

```rust
    fn write_cells(&mut self, frame: &Layer, positions: &[(u16, u16)]) -> io::Result<()> {
        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut underline_color = Color::Reset;
        let mut modifier = Modifier::empty();
        let mut underline: Option<UnderlineStyle> = None;
        let mut link: Option<&Link> = None;
        let mut next: Option<(u16, u16)> = None;

        for &(x, y) in positions {
            let cell = &frame.cells[(x, y)];
            if next != Some((x, y)) {
                ansi::move_to(&mut self.out, x, y)?;
            }
            let wanted = cell.modifier - Modifier::UNDERLINED;
            if wanted != modifier {
                queue_modifier_diff(&mut self.out, modifier, wanted)?;
                modifier = wanted;
            }
            let wanted = cell
                .modifier
                .contains(Modifier::UNDERLINED)
                .then(|| *frame.underline.get(x, y));
            if wanted != underline {
                let attribute = self.underline_attribute(wanted);
                ansi::sgr(&mut self.out, attribute)?;
                underline = wanted;
            }
            if cell.fg != fg {
                ansi::foreground(&mut self.out, cell.fg)?;
                fg = cell.fg;
            }
            if cell.bg != bg {
                ansi::background(&mut self.out, cell.bg)?;
                bg = cell.bg;
            }
            let wanted = frame.link.get(x, y).as_ref().filter(|_| self.hyperlinks);
            if wanted != link {
                queue_hyperlink(&mut self.out, wanted)?;
                link = wanted;
            }
            if cell.underline_color != underline_color {
                ansi::underline_color(&mut self.out, cell.underline_color)?;
                underline_color = cell.underline_color;
            }
            self.out.write_all(cell.symbol().as_bytes())?;
            next = Some((x.saturating_add(cell_width(cell)), y));
        }
        if link.is_some() {
            queue_hyperlink(&mut self.out, None)?;
        }
        ansi::foreground(&mut self.out, Color::Reset)?;
        ansi::background(&mut self.out, Color::Reset)?;
        ansi::underline_color(&mut self.out, Color::Reset)?;
        ansi::sgr(&mut self.out, Sgr::Reset)
    }

    fn underline_attribute(&self, underline: Option<UnderlineStyle>) -> Sgr {
        match underline {
            None => Sgr::NoUnderline,
            Some(_) if !self.styled_underlines => Sgr::Underlined,
            Some(UnderlineStyle::Straight) => Sgr::Underlined,
            Some(UnderlineStyle::Curly) => Sgr::Undercurled,
            Some(UnderlineStyle::Dotted) => Sgr::Underdotted,
            Some(UnderlineStyle::Dashed) => Sgr::Underdashed,
            Some(UnderlineStyle::Double) => Sgr::DoubleUnderlined,
        }
    }
```

In `queue_modifier_diff`, replace every `queue!(out, SetAttribute(Attribute::X))?;` with `ansi::sgr(out, Sgr::X)?;`, keeping the same `X` (for example `queue!(out, SetAttribute(Attribute::NoReverse))?;` becomes `ansi::sgr(out, Sgr::NoReverse)?;`). There are 16 of them, all in that function, and nothing else there changes:

```bash
sed -i 's/queue!(out, SetAttribute(Attribute::\([A-Za-z]*\)))?;/ansi::sgr(out, Sgr::\1)?;/' \
    rust/crates/lem-ratatui/src/present.rs
grep -c 'ansi::sgr(out, Sgr::' rust/crates/lem-ratatui/src/present.rs   # 16
```

Delete the functions `crossterm_color` and `cursor_style` at the end of the non-test code. The `#[cfg(test)] mod tests` block does not change.

- [ ] **Step 4: The terminal guard and the keyboard stack write through `ansi`**

In `src/term.rs`, replace the crossterm `use` lines:

```rust
use crossterm::cursor::{self, SetCursorStyle};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
```
```rust
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
```

with:

```rust
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
```

and add `use crate::ansi;` beside `use crate::keyboard;`. In `Cleanup::restore`, replace the `execute!(…)` statement with:

```rust
        let screen = out
            .write_all(ansi::LEAVE.as_bytes())
            .and_then(|()| out.flush());
```

In `Guard::new_if_interactive`, replace the `execute!(&mut guard.tty, EnterAlternateScreen, cursor::Hide, EnableMouseCapture, EnableBracketedPaste)?;` statement with:

```rust
        guard.tty.write_all(ansi::ENTER.as_bytes())?;
        guard.tty.flush()?;
```

In `src/keyboard.rs`, replace the imports

```rust
use crossterm::Command;
use crossterm::event::{
    KeyboardEnhancementFlags as Flags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
```

with

```rust
use crossterm::event::KeyboardEnhancementFlags as Flags;
```

and add `use crate::ansi;` after the `lem_protocols` import. Change the push to `write_stack_command(out, pushed, &ansi::push_keyboard(requested.bits()), true)`, the pop in `pop_if_owned` to `write_stack_command(out, pushed, ansi::POP_KEYBOARD, false)`, and `write_stack_command` to:

```rust
fn write_stack_command(
    out: &mut impl Write,
    pushed: &AtomicBool,
    command: &str,
    owned_after_write: bool,
) -> io::Result<()> {
    // write_all completing marks the complete terminal command precisely,
    // separately from the subsequent flush result.
    out.write_all(command.as_bytes())?;
    pushed.store(owned_after_write, Ordering::Release);
    out.flush()
}
```

- [ ] **Step 5: Run everything that pins the bytes**

Run: `(cd rust && cargo test -p lem-ratatui && cargo clippy --all-targets && cargo fmt)`
Expected: all tests PASS, the presenter, `term` and `keyboard` tests unchanged; clippy prints no warnings.

Run: `(cd rust && cargo build -p lem-ratatui) && python3 scripts/keyboard-acceptance.py`
Expected: `8/8 keyboard acceptance checks passed`.

- [ ] **Step 6: Commit**

```bash
git add rust/crates/lem-ratatui/src/ansi.rs rust/crates/lem-ratatui/src/main.rs \
        rust/crates/lem-ratatui/src/present.rs rust/crates/lem-ratatui/src/term.rs \
        rust/crates/lem-ratatui/src/keyboard.rs
git commit -m "refactor(ratatui): the display writes its own escape sequences"
```

---

### Task 2: Read the terminal with termina

**Files:**
- Create: `rust/crates/lem-ratatui/src/probe.rs`, `rust/crates/lem-ratatui/src/events.rs`
- Modify: `rust/Cargo.toml`, `rust/crates/lem-ratatui/Cargo.toml`, `src/ansi.rs`, `src/input.rs`, `src/golden.rs`, `src/keyboard.rs`, `src/term.rs`, `src/main.rs`, `scripts/keyboard-acceptance.py`
- Regenerated: `rust/Cargo.lock`

**Interfaces:**
- Consumes (Task 1): `ansi::{ENTER, LEAVE, POP_KEYBOARD, push_keyboard}`.
- Produces:
  - `ansi::QUERY_KEYBOARD: &str`
  - `input::Encoding { Legacy, Kitty }` (`Clone, Copy, Debug, PartialEq, Eq`); `input::legacy_control(key: KeyEvent) -> KeyEvent`; `input::decode(event: &mut Event, encoding: Encoding)`
  - `events::Events` trait: `fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>>`, implemented for `termina::EventReader`
  - `events::Input<E: Events>`: `new(events: E, pending: VecDeque<Event>, encoding: Encoding) -> Self`, `next(&mut self, timeout: Duration) -> io::Result<Option<Event>>`
  - `#[cfg(test)] events::script::{Script, key, ctrl}`: `Script::of(steps: impl IntoIterator<Item = io::Result<Option<Event>>>) -> Script`, `Script::left(&self) -> usize`, `key(c: char) -> Event`, `ctrl(c: char) -> Event`
  - `probe::keyboard_flags(out: &mut impl Write, events: &mut impl Events, pending: &mut VecDeque<Event>, timeout: Duration) -> io::Result<Option<KittyKeyboardFlags>>`
  - `keyboard::negotiate<W: Write>(out: &mut W, pushed: &AtomicBool, legacy: bool, events: &mut impl Events, pending: &mut VecDeque<Event>) -> TerminalCapabilities`; `keyboard::encoding(capabilities: &TerminalCapabilities) -> Encoding`
  - `term::Guard::size(&self) -> io::Result<(u16, u16)>`; `term::Guard::input(&mut self) -> events::Input<EventReader>`

`Event` is `termina::Event`; `KeyEvent`, `KeyCode`, `Modifiers` and friends come from `termina::event`; `KittyKeyboardFlags`, `Csi`, `Keyboard`, `Device` from `termina::escape::csi`.

- [ ] **Step 1: Swap the dependency**

In `rust/crates/lem-ratatui/Cargo.toml`, replace `crossterm = "0.29.0"` with `termina = "0.4"`, and change the description to `"Terminal display half for Lem: paints lem.display.v1 frames on the terminal."`.

In `rust/Cargo.toml`, delete these three lines (keep `exclude = ["vendor/crossterm"]` until Task 3 deletes the directory):

```toml
# Local, documented terminal-query and control-byte fixes against upstream 0.29.0.
[patch.crates-io]
crossterm = { path = "vendor/crossterm" }
```

In `src/ansi.rs`, delete the whole `#[cfg(test)] mod same_as_crossterm { … }` block (Task 1 proved it; crossterm is gone), and add after `POP_KEYBOARD`:

```rust
/// Ask for the active kitty keyboard flags, then for the primary device
/// attributes, which every terminal answers: DA1 with no flags before it
/// means no protocol (kitty's recommended detection).
pub const QUERY_KEYBOARD: &str = "\x1b[?u\x1b[c";
```

Nothing compiles until Step 8; the next steps are written test-first, but run together at Step 9.

- [ ] **Step 2: `input.rs` on termina's types, with the C0 decoding**

Replace the module comment's first line `//! crossterm events to \`lem.display.v1\` input.` with `//! Terminal events to \`lem.display.v1\` input.`, and the `use` block with:

```rust
use lem_protocols::display::v1::{self, NamedKey, key, mouse};
use termina::Event;
use termina::event::{
    KeyCode, KeyEvent, KeyEventState, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// How the terminal encodes keys: legacy bytes, or the kitty keyboard
/// protocol once negotiation confirmed it (`keyboard.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Legacy,
    Kitty,
}

/// Name the control bytes 0x1C–0x1F by their ASCII keys, `C-\`, `C-]`,
/// `C-^` and `C-_`, for a key read under legacy encoding.
///
/// The parser reports those bytes as Ctrl+4..7, the keys that share them
/// in a legacy terminal. Under legacy encoding a Ctrl+digit 4..7 can only
/// have come from them; under the kitty protocol it is the digit, sent as
/// CSI u. So the encoding decides (ADR 0021).
pub fn legacy_control(key: KeyEvent) -> KeyEvent {
    let name = match key.code {
        KeyCode::Char('4') => '\\',
        KeyCode::Char('5') => ']',
        KeyCode::Char('6') => '^',
        KeyCode::Char('7') => '_',
        _ => return key,
    };
    if key.modifiers.contains(Modifiers::CONTROL) && !key.state.contains(KeyEventState::KEYPAD) {
        KeyEvent {
            code: KeyCode::Char(name),
            ..key
        }
    } else {
        key
    }
}

/// Decode `event` as read under `encoding` (see [`legacy_control`]).
pub fn decode(event: &mut Event, encoding: Encoding) {
    if encoding == Encoding::Legacy
        && let Event::Key(key) = event
    {
        *key = legacy_control(*key);
    }
}
```

In `key`, replace the comment above `KeyCode::Char(c) =>` with `// Already decoded for its encoding (\`decode\`).`. Then rename crossterm's names to termina's across the file, tests included:

```bash
sed -i 's/KeyModifiers/Modifiers/g; s/KeyCode::F(/KeyCode::Function(/g; s/KeyCode::Esc\b/KeyCode::Escape/g' \
    rust/crates/lem-ratatui/src/input.rs
```

Add these tests to `input.rs`'s `mod tests`:

```rust
    #[test]
    fn under_legacy_encoding_ctrl_4_to_7_are_the_ascii_controls() {
        for (digit, name) in [('4', '\\'), ('5', ']'), ('6', '^'), ('7', '_')] {
            let key = legacy_control(KeyEvent::new(KeyCode::Char(digit), Modifiers::CONTROL));
            assert_eq!(key.code, KeyCode::Char(name));
            assert_eq!(key.modifiers, Modifiers::CONTROL);
        }
    }

    #[test]
    fn meta_survives_the_renaming() {
        let key = legacy_control(KeyEvent::new(
            KeyCode::Char('7'),
            Modifiers::CONTROL | Modifiers::ALT,
        ));
        assert_eq!(key.code, KeyCode::Char('_'));
        assert_eq!(key.modifiers, Modifiers::CONTROL | Modifiers::ALT);
    }

    #[test]
    fn other_keys_keep_their_names() {
        let mut keypad = KeyEvent::new(KeyCode::Char('4'), Modifiers::CONTROL);
        keypad.state = KeyEventState::KEYPAD;
        for key in [
            KeyEvent::new(KeyCode::Char('4'), Modifiers::NONE),
            KeyEvent::new(KeyCode::Char('8'), Modifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('3'), Modifiers::CONTROL),
            keypad,
        ] {
            assert_eq!(legacy_control(key), key);
        }
    }

    #[test]
    fn decoding_renames_only_legacy_keys() {
        let ctrl_5 = || Event::Key(KeyEvent::new(KeyCode::Char('5'), Modifiers::CONTROL));
        let mut legacy = ctrl_5();
        decode(&mut legacy, Encoding::Legacy);
        assert_eq!(
            legacy,
            Event::Key(KeyEvent::new(KeyCode::Char(']'), Modifiers::CONTROL))
        );
        let mut kitty = ctrl_5();
        decode(&mut kitty, Encoding::Kitty);
        assert_eq!(kitty, ctrl_5());
        let mut paste = Event::Paste("5".into());
        decode(&mut paste, Encoding::Legacy);
        assert_eq!(paste, Event::Paste("5".into()));
    }
```

- [ ] **Step 3: `golden.rs` on termina's types**

Replace the crossterm `use` with:

```rust
use termina::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, Modifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
```

Replace `keypad` with:

```rust
fn keypad(code: KeyCode, modifiers: Modifiers) -> to_editor::Message {
    to_editor::Message::Key(
        input::key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::KEYPAD,
        })
        .expect("a known keypad key"),
    )
}
```

Then rename across the file:

```bash
sed -i 's/KeyModifiers/Modifiers/g; s/KeyCode::F(/KeyCode::Function(/g' \
    rust/crates/lem-ratatui/src/golden.rs
```

The fixture is the test: it must still match byte for byte. Never set `LEM_UPDATE_FIXTURES`.

- [ ] **Step 4: Write `events.rs`, tests first**

Create `rust/crates/lem-ratatui/src/events.rs`:

```rust
//! The terminal's input, as the main loop reads it (ADR 0021).
//!
//! Keyboard negotiation reads the terminal before the main loop does and
//! sets aside what is not a reply. `Input` hands those over first, in
//! order, then the terminal's own events.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use termina::{Event, EventReader};

use crate::input::{self, Encoding};

/// Somewhere terminal events come from.
pub trait Events {
    /// The next event, waiting at most `timeout`; `None` when none came.
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>>;
}

impl Events for EventReader {
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        // Unfiltered both times, so events come out in the order they
        // arrived: a filtered read moves the events it skips behind the
        // ones still queued.
        if self.poll(Some(timeout), |_| true)? {
            self.read(|_| true).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// The main loop's terminal input.
pub struct Input<E> {
    events: E,
    pending: VecDeque<Event>,
    encoding: Encoding,
}

impl<E: Events> Input<E> {
    /// `pending` is what negotiation set aside, already decoded;
    /// `encoding` is the one negotiation settled on.
    pub fn new(events: E, pending: VecDeque<Event>, encoding: Encoding) -> Self {
        Self {
            events,
            pending,
            encoding,
        }
    }

    /// The next event, waiting at most `timeout` for the terminal.
    pub fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let mut event = self.events.next(timeout)?;
        if let Some(event) = event.as_mut() {
            input::decode(event, self.encoding);
        }
        Ok(event)
    }
}

/// Scripted terminal events, for this crate's tests.
#[cfg(test)]
pub mod script {
    use super::*;
    use termina::event::{KeyCode, KeyEvent, Modifiers};

    /// Each step answers one `next`; after the last, every wait ends with
    /// nothing, as a silent terminal's would.
    pub struct Script(VecDeque<io::Result<Option<Event>>>);

    impl Script {
        pub fn of(steps: impl IntoIterator<Item = io::Result<Option<Event>>>) -> Self {
            Self(steps.into_iter().collect())
        }

        /// Steps not read yet.
        pub fn left(&self) -> usize {
            self.0.len()
        }
    }

    impl Events for Script {
        fn next(&mut self, _timeout: Duration) -> io::Result<Option<Event>> {
            self.0.pop_front().unwrap_or(Ok(None))
        }
    }

    pub fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), Modifiers::NONE))
    }

    pub fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), Modifiers::CONTROL))
    }
}

#[cfg(test)]
mod tests {
    use super::script::{Script, ctrl, key};
    use super::*;

    const WAIT: Duration = Duration::from_millis(10);

    fn live(event: Event) -> Script {
        Script::of([Ok(Some(event))])
    }

    #[test]
    fn set_aside_events_come_first_in_order() {
        let pending = VecDeque::from([key('a'), key('b')]);
        let mut input = Input::new(live(key('c')), pending, Encoding::Kitty);
        for expected in ['a', 'b', 'c'] {
            assert_eq!(input.next(WAIT).unwrap(), Some(key(expected)));
        }
        assert_eq!(input.next(WAIT).unwrap(), None);
    }

    #[test]
    fn under_legacy_encoding_live_c0_keys_are_named() {
        let mut input = Input::new(live(ctrl('5')), VecDeque::new(), Encoding::Legacy);
        assert_eq!(input.next(WAIT).unwrap(), Some(ctrl(']')));
    }

    #[test]
    fn under_the_kitty_protocol_ctrl_digits_stay_digits() {
        let mut input = Input::new(live(ctrl('5')), VecDeque::new(), Encoding::Kitty);
        assert_eq!(input.next(WAIT).unwrap(), Some(ctrl('5')));
    }

    #[test]
    fn set_aside_keys_are_not_decoded_again() {
        // Negotiation decoded it by when it was read (keyboard.rs).
        let pending = VecDeque::from([ctrl('5')]);
        let mut input = Input::new(Script::of([]), pending, Encoding::Legacy);
        assert_eq!(input.next(WAIT).unwrap(), Some(ctrl('5')));
    }

    #[test]
    fn errors_reach_the_caller() {
        let gone = io::Error::new(io::ErrorKind::UnexpectedEof, "terminal gone");
        let mut input = Input::new(Script::of([Err(gone)]), VecDeque::new(), Encoding::Kitty);
        assert_eq!(
            input.next(WAIT).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
```

- [ ] **Step 5: Write `probe.rs`, tests first**

Create `rust/crates/lem-ratatui/src/probe.rs`:

```rust
//! Terminal queries, answered on the input stream (ADR 0021).
//!
//! The terminal answers in the stream that carries keys. So a query reads
//! every event itself, unfiltered: replies decide the answer, and anything
//! else is set aside, in order, for the main loop.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use termina::Event;
use termina::escape::csi::{Csi, Device, Keyboard, KittyKeyboardFlags};

use crate::ansi;
use crate::events::Events;

/// The terminal's active kitty keyboard flags: `Some` when it reported
/// them, `None` when it answered DA1 without them, so has no protocol.
///
/// Waits at most `timeout`. Flags with no DA1 by then are still the
/// answer; no reply at all is `TimedOut`. Events that are not replies go
/// to `pending`, as they came. A read error is returned, and what was set
/// aside before it stays set aside.
pub fn keyboard_flags(
    out: &mut impl Write,
    events: &mut impl Events,
    pending: &mut VecDeque<Event>,
    timeout: Duration,
) -> io::Result<Option<KittyKeyboardFlags>> {
    let started = Instant::now();
    let timed_out = || {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "the terminal sent no keyboard flags or DA1 before the deadline",
        )
    };
    if timeout.is_zero() {
        return Err(timed_out());
    }
    out.write_all(ansi::QUERY_KEYBOARD.as_bytes())?;
    out.flush()?;

    let mut flags = None;
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        let event = if remaining.is_zero() {
            None
        } else {
            events.next(remaining)?
        };
        match event {
            None => return flags.map(Some).ok_or_else(timed_out),
            Some(Event::Csi(Csi::Keyboard(Keyboard::ReportFlags(reported)))) => {
                flags = Some(reported);
            }
            // DA1 answers last, so the flags, if any, came before it.
            Some(Event::Csi(Csi::Device(Device::DeviceAttributes(_)))) => return Ok(flags),
            Some(other) => pending.push_back(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::script::{Script, ctrl, key};

    const BUDGET: Duration = Duration::from_secs(1);

    fn flags(bits: u8) -> Event {
        Event::Csi(Csi::Keyboard(Keyboard::ReportFlags(
            KittyKeyboardFlags::from_bits_truncate(bits),
        )))
    }

    fn da1() -> Event {
        Event::Csi(Csi::Device(Device::DeviceAttributes(())))
    }

    fn arriving(events: impl IntoIterator<Item = Event>) -> Script {
        Script::of(events.into_iter().map(|event| Ok(Some(event))))
    }

    #[test]
    fn flags_then_da1_are_the_answer_and_the_query_is_written_once() {
        let mut out = Vec::new();
        let answer = keyboard_flags(
            &mut out,
            &mut arriving([flags(5), da1()]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap();
        assert_eq!(answer, Some(KittyKeyboardFlags::from_bits_truncate(5)));
        assert_eq!(out, b"\x1b[?u\x1b[c");
    }

    #[test]
    fn da1_alone_means_no_protocol() {
        let answer = keyboard_flags(
            &mut Vec::new(),
            &mut arriving([da1()]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap();
        assert_eq!(answer, None);
    }

    #[test]
    fn silence_is_a_timeout() {
        let error = keyboard_flags(
            &mut Vec::new(),
            &mut Script::of([]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn flags_without_da1_are_still_the_answer() {
        let answer = keyboard_flags(
            &mut Vec::new(),
            &mut arriving([flags(1)]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap();
        assert_eq!(answer, Some(KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES));
    }

    #[test]
    fn everything_else_is_set_aside_in_order_and_nothing_after_da1_is_read() {
        let mut script = arriving([key('a'), ctrl('5'), flags(0), key('b'), da1(), key('c')]);
        let mut pending = VecDeque::new();
        let answer = keyboard_flags(&mut Vec::new(), &mut script, &mut pending, BUDGET).unwrap();
        assert_eq!(answer, Some(KittyKeyboardFlags::empty()));
        assert_eq!(pending, [key('a'), ctrl('5'), key('b')]);
        assert_eq!(script.left(), 1, "the key after DA1 is the main loop's");
    }

    #[test]
    fn a_read_error_is_returned_and_what_was_set_aside_stays() {
        let mut script = Script::of([Ok(Some(key('a'))), Err(io::Error::other("gone"))]);
        let mut pending = VecDeque::new();
        assert!(keyboard_flags(&mut Vec::new(), &mut script, &mut pending, BUDGET).is_err());
        assert_eq!(pending, [key('a')]);
    }

    #[test]
    fn no_budget_asks_nothing() {
        let mut out = Vec::new();
        let error = keyboard_flags(
            &mut out,
            &mut Script::of([]),
            &mut VecDeque::new(),
            Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(out.is_empty());
    }
}
```

- [ ] **Step 6: Negotiation over the probe**

In `src/keyboard.rs`, replace everything above `pub fn pop_if_owned` with:

```rust
//! Bounded, confirmed keyboard enhancement on the controlling terminal.
//!
//! The display is the terminal's only reader: negotiation reads replies
//! itself (`probe.rs`) and sets aside the keys typed meanwhile for the
//! main loop (`events::Input`), decoded by when they were read.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lem_protocols::display::v1::TerminalCapabilities;
use termina::Event;
use termina::escape::csi::KittyKeyboardFlags as Flags;

use crate::ansi;
use crate::events::Events;
use crate::input::{self, Encoding};
use crate::probe;

const BUDGET: Duration = Duration::from_secs(1);

/// The flag shared with terminal cleanup records completed push commands,
/// rather than attempts that may have written no complete command.
/// Events read meanwhile that are not replies go to `pending`, decoded.
pub fn negotiate<W: Write>(
    out: &mut W,
    pushed: &AtomicBool,
    legacy: bool,
    events: &mut impl Events,
    pending: &mut VecDeque<Event>,
) -> TerminalCapabilities {
    let started = Instant::now();
    let mut before_push = None;
    let capabilities = negotiate_with(
        out,
        pushed,
        legacy,
        |out, timeout| {
            let flags = probe::keyboard_flags(out, events, pending, timeout);
            // Only the first query is answered before any push.
            before_push.get_or_insert(pending.len());
            flags
        },
        || started.elapsed(),
    );
    decode_set_aside(pending, before_push.unwrap_or(0), encoding(&capabilities));
    capabilities
}

/// How the terminal encodes keys under `capabilities`.
pub fn encoding(capabilities: &TerminalCapabilities) -> Encoding {
    if capabilities.keyboard_disambiguation {
        Encoding::Kitty
    } else {
        Encoding::Legacy
    }
}

/// Decode what negotiation set aside by the encoding in force when it
/// was read: the first `before_push` events came before any push, so
/// under legacy encoding; the rest under the outcome, `after`.
fn decode_set_aside(pending: &mut VecDeque<Event>, before_push: usize, after: Encoding) {
    for (index, event) in pending.iter_mut().enumerate() {
        let encoding = if index < before_push {
            Encoding::Legacy
        } else {
            after
        };
        input::decode(event, encoding);
    }
}

fn negotiate_with<W: Write>(
    out: &mut W,
    pushed: &AtomicBool,
    legacy: bool,
    mut query: impl FnMut(&mut W, Duration) -> io::Result<Option<Flags>>,
    mut elapsed: impl FnMut() -> Duration,
) -> TerminalCapabilities {
    let legacy_capabilities = TerminalCapabilities::default();
    if legacy {
        return legacy_capabilities;
    }

    let mut remaining = || BUDGET.saturating_sub(elapsed());
    let timeout = remaining();
    if timeout.is_zero() || !matches!(query(out, timeout), Ok(Some(_))) {
        return legacy_capabilities;
    }
    // Support detection can consume the whole budget. Avoid pushing an
    // enhancement we already know we will have no time to confirm.
    if remaining().is_zero() {
        return legacy_capabilities;
    }

    let requested = Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALTERNATE_KEYS;
    if write_stack_command(out, pushed, &ansi::push_keyboard(requested.bits()), true).is_ok() {
        let timeout = remaining();
        if !timeout.is_zero()
            && let Ok(Some(active)) = query(out, timeout)
            && active.contains(requested)
            && !remaining().is_zero()
        {
            return TerminalCapabilities {
                keyboard_disambiguation: true,
                alternate_key_reporting: true,
                // The protocol can encode provenance, but each
                // event's keypad bit remains authoritative.
                keypad_identity: true,
                // The clipboard is not the keyboard's; main.rs sets it.
                ..TerminalCapabilities::default()
            };
        }
    }

    // Balance a completed push, including one whose flush failed. An
    // incomplete command creates no stack entry we are entitled to pop.
    let _ = pop_if_owned(out, pushed);
    legacy_capabilities
}
```

`pop_if_owned` and `write_stack_command` are unchanged from Task 1.

In `mod tests`, the injected query now also receives the output: change each closure's parameter list from `|timeout|` to `|_, timeout|`, and from `|_|` to `|_, _|`. Nothing else in the existing tests changes. Then add:

```rust
    #[test]
    fn set_aside_keys_are_decoded_by_when_they_were_read() {
        use crate::events::script::ctrl;
        // One Ctrl+5 read before the push, one during confirmation.
        let mut confirmed = VecDeque::from([ctrl('5'), ctrl('5')]);
        decode_set_aside(&mut confirmed, 1, Encoding::Kitty);
        assert_eq!(confirmed, [ctrl(']'), ctrl('5')]);
        let mut refused = VecDeque::from([ctrl('5'), ctrl('5')]);
        decode_set_aside(&mut refused, 1, Encoding::Legacy);
        assert_eq!(refused, [ctrl(']'), ctrl(']')]);
    }

    #[test]
    fn the_encoding_follows_confirmed_disambiguation() {
        assert_eq!(encoding(&TerminalCapabilities::default()), Encoding::Legacy);
        let confirmed = TerminalCapabilities {
            keyboard_disambiguation: true,
            ..TerminalCapabilities::default()
        };
        assert_eq!(encoding(&confirmed), Encoding::Kitty);
    }
```

- [ ] **Step 7: The terminal guard over termina**

In `src/term.rs`, replace everything above `#[cfg(test)] mod tests` with:

```rust
//! Terminal lifecycle.
//!
//! This is a guard rather than a pair of functions because restoring the
//! terminal when Lem dies is the single strongest argument for the
//! two-process split
//! (`../../../docs/adr/0004-two-processes-not-embedded-lisp.md`), and
//! that only holds if restore runs on every exit path — panics included.
//!
//! The terminal is the controlling one, opened by name, not stdio: stdin
//! and stdout carry the protocol (`transport.rs`). termina reads keys and
//! the window size from `/dev/tty` when stdin and stdout are not
//! terminals; the display writes through a handle of its own (ADR 0021).
//!
//! The mouse is captured (ADR 0013), which takes the terminal's own
//! click-and-drag selection away; most terminals keep it on Shift+drag.
//! Bracketed paste is on, so pasted text arrives as one `Paste`.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use lem_protocols::display::v1::TerminalCapabilities;
use termina::{Event, EventReader, PlatformTerminal, Terminal};

use crate::ansi;
use crate::events::Input;
use crate::keyboard;

#[cfg(unix)]
const TTY: &str = "/dev/tty";
#[cfg(windows)]
const TTY: &str = "CONOUT$";

/// Raw mode and the alternate screen, released on drop.
pub struct Guard {
    terminal: PlatformTerminal,
    tty: File,
    cleanup: Arc<Cleanup>,
    capabilities: TerminalCapabilities,
    /// What keyboard negotiation read that was not a reply.
    pending: VecDeque<Event>,
}

/// Shared by the panic hook and guard, without a lock that a panic could
/// strand. Exactly one caller owns restoration, even while unwinding.
#[derive(Default)]
struct Cleanup {
    active: AtomicBool,
    keyboard_pushed: AtomicBool,
}

impl Cleanup {
    fn restore(
        &self,
        out: &mut impl Write,
        disable_raw: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        if !self.active.swap(false, Ordering::AcqRel) {
            return Ok(());
        }

        let keyboard = keyboard::pop_if_owned(out, &self.keyboard_pushed);
        // Leave the screen even if popping the keyboard stack failed.
        let screen = out
            .write_all(ansi::LEAVE.as_bytes())
            .and_then(|()| out.flush());
        // A broken output must never prevent restoring the terminal mode.
        let raw = disable_raw();
        keyboard.and(screen).and(raw)
    }
}

impl Guard {
    /// Take over the controlling terminal, or return `Ok(None)` when there
    /// is none — under a test harness, CI, or a service manager — where
    /// there is nothing to draw on and nothing to restore.
    ///
    /// Installs a panic hook that restores first and then defers to the
    /// previous hook, so a panic message is still printed — but onto a
    /// terminal that can display it.
    pub fn new_if_interactive() -> Result<Option<Self>> {
        let tty = match OpenOptions::new().read(true).write(true).open(TTY) {
            Ok(tty) if tty.is_terminal() => tty,
            _ => return Ok(None),
        };
        let mut guard = Self {
            terminal: PlatformTerminal::new()?,
            tty,
            cleanup: Arc::default(),
            capabilities: TerminalCapabilities::default(),
            pending: VecDeque::new(),
        };
        // Construct the guard before raw mode. No fallible operation may
        // intervene between entering raw mode and arming restoration.
        guard.terminal.enter_raw_mode()?;
        guard.cleanup.active.store(true, Ordering::Release);

        // termina runs this on a handle of its own, then restores the
        // terminal's modes itself, then calls the previous hook.
        let cleanup = Arc::clone(&guard.cleanup);
        guard.terminal.set_panic_hook(move |out| {
            let _ = cleanup.restore(out, || Ok(()));
        });

        guard.tty.write_all(ansi::ENTER.as_bytes())?;
        guard.tty.flush()?;
        let legacy =
            std::env::var_os("LEM_RATATUI_KEYBOARD").is_some_and(|value| value == "legacy");
        let mut events = guard.terminal.event_reader();
        guard.capabilities = keyboard::negotiate(
            &mut guard.tty,
            &guard.cleanup.keyboard_pushed,
            legacy,
            &mut events,
            &mut guard.pending,
        );

        Ok(Some(guard))
    }

    /// Somewhere to draw.
    pub fn writer(&self) -> io::Result<File> {
        self.tty.try_clone()
    }

    /// Confirmed active keyboard capabilities for this session.
    pub fn capabilities(&self) -> TerminalCapabilities {
        self.capabilities
    }

    /// The terminal's size in cells: columns, then rows.
    pub fn size(&self) -> io::Result<(u16, u16)> {
        let size = self.terminal.get_dimensions()?;
        Ok((size.cols, size.rows))
    }

    /// The main loop's input: what negotiation set aside, then the
    /// terminal's events, decoded as negotiation settled. Take it once.
    pub fn input(&mut self) -> Input<EventReader> {
        Input::new(
            self.terminal.event_reader(),
            std::mem::take(&mut self.pending),
            keyboard::encoding(&self.capabilities),
        )
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let terminal = &mut self.terminal;
        let _ = self
            .cleanup
            .restore(&mut self.tty, || terminal.enter_cooked_mode());
    }
}
```

The `#[cfg(test)] mod tests` block (the three `Cleanup` tests) does not change.

- [ ] **Step 8: The main loop reads `Input`**

In `src/main.rs`:

1. Add `mod events;` after `mod colors;` and `mod probe;` after `mod present;`.
2. Replace `use crossterm::event::{self, Event, KeyEventKind};` with:

```rust
use termina::Event;
use termina::event::KeyEventKind;
```

3. Replace `display_size` with:

```rust
/// The terminal's current size, or [`HEADLESS_SIZE`] without one.
fn display_size(guard: Option<&term::Guard>) -> (u16, u16) {
    guard
        .and_then(|guard| guard.size().ok())
        .unwrap_or(HEADLESS_SIZE)
}
```

4. In `main`, change the comment above `colors::query()` from "before crossterm reads input" to "before termina reads input"; change `let guard = term::Guard::new_if_interactive()?;` to `let mut guard = …`; after the `let mut presenter = match &guard { … };` statement add:

```rust
    // The terminal's input, for as long as there is a terminal.
    let mut events = guard.as_mut().map(term::Guard::input);
```

and change `display_size(guard.is_some())` to `display_size(guard.as_ref())`.

5. Replace the input block at the top of the main loop (from `// Input first` through the closing brace of `if presenter.is_some() && event::poll(…)? { … }`) with:

```rust
        // Input first, so a keystroke is never delayed behind a frame.
        if let Some(events) = events.as_mut()
            && let Some(event) = events.next(Duration::from_millis(10))?
        {
            let message = match event {
                // Press only: under the kitty protocol and on Windows,
                // releases and repeats arrive too and would double every
                // keystroke.
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    input::key(key).map(to_editor::Message::Key)
                }
                Event::Mouse(mouse) => Some(to_editor::Message::Mouse(input::mouse(mouse))),
                Event::Paste(text) => Some(to_editor::Message::Paste(v1::Paste { text })),
                // Lem lays out again and repaints (lem:update-on-display-resized).
                Event::WindowResized(size) => {
                    (width, height) = (size.cols, size.rows);
                    Some(to_editor::Message::Resize(v1::Resize {
                        width: u32::from(size.cols),
                        height: u32::from(size.rows),
                    }))
                }
                _ => None,
            };
            if let Some(message) = message {
                writer.send(message)?;
            }
        }
```

A terminal that hangs up now ends here: `events.next` returns `UnexpectedEof`, `?` returns it from `main`, and dropping the guard restores what it can.

- [ ] **Step 9: Build and run the unit tests**

Run: `(cd rust && cargo test -p lem-ratatui)`
Expected: PASS, including `input::tests` (4 new), `events::tests` (5), `probe::tests` (7), `keyboard::tests` (2 new), and `golden::golden_display_inputs_are_current` against the unchanged fixture.

Run: `git status --short proto`
Expected: nothing — `proto/fixtures/display-inputs.v1.bin` and its text twin are untouched.

Run: `(cd rust && cargo tree -i crossterm)`
Expected: `error: package ID specification \`crossterm\` did not match any packages` — crossterm is gone from the graph.

Run: `(cd rust && cargo clippy --all-targets && cargo fmt)`
Expected: no warnings.

- [ ] **Step 10: Keyboard acceptance, with one sequence spelled the kitty way**

termina decodes a raw 0x1D as Ctrl+5 once the kitty protocol is confirmed (ADR 0021, Consequences). The `enhanced` check sends one raw 0x1D under the protocol; send Ctrl+] as the protocol encodes it instead. In `scripts/keyboard-acceptance.py`, `run_case`, replace

```python
        if send_keys:
            os.write(session.master,
                     b"\x1b[53;5u\x1d\x1b[103;6u\x1b[103:71;6u\x07"
                     b"\x1b[57403;5u\x1b[52;5u\xc4\x8d\xc5\xa1")
```

with

```python
        if send_keys:
            # Ctrl+] as the kitty protocol sends it, CSI 93;5u. A raw 0x1D
            # here reads as Ctrl+5 until termina names the C0 bytes (ADR 0021).
            os.write(session.master,
                     b"\x1b[53;5u\x1b[93;5u\x1b[103;6u\x1b[103:71;6u\x07"
                     b"\x1b[57403;5u\x1b[52;5u\xc4\x8d\xc5\xa1")
```

The expected keys do not change.

Run: `(cd rust && cargo build -p lem-ratatui) && python3 scripts/keyboard-acceptance.py`
Expected: `8/8 keyboard acceptance checks passed`, with `PASS enhanced with queued keys` first. The `legacy` and `override` checks still send raw `\x1c\x1d\x1e\x1f` and still expect `\`, `]`, `^`, `_`.

- [ ] **Step 11: Commit**

```bash
git add rust/Cargo.toml rust/Cargo.lock rust/crates/lem-ratatui/Cargo.toml \
        rust/crates/lem-ratatui/src scripts/keyboard-acceptance.py
git commit -m "feat(ratatui): read the terminal with termina instead of crossterm"
```

---

### Task 3: Drop the vendored crossterm; prove what it fixed stays fixed

**Files:**
- Delete: `rust/vendor/` (only `vendor/crossterm` is in it)
- Modify: `rust/Cargo.toml`, `scripts/keyboard-acceptance.py`, `README.md`, `lisp/implementation.lisp`, `display-relay/tests/golden.lisp`

**Interfaces:**
- Consumes (Task 2): the display binary as built; `LEM_RATATUI_KEYBOARD`.
- Produces: keyboard acceptance checks `hangup` and `stall` (10 checks in all).

- [ ] **Step 1: Write the two acceptance checks**

In `scripts/keyboard-acceptance.py`:

1. Add `import signal` to the imports (alphabetically, after `import select`).
2. Extend the module docstring's first paragraph with: `It also checks that a terminal hang-up ends the display, and that input which never completes an escape sequence does not hold up the protocol.`
3. Give `Session.__init__` an `ignore_hangup=False` keyword and use it in `controlling_terminal`:

```python
    def __init__(self, binary, mode, *, queued=False, answer_colours=True, ignore_hangup=False):
```

```python
        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)
            if ignore_hangup:
                # Leave end-of-file as the only sign the terminal went away.
                signal.signal(signal.SIGHUP, signal.SIG_IGN)
```

4. Add a method after `exit`:

```python
    def hang_up(self):
        """Close the terminal, as a closed window or a dropped SSH session does."""
        self.readers.discard(self.master)
        os.close(self.master)
        self.master = None
```

5. In `close`, change `os.close(self.master)` to:

```python
        if self.master is not None:
            os.close(self.master)
```

6. Add after `run_case`:

```python
def run_hangup(binary):
    session = Session(binary, "legacy", ignore_hangup=True)
    try:
        session.hello()
        session.hang_up()
        started = time.monotonic()
        # A display that spins on end-of-file never gets here.
        session.until(lambda: session.child.poll() is not None, seconds=2)
        print(f"PASS hangup: the display exited {time.monotonic() - started:.3f}s "
              f"after its terminal closed")
    finally:
        session.close()


def run_stall(binary):
    session = Session(binary, "legacy")
    try:
        session.hello()
        # An escape sequence that never completes is no event. The display
        # must go on serving Lem, not wait inside read() for the next key.
        os.write(session.master, b"\x1b[")
        session.pump(0.2)
        started = time.monotonic()
        session.exit()
        elapsed = time.monotonic() - started
        assert elapsed < 1, f"stall: Lem's Exit waited {elapsed:.3f}s behind terminal input"
        print(f"PASS stall: Exit served {elapsed:.3f}s after an unfinished escape sequence")
    finally:
        session.close()
```

7. In `main`, after the `for mode in …: run_case(binary, mode)` loop, add `run_hangup(binary)` and `run_stall(binary)`, and change the final line to `print("10/10 keyboard acceptance checks passed")`.

- [ ] **Step 2: Run the keyboard acceptance**

Run: `(cd rust && cargo build -p lem-ratatui) && python3 scripts/keyboard-acceptance.py`
Expected: `10/10 keyboard acceptance checks passed`; `PASS hangup` well under 2 s, `PASS stall` well under 1 s.

These checks pass on termina by design rather than failing first: they guard against regressions toward crossterm 0.29's behaviour. To see them discriminate, revert locally to Task 1's commit with the `[patch.crates-io]` lines removed (unpatched crossterm): `hangup` times out with the display spinning, and `stall` times out waiting for Exit. Do not commit that experiment.

- [ ] **Step 3: Delete the vendored crossterm**

```bash
git rm -r -q rust/vendor
```

In `rust/Cargo.toml`, delete the line `exclude = ["vendor/crossterm"]`.

Run: `(cd rust && cargo test && cargo clippy --all-targets)`
Expected: the whole workspace passes; no warnings.

Run: `grep -rn crossterm rust/Cargo.toml rust/crates`
Expected: only comments naming crossterm as where something came from: `ratatui-crossterm` in `rust/crates/lem-ratatui/Cargo.toml` (above `ratatui-core`) and in `src/present.rs` (module and function comments), and the byte-for-byte lineage in `src/ansi.rs`'s comments. No `use crossterm`, no dependency.

- [ ] **Step 4: Docs and comments**

`README.md`:

1. Line 3: replace `A terminal frontend for Lem built on \`crossterm\`. Lem's side is` with `A terminal frontend for Lem, for Linux, macOS and Windows through WSL. Lem's side is`.
2. Replace

```
else. The display writes to the terminal itself, with crossterm, because
Ratatui's own output cannot draw curly or dotted underlines. Calling it a
crossterm frontend that borrows Ratatui's cell buffer is more honest.

See [`docs/adr/0002`](docs/adr/0002-ratatui-core-over-full-ratatui.md) and
[`0016`](docs/adr/0016-underline-styles.md).
```

with

```
else. The display writes to the terminal itself, because Ratatui's own
output cannot draw curly or dotted underlines, and reads it with
`termina`. Calling it a termina frontend that borrows Ratatui's cell
buffer is more honest.

See [`docs/adr/0002`](docs/adr/0002-ratatui-core-over-full-ratatui.md),
[`0016`](docs/adr/0016-underline-styles.md) and
[`0021`](docs/adr/0021-termina-instead-of-a-patched-crossterm.md).
```

3. In "Keys and mouse", replace the paragraph beginning `The Rust workspace carries a narrowly patched crossterm 0.29.0` (seven lines, ending `The local patch record documents the upstream source and fixes.`) with

```
The display reads the terminal with `termina` (ADR 0021). Its keyboard
query writes only to the terminal, never to the protocol on stdout,
waits at most one second in all, and keeps keys typed meanwhile, in
order. Under legacy encoding the raw C0 bytes 0x1C–0x1F are named by
their ASCII keys, `C-\`, `C-]`, `C-^` and `C-_`; under the kitty
protocol, Ctrl+4 through Ctrl+7 are the digits.
```

4. In the focused keyboard checks block, delete the two lines `cargo test --manifest-path rust/vendor/crossterm/Cargo.toml --lib` and `python3 rust/vendor/crossterm/tests/keyboard_probe_pty.py`. Replace the paragraph after the block,

```
These use isolated PTYs and protocol pipes. They verify negotiation,
fallback, queued keys and restoration without opening Lem or changing a
personal configuration. Physical gestures still need checking in the
terminal and keyboard layout in use.
```

with

```
These use isolated PTYs and protocol pipes. They verify negotiation,
fallback, queued keys, restoration, a terminal hang-up and unfinished
input without opening Lem or changing a personal configuration.
Physical gestures still need checking in the terminal and keyboard
layout in use.
```

`lisp/implementation.lisp`, in the `:window-left-margin` comment block, replace

```lisp
   ;; Off, matching ncurses, despite modern terminals supporting SGR 58
   ;; and crossterm exposing SetUnderlineColor.
```

with

```lisp
   ;; Off, matching ncurses, despite modern terminals supporting SGR 58,
   ;; which the display can write.
```

`display-relay/tests/golden.lisp`, line 10: replace `conversions from crossterm events` with `conversions from terminal events`.

- [ ] **Step 5: Everything, end to end**

Run: `make test`
Expected: Rust and Lisp unit tests pass.

Run: `SB=~/.roswell/impls/x86-64/linux/sbcl-bin/2.6.8; PATH="$SB/bin:$PATH" SBCL_HOME="$SB/lib/sbcl" make all && python3 scripts/acceptance.py`
Expected: every acceptance check passes, including resize reflow (check 6), which now arrives as termina's `WindowResized`.

Run: `python3 scripts/keyboard-acceptance.py`
Expected: `10/10 keyboard acceptance checks passed`.

- [ ] **Step 6: Commit**

```bash
git add -A rust/Cargo.toml rust/Cargo.lock rust/vendor scripts/keyboard-acceptance.py \
        README.md lisp/implementation.lisp display-relay/tests/golden.lisp
git commit -m "feat(ratatui): drop the patched crossterm; check hang-up and stalls"
```

---

## After the plan

Not tasks here, but what ADR 0021 leaves open:

- Upstream to termina: name 0x1C–0x1F as `\`, `]`, `^`, `_` (crossterm PR #1036's change), and parse keyboard flag replies as decimal, rejecting malformed ones (crossterm PR #1135's change). Once a termina release carries the first, restore the raw `\x1d` in `keyboard-acceptance.py`'s `enhanced` check.
- Close or update crossterm PR #1135 as the owner sees fit.
