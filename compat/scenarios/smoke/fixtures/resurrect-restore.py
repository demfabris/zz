import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=60)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


def settle(probe, want, seconds=25):
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
root = home / ("resurrect-restore-" + side)
shutil.rmtree(root, ignore_errors=True)
saved = root / "saved"
plugin = home / ".tmux/plugins/tmux-resurrect"
session = "resurrect-restore"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

for name in ("alpha", "beta"):
    (root / name).mkdir(parents=True)


def pane_script(title):
    script = root / (title + ".sh")
    script.write_text("printf '\\033]2;%s\\007'\nexec sleep 600\n" % title)
    return "exec sh " + str(script)


def panes():
    rows = tmux("list-panes", "-t", session + ":0", "-F",
                "#{pane_index}|#{pane_title}|#{pane_current_path}")
    return "\n".join(row.replace(str(root), "<DIR>") for row in rows.splitlines())


try:
    tmux("set-option", "-g", "status", "off")
    tmux("set-option", "-g", "@resurrect-dir", str(saved))
    tmux("run-shell", str(plugin / "resurrect.tmux"))
    print("RESTORE_SAVE_SCRIPT=" + tmux("show-options", "-gqv",
                                        "@resurrect-save-script-path")
          .replace(str(plugin), "<PLUGIN>"))
    print("RESTORE_SCRIPT=" + tmux("show-options", "-gqv",
                                   "@resurrect-restore-script-path")
          .replace(str(plugin), "<PLUGIN>"))

    tmux("new-session", "-d", "-s", session, "-n", "restored", "-x", "80", "-y", "24",
         "-c", str(root / "alpha"), pane_script("ALPHA"))
    tmux("set-window-option", "-t", session + ":0", "automatic-rename", "off")
    tmux("split-window", "-t", session + ":0.0", "-c", str(root / "beta"),
         pane_script("BETA"))
    before = settle(panes, "0|ALPHA|<DIR>/alpha\n1|BETA|<DIR>/beta")
    print("RESTORE_BEFORE=" + before)

    tmux("run-shell", str(plugin / "scripts/save.sh") + " quiet")
    records = sorted(
        line.split("\t")[2] + ":" + line.split("\t")[5] + ":" + line.split("\t")[6]
        for line in (saved / "last").read_text().splitlines()
        if line.startswith("pane\t" + session + "\t"))
    print("RESTORE_SAVED_RECORDS=" + ",".join(records))

    tmux("kill-session", "-t", "=" + session)
    settle(lambda: session in tmux("list-sessions", "-F", "#{session_name}").split(),
           False)
    print("RESTORE_SESSION_GONE=True")

    subprocess.run(["tmux", "run-shell", str(plugin / "scripts/restore.sh")],
                   capture_output=True, timeout=120)
    settle(lambda: session in tmux("list-sessions", "-F", "#{session_name}").split(),
           True)
    settle(lambda: tmux("display-message", "-p", "-t", session,
                        "#{session_windows}"), "1")
    print("RESTORE_WINDOW=" + tmux("list-windows", "-t", session, "-F",
                                   "#{window_index}|#{window_name}|#{window_panes}"))
    print("RESTORE_AFTER=" + settle(panes, before))
    result = "clean:two-panes-restored"
except Exception as error:
    print(repr(error), flush=True)
finally:
    subprocess.run(["tmux", "kill-session", "-t", "=" + session],
                   capture_output=True, timeout=15)
    tmux("set-environment", "-g", "ZZ_RESURRECT_RESTORE", result)
