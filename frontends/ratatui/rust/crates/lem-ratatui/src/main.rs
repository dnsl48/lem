//! Terminal display half for Lem.
//!
//! This process owns the terminal. It speaks `lem.display.v1` over its own
//! stdio (`frontends/ratatui/proto`), paints the frames it is sent into
//! per-view cell buffers, composites them, and draws, with the terminal's
//! own cursor where Lem's is. Starting Lem and connecting the two is
//! `lem-ratatui-launcher`'s job, not this one's.

mod ansi;
mod colors;
mod events;
#[cfg(test)]
mod golden;
mod input;
mod keyboard;
mod metrics;
mod paint;
mod present;
mod probe;
mod screen;
mod support;
mod term;
mod transport;
mod views;

use std::io::BufWriter;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use lem_protocols::display::v1::{self, to_display, to_editor};
use paint::Layer;
use present::Presenter;
use screen::Screen;
use termina::Event;
use termina::event::KeyEventKind;

/// Size reported when there is no terminal to measure: headless runs and
/// tests.
const HEADLESS_SIZE: (u16, u16) = (80, 24);

/// The terminal's current size, or [`HEADLESS_SIZE`] without one.
fn display_size(guard: Option<&term::Guard>) -> (u16, u16) {
    guard
        .and_then(|guard| guard.size().ok())
        .unwrap_or(HEADLESS_SIZE)
}

/// How the session ended, for the lines `scripts/workload.py` reads.
fn report(frames: usize, metrics: &metrics::Frames) {
    eprintln!("lem-ratatui: Lem exited after {frames} frames");
    eprintln!("lem-ratatui: {}", metrics.report());
}

/// Whether this display writes the terminal's clipboard with OSC 52 (ADR
/// 0020): only with a terminal to write it to, whatever `env` says.
fn clipboard_write(terminal: bool, env: impl Fn(&str) -> Option<String>) -> bool {
    terminal && support::osc52(env)
}

/// This session's id: the launcher's, from `LEM_SESSION_ID` (ADR 0019),
/// or a new UUIDv7 when the display runs without one, as by hand.
fn session_id(from_launcher: Option<String>) -> String {
    from_launcher
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string())
}

fn main() -> Result<()> {
    let started = Instant::now();

    // Asked before the terminal is taken over and before termina reads
    // input, which would swallow the reply as keystrokes.
    let colors = colors::query();

    // Bound for the whole run: dropping it restores the terminal, so it
    // must outlive the draw loop. Matched by reference — consuming it here
    // would restore the terminal before the first frame is painted.
    let mut guard = term::Guard::new_if_interactive()?;
    let styled_underlines = support::styled_underlines(|name| std::env::var(name).ok());
    let hyperlinks = support::hyperlinks(|name| std::env::var(name).ok());
    let osc52 = clipboard_write(guard.is_some(), |name| std::env::var(name).ok());
    let mut presenter = match &guard {
        Some(guard) => Some(
            Presenter::new(BufWriter::new(guard.writer()?), styled_underlines)
                .with_hyperlinks(hyperlinks),
        ),
        None => None,
    };
    // The terminal's input, for as long as there is a terminal.
    let mut events = guard.as_mut().map(term::Guard::input);

    // Lem must be told the real geometry, not an assumed 80x24, or every
    // frame is laid out for the wrong screen. Kept up to date from resize
    // events: each frame is composited at this size.
    let (mut width, mut height) = display_size(guard.as_ref());

    let (mut reader, mut writer) = transport::stdio(started);
    let mut screen = Screen::default();

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
    let session = session_id(std::env::var("LEM_SESSION_ID").ok());
    writer.send(to_editor::Message::Hello(v1::Hello {
        protocol_version: v1::PROTOCOL_VERSION,
        session_id: session,
        width: u32::from(width),
        height: u32::from(height),
        foreground: colors.foreground,
        background: colors.background,
        capabilities: Some(v1::TerminalCapabilities {
            clipboard_write: osc52,
            ..guard
                .as_ref()
                .map(term::Guard::capabilities)
                .unwrap_or_default()
        }),
    }))?;

    let mut frames = 0usize;
    let mut metrics = metrics::Frames::default();

    loop {
        // Input first, so a keystroke is never delayed behind a frame.
        if let Some(events) = events.as_mut()
            && let Some(event) = events.next(Duration::from_millis(10))?
        {
            let message = match event {
                // Press only: under the kitty protocol and on Windows,
                // releases and repeats arrive too and would double every
                // keystroke.
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    input::key(key).map(to_editor::Message::Key)
                }
                Event::Mouse(mouse) => Some(to_editor::Message::Mouse(input::mouse(mouse))),
                Event::Paste(text) => Some(to_editor::Message::Paste(v1::Paste { text })),
                // Lem lays out again and repaints (lem:update-on-display-resized).
                Event::WindowResized(size) => {
                    (width, height) = (size.cols, size.rows);
                    Some(to_editor::Message::Resize(v1::Resize {
                        width: u32::from(size.cols),
                        height: u32::from(size.rows),
                    }))
                }
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
                    let Some(presenter) = presenter.as_mut() else {
                        // Headless: nothing to draw to, so report what the
                        // frame would have painted and stop.
                        eprintln!("lem-ratatui: {} views composited", screen.views.len());
                        eprintln!("lem-ratatui: {}", metrics.report());
                        return Ok(());
                    };
                    let mut layer = Layer::new(width, height);
                    screen.render_into(&mut layer);
                    presenter.present(&layer, screen.cursor_position(), screen.cursor_shape())?;
                }
                // The display has no clipboard to read (ADR 0020); a relay
                // that asks anyway is answered at once, with nothing.
                Some(to_display::Message::ClipboardRequest(_)) => {
                    writer.send(to_editor::Message::ClipboardReply(v1::ClipboardReply {
                        reply_to: message.seq,
                        text: None,
                    }))?;
                }
                Some(to_display::Message::SetClipboard(set)) => {
                    if let Some(presenter) = presenter.as_mut().filter(|_| osc52) {
                        presenter.set_clipboard(&set.text)?;
                    }
                }
                Some(to_display::Message::Exit(exit)) => {
                    // Restore the terminal before saying why, so it can
                    // be read.
                    drop(presenter);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headless_display_never_writes_the_clipboard() {
        let forced = |name: &str| (name == "LEM_RATATUI_OSC52").then(|| "1".to_owned());
        assert!(!clipboard_write(false, forced));
        assert!(clipboard_write(true, forced));
    }

    #[test]
    fn the_launchers_session_id_is_used() {
        assert_eq!(
            session_id(Some("from-the-launcher".into())),
            "from-the-launcher"
        );
    }

    #[test]
    fn without_one_the_display_makes_a_uuid_v7() {
        for missing in [None, Some(String::new())] {
            let id = session_id(missing);
            assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 7);
        }
    }
}
