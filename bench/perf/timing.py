import math
import os
import time

_devnull = None


def devnull():
    global _devnull
    if _devnull is None:
        _devnull = os.open("/dev/null", os.O_RDWR)
    return _devnull


def spawn_ms(argv, env):
    fd = devnull()
    start = time.perf_counter_ns()
    pid = os.posix_spawn(
        argv[0],
        argv,
        env,
        file_actions=[(os.POSIX_SPAWN_DUP2, fd, 0), (os.POSIX_SPAWN_DUP2, fd, 1), (os.POSIX_SPAWN_DUP2, fd, 2)],
    )
    _, status = os.waitpid(pid, 0)
    return (time.perf_counter_ns() - start) / 1e6, os.waitstatus_to_exitcode(status)


def quantile(sorted_values, q):
    if not sorted_values:
        return None
    if len(sorted_values) == 1:
        return sorted_values[0]
    pos = q * (len(sorted_values) - 1)
    lo = math.floor(pos)
    hi = math.ceil(pos)
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * (pos - lo)


def stats(values):
    if values is None:
        return None
    if not isinstance(values, (list, tuple)):
        values = [values]
    values = sorted(v for v in values if v is not None)
    if not values:
        return None
    return {
        "median": round(quantile(values, 0.5), 4),
        "p10": round(quantile(values, 0.1), 4),
        "p90": round(quantile(values, 0.9), 4),
        "p99": round(quantile(values, 0.99), 4),
        "min": round(values[0], 4),
        "max": round(values[-1], 4),
        "n": len(values),
    }


def loadavg():
    return [round(x, 2) for x in os.getloadavg()]
