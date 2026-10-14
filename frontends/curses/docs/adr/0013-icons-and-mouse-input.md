# 0013. Icons as text, clicks counted by the relay, and every message's identity and time

**Status:** Accepted — September 2026
**Answers:** the two questions [0011](0011-a-frame-oriented-protocol.md)
left open besides character width, which
[0012](0012-the-relay-frame-model.md) settled.

## Context

0011 left three questions for phase 2. Two remain.

**Icon glyphs.** 0011 worried about Nerd Font private-use glyphs, which
render only where the terminal's font has them, and considered a
capability in `Hello`. What actually reaches the relay is narrower:

- Lem core registers six icons (`src/common/character/icon.lisp:46`),
  all standard Unicode: 📁, 🔒, ▸, ▾, • and one more of the same kind.
- The large private-use tables (all-the-icons and the like) belong to
  `frontends/sdl2/icon.lisp` and `frontends/server/icon.lisp`. Neither is
  in our image since [0009](0009-our-own-relay.md) removed `lem-server`.
- Lem already computes an icon's width in cells, and 0012 made that
  width authoritative.

**Mouse input.** What Lem's core consumes (`src/mouse.lisp`):

- Coordinates are screen cells; Lem finds the window
  (`focus-window-position`).
- A press has a button and a **click count**, and Lem acts on the count:
  2 selects an expression, 3 a form (`src/mouse.lisp:109-126`). Lem
  does not detect repeated presses itself. SDL reports a count, and the
  browser client forwards `event.detail`, the browser's count.
- A release has a button. A motion has the held button, or none.
- A wheel event has deltas in lines, positive up or left: Lem scrolls
  by `-(wheel-y × *scroll-speed*)` lines, and SDL and the browser client
  (`wheelY: -scrollY`) both send that sign.
- Mouse events have no modifier slots, and nothing reads the pixel
  coordinates (`get-relative-mouse-coordinates-pixels` is never called).

crossterm reports presses, releases, drags, moves and scroll notches, but
no click count.

## Decision

**Icons are ordinary text.** An icon is a `Put` like any other run,
fitted to the width Lem gave it. There is no icon message, font field or
capability. Private-use glyphs a user registers still pass through; how
they look is up to the terminal's font.

**The display reports mouse events as they happen, and the relay counts
clicks.** The relay is the adapter between the display and Lem, so
turning presses into Lem's click counts belongs there, once, rather than
in every display. A press counts as a repeat when it has the same button
and cell as the previous press and arrives within
`lem-relay/input:*click-interval*` (default 0.5 s, settable from
`init.lisp`). The count grows while that holds (1, 2, 3…) and restarts at
1 otherwise. "Within" is judged by when the display saw each press, from
its timestamp (below), not by when it arrived. A wire without
timestamps, as today's JSON, falls back to arrival time.

**Every message carries its sender's time.** Both envelopes, `ToDisplay`
and `ToEditor`, have `uint64 time_us`: when the sender produced the
message, in microseconds on the sender's monotonic clock, counted from
when the sender started.

- **Monotonic, not wall-clock.** Wall time jumps (NTP, suspend, a manual
  change), and interval arithmetic is what timestamps are for. Rust's
  `Instant` does not serialise, so the display sends elapsed time since
  its start. The relay uses `get-internal-real-time`, which in SBCL is
  `CLOCK_MONOTONIC_COARSE`: monotonic, with a resolution of a few
  milliseconds.
- **Compared only with the same sender's.** Display times are compared
  with display times (click intervals, event order), relay times with
  relay times. Relating the two clocks, for latency measurement, needs an
  offset taken at the handshake. `Hello` and `Welcome` can carry one
  later without breaking anything.
- **On everything, from the start.** Input times drive click counting
  now. Frame times cost eight bytes and give later extensions an order of
  events that does not depend on arrival: async events, latency
  measurement, and the race conditions those bring.

**Every message is identified by session and sequence number, not by a
UUID of its own.**

- **`session_id`: a UUIDv7 per session.** The display makes it, sends it
  in `Hello`, and the relay echoes it in `Welcome`. Both sides put it on
  their log lines. v7 because it sorts by creation time, so sessions list
  in order in merged logs. It is made once, in Rust, where the `uuid`
  crate provides v7. The Quicklisp `uuid` library offers v1 and v4 only.
- **`seq`: a per-sender counter on both envelopes**, next to `time_us`:
  1 for a sender's first message in a session, one more for each after.
  `(session_id, sender, seq)` identifies any message anywhere. Messages
  are numbered as they are written, under the writer's lock, so numbers
  follow wire order whichever thread sends. `seq` is a `uint64`
  varint: 1 byte up to 127, 3 bytes up to about two million, and no
  overflow before 2⁶⁴, which at ten thousand messages a second is about
  58 million years.
- **Every frame carries `input_seq`:** the `seq` of the last display
  message the relay had handed to Lem when it closed the frame. That
  links a keystroke to the frames after it, for latency tracing, and lets
  a replay check that the same inputs produced the same frames. It means
  *delivered to Lem's event queue*, not *processed by Lem*. Marking
  consumption would put markers into that queue, which gets in the way of
  how Lem coalesces `:resize` events. That is a possible refinement, not
  part of this decision.
- **A reply names its request by `reply_to`,** the request's `seq`.
  `ClipboardReply` is the first to use it, replacing a clipboard-only
  id counter. Any later request/response uses the same field.

A UUID per message was the alternative, and it gives less. It is
unordered (v4), or ordered only to the millisecond (v7), which a burst
of keys shares. It shows no gaps or repeats. A replay generates new ones
unless they were recorded. Linking a frame to its inputs would need a
list of UUIDs rather than one number. It costs 16 bytes, twice a
keypress's message, and would need a hand-written v7 in Lisp. If an
outside system ever needs one per event (OpenTelemetry, say), it can be
derived from `(session_id, sender, seq)` when exporting. That is
deterministic, so a replay exports the same ids.

**The wire carries cells, never pixels, and no click count:**

```proto
message Mouse {
  uint32 x = 1;                      // screen cell, 0-based
  uint32 y = 2;
  oneof action {
    Press press = 3;
    Release release = 4;
    Move move = 5;
    Wheel wheel = 6;
  }
}
message Press   { Button button = 1; }
message Release { Button button = 1; }
message Move    { optional Button button = 1; }   // a held button makes it a drag
message Wheel   { sint32 dx = 1; sint32 dy = 2; }  // lines; positive is left / up
enum Button { BUTTON_UNSPECIFIED = 0; LEFT = 1; MIDDLE = 2; RIGHT = 3; }
```

The time is not repeated in `Mouse`: it is the envelope's `time_us`.
`relay.proto` is the source of truth. This is its intended shape, for
review.

**The display captures the mouse in phase 2,** with no option to switch
it off for now. Capturing takes the terminal's own click-and-drag
selection away, so the README documents Shift+drag, which most terminals
keep for selection while an application has the mouse.

## Consequences

- One click-counting rule for every display, in Lisp, and tested there
  (`relay/tests/input.lisp`). A display is a thinner port.
- The relay holds a little input state: the last press and its time.
  Because that time is the display's, transit delays do not turn a
  double click into two singles, even over a slow link.
- Every input and every frame in the Lisp model carries a time
  (`input-time`, `frame-time`). Inputs carry the display's `seq`
  (`input-seq`), frames the last one delivered (`frame-input-seq`), and
  the codec numbers what it sends. The protobuf codec only has to put
  them on the wire. Today's JSON wire carries none of them, but the relay
  still numbers its messages, since clipboard replies are matched by it.
- The model's frame counter is gone: a frame is identified by its
  envelope's `seq`, like every other message.
- Today's JSON wire can carry a browser's `clicks`. `lem-relay/json`
  now ignores it, so there is one rule for every display, not two.
- Terminal-native selection needs Shift once the display captures the
  mouse.
- Nothing about icons needs protocol support. If a terminal-specific
  icon set is ever wanted, it is a Lem-side registration (as SDL2 has),
  not a wire change.
