#!/usr/bin/env python3
"""Exercise the built display's keyboard negotiation through isolated PTYs.

    python3 frontends/ratatui/scripts/keyboard-acceptance.py [display-binary]

Defaults to the Rust debug display. Needs only Python's standard library,
does not start Lem or read its personal configuration, and never touches the
caller's terminal. The child's stdin/stdout are protocol pipes; its controlling
PTY supplies terminal replies and keys. No generated files are needed.
"""

import argparse
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import termios
import time


FRONTEND = Path(__file__).resolve().parents[1]
QUERY = b"\x1b[?u"
PUSH = b"\x1b[>5u"
POP = b"\x1b[<1u"
LEAVE_SCREEN = b"\x1b[?1049l"
DA1 = b"\x1b[?1;2c"
TERMINAL_COMMAND = re.compile(rb"\x1b\[(?:\?u|0?c|>5u|<1u)")


def varint(data, offset=0):
    """Read an unsigned protobuf varint, distinguishing incomplete input."""
    value = 0
    for index in range(10):
        if offset + index == len(data):
            return None
        byte = data[offset + index]
        assert index < 9 or byte <= 1, "overflowing protobuf varint"
        value |= (byte & 0x7f) << (7 * index)
        if byte < 0x80:
            return value, offset + index + 1
    raise AssertionError("overlong protobuf varint")


def fields(data):
    """Decode the varint/length-delimited fields used by Hello and Key.

    A small strict reader keeps this smoke test independent of protoc and
    Python protobuf. Unsupported wire types and truncated fields are errors,
    so terminal escape output cannot silently pass as a protocol message.
    """
    result = {}
    offset = 0
    while offset < len(data):
        tag = varint(data, offset)
        assert tag is not None, "truncated protobuf field tag"
        tag, offset = tag
        number, wire = tag >> 3, tag & 7
        assert number > 0, "invalid protobuf field number"
        value = varint(data, offset)
        assert value is not None, "truncated protobuf field value"
        value, offset = value
        if wire == 2:
            end = offset + value
            assert end <= len(data), "truncated protobuf byte field"
            value, offset = data[offset:end], end
        else:
            assert wire == 0, f"unexpected protobuf wire type {wire}"
        result.setdefault(number, []).append(value)
    return result


def single(message, field, default=None):
    values = message.get(field, [default])
    assert len(values) == 1, f"duplicate protobuf field {field}"
    return values[0]


def key_description(payload):
    key = fields(payload)
    assert set(key) <= {1, 2, 3, 4, 5}, f"unknown Key fields: {key}"
    assert len(set(key) & {1, 2, 3}) == 1, "Key needs exactly one code"
    modifiers = []
    for item in key.get(4, []):
        if isinstance(item, int):
            modifiers.append(item)
        else:
            offset = 0
            while offset < len(item):
                decoded = varint(item, offset)
                assert decoded is not None, "truncated packed modifier"
                modifier, offset = decoded
                modifiers.append(modifier)
    code = next(number for number in (1, 2, 3) if number in key)
    value = single(key, code)
    if code == 1:
        value = value.decode("utf-8")
    return (code, value, tuple(modifiers), bool(single(key, 5, 0)))


def text_key(text, modifiers=(), keypad=False):
    return (1, text, tuple(modifiers), keypad)


class Session:
    def __init__(self, binary, mode, *, queued=False, answer_colours=True):
        self.mode = mode
        self.queued = queued
        self.answer_colours = answer_colours
        self.master, self.slave = pty.openpty()
        self.before = termios.tcgetattr(self.slave)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
        self.terminal = bytearray()
        self.wire = bytearray()
        self.stderr = bytearray()
        self.messages = []
        self.keys = []
        self.command_offset = 0
        self.keyboard_queries = 0
        self.pending_query = False
        self.negotiation_started = None
        self.hello_at = None
        self.started = time.monotonic()

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)

        # This display-only test needs no home directory, configuration or
        # desktop clipboard connection. Keep the child environment minimal.
        env = {"TERM": "xterm-256color", "LANG": "C.UTF-8"}
        if mode == "override":
            env["LEM_RATATUI_KEYBOARD"] = "legacy"
        self.child = subprocess.Popen(
            [str(binary)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            pass_fds=(self.slave,),
            preexec_fn=controlling_terminal,
        )
        self.readers = {self.master, self.child.stdout.fileno(), self.child.stderr.fileno()}

    def answer(self):
        # Keep the unconsumed suffix so split escape sequences work too.
        for match in TERMINAL_COMMAND.finditer(self.terminal, self.command_offset):
            self.command_offset = match.end()
            command = match.group()
            if command == QUERY:
                self.keyboard_queries += 1
                self.pending_query = True
                if self.negotiation_started is None:
                    self.negotiation_started = time.monotonic()
            elif command in (b"\x1b[c", b"\x1b[0c"):
                if self.pending_query:
                    self.pending_query = False
                    self.answer_keyboard()
                elif self.answer_colours:
                    # Colours have their own DA1 probe before raw mode.
                    os.write(self.master, DA1)

    def answer_keyboard(self):
        query = self.keyboard_queries
        if self.mode in ("timeout", "silent"):
            return
        if self.mode == "malformed":
            os.write(self.master, b"\x1b[?xu")
            return
        if self.mode == "legacy":
            os.write(self.master, DA1)
            return
        if self.mode == "confirmation_timeout" and query == 2:
            return
        flags = 0 if query == 1 else (1 if self.mode == "confirmation_failed" else 5)
        prefix = b""
        suffix = b""
        if self.queued:
            # Entered during each query, never read separately by the harness.
            prefix = b"a\x1d" if query == 1 else b"\x1b[53;5u\xc4\x8d"
            suffix = b"b" if query == 1 else b"\xc5\xa1"
        os.write(self.master, prefix + f"\x1b[?{flags}u".encode() + DA1 + suffix)

    def decode_wire(self):
        while self.wire:
            header = varint(self.wire)
            if header is None:
                return
            size, offset = header
            assert 0 < size <= 1 << 20, f"invalid protocol message size {size}"
            if len(self.wire) < offset + size:
                return
            message = fields(bytes(self.wire[offset:offset + size]))
            del self.wire[:offset + size]
            assert set(message) <= set(range(1, 10)), f"unknown ToEditor fields: {message}"
            assert single(message, 1) == len(self.messages) + 1, "broken wire sequence"
            kinds = set(message) - {1, 2}
            assert len(kinds) == 1, f"invalid ToEditor message: {message}"
            kind = kinds.pop()
            assert isinstance(single(message, kind), bytes), "message must be length-delimited"
            if not self.messages:
                assert kind == 3, "Hello must be first"
                self.hello_at = time.monotonic()
            else:
                assert kind in (4, 8), f"unexpected display message {kind}"
            self.messages.append(message)
            if kind == 4:
                self.keys.append(key_description(single(message, 4)))

    def pump(self, seconds=0.05):
        ready, _, _ = select.select(list(self.readers), [], [], seconds)
        for fd in ready:
            chunk = os.read(fd, 65536)
            if not chunk:
                self.readers.remove(fd)
            elif fd == self.master:
                self.terminal.extend(chunk)
                self.answer()
            elif fd == self.child.stdout.fileno():
                self.wire.extend(chunk)
                self.decode_wire()
            else:
                self.stderr.extend(chunk)

    def until(self, predicate, seconds=3.5):
        deadline = time.monotonic() + seconds
        while not predicate():
            assert time.monotonic() < deadline, (
                f"{self.mode}: timed out; queries={self.keyboard_queries}; "
                f"keys={self.keys}; stderr={self.stderr.decode(errors='replace')}"
            )
            self.pump()

    def hello(self):
        self.until(lambda: self.hello_at is not None)
        hello = fields(single(self.messages[0], 3))
        assert single(hello, 1, 0) > 0, "missing schema revision"
        assert (single(hello, 3), single(hello, 4)) == (100, 30), "lost terminal geometry"
        capabilities = single(hello, 7)
        assert capabilities is not None, "display must report known capabilities"
        capabilities = fields(capabilities)
        assert set(capabilities) <= {1, 2, 3}, "unexpected capability fields"
        return tuple(bool(single(capabilities, field, 0)) for field in (1, 2, 3))

    def exit(self):
        # ToDisplay(seq=1, exit=Exit()) in standard delimited protobuf.
        self.child.stdin.write(b"\x04\x08\x01\x3a\x00")
        self.child.stdin.flush()
        self.until(lambda: self.child.poll() is not None)
        while select.select(list(self.readers), [], [], 0)[0]:
            self.pump(0)
        assert self.child.returncode == 0, self.stderr.decode(errors="replace")
        assert not self.wire, f"truncated or polluted protocol stdout: {self.wire!r}"
        assert termios.tcgetattr(self.slave) == self.before, "terminal modes not restored"
        assert self.terminal.count(LEAVE_SCREEN) == 1, "alternate screen not restored exactly once"
        if POP in self.terminal:
            assert self.terminal.index(POP) < self.terminal.index(LEAVE_SCREEN), "keyboard popped too late"

    def close(self):
        if self.child.poll() is None:
            self.child.kill()
            self.child.wait()
        for stream in (self.child.stdin, self.child.stdout, self.child.stderr):
            stream.close()
        os.close(self.master)
        os.close(self.slave)


def run_case(binary, mode, *, queued=False, send_keys=False):
    session = Session(binary, mode, queued=queued, answer_colours=mode != "silent")
    try:
        enhanced = mode == "enhanced"
        assert session.hello() == (enhanced,) * 3, f"{mode}: incorrect capabilities"
        expected = []
        if queued:
            expected += [text_key("a"), text_key("]", (1,)), text_key("b"),
                         text_key("5", (1,)), text_key("č"), text_key("š")]
        if send_keys:
            os.write(session.master,
                     b"\x1b[53;5u\x1d\x1b[103;6u\x1b[103:71;6u\x07"
                     b"\x1b[57403;5u\x1b[52;5u\xc4\x8d\xc5\xa1")
            expected += [text_key("5", (1,)), text_key("]", (1,)),
                         text_key("g", (1, 3)), text_key("G", (1,)), text_key("g", (1,)),
                         text_key("4", (1,), True), text_key("4", (1,)),
                         text_key("č"), text_key("š")]
        elif mode in ("legacy", "override"):
            os.write(session.master, b"a\x1c\x1d\x1e\x1f\xc4\x8d\xc5\xa1")
            expected += [text_key("a"), text_key("\\", (1,)), text_key("]", (1,)),
                         text_key("^", (1,)), text_key("_", (1,)),
                         text_key("č"), text_key("š")]
        session.until(lambda: len(session.keys) >= len(expected))
        session.exit()
        assert session.keys == expected, f"{mode}: lost, changed or unexpected keys: {session.keys}"
        expected_queries = 0 if mode == "override" else (2 if mode in (
            "enhanced", "confirmation_failed", "confirmation_timeout") else 1)
        expected_pushes = int(expected_queries == 2)
        assert session.keyboard_queries == expected_queries, f"{mode}: unexpected probe count"
        assert session.terminal.count(PUSH) == expected_pushes, f"{mode}: incorrect flag pushes"
        assert session.terminal.count(POP) == expected_pushes, f"{mode}: incorrect flag pops"
        elapsed = session.hello_at - session.started
        if session.negotiation_started is not None:
            negotiation = session.hello_at - session.negotiation_started
            assert negotiation < 1.6, f"{mode}: keyboard negotiation exceeded its one-second budget"
            if mode in ("timeout", "silent", "confirmation_timeout"):
                assert negotiation >= 0.85, f"{mode}: missing bounded wait"
        else:
            negotiation = 0
        assert elapsed < 3.5, f"{mode}: startup was not bounded"
        print(f"PASS {mode}{' with queued keys' if queued else ''}: "
              f"Hello {elapsed:.3f}s, keyboard {negotiation:.3f}s, {len(expected)} keys")
    finally:
        session.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="?", type=Path,
                        default=FRONTEND / "rust/target/debug/lem-ratatui")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"display binary does not exist: {binary}; build lem-ratatui first")
    run_case(binary, "enhanced", queued=True, send_keys=True)
    for mode in ("legacy", "override", "timeout", "silent", "malformed",
                 "confirmation_failed", "confirmation_timeout"):
        run_case(binary, mode)
    print("8/8 keyboard acceptance checks passed")


if __name__ == "__main__":
    main()
