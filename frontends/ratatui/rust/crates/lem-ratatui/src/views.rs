//! Per-view cell buffers and the compositing step.
//!
//! The browser display half gets this for free: each view is its own
//! canvas and the browser composites them by z-index. A terminal has one
//! grid and composites nothing, so views are painted into separate
//! buffers and blitted here in layer order. See
//! `../../../docs/protocol-notes.md` section 7.

// Exercised by this module's tests until Task 6 wires the registry into
// the frame loop; the allow comes off with that change.
#![allow(dead_code)]

use lem_protocol::{View, ViewKind, ViewType};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

/// A view and the cells painted into it.
pub struct ViewBuffer {
    pub view: View,
    pub buffer: Buffer,
}

/// Every live view, in insertion order.
#[derive(Default)]
pub struct Registry {
    views: Vec<ViewBuffer>,
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
        self.remove(view.id);
        self.views.push(ViewBuffer { view, buffer });
    }

    pub fn remove(&mut self, id: u64) {
        self.views.retain(|vb| vb.view.id != id);
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
        for vb in ordered {
            for y in 0..vb.buffer.area.height {
                let Some(sy) = vb.view.y.checked_add(y) else {
                    break;
                };
                if sy >= area.height {
                    break;
                }
                for x in 0..vb.buffer.area.width {
                    let Some(sx) = vb.view.x.checked_add(x) else {
                        break;
                    };
                    if sx >= area.width {
                        break;
                    }
                    screen[(sx, sy)] = vb.buffer[(x, y)].clone();
                }
            }
        }
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
