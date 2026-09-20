//! LSP-style `Content-Length` framing.
//!
//! The header counts **bytes**, not characters. Getting that wrong
//! desynchronises the stream permanently on the first non-ASCII character,
//! which is exactly the bug the Lisp half had to fix on its side
//! (`../../../lisp/jsonrpc-stdio-fixes.lisp`).

use std::io::{self, BufRead, Write};

/// Read one framed message body. `Ok(None)` means clean end of stream.
///
/// A stream that ends part-way through a body is an error rather than a
/// short read: a truncated frame is indistinguishable from a desync, and
/// continuing would misinterpret the next header as message content.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length: Option<usize> = None;
    let mut saw_header = false;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return if saw_header {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream ended inside a message header",
                ))
            } else {
                Ok(None)
            };
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        saw_header = true;
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }

    let Some(length) = length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "framed message without a Content-Length header",
        ));
    };

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Write one framed message body.
pub fn write_message(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_framed_message() {
        let raw = b"Content-Length: 7\r\n\r\n{\"a\":1}";
        let mut cursor = &raw[..];
        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn reads_two_messages_back_to_back() {
        let raw = b"Content-Length: 2\r\n\r\n{}Content-Length: 2\r\n\r\n[]";
        let mut cursor = &raw[..];
        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), b"{}");
        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), b"[]");
        assert!(read_message(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn counts_bytes_not_characters() {
        // The body is 10 bytes but 9 characters: "é" is two bytes.
        // A reader that counts characters truncates it and then
        // mis-frames everything after it.
        let raw = "Content-Length: 10\r\n\r\n{\"a\":\"é\"}".as_bytes();
        let mut cursor = raw;
        let body = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(String::from_utf8(body).unwrap(), "{\"a\":\"é\"}");
    }

    #[test]
    fn a_multibyte_message_does_not_desync_the_next_one() {
        let mut buf = Vec::new();
        write_message(&mut buf, "{\"t\":\"héllo wörld\"}".as_bytes()).unwrap();
        write_message(&mut buf, b"{\"next\":true}").unwrap();
        let mut cursor = &buf[..];
        assert_eq!(
            String::from_utf8(read_message(&mut cursor).unwrap().unwrap()).unwrap(),
            "{\"t\":\"héllo wörld\"}"
        );
        assert_eq!(
            read_message(&mut cursor).unwrap().unwrap(),
            b"{\"next\":true}"
        );
    }

    #[test]
    fn tolerates_extra_headers_and_lax_spacing() {
        let raw = b"Content-Type: application/json\r\nContent-Length:2\r\n\r\n{}";
        let mut cursor = &raw[..];
        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), b"{}");
    }

    #[test]
    fn an_empty_stream_is_a_clean_end() {
        let mut cursor = &b""[..];
        assert!(read_message(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn a_truncated_body_is_an_error_not_a_short_read() {
        let raw = b"Content-Length: 99\r\n\r\n{}";
        let mut cursor = &raw[..];
        assert!(read_message(&mut cursor).is_err());
    }

    /// Frame every message from the committed capture, read them back,
    /// and check nothing is lost. Synthetic tests above cover the edges;
    /// this one covers real message sizes — 41KB across nine messages,
    /// the largest a single ~17KB bulk frame.
    #[test]
    fn round_trips_the_captured_session() {
        let capture = include_str!("../tests/fixtures/frame.jsonl");
        let messages: Vec<&str> = capture.lines().filter(|l| !l.trim().is_empty()).collect();
        assert!(!messages.is_empty(), "fixture should not be empty");

        let mut wire = Vec::new();
        for message in &messages {
            write_message(&mut wire, message.as_bytes()).unwrap();
        }

        let mut cursor = &wire[..];
        let mut read_back = Vec::new();
        while let Some(body) = read_message(&mut cursor).unwrap() {
            read_back.push(String::from_utf8(body).unwrap());
        }

        assert_eq!(read_back.len(), messages.len());
        for (got, want) in read_back.iter().zip(&messages) {
            assert_eq!(got, want);
        }
    }

    #[test]
    fn round_trips() {
        let mut buf = Vec::new();
        write_message(&mut buf, b"{\"hello\":true}").unwrap();
        assert!(buf.starts_with(b"Content-Length: 14\r\n\r\n"));
        let mut cursor = &buf[..];
        assert_eq!(
            read_message(&mut cursor).unwrap().unwrap(),
            b"{\"hello\":true}"
        );
    }
}
