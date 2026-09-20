# 0005. stdio as the default transport

**Status:** Accepted — September 2026

## Context

[0004](0004-two-processes-not-embedded-lisp.md) settles that there are two
processes. This record picks how they talk. `lem-server` already
implements three transports, so this is a default-picking question rather
than an implementation one:

```lisp
;; frontends/server/lem-server.asd
"jsonrpc/transport/stdio"
"jsonrpc/transport/websocket"
#-os-windows "jsonrpc/transport/local-domain-socket"
```

**D-Bus** is the wrong tool by construction. It is built for
low-frequency control messages between desktop services, and every
message routes through a broker — userspace to `dbus-daemon` and back —
doubling the hops for a stream of paint commands. It is Linux-centric,
and adopting it would mean re-expressing the protocol in D-Bus's type
system, discarding the reuse that [0001](0001-reuse-lem-server-jsonrpc.md)
is built on. It buys service discovery and activation, neither of which
apply to a process we spawned ourselves.

**TCP**'s problem is the security surface. There is no authentication
anywhere in this protocol, and a localhost port is reachable by any
process owned by any user on the machine. For an editor that evaluates
arbitrary Lisp that is a real hole, on top of port allocation and
conflicts for no compensating benefit.

That leaves **stdio** against a **unix domain socket**:

| | stdio | unix socket |
|---|---|---|
| Setup | none | path, `XDG_RUNTIME_DIR`, stale-socket cleanup |
| Lifecycle | free: child dies with parent, EOF signals death | managed by us |
| Reconnect | impossible | yes |
| Multiple clients | no | yes |
| Windows | works (anonymous pipes) | not compiled in |
| stdout | consumed by protocol | free for logging |
| Access control | fd is private to the parent | filesystem permissions |

Throughput is not a differentiator: both are kernel buffers and the
difference is noise against 40KB worst-case frames. Nobody should pick on
that basis.

The insight that decides it: **a unix socket is not a transport upgrade,
it is a product decision in disguise.** Its real advantage is
reconnection, which only matters given a daemon model where Lem runs
headless and terminals attach and detach like tmux. That is an
attractive feature, but it would be adopted for the workflow, not for the
plumbing.

## Decision

**stdio is the default and the only transport the PoC supports.** Zero
configuration, lifecycle for free, no cleanup, no ports, no paths, and
the one option that works unchanged on Windows.

**Unix socket and TCP are noted as potential future features. No decision
is taken about either here** — not to adopt, not to reject. Each needs
its own investigation and its own record:

- *Unix socket* is inseparable from the daemon/attach question above.
  Investigating it means investigating whether Lem should have a headless
  mode, not just whether it should open a socket.
- *TCP* is the only answer for two cases where nothing else works:
  Windows (where the unix-socket transport is not even compiled in) and
  genuine remote attach. Its authentication story would have to be
  designed, not inherited.

Nothing in the PoC should assume stdio is permanent. The Rust side's
connect path stays behind a seam so a second transport is an addition
rather than a rewrite.

## Consequences

- **`*standard-output*` becomes protocol.** Any stray `(format t ...)`,
  compiler note, or chatty library writing to standard output corrupts
  the frame stream — the classic LSP-over-stdio failure. There is
  precedent for the mitigation in the tree: `run-websocket-server`
  already muffles `*trace-output*` and `*error-output*` into a broadcast
  stream. The stdio path needs the same treatment for `*standard-output*`.
- **The child's stderr must go to a log file**, not be inherited from the
  Rust parent, or backtraces land on the TTY and shred the display.
- No reconnection: if either half dies, the session ends. Accepted, and
  partly a feature — it is what lets the Rust parent detect EOF and
  restore the terminal ([0004](0004-two-processes-not-embedded-lisp.md)).
- Exactly one display client. Multi-client viewing is not available and
  is not being pursued.
- The existing stdio transport reads its input a character at a time,
  recomputing `babel:string-size-in-octets` per character
  (`frontends/server/jsonrpc-stdio-patch.lisp`). Only input events travel
  that direction, so it is roughly 100 iterations per keystroke and
  harmless — but it is the path we now depend on, and worth knowing
  before anyone profiles input latency and is surprised.
