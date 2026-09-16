import json
import re
from pathlib import Path

root = Path(__file__).resolve().parent
selected = (root / 'delta-selection-full.txt').read_text().splitlines()
initial = (root / 'delta-selection.txt').read_text().splitlines()
extra = (root / 'delta-extra-selection.txt').read_text().splitlines()
expected = {name.removesuffix('.txt'): name for name in selected}
commands = (root / 'commands.txt').read_text()
rows = {}
shards = []
first_failures = []
final_failures = []
known = []
pattern = re.compile(r'^(.*?): (\d+) step\(s\), (\d+) TOPO divergence\(s\), (\d+) GEO divergence\(s\), (\d+) FMT divergence\(s\), (\d+) OUT divergence\(s\), (\d+) WARN divergence\(s\)$', re.M)
for shard in [0, 1, "extra"]:
    label = f'corpus-complete-{shard}'
    output = re.sub(r'\x1b\[[0-9;]*m', '', (root / f'{label}.txt').read_text())
    for match in pattern.finditer(output):
        name, *counts = match.groups()
        values = [int(value) for value in counts]
        rows[name] = dict(zip(['steps', 'topology', 'geometry', 'format', 'output', 'warning'], values))
    exits = re.findall(rf'END {label} exit=(\d+)', commands)
    shards.append({'label': label, 'selected_rows': len(extra) if shard == 'extra' else len(initial[shard::2]), 'exit_status': int(exits[-1]) if exits else None})
    first_failures += re.findall(r'warn: (.+) failed; it is retried alone after the corpus', output)
    final_failures += re.findall(r'warn: (.+) failed again alone;', output)
    known += re.findall(r'warn: (.+) has its exact documented divergence \(([^)]+)\)', output)
unrun = [path for name, path in expected.items() if name not in rows]
summary = {
    'selected_rows': len(selected),
    'completed_rows': len(rows),
    'completed_steps': sum(row['steps'] for row in rows.values()),
    'complete': not unrun and all(shard['exit_status'] is not None for shard in shards),
    'unrun_rows': unrun,
    'unexpected_rows': sorted(set(rows) - set(expected)),
    'shards': shards,
    'first_pass_failures': first_failures,
    'failed_after_retry': final_failures,
    'known_divergences': [{'row': name, 'tuple': value} for name, value in known],
    'results': {expected.get(name, name): counts for name, counts in sorted(rows.items())},
}
(root / 'corpus-coverage.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({key: value for key, value in summary.items() if key != 'results'}, indent=2))
