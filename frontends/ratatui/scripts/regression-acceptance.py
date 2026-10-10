#!/usr/bin/env python3
"""Check narrow panes, crash statuses and informational CLI output.

    make -C frontends/ratatui
    python3 frontends/ratatui/scripts/regression-acceptance.py
    make -C frontends/ratatui dist
    python3 frontends/ratatui/scripts/regression-acceptance.py --bundle

Uses temporary Lem homes and XDG directories. The bundled checks remove
binary overrides so that the embedded peers are exercised too.
"""
import argparse
import errno
import os
from pathlib import Path
import pty
import re
import select
import subprocess
import tempfile
import time

from harness import Session


FRONTEND = Path(__file__).resolve().parents[1]
CRASH = "acceptance: deliberate editor crash"
# Ordinary command errors open a recoverable popup. Use the editor's fatal
# error handler, the same path taken when drawing itself fails.
CRASH_FORM = f"(handler-bind ((error #'lem-core::bailout)) (error \"{CRASH}\"))"


def check_cli(binary, env):
    isolated = dict(os.environ, **{k: v for k, v in env.items() if v is not None})
    for key, value in env.items():
        if value is None:
            isolated.pop(key, None)
    # Neither of these peers is needed for information or argument errors.
    isolated.update(LEM_SESSION_ID="inherited-regression-session",
                    LEM_RATATUI_TERMINAL="/nonexistent/display",
                    LEM_RATATUI_OS_AGENT="/nonexistent/agent")
    for args, expected in ((["--help"], "Usage: lem"), (["-h"], "Usage: lem"),
                           (["--version"], "lem "), (["-v"], "lem "),
                           (["file.txt", "--version", "--help"], "Usage: lem")):
        result = subprocess.run([binary, *args], env=isolated, capture_output=True,
                                text=True, timeout=15, start_new_session=True)
        assert result.returncode == 0, (args, result.returncode, result.stderr)
        assert result.stdout.startswith(expected), (args, result.stdout)
        assert "SBCL" not in result.stdout, result.stdout
        assert not result.stderr, result.stderr
    # An empty marker still counts as a protocol session in the Lisp image;
    # the public launcher's direct path must remove that inherited value too.
    result = subprocess.run([binary, "--help"], env=dict(isolated, LEM_SESSION_ID=""),
                            capture_output=True, text=True, timeout=15,
                            start_new_session=True)
    assert result.returncode == 0 and result.stdout.startswith("Usage: lem"), result
    assert not result.stderr, result.stderr
    for args, expected in ((["-i"], "Please specify an interface to use."),
                           (["--interface"], "Please specify an interface to use."),
                           (["--help", "--interface"], "Please specify an interface to use."),
                           (["--log-filename"], "Please, specify a filename to log to."),
                           (["--version", "--log-filename"], "Please, specify a filename to log to.")):
        result = subprocess.run([binary, *args], env=isolated, capture_output=True,
                                text=True, timeout=15, start_new_session=True)
        assert result.returncode == 2, (args, result.returncode, result.stderr)
        assert not result.stdout, (args, result.stdout)
        assert result.stderr == f"lem-ratatui: {expected}\n", (args, result.stderr)
    assert not (Path(env["XDG_STATE_HOME"]) / "lem-ratatui").exists(), "CLI started session logs"
    assert not (Path(env["XDG_RUNTIME_DIR"]) / "lem-ratatui").exists(), "CLI started agent"
    print("PASS CLI: inherited session cleared, information status 0, missing operands status 2, no display or agent")


def check_pane(binary, env, mode):
    with tempfile.TemporaryDirectory(prefix=f"lem-narrow-{mode}-") as directory:
        path = Path(directory) / "narrow.txt"
        marker = f"narrow-{mode}-{Path(directory).name}"
        session = Session(15 if mode == "startup" else 100, 24, env=env,
                          binary=binary, args=[str(path)])
        try:
            session.pump(2)
            if mode == "resize":
                session.winsize(15, 24)
                session.pump(1)
            elif mode == "split":
                session.send([b"\x18", b"3"])
                session.pump(1)
                session.winsize(31, 24)
                session.pump(1)
            # Rendering and editor input must both remain usable after the
            # narrow modeline is drawn. A process that merely stays alive
            # (or exits early with status 0) cannot satisfy this disk check.
            session.send([marker], delay=0)
            session.send([b"\x18", b"\x13"])
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                session.pump(0.2)
                if path.is_file() and path.read_text() == marker:
                    break
            assert path.is_file(), (mode, "save created no file", session.plain()[-3000:])
            assert path.read_text() == marker, (mode, path.read_text(), session.plain()[-3000:])
            session.send([b"\x18", b"\x03"])
            status = session.wait(8)
            output = session.plain()
            assert status == 0, (mode, status, output[-3000:])
            assert "Backtrace" not in output and "Lem exited with" not in output, (mode, output)
            frames = re.search(r"Lem exited after (\d+) frames", output)
            assert frames and int(frames[1]) > 0, (mode, output)
            assert session.modes_restored(), f"{mode}: terminal was not restored"
            print(f"PASS narrow {mode}: marker saved, {frames[1]} frames, status 0, terminal restored")
        finally:
            session.kill()
            os.close(session.fd)


def check_crash(binary, env):
    session = Session(env=env, binary=binary, args=["--eval", CRASH_FORM])
    try:
        status = session.wait(10)
        output = session.plain()
        assert isinstance(status, int) and os.waitstatus_to_exitcode(status) == 1, (status, output)
        assert CRASH in output, output
        assert "the display exited" not in output, output
        diagnostic = re.search(r"Lem exited with exit status: 1; see ([^\r\n]+)", output)
        assert diagnostic, output
        log = Path(diagnostic[1])
        assert log.is_file(), f"crash diagnostic named missing log: {log}"
        assert CRASH in log.read_text(errors="replace"), f"fatal report missing from {log}"
        assert session.modes_restored(), "crash left terminal modes changed"
        assert "\x1b[?1049l" in session.since(0), "crash did not leave alternate screen"
        print("PASS editor crash: status 1, Lem log named with fatal report, terminal restored")
    finally:
        session.kill()
        os.close(session.fd)


def check_lisp_crash(env):
    # ToEditor(seq=1, hello=Hello(protocol_version=3, width=80, height=24)).
    # Version 3 matches PROTOCOL_VERSION in rust/crates/lem-protocols/src/display/v1.rs
    # and +protocol-version+ in display-relay/protobuf/codec.lisp.
    # Keep the input pipe open: the relay may otherwise see EOF and terminate
    # normally before its editor thread starts.
    lisp = FRONTEND / "dist/lem-ratatui-lisp"
    child_env = {k: v for k, v in dict(os.environ, **env).items() if v is not None}
    with subprocess.Popen([lisp, "--end-runtime-options", "--eval", CRASH_FORM],
                          env=child_env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE) as child:
        child.stdin.write(b"\x0a\x08\x01\x1a\x06\x08\x03\x18\x50\x20\x18")
        child.stdin.flush()
        # Drain both output streams while retaining the input pipe. The crash
        # report is small, but relying on the pipe capacity can deadlock.
        readers = [child.stdout, child.stderr]
        output = {stream: bytearray() for stream in readers}
        deadline = time.monotonic() + 10
        try:
            while readers:
                assert time.monotonic() < deadline, "Lisp crash did not terminate"
                for stream in select.select(readers, [], [], 0.1)[0]:
                    chunk = os.read(stream.fileno(), 65536)
                    if chunk:
                        output[stream].extend(chunk)
                    else:
                        readers.remove(stream)
            assert child.wait(timeout=1) == 1, child.returncode
            assert CRASH.encode() in output[child.stderr], output[child.stderr]
            assert CRASH.encode() in output[child.stdout], "missing protocol Exit report"
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
    print("PASS Lisp crash: child status 1, crash report logged and sent")


def image_with_terminal_stdout(lisp, args, env):
    """Run the saved image with a TTY on fd 1 and capture both output streams."""
    master, slave = pty.openpty()
    child = None
    try:
        child = subprocess.Popen([lisp, "--end-runtime-options", *args], env=env,
                                 stdin=subprocess.DEVNULL, stdout=slave,
                                 stderr=subprocess.PIPE, start_new_session=True)
        os.close(slave)
        slave = None
        readers = [master, child.stderr.fileno()]
        output = {fd: bytearray() for fd in readers}
        deadline = time.monotonic() + 15
        while readers:
            assert time.monotonic() < deadline, (args, "Lisp image did not terminate")
            for fd in select.select(readers, [], [], 0.1)[0]:
                try:
                    chunk = os.read(fd, 65536)
                except OSError as error:
                    if fd != master or error.errno != errno.EIO:
                        raise
                    chunk = b""
                if chunk:
                    output[fd].extend(chunk)
                else:
                    readers.remove(fd)
        status = child.wait(timeout=1)
        return status, bytes(output[master]), bytes(output[child.stderr.fileno()])
    finally:
        if slave is not None:
            os.close(slave)
        os.close(master)
        if child is not None:
            if child.poll() is None:
                child.kill()
                child.wait()
            child.stderr.close()


def check_lisp_terminal(env):
    lisp = FRONTEND / "dist/lem-ratatui-lisp"
    isolated = {k: v for k, v in dict(os.environ, **env).items() if v is not None}
    status, stdout, stderr = image_with_terminal_stdout(lisp, [], isolated)
    assert status == 2, (status, stdout, stderr)
    assert not stdout, stdout
    assert stderr == b"lem-ratatui: protocol stdout must not be a TTY\n", stderr
    for args, expected in ((["--help"], b"Usage: lem"), (["--version"], b"lem ")):
        status, stdout, stderr = image_with_terminal_stdout(lisp, args, isolated)
        assert status == 0 and stdout.startswith(expected), (args, status, stdout, stderr)
        assert not stderr, (args, stderr)
    status, stdout, stderr = image_with_terminal_stdout(lisp, ["--interface"], isolated)
    assert status == 2 and not stdout, (status, stdout, stderr)
    assert stderr == b"lem-ratatui: Please specify an interface to use.\n", stderr
    print("PASS Lisp TTY stdout: protocol refused, information and argument diagnostics available")


def check_lisp_session(env):
    """A marked protocol session must never receive informational stdout."""
    lisp = FRONTEND / "dist/lem-ratatui-lisp"
    isolated = {k: v for k, v in dict(os.environ, **env).items() if v is not None}
    diagnostic = ("lem-ratatui: LEM_SESSION_ID is set, so this is treated as a protocol session; "
                  "run the lem-ratatui launcher for --help or --version\n")
    for marker in ("regression-protocol-session", ""):
        child_env = dict(isolated, LEM_SESSION_ID=marker)
        for args in (["--help"], ["-h"], ["--version"], ["-v"]):
            result = subprocess.run([lisp, "--end-runtime-options", *args],
                                    env=child_env, stdin=subprocess.DEVNULL,
                                    capture_output=True, text=True, timeout=15,
                                    start_new_session=True)
            assert result.returncode == 2, (marker, args, result.returncode, result.stderr)
            assert not result.stdout, (marker, args, result.stdout)
            assert result.stderr == diagnostic, (marker, args, result.stderr)
        for args, expected in ((["--help", "--interface"], "Please specify an interface to use."),
                               (["--version", "--log-filename"], "Please, specify a filename to log to.")):
            result = subprocess.run([lisp, "--end-runtime-options", *args],
                                    env=child_env, stdin=subprocess.DEVNULL,
                                    capture_output=True, text=True, timeout=15,
                                    start_new_session=True)
            assert result.returncode == 2, (marker, args, result.returncode, result.stderr)
            assert not result.stdout, (marker, args, result.stdout)
            assert result.stderr == f"lem-ratatui: {expected}\n", (marker, args, result.stderr)
    print("PASS Lisp protocol session: information refused for non-empty and empty markers, parse errors take precedence")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", action="store_true")
    args = parser.parse_args()
    binary = FRONTEND / "dist" / ("lem-ratatui" if args.bundle else "lem-ratatui-dev")
    assert binary.is_file(), f"build {binary} first"
    with tempfile.TemporaryDirectory(prefix="lem-regression-") as directory:
        root = Path(directory)
        env = {"LEM_HOME": str(root / "home") + "/",
               "XDG_STATE_HOME": str(root / "state"),
               "XDG_RUNTIME_DIR": str(root / "runtime"),
               "XDG_CACHE_HOME": str(root / "cache"),
               "LEM_RATATUI_LISP": None, "LEM_RATATUI_TERMINAL": None,
               "LEM_RATATUI_OS_AGENT": None, "LEM_SESSION_ID": None}
        for name in ("home", "runtime"):
            (root / name).mkdir(mode=0o700)
        check_cli(binary, env)
        for mode in ("startup", "resize", "split"):
            check_pane(binary, env, mode)
        check_crash(binary, env)
        if not args.bundle:
            check_lisp_crash(env)
            check_lisp_terminal(env)
            check_lisp_session(env)
    print("Regression acceptance checks passed")


if __name__ == "__main__":
    main()
