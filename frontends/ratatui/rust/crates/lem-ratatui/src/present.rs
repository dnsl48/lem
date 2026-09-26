//! Putting composited frames on the terminal.
//!
//! This replaces Ratatui's `Terminal` and `ratatui-crossterm`'s backend
//! (ADR 0016), which cannot draw an underline's style: Ratatui's cell has
//! one underline flag. What is kept from Ratatui is its buffer, with its
//! grapheme, wide-character and clipping handling, and its diff. The
//! underline style lives beside the buffer (`paint::Layer`), so a cell is
//! redrawn when Ratatui's diff says so *or* its underline style changed.
//!
//! The writing follows `ratatui-crossterm`'s: move only when not already
//! there, then change only what differs from the cell before.

use std::collections::BTreeSet;
use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color as CColor, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
    SetUnderlineColor,
};
use crossterm::terminal::{Clear, ClearType};
use lem_protocol::v1::{CursorShape, UnderlineStyle};
use ratatui_core::buffer::Cell;
use ratatui_core::layout::Position;
use ratatui_core::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::paint::Layer;

/// Writes frames to the terminal, each as the difference from the last.
pub struct Presenter<W: Write> {
    out: W,
    previous: Option<Layer>,
    styled_underlines: bool,
    shape: Option<CursorShape>,
}

impl<W: Write> Presenter<W> {
    /// `styled_underlines`: whether the terminal draws curly and the other
    /// underline styles (`support::styled_underlines`); when not, they are
    /// drawn straight.
    pub fn new(out: W, styled_underlines: bool) -> Self {
        Self {
            out,
            previous: None,
            styled_underlines,
            shape: None,
        }
    }

    /// Put `frame` on the terminal, then the cursor: shown at `cursor`
    /// with `shape`, or hidden.
    pub fn present(
        &mut self,
        frame: &Layer,
        cursor: Option<Position>,
        shape: Option<CursorShape>,
    ) -> io::Result<()> {
        queue!(self.out, Hide)?;
        let previous = match self.previous.take() {
            Some(previous) if previous.area() == frame.area() => previous,
            // First frame, or a new size: start from a cleared screen,
            // which is what an empty layer describes.
            _ => {
                queue!(self.out, Clear(ClearType::All))?;
                Layer::new(frame.area().width, frame.area().height)
            }
        };
        let positions = changed(&previous, frame);
        self.write_cells(frame, &positions)?;

        if shape != self.shape {
            if let Some(shape) = shape {
                queue!(self.out, cursor_style(shape))?;
            }
            self.shape = shape;
        }
        if let Some(position) = cursor {
            queue!(self.out, MoveTo(position.x, position.y), Show)?;
        }
        self.out.flush()?;
        self.previous = Some(frame.clone());
        Ok(())
    }

    fn write_cells(&mut self, frame: &Layer, positions: &[(u16, u16)]) -> io::Result<()> {
        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut underline_color = Color::Reset;
        let mut modifier = Modifier::empty();
        let mut underline: Option<UnderlineStyle> = None;
        let mut next: Option<(u16, u16)> = None;

        for &(x, y) in positions {
            let cell = &frame.cells[(x, y)];
            if next != Some((x, y)) {
                queue!(self.out, MoveTo(x, y))?;
            }
            let wanted = cell.modifier - Modifier::UNDERLINED;
            if wanted != modifier {
                queue_modifier_diff(&mut self.out, modifier, wanted)?;
                modifier = wanted;
            }
            let wanted = cell
                .modifier
                .contains(Modifier::UNDERLINED)
                .then(|| frame.underline.get(x, y));
            if wanted != underline {
                let attribute = self.underline_attribute(wanted);
                queue!(self.out, SetAttribute(attribute))?;
                underline = wanted;
            }
            if cell.fg != fg {
                queue!(self.out, SetForegroundColor(crossterm_color(cell.fg)))?;
                fg = cell.fg;
            }
            if cell.bg != bg {
                queue!(self.out, SetBackgroundColor(crossterm_color(cell.bg)))?;
                bg = cell.bg;
            }
            if cell.underline_color != underline_color {
                queue!(
                    self.out,
                    SetUnderlineColor(crossterm_color(cell.underline_color))
                )?;
                underline_color = cell.underline_color;
            }
            queue!(self.out, Print(cell.symbol()))?;
            next = Some((x.saturating_add(cell_width(cell)), y));
        }
        queue!(
            self.out,
            SetForegroundColor(CColor::Reset),
            SetBackgroundColor(CColor::Reset),
            SetUnderlineColor(CColor::Reset),
            SetAttribute(Attribute::Reset)
        )
    }

    fn underline_attribute(&self, underline: Option<UnderlineStyle>) -> Attribute {
        match underline {
            None => Attribute::NoUnderline,
            Some(_) if !self.styled_underlines => Attribute::Underlined,
            Some(UnderlineStyle::Straight) => Attribute::Underlined,
            Some(UnderlineStyle::Curly) => Attribute::Undercurled,
            Some(UnderlineStyle::Dotted) => Attribute::Underdotted,
            Some(UnderlineStyle::Dashed) => Attribute::Underdashed,
            Some(UnderlineStyle::Double) => Attribute::DoubleUnderlined,
        }
    }
}

/// How many columns a cell's symbol takes: 1 at least, 2 for a wide one.
fn cell_width(cell: &Cell) -> u16 {
    u16::try_from(cell.symbol().width()).unwrap_or(1).max(1)
}

/// Every position to redraw, in row order: what Ratatui's diff finds, and
/// underlined cells whose underline style changed, which it cannot see.
/// A cell covered by the wide character to its left is not a position of
/// its own: printing it would split that character.
fn changed(previous: &Layer, frame: &Layer) -> Vec<(u16, u16)> {
    let mut positions: BTreeSet<(u16, u16)> = previous
        .cells
        .diff(&frame.cells)
        .into_iter()
        .map(|(x, y, _)| (y, x))
        .collect();
    let area = frame.area();
    for y in 0..area.height {
        let mut x = 0;
        while x < area.width {
            let cell = &frame.cells[(x, y)];
            let underlined = cell.modifier.contains(Modifier::UNDERLINED)
                || previous.cells[(x, y)]
                    .modifier
                    .contains(Modifier::UNDERLINED);
            if underlined && previous.underline.get(x, y) != frame.underline.get(x, y) {
                positions.insert((y, x));
            }
            x = x.saturating_add(cell_width(cell));
        }
    }
    positions.into_iter().map(|(y, x)| (x, y)).collect()
}

/// Change the modifiers other than underlining, as `ratatui-crossterm`
/// does: bold and dim share one reset, so what remains is re-applied.
fn queue_modifier_diff(out: &mut impl Write, from: Modifier, to: Modifier) -> io::Result<()> {
    let removed = from - to;
    if removed.contains(Modifier::REVERSED) {
        queue!(out, SetAttribute(Attribute::NoReverse))?;
    }
    let reset_intensity = removed.contains(Modifier::BOLD) || removed.contains(Modifier::DIM);
    if reset_intensity {
        queue!(out, SetAttribute(Attribute::NormalIntensity))?;
        if to.contains(Modifier::DIM) {
            queue!(out, SetAttribute(Attribute::Dim))?;
        }
        if to.contains(Modifier::BOLD) {
            queue!(out, SetAttribute(Attribute::Bold))?;
        }
    }
    if removed.contains(Modifier::ITALIC) {
        queue!(out, SetAttribute(Attribute::NoItalic))?;
    }
    if removed.contains(Modifier::CROSSED_OUT) {
        queue!(out, SetAttribute(Attribute::NotCrossedOut))?;
    }
    if removed.contains(Modifier::HIDDEN) {
        queue!(out, SetAttribute(Attribute::NoHidden))?;
    }
    if removed.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK) {
        queue!(out, SetAttribute(Attribute::NoBlink))?;
    }

    let added = to - from;
    if added.contains(Modifier::REVERSED) {
        queue!(out, SetAttribute(Attribute::Reverse))?;
    }
    if added.contains(Modifier::BOLD) && !reset_intensity {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    if added.contains(Modifier::DIM) && !reset_intensity {
        queue!(out, SetAttribute(Attribute::Dim))?;
    }
    if added.contains(Modifier::ITALIC) {
        queue!(out, SetAttribute(Attribute::Italic))?;
    }
    if added.contains(Modifier::CROSSED_OUT) {
        queue!(out, SetAttribute(Attribute::CrossedOut))?;
    }
    if added.contains(Modifier::HIDDEN) {
        queue!(out, SetAttribute(Attribute::Hidden))?;
    }
    if added.contains(Modifier::SLOW_BLINK) {
        queue!(out, SetAttribute(Attribute::SlowBlink))?;
    }
    if added.contains(Modifier::RAPID_BLINK) {
        queue!(out, SetAttribute(Attribute::RapidBlink))?;
    }
    Ok(())
}

/// Ratatui's colour as crossterm's, as `ratatui-crossterm` maps it.
fn crossterm_color(color: Color) -> CColor {
    match color {
        Color::Reset => CColor::Reset,
        Color::Black => CColor::Black,
        Color::Red => CColor::DarkRed,
        Color::Green => CColor::DarkGreen,
        Color::Yellow => CColor::DarkYellow,
        Color::Blue => CColor::DarkBlue,
        Color::Magenta => CColor::DarkMagenta,
        Color::Cyan => CColor::DarkCyan,
        Color::Gray => CColor::Grey,
        Color::DarkGray => CColor::DarkGrey,
        Color::LightRed => CColor::Red,
        Color::LightGreen => CColor::Green,
        Color::LightBlue => CColor::Blue,
        Color::LightYellow => CColor::Yellow,
        Color::LightMagenta => CColor::Magenta,
        Color::LightCyan => CColor::Cyan,
        Color::White => CColor::White,
        Color::Indexed(i) => CColor::AnsiValue(i),
        Color::Rgb(r, g, b) => CColor::Rgb { r, g, b },
    }
}

fn cursor_style(shape: CursorShape) -> SetCursorStyle {
    match shape {
        CursorShape::Bar => SetCursorStyle::SteadyBar,
        CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
        CursorShape::Box | CursorShape::Unspecified => SetCursorStyle::SteadyBlock,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Paint;
    use ratatui_core::style::Style;

    fn underlined(style: UnderlineStyle) -> Paint {
        Paint {
            style: Style::default().add_modifier(Modifier::UNDERLINED),
            underline: style,
        }
    }

    fn written(presenter: &mut Presenter<Vec<u8>>, frame: &Layer) -> String {
        presenter.out.clear();
        presenter.present(frame, None, None).unwrap();
        String::from_utf8(presenter.out.clone()).unwrap()
    }

    fn frame_with(text: &str, paint: Paint) -> Layer {
        let mut layer = Layer::new(10, 2);
        layer.put(0, 0, text, text.chars().count() as u16, paint);
        layer
    }

    #[test]
    fn a_curly_underline_is_sgr_4_3() {
        let mut presenter = Presenter::new(Vec::new(), true);
        let out = written(
            &mut presenter,
            &frame_with("err", underlined(UnderlineStyle::Curly)),
        );
        assert!(out.contains("\x1b[4:3m"), "{out:?}");
        assert!(out.contains("err"));
    }

    #[test]
    fn every_style_has_its_sequence() {
        for (style, sgr) in [
            (UnderlineStyle::Straight, "\x1b[4m"),
            (UnderlineStyle::Double, "\x1b[4:2m"),
            (UnderlineStyle::Curly, "\x1b[4:3m"),
            (UnderlineStyle::Dotted, "\x1b[4:4m"),
            (UnderlineStyle::Dashed, "\x1b[4:5m"),
        ] {
            let mut presenter = Presenter::new(Vec::new(), true);
            let out = written(&mut presenter, &frame_with("x", underlined(style)));
            assert!(out.contains(sgr), "{style:?}: {out:?}");
        }
    }

    #[test]
    fn without_support_every_style_is_straight() {
        let mut presenter = Presenter::new(Vec::new(), false);
        let out = written(
            &mut presenter,
            &frame_with("err", underlined(UnderlineStyle::Curly)),
        );
        assert!(out.contains("\x1b[4m"), "{out:?}");
        assert!(!out.contains("\x1b[4:"), "{out:?}");
    }

    #[test]
    fn a_change_of_underline_style_alone_is_redrawn() {
        // Ratatui's diff cannot see it: the cells are identical.
        let mut presenter = Presenter::new(Vec::new(), true);
        written(
            &mut presenter,
            &frame_with("err", underlined(UnderlineStyle::Straight)),
        );
        let out = written(
            &mut presenter,
            &frame_with("err", underlined(UnderlineStyle::Curly)),
        );
        assert!(out.contains("\x1b[4:3m"), "{out:?}");
        assert!(out.contains("err"), "{out:?}");
    }

    #[test]
    fn an_unchanged_frame_writes_no_cells() {
        let mut presenter = Presenter::new(Vec::new(), true);
        let frame = frame_with("same", underlined(UnderlineStyle::Curly));
        written(&mut presenter, &frame);
        let out = written(&mut presenter, &frame);
        assert!(!out.contains("same"), "{out:?}");
    }

    #[test]
    fn nothing_writes_blink_or_hidden() {
        let mut presenter = Presenter::new(Vec::new(), true);
        let out = written(
            &mut presenter,
            &frame_with("err", underlined(UnderlineStyle::Curly)),
        );
        for sgr in ["\x1b[5m", "\x1b[6m", "\x1b[8m"] {
            assert!(!out.contains(sgr), "{sgr:?} in {out:?}");
        }
    }

    #[test]
    fn a_wide_character_is_not_split_by_an_underline_change() {
        let mut presenter = Presenter::new(Vec::new(), true);
        written(
            &mut presenter,
            &frame_with("日", underlined(UnderlineStyle::Straight)),
        );
        let mut frame = Layer::new(10, 2);
        frame.put(0, 0, "日", 2, underlined(UnderlineStyle::Curly));
        let out = written(&mut presenter, &frame);
        assert_eq!(out.matches("日").count(), 1, "{out:?}");
        assert!(
            !out.contains("\x1b[1;2H"),
            "no write into its second column: {out:?}"
        );
    }

    #[test]
    fn the_cursor_is_shown_where_asked_and_shaped() {
        let mut presenter = Presenter::new(Vec::new(), true);
        presenter
            .present(
                &Layer::new(4, 2),
                Some(Position::new(2, 1)),
                Some(CursorShape::Bar),
            )
            .unwrap();
        let out = String::from_utf8(presenter.out.clone()).unwrap();
        assert!(out.ends_with("\x1b[2;3H\x1b[?25h"), "{out:?}");
        assert!(out.contains("\x1b[6 q"), "steady bar: {out:?}");
    }

    #[test]
    fn a_new_size_starts_from_a_cleared_screen() {
        let mut presenter = Presenter::new(Vec::new(), true);
        written(&mut presenter, &Layer::new(4, 2));
        let out = written(&mut presenter, &Layer::new(6, 3));
        assert!(out.contains("\x1b[2J"), "{out:?}");
    }
}
