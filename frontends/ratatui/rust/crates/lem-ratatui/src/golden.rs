//! What the display sends, pinned: the Rust → Lisp golden test.
//!
//! Builds a `ToEditor` sequence through the display's own conversions
//! (`input::key`, `input::mouse`) and compares it with
//! `proto/fixtures/display-inputs.v1.bin`, which the Lisp half decodes in
//! `relay/tests/golden.lisp` and checks against what Lem receives: key
//! names, modifiers, click counts from the timestamps.
//!
//! After an intended change, rewrite the fixture and review its text twin:
//!
//!     LEM_UPDATE_FIXTURES=1 cargo test -p lem-ratatui golden

use std::path::PathBuf;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use lem_protocol::v1::{self, to_editor};

use crate::input;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../proto/fixtures")
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> to_editor::Message {
    to_editor::Message::Key(input::key(KeyEvent::new(code, modifiers)).expect("a known key"))
}

fn keypad(code: KeyCode, modifiers: KeyModifiers) -> to_editor::Message {
    to_editor::Message::Key(
        input::key(KeyEvent::new_with_kind_and_state(
            code,
            modifiers,
            KeyEventKind::Press,
            KeyEventState::KEYPAD,
        ))
        .expect("a known keypad key"),
    )
}

fn mouse(kind: MouseEventKind) -> to_editor::Message {
    to_editor::Message::Mouse(input::mouse(MouseEvent {
        kind,
        column: 7,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }))
}

/// The session, as (time in microseconds, message). Times are fixed so the
/// fixture is too; the two presses are 200 ms apart, a double click.
fn session() -> Vec<(u64, to_editor::Message)> {
    vec![
        (
            0,
            to_editor::Message::Hello(v1::Hello {
                protocol_version: v1::PROTOCOL_VERSION,
                session_id: "01920000-0000-7000-8000-000000000000".into(),
                width: 100,
                height: 30,
                foreground: None,
                background: Some(0x101010),
                capabilities: Some(v1::TerminalCapabilities {
                    keyboard_disambiguation: true,
                    alternate_key_reporting: true,
                    keypad_identity: true,
                }),
            }),
        ),
        (1_000, key(KeyCode::Char('x'), KeyModifiers::CONTROL)),
        (2_000, key(KeyCode::Enter, KeyModifiers::NONE)),
        (3_000, key(KeyCode::F(5), KeyModifiers::NONE)),
        (
            4_000,
            key(KeyCode::Char('A'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        ),
        (5_000, key(KeyCode::Char('\\'), KeyModifiers::CONTROL)),
        (6_000, key(KeyCode::Char(' '), KeyModifiers::NONE)),
        (7_000, key(KeyCode::BackTab, KeyModifiers::SHIFT)),
        (8_000, key(KeyCode::Char('é'), KeyModifiers::NONE)),
        (9_000, key(KeyCode::Char('č'), KeyModifiers::NONE)),
        (10_000, key(KeyCode::Char('š'), KeyModifiers::NONE)),
        (
            11_000,
            key(
                KeyCode::Char('g'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        ),
        (12_000, key(KeyCode::Char('G'), KeyModifiers::CONTROL)),
        (13_000, key(KeyCode::Char('g'), KeyModifiers::CONTROL)),
        (14_000, key(KeyCode::Char('5'), KeyModifiers::CONTROL)),
        (15_000, keypad(KeyCode::Char('5'), KeyModifiers::CONTROL)),
        (
            16_000,
            keypad(
                KeyCode::Char('5'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
        ),
        (17_000, keypad(KeyCode::Char('5'), KeyModifiers::NONE)),
        (18_000, keypad(KeyCode::Char('+'), KeyModifiers::CONTROL)),
        (19_000, keypad(KeyCode::Enter, KeyModifiers::NONE)),
        (20_000, keypad(KeyCode::Left, KeyModifiers::SHIFT)),
        (1_000_000, mouse(MouseEventKind::Down(MouseButton::Left))),
        (1_200_000, mouse(MouseEventKind::Down(MouseButton::Left))),
        (1_300_000, mouse(MouseEventKind::Drag(MouseButton::Right))),
        (1_400_000, mouse(MouseEventKind::ScrollUp)),
        (
            2_000_000,
            to_editor::Message::Paste(v1::Paste {
                text: "pasted".into(),
            }),
        ),
        (
            2_100_000,
            to_editor::Message::Resize(v1::Resize {
                width: 120,
                height: 40,
            }),
        ),
        (
            2_200_000,
            to_editor::Message::ClipboardReply(v1::ClipboardReply {
                reply_to: 7,
                text: Some("from the display".into()),
            }),
        ),
        (
            2_300_000,
            to_editor::Message::ClipboardReply(v1::ClipboardReply {
                reply_to: 8,
                text: None,
            }),
        ),
    ]
}

fn envelopes() -> Vec<v1::ToEditor> {
    session()
        .into_iter()
        .enumerate()
        .map(|(i, (time_us, message))| v1::ToEditor {
            seq: i as u64 + 1,
            time_us,
            message: Some(message),
        })
        .collect()
}

#[test]
fn golden_display_inputs_are_current() {
    let envelopes = envelopes();
    let mut wire = Vec::new();
    for envelope in &envelopes {
        v1::write_delimited(&mut wire, envelope).unwrap();
    }
    let path = fixtures().join("display-inputs.v1.bin");
    if std::env::var_os("LEM_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &wire).unwrap();
        let text: String = envelopes.iter().map(|e| format!("{e:#?}\n")).collect();
        std::fs::write(
            fixtures().join("display-inputs.v1.txt"),
            format!("# Generated by lem-ratatui's golden test; see display-inputs.v1.bin.\n{text}"),
        )
        .unwrap();
    }
    let committed = std::fs::read(&path).expect("the fixture; LEM_UPDATE_FIXTURES=1 writes it");
    assert!(
        committed == wire,
        "the display's inputs changed; if intended, rerun with LEM_UPDATE_FIXTURES=1 \
         and review display-inputs.v1.txt"
    );
}
