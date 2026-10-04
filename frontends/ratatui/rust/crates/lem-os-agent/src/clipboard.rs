//! The clipboard the agent offers: the desktop's, or a file standing in
//! for it.

use std::path::PathBuf;

/// Somewhere text can be copied to and pasted from. `Send`: it is taken
/// out of the shared slot `main` hands the lifeline thread and dropped
/// there (on-session-end handoff), on whichever thread that runs on.
pub trait Clipboard: Send {
    /// The text on it, or `None` when there is nothing readable.
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// The desktop's clipboard, through `arboard` (X11).
struct System(arboard::Clipboard);

impl Clipboard for System {
    fn get(&mut self) -> Option<String> {
        self.0.get_text().ok()
    }

    fn set(&mut self, text: &str) {
        let _ = self.0.set_text(text);
    }
}

/// A file as the clipboard: `LEM_OS_AGENT_CLIPBOARD_FILE`. For tests, and
/// for trying the agent on a machine with no desktop.
struct File(PathBuf);

impl Clipboard for File {
    fn get(&mut self) -> Option<String> {
        std::fs::read_to_string(&self.0).ok()
    }

    fn set(&mut self, text: &str) {
        let _ = std::fs::write(&self.0, text);
    }
}

/// The clipboard to offer, or `None` when there is none: no display
/// server, as over SSH or in a container. That is not an error; the
/// agent says so in `Welcome` and Lem uses the display's instead.
pub fn select() -> Option<Box<dyn Clipboard>> {
    if let Some(path) = std::env::var_os("LEM_OS_AGENT_CLIPBOARD_FILE").filter(|p| !p.is_empty()) {
        return Some(Box::new(File(PathBuf::from(path))));
    }
    match arboard::Clipboard::new() {
        Ok(clipboard) => Some(Box::new(System(clipboard))),
        Err(error) => {
            eprintln!("lem-os-agent: no system clipboard ({error})");
            None
        }
    }
}
