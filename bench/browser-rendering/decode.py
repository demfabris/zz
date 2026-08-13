#!/usr/bin/env python3
"""Turn a screen capture of pages/flicker.html into presented-frame metrics.

The page paints its frame index as a binary bar pattern once per rAF. Counting
distinct patterns in a video of the screen gives the number of frames that
actually reached the display -- independent of what the web engine thought it
produced, and therefore comparable across a windowed Chromium view and an
offscreen-rendered one pumped through a host compositor.

    ./decode.py crop capture.mov              # what region will be read?
    ./decode.py capture.mov --label zz        # decode + append to results/
    ./decode.py summarize

Requires ffmpeg/ffprobe on PATH. Nothing else.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
RESULTS = HERE / "results"
DECODE_JSONL = RESULTS / "decode.jsonl"
ENGINE_JSONL = RESULTS / "engine.jsonl"

SAMPLES_PER_BAR = 3  # sample each bar 3x and keep the middle: tolerates a sloppy crop
MIN_CONTRAST = 40    # 8-bit levels between black and white bars before we distrust the crop


def die(msg):
    sys.exit("decode: " + msg)


def run(cmd, expect_binary=True):
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        err = proc.stderr.decode("utf-8", "replace").strip().splitlines()
        die("%s failed: %s" % (cmd[0], err[-1] if err else "exit %d" % proc.returncode))
    return proc.stdout if expect_binary else proc.stdout.decode("utf-8", "replace")


def frac(text):
    if not text or "/" not in text:
        return None
    num, den = text.split("/", 1)
    try:
        num, den = float(num), float(den)
    except ValueError:
        return None
    return num / den if den else None


def probe(path):
    out = run([
        "ffprobe", "-v", "error", "-select_streams", "v:0",
        "-show_entries", "stream=width,height,avg_frame_rate,r_frame_rate",
        "-show_entries", "format=duration",
        "-of", "json", str(path),
    ], expect_binary=False)
    data = json.loads(out)
    if not data.get("streams"):
        die("no video stream in %s" % path)
    stream = data["streams"][0]
    fps = frac(stream.get("avg_frame_rate")) or frac(stream.get("r_frame_rate"))
    return {
        "width": int(stream["width"]),
        "height": int(stream["height"]),
        "fps": fps,
        "duration": float(data.get("format", {}).get("duration") or 0.0),
    }


def autocrop(path, meta, samples=12, analysis_w=240, thresh=0.40):
    """Find the flickering region by per-pixel range across sampled frames.

    Bit 0 flips every frame, so the bars have maximal variance and static app
    chrome none: the bounding box of high-variance pixels is the page viewport.
    """
    duration = meta["duration"]
    if duration <= 0:
        die("cannot autocrop: ffprobe reports no duration; pass --crop W:H:X:Y")

    width = analysis_w
    height = max(1, round(meta["height"] * width / meta["width"]))
    npix = width * height

    frames = []
    for i in range(samples):
        when = duration * (i + 0.5) / samples
        buf = run([
            "ffmpeg", "-v", "error", "-ss", "%.3f" % when, "-i", str(path),
            "-frames:v", "1", "-vf", "scale=%d:%d:flags=area" % (width, height),
            "-f", "rawvideo", "-pix_fmt", "gray", "-",
        ])
        if len(buf) == npix:
            frames.append(buf)
    if len(frames) < 2:
        die("autocrop: could not sample frames from %s" % path)

    spread = bytearray(npix)
    for p in range(npix):
        lo, hi = 255, 0
        for f in frames:
            v = f[p]
            if v < lo:
                lo = v
            if v > hi:
                hi = v
        spread[p] = hi - lo

    peak = max(spread)
    if peak < 30:
        die("autocrop: nothing in this video is flickering: did the page run?")

    cut = peak * thresh
    xs, ys = [], []
    for y in range(height):
        base = y * width
        for x in range(width):
            if spread[base + x] >= cut:
                xs.append(x)
                ys.append(y)

    sx = meta["width"] / width
    sy = meta["height"] / height
    x0, x1 = min(xs), max(xs)
    y0, y1 = min(ys), max(ys)
    cx = int(round(x0 * sx))
    cy = int(round(y0 * sy))
    cw = max(1, int(round((x1 + 1) * sx)) - cx)
    ch = max(1, int(round((y1 + 1) * sy)) - cy)
    # ffmpeg's crop filter wants even offsets for some pixel formats; harmless here.
    return (cw, ch, cx, cy)


def parse_crop(text):
    parts = text.split(":")
    if len(parts) != 4:
        die("--crop wants W:H:X:Y, got %r" % text)
    try:
        return tuple(int(p) for p in parts)
    except ValueError:
        die("--crop wants four integers, got %r" % text)


def bar_samples(path, crop, bars):
    """One byte per bar per video frame, sampled at the bar centres."""
    wide = bars * SAMPLES_PER_BAR
    vf = "crop=%d:%d:%d:%d,scale=%d:1:flags=area" % (crop[0], crop[1], crop[2], crop[3], wide)
    raw = run([
        "ffmpeg", "-v", "error", "-i", str(path), "-vf", vf,
        "-f", "rawvideo", "-pix_fmt", "gray", "-",
    ])
    if len(raw) < wide:
        die("no frames decoded: check the crop")
    nframes = len(raw) // wide
    centres = []
    for i in range(nframes):
        row = raw[i * wide:(i + 1) * wide]
        centres.append([row[b * SAMPLES_PER_BAR + 1] for b in range(bars)])
    return centres


def percentile(sorted_values, p):
    if not sorted_values:
        return 0
    i = min(len(sorted_values) - 1, max(0, int(round(p * (len(sorted_values) - 1)))))
    return sorted_values[i]


def longest_true_run(flags):
    best_start = best_len = cur_start = cur_len = 0
    for i, flag in enumerate(flags):
        if flag:
            if cur_len == 0:
                cur_start = i
            cur_len += 1
            if cur_len > best_len:
                best_len, best_start = cur_len, cur_start
        else:
            cur_len = 0
    return best_start, best_len


def analyse(centres, bars, capture_fps):
    bits = bars - 1
    modulus = 1 << bits
    warnings = []

    flat = sorted(v for row in centres for v in row)
    black, white = flat[0], flat[-1]
    # Marker bar is white while running and mid-grey while idle, so 75% of the
    # way to white separates them with a wide margin either side.
    marker_gate = black + 0.75 * (white - black)
    running = [row[0] >= marker_gate for row in centres]

    start, length = longest_true_run(running)
    total_running = sum(running)
    if length < 4:
        die("no run region found: the marker bar never went white (wrong crop, or the page never started)")
    if length < 0.5 * total_running:
        die("marker bar is blinking (%d of %d marker-white frames fall outside the longest "
            "run): the crop is misaligned with the bars; check it with `decode.py crop`"
            % (total_running - length, total_running))

    # Re-derive the levels from the run region only: a capture that is mostly
    # idle grey would otherwise drag the black reference up.
    region = centres[start:start + length]
    data_vals = sorted(v for row in region for v in row[1:])
    black = percentile(data_vals, 0.005)
    white = percentile(data_vals, 0.995)
    contrast = white - black
    if contrast < MIN_CONTRAST:
        warnings.append("low contrast (%d levels): the crop probably misses the bars" % contrast)
    mid = (black + white) / 2.0

    raw_index = []
    for row in region:
        n = 0
        for b in range(bits):
            if row[b + 1] > mid:
                n |= 1 << b
        raw_index.append(n)

    indices = []
    ambiguous = 0
    prev_raw = None
    prev_un = 0
    for r in raw_index:
        if prev_raw is None:
            prev_un = r
        else:
            delta = (r - prev_raw) % modulus
            if delta >= modulus // 2:
                ambiguous += 1
            prev_un += delta
        indices.append(prev_un)
        prev_raw = r

    if ambiguous:
        warnings.append("%d ambiguous index jumps (>= half the %d-frame wrap window)" % (ambiguous, modulus))

    # How long each presented frame stayed on screen, in captured frames.
    holds = []
    hold = 1
    changes = 0
    for i in range(1, len(indices)):
        if indices[i] == indices[i - 1]:
            hold += 1
        else:
            changes += 1
            holds.append(hold)
            hold = 1
    holds.append(hold)

    span = indices[-1] - indices[0]
    presented = changes + 1
    dropped = max(0, span - changes)
    run_secs = length / capture_fps
    presented_fps = changes / run_secs if run_secs > 0 else 0.0
    engine_fps = span / run_secs if run_secs > 0 else 0.0

    holds_sorted = sorted(holds)
    to_ms = 1000.0 / capture_fps

    if run_secs < 1.0:
        warnings.append("run region is only %.2fs: capture the whole run, or the page stopped early" % run_secs)
    if capture_fps < 2 * engine_fps:
        warnings.append(
            "capture %.0ffps vs engine %.0ffps: presented_fps is still sound but "
            "per-frame drop attribution is aliased: capture at >=2x" % (capture_fps, engine_fps)
        )
    if capture_fps < 100:
        warnings.append(
            "capture is only %.0ffps: if this came off a phone, share the ORIGINAL "
            "slo-mo file rather than an export (exports bake the slowdown in at 30fps)"
            % capture_fps
        )

    return {
        "capture_fps": round(capture_fps, 2),
        "capture_frames": len(centres),
        "run_frames": length,
        "run_secs": round(run_secs, 3),
        "contrast": contrast,
        "engine_frames": span,
        "engine_fps_observed": round(engine_fps, 2),
        "presented_frames": presented,
        "presented_fps": round(presented_fps, 2),
        "fidelity_pct": round(100.0 * presented_fps / engine_fps, 1) if engine_fps > 0 else 0.0,
        "dropped_frames": dropped,
        "dropped_pct": round(100.0 * dropped / span, 1) if span > 0 else 0.0,
        "present_interval_ms": {
            "p50": round(percentile(holds_sorted, 0.50) * to_ms, 2),
            "p90": round(percentile(holds_sorted, 0.90) * to_ms, 2),
            "p99": round(percentile(holds_sorted, 0.99) * to_ms, 2),
            "max": round(holds_sorted[-1] * to_ms, 2),
        },
        "warnings": warnings,
    }


def resolve_crop(path, meta, args):
    if args.crop:
        return parse_crop(args.crop), "manual"
    return autocrop(path, meta), "auto"


def cmd_crop(args):
    path = Path(args.video)
    meta = probe(path)
    crop, how = resolve_crop(path, meta, args)
    print("video      %dx%d  %.2f fps  %.2fs" % (meta["width"], meta["height"], meta["fps"] or 0, meta["duration"]))
    print("crop (%s) %d:%d:%d:%d" % (how, crop[0], crop[1], crop[2], crop[3]))
    pct = 100.0 * (crop[0] * crop[1]) / (meta["width"] * meta["height"])
    print("           %.1f%% of the frame" % pct)

    out = Path(args.out) if args.out else RESULTS / ("crop-%s.png" % path.stem)
    out.parent.mkdir(exist_ok=True)
    when = max(0.0, meta["duration"] * 0.5)
    run([
        "ffmpeg", "-v", "error", "-y", "-ss", "%.3f" % when, "-i", str(path),
        "-frames:v", "1", "-vf", "crop=%d:%d:%d:%d" % crop, str(out),
    ])
    print("preview    %s  (should be nothing but the bars, edge to edge)" % out)


def cmd_decode(args):
    path = Path(args.video)
    if not path.is_file():
        die("no such file: %s" % path)
    meta = probe(path)
    capture_fps = args.capture_fps or meta["fps"]
    if not capture_fps:
        die("cannot determine capture frame rate; pass --capture-fps")

    crop, how = resolve_crop(path, meta, args)
    centres = bar_samples(path, crop, args.bars)
    result = analyse(centres, args.bars, capture_fps)

    result.update({
        "kind": "decode",
        "label": args.label or path.stem,
        "ts": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "video": str(path),
        "crop": "%d:%d:%d:%d" % crop,
        "crop_source": how,
        "bars": args.bars,
        "video_size": "%dx%d" % (meta["width"], meta["height"]),
    })

    RESULTS.mkdir(exist_ok=True)
    with DECODE_JSONL.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(result, separators=(",", ":")) + "\n")

    print("label            %s" % result["label"])
    print("crop (%s)       %s of %s" % (how, result["crop"], result["video_size"]))
    print("capture          %.2f fps, %d frames, run region %.2fs" %
          (result["capture_fps"], result["capture_frames"], result["run_secs"]))
    print("engine produced  %d frames  (%.1f fps)" % (result["engine_frames"], result["engine_fps_observed"]))
    print("display showed   %d frames  (%.1f fps)" % (result["presented_frames"], result["presented_fps"]))
    print("pump fidelity    %.1f%%   dropped %d (%.1f%%)" %
          (result["fidelity_pct"], result["dropped_frames"], result["dropped_pct"]))
    pi = result["present_interval_ms"]
    print("frame interval   p50 %.2fms  p99 %.2fms  max %.2fms" % (pi["p50"], pi["p99"], pi["max"]))
    for warn in result["warnings"]:
        print("  !! %s" % warn)
    print("\nappended to %s" % DECODE_JSONL)


def load_jsonl(path):
    if not path.is_file():
        return []
    rows = []
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                try:
                    rows.append(json.loads(line))
                except ValueError:
                    continue
    return rows


def cmd_summarize(args):
    engine, decoded = {}, {}
    for row in load_jsonl(ENGINE_JSONL):
        engine[(row.get("label") or "?", row.get("load_ms", 0))] = row
    for row in load_jsonl(DECODE_JSONL):
        decoded[row.get("label") or "?"] = row

    if not engine and not decoded:
        die("nothing in results/ yet")

    keys = sorted(set(engine) | set((lbl, engine.get((lbl, 0), {}).get("load_ms", 0)) for lbl in decoded))
    lines = [
        "| label | load | engine fps | presented fps | fidelity | dropped | p50 present | p99 present | long frames |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    def cell(row, key, fmt):
        value = row.get(key)
        return fmt % value if value is not None else "–"

    for label, load in keys:
        eng = engine.get((label, load), {})
        # A capture has no load dimension of its own; it joins to the plain run.
        # Capture a loaded run under its own label (zz-load4) to get its own row.
        dec = decoded.get(label, {}) if load == 0 else {}
        pi = dec.get("present_interval_ms", {})
        lines.append("| %s%s | %sms | %s | %s | %s | %s | %s | %s | %s |" % (
            label,
            " ⚠" if eng.get("tainted") else "",
            load,
            cell(eng, "engine_fps", "%.1f"),
            cell(dec, "presented_fps", "%.1f"),
            cell(dec, "fidelity_pct", "%.0f%%"),
            cell(dec, "dropped_pct", "%.1f%%"),
            cell(pi, "p50", "%.2fms"),
            cell(pi, "p99", "%.2fms"),
            cell(eng, "long_frame_pct", "%.1f%%"),
        ))

    table = "\n".join(lines)
    print(table)

    notes = []
    for (label, _load), row in sorted(engine.items()):
        for reason in row.get("tainted") or []:
            notes.append("%s: TAINTED, %s" % (label, reason))
        # The page counts its own rAF calls; the video counts index advances:
        # two independent measurements of the same thing.
        if _load == 0 and label in decoded:
            reported = row.get("engine_fps") or 0
            observed = decoded[label].get("engine_fps_observed") or 0
            if reported > 0 and observed > 0 and abs(reported - observed) > 0.1 * max(reported, observed):
                notes.append(
                    "%s: page reported %.1f engine fps but the video shows %.1f: the capture "
                    "and the report are probably from different runs or windows"
                    % (label, reported, observed))
    for label, row in sorted(decoded.items()):
        for warn in row.get("warnings", []):
            notes.append("%s: %s" % (label, warn))
    if notes:
        print()
        for note in notes:
            print("!! %s" % note)

    RESULTS.mkdir(exist_ok=True)
    out = RESULTS / "summary.md"
    out.write_text("# browser-rendering\n\n%s\n" % table, encoding="utf-8")
    print("\nwrote %s" % out)


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd")

    def add_common(parser):
        parser.add_argument("video")
        parser.add_argument("--crop", help="W:H:X:Y; omit to locate the bars automatically")

    p_crop = sub.add_parser("crop", help="show/preview the region that will be read")
    add_common(p_crop)
    p_crop.add_argument("--out", help="preview PNG path")
    p_crop.set_defaults(func=cmd_crop)

    p_dec = sub.add_parser("decode", help="decode a capture into metrics")
    add_common(p_dec)
    p_dec.add_argument("--label", help="defaults to the video filename")
    p_dec.add_argument("--bars", type=int, default=13, help="must match the page's bars= (default 13)")
    p_dec.add_argument("--capture-fps", type=float, help="override the container's frame rate")
    p_dec.set_defaults(func=cmd_decode)

    p_sum = sub.add_parser("summarize", help="render results/ as a markdown table")
    p_sum.set_defaults(func=cmd_summarize)

    argv = sys.argv[1:]
    if argv and argv[0] not in {"crop", "decode", "summarize", "-h", "--help"}:
        argv = ["decode"] + argv

    args = ap.parse_args(argv)
    if not getattr(args, "func", None):
        ap.print_help()
        sys.exit(2)
    args.func(args)


if __name__ == "__main__":
    main()
