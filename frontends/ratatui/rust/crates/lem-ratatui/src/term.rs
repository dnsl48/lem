//! Terminal lifecycle.
//!
//! This is a guard rather than a pair of functions because restoring the
//! terminal when Lem dies is the single strongest argument for the
//! two-process split
//! (`../../../docs/adr/0004-two-processes-not-embedded-lisp.md`), and
//! that only holds if restore runs on every exit path — panics included.
//!
//! The terminal is the controlling one, opened by name, not stdio: stdin
//! and stdout carry the protocol (`transport.rs`). crossterm already reads
//! keys and the window size from `/dev/tty` when stdin is not a TTY, so
//! only output has to be pointed there.
//!
//! The mouse is captured (ADR 0013), which takes the terminal's own
//! click-and-drag selection away; most terminals keep it on Shift+drag.
//! Bracketed paste is on, so pasted text arrives as one `Paste`.

use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use crossterm::cursor::{self, SetCursorStyle};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use lem_protocol::v1::TerminalCapabilities;

use crate::keyboard;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

#[cfg(unix)]
const TTY: &str = "/dev/tty";
#[cfg(windows)]
const TTY: &str = "CONOUT$";

/// Raw mode and the alternate screen, released on drop.
pub struct Guard {
    tty: File,
    restoration: Arc<Restoration>,
    capabilities: TerminalCapabilities,
}

struct Restoration {
    tty: File,
    cleanup: Cleanup,
}

impl Restoration {
    fn restore(&self) -> io::Result<()> {
        self.cleanup.restore(&mut &self.tty, disable_raw_mode)
    }
}

/// Shared by the panic hook and guard, without a lock that a panic could
/// strand. Exactly one caller owns restoration, even while unwinding.
#[derive(Default)]
struct Cleanup {
    active: AtomicBool,
    keyboard_pushed: AtomicBool,
}

impl Cleanup {
    fn restore(
        &self,
        out: &mut impl Write,
        disable_raw: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        if !self.active.swap(false, Ordering::AcqRel) {
            return Ok(());
        }

        let keyboard = keyboard::pop_if_owned(out, &self.keyboard_pushed);
        // Leave the screen even if popping the keyboard stack failed.
        let screen = execute!(
            out,
            DisableBracketedPaste,
            DisableMouseCapture,
            SetCursorStyle::DefaultUserShape,
            cursor::Show,
            LeaveAlternateScreen
        );
        // A broken output must never prevent restoring the terminal mode.
        let raw = disable_raw();
        keyboard.and(screen).and(raw)
    }
}

impl Guard {
    /// Take over the controlling terminal, or return `Ok(None)` when there
    /// is none — under a test harness, CI, or a service manager — where
    /// there is nothing to draw on and nothing to restore.
    ///
    /// Installs a panic hook that restores first and then defers to the
    /// previous hook, so a panic message is still printed — but onto a
    /// terminal that can display it.
    pub fn new_if_interactive() -> Result<Option<Self>> {
        let tty = match OpenOptions::new().read(true).write(true).open(TTY) {
            Ok(tty) if tty.is_terminal() => tty,
            _ => return Ok(None),
        };

        let out = tty.try_clone()?;
        let restoration = Arc::new(Restoration {
            tty: out,
            cleanup: Cleanup::default(),
        });
        let mut guard = Self {
            tty,
            restoration,
            capabilities: TerminalCapabilities::default(),
        };
        // Construct the guard before raw mode. No fallible operation may
        // intervene between entering raw mode and arming restoration.
        enable_raw_mode()?;
        guard
            .restoration
            .cleanup
            .active
            .store(true, Ordering::Release);

        let restore_on_panic = Arc::clone(&guard.restoration);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore_on_panic.restore();
            previous(info);
        }));

        execute!(
            &mut guard.tty,
            EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        let legacy =
            std::env::var_os("LEM_RATATUI_KEYBOARD").is_some_and(|value| value == "legacy");
        guard.capabilities = keyboard::negotiate(
            &mut guard.tty,
            &guard.restoration.cleanup.keyboard_pushed,
            legacy,
        );

        Ok(Some(guard))
    }

    /// Somewhere to draw.
    pub fn writer(&self) -> io::Result<File> {
        self.tty.try_clone()
    }

    /// Confirmed active keyboard capabilities for this session.
    pub fn capabilities(&self) -> TerminalCapabilities {
        self.capabilities
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.restoration.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn panic_then_drop_restores_once_and_pops_before_leaving_screen() {
        let cleanup = Cleanup {
            active: AtomicBool::new(true),
            keyboard_pushed: AtomicBool::new(true),
        };
        let mut out = Vec::new();
        let raw_calls = Cell::new(0);
        for _ in 0..2 {
            cleanup
                .restore(&mut out, || {
                    raw_calls.set(raw_calls.get() + 1);
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(raw_calls.get(), 1);
        let output = String::from_utf8(out).unwrap();
        assert!(output.starts_with("\x1b[<1u"));
        assert_eq!(output.matches("\x1b[<1u").count(), 1);
        assert_eq!(output.matches("\x1b[?1049l").count(), 1);
        assert!(output.find("\x1b[<1u") < output.find("\x1b[?1049l"));
    }

    #[test]
    fn raw_mode_is_restored_even_when_all_output_fails() {
        struct BrokenOutput;
        impl Write for BrokenOutput {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed tty"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed tty"))
            }
        }
        let cleanup = Cleanup {
            active: AtomicBool::new(true),
            keyboard_pushed: AtomicBool::new(true),
        };
        let raw_called = Cell::new(false);
        assert!(
            cleanup
                .restore(&mut BrokenOutput, || {
                    raw_called.set(true);
                    Ok(())
                })
                .is_err()
        );
        assert!(raw_called.get());
        assert!(!cleanup.active.load(Ordering::Acquire));
    }

    #[test]
    fn legacy_cleanup_leaves_the_keyboard_stack_alone() {
        let cleanup = Cleanup {
            active: AtomicBool::new(true),
            ..Default::default()
        };
        let mut out = Vec::new();
        cleanup.restore(&mut out, || Ok(())).unwrap();
        assert!(!String::from_utf8(out).unwrap().contains("\x1b[<1u"));
    }
}
