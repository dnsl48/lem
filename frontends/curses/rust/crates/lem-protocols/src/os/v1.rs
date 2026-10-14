//! `lem.os.v1`: the protocol between Lem and the OS agent (ADR 0019).
//!
//! The types are generated from
//! `frontends/curses/proto/lem/os/v1/os.proto` by `build.rs`; the schema,
//! with its comments, is the reference. Framing on a stream is
//! [`crate::framing`], shared by every link.

mod generated {
    // Generated code: its style is prost's, not ours.
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/lem.os.v1.rs"));
}
pub use generated::*;

/// The revision of this schema this crate was built against (`Hello`).
pub const PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn a_welcome_round_trips() {
        let welcome = ToEditor {
            seq: 1,
            time_us: 5,
            message: Some(to_editor::Message::Welcome(Welcome {
                protocol_version: PROTOCOL_VERSION,
                session_id: "0190a0d6-0000-7000-8000-000000000000".into(),
                capabilities: Some(Capabilities { clipboard: true }),
                reply_to: 1,
            })),
        };
        let bytes = welcome.encode_to_vec();
        assert_eq!(ToEditor::decode(bytes.as_slice()).unwrap(), welcome);
    }

    #[test]
    fn an_empty_clipboard_is_absent_text_not_empty_text() {
        let reply = ClipboardReply {
            reply_to: 2,
            text: None,
        };
        let decoded = ClipboardReply::decode(reply.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.text, None);
    }
}
