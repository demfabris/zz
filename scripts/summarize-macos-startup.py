#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import subprocess
import xml.etree.ElementTree as ET
from datetime import datetime, timedelta
from pathlib import Path

from _instruments import export_table, fail, read_metadata, use_summary_name


use_summary_name("zz startup summary")


class Table:
    def __init__(self, path: Path):
        self.root = ET.parse(path).getroot()
        self.references = {
            node.attrib["id"]: node
            for node in self.root.iter()
            if "id" in node.attrib
        }

    def resolve(self, node: ET.Element) -> ET.Element:
        visited = set()
        while "ref" in node.attrib:
            reference = node.attrib["ref"]
            if reference in visited or reference not in self.references:
                fail(f"invalid XML reference {reference}")
            visited.add(reference)
            node = self.references[reference]
        return node

    def attributed(self, node: ET.Element, pid: int) -> bool:
        node = self.resolve(node)
        if node.tag == "process":
            return any(
                self.resolve(child).tag == "pid"
                and self.resolve(child).text == str(pid)
                for child in node
            )
        return any(self.attributed(child, pid) for child in node)

    def rows(self):
        for table in self.root.findall(".//node"):
            fields = [
                node.text for node in table.findall("schema/col/mnemonic")
            ]
            for row in table.findall("row"):
                if len(fields) != len(row):
                    fail("XML row does not match its schema")
                yield dict(zip(fields, (self.resolve(node) for node in row)))


def first_displayed(path: Path, pid: int) -> tuple[dict | None, int]:
    table = Table(path)
    events = []
    for row in table.rows():
        label = row.get("event-label")
        start = row.get("start")
        if label is None or start is None or not table.attributed(label, pid):
            continue
        if start.text is None:
            continue
        events.append(
            {
                "trace_ns": int(start.text),
                "description": label.get("fmt"),
                "compositor_surface_id": row["surface-id"].text
                if "surface-id" in row else None,
            }
        )
    return min(events, key=lambda event: event["trace_ns"]) if events else None, len(events)


def process_creation_start(path: Path, pid: int) -> int | None:
    table = Table(path)
    starts = []
    for row in table.rows():
        process = row.get("process")
        period = row.get("period")
        start = row.get("start")
        if process is None or period is None or start is None:
            continue
        if (
            table.attributed(process, pid)
            and period.text == "Initializing - Process Creation"
            and start.text is not None
        ):
            starts.append(int(start.text))
    return min(starts) if starts else None


def log_milestones(path: Path, pid: int) -> tuple[dict[str, float], datetime | None]:
    milestones = {}
    logger_start = None
    if not path.is_file():
        return milestones, logger_start
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if f"role=app pid={pid} " not in line:
            continue
        elapsed = re.search(r"\belapsed_us=([0-9]+)", line)
        if elapsed is None:
            continue
        elapsed_us = int(elapsed[1])
        if " process_start " in line and logger_start is None:
            try:
                logger_start = datetime.fromisoformat(line.split()[0].replace("Z", "+00:00")) - timedelta(microseconds=elapsed_us)
            except ValueError:
                pass
        if "target=zz::diagnostics::app_render render " in line:
            milestones.setdefault("first_ui_render_since_logger_ms", elapsed_us / 1000)
            if re.search(r"\bterminals=[1-9][0-9]*\b", line):
                milestones.setdefault("first_terminal_render_since_logger_ms", elapsed_us / 1000)
    return milestones, logger_start


def summarize_run(run_dir: Path) -> dict:
    run_dir = run_dir.resolve()
    metadata = read_metadata(run_dir)
    if metadata.get("mode") != "startup":
        fail(f"{run_dir} is not a startup capture")
    trace = run_dir / "startup-gui.trace"
    toc_path = run_dir / "startup-toc.xml"
    if not toc_path.is_file():
        subprocess.run(
            ["xcrun", "xctrace", "export", "--input", str(trace), "--toc", "--output", str(toc_path)],
            check=True,
        )
    toc = ET.parse(toc_path).getroot()
    target = toc.find("run/info/target/process")
    if target is None or target.get("type") != "launched":
        fail("trace does not identify a launched target process")
    if toc.findtext("run/info/summary/template-name") != "Metal System Trace":
        fail("trace does not identify the Metal System Trace template")
    recording_status = int(metadata.get("xctrace_exit_status") or 0)
    end_reason = toc.findtext("run/info/summary/end-reason")
    if recording_status != 0 and end_reason != "Time limit reached":
        fail(f"xctrace exited {recording_status} and the trace did not reach its time limit: {end_reason}")
    pid = int(target.attrib["pid"])
    schemas = {node.get("schema") for node in toc.findall(".//table")}
    exported = {}
    for schema in ("displayed-surfaces-interval", "life-cycle-period"):
        if schema not in schemas:
            continue
        path = run_dir / f"startup-{schema}.xml"
        if not path.is_file():
            export_table(trace, schema, path)
        exported[schema] = path

    first, count = (None, 0)
    if "displayed-surfaces-interval" in exported:
        first, count = first_displayed(exported["displayed-surfaces-interval"], pid)
    launch_ns = None
    if "life-cycle-period" in exported:
        launch_ns = process_creation_start(exported["life-cycle-period"], pid)
    milestones, logger_start = log_milestones(run_dir / "logs/zz.app.log", pid)
    logger_display_ms = None
    trace_start = toc.findtext("run/info/summary/start-date")
    if first is not None and logger_start is not None and trace_start is not None:
        logger_trace_ns = (logger_start - datetime.fromisoformat(trace_start)).total_seconds() * 1e9
        logger_display_ms = (first["trace_ns"] - logger_trace_ns) / 1e6
    return {
        "run_dir": str(run_dir),
        "gui_pid": pid,
        "app": metadata.get("app"),
        "app_sha256": metadata.get("app_sha256"),
        "xctrace_exit_status": recording_status,
        "recording_end_reason": end_reason,
        "first_displayed_surface": first,
        "attributed_displayed_surface_rows": count,
        "display_unavailable_reason": None if first else "No displayed surface has a structured process attribution matching the launched GUI PID.",
        "process_creation_start_trace_ns": launch_ns,
        "first_displayed_since_launch_ms": (first["trace_ns"] - launch_ns) / 1e6
        if first is not None and launch_ns is not None else None,
        "first_displayed_since_logger_ms_approx": logger_display_ms,
        "log_milestones": milestones,
        "measurement_notes": [
            "Launch timing begins at the trace lifecycle Process Creation interval and includes Instruments overhead.",
            "Logger alignment is approximate because the trace wall-clock start has millisecond precision.",
            "UI and terminal render logs record CPU render calls; they do not establish when those contents reached the display.",
            "The first displayed app surface does not establish that terminal contents were ready.",
        ],
    }


def print_human(summary: dict) -> None:
    print(summary["run_dir"])
    first = summary["first_displayed_surface"]
    if first is None:
        print(f"  First displayed frame: unavailable. {summary['display_unavailable_reason']}")
    else:
        print(f"  First displayed frame: {first['trace_ns'] / 1e6:.3f} ms on the trace timeline (GUI PID {summary['gui_pid']})")
        launch_ms = summary["first_displayed_since_launch_ms"]
        if launch_ms is None:
            print("  Time since process launch: unavailable; no attributed Process Creation interval")
        else:
            print(f"  Instrumented launch to first display: {launch_ms:.3f} ms")
        logger_ms = summary["first_displayed_since_logger_ms_approx"]
        if logger_ms is not None:
            print(f"  Logger start to first display: approximately {logger_ms:.3f} ms")
    labels = {
        "first_ui_render_since_logger_ms": "First UI render call",
        "first_terminal_render_since_logger_ms": "First render call with a terminal",
    }
    for name, value in summary["log_milestones"].items():
        print(f"  {labels[name]}: {value:.3f} ms since logger start")
    print("  Instruments adds startup overhead; render calls do not measure display or terminal readiness.")


def main() -> None:
    parser = argparse.ArgumentParser(description="Measure the first displayed app frame in a macOS launch trace.")
    parser.add_argument("run", type=Path)
    arguments = parser.parse_args()
    summary = summarize_run(arguments.run)
    (arguments.run / "startup-summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print_human(summary)


if __name__ == "__main__":
    main()
