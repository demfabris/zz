import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
name, *command = sys.argv[1:]
env = {key: value for key, value in os.environ.items() if not any(word in key.upper() for word in ['TOKEN', 'SECRET', 'PASSWORD', 'API_KEY'])}
env.update(ZZ_COMPAT_ZZ=str(root / 'target/debug/zz'), ZZ_COMPAT_TMUX=str(root / 'compat/.cache/tmux-src/tmux'), ZZ_COMPAT_CORPUS=str(root / 'compat/.cache/plugins'))
start = time.monotonic()
with (evidence / f'{name}.txt').open('w') as output:
    result = subprocess.run(command, env=env, stdout=output, stderr=subprocess.STDOUT)
(evidence / f'{name}.result.json').write_text(json.dumps(dict(command=command, exit=result.returncode, seconds=round(time.monotonic()-start, 1)), indent=2)+'\n')
sys.exit(result.returncode)
