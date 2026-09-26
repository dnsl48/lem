//! What the display knows between frames, and how a frame changes it.

use lem_protocol::v1::{self, CursorShape, op};
use ratatui_core::layout::Position;
use ratatui_core::style::Color;

use crate::paint::{self, Layer, Styles};
use crate::views::{Registry, View, cells};

/// Every view, the session's styles, the default colours and the cursor.
#[derive(Default)]
pub struct Screen {
    pub views: Registry,
    styles: Styles,
    foreground: Option<Color>,
    background: Option<Color>,
    cursor: Option<v1::Cursor>,
}

impl Screen {
    /// Apply one frame: define its styles, run its ops, take its state.
    pub fn apply(&mut self, frame: v1::Frame) {
        for style in &frame.styles {
            self.styles.define(style);
        }
        for op in frame.ops {
            if let Some(op) = op.op {
                self.apply_op(op);
            }
        }
        // Absent means "as before": the relay sends defaults when they
        // change, and a cursor with every frame it has one.
        if let Some(defaults) = frame.defaults {
            self.foreground = defaults.foreground.map(paint::color);
            self.background = defaults.background.map(paint::color);
        }
        self.cursor = frame.cursor;
    }

    fn apply_op(&mut self, op: op::Op) {
        let views = &mut self.views;
        match op {
            op::Op::ViewCreated(created) => views.insert(View::from(&created)),
            op::Op::ViewDeleted(deleted) => views.remove(deleted.view),
            op::Op::ViewMoved(m) => views.move_to(m.view, cells(m.x), cells(m.y)),
            op::Op::ViewResized(r) => views.resize(r.view, cells(r.width), cells(r.height)),
            op::Op::ViewCleared(c) => {
                if let Some(vb) = views.get_mut(c.view) {
                    vb.body.clear_eob(0);
                }
            }
            op::Op::ViewsStacked(stacked) => views.stack(stacked.views),
            op::Op::Put(p) => {
                let paint = self.styles.get(p.style);
                let link = p.link.as_deref().and_then(paint::link_of);
                if let Some(vb) = views.get_mut(p.view) {
                    vb.body.put_linked(
                        cells(p.x),
                        cells(p.y),
                        &p.text,
                        cells(p.width),
                        paint,
                        link.as_ref(),
                    );
                }
            }
            op::Op::LineCleared(c) => {
                if let Some(vb) = views.get_mut(c.view) {
                    vb.body.clear_eol(cells(c.x), cells(c.y));
                }
            }
            op::Op::RestCleared(c) => {
                if let Some(vb) = views.get_mut(c.view) {
                    vb.body.clear_eob(cells(c.y));
                }
            }
            op::Op::ModelinePainted(m) => {
                if let Some(vb) = views.get_mut(m.view) {
                    // Sent whole: the first run is the full-width blank.
                    vb.modeline.clear_eob(0);
                    for run in m.runs {
                        let paint = self.styles.get(run.style);
                        vb.modeline
                            .put(cells(run.x), 0, &run.text, cells(run.width), paint);
                    }
                }
            }
        }
    }

    /// Where the terminal's own cursor goes, if it shows: the cursor's
    /// cell within its view, on the screen.
    pub fn cursor_position(&self) -> Option<Position> {
        let cursor = self.cursor.as_ref().filter(|c| !c.hidden)?;
        let view = &self.views.get(cursor.view)?.view;
        Some(Position::new(
            view.x.saturating_add(cells(cursor.x)),
            view.y.saturating_add(cells(cursor.y)),
        ))
    }

    /// The cursor's shape, while there is a cursor.
    pub fn cursor_shape(&self) -> Option<CursorShape> {
        self.cursor.as_ref().map(|c| c.shape())
    }

    /// Composite, then fill every default colour left with the theme's.
    ///
    /// Done here rather than when painting, so a theme change reaches
    /// cells painted before it, and blank screen with them.
    pub fn render_into(&self, screen: &mut Layer) {
        self.views.composite(screen);
        if self.foreground.is_none() && self.background.is_none() {
            return;
        }
        let area = screen.area();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut screen.cells[(x, y)];
                if let Some(fg) = self.foreground.filter(|_| cell.fg == Color::Reset) {
                    cell.fg = fg;
                }
                if let Some(bg) = self.background.filter(|_| cell.bg == Color::Reset) {
                    cell.bg = bg;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(view: u32, width: u32, height: u32) -> v1::Op {
        v1::Op {
            op: Some(op::Op::ViewCreated(v1::ViewCreated {
                view,
                x: 0,
                y: 0,
                width,
                height,
                kind: v1::ViewKind::Tile as i32,
                modeline: true,
                border: 0,
                border_shape: v1::BorderShape::None as i32,
            })),
        }
    }

    fn put(view: u32, x: u32, text: &str, style: u32) -> v1::Op {
        v1::Op {
            op: Some(op::Op::Put(v1::Put {
                view,
                x,
                y: 0,
                text: text.into(),
                width: text.chars().count() as u32,
                style,
                link: None,
            })),
        }
    }

    fn render(screen: &Screen, w: u16, h: u16) -> ratatui_core::buffer::Buffer {
        let mut layer = Layer::new(w, h);
        screen.render_into(&mut layer);
        layer.cells
    }

    #[test]
    fn a_frame_defines_styles_before_its_ops_use_them() {
        let mut screen = Screen::default();
        screen.apply(v1::Frame {
            styles: vec![v1::Style {
                id: 1,
                foreground: Some(0x00FF00),
                ..Default::default()
            }],
            ops: vec![created(1, 4, 1), put(1, 0, "ok", 1)],
            ..Default::default()
        });
        let buffer = render(&screen, 4, 2);
        assert_eq!(buffer[(0, 0)].symbol(), "o");
        assert_eq!(buffer[(0, 0)].fg, Color::Rgb(0, 0xFF, 0));
    }

    #[test]
    fn theme_defaults_fill_what_styles_leave_unset() {
        let mut screen = Screen::default();
        screen.apply(v1::Frame {
            ops: vec![created(1, 4, 1), put(1, 0, "a", 0)],
            defaults: Some(v1::Defaults {
                foreground: Some(0xDDDDDD),
                background: Some(0x1C1C1C),
            }),
            ..Default::default()
        });
        let buffer = render(&screen, 4, 2);
        assert_eq!(buffer[(0, 0)].fg, Color::Rgb(0xDD, 0xDD, 0xDD));
        assert_eq!(
            buffer[(3, 0)].bg,
            Color::Rgb(0x1C, 0x1C, 0x1C),
            "blank cells too"
        );
    }

    #[test]
    fn defaults_persist_until_changed() {
        let mut screen = Screen::default();
        screen.apply(v1::Frame {
            defaults: Some(v1::Defaults {
                background: Some(0x101010),
                ..Default::default()
            }),
            ..Default::default()
        });
        screen.apply(v1::Frame::default());
        assert_eq!(
            render(&screen, 1, 1)[(0, 0)].bg,
            Color::Rgb(0x10, 0x10, 0x10)
        );
    }

    #[test]
    fn a_modeline_is_repainted_whole() {
        let mut screen = Screen::default();
        let modeline = |text: &str| v1::Op {
            op: Some(op::Op::ModelinePainted(v1::ModelinePainted {
                view: 1,
                runs: vec![
                    v1::Run {
                        x: 0,
                        text: "    ".into(),
                        width: 4,
                        style: 0,
                    },
                    v1::Run {
                        x: 0,
                        text: text.into(),
                        width: text.len() as u32,
                        style: 0,
                    },
                ],
            })),
        };
        screen.apply(v1::Frame {
            ops: vec![created(1, 4, 1), modeline("long")],
            ..Default::default()
        });
        screen.apply(v1::Frame {
            ops: vec![modeline("ab")],
            ..Default::default()
        });
        let buffer = render(&screen, 4, 2);
        assert_eq!(buffer[(1, 1)].symbol(), "b");
        assert_eq!(buffer[(2, 1)].symbol(), " ", "nothing left of the old one");
    }

    #[test]
    fn the_cursor_is_placed_within_its_view() {
        let mut screen = Screen::default();
        let mut moved = created(1, 10, 3);
        if let Some(op::Op::ViewCreated(view)) = moved.op.as_mut() {
            view.x = 5;
            view.y = 2;
        }
        screen.apply(v1::Frame {
            ops: vec![moved],
            cursor: Some(v1::Cursor {
                view: 1,
                x: 3,
                y: 1,
                hidden: false,
                shape: CursorShape::Bar as i32,
            }),
            ..Default::default()
        });
        assert_eq!(screen.cursor_position(), Some(Position::new(8, 3)));
        assert_eq!(screen.cursor_shape(), Some(CursorShape::Bar));
    }

    #[test]
    fn a_hidden_cursor_has_no_position() {
        let mut screen = Screen::default();
        screen.apply(v1::Frame {
            ops: vec![created(1, 4, 1)],
            cursor: Some(v1::Cursor {
                view: 1,
                hidden: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(screen.cursor_position(), None);
    }
}
