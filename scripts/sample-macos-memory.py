#!/usr/bin/env python3

from __future__ import annotations

import argparse
import csv
import ctypes
import os
import subprocess
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

from _instruments import duration_seconds, fail, use_summary_name


use_summary_name("zz memory capture")


class RusageInfoV0(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64)
        for name in (
            "user_time",
            "system_time",
            "pkg_idle_wkups",
            "interrupt_wkups",
            "pageins",
            "wired_size",
            "resident_size",
            "phys_footprint",
            "proc_start_abstime",
            "proc_exit_abstime",
        )
    ]


def positive_pid(value: str) -> int:
    pid = int(value)
    if not 0 < pid <= 2**31 - 1:
        raise argparse.ArgumentTypeError("PID must be a positive 32-bit integer")
    return pid


def process_tree(roots: dict[int, str]) -> dict[int, tuple[int, str, str]]:
    result = subprocess.run(
        ["ps", "-axo", "pid=,ppid=,comm="],
        check=True,
        capture_output=True,
        text=True,
    )
    processes = {}
    for line in result.stdout.splitlines():
        fields = line.split(maxsplit=2)
        if len(fields) == 3:
            processes[int(fields[0])] = (int(fields[1]), fields[2])
    owned = {
        pid: (processes[pid][0], role, processes[pid][1])
        for pid, role in roots.items()
        if pid in processes
    }
    while True:
        children = {
            pid: (parent, owned[parent][1].removesuffix("-child") + "-child", name)
            for pid, (parent, name) in processes.items()
            if pid not in owned and parent in owned and pid != os.getpid()
        }
        if not children:
            return owned
        owned.update(children)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Sample macOS physical footprint and RSS every 250 ms without stopping attached processes."
    )
    parser.add_argument("--pid", type=positive_pid, required=True)
    parser.add_argument("--daemon-pid", type=positive_pid)
    parser.add_argument("--duration", default="60s")
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    duration = duration_seconds(arguments.duration)
    if sys.platform != "darwin":
        fail("macOS is required")
    if arguments.pid == arguments.daemon_pid:
        fail("GUI and daemon PIDs must differ")

    library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    library.proc_pid_rusage.argtypes = [
        ctypes.c_int,
        ctypes.c_int,
        ctypes.POINTER(RusageInfoV0),
    ]
    library.proc_pid_rusage.restype = ctypes.c_int

    def read_usage(pid: int) -> RusageInfoV0:
        usage = RusageInfoV0()
        if library.proc_pid_rusage(pid, 0, ctypes.byref(usage)) != 0:
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error))
        if usage.proc_exit_abstime:
            raise OSError("process exited")
        return usage

    roots = {arguments.pid: "gui"}
    if arguments.daemon_pid is not None:
        roots[arguments.daemon_pid] = "daemon"
    identities = {}
    for pid in roots:
        try:
            identities[pid] = read_usage(pid).proc_start_abstime
        except OSError as error:
            fail(f"cannot read PID {pid}: {error}")

    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    samples = defaultdict(list)
    errors = defaultdict(int)
    start = time.monotonic()
    started_at = datetime.now(timezone.utc).isoformat()
    interrupted = False
    print(f"zz memory capture: sampling into {output}", flush=True)
    with (output / "memory.csv").open("w", newline="", encoding="utf-8") as file:
        writer = csv.writer(file)
        writer.writerow(
            ["elapsed_seconds", "pid", "ppid", "role", "command", "rss_bytes", "physical_footprint_bytes", "error"]
        )
        try:
            while time.monotonic() - start < duration:
                elapsed = time.monotonic() - start
                root_usage = {}
                for pid, role in list(roots.items()):
                    try:
                        usage = read_usage(pid)
                        if usage.proc_start_abstime != identities[pid]:
                            raise OSError("PID was reused")
                        root_usage[pid] = usage
                    except OSError as error:
                        errors[role] += 1
                        writer.writerow([f"{elapsed:.6f}", pid, "", role, "", "", "", str(error)])
                        del roots[pid]
                owned = process_tree(roots)
                totals = defaultdict(lambda: [0, 0])
                for pid, (parent, role, command) in sorted(owned.items()):
                    try:
                        usage = root_usage[pid] if pid in root_usage else read_usage(pid)
                    except OSError as error:
                        errors[role] += 1
                        writer.writerow([f"{elapsed:.6f}", pid, parent, role, command, "", "", str(error)])
                        continue
                    rss = usage.resident_size
                    footprint = usage.phys_footprint
                    writer.writerow([f"{elapsed:.6f}", pid, parent, role, command, rss, footprint, ""])
                    totals[role][0] += rss
                    totals[role][1] += footprint
                for role, values in totals.items():
                    samples[role].append(values)
                file.flush()
                if not roots:
                    print("zz memory capture: attached processes exited", file=sys.stderr)
                    break
                next_sample = (int((time.monotonic() - start) / 0.25) + 1) * 0.25
                time.sleep(max(0, min(next_sample, duration) - (time.monotonic() - start)))
        except KeyboardInterrupt:
            interrupted = True

    lines = [
        f"Started: {started_at}",
        f"Duration: {time.monotonic() - start:.2f}s; interval: 250ms",
        "Per-role totals, MiB; min / max / first / last",
    ]
    for role, values in sorted(samples.items()):
        for index, metric in enumerate(("RSS", "physical footprint")):
            series = [sample[index] / 2**20 for sample in values]
            lines.append(
                f"  {role} {metric}: {min(series):.2f} / {max(series):.2f} / "
                f"{series[0]:.2f} / {series[-1]:.2f} ({len(series)} samples)"
            )
    for role, count in sorted(errors.items()):
        lines.append(f"  {role}: {count} unavailable process samples; see CSV errors")
    if interrupted:
        lines.append("Capture interrupted; partial results saved.")
    summary = "\n".join(lines) + "\n"
    (output / "memory-summary.txt").write_text(summary, encoding="utf-8")
    print(summary, end="")


if __name__ == "__main__":
    main()
