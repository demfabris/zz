import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


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
root = home / ("nav-runtime-" + side)
shutil.rmtree(root, ignore_errors=True)
(root / "bin").mkdir(parents=True)
plugin = home / ".tmux/plugins/vim-tmux-navigator/vim-tmux-navigator.tmux"
reader = home / "plugin-reader.py"
session = "nav-runtime"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

python = shutil.which("python3")
for name in ("vim", "pager"):
    (root / "bin" / name).symlink_to(python)
left = root / "left.hex"
right = root / "right.hex"


def pane(name, sink):
    return "exec %s %s %s" % (root / "bin" / name, reader, sink)


try:
    tmux("set-option", "-g", "status", "off")
    tmux("new-session", "-d", "-s", session, "-n", "nav", "-x", "80", "-y", "24",
         "-c", str(root), pane("vim", left))
    tmux("split-window", "-h", "-t", session + ":0.0", "-c", str(root),
         pane("pager", right))
    settle(lambda: (left.with_suffix(".hex.ready").exists()
                    and right.with_suffix(".hex.ready").exists()), True)
    tmux("run-shell", str(plugin))

    rows = tmux("list-keys", "-T", "root", "-F", "#{key_string}\t#{key_command}")
    binding = [row.split("\t", 1)[1] for row in rows.splitlines()
               if row.split("\t", 1)[0] == "C-h"]
    if len(binding) != 1:
        raise AssertionError(("one root C-h binding", rows))
    binding = binding[0]
    print("NAV_BINDING=" + binding)

    names = {}
    for index in ("0", "1"):
        target = "%s:0.%s" % (session, index)
        tty = tmux("display-message", "-p", "-t", target, "#{pane_tty}")
        if not tty.startswith("/dev/pts/"):
            raise AssertionError(("pane_tty", index, tty))
        roster = subprocess.run(["ps", "-o", "state=", "-o", "comm=", "-t", tty],
                                capture_output=True, timeout=30).stdout.decode()
        names[index] = sorted(line.split()[-1] for line in roster.splitlines()
                              if line.strip())
    print("NAV_PANE0_PROCESSES=" + ",".join(names["0"]))
    print("NAV_PANE1_PROCESSES=" + ",".join(names["1"]))

    tmux("select-pane", "-t", session + ":0.1")
    tmux("run-shell", "-t", session + ":0.1", "-C", binding)
    print("NAV_NON_VIM_ACTIVE=" + settle(
        lambda: tmux("display-message", "-p", "-t", session, "#{pane_index}"), "0"))
    print("NAV_NON_VIM_PANE_BYTES=" + right.read_text())

    tmux("select-pane", "-t", session + ":0.0")
    tmux("run-shell", "-t", session + ":0.0", "-C", binding)
    print("NAV_VIM_PANE_BYTES=" + settle(lambda: left.read_text(), "08"))
    active = tmux("display-message", "-p", "-t", session, "#{pane_index}")
    if active != "0":
        raise AssertionError(("vim branch moved the active pane", active))
    print("NAV_VIM_ACTIVE=" + active)
    result = "clean:is-vim-branches-split"
except Exception as error:
    print(repr(error), flush=True)
finally:
    subprocess.run(["tmux", "kill-session", "-t", "=" + session],
                   capture_output=True, timeout=15)
    tmux("set-environment", "-g", "ZZ_NAV_RUNTIME", result)
