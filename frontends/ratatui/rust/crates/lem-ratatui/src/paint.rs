//! Turning `lem.relay.v1` ops into cells.

use std::collections::HashMap;
use std::sync::Arc;

use lem_protocol::v1::{self, UnderlineStyle};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};

/// A packed `0xRRGGBB`.
pub fn color(rgb: u32) -> Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color::Rgb(r, g, b)
}

/// How a relay style paints a cell: a Ratatui style, and the underline
/// style, which Ratatui's style cannot hold (ADR 0016).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Paint {
    pub style: Style,
    pub underline: UnderlineStyle,
}

/// A relay style as a cell style. An absent colour stays unset, so the
/// default colours show through (ADR 0012): the theme's once it sets them,
/// applied when compositing, or the terminal's own.
pub fn paint_of(style: &v1::Style) -> Paint {
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
    Paint {
        style: out,
        underline: style.underline_style(),
    }
}

/// The session's styles, by id. The relay defines each in the first frame
/// that uses it; 0, and any id not defined, is no style.
#[derive(Default)]
pub struct Styles {
    table: HashMap<u32, Paint>,
}

impl Styles {
    pub fn define(&mut self, style: &v1::Style) {
        self.table.insert(style.id, paint_of(style));
    }

    pub fn get(&self, id: u32) -> Paint {
        self.table.get(&id).copied().unwrap_or_default()
    }
}

/// The URL a cell links to (ADR 0018). Shared: every cell of a link, and
/// every copy of the layer, holds the same one.
pub type Link = Arc<str>;

/// The longest URL made a link. Terminals cap them too (kitty and VTE at
/// about 2 KiB); a longer one is drawn as plain text.
pub const LINK_LIMIT: usize = 2048;

/// `url` as a link, if it can be one: non-empty, within `LINK_LIMIT`, and
/// printable ASCII only, as OSC 8 requires. The URL comes from buffer
/// text, so anything else, an escape above all, could reach the terminal
/// as a command; the relay drops such URLs, and so does the display.
pub fn link_of(url: &str) -> Option<Link> {
    let printable = url.bytes().all(|b| (0x20..=0x7E).contains(&b));
    (printable && !url.is_empty() && url.len() <= LINK_LIMIT).then(|| Arc::from(url))
}

/// One value per cell, beside a buffer of the same size: what Ratatui's
/// cell cannot hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid<T> {
    width: u16,
    values: Vec<T>,
}

impl<T: Clone> Grid<T> {
    pub fn new(width: u16, height: u16, value: T) -> Self {
        Self {
            width,
            values: vec![value; usize::from(width) * usize::from(height)],
        }
    }

    fn index(&self, x: u16, y: u16) -> usize {
        usize::from(y) * usize::from(self.width) + usize::from(x)
    }

    pub fn get(&self, x: u16, y: u16) -> &T {
        &self.values[self.index(x, y)]
    }

    pub fn set(&mut self, x: u16, y: u16, value: T) {
        let i = self.index(x, y);
        self.values[i] = value;
    }
}

/// Cells, with each one's underline style and link beside them: what a
/// view paints into and what the screen is composited into (ADR 0016,
/// 0018). Every operation here keeps the three in step.
///
/// The underline style only shows where the cell is underlined.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub cells: Buffer,
    pub underline: Grid<UnderlineStyle>,
    pub link: Grid<Option<Link>>,
}

impl Layer {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            cells: Buffer::empty(Rect::new(0, 0, width, height)),
            underline: Grid::new(width, height, UnderlineStyle::Straight),
            link: Grid::new(width, height, None),
        }
    }

    /// Copy cell (`sx`, `sy`) of `source`, with its underline style and
    /// link, to (`x`, `y`).
    pub fn copy_cell(&mut self, x: u16, y: u16, source: &Layer, sx: u16, sy: u16) {
        self.cells[(x, y)] = source.cells[(sx, sy)].clone();
        self.underline.set(x, y, *source.underline.get(sx, sy));
        self.link.set(x, y, source.link.get(sx, sy).clone());
    }

    /// Set the symbol of cell (`x`, `y`), which is no longer part of any
    /// link: a border or separator drawn over text.
    pub fn set_symbol(&mut self, x: u16, y: u16, symbol: &str) {
        self.cells[(x, y)].set_symbol(symbol);
        self.link.set(x, y, None);
    }

    pub fn area(&self) -> Rect {
        self.cells.area
    }

    /// Paint a run of text into exactly `width` cells from (`x`, `y`).
    ///
    /// `width` is what Lem laid the text out as taking, and it is
    /// authoritative (ADR 0012): text wider than that is clipped, and text
    /// narrower is padded with blanks in its style, so a character whose
    /// width the two sides disagree on cannot shift the rest of the line.
    ///
    /// `Buffer::set_stringn` does the grapheme work: combining marks stay
    /// attached, wide characters take two cells with the second blanked,
    /// and it clips at `width` and at the right edge. It indexes the row
    /// directly, so the row bound is checked here.
    pub fn put(&mut self, x: u16, y: u16, text: &str, width: u16, paint: Paint) {
        self.put_linked(x, y, text, width, paint, None);
    }

    /// `put`, the run linking to `link` (ADR 0018).
    pub fn put_linked(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        width: u16,
        paint: Paint,
        link: Option<&Link>,
    ) {
        let area = self.area();
        if y >= area.height || x >= area.width {
            return;
        }
        let (end, _) = self
            .cells
            .set_stringn(x, y, text, usize::from(width), paint.style);
        let limit = x.saturating_add(width).min(area.width);
        for cx in end..limit {
            self.cells[(cx, y)].reset();
            self.cells[(cx, y)].set_style(paint.style);
        }
        for cx in x..limit.max(end) {
            self.underline.set(cx, y, paint.underline);
            self.link.set(cx, y, link.cloned());
        }
    }

    fn reset(&mut self, x: u16, y: u16) {
        self.cells[(x, y)].reset();
        self.underline.set(x, y, UnderlineStyle::Straight);
        self.link.set(x, y, None);
    }

    /// Blank the rest of row `y` from column `x`.
    pub fn clear_eol(&mut self, x: u16, y: u16) {
        let area = self.area();
        if y >= area.height {
            return;
        }
        for cx in x..area.width {
            self.reset(cx, y);
        }
    }

    /// Blank every row from `y` downwards.
    pub fn clear_eob(&mut self, y: u16) {
        let area = self.area();
        for cy in y..area.height {
            for cx in 0..area.width {
                self.reset(cx, cy);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Paint {
        Paint::default()
    }

    fn curly() -> Paint {
        Paint {
            underline: UnderlineStyle::Curly,
            ..Paint::default()
        }
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
        let paint = paint_of(&v1::Style {
            id: 1,
            foreground: Some(0xFF0000),
            bold: true,
            reverse: true,
            underline: true,
            underline_color: Some(0x00FF00),
            underline_style: UnderlineStyle::Curly as i32,
            ..Default::default()
        });
        let style = paint.style;
        assert_eq!(style.fg, Some(Color::Rgb(0xFF, 0, 0)));
        assert_eq!(style.bg, None, "absent stays unset, for the defaults");
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(style.underline_color, Some(Color::Rgb(0, 0xFF, 0)));
        assert_eq!(paint.underline, UnderlineStyle::Curly);
    }

    #[test]
    fn font_styles_map_to_modifiers() {
        let style = paint_of(&v1::Style {
            id: 1,
            italic: true,
            strikethrough: true,
            dim: true,
            ..Default::default()
        })
        .style;
        for modifier in [Modifier::ITALIC, Modifier::CROSSED_OUT, Modifier::DIM] {
            assert!(style.add_modifier.contains(modifier), "{modifier:?}");
        }
        assert!(
            !paint_of(&v1::Style::default())
                .style
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
        assert!(styles.get(3).style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(styles.get(0), Paint::default());
        assert_eq!(styles.get(9), Paint::default());
    }

    #[test]
    fn text_goes_where_it_is_put() {
        let mut layer = Layer::new(10, 2);
        layer.put(2, 1, "hi", 2, plain());
        assert_eq!(layer.cells[(2, 1)].symbol(), "h");
        assert_eq!(layer.cells[(3, 1)].symbol(), "i");
    }

    #[test]
    fn a_wide_character_takes_two_cells() {
        let mut layer = Layer::new(6, 1);
        layer.put(1, 0, "\u{1F512}", 2, plain());
        layer.put(3, 0, "ab", 2, plain());
        assert_eq!(layer.cells[(1, 0)].symbol(), "\u{1F512}");
        assert_eq!(
            layer.cells[(3, 0)].symbol(),
            "a",
            "the next run starts two cells on"
        );
    }

    #[test]
    fn a_run_is_padded_to_its_width() {
        // Lem said 4 cells; the text takes 2 here. The rest is blank in
        // the run's style, not whatever was there before.
        let mut layer = Layer::new(6, 1);
        layer.put(0, 0, "xxxxxx", 6, plain());
        let red = Paint {
            style: Style::default().bg(Color::Rgb(0xFF, 0, 0)),
            ..Paint::default()
        };
        layer.put(0, 0, "ab", 4, red);
        assert_eq!(layer.cells[(2, 0)].symbol(), " ");
        assert_eq!(layer.cells[(3, 0)].bg, Color::Rgb(0xFF, 0, 0));
        assert_eq!(layer.cells[(4, 0)].symbol(), "x", "nothing past the width");
    }

    #[test]
    fn a_run_is_clipped_to_its_width() {
        let mut layer = Layer::new(6, 1);
        layer.put(0, 0, "abcdef", 3, plain());
        assert_eq!(layer.cells[(2, 0)].symbol(), "c");
        assert_eq!(layer.cells[(3, 0)].symbol(), " ", "clipped at 3");
    }

    #[test]
    fn a_run_sets_its_underline_style_on_every_cell_it_takes() {
        let mut layer = Layer::new(8, 1);
        layer.put(1, 0, "ab", 4, curly());
        assert_eq!(*layer.underline.get(0, 0), UnderlineStyle::Straight);
        for x in 1..5 {
            assert_eq!(
                *layer.underline.get(x, 0),
                UnderlineStyle::Curly,
                "cell {x}"
            );
        }
        assert_eq!(*layer.underline.get(5, 0), UnderlineStyle::Straight);
    }

    #[test]
    fn clearing_blanks_cells_and_underline_styles() {
        let mut layer = Layer::new(4, 3);
        for y in 0..3 {
            layer.put(0, y, "abcd", 4, curly());
        }
        layer.clear_eol(2, 0);
        assert_eq!(layer.cells[(1, 0)].symbol(), "b");
        assert_eq!(*layer.underline.get(1, 0), UnderlineStyle::Curly);
        assert_eq!(layer.cells[(2, 0)].symbol(), " ");
        assert_eq!(*layer.underline.get(2, 0), UnderlineStyle::Straight);
        layer.clear_eob(2);
        assert_eq!(layer.cells[(0, 1)].symbol(), "a", "row 1 untouched");
        assert_eq!(*layer.underline.get(0, 2), UnderlineStyle::Straight);
    }

    #[test]
    fn writing_off_the_layer_does_not_panic() {
        let mut layer = Layer::new(4, 2);
        layer.put(3, 0, "long", 4, plain());
        layer.put(0, 9, "off-screen", 10, plain());
        layer.put(9, 0, "off-screen", 10, plain());
        layer.clear_eol(0, 9);
        layer.clear_eob(9);
        assert_eq!(layer.cells[(3, 0)].symbol(), "l");
    }

    #[test]
    fn only_printable_ascii_urls_are_links() {
        assert_eq!(
            link_of("https://example.com/a?b=c").as_deref(),
            Some("https://example.com/a?b=c")
        );
        assert!(link_of("").is_none());
        assert!(
            link_of("https://x/\x1b]8;;evil\x1b\\").is_none(),
            "an escape"
        );
        assert!(link_of("https://x/\x07").is_none(), "a bell ends OSC too");
        assert!(link_of("https://x/\u{e9}").is_none(), "not ASCII");
        assert!(link_of(&format!("https://x/{}", "a".repeat(LINK_LIMIT))).is_none());
    }

    #[test]
    fn a_linked_run_links_every_cell_it_takes() {
        let mut layer = Layer::new(8, 1);
        let link = link_of("https://example.com");
        layer.put_linked(1, 0, "ab", 4, plain(), link.as_ref());
        assert_eq!(*layer.link.get(0, 0), None);
        for x in 1..5 {
            assert_eq!(*layer.link.get(x, 0), link, "cell {x}");
        }
        assert_eq!(*layer.link.get(5, 0), None);
        layer.put(1, 0, "ab", 2, plain());
        assert_eq!(*layer.link.get(1, 0), None, "unlinked when repainted");
        layer.clear_eol(0, 0);
        assert_eq!(*layer.link.get(3, 0), None, "and when cleared");
    }
}
