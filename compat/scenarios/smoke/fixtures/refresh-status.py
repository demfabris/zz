import os
import pathlib
import shlex
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def run(args, check=True):
    result = subprocess.run(args, capture_output=True, timeout=15)
    if check and result.returncode:
        raise RuntimeError((args, result.returncode, result.stderr))
    return result


def tmux(*args):
    return run(["tmux", *args]).stdout.decode().strip()


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
root = pathlib.Path(os.environ["HOME"]) / ("refresh-status-" + side)
root.mkdir(exist_ok=True)
value = root / "value"
recorder = [os.environ["ZZ_SMOKE_TMUX_BIN"], "-L", f"zzprobe-refresh-{side}-{os.getpid()}",
            "-f", "/dev/null"]
session = "refresh-status"
started = False
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")


def screen():
    return run([*recorder, "capture-pane", "-p", "-t", "recorder"]).stdout


def await_marker(marker):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        captured = screen()
        if marker in captured:
            return
        time.sleep(0.05)
    raise AssertionError((marker, captured))


try:
    value.write_text("REFRESH-ONE\n")
    tmux("new-session", "-d", "-s", session, "sleep 600")
    for option, setting in (("status", "on"), ("status-interval", "0"),
                            ("status-keys", "emacs"), ("status-left", ""),
                            ("status-right-length", "40"),
                            ("status-right", f"#(cat {shlex.quote(str(value))})")):
        tmux("set-option", "-t", session, option, setting)
    if side == "zz":
        attach = [os.environ["ZZ_SMOKE_ZZ_BIN"], "--socket", os.environ["ZZ_SMOKE_ZZ_SOCKET"]]
    else:
        attach = [os.environ["ZZ_SMOKE_TMUX_BIN"], "-L", os.environ["ZZ_SMOKE_TMUX_LABEL"]]
    command = ["env", "-u", "TMUX", "-u", "TMUX_PANE", "-u", "ZZ_SOCKET",
               "-u", "ZZ_SESSION", "-u", "ZZ_PANE", "LC_ALL=en_US.UTF-8",
               "LANG=en_US.UTF-8", *attach, "attach-session", "-t", "=" + session]
    run([*recorder, "new-session", "-d", "-x", "100", "-y", "30", "-s", "recorder",
         shlex.join(command)])
    started = True
    await_marker(b"REFRESH-ONE")
    target = tmux("list-clients", "-t", session, "-F", "#{client_name}")
    if not target or "\n" in target:
        raise AssertionError(("one attached client", target))
    value.write_text("REFRESH-TWO\n")
    time.sleep(0.3)
    before = screen()
    if b"REFRESH-ONE" not in before or b"REFRESH-TWO" in before:
        raise AssertionError(("cached with interval zero", before))
    tmux("refresh-client", "-S", "-t", target)
    await_marker(b"REFRESH-TWO")
    value.write_text("REFRESH-THREE\n")
    tmux("refresh-client", "-t", target)
    await_marker(b"REFRESH-THREE")
    pan = run(["tmux", "refresh-client", "-U", "-t", target], check=False)
    if side == "zz":
        if pan.returncode == 0 or b"refresh-client interactive behavior" not in pan.stderr:
            raise AssertionError(("pan remains refused", pan.returncode, pan.stderr))
    elif pan.returncode != 0:
        raise AssertionError(("pin pan succeeds", pan.returncode, pan.stderr))
    result = "clean:5"
except Exception as error:
    print(repr(error), flush=True)
finally:
    if started:
        run([*recorder, "kill-server"], check=False)
    run(["tmux", "kill-session", "-t", "=" + session], check=False)
    tmux("set-environment", "-g", "ZZ_REFRESH_STATUS", result)
