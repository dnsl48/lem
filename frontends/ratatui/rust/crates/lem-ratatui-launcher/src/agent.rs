//! The OS agent while a session runs (ADR 0019): told where to listen,
//! and kept alive by a pipe on its stdin that only this process holds.
//! Closing that pipe is how the agent learns the session is over; if
//! this process dies, the kernel closes it just the same.
//!
//! It is started in its own process group (`before_spawn`, Unix only),
//! so a terminal hangup delivered to this process's foreground process
//! group — as happens when the launcher, a session leader, is killed —
//! is not also delivered to the agent. Without that, `SIGHUP`'s default,
//! immediate disposition could beat the agent's own EOF-triggered
//! cleanup to removing its socket. A `SIGKILL` of the launcher itself is
//! not survivable by any of this process's code either way: nothing runs
//! to remove the (by then empty) session directory in that one case, and
//! that is accepted.

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::{Context, Result};

pub struct Agent {
    handle: duct::Handle,
    lifeline: Option<os_pipe::PipeWriter>,
}

impl Agent {
    /// Start `program --socket <socket>`, with `session` as its
    /// `LEM_SESSION_ID`. Its stderr goes to `log` when there is one and it
    /// can be created; discarded otherwise (a warning, not a failure —
    /// the agent still starts), same as when the launcher has nowhere to
    /// put a log file at all.
    pub fn start(
        program: &Path,
        socket: &Path,
        log: Option<&Path>,
        session: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Self> {
        let (stdin, lifeline) = os_pipe::pipe().context("creating a pipe")?;
        let args = [OsString::from("--socket"), socket.as_os_str().to_owned()];
        let cmd = duct::cmd(program, args)
            .env("LEM_SESSION_ID", session)
            .before_spawn(detach_from_terminal)
            .unchecked();
        // `stdin_file` takes `stdin` by value, so (as in `run_both`) the
        // whole chain through `start` must be one expression per branch,
        // not bound partway: a bound `Expression` lives to the end of
        // this function regardless of being shadowed, keeping its copy
        // of the pipe open longer than intended.
        let start = match log.and_then(|log| crate::open_log_or_warn(log, "the OS agent", warnings))
        {
            Some(stderr) => cmd
                .stdin_file(stdin)
                .stdout_null()
                .stderr_file(stderr)
                .start(),
            None => cmd.stdin_file(stdin).stdout_null().stderr_null().start(),
        };
        let handle = start.with_context(|| format!("starting {}", program.display()))?;
        Ok(Self {
            handle,
            lifeline: Some(lifeline),
        })
    }

    /// Close the lifeline, give the agent `grace` to exit, then kill it.
    /// `None` when it had to be killed.
    pub fn stop(mut self, grace: Duration) -> Result<Option<ExitStatus>> {
        drop(self.lifeline.take());
        match self.handle.wait_timeout(grace)? {
            Some(output) => Ok(Some(output.status)),
            None => {
                self.handle.kill()?;
                let _ = self.handle.wait();
                Ok(None)
            }
        }
    }
}

/// Put the child in its own process group (`setpgid(0, 0)` before it
/// execs), so it does not share the launcher's — and so is not sent a
/// `SIGHUP` when the launcher, a terminal session leader, dies. No-op on
/// non-Unix, which has no such thing to detach from.
#[cfg(unix)]
fn detach_from_terminal(cmd: &mut std::process::Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    Ok(())
}

#[cfg(not(unix))]
fn detach_from_terminal(_cmd: &mut std::process::Command) -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    /// A script standing in for the agent: it ignores its arguments.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("agent.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn closing_the_lifeline_ends_an_agent_that_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "cat >/dev/null");
        let agent = Agent::start(
            &program,
            &dir.path().join("s"),
            Some(&dir.path().join("log")),
            "session",
            &mut Vec::new(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            agent.handle.try_wait().unwrap().is_none(),
            "running while the pipe is open"
        );
        let status = agent.stop(Duration::from_secs(5)).unwrap();
        assert!(status.expect("exited by itself").success());
    }

    #[test]
    fn an_agent_that_ignores_it_is_killed() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "exec sleep 30");
        let agent = Agent::start(
            &program,
            &dir.path().join("s"),
            Some(&dir.path().join("log")),
            "session",
            &mut Vec::new(),
        )
        .unwrap();
        assert!(agent.stop(Duration::from_millis(100)).unwrap().is_none());
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-agent");
        assert!(
            Agent::start(
                &missing,
                &dir.path().join("s"),
                Some(&dir.path().join("log")),
                "session",
                &mut Vec::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn the_agent_is_told_its_socket_and_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let program = script(
            dir.path(),
            &format!("echo \"$@ $LEM_SESSION_ID\" > {}", out.display()),
        );
        let socket = dir.path().join("os-agent.sock");
        let agent = Agent::start(
            &program,
            &socket,
            Some(&dir.path().join("log")),
            "the-session-id",
            &mut Vec::new(),
        )
        .unwrap();
        agent.stop(Duration::from_secs(5)).unwrap();
        assert_eq!(
            std::fs::read_to_string(out).unwrap().trim(),
            format!("--socket {} the-session-id", socket.display())
        );
    }

    #[test]
    fn without_a_log_path_its_stderr_is_discarded_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "cat >/dev/null");
        let agent = Agent::start(
            &program,
            &dir.path().join("s"),
            None,
            "session",
            &mut Vec::new(),
        )
        .unwrap();
        let status = agent.stop(Duration::from_secs(5)).unwrap();
        assert!(status.expect("exited by itself").success());
        assert!(!dir.path().join("log").exists(), "no log file was created");
    }

    #[test]
    fn a_log_that_cannot_be_created_is_a_warning_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "cat >/dev/null");
        // `not_a_dir` is a regular file, so a log path under it can never
        // be created (ENOTDIR) — root-proof, unlike a 0500 directory.
        let not_a_dir = dir.path().join("not-a-dir");
        std::fs::write(&not_a_dir, "").unwrap();
        let log = not_a_dir.join("log");
        let mut warnings = Vec::new();
        let agent = Agent::start(
            &program,
            &dir.path().join("s"),
            Some(&log),
            "session",
            &mut warnings,
        )
        .unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("OS agent"), "{warnings:?}");
        let status = agent.stop(Duration::from_secs(5)).unwrap();
        assert!(status.expect("exited by itself: not fatal").success());
    }
}
