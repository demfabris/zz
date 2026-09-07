import os
import pathlib
import shutil
import socket
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


def settle(probe, want, seconds=15):
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
root = home / ("pane-runtime-facts-" + side)
shutil.rmtree(root, ignore_errors=True)
root.mkdir(parents=True)
reader = home / "plugin-reader.py"
session = "pane-facts"
host = socket.gethostname()
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

sink0 = root / "sink0.hex"
sink1 = root / "sink1.hex"


def sink(path):
    return path.read_text() if path.exists() else ""


immediate = root / "immediate.sh"
immediate.write_text(
    "tmux display-message -p 'IMMEDIATE|#{pane_current_path}|[#{pane_path}]'"
    " > %s/immediate$1.out 2>&1\n"
    "exec sleep 600\n" % root)

spawn = root / "spawn.sh"
spawn.write_text(
    "tmux display-message -p 'FIRST|#{pane_current_path}|[#{pane_path}]' > %s\n"
    "while [ ! -f %s ]; do sleep 0.03; done\n"
    "printf '\\033]7;file://zzhost/tmp\\007'\n"
    "exec sleep 600\n" % (root / "first.out", root / "osc7.go"))

try:
    tmux("set-option", "-g", "status", "off")
    tmux("new-session", "-d", "-s", session, "-n", "facts", "-x", "80", "-y", "24",
         "-c", str(root), "exec python3 %s %s" % (reader, sink0))
    tmux("split-window", "-h", "-t", session + ":facts.0", "-c", str(root),
         "exec python3 %s %s" % (reader, sink1))
    settle(lambda: (root / "sink0.hex.ready").exists()
           and (root / "sink1.hex.ready").exists(), True)
    tmux("select-pane", "-t", session + ":facts.1")

    tmux("run-shell", "-t", session + ":facts.0", "-C", "send-keys -l A")
    settle(lambda: sink(sink1), "41")
    print("NESTED_RUN_SHELL_TARGET_SINK=[" + sink(sink0) + "]")
    print("NESTED_RUN_SHELL_CURRENT_SINK=[" + sink(sink1) + "]")

    tmux("if-shell", "-t", session + ":facts.0", "true", "send-keys -l B")
    settle(lambda: sink(sink1), "4142")
    print("NESTED_IF_SHELL_TARGET_SINK=[" + sink(sink0) + "]")
    print("NESTED_IF_SHELL_CURRENT_SINK=[" + sink(sink1) + "]")

    tty0 = tmux("display-message", "-p", "-t", session + ":facts.0", "#{pane_tty}")
    if not tty0.startswith("/dev/"):
        raise AssertionError(("pane_tty", tty0))
    tmux("if-shell", "-t", session + ":facts.0",
         "test '#{pane_tty}' = " + tty0, "send-keys -l C", "send-keys -l D")
    settle(lambda: len(sink(sink1)), 6)
    print("NESTED_IF_SHELL_CONDITION_SINK=[" + sink(sink1) + "]")

    tmux("run-shell", "-t", session + ":facts.0", "-C", "send-keys -l #{pane_index}")
    settle(lambda: len(sink(sink1)), 8)
    print("NESTED_RUN_SHELL_EXPANSION_SINK=[" + sink(sink1) + "]")
    print("NESTED_TARGET_SINK_TOTAL=[" + sink(sink0) + "]")

    tmux("new-window", "-t", session + ":", "-n", "title-exec", "-c", str(root),
         "exec sleep 600")
    exec_title = tmux("display-message", "-p", "-t", session + ":title-exec.0",
                      "#{pane_title}")
    print("SPAWN_TITLE_IS_HOSTNAME=" + str(exec_title == host))
    print("SPAWN_TITLE_T_MATCHES=" + str(
        tmux("display-message", "-p", "-t", session + ":title-exec.0", "#T")
        == exec_title))

    tmux("select-pane", "-t", session + ":title-exec.0", "-T", "chosen-title")
    print("SELECT_PANE_TITLE=" + settle(
        lambda: tmux("display-message", "-p", "-t", session + ":title-exec.0",
                     "#{pane_title}"), "chosen-title"))

    osc2 = root / "osc2.sh"
    osc2.write_text("printf '\\033]2;pane-osc2-title\\007'\nexec sleep 600\n")
    tmux("new-window", "-t", session + ":", "-n", "title-osc2", "-c", str(root),
         "exec sh " + str(osc2))
    print("OSC2_TITLE=" + settle(
        lambda: tmux("display-message", "-p", "-t", session + ":title-osc2.0",
                     "#{pane_title}"), "pane-osc2-title"))

    print("NEW_WINDOW_PRINT=" + tmux(
        "new-window", "-t", session + ":", "-n", "printed", "-c", str(root),
        "-P", "-F", "[#{pane_path}]|#{pane_title}",
        "exec sleep 600").replace(str(root), "<DIR>").replace(host, "<HOST>"))

    for index in (1, 2, 3):
        window = "%s:immediate%d" % (session, index)
        tmux("new-window", "-t", session + ":", "-n", "immediate%d" % index,
             "-c", str(root), "exec sh %s %d" % (immediate, index))
        print("IMMEDIATE%d_OUTSIDE=%s" % (index, tmux(
            "display-message", "-p", "-t", window + ".0",
            "#{pane_current_path}|[#{pane_path}]").replace(str(root), "<DIR>")))
    for index in (1, 2, 3):
        print("IMMEDIATE%d_INSIDE=%s" % (index, settle(
            lambda: (root / ("immediate%d.out" % index)).read_text().strip()
            if (root / ("immediate%d.out" % index)).exists() else "",
            "IMMEDIATE|" + str(root) + "|[]").replace(str(root), "<DIR>")))

    tmux("new-window", "-t", session + ":", "-n", "spawn", "-c", str(root),
         "exec sh " + str(spawn))
    first = settle(lambda: (root / "first.out").read_text().strip()
                   if (root / "first.out").exists() else "",
                   "FIRST|" + str(root) + "|[]")
    print("SPAWN_FIRST_FACTS=" + first.replace(str(root), "<DIR>"))
    print("PANE_PATH_BEFORE_OSC7=[" + tmux(
        "display-message", "-p", "-t", session + ":spawn.0", "#{pane_path}") + "]")
    (root / "osc7.go").write_text("1")
    print("PANE_PATH_AFTER_OSC7=[" + settle(
        lambda: tmux("display-message", "-p", "-t", session + ":spawn.0",
                     "#{pane_path}"), "file://zzhost/tmp") + "]")
    print("PANE_CURRENT_PATH_AFTER_OSC7=" + tmux(
        "display-message", "-p", "-t", session + ":spawn.0",
        "#{pane_current_path}").replace(str(root), "<DIR>"))
    tmux("set-window-option", "-g", "remain-on-exit", "on")
    tmux("new-window", "-t", session + ":", "-n", "dead", "-c", str(root),
         "exec sh -c 'exit 0'")
    settle(lambda: tmux("display-message", "-p", "-t", session + ":dead.0",
                        "#{pane_dead}"), "1")
    print("DEAD_PANE_CURRENT_PATH=[" + tmux(
        "display-message", "-p", "-t", session + ":dead.0",
        "#{pane_current_path}").replace(str(root), "<DIR>") + "]")
    print("DEAD_PANE_PATH=[" + tmux(
        "display-message", "-p", "-t", session + ":dead.0",
        "#{pane_path}") + "]")
    tmux("set-window-option", "-g", "remain-on-exit", "off")
    result = "clean:nested-target-title-default-spawn-path-dead"
except Exception as error:
    print(repr(error), flush=True)
finally:
    subprocess.run(["tmux", "kill-session", "-t", "=" + session],
                   capture_output=True, timeout=15)
    tmux("set-environment", "-g", "ZZ_PANE_RUNTIME_FACTS", result)
