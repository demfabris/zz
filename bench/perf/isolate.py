import os
import shutil
import signal
import subprocess
import tempfile
import time

import probe

HISTORY_LIMIT = 10000
TMUX_CANDIDATES = ("/opt/homebrew/bin/tmux", "/usr/local/bin/tmux", "/usr/bin/tmux")

SCRUB = ("TMUX", "TMUX_PANE", "ZZ_PANE", "ZZ_SESSION", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "COLORTERM")
KEEP = ("PATH", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE")


class Env:
    def __init__(self, keep=False):
        self.tag = f"zzpf-{os.getpid()}"
        self.root = tempfile.mkdtemp(prefix=f"{self.tag}-")
        self.keep = keep
        self.zz_socket = f"/tmp/{self.tag}.sock"
        home = os.path.join(self.root, "home")
        env = {k: os.environ[k] for k in KEEP if k in os.environ}
        env.setdefault("LANG", "en_US.UTF-8")
        env.update(
            HOME=home,
            XDG_CONFIG_HOME=os.path.join(home, ".config"),
            XDG_DATA_HOME=os.path.join(home, ".local/share"),
            XDG_STATE_HOME=os.path.join(home, ".local/state"),
            XDG_CACHE_HOME=os.path.join(home, ".cache"),
            XDG_RUNTIME_DIR=os.path.join(self.root, "run"),
            TMPDIR=os.path.join(self.root, "tmp") + "/",
            TMUX_TMPDIR=self.root,
            ZZ_DATA_DIR=os.path.join(self.root, "zzdata"),
            ZZ_LOG_DIR=os.path.join(self.root, "zzlog"),
            ZZ_SOCKET=self.zz_socket,
            ZZ_TRAY="0",
            ZZ_UPDATE_CHECK="0",
            SHELL="/bin/bash",
            TERM="xterm-256color",
            BASH_SILENCE_DEPRECATION_WARNING="1",
        )
        for key in SCRUB:
            env.pop(key, None)
        for path in (home, env["XDG_RUNTIME_DIR"], env["TMPDIR"], env["ZZ_DATA_DIR"], env["ZZ_LOG_DIR"]):
            os.makedirs(path, mode=0o700, exist_ok=True)
        self.config = os.path.join(self.root, "perf.conf")
        with open(self.config, "w") as f:
            f.write(f"set -g history-limit {HISTORY_LIMIT}\n")
        self.env = env
        self.muxes = []
        self.clients = set()
        self.tracked = set()
        self.orphans_killed = 0

    def path(self, *parts):
        return os.path.join(self.root, *parts)

    def strays(self):
        tagged = {pid for pid, _, _ in processes(self.tag) if pid != os.getpid()}
        return sorted(tagged | living(self.tracked))

    def cleanup(self):
        for pid in list(self.clients):
            try:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            except (ProcessLookupError, ChildProcessError):
                pass
        self.clients.clear()
        for mux in self.muxes:
            mux.kill()
        deadline = time.monotonic() + 3
        strays = self.strays()
        while strays and time.monotonic() < deadline:
            for pid in strays:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            time.sleep(0.1)
            strays = self.strays()
        for mux in self.muxes:
            mux.remove_socket()
        if not self.keep:
            shutil.rmtree(self.root, ignore_errors=True)
        return strays


class Mux:
    detach_keys = b"\x02d"

    def __init__(self, name, binary, env):
        self.name = name
        self.binary = binary
        self.envobj = env
        self.env = dict(env.env)
        self._pid = None
        env.muxes.append(self)

    def argv(self, *args):
        raise NotImplementedError

    def run(self, *args, check=False, timeout=60, env=None):
        proc = subprocess.run(
            self.argv(*args),
            env=env or self.env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if check and proc.returncode != 0:
            raise RuntimeError(f"{self.name} {' '.join(args)} exited {proc.returncode}: {proc.stderr.strip()[:300]}")
        return proc

    def out(self, *args):
        return self.run(*args, check=True).stdout.strip()

    def pid(self):
        if self._pid is None or not probe.alive(self._pid):
            self._pid = int(self.out("display-message", "-p", "#{pid}"))
        return self._pid

    def sample(self):
        return probe.sample(self.pid())

    def server_running(self):
        return bool(self.server_pids())

    def server_pids(self):
        raise NotImplementedError

    def kill(self):
        tree = descendants(self.server_pids())
        try:
            self.run("kill-server", timeout=10)
        except subprocess.TimeoutExpired:
            pass
        deadline = time.monotonic() + 5
        while self.server_pids() and time.monotonic() < deadline:
            time.sleep(0.02)
        for pid in self.server_pids():
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        survivors = living(tree)
        deadline = time.monotonic() + 1
        while survivors and time.monotonic() < deadline:
            time.sleep(0.05)
            survivors = living(survivors)
        for pid in survivors:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        if survivors:
            self.envobj.orphans_killed += len(survivors)
            time.sleep(0.1)
            self.envobj.tracked |= living(survivors)
        self._pid = None
        self.remove_socket()

    def remove_socket(self):
        path = self.socket_path()
        if path and os.path.exists(path) and not self.server_pids():
            try:
                os.unlink(path)
            except OSError:
                pass

    def attach_argv(self, target):
        return self.argv("attach-session", "-t", target)

    def control_argv(self, target):
        return self.argv("-C", "attach-session", "-t", target)


class Zz(Mux):
    def __init__(self, binary, env):
        super().__init__("zz", binary, env)
        self.socket = env.zz_socket

    def argv(self, *args):
        return [self.binary, "-f", self.envobj.config, *args]

    def socket_path(self):
        return self.socket

    def server_pids(self):
        return [pid for pid, ppid, _ in processes(f"--socket {self.socket} daemon") if ppid != os.getpid()]


class Tmux(Mux):
    def __init__(self, binary, env):
        super().__init__("tmux", binary, env)
        self.label = env.tag

    def argv(self, *args):
        return [self.binary, "-L", self.label, "-f", self.envobj.config, *args]

    def socket_path(self):
        return os.path.join(self.envobj.root, f"tmux-{os.getuid()}", self.label)

    def server_pids(self):
        return [pid for pid, ppid, _ in processes(f"-L {self.label} ") if ppid != os.getpid()]


def processes(needle):
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,args="], capture_output=True, text=True).stdout
    found = []
    for line in out.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and needle in parts[2]:
            found.append((int(parts[0]), int(parts[1]), parts[2]))
    return [p for p in found if p[0] != os.getpid()]


def process_table():
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,stat="], capture_output=True, text=True).stdout
    table = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 3:
            table[int(parts[0])] = (int(parts[1]), parts[2])
    return table


def descendants(roots):
    table = process_table()
    children = {}
    for pid, (ppid, _) in table.items():
        children.setdefault(ppid, []).append(pid)
    found = set()
    stack = list(roots)
    while stack:
        for child in children.get(stack.pop(), []):
            if child not in found and child != os.getpid():
                found.add(child)
                stack.append(child)
    return found


def living(pids):
    if not pids:
        return set()
    table = process_table()
    return {pid for pid in pids if pid in table and not table[pid][1].startswith("Z")}


def linked_libraries(binary):
    tool = ["otool", "-L", binary] if probe.MACOS else ["ldd", binary]
    try:
        return subprocess.run(tool, capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return ""


def symbols(binary):
    out = ""
    for tool in (["nm", "-D", binary], ["nm", binary]):
        try:
            out += subprocess.run(tool, capture_output=True, text=True, timeout=30).stdout
        except (OSError, subprocess.TimeoutExpired):
            pass
    return out


def is_asan(binary):
    libs = linked_libraries(binary)
    return "libclang_rt.asan" in libs or "libasan" in libs or "__asan_init" in symbols(binary)


def is_native_executable(path):
    try:
        with open(path, "rb") as f:
            magic = f.read(4)
    except OSError:
        return False
    return magic == b"\x7fELF" or magic in (
        b"\xfe\xed\xfa\xce",
        b"\xfe\xed\xfa\xcf",
        b"\xce\xfa\xed\xfe",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
    )


def tmux_version(path):
    try:
        return subprocess.run([path, "-V"], capture_output=True, text=True, timeout=10, stdin=subprocess.DEVNULL).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return ""


def check_tmux(path, zz_bin):
    real = os.path.realpath(path)
    if not os.access(real, os.X_OK):
        return None, f"{path}: not executable"
    if not is_native_executable(real):
        return None, f"{path} -> {real}: not a Mach-O or ELF executable (a wrapper script?)"
    if real == os.path.realpath(zz_bin):
        return None, f"{path} -> {real}: this is the zz binary"
    version = tmux_version(real)
    if not version.startswith("tmux ") or "-zz" in version:
        return None, f"{path} -> {real}: -V says {version!r}, not a release tmux"
    if is_asan(real):
        return None, f"{path} -> {real}: it links AddressSanitizer; use a release tmux"
    return real, None


def resolve_tmux(explicit, zz_bin):
    if explicit:
        real, why = check_tmux(explicit, zz_bin)
        return real, [why] if why else []
    tried = []
    seen = set()
    for candidate in (*TMUX_CANDIDATES, shutil.which("tmux")):
        if not candidate or not os.path.exists(candidate) or os.path.realpath(candidate) in seen:
            continue
        seen.add(os.path.realpath(candidate))
        real, why = check_tmux(candidate, zz_bin)
        if real:
            return real, tried
        tried.append(why)
    return None, tried or ["no tmux found in " + ", ".join(TMUX_CANDIDATES) + " or on PATH"]
