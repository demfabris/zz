import ctypes
import errno
import os
import sys
from dataclasses import dataclass

MACOS = sys.platform == "darwin"
LINUX = sys.platform.startswith("linux")


@dataclass
class Sample:
    cpu_ns: int
    child_cpu_ns: int
    footprint: int
    rss: int
    threads: int
    wakeups: int
    instructions: int


if MACOS:
    _libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    _libsys = ctypes.CDLL("/usr/lib/libSystem.B.dylib")

    class _RusageV4(ctypes.Structure):
        _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [
            (name, ctypes.c_uint64)
            for name in (
                "user_time system_time pkg_idle_wkups interrupt_wkups pageins wired_size "
                "resident_size phys_footprint proc_start_abstime proc_exit_abstime "
                "child_user_time child_system_time child_pkg_idle_wkups child_interrupt_wkups "
                "child_pageins child_elapsed_abstime diskio_bytesread diskio_byteswritten "
                "qos_default qos_maintenance qos_background qos_utility qos_legacy "
                "qos_user_initiated qos_user_interactive billed_system_time serviced_system_time "
                "logical_writes lifetime_max_phys_footprint instructions cycles billed_energy "
                "serviced_energy interval_max_phys_footprint runnable_time"
            ).split()
        ]

    class _TaskInfo(ctypes.Structure):
        _fields_ = [
            (name, ctypes.c_uint64)
            for name in "virtual_size resident_size total_user total_system threads_user threads_system".split()
        ] + [
            (name, ctypes.c_int32)
            for name in (
                "policy faults pageins cow_faults messages_sent messages_received "
                "syscalls_mach syscalls_unix csw threadnum numrunning priority"
            ).split()
        ]

    class _Timebase(ctypes.Structure):
        _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]

    _tb = _Timebase()
    _libsys.mach_timebase_info(ctypes.byref(_tb))
    _NUMER, _DENOM = _tb.numer, _tb.denom
    _RUSAGE_INFO_V4 = 4
    _PROC_PIDTASKINFO = 4
    _PROC_PIDLISTTHREADIDS = 28
    _PROC_PIDLISTTHREADS = 6

    def _ticks(value):
        return value * _NUMER // _DENOM

    def sample(pid):
        ru = _RusageV4()
        if _libproc.proc_pid_rusage(int(pid), _RUSAGE_INFO_V4, ctypes.byref(ru)) != 0:
            raise ProcessLookupError(pid)
        ti = _TaskInfo()
        got = _libproc.proc_pidinfo(int(pid), _PROC_PIDTASKINFO, ctypes.c_uint64(0), ctypes.byref(ti), ctypes.sizeof(ti))
        threads = ti.threadnum if got == ctypes.sizeof(ti) else 0
        return Sample(
            cpu_ns=_ticks(ru.user_time + ru.system_time),
            child_cpu_ns=_ticks(ru.child_user_time + ru.child_system_time),
            footprint=ru.phys_footprint,
            rss=ru.resident_size,
            threads=threads,
            wakeups=ru.interrupt_wkups + ru.pkg_idle_wkups,
            instructions=ru.instructions,
        )

    _thread_flavor = None

    def thread_ids(pid):
        global _thread_flavor
        buf = (ctypes.c_uint64 * 4096)()
        flavors = [_thread_flavor] if _thread_flavor else [_PROC_PIDLISTTHREADIDS, _PROC_PIDLISTTHREADS]
        for flavor in flavors:
            got = _libproc.proc_pidinfo(int(pid), flavor, ctypes.c_uint64(0), ctypes.byref(buf), ctypes.sizeof(buf))
            if got > 0:
                _thread_flavor = flavor
                return set(buf[: got // 8])
        return set()

    def thread_ids_unique():
        return _thread_flavor == _PROC_PIDLISTTHREADIDS

    HAS_INSTRUCTIONS = True
    INSTRUCTIONS_SOURCE = "ri_instructions"
    CPU_SOURCE = "proc_pid_rusage"

elif LINUX:
    _CLK = os.sysconf("SC_CLK_TCK")
    _libc = ctypes.CDLL(None, use_errno=True)
    _libc.syscall.restype = ctypes.c_long
    _PERF_EVENT_OPEN = {"x86_64": 298, "aarch64": 241}.get(os.uname().machine)
    _PERF_INHERIT, _PERF_EXCLUDE_KERNEL, _PERF_EXCLUDE_HV, _PERF_INHERIT_THREAD = 1 << 1, 1 << 5, 1 << 6, 1 << 35
    _PERF_FLAG_FD_CLOEXEC = 8

    class _PerfEventAttr(ctypes.Structure):
        _fields_ = [
            ("type", ctypes.c_uint32),
            ("size", ctypes.c_uint32),
            ("config", ctypes.c_uint64),
            ("sample_period", ctypes.c_uint64),
            ("sample_type", ctypes.c_uint64),
            ("read_format", ctypes.c_uint64),
            ("flags", ctypes.c_uint64),
            ("wakeup_events", ctypes.c_uint32),
            ("bp_type", ctypes.c_uint32),
            ("config1", ctypes.c_uint64),
        ]

    def _open_counter(tid):
        if _PERF_EVENT_OPEN is None:
            raise OSError(0, f"no perf_event_open syscall number for {os.uname().machine}")
        attr = _PerfEventAttr(
            type=0,
            size=ctypes.sizeof(_PerfEventAttr),
            config=1,
            flags=_PERF_INHERIT | _PERF_INHERIT_THREAD | _PERF_EXCLUDE_KERNEL | _PERF_EXCLUDE_HV,
        )
        fd = _libc.syscall(
            ctypes.c_long(_PERF_EVENT_OPEN),
            ctypes.byref(attr),
            ctypes.c_int(tid),
            ctypes.c_int(-1),
            ctypes.c_int(-1),
            ctypes.c_ulong(_PERF_FLAG_FD_CLOEXEC),
        )
        if fd < 0:
            err = ctypes.get_errno()
            raise OSError(err, os.strerror(err))
        return fd

    def _instructions_probe():
        try:
            os.close(_open_counter(0))
            return True, "perf_event_open user-space instructions"
        except OSError as e:
            return False, f"none: perf_event_open failed ({e.strerror}, perf_event_paranoid {_paranoid()})"

    def _paranoid():
        try:
            with open("/proc/sys/kernel/perf_event_paranoid") as f:
                return f.read().strip()
        except OSError:
            return "?"

    HAS_INSTRUCTIONS, INSTRUCTIONS_SOURCE = _instructions_probe()
    _counters = {}

    def _instructions(pid, start):
        key = (pid, start)
        fds = _counters.get(key)
        if fds is None:
            for stale in [k for k in _counters if not alive(k[0])]:
                for fd in _counters.pop(stale):
                    os.close(fd)
            fds = []
            for tid in os.listdir(f"/proc/{pid}/task"):
                try:
                    fds.append(_open_counter(int(tid)))
                except OSError as e:
                    if e.errno != errno.ESRCH:
                        for fd in fds:
                            os.close(fd)
                        raise
            _counters[key] = fds
        return sum(int.from_bytes(os.read(fd, 8), sys.byteorder) for fd in fds)

    class _Timespec(ctypes.Structure):
        _fields_ = [("tv_sec", ctypes.c_long), ("tv_nsec", ctypes.c_long)]

    def _clock_cpu_ns(pid):
        clock = ctypes.c_int()
        if _libc.clock_getcpuclockid(int(pid), ctypes.byref(clock)) != 0:
            return None
        ts = _Timespec()
        if _libc.clock_gettime(clock.value, ctypes.byref(ts)) != 0:
            return None
        return ts.tv_sec * 1_000_000_000 + ts.tv_nsec

    def _schedstat_ns(pid):
        total = 0
        try:
            tids = os.listdir(f"/proc/{pid}/task")
        except OSError:
            return None
        for tid in tids:
            try:
                with open(f"/proc/{pid}/task/{tid}/schedstat") as f:
                    total += int(f.read().split()[0])
            except (OSError, ValueError, IndexError):
                pass
        return total

    try:
        CPU_SOURCE = "clock_getcpuclockid" if _clock_cpu_ns(os.getpid()) is not None else "schedstat"
    except AttributeError:
        CPU_SOURCE = "schedstat"

    def _status(pid):
        out = {}
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                key, _, value = line.partition(":")
                out[key] = value.strip()
        return out

    def _kb(value):
        return int(value.split()[0]) * 1024 if value else 0

    def _footprint(pid, status):
        try:
            with open(f"/proc/{pid}/smaps_rollup") as f:
                fields = {}
                for line in f:
                    key, _, value = line.partition(":")
                    if value.strip().endswith("kB"):
                        fields[key] = _kb(value.strip())
            if "Pss_Anon" in fields:
                return fields["Pss_Anon"] + fields.get("Pss_Shmem", 0) + fields.get("SwapPss", 0)
        except OSError:
            pass
        return _kb(status.get("RssAnon", "0 kB")) + _kb(status.get("VmSwap", "0 kB"))

    def _switches(pid):
        total = 0
        try:
            tids = os.listdir(f"/proc/{pid}/task")
        except OSError:
            return 0
        for tid in tids:
            try:
                with open(f"/proc/{pid}/task/{tid}/status") as f:
                    for line in f:
                        if line.startswith(("voluntary_ctxt_switches", "nonvoluntary_ctxt_switches")):
                            total += int(line.split(":")[1])
            except OSError:
                pass
        return total

    def sample(pid):
        try:
            with open(f"/proc/{pid}/stat") as f:
                stat = f.read()
            status = _status(pid)
        except OSError:
            raise ProcessLookupError(pid)
        fields = stat[stat.rindex(")") + 2 :].split()
        utime, stime, cutime, cstime = (int(x) for x in fields[11:15])
        tick_ns = 1_000_000_000 // _CLK
        cpu_ns = _clock_cpu_ns(pid) if CPU_SOURCE == "clock_getcpuclockid" else None
        if cpu_ns is None:
            cpu_ns = _schedstat_ns(pid)
        if cpu_ns is None:
            cpu_ns = (utime + stime) * tick_ns
        return Sample(
            cpu_ns=cpu_ns,
            child_cpu_ns=(cutime + cstime) * tick_ns,
            footprint=_footprint(pid, status),
            rss=_kb(status.get("VmRSS", "0 kB")),
            threads=int(status.get("Threads", "0")),
            wakeups=_switches(pid),
            instructions=_instructions(pid, fields[19]) if HAS_INSTRUCTIONS else 0,
        )

    def thread_ids(pid):
        try:
            return {int(t) for t in os.listdir(f"/proc/{pid}/task")}
        except OSError:
            return set()

    def thread_ids_unique():
        return False

else:
    raise SystemExit("bench/perf supports macOS and Linux only")


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
