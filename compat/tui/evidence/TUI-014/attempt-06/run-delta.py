from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import json
import os
import re
import subprocess
import time

root = Path(__file__).resolve().parents[5]
os.chdir(root)
evidence = Path(__file__).resolve().parent
environment = dict(os.environ, ZZ_COMPAT_ZZ=str(root / 'target/debug/zz-modes-proof'), ZZ_COMPAT_TMUX=str(root / 'compat/.cache/tmux-src/tmux'), ZZ_COMPAT_CORPUS=str(root / 'compat/.cache/plugins'))
commands = 'clock-mode,switch-mode,customize-mode,suspend-client,server-access,choose-client,choose-tree,choose-buffer,capture-pane,set-option,send-keys,send-prefix,copy-mode'
selection_command = ['compat/run.sh', '--delta', 'origin/main...HEAD', '--commands', commands, '--list']
selection = subprocess.check_output(selection_command, env=environment, text=True).splitlines()
(evidence / 'corpus-selection-final.txt').write_text('\n'.join(selection) + '\n')
assert len(selection) == len(set(selection))
groups = [[], []]
weights = [0, 0]
for row in sorted(selection, key=lambda row: len((root / 'compat/scenarios' / row).read_bytes()), reverse=True):
    index = min(range(2), key=weights.__getitem__)
    groups[index].append(row)
    weights[index] += len((root / 'compat/scenarios' / row).read_bytes())
(evidence / 'corpus-groups.json').write_text(json.dumps(groups, indent=2) + '\n')

def run_group(pair):
    index, rows = pair
    command = ['compat/run.sh', '--strict-geometry', *rows]
    started = time.time()
    print('START', index, len(rows), time.strftime('%FT%T%z'), flush=True)
    with (evidence / f'corpus-final-{index}.txt').open('w') as output:
        output.write('COMMAND ZZ_COMPAT_ZZ=' + environment['ZZ_COMPAT_ZZ'] + ' ' + ' '.join(command) + '\n')
        output.flush()
        result = subprocess.run(command, env=environment, stdout=output, stderr=subprocess.STDOUT)
        output.write(f'\nEXIT {result.returncode}\nELAPSED {time.time() - started:.2f}s\n')
    print('END', index, result.returncode, flush=True)
    return {'group': index, 'exit': result.returncode, 'rows': rows}

with ThreadPoolExecutor(max_workers=2) as executor:
    results = list(executor.map(run_group, enumerate(groups)))
(evidence / 'corpus-results.json').write_text(json.dumps(results, indent=2) + '\n')
with (evidence / 'corpus-final-details.txt').open('wb') as output:
    for row in selection:
        path = root / 'compat/results' / Path(row).with_suffix('.log')
        output.write(f'\nFILE {path.relative_to(root)}\n'.encode())
        data = path.read_bytes() if path.exists() else b'MISSING\n'
        data = re.sub(rb'\b(?:gh[oopsur]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})\b', b'[REDACTED_GITHUB_TOKEN]', data)
        output.write(data)
raise SystemExit(0 if all(result['exit'] == 0 for result in results) else 1)
