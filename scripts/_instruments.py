"""Shared Instruments plumbing for the macOS profiling summary scripts."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path
from typing import NoReturn


_SUMMARY_NAME = "zz summary"


def use_summary_name(name: str) -> None:
    """Prefix this script's failures with its own summary name."""
    global _SUMMARY_NAME
    _SUMMARY_NAME = name


def fail(message: str) -> NoReturn:
    raise SystemExit(f"{_SUMMARY_NAME}: {message}")


def read_metadata(run_dir: Path) -> dict[str, str]:
    path = run_dir / "metadata.txt"
    if not path.is_file():
        fail(f"missing metadata: {path}")

    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition("=")
        if separator:
            values[key] = value
    return values


def duration_seconds(value: str) -> float:
    match = re.fullmatch(r"([1-9][0-9]*)(ms|s|m|h)", value)
    if match is None:
        fail(f"unsupported capture duration {value!r}")
    amount = int(match.group(1))
    return amount * {"ms": 0.001, "s": 1.0, "m": 60.0, "h": 3600.0}[match.group(2)]


def export_table(trace: Path, schema: str, output: Path) -> None:
    xpath = f'/trace-toc/run[@number="1"]/data/table[@schema="{schema}"]'
    result = subprocess.run(
        [
            "xcrun",
            "xctrace",
            "export",
            "--input",
            str(trace),
            "--xpath",
            xpath,
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        fail(f"could not export {schema} from {trace}: {detail}")
