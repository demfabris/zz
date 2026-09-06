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
root = home / ("fpp-runtime-" + side)
shutil.rmtree(root, ignore_errors=True)
(root / "bin").mkdir(parents=True)
plugin = home / ".tmux/plugins/tmux-fpp/fpp.tmux"
session = "fpp-runtime"
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

fake = root / "bin/fpp"
fake.write_text(
    "#!/bin/sh\n"
    "cat > %s\n"
    "attempt=0\n"
    "while [ \"$attempt\" -lt 60 ]; do\n"
    "  seen=$(tmux display-message -p '#{window_name}|#{pane_current_path}' 2>&1)\n"
    "  case \"$seen\" in *'|/'*) printf '%%s\\n' \"$seen\" > %s; break;; esac\n"
    "  attempt=$((attempt + 1))\n"
    "  sleep 0.05\n"
    "done\n"
    % (root / "fpp.stdin", root / "fpp.here"))
fake.chmod(0o755)

try:
    tmux("set-option", "-g", "status", "off")
    tmux("set-option", "-g", "@fpp-path", str(fake))
    script = root / "pane.sh"
    script.write_text("printf 'FPP-LINE-ONE\\nFPP-LINE-TWO\\n'\nexec sleep 600\n")
    tmux("new-session", "-d", "-s", session, "-n", "source", "-x", "80", "-y", "24",
         "-c", str(root), "exec sh " + str(script))
    tmux("set-window-option", "-t", session + ":0", "automatic-rename", "off")
    settle(lambda: tmux("capture-pane", "-p", "-t", session + ":0.0").strip(),
           "FPP-LINE-ONE\nFPP-LINE-TWO")
    tmux("run-shell", str(plugin))

    rows = tmux("list-keys", "-T", "prefix", "-F", "#{key_string}\t#{key_command}")
    binding = [row.split("\t", 1)[1] for row in rows.splitlines()
               if row.split("\t", 1)[0] == "f"]
    if len(binding) != 1:
        raise AssertionError(("one prefix f binding", rows))
    binding = binding[0]
    print("FPP_BINDING=" + binding.replace(str(home), "<HOME>"))

    tmux("select-window", "-t", session + ":0")
    tmux("run-shell", "-t", session + ":0.0", "-C", binding)
    settle(lambda: (root / "fpp.here").read_text().strip()
           if (root / "fpp.here").exists() else "",
           "fpp|" + str(root))
    print("FPP_STDIN=" + (root / "fpp.stdin").read_text().strip()
          .replace("\n", " ; "))
    print("FPP_CONTEXT=" + (root / "fpp.here").read_text().strip()
          .replace(str(root), "<DIR>"))
    settle(lambda: tmux("list-windows", "-t", session, "-F",
                        "#{window_name}").split(), ["source"])
    print("FPP_WINDOW_CLOSED=True")
    result = "clean:prefix-f-new-window"
except Exception as error:
    print(repr(error), flush=True)
finally:
    subprocess.run(["tmux", "kill-session", "-t", "=" + session],
                   capture_output=True, timeout=15)
    tmux("set-environment", "-g", "ZZ_FPP_RUNTIME", result)
