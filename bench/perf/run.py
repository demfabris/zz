#!/usr/bin/env python3
import argparse
import datetime
import glob
import hashlib
import importlib
import json
import os
import platform
import select
import signal
import socket
import subprocess
import sys
import time
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)

import gate
import isolate
import probe
import timing
from ptyclient import PtyClient

GROUPS = ["cli", "spawn", "cold", "config", "chatty", "idle", "mem", "attach", "echo", "throughput", "control", "statusjob"]
PANE = os.path.join(HERE, "pane")


class Ctx:
    def __init__(self, args, env, zz, tmux):
        self.args = args
        self.quick = args.quick
        self.env = env
        self.zz = zz
        self.tmux = tmux
        self.muxes = [zz, tmux]
        self.metrics = []
        self.group = None
        self.python = sys.executable
        self.history = {}
        self.ceilings = {}

    def order(self, i):
        return self.muxes if i % 2 == 0 else self.muxes[::-1]

    def pick(self, full, quick):
        return quick if self.quick else full

    def add(self, metric_id, unit, kind, zz, tmux=None, notes=None, better=None):
        entry = {
            "id": metric_id,
            "group": self.group,
            "unit": unit,
            "kind": kind,
            "zz": timing.stats(zz),
            "tmux": timing.stats(tmux),
        }
        if better:
            entry["better"] = better
        if notes:
            entry["notes"] = [notes] if isinstance(notes, str) else list(notes)
        self.metrics.append(entry)
        return entry

    def add_instr(self, metric_id, unit, zz, tmux=None, notes=None):
        if not probe.HAS_INSTRUCTIONS:
            return None
        return self.add(metric_id, unit, "instr", zz, tmux, notes)

    def pane(self, script, *args):
        quoted = " ".join(str(a) for a in args)
        return f"exec {self.python} {os.path.join(PANE, script)} {quoted}".strip()

    def reset(self):
        for mux in self.muxes:
            mux.kill()

    def session(self, mux, name, cols=180, rows=50, command=None):
        args = ["new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows)]
        if command:
            args.append(command)
        mux.run(*args, check=True)
        mux.run("set-option", "-g", "default-size", f"{cols}x{rows}")
        seen = self.history.setdefault(self.group, {})
        if mux.name not in seen:
            seen[mux.name] = mux.out("show-options", "-gv", "history-limit")

    def attach(self, mux, target, cols=180, rows=50, env=None):
        return PtyClient(mux.attach_argv(target), env or mux.env, self.env, cols=cols, rows=rows)

    def wait(self, predicate, timeout, step=0.05):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return True
            time.sleep(step)
        return predicate()

    def drain(self, clients, seconds, tick=None):
        deadline = time.perf_counter() + seconds
        got = {id(c): 0 for c in clients}
        while True:
            remaining = deadline - time.perf_counter()
            if remaining <= 0:
                return got
            live = [c for c in clients if not c.closed]
            if tick:
                tick()
            if not live:
                time.sleep(min(remaining, 0.002 if tick else 0.05))
                continue
            ready, _, _ = select.select([c.fd for c in live], [], [], min(remaining, 0.002 if tick else 0.05))
            for c in live:
                if c.fd in ready:
                    data = c._read_once(0)
                    if data:
                        got[id(c)] += len(data)

    def samples(self, muxes=None):
        return {m.name: m.sample() for m in (muxes or self.muxes)}


def git_sha():
    try:
        sha = subprocess.run(["git", "-C", REPO, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
        dirty = subprocess.run(
            ["git", "-C", REPO, "status", "--porcelain", "--untracked-files=no", "--", "crates", "third_party", "Cargo.toml", "Cargo.lock"],
            capture_output=True,
            text=True,
        ).stdout.strip()
        return sha, bool(dirty)
    except OSError:
        return "", False


def head_commit_time():
    try:
        out = subprocess.run(["git", "-C", REPO, "log", "-1", "--format=%ct"], capture_output=True, text=True).stdout.strip()
        return int(out) if out else None
    except (OSError, ValueError):
        return None


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def default_w0(host, quick):
    pattern = os.path.join(HERE, "results", f"baseline-{'quick-' if quick else ''}{host}-*.json")
    found = sorted(glob.glob(pattern))
    return found[0] if len(found) == 1 else None


def resolve_w0(arg, host, quick):
    if arg == "none":
        return None
    return os.path.abspath(arg) if arg else default_w0(host, quick)


def sample_warnings(result, quick, label):
    if result and bool(result.get("meta", {}).get("quick")) != bool(quick):
        mode = "quick" if quick else "full"
        other = "quick" if result["meta"].get("quick") else "full"
        return [f"{label} is a {other} run and this is a {mode} run; sample counts differ"]
    return []


def build_profile(path):
    parts = os.path.realpath(path).split(os.sep)
    for known in ("release", "debug", "profiling"):
        if known in parts:
            return known
    return "unknown"


def version(argv):
    try:
        return subprocess.run(argv, capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return ""


def fmt(value):
    if value is None:
        return "-"
    if abs(value) >= 1000:
        return f"{value:.0f}"
    if abs(value) >= 10:
        return f"{value:.1f}"
    return f"{value:.3g}"


def rule_text(rule):
    if not rule:
        return ""
    parts = []
    for key in ("abs", "ratio", "plus", "vs_w0"):
        if key in rule:
            parts.append(f"{key}={rule[key]}" + (f"+{rule['slack']}" if key == "ratio" and rule.get("slack") else ""))
    if rule.get("combine") == "any":
        parts.append("any")
    return " ".join(parts)


def print_table(metrics):
    header = f"{'metric':44s} {'unit':6s} {'zz':>9s} {'zz p90':>9s} {'tmux':>9s} {'ratio':>7s}  verdict  threshold"
    print(header)
    print("-" * len(header))
    for m in metrics:
        zz = m.get("zz") or {}
        tm = m.get("tmux") or {}
        line = (
            f"{m['id']:44s} {m['unit']:6s} {fmt(zz.get('median')):>9s} {fmt(zz.get('p90')):>9s} "
            f"{fmt(tm.get('median')):>9s} {fmt(m.get('ratio')):>7s}  {m['verdict']:7s}  {rule_text(m.get('threshold'))}"
        )
        notes = m.get("notes", []) + m.get("gate_notes", [])
        if notes:
            line += "  # " + "; ".join(notes)
        print(line)


def rescore(args):
    with open(args.rescore) as f:
        result = json.load(f)
    thresholds = gate.load_thresholds(args.thresholds)
    quick = result["meta"].get("quick", False)
    host = result["meta"].get("host", "")
    base = gate.load_result(args.baseline)
    w0_path = resolve_w0(args.w0, host, quick)
    w0 = gate.load_result(w0_path)
    noisy = result["meta"].get("noisy", False)
    ref_path, reference = gate.reference_for(thresholds, host, HERE)
    run = gate.index(result)
    for metric in result["metrics"]:
        gate.evaluate(metric, thresholds, args.stage, strict=args.strict, noisy=noisy, baseline=gate.index(base), w0=gate.index(w0), run=run, reference=reference)
    result["meta"]["reference"] = ref_path
    result["summary"] = gate.summarize(result["metrics"])
    result["meta"]["rescored_stage"] = args.stage
    result["meta"]["w0"] = w0_path
    print_table(result["metrics"])
    for warning in sample_warnings(base, quick, "--baseline") + sample_warnings(w0, quick, "w0"):
        print(f"warning: {warning}")
    print(f"summary at {args.stage}: {result['summary']}  (w0 {w0_path or 'none'})")
    if args.json:
        with open(args.json, "w") as f:
            json.dump(result, f, indent=1)
            f.write("\n")
    return 1 if result["summary"]["fail"] or result["summary"]["error"] else 0


def with_unit(value, unit):
    if unit == "count":
        return f"{value:g}"
    if unit in ("%", "1/s"):
        return f"{value:g}{'%' if unit == '%' else '/s'}"
    return f"{value:g} {unit}"


def plain(value):
    if value is None:
        return "-"
    return str(int(value)) if float(value).is_integer() else fmt(value)


def rule_md(rule, unit, higher):
    if not rule:
        return ""
    op = ">=" if higher else "<="
    parts = []
    if "abs" in rule:
        parts.append(f"{op} {with_unit(rule['abs'], unit)}")
    if "ratio" in rule:
        slack = rule.get("slack")
        ceiling = rule.get("ceiling")
        near = ""
        if ceiling:
            near = f" or {op} {ceiling:g}x the pty ceiling" if higher else f" or {op} the pty ceiling's time / {ceiling:g}"
        parts.append(f"{op} {rule['ratio']:g}x" + (f" {'-' if higher else '+'} {with_unit(slack, unit)}" if slack else "") + near)
    if "plus" in rule:
        parts.append(f"{op} tmux {'-' if higher else '+'} {with_unit(rule['plus'], unit)}")
    if "vs_w0" in rule:
        parts.append(f"{op} {rule['vs_w0']:g}x W0")
    return (" or " if rule.get("combine") == "any" else ", ").join(parts)


def targets(args):
    thresholds = gate.load_thresholds(args.thresholds)
    host = socket.gethostname().split(".")[0]
    w0_path = resolve_w0(args.w0, host, args.quick)
    w0 = gate.load_result(w0_path)
    if not w0:
        sys.exit("targets needs a W0 result (--w0 PATH)")
    print("| Metric id | unit | tmux | zz W0 | " + " | ".join(gate.STAGES) + " |")
    print("|---|---|---|---|" + "---|" * len(gate.STAGES))
    metrics = list(w0["metrics"])
    seen = {m["id"] for m in metrics}
    metrics.extend(
        {"id": metric_id, "unit": entry["unit"]}
        for metric_id, entry in thresholds["metrics"].items()
        if metric_id not in seen and "unit" in entry and not any(ch in metric_id for ch in "*?[")
    )
    for m in metrics:
        entry = gate.find_entry(thresholds, m["id"])
        if gate.report_only(thresholds, m["id"]) or not entry:
            continue
        higher = entry.get("better", m.get("better", "lower")) == "higher"
        cells = []
        prev = None
        for stage in gate.STAGES:
            rule = gate.rule_for(entry, stage)
            cells.append(rule_md(rule, m["unit"], higher) if rule != prev else "")
            prev = rule
        if not any(cells):
            continue
        tm = (m.get("tmux") or {}).get("median")
        zz = (m.get("zz") or {}).get("median")
        print(f"| `{m['id']}` | {m['unit']} | {plain(tm)} | {plain(zz)} | " + " | ".join(cells) + " |")
    return 0


def main():
    parser = argparse.ArgumentParser(description="zz vs tmux daemon benchmark gate")
    parser.add_argument("--zz", default=os.path.join(REPO, "target", "release", "zz_cli"))
    parser.add_argument("--headless", default=os.path.join(REPO, "target", "release", "examples", "perf_client"))
    parser.add_argument("--tmux")
    parser.add_argument("--stage", default="baseline", choices=gate.STAGES)
    parser.add_argument("--json")
    parser.add_argument("--baseline")
    parser.add_argument("--w0")
    parser.add_argument("--quick", action="store_true")
    parser.add_argument("--only", default="")
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--thresholds", default=os.path.join(HERE, "thresholds.json"))
    parser.add_argument("--rescore", metavar="JSON")
    parser.add_argument("--targets", action="store_true")
    args = parser.parse_args()
    if args.rescore:
        sys.exit(rescore(args))
    if args.targets:
        sys.exit(targets(args))

    zz_bin = os.path.abspath(args.zz)
    if not os.access(zz_bin, os.X_OK):
        sys.exit(f"zz binary not found: {zz_bin} (cargo build --release -p zz-cli)")
    tmux_bin, refused = isolate.resolve_tmux(args.tmux, zz_bin)
    if not tmux_bin:
        sys.exit("no usable release tmux: " + "; ".join(refused) + " (pass --tmux)")
    for why in refused:
        print(f"skipped {why}")
    only = [g.strip() for g in args.only.split(",") if g.strip()]
    unknown = [g for g in only if g not in GROUPS]
    if unknown:
        sys.exit(f"unknown groups: {', '.join(unknown)} (known: {', '.join(GROUPS)})")
    groups = only or GROUPS

    sha, dirty = git_sha()
    host = socket.gethostname().split(".")[0]
    zz_mtime = int(os.stat(zz_bin).st_mtime)
    head_time = head_commit_time()
    warnings = []
    if head_time and zz_mtime < head_time:
        warnings.append("zz binary is older than the HEAD commit; rebuild (cargo build --release -p zz-cli) unless HEAD changed no code")
    if not probe.HAS_INSTRUCTIONS:
        warnings.append(f"no instruction counts ({probe.INSTRUCTIONS_SOURCE}); instr metrics and the 5% rule are skipped")
    base = gate.load_result(args.baseline)
    w0_path = resolve_w0(args.w0, host, args.quick)
    w0 = gate.load_result(w0_path)
    warnings += sample_warnings(base, args.quick, "--baseline") + sample_warnings(w0, args.quick, "w0")
    started = datetime.datetime.now(datetime.timezone.utc)
    if not args.json:
        stamp = started.strftime("%Y%m%dT%H%M%SZ")
        args.json = os.path.join(HERE, "results", "local", f"{args.stage}-{host}-{sha[:8]}-{stamp}.json")
    meta = {
        "stage": args.stage,
        "quick": args.quick,
        "strict": args.strict,
        "groups": groups,
        "started_at": started.isoformat(timespec="seconds"),
        "host": host,
        "os": f"{platform.system()} {platform.release()}",
        "arch": platform.machine(),
        "ncpu": os.cpu_count(),
        "python": platform.python_version(),
        "git_sha": sha,
        "git_dirty": dirty,
        "zz_bin": zz_bin,
        "zz_version": version([zz_bin, "-V"]),
        "zz_sha256": file_sha256(zz_bin),
        "zz_mtime": datetime.datetime.fromtimestamp(zz_mtime, datetime.timezone.utc).isoformat(timespec="seconds"),
        "zz_profile": build_profile(zz_bin),
        "tmux_bin": tmux_bin,
        "tmux_version": version([tmux_bin, "-V"]),
        "tmux_libs": [l.strip().split(" ")[0] for l in isolate.linked_libraries(tmux_bin).splitlines()[1:] if l.strip()],
        "baseline": os.path.abspath(args.baseline) if args.baseline else None,
        "w0": w0_path,
        "history_limit": isolate.HISTORY_LIMIT,
        "cpu_source": probe.CPU_SOURCE,
        "instructions": probe.HAS_INSTRUCTIONS,
        "instructions_source": probe.INSTRUCTIONS_SOURCE,
        "warnings": warnings,
        "loadavg_start": timing.loadavg(),
    }
    print(f"perf-gate {args.stage}{' quick' if args.quick else ''}: zz {meta['zz_version']} ({meta['zz_profile']}, {sha[:8]}{'+dirty' if dirty else ''}) vs {meta['tmux_version']} ({tmux_bin})")
    print(f"load average {meta['loadavg_start']} on {meta['ncpu']} cpus; groups: {', '.join(groups)}; w0 {w0_path or 'none'}", flush=True)
    for warning in warnings:
        print(f"warning: {warning}", flush=True)

    env = isolate.Env(keep=args.keep)
    meta["root"] = env.root
    meta["knobs"] = env.knobs
    if env.knobs:
        print(f"rollback knobs passed to both muxes: {' '.join(f'{k}={v}' for k, v in sorted(env.knobs.items()))}", flush=True)
    zz = isolate.Zz(zz_bin, env)
    tmux = isolate.Tmux(tmux_bin, env)
    ctx = Ctx(args, env, zz, tmux)
    errors = []
    timings = {}

    def on_signal(signum, _frame):
        raise KeyboardInterrupt(f"signal {signum}")

    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, on_signal)
    interrupted = False
    started_mono = time.monotonic()
    try:
        for name in groups:
            ctx.group = name
            t0 = time.monotonic()
            print(f"[{name}] ...", flush=True)
            try:
                ctx.reset()
                importlib.import_module(f"groups.{name}").run(ctx)
            except KeyboardInterrupt:
                raise
            except Exception as exc:
                errors.append({"group": name, "error": f"{type(exc).__name__}: {exc}", "trace": traceback.format_exc()})
                print(f"[{name}] error: {exc}", flush=True)
            finally:
                ctx.reset()
            timings[name] = round(time.monotonic() - t0, 1)
            print(f"[{name}] {timings[name]} s", flush=True)
    except KeyboardInterrupt as exc:
        interrupted = True
        errors.append({"group": ctx.group, "error": f"interrupted: {exc}"})
    finally:
        strays = env.cleanup()

    meta["loadavg_end"] = timing.loadavg()
    meta["duration_s"] = round(time.monotonic() - started_mono, 1)
    meta["group_seconds"] = timings
    meta["stray_processes_after"] = strays
    meta["orphans_killed"] = env.orphans_killed
    meta["group_history_limit"] = ctx.history
    for group, seen in ctx.history.items():
        values = set(seen.values())
        if values != {str(isolate.HISTORY_LIMIT)}:
            errors.append({"group": group, "error": f"history-limit differs from {isolate.HISTORY_LIMIT}: {seen}"})
    noisy = gate.is_noisy(meta["loadavg_start"], meta["loadavg_end"], meta["ncpu"])
    meta["noisy"] = noisy

    thresholds = gate.load_thresholds(args.thresholds)
    ref_path, reference = gate.reference_for(thresholds, host, HERE)
    meta["reference"] = ref_path
    run = {m["id"]: m for m in ctx.metrics}
    for metric in ctx.metrics:
        gate.evaluate(metric, thresholds, args.stage, strict=args.strict, noisy=noisy, baseline=gate.index(base), w0=gate.index(w0), run=run, reference=reference)
    summary = gate.summarize(ctx.metrics)
    if strays:
        errors.append({"group": None, "error": f"stray processes after cleanup: {strays}"})
    result = {"schema": 2, "meta": meta, "metrics": ctx.metrics, "summary": summary, "errors": errors}
    os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
    with open(args.json, "w") as f:
        json.dump(result, f, indent=1)
        f.write("\n")

    print()
    print_table(ctx.metrics)
    print()
    for err in errors:
        print(f"error in {err['group']}: {err['error']}")
    for warning in warnings:
        print(f"warning: {warning}")
    if noisy:
        print(f"note: load average {meta['loadavg_start']} -> {meta['loadavg_end']} on {meta['ncpu']} cpus; wall clock misses only warn{'' if not args.strict else ' (but --strict)'}")
    print(f"summary: {summary}  ({meta['duration_s']} s)  -> {args.json}")
    if interrupted:
        sys.exit(130)
    sys.exit(1 if summary["fail"] or summary["error"] or errors else 0)


if __name__ == "__main__":
    main()
