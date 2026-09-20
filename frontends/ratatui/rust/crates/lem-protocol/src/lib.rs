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

/// A recognised instruction, or a record that one was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Put(Put),
    ModelinePut(Put),
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
