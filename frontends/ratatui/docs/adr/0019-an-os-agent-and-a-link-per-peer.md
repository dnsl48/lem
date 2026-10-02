# 0019. An OS agent, and a link per peer rather than a bus

**Status:** Accepted — October 2026
**Revises:** [0008](0008-a-launcher-owns-the-processes.md) (what the
launcher starts), [0009](0009-our-own-relay.md) (the relay's name),
[0013](0013-icons-and-mouse-input.md) (who makes the session id)

## Context

[0008](0008-a-launcher-owns-the-processes.md) moved everything about the
operating system out of the display and into the launcher, and named
what would come next: "system tray presence, collecting and reporting
errors, possibly telemetry". Now that it is about to arrive — a tray
icon, D-Bus, notifications, a clipboard with history — it does not fit
the launcher either:

- **Event loops.** Tray libraries want a main thread of their own: macOS
  requires it, and `tray-icon` on Linux runs GTK's. D-Bus through `zbus`
  wants an async runtime. The launcher is synchronous `duct` code that
  waits on two children; the display is a crossterm loop. Neither should
  host the other's loop.
- **Failure.** A tray or D-Bus library that crashes or hangs should not
  take the editor or the terminal with it.
- **Absence.** Over SSH, in a container or in CI there is no desktop.
  `lem-ratatui`'s `clipboard.rs` already treats that as "do nothing"
  rather than an error; every OS feature needs the same.
- **Not a terminal concern.** Nothing here depends on there being a
  terminal. The SDL2 frontend could use a tray and a clipboard history
  as well as this one.

The launcher's own job — finding, unpacking, starting, logging,
reporting exits — is worth keeping small, and 0008 kept it free of the
protocol on purpose.

**There is no bus today.** The launcher joins two anonymous pipes
between Lem and the display, and that is the whole topology. A third
participant needs a decision about how it is reached. A hub that routes
namespaced envelopes would:

- reverse 0008 by making the launcher (or something it starts) parse
  every message;
- put a hop on the frame and input path, which is the one that is
  latency-sensitive;
- share Lem's single stdout between frames and everything else, so that
  a large clipboard history queues frames behind it.

Nothing in what is planned needs two of the other processes to talk to
each other without Lem: every exchange is Lem and one peer.

**Clipboard.** Over SSH the clipboard the user means is on *their*
machine, which only the terminal can reach (OSC 52). An agent on the
remote host writing with `arboard` reaches the wrong clipboard, or none.
So the clipboard cannot simply move from the display to the agent.

**Names.** "Relay" names what 0009's system does — relays `lem-if` to
another process — but not to what. The protocol is named after it,
`lem.relay.v1`, though it is the protocol of the link between Lem and a
display. With a second link both need to say which peer they are for.
`lem-os` was considered for the agent's Lisp side and rejected: it reads
as Lem's own way to the operating system, which may well exist one day
without an agent. `lem-display` was rejected for the same reason:
`src/display/` is core's redisplay, and `lem-if:update-display` uses the
word that way. "Link" was rejected for the shared layer: in this
frontend a link is a hyperlink (`lisp/links.lisp`, `Put.link`,
[0018](0018-hyperlinks.md)).

## Decision

**A fourth process, `lem-os-agent`, owns integration with the desktop.
Each peer has its own link to Lem and its own protobuf package; Lem is
the only process every link reaches, and nothing routes between links.**

```
                     ┌──────────────────────┐
                     │ lem-ratatui-launcher │  finds, starts, logs,
                     └──────────────────────┘  reports exits
                    spawns │      │      │
           ┌───────────────┘      │      └───────────────┐
           ▼                      ▼                      ▼
 ┌──────────────────┐  ┌─────────────────────────┐  ┌──────────────┐
 │   lem-ratatui    │◄►│           Lem           │◄►│ lem-os-agent │
 │    (display)     │  │ lem-display-relay       │  │ tray, D-Bus, │
 └──────────────────┘  │ lem-os-agent            │  │ clipboard    │
   lem.display.v1      │ lem-ipc-channels        │  └──────────────┘
   stdio pipes         └─────────────────────────┘   lem.os.v1
                                                     local socket
```

### Protocols: a package per link

| Package | Link | Schema |
|---|---|---|
| `lem.display.v1` | Lem ↔ a display | `proto/lem/display/v1/display.proto`: today's `lem.relay.v1`, renamed |
| `lem.os.v1` | Lem ↔ the OS agent | `proto/lem/os/v1/os.proto`: new |

- **Each versions on its own.** 0010's rule — never renumber, an
  incompatible change is a new package — applies per package. A tray
  change never bumps the display protocol.
- **The rename is free on the wire.** Protobuf's binary encoding does not
  carry package names; only generated identifiers change. Captured
  fixtures stay valid.
- **The same conventions on every link**, from 0011 and 0013: envelopes
  with `seq` and `time_us`, a `Hello`/`Welcome` handshake in which the
  side that connects speaks first, replies naming their request by
  `seq`, and capabilities announced in the handshake so that nothing
  required fails silently when absent. They are repeated per package
  rather than shared through an import until a third link shows what is
  actually common.
- **`lem.os.v1` starts with the handshake and the clipboard** (read,
  write; the agent's `Welcome` says whether it has one). History,
  notifications and the tray are additions within v1, each with a
  capability, each reviewed as it comes.
- `proto/` stays at the top of the frontend, language-neutral, because
  both toolchains compile it. The Rust crate is where it is checked, not
  where it lives.

### Rust

- **`lem-protocols`**, renamed from `lem-protocol` because it now holds
  one protocol per link, has the generated types for every package
  (`lem_protocols::display::v1`, `lem_protocols::os::v1`) and
  length-delimited framing. A library: no runtime, no routing. There is
  no broker crate, because there is nothing to broker.
- **`lem-os-agent`** is a new binary crate. It listens on the socket it
  is given, answers the handshake with what this desktop offers, and
  reports no capabilities rather than failing where there is no desktop.
- **`lem-ratatui`** depends on `lem-protocols`, for
  `display::v1`.
- **`lem-ratatui-launcher`** still depends on neither. For the agent it:
  1. finds it as it finds the display: `LEM_RATATUI_OS_AGENT`, the
     bundle (which embeds it too), or beside itself;
  2. makes a private directory (`$XDG_RUNTIME_DIR`, else a 0700
     temporary directory) and picks a socket path in it;
  3. starts the agent with the path, and with its stdin a pipe the
     launcher holds open, so the launcher going away for any reason is
     EOF to the agent. For now the agent exits on it; later the agent's
     own configuration may choose instead to detach and carry on as a
     daemon, which needs nothing from the launcher;
  4. starts Lem with `LEM_OS_AGENT_SOCKET` set;
  5. reports an agent that dies as a warning with its log path, not as
     the end of the session, and removes the directory on exit.
- **The launcher makes the session id**, a UUIDv7 in its canonical text
  form as 0013 specifies, and passes it to all three as `LEM_SESSION_ID`, so one id is on every
  process's log lines. The display still sends it in `Hello`; it makes
  its own only when the variable is absent, as when run by hand.

### Lisp

Three systems where there was one:

| System | Role |
|---|---|
| `lem-ipc-channels` | A `channel` to one peer process over a stream: length-delimited framing, numbering, the handshake, matching replies to requests with a deadline, and a reader thread delivering to Lem with `send-event`. Codec-agnostic: a link supplies encode and decode. Extracted from `relay/protobuf/framing.lisp` and `serve.lisp`, and the reply queue and `await-clipboard` in `relay/relay.lisp`. |
| `lem-display-relay` | Today's `lem-relay`, renamed, with `/protobuf` and `/tests` as before: `lem-if:*` to `lem.display.v1`, over a channel on stdio. |
| `lem-os-agent` | `lem.os.v1` over a channel on `LEM_OS_AGENT_SOCKET`, and the public API for Lem, extensions and init files. |

- **Extensions see a typed API, not messages.** For example
  `(lem-os-agent:capable-p :clipboard)`, and later
  `(lem-os-agent:notify "Build done" ...)` and a hook for tray activation.
  Generated protobuf classes are not exported, so the schema can evolve
  without breaking user configuration.
- **The agent link does not go through the relay.** `lem-os-agent`
  depends on `lem-ipc-channels` and `lem/core`, not on
  `lem-display-relay`, so a frontend other than this one can load it.
  It lives in `frontends/ratatui/` until a second frontend does.
- **Connecting.** `lem-os-agent` connects when Lem starts, retrying
  briefly while the agent comes up. With no variable, no agent, or a
  connection that drops, it reports no capabilities and every call is a
  no-op, as the display's clipboard is today without a display server.

### Clipboard

The `ratatui` class's `lem-if:clipboard-copy` and `lem-if:clipboard-paste`
choose: the agent when it reports a clipboard, otherwise the display,
through the relay as today. Over SSH the agent finds no display server,
reports no clipboard, and the display answers — which is where an OSC 52
fallback belongs. The relay's 0.1-second deadline
(`*clipboard-timeout*`) applies to whichever answers.

### Renaming first

The renames — `lem.relay.v1` to `lem.display.v1`, `lem-relay` to
`lem-display-relay`, `relay/` to `display-relay/`, `capture-relay-v1` to
`capture-display-v1`, and the `lem-protocol` crate to `lem-protocols` —
are one change of their own, landed before
anything else here, with no change in behaviour. ADRs 0009–0018 keep the
old names: they are records of what was decided at the time.

## Consequences

- **Four processes.** The agent is optional: a session without it
  behaves exactly as today. Two features of the editor now depend on a
  process that may not be there, and both have to be tested both ways.
- **Lem grows a second reader thread**, one per channel, and has to be
  well-behaved when either peer is slow or gone. The display link's
  behaviour is unchanged.
- **Several tray icons with several editors.** The agent is per session
  to begin with. Becoming one per user later is a change of lifetime,
  not of protocol, because the transport is already a socket that Lem
  connects to, and Lem's `Hello` to the agent carries the session id. What it
  would add is the agent serving several Lems and knowing which one a
  tray click is for.
- **Clipboard code in two places** until the display's `arboard` is
  reconsidered: the agent for the desktop, the display for wherever the
  terminal is. Whether the display keeps `arboard` once the agent exists,
  and OSC 52 itself, are follow-ups.
- **Local sockets are Unix first.** Lem connects with `sb-bsd-sockets`'
  `local-socket`. Windows has AF_UNIX since Windows 10, but neither SBCL's
  support for it nor the agent is tested there, as Windows generally is
  not (0008).
- **The Rust and Lisp sides of the agent link share the name
  `lem-os-agent`**: a client named after its service. Prose says "the
  agent" for the process and "`lem-os-agent` in Lisp" for the system.
- **`make dist`** embeds a third binary in the launcher.
