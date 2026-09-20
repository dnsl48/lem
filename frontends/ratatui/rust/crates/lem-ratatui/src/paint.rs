//! Turning protocol paint commands into cells.

use lem_protocol::{Attribute, Put, Underline};
use ratatui_core::style::{Color, Modifier, Style};

use crate::views::ViewBuffer;

/// Parse a `"#RRGGBB"` wire colour.
///
/// Returns `None` for anything else, including the nulls the wire sends
/// for "no colour set" — those leave the style's field unset so the
/// terminal's own default shows through.
pub fn parse_color(raw: &str) -> Option<Color> {
    let hex = raw.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let component = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    Some(Color::Rgb(
        component(0..2)?,
        component(2..4)?,
        component(4..6)?,
    ))
}

/// Map a protocol attribute onto a cell style.
pub fn style_of(attribute: &Attribute) -> Style {
    let mut style = Style::default();
    if let Some(fg) = attribute.foreground.as_deref().and_then(parse_color) {
        style = style.fg(fg);
    }
    if let Some(bg) = attribute.background.as_deref().and_then(parse_color) {
        style = style.bg(bg);
    }
    if attribute.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if attribute.reverse {
        style = style.add_modifier(Modifier::REVERSED);
    }
    match &attribute.underline {
        Some(Underline::On(true)) => style = style.add_modifier(Modifier::UNDERLINED),
        Some(Underline::Color(raw)) => {
            style = style.add_modifier(Modifier::UNDERLINED);
            if let Some(color) = parse_color(raw) {
                style = style.underline_color(color);
            }
        }
        _ => {}
    }
    style
}

/// Paint one run of text into a view.
///
/// `Buffer::set_stringn` does the hard part: it splits on graphemes, so
/// combining marks stay attached, advances by each grapheme's display
/// width, blanks the trailing cell of a double-width one, and clips at
/// the right edge. Lem's startup modeline contains U+1F512, so this is
/// load-bearing from the first frame, not a refinement.
///
/// It does *not* bound-check the row — it indexes `y` directly and would
/// panic — so that check is here.
pub fn put(vb: &mut ViewBuffer, put: &Put) {
    if put.y >= vb.buffer.area.height {
        return;
    }
    let style = put.attribute.as_ref().map(style_of).unwrap_or_default();
    vb.buffer
        .set_stringn(put.x, put.y, &put.text, put.text_width as usize, style);
}

/// Blank the rest of row `y` from column `x`.
pub fn clear_eol(vb: &mut ViewBuffer, x: u16, y: u16) {
    let area = vb.buffer.area;
    if y >= area.height {
        return;
    }
    for cx in x..area.width {
        vb.buffer[(cx, y)].reset();
    }
}

/// Blank every row from `y` downwards.
pub fn clear_eob(vb: &mut ViewBuffer, y: u16) {
    let area = vb.buffer.area;
    for cy in y..area.height {
        for cx in 0..area.width {
            vb.buffer[(cx, cy)].reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lem_protocol::{View, ViewInfo, ViewKind, ViewType};
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;

    fn view_buffer(w: u16, h: u16) -> ViewBuffer {
        ViewBuffer {
            view: View {
                id: 1,
                x: 0,
                y: 0,
                width: w,
                height: h,
                use_modeline: None,
                kind: ViewKind::Tile,
                content_type: ViewType::Editor,
            },
            buffer: Buffer::empty(Rect::new(0, 0, w, h)),
        }
    }

    fn put_at(x: u16, y: u16, text: &str, width: u16, attribute: Option<Attribute>) -> Put {
        Put {
            view_info: ViewInfo { id: 1 },
            x,
            y,
            text: text.to_string(),
            text_width: width,
            attribute,
        }
    }

    #[test]
    fn parses_wire_colors() {
        assert_eq!(parse_color("#AABBCC"), Some(Color::Rgb(0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_color("#000000"), Some(Color::Rgb(0, 0, 0)));
        assert_eq!(parse_color("nonsense"), None);
        assert_eq!(parse_color("#AABB"), None, "short hex");
        assert_eq!(parse_color("#GGHHII"), None, "non-hex digits");
    }

    #[test]
    fn writes_text_at_a_cell_position() {
        let mut vb = view_buffer(10, 2);
        put(&mut vb, &put_at(2, 1, "hi", 2, None));
        assert_eq!(vb.buffer[(2, 1)].symbol(), "h");
        assert_eq!(vb.buffer[(3, 1)].symbol(), "i");
    }

    #[test]
    fn applies_colors_and_bold() {
        let attribute = Attribute {
            foreground: Some("#FF0000".into()),
            background: Some("#000000".into()),
            bold: true,
            ..Attribute::default()
        };
        let style = style_of(&attribute);
        assert_eq!(style.fg, Some(Color::Rgb(0xFF, 0, 0)));
        assert_eq!(style.bg, Some(Color::Rgb(0, 0, 0)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_null_colour_leaves_the_style_unset() {
        // Both fields are genuinely null on the wire for default text.
        let style = style_of(&Attribute::default());
        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
    }

    #[test]
    fn reverse_and_underline_map_to_modifiers() {
        let style = style_of(&Attribute {
            reverse: true,
            underline: Some(Underline::On(true)),
            ..Attribute::default()
        });
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn underline_color_is_carried_through() {
        let style = style_of(&Attribute {
            underline: Some(Underline::Color("#00FF00".into())),
            ..Attribute::default()
        });
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(style.underline_color, Some(Color::Rgb(0, 0xFF, 0)));
    }

    #[test]
    fn a_wide_character_occupies_two_cells() {
        // The capture's modeline carries U+1F512 with textWidth 2. Writing
        // it as one cell would shift everything after it left by one.
        let mut vb = view_buffer(6, 1);
        put(&mut vb, &put_at(1, 0, "\u{1F512}", 2, None));
        put(&mut vb, &put_at(3, 0, "ab", 2, None));
        assert_eq!(vb.buffer[(1, 0)].symbol(), "\u{1F512}");
        // The trailing cell is reset, not emptied: Cell::reset gives it a
        // space. What matters is that it holds no glyph of its own and the
        // next grapheme starts two cells along, not one.
        assert_eq!(vb.buffer[(2, 0)].symbol(), " ", "continuation cell");
        assert_eq!(vb.buffer[(3, 0)].symbol(), "a");
        assert_eq!(vb.buffer[(4, 0)].symbol(), "b");
    }

    #[test]
    fn writing_past_the_right_edge_does_not_panic() {
        let mut vb = view_buffer(4, 1);
        put(&mut vb, &put_at(3, 0, "long", 4, None));
        assert_eq!(vb.buffer[(3, 0)].symbol(), "l");
    }

    #[test]
    fn writing_past_the_bottom_edge_does_not_panic() {
        // set_stringn clips x but indexes y directly, so the row bound is
        // ours to enforce.
        let mut vb = view_buffer(4, 2);
        put(&mut vb, &put_at(0, 9, "off-screen", 10, None));
        assert_eq!(vb.buffer[(0, 0)].symbol(), " ");
    }

    #[test]
    fn clear_eol_blanks_the_rest_of_one_row_only() {
        let mut vb = view_buffer(5, 2);
        put(&mut vb, &put_at(0, 0, "abcde", 5, None));
        put(&mut vb, &put_at(0, 1, "fghij", 5, None));
        clear_eol(&mut vb, 2, 0);
        assert_eq!(vb.buffer[(1, 0)].symbol(), "b");
        assert_eq!(vb.buffer[(2, 0)].symbol(), " ");
        assert_eq!(vb.buffer[(4, 0)].symbol(), " ");
        assert_eq!(vb.buffer[(2, 1)].symbol(), "h", "row 1 untouched");
    }

    #[test]
    fn clear_eob_blanks_every_row_from_y_down() {
        let mut vb = view_buffer(3, 3);
        for y in 0..3 {
            put(&mut vb, &put_at(0, y, "xyz", 3, None));
        }
        clear_eob(&mut vb, 1);
        assert_eq!(vb.buffer[(0, 0)].symbol(), "x");
        assert_eq!(vb.buffer[(0, 1)].symbol(), " ");
        assert_eq!(vb.buffer[(0, 2)].symbol(), " ");
    }

    #[test]
    fn clearing_out_of_range_rows_is_a_no_op() {
        let mut vb = view_buffer(3, 2);
        clear_eol(&mut vb, 0, 9);
        clear_eob(&mut vb, 9);
        assert_eq!(vb.buffer[(0, 0)].symbol(), " ");
    }
}
