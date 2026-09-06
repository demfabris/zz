import json
import os
import pathlib
import re
import select
import shlex
import subprocess
import sys
import tempfile
import time

sys.stdout.reconfigure(line_buffering=True)
SIDE = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
PIN = os.environ["ZZ_SMOKE_TMUX_BIN"]
ROOT = pathlib.Path(tempfile.mkdtemp(prefix="zz-notify-"))
BASE = ([os.environ["ZZ_SMOKE_ZZ_BIN"], "--socket", str(ROOT / "s")]
        if SIDE == "zz" else [PIN, "-L", f"zzprobe-notify-{os.getpid()}"])
BASE += ["-f", "/dev/null"]
ENV = dict(os.environ)
for key in ("TMUX", "TMUX_PANE", "ZZ_SOCKET", "ZZ_SESSION", "ZZ_PANE"):
    ENV.pop(key, None)
TRACES = {}


def cli(*args, check=True):
    result = subprocess.run([*BASE, *args], capture_output=True, env=ENV, timeout=15)
    if check and result.returncode:
        raise AssertionError((args, result.returncode, result.stderr))
    return result.stdout.decode().strip()


class Control:
    def __init__(self, session):
        self.process = subprocess.Popen([*BASE, "-C", "attach-session", "-t", session],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, env=ENV)
        self.lines = []
        self.pending = b""
        self.serial = 0
        self.guard = None
        self.until(lambda: any(line.startswith("%session-changed ") for line in self.lines))
        self.sync()
        self.cursor = 0

    def read(self, timeout):
        if select.select([self.process.stdout], [], [], timeout)[0]:
            data = os.read(self.process.stdout.fileno(), 65536)
            if not data:
                return False
            self.pending += data
            while b"\n" in self.pending:
                line, self.pending = self.pending.split(b"\n", 1)
                decoded = line.decode("utf-8", "backslashreplace")
                if decoded.startswith("%begin "):
                    if self.guard is not None:
                        raise AssertionError(("nested control block", decoded, self.guard))
                    self.guard = decoded.split()[1:]
                elif decoded.startswith(("%end ", "%error ")):
                    if self.guard != decoded.split()[1:]:
                        raise AssertionError(("mismatched control block", decoded, self.guard))
                    self.guard = None
                self.lines.append(decoded)
            return True
        return False

    def until(self, predicate, timeout=8):
        deadline = time.monotonic() + timeout
        while not predicate():
            if time.monotonic() >= deadline:
                raise AssertionError(("control wait", self.lines[-20:]))
            self.read(min(0.05, deadline - time.monotonic()))

    def send(self, command):
        self.process.stdin.write((command + "\n").encode())
        self.process.stdin.flush()

    def sync(self):
        self.serial += 1
        marker = f"NOTIFY_BARRIER_{self.serial}"
        self.send("display-message -p " + marker)
        self.until(lambda: marker in self.lines and any(
            line.startswith("%end ") for line in self.lines[self.lines.index(marker) + 1:]))
        while self.read(0):
            pass

    def phase(self, name, command=None, actions=(), until=None):
        start = self.cursor
        self.sync()
        if command:
            self.send(command)
        if command and actions:
            self.sync()
        for action in actions:
            cli(*action)
        if SIDE == "zz" and name in {"linked-window-add", "linked-window-close"}:
            until = "unsupported command:"
        if until:
            self.until(lambda: any(until in line for line in self.lines[start:]))
        if name != "exit":
            self.sync()
        lines = self.lines[start:]
        self.cursor = len(self.lines)
        blocks = []
        active = []
        for line in lines:
            if line.startswith("%begin "):
                active = [line]
            elif active:
                active.append(line)
                if line.startswith(("%end ", "%error ")):
                    if any(value.startswith("NOTIFY_BARRIER_") for value in active):
                        blocks.extend(value for value in active[1:-1] if not value.startswith("NOTIFY_BARRIER_"))
                    else:
                        blocks.extend(active)
                    active = []
            else:
                blocks.append(line)
        blocks.extend(active)
        clean = []
        for line in blocks:
            if line.startswith("NOTIFY_BARRIER_"):
                continue
            line = re.sub(r"^%(begin|end|error) \d+ \d+ ", r"%\1 T B ", line)
            line = re.sub(r"(?:device|client)-\d+", "CLIENT", line)
            line = re.sub(r"(%extended-output %\d+) \d+ :", r"\1 AGE :", line)
            line = line.replace(str(ROOT), "FIXTURE")
            clean.append(line)
        TRACES[name] = clean
        return clean

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)


control = None
peer = None
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
try:
    main_ready = ROOT / "main-ready"
    cli("new-session", "-d", "-s", "notify-main", "-x", "80", "-y", "24", "stty -echo; : > " + shlex.quote(str(main_ready)) + "; exec cat")
    deadline = time.monotonic() + 8
    while not main_ready.exists():
        if time.monotonic() >= deadline:
            raise AssertionError("main pane did not disable echo")
        time.sleep(0.025)
    cli("set-option", "-g", "automatic-rename", "off")
    cli("rename-window", "-t", "notify-main:0", "main")
    control = Control("notify-main")
    control.phase("startup")
    target = cli("list-clients", "-F", "#{client_name}")
    control.phase("queries", "list-sessions -F '#{session_id} #{session_name}' ; list-windows -F '#{window_id} #{window_layout}' ; list-panes -F '#{pane_id} #{pane_width} #{pane_height}'")
    control.phase("error", "definitely-no-such-command")
    control.phase("resize", "refresh-client -C 100,30", until="%layout-change ")
    control.phase("live-layout", "list-windows -F '#{window_id} #{window_layout} #{window_visible_layout}'")
    ready = ROOT / "second-ready"
    control.phase("window-add", "new-window -d -n second " + shlex.quote("stty -echo; : > " + shlex.quote(str(ready)) + "; exec cat"), until="%window-add ")
    deadline = time.monotonic() + 8
    while not ready.exists():
        if time.monotonic() >= deadline:
            raise AssertionError("second pane did not disable echo")
        time.sleep(0.025)
    control.phase("window-renamed", "rename-window -t notify-main:1 renamed", until="%window-renamed ")
    control.phase("session-window-changed", "select-window -t notify-main:1", until="%session-window-changed ")
    control.phase("split", "split-window -h -t notify-main:1 'stty -echo; exec cat'", until="%layout-change ")
    control.phase("window-pane-changed", "select-pane -t notify-main:1.0", until="%window-pane-changed ")
    control.phase("output", actions=[("send-keys", "-t", "notify-main:1.0", "-l", "NOTIFY_OUTPUT"), ("send-keys", "-t", "notify-main:1.0", "Enter")], until="NOTIFY_OUTPUT\\015\\012")
    pane = cli("display-message", "-p", "-t", "notify-main:1.0", "#{pane_id}")
    control.phase("activity-watch", "set-window-option -t notify-main:0 monitor-activity on")
    control.phase("activity-output", actions=[("send-keys", "-t", "notify-main:0.0", "-l", "NOTIFY_ACTIVITY"), ("send-keys", "-t", "notify-main:0.0", "Enter")], until="NOTIFY_ACTIVITY\\015\\012")
    control.phase("activity-layout", "resize-window -t notify-main:0 -x 101 -y 31", until="%layout-change ")
    control.phase("pause", "refresh-client -A " + "'" + pane + ":pause'", until="%pause ")
    control.phase("continue", "refresh-client -A " + "'" + pane + ":continue'", until="%continue ")
    control.phase("extended-output", "refresh-client -f pause-after=10", actions=[("send-keys", "-t", pane, "-l", "NOTIFY_EXTENDED"), ("send-keys", "-t", pane, "Enter")], until="NOTIFY_EXTENDED\\015\\012")
    control.phase("paste-buffer-changed", "set-buffer -b notify-buffer data", until="%paste-buffer-changed ")
    control.phase("paste-buffer-deleted", "delete-buffer -b notify-buffer", until="%paste-buffer-deleted ")
    control.phase("subscription-changed", "refresh-client -B 'notify::#{session_name}'", until="%subscription-changed ")
    control.phase("subscription-remove", "refresh-client -B notify")
    control.phase("environment-set", "set-environment -g NOTIFY_ENV EXPANDED")
    control.phase("environment-expansion", 'display-message -p "$NOTIFY_ENV"')
    control.phase("wait-output", "run-shell 'printf NOTIFY_WAIT'", until="NOTIFY_WAIT")
    control.phase("hook-set", "set-hook -g after-rename-window 'display-message -p NOTIFY_HOOK'")
    control.phase("hook-output", "rename-window -t notify-main:0 hooked", until="NOTIFY_HOOK")
    control.phase("hook-remove", "set-hook -gu after-rename-window")
    control.phase("percent-word", "refresh-client -A " + pane + ":pause")
    control.phase("percent-recover", "refresh-client -A '" + pane + ":continue'")
    control.phase("message", actions=[("display-message", "-c", target, "NOTIFY_MESSAGE")], until="%message ")
    bad = ROOT / "invalid.conf"
    bad.write_text("definitely-no-such-command\n")
    control.phase("config-error", "source-file " + shlex.quote(str(bad)))
    control.phase("pane-mode-changed", "copy-mode -t " + pane, until="%pane-mode-changed ")
    control.phase("pane-mode-ended", "send-keys -X -t " + pane + " cancel", until="%pane-mode-changed ")
    control.phase("sessions-changed", "new-session -d -s notify-other -n other 'stty -echo; exec cat'", until="%sessions-changed")
    control.phase("unlinked-window-renamed", "rename-window -t notify-other:0 outside", until="%unlinked-window-renamed ")
    control.phase("linked-window-add", "link-window -s notify-main:0 -t notify-other:1", until="%window-add ")
    control.phase("linked-window-close", "unlink-window -t notify-other:1", until="window-close ")
    control.phase("session-renamed", "rename-session -t notify-other renamed-other", until="%session-renamed ")
    old_clients = set(cli("list-clients", "-F", "#{client_name}").splitlines())
    peer = Control("notify-main")
    peer_name, = set(cli("list-clients", "-F", "#{client_name}").splitlines()) - old_clients
    control.phase("client-session-changed", actions=[("switch-client", "-c", peer_name, "-t", "renamed-other")], until="%client-session-changed ")
    control.phase("client-detached", actions=[("detach-client", "-t", peer_name)], until="%client-detached ")
    peer.close()
    peer = None
    control.phase("session-changed", "switch-client -t renamed-other", until="%session-changed ")
    control.phase("live-layout-after-switch", "list-windows -F '#{window_id} #{window_layout} #{window_visible_layout}'")
    control.phase("unlinked-window-close", "kill-window -t notify-main:1", until="%unlinked-window-close ")
    control.phase("exit", "detach-client", until="%exit")
    result = "clean"
except Exception as error:
    print(repr(error))
finally:
    if peer:
        peer.close()
    if control:
        control.close()
    cli("kill-server", check=False)
    destination = os.environ.get("ZZ_NOTIFY_RECORD")
    if destination:
        pathlib.Path(destination).write_text(json.dumps(TRACES, indent=2) + "\n")
    else:
        expected = json.loads((pathlib.Path(__file__).with_name("control-notify.json")).read_text())[SIDE]
        if TRACES != expected:
            print(json.dumps(TRACES, indent=2))
            result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")
    if not destination and "ZZ_SMOKE_CANARY" in os.environ:
        subprocess.run(["tmux", "set-environment", "-g", "ZZ_CONTROL_NOTIFY", result], capture_output=True, timeout=15)
