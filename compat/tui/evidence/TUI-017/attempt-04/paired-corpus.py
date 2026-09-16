import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path.cwd()
p = root / 'compat/tui/evidence/TUI-017/attempt-04'
scenario = sys.argv[1]
count = int(sys.argv[2])
label = scenario.replace('/', '-')
results = []
for iteration in range(1, count + 1):
    for side, source, binary in [('main', root / 'target/capture-12-reconciled-main-src', root / 'target/capture-12-reconciled-main-build/debug/zz'), ('candidate', root, root / 'target/debug/zz')]:
        env = dict(os.environ, ZZ_COMPAT_ZZ=str(binary), ZZ_COMPAT_TMUX=str(root / 'compat/.cache/tmux-src/tmux'), ZZ_COMPAT_CORPUS=str(root / 'compat/.cache/plugins'))
        command = [str(source / 'compat/run.sh'), scenario]
        result = subprocess.run(command, env=env, capture_output=True, text=True)
        output = (source / 'compat/results' / (scenario + '.log')).read_text()
        name = f'paired-{label}-{iteration}-{side}'
        (p / (name + '.txt')).write_text('command: ' + json.dumps(command) + '\n' + result.stdout + result.stderr + f'\nexit_status: {result.returncode}\n')
        (p / (name + '.log')).write_text(output)
        entry = {'iteration': iteration, 'side': side, 'scenario': scenario, 'exit_status': result.returncode, 'sha256': hashlib.sha256(output.encode()).hexdigest(), 'summary': next(line for line in output.splitlines() if line.startswith('SUMMARY ')), 'difference_lines': [line for line in output.splitlines() if line.startswith(('-', '+')) and not line.startswith(('---', '+++'))]}
        results.append(entry)
        print(json.dumps(entry), flush=True)
        (p / ('paired-' + label + '.json')).write_text(json.dumps(results, indent=2) + '\n')
