"""Observe what an attached client does when the server ends its attachment.

Drives one real pty client per case against whichever multiplexer the smoke
fixture selected, then prints one observation line per case. The differential
compares those lines, so the shapes below are the pin's behaviour, not a
verdict this script invents.
"""

import fcntl
import os
import pty
import signal
import struct
import subprocess
import sys
import termios
import threading
import time

SESSION = "clientexit"
GOT_HANGUP = {"seen": False}


def on_hangup(_signum, _frame):
    GOT_HANGUP["seen"] = True


def cli(prefix, *args):
    return subprocess.run([*prefix, *args], capture_output=True, text=True)


def attach(prefix):
    argv = [*prefix, "attach-session", "-t", "=" + SESSION]
    pid, master = pty.fork()
    if pid == 0:
        environment = dict(os.environ)
        environment["TERM"] = "xterm-256color"
        for name in ("TMUX", "TMUX_PANE", "ZZ_SOCKET", "ZZ_SESSION", "ZZ_PANE"):
            environment.pop(name, None)
        os.execvpe(argv[0], argv, environment)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    recorded = bytearray()

    def drain():
        while True:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                break
            if not chunk:
                break
            recorded.extend(chunk)

    threading.Thread(target=drain, daemon=True).start()
    return pid, recorded


def clients(prefix):
    result = cli(prefix, "list-clients", "-t", "=" + SESSION, "-F", "x")
    return result.stdout.count("x")


def await_clients(prefix, count):
    for _ in range(400):
        if clients(prefix) == count:
            return True
        time.sleep(0.05)
    return False


def client_name(prefix):
    return cli(
        prefix, "list-clients", "-t", "=" + SESSION, "-F", "#{client_name}"
    ).stdout.strip()


def notice(recorded):
    text = bytes(recorded).decode("utf-8", "replace")
    hangup = "[detached and SIGHUP (from session %s)]" % SESSION
    plain = "[detached (from session %s)]" % SESSION
    if hangup in text:
        return "hangup"
    if plain in text:
        return "plain"
    if "[detached" in text:
        return "other"
    return "none"


def reap(pid):
    try:
        _, status = os.waitpid(pid, 0)
    except OSError:
        return "lost"
    if os.WIFEXITED(status):
        return "exit%d" % os.WEXITSTATUS(status)
    if os.WIFSIGNALED(status):
        return "signal%d" % os.WTERMSIG(status)
    return "unknown"


def observe(label, prefix, arguments, settle=True):
    GOT_HANGUP["seen"] = False
    pid, recorded = attach(prefix)
    if not await_clients(prefix, 1):
        print("%s attach-failed" % label)
        return
    time.sleep(0.4)
    name = client_name(prefix)
    result = cli(prefix, "detach-client", *[a.replace("@CLIENT@", name) for a in arguments])
    if settle:
        status = reap(pid)
        time.sleep(0.3)
        remaining = clients(prefix)
    else:
        time.sleep(0.8)
        remaining = clients(prefix)
        status = "attached"
        os.kill(pid, signal.SIGKILL)
        reap(pid)
    print(
        "%s rc=%d stderr=%s notice=%s hangup=%s status=%s clients=%d"
        % (
            label,
            result.returncode,
            result.stderr.strip() or "-",
            notice(recorded),
            GOT_HANGUP["seen"],
            status,
            remaining,
        )
    )


def main():
    work = sys.argv[1]
    prefix = sys.argv[2:]
    os.makedirs(work, exist_ok=True)
    signal.signal(signal.SIGHUP, on_hangup)

    cli(prefix, "kill-session", "-t", "=" + SESSION)
    cli(prefix, "new-session", "-d", "-s", SESSION)

    # An empty -E is server_client_exec's early return: nothing happens.
    observe("empty-exec", prefix, ["-E", "", "-t", "@CLIENT@"], settle=False)
    observe("plain", prefix, ["-t", "@CLIENT@"])
    observe("parent-hangup", prefix, ["-P", "-t", "@CLIENT@"])

    marker = os.path.join(work, "exec-marker")
    if os.path.exists(marker):
        os.remove(marker)
    observe(
        "exec",
        prefix,
        ["-E", "printf EXECMARK >%s; exit 7" % marker, "-t", "@CLIENT@"],
    )
    for _ in range(100):
        if os.path.exists(marker):
            break
        time.sleep(0.05)
    written = ""
    if os.path.exists(marker):
        with open(marker, "r") as handle:
            written = handle.read()
    print("exec-marker %s" % (written or "-"))

    # -E beats -P, the way cmd_detach_client_exec tests cmd before msgtype.
    observe("exec-over-hangup", prefix, ["-P", "-E", "exit 5", "-t", "@CLIENT@"])

    cli(prefix, "kill-session", "-t", "=" + SESSION)


if __name__ == "__main__":
    main()
