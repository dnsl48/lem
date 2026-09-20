"""MVP acceptance for the Ratatui frontend: the 8-point script in docs/poc-plan.md.

Run from anywhere once both halves are built:

    sbcl --load .qlot/setup.lisp --load frontends/ratatui/build.lisp
    cd frontends/ratatui/rust && cargo build --release
    python3 frontends/ratatui/scripts/acceptance.py

Assert on effects — disk contents, exit statuses, termios flags — never on
the terminal byte stream. ratatui diffs per cell, so typed text never
appears contiguously, and only changed digits of the modeline are
repainted. Three checks here first reported false failures for exactly
that reason.
"""
import os, pty, select, time, re, fcntl, termios, struct, signal, subprocess, sys

REPO = "/var/mnt/workbench/toys/lem/ratatui"
BIN = f"{REPO}/frontends/ratatui/rust/target/release/lem-ratatui"
LISP = f"{REPO}/frontends/ratatui/lem-ratatui-lisp"
ENV = dict(os.environ, LEM_HOME="/tmp/lem-scratch/", TERM="xterm-256color")

class Session:
    def __init__(self, cols=100, rows=30):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.execve(BIN, [BIN, LISP], ENV)
        self.winsize(cols, rows)
        self.out = b""
    def winsize(self, c, r):
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", r, c, 0, 0))
        try: os.kill(self.pid, signal.SIGWINCH)
        except ProcessLookupError: pass
    def pump(self, sec):
        end = time.time() + sec
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.2)
            if r:
                try: c = os.read(self.fd, 1 << 20)
                except OSError: return
                if not c: return
                self.out += c
    def send(self, items, delay=0.12):
        for it in items:
            os.write(self.fd, it if isinstance(it, bytes) else it.encode())
            time.sleep(delay)
    def child_pid(self):
        r = subprocess.run(["pgrep", "-P", str(self.pid)], capture_output=True, text=True)
        pids = [int(p) for p in r.stdout.split()]
        return pids[0] if pids else None
    def mark(self): return len(self.out)
    def since(self, m): return self.out[m:].decode(errors="replace")
    def plain(self, m=0):
        return re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", self.since(m)).replace("\x1b(B", "")
    def modes_restored(self):
        a = termios.tcgetattr(self.fd)
        return bool(a[3] & termios.ICANON) and bool(a[3] & termios.ECHO)
    def wait(self, sec=6):
        end = time.time() + sec
        while time.time() < end:
            try:
                done, st = os.waitpid(self.pid, os.WNOHANG)
                if done: return st
            except ChildProcessError: return "reaped"
            self.pump(0.3)
        return None
    def kill(self):
        try: os.kill(self.pid, 9); os.waitpid(self.pid, 0)
        except Exception: pass

def edit(path, keys, settle=8):
    """Open PATH, run KEYS, save, quit; return what landed on disk."""
    if os.path.exists(path): os.remove(path)
    t = Session(); t.pump(settle)
    t.send([b"\x18", b"\x06"]); t.pump(1.5)          # C-x C-f
    t.send(path); t.send([b"\r"]); t.pump(2)
    keys(t)
    t.send([b"\x18", b"\x13"]); t.pump(2)            # C-x C-s
    t.send([b"\x18", b"\x03"]); t.wait(8); t.kill()  # C-x C-c
    return open(path).read() if os.path.exists(path) else "<no file>"

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
s = Session(); s.pump(8)
child = s.child_pid()
if child:
    os.kill(child, signal.SIGKILL)
status = s.wait(12)
restored = s.modes_restored()
check(8, "killing Lem leaves the terminal clean", status == 0 and restored,
      f"child={child}, status={status}, modes restored={restored}")
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

print()
failed = [r for r in results if not r[2]]
print(f"{len(results) - len(failed)}/{len(results)} acceptance checks passed")
sys.exit(1 if failed else 0)
