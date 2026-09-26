//! Turning `lem.relay.v1` ops into cells.

use std::collections::HashMap;

use lem_protocol::v1;
use ratatui_core::buffer::Buffer;
use ratatui_core::style::{Color, Modifier, Style};

/// A packed `0xRRGGBB`.
pub fn color(rgb: u32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::Rgb(r, g, b)
}

/// A relay style as a cell style. An absent colour stays unset, so the
/// default colours show through (ADR 0012): the theme's once it sets them,
/// applied when compositing, or the terminal's own.
pub fn style_of(style: &v1::Style) -> Style {
    let mut out = Style::default();
    if let Some(fg) = style.foreground {
        out = out.fg(color(fg));
    }
    if let Some(bg) = style.background {
        out = out.bg(color(bg));
    }
    if style.bold {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.reverse {
        out = out.add_modifier(Modifier::REVERSED);
    }
    // Font styles (ADR 0015).
    if style.italic {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.strikethrough {
        out = out.add_modifier(Modifier::CROSSED_OUT);
    }
    if style.dim {
        out = out.add_modifier(Modifier::DIM);
    }
    if style.underline {
        out = out.add_modifier(Modifier::UNDERLINED);
        if let Some(underline) = style.underline_color {
            out = out.underline_color(color(underline));
        }
    }
    out
}

/// The session's styles, by id. The relay defines each in the first frame
/// that uses it; 0, and any id not defined, is no style.
#[derive(Default)]
pub struct Styles {
    table: HashMap<u32, Style>,
}

impl Styles {
    pub fn define(&mut self, style: &v1::Style) {
        self.table.insert(style.id, style_of(style));
    }

    pub fn get(&self, id: u32) -> Style {
        self.table.get(&id).copied().unwrap_or_default()
    }
}

/// Paint a run of text into exactly `width` cells from (`x`, `y`).
///
/// `width` is what Lem laid the text out as taking, and it is
/// authoritative (ADR 0012): text wider than that is clipped, and text
/// narrower is padded with blanks in its style, so a character whose width
/// the two sides disagree on cannot shift the rest of the line.
///
/// `Buffer::set_stringn` does the grapheme work: combining marks stay
/// attached, wide characters take two cells with the second blanked, and
/// it clips at `width` and at the right edge. It indexes the row directly,
/// so the row bound is checked here.
pub fn put(buffer: &mut Buffer, x: u16, y: u16, text: &str, width: u16, style: Style) {
    let area = buffer.area;
    if y >= area.height || x >= area.width {
        return;
    }
    let (end, _) = buffer.set_stringn(x, y, text, usize::from(width), style);
    let limit = x.saturating_add(width).min(area.width);
    for cx in end..limit {
        buffer[(cx, y)].reset();
        buffer[(cx, y)].set_style(style);
    }
}

/// Blank the rest of row `y` from column `x`.
pub fn clear_eol(buffer: &mut Buffer, x: u16, y: u16) {
    let area = buffer.area;
    if y >= area.height {
        return;
    }
    for cx in x..area.width {
        buffer[(cx, y)].reset();
    }
}

/// Blank every row from `y` downwards.
pub fn clear_eob(buffer: &mut Buffer, y: u16) {
    let area = buffer.area;
    for cy in y..area.height {
        for cx in 0..area.width {
            buffer[(cx, cy)].reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::layout::Rect;

    fn buffer(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    #[test]
    fn packed_colours_unpack() {
        assert_eq!(color(0xAABBCC), Color::Rgb(0xAA, 0xBB, 0xCC));
        assert_eq!(
            color(0),
            Color::Rgb(0, 0, 0),
            "black is a colour, not a default"
        );
    }

    #[test]
    fn a_style_maps_colours_and_attributes() {
        let style = style_of(&v1::Style {
            id: 1,
            foreground: Some(0xFF0000),
            bold: true,
            reverse: true,
            underline: true,
            underline_color: Some(0x00FF00),
            ..Default::default()
        });
        assert_eq!(style.fg, Some(Color::Rgb(0xFF, 0, 0)));
        assert_eq!(style.bg, None, "absent stays unset, for the defaults");
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(style.underline_color, Some(Color::Rgb(0, 0xFF, 0)));
    }

    #[test]
    fn font_styles_map_to_modifiers() {
        let style = style_of(&v1::Style {
            id: 1,
            italic: true,
            strikethrough: true,
            dim: true,
            ..Default::default()
        });
        for modifier in [Modifier::ITALIC, Modifier::CROSSED_OUT, Modifier::DIM] {
            assert!(style.add_modifier.contains(modifier), "{modifier:?}");
        }
        assert!(
            !style_of(&v1::Style::default())
                .add_modifier
                .contains(Modifier::ITALIC)
        );
    }

    #[test]
    fn undefined_style_ids_are_no_style() {
        let mut styles = Styles::default();
        styles.define(&v1::Style {
            id: 3,
            bold: true,
            ..Default::default()
        });
        assert!(styles.get(3).add_modifier.contains(Modifier::BOLD));
        assert_eq!(styles.get(0), Style::default());
        assert_eq!(styles.get(9), Style::default());
    }

    #[test]
    fn text_goes_where_it_is_put() {
        let mut b = buffer(10, 2);
        put(&mut b, 2, 1, "hi", 2, Style::default());
        assert_eq!(b[(2, 1)].symbol(), "h");
        assert_eq!(b[(3, 1)].symbol(), "i");
    }

    #[test]
    fn a_wide_character_takes_two_cells() {
        let mut b = buffer(6, 1);
        put(&mut b, 1, 0, "\u{1F512}", 2, Style::default());
        put(&mut b, 3, 0, "ab", 2, Style::default());
        assert_eq!(b[(1, 0)].symbol(), "\u{1F512}");
        assert_eq!(b[(3, 0)].symbol(), "a", "the next run starts two cells on");
    }

    #[test]
    fn a_run_is_padded_to_its_width() {
        // Lem said 4 cells; the text takes 2 here. The rest is blank in
        // the run's style, not whatever was there before.
        let mut b = buffer(6, 1);
        put(&mut b, 0, 0, "xxxxxx", 6, Style::default());
        let red = Style::default().bg(Color::Rgb(0xFF, 0, 0));
        put(&mut b, 0, 0, "ab", 4, red);
        assert_eq!(b[(2, 0)].symbol(), " ");
        assert_eq!(b[(3, 0)].bg, Color::Rgb(0xFF, 0, 0));
        assert_eq!(b[(4, 0)].symbol(), "x", "nothing past the width");
    }

    #[test]
    fn a_run_is_clipped_to_its_width() {
        let mut b = buffer(6, 1);
        put(&mut b, 0, 0, "abcdef", 3, Style::default());
        assert_eq!(b[(2, 0)].symbol(), "c");
        assert_eq!(b[(3, 0)].symbol(), " ", "clipped at 3");
    }

    #[test]
    fn writing_off_the_buffer_does_not_panic() {
        let mut b = buffer(4, 2);
        put(&mut b, 3, 0, "long", 4, Style::default());
        put(&mut b, 0, 9, "off-screen", 10, Style::default());
        put(&mut b, 9, 0, "off-screen", 10, Style::default());
        assert_eq!(b[(3, 0)].symbol(), "l");
    }

    #[test]
    fn clearing_blanks_what_it_says() {
        let mut b = buffer(5, 3);
        for y in 0..3 {
            put(&mut b, 0, y, "abcde", 5, Style::default());
        }
        clear_eol(&mut b, 2, 0);
        assert_eq!(b[(1, 0)].symbol(), "b");
        assert_eq!(b[(2, 0)].symbol(), " ");
        clear_eob(&mut b, 2);
        assert_eq!(b[(0, 1)].symbol(), "a", "row 1 untouched");
        assert_eq!(b[(0, 2)].symbol(), " ");
        clear_eol(&mut b, 0, 9);
        clear_eob(&mut b, 9);
    }
}
