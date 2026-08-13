#!/usr/bin/env python3
"""Export a Time Profiler table and summarize zz-owned macOS processes."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

from _instruments import (
    duration_seconds,
    export_table,
    fail,
    read_metadata,
    use_summary_name,
)


use_summary_name("zz CPU summary")

TIME_PROFILE_SCHEMA = "time-profile"
PROCESS_SNAPSHOTS = (
    "processes-before.txt",
    "processes-capture.txt",
    "processes-after.txt",
)


@dataclass(frozen=True)
class ProcessSnapshot:
    pid: int
    parent_pid: int
    command_line: str


@dataclass(frozen=True)
class ProcessCell:
    pid: int
    formatted: str


@dataclass(frozen=True)
class FrameSymbol:
    name: str
    binary: str


def read_process_snapshots(run_dir: Path) -> dict[int, ProcessSnapshot]:
    snapshots: dict[int, ProcessSnapshot] = {}
    for name in PROCESS_SNAPSHOTS:
        path = run_dir / name
        if not path.is_file():
            continue
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            # The first six ps columns never contain spaces. Everything after
            # elapsed time is retained verbatim because macOS app paths do.
            columns = line.split(maxsplit=6)
            if len(columns) != 7:
                continue
            try:
                pid = int(columns[0])
                parent_pid = int(columns[1])
            except ValueError:
                continue
            snapshots[pid] = ProcessSnapshot(
                pid=pid,
                parent_pid=parent_pid,
                command_line=columns[6],
            )
    return snapshots


def process_role(
    process: ProcessSnapshot | None,
    pid: int,
    gui_pid: int,
    daemon_pid: int,
) -> str:
    if pid == gui_pid:
        return "gui"
    if pid == daemon_pid:
        return "daemon"
    command_line = process.command_line.lower() if process is not None else ""
    if "--type=gpu-process" in command_line:
        return "cef-gpu"
    if "--type=renderer" in command_line:
        return "cef-renderer"
    if (
        "--type=utility" in command_line
        and "network.mojom.networkservice" in command_line
    ):
        return "cef-network"
    if "--type=utility" in command_line:
        return "cef-utility"
    if "--type=" in command_line or "zz helper" in command_line:
        return "cef-other"
    return "owned-child"


def resolve_reference(
    element: ET.Element,
    values: dict[str, object],
    path: Path,
) -> object:
    if element.tag == "sentinel":
        return ()
    reference = element.attrib.get("ref")
    if reference is not None:
        try:
            return values[reference]
        except KeyError:
            fail(f"{path} contains an unresolved XML reference {reference}")
    identifier = element.attrib.get("id")
    if identifier is not None and identifier in values:
        return values[identifier]
    fail(f"{path} contains an unresolved {element.tag} value")


def parse_time_profile(
    path: Path,
    owned_pids: set[int],
) -> tuple[
    Counter[int],
    Counter[tuple[int, str]],
    dict[int, Counter[FrameSymbol]],
    dict[int, Counter[FrameSymbol]],
]:
    fields: list[str] = []
    values: dict[str, object] = {}
    process_weights: Counter[int] = Counter()
    thread_weights: Counter[tuple[int, str]] = Counter()
    leaf_weights: dict[int, Counter[FrameSymbol]] = {}
    inclusive_weights: dict[int, Counter[FrameSymbol]] = {}

    for _, element in ET.iterparse(path, events=("end",)):
        identifier = element.attrib.get("id")

        if element.tag == "schema":
            fields = [
                mnemonic.text.strip()
                for mnemonic in element.findall("./col/mnemonic")
                if mnemonic.text and mnemonic.text.strip()
            ]
            element.clear()
            continue

        if element.tag == "pid" and identifier is not None:
            values[identifier] = int((element.text or "0").strip())
        elif element.tag == "weight" and identifier is not None:
            values[identifier] = int((element.text or "0").strip())
        elif element.tag == "binary" and identifier is not None:
            values[identifier] = element.attrib.get("name", "")
        elif element.tag == "frame" and identifier is not None:
            binary = ""
            binary_element = element.find("binary")
            if binary_element is not None:
                if "ref" in binary_element.attrib:
                    binary = str(resolve_reference(binary_element, values, path))
                else:
                    binary = binary_element.attrib.get("name", "")
            values[identifier] = FrameSymbol(
                name=element.attrib.get("name", ""),
                binary=binary,
            )
        elif element.tag == "backtrace" and identifier is not None:
            frames: list[FrameSymbol] = []
            for frame_element in element.findall("frame"):
                if "ref" in frame_element.attrib or "id" in frame_element.attrib:
                    frame = resolve_reference(frame_element, values, path)
                    if isinstance(frame, FrameSymbol):
                        frames.append(frame)
                else:
                    frames.append(
                        FrameSymbol(
                            name=frame_element.attrib.get("name", ""),
                            binary="",
                        )
                    )
            values[identifier] = tuple(frames)
        elif element.tag == "tagged-backtrace" and identifier is not None:
            backtrace = element.find("backtrace")
            if backtrace is None:
                values[identifier] = ()
            else:
                resolved = resolve_reference(backtrace, values, path)
                values[identifier] = resolved if isinstance(resolved, tuple) else ()
        elif element.tag == "process" and identifier is not None:
            pid = 0
            pid_element = element.find("pid")
            if pid_element is not None:
                if "ref" in pid_element.attrib:
                    pid = int(resolve_reference(pid_element, values, path))
                else:
                    pid = int((pid_element.text or "0").strip())
            values[identifier] = ProcessCell(
                pid=pid,
                formatted=element.attrib.get("fmt", ""),
            )
        elif element.tag == "thread" and identifier is not None:
            values[identifier] = element.attrib.get("fmt", "")
        elif element.tag == "row":
            if not fields:
                fail(f"{path} has rows before its schema")
            row = list(element)
            if len(row) != len(fields):
                fail(
                    f"{path} row has {len(row)} cells but schema has "
                    f"{len(fields)} fields"
                )
            cells = dict(zip(fields, row))
            process = resolve_reference(cells["process"], values, path)
            if not isinstance(process, ProcessCell) or process.pid not in owned_pids:
                element.clear()
                continue
            pid = process.pid
            weight = int(resolve_reference(cells["weight"], values, path))
            thread = str(resolve_reference(cells["thread"], values, path))
            stack_value = resolve_reference(cells["stack"], values, path)
            stack = (
                stack_value
                if isinstance(stack_value, tuple)
                and all(isinstance(frame, FrameSymbol) for frame in stack_value)
                else ()
            )

            process_weights[pid] += weight
            thread_weights[(pid, thread)] += weight
            if stack:
                leaf_weights.setdefault(pid, Counter())[stack[0]] += weight
                inclusive = inclusive_weights.setdefault(pid, Counter())
                for frame in set(stack):
                    inclusive[frame] += weight
            element.clear()

    return process_weights, thread_weights, leaf_weights, inclusive_weights


def demangle(names: list[str]) -> dict[str, str]:
    rustfilt = shutil.which("rustfilt")
    if rustfilt is None or not names:
        return {name: name for name in names}
    result = subprocess.run(
        [rustfilt],
        input="\n".join(names) + "\n",
        check=False,
        capture_output=True,
        text=True,
    )
    outputs = result.stdout.splitlines()
    if result.returncode != 0 or len(outputs) != len(names):
        return {name: name for name in names}
    return dict(zip(names, outputs))


def hot_symbols(
    weights: Counter[FrameSymbol],
    total_ns: int,
    limit: int,
) -> list[dict[str, object]]:
    selected = [
        (symbol, weight)
        for symbol, weight in weights.most_common()
        if symbol.name
    ][:limit]
    demangled = demangle([symbol.name for symbol, _ in selected])
    return [
        {
            "symbol": demangled[symbol.name],
            "binary": symbol.binary,
            "cpu_ms": weight / 1_000_000,
            "percent_of_process": weight / total_ns * 100 if total_ns else 0.0,
        }
        for symbol, weight in selected
    ]


def hot_actionable_inclusive_symbols(
    weights: Counter[FrameSymbol],
    total_ns: int,
    limit: int,
) -> list[dict[str, object]]:
    candidates = [
        (symbol, weight)
        for symbol, weight in weights.most_common()
        if symbol.name and symbol.binary == "zz"
    ][: max(limit * 16, 128)]
    demangled = demangle([symbol.name for symbol, _ in candidates])
    generic_prefixes = (
        "<() as objc::message::MessageArguments>::invoke",
        "<alloc::boxed::Box<dyn core::ops::function::FnMut",
        "<async_task::runnable::Runnable",
        "<core::pin::Pin<alloc::boxed::Box<dyn core::future",
        "<fn() -> std::process::ExitCode as core::ops::function::FnOnce",
        "<gpui::app::Application>::run",
    )
    selected: list[dict[str, object]] = []
    for symbol, weight in candidates:
        name = demangled[symbol.name]
        if name == "main" or name.startswith(generic_prefixes):
            continue
        selected.append(
            {
                "symbol": name,
                "binary": symbol.binary,
                "cpu_ms": weight / 1_000_000,
                "percent_of_process": weight / total_ns * 100 if total_ns else 0.0,
            }
        )
        if len(selected) == limit:
            break
    return selected


def summarize_run(run_dir: Path, top: int) -> dict[str, object]:
    run_dir = run_dir.resolve()
    metadata = read_metadata(run_dir)
    if metadata.get("mode") != "cpu":
        fail(f"{run_dir} is not a CPU capture")
    try:
        gui_pid = int(metadata["gui_pid"])
        daemon_pid = int(metadata["daemon_pid"])
    except (KeyError, ValueError):
        fail(f"{run_dir}/metadata.txt has no valid gui_pid/daemon_pid")

    target = metadata.get("target", "gui")
    trace = run_dir / f"cpu-{target}.trace"
    if not trace.exists():
        fail(f"missing trace: {trace}")

    snapshots = read_process_snapshots(run_dir)
    owned_pids = set(snapshots) | {gui_pid, daemon_pid}
    with tempfile.TemporaryDirectory(prefix="zz-cpu-summary.") as temporary:
        exported = Path(temporary) / "time-profile.xml"
        export_table(trace, TIME_PROFILE_SCHEMA, exported)
        process_weights, thread_weights, leaf_weights, inclusive_weights = (
            parse_time_profile(exported, owned_pids)
        )

    capture_seconds = duration_seconds(metadata["duration"])
    roles: Counter[str] = Counter()
    processes: list[dict[str, object]] = []
    for pid, weight in process_weights.most_common():
        snapshot = snapshots.get(pid)
        role = process_role(snapshot, pid, gui_pid, daemon_pid)
        roles[role] += weight
        process_threads = [
            (thread, thread_weight)
            for (thread_pid, thread), thread_weight in thread_weights.items()
            if thread_pid == pid
        ]
        process_threads.sort(key=lambda item: item[1], reverse=True)
        processes.append(
            {
                "pid": pid,
                "role": role,
                "command_line": (
                    snapshot.command_line if snapshot is not None else ""
                ),
                "cpu_ms": weight / 1_000_000,
                "one_core_percent": weight
                / 1_000_000_000
                / capture_seconds
                * 100,
                "threads": [
                    {
                        "thread": thread,
                        "cpu_ms": thread_weight / 1_000_000,
                        "percent_of_process": thread_weight / weight * 100,
                    }
                    for thread, thread_weight in process_threads[:top]
                ],
                "top_leaf_symbols": hot_symbols(
                    leaf_weights.get(pid, Counter()), weight, top
                ),
                "top_inclusive_symbols": hot_actionable_inclusive_symbols(
                    inclusive_weights.get(pid, Counter()), weight, top
                ),
            }
        )

    owned_total = sum(process_weights.values())
    return {
        "run_dir": str(run_dir),
        "target": target,
        "configured_duration_seconds": capture_seconds,
        "owned_cpu_ms": owned_total / 1_000_000,
        "owned_one_core_percent": owned_total
        / 1_000_000_000
        / capture_seconds
        * 100,
        "roles": {
            role: {
                "cpu_ms": weight / 1_000_000,
                "one_core_percent": weight
                / 1_000_000_000
                / capture_seconds
                * 100,
                "percent_of_owned": weight / owned_total * 100
                if owned_total
                else 0.0,
            }
            for role, weight in roles.most_common()
        },
        "processes": processes,
    }


def print_human(summary: dict[str, object]) -> None:
    print(summary["run_dir"])
    print(
        f"  owned CPU: {summary['owned_cpu_ms']:.1f} ms / "
        f"{summary['configured_duration_seconds']:.2f}s "
        f"({summary['owned_one_core_percent']:.2f}% of one core)"
    )
    roles = summary["roles"]
    assert isinstance(roles, dict)
    print("  roles:")
    for role, metrics in roles.items():
        assert isinstance(metrics, dict)
        print(
            f"    {role}: {metrics['cpu_ms']:.1f} ms "
            f"({metrics['one_core_percent']:.2f}% core, "
            f"{metrics['percent_of_owned']:.1f}% owned)"
        )

    processes = summary["processes"]
    assert isinstance(processes, list)
    for process in processes:
        assert isinstance(process, dict)
        print(
            f"  pid {process['pid']} [{process['role']}]: "
            f"{process['cpu_ms']:.1f} ms "
            f"({process['one_core_percent']:.2f}% core)"
        )
        leaves = process["top_leaf_symbols"]
        assert isinstance(leaves, list)
        if leaves:
            print("    top leaf symbols:")
        for leaf in leaves:
            assert isinstance(leaf, dict)
            binary = f" [{leaf['binary']}]" if leaf["binary"] else ""
            print(f"      {leaf['cpu_ms']:.1f} ms {leaf['symbol']}{binary}")
        inclusive_symbols = process["top_inclusive_symbols"]
        assert isinstance(inclusive_symbols, list)
        if inclusive_symbols:
            print("    top inclusive symbols:")
        for symbol in inclusive_symbols:
            assert isinstance(symbol, dict)
            binary = f" [{symbol['binary']}]" if symbol["binary"] else ""
            print(f"      {symbol['cpu_ms']:.1f} ms {symbol['symbol']}{binary}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Summarize zz-owned process CPU from a macOS Time Profiler run."
    )
    parser.add_argument("run", type=Path)
    parser.add_argument("--top", type=int, default=6)
    parser.add_argument("--json", action="store_true", dest="as_json")
    arguments = parser.parse_args()
    if arguments.top < 1:
        fail("--top must be positive")
    summary = summarize_run(arguments.run, arguments.top)
    if arguments.as_json:
        print(json.dumps(summary, indent=2, sort_keys=True))
    else:
        print_human(summary)


if __name__ == "__main__":
    main()
