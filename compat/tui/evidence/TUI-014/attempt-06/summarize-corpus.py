from pathlib import Path
import json
import re

root = Path(__file__).resolve().parents[5]
evidence = Path(__file__).resolve().parent
selection = (evidence / "corpus-selection-final.txt").read_text().splitlines()
pattern = re.compile(r"^SUMMARY steps=(\d+) topo_divergences=(\d+) geo_divergences=(\d+) fmt_divergences=(\d+) out_divergences=(\d+) warn_divergences=(\d+)$", re.M)
rows = []
unrun = []
for row in selection:
    path = root / "compat/results" / Path(row).with_suffix(".log")
    matches = pattern.findall(path.read_text(errors="replace")) if path.exists() else []
    if not matches:
        unrun.append(row)
        continue
    values = [int(value) for value in matches[-1]]
    rows.append(dict(zip(["row", "steps", "topology", "geometry", "formats", "output", "warnings"], [row, *values])))
summary = {
    "binary_source_revision": "f884b266",
    "selection_count": len(selection),
    "completed_count": len(rows),
    "unrun": unrun,
    "steps": sum(row["steps"] for row in rows),
    "zero_divergence_rows": sum(not any(row[key] for key in ["topology", "geometry", "formats", "output", "warnings"]) for row in rows),
    "rows_with_divergences": [row for row in rows if any(row[key] for key in ["topology", "geometry", "formats", "output", "warnings"])],
    "rows": rows,
}
(evidence / "corpus-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps({key: value for key, value in summary.items() if key != "rows"}, indent=2))
