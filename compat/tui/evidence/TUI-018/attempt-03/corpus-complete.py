import subprocess
import sys
from pathlib import Path

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-018/attempt-03'
rows = (evidence / 'delta-selection.txt').read_text().splitlines()
part = int(sys.argv[1])
selected = rows[part::2]
assert len(rows) == len(set(rows))
assert part in (0, 1)
print(f'part {part}: {len(selected)} of {len(rows)} selected rows', flush=True)
print('\n'.join(selected), flush=True)
raise SystemExit(subprocess.call([
    'bash', str(evidence / 'run.sh'), f'corpus-complete-{part}',
    'env', f'ZZ_COMPAT_ZZ={root}/target/c11-alias-proof/zz',
    'compat/run.sh', '--strict-geometry', *selected,
]))
