//! `lem.relay.v1`: the protocol between `lem-relay` and a display.
//!
//! The types are generated from `frontends/ratatui/proto/lem/relay/v1/relay.proto`
//! by `build.rs`; the schema, with its comments, is the reference. This
//! module adds only what `prost` leaves to its user: framing on a stream.
//!
//! Each message is framed with protobuf's standard length-delimited format,
//! a varint byte length and then the message, which `prost` produces with
//! `encode_length_delimited` and the Lisp half with
//! `lem-relay/protobuf/framing`. `prost` decodes the format from a buffer;
//! a pipe has to be read up to the end of the length first, which is what
//! [`read_delimited`] does.

mod generated {
    // Generated code: its style is prost's, not ours.
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/lem.relay.v1.rs"));
}
pub use generated::*;

use std::io::{self, Read, Write};

use prost::Message;

/// The revision of this schema this crate was built against (`Hello`).
pub const PROTOCOL_VERSION: u32 = 2;

/// Read one length-delimited message body. `Ok(None)` means the stream
/// ended cleanly, between messages.
///
/// A stream that ends inside the length or the body is an error: a
/// truncated message cannot be told apart from a desynchronised stream.
pub fn read_delimited(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut length: u64 = 0;
    let mut shift = 0;
    loop {
        let mut byte = [0u8];
        if reader.read(&mut byte)? == 0 {
            return if shift == 0 {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream ended inside a length prefix",
                ))
            };
        }
        if shift >= 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "length prefix longer than a 64-bit varint",
            ));
        }
        length |= u64::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    let length = usize::try_from(length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "message too long"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Write `message` length-delimited and flush.
pub fn write_delimited(writer: &mut impl Write, message: &impl Message) -> io::Result<()> {
    writer.write_all(&message.encode_length_delimited_to_vec())?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seq: u64, text: &str) -> ToEditor {
        ToEditor {
            seq,
            time_us: 1000 * seq,
            message: Some(to_editor::Message::Key(Key {
                code: Some(key::Code::Text(text.into())),
                modifiers: vec![Modifier::Ctrl as i32],
                keypad: false,
            })),
        }
    }

    #[test]
    fn messages_round_trip_back_to_back() {
        let mut wire = Vec::new();
        write_delimited(&mut wire, &key(1, "a")).unwrap();
        write_delimited(&mut wire, &key(2, "é")).unwrap();
        let mut stream = wire.as_slice();
        for expected in [key(1, "a"), key(2, "é")] {
            let body = read_delimited(&mut stream).unwrap().unwrap();
            assert_eq!(ToEditor::decode(body.as_slice()).unwrap(), expected);
        }
        assert!(read_delimited(&mut stream).unwrap().is_none(), "clean end");
    }

    #[test]
    fn a_long_message_takes_a_multi_byte_length() {
        let long = key(1, &"x".repeat(300));
        let wire = long.encode_length_delimited_to_vec();
        assert!(wire[0] & 0x80 != 0, "the length continues past one byte");
        let body = read_delimited(&mut wire.as_slice()).unwrap().unwrap();
        assert_eq!(ToEditor::decode(body.as_slice()).unwrap(), long);
    }

    #[test]
    fn a_truncated_message_is_an_error() {
        let wire = key(1, "abc").encode_length_delimited_to_vec();
        assert!(
            read_delimited(&mut &wire[..1]).is_err(),
            "cut inside the body"
        );
        assert!(
            read_delimited(&mut &[0x80u8][..]).is_err(),
            "cut inside the length"
        );
    }

    #[test]
    fn the_schemas_enums_lose_their_prefixes() {
        // relay.proto prefixes enum values with their type, as protobuf
        // advises; prost strips the prefix.
        assert_eq!(ViewKind::Floating as i32, 3);
        assert_eq!(NamedKey::PageUp as i32, 13);
        assert_eq!(CursorShape::Bar as i32, 2);
    }
}
