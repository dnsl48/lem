//! Terminal display half for Lem.
//!
//! This process owns the terminal. It speaks `lem.relay.v1` over its own
//! stdio (`frontends/ratatui/proto`), paints the frames it is sent into
//! per-view cell buffers, composites them, and draws, with the terminal's
//! own cursor where Lem's is. Starting Lem and connecting the two is
//! `lem-ratatui-launcher`'s job, not this one's.

mod clipboard;
mod input;
mod metrics;
mod paint;
mod screen;
mod term;
mod transport;
mod views;

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{self, Event, KeyEventKind};
use lem_protocol::v1::{self, CursorShape, to_display, to_editor};
use ratatui_core::terminal::Terminal;
use ratatui_crossterm::CrosstermBackend;
use screen::Screen;

/// Size reported when there is no terminal to measure: headless runs and
/// tests.
const HEADLESS_SIZE: (u16, u16) = (80, 24);

/// The terminal's current size, or [`HEADLESS_SIZE`] without one.
fn display_size(interactive: bool) -> (u16, u16) {
    if interactive {
        crossterm::terminal::size().unwrap_or(HEADLESS_SIZE)
    } else {
        HEADLESS_SIZE
    }
}

fn cursor_style(shape: CursorShape) -> SetCursorStyle {
    match shape {
        CursorShape::Bar => SetCursorStyle::SteadyBar,
        CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
        CursorShape::Box | CursorShape::Unspecified => SetCursorStyle::SteadyBlock,
    }
}

/// How the session ended, for the lines `scripts/workload.py` reads.
fn report(frames: usize, metrics: &metrics::Frames) {
    eprintln!("lem-ratatui: Lem exited after {frames} frames");
    eprintln!("lem-ratatui: {}", metrics.report());
}

fn main() -> Result<()> {
    let started = Instant::now();

    // Bound for the whole run: dropping it restores the terminal, so it
    // must outlive the draw loop. Matched by reference — consuming it here
    // would restore the terminal before the first frame is painted.
    let guard = term::Guard::new_if_interactive()?;
    let mut tty = match &guard {
        Some(guard) => Some(guard.writer()?),
        None => None,
    };
    let mut terminal = match &guard {
        Some(guard) => Some(Terminal::new(CrosstermBackend::new(guard.writer()?))?),
        None => None,
    };

    // Lem must be told the real geometry, not an assumed 80x24, or every
    // frame is laid out for the wrong screen.
    let (width, height) = display_size(guard.is_some());

    let (mut reader, mut writer) = transport::stdio(started);
    let mut clipboard = clipboard::Clipboard::new();
    let mut screen = Screen::default();
    let mut shape = None;

    // The reader blocks, and the main loop must also watch the terminal,
    // so reading moves to its own thread and arrives as messages.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            match reader.recv() {
                Ok(Some(received)) => {
                    if tx.send(received).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    eprintln!("lem-ratatui: {error:#}");
                    break;
                }
            }
        }
        // Dropping tx closes the channel, which is how the main loop
        // learns that Lem hung up.
    });

    // The display speaks first. The relay answers Welcome and then its
    // first frame, unasked (ADR 0011).
    let session = uuid::Uuid::now_v7();
    writer.send(to_editor::Message::Hello(v1::Hello {
        protocol_version: v1::PROTOCOL_VERSION,
        session_id: session.to_string(),
        width: u32::from(width),
        height: u32::from(height),
        foreground: None,
        background: None,
    }))?;

    let mut frames = 0usize;
    let mut metrics = metrics::Frames::default();

    loop {
        // Input first, so a keystroke is never delayed behind a frame.
        if terminal.is_some() && event::poll(Duration::from_millis(10))? {
            let message = match event::read()? {
                // Press only: under the kitty protocol and on Windows,
                // releases and repeats arrive too and would double every
                // keystroke.
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    input::key(key).map(to_editor::Message::Key)
                }
                Event::Mouse(mouse) => Some(to_editor::Message::Mouse(input::mouse(mouse))),
                Event::Paste(text) => Some(to_editor::Message::Paste(v1::Paste { text })),
                // Lem lays out again and repaints (lem:update-on-display-resized).
                Event::Resize(columns, rows) => Some(to_editor::Message::Resize(v1::Resize {
                    width: u32::from(columns),
                    height: u32::from(rows),
                })),
                _ => None,
            };
            if let Some(message) = message {
                writer.send(message)?;
            }
        }

        loop {
            let received = match rx.try_recv() {
                Ok(received) => received,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    report(frames, &metrics);
                    return Ok(());
                }
            };
            let transport::Received {
                message,
                bytes,
                decode,
            } = received;
            match message.message {
                Some(to_display::Message::Frame(frame)) => {
                    metrics.record(bytes, decode);
                    screen.apply(frame);
                    frames += 1;
                    let Some(terminal) = terminal.as_mut() else {
                        // Headless: nothing to draw to, so report what the
                        // frame would have painted and stop.
                        eprintln!("lem-ratatui: {} views composited", screen.views.len());
                        eprintln!("lem-ratatui: {}", metrics.report());
                        return Ok(());
                    };
                    // The cursor's shape goes out before the frame, which
                    // shows the cursor where Lem's is, or hides it.
                    let wanted = screen.cursor_shape();
                    if wanted != shape {
                        if let (Some(tty), Some(wanted)) = (tty.as_mut(), wanted) {
                            crossterm::execute!(tty, cursor_style(wanted))?;
                            tty.flush()?;
                        }
                        shape = wanted;
                    }
                    terminal.draw(|frame| {
                        frame.render_widget(&screen, frame.area());
                        if let Some(position) = screen.cursor_position() {
                            frame.set_cursor_position(position);
                        }
                    })?;
                }
                // The relay waits only 0.1s for this, so it is answered
                // inline rather than handed to a thread.
                Some(to_display::Message::ClipboardRequest(_)) => {
                    writer.send(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                        reply_to: message.seq,
                        text: clipboard.get(),
                    }))?;
                }
                Some(to_display::Message::SetClipboard(set)) => clipboard.set(&set.text),
                Some(to_display::Message::Exit(exit)) => {
                    // Restore the terminal before saying why, so it can
                    // be read.
                    drop(terminal);
                    drop(guard);
                    if !exit.reason.is_empty() {
                        eprintln!("lem-ratatui: {}", exit.reason);
                    }
                    report(frames, &metrics);
                    return Ok(());
                }
                Some(to_display::Message::Welcome(_)) | None => {}
            }
        }
    }
}
