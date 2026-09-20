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
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lem_protocol::{framing, rpc::Incoming};

/// A running Lem process. Killed and reaped when dropped.
///
/// Reading and writing are split off so the reader can move onto its own
/// thread while the main loop keeps the writer: the display half has to
/// wait on terminal events and protocol frames at the same time, and
/// `recv` blocks.
pub struct Lem {
    child: Child,
}

/// The protocol stream coming from Lem.
pub struct LemReader {
    stdout: BufReader<ChildStdout>,
}

/// The protocol stream going to Lem.
pub struct LemWriter {
    stdin: BufWriter<ChildStdin>,
}

impl Lem {
    /// Spawn `program`, piping its stdio and redirecting its stderr to `log`.
    pub fn spawn(program: &Path, log: &Path) -> Result<(Self, LemReader, LemWriter)> {
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
        Ok((
            Self { child },
            LemReader {
                stdout: BufReader::new(stdout),
            },
            LemWriter {
                stdin: BufWriter::new(stdin),
            },
        ))
    }
}

/// One message, with what it cost to read.
///
/// The size and timing travel with the message so the frame loop can
/// account for them without the reader knowing what a frame is (ADR 0003).
pub struct Received {
    pub incoming: Incoming,
    /// Framed body length in bytes, excluding the header.
    pub bytes: usize,
    /// Time spent decoding the JSON-RPC envelope.
    pub decode: Duration,
}

impl LemReader {
    /// Read the next message. `Ok(None)` means Lem exited.
    pub fn recv(&mut self) -> Result<Option<Received>> {
        let Some(body) = framing::read_message(&mut self.stdout)? else {
            return Ok(None);
        };
        let started = Instant::now();
        let incoming: Incoming = serde_json::from_slice(&body).with_context(|| {
            format!(
                "decoding a {} byte message: {}",
                body.len(),
                String::from_utf8_lossy(&body[..body.len().min(200)])
            )
        })?;
        Ok(Some(Received {
            incoming,
            bytes: body.len(),
            decode: started.elapsed(),
        }))
    }
}

impl LemWriter {
    /// Send one already-serialised message.
    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        framing::write_message(&mut self.stdin, body)?;
        Ok(())
    }
}

impl Drop for Lem {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
