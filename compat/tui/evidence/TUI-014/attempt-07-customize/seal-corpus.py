import hashlib
import json
from pathlib import Path
import re
import shutil
import tarfile

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
result = json.loads((evidence / 'delta-run.result.json').read_text())
text = re.sub(r'\x1b\[[0-9;]*m', '', (evidence / 'delta-run.txt').read_text())
selected = [line.removesuffix('.txt') for line in (evidence / 'delta-list.txt').read_text().splitlines() if line]
pattern = r'^([^\n]+): (\d+) step\(s\), (\d+) TOPO divergence\(s\), (\d+) GEO divergence\(s\), (\d+) FMT divergence\(s\), (\d+) OUT divergence\(s\), (\d+) WARN divergence\(s\)'
rows = {}
for name, *counts in re.findall(pattern, text, re.M):
    rows[name] = dict(zip(['steps', 'topology', 'geometry', 'formats', 'output', 'warnings'], map(int, counts)))
failures = re.findall(r'warn: (.+) failed again alone;', text)
known = re.findall(r'warn: (.+) has its exact documented divergence', text)
retried = re.findall(r'==> retrying (.+) alone', text)
summary = dict(command=result['command'], exit=result['exit'], selected=len(selected), completed=len(rows), steps=sum(row['steps'] for row in rows.values()), unrun=sorted(set(selected)-set(rows)), failed=failures, known=sorted(set(known)), retried=retried, recovered=sorted(set(retried)-set(failures)), rows=rows)
(evidence / 'corpus-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
for name in rows:
    source = root / 'compat/results' / (name + '.log')
    data = source.read_bytes()
    assert re.search(rb'^SUMMARY ', data, re.M)
    digest = hashlib.sha256(data).hexdigest()
    target = evidence / 'corpus-logs' / (name + '.' + digest[:12] + '.log')
    target.parent.mkdir(parents=True, exist_ok=True)
    if not target.exists():
        target.write_bytes(data)
manifest = []
for source in sorted((evidence / 'corpus-logs').rglob('*.log')):
    data = source.read_bytes()
    name = re.search(rb'^# Scenario: (.+)$', data, re.M).group(1).decode()
    manifest.append(dict(scenario=name, sha256=hashlib.sha256(data).hexdigest(), path=str(source.relative_to(evidence))))
(evidence / 'corpus-log-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
assert set(rows) <= {entry['scenario'] for entry in manifest}
archive = evidence / 'corpus-logs.tar.gz'
assert not archive.exists()
with tarfile.open(archive, 'w:gz') as output:
    for entry in manifest:
        source = evidence / entry['path']
        assert hashlib.sha256(source.read_bytes()).hexdigest() == entry['sha256']
        output.add(source, arcname=entry['path'], recursive=False)
with tarfile.open(archive) as source:
    for entry in manifest:
        data = source.extractfile(entry['path']).read()
        assert hashlib.sha256(data).hexdigest() == entry['sha256']
shutil.rmtree(evidence / 'corpus-logs')
print(json.dumps({key:value for key,value in summary.items() if key != 'rows'}, indent=2))
