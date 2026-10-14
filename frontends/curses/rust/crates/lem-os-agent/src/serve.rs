//! One connection from Lem: the handshake, then its requests until it
//! hangs up.

use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use lem_protocols::framing;
use lem_protocols::os::v1::{self, to_agent, to_editor};
use prost::Message;

use crate::clipboard::Clipboard;

/// Numbers and times what the agent sends, as ADR 0013 asks of every link.
struct Writer<W> {
    out: W,
    seq: u64,
    started: Instant,
}

impl<W: Write> Writer<W> {
    fn new(out: W) -> Self {
        Self {
            out,
            seq: 0,
            started: Instant::now(),
        }
    }

    fn send(&mut self, message: to_editor::Message) -> io::Result<()> {
        self.seq += 1;
        let envelope = v1::ToEditor {
            seq: self.seq,
            time_us: u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX),
            message: Some(message),
        };
        framing::write_delimited(&mut self.out, &envelope)
    }
}

fn read(input: &mut impl Read) -> Result<Option<v1::ToAgent>> {
    let Some(body) = framing::read_delimited(input)? else {
        return Ok(None);
    };
    Ok(Some(
        v1::ToAgent::decode(body.as_slice()).context("decoding a ToAgent")?,
    ))
}

/// Serve one Lem, reading `input` and writing `output`, until it hangs up.
/// Returns the session id its `Hello` carried.
///
/// The first message must be a `Hello` with a protocol version; anything
/// else ends the connection unanswered. A message this revision does not
/// know decodes with no `message` and is ignored, as a second `Hello` is.
///
/// `clipboard` is shared with the lifeline thread (`main.rs`), which can
/// take it out from under us on the session's way out; locked only for
/// each access, not for the whole connection, so that taking it does not
/// wait on a Lem that is still connected.
pub fn serve(
    mut input: impl Read,
    output: impl Write,
    clipboard: &Mutex<Option<Box<dyn Clipboard>>>,
) -> Result<String> {
    let mut writer = Writer::new(output);
    let first = read(&mut input)?.context("Lem hung up before Hello")?;
    let Some(to_agent::Message::Hello(hello)) = first.message else {
        bail!("the first message was not Hello");
    };
    if hello.protocol_version == 0 {
        bail!("Hello carried no protocol_version");
    }
    writer.send(to_editor::Message::Welcome(v1::Welcome {
        protocol_version: v1::PROTOCOL_VERSION,
        session_id: hello.session_id.clone(),
        capabilities: Some(v1::Capabilities {
            clipboard: clipboard.lock().unwrap().is_some(),
        }),
        reply_to: first.seq,
    }))?;

    while let Some(envelope) = read(&mut input)? {
        match envelope.message {
            Some(to_agent::Message::ClipboardRequest(_)) => {
                let text = clipboard.lock().unwrap().as_mut().and_then(|c| c.get());
                writer.send(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                    reply_to: envelope.seq,
                    text,
                }))?;
            }
            Some(to_agent::Message::SetClipboard(set)) => {
                if let Some(clipboard) = clipboard.lock().unwrap().as_mut() {
                    clipboard.set(&set.text);
                }
            }
            Some(to_agent::Message::Hello(_)) | None => {}
        }
    }
    Ok(hello.session_id)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// A clipboard the test can look into afterwards. `Arc<Mutex<_>>`,
    /// not `Rc<RefCell<_>>`: `Clipboard` requires `Send` (main.rs shares
    /// one with its lifeline thread), so a test double must satisfy that
    /// too.
    struct Memory(Arc<Mutex<Option<String>>>);

    impl Clipboard for Memory {
        fn get(&mut self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
        fn set(&mut self, text: &str) {
            *self.0.lock().unwrap() = Some(text.to_owned());
        }
    }

    #[allow(clippy::type_complexity)]
    fn memory(
        text: Option<&str>,
    ) -> (
        Mutex<Option<Box<dyn Clipboard>>>,
        Arc<Mutex<Option<String>>>,
    ) {
        let cell = Arc::new(Mutex::new(text.map(str::to_owned)));
        (Mutex::new(Some(Box::new(Memory(cell.clone())))), cell)
    }

    fn wire(messages: &[(u64, to_agent::Message)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (seq, message) in messages {
            let envelope = v1::ToAgent {
                seq: *seq,
                time_us: 0,
                message: Some(message.clone()),
            };
            framing::write_delimited(&mut out, &envelope).unwrap();
        }
        out
    }

    fn sent(out: &[u8]) -> Vec<v1::ToEditor> {
        let mut stream = out;
        let mut messages = Vec::new();
        while let Some(body) = framing::read_delimited(&mut stream).unwrap() {
            messages.push(v1::ToEditor::decode(body.as_slice()).unwrap());
        }
        messages
    }

    fn hello(version: u32) -> to_agent::Message {
        to_agent::Message::Hello(v1::Hello {
            protocol_version: version,
            session_id: "s".into(),
        })
    }

    fn welcome(message: &v1::ToEditor) -> &v1::Welcome {
        match &message.message {
            Some(to_editor::Message::Welcome(welcome)) => welcome,
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn welcomes_lem_with_what_it_can_do() {
        let (clipboard, _) = memory(None);
        let mut out = Vec::new();
        let session = serve(wire(&[(1, hello(1))]).as_slice(), &mut out, &clipboard).unwrap();
        assert_eq!(session, "s");
        let sent = sent(&out);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].seq, 1);
        let welcome = welcome(&sent[0]);
        assert_eq!(welcome.protocol_version, v1::PROTOCOL_VERSION);
        assert_eq!(welcome.session_id, "s");
        assert_eq!(welcome.reply_to, 1);
        assert_eq!(
            welcome.capabilities,
            Some(v1::Capabilities { clipboard: true })
        );
    }

    #[test]
    fn without_a_clipboard_it_says_so() {
        let mut out = Vec::new();
        serve(
            wire(&[(1, hello(1))]).as_slice(),
            &mut out,
            &Mutex::new(None),
        )
        .unwrap();
        assert_eq!(
            welcome(&sent(&out)[0]).capabilities,
            Some(v1::Capabilities { clipboard: false })
        );
    }

    #[test]
    fn answers_a_clipboard_request_by_its_seq() {
        let (clipboard, _) = memory(Some("pasted"));
        let mut out = Vec::new();
        let input = wire(&[
            (1, hello(1)),
            (
                2,
                to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}),
            ),
        ]);
        serve(input.as_slice(), &mut out, &clipboard).unwrap();
        let sent = sent(&out);
        assert_eq!(sent[1].seq, 2, "numbered in order");
        assert_eq!(
            sent[1].message,
            Some(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                reply_to: 2,
                text: Some("pasted".into()),
            }))
        );
    }

    #[test]
    fn a_request_without_a_clipboard_gets_no_text() {
        let mut out = Vec::new();
        let input = wire(&[
            (1, hello(1)),
            (
                2,
                to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}),
            ),
        ]);
        serve(input.as_slice(), &mut out, &Mutex::new(None)).unwrap();
        assert_eq!(
            sent(&out)[1].message,
            Some(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                reply_to: 2,
                text: None,
            }))
        );
    }

    #[test]
    fn sets_the_clipboard() {
        let (clipboard, cell) = memory(None);
        let input = wire(&[
            (1, hello(1)),
            (
                2,
                to_agent::Message::SetClipboard(v1::SetClipboard {
                    text: "copied".into(),
                }),
            ),
        ]);
        serve(input.as_slice(), &mut Vec::new(), &clipboard).unwrap();
        assert_eq!(cell.lock().unwrap().as_deref(), Some("copied"));
    }

    #[test]
    fn the_first_message_must_be_hello() {
        let input = wire(&[(
            1,
            to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}),
        )]);
        let mut out = Vec::new();
        assert!(serve(input.as_slice(), &mut out, &Mutex::new(None)).is_err());
        assert!(
            out.is_empty(),
            "nothing sent to a peer that did not say Hello"
        );
    }

    #[test]
    fn a_hello_without_a_protocol_version_is_refused() {
        let mut out = Vec::new();
        assert!(
            serve(
                wire(&[(1, hello(0))]).as_slice(),
                &mut out,
                &Mutex::new(None)
            )
            .is_err()
        );
        assert!(out.is_empty());
    }

    #[test]
    fn hanging_up_before_hello_is_an_error() {
        assert!(serve(&[][..], &mut Vec::new(), &Mutex::new(None)).is_err());
    }
}
