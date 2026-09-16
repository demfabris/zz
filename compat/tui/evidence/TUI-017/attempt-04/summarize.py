import json
from pathlib import Path
import re
import time

root = Path.cwd()
p = root / 'compat/tui/evidence/TUI-017/attempt-04'
main = root / 'target/capture-12-reconciled-main-src'
while not (p / 'delta-exits.json').exists() or not (p / '11-main-failures.txt').exists() or 'exit_status:' not in (p / '11-main-failures.txt').read_text():
    time.sleep(2)
selection = (p / 'delta-selection.txt').read_text().splitlines()
manifest = json.loads((p / 'delta-batches.json').read_text())
assert sorted(sum(manifest['batches'], [])) == sorted(selection)
assert len(selection) == len(set(selection))

def result(source, name):
    path = source / 'compat/results' / (name.removesuffix('.txt') + '.log')
    if not path.exists():
        return None
    if source == root and name == 'smoke/split-window-wait.txt':
        path = p / 'delta-split-window-wait.log'
    if name == 'smoke/control-hard-loss.txt':
        path = p / ('delta-control-hard-loss-' + ('candidate' if source == root else 'main') + '.log')
    if source == root and name == 'smoke/plugin-runtime-continuum.txt':
        path = p / 'delta-continuum.log'
    text = path.read_text()
    summary = next((line for line in reversed(text.splitlines()) if line.startswith('SUMMARY ')), None)
    if summary is None:
        return None
    fields = dict(re.findall(r'(\w+)=(\S+)', summary))
    failed = fields.get('status') == 'skip' or any(int(fields.get(key + '_divergences', 0)) for key in ['topo', 'fmt', 'out', 'warn'])
    differences = [line for line in text.splitlines() if line.startswith(('-', '+')) and not line.startswith(('---', '+++'))]
    return {'summary': fields, 'failed': failed, 'difference_lines': differences}

rows = {name: result(root, name) for name in selection}
missing = [name for name, row in rows.items() if row is None]
failed = [name for name, row in rows.items() if row and row['failed']]
summary = {'selected': len(selection), 'completed': len(selection) - len(missing), 'missing': missing, 'failed': failed, 'rows': rows}
(p / 'corpus-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
old = json.loads((p.parent / 'attempt-03/corpus-summary.json').read_text())['failed']
comparison = {name: {'candidate': rows.get(name), 'main': result(main, name), 'classification': 'pending inspection'} for name in sorted(set(old + failed))}
(p / 'failure-comparison.json').write_text(json.dumps(comparison, indent=2) + '\n')
print(f"Delta corpus: {summary['completed']}/{summary['selected']} completed, {len(failed)} final divergent rows, {len(missing)} missing")
print('Additional main comparisons needed:', [name for name, row in comparison.items() if row['main'] is None])
