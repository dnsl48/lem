//! System clipboard access.
//!
//! Lem asks for the clipboard by notification and waits on a queue with a
//! **0.1 second timeout** (`lem-if:clipboard-paste` in
//! `frontends/server/main.lisp`). Missing that deadline is not fatal —
//! the paste simply yields nothing — but it does mean the read has to be
//! synchronous and quick rather than handed to a thread.

use arboard::Clipboard as SystemClipboard;

/// The system clipboard, or a no-op when there is none to talk to.
///
/// Initialisation fails on a machine with no display server — over SSH
/// without forwarding, in a container, in CI. That is not an error worth
/// stopping for: the editor keeps working and copy and paste do nothing,
/// which is exactly what happened before this existed.
pub struct Clipboard {
    inner: Option<SystemClipboard>,
}

impl Clipboard {
    pub fn new() -> Self {
        let inner = match SystemClipboard::new() {
            Ok(clipboard) => Some(clipboard),
            Err(error) => {
                eprintln!("lem-ratatui: no system clipboard ({error}); copy and paste disabled");
                None
            }
        };
        Self { inner }
    }

    /// Current clipboard text, or an empty string.
    ///
    /// An empty string is returned rather than nothing on failure because
    /// Lem is waiting: replying promptly with nothing beats letting it
    /// time out, and the visible result is the same.
    pub fn get(&mut self) -> String {
        self.inner
            .as_mut()
            .and_then(|clipboard| clipboard.get_text().ok())
            .unwrap_or_default()
    }

    pub fn set(&mut self, text: &str) {
        if let Some(clipboard) = self.inner.as_mut() {
            let _ = clipboard.set_text(text);
        }
    }
}
