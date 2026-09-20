//! Terminal display half for Lem.
//!
//! This process owns the terminal. It spawns Lem as a child speaking the
//! `lem-server` JSON-RPC protocol over stdio, paints the frames it is
//! sent into per-view cell buffers, composites them, and draws.
//!
//! Status: rendering only. Input is Task 7 in `../../../docs/poc-plan.md`.

mod child;
mod paint;
mod term;
mod views;

use std::io;
use std::path::PathBuf;

use anyhow::Result;
use lem_protocol::rpc::{self, Incoming};
use lem_protocol::{Bulk, Instruction};
use ratatui_core::terminal::Terminal;
use ratatui_crossterm::CrosstermBackend;
use views::Registry;

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn size() -> serde_json::Value {
    serde_json::json!({"width": WIDTH, "height": HEIGHT})
}

/// Apply one `bulk` frame to the registry.
///
/// Returns true when the frame is complete — every bulk ends with
/// `update-display`, which is the signal to composite and draw. See
/// `../../../docs/protocol-notes.md` section 14.
fn apply_frame(registry: &mut Registry, bulk: Bulk) -> bool {
    let mut complete = false;
    for raw in bulk {
        // A recognised method carrying an argument that does not fit its
        // type is worth knowing about, but never worth killing the editor
        // over: skip the instruction and keep the frame.
        let instruction = match raw.parse() {
            Ok(instruction) => instruction,
            Err(error) => {
                eprintln!("lem-ratatui: undecodable instruction: {error}");
                continue;
            }
        };
        match instruction {
            Instruction::MakeView(view) => registry.insert(view),
            Instruction::DeleteView(arg) => registry.remove(arg.view_info.id),
            Instruction::ResizeView(r) => registry.resize(r.view_info.id, r.width, r.height),
            Instruction::MoveView(m) => registry.move_to(m.view_info.id, m.x, m.y),
            Instruction::Put(p) | Instruction::ModelinePut(p) => {
                if let Some(vb) = registry.get_mut(p.view_info.id) {
                    paint::put(vb, &p);
                }
            }
            Instruction::ClearEol(c) => {
                if let Some(vb) = registry.get_mut(c.view_info.id) {
                    paint::clear_eol(vb, c.x, c.y);
                }
            }
            Instruction::Clear(c) | Instruction::ClearEob(c) => {
                if let Some(vb) = registry.get_mut(c.view_info.id) {
                    paint::clear_eob(vb, c.y);
                }
            }
            Instruction::MoveCursor(_) => {}
            Instruction::Other { method } => {
                complete |= method == "update-display";
            }
        }
    }
    complete
}

fn main() -> Result<()> {
    let program = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../lem-ratatui-lisp"));
    let log = PathBuf::from("/tmp/lem-ratatui.log");

    // Held for the whole run; dropping it restores the terminal. None when
    // stdout is not a TTY, where there is nothing to take over and the
    // backend would have no size to report.
    // Bound for the whole run: dropping it restores the terminal, so it
    // must outlive the draw loop. Matched by reference — consuming it here
    // would restore the terminal before the first frame is painted.
    let guard = term::Guard::new_if_interactive()?;
    let mut terminal = match &guard {
        Some(_) => Some(Terminal::new(CrosstermBackend::new(io::stdout()))?),
        None => None,
    };

    let mut lem = child::Lem::spawn(&program, &log)?;
    let mut registry = Registry::default();

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
    let mut frames = 0usize;

    while let Some(incoming) = lem.recv()? {
        match incoming {
            Incoming::Response { .. } if !logged_in => {
                logged_in = true;
                lem.send(&rpc::notification(
                    "redraw",
                    serde_json::json!({"size": size()}),
                )?)?;
            }
            Incoming::Notification { method, params } if method == "bulk" => {
                if apply_frame(&mut registry, serde_json::from_value(params)?) {
                    frames += 1;
                    match terminal.as_mut() {
                        Some(terminal) => {
                            terminal.draw(|frame| frame.render_widget(&registry, frame.area()))?;
                        }
                        None if frames >= 1 => {
                            // Headless: nothing to draw to, so report what
                            // the frame would have painted and stop.
                            eprintln!("lem-ratatui: {} views composited", registry.len());
                            return Ok(());
                        }
                        None => {}
                    }
                }
            }
            Incoming::Notification { .. } | Incoming::Response { .. } => {}
        }
    }

    eprintln!("lem-ratatui: Lem exited after {frames} frames");
    Ok(())
}
