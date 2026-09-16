import json
from pathlib import Path
import re
import sys
import time

root = Path.cwd()
folder = root / 'compat/tui/evidence/TUI-017/attempt-03'
while not (folder / 'delta-batch-exits.json').exists():
    time.sleep(2)
selected = (folder / '14-delta-selection.txt').read_text().splitlines()
manifest = json.loads((folder / 'delta-batches.json').read_text())
covered = [name + '.txt' for name in manifest['prefix']]
covered += [str(Path(path).relative_to(root / 'compat/scenarios')) for batch in manifest['batches'] for path in batch]
assert len(covered) == len(set(covered)) == len(selected)
assert set(covered) == set(selected)
rows = []
missing = []
for name in selected:
    path = root / 'compat/results' / (name.removesuffix('.txt') + '.log')
    text = path.read_text() if path.exists() else ''
    summary = next((line for line in reversed(text.splitlines()) if line.startswith('SUMMARY ')), None)
    if summary is None:
        missing.append(name)
        continue
    fields = dict(re.findall(r'(\w+)=(\S+)', summary))
    failed = fields.get('status') == 'skip' or any(int(fields.get(key + '_divergences', 0)) for key in ['topo', 'fmt', 'out', 'warn'])
    rows.append({'scenario': name, 'summary': fields, 'failed': failed})
result = {'selected': len(selected), 'completed': len(rows), 'missing': missing, 'failed': [row['scenario'] for row in rows if row['failed']], 'rows': rows}
(folder / 'corpus-summary.json').write_text(json.dumps(result, indent=2) + '\n')
print(f"Delta corpus: {len(rows)}/{len(selected)} completed, {len(result['failed'])} final divergent rows, {len(missing)} missing")
for name in result['failed']:
    print('DIFF', name)
sys.exit(1 if missing or result['failed'] else 0)
