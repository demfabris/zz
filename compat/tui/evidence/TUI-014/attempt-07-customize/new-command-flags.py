import json
from pathlib import Path
import subprocess
import sys

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
tmux = [str(root / 'compat/.cache/tmux-src/tmux'), '-L', 'zz-customize-new-flags']
zz = [str(root / 'target/debug/zz-customize-final'), '--socket', '/tmp/zz-customize-new-flags.sock']
rows = []
subprocess.run([*tmux, '-f', '/dev/null', 'new-session', '-d', 'sleep 60'], check=True)
try:
    for name, alias, required in [('customize-mode', None, '-F'), ('suspend-client', 'suspendc', '-t')]:
        commands = [[name, value] for value in ['-0', '-@', '--bogus', '-?', required]]
        if alias:
            commands.append([alias, '-0'])
        for args in commands:
            results = []
            for base in [zz, tmux]:
                result = subprocess.run([*base, *args], capture_output=True, text=True)
                results.append(dict(exit=result.returncode, stdout=result.stdout, stderr=result.stderr))
            rows.append(dict(arguments=args, zz=results[0], tmux=results[1], identical=results[0] == results[1]))
finally:
    subprocess.run([*tmux, 'kill-server'], check=True)
(evidence / 'new-command-flags.json').write_text(json.dumps(rows, indent=2) + '\n')
failures = sum(not row['identical'] for row in rows)
print(f'{len(rows)} exact CLI comparisons, {failures} failures')
sys.exit(bool(failures))
