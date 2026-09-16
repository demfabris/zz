import json
from pathlib import Path

p = Path(__file__).resolve().parent
rows = json.loads((p / 'failure-comparison.json').read_text())
observations = [json.loads(line) for line in (p / 'corpus-observations.jsonl').read_text().splitlines()]
for name, row in rows.items():
    differences = row['candidate']['difference_lines']
    matches = [observation['sha256'] for observation in observations if observation['side'] == 'main' and observation['scenario'] == name and [line for line in observation['output'].splitlines() if line.startswith(('-', '+')) and not line.startswith(('---', '+++'))] == differences]
    row['matching_clean_main_observations'] = matches
    if matches:
        row['classification'] = 'inherited'
        row['reason'] = 'An unmodified clean e40a13e1 corpus run has exactly these difference lines; the raw output is retained in corpus-observations.jsonl under the matching hash.'
    else:
        assert name == 'smoke/split-window-wait.txt', name
        row['classification'] = 'caused-here'
        row['reason'] = 'Conservative corpus attribution: candidate fails and clean main passes the initial run and all three paired repeats. A smaller timing probe can produce one pane on both binaries; no source-level cause or fix is established. The unchanged corpus regression remains open.'
(p / 'failure-comparison.json').write_text(json.dumps(rows, indent=2) + '\n')
summary = json.loads((p / 'corpus-summary.json').read_text())
first = {}
for observation in observations:
    if observation['side'] == 'candidate':
        first.setdefault(observation['scenario'], observation)
cleared = []
for name, observation in first.items():
    fields = dict(part.split('=', 1) for part in observation['summary'].split()[1:])
    failed = any(int(fields.get(key + '_divergences', 0)) for key in ['topo', 'fmt', 'out', 'warn'])
    if failed and not summary['rows'][name]['failed']:
        cleared.append({'scenario': name, 'initial_sha256': observation['sha256'], 'final_summary': summary['rows'][name]['summary']})
(p / 'corpus-retries.json').write_text(json.dumps(cleared, indent=2) + '\n')
assert summary['completed'] == summary['selected'] == 163
assert not summary['missing']
assert len(rows) == 13
assert sum(row['classification'] == 'inherited' for row in rows.values()) == 12
print('163/163 delta rows completed; 13 final failures: 12 inherited, 1 caused-here at the corpus boundary; ' + str(len(cleared)) + ' first-pass failures cleared on retry')
