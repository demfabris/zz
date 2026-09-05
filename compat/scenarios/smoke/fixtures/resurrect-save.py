import os
import pathlib
import re
import shlex
import subprocess
import sys
import tarfile
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
root = pathlib.Path(os.environ["HOME"]) / ("resurrect-save-" + side)
root.mkdir(exist_ok=True)
saved = root / "saved"
plugin = pathlib.Path(os.environ["HOME"]) / ".tmux/plugins/tmux-resurrect"
fifo = root / "input"
os.mkfifo(fifo)
fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
session = "resurrect-save"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
try:
    script = root / "pane.sh"
    script.write_text("""IFS= read -r action < input
printf '\\033]2;RESURRECT\\007'
seq 1 100
: > ready
IFS= read -r action < input
""")
    tmux("set-option", "-g", "status", "off")
    tmux("set-option", "-g", "@resurrect-capture-pane-contents", "on")
    tmux("set-option", "-g", "@resurrect-dir", str(saved))
    tmux("run-shell", shlex.quote(str(plugin / "resurrect.tmux")))
    tmux("new-session", "-d", "-s", session, "-n", "resurrect", "-x", "80", "-y", "24",
         "-c", str(root), "exec sh " + shlex.quote(str(script)))
    pane = session + ":0.0"
    tmux("set-window-option", "-t", pane, "automatic-rename", "off")
    tmux("resize-window", "-t", pane, "-x", "80", "-y", "24")
    os.write(fd, b"go\n")
    deadline = time.monotonic() + 10
    observed = ""
    while time.monotonic() < deadline:
        observed = tmux("display", "-p", "-t", pane,
                        "#{history_size}|#{cursor_y}|#{pane_title}|#{pane_current_command}")
        if (root / "ready").exists() and observed == "77|23|RESURRECT|sh":
            break
        time.sleep(0.03)
    else:
        raise AssertionError(("settled pane", observed))
    tmux("run-shell", shlex.quote(str(plugin / "scripts/save.sh")) + " quiet")
    last = saved / "last"
    filename = os.readlink(last)
    if not re.fullmatch(r"tmux_resurrect_\d{8}T\d{6}\.txt", filename):
        raise AssertionError(("saved timestamp filename", filename))
    records = [line for line in last.read_text().splitlines()
               if line.startswith("pane\t" + session + "\t")]
    if len(records) != 1:
        raise AssertionError(("one saved pane", records))
    normalized = records[0].replace(str(root), "<DIR>")
    expected_record = "\t".join(["pane", session, "0", "1", ":*", "0", "RESURRECT",
                                  ":<DIR>", "1", "sh", ":"])
    if normalized != expected_record:
        raise AssertionError(("saved pane record", normalized, expected_record))
    with tarfile.open(saved / "pane_contents.tar.gz", "r:gz") as archive:
        member = archive.extractfile("./pane_contents/pane-" + pane)
        if member is None:
            raise AssertionError("missing captured pane")
        content = member.read()
    expected = "".join(f"{number}\n" for number in range(1, 101)).encode()
    if content != expected or len(content.splitlines()) != 100:
        raise AssertionError(("saved content", len(content.splitlines()), content))
    print("RESURRECT_FILE=tmux_resurrect_<timestamp>.txt")
    print("RESURRECT_PANE=" + normalized)
    print("RESURRECT_CONTENT_LINES=100")
    print("RESURRECT_CONTENT_BEGIN\n" + content.decode() + "RESURRECT_CONTENT_END")
    result = "clean:100-lines"
except Exception as error:
    print(repr(error), flush=True)
finally:
    subprocess.run(["tmux", "kill-session", "-t", "=" + session], capture_output=True, timeout=15)
    os.close(fd)
    tmux("set-environment", "-g", "ZZ_RESURRECT_SAVE", result)
