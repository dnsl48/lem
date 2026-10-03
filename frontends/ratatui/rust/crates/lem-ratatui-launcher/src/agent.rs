//! The OS agent while a session runs (ADR 0019): told where to listen,
//! and kept alive by a pipe on its stdin that only this process holds.
//! Closing that pipe is how the agent learns the session is over; if
//! this process dies, the kernel closes it just the same.

use std::ffi::OsString;
use std::fs::File;
use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::{Context, Result};

pub struct Agent {
    handle: duct::Handle,
    lifeline: Option<os_pipe::PipeWriter>,
}

impl Agent {
    /// Start `program --socket <socket>`, its stderr to `log`.
    pub fn start(program: &Path, socket: &Path, log: &Path) -> Result<Self> {
        let (stdin, lifeline) = os_pipe::pipe().context("creating a pipe")?;
        let stderr = File::create(log).with_context(|| format!("creating {}", log.display()))?;
        let args = [OsString::from("--socket"), socket.as_os_str().to_owned()];
        let handle = duct::cmd(program, args)
            .stdin_file(stdin)
            .stdout_null()
            .stderr_file(stderr)
            .unchecked()
            .start()
            .with_context(|| format!("starting {}", program.display()))?;
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
        let agent = Agent::start(&program, &dir.path().join("s"), &dir.path().join("log")).unwrap();
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
        let agent = Agent::start(&program, &dir.path().join("s"), &dir.path().join("log")).unwrap();
        assert!(agent.stop(Duration::from_millis(100)).unwrap().is_none());
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-agent");
        assert!(Agent::start(&missing, &dir.path().join("s"), &dir.path().join("log")).is_err());
    }

    #[test]
    fn the_agent_is_told_its_socket() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("args");
        let program = script(dir.path(), &format!("echo \"$@\" > {}", out.display()));
        let socket = dir.path().join("os-agent.sock");
        let agent = Agent::start(&program, &socket, &dir.path().join("log")).unwrap();
        agent.stop(Duration::from_secs(5)).unwrap();
        assert_eq!(
            std::fs::read_to_string(out).unwrap().trim(),
            format!("--socket {}", socket.display())
        );
    }
}
