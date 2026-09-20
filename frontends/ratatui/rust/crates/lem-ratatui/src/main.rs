//! Terminal display half for Lem.
//!
//! This process owns the terminal. It spawns Lem as a child speaking the
//! `lem-server` JSON-RPC protocol over stdio, paints the frames it is
//! sent into a cell buffer, and sends key and mouse events back.
//!
//! Status: transport and terminal lifecycle only. Frames are counted, not
//! yet painted — that is Task 6 in `../../../docs/poc-plan.md`.

mod child;
mod term;
mod views;

use std::path::PathBuf;

use anyhow::Result;
use lem_protocol::rpc::{self, Incoming};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn size() -> serde_json::Value {
    serde_json::json!({"width": WIDTH, "height": HEIGHT})
}

fn main() -> Result<()> {
    let program = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../lem-ratatui-lisp"));
    let log = PathBuf::from("/tmp/lem-ratatui.log");

    // Held for the whole run; dropping it restores the terminal. None when
    // stdout is not a TTY, where there is nothing to take over.
    let _guard = term::Guard::new_if_interactive()?;

    let mut lem = child::Lem::spawn(&program, &log)?;

    // Both halves of the handshake are mandatory. Colours must be non-nil
    // or the editor's unbound colour slots make every update-display throw
    // and it emits nothing, forever; and the login *response* must be
    // followed by `redraw` before any frame arrives. See
    // docs/protocol-notes.md section 11.
    lem.send(&rpc::request(
        1,
        "login",
        serde_json::json!({
            "size": size(),
            "foreground": "#DDDDDD",
            "background": "#111111",
        }),
    )?)?;

    let mut logged_in = false;
    let mut notifications = 0usize;

    // Stop after one complete frame. Every `bulk` ends with an
    // `update-display` instruction — one bulk is exactly one frame — and
    // the editor keeps emitting them indefinitely (cursor blink, modeline
    // clock), so a fixed message count would either hang or cut a frame
    // in half.
    while let Some(incoming) = lem.recv()? {
        match incoming {
            Incoming::Response { .. } if !logged_in => {
                logged_in = true;
                lem.send(&rpc::notification(
                    "redraw",
                    serde_json::json!({"size": size()}),
                )?)?;
            }
            Incoming::Notification { method, params } => {
                notifications += 1;
                if method == "bulk" {
                    let instructions = params.as_array().map_or(&[][..], Vec::as_slice);
                    let complete = instructions.iter().any(|i| {
                        i.get("method").and_then(|m| m.as_str()) == Some("update-display")
                    });
                    eprintln!(
                        "lem-ratatui: frame of {} instructions{}",
                        instructions.len(),
                        if complete { " (complete)" } else { "" }
                    );
                    if complete {
                        eprintln!("lem-ratatui: first frame after {notifications} notifications");
                        return Ok(());
                    }
                }
            }
            Incoming::Response { .. } => {}
        }
    }

    anyhow::bail!("Lem exited after {notifications} notifications without a complete frame")
}
