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
use lem_protocol::{framing, rpc::Incoming};

/// The protocol stream coming from Lem.
///
/// Split from [`LemWriter`] so the reader can move onto its own thread
/// while the main loop keeps the writer: the display half has to wait on
/// terminal events and protocol frames at the same time, and `recv`
/// blocks.
pub struct LemReader {
    stdin: BufReader<Stdin>,
}

/// The protocol stream going to Lem.
pub struct LemWriter {
    stdout: BufWriter<Stdout>,
}

/// Claim stdin and stdout as the connection to Lem.
pub fn stdio() -> (LemReader, LemWriter) {
    (
        LemReader {
            stdin: BufReader::new(io::stdin()),
        },
        LemWriter {
            stdout: BufWriter::new(io::stdout()),
        },
    )
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
    /// Read the next message. `Ok(None)` means Lem hung up.
    pub fn recv(&mut self) -> Result<Option<Received>> {
        let Some(body) = framing::read_message(&mut self.stdin)? else {
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
        framing::write_message(&mut self.stdout, body)?;
        Ok(())
    }
}
