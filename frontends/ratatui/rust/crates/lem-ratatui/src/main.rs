//! Terminal display half for Lem.
//!
//! This process owns the terminal. It spawns Lem as a child speaking the
//! `lem-server` JSON-RPC protocol over stdio, paints the frames it is sent
//! into a cell buffer, and sends key and mouse events back.
//!
//! The direction matters: Lem must be the child, because with
//! `--mode=stdio` its stdout carries protocol traffic and must not be a
//! TTY. See `../../../docs/adr/0001-reuse-lem-server-jsonrpc.md`.
//!
//! Status: scaffold. Nothing is wired up yet. The build order is:
//!
//! 1. spawn `lem --interface RATATUI` with piped stdio, read
//!    `Content-Length`-framed JSON-RPC
//! 2. `login` with the terminal size and colours
//! 3. maintain one `Buffer` per view; composite in z-order on
//!    `update-display` (tiles, header, floating)
//! 4. translate crossterm events into Lem key encodings and `notify`
//!    them back as `input`
//!
//! Step 3 is the one with no equivalent in the browser display half: a
//! terminal has a single grid and composites nothing on its own. See
//! `../../../docs/protocol-notes.md` section 7.

fn main() {
    eprintln!(
        "lem-ratatui: scaffold only — see docs/adr/ for the design, \
         nothing is implemented yet"
    );
}
