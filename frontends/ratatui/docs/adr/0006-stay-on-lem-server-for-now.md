# 0006. Stay on `lem-server` for now

**Status:** Accepted — September 2026
**Revisits:** [0001](0001-reuse-lem-server-jsonrpc.md)

## Context

[0001](0001-reuse-lem-server-jsonrpc.md) chose to reuse `lem-server`
before any of it had been run. Task 1 ran it, and three of its
assumptions now have evidence attached — enough to ask whether the Lisp
half should be our own code instead.

The case against `lem-server`, fairly stated: it is buggy on the path we
need, its protocol is wasteful, and it carries browser-specific
machinery a terminal will never use.

**The bugs are not where they appear to be.** All three defects Task 1
hit were in *jsonrpc's stdio transport*, not in `lem-server`'s protocol
code (see [protocol-notes](../protocol-notes.md) section 13). Writing our
own server would not have avoided a single one of them — we would still
need a transport, and would have hit the same missing
`on-open-connection` and the same character-vs-byte framing. Only two of
Task 1's eight findings are `lem-server`'s doing: colour slots with no
`:initform`, and `redraw` being required after `login`. Both are
one-line workarounds on our side.

Against that, `lem-server`'s 943 lines of `lem-if` implementation
produced correct frames on first contact once the transport worked. The
valuable part is not the line count but the ~150 lines of `draw-object`,
which maps every drawing-object subclass — `text-object`, `icon-object`,
`eol-cursor-object`, `extend-to-eol-object`, `line-end-object`,
`image-object` — to wire instructions, including non-obvious behaviour
such as `set-last-print-cursor` being the only mechanism by which cursor
position is tracked at all. That is precisely the code we would most
likely get subtly wrong while still learning the protocol.

**The waste is real and measured.** On the committed fixture:

```
fixture total            41,042 bytes  (133 instructions)
browser-only fields      17,912 bytes  (44%)   pixelX/Y/W/H, font,
                                               content, border, border_shape, type
attribute payloads        8,478 bytes  (21%)   across 80 puts
  distinct attributes           10             interning saves ~17%
```

Roughly 61% of the wire is of no use to a terminal. But that argues for
changing the protocol, not for replacing the server: both fixes are on
the order of 30 lines *inside* `lem-server`, are backward compatible
(capability negotiation at `login`, attribute interning), and would
benefit the browser client too. They are levers 1 and 2 of
[0003](0003-keep-json-codec-for-now.md), now with numbers.

**The coupling argument turned out weaker than assumed.** The obvious
objection to forking is that drawing objects are internal Lem classes and
a fork would rot. Measured:

```
commits touching BOTH src/display/ and frontends/server/main.lisp
since 2024-01-01:  2
```

Twice in twenty months. A fork would need syncing about annually —
cheap, not prohibitive. This is a materially weaker objection than it
appears, and it is recorded here so the option is not dismissed on a
premise that does not hold.

Three options were weighed: stay; write our own; or fork ~400 lines of
`main.lisp` into `lisp/server.lisp` and strip the browser paths. The
third is the serious alternative — it keeps the working `draw-object`
mapping while giving up nothing but an annual sync.

## Decision

**Stay on `lem-server` for now.** Do not fork and do not rewrite.

Not because forking is wrong, but because the decision is reversible and
gets strictly better-informed by waiting. The PoC exists to find out
whether the drawing-object-to-cell mapping is clean. Forking now pays the
cost before that is known; forking after Task 10 costs the same ~400
lines with the protocol understood and a working display half to test the
fork against.

**Reconsider when either of these happens:**

- the PoC reaches its acceptance criteria (Task 10), at which point the
  mapping is understood and the fork is a mechanical exercise; or
- we want a protocol change `lem-server` will not take. Wanting one it
  *would* take is not a trigger — that is a contribution, not a fork.

## Consequences

- We keep inheriting fixes to the drawing pipeline, and keep paying two
  one-line workarounds for the handshake.
- The browser-specific machinery stays on the wire. This is cheaper than
  it looks: inert nulls plus methods that already decode to
  `Instruction::Other`. It is wire bloat, and
  [0005](0005-stdio-as-the-default-transport.md) already established that
  the wire is not the constraint — a local pipe does not care about 41KB
  frames.
- We carry `lisp/jsonrpc-stdio-fixes.lisp` as a stack of monkey-patches
  over `lem-server`'s own. Acceptable for a PoC, bad permanently, and an
  argument that gains weight if it ever grows a fourth entry.
- The measurements above are the baseline. If a later profile shows the
  61% actually costs something, lever order is: capability negotiation,
  then attribute interning, both inside `lem-server` — and only then
  reconsider ownership.
