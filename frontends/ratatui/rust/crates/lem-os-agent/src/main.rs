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
    watch_lifeline(socket.to_path_buf());
    // Bound, so ours to remove: on every way out of the loop below, not
    // only the lifeline's. (The lifeline's own exit, on stdin closing,
    // removes it separately — `process::exit` runs no destructors here.)
    cleanup(socket, accept(&listener))
}

#[cfg(unix)]
fn accept(listener: &std::os::unix::net::UnixListener) -> anyhow::Result<()> {
    use anyhow::Context;

    let session = std::env::var("LEM_SESSION_ID").unwrap_or_default();
    let mut clipboard = clipboard::select();
    for stream in listener.incoming() {
        let stream = stream.context("accepting a connection")?;
        let reader = stream.try_clone().context("cloning the connection")?;
        match serve::serve(reader, stream, &mut clipboard) {
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

/// Exit when stdin closes, removing the socket first.
#[cfg(unix)]
fn watch_lifeline(socket: PathBuf) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = std::fs::remove_file(&socket);
        std::process::exit(0);
    });
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
}
