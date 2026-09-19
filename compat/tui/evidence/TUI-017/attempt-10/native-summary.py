import json
import statistics
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
name = sys.argv[1] if len(sys.argv) > 1 else "native-samples.jsonl"
rows = [json.loads(line) for line in (here / name).read_text().splitlines() if line]
keys = []
for row in rows:
    key = (row["sample"]["workload"], row["sample"]["variant"])
    if key not in keys:
        keys.append(key)
print("workload variant cpu_median_candidate cpu_median_main cpu_delta% cpu_per_block_delta% cpu_paired_median% cpu_paired_min% cpu_paired_max% n instructions_candidate instructions_main instructions_delta%")
for workload, variant in keys:
    picked = [r for r in rows if r["sample"]["workload"] == workload and r["sample"]["variant"] == variant]
    def cpu(binary, block=None):
        return [r["sample"]["cpu_seconds"] for r in picked if r["binary"] == binary and (block is None or r["block"] == block)]
    cand, main = statistics.median(cpu("candidate")), statistics.median(cpu("main"))
    blocks = []
    for block in sorted({r["block"] for r in picked}):
        blocks.append(f"{(statistics.median(cpu('candidate', block)) / statistics.median(cpu('main', block)) - 1) * 100:+.1f}")
    pairs = {}
    for r in picked:
        pairs.setdefault((r["block"], r["pair"]), {})[r["binary"]] = r["sample"]["cpu_seconds"]
    ratios = [(p["candidate"] / p["main"] - 1) * 100 for p in pairs.values()]
    counts = {binary: [r["sample"].get("instructions", -1) for r in picked if r["binary"] == binary] for binary in ("candidate", "main")}
    if min(counts["candidate"] + counts["main"]) > 0:
        ic, im = statistics.median(counts["candidate"]), statistics.median(counts["main"])
        instructions = f"{ic:.0f} {im:.0f} {(ic / im - 1) * 100:+.4f}%"
    else:
        instructions = "- - -"
    print(f"{workload} {variant} {cand:.6f} {main:.6f} {(cand / main - 1) * 100:+.2f}% [{' '.join(blocks)}] "
          f"{statistics.median(ratios):+.2f}% {min(ratios):+.1f}% {max(ratios):+.1f}% {len(ratios)} {instructions}")
