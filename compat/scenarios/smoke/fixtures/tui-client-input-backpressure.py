"""Does an attached client still act on a key while its own pty output is backed up?

The fixture attaches a real client on a pty at 80x24 and presses the same F5
binding twice. The first press happens with the master drained and is the
canary: it fails loudly if the binding never worked at all. The second happens
while the pane runs an endless producer of unique lines and nothing reads the
master, so every frame the client wants to draw is a fresh screen and the sink
is saturated.

The pin's client hands its terminal bytes to a libevent buffer and never blocks
its event loop on the tty, so the second press acts as fast as the first. A
client that writes its frames straight through from the loop that also handles
input blocks there instead, and the key sits in the pty until something drains
the master.

Saturation is derived, not guessed: sink_capacity() fills a scratch pty until
the write would block, so the kernel's own buffer size is measured here, and
the drain window measures how fast this client fills it. The undrained window
is eight times the quotient, floored and capped, so both sides wait only as
long as their own measured fill rate needs.

ZZ_BACKPRESSURE_VERBOSE=1 prints the latencies and the derivation; the scenario
keeps them out so the two sides can be diffed.
"""

import fcntl
import os
import pathlib
import pty
import select
import shlex
import struct
import subprocess
import sys
import termios
import time
import tty as ttymod

sys.stdout.reconfigure(line_buffering=True)

KEY_TIMEOUT = 10.0
KEY_F5 = b"\x1b[15~"
DRAIN_WINDOW = 1.0
DRAIN_POLL = 0.05
SATURATION_MULTIPLE = 8
SATURATION_CAP = 20.0
SATURATION_FLOOR = 0.5

verbose = bool(os.environ.get("ZZ_BACKPRESSURE_VERBOSE"))
side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
home = pathlib.Path(os.environ["HOME"])
mark = home / ("backpressure-" + side + ".mark")
flooding = home / ("backpressure-" + side + ".flooding")
session = "backpressure"
verdict = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
child = None
master = None


def tmux(*args, check=True):
    result = subprocess.run(["tmux", *args], capture_output=True, text=True, timeout=20)
    if check and result.returncode:
        raise RuntimeError((args, result.returncode, result.stderr))
    return result.stdout.strip()


def await_condition(predicate, timeout, label):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.02)
    raise AssertionError(label)


def sink_capacity():
    upper, lower = pty.openpty()
    ttymod.setraw(lower)
    os.set_blocking(lower, False)
    block = b"x" * 4096
    total = 0
    try:
        while total < (1 << 24):
            try:
                total += os.write(lower, block)
            except BlockingIOError:
                break
    finally:
        os.close(lower)
        os.close(upper)
    return total


def drain_once():
    got = 0
    while select.select([master], [], [], 0)[0]:
        try:
            chunk = os.read(master, 1 << 20)
        except OSError:
            break
        if not chunk:
            break
        got += len(chunk)
    return got


def press_and_time(drain):
    mark.unlink(missing_ok=True)
    started = time.monotonic()
    os.write(master, KEY_F5)
    deadline = started + KEY_TIMEOUT
    while time.monotonic() < deadline:
        if drain:
            drain_once()
        if mark.exists():
            return time.monotonic() - started
        time.sleep(0.02)
    return None


def report(label, latency):
    if verbose:
        print("%s key acted=%s latency=%.3f" % (label, "yes" if latency else "no", latency or -1))
    else:
        print("%s key acted=%s" % (label, "yes" if latency else "no"))


try:
    capacity = sink_capacity()
    tmux("new-session", "-d", "-s", session, "-x", "80", "-y", "24", "sleep 600")
    tmux("set-option", "-g", "status-keys", "emacs")
    tmux("bind-key", "-n", "F5", "run-shell", "-b", "touch " + shlex.quote(str(mark)))
    pane = tmux("display-message", "-p", "-t", session + ":0", "#{pane_id}")

    if side == "zz":
        attach = [os.environ["ZZ_SMOKE_ZZ_BIN"], "--socket", os.environ["ZZ_SMOKE_ZZ_SOCKET"]]
    else:
        attach = [os.environ["ZZ_SMOKE_TMUX_BIN"], "-f", "/dev/null",
                  "-L", os.environ["ZZ_SMOKE_TMUX_LABEL"]]
    argv = ["env", "-u", "TMUX", "-u", "TMUX_PANE", "-u", "ZZ_SOCKET", "-u", "ZZ_SESSION",
            "-u", "ZZ_PANE", "-u", "EDITOR", "-u", "VISUAL", "TERM=xterm-256color",
            *attach, "attach-session", "-t", "=" + session]

    child, master = pty.fork()
    if child == 0:
        os.execvp(argv[0], argv)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    await_condition(lambda: bool(tmux("list-clients", "-t", "=" + session,
                                      "-F", "#{client_name}", check=False)),
                    15, "client attached")
    drain_once()

    report("drained", press_and_time(drain=True))

    flooding.unlink(missing_ok=True)
    tmux("respawn-pane", "-k", "-t", pane,
         "sh -c 'touch %s; exec base64 /dev/urandom'" % shlex.quote(str(flooding)))
    await_condition(flooding.exists, 30, "endless producer started")

    drained_bytes = 0
    window_start = time.monotonic()
    while time.monotonic() - window_start < DRAIN_WINDOW:
        time.sleep(DRAIN_POLL)
        drained_bytes += drain_once()
    rate = drained_bytes / (time.monotonic() - window_start)
    if rate <= 0:
        raise AssertionError("attached client wrote nothing while the pane flooded")
    saturate = min(SATURATION_CAP, max(SATURATION_FLOOR, SATURATION_MULTIPLE * capacity / rate))
    if verbose:
        print("sink capacity=%d rate=%.0f bytes/sec saturate=%.2fs" % (capacity, rate, saturate))
    deadline = time.monotonic() + saturate
    while time.monotonic() < deadline:
        time.sleep(0.05)
    print("backlog saturated=yes")

    backlogged = press_and_time(drain=False)
    report("backlogged", backlogged)
    tmux("respawn-pane", "-k", "-t", pane, "sleep 600", check=False)

    verdict = "drained=acted backlogged=%s" % ("acted" if backlogged else "stalled")
finally:
    if master is not None:
        try:
            os.write(master, b"\x02d")
        except OSError:
            pass
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            drain_once()
            done, _ = os.waitpid(child, os.WNOHANG)
            if done:
                child = None
                break
            time.sleep(0.02)
        try:
            os.close(master)
        except OSError:
            pass
        if child:
            os.kill(child, 9)
            os.waitpid(child, 0)
    subprocess.run(["tmux", "set-environment", "-g", "ZZ_TUI_BACKPRESSURE", verdict],
                   capture_output=True, timeout=20)
