//! The protocol stream to and from Lem, over this process's stdio.
//!
//! Whoever starts this binary connects its stdin to Lem's stdout and its
//! stdout to Lem's stdin — normally `lem-ratatui-launcher`. How Lem is
//! found and started is none of this crate's business, which is also why
//! the terminal is reached through `/dev/tty` rather than stdio
//! (`term.rs`).
//!
//! Stdout is therefore protocol, and nothing else may write to it: any
//! diagnostic goes to stderr.

use std::io::{self, BufReader, BufWriter, Stdin, Stdout};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lem_protocol::v1::{self, to_editor};
use prost::Message;

/// The protocol stream coming from Lem.
///
/// Split from [`LemWriter`] so the reader can move onto its own thread
/// while the main loop keeps the writer: the display half has to wait on
/// terminal events and protocol frames at the same time, and `recv`
/// blocks.
pub struct LemReader {
    stdin: BufReader<Stdin>,
}

/// The protocol stream going to Lem: numbers and times each message
/// (ADR 0013).
pub struct LemWriter {
    stdout: BufWriter<Stdout>,
    seq: u64,
    started: Instant,
}

/// Claim stdin and stdout as the connection to Lem. `started` is the
/// display's clock origin: every message's `time_us` counts from it.
pub fn stdio(started: Instant) -> (LemReader, LemWriter) {
    (
        LemReader {
            stdin: BufReader::new(io::stdin()),
        },
        LemWriter {
            stdout: BufWriter::new(io::stdout()),
            seq: 0,
            started,
        },
    )
}

/// One message, with what it cost to read.
///
/// The size and timing travel with the message so the frame loop can
/// account for them without the reader knowing what a frame is (ADR 0003).
pub struct Received {
    pub message: v1::ToDisplay,
    /// Message length in bytes, excluding its length prefix.
    pub bytes: usize,
    /// Time spent decoding it.
    pub decode: Duration,
}

impl LemReader {
    /// Read the next message. `Ok(None)` means Lem hung up.
    pub fn recv(&mut self) -> Result<Option<Received>> {
        let Some(body) = v1::read_delimited(&mut self.stdin)? else {
            return Ok(None);
        };
        let started = Instant::now();
        let message = v1::ToDisplay::decode(body.as_slice())
            .with_context(|| format!("decoding a {} byte message", body.len()))?;
        Ok(Some(Received {
            message,
            bytes: body.len(),
            decode: started.elapsed(),
        }))
    }
}

impl LemWriter {
    /// Send one message, numbered and timed. Returns its `seq`.
    pub fn send(&mut self, message: to_editor::Message) -> Result<u64> {
        self.seq += 1;
        let envelope = v1::ToEditor {
            seq: self.seq,
            time_us: u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX),
            message: Some(message),
        };
        v1::write_delimited(&mut self.stdout, &envelope)?;
        Ok(self.seq)
    }
}
