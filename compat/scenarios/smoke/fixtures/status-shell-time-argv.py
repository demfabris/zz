import json
import os
import pathlib
import shlex
import subprocess
import sys
import tempfile
import time

sys.stdout.reconfigure(line_buffering=True)

with tempfile.TemporaryDirectory(prefix="zz-status-time-argv-") as directory:
    root = pathlib.Path(directory)
    environment = dict(os.environ, HOME=directory, XDG_CONFIG_HOME=directory)
    for name in ("TMUX", "TMUX_PANE", "ZZ_SOCKET", "ZZ_SESSION", "ZZ_PANE"):
        environment.pop(name, None)
    pin = sys.argv[1]
    server = [pin, "-L", f"zzprobe-time-argv-{os.getpid()}", "-f", "/dev/null"]
    recorder = [pin, "-L", f"zzprobe-time-rec-{os.getpid()}", "-f", "/dev/null"]

    def run(arguments, check=True):
        return subprocess.run(arguments, env=environment, capture_output=True, timeout=10, check=check)

    recorded = root / "argv"
    script = root / "job"
    script.write_text(f"printf '%s\\n' \"$1\" >> {shlex.quote(str(recorded))}\nprintf '%s' \"$1\"\n")
    try:
        run([*server, "new-session", "-d", "-s", "probe", "sleep 60"])
        run([*server, "set-option", "-g", "status-interval", "1"])
        run([*server, "set-option", "-g", "status-right-length", "80"])
        run([*server, "set-option", "-g", "status-right", f"ARG[#(sh {shlex.quote(str(script))} %s%N)]"])
        command = shlex.join(["env", "-u", "TMUX", "-u", "TMUX_PANE", *server, "attach-session", "-t", "probe"])
        run([*recorder, "new-session", "-d", "-x", "120", "-y", "30", "-s", "recorder", command])
        deadline = time.monotonic() + 6
        while True:
            arguments = recorded.read_text().splitlines() if recorded.exists() else []
            unique = list(dict.fromkeys(arguments))
            if len(unique) >= 2:
                break
            if time.monotonic() >= deadline:
                raise AssertionError(("two time-expanded shell arguments", arguments))
            time.sleep(0.025)
        assert all(value.endswith("%N") and value[:-2].isdigit() for value in unique), unique
        print(json.dumps({"shell_arguments": arguments, "strftime_before_shell": True}))
    finally:
        run([*recorder, "kill-server"], check=False)
        run([*server, "kill-server"], check=False)
