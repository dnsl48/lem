//! What the Lisp relay sends, decoded here: the Lisp → Rust golden test.
//!
//! `proto/fixtures/relay-session.v1.bin` is written by
//! `scripts/capture-relay-v1.lisp`, which runs the real editor through
//! `lem-relay/protobuf`: typing non-ASCII and CJK text, splitting a window,
//! opening a popup, setting a theme background and a bar cursor, copying.
//! `relay-session.v1.txt` beside it is the same in text format, for review.

use std::collections::HashSet;

use lem_protocol::v1::{self, CursorShape, ViewKind, op, to_display};
use prost::Message;

fn messages() -> Vec<v1::ToDisplay> {
    let mut wire: &[u8] = include_bytes!("../../../../proto/fixtures/relay-session.v1.bin");
    let mut messages = Vec::new();
    while let Some(body) = v1::read_delimited(&mut wire).expect("well-framed") {
        messages.push(v1::ToDisplay::decode(body.as_slice()).expect("a ToDisplay"));
    }
    messages
}

fn frames() -> Vec<v1::Frame> {
    messages()
        .into_iter()
        .filter_map(|m| match m.message {
            Some(to_display::Message::Frame(frame)) => Some(frame),
            _ => None,
        })
        .collect()
}

fn ops() -> Vec<op::Op> {
    frames()
        .into_iter()
        .flat_map(|f| f.ops)
        .filter_map(|o| o.op)
        .collect()
}

#[test]
fn messages_are_numbered_in_order_and_timed_forwards() {
    let messages = messages();
    assert!(messages.len() >= 5);
    for (i, message) in messages.iter().enumerate() {
        assert_eq!(message.seq, i as u64 + 1, "seq counts from 1 in wire order");
    }
    for pair in messages.windows(2) {
        assert!(pair[0].time_us <= pair[1].time_us, "a monotonic clock");
    }
}

#[test]
fn every_style_is_defined_before_it_is_used_and_only_once() {
    let mut defined = HashSet::new();
    for frame in frames() {
        for style in &frame.styles {
            assert!(style.id != 0, "0 is no style and is never defined");
            assert!(defined.insert(style.id), "style {} defined twice", style.id);
        }
        for op in frame.ops.iter().filter_map(|o| o.op.as_ref()) {
            let used: Vec<u32> = match op {
                op::Op::Put(put) => vec![put.style],
                op::Op::ModelinePainted(m) => m.runs.iter().map(|r| r.style).collect(),
                _ => vec![],
            };
            for id in used {
                assert!(
                    id == 0 || defined.contains(&id),
                    "style {id} used before defined"
                );
            }
        }
    }
}

#[test]
fn the_session_paints_what_was_done() {
    let ops = ops();
    let views: Vec<&v1::ViewCreated> = ops
        .iter()
        .filter_map(|o| match o {
            op::Op::ViewCreated(v) => Some(v),
            _ => None,
        })
        .collect();
    assert!(
        views.iter().filter(|v| v.kind() == ViewKind::Tile).count() >= 2,
        "the window and its split"
    );
    let popup = views
        .iter()
        .find(|v| v.kind() == ViewKind::Floating)
        .expect("the popup");
    assert!(popup.border > 0);

    let text: String = ops
        .iter()
        .filter_map(|o| match o {
            op::Op::Put(put) => Some(put.text.as_str()),
            _ => None,
        })
        .collect();
    for expected in ["héllo", "λ", "日本語", "a popup"] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    let wide = ops
        .iter()
        .find_map(|o| match o {
            op::Op::Put(put) if put.text.contains("日本語") => Some(put),
            _ => None,
        })
        .expect("the CJK run");
    let expected = wide.text.chars().count() + "日本語".chars().count();
    assert_eq!(wide.width as usize, expected, "cells, not characters");
}

#[test]
fn the_popup_is_stacked_on_top() {
    let popup = ops()
        .iter()
        .find_map(|o| match o {
            op::Op::ViewCreated(v) if v.kind() == ViewKind::Floating => Some(v.view),
            _ => None,
        })
        .expect("the popup");
    let last_order = ops()
        .iter()
        .rev()
        .find_map(|o| match o {
            op::Op::ViewsStacked(s) => Some(s.views.clone()),
            _ => None,
        })
        .expect("a stacking order");
    assert_eq!(last_order.last(), Some(&popup));
}

#[test]
fn frame_state_arrives() {
    let frames = frames();
    assert!(frames[0].defaults.is_some(), "the first frame's defaults");
    let theme = frames
        .iter()
        .filter_map(|f| f.defaults.as_ref())
        .find_map(|d| d.background);
    assert_eq!(
        theme,
        Some(0x1C1C1C),
        "the theme background, when it changed"
    );
    assert!(frames.iter().all(|f| f.cursor.is_some()));
    assert_eq!(
        frames.last().unwrap().cursor.as_ref().unwrap().shape(),
        CursorShape::Bar
    );
    assert!(
        frames.iter().all(|f| f.input_seq.is_none()),
        "no input in this session"
    );
}

#[test]
fn a_copy_is_its_own_message() {
    let copied = messages().into_iter().find_map(|m| match m.message {
        Some(to_display::Message::SetClipboard(set)) => Some(set.text),
        _ => None,
    });
    assert_eq!(copied.as_deref(), Some("copied"));
}
