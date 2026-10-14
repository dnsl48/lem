//! Process-level checks for launcher routing. The child scripts record what
//! the launcher starts; acceptance tests exercise the real saved Lisp image.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Children {
    dir: tempfile::TempDir,
    lisp: PathBuf,
    display: PathBuf,
    agent: PathBuf,
}

impl Children {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, body: &str| {
            let path = dir.path().join(name);
            fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        };
        let lisp = script(
            "lisp",
            r#"printf '%s\n' "$@" > "$TEST_DIR/args"
printf '%s\n' "${LEM_SESSION_ID-unset}" > "$TEST_DIR/session"
printf 'Lem test information\n'
printf 'Lem test diagnostic\n' >&2
case "${TEST_LISP_MODE-exit}" in
    signal) kill -TERM "$$" ;;
    wait) exec sleep 30 ;;
esac
exit "$TEST_EXIT""#,
        );
        let display = script(
            "display",
            r#"touch "$TEST_DIR/display-started"
case "${TEST_DISPLAY_MODE-drain}" in
    drain) cat > /dev/null ;;
    first-line) IFS= read -r line ;;
esac
exit "${TEST_DISPLAY_EXIT-0}""#,
        );
        let agent = script(
            "agent",
            r#"touch "$TEST_DIR/agent-started"
cat > /dev/null"#,
        );
        fs::create_dir(dir.path().join("runtime")).unwrap();
        Self {
            dir,
            lisp,
            display,
            agent,
        }
    }

    fn command(&self, args: &[&str], status: u8) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lem-curses-launcher"));
        command
            .args(args)
            .env("LEM_CURSES_LISP", &self.lisp)
            .env("LEM_CURSES_TERMINAL", &self.display)
            .env("LEM_CURSES_OS_AGENT", &self.agent)
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .env("XDG_RUNTIME_DIR", self.dir.path().join("runtime"))
            .env_remove("LEM_SESSION_ID")
            .env("TEST_DIR", self.dir.path())
            .env("TEST_EXIT", status.to_string())
            .env("TEST_LISP_MODE", "exit")
            .env("TEST_DISPLAY_MODE", "drain")
            .env("TEST_DISPLAY_EXIT", "0");
        command
    }

    fn run(&self, args: &[&str], status: u8) -> Output {
        self.command(args, status).output().unwrap()
    }

    fn assert_args(&self, args: &[&str]) {
        let actual = fs::read_to_string(self.dir.path().join("args")).unwrap();
        let expected = std::iter::once("--end-runtime-options")
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(actual, expected);
    }

    fn assert_no_session(&self) {
        assert!(!self.dir.path().join("display-started").exists());
        assert!(!self.dir.path().join("agent-started").exists());
        assert!(!self.dir.path().join("state").exists());
        assert_eq!(
            fs::read_dir(self.dir.path().join("runtime"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read_to_string(self.dir.path().join("session")).unwrap(),
            "unset\n"
        );
    }

    fn assert_lem_failure(&self, output: &Output) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Lem exited with"), "{stderr}");
        assert!(!stderr.contains("the display exited"), "{stderr}");
        let session = fs::read_to_string(self.dir.path().join("session")).unwrap();
        let log = self
            .dir
            .path()
            .join("state/lem-curses")
            .join(format!("lem-{}.log", session.trim()));
        assert!(
            stderr.contains(&format!("; see {}\n", log.display())),
            "{stderr}"
        );
        assert_eq!(fs::read_to_string(log).unwrap(), "Lem test diagnostic\n");
    }
}

#[test]
fn informational_arguments_inherit_stdio_without_starting_a_session() {
    // Keep these vectors aligned with lisp/tests/main.lisp, which checks
    // their interpretation against Lem's real parser.
    for args in [
        vec!["-h"],
        vec!["--help"],
        vec!["-v"],
        vec!["--version"],
        vec!["README.md", "--help"],
        vec!["--", "--version"],
        vec!["--version", "--help"],
        vec!["--help", "--eval"],
    ] {
        let children = Children::new();
        let output = children.run(&args, 0);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stdout, b"Lem test information\n");
        assert_eq!(output.stderr, b"Lem test diagnostic\n");
        children.assert_args(&args);
        children.assert_no_session();
    }
}

#[test]
fn informational_child_failure_is_propagated() {
    let children = Children::new();
    let output = children.run(&["--help"], 23);
    assert_eq!(output.status.code(), Some(23));
    assert_eq!(output.stdout, b"Lem test information\n");
    assert_eq!(output.stderr, b"Lem test diagnostic\n");
    children.assert_no_session();
}

#[test]
fn informational_calls_do_not_need_display_or_agent_executables() {
    let mut children = Children::new();
    children.display = children.dir.path().join("missing-display");
    children.agent = children.dir.path().join("missing-agent");
    let output = children.run(&["--version"], 0);
    assert!(output.status.success(), "{output:?}");
    children.assert_no_session();
}

#[test]
fn informational_calls_clear_an_inherited_session_id() {
    let children = Children::new();
    let output = children
        .command(&["--help"], 0)
        .env("LEM_SESSION_ID", "outer-session")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    children.assert_args(&["--help"]);
    children.assert_no_session();
}

#[test]
fn editor_sessions_replace_an_inherited_session_id() {
    let children = Children::new();
    let output = children
        .command(&[], 0)
        .env("LEM_SESSION_ID", "outer-session")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(children.dir.path().join("display-started").exists());
    assert!(children.dir.path().join("agent-started").exists());
    let session = fs::read_to_string(children.dir.path().join("session")).unwrap();
    assert_ne!(session.trim(), "outer-session");
    assert!(uuid::Uuid::parse_str(session.trim()).is_ok(), "{session:?}");
}

#[test]
fn option_operands_and_editor_arguments_stay_on_the_session_path() {
    // Paired with the editor-arguments test in lisp/tests/main.lisp.
    for args in [
        vec!["-e", "--help"],
        vec!["--eval", "--version"],
        vec!["-i", "-h"],
        vec!["--interface", "-v"],
        vec!["--log-filename", "--help"],
        vec!["-e"],
        vec!["--eval"],
        vec!["--"],
        vec!["README.md"],
    ] {
        let children = Children::new();
        let output = children.run(&args, 0);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        children.assert_args(&args);
        assert!(children.dir.path().join("display-started").exists());
        assert!(children.dir.path().join("agent-started").exists());
        assert_ne!(
            fs::read_to_string(children.dir.path().join("session")).unwrap(),
            "unset\n"
        );
    }
}

#[test]
fn missing_required_operands_inherit_stdio_without_starting_a_session() {
    // Paired with the malformed-arguments test in lisp/tests/main.lisp.
    for args in [
        vec!["-i"],
        vec!["--interface"],
        vec!["--log-filename"],
        vec!["--help", "--interface"],
        vec!["--version", "--log-filename"],
        vec!["README.md", "-i"],
    ] {
        let mut children = Children::new();
        children.display = children.dir.path().join("missing-display");
        children.agent = children.dir.path().join("missing-agent");
        let output = children.run(&args, 2);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert_eq!(output.stdout, b"Lem test information\n");
        assert_eq!(output.stderr, b"Lem test diagnostic\n");
        children.assert_args(&args);
        children.assert_no_session();
    }
}

#[test]
fn an_editor_crash_fails_the_launcher_even_if_the_display_succeeds() {
    let children = Children::new();
    let output = children.run(&[], 1);
    assert_eq!(output.status.code(), Some(1));
    children.assert_lem_failure(&output);
    children.assert_args(&[]);
}

#[test]
fn an_editor_crash_takes_precedence_over_the_display_failure() {
    let children = Children::new();
    let output = children
        .command(&[], 23)
        .env("TEST_DISPLAY_EXIT", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(23));
    children.assert_lem_failure(&output);
}

#[test]
fn a_display_failure_is_reported_when_lem_succeeds() {
    let children = Children::new();
    let output = children
        .command(&[], 0)
        .env("TEST_DISPLAY_EXIT", "24")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(24));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("the display exited with"), "{stderr}");
    assert!(!stderr.contains("Lem exited with"), "{stderr}");
}

#[test]
fn an_independently_signalled_lem_is_reported_when_the_display_succeeds() {
    let children = Children::new();
    let output = children
        .command(&[], 0)
        .env("TEST_LISP_MODE", "signal")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    children.assert_lem_failure(&output);
}

#[test]
fn a_display_failure_takes_precedence_over_a_signalled_lem() {
    let children = Children::new();
    let output = children
        .command(&[], 0)
        .env("TEST_LISP_MODE", "signal")
        .env("TEST_DISPLAY_EXIT", "24")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(24));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("the display exited with"), "{stderr}");
    assert!(!stderr.contains("Lem exited with"), "{stderr}");
}

#[test]
fn display_driven_cleanup_of_lem_keeps_a_successful_exit() {
    let children = Children::new();
    let output = children
        .command(&[], 0)
        .env("TEST_LISP_MODE", "wait")
        .env("TEST_DISPLAY_MODE", "first-line")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}
