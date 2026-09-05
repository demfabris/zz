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
mode = os.environ.get("KEYS_CONTRACT_MODE", "prefix")
session = "keyscontract"
created_sessions = [session]
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
    raise AssertionError(label + ": " + repr(text()[-800:]))


def text():
    visible = re.sub(rb"\x1b\].*?(?:\x07|\x1b\\)", b"", bytes(screen), flags=re.S)
    return re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", visible)


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
    if mode == "find-window":
        client = cli("list-clients", "-t", "=" + session, "-F", "#{client_name}")
        cli("new-window", "-t", session, "-n", "KEYS_FIND_SENTINEL")
        cli("select-window", "-t", session + ":0")
        screen.clear()
        send(b"\x02f")
        wait_for(lambda: b"(find-window)" in text(), "find-window prompt")
        send(b"KEYS_FIND_SENTINEL\r")
        time.sleep(0.5)
        print("after-submit table=" + cli("display-message", "-p", "-c", client, "#{client_key_table}"))
        print("after-submit pane_in_mode=" + cli("display-message", "-p", "-c", client, "#{pane_in_mode}"))
        send(b"\x1b[B\r")
        time.sleep(0.5)
        selected = cli("display-message", "-p", "-c", client, "#{window_index}")
        print("after-select window_index=" + selected)
        assert selected == ("0" if "--socket" in prefix else "1"), selected
        send(b"\x02d")
        reap()
    elif mode == "activity":
        client = cli("list-clients", "-t", "=" + session, "-F", "#{client_name}")
        def fact(value, target=None):
            args = ["display-message", "-p", "-c", client]
            if target is not None:
                args.extend(["-t", target])
            return cli(*args, "#{" + value + "}")
        cli("new-window", "-t", session, "-n", "alerted")
        cli("select-window", "-t", session + ":0")
        cli("set-window-option", "-t", session + ":1", "monitor-activity", "on")
        cli("respawn-pane", "-k", "-t", session + ":1", 'printf "ALERT_READY\\n"; exec cat')
        wait_for(lambda: fact("window_activity_flag", session + ":1") == "1", "next activity flag")
        send(b"\x02\x1bn")
        time.sleep(0.5)
        selected = fact("window_index")
        print("M-n activity=1 window_index=" + selected)
        assert selected == ("0" if "--socket" in prefix else "1"), selected
        cli("select-window", "-t", session + ":1")
        cli("set-window-option", "-t", session + ":0", "monitor-activity", "on")
        cli("respawn-pane", "-k", "-t", victim, 'printf "ALERT_PREVIOUS\\n"; exec cat')
        wait_for(lambda: fact("window_activity_flag", session + ":0") == "1", "previous activity flag")
        send(b"\x02\x1bp")
        time.sleep(0.5)
        selected = fact("window_index")
        print("M-p activity=1 window_index=" + selected)
        assert selected == ("1" if "--socket" in prefix else "0"), selected
        send(b"\x02d")
        reap()
    elif mode == "lifecycle":
        client = cli("list-clients", "-t", "=" + session, "-F", "#{client_name}")
        target = panes()[0]
        cli("select-pane", "-t", target)
        cli("resize-pane", "-t", target, "-x", "30")
        def width():
            return cli("display-message", "-p", "-t", target, "#{pane_width}")
        def table():
            return cli("display-message", "-p", "-c", client, "#{client_key_table}")
        def capture():
            return cli("capture-pane", "-p", "-t", target)
        wait_for(lambda: width() == "30", "initial pane width")
        cli("bind-key", "-T", "resize", "Left", "resize-pane", "-L", "5")
        cli("bind-key", "-T", "resize", "Right", "resize-pane", "-R", "5")
        cli("bind-key", "-n", "M-r", "switch-client", "-T", "resize")
        send(b"\x1br")
        wait_for(lambda: table() == "resize", "custom table armed")
        send(b"\x1b[D")
        wait_for(lambda: table() == "root", "bound key reset")
        print("bound table=" + table() + " width=" + width())
        assert width() == "25", width()
        cli("respawn-pane", "-k", "-t", target, 'stty -echo -icanon; printf "KEYS_READY\\n"; exec cat')
        wait_for(lambda: "KEYS_READY" in capture(), "shell ready")
        send(b"CONTROL_Z\n")
        wait_for(lambda: "CONTROL_Z" in capture(), "root key delivery canary")
        send(b"\x1br")
        wait_for(lambda: table() == "resize", "custom table rearmed")
        send(b"a")
        wait_for(lambda: table() == "root", "unbound reset")
        send(b"\n")
        assert "a" not in capture(), capture()
        print("unbound table=root a=dropped root-canary=delivered")
        cli("bind-key", "-n", "b", "set-option", "-g", "@root-retry", "fired")
        send(b"\x1br")
        wait_for(lambda: table() == "resize", "retry table armed")
        send(b"b")
        wait_for(lambda: cli("show-options", "-gqv", "@root-retry") == "fired", "retry in root")
        assert table() == "root", table()
        print("unbound root-binding=fired table=root")
        cli("set-option", "-g", "repeat-time", "1500")
        cli("bind-key", "-r", "-T", "resize", "Right", "resize-pane", "-R", "5")
        send(b"\x1br")
        wait_for(lambda: table() == "resize", "repeat table armed")
        send(b"\x1b[C")
        wait_for(lambda: width() == "30", "first repeat resize")
        assert table() == "resize", table()
        send(b"\x1b[C")
        wait_for(lambda: width() == "35", "second repeat resize")
        assert table() == "resize", table()
        print("repeat table=resize widths=30,35")
        wait_for(lambda: table() == "root", "repeat timer reset", timeout=5)
        print("repeat expired=root")
        send(b"\x02d")
        reap()
    elif mode == "shift":
        client = cli("list-clients", "-t", "=" + session, "-F", "#{client_name}")
        cli("new-window", "-t", session, "-n", "second")
        cli("select-window", "-t", session + ":1")
        cli("bind-key", "-n", "S-Left", "previous-window")
        cli("bind-key", "-n", "S-Right", "next-window")
        def window():
            return cli("display-message", "-p", "-c", client, "#{window_index}")
        wait_for(lambda: window() == "1", "initial window")
        send(b"\x1b[1;2D")
        wait_for(lambda: window() == "0", "shift-left previous-window")
        print("S-Left window=0")
        send(b"\x1b[1;2C")
        wait_for(lambda: window() == "1", "shift-right next-window")
        print("S-Right window=1")
        keys = [
            ("S-Up", b"\x1b[1;2A"), ("S-Down", b"\x1b[1;2B"),
            ("S-Left", b"\x1b[1;2D"), ("S-Right", b"\x1b[1;2C"),
            ("BTab", b"\x1b[Z"),
            ("S-Home", b"\x1b[1;2H"), ("S-End", b"\x1b[1;2F"),
            ("S-PPage", b"\x1b[5;2~"), ("S-NPage", b"\x1b[6;2~"),
            ("S-Insert", b"\x1b[2;2~"), ("S-Delete", b"\x1b[3;2~"),
        ]
        for number, final in enumerate("PQRS", 1):
            keys.append((f"S-F{number}", f"\x1b[1;2{final}".encode()))
        for number, code in enumerate([15, 17, 18, 19, 20, 21, 23, 24], 5):
            keys.append((f"S-F{number}", f"\x1b[{code};2~".encode()))
        for name, data in keys:
            cli("bind-key", "-n", name, "set-option", "-g", "@shift-key", name)
            cli("set-option", "-g", "@shift-key", "pending")
            send(data)
            wait_for(lambda: cli("show-options", "-gqv", "@shift-key") == name, name)
            print(name + " fired")
        send(b"\x02d")
        reap()
    else:
        client = cli("list-clients", "-t", "=" + session, "-F", "#{client_name}")
        def fact(value, target=None):
            args = ["display-message", "-p", "-c", client]
            if target is not None:
                args.extend(["-t", target])
            return cli(*args, "#{" + value + "}")
        def prefix_key(data):
            drain()
            screen.clear()
            send(b"\x02" + data)
        def in_output():
            if "--socket" in prefix:
                return fact("client_key_table") in ("copy-mode", "copy-mode-vi")
            return fact("pane_in_mode") == "1"
        def close_output():
            send(b"q")
            wait_for(lambda: not in_output(), "output mode closed")
        def output_key(data, marker):
            prefix_key(data)
            wait_for(in_output, "output view " + data.decode())
            send(b"G")
            wait_for(lambda: marker in text(), "output content " + repr(marker))
            close_output()
        cli("set-window-option", "-t", session + ":0", "mode-keys", "vi")
        cli("rename-window", "-t", session + ":0", "primary")
        prefix_key(b"&")
        wait_for(lambda: b"kill-window primary? (y/n)" in text(), "window confirmation")
        send(b"n")
        assert panes() == before, "n killed the window"
        print("& prompt=kill-window #W? (y/n) n=alive")
        cli("bind-key", "-n", "-N", "KEYS_NOTE_SENTINEL", "F12", "display-message", "note")
        output_key(b"?", b"KEYS_NOTE_SENTINEL")
        print("? output-mode=1 note=KEYS_NOTE_SENTINEL")
        cli("set-buffer", "-b", "KEYS_BUFFER", "BUFFER_SENTINEL")
        output_key(b"#", b"KEYS_BUFFER")
        print("# output-mode=1 buffer=KEYS_BUFFER")
        cli("set-buffer", "AUTOMATIC_SENTINEL")
        assert len(cli("list-buffers", "-F", "#{buffer_name}").splitlines()) == 2
        prefix_key(b"-")
        wait_for(lambda: cli("list-buffers", "-F", "#{buffer_name}") == "KEYS_BUFFER", "delete newest automatic buffer")
        print("- buffers=2->1 named=kept")
        prefix_key(b"m")
        wait_for(lambda: fact("pane_marked") == "1", "mark pane")
        print("m pane_marked=1")
        prefix_key(b"M")
        wait_for(lambda: fact("pane_marked") == "0", "clear marked pane")
        print("M pane_marked=0")
        cli("rename-window", "-t", session + ":0", "KEYS_INFO_SENTINEL")
        def has_window_information():
            return any(" command: " not in line and "KEYS_INFO_SENTINEL" in line for line in cli("show-messages").splitlines())
        assert not has_window_information()
        prefix_key(b"i")
        wait_for(has_window_information, "window information message")
        print("i show-messages=KEYS_INFO_SENTINEL")
        send(b"\x1b")
        cli("display-message", "-c", client, "KEYS_MESSAGE_SENTINEL")
        time.sleep(0.2)
        send(b"\x1b")
        output_key(b"~", b"KEYS_MESSAGE_SENTINEL")
        print("~ output-mode=1 message=KEYS_MESSAGE_SENTINEL")
        cli("rename-window", "-t", session + ":0", "primary")
        cli("respawn-pane", "-k", "-t", victim, 'seq 1 100; printf "HISTORY_READY\\n"; exec cat')
        wait_for(lambda: "HISTORY_READY" in cli("capture-pane", "-p", "-t", victim), "history ready")
        prefix_key(b"\x1b[5~")
        wait_for(lambda: fact("pane_in_mode") == "1", "PPage copy mode")
        wait_for(lambda: int(fact("scroll_position") or "0") > 0, "PPage scroll")
        print("PPage pane_in_mode=1 scroll_position=positive")
        send(b"q")
        wait_for(lambda: fact("pane_in_mode") == "0", "copy mode closed")
        cli("new-window", "-t", session, "-n", "KEYS_FIND_SENTINEL")
        cli("select-window", "-t", session + ":0")
        prefix_key(b"f")
        wait_for(lambda: b"(find-window)" in text(), "find-window prompt")
        send(b"\x03")
        print("f prompt=find-window cancelled; execution remains open")
        cli("select-window", "-t", session + ":0")
        prefix_key(b"'")
        wait_for(lambda: b"index" in text(), "select-window prompt")
        send(b"1\r")
        wait_for(lambda: fact("window_index") == "1", "select-window result")
        print("' prompt=index selected=1")
        cli("select-window", "-t", session + ":0")
        original_window = fact("window_id")
        prefix_key(b".")
        wait_for(lambda: b"(move-window)" in text(), "move-window prompt")
        send((session + ":7\r").encode())
        wait_for(lambda: fact("window_index", original_window) == "7", "move-window result")
        print(". prompt=move-window moved=7")
        cli("move-window", "-s", original_window, "-t", session + ":0")
        cli("select-window", "-t", session + ":0")
        next_session = session + "-next"
        cli("new-session", "-d", "-s", next_session)
        created_sessions.append(next_session)
        prefix_key(b")")
        wait_for(lambda: fact("session_name") == next_session, "next session")
        print(") session=next")
        prefix_key(b"(")
        wait_for(lambda: fact("session_name") == session, "previous session")
        print("( session=original")
        prefix_key(b"L")
        wait_for(lambda: fact("session_name") == next_session, "last session")
        print("L session=next")
        cli("switch-client", "-c", client, "-t", session)
        cli("select-window", "-t", session + ":0")
        cli("select-pane", "-t", victim)
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
    for created in reversed(created_sessions):
        subprocess.run([*prefix, "kill-session", "-t", "=" + created], capture_output=True, timeout=15)
