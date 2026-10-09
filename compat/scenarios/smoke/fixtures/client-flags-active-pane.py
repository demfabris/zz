import os
import select
import subprocess
import sys
import time

if os.environ.get("ZZ_SMOKE_ZZ_BIN"):
    BASE = [os.environ["ZZ_SMOKE_ZZ_BIN"], "--socket", os.environ["ZZ_SMOKE_ZZ_SOCKET"]]
else:
    BASE = [os.environ["ZZ_SMOKE_TMUX_BIN"], "-L", os.environ["ZZ_SMOKE_TMUX_LABEL"]]
ENV = dict(os.environ)
for key in ("TMUX", "TMUX_PANE", "ZZ_SOCKET", "ZZ_SESSION", "ZZ_PANE"):
    ENV.pop(key, None)
ENV["LC_ALL"] = "C"
ENV.pop("LANG", None)

COMMANDS = [
    "display-message -p 'F1=#{client_flags}'",
    "refresh-client -f active-pane",
    "display-message -p 'F2=#{client_flags}'",
    "refresh-client -f '!ignore-size,!active-pane,no-detach-on-destroy'",
    "display-message -p 'F3=#{client_flags}'",
]

result = "failed"
process = subprocess.Popen(
    [*BASE, "-C", "attach-session", "-f", "active-pane,ignore-size", "-t", "w"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=ENV)
try:
    process.stdin.write("".join(command + "\n" for command in COMMANDS).encode())
    process.stdin.flush()
    pending = b""
    rows = []
    errors = 0
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and not any(row.startswith("F3=") for row in rows):
        if not select.select([process.stdout], [], [], 0.1)[0]:
            continue
        data = os.read(process.stdout.fileno(), 65536)
        if not data:
            break
        pending += data
        while b"\n" in pending:
            line, pending = pending.split(b"\n", 1)
            text = line.decode("utf-8", "backslashreplace")
            if text.startswith("%error "):
                errors += 1
            if text.startswith("F"):
                rows.append(text)
    result = "|".join(rows) + f"|errors={errors}"
finally:
    process.stdin.close()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
subprocess.run(["tmux", "set-environment", "-g", "CLIENT_FLAGS_ACTIVE_PANE", result],
               capture_output=True, timeout=15)
print(result, file=sys.stderr)
