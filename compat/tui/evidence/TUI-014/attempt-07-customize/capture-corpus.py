import hashlib
import json
from pathlib import Path
import re
import time

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
output = evidence / 'corpus-logs'
output.mkdir(exist_ok=True)
seen = set()
manifest = []
while True:
    names = re.findall(r'^(.+): \d+ step\(s\),', (evidence / 'delta-run.txt').read_text(), re.M)
    for name in names:
        source = root / 'compat/results' / (name + '.log')
        if not source.exists():
            continue
        data = source.read_bytes()
        if not re.search(rb'^SUMMARY ', data, re.M):
            continue
        digest = hashlib.sha256(data).hexdigest()
        if (name, digest) in seen:
            continue
        seen.add((name, digest))
        target = output / (name + '.' + digest[:12] + '.log')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        manifest.append(dict(scenario=name, sha256=digest, path=str(target.relative_to(evidence))))
        (evidence / 'corpus-log-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if (evidence / 'delta-run.result.json').exists():
        break
    time.sleep(1)
