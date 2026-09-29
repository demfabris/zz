import fnmatch
import json
import os

STAGES = ["baseline", "wave1", "wave2", "wave3", "final"]
HARD_KINDS = {"cpu", "bytes", "mem", "threads", "count", "throughput"}
WALL_KINDS = {"wall"}
NOISY_LOAD_PER_CPU = 0.5
RULE_KEYS = {"abs", "ratio", "plus", "vs_w0", "ceiling"}


def load_thresholds(path):
    with open(path) as f:
        return json.load(f)


def find_entry(thresholds, metric_id):
    metrics = thresholds.get("metrics", {})
    if metric_id in metrics:
        return metrics[metric_id]
    best = None
    for pattern, entry in metrics.items():
        if any(ch in pattern for ch in "*?[") and fnmatch.fnmatchcase(metric_id, pattern):
            if best is None or len(pattern) > len(best[0]):
                best = (pattern, entry)
    return best[1] if best else None


def report_only(thresholds, metric_id):
    return any(fnmatch.fnmatchcase(metric_id, p) for p in thresholds.get("report_only", []))


def rule_for(entry, stage):
    if not entry:
        return None
    rule = None
    for name in STAGES[: STAGES.index(stage) + 1]:
        if name in entry:
            rule = entry[name]
    return rule


def tolerance_for(thresholds, entry, kind):
    tol = dict(thresholds.get("tolerance", {}).get(kind) or {})
    tol.update((entry or {}).get("tolerance") or {})
    return tol if "factor" in tol else None


def worse(value, reference, tol, higher):
    floor = tol.get("floor", 0.0)
    if higher:
        return value < reference / tol["factor"] and reference - value > floor
    return value > reference * tol["factor"] and value - reference > floor


def scaled_abs(bound, kind, tmux, ref_tmux, scaling):
    how = (scaling or {}).get(kind)
    if not how or not tmux or not ref_tmux:
        return None
    if how == "ratio":
        return bound * tmux / ref_tmux
    if how == "plus":
        return bound - ref_tmux + tmux
    return None


def _checks(rule, value, tmux, w0, higher, kind=None, ref_tmux=None, scaling=None, ceiling=None):
    results = []

    def ok(bound):
        return value >= bound if higher else value <= bound

    if "abs" in rule:
        scaled = None if higher else scaled_abs(rule["abs"], kind, tmux, ref_tmux, scaling)
        if scaled is None:
            results.append(("abs", rule["abs"], ok(rule["abs"])))
        else:
            results.append((f"abs@{scaling[kind]}", round(scaled, 4), ok(scaled)))
    if "ratio" in rule and tmux is not None:
        slack = rule.get("slack", 0.0)
        bound = rule["ratio"] * tmux - slack if higher else rule["ratio"] * tmux + slack
        fraction = rule.get("ceiling")
        if fraction and ceiling and not ok(bound):
            limit = fraction * ceiling if higher else ceiling / fraction
            results.append(("ceiling", round(limit, 4), ok(limit)))
        else:
            results.append(("ratio", round(bound, 4), ok(bound)))
    if "plus" in rule and tmux is not None:
        bound = tmux - rule["plus"] if higher else tmux + rule["plus"]
        results.append(("plus", round(bound, 4), ok(bound)))
    if "vs_w0" in rule and w0 is not None:
        bound = rule["vs_w0"] * w0
        results.append(("vs_w0", round(bound, 4), ok(bound)))
    return results


def _median(index, metric_id):
    if index is None:
        return None
    return ((index.get(metric_id) or {}).get("zz") or {}).get("median")


def ceiling_id(metric_id):
    parts = metric_id.split(".")
    return ".".join([parts[0], "ceiling", *parts[2:]]) if len(parts) > 2 else None


def evaluate(metric, thresholds, stage, strict=False, noisy=False, baseline=None, w0=None, run=None, reference=None):
    entry = find_entry(thresholds, metric["id"])
    higher = (entry or {}).get("better", metric.get("better", "lower")) == "higher"
    metric["better"] = "higher" if higher else "lower"
    for stale in ("ratio", "vs_baseline", "vs_w0", "regressed", "drifted", "checks", "report_only"):
        metric.pop(stale, None)
    kind = metric.get("kind")
    zz = (metric.get("zz") or {}).get("median")
    tmux = (metric.get("tmux") or {}).get("median")
    if zz is not None and tmux:
        metric["ratio"] = round(zz / tmux, 3)
    notes = []
    hard_regression = False
    only = report_only(thresholds, metric["id"])
    tol = tolerance_for(thresholds, entry, kind)
    for label, key, flag, index in (("previous", "vs_baseline", "regressed", baseline), ("w0", "vs_w0", "drifted", w0)):
        ref = _median(index, metric["id"])
        if not ref or zz is None:
            continue
        metric[key] = round(zz / ref, 3)
        if tol and worse(zz, ref, tol, higher):
            metric[flag] = True
            notes.append(f"{abs(zz / ref - 1) * 100:.0f}% worse than {label}")
            hard_regression |= bool(tol.get("hard")) and not only
    rule = None if only else rule_for(entry, stage)
    if only:
        metric["report_only"] = True
    metric["threshold"] = rule
    wall_hard = strict or not noisy
    if zz is None:
        metric["verdict"] = "error"
    elif not rule:
        metric["verdict"] = "info"
    else:
        ref_tmux = ((reference or {}).get(metric["id"]) or {}).get("tmux") or {}
        checks = _checks(
            rule,
            zz,
            tmux,
            _median(w0, metric["id"]),
            higher,
            kind=kind,
            ref_tmux=ref_tmux.get("median"),
            scaling=thresholds.get("reference", {}).get("scale") if reference else None,
            ceiling=_median(run, ceiling_id(metric["id"])) if "ceiling" in rule else None,
        )
        metric["checks"] = [{"kind": k, "bound": b, "ok": o} for k, b, o in checks]
        if not checks:
            metric["verdict"] = "info"
            notes.append("no check could be evaluated")
        else:
            passed = any(o for _, _, o in checks) if rule.get("combine") == "any" else all(o for _, _, o in checks)
            if passed:
                metric["verdict"] = "pass"
            elif kind in HARD_KINDS or strict or (kind in WALL_KINDS and wall_hard):
                metric["verdict"] = "fail"
            else:
                metric["verdict"] = "warn"
    if hard_regression and metric["verdict"] in ("pass", "info", "warn"):
        metric["verdict"] = "fail"
    if noisy and kind in WALL_KINDS:
        notes.append("wall clock on a loaded host" + ("" if strict else ", misses only warn"))
    if notes:
        metric["gate_notes"] = notes
    else:
        metric.pop("gate_notes", None)
    return metric


def is_noisy(load_start, load_end, ncpu):
    return max(load_start[0], load_end[0]) / max(ncpu, 1) > NOISY_LOAD_PER_CPU


def summarize(metrics):
    out = {"pass": 0, "fail": 0, "warn": 0, "info": 0, "error": 0}
    for m in metrics:
        out[m["verdict"]] = out.get(m["verdict"], 0) + 1
    out["regressed"] = sum(1 for m in metrics if m.get("regressed"))
    out["drifted"] = sum(1 for m in metrics if m.get("drifted"))
    return out


def load_result(path):
    if not path or not os.path.exists(path):
        return None
    with open(path) as f:
        return json.load(f)


def reference_for(thresholds, host, here):
    ref = thresholds.get("reference") or {}
    if not ref.get("host") or ref["host"] == host or not ref.get("w0"):
        return None, None
    path = os.path.join(here, ref["w0"])
    return path, index(load_result(path))


def index(result):
    if result is None:
        return None
    return {m["id"]: m for m in result.get("metrics", [])}
