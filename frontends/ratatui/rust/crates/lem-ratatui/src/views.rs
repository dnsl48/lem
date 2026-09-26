//! Per-view cell buffers and the compositing step.
//!
//! The browser display half gets this for free: each view is its own
//! canvas and the browser composites them by z-index. A terminal has one
//! grid and composites nothing, so views are painted into separate
//! buffers and blitted here, in the order the relay states
//! (`ViewsStacked`, ADR 0012). See `../../../docs/protocol-notes.md`
//! section 7.

use lem_protocol::v1::{self, BorderShape, ViewKind};
use ratatui_core::buffer::Buffer;

use crate::paint::Layer;
use ratatui_core::layout::Rect;

/// A view: where one Lem window's buffer goes on the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub id: u32,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub kind: ViewKind,
    /// A modeline row below the view's height.
    pub modeline: bool,
    /// A floating window's border, in cells outside the view; 0 for none.
    pub border: u16,
    pub border_shape: BorderShape,
}

/// Screen coordinates are `u16` here and in ratatui; anything larger is
/// off any real screen, so it saturates rather than wraps.
pub fn cells(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

impl From<&v1::ViewCreated> for View {
    fn from(created: &v1::ViewCreated) -> Self {
        Self {
            id: created.view,
            x: cells(created.x),
            y: cells(created.y),
            width: cells(created.width),
            height: cells(created.height),
            kind: created.kind(),
            modeline: created.modeline,
            border: cells(created.border),
            border_shape: created.border_shape(),
        }
    }
}

/// A view and the cells painted into it.
///
/// The modeline is a *separate* one-row buffer rather than the view's
/// last row, because Lem treats it as an extra row below the view: the
/// browser client allocates `height + (useModeline ? 1 : 0)`, and every
/// `modeline-put` arrives at `y: 0` of its own coordinate space. Painting
/// it into the view buffer would overwrite the first line of the file.
pub struct ViewBuffer {
    pub view: View,
    pub body: Layer,
    pub modeline: Layer,
}

/// Every live view, in creation order, and the order to composite them in.
#[derive(Default)]
pub struct Registry {
    views: Vec<ViewBuffer>,
    /// The relay's stacking order, bottom first, once it has sent one.
    /// Views it does not list are not shown: a virtual frame the frame
    /// multiplexer switched away from keeps its views alive.
    stacking: Option<Vec<u32>>,
}

/// Copy `source` onto `screen` at (`x`, `y`), clipped to `area`.
fn blit(screen: &mut Layer, source: &Layer, x: u16, y: u16, area: Rect) {
    let size = source.area();
    for sy in 0..size.height {
        let Some(ty) = y.checked_add(sy) else { return };
        if ty >= area.height {
            return;
        }
        for sx in 0..size.width {
            let Some(tx) = x.checked_add(sx) else { break };
            if tx >= area.width {
                break;
            }
            screen.cells[(tx, ty)] = source.cells[(sx, sy)].clone();
            screen.underline.set(tx, ty, source.underline.get(sx, sy));
        }
    }
}

/// Drawn in the column Lem reserves to the left of each split.
const SEPARATOR: &str = "\u{2502}";

/// Copy one view and its modeline onto the screen.
fn blit_view(screen: &mut Layer, vb: &ViewBuffer, area: Rect) {
    blit(screen, &vb.body, vb.view.x, vb.view.y, area);
    if vb.view.modeline {
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
    let rows = view.height + u16::from(view.modeline);
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
    if view.border == 0 {
        return;
    }
    let (size, x, y) = (i32::from(view.border), i32::from(view.x), i32::from(view.y));
    let (w, h) = (i32::from(view.width), i32::from(view.height));

    if view.border_shape == BorderShape::LeftBorder {
        for row in 0..h {
            put_cell(screen, x - size, y + row, glyph::VERTICAL, area);
        }
        return;
    }

    let (left, top) = (x - size, y - size);
    let (right, bottom) = (left + w + 2 * size - 1, top + h + 2 * size - 1);

    // A drop curtain hangs from whatever is above it, so its top corners
    // join that line rather than turning away from it.
    let (tl, tr) = if view.border_shape == BorderShape::DropCurtain {
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

/// Painting order before the relay has stated one: tiles are the
/// background, floating windows the top.
fn layer(kind: ViewKind) -> u8 {
    match kind {
        ViewKind::Tile | ViewKind::Unspecified => 0,
        ViewKind::Header => 1,
        ViewKind::Floating => 2,
    }
}

impl Registry {
    /// Add a view, replacing any existing one with the same id.
    pub fn insert(&mut self, view: View) {
        let body = Layer::new(view.width, view.height);
        let modeline = Layer::new(view.width, 1);
        self.remove(view.id);
        self.views.push(ViewBuffer {
            view,
            body,
            modeline,
        });
    }

    pub fn remove(&mut self, id: u32) {
        self.views.retain(|vb| vb.view.id != id);
    }

    /// Set the compositing order, bottom first.
    pub fn stack(&mut self, order: Vec<u32>) {
        self.stacking = Some(order);
    }

    /// Look a view up.
    pub fn get(&self, id: u32) -> Option<&ViewBuffer> {
        self.views.iter().find(|vb| vb.view.id == id)
    }

    /// How many views are tracked, shown or not.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut ViewBuffer> {
        self.views.iter_mut().find(|vb| vb.view.id == id)
    }

    /// Resize a view's buffer, discarding its contents.
    ///
    /// Lem repaints a resized view in the same frame, so there is nothing
    /// worth preserving and a stale-size buffer would mis-clip the writes
    /// that follow.
    pub fn resize(&mut self, id: u32, width: u16, height: u16) {
        if let Some(vb) = self.get_mut(id) {
            vb.view.width = width;
            vb.view.height = height;
            vb.body = Layer::new(width, height);
            vb.modeline = Layer::new(width, 1);
        }
    }

    /// Reposition a view, keeping its contents.
    pub fn move_to(&mut self, id: u32, x: u16, y: u16) {
        if let Some(vb) = self.get_mut(id) {
            vb.view.x = x;
            vb.view.y = y;
        }
    }

    /// Blit every shown view into `screen`, in the relay's stacking
    /// order, or before it has sent one, tiles, then headers, then
    /// floating windows on top.
    pub fn composite(&self, screen: &mut Layer) {
        let ordered: Vec<&ViewBuffer> = match &self.stacking {
            Some(order) => order.iter().filter_map(|id| self.get(*id)).collect(),
            None => {
                let mut all: Vec<&ViewBuffer> = self.views.iter().collect();
                all.sort_by_key(|vb| layer(vb.view.kind));
                all
            }
        };

        let area = screen.area();

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
            draw_separator(&mut screen.cells, &vb.view, area);
        }
        for vb in &above {
            // Border first: it rings the view rather than overlapping it,
            // but drawing it first keeps a neighbouring window's content
            // from being clipped by our frame.
            draw_border(&mut screen.cells, &vb.view, area);
            blit_view(screen, vb, area);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: u32, x: u16, y: u16, w: u16, h: u16, kind: ViewKind) -> View {
        View {
            id,
            x,
            y,
            width: w,
            height: h,
            kind,
            modeline: false,
            border: 0,
            border_shape: BorderShape::None,
        }
    }

    fn fill(registry: &mut Registry, id: u32, ch: char) {
        let vb = registry.get_mut(id).expect("view should exist");
        let area = vb.body.area();
        for y in 0..area.height {
            for x in 0..area.width {
                vb.body.cells[(x, y)].set_char(ch);
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

        let mut screen = Layer::new(10, 4);
        registry.composite(&mut screen);

        assert_eq!(screen.cells[(0, 0)].symbol(), "t");
        assert_eq!(screen.cells[(3, 1)].symbol(), "f", "floating must win");
        assert_eq!(
            screen.cells[(3, 3)].symbol(),
            "t",
            "below the floating view"
        );
    }

    #[test]
    fn layer_order_does_not_depend_on_insertion_order() {
        let mut registry = Registry::default();
        registry.insert(view(2, 0, 0, 4, 1, ViewKind::Floating));
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 2, 'f');
        fill(&mut registry, 1, 't');

        let mut screen = Layer::new(4, 1);
        registry.composite(&mut screen);
        assert_eq!(
            screen.cells[(0, 0)].symbol(),
            "f",
            "floating wins regardless"
        );
    }

    #[test]
    fn removing_a_view_exposes_what_was_under_it() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 6, 2, ViewKind::Tile));
        registry.insert(view(2, 1, 0, 2, 1, ViewKind::Floating));
        fill(&mut registry, 1, 't');
        fill(&mut registry, 2, 'f');
        registry.remove(2);

        let mut screen = Layer::new(6, 2);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(1, 0)].symbol(), "t");
    }

    #[test]
    fn compositing_carries_underline_styles() {
        let mut registry = Registry::default();
        registry.insert(view(1, 2, 1, 4, 1, ViewKind::Tile));
        registry
            .get_mut(1)
            .unwrap()
            .body
            .underline
            .set(1, 0, v1::UnderlineStyle::Curly);
        let mut screen = Layer::new(8, 3);
        registry.composite(&mut screen);
        assert_eq!(screen.underline.get(3, 1), v1::UnderlineStyle::Curly);
        assert_eq!(screen.underline.get(2, 1), v1::UnderlineStyle::Straight);
    }

    #[test]
    fn the_stated_order_wins_over_kinds() {
        // Two floating windows: the relay says which is on top.
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Floating));
        registry.insert(view(2, 0, 0, 4, 1, ViewKind::Floating));
        fill(&mut registry, 1, 'a');
        fill(&mut registry, 2, 'b');
        registry.stack(vec![2, 1]);

        let mut screen = Layer::new(4, 1);
        registry.composite(&mut screen);
        assert_eq!(
            screen.cells[(0, 0)].symbol(),
            "a",
            "listed last, drawn last"
        );
    }

    #[test]
    fn views_not_stacked_are_not_shown() {
        // A virtual frame switched away from keeps its views alive.
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        registry.insert(view(2, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 1, 'a');
        fill(&mut registry, 2, 'b');
        registry.stack(vec![1]);

        let mut screen = Layer::new(4, 1);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(0, 0)].symbol(), "a");
        assert_eq!(registry.len(), 2, "still tracked");
    }

    #[test]
    fn the_modeline_sits_one_row_below_the_view() {
        // Lem allocates the modeline as an extra row: a view of height 2
        // at y=1 owns screen rows 1-2, and its modeline is row 3. Painting
        // it into the view would overwrite the buffer's first line.
        let mut registry = Registry::default();
        let mut v = view(1, 0, 1, 4, 2, ViewKind::Tile);
        v.modeline = true;
        registry.insert(v);
        fill(&mut registry, 1, 'b');
        {
            let vb = registry.get_mut(1).unwrap();
            for x in 0..4 {
                vb.modeline.cells[(x, 0)].set_char('m');
            }
        }

        let mut screen = Layer::new(4, 5);
        registry.composite(&mut screen);

        assert_eq!(
            screen.cells[(0, 1)].symbol(),
            "b",
            "first buffer row intact"
        );
        assert_eq!(screen.cells[(0, 2)].symbol(), "b", "last buffer row");
        assert_eq!(
            screen.cells[(0, 3)].symbol(),
            "m",
            "modeline below the view"
        );
    }

    #[test]
    fn a_view_without_a_modeline_reserves_no_row() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 1, 'b');
        {
            let vb = registry.get_mut(1).unwrap();
            vb.modeline.cells[(0, 0)].set_char('m');
        }
        let mut screen = Layer::new(4, 3);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(0, 0)].symbol(), "b");
        assert_eq!(screen.cells[(0, 1)].symbol(), " ", "no modeline painted");
    }

    #[test]
    fn a_split_gets_a_separator_in_its_reserved_column() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 2, ViewKind::Tile));
        let mut right = view(2, 6, 0, 4, 2, ViewKind::Tile);
        right.modeline = true;
        registry.insert(right);
        fill(&mut registry, 1, 'l');
        fill(&mut registry, 2, 'r');

        let mut screen = Layer::new(12, 4);
        registry.composite(&mut screen);

        assert_eq!(
            screen.cells[(5, 0)].symbol(),
            "\u{2502}",
            "separator column"
        );
        assert_eq!(screen.cells[(5, 1)].symbol(), "\u{2502}");
        assert_eq!(
            screen.cells[(5, 2)].symbol(),
            "\u{2502}",
            "spans the modeline row"
        );
        assert_eq!(screen.cells[(6, 0)].symbol(), "r", "view content untouched");
        assert_eq!(screen.cells[(3, 0)].symbol(), "l", "left pane untouched");
    }

    #[test]
    fn a_view_at_the_left_edge_has_no_separator() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 2, ViewKind::Tile));
        fill(&mut registry, 1, 'l');
        let mut screen = Layer::new(6, 2);
        registry.composite(&mut screen);
        assert_eq!(
            screen.cells[(0, 0)].symbol(),
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

        let mut screen = Layer::new(8, 2);
        registry.composite(&mut screen);
        assert_eq!(
            screen.cells[(2, 0)].symbol(),
            "f",
            "floating wins over the separator"
        );
        assert_eq!(
            screen.cells[(2, 1)].symbol(),
            "\u{2502}",
            "still drawn below it"
        );
    }

    fn floating(id: u32, x: u16, y: u16, w: u16, h: u16) -> View {
        let mut v = view(id, x, y, w, h, ViewKind::Floating);
        v.border = 1;
        v
    }

    #[test]
    fn a_floating_window_is_ringed_by_a_box() {
        // The border sits outside the view: a 2x1 view at (2,2) is ringed
        // by a box from (1,1) to (4,3).
        let mut registry = Registry::default();
        registry.insert(floating(1, 2, 2, 2, 1));
        fill(&mut registry, 1, 'f');

        let mut screen = Layer::new(8, 6);
        registry.composite(&mut screen);

        assert_eq!(screen.cells[(1, 1)].symbol(), "\u{256d}", "top left");
        assert_eq!(screen.cells[(4, 1)].symbol(), "\u{256e}", "top right");
        assert_eq!(screen.cells[(1, 3)].symbol(), "\u{2570}", "bottom left");
        assert_eq!(screen.cells[(4, 3)].symbol(), "\u{256f}", "bottom right");
        assert_eq!(screen.cells[(2, 1)].symbol(), "\u{2500}", "top edge");
        assert_eq!(screen.cells[(1, 2)].symbol(), "\u{2502}", "left edge");
        assert_eq!(screen.cells[(2, 2)].symbol(), "f", "content survives");
    }

    #[test]
    fn a_drop_curtain_joins_what_is_above_it() {
        let mut registry = Registry::default();
        let mut v = floating(1, 2, 2, 2, 1);
        v.border_shape = BorderShape::DropCurtain;
        registry.insert(v);

        let mut screen = Layer::new(8, 6);
        registry.composite(&mut screen);
        assert_eq!(
            screen.cells[(1, 1)].symbol(),
            "\u{251c}",
            "top left tees right"
        );
        assert_eq!(
            screen.cells[(4, 1)].symbol(),
            "\u{2524}",
            "top right tees left"
        );
        assert_eq!(
            screen.cells[(1, 3)].symbol(),
            "\u{2570}",
            "bottom corners unchanged"
        );
    }

    #[test]
    fn a_left_border_is_a_rule_not_a_box() {
        let mut registry = Registry::default();
        let mut v = floating(1, 2, 2, 2, 2);
        v.border_shape = BorderShape::LeftBorder;
        registry.insert(v);

        let mut screen = Layer::new(8, 6);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(1, 2)].symbol(), "\u{2502}");
        assert_eq!(screen.cells[(1, 3)].symbol(), "\u{2502}");
        assert_eq!(screen.cells[(1, 1)].symbol(), " ", "no box above");
        assert_eq!(screen.cells[(4, 2)].symbol(), " ", "nothing on the right");
    }

    #[test]
    fn a_border_against_the_screen_edge_is_clipped_not_wrapped() {
        // A window at (0,0) puts its border at -1; those cells must be
        // dropped rather than wrapping onto the far side.
        let mut registry = Registry::default();
        registry.insert(floating(1, 0, 0, 3, 1));
        fill(&mut registry, 1, 'f');

        let mut screen = Layer::new(6, 4);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(0, 0)].symbol(), "f", "content still drawn");
        assert_eq!(
            screen.cells[(3, 0)].symbol(),
            "\u{2502}",
            "right edge lands"
        );
        assert_eq!(
            screen.cells[(0, 1)].symbol(),
            "\u{2500}",
            "bottom edge lands"
        );
        assert_eq!(screen.cells[(5, 3)].symbol(), " ", "nothing wrapped");
    }

    #[test]
    fn a_view_without_a_border_gets_none() {
        let mut registry = Registry::default();
        registry.insert(view(1, 2, 2, 2, 1, ViewKind::Floating));
        let mut screen = Layer::new(8, 6);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(1, 1)].symbol(), " ");
    }

    #[test]
    fn views_are_clipped_to_the_screen() {
        let mut registry = Registry::default();
        registry.insert(view(1, 4, 0, 8, 2, ViewKind::Tile));
        fill(&mut registry, 1, 't');

        let mut screen = Layer::new(6, 2);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(5, 0)].symbol(), "t");
    }

    #[test]
    fn resize_reallocates_the_buffer() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 10, 4, ViewKind::Tile));
        registry.resize(1, 20, 8);
        let vb = registry.get_mut(1).unwrap();
        assert_eq!(vb.body.area().width, 20);
        assert_eq!(vb.body.area().height, 8);
        assert_eq!(vb.view.width, 20);
        assert_eq!(vb.view.height, 8);
    }

    #[test]
    fn move_to_repositions_without_clearing() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 4, 1, ViewKind::Tile));
        fill(&mut registry, 1, 't');
        registry.move_to(1, 2, 3);

        let mut screen = Layer::new(8, 5);
        registry.composite(&mut screen);
        assert_eq!(screen.cells[(2, 3)].symbol(), "t", "moved, contents intact");
        assert_eq!(screen.cells[(0, 0)].symbol(), " ", "vacated");
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
    fn a_view_is_made_from_what_the_relay_sends() {
        let view = View::from(&v1::ViewCreated {
            view: 4,
            x: 1,
            y: 2,
            width: 30,
            height: 8,
            kind: ViewKind::Floating as i32,
            modeline: false,
            border: 1,
            border_shape: BorderShape::DropCurtain as i32,
        });
        assert_eq!(
            (view.id, view.x, view.y, view.width, view.height),
            (4, 1, 2, 30, 8)
        );
        assert_eq!(view.kind, ViewKind::Floating);
        assert_eq!(view.border_shape, BorderShape::DropCurtain);
    }
}
