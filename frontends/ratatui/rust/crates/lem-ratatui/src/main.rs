//! Terminal display half for Lem.
//!
//! This process owns the terminal. It spawns Lem as a child speaking the
//! `lem-server` JSON-RPC protocol over stdio, paints the frames it is
//! sent into per-view cell buffers, composites them, and draws.
//!
//! Status: rendering, keyboard input, reflow on resize and clipboard.

#[cfg(feature = "bundle")]
mod bundle;
mod child;
mod clipboard;
mod input;
mod metrics;
mod paint;
mod term;
mod views;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use lem_protocol::rpc::{self, Incoming};
use lem_protocol::{Bulk, Instruction};
use ratatui_core::terminal::Terminal;
use ratatui_crossterm::CrosstermBackend;
use views::Registry;

/// Size reported when there is no terminal to measure — headless runs and
/// tests. Matches the geometry the committed fixture was captured at.
const HEADLESS_SIZE: (u16, u16) = (80, 24);

fn size_json((width, height): (u16, u16)) -> serde_json::Value {
    serde_json::json!({"width": width, "height": height})
}

/// The terminal's current size, or [`HEADLESS_SIZE`] without one.
fn display_size(interactive: bool) -> (u16, u16) {
    if interactive {
        crossterm::terminal::size().unwrap_or(HEADLESS_SIZE)
    } else {
        HEADLESS_SIZE
    }
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
            Instruction::Put(p) => {
                if let Some(vb) = registry.get_mut(p.view_info.id) {
                    paint::put(vb, &p);
                }
            }
            Instruction::ModelinePut(p) => {
                if let Some(vb) = registry.get_mut(p.view_info.id) {
                    paint::modeline_put(vb, &p);
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
    let program = match std::env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        #[cfg(feature = "bundle")]
        None => bundle::image()?,
        #[cfg(not(feature = "bundle"))]
        None => PathBuf::from("../lem-ratatui-lisp"),
    };
    let log = PathBuf::from("/tmp/lem-ratatui.log");

    // Bound for the whole run: dropping it restores the terminal, so it
    // must outlive the draw loop. Matched by reference — consuming it here
    // would restore the terminal before the first frame is painted.
    let guard = term::Guard::new_if_interactive()?;
    let mut terminal = match &guard {
        Some(_) => Some(Terminal::new(CrosstermBackend::new(io::stdout()))?),
        None => None,
    };

    // Lem must be told the real geometry, not an assumed 80x24, or every
    // frame is laid out for the wrong screen.
    let mut size = display_size(guard.is_some());

    // `_lem` is held only for its Drop, which kills and reaps the child.
    let (_lem, mut reader, mut writer) = child::Lem::spawn(&program, &log)?;
    let mut clipboard = clipboard::Clipboard::new();
    let mut registry = Registry::default();

    // The reader blocks, and the main loop must also watch the terminal,
    // so reading moves to its own thread and arrives as messages.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(received)) = reader.recv() {
            if tx.send(received).is_err() {
                break;
            }
        }
        // Dropping tx closes the channel, which is how the main loop
        // learns that Lem exited.
    });

    writer.send(&rpc::request(
        1,
        "login",
        serde_json::json!({
            "size": size_json(size),
            "foreground": "#DDDDDD",
            "background": "#111111",
        }),
    )?)?;

    let mut logged_in = false;
    let mut frames = 0usize;
    let mut metrics = metrics::Frames::default();

    loop {
        // Keyboard first, so a keystroke is never delayed behind a frame.
        if terminal.is_some() && event::poll(Duration::from_millis(10))? {
            match event::read()? {
                // Press only: under the kitty protocol and on Windows,
                // releases and repeats arrive too and would double every
                // keystroke.
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(payload) = input::convert(key) {
                        writer.send(&rpc::notification(
                            "input",
                            serde_json::json!({"kind": "key", "value": payload}),
                        )?)?;
                    }
                }
                // `redraw` is what reflows: it resizes the display, tells
                // every client, and forces a full repaint. Lem answers with
                // resize-view and move-view for each window, which the
                // registry applies.
                Event::Resize(columns, rows) => {
                    size = (columns, rows);
                    writer.send(&rpc::notification(
                        "redraw",
                        serde_json::json!({
                            "size": size_json(size),
                        }),
                    )?)?;
                }
                _ => {}
            }
        }

        loop {
            let received = match rx.try_recv() {
                Ok(received) => received,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    eprintln!("lem-ratatui: Lem exited after {frames} frames");
                    eprintln!("lem-ratatui: {}", metrics.report());
                    return Ok(());
                }
            };
            let child::Received {
                incoming,
                bytes,
                decode,
            } = received;
            match incoming {
                Incoming::Response { .. } if !logged_in => {
                    logged_in = true;
                    writer.send(&rpc::notification(
                        "redraw",
                        serde_json::json!({"size": size_json(size)}),
                    )?)?;
                }
                Incoming::Notification { method, params } if method == "bulk" => {
                    // Decode cost is the envelope plus the instruction
                    // array; both are JSON work that a different codec
                    // would change, so both are counted.
                    let started = Instant::now();
                    let bulk: Bulk = serde_json::from_value(params)?;
                    metrics.record(bytes, decode + started.elapsed());
                    if apply_frame(&mut registry, bulk) {
                        frames += 1;
                        match terminal.as_mut() {
                            Some(terminal) => {
                                terminal
                                    .draw(|frame| frame.render_widget(&registry, frame.area()))?;
                            }
                            None => {
                                // Headless: nothing to draw to, so report
                                // what the frame would have painted and stop.
                                eprintln!("lem-ratatui: {} views composited", registry.len());
                                eprintln!("lem-ratatui: {}", metrics.report());
                                return Ok(());
                            }
                        }
                    }
                }
                // Lem waits only 0.1s for this before giving up, so it
                // is answered inline rather than handed to a thread.
                Incoming::Notification { method, .. } if method == "get-clipboard-text" => {
                    let text = clipboard.get();
                    writer.send(&rpc::notification(
                        "got-clipboard-text",
                        serde_json::json!({ "text": text }),
                    )?)?;
                }
                Incoming::Notification { method, params } if method == "set-clipboard-text" => {
                    if let Some(text) = params.get("text").and_then(|value| value.as_str()) {
                        clipboard.set(text);
                    }
                }
                _ => {}
            }
        }
    }
}
