//! Owning the Lem child process and its stdio.
//!
//! Lem is the child rather than the parent because with `--mode=stdio`
//! its stdout carries protocol traffic and must not be a TTY
//! (`../../../docs/adr/0004-two-processes-not-embedded-lisp.md`). Its
//! stderr goes to a log file for the same reason: inherited, a backtrace
//! would land on the display and shred it.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result};
use lem_protocol::{framing, rpc::Incoming};

/// A running Lem process, addressed as a stream of protocol messages.
pub struct Lem {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Lem {
    /// Spawn `program`, piping its stdio and redirecting its stderr to `log`.
    pub fn spawn(program: &Path, log: &Path) -> Result<Self> {
        let stderr =
            File::create(log).with_context(|| format!("creating log file {}", log.display()))?;
        let mut child = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .with_context(|| format!("spawning {}", program.display()))?;
        let stdin = child.stdin.take().context("child stdin was not piped")?;
        let stdout = child.stdout.take().context("child stdout was not piped")?;
        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    /// Send one already-serialised message.
    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        framing::write_message(&mut self.stdin, body)?;
        Ok(())
    }

    /// Read the next message. `Ok(None)` means Lem exited.
    pub fn recv(&mut self) -> Result<Option<Incoming>> {
        let Some(body) = framing::read_message(&mut self.stdout)? else {
            return Ok(None);
        };
        let incoming = serde_json::from_slice(&body).with_context(|| {
            format!(
                "decoding a {} byte message: {}",
                body.len(),
                String::from_utf8_lossy(&body[..body.len().min(200)])
            )
        })?;
        Ok(Some(incoming))
    }
}

impl Drop for Lem {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
