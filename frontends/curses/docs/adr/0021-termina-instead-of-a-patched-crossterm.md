# 0021. termina instead of a patched crossterm

**Status:** Accepted — October 2026
**Revises:** [0014](0014-the-display-describes-keys.md) (how the control
bytes 0x1C–0x1F are decoded) and
[0016](0016-underline-styles.md) (what writes the display's output)

## Context

The display reads keys from the controlling terminal while its stdin and
stdout carry `lem.display.v1` ([0005](0005-stdio-as-the-default-transport.md)).
crossterm 0.29.0, the last release, does not survive that arrangement
unpatched, so `rust/vendor/crossterm` carries about 390 lines of local
fixes. Without them:

- **Startup breaks.** `query_keyboard_enhancement_flags` opens `/dev/tty`
  read-only, its write fails, and the query falls back to stdout: six
  bytes of escape sequence land in the protocol stream before `Hello`.
  Fixed on crossterm's master (#1026), unreleased.
- **The display can hang.** When the terminal goes away, the input loop
  spins on a zero-byte read at 100% CPU and `poll` never returns (#793,
  open since 2023; PR #1067 open). And because the descriptor is blocking,
  input that parses into no event leaves `poll(10ms)` inside a `read`
  until the next keystroke, so frames Lem sends stop being painted.
  Nobody upstream has proposed a fix for the second.
- **The query is unbounded.** Two seconds per query, twice at startup; it
  waits forever for DA1 after a flags reply (#1133, PR #1134 open), and
  loops forever on a persistent poll error.
- **C0 control keys are misnamed.** The bytes 0x1C–0x1F arrive as
  Ctrl+4..7 rather than `C-\`, `C-]`, `C-^`, `C-_` (PR #1036, open since
  February). In Lem those are undo and redo.
- Smaller: flag replies are read by their first ASCII digit (our #1135,
  open), and keys typed during negotiation can be reordered.

crossterm's design is why each of these needed a patch rather than code of
ours: terminal replies are private `InternalEvent`s behind private filters,
so an application cannot send a query and read the answer itself. Upstream
is active again (edition 2024, CI, a pending 0.30), but the input and
query fixes sit unreviewed.

Alternatives considered:

- **termina** (helix-editor, MIT OR MPL-2.0, 0.4.0, August 2026). Written
  by a Helix maintainer to replace crossterm in Helix, adapted from
  crossterm's parser and termwiz's escape model. It opens `/dev/tty` for
  whichever of stdin or stdout is not a terminal, so a protocol pipe is
  safe by construction. It returns end-of-file as an error, and reads only
  after `poll` reports input, so it never blocks in `read`. Replies are
  public events (`Event::Csi`, `Osc`, `Dcs`): the application writes its
  own queries and reads the answers with a filter and a timeout. It also
  parses mode reports and light/dark notifications, the two replies that
  wedge crossterm's parser (#1104); other unknown `CSI ? …` replies still
  wedge both. It inherits two crossterm parser bugs: the C0 mapping above,
  and first-digit flag parsing. On Windows, Helix still uses crossterm,
  because termina's VT input mode mishandles some non-ASCII input; termina
  has since added a legacy console mode behind a feature.
- **termwiz** (WezTerm): capable, but released last in March 2025 from
  the WezTerm monorepo, with a heavy dependency tree.
- **termion**: Unix-only, and no kitty keyboard protocol.
- **Keeping the patch until crossterm 0.30.** Only #1026 is merged. The
  rebase onto 0.30 is real work (the libc backend is gone, internal
  modules renamed), and every new terminal query would need another patch.

`ratatui-core` does not depend on any of them
([0002](0002-ratatui-core-over-full-ratatui.md), 0016), so the choice is the
display's alone. crossterm is used in six of its files.

The platforms that matter are Linux, macOS and Windows through WSL, where
the display is a Linux process. Native Windows is welcome when it is free,
not a requirement.

## Decision

**The display uses termina for the terminal's input, its mode and its
size, and writes every output byte itself. The vendored crossterm goes.**

- **Input:** `termina::PlatformTerminal` provides raw mode, the window
  size, the panic-hook restore of terminal modes, and events. The main
  loop reads events through `events::Input`, which serves what keyboard
  negotiation set aside before the terminal's own events.
- **Keyboard negotiation** keeps its one-second budget, its confirmation
  query and its ownership rules (`keyboard.rs`). The query is ours
  (`probe.rs`): it writes `CSI ? u` and DA1 to the terminal, then reads
  *every* event itself until DA1 or the deadline. Replies decide the
  answer; everything else is kept, in order, for the main loop. Reading
  everything ourselves, rather than through termina's filtered reads,
  keeps the order of keys typed during negotiation.
- **C0 control keys are decoded by the encoding in force when they were
  read.** While the terminal sends legacy encoding, Ctrl+4..7 can only
  come from the bytes 0x1C–0x1F, so the display names them `\`, `]`, `^`,
  `_` (`input::legacy_control`). That covers keys read before the
  enhancement push, and every key when negotiation falls back to legacy.
  Under the kitty protocol, Ctrl+4..7 are the digits.
- **Output:** a module of our own (`ansi.rs`) writes every sequence the
  display sends: the cell diff's SGR, cursor movement and shape, screen
  and mouse modes, keyboard push and pop, queries. The bytes are exactly
  what crossterm 0.29 wrote, so frames on the terminal do not change. This
  completes 0016's "our own terminal output": no terminal library formats
  what the display draws. termina's escape types are not used for output,
  because their formatting differs where it matters (double underline as
  SGR 21, not `4:2`).
- **Platforms:** Linux and macOS natively; Windows through WSL. Native
  Windows builds in termina's default VT input mode on a best-effort
  basis, untested. termina's `windows-legacy` feature is not used.

## Consequences

- `rust/vendor/crossterm`, its `[patch.crates-io]` entry and its PTY probe
  go. What the patch tested moves into the display's own tests and into
  `scripts/keyboard-acceptance.py`, which gains two checks for the reasons
  behind this ADR: a terminal hang-up must end the display, and input that
  never completes an event must not hold up Lem's messages.
- New terminal queries (pixel size, mode reports, theme notifications)
  are application code, not library patches.
- **A gap until termina's parser is fixed:** a raw 0x1C–0x1F byte read
  *after* the kitty protocol is confirmed decodes as Ctrl+4..7. Terminals
  speaking the protocol encode those keys as CSI u, so this needs a
  terminal that mixes encodings. The fix belongs upstream (a port of
  crossterm #1036); once termina releases it, `legacy_control` stays
  correct and the acceptance check regains its raw byte under the kitty
  protocol.
- termina accepts a malformed flags reply such as `CSI ? x u` (the port
  of #1135 fixes that too). Negotiation stays safe: an unconfirmable
  answer ends in the legacy keyboard, as before.
- termina enters raw mode with `TCSAFLUSH`, so keys typed before the
  display takes the terminal are discarded rather than read. crossterm
  used `TCSANOW`.
- crossterm 0.29 honoured `NO_COLOR` by writing a bare `CSI m` for every
  colour, which also reset the other attributes. The display now always
  draws Lem's colours.
- termina has one main author. Its code is small (about 8,500 lines with
  Windows and tests), readable, and largely crossterm's, so a fork, if
  ever needed, would be a small one.
- Kitty graphics remain future work on either library: termina does not
  parse APC replies (`ESC _ G … ESC \`) or the cell-pixel-size report.
  With replies as public events, adding them is a contained parser change
  upstream.
