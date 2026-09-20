//! Types for the `lem-server` JSON-RPC display protocol.
//!
//! This crate deliberately knows nothing about terminals. Everything here
//! is decoding and shape: given the bytes Lem emits, produce typed frames.
//! That keeps the interesting half of the work testable against captured
//! fixtures with no TTY attached, which is the reason it is a separate
//! crate from `lem-ratatui`.
//!
//! The protocol is documented in `../../../docs/protocol-notes.md`.
//! The other side of this wire is `frontends/server/frontend/editor.js`.
//!
//! Status: scaffold. Only the hot-path subset is modelled so far.

pub mod framing;
pub mod rpc;

use serde::{Deserialize, Serialize};

/// A view's identity, as carried by hot-path messages.
///
/// Lem sends only the id rather than the whole view object; see commit
/// `9fd84616`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewInfo {
    pub id: u64,
}

/// How a run of cells should be styled.
///
/// Colours arrive as `"#RRGGBB"` strings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Attribute {
    pub foreground: Option<String>,
    pub background: Option<String>,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub underline: Option<Underline>,
    #[serde(default)]
    pub cursor: bool,
}

/// Underline is tri-state on the wire: absent, on, or on-in-this-colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Underline {
    On(bool),
    Color(String),
}

/// Paint a run of text at a cell position.
///
/// `text_width` is precomputed by Lem, so the display half never needs its
/// own string-width calculation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Put {
    pub view_info: ViewInfo,
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub text_width: u16,
    pub attribute: Option<Attribute>,
}

/// One instruction inside a `bulk` frame, still undecoded.
///
/// Frames are read in two steps rather than one. Decoding straight into
/// [`Instruction`] would make an unrecognised method a hard parse error
/// that kills the whole frame, and the browser display half legitimately
/// receives methods a terminal has no use for (`js-eval`, `load-css`,
/// `set-font`). A new method appearing upstream must not take the editor
/// down, so the envelope is decoded first and the argument only after the
/// method is recognised.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawInstruction {
    pub method: String,
    #[serde(default)]
    pub argument: serde_json::Value,
}

/// Where Lem last printed its cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveCursor {
    pub view_info: ViewInfo,
    pub x: u16,
    pub y: u16,
}

/// A recognised instruction, or a record that one was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Put(Put),
    ModelinePut(Put),
    MakeView(View),
    DeleteView(ViewInfoArg),
    Clear(Clear),
    ClearEol(Clear),
    ClearEob(Clear),
    ResizeView(ResizeView),
    MoveView(MoveView),
    MoveCursor(MoveCursor),
    /// A method this display half does not implement.
    Other {
        method: String,
    },
}

impl RawInstruction {
    /// Decode the argument now that the method is known.
    ///
    /// Errors only when a *recognised* method carries an argument that
    /// does not fit its type, which is a genuine protocol mismatch worth
    /// surfacing. Unknown methods yield [`Instruction::Other`].
    pub fn parse(self) -> serde_json::Result<Instruction> {
        Ok(match self.method.as_str() {
            "put" => Instruction::Put(serde_json::from_value(self.argument)?),
            "modeline-put" => Instruction::ModelinePut(serde_json::from_value(self.argument)?),
            "make-view" => Instruction::MakeView(serde_json::from_value(self.argument)?),
            "delete-view" => Instruction::DeleteView(serde_json::from_value(self.argument)?),
            "clear" => Instruction::Clear(serde_json::from_value(self.argument)?),
            "clear-eol" => Instruction::ClearEol(serde_json::from_value(self.argument)?),
            "clear-eob" => Instruction::ClearEob(serde_json::from_value(self.argument)?),
            "resize-view" => Instruction::ResizeView(serde_json::from_value(self.argument)?),
            "move-view" => Instruction::MoveView(serde_json::from_value(self.argument)?),
            "move-cursor" => Instruction::MoveCursor(serde_json::from_value(self.argument)?),
            // `redraw-view-after`, `change-view` and `update-display` carry
            // nothing a terminal acts on beyond their arrival, and fall
            // through to Other deliberately.
            _ => Instruction::Other {
                method: self.method,
            },
        })
    }
}

/// A whole frame: Lem batches every instruction between two
/// `update-display` calls into a single `bulk` notification.
pub type Bulk = Vec<RawInstruction>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_put_instruction() {
        let json = r##"{
            "method": "put",
            "argument": {
                "viewInfo": {"id": 3},
                "x": 0, "y": 5,
                "text": "defun", "textWidth": 5,
                "attribute": {
                    "foreground": "#AABBCC", "background": "#111111",
                    "reverse": false, "bold": true,
                    "underline": null, "cursor": false
                }
            }
        }"##;

        let raw: RawInstruction = serde_json::from_str(json).unwrap();
        let Instruction::Put(put) = raw.parse().unwrap() else {
            panic!("expected a put instruction");
        };

        assert_eq!(put.view_info.id, 3);
        assert_eq!(put.text, "defun");
        assert_eq!(put.text_width, 5);
        assert!(put.attribute.unwrap().bold);
    }

    #[test]
    fn unknown_methods_do_not_fail_the_frame() {
        let json = r##"[
            {"method": "load-css", "argument": {"content": "body{}"}},
            {"method": "put", "argument": {
                "viewInfo": {"id": 1}, "x": 0, "y": 0,
                "text": "a", "textWidth": 1, "attribute": null}}
        ]"##;

        let bulk: Bulk = serde_json::from_str(json).unwrap();
        let parsed: Vec<Instruction> = bulk.into_iter().map(|raw| raw.parse().unwrap()).collect();

        assert_eq!(
            parsed[0],
            Instruction::Other {
                method: "load-css".into()
            }
        );
        assert!(matches!(parsed[1], Instruction::Put(_)));
    }

    #[test]
    fn underline_carries_either_a_flag_or_a_colour() {
        let colored: Attribute = serde_json::from_str(r##"{"underline": "#FF0000"}"##).unwrap();
        assert_eq!(colored.underline, Some(Underline::Color("#FF0000".into())));

        let plain: Attribute = serde_json::from_str(r##"{"underline": true}"##).unwrap();
        assert_eq!(plain.underline, Some(Underline::On(true)));
    }
}

/// Which layer a view composites into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    Tile,
    Header,
    Floating,
}

/// What a view contains.
///
/// `Html` views are real — Lem's tabbar is one — and a terminal cannot
/// paint them. They are tracked so their geometry stays consistent, but
/// skipped when compositing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewType {
    #[default]
    Editor,
    Html,
}

/// How a floating window's border is drawn.
///
/// Sent lower-cased by `lem-server`; absent means a full box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BorderShape {
    /// A box whose top corners join what sits above it.
    DropCurtain,
    /// A single rule down the left edge, no box.
    LeftBorder,
}

/// A window, positioned in character cells by Lem.
///
/// Casing on the wire is mixed: `pixelX` is camelCase while
/// `use_modeline` is snake_case. Renamed per field rather than with a
/// blanket `rename_all`, which would silently drop the snake_case ones.
///
/// `use_modeline` is `Option<bool>` because the wire sends `null` for
/// views that have no modeline, and `#[serde(default)]` on a plain `bool`
/// rejects an explicit null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub id: u64,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    #[serde(default)]
    pub use_modeline: Option<bool>,
    pub kind: ViewKind,
    #[serde(rename = "type", default)]
    pub content_type: ViewType,
    /// Border thickness in cells; floating windows default to 1.
    #[serde(default)]
    pub border: Option<u16>,
    #[serde(default)]
    pub border_shape: Option<BorderShape>,
}

impl View {
    /// Whether this view reserves its last row for a modeline.
    pub fn has_modeline(&self) -> bool {
        self.use_modeline.unwrap_or(false)
    }
}

/// Argument of `resize-view`: dimensions only, not a whole view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeView {
    pub view_info: ViewInfo,
    pub width: u16,
    pub height: u16,
}

/// Argument of `move-view`: position only, not a whole view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveView {
    pub view_info: ViewInfo,
    pub x: u16,
    pub y: u16,
}

/// Argument of `clear`, `clear-eol` and `clear-eob`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clear {
    pub view_info: ViewInfo,
    #[serde(default)]
    pub x: u16,
    #[serde(default)]
    pub y: u16,
}

/// Argument of messages carrying nothing but a view reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewInfoArg {
    pub view_info: ViewInfo,
}
