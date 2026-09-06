import fcntl
import os
import pathlib
import pty
import re
import shutil
import struct
import termios
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


def attach(session, columns, rows):
    pid, master = pty.fork()
    if pid == 0:
        os.execvp("tmux", ["tmux", "attach-session", "-t", session])
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    return pid, master


def settle(probe, want, seconds=20):
    deadline = time.monotonic() + seconds
    seen = None
    while time.monotonic() < deadline:
        seen = probe()
        if seen == want:
            return seen
        time.sleep(0.05)
    raise AssertionError((want, seen))


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
home = pathlib.Path(os.environ["HOME"])
root = home / ("continuum-runtime-" + side)
shutil.rmtree(root, ignore_errors=True)
root.mkdir(parents=True)
saved = root / "saved"
continuum = home / ".tmux/plugins/tmux-continuum"
resurrect = home / ".tmux/plugins/tmux-resurrect"
session = "continuum-runtime"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

try:
    tmux("set-option", "-g", "@continuum-restore", "off")
    tmux("set-option", "-g", "@continuum-save-interval", "15")
    tmux("set-option", "-g", "@resurrect-dir", str(saved))
    tmux("set-option", "-g", "status-left", "")
    tmux("set-option", "-g", "status-right", "#{continuum_status}")
    script = root / "pane.sh"
    script.write_text("printf '\\033]2;CONTINUUM\\007'\nexec sleep 600\n")
    tmux("new-session", "-d", "-s", session, "-n", "continuum", "-x", "80", "-y", "24",
         "-c", str(root), "exec sh " + str(script))
    tmux("set-window-option", "-t", session + ":0", "automatic-rename", "off")
    settle(lambda: tmux("display-message", "-p", "-t", session + ":0.0",
                        "#{pane_title}"), "CONTINUUM")
    tmux("run-shell", str(resurrect / "resurrect.tmux"))
    tmux("run-shell", str(continuum / "continuum.tmux"))

    print("CONTINUUM_STATUS_RIGHT=" + tmux("show-options", "-gv", "status-right")
          .replace(str(continuum), "<PLUGIN>"))
    print("CONTINUUM_SAVE_SCRIPT=" + tmux("show-options", "-gqv",
                                          "@resurrect-save-script-path")
          .replace(str(resurrect), "<PLUGIN>"))

    tmux("set-option", "-g", "@continuum-save-last-timestamp", "0")
    tmux("set-option", "-g", "status", "on")
    tmux("set-option", "-g", "status-interval", "1")
    client, master = attach(session, 100, 30)
    settle(lambda: tmux("display-message", "-p", "-t", session,
                        "#{session_attached}"), "1")
    for interval in ("15", "0"):
        tmux("set-option", "-g", "@continuum-save-interval", interval)
        indicator = subprocess.run(
            ["bash", str(continuum / "scripts/continuum_status.sh")],
            capture_output=True, timeout=30)
        print("CONTINUUM_STATUS_TEXT[%s]=%s" % (interval,
                                                indicator.stdout.decode().strip()))
    tmux("set-option", "-g", "@continuum-save-interval", "15")
    stamp = settle(lambda: tmux("show-options", "-gqv",
                                "@continuum-save-last-timestamp") != "0", True)
    print("CONTINUUM_SAVE_TIMESTAMP_ADVANCED=%s" % stamp)
    last = saved / "last"
    settle(last.exists, True)
    filename = os.readlink(last)
    print("CONTINUUM_SAVE_FILE=%s" % bool(
        re.fullmatch(r"tmux_resurrect_\d{8}T\d{6}\.txt", filename)))
    records = [line for line in last.read_text().splitlines()
               if line.startswith("pane\t" + session + "\t")]
    print("CONTINUUM_SAVE_PANES=" + "\n".join(
        record.replace(str(root), "<DIR>") for record in records))
    result = "clean:status-job-and-save-trigger"
except Exception as error:
    print(repr(error), flush=True)
finally:
    subprocess.run(["tmux", "kill-session", "-t", "=" + session],
                   capture_output=True, timeout=15)
    try:
        os.close(master)
        os.waitpid(client, 0)
    except (NameError, OSError, ChildProcessError):
        pass
    tmux("set-environment", "-g", "ZZ_CONTINUUM_RUNTIME", result)
