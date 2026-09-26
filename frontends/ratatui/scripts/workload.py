"""The fixed workload from ADR 0003, measured: open a file, type 40
characters, save, quit.

Run from anywhere once both halves are built:

    make -C frontends/ratatui
    python3 frontends/ratatui/scripts/workload.py [runs]

The numbers come from the display half itself: on Lem's exit it prints
how many frames it applied and what arrived on the wire. Frame counts
depend on timing (a modified buffer emits frames while idle), so the
workload runs several times and the median is the number to compare.
"""
import os, re, statistics, sys

from harness import Session

PATH = "/tmp/lem-workload.txt"
TEXT = "the quick brown fox jumps over a lazy do"   # 40 characters
assert len(TEXT) == 40


def run():
    """One pass of the workload; returns (applied, received, bytes)."""
    if os.path.exists(PATH): os.remove(PATH)
    t = Session(); t.pump(8)
    t.send([b"\x18", b"\x06"]); t.pump(1.5)          # C-x C-f
    t.send(PATH); t.send([b"\r"]); t.pump(2)
    t.send(TEXT, delay=0.12); t.pump(1)
    t.send([b"\x18", b"\x13"]); t.pump(2)            # C-x C-s
    t.send([b"\x18", b"\x03"]); t.wait(8); t.kill()  # C-x C-c
    text = t.plain()
    saved = open(PATH).read() if os.path.exists(PATH) else ""
    if saved.rstrip("\n") != TEXT:
        sys.exit(f"workload did not save what it typed: {saved!r}")
    applied = re.search(r"Lem exited after (\d+) frames", text)
    wire = re.search(r"lem-ratatui: (\d+) frames, (\d+) B total", text)
    if not (applied and wire):
        sys.exit("no metrics report in the display's output")
    return int(applied.group(1)), int(wire.group(1)), int(wire.group(2))


runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
results = []
for n in range(1, runs + 1):
    applied, received, total = run()
    results.append((applied, received, total))
    print(f"run {n}: {applied} frames applied, {received} received, {total:,} B")

applied = statistics.median(r[0] for r in results)
received = statistics.median(r[1] for r in results)
total = statistics.median(r[2] for r in results)
print(f"median: {applied:g} frames applied, {received:g} received, {total:,.0f} B")
