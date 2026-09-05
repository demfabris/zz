import fcntl
import os
from pathlib import Path
import pty
import re
import select
import shlex
import signal
import struct
import subprocess
import sys
import termios
import time
import tty

sys.stdout.reconfigure(line_buffering=True)

if sys.argv[1] == "record":
    tty.setraw(0)
    os.write(1, b"\x1b[?2004hPASTE_READY")
    data = b""
    while not data.endswith(b"\x1b[201~"):
        data += os.read(0, 1024)
    Path(sys.argv[2]).write_bytes(data)
    while True:
        signal.pause()

work = Path(sys.argv[1])
prefix = sys.argv[2:]
session = "keyscontract"
master = None
pid = None
screen = bytearray()


def cli(*args):
    result = subprocess.run([*prefix, *args], capture_output=True, text=True, timeout=15)
    if result.returncode:
        raise RuntimeError(f"{args}: {result.stderr.strip()}")
    return result.stdout.strip()


def drain():
    if master is None:
        return
    while select.select([master], [], [], 0)[0]:
        try:
            data = os.read(master, 65536)
        except OSError:
            break
        if not data:
            break
        screen.extend(data)


def wait_for(predicate, label, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        drain()
        if predicate():
            return
        time.sleep(0.04)
    raise AssertionError(label)


def text():
    return re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(screen))


def send(data):
    os.write(master, data)
    time.sleep(0.15)
    drain()


def panes():
    return cli("list-panes", "-t", session + ":0", "-F", "#{pane_id}").splitlines()


def reap():
    global pid
    if pid is None:
        return
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        drain()
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            pid = None
            assert os.waitstatus_to_exitcode(status) == 0, status
            return
        time.sleep(0.04)
    raise AssertionError("client exit stalled")


try:
    cli("new-session", "-d", "-s", session, "-x", "100", "-y", "30")
    cli("set-option", "-g", "status-keys", "emacs")
    cli("split-window", "-h", "-t", session + ":0")
    victim = cli("display-message", "-p", "-t", session + ":0", "#{pane_id}")
    index = cli("display-message", "-p", "-t", victim, "#{pane_index}")
    before = panes()
    pid, master = pty.fork()
    if pid == 0:
        environment = dict(os.environ, TERM="xterm-256color")
        for name in ("TMUX", "TMUX_PANE", "ZZ_SOCKET", "ZZ_SESSION", "ZZ_PANE"):
            environment.pop(name, None)
        os.execvpe(prefix[0], [*prefix, "attach-session", "-t", "=" + session], environment)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    wait_for(lambda: bool(cli("list-clients", "-t", "=" + session, "-F", "#{client_name}")), "attach")
    time.sleep(0.5)
    screen.clear()
    send(b"\x02x")
    prompt = f"kill-pane {index}? (y/n)".encode()
    wait_for(lambda: prompt in text(), "prefix x prompt")
    print("confirm prompt=kill-pane #P? (y/n)")
    send(b"n")
    assert panes() == before, "n killed a pane"
    print("confirm n=alive")
    screen.clear()
    send(b"\x02x")
    wait_for(lambda: prompt in text(), "second prefix x prompt")
    send(b"y")
    wait_for(lambda: victim not in panes(), "y did not kill pane")
    assert len(panes()) == len(before) - 1
    print("confirm y=killed")
    output = work / "paste"
    command = shlex.join(["python3", str(Path(__file__).resolve()), "record", str(output)])
    cli("respawn-pane", "-k", "-t", panes()[0], command)
    screen.clear()
    wait_for(lambda: b"PASTE_READY" in text(), "paste recorder")
    cli("set-buffer", "KEYS_PASTE")
    send(b"\x02]")
    wait_for(output.exists, "paste bytes")
    data = output.read_bytes()
    assert data == b"\x1b[200~KEYS_PASTE\x1b[201~", data
    print("paste=" + data.hex())
    send(b"\x02d")
    wait_for(lambda: not cli("list-clients", "-t", "=" + session, "-F", "#{client_name}"), "prefix d detach")
    reap()
    print("detach clients=0 exit=0")
finally:
    if pid:
        os.kill(pid, signal.SIGKILL)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if os.waitpid(pid, os.WNOHANG)[0]:
                break
            time.sleep(0.04)
    if master is not None:
        os.close(master)
    subprocess.run([*prefix, "kill-session", "-t", "=" + session], capture_output=True, timeout=15)
