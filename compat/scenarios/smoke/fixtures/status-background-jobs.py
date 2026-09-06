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
root = pathlib.Path(os.environ["HOME"]) / ("status-background-" + side)
root.mkdir(exist_ok=True)
value = root / "value"
starts = root / "starts"
script = root / "job"
recorder = [os.environ["ZZ_SMOKE_TMUX_BIN"], "-L", f"zzprobe-status-{side}-{os.getpid()}",
            "-f", "/dev/null"]
session = "status-background"
started = False
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")


def screen():
    return run([*recorder, "capture-pane", "-p", "-t", "recorder"]).stdout


def await_condition(predicate, timeout, label):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        observed = predicate()
        if observed:
            return observed
        time.sleep(0.025)
    raise AssertionError((label, screen()))


try:
    value.write_text("slow\n")
    script.write_text(f"printf 'start\\n' >> {shlex.quote(str(starts))}\nsleep 3\ncat {shlex.quote(str(value))}\n")
    tmux("new-session", "-d", "-s", session, "sleep 600")
    job = f"#(sh {shlex.quote(str(script))})"
    for option, setting in (("status", "on"), ("status-interval", "1"),
                            ("status-keys", "emacs"), ("status-left", f"LEFT[{job}]"),
                            ("status-left-length", "40"), ("status-right-length", "40"),
                            ("status-right", f"RIGHT[{job}]")):
        tmux("set-option", "-t", session, option, setting)
    if side == "zz":
        attach = [os.environ["ZZ_SMOKE_ZZ_BIN"], "--socket", os.environ["ZZ_SMOKE_ZZ_SOCKET"]]
    else:
        attach = [os.environ["ZZ_SMOKE_TMUX_BIN"], "-f", "/dev/null", "-L", os.environ["ZZ_SMOKE_TMUX_LABEL"]]
    command = ["env", "-u", "TMUX", "-u", "TMUX_PANE", "-u", "ZZ_SOCKET",
               "-u", "ZZ_SESSION", "-u", "ZZ_PANE", "LC_ALL=en_US.UTF-8",
               "LANG=en_US.UTF-8", *attach, "attach-session", "-t", "=" + session]
    before_attach = time.monotonic()
    run([*recorder, "new-session", "-d", "-x", "120", "-y", "30", "-s", "recorder",
         shlex.join(command)])
    started = True
    await_condition(lambda: b"RIGHT[]" in screen(), 1.5, "first drawn expansion is empty")
    elapsed = time.monotonic() - before_attach
    if elapsed >= 1.5:
        raise AssertionError(("attach latency below 1.5 seconds", elapsed))
    await_condition(lambda: starts.exists(), 1, "job started")
    if starts.read_text().splitlines() != ["start"]:
        raise AssertionError(("same command in left and right shares a job", starts.read_text()))
    await_condition(lambda: b"RIGHT[slow]" in screen(), 10, "slow job completes without cancellation")
    value.write_text("changed\n")
    cached = screen()
    if b"RIGHT[slow]" not in cached:
        raise AssertionError(("last output retained during rerun", cached))
    await_condition(lambda: b"RIGHT[changed]" in screen(), 8, "interval reruns without forced refresh")
    if len(starts.read_text().splitlines()) < 2:
        raise AssertionError(("job reran", starts.read_text()))
    target = tmux("list-clients", "-t", session, "-F", "#{client_name}")
    value.write_text("forced\n")
    tmux("refresh-client", "-S", "-t", target)
    if b"RIGHT[changed]" not in screen():
        raise AssertionError(("forced refresh keeps last output", screen()))
    await_condition(lambda: b"RIGHT[forced]" in screen(), 8, "forced slow job completes")
    print("status jobs: attach below 1.5s; shared job; slow output; cached output; interval rerun")
    tags = root / "tags"
    tag_script = root / "tag-job"
    tag_script.write_text(f"printf 'start\\n' >> {shlex.quote(str(tags))}\nsleep 3\nprintf tag\n")
    tmux("new-window", "-d", "-t", session, "-n", "second", "sleep 600")
    tmux("set-option", "-t", session, "status-left", "")
    tmux("set-option", "-t", session, "status-right", "KEY#{W:[#(sh " + shlex.quote(str(tag_script)) + ")]}")
    await_condition(lambda: b"KEY[][]" in screen(), 1.5, "window loop first expansion")
    await_condition(lambda: tags.exists(), 1, "window loop jobs started")
    expected_jobs = 1 if side == "zz" else 2
    actual_jobs = len(tags.read_text().splitlines())
    if actual_jobs != expected_jobs:
        raise AssertionError(("window format job keys", actual_jobs, expected_jobs))
    print("KNOWN DIVERGENCE status-shell-jobs-format-key: two windows start pin=2 jobs, zz=1")
    wide_script = root / "wide-job"
    wide_script.write_text("printf '%05000dTAIL\\n' 0\n")
    tmux("set-option", "-t", session, "status-left-length", "1")
    tmux("set-option", "-t", session, "status-left", "#(sh " + shlex.quote(str(wide_script)) + ")")
    tmux("set-option", "-t", session, "status-right", "LIMIT[#{=-4:#{E:status-left}}]")
    await_condition(lambda: b"LIMIT[TAIL]" in screen(), 5, "long cached output retains its tail")
    print("status jobs: long cached output retains TAIL on both binaries")
    result = "clean:9"
except Exception as error:
    print(repr(error), flush=True)
finally:
    if started:
        run([*recorder, "kill-server"], check=False)
    run(["tmux", "kill-session", "-t", "=" + session], check=False)
    tmux("set-environment", "-g", "ZZ_STATUS_BACKGROUND_JOBS", result)
