import subprocess
from pathlib import Path

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-018/attempt-03'
rows = (evidence / 'delta-extra-selection.txt').read_text().splitlines()
print(f'additional selection: {len(rows)} rows', flush=True)
print('\n'.join(rows), flush=True)
raise SystemExit(subprocess.call([
    'bash', str(evidence / 'run.sh'), 'corpus-complete-extra',
    'env', f'ZZ_COMPAT_ZZ={root}/target/c11-alias-proof/zz',
    'compat/run.sh', '--strict-geometry', *rows,
]))
