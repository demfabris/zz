import os
import pathlib
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=15)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stderr))
    return result.stdout.decode().strip()


def await_file(path):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.02)
    raise RuntimeError("missing marker " + str(path))


def facts(pane):
    return tmux("display-message", "-p", "-t", pane,
                "#{history_size} #{cursor_x} #{cursor_y} #{alternate_on}")


def await_facts(pane, expected):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        value = facts(pane)
        if value == expected:
            return value
        time.sleep(0.02)
    raise AssertionError((value, expected))


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
root = pathlib.Path(os.environ["HOME"]) / ("terminal-facts-" + side)
root.mkdir(exist_ok=True)
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
panes = []
fds = []
observed = []
try:
    tmux("set-option", "-g", "status", "off")
    for burst in (False, True):
        name = "factburst" if burst else "facts"
        work = root / name
        work.mkdir(exist_ok=True)
        fifo = work / "input"
        os.mkfifo(fifo)
        fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
        fds.append(fd)
        script = work / "pane.sh"
        script.write_text("""seq 1 100
if [ "$1" = burst ]; then printf '\\033[?1049h'; fi
: > "$2/ready"
while IFS= read -r action; do
    case "$action" in
        alt) printf '\\033[?1049h' ;;
        primary) printf '\\033[?1049l' ;;
        wrap) printf '%080d' 0 ;;
    esac
    : > "$2/$action"
done < "$2/input"
""")
        pane = tmux("new-window", "-d", "-P", "-F", "#{pane_id}", "-n", name,
                    f"sh {script} {'burst' if burst else 'plain'} {work}")
        panes.append(pane)
        tmux("resize-window", "-t", pane, "-x", "80", "-y", "24")
        await_file(work / "ready")
        observed.append(await_facts(pane, "77 0 23 1" if burst else "77 0 23 0"))
        if not burst:
            for action, expected_row in (("alt", "77 0 23 1"),
                                         ("primary", "77 0 23 0"),
                                         ("wrap", "77 80 23 0")):
                os.write(fd, (action + "\n").encode())
                await_file(work / action)
                observed.append(await_facts(pane, expected_row))
    expected = ["77 0 23 0", "77 0 23 1", "77 0 23 0", "77 80 23 0", "77 0 23 1"]
    if observed != expected:
        raise AssertionError((observed, expected))
    result = "clean:5:" + "|".join(observed)
except Exception as error:
    print(repr(error), flush=True)
finally:
    for pane in panes:
        tmux("kill-pane", "-t", pane)
    for fd in fds:
        os.close(fd)
    tmux("set-environment", "-g", "ZZ_TERMINAL_FACTS", result)
