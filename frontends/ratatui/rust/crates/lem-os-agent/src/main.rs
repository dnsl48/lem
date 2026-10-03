//! Lem's OS agent (ADR 0019): what the desktop offers an editor, over
//! `lem.os.v1` on a local socket.
//!
//! The launcher starts it with `--socket PATH` and a pipe on its stdin
//! that only the launcher holds. When that pipe closes — the session is
//! over, or the launcher died — the agent removes its socket and exits.
//! It serves one Lem at a time: an agent belongs to one session.

mod clipboard;
mod serve;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clipboard::Clipboard;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let socket = match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None) if flag == "--socket" => PathBuf::from(path),
        _ => {
            eprintln!("usage: lem-os-agent --socket PATH");
            return ExitCode::from(2);
        }
    };
    match run(&socket) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lem-os-agent: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
fn run(socket: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::net::UnixListener;

    use anyhow::Context;

    // Binding fails when the path exists: a socket is never taken over.
    let listener =
        UnixListener::bind(socket).with_context(|| format!("listening on {}", socket.display()))?;
    // Shared with the lifeline thread, so it can take the clipboard out
    // and drop it — running its `Drop` — before this process exits.
    let clipboard = Arc::new(Mutex::new(clipboard::select()));
    watch_lifeline(socket.to_path_buf(), Arc::clone(&clipboard));
    // Bound, so ours to remove: on every way out of the loop below, not
    // only the lifeline's. (The lifeline's own exit, on stdin closing,
    // removes it separately — `process::exit` runs no destructors here.)
    cleanup(socket, accept(&listener, &clipboard))
}

#[cfg(unix)]
fn accept(
    listener: &std::os::unix::net::UnixListener,
    clipboard: &Mutex<Option<Box<dyn Clipboard>>>,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let session = std::env::var("LEM_SESSION_ID").unwrap_or_default();
    for stream in listener.incoming() {
        let stream = stream.context("accepting a connection")?;
        let reader = stream.try_clone().context("cloning the connection")?;
        match serve::serve(reader, stream, clipboard) {
            Ok(_) => eprintln!("lem-os-agent: session {session}: Lem hung up"),
            Err(error) => eprintln!("lem-os-agent: session {session}: {error:#}"),
        }
    }
    Ok(())
}

/// Remove `socket` and pass `result` through, whichever it is. Called only
/// once the agent owns `socket` (bind succeeded), so this is the one path
/// out of `run` that returns instead of calling `process::exit` — it must
/// not leave the socket behind either.
#[cfg(unix)]
fn cleanup<T>(socket: &std::path::Path, result: anyhow::Result<T>) -> anyhow::Result<T> {
    let _ = std::fs::remove_file(socket);
    result
}

#[cfg(not(unix))]
fn run(_socket: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("only Unix is supported")
}

/// Exit when stdin closes: hand the clipboard back first, then remove
/// the socket.
#[cfg(unix)]
fn watch_lifeline(socket: PathBuf, clipboard: Arc<Mutex<Option<Box<dyn Clipboard>>>>) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        hand_back_clipboard(&clipboard);
        let _ = std::fs::remove_file(&socket);
        std::process::exit(0);
    });
}

/// Take the clipboard out of the slot `accept` shares with us, and drop
/// it here, running its own `Drop` — on X11, `arboard`'s asks the
/// clipboard manager to take over the selection, up to ~100 ms. Called
/// before `watch_lifeline` exits the process: `process::exit` runs no
/// destructors, so whatever is still in the slot by then would never be
/// handed back, and a clipboard manager would lose what Lem copied.
#[cfg(unix)]
fn hand_back_clipboard(clipboard: &Mutex<Option<Box<dyn Clipboard>>>) {
    drop(clipboard.lock().unwrap().take());
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn cleanup_removes_the_socket_whether_the_loop_errored_or_not() {
        for result in [Ok(()), Err(anyhow::anyhow!("accept failed"))] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let socket = file.path().to_path_buf();
            let ok = result.is_ok();
            let outcome = cleanup(&socket, result);
            assert_eq!(outcome.is_ok(), ok);
            assert!(!socket.exists(), "socket removed");
        }
    }

    /// A clipboard that records whether its `Drop` ran, standing in for
    /// `arboard`'s (which, on X11, hands the selection to a clipboard
    /// manager there).
    struct Recording(Arc<std::sync::atomic::AtomicBool>);

    impl Clipboard for Recording {
        fn get(&mut self) -> Option<String> {
            None
        }
        fn set(&mut self, _text: &str) {}
    }

    impl Drop for Recording {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn hand_back_clipboard_drops_what_was_in_the_slot() {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let clipboard: Arc<Mutex<Option<Box<dyn Clipboard>>>> =
            Arc::new(Mutex::new(Some(Box::new(Recording(Arc::clone(&dropped))))));

        hand_back_clipboard(&clipboard);

        assert!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            "Drop ran"
        );
        assert!(clipboard.lock().unwrap().is_none(), "taken out of the slot");
    }

    #[test]
    fn hand_back_clipboard_is_a_no_op_with_none_in_the_slot() {
        let clipboard: Arc<Mutex<Option<Box<dyn Clipboard>>>> = Arc::new(Mutex::new(None));
        hand_back_clipboard(&clipboard); // must not panic
        assert!(clipboard.lock().unwrap().is_none());
    }
}
