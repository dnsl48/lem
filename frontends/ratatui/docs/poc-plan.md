# Ratatui frontend PoC — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open a file in Lem inside a modern terminal, move the cursor,
type, see syntax colours, and resize the window — driven by stock
`lem-server` protocol frames painted by a Rust process.

**Architecture:** Two OS processes. The Rust binary owns the TTY and
spawns Lem as a child speaking JSON-RPC over stdio. Lem resolves all
layout and sends absolutely-positioned paint commands in cell
coordinates; Rust maintains one `ratatui_core::buffer::Buffer` per view,
composites them in z-order, and diffs to the terminal through
`ratatui-crossterm`. Input flows back as `input` notifications.

**Tech Stack:** SBCL + `lem-server` + `jsonrpc` (Lisp half); Rust 2024
with `ratatui-core`, `ratatui-crossterm`, `crossterm`, `serde`,
`serde_json`, `anyhow` (display half).

**Spec:** [`adr/`](adr/) records 0001-0005 and
[`protocol-notes.md`](protocol-notes.md). Read both before starting.

## Global Constraints

- **Scope is the MVP sentence above.** Single window. No popups, menus,
  images, icons, mouse, clipboard, or `html-buffer`. Those are
  `lem-if:*` methods that stay inherited-and-unused, or protocol
  messages that decode to `Instruction::Other`.
- **stdout is the wire.** Under `--mode=stdio` any stray Lisp output
  corrupts the frame stream. `*standard-output*` must be rebound before
  editor code runs (ADR 0005).
- **The child's stderr goes to a log file**, never inherited from the
  Rust parent, or backtraces land on the TTY.
- **Wire casing is mixed and must not be normalised blindly.**
  `make-view` sends `use_modeline` and `border_shape` in snake_case
  alongside `pixelX`/`pixelWidth` in camelCase. A blanket
  `#[serde(rename_all = "camelCase")]` on the view struct silently drops
  two fields. Per-field `rename` where they disagree.
- **Framing is LSP-style:** `Content-Length: N\r\n\r\n<json>`, where N
  counts **bytes**, not characters.
- **Key syms are Lem's vocabulary, not crossterm's.** Single-character
  strings for insertion keys; named syms otherwise: `Return`, `Tab`,
  `Escape`, `Space`, `Backspace`, `Delete`, `Up`, `Down`, `Left`,
  `Right`, `Home`, `End`, `PageUp`, `PageDown`, `F1`-`F12`. Authoritative
  list: `frontends/ncurses/key.lisp`.
- **`shift` must be false for single-character syms.** `convert-keyevent`
  (`frontends/server/main.lisp:767`) drops it via
  `lem:insertion-key-sym-p`, which is simply `(= 1 (length sym))`.
  Sending `shift: true` with sym `"A"` produces a key Lem cannot match.
- **Isolation: create and modify nothing outside `frontends/ratatui/`.**
  The PoC needs zero edits to existing files. ASDF finds
  `lem-ratatui.asd` by name through the same tree scan that resolves
  `"lem-server"` in `lem.asd:322` — which uses no pathnames — and
  `get-default-implementation` finds the `ratatui` class at runtime by
  scanning direct subclasses. Nothing needs registering. If a task seems
  to require touching `src/`, `scripts/`, `Makefile`, `lem.asd` or
  another frontend, stop and raise it rather than reaching outside.
- **Paths in `Files:` blocks are relative to `frontends/ratatui/`.** Shell
  commands state their own working directory; `qlot` and `sbcl` commands
  run from the repo root, `cargo` commands from `frontends/ratatui/rust/`.
- **Rust gates:** `cargo test`, `cargo clippy --all-targets` (no
  warnings) and `cargo fmt --all --check` pass before every commit.

---

## Status

### Done

- **Design.** ADR 0001 (reuse the JSON-RPC protocol), 0002
  (`ratatui-core`, not full Ratatui), 0003 (keep JSON), 0004 (two
  processes), 0005 (stdio default). Research in `protocol-notes.md`.
- **Lisp scaffold.** `lem-ratatui.asd`; `lisp/implementation.lisp`
  defining the `ratatui` class with terminal capability flags
  (`:no-force-needed nil` for occlusion repair,
  `:support-pixel-positioning nil`, `:html-support nil`, margins 1);
  `lisp/main.lisp` with a `main` that serves stdio.
- **Rust scaffold.** Workspace with `lem-protocol` and `lem-ratatui`.
  `lem-protocol` defines `ViewInfo`, `Attribute`, `Underline`, `Put`,
  `RawInstruction`, `Instruction`, `Bulk`, with the raw-then-parse split
  so unknown methods cannot fail a frame. 3 tests green, clippy clean.
- Dependencies resolved: `ratatui-core 0.1.2`, `ratatui-crossterm 0.1.2`,
  `crossterm 0.29`, `serde 1.0.229`, `serde_json 1.0.151`, `anyhow 1.0`.

### Left

Tasks 1-10 below. Task 1 is the only one that unblocks the rest; Tasks 4
and 5 are pure logic and can be done in any order relative to 2 and 3.

---

## File structure

| File | Responsibility |
|---|---|
| `lisp/main.lisp` *(modify)* | stdout muffling; stdio entry point |
| `lisp/transport.lisp` *(create, only if Task 1 Step 2 requires it)* | stdio transport override giving the wire an explicit stream |
| `build.lisp` *(create)* | `save-lisp-and-die` the Lisp half |
| `rust/crates/lem-protocol/src/lib.rs` *(modify)* | add `View`, `Clear`, `MoveCursor`, `Size`, `LoginParams`, `KeyInput` |
| `rust/crates/lem-protocol/src/framing.rs` *(create)* | `Content-Length` read/write over any `BufRead`/`Write` |
| `rust/crates/lem-protocol/src/rpc.rs` *(create)* | JSON-RPC envelope: `Notification`, `Request`, `Response` |
| `rust/crates/lem-ratatui/src/child.rs` *(create)* | spawn Lem, own its stdio, stderr to a log |
| `rust/crates/lem-ratatui/src/term.rs` *(create)* | raw mode, alternate screen, panic-safe restore |
| `rust/crates/lem-ratatui/src/views.rs` *(create)* | view registry, per-view `Buffer`, z-order compositing |
| `rust/crates/lem-ratatui/src/paint.rs` *(create)* | apply `put`/`clear-*` to a view buffer; `Attribute` → `Style` |
| `rust/crates/lem-ratatui/src/input.rs` *(create)* | crossterm `KeyEvent` → Lem key sym payload |
| `rust/crates/lem-ratatui/src/metrics.rs` *(create)* | per-frame byte and decode-time counters |
| `rust/crates/lem-ratatui/src/main.rs` *(modify)* | wire the above into an event loop |

---

### Task 1: Lisp half builds and emits frames

Nothing downstream can be tested against real data until this produces
bytes. The deliverable is a captured frame on disk that later tasks use
as a test fixture.

**Files:**
- Modify: `lisp/main.lisp`
- Create: `build.lisp`
- Create: `lisp/transport.lisp` *(only if Step 2 requires it)*
- Modify: `lem-ratatui.asd` *(only if `transport.lisp` is added)*
- Create: `rust/crates/lem-protocol/tests/fixtures/frame.jsonl`

**Interfaces:**
- Produces: a `lem-ratatui` executable that speaks `Content-Length`-framed
  JSON-RPC on stdin/stdout; a captured first-frame fixture.

- [ ] **Step 1: Install dependencies**

```bash
qlot install
```

- [ ] **Step 2: Establish which stream the transport writes to**

`*standard-output*` is both the protocol wire and where stray editor
output goes, and those cannot be the same stream. The muffling strategy
depends on how `jsonrpc/transport/stdio` acquires its stream, which is
only readable after `qlot install`:

```bash
sed -n '1,80p' .qlot/dists/quicklisp/software/jsonrpc*/transport/stdio.lisp \
  2>/dev/null || find .qlot -path '*jsonrpc*' -name 'stdio.lisp' -exec sed -n '1,80p' {} +
```

Look for where the connection's socket is set. Two outcomes:

- **It captures `*standard-output*` when the transport starts.** Then
  muffling must not wrap `server-listen`. Bind the null stream only
  around the editor, and leave the transport's own dynamic extent alone.
- **It uses `sb-sys:*stdout*`, an explicit fd, or a stored slot.** Then
  `*standard-output*` is free, and Step 3's rebinding is safe as written.

Record which one in a comment above `main`. Step 6's capture is the test
that proves it: an empty capture means the protocol went to the null
stream.

If that happens, **do not edit
`frontends/server/jsonrpc-stdio-patch.lisp`.** Override the same method
from our own folder instead — it loads after `lem-server`, because we
depend on it, so ours wins. Create
`frontends/ratatui/lisp/transport.lisp`:

```lisp
(defpackage :lem-ratatui/transport
  (:use :cl)
  (:export :*protocol-output*))
(in-package :lem-ratatui/transport)

(defvar *protocol-output* nil
  "The real stdout, captured before `*standard-output*' is muffled.

Under --mode=stdio the protocol and any stray editor output would
otherwise share one stream, and they cannot: muffling `*standard-output*'
to protect the wire also silences the wire. `main' captures the true
stdout here before rebinding, and the transport override below writes to
it rather than to whatever `*standard-output*' happens to be.")

;;; Overrides `lem-server''s own override of this method
;;; (frontends/server/jsonrpc-stdio-patch.lisp). Copy that method body
;;; verbatim and change only the stream it writes to; keeping the bodies
;;; otherwise identical is what makes a later upstream divergence
;;; visible on inspection.
(defmethod jsonrpc/transport/stdio::send-message-using-transport
    ((transport jsonrpc/transport/stdio::stdio-transport) connection message)
  (let ((json (babel:string-to-octets
               (with-output-to-string (s)
                 (yason:encode message s))))
        (stream (or *protocol-output*
                    (jsonrpc/transport/stdio::connection-socket connection))))
    (format stream "Content-Length: ~A~C~C~:*~:*~C~C"
            (length json)
            #\Return
            #\Newline)
    (write-sequence json stream)
    (force-output stream)))
```

Then add it to `lem-ratatui.asd` **before** `main`, and set
`lem-ratatui/transport:*protocol-output*` from `main` in Step 3:

```lisp
  :components ((:file "implementation")
               (:file "transport")
               (:file "main")))
```

Two monkey-patches stacked on one generic function is acceptable for a
PoC and bad permanently. The real fix is the upstream change already
recorded in the README: teach `run-stdio-server` an `:interface`
argument and an explicit output stream, which deletes both this override
and the `lem-server::` internals coupling.

- [ ] **Step 3: Rebind `*standard-output*` so it cannot corrupt the wire**

Apply the branch chosen in Step 2. Where the transport does *not* capture
`*standard-output*`, the body of `main` in `lisp/main.lisp` becomes:

```lisp
(defun main (&optional (args (uiop:command-line-arguments)))
  "Run Lem with the Ratatui frontend, serving JSON-RPC over stdio.

Intended to be spawned as a child process by the Rust display binary,
which owns the terminal: this process's stdout carries protocol traffic,
not output for a human, so it must never be attached to a TTY.

`lem-server' internals are used here because the exported
`lem-server:run-stdio-server' hardcodes --interface JSONRPC and so cannot
select another implementation. Teaching it an :interface argument is the
right fix and should be proposed upstream; until then this duplicates its
three lines."
  (let* ((protocol-stream *standard-output*)
         (null-stream (make-broadcast-stream))
         (*standard-output* null-stream)
         (*trace-output* null-stream)
         (lem-server::*server-runner*
           (make-instance 'lem-server::stdio-server-runner)))
    ;; Only consumed by the Step 2 override; harmless when unused.
    (setf lem-ratatui/transport:*protocol-output* protocol-stream)
    (lem-server::init)
    (apply #'lem:lem (append args (list "--interface" "RATATUI")))))
```

- [ ] **Step 4: Write the build script**

Create `frontends/ratatui/build.lisp` — in our folder, not `scripts/`,
so the PoC stays self-contained:

```lisp
(ql:quickload :lem-ratatui)

(lem:init-at-build-time)

(sb-ext:save-lisp-and-die "lem-ratatui-lisp"
                          :toplevel #'lem-ratatui:main
                          :executable t)
```

- [ ] **Step 5: Build and verify the implementation is discoverable**

```bash
sbcl --load .qlot/setup.lisp --load frontends/ratatui/build.lisp
```

Expected: a `lem-ratatui-lisp` binary in the repo root. If the build
errors with "Implementation does not exist", the `ratatui` class is not a
*direct* subclass of `lem-core:implementation` — see the docstring in
`lisp/implementation.lisp`.

- [ ] **Step 6: Capture a real frame**

The child expects a `login` request before it emits anything. Send one
and record what comes back:

```bash
printf 'Content-Length: 102\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"login","params":{"size":{"width":80,"height":24},"foreground":null}}' \
  | ./lem-ratatui-lisp 2>/tmp/lem-ratatui.log \
  | head -c 200000 > /tmp/first-frame.raw
wc -c /tmp/first-frame.raw
```

Expected: a non-empty capture containing `"method":"bulk"`. If it is
empty, read `/tmp/lem-ratatui.log`.

- [ ] **Step 7: Extract the first bulk frame as a fixture**

```bash
mkdir -p frontends/ratatui/rust/crates/lem-protocol/tests/fixtures
python3 - <<'PY'
import re, json
raw = open('/tmp/first-frame.raw','rb').read()
out = []
while True:
    m = re.match(rb'Content-Length: (\d+)\r\n\r\n', raw)
    if not m: break
    n = int(m.group(1)); body = raw[m.end():m.end()+n]
    raw = raw[m.end()+n:]
    out.append(json.loads(body))
path = 'frontends/ratatui/rust/crates/lem-protocol/tests/fixtures/frame.jsonl'
with open(path, 'w') as f:
    for msg in out:
        f.write(json.dumps(msg) + '\n')
print(f"{len(out)} messages ->", path)
PY
```

Expected: at least one message, with `bulk` among the methods.

- [ ] **Step 8: Commit**

```bash
git add frontends/ratatui/
git commit -m "feat(ratatui): build the Lisp half and capture a protocol fixture"
```

---

### Task 2: `Content-Length` framing and the JSON-RPC envelope

**Files:**
- Create: `rust/crates/lem-protocol/src/framing.rs`
- Create: `rust/crates/lem-protocol/src/rpc.rs`
- Modify: `rust/crates/lem-protocol/src/lib.rs`

**Interfaces:**
- Produces: `framing::read_message(&mut impl BufRead) -> io::Result<Option<Vec<u8>>>`,
  `framing::write_message(&mut impl Write, &[u8]) -> io::Result<()>`,
  `rpc::Incoming` (`Notification { method, params }` / `Response { id, result }`),
  `rpc::notification(method, params) -> Vec<u8>`.

- [ ] **Step 1: Write the failing tests**

Create `rust/crates/lem-protocol/src/framing.rs`:

```rust
//! LSP-style `Content-Length` framing, as spoken by
//! `frontends/server/jsonrpc-stdio-patch.lisp`.

use std::io::{self, BufRead, Write};

/// Read one framed message body. `Ok(None)` means clean end of stream.
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    todo!()
}

/// Write one framed message body.
pub fn write_message(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_framed_message() {
        let raw = b"Content-Length: 7\r\n\r\n{\"a\":1}";
        let mut cursor = &raw[..];
        let body = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(body, b"{\"a\":1}");
    }

    #[test]
    fn reads_two_messages_back_to_back() {
        let raw = b"Content-Length: 2\r\n\r\n{}Content-Length: 2\r\n\r\n[]";
        let mut cursor = &raw[..];
        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), b"{}");
        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), b"[]");
        assert!(read_message(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn counts_bytes_not_characters() {
        // The body is 10 bytes but 9 characters: "é" is two bytes.
        // A reader that counts characters truncates it.
        let raw = "Content-Length: 10\r\n\r\n{\"a\":\"é\"}".as_bytes();
        let mut cursor = raw;
        let body = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(String::from_utf8(body).unwrap(), "{\"a\":\"é\"}");
    }

    #[test]
    fn round_trips() {
        let mut buf = Vec::new();
        write_message(&mut buf, b"{\"hello\":true}").unwrap();
        let mut cursor = &buf[..];
        assert_eq!(
            read_message(&mut cursor).unwrap().unwrap(),
            b"{\"hello\":true}"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p lem-protocol framing
```

Expected: FAIL, `not yet implemented`.

- [ ] **Step 3: Implement framing**

```rust
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let Some(length) = length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "framed message without Content-Length",
        ));
    };
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_message(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(body)?;
    writer.flush()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p lem-protocol framing
```

Expected: 4 passed.

- [ ] **Step 5: Write the envelope test**

Create `rust/crates/lem-protocol/src/rpc.rs`:

```rust
//! The JSON-RPC 2.0 envelope. Lem sends notifications; it answers
//! `login` with a response.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Notification {
        method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    Response {
        id: u64,
        #[serde(default)]
        result: serde_json::Value,
    },
}

#[derive(Debug, Serialize)]
struct OutgoingNotification<'a, T> {
    jsonrpc: &'static str,
    method: &'a str,
    params: T,
}

#[derive(Debug, Serialize)]
struct OutgoingRequest<'a, T> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: T,
}

/// Serialise a notification to send to Lem.
pub fn notification<T: Serialize>(method: &str, params: T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&OutgoingNotification { jsonrpc: "2.0", method, params })
}

/// Serialise a request to send to Lem.
pub fn request<T: Serialize>(id: u64, method: &str, params: T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&OutgoingRequest { jsonrpc: "2.0", id, method, params })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_bulk_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"bulk","params":[]}"#;
        let incoming: Incoming = serde_json::from_str(json).unwrap();
        assert!(matches!(
            incoming,
            Incoming::Notification { ref method, .. } if method == "bulk"
        ));
    }

    #[test]
    fn decodes_a_login_response() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"size":{"width":80,"height":24}}}"#;
        let incoming: Incoming = serde_json::from_str(json).unwrap();
        let Incoming::Response { id, result } = incoming else {
            panic!("expected a response");
        };
        assert_eq!(id, 1);
        assert_eq!(result["size"]["width"], 80);
    }

    #[test]
    fn notifications_carry_the_jsonrpc_version() {
        let bytes = notification("input", serde_json::json!({"kind": "abort"})).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["method"], "input");
    }
}
```

- [ ] **Step 6: Declare the modules and run everything**

In `lib.rs`, above the existing type definitions:

```rust
pub mod framing;
pub mod rpc;
```

```bash
cargo test -p lem-protocol && cargo clippy --all-targets && cargo fmt --all
```

Expected: 10 passed, no clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add rust/crates/lem-protocol/src
git commit -m "feat(ratatui): Content-Length framing and JSON-RPC envelope"
```

---

### Task 3: Spawn the child and own the terminal

**Files:**
- Create: `rust/crates/lem-ratatui/src/child.rs`
- Create: `rust/crates/lem-ratatui/src/term.rs`
- Modify: `rust/crates/lem-ratatui/src/main.rs`

**Interfaces:**
- Produces: `child::Lem::spawn(path: &Path, log: &Path) -> anyhow::Result<Lem>`
  with `Lem::send(&mut self, body: &[u8])` and
  `Lem::recv(&mut self) -> anyhow::Result<Option<Incoming>>`;
  `term::Guard::new() -> anyhow::Result<Guard>` restoring on `Drop`.

- [ ] **Step 1: Write the child transport**

Create `rust/crates/lem-ratatui/src/child.rs`:

```rust
//! Owning the Lem child process and its stdio.
//!
//! Lem is the child because with `--mode=stdio` its stdout carries
//! protocol traffic and must not be a TTY (ADR 0004). Its stderr goes to
//! a log file for the same reason: inherited, a backtrace would land on
//! the display.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result};
use lem_protocol::{framing, rpc::Incoming};

pub struct Lem {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Lem {
    pub fn spawn(program: &Path, log: &Path) -> Result<Self> {
        let stderr = File::create(log)
            .with_context(|| format!("creating log file {}", log.display()))?;
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

    pub fn send(&mut self, body: &[u8]) -> Result<()> {
        framing::write_message(&mut self.stdin, body)?;
        Ok(())
    }

    /// Read the next message. `Ok(None)` means Lem exited.
    pub fn recv(&mut self) -> Result<Option<Incoming>> {
        let Some(body) = framing::read_message(&mut self.stdout)? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_slice(&body)?))
    }
}

impl Drop for Lem {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```

- [ ] **Step 2: Write the terminal guard**

Create `rust/crates/lem-ratatui/src/term.rs`:

```rust
//! Terminal lifecycle.
//!
//! The reason this is a guard rather than a pair of functions: restoring
//! the terminal when Lem dies is the single strongest argument for the
//! two-process split (ADR 0004), and it only holds if restore runs on
//! every exit path, panics included.

use std::io::{self, Stdout, Write};

use anyhow::Result;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};

pub struct Guard {
    out: Stdout,
}

impl Guard {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, cursor::Hide)?;

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore();
            previous(info);
        }));

        Ok(Self { out })
    }

    pub fn out(&mut self) -> &mut Stdout {
        &mut self.out
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.out.flush();
        let _ = restore();
    }
}

fn restore() -> Result<()> {
    execute!(io::stdout(), cursor::Show, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
```

- [ ] **Step 3: Wire a smoke test into `main.rs`**

Replace the body of `main` in `rust/crates/lem-ratatui/src/main.rs`:

```rust
mod child;
mod term;

use std::path::PathBuf;

use anyhow::Result;

fn main() -> Result<()> {
    let program = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./lem-ratatui-lisp"));
    let log = PathBuf::from("/tmp/lem-ratatui.log");

    let mut lem = child::Lem::spawn(&program, &log)?;
    let body = lem_protocol::rpc::request(
        1,
        "login",
        serde_json::json!({"size": {"width": 80, "height": 24}}),
    )?;
    lem.send(&body)?;

    let mut frames = 0usize;
    while let Some(_incoming) = lem.recv()? {
        frames += 1;
        if frames >= 5 {
            break;
        }
    }
    eprintln!("received {frames} messages");
    Ok(())
}
```

- [ ] **Step 4: Verify against the real Lisp half**

```bash
cargo run -p lem-ratatui -- ../../../../lem-ratatui-lisp
```

Expected: `received 5 messages`, and `/tmp/lem-ratatui.log` contains no
protocol JSON (if it does, stdout muffling in Task 1 is incomplete).

- [ ] **Step 5: Commit**

```bash
cargo clippy --all-targets && cargo fmt --all
git add rust/crates/lem-ratatui/src
git commit -m "feat(ratatui): spawn Lem as a child and guard the terminal"
```

---

### Task 4: View registry and z-order compositing

Pure logic, no TTY, no child process. This is the piece with no
equivalent in the browser display half (`protocol-notes.md` section 7).

**Files:**
- Modify: `rust/crates/lem-protocol/src/lib.rs` (add `View`)
- Create: `rust/crates/lem-ratatui/src/views.rs`

**Interfaces:**
- Consumes: `lem_protocol::ViewInfo`.
- Produces: `lem_protocol::View`; `views::Registry` with
  `insert(View)`, `remove(id: u64)`, `get_mut(id: u64) -> Option<&mut ViewBuffer>`,
  `composite(&self, screen: &mut Buffer)`.
- `ViewBuffer` exposes `pub buffer: Buffer` and `pub view: View`.

- [ ] **Step 1: Add the `View` type**

Append to `rust/crates/lem-protocol/src/lib.rs`:

```rust
/// A window, positioned in character cells by Lem.
///
/// Casing on the wire is mixed: `pixelX` is camelCase while
/// `use_modeline` and `border_shape` are snake_case. Renaming per field
/// rather than with a blanket `rename_all`, which would silently drop
/// the two snake_case fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub id: u64,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    #[serde(default)]
    pub use_modeline: bool,
    pub kind: ViewKind,
}

/// Which layer a view composites into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    Tile,
    Header,
    Floating,
}
```

- [ ] **Step 2: Write the failing compositing tests**

Create `rust/crates/lem-ratatui/src/views.rs`:

```rust
//! Per-view cell buffers and the compositing step.
//!
//! The browser display half gets this free: each view is its own canvas
//! and the browser composites them by z-index. A terminal has one grid
//! and composites nothing, so views are painted into separate buffers and
//! blitted here in layer order.

use lem_protocol::{View, ViewKind};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

pub struct ViewBuffer {
    pub view: View,
    pub buffer: Buffer,
}

#[derive(Default)]
pub struct Registry {
    views: Vec<ViewBuffer>,
}

impl Registry {
    pub fn insert(&mut self, view: View) {
        todo!()
    }

    pub fn remove(&mut self, id: u64) {
        todo!()
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut ViewBuffer> {
        todo!()
    }

    /// Blit every view into `screen`, tiles first, then headers, then
    /// floating windows on top.
    pub fn composite(&self, screen: &mut Buffer) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: u64, x: u16, y: u16, w: u16, h: u16, kind: ViewKind) -> View {
        View { id, x, y, width: w, height: h, use_modeline: false, kind }
    }

    fn fill(registry: &mut Registry, id: u64, ch: char) {
        let vb = registry.get_mut(id).unwrap();
        let area = vb.buffer.area;
        for y in 0..area.height {
            for x in 0..area.width {
                vb.buffer[(x, y)].set_char(ch);
            }
        }
    }

    #[test]
    fn floating_views_paint_over_tiles() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 10, 4, ViewKind::Tile));
        registry.insert(view(2, 2, 1, 4, 2, ViewKind::Floating));
        fill(&mut registry, 1, 't');
        fill(&mut registry, 2, 'f');

        let mut screen = Buffer::empty(Rect::new(0, 0, 10, 4));
        registry.composite(&mut screen);

        assert_eq!(screen[(0, 0)].symbol(), "t");
        assert_eq!(screen[(3, 1)].symbol(), "f", "floating must win");
        assert_eq!(screen[(3, 3)].symbol(), "t", "below the floating view");
    }

    #[test]
    fn removing_a_view_exposes_what_was_under_it() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 6, 2, ViewKind::Tile));
        registry.insert(view(2, 1, 0, 2, 1, ViewKind::Floating));
        fill(&mut registry, 1, 't');
        fill(&mut registry, 2, 'f');
        registry.remove(2);

        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 2));
        registry.composite(&mut screen);
        assert_eq!(screen[(1, 0)].symbol(), "t");
    }

    #[test]
    fn views_are_clipped_to_the_screen() {
        let mut registry = Registry::default();
        registry.insert(view(1, 4, 0, 8, 2, ViewKind::Tile));
        fill(&mut registry, 1, 't');

        let mut screen = Buffer::empty(Rect::new(0, 0, 6, 2));
        registry.composite(&mut screen);
        assert_eq!(screen[(5, 0)].symbol(), "t");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cargo test -p lem-ratatui views
```

Expected: FAIL, `not yet implemented`.

- [ ] **Step 4: Implement the registry**

```rust
impl ViewKind {
    fn layer(self) -> u8 {
        match self {
            ViewKind::Tile => 0,
            ViewKind::Header => 1,
            ViewKind::Floating => 2,
        }
    }
}

impl Registry {
    pub fn insert(&mut self, view: View) {
        let area = Rect::new(0, 0, view.width, view.height);
        let buffer = Buffer::empty(area);
        self.remove(view.id);
        self.views.push(ViewBuffer { view, buffer });
    }

    pub fn remove(&mut self, id: u64) {
        self.views.retain(|vb| vb.view.id != id);
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut ViewBuffer> {
        self.views.iter_mut().find(|vb| vb.view.id == id)
    }

    pub fn composite(&self, screen: &mut Buffer) {
        let mut ordered: Vec<&ViewBuffer> = self.views.iter().collect();
        ordered.sort_by_key(|vb| vb.view.kind.layer());

        let screen_area = screen.area;
        for vb in ordered {
            for y in 0..vb.buffer.area.height {
                for x in 0..vb.buffer.area.width {
                    let sx = vb.view.x + x;
                    let sy = vb.view.y + y;
                    if sx >= screen_area.width || sy >= screen_area.height {
                        continue;
                    }
                    screen[(sx, sy)] = vb.buffer[(x, y)].clone();
                }
            }
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p lem-ratatui views && cargo clippy --all-targets && cargo fmt --all
```

Expected: 3 passed, no warnings.

- [ ] **Step 6: Commit**

```bash
git add rust/crates/lem-protocol/src/lib.rs rust/crates/lem-ratatui/src/views.rs
git commit -m "feat(ratatui): view registry with z-order compositing"
```

---

### Task 5: Paint `put` and `clear-*` into a view buffer

**Files:**
- Modify: `rust/crates/lem-protocol/src/lib.rs` (add `Clear`)
- Create: `rust/crates/lem-ratatui/src/paint.rs`

**Interfaces:**
- Consumes: `lem_protocol::{Put, Attribute, Underline}`, `views::ViewBuffer`.
- Produces: `paint::style_of(&Attribute) -> Style`,
  `paint::put(&mut ViewBuffer, &Put)`,
  `paint::clear_eol(&mut ViewBuffer, x: u16, y: u16)`,
  `paint::clear_eob(&mut ViewBuffer, y: u16)`.

- [ ] **Step 1: Add the `Clear` argument type**

Append to `rust/crates/lem-protocol/src/lib.rs`:

```rust
/// Argument of `clear`, `clear-eol` and `clear-eob`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clear {
    pub view_info: ViewInfo,
    #[serde(default)]
    pub x: u16,
    #[serde(default)]
    pub y: u16,
}
```

- [ ] **Step 2: Write the failing paint tests**

Create `rust/crates/lem-ratatui/src/paint.rs`:

```rust
//! Turning protocol paint commands into cells.

use lem_protocol::{Attribute, Put, Underline};
use ratatui_core::style::{Color, Modifier, Style};

use crate::views::ViewBuffer;

/// Parse a `"#RRGGBB"` wire colour.
pub fn parse_color(raw: &str) -> Option<Color> {
    todo!()
}

pub fn style_of(attribute: &Attribute) -> Style {
    todo!()
}

pub fn put(vb: &mut ViewBuffer, put: &Put) {
    todo!()
}

pub fn clear_eol(vb: &mut ViewBuffer, x: u16, y: u16) {
    todo!()
}

pub fn clear_eob(vb: &mut ViewBuffer, y: u16) {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lem_protocol::{View, ViewInfo, ViewKind};
    use ratatui_core::buffer::Buffer;
    use ratatui_core::layout::Rect;

    fn view_buffer(w: u16, h: u16) -> ViewBuffer {
        ViewBuffer {
            view: View { id: 1, x: 0, y: 0, width: w, height: h,
                         use_modeline: false, kind: ViewKind::Tile },
            buffer: Buffer::empty(Rect::new(0, 0, w, h)),
        }
    }

    fn put_at(x: u16, y: u16, text: &str, attribute: Option<Attribute>) -> Put {
        Put {
            view_info: ViewInfo { id: 1 },
            x, y,
            text: text.to_string(),
            text_width: text.chars().count() as u16,
            attribute,
        }
    }

    #[test]
    fn parses_wire_colors() {
        assert_eq!(parse_color("#AABBCC"), Some(Color::Rgb(0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_color("nonsense"), None);
    }

    #[test]
    fn writes_text_at_a_cell_position() {
        let mut vb = view_buffer(10, 2);
        put(&mut vb, &put_at(2, 1, "hi", None));
        assert_eq!(vb.buffer[(2, 1)].symbol(), "h");
        assert_eq!(vb.buffer[(3, 1)].symbol(), "i");
    }

    #[test]
    fn applies_colors_and_bold() {
        let attribute = Attribute {
            foreground: Some("#FF0000".into()),
            background: Some("#000000".into()),
            bold: true,
            ..Attribute::default()
        };
        let style = style_of(&attribute);
        assert_eq!(style.fg, Some(Color::Rgb(0xFF, 0, 0)));
        assert_eq!(style.bg, Some(Color::Rgb(0, 0, 0)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn underline_color_is_carried_through() {
        let attribute = Attribute {
            underline: Some(Underline::Color("#00FF00".into())),
            ..Attribute::default()
        };
        let style = style_of(&attribute);
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(style.underline_color, Some(Color::Rgb(0, 0xFF, 0)));
    }

    #[test]
    fn writing_past_the_edge_does_not_panic() {
        let mut vb = view_buffer(4, 1);
        put(&mut vb, &put_at(3, 0, "long", None));
        assert_eq!(vb.buffer[(3, 0)].symbol(), "l");
    }

    #[test]
    fn clear_eol_blanks_the_rest_of_one_row_only() {
        let mut vb = view_buffer(5, 2);
        put(&mut vb, &put_at(0, 0, "abcde", None));
        put(&mut vb, &put_at(0, 1, "fghij", None));
        clear_eol(&mut vb, 2, 0);
        assert_eq!(vb.buffer[(1, 0)].symbol(), "b");
        assert_eq!(vb.buffer[(2, 0)].symbol(), " ");
        assert_eq!(vb.buffer[(2, 1)].symbol(), "h", "row 1 untouched");
    }

    #[test]
    fn clear_eob_blanks_every_row_from_y_down() {
        let mut vb = view_buffer(3, 3);
        for y in 0..3 {
            put(&mut vb, &put_at(0, y, "xyz", None));
        }
        clear_eob(&mut vb, 1);
        assert_eq!(vb.buffer[(0, 0)].symbol(), "x");
        assert_eq!(vb.buffer[(0, 1)].symbol(), " ");
        assert_eq!(vb.buffer[(0, 2)].symbol(), " ");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cargo test -p lem-ratatui paint
```

Expected: FAIL, `not yet implemented`.

- [ ] **Step 4: Implement painting**

```rust
pub fn parse_color(raw: &str) -> Option<Color> {
    let hex = raw.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

pub fn style_of(attribute: &Attribute) -> Style {
    let mut style = Style::default();
    if let Some(fg) = attribute.foreground.as_deref().and_then(parse_color) {
        style = style.fg(fg);
    }
    if let Some(bg) = attribute.background.as_deref().and_then(parse_color) {
        style = style.bg(bg);
    }
    if attribute.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if attribute.reverse {
        style = style.add_modifier(Modifier::REVERSED);
    }
    match &attribute.underline {
        Some(Underline::On(true)) => style = style.add_modifier(Modifier::UNDERLINED),
        Some(Underline::Color(raw)) => {
            style = style.add_modifier(Modifier::UNDERLINED);
            if let Some(color) = parse_color(raw) {
                style = style.underline_color(color);
            }
        }
        _ => {}
    }
    style
}

pub fn put(vb: &mut ViewBuffer, put: &Put) {
    let area = vb.buffer.area;
    if put.y >= area.height {
        return;
    }
    let style = put.attribute.as_ref().map(style_of).unwrap_or_default();
    let mut x = put.x;
    for grapheme in put.text.chars() {
        if x >= area.width {
            break;
        }
        let cell = &mut vb.buffer[(x, put.y)];
        cell.set_char(grapheme);
        cell.set_style(style);
        x += 1;
    }
}

pub fn clear_eol(vb: &mut ViewBuffer, x: u16, y: u16) {
    let area = vb.buffer.area;
    if y >= area.height {
        return;
    }
    for cx in x..area.width {
        vb.buffer[(cx, y)].reset();
    }
}

pub fn clear_eob(vb: &mut ViewBuffer, y: u16) {
    let area = vb.buffer.area;
    for cy in y..area.height {
        for cx in 0..area.width {
            vb.buffer[(cx, cy)].reset();
        }
    }
}
```

`put` walks `chars()`, which is wrong for combining marks and wide
characters. That is deliberate for the MVP — Lem sends a precomputed
`text_width`, so the correct fix is to lay out by that width. Leave a
`// TODO(wide-chars)` comment and raise it after the MVP runs.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p lem-ratatui paint && cargo clippy --all-targets && cargo fmt --all
```

Expected: 7 passed, no warnings.

- [ ] **Step 6: Commit**

```bash
git add rust/crates/lem-protocol/src/lib.rs rust/crates/lem-ratatui/src/paint.rs
git commit -m "feat(ratatui): paint put and clear commands into view buffers"
```

---

### Task 6: Apply whole frames and draw

**Files:**
- Modify: `rust/crates/lem-protocol/src/lib.rs` (extend `Instruction`)
- Modify: `rust/crates/lem-ratatui/src/main.rs`

**Interfaces:**
- Produces: `Instruction::{MakeView, DeleteView, Clear, ClearEol, ClearEob, MoveCursor, UpdateDisplay, ResizeDisplay}`;
  `apply_frame(&mut Registry, Bulk)` in `main.rs`.

- [ ] **Step 1: Extend the instruction set**

In `rust/crates/lem-protocol/src/lib.rs`, replace the `Instruction` enum
and the match in `RawInstruction::parse`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Put(Put),
    ModelinePut(Put),
    MakeView(View),
    DeleteView(ViewInfoArg),
    Clear(Clear),
    ClearEol(Clear),
    ClearEob(Clear),
    MoveCursor(MoveCursor),
    /// A method this display half does not implement.
    Other { method: String },
}

/// Argument of messages that carry nothing but a view reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewInfoArg {
    pub view_info: ViewInfo,
}

/// Where Lem last printed its cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveCursor {
    pub view_info: ViewInfo,
    pub x: u16,
    pub y: u16,
}

impl RawInstruction {
    pub fn parse(self) -> serde_json::Result<Instruction> {
        Ok(match self.method.as_str() {
            "put" => Instruction::Put(serde_json::from_value(self.argument)?),
            "modeline-put" => Instruction::ModelinePut(serde_json::from_value(self.argument)?),
            "make-view" => Instruction::MakeView(serde_json::from_value(self.argument)?),
            "delete-view" => Instruction::DeleteView(serde_json::from_value(self.argument)?),
            "clear" => Instruction::Clear(serde_json::from_value(self.argument)?),
            "clear-eol" => Instruction::ClearEol(serde_json::from_value(self.argument)?),
            "clear-eob" => Instruction::ClearEob(serde_json::from_value(self.argument)?),
            "move-cursor" => Instruction::MoveCursor(serde_json::from_value(self.argument)?),
            _ => Instruction::Other { method: self.method },
        })
    }
}
```

- [ ] **Step 2: Write a fixture-driven test**

Create `rust/crates/lem-protocol/tests/fixture.rs`:

```rust
//! Decode the frame captured from a real editor in Task 1.

use lem_protocol::{Bulk, Instruction};

#[test]
fn every_message_in_the_captured_frame_decodes() {
    let raw = include_str!("fixtures/frame.jsonl");
    let mut puts = 0usize;
    let mut views = 0usize;

    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let msg: serde_json::Value = serde_json::from_str(line).unwrap();
        if msg["method"] != "bulk" {
            continue;
        }
        let bulk: Bulk = serde_json::from_value(msg["params"].clone()).unwrap();
        for raw_instruction in bulk {
            match raw_instruction.parse().unwrap() {
                Instruction::Put(_) => puts += 1,
                Instruction::MakeView(_) => views += 1,
                _ => {}
            }
        }
    }

    assert!(views > 0, "expected at least one make-view in the capture");
    assert!(puts > 0, "expected at least one put in the capture");
}
```

- [ ] **Step 3: Run it**

```bash
cargo test -p lem-protocol --test fixture
```

Expected: PASS. A failure here means the wire shapes in `lib.rs`
disagree with reality — fix the types, not the test.

- [ ] **Step 4: Apply frames in `main.rs`**

Add to `rust/crates/lem-ratatui/src/main.rs`:

```rust
mod paint;
mod views;

use lem_protocol::{Bulk, Instruction};
use views::Registry;

fn apply_frame(registry: &mut Registry, bulk: Bulk) {
    for raw in bulk {
        let instruction = match raw.parse() {
            Ok(instruction) => instruction,
            // A recognised method with an unexpected argument is worth
            // knowing about, but never worth killing the editor over.
            Err(error) => {
                eprintln!("lem-ratatui: undecodable instruction: {error}");
                continue;
            }
        };
        match instruction {
            Instruction::MakeView(view) => registry.insert(view),
            Instruction::DeleteView(arg) => registry.remove(arg.view_info.id),
            Instruction::Put(p) | Instruction::ModelinePut(p) => {
                if let Some(vb) = registry.get_mut(p.view_info.id) {
                    paint::put(vb, &p);
                }
            }
            Instruction::ClearEol(c) => {
                if let Some(vb) = registry.get_mut(c.view_info.id) {
                    paint::clear_eol(vb, c.x, c.y);
                }
            }
            Instruction::Clear(c) | Instruction::ClearEob(c) => {
                if let Some(vb) = registry.get_mut(c.view_info.id) {
                    paint::clear_eob(vb, c.y);
                }
            }
            Instruction::MoveCursor(_) | Instruction::Other { .. } => {}
        }
    }
}
```

- [ ] **Step 5: Draw on `update-display`**

Replace the read loop in `main` with:

```rust
    let mut guard = term::Guard::new()?;
    let backend = ratatui_crossterm::CrosstermBackend::new(guard.out());
    let mut terminal = ratatui_core::terminal::Terminal::new(backend)?;
    let mut registry = Registry::default();

    while let Some(incoming) = lem.recv()? {
        let lem_protocol::rpc::Incoming::Notification { method, params } = incoming else {
            continue;
        };
        match method.as_str() {
            "bulk" => apply_frame(&mut registry, serde_json::from_value(params)?),
            "update-display" => {
                terminal.draw(|frame| registry.composite(frame.buffer_mut()))?;
            }
            _ => {}
        }
    }
```

- [ ] **Step 6: Verify against the real editor**

```bash
cargo run -p lem-ratatui -- ../../../../lem-ratatui-lisp
```

Expected: Lem's startup screen appears. It will not respond to input yet
— that is Task 7. Quit with `Ctrl-C`; the terminal must come back clean.

- [ ] **Step 7: Commit**

```bash
cargo clippy --all-targets && cargo fmt --all
git add rust/crates
git commit -m "feat(ratatui): apply protocol frames and draw the composited screen"
```

---

### Task 7: Input

**Files:**
- Create: `rust/crates/lem-ratatui/src/input.rs`
- Modify: `rust/crates/lem-ratatui/src/main.rs`

**Interfaces:**
- Produces: `input::KeyPayload { key, ctrl, meta, super_, shift }`
  (serialised with `super_` renamed to `super`) and
  `input::convert(KeyEvent) -> Option<KeyPayload>`.

- [ ] **Step 1: Write the failing conversion tests**

Create `rust/crates/lem-ratatui/src/input.rs`:

```rust
//! crossterm key events to Lem key syms.
//!
//! The vocabulary is Lem's, not crossterm's — see
//! `frontends/ncurses/key.lisp` for the authoritative list, and
//! `convert-keyevent` in `frontends/server/main.lisp:767` for the
//! receiving end.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyPayload {
    pub key: String,
    pub ctrl: bool,
    pub meta: bool,
    #[serde(rename = "super")]
    pub super_: bool,
    pub shift: bool,
}

pub fn convert(event: KeyEvent) -> Option<KeyPayload> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Option<KeyPayload> {
        convert(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_characters_become_single_character_syms() {
        let payload = key(KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
        assert_eq!(payload.key, "a");
        assert!(!payload.ctrl && !payload.meta && !payload.shift);
    }

    #[test]
    fn control_is_carried_as_a_flag() {
        let payload = key(KeyCode::Char('x'), KeyModifiers::CONTROL).unwrap();
        assert_eq!(payload.key, "x");
        assert!(payload.ctrl);
    }

    #[test]
    fn shift_is_dropped_for_insertion_keys() {
        // Lem's convert-keyevent discards shift when the sym is a single
        // character; sending it produces a key Lem cannot match.
        let payload = key(KeyCode::Char('A'), KeyModifiers::SHIFT).unwrap();
        assert_eq!(payload.key, "A");
        assert!(!payload.shift, "shift must be dropped for a 1-char sym");
    }

    #[test]
    fn shift_survives_for_named_keys() {
        let payload = key(KeyCode::F(3), KeyModifiers::SHIFT).unwrap();
        assert_eq!(payload.key, "F3");
        assert!(payload.shift);
    }

    #[test]
    fn space_has_its_own_sym() {
        assert_eq!(key(KeyCode::Char(' '), KeyModifiers::NONE).unwrap().key, "Space");
    }

    #[test]
    fn named_keys_use_lems_vocabulary() {
        for (code, expected) in [
            (KeyCode::Enter, "Return"),
            (KeyCode::Tab, "Tab"),
            (KeyCode::Esc, "Escape"),
            (KeyCode::Backspace, "Backspace"),
            (KeyCode::Delete, "Delete"),
            (KeyCode::Up, "Up"),
            (KeyCode::Down, "Down"),
            (KeyCode::Left, "Left"),
            (KeyCode::Right, "Right"),
            (KeyCode::Home, "Home"),
            (KeyCode::End, "End"),
            (KeyCode::PageUp, "PageUp"),
            (KeyCode::PageDown, "PageDown"),
        ] {
            assert_eq!(key(code, KeyModifiers::NONE).unwrap().key, expected);
        }
    }

    #[test]
    fn bare_modifier_presses_are_dropped() {
        assert!(key(KeyCode::Null, KeyModifiers::NONE).is_none());
    }

    #[test]
    fn super_is_serialised_under_its_wire_name() {
        let payload = key(KeyCode::Char('a'), KeyModifiers::SUPER).unwrap();
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["super"], true);
        assert!(json.get("super_").is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p lem-ratatui input
```

Expected: FAIL, `not yet implemented`.

- [ ] **Step 3: Implement the conversion**

```rust
pub fn convert(event: KeyEvent) -> Option<KeyPayload> {
    let sym = match event.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Return".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Tab".to_string(),
        KeyCode::Esc => "Escape".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return None,
    };

    // Mirror lem:insertion-key-sym-p — a sym of length 1 is an insertion
    // key, and Lem's convert-keyevent drops shift for those.
    let is_insertion = sym.chars().count() == 1;

    Some(KeyPayload {
        ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
        meta: event.modifiers.contains(KeyModifiers::ALT),
        super_: event.modifiers.contains(KeyModifiers::SUPER),
        shift: !is_insertion && event.modifiers.contains(KeyModifiers::SHIFT),
        key: sym,
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p lem-ratatui input
```

Expected: 8 passed.

- [ ] **Step 5: Poll for events in the main loop**

The loop currently blocks in `lem.recv()`. Move the read onto a thread
and select over a channel. In `main.rs`:

```rust
    let (tx, rx) = std::sync::mpsc::channel::<lem_protocol::rpc::Incoming>();
    std::thread::spawn(move || {
        while let Ok(Some(incoming)) = lem.recv() {
            if tx.send(incoming).is_err() {
                break;
            }
        }
    });
```

Lem must be moved into the thread, so keep a clone of its stdin handle
for sending. Change `child::Lem` to expose
`pub fn split(self) -> (LemReader, LemWriter)` where `LemWriter::send`
takes `&mut self` — the reader thread owns the reader, the main loop owns
the writer.

Then poll both sources:

```rust
    loop {
        if crossterm::event::poll(std::time::Duration::from_millis(5))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                if key.kind == crossterm::event::KeyEventKind::Press {
                    if let Some(payload) = input::convert(key) {
                        let body = lem_protocol::rpc::notification(
                            "input",
                            serde_json::json!({"kind": "key", "value": payload}),
                        )?;
                        writer.send(&body)?;
                    }
                }
            }
        }
        while let Ok(incoming) = rx.try_recv() {
            // ... existing bulk / update-display handling
        }
    }
```

`KeyEventKind::Press` matters: on Windows and under the kitty keyboard
protocol, release events arrive too and would double every keystroke.

- [ ] **Step 6: Verify end to end**

```bash
cargo run -p lem-ratatui -- ../../../../lem-ratatui-lisp
```

Expected: typing inserts characters; arrow keys move the cursor; `C-x
C-f` opens the file prompt.

- [ ] **Step 7: Commit**

```bash
cargo clippy --all-targets && cargo fmt --all
git add rust/crates/lem-ratatui/src
git commit -m "feat(ratatui): translate crossterm key events into Lem input"
```

---

### Task 8: Resize

**Files:**
- Modify: `rust/crates/lem-ratatui/src/main.rs`

**Interfaces:**
- Consumes: `rpc::notification`, the existing writer handle.

- [ ] **Step 1: Handle the resize event**

crossterm delivers `Event::Resize(cols, rows)`. Lem expects a `redraw`
notification carrying the new size — `redraw`
(`frontends/server/main.lisp:190`) calls `resize-display`, re-notifies
every client, and forces a full redraw. Add to the event poll:

```rust
            crossterm::event::Event::Resize(cols, rows) => {
                let body = lem_protocol::rpc::notification(
                    "redraw",
                    serde_json::json!({"size": {"width": cols, "height": rows}}),
                )?;
                writer.send(&body)?;
            }
```

- [ ] **Step 2: Resize view buffers when Lem re-sends geometry**

Lem answers a resize by re-emitting `make-view` for every window, and
`Registry::insert` already replaces a view of the same id with a
correctly sized buffer. Verify by adding to `views.rs`:

```rust
    #[test]
    fn reinserting_a_view_resizes_its_buffer() {
        let mut registry = Registry::default();
        registry.insert(view(1, 0, 0, 10, 4, ViewKind::Tile));
        registry.insert(view(1, 0, 0, 20, 8, ViewKind::Tile));
        let vb = registry.get_mut(1).unwrap();
        assert_eq!(vb.buffer.area.width, 20);
        assert_eq!(vb.buffer.area.height, 8);
    }
```

- [ ] **Step 3: Run the test**

```bash
cargo test -p lem-ratatui views
```

Expected: 4 passed.

- [ ] **Step 4: Verify by hand**

Run the editor and resize the terminal window. Expected: the layout
reflows with no stale cells and no panic.

- [ ] **Step 5: Commit**

```bash
cargo clippy --all-targets && cargo fmt --all
git add rust/crates/lem-ratatui/src
git commit -m "feat(ratatui): reflow on terminal resize"
```

---

### Task 9: Frame instrumentation

ADR 0003 defers the codec decision explicitly *on condition* that the PoC
produces numbers. This task is that condition.

**Files:**
- Create: `rust/crates/lem-ratatui/src/metrics.rs`
- Modify: `rust/crates/lem-ratatui/src/main.rs`

**Interfaces:**
- Produces: `metrics::Frames` with `record(bytes: usize, decode: Duration)`
  and `report() -> String`.

- [ ] **Step 1: Write the failing test**

Create `rust/crates/lem-ratatui/src/metrics.rs`:

```rust
//! Per-frame byte and decode-time counters.
//!
//! ADR 0003 keeps the JSON codec on the explicit condition that the PoC
//! measures it. These are those measurements.

use std::time::Duration;

#[derive(Debug, Default)]
pub struct Frames {
    count: u64,
    bytes: u64,
    max_bytes: usize,
    decode: Duration,
}

impl Frames {
    pub fn record(&mut self, bytes: usize, decode: Duration) {
        todo!()
    }

    pub fn report(&self) -> String {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_totals_and_averages() {
        let mut frames = Frames::default();
        frames.record(1000, Duration::from_micros(100));
        frames.record(3000, Duration::from_micros(300));

        let report = frames.report();
        assert!(report.contains("2 frames"), "{report}");
        assert!(report.contains("avg 2000 B"), "{report}");
        assert!(report.contains("max 3000 B"), "{report}");
        assert!(report.contains("avg decode 200"), "{report}");
    }

    #[test]
    fn an_empty_report_does_not_divide_by_zero() {
        assert!(Frames::default().report().contains("0 frames"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p lem-ratatui metrics
```

Expected: FAIL, `not yet implemented`.

- [ ] **Step 3: Implement**

```rust
impl Frames {
    pub fn record(&mut self, bytes: usize, decode: Duration) {
        self.count += 1;
        self.bytes += bytes as u64;
        self.max_bytes = self.max_bytes.max(bytes);
        self.decode += decode;
    }

    pub fn report(&self) -> String {
        if self.count == 0 {
            return "0 frames".to_string();
        }
        let avg_bytes = self.bytes / self.count;
        let avg_decode = self.decode.as_micros() / u128::from(self.count);
        format!(
            "{} frames, avg {avg_bytes} B, max {} B, avg decode {avg_decode}us",
            self.count, self.max_bytes
        )
    }
}
```

- [ ] **Step 4: Run to verify success**

```bash
cargo test -p lem-ratatui metrics
```

Expected: 2 passed.

- [ ] **Step 5: Record real frames and print on exit**

In `main.rs`, time the `bulk` decode and print the report to stderr when
the loop ends:

```rust
            "bulk" => {
                let started = std::time::Instant::now();
                let bulk: Bulk = serde_json::from_value(params)?;
                let elapsed = started.elapsed();
                frames.record(raw_len, elapsed);
                apply_frame(&mut registry, bulk);
            }
```

`raw_len` is the framed body length — thread it through from
`framing::read_message` by having the reader thread send
`(usize, Incoming)` pairs.

- [ ] **Step 6: Capture a session's numbers**

Run the editor, open a file, type a few lines, resize once, quit.
Record the printed report in
`frontends/ratatui/docs/adr/0003-keep-json-codec-for-now.md` under a new
**Measurements** heading, with the date and what the session did.

- [ ] **Step 7: Commit**

```bash
cargo clippy --all-targets && cargo fmt --all
git add rust/crates/lem-ratatui/src frontends/ratatui/docs/adr/0003-keep-json-codec-for-now.md
git commit -m "feat(ratatui): instrument frame size and decode time"
```

---

### Task 10: MVP acceptance

**Files:**
- Modify: `frontends/ratatui/README.md`

- [ ] **Step 1: Walk the acceptance script**

Run `cargo run -p lem-ratatui -- ../../../../lem-ratatui-lisp` and confirm
each of these, noting anything that fails:

1. The editor's startup screen renders, with the modeline in place.
2. `C-x C-f` opens a prompt; opening `src/lem.lisp` shows Lisp source.
3. Syntax colours appear (comments and strings differ from code).
4. Arrow keys and `C-n`/`C-p`/`C-f`/`C-b` move the cursor visibly.
5. Typing inserts text; `C-/` or `C-x u` undoes it.
6. Resizing the terminal reflows the layout with no stale cells.
7. `C-x C-c` exits and the terminal is left clean — no raw mode, no
   alternate screen, cursor visible.
8. Killing the child (`pkill lem-ratatui-lisp`) also leaves the terminal
   clean.

- [ ] **Step 2: Update the README**

Replace the status block at the top of `frontends/ratatui/README.md`:

```markdown
> **Status: working PoC.** Opens files, edits, renders syntax colours and
> reflows on resize. Single window only — no popups, mouse, clipboard or
> images. See `docs/poc-plan.md` for what is deliberately absent.
```

Move any acceptance step that failed into the README's **Known gaps**
list rather than leaving it undocumented.

- [ ] **Step 3: Commit**

```bash
git add frontends/ratatui/README.md
git commit -m "docs(ratatui): record PoC acceptance results"
```

---

## Deliberately out of scope

Each of these is a `lem-if` method or protocol message that stays
unimplemented for the MVP. They decode to `Instruction::Other` and are
ignored, which is why unknown methods must never fail a frame.

| Area | Messages / methods |
|---|---|
| Popups and menus | `display-popup-menu`, `popup-menu-*`, `display-context-menu` |
| Mouse | `mousedown`, `mouseup`, `mousemove`, `wheel` |
| Clipboard | `get-clipboard-text`, `set-clipboard-text` |
| Images and icons | `image-object`, `icon-object`, icon fonts |
| Browser-only | `js-eval`, `load-css`, `set-font`, `get-font`, `change-view` (html) |
| Cursor shape | `update-cursor-shape` |

Also deferred, with their own future records: wide-character and
combining-mark layout (Task 5 walks `chars()`), capability negotiation at
`login` (ADR 0001), protocol version exchange (ADR 0004), and the unix
socket and TCP transports (ADR 0005).
