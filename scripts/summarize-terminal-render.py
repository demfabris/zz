#!/usr/bin/env python3
"""Summarize opt-in terminal-render diagnostics from a profiling run."""

from __future__ import annotations

import argparse
import json
import math
import re
from pathlib import Path


FIELDS = (
    "cached_row_hits",
    "cached_row_misses",
    "uncached_rows",
    "prepared_text_rows",
    "elapsed_us",
)
FIELD_PATTERN = re.compile(
    r"\b(" + "|".join(re.escape(field) for field in FIELDS) + r")=([0-9]+)"
)


def fail(message: str) -> None:
    raise SystemExit(f"zz terminal summary: {message}")


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


def resolve_logs(path: Path) -> list[Path]:
    path = path.resolve()
    if path.is_file():
        return [path]
    if not path.is_dir():
        fail(f"path does not exist: {path}")
    logs = [path / "app.stderr.log"]
    logs.extend(sorted((path / "logs").glob("*.log")))
    existing = [log for log in logs if log.is_file()]
    if not existing:
        fail(f"no app.stderr.log or logs/*.log under {path}")
    return existing


def summarize(path: Path) -> dict[str, object]:
    logs = resolve_logs(path)
    records: list[dict[str, int]] = []
    for log in logs:
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            if "terminal_render" not in line or " prepaint " not in line:
                continue
            values = {name: int(value) for name, value in FIELD_PATTERN.findall(line)}
            if all(field in values for field in FIELDS):
                records.append(values)

    if not records:
        fail(
            f"no cache-aware terminal prepaint records found in "
            f"{', '.join(str(log) for log in logs)}"
        )

    content_records = [
        record for record in records if record["prepared_text_rows"] > 1
    ]
    measured_records = content_records or records
    hits = sum(record["cached_row_hits"] for record in measured_records)
    misses = sum(record["cached_row_misses"] for record in measured_records)
    uncached = sum(record["uncached_rows"] for record in measured_records)
    lookups = hits + misses
    misses_per_frame = [
        record["cached_row_misses"] for record in measured_records
    ]
    elapsed = [record["elapsed_us"] for record in measured_records]
    prepared_rows = [
        record["prepared_text_rows"] for record in measured_records
    ]
    return {
        "path": str(path.resolve()),
        "logs": [str(log) for log in logs],
        "prepaint_frames": len(records),
        "measured_frames": len(measured_records),
        "measurement_scope": "content-active" if content_records else "all",
        "cached_rows": {
            "hits": hits,
            "misses": misses,
            "hit_rate_percent": hits / lookups * 100 if lookups else 0.0,
            "misses_per_frame_p50": percentile(misses_per_frame, 0.50),
            "misses_per_frame_p95": percentile(misses_per_frame, 0.95),
            "misses_per_frame_max": max(misses_per_frame),
            "frames_with_misses": sum(miss > 0 for miss in misses_per_frame),
        },
        "uncached_rows": uncached,
        "prepared_text_rows": {
            "per_frame_p50": percentile(prepared_rows, 0.50),
            "per_frame_p95": percentile(prepared_rows, 0.95),
        },
        "prepaint_time_us": {
            "total": sum(elapsed),
            "per_frame_p50": percentile(elapsed, 0.50),
            "per_frame_p95": percentile(elapsed, 0.95),
            "per_frame_max": max(elapsed),
        },
    }


def print_human(summary: dict[str, object]) -> None:
    cached = summary["cached_rows"]
    prepared = summary["prepared_text_rows"]
    elapsed = summary["prepaint_time_us"]
    assert isinstance(cached, dict)
    assert isinstance(prepared, dict)
    assert isinstance(elapsed, dict)
    print(summary["path"])
    print(
        f"  prepaint frames: {summary['prepaint_frames']} total / "
        f"{summary['measured_frames']} {summary['measurement_scope']}"
    )
    print(
        "  cached rows: "
        f"{cached['hits']} hits / {cached['misses']} misses "
        f"({cached['hit_rate_percent']:.2f}% hit rate)"
    )
    print(
        "  misses/frame p50/p95/max: "
        f"{cached['misses_per_frame_p50']:.2f}/"
        f"{cached['misses_per_frame_p95']:.2f}/"
        f"{cached['misses_per_frame_max']}"
    )
    print(
        "  prepared text rows/frame p50/p95: "
        f"{prepared['per_frame_p50']:.2f}/{prepared['per_frame_p95']:.2f}"
    )
    print(
        "  prepaint µs/frame p50/p95/max: "
        f"{elapsed['per_frame_p50']:.2f}/"
        f"{elapsed['per_frame_p95']:.2f}/"
        f"{elapsed['per_frame_max']}"
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Summarize terminal row-cache behavior from diagnostic logs."
    )
    parser.add_argument("path", type=Path)
    parser.add_argument("--json", action="store_true", dest="as_json")
    arguments = parser.parse_args()
    summary = summarize(arguments.path)
    if arguments.as_json:
        print(json.dumps(summary, indent=2, sort_keys=True))
    else:
        print_human(summary)


if __name__ == "__main__":
    main()
