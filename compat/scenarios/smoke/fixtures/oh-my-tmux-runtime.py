import fcntl
import os
import pathlib
import pty
import shutil
import struct
import subprocess
import sys
import termios
import threading
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=60)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


def attach(session, columns, rows):
    pid, master = pty.fork()
    if pid == 0:
        os.execvp("tmux", ["tmux", "attach-session", "-t", "=" + session])
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    drain = threading.Thread(target=drain_master, args=(master,), daemon=True)
    drain.start()
    return pid, master


def drain_master(master):
    while True:
        try:
            if not os.read(master, 65536):
                return
        except OSError:
            return


def settle(probe, want, seconds=25):
    deadline = time.monotonic() + seconds
    seen = None
    while time.monotonic() < deadline:
        seen = probe()
        if seen == want:
            return seen
        time.sleep(0.05)
    raise AssertionError((want, seen))


def environment_value(name):
    for row in tmux("show-environment", "-g").splitlines():
        if row.startswith(name + "="):
            return row[len(name) + 1:]
    return ""


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
home = pathlib.Path(os.environ["HOME"])
root = home / ("oh-my-tmux-runtime-" + side)
shutil.rmtree(root, ignore_errors=True)
(root / "bin").mkdir(parents=True)
session = "omt-runtime"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

clipboard = root / "clipboard.txt"
argv = root / "clipboard.argv"
username = root / "username.txt"
tool = "pbcopy" if shutil.which("pbcopy") else "xsel"
fake_tool = root / "bin" / tool
fake_tool.write_text("#!/bin/sh\nprintf '%s\\n' \"$*\" > "
                     + str(argv) + "\ncat >> " + str(clipboard) + "\n")
fake_tool.chmod(0o755)


def normalise(text):
    return (text.replace(str(root), "<DIR>")
                .replace(str(home), "<HOME>")
                .replace(os.environ.get("ZZ_SMOKE_ZZ_SOCKET", "\0"), "<SOCKET>"))


path = str(root / "bin") + ":" + os.environ.get("PATH", "/usr/bin:/bin")


def pin_environment():
    for name, value in (("PATH", path), ("XDG_SESSION_TYPE", "x11")):
        tmux("set-environment", "-g", name, value)
        for target in tmux("list-sessions", "-F", "#{session_id}").split():
            tmux("set-environment", "-t", target, name, value)


try:
    conf = home / ".tmux.conf"
    tmux("set-option", "-g", "status", "off")
    pin_environment()

    program = environment_value("TMUX_PROGRAM")
    socket = environment_value("TMUX_SOCKET")
    server_pid = tmux("display-message", "-p", "#{pid}")
    try:
        server_exe = os.readlink("/proc/%s/exe" % server_pid)
    except OSError:
        server_exe = ""
    roundtrip = subprocess.run(
        [program, "-S", socket, "display-message", "-p", "#{pid}"],
        capture_output=True, timeout=30)
    print("OMT_TMUX_PROGRAM_EXECUTABLE=%s" % os.access(program, os.X_OK))
    print("OMT_TMUX_PROGRAM_ANSWERS_SERVER_PID=%s"
          % (roundtrip.returncode == 0
             and roundtrip.stdout.decode().strip() == server_pid))
    print("OMT_TMUX_PROGRAM_IS_SERVER_EXE=%s"
          % (bool(server_exe) and program == server_exe))
    print("OMT_TMUX_SOCKET_IS_SOCKET_PATH=%s"
          % (socket == tmux("display-message", "-p", "#{socket_path}")))
    print("OMT_TMUX_CONF=" + normalise(environment_value("TMUX_CONF")))

    tmux("source-file", str(conf))

    def prefix_y():
        rows = tmux("list-keys", "-T", "prefix", "-F",
                    "#{key_string}\t#{key_command}")
        found = [row.split("\t", 1)[1] for row in rows.splitlines()
                 if row.split("\t", 1)[0] == "y"]
        return found[0] if len(found) == 1 else ""

    settle(lambda: tool in prefix_y(), True)
    print("OMT_PREFIX_Y_BINDING=" + normalise(prefix_y()))

    tmux("new-session", "-d", "-s", session, "-n", "omt", "-x", "80", "-y", "24",
         "-c", str(root), "exec sleep 600")
    pin_environment()
    pane = tmux("display-message", "-p", "-t", session, "#{pane_id}")
    window = tmux("display-message", "-p", "-t", session, "#{window_id}")
    client, master = attach(session, 80, 24)
    settle(lambda: tmux("display-message", "-p", "-t", session,
                        "#{session_attached}"), "1")
    tmux("set-window-option", "-t", window, "window-size", "manual")
    tmux("resize-window", "-t", window, "-x", "80", "-y", "24")
    settle(lambda: tmux("display-message", "-p", "-t", pane,
                        "#{pane_width}x#{pane_height}"), "80x24")

    tmux("set-buffer", "OH-MY-TMUX-PREFIX-Y")
    print("OMT_BUFFER_BEFORE=" + tmux("list-buffers", "-F", "#{buffer_sample}"))
    os.write(master, b"\x02y")

    settle(lambda: clipboard.exists() and clipboard.stat().st_size > 0, True)
    print("OMT_CLIPBOARD_HEX=" + clipboard.read_bytes().hex())
    print("OMT_CLIPBOARD_TEXT=" + clipboard.read_text())
    print("OMT_CLIPBOARD_TOOL=" + tool)
    print("OMT_CLIPBOARD_ARGV=" + argv.read_text().strip())
    print("OMT_BUFFER_AFTER=" + tmux("list-buffers", "-F", "#{buffer_sample}"))
    print("OMT_PANE_MODE=" + tmux("display-message", "-p", "-t", pane,
                                  "#{pane_in_mode},#{pane_mode}"))

    left = tmux("show-options", "-gv", "status-left")
    right = tmux("show-options", "-gv", "status-right")
    print("OMT_STATUS_LEFT=" + normalise(left))
    print("OMT_STATUS_RIGHT=" + normalise(right))
    helper = ("#(cut -c3- '" + str(conf) + "' | sh -s _username '#{pane_pid}' "
              "'#{b:pane_tty}' false '#D' | tee " + str(username) + ")")
    tmux("set-option", "-g", "status-interval", "1")
    tmux("set-option", "-g", "status-right", helper)
    tmux("set-option", "-g", "status", "on")
    settle(lambda: username.exists() and username.stat().st_size > 0, True)
    print("OMT_STATUS_HELPER_USERNAME=" + username.read_text().strip())
    replayed = root / "username-run-shell.txt"
    tmux("run-shell", "-t", pane,
         "cut -c3- '" + str(conf) + "' | sh -s _username '#{pane_pid}' "
         "'#{b:pane_tty}' false '#D' > " + str(replayed))
    settle(lambda: replayed.exists() and replayed.stat().st_size > 0, True)
    print("OMT_RUN_SHELL_USERNAME=" + replayed.read_text().strip())
    result = "clean:prefix-y-clipboard"
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
    tmux("set-environment", "-g", "ZZ_OH_MY_TMUX_RUNTIME", result)
