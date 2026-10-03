//! The agent as the launcher runs it: a socket to listen on, a pipe on
//! stdin, and the clipboard a file (`LEM_OS_AGENT_CLIPBOARD_FILE`).

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use lem_protocols::framing;
use lem_protocols::os::v1::{self, to_agent, to_editor};
use prost::Message;

struct Agent {
    child: Child,
    socket: PathBuf,
    /// Held so the directory outlives the agent.
    _dir: tempfile::TempDir,
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(clipboard_file: Option<&Path>) -> Agent {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("os-agent.sock");
    let mut command = Command::new(env!("CARGO_BIN_EXE_lem-os-agent"));
    command
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::piped())
        .env("LEM_SESSION_ID", "test-session");
    match clipboard_file {
        Some(file) => command.env("LEM_OS_AGENT_CLIPBOARD_FILE", file),
        None => command.env_remove("LEM_OS_AGENT_CLIPBOARD_FILE"),
    };
    let child = command.spawn().unwrap();
    Agent {
        child,
        socket,
        _dir: dir,
    }
}

fn connect(socket: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(socket) {
            Ok(stream) => return stream,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("connecting to {}: {error}", socket.display()),
        }
    }
}

fn send(stream: &mut UnixStream, seq: u64, message: to_agent::Message) {
    let envelope = v1::ToAgent {
        seq,
        time_us: 0,
        message: Some(message),
    };
    framing::write_delimited(stream, &envelope).unwrap();
}

fn receive(stream: &mut UnixStream) -> to_editor::Message {
    let body = framing::read_delimited(stream).unwrap().expect("a message");
    v1::ToEditor::decode(body.as_slice())
        .unwrap()
        .message
        .expect("a known message")
}

fn hello(stream: &mut UnixStream) -> v1::Welcome {
    send(
        stream,
        1,
        to_agent::Message::Hello(v1::Hello {
            protocol_version: v1::PROTOCOL_VERSION,
            session_id: "test-session".into(),
        }),
    );
    match receive(stream) {
        to_editor::Message::Welcome(welcome) => welcome,
        other => panic!("expected Welcome, got {other:?}"),
    }
}

fn wait(child: &mut Child, within: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn welcomes_lem_and_offers_the_file_clipboard() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let agent = start(Some(file.path()));
    let welcome = hello(&mut connect(&agent.socket));
    assert_eq!(welcome.session_id, "test-session");
    assert_eq!(welcome.reply_to, 1);
    assert_eq!(
        welcome.capabilities,
        Some(v1::Capabilities { clipboard: true })
    );
}

#[test]
fn pastes_and_copies_through_the_clipboard() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "on the clipboard").unwrap();
    let agent = start(Some(file.path()));
    let mut stream = connect(&agent.socket);
    hello(&mut stream);

    send(
        &mut stream,
        2,
        to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}),
    );
    assert_eq!(
        receive(&mut stream),
        to_editor::Message::ClipboardReply(v1::ClipboardReply {
            reply_to: 2,
            text: Some("on the clipboard".into()),
        })
    );

    send(
        &mut stream,
        3,
        to_agent::Message::SetClipboard(v1::SetClipboard {
            text: "copied".into(),
        }),
    );
    // The agent does not answer SetClipboard; a request after it is
    // answered only once the set is done, so asking again orders them.
    send(
        &mut stream,
        4,
        to_agent::Message::ClipboardRequest(v1::ClipboardRequest {}),
    );
    receive(&mut stream);
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "copied");
}

#[test]
fn exits_and_removes_its_socket_when_stdin_closes() {
    let mut agent = start(None);
    drop(connect(&agent.socket));
    drop(agent.child.stdin.take());
    let status = wait(&mut agent.child, Duration::from_secs(5)).expect("exited");
    assert!(status.success(), "{status}");
    assert!(!agent.socket.exists(), "socket removed");
}

#[test]
fn refuses_a_socket_path_that_exists() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("taken");
    std::fs::write(&socket, "").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lem-os-agent"))
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    assert!(socket.exists(), "not removed: it was not the agent's");
}

#[test]
fn bad_arguments_are_a_usage_error() {
    let status = Command::new(env!("CARGO_BIN_EXE_lem-os-agent"))
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
