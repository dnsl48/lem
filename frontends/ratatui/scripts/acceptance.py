"""MVP acceptance for the Ratatui frontend: the checks from docs/poc-plan.md,
plus regression guards added since.

Run from anywhere once both halves are built:

    make -C frontends/ratatui
    python3 frontends/ratatui/scripts/acceptance.py

Assert on effects — disk contents, exit statuses, termios flags — never on
the terminal byte stream. ratatui diffs per cell, so typed text never
appears contiguously, and only changed digits of the modeline are
repainted. Three checks here first reported false failures for exactly
that reason.
"""
import os, re, signal, sys

from harness import REPO, Session, edit

results = []
def check(n, desc, ok, detail=""):
    results.append((n, desc, ok, detail))
    print(f"{'PASS' if ok else 'FAIL'}  {n}. {desc}" + (f"   [{detail}]" if detail else ""))

# --- 1, 2, 3, 4, 5: one session ------------------------------------------
s = Session()
s.pump(8)
start = s.plain()
check(1, "startup screen renders with modeline",
      "Welcome to Lem" in start and "Dashboard" in start,
      f"{len(s.out)} bytes")

m = s.mark()
s.send([b"\x18", b"\x06"]); s.pump(1.2)            # C-x C-f
s.send(f"{REPO}/src/lem.lisp"); s.send([b"\r"]); s.pump(3)
src = s.plain(m)
check(2, "C-x C-f opens a Lisp source file",
      "lem.lisp" in src and ("defpackage" in src or "in-package" in src or "defun" in src),
      "lem.lisp visible")

colours = set(re.findall(r"\x1b\[38;2;(\d+;\d+;\d+)", s.since(m)))
check(3, "syntax colours appear", len(colours) >= 4, f"{len(colours)} distinct foreground colours")

m = s.mark(); s.send([b"\x0e"] * 2, delay=0.2); s.pump(1.5)
s.kill()

# Cursor keys: proven by where the next character lands, not by scraping
# the modeline — only changed digits are repainted, so a substring search
# of the diff finds nothing.
content = edit("/tmp/lem-accept-cursor.txt",
               lambda t: (t.send("abc", delay=0.15), t.pump(0.6),
                          t.send([b"\x1b[D", b"\x1b[D"], delay=0.25), t.pump(0.6),
                          t.send("X", delay=0.15), t.pump(0.6)))
check(4, "cursor keys move the point", content.strip() == "aXbc", f"file={content.strip()!r}")
s.kill()

# --- 5: typing and undo, verified on disk ------------------------------
# C-x u, not C-/: a terminal does not send what C-/ implies.
content = edit("/tmp/lem-accept-undo.txt",
               lambda t: (t.send("hello", delay=0.15), t.pump(0.8),
                          [(t.send([b"\x18", b"u"], delay=0.2), t.pump(0.5)) for _ in range(6)]))
check(5, "typing inserts and undo reverts", "hello" not in content, f"file={content.strip()!r}")

# --- 6: resize reflow ----------------------------------------------------
s = Session(60, 20); s.pump(7)
m = s.mark(); s.winsize(100, 30); s.pump(4)
def extent(text):
    widest, col = 0, 1
    for tok in re.split(r"(\x1b\[[0-9;?]*[a-zA-Z])", text):
        if tok.startswith("\x1b["):
            mm = re.match(r"\x1b\[(\d+);(\d+)H", tok)
            if mm: col = int(mm.group(2))
            continue
        run = "".join(c for c in tok if c.isprintable())
        if run: widest = max(widest, col + len(run) - 1); col += len(run)
    return widest
grown = extent(s.since(m))
m = s.mark(); s.winsize(60, 20); s.pump(4)
shrunk = extent(s.since(m))
check(6, "resize reflows the layout", shrunk <= 60 < grown, f"grow->{grown}, shrink->{shrunk}")
s.kill()

# --- 7: C-x C-c exits and restores the terminal --------------------------
s = Session(); s.pump(8)
s.send([b"\x18", b"\x03"]); status = s.wait(8)
restored = s.modes_restored()
alt_left = "\x1b[?1049l" in s.plain() or "\x1b[?1049l" in s.since(0)
check(7, "C-x C-c exits cleanly and restores the terminal",
      status == 0 and restored, f"status={status}, ICANON+ECHO restored={restored}")
s.kill()

# --- 8: killing Lem restores the terminal --------------------------------
# The display sees EOF and hands the terminal back; the launcher, seeing
# Lem die of a signal, reports it and exits 1.
s = Session(); s.pump(8)
child = s.lem_pid()
if child:
    os.kill(child, signal.SIGKILL)
status = s.wait(12)
restored = s.modes_restored()
exited_1 = isinstance(status, int) and os.WIFEXITED(status) and os.WEXITSTATUS(status) == 1
reported = "Lem exited with signal" in s.plain()
check(8, "killing Lem leaves the terminal clean and is reported",
      exited_1 and restored and reported,
      f"child={child}, status={status}, modes restored={restored}, reported={reported}")
s.kill()

# --- 9: multi-column list renders instead of crashing ------------------
# C-x C-b goes through lem/multi-column-list, which reaches for the
# *theme's* foreground when the implementation claims underline-colour
# support. lem-default leaves that NIL on purpose, so the branch dies in
# darken-color. Regression guard for that.
s = Session(); s.pump(8)
s.send([b"\x18", b"\x06"]); s.pump(1.5)
s.send("/tmp/lem-accept-list.txt"); s.send([b"\r"]); s.pump(2)
s.send([b"\x18", b"\x02"]); s.pump(3)          # C-x C-b
text = s.plain()
check(9, "C-x C-b lists buffers without crashing",
      "not of type" not in text and "Buffer" in text,
      "no backtrace, header present")
s.kill()

# --- 10: the terminal's own colours reach Lem ----------------------------
# The display asks with OSC 10/11 before taking over the terminal, and the
# relay logs what Hello carried. A terminal that answers only DA1 must
# cost no wait: the display then knows at once that colours are not coming.
def logged_colours():
    log = os.path.join(os.environ.get("LEM_HOME", "/tmp/lem-scratch/"), "debug.log")
    try:
        lines = [l for l in open(log, errors="replace") if "terminal colours" in l]
    except FileNotFoundError:
        return ""
    return lines[-1].strip() if lines else ""

s = Session(colors=("dddd/dddd/dddd", "fafa/fafa/fafa")); s.pump(8)
answered = logged_colours()
s.send([b"\x18", b"\x03"]); s.wait(8); s.kill()
s = Session(); s.pump(3)
silent = logged_colours()
first_frame = "Welcome to Lem" in s.plain()
s.send([b"\x18", b"\x03"]); s.wait(8); s.kill()
check(10, "the terminal's colours reach Lem, and their absence costs no wait",
      "#DDDDDD on #FAFAFA" in answered and "unknown on unknown" in silent and first_frame,
      f"answered: {answered[-24:]!r}; silent: {silent[-24:]!r}; first frame within 3s: {first_frame}")

# --- 11: font styles reach the terminal -----------------------------------
# Markdown emphasis is drawn in document-italic-attribute, which
# lem-ratatui marks :italic after every theme load (ADR 0015). The relay
# carries it and the display turns it into SGR 3.
with open("/tmp/lem-accept-italic.md", "w") as f:
    f.write("plain *emphasis* plain\n")
s = Session(); s.pump(8)
m = s.mark()
s.send([b"\x18", b"\x06"]); s.pump(1.5)           # C-x C-f
s.send("/tmp/lem-accept-italic.md"); s.send([b"\r"]); s.pump(3)
drawn = s.since(m)
check(11, "Markdown emphasis is drawn in italic",
      "\x1b[3m" in drawn and "emphasis" in s.plain(m),
      f"SGR 3 sent: {'\x1b[3m' in drawn}")
s.kill()

print()
failed = [r for r in results if not r[2]]
print(f"{len(results) - len(failed)}/{len(results)} acceptance checks passed")
sys.exit(1 if failed else 0)
