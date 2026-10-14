# 0020. The clipboard belongs to the agent; the display offers OSC 52

**Status:** Accepted — October 2026
**Revises:** [0019](0019-an-os-agent-and-a-link-per-peer.md) (its
clipboard split, and the follow-ups it left open)

## Context

[0019](0019-an-os-agent-and-a-link-per-peer.md) gave the clipboard to the
OS agent when it has one and left the display's `arboard` in place as the
fallback, "for wherever the terminal is". Its Consequences named the
follow-ups: whether the display keeps `arboard` once the agent exists, and
OSC 52.

That fallback reasoning does not hold:

- The launcher starts Lem, the display and the agent **on the same host**.
  Over SSH the display's `arboard` talks to the remote machine's display
  server, exactly as the agent's does. It never reaches the user's machine;
  only the terminal can, through OSC 52, which the display does not send.
- On a desktop the agent has the clipboard, so the display's is never used.
- So the display's `arboard` duplicates the agent's everywhere and helps
  nowhere. It is also OS integration in the process that should only paint
  cells, which is what [0008](0008-a-launcher-owns-the-processes.md) and
  0019 set out to remove.

OSC 52 (`ESC ] 52 ; c ; <base64> ST`) asks the terminal to put text on
*its* clipboard: the one on the user's machine, even over SSH, with
nothing installed there. It is not OS integration but terminal protocol,
and only the display owns the terminal, so it cannot move to the agent.

Reading the clipboard through OSC 52 (`ESC ] 52 ; c ; ? ST`, answered on
the input stream) is a different matter. Most terminals refuse it or ask
the user first, because it lets any program on the other end of an SSH
session read what the user copied; and its answer would have to be picked
out of the input stream within Lem's 0.1 s paste deadline. Pasting from
the user's machine already works without it: the terminal's own paste
arrives as bracketed paste (`Paste`).

A distributed mode — Lem headless on a host, display and agent on the
user's machine, the links carried over SSH — would make the user-side
agent the clipboard. It needs nothing from this decision beyond the agent
owning the clipboard, and is left to its own ADR.

## Decision

**The OS agent is the only process that uses the system clipboard. The
display drops `arboard`, and offers OSC 52 as a write-only terminal
capability that Lem falls back to when no agent has a clipboard.**

- **Display:** `clipboard.rs`, `examples/clip.rs` and the `arboard`
  dependency go. On `SetClipboard` it writes OSC 52 when the terminal
  supports it, and nothing otherwise. It answers a `ClipboardRequest` at
  once with no text.
- **Terminal support** (`support::osc52`), as for underline styles and
  hyperlinks: by identity, with an override. kitty, WezTerm, foot,
  Alacritty, Ghostty, Contour, iTerm2 and Windows Terminal. VTE (GNOME
  Terminal and friends) has no OSC 52, and xterm, Konsole, tmux and screen
  count as unsupported: xterm and Konsole depend on settings, and tmux
  passes OSC 52 on only with `set-clipboard on`. `LEM_RATATUI_OSC52=1` or
  `0` settles it either way.
- **Schema:** `TerminalCapabilities` gains `bool clipboard_write = 4`,
  announced in `Hello`. An addition within v1; `protocol_version` becomes
  3. `ClipboardRequest` and `ClipboardReply` stay in the schema (fields are
  never renumbered or reused) and are documented as unused by revision 3:
  the display has nothing to answer with.
- **Relay:** `lem-if:clipboard-copy` sends `SetClipboard` only when the
  display announced `clipboard_write`; `lem-if:clipboard-paste` returns NIL
  without asking. Lem then yanks from its own kill ring, as it does with no
  clipboard at all.
- **Arbitration** stays where 0019 put it, in the `ratatui` class: the
  agent when it has a clipboard, otherwise the display through the relay.
  So copying reaches the desktop through the agent locally, and the user's
  machine through the terminal over SSH; pasting comes from the agent, or
  from the terminal's own paste.
- **Payload:** OSC 52 carries the text UTF-8, base64-encoded, for the
  clipboard selection (`c`). No size limit is imposed; a terminal that caps
  it truncates or ignores what is too large.

## Consequences

- One clipboard implementation, in the agent; the display is back to
  painting cells and reading input.
- Over plain SSH (nothing installed on the user's machine), copying in Lem
  reaches the user's clipboard in the terminals listed. Pasting their
  clipboard into Lem is the terminal's paste, not `C-y`, which yanks Lem's
  own kill ring.
- Without an agent and without OSC 52 (a local session in GNOME Terminal
  with the agent disabled, say), copy and paste stay inside Lem. Before
  this, the display's `arboard` covered that case; now the agent is how
  Lem reaches a local desktop's clipboard.
- On X11, the agent hands its clipboard to a clipboard manager at exit (the
  final review's fix to 0019's work); the README's caveat that copied text
  is "gone once Lem exits" is corrected with this change.
- OSC 52 is written outside a frame, through the presenter's output, so it
  cannot interleave with a frame being drawn.
- Tested without a terminal: the escape bytes and their base64, the
  support table and its override, the capability in `Hello` and the
  relay's use of it. An acceptance check forces `LEM_RATATUI_OSC52=1` with
  no agent, copies, and finds the OSC 52 sequence with the right payload
  on the terminal; and with `0`, finds none.
