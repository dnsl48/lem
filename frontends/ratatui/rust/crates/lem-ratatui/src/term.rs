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

use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};

#[cfg(unix)]
const TTY: &str = "/dev/tty";
#[cfg(windows)]
const TTY: &str = "CONOUT$";

/// Raw mode and the alternate screen, released on drop.
pub struct Guard {
    tty: File,
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

        enable_raw_mode()?;
        let out = tty.try_clone()?;
        execute!(&out, EnterAlternateScreen, cursor::Hide)?;

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore(&mut &out);
            previous(info);
        }));

        Ok(Some(Self { tty }))
    }

    /// Somewhere to draw.
    pub fn writer(&self) -> io::Result<File> {
        self.tty.try_clone()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.tty.flush();
        let _ = restore(&mut self.tty);
    }
}

fn restore(out: &mut impl Write) -> Result<()> {
    execute!(out, cursor::Show, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
