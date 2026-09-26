//! The Lisp relay's JSON output, decoded by this crate.
//!
//! `fixtures/relay-frames.jsonl` is written by
//! `frontends/ratatui/scripts/capture-relay-json.lisp`, which runs the real
//! editor through `lem-relay/json`: typing non-ASCII text, splitting a
//! window, opening a popup, changing the theme background and the cursor
//! shape, copying. `frame.jsonl` is the same check against what
//! `lem-server` sent; this one keeps the replacement honest while the
//! display half is unchanged (relay-plan.md, phase 1).

use lem_protocol::{BorderShape, Bulk, Instruction, ViewKind};

fn messages() -> Vec<serde_json::Value> {
    include_str!("fixtures/relay-frames.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every line is one JSON message"))
        .collect()
}

fn instructions() -> Vec<Instruction> {
    messages()
        .into_iter()
        .filter(|message| message["method"] == "bulk")
        .flat_map(|message| serde_json::from_value::<Bulk>(message["params"].clone()).unwrap())
        .map(|raw| raw.parse().expect("every instruction should decode"))
        .collect()
}

#[test]
fn every_message_is_a_json_rpc_notification() {
    for message in messages() {
        assert_eq!(message["jsonrpc"], "2.0", "{message}");
        assert!(message["method"].is_string(), "{message}");
    }
}

#[test]
fn every_frame_ends_in_update_display() {
    for message in messages().iter().filter(|m| m["method"] == "bulk") {
        let bulk = message["params"].as_array().unwrap();
        assert_eq!(bulk.last().unwrap()["method"], "update-display");
    }
}

#[test]
fn only_the_frame_boundary_falls_through() {
    let mut unhandled: Vec<String> = instructions()
        .into_iter()
        .filter_map(|i| match i {
            Instruction::Other { method } => Some(method),
            _ => None,
        })
        .collect();
    unhandled.sort();
    unhandled.dedup();
    assert_eq!(unhandled, vec!["update-display"]);
}

#[test]
fn the_session_paints_what_was_done() {
    let all = instructions();
    let views: Vec<_> = all
        .iter()
        .filter_map(|i| match i {
            Instruction::MakeView(view) => Some(view),
            _ => None,
        })
        .collect();
    assert_eq!(views.len(), 3, "the window, its split and the popup");
    let popup = views
        .iter()
        .find(|v| v.kind == ViewKind::Floating)
        .expect("a popup");
    assert!(popup.border.is_some_and(|border| border > 0));
    assert!(popup.border_shape.is_none() || popup.border_shape == Some(BorderShape::DropCurtain));

    let text: String = all
        .iter()
        .filter_map(|i| match i {
            Instruction::Put(put) => Some(put.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains("héllo"), "{text}");
    assert!(text.contains("日本語"), "{text}");
    assert!(text.contains("a popup"), "{text}");

    let wide = all.iter().find_map(|i| match i {
        Instruction::Put(put) if put.text.contains("日本語") => Some(put),
        _ => None,
    });
    let wide = wide.expect("the CJK run");
    assert_eq!(
        usize::from(wide.text_width),
        wide.text.chars().count() + "日本語".chars().count(),
        "cells, not characters"
    );
    assert!(all.iter().any(|i| matches!(i, Instruction::MoveCursor(_))));
    assert!(all.iter().any(|i| matches!(i, Instruction::ModelinePut(_))));
}

#[test]
fn state_outside_frames_uses_lem_servers_messages() {
    let methods: Vec<String> = messages()
        .iter()
        .map(|m| m["method"].as_str().unwrap().to_string())
        .collect();
    assert!(methods.contains(&"update-background".to_string()));
    assert!(methods.contains(&"update-cursor-shape".to_string()));
    assert!(methods.contains(&"set-clipboard-text".to_string()));
}
