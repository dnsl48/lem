//! Per-view cell buffers and the compositing step.
//!
//! The browser display half gets this for free: each view is its own
//! canvas and the browser composites them by z-index. A terminal has one
//! grid and composites nothing, so views are painted into separate
//! buffers and blitted here in layer order. See
//! `../../../docs/protocol-notes.md` section 7.

use lem_protocol::{BorderShape, View, ViewKind, ViewType};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::widgets::Widget;

/// A view and the cells painted into it.
///
/// The modeline is a *separate* one-row buffer rather than the view's
/// last row, because Lem treats it as an extra row below the view: the
/// browser client allocates `height + (useModeline ? 1 : 0)`, and every
/// `modeline-put` arrives at `y: 0` of its own coordinate space. Painting
/// it into the view buffer would overwrite the first line of the file.
pub struct ViewBuffer {
    pub view: View,
    pub buffer: Buffer,
    pub modeline: Buffer,
}

/// Every live view, in insertion order.
#[derive(Default)]
pub struct Registry {
    views: Vec<ViewBuffer>,
}

/// Copy `source` onto `screen` at (`x`, `y`), clipped to `area`.
fn blit(screen: &mut Buffer, source: &Buffer, x: u16, y: u16, area: Rect) {
    for sy in 0..source.area.height {
        let Some(ty) = y.checked_add(sy) else { return };
        if ty >= area.height {
            return;
        }
        for sx in 0..source.area.width {
            let Some(tx) = x.checked_add(sx) else { break };
            if tx >= area.width {
                break;
            }
            screen[(tx, ty)] = source[(sx, sy)].clone();
        }
    }
}

/// Drawn in the column Lem reserves to the left of each split.
const SEPARATOR: &str = "\u{2502}";

/// Copy one view and its modeline onto the screen.
fn blit_view(screen: &mut Buffer, vb: &ViewBuffer, area: Rect) {
    blit(screen, &vb.buffer, vb.view.x, vb.view.y, area);
    if vb.view.has_modeline() {
        // One row immediately below the view, not its last row.
        let y = vb.view.y.saturating_add(vb.view.height);
        blit(screen, &vb.modeline, vb.view.x, y, area);
    }
}

/// Draw the separator to the left of a split.
///
/// Lem reserves the column through `:window-left-margin`, and the browser
/// client draws its `VerticalBorder` there — half a cell left of the
/// view's origin, which in a terminal is the cell at `x - 1`. A view at
/// x = 0 has nothing to its left and gets none. The line spans the
/// modeline row too, matching `height + (useModeline ? 1 : 0)`.
fn draw_separator(screen: &mut Buffer, view: &View, area: Rect) {
    let Some(x) = view.x.checked_sub(1) else {
        return;
    };
    if x >= area.width {
        return;
    }
    let rows = view.height + u16::from(view.has_modeline());
    for row in 0..rows {
        let Some(y) = view.y.checked_add(row) else {
            return;
        };
        if y >= area.height {
            return;
        }
        screen[(x, y)].set_symbol(SEPARATOR);
    }
}

/// Box-drawing characters, matching `lem-ncurses/style`.
mod glyph {
    pub const HORIZONTAL: &str = "\u{2500}";
    pub const VERTICAL: &str = "\u{2502}";
    pub const TOP_LEFT: &str = "\u{256d}";
    pub const TOP_RIGHT: &str = "\u{256e}";
    pub const BOTTOM_RIGHT: &str = "\u{256f}";
    pub const BOTTOM_LEFT: &str = "\u{2570}";
    pub const TEE_RIGHT: &str = "\u{251c}";
    pub const TEE_LEFT: &str = "\u{2524}";
}

/// Write one cell, ignoring anything off-screen.
///
/// Signed coordinates because a border is drawn *outside* its view, and a
/// window flush against the top or left edge puts part of it at -1.
fn put_cell(screen: &mut Buffer, x: i32, y: i32, symbol: &str, area: Rect) {
    if x < 0 || y < 0 || x >= i32::from(area.width) || y >= i32::from(area.height) {
        return;
    }
    screen[(x as u16, y as u16)].set_symbol(symbol);
}

/// Draw a floating window's border.
///
/// The border lives outside the view, exactly as `lem-ncurses/view`
/// places it: the box is inset by `border` cells on every side, so a view
/// at (x, y) of w by h is ringed by a box at (x-border, y-border) of
/// (w + 2*border) by (h + 2*border). `left-border` is the exception — a
/// single rule down the left edge, spanning only the view's own height.
fn draw_border(screen: &mut Buffer, view: &View, area: Rect) {
    let Some(size) = view.border.filter(|size| *size > 0) else {
        return;
    };
    let (size, x, y) = (i32::from(size), i32::from(view.x), i32::from(view.y));
    let (w, h) = (i32::from(view.width), i32::from(view.height));

    if view.border_shape == Some(BorderShape::LeftBorder) {
        for row in 0..h {
            put_cell(screen, x - size, y + row, glyph::VERTICAL, area);
        }
        return;
    }

    let (left, top) = (x - size, y - size);
    let (right, bottom) = (left + w + 2 * size - 1, top + h + 2 * size - 1);

    // A drop curtain hangs from whatever is above it, so its top corners
    // join that line rather than turning away from it.
    let (tl, tr) = if view.border_shape == Some(BorderShape::DropCurtain) {
        (glyph::TEE_RIGHT, glyph::TEE_LEFT)
    } else {
        (glyph::TOP_LEFT, glyph::TOP_RIGHT)
    };

    put_cell(screen, left, top, tl, area);
    put_cell(screen, right, top, tr, area);
    put_cell(screen, left, bottom, glyph::BOTTOM_LEFT, area);
    put_cell(screen, right, bottom, glyph::BOTTOM_RIGHT, area);
    for column in (left + 1)..right {
        put_cell(screen, column, top, glyph::HORIZONTAL, area);
        put_cell(screen, column, bottom, glyph::HORIZONTAL, area);
    }
    for row in (top + 1)..bottom {
        put_cell(screen, left, row, glyph::VERTICAL, area);
        put_cell(screen, right, row, glyph::VERTICAL, area);
    }
}

/// Painting order. Tiles are the background, floating windows the top.
fn layer(kind: ViewKind) -> u8 {
    match kind {
        ViewKind::Tile => 0,
        ViewKind::Header => 1,
        ViewKind::Floating => 2,
    }
}

impl Registry {
    /// Add a view, replacing any existing one with the same id.
    pub fn insert(&mut self, view: View) {
        let buffer = Buffer::empty(Rect::new(0, 0, view.width, view.height));
        let modeline = Buffer::empty(Rect::new(0, 0, view.width, 1));
        self.remove(view.id);
        self.views.push(ViewBuffer {
            view,
            buffer,
            modeline,
        });
    }

    pub fn remove(&mut self, id: u64) {
        self.views.retain(|vb| vb.view.id != id);
    }

    /// How many views are tracked, html ones included.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut ViewBuffer> {
        self.views.iter_mut().find(|vb| vb.view.id == id)
    }

    /// Resize a view's buffer, discarding its contents.
    ///
    /// Lem repaints a resized view in the same frame, so there is nothing
    /// worth preserving and a stale-size buffer would mis-clip the writes
    /// that follow.
    pub fn resize(&mut self, id: u64, width: u16, height: u16) {
        if let Some(vb) = self.get_mut(id) {
            vb.view.width = width;
            vb.view.height = height;
            vb.buffer = Buffer::empty(Rect::new(0, 0, width, height));
            vb.modeline = Buffer::empty(Rect::new(0, 0, width, 1));
        }
    }

    /// Reposition a view, keeping its contents.
    pub fn move_to(&mut self, id: u64, x: u16, y: u16) {
        if let Some(vb) = self.get_mut(id) {
            vb.view.x = x;
            vb.view.y = y;
        }
    }

    /// Blit every paintable view into `screen`: tiles, then headers, then
    /// floating windows on top.
    ///
    /// Html views are skipped. A terminal cannot render them, and blitting
    /// their empty buffer would blank whatever lies beneath — Lem's tabbar
    /// is one, and it covers the top rows of the screen.
    pub fn composite(&self, screen: &mut Buffer) {
        let mut ordered: Vec<&ViewBuffer> = self
            .views
            .iter()
            .filter(|vb| vb.view.content_type == ViewType::Editor)
            .collect();
        ordered.sort_by_key(|vb| layer(vb.view.kind));

        let area = screen.area;

        // Separators go on after every tile is painted, so a neighbour
        // cannot overwrite one — and before headers and floating windows,
        // which should cover them.
        let (tiles, above): (Vec<_>, Vec<_>) = ordered
            .into_iter()
            .partition(|vb| vb.view.kind == ViewKind::Tile);
        for vb in &tiles {
            blit_view(screen, vb, area);
        }
        for vb in &tiles {
            draw_separator(screen, &vb.view, area);
        }
        for vb in &above {
            // Border first: it rings the view rather than overlapping it,
            // but drawing it first keeps a neighbouring window's content
            // from being clipped by our frame.
            draw_border(screen, &vb.view, area);
            blit_view(screen, vb, area);
        }
    }
}

/// Renders the composited screen.
///
/// `Frame`'s buffer is `pub(crate)`, so the only way into it is the
/// `Widget` trait — which lives in `ratatui-core` alongside the buffer.
/// This is the one piece of Ratatui's widget system the frontend uses,
/// and it is the trait, not the widget library (ADR 0002).
impl Widget for &Registry {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        self.composite(buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: u64, x: u16, y: u16, w: u16, h: u16, kind: ViewKind) -> View {
        View {
            id,
            x,
            y,
            width: w,
            height: h,
            use_modeline: None,
            kind,
            content_type: ViewType::Editor,
            border: None,
            border_shape: None,
        }
    }

    fn fill(registry: &mut Registry, id: u64, ch: char) {
        let vb = registry.get_mut(id).expect("view should exist");
        let area = vb.buffer.area;
        for y in 0..area.height {
            for x in 0..area.width {
                vb.buffer[(x, y)].set_char(ch);
            }
        }
    }

    #[test]
    fn floating_views_paint_over_tiles() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 10, 4, ViewKind::Tile));
        registry.insert(view(2, 2, 1, 4, 2, ViewKind::Floating));
        fill(&mut registry, 1, 't');
        fill(&mut registry, 2, 'f');

        let mut screen = Buffer::empty(Rect::new(0, 0, 10, 4));
        registry.composite(&mut screen);

        assert_eq!(screen[(0, 0)].symbol(), "t");
        assert_eq!(screen[(3, 1)].symbol(), "f", "floating must win");
        assert_eq!(screen[(3, 3)].symbol(), "t", "below the floating view");
    }

    #[test]
    fn layer_order_does_not_depend_on_insertion_order() {
        let mut registry = Registry::default();
        registry.insert(view(2, 0, 0, 4, 1, ViewKind::Floating));
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 2, 'f');
        fill(&mut registry, 1, 't');

        let mut screen = Buffer::empty(Rect::new(0, 0, 4, 1));
        registry.composite(&mut screen);
        assert_eq!(screen[(0, 0)].symbol(), "f", "floating wins regardless");
    }

    #[test]
    fn removing_a_view_exposes_what_was_under_it() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 6, 2, ViewKind::Tile));
        registry.insert(view(2, 1, 0, 2, 1, ViewKind::Floating));
        fill(&mut registry, 1, 't');
        fill(&mut registry, 2, 'f');
        registry.remove(2);

        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 2));
        registry.composite(&mut screen);
        assert_eq!(screen[(1, 0)].symbol(), "t");
    }

    #[test]
    fn html_views_are_tracked_but_never_painted() {
        // Lem's tabbar is an html header occupying the top rows. A
        // terminal cannot render it, and painting its blank buffer would
        // wipe whatever is beneath.
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 6, 2, ViewKind::Tile));
        let mut tabbar = view(2, 0, 0, 6, 1, ViewKind::Header);
        tabbar.content_type = ViewType::Html;
        registry.insert(tabbar);
        fill(&mut registry, 1, 't');

        assert!(registry.get_mut(2).is_some(), "still tracked");
        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 2));
        registry.composite(&mut screen);
        assert_eq!(screen[(0, 0)].symbol(), "t", "tile shows through");
    }

    #[test]
    fn the_modeline_sits_one_row_below_the_view() {
        // Lem allocates the modeline as an extra row: a view of height 2
        // at y=1 owns screen rows 1-2, and its modeline is row 3. Painting
        // it into the view would overwrite the buffer's first line.
        let mut registry = Registry::default();
        let mut v = view(1, 0, 1, 4, 2, ViewKind::Tile);
        v.use_modeline = Some(true);
        registry.insert(v);
        fill(&mut registry, 1, 'b');
        {
            let vb = registry.get_mut(1).unwrap();
            for x in 0..4 {
                vb.modeline[(x, 0)].set_char('m');
            }
        }

        let mut screen = Buffer::empty(Rect::new(0, 0, 4, 5));
        registry.composite(&mut screen);

        assert_eq!(screen[(0, 1)].symbol(), "b", "first buffer row intact");
        assert_eq!(screen[(0, 2)].symbol(), "b", "last buffer row");
        assert_eq!(screen[(0, 3)].symbol(), "m", "modeline below the view");
    }

    #[test]
    fn a_view_without_a_modeline_reserves_no_row() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 1, 'b');
        {
            let vb = registry.get_mut(1).unwrap();
            vb.modeline[(0, 0)].set_char('m');
        }
        let mut screen = Buffer::empty(Rect::new(0, 0, 4, 3));
        registry.composite(&mut screen);
        assert_eq!(screen[(0, 0)].symbol(), "b");
        assert_eq!(screen[(0, 1)].symbol(), " ", "no modeline painted");
    }

    #[test]
    fn a_split_gets_a_separator_in_its_reserved_column() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 2, ViewKind::Tile));
        let mut right = view(2, 6, 0, 4, 2, ViewKind::Tile);
        right.use_modeline = Some(true);
        registry.insert(right);
        fill(&mut registry, 1, 'l');
        fill(&mut registry, 2, 'r');

        let mut screen = Buffer::empty(Rect::new(0, 0, 12, 4));
        registry.composite(&mut screen);

        assert_eq!(screen[(5, 0)].symbol(), "\u{2502}", "separator column");
        assert_eq!(screen[(5, 1)].symbol(), "\u{2502}");
        assert_eq!(
            screen[(5, 2)].symbol(),
            "\u{2502}",
            "spans the modeline row"
        );
        assert_eq!(screen[(6, 0)].symbol(), "r", "view content untouched");
        assert_eq!(screen[(3, 0)].symbol(), "l", "left pane untouched");
    }

    #[test]
    fn a_view_at_the_left_edge_has_no_separator() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 2, ViewKind::Tile));
        fill(&mut registry, 1, 'l');
        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 2));
        registry.composite(&mut screen);
        assert_eq!(
            screen[(0, 0)].symbol(),
            "l",
            "nothing to the left to draw in"
        );
    }

    #[test]
    fn floating_windows_cover_separators() {
        let mut registry = Registry::default();
        registry.insert(view(1, 3, 0, 4, 2, ViewKind::Tile));
        registry.insert(view(2, 0, 0, 6, 1, ViewKind::Floating));
        fill(&mut registry, 1, 't');
        fill(&mut registry, 2, 'f');

        let mut screen = Buffer::empty(Rect::new(0, 0, 8, 2));
        registry.composite(&mut screen);
        assert_eq!(
            screen[(2, 0)].symbol(),
            "f",
            "floating wins over the separator"
        );
        assert_eq!(screen[(2, 1)].symbol(), "\u{2502}", "still drawn below it");
    }

    fn floating(id: u64, x: u16, y: u16, w: u16, h: u16) -> View {
        let mut v = view(id, x, y, w, h, ViewKind::Floating);
        v.border = Some(1);
        v
    }

    #[test]
    fn a_floating_window_is_ringed_by_a_box() {
        // The border sits outside the view: a 2x1 view at (2,2) is ringed
        // by a box from (1,1) to (4,3).
        let mut registry = Registry::default();
        registry.insert(floating(1, 2, 2, 2, 1));
        fill(&mut registry, 1, 'f');

        let mut screen = Buffer::empty(Rect::new(0, 0, 8, 6));
        registry.composite(&mut screen);

        assert_eq!(screen[(1, 1)].symbol(), "\u{256d}", "top left");
        assert_eq!(screen[(4, 1)].symbol(), "\u{256e}", "top right");
        assert_eq!(screen[(1, 3)].symbol(), "\u{2570}", "bottom left");
        assert_eq!(screen[(4, 3)].symbol(), "\u{256f}", "bottom right");
        assert_eq!(screen[(2, 1)].symbol(), "\u{2500}", "top edge");
        assert_eq!(screen[(1, 2)].symbol(), "\u{2502}", "left edge");
        assert_eq!(screen[(2, 2)].symbol(), "f", "content survives");
    }

    #[test]
    fn a_drop_curtain_joins_what_is_above_it() {
        let mut registry = Registry::default();
        let mut v = floating(1, 2, 2, 2, 1);
        v.border_shape = Some(BorderShape::DropCurtain);
        registry.insert(v);

        let mut screen = Buffer::empty(Rect::new(0, 0, 8, 6));
        registry.composite(&mut screen);
        assert_eq!(screen[(1, 1)].symbol(), "\u{251c}", "top left tees right");
        assert_eq!(screen[(4, 1)].symbol(), "\u{2524}", "top right tees left");
        assert_eq!(
            screen[(1, 3)].symbol(),
            "\u{2570}",
            "bottom corners unchanged"
        );
    }

    #[test]
    fn a_left_border_is_a_rule_not_a_box() {
        let mut registry = Registry::default();
        let mut v = floating(1, 2, 2, 2, 2);
        v.border_shape = Some(BorderShape::LeftBorder);
        registry.insert(v);

        let mut screen = Buffer::empty(Rect::new(0, 0, 8, 6));
        registry.composite(&mut screen);
        assert_eq!(screen[(1, 2)].symbol(), "\u{2502}");
        assert_eq!(screen[(1, 3)].symbol(), "\u{2502}");
        assert_eq!(screen[(1, 1)].symbol(), " ", "no box above");
        assert_eq!(screen[(4, 2)].symbol(), " ", "nothing on the right");
    }

    #[test]
    fn a_border_against_the_screen_edge_is_clipped_not_wrapped() {
        // A window at (0,0) puts its border at -1; those cells must be
        // dropped rather than wrapping onto the far side.
        let mut registry = Registry::default();
        registry.insert(floating(1, 0, 0, 3, 1));
        fill(&mut registry, 1, 'f');

        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 4));
        registry.composite(&mut screen);
        assert_eq!(screen[(0, 0)].symbol(), "f", "content still drawn");
        assert_eq!(screen[(3, 0)].symbol(), "\u{2502}", "right edge lands");
        assert_eq!(screen[(0, 1)].symbol(), "\u{2500}", "bottom edge lands");
        assert_eq!(screen[(5, 3)].symbol(), " ", "nothing wrapped");
    }

    #[test]
    fn a_view_without_a_border_gets_none() {
        let mut registry = Registry::default();
        registry.insert(view(1, 2, 2, 2, 1, ViewKind::Floating));
        let mut screen = Buffer::empty(Rect::new(0, 0, 8, 6));
        registry.composite(&mut screen);
        assert_eq!(screen[(1, 1)].symbol(), " ");
    }

    #[test]
    fn views_are_clipped_to_the_screen() {
        let mut registry = Registry::default();
        registry.insert(view(1, 4, 0, 8, 2, ViewKind::Tile));
        fill(&mut registry, 1, 't');

        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 2));
        registry.composite(&mut screen);
        assert_eq!(screen[(5, 0)].symbol(), "t");
    }

    #[test]
    fn resize_reallocates_the_buffer() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 10, 4, ViewKind::Tile));
        registry.resize(1, 20, 8);
        let vb = registry.get_mut(1).unwrap();
        assert_eq!(vb.buffer.area.width, 20);
        assert_eq!(vb.buffer.area.height, 8);
        assert_eq!(vb.view.width, 20);
        assert_eq!(vb.view.height, 8);
    }

    #[test]
    fn move_to_repositions_without_clearing() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 1, 't');
        registry.move_to(1, 2, 3);

        let mut screen = Buffer::empty(Rect::new(0, 0, 8, 5));
        registry.composite(&mut screen);
        assert_eq!(screen[(2, 3)].symbol(), "t", "moved, contents intact");
        assert_eq!(screen[(0, 0)].symbol(), " ", "vacated");
    }

    #[test]
    fn operations_on_an_unknown_view_are_ignored() {
        let mut registry = Registry::default();
        registry.resize(99, 10, 10);
        registry.move_to(99, 1, 1);
        registry.remove(99);
        assert!(registry.get_mut(99).is_none());
    }

    #[test]
    fn decodes_the_real_views_from_the_capture() {
        // Proves the mixed snake_case/camelCase handling and the null
        // use_modeline against actual wire data, not a hand-written sample.
        // Decoded straight from the argument rather than through
        // Instruction, which does not learn make-view until Task 6.
        let capture = include_str!("../../lem-protocol/tests/fixtures/frame.jsonl");
        let mut views: Vec<View> = Vec::new();
        for line in capture.lines().filter(|l| !l.trim().is_empty()) {
            let msg: serde_json::Value = serde_json::from_str(line).unwrap();
            if msg["method"] != "bulk" {
                continue;
            }
            for instruction in msg["params"].as_array().unwrap() {
                if instruction["method"] == "make-view" {
                    views.push(serde_json::from_value(instruction["argument"].clone()).unwrap());
                }
            }
        }

        assert_eq!(views.len(), 2);

        let editor = &views[0];
        assert_eq!(editor.id, 1);
        assert_eq!((editor.x, editor.y), (0, 2));
        assert_eq!((editor.width, editor.height), (80, 21));
        assert_eq!(editor.kind, ViewKind::Tile);
        assert_eq!(editor.content_type, ViewType::Editor);
        assert!(editor.has_modeline());

        let tabbar = &views[1];
        assert_eq!(tabbar.id, 2);
        assert_eq!(tabbar.kind, ViewKind::Header);
        assert_eq!(tabbar.content_type, ViewType::Html);
        assert!(!tabbar.has_modeline(), "null use_modeline reads as false");
    }
}
