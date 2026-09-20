//! Terminal lifecycle.
//!
//! This is a guard rather than a pair of functions because restoring the
//! terminal when Lem dies is the single strongest argument for the
//! two-process split
//! (`../../../docs/adr/0004-two-processes-not-embedded-lisp.md`), and
//! that only holds if restore runs on every exit path — panics included.

use std::io::{self, IsTerminal, Stdout, Write};

use anyhow::Result;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};

/// Raw mode and the alternate screen, released on drop.
pub struct Guard {
    out: Stdout,
}

impl Guard {
    /// Take over the terminal.
    ///
    /// Installs a panic hook that restores first and then defers to the
    /// previous hook, so a panic message is still printed — but onto a
    /// terminal that can display it.
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, cursor::Hide)?;

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore();
            previous(info);
        }));

        Ok(Self { out })
    }

    /// Take over the terminal only when there is one.
    ///
    /// Returns `Ok(None)` when stdout is not a TTY — under a test harness,
    /// a pipe, or CI — where `enable_raw_mode` would fail and there is
    /// nothing to restore anyway.
    pub fn new_if_interactive() -> Result<Option<Self>> {
        if io::stdout().is_terminal() {
            Ok(Some(Self::new()?))
        } else {
            Ok(None)
        }
    }

    /// The stream to build a backend on.
    // Unused until Task 6 constructs the CrosstermBackend from it; kept
    // here because it is the Guard's whole reason for owning a Stdout.
    #[allow(dead_code)]
    pub fn out(&mut self) -> &mut Stdout {
        &mut self.out
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.out.flush();
        let _ = restore();
    }
}

fn restore() -> Result<()> {
    execute!(io::stdout(), cursor::Show, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
