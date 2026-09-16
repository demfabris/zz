import hashlib
import json
from pathlib import Path
import time

root = Path.cwd()
folder = root / 'compat/tui/evidence/TUI-017/attempt-03'
selection = (folder / '14-delta-selection.txt').read_text().splitlines()
seen = set()
while True:
    for name in selection:
        path = root / 'compat/results' / (name.removesuffix('.txt') + '.log')
        if not path.exists():
            continue
        text = path.read_text(errors='replace')
        summary = next((line for line in reversed(text.splitlines()) if line.startswith('SUMMARY ')), None)
        if summary is None:
            continue
        digest = hashlib.sha256(text.encode()).hexdigest()
        key = (name, digest)
        if key in seen:
            continue
        seen.add(key)
        with (folder / 'corpus-observations.jsonl').open('a') as output:
            output.write(json.dumps({'scenario': name, 'sha256': digest, 'summary': summary, 'output': text}) + '\n')
    if (folder / 'delta-batch-exits.json').exists():
        break
    time.sleep(2)
print(f'Retained {len(seen)} complete scenario observations')
