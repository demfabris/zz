"""Measure where a structural control notification lands relative to an open
block, on either binary.

``control.c``'s ``control_write`` appends to ``all_blocks`` whenever that list
is not empty, so a line written while output blocks or a command block are
pending is queued behind them instead of going straight out.  The question this
probe answers is whether that ever moves a structural notification such as
``%window-add`` or ``%layout-change`` inside a block on pinned tmux, which is
where zz would differ: zz defers every notification it raises during a block
past the guard, and only ``%pause``/``%continue`` go straight out.

Run it as ``python3 control-notify-block-order-probe.py <tmux|zz> <binary>``
with a scrubbed ``HOME``.  It prints the two shapes it measured:

``blocking-command`` runs ``run-shell 'sleep 1.2'`` on the control client, which
holds that client's command queue, and raises ``new-window`` and
``resize-window`` from a second client while the guard is open.

``pending-output`` makes the attached pane write 400 lines so real output blocks
queue up, then raises ``new-window`` from a second client mid-stream.

Measured 2026-09-06 on pinned d77c9dc6 and on zz at PROTOCOL_VERSION 99: both
shapes put every structural notification after the block on both binaries, so
the two engines agree.  The pin's only synchronous writers are
``control_pause_pane`` and ``control_continue_pane``; every other notification
is a ``notify_add`` command-queue item and therefore runs after the block it was
raised during has closed.
"""

import os
import pathlib
import re
import select
import subprocess
import sys
import tempfile
import time

sys.stdout.reconfigure(line_buffering=True)


class Control:
    def __init__(self, base, env, session):
        self.process = subprocess.Popen(
            [*base, "-C", "attach-session", "-t", session],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env)
        self.lines = []
        self.pending = b""

    def pump(self, timeout):
        if not select.select([self.process.stdout], [], [], timeout)[0]:
            return False
        data = os.read(self.process.stdout.fileno(), 65536)
        if not data:
            return False
        self.pending += data
        while b"\n" in self.pending:
            line, self.pending = self.pending.split(b"\n", 1)
            self.lines.append(line.decode("utf-8", "backslashreplace"))
        return True

    def until(self, predicate, timeout=10):
        deadline = time.monotonic() + timeout
        while not predicate() and time.monotonic() < deadline:
            self.pump(0.02)
        return predicate()

    def drain(self, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.pump(0.05)

    def send(self, command):
        self.process.stdin.write((command + "\n").encode())
        self.process.stdin.flush()

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)


def placement(lines, notification):
    index = next((i for i, line in enumerate(lines)
                  if line.startswith(notification)), None)
    if index is None:
        return f"{notification}: absent"
    inside = False
    for line in lines[:index]:
        if line.startswith("%begin "):
            inside = True
        elif line.startswith(("%end ", "%error ")):
            inside = False
    return f"{notification}: {'inside a block' if inside else 'after the block'}"


def main():
    side, binary = sys.argv[1], sys.argv[2]
    root = pathlib.Path(tempfile.mkdtemp(prefix="zz-notify-block-"))
    base = ([binary, "--socket", str(root / "s")] if side == "zz"
            else [binary, "-L", f"zzprobe-block-{os.getpid()}"]) + ["-f", "/dev/null"]
    env = dict(os.environ)
    for key in ("TMUX", "TMUX_PANE", "ZZ_SOCKET", "ZZ_SESSION", "ZZ_PANE"):
        env.pop(key, None)

    def cli(*args):
        return subprocess.run([*base, *args], capture_output=True, env=env, timeout=20)

    go = root / "go"
    cli("new-session", "-d", "-s", "block", "-x", "80", "-y", "24", "sh", "-c",
        f"while [ ! -f {go} ]; do sleep 0.02; done; "
        "i=0; while [ $i -lt 400 ]; do echo PROBELINE$i; i=$((i+1)); done; exec cat")
    control = Control(base, env, "block")
    control.until(lambda: any(line.startswith("%session-changed")
                              for line in control.lines))
    control.drain(0.5)

    control.lines.clear()
    control.send("run-shell 'sleep 1.2'")
    control.until(lambda: any(line.startswith("%begin") for line in control.lines))
    time.sleep(0.3)
    cli("new-window", "-d", "-n", "blockwin", "cat")
    cli("resize-window", "-t", "block:0", "-x", "70", "-y", "20")
    control.until(lambda: any(line.startswith("%layout-change")
                              for line in control.lines))
    control.drain(0.6)
    print(f"{side} blocking-command: " + ", ".join(
        placement(control.lines, name) for name in ("%window-add", "%layout-change")))

    control.lines.clear()
    go.write_text("x")
    control.until(lambda: any("PROBELINE5" in line for line in control.lines))
    cli("new-window", "-d", "-n", "streamwin", "cat")
    control.until(lambda: any(line.startswith("%window-add")
                              for line in control.lines))
    control.drain(1.0)
    index = next(i for i, line in enumerate(control.lines)
                 if line.startswith("%window-add"))
    seen = re.findall(r"PROBELINE(\d+)", "".join(control.lines[:index]))
    print(f"{side} pending-output: " + placement(control.lines, "%window-add")
          + f", pane lines already written {seen[-1] if seen else 'none'} of 399")

    control.close()
    cli("kill-server")


if __name__ == "__main__":
    main()
