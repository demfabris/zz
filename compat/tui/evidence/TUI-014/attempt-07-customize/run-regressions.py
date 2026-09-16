import json
import os
from pathlib import Path
import subprocess
import time

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
results = []
for fixture in ['tui-choosers', 'tui-copy-mode', 'tui-screen-diff', 'tui-overlays', 'attached-client']:
    for mode in ['', '--self-check'] if fixture != 'attached-client' else ['']:
        name = fixture + ('-self-check' if mode else '')
        command = [str(root / f'compat/{fixture}.sh'), *([mode] if mode else []), str(root / 'target/debug/zz'), str(root / 'compat/.cache/tmux-src/tmux')]
        start = time.monotonic()
        with (evidence / f'{name}.txt').open('w') as output:
            try:
                result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT, timeout=2400)
                code = result.returncode
            except subprocess.TimeoutExpired:
                code = 124
        results.append(dict(command=command, exit=code, seconds=round(time.monotonic()-start, 1)))
        (evidence / 'regression-results.json').write_text(json.dumps(results, indent=2)+'\n')
