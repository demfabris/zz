#!/usr/bin/env python3
"""Export and summarize the zz-owned rows in a macOS Metal trace."""

from __future__ import annotations

import argparse
import json
import math
import shutil
import sys
import tempfile
import xml.etree.ElementTree as ET
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

from _instruments import (
    duration_seconds,
    export_table,
    fail,
    read_metadata,
    use_summary_name,
)


use_summary_name("zz Metal summary")

TABLE_SCHEMAS = {
    "command_buffers": "metal-application-command-buffer-submissions",
    "presents": "ca-client-present-request",
    "gpu_intervals": "metal-gpu-intervals",
}


@dataclass(frozen=True)
class Cell:
    tag: str
    raw: str | None
    formatted: str | None
    pid: int | None = None


def table_rows(path: Path) -> Iterator[tuple[list[str], list[Cell]]]:
    fields: list[str] = []
    cells_by_id: dict[str, Cell] = {}

    def resolve(element: ET.Element) -> Cell:
        reference = element.attrib.get("ref")
        if reference is not None:
            try:
                return cells_by_id[reference]
            except KeyError:
                fail(f"{path} contains an unresolved XML reference {reference}")

        identifier = element.attrib.get("id")
        if identifier is not None and identifier in cells_by_id:
            return cells_by_id[identifier]

        raw = element.text.strip() if element.text and element.text.strip() else None
        process_pid = None
        if element.tag == "process":
            for child in element:
                child_cell = resolve(child)
                if child_cell.tag == "pid" and child_cell.raw is not None:
                    process_pid = int(child_cell.raw)
                    break
        return Cell(
            tag=element.tag,
            raw=raw,
            formatted=element.attrib.get("fmt") or raw,
            pid=process_pid,
        )

    for event, element in ET.iterparse(path, events=("end",)):
        if element.tag == "schema":
            fields = [
                mnemonic.text.strip()
                for mnemonic in element.findall("./col/mnemonic")
                if mnemonic.text and mnemonic.text.strip()
            ]
            element.clear()
            continue

        identifier = element.attrib.get("id")
        if identifier is not None:
            cells_by_id[identifier] = resolve(element)

        if element.tag == "row":
            if not fields:
                fail(f"{path} has rows before its schema")
            row = [resolve(child) for child in element]
            if len(row) != len(fields):
                fail(
                    f"{path} row has {len(row)} cells but schema has {len(fields)} fields"
                )
            yield fields, row
            element.clear()


def percentile(values: list[int], percentile_value: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * percentile_value
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return float(ordered[lower])
    return ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


def span_ns(starts: list[int], durations: list[int] | None = None) -> int:
    if not starts:
        return 0
    if durations is None:
        return max(starts) - min(starts)
    return max(start + duration for start, duration in zip(starts, durations)) - min(starts)


def union_ns(intervals: list[tuple[int, int]]) -> int:
    if not intervals:
        return 0
    ordered = sorted(intervals)
    total = 0
    start, end = ordered[0]
    for next_start, next_end in ordered[1:]:
        if next_start <= end:
            end = max(end, next_end)
        else:
            total += end - start
            start, end = next_start, next_end
    return total + end - start


def selected_rows(
    path: Path, pid: int
) -> Iterator[tuple[dict[str, int], list[Cell]]]:
    indexes: dict[str, int] | None = None
    for fields, row in table_rows(path):
        if indexes is None:
            indexes = {field: index for index, field in enumerate(fields)}
            if "process" not in indexes:
                fail(f"{path} schema does not contain a process column")
        process = row[indexes["process"]]
        if process.pid == pid:
            yield indexes, row


def summarize_run(run_dir: Path) -> dict[str, object]:
    run_dir = run_dir.resolve()
    metadata = read_metadata(run_dir)
    if metadata.get("mode") != "metal" or metadata.get("target") != "gui":
        fail(f"{run_dir} is not a gui Metal capture")
    try:
        pid = int(metadata["gui_pid"])
    except (KeyError, ValueError):
        fail(f"{run_dir}/metadata.txt has no valid gui_pid")

    trace = run_dir / "metal-gui.trace"
    if not trace.exists():
        fail(f"missing trace: {trace}")

    with tempfile.TemporaryDirectory(prefix="zz-metal-summary.") as temporary:
        export_dir = Path(temporary)
        exported = {
            name: export_dir / f"{name}.xml" for name in TABLE_SCHEMAS
        }
        for name, schema in TABLE_SCHEMAS.items():
            export_table(trace, schema, exported[name])

        command_starts: list[int] = []
        command_durations: list[int] = []
        encoder_durations: list[int] = []
        encoder_counts: set[int] = set()
        frame_numbers: list[int] = []
        for indexes, row in selected_rows(exported["command_buffers"], pid):
            command_starts.append(int(row[indexes["start"]].raw or 0))
            command_durations.append(int(row[indexes["duration"]].raw or 0))
            encoder_durations.append(int(row[indexes["encoder-time"]].raw or 0))
            encoder_counts.add(int(row[indexes["num-encoders"]].raw or 0))
            frame_numbers.append(int(row[indexes["frame-number"]].raw or 0))

        present_starts: list[int] = []
        surfaces: set[str] = set()
        for indexes, row in selected_rows(exported["presents"], pid):
            present_starts.append(int(row[indexes["timestamp"]].raw or 0))
            surface = row[indexes["surface-id"]].raw
            if surface is not None:
                surfaces.add(surface)

        gpu_starts: list[int] = []
        gpu_durations: list[int] = []
        gpu_latencies: list[int] = []
        gpu_intervals: list[tuple[int, int]] = []
        channel_intervals: dict[str, list[tuple[int, int]]] = defaultdict(list)
        for indexes, row in selected_rows(exported["gpu_intervals"], pid):
            start = int(row[indexes["start"]].raw or 0)
            duration = int(row[indexes["duration"]].raw or 0)
            latency = row[indexes["start-latency"]].raw
            channel = row[indexes["channel-name"]].formatted or "Unknown"
            interval = (start, start + duration)
            gpu_starts.append(start)
            gpu_durations.append(duration)
            gpu_intervals.append(interval)
            channel_intervals[channel].append(interval)
            if latency is not None:
                gpu_latencies.append(int(latency))

    configured_span_seconds = duration_seconds(metadata["duration"])
    observed_span_ns = max(
        span_ns(command_starts, command_durations),
        span_ns(present_starts),
        span_ns(gpu_starts, gpu_durations),
    )
    observed_span_seconds = (
        observed_span_ns / 1_000_000_000
        if observed_span_ns
        else configured_span_seconds
    )
    present_gaps = [
        second - first for first, second in zip(present_starts, present_starts[1:])
    ]
    gpu_union = union_ns(gpu_intervals)

    return {
        "run_dir": str(run_dir),
        "gui_pid": pid,
        "configured_duration_seconds": configured_span_seconds,
        "observed_activity_span_seconds": observed_span_seconds,
        "command_buffers": {
            "count": len(command_starts),
            "per_second": len(command_starts) / observed_span_seconds,
            "submission_cpu_ms_total": sum(command_durations) / 1_000_000,
            "encoder_cpu_ms_total": sum(encoder_durations) / 1_000_000,
            "encoder_cpu_us_p50": percentile(encoder_durations, 0.50) / 1_000,
            "encoder_cpu_us_p95": percentile(encoder_durations, 0.95) / 1_000,
            "encoders_per_buffer": sorted(encoder_counts),
            "frame_range": (
                [min(frame_numbers), max(frame_numbers)] if frame_numbers else None
            ),
        },
        "presents": {
            "count": len(present_starts),
            "per_second": len(present_starts) / observed_span_seconds,
            "gap_ms_p50": percentile(present_gaps, 0.50) / 1_000_000,
            "gap_ms_p95": percentile(present_gaps, 0.95) / 1_000_000,
            "gap_ms_max": max(present_gaps, default=0) / 1_000_000,
            "surface_count": len(surfaces),
        },
        "gpu": {
            "interval_count": len(gpu_intervals),
            "execution_ms_raw": sum(gpu_durations) / 1_000_000,
            "execution_ms_union": gpu_union / 1_000_000,
            "occupancy_percent": gpu_union / 1_000_000_000 / observed_span_seconds * 100,
            "cpu_to_gpu_latency_us_p50": percentile(gpu_latencies, 0.50) / 1_000,
            "cpu_to_gpu_latency_us_p95": percentile(gpu_latencies, 0.95) / 1_000,
            "channels": {
                channel: {
                    "count": len(intervals),
                    "execution_ms_raw": sum(end - start for start, end in intervals)
                    / 1_000_000,
                    "execution_ms_union": union_ns(intervals) / 1_000_000,
                }
                for channel, intervals in sorted(channel_intervals.items())
            },
        },
    }


def print_human(summary: dict[str, object]) -> None:
    command_buffers = summary["command_buffers"]
    presents = summary["presents"]
    gpu = summary["gpu"]
    assert isinstance(command_buffers, dict)
    assert isinstance(presents, dict)
    assert isinstance(gpu, dict)

    print(summary["run_dir"])
    print(
        "  command buffers: "
        f"{command_buffers['count']} ({command_buffers['per_second']:.2f}/s), "
        f"encoder CPU {command_buffers['encoder_cpu_ms_total']:.3f} ms total"
    )
    print(
        "  presents: "
        f"{presents['count']} ({presents['per_second']:.2f}/s), "
        f"gap p50/p95/max {presents['gap_ms_p50']:.3f}/"
        f"{presents['gap_ms_p95']:.3f}/{presents['gap_ms_max']:.3f} ms"
    )
    print(
        "  GPU: "
        f"{gpu['execution_ms_union']:.3f} ms union, "
        f"{gpu['occupancy_percent']:.3f}% occupancy, "
        f"CPU→GPU latency p50/p95 {gpu['cpu_to_gpu_latency_us_p50']:.2f}/"
        f"{gpu['cpu_to_gpu_latency_us_p95']:.2f} µs"
    )
    channels = gpu["channels"]
    assert isinstance(channels, dict)
    for channel, metrics in channels.items():
        assert isinstance(metrics, dict)
        print(
            f"    {channel}: {metrics['count']} intervals, "
            f"{metrics['execution_ms_union']:.3f} ms"
        )


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Summarize zz-owned command buffers, presents, and GPU work."
    )
    parser.add_argument("run_dir", nargs="+", type=Path)
    parser.add_argument("--json", action="store_true", dest="as_json")
    arguments = parser.parse_args()

    if sys.platform != "darwin":
        fail("macOS is required")
    if shutil.which("xcrun") is None:
        fail("xcrun is required")

    summaries = [summarize_run(path) for path in arguments.run_dir]
    if arguments.as_json:
        print(json.dumps(summaries, indent=2, sort_keys=True))
    else:
        for index, summary in enumerate(summaries):
            if index:
                print()
            print_human(summary)


if __name__ == "__main__":
    main()
