import json
import os
from pathlib import Path
import subprocess
import sys

root = Path.cwd()
p = Path(__file__).resolve().parent
label = sys.argv[1] if len(sys.argv) > 1 else "direct"
for side, binary in [('candidate', root / 'target/capture-12-before-final-rebase/zz'), ('main', root / 'target/capture-12-reconciled-main-build/debug/zz')]:
    command = ['compat/diff-scenario.sh', '--strict-geometry', 'compat/scenarios/smoke/split-window-wait.txt', str(binary), str(root / 'compat/.cache/tmux-src/tmux')]
    env = dict(os.environ, ZZ_COMPAT_ZZ=str(binary))
    result = subprocess.run(command, env=env, capture_output=True, text=True)
    (p / (label + '-' + side + '.txt')).write_text('command: ' + json.dumps(command) + '\n' + result.stdout + result.stderr + f'\nexit_status: {result.returncode}\n')
    output = (root / 'compat/results/smoke/split-window-wait.log').read_text()
    (p / (label + '-transcript-' + side + '.txt')).write_text(output)
    print(side, result.returncode, output.splitlines()[-1], flush=True)
