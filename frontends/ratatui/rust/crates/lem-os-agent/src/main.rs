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
