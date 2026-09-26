#!/usr/bin/env python3
"""Exercise the vendored query using isolated PTYs, never the caller's terminal."""
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import termios
import time

VENDOR = Path(__file__).resolve().parents[1]
WORKSPACE = VENDOR.parents[1]
BUILD = WORKSPACE / "target" / "crossterm-keyboard-probe"
QUERY = b"\x1b[?u\x1b[c"


def build_probe():
    BUILD.mkdir(parents=True, exist_ok=True)
    temporary = BUILD / "tmp"
    temporary.mkdir(exist_ok=True)
    manifest = BUILD / "Cargo.toml"
    manifest.write_text(
        '[package]\nname = "lem-crossterm-keyboard-probe"\n'
        'version = "0.0.0"\nedition = "2021"\n[workspace]\n'
        '[dependencies]\ncrossterm = { path = '
        + json.dumps(str(VENDOR))
        + ' }\n[[bin]]\nname = "keyboard-probe"\npath = '
        + json.dumps(str(VENDOR / "tests" / "keyboard_probe.rs"))
        + "\n"
    )
    subprocess.run(
        ["cargo", "build", "--quiet", "--manifest-path", str(manifest)],
        cwd=WORKSPACE,
        env={**os.environ, "TMPDIR": str(temporary)},
        check=True,
    )
    return BUILD / "target" / "debug" / "keyboard-probe"


def run_case(binary, name, response, expected, expected_keys=(), timeout_ms=180):
    master, slave = pty.openpty()
    before = termios.tcgetattr(slave)
    descriptor_flags = fcntl.fcntl(slave, fcntl.F_GETFL)

    def controlling_tty():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    child = subprocess.Popen(
        [str(binary), str(timeout_ms), str(len(expected_keys))],
        stdin=slave,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        preexec_fn=controlling_tty,
    )
    try:
        observed = b""
        deadline = time.monotonic() + 2
        while QUERY not in observed:
            remaining = deadline - time.monotonic()
            assert remaining > 0, (name, "no terminal query", observed)
            readable, _, _ = select.select([master], [], [], remaining)
            assert readable, (name, "no terminal query", observed)
            observed += os.read(master, 1024)
        assert observed == QUERY, (name, "unexpected terminal output", observed)
        if response:
            os.write(master, response)
        stdout, stderr = child.communicate(timeout=2)
        assert child.returncode == 0, (name, stdout, stderr)
        assert b"\x1b" not in stdout, (name, "query leaked into protocol stdout", stdout)
        lines = stdout.decode().splitlines()
        assert f"RESULT={expected}" in lines, (name, lines)
        assert "RAW_BEFORE=false" in lines and "RAW_AFTER=false" in lines, (name, lines)
        elapsed = int(next(line.split("=", 1)[1] for line in lines if line.startswith("ELAPSED_MS=")))
        assert elapsed < timeout_ms + 500, (name, "query exceeded deadline", elapsed)
        if response is None or expected == "flags:5" and response == b"\x1b[?5u":
            assert elapsed >= timeout_ms - 30, (name, "query skipped bounded wait", elapsed)
        if name == "legacy":
            assert elapsed < timeout_ms, (name, "DA1-only response did not finish promptly", elapsed)
        keys = [line.removeprefix("KEY=") for line in lines if line.startswith("KEY=")]
        assert keys == list(expected_keys), (name, "queued keys changed or reordered", keys)
        assert termios.tcgetattr(slave) == before, (name, "terminal mode not restored")
        assert fcntl.fcntl(slave, fcntl.F_GETFL) == descriptor_flags, (name, "stdin flags changed")
        print(f"PASS {name} ({elapsed} ms)")
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        os.close(master)
        os.close(slave)


def run_hangup(binary):
    master, slave = pty.openpty()

    def controlling_tty():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        signal.signal(signal.SIGHUP, signal.SIG_IGN)

    child = subprocess.Popen(
        [str(binary), "180", "0"],
        stdin=slave,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        preexec_fn=controlling_tty,
    )
    try:
        readable, _, _ = select.select([master], [], [], 2)
        assert readable and os.read(master, 1024) == QUERY, "no hangup query"
        os.close(master)
        master = None
        stdout, stderr = child.communicate(timeout=2)
        assert child.returncode == 0, (stdout, stderr)
        assert b"RESULT=error:" in stdout, stdout
        assert b"\x1b" not in stdout, stdout
        # The disconnected terminal cannot accept mode restoration; only require
        # prompt error propagation, without a repeating EOF/EIO read loop.
        print("PASS terminal_hangup (prompt error, no retry loop)")
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        if master is not None:
            os.close(master)
        os.close(slave)


def main():
    binary = build_probe()
    run_case(binary, "supported", b"\x1b[?5u\x1b[?1;2c", "flags:5")
    run_case(binary, "legacy", b"\x1b[?1;2c", "none")
    run_case(binary, "timeout", None, "error:TimedOut")
    run_case(binary, "flags_without_DA1", b"\x1b[?5u", "flags:5")
    run_case(
        binary,
        "interleaved_keys",
        b"a\x1d\x1b[?5u\x1b[53;5u\xc4\x8d\x1b[?1;2c\xc5\xa1",
        "flags:5",
        ("97:0", "93:2", "53:2", "269:0", "353:0"),
    )
    run_case(binary, "malformed_reply", b"\x1b[?xu", "error:TimedOut")
    run_case(binary, "partial_reply", b"\x1b[?", "error:TimedOut")
    run_case(binary, "timeout_keeps_input", b"q\x1d", "error:TimedOut", ("113:0", "93:2"))
    run_hangup(binary)
    # Without a controlling TTY the query must fail without a stdout fallback.
    child = subprocess.run(
        [str(binary), "180", "0"],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        start_new_session=True,
        check=True,
        timeout=2,
    )
    assert b"RESULT=error:" in child.stdout, child.stdout
    assert b"\x1b" not in child.stdout, child.stdout
    print("PASS no_controlling_tty (no protocol stdout leakage)")


if __name__ == "__main__":
    main()
