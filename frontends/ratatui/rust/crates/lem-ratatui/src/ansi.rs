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
        assert_eq!(
            written(|out| underline_color(out, Color::Reset)),
            "\x1b[59m"
        );
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
            assert_eq!(
                written(|out| sgr(out, attribute)),
                expected,
                "{attribute:?}"
            );
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
            assert_eq!(
                ours(|out| foreground(out, color)),
                theirs(SetForegroundColor(c))
            );
            assert_eq!(
                ours(|out| background(out, color)),
                theirs(SetBackgroundColor(c))
            );
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
