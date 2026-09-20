//! Per-view cell buffers and the compositing step.
//!
//! The browser display half gets this for free: each view is its own
//! canvas and the browser composites them by z-index. A terminal has one
//! grid and composites nothing, so views are painted into separate
//! buffers and blitted here in layer order. See
//! `../../../docs/protocol-notes.md` section 7.

use lem_protocol::{View, ViewKind, ViewType};
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
