import fcntl
import os
import pathlib
import pty
import shutil
import struct
import subprocess
import sys
import termios
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


def settle(probe, want, seconds=10):
    deadline = time.monotonic() + seconds
    seen = None
    while time.monotonic() < deadline:
        seen = probe()
        if seen == want:
            return seen
        time.sleep(0.03)
    raise AssertionError((want, seen))


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
home = pathlib.Path(os.environ["HOME"])
root = home / ("yank-runtime-" + side)
shutil.rmtree(root, ignore_errors=True)
(root / "bin").mkdir(parents=True)
plugin = home / ".tmux/plugins/tmux-yank"
reader = home / "plugin-reader.py"
session = "yank-runtime"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

clipboard = root / "clipboard.txt"
fake_clip = root / "bin/fake-clip"
fake_clip.write_text("#!/bin/sh\ncat > %s\n" % clipboard)
fake_clip.chmod(0o755)

try:
    tmux("set-option", "-g", "status", "off")
    tmux("set-option", "-g", "@override_copy_command", str(fake_clip))
    tmux("set-window-option", "-g", "mode-keys", "emacs")
    script = root / "pane.sh"
    script.write_text("printf 'alpha beta gamma\\r'\nexec python3 %s %s\n"
                      % (reader, root / "pane.hex"))
    tmux("new-session", "-d", "-s", session, "-n", "yank", "-x", "80", "-y", "24",
         "-c", str(root), "exec sh " + str(script))
    pane = tmux("display-message", "-p", "-t", session + ":0.0", "#{pane_id}")
    client, master = attach(session, 100, 30)
    settle(lambda: tmux("display-message", "-p", "-t", session,
                        "#{session_attached}"), "1")
    tmux("set-window-option", "-t", session + ":0", "window-size", "manual")
    tmux("resize-window", "-t", session + ":0", "-x", "80", "-y", "24")
    settle(lambda: tmux("display-message", "-p", "-t", pane,
                        "#{pane_width}x#{pane_height}"), "80x24")
    settle(lambda: tmux("display-message", "-p", "-t", pane,
                        "#{cursor_x},#{cursor_y},#{pane_in_mode}"), "0,0,0")
    print("YANK_PANE_ROW=" + tmux("capture-pane", "-p", "-t", pane).splitlines()[0])

    environment = dict(os.environ)
    environment["TMUX_PANE"] = pane
    environment["ZZ_PANE"] = pane
    copy_line = subprocess.run(
        ["bash", str(plugin / "scripts/copy_line.sh")],
        capture_output=True, timeout=60, env=environment)
    print("YANK_COPY_LINE_EXIT=%d" % copy_line.returncode)
    print("YANK_COPY_LINE_STDERR=" + copy_line.stderr.decode().strip())

    settle(lambda: clipboard.exists() and clipboard.stat().st_size > 0, True)
    print("YANK_CLIPBOARD_HEX=" + clipboard.read_bytes().hex())
    print("YANK_CLIPBOARD_TEXT=" + clipboard.read_text())
    print("YANK_BUFFERS=" + tmux("list-buffers", "-F", "#{buffer_sample}"))
    print("YANK_PANE_MODE=" + tmux("display-message", "-p", "-t", pane,
                                   "#{pane_in_mode},#{pane_mode}"))
    print("YANK_DISPLAY_TIME=" + tmux("show-options", "-gv", "display-time"))
    result = "clean:copy-pipe-and-cancel"
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
    tmux("set-environment", "-g", "ZZ_YANK_RUNTIME", result)
