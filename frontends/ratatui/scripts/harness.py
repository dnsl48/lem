"""Drive the launcher in a pty, as a user at a terminal would.

Shared by acceptance.py and workload.py. Needs both halves built:
`make -C frontends/ratatui`.
"""
import os, pty, select, time, re, fcntl, termios, struct, signal, subprocess, sys

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../.."))
BIN = f"{REPO}/frontends/ratatui/rust/target/release/lem-ratatui-launcher"
LISP = f"{REPO}/frontends/ratatui/dist/lem-ratatui-lisp"
ENV = dict(os.environ, LEM_HOME="/tmp/lem-scratch/", TERM="xterm-256color", LEM_RATATUI_LISP=LISP)

class Session:
    """The launcher in a pty, answering what a terminal answers.

    DA1 (`ESC [ c`) is always answered, as every terminal does. With
    `colors=(fg, bg)`, as "rrrr/gggg/bbbb" hex, OSC 10 and 11 are too:
    that is how the display learns the terminal's colours (colors.rs).
    """
    def __init__(self, cols=100, rows=30, colors=None, env=None):
        self.colors = colors
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.execve(BIN, [BIN], dict(ENV, **(env or {})))
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
                self.answer(c)
    def answer(self, chunk):
        replies = []
        if self.colors:
            if b"\x1b]10;?" in chunk:
                replies.append(b"\x1b]10;rgb:" + self.colors[0].encode() + b"\x1b\\")
            if b"\x1b]11;?" in chunk:
                replies.append(b"\x1b]11;rgb:" + self.colors[1].encode() + b"\x1b\\")
        if b"\x1b[c" in chunk:
            replies.append(b"\x1b[?62;22c")
        for reply in replies:
            os.write(self.fd, reply)
    def send(self, items, delay=0.12):
        for it in items:
            os.write(self.fd, it if isinstance(it, bytes) else it.encode())
            time.sleep(delay)
    def lem_pid(self):
        r = subprocess.run(["pgrep", "-P", str(self.pid), "-f", "lem-ratatui-lisp"],
                           capture_output=True, text=True)
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
                # The pty can still hold what was written just before
                # exit (the display's metrics report among it).
                if done: self.pump(1); return st
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
