import hashlib
import json
from pathlib import Path
import re

root = Path.cwd()
evidence = Path(__file__).resolve().parent
scenarios = ['show-options-hooks', 'smoke/alias-group-forgery', 'smoke/args-parse-choosers', 'smoke/cli-chain-parse-abort', 'smoke/command-flag-errors', 'smoke/daemon-invalid-flags', 'smoke/jobs-command-environment', 'smoke/plugin-runtime-resurrect-restore', 'smoke/positional-maximums', 'smoke/positional-minimums', 'smoke/source-replay-diagnostics', 'smoke/split-window-wait', 'smoke/status-background-jobs', 'capture-pane', 'targets']
main_run = re.sub(r'\x1b\[[0-9;]*m', '', (evidence / '44-final-main-corpus.txt').read_text())
rows = []
for scenario in scenarios:
    row = {'scenario': scenario}
    for side, source in [('candidate', root / 'compat/results'), ('main', root / 'target/capture-12-final-main-src/compat/results')]:
        path = source / (scenario + '.log')
        if not path.exists():
            continue
        target = evidence / ('corpus-' + side) / (scenario + '.txt')
        data = target.read_bytes() if target.exists() else path.read_bytes()
        text = data.decode()
        summary = re.findall(r'^SUMMARY .+$', text, re.M)
        assert len(summary) == 1, str(path)
        counts = {key: int(value) for key, value in re.findall(r'(\w+)=(\d+)', summary[0])}
        target = evidence / ('corpus-' + side) / (scenario + '.txt')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        row[side] = {'summary': summary[0], 'counts': counts, 'diverges': any(value for key, value in counts.items() if key != 'steps'), 'sha256': hashlib.sha256(data).hexdigest(), 'artifact': str(target.relative_to(root)), 'diff_lines': [line for line in text.splitlines() if line.startswith(('-', '+')) and not line.startswith(('---', '+++'))]}
    marker = '==> running ' + scenario + '\n'
    if marker in main_run:
        section = main_run.split(marker, 1)[1].split('==> ', 1)[0]
        excerpt = '\n'.join(line[4:] if line.startswith('    ') else line for line in section.splitlines()) + '\n'
        summaries = re.findall(r'^SUMMARY .+$', excerpt, re.M)
        if summaries:
            counts = {key: int(value) for key, value in re.findall(r'(\w+)=(\d+)', summaries[-1])}
            target = evidence / 'corpus-main-first' / (scenario + '.txt')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(excerpt)
            row['main_first'] = {'summary': summaries[-1], 'counts': counts, 'diverges': any(value for key, value in counts.items() if key != 'steps'), 'artifact': str(target.relative_to(root)), 'kind': 'failure excerpt retained by run.sh in artifact 44', 'diff_lines': [line for line in excerpt.splitlines() if line.startswith(('-', '+')) and not line.startswith(('---', '+++'))]}
    assert 'candidate' in row
    if not row['candidate']['diverges']:
        row['classification'] = 'pass'
    else:
        match = next((name for name in ['main', 'main_first'] if row.get(name, {}).get('diverges') and row['candidate']['diff_lines'] == row[name]['diff_lines']), None)
        if match:
            row['classification'] = 'inherited; exact diff lines reproduce on clean d1694e65'
        else:
            match = next((name for name in ['main', 'main_first'] if row.get(name, {}).get('diverges') and set(row['candidate']['diff_lines']).issubset(set(row[name]['diff_lines']))), None)
            assert match, scenario
            row['classification'] = 'inherited shared failure; clean main contains every final candidate diff line plus an additional failure, not an exact whole-row reproduction'
        row['matching_main_observation'] = match
        if not row['main']['diverges']:
            row['classification'] += '; main passes on retry, shared timing sensitivity'
    rows.append(row)
result = {'candidate_rows': len(rows), 'main_rows': sum('main' in row for row in rows), 'candidate_failures': [row['scenario'] for row in rows if row['candidate']['diverges']], 'caused_here': [], 'rows': rows}
(evidence / 'corpus-summary.json').write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
print(json.dumps({key: value for key, value in result.items() if key != 'rows'}))
