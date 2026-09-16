import concurrent.futures
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

root = Path.cwd()
folder = root / 'compat/tui/evidence/TUI-017/attempt-03'
pid = int(sys.argv[1])
assert Path(f'/proc/{pid}/cwd').resolve() == root
assert b'compat/run.sh' in Path(f'/proc/{pid}/cmdline').read_bytes()
os.kill(pid, signal.SIGSTOP)
original = (folder / '28-delta-corpus.txt').read_text()
started = re.findall(r'running ([^\n]+)', re.sub(r'\x1b\[[0-9;]*m', '', original))
selection = (folder / '14-delta-selection.txt').read_text().splitlines()
selected = [root / 'compat/scenarios' / line for line in selection if line.endswith('.txt')]
assert len(selected) == 163, len(selected)
names = {str(p.relative_to(root / 'compat/scenarios')).removesuffix('.txt'): p for p in selected}
assert all(name in names for name in started), started
remaining = [str(p) for name, p in names.items() if name not in started]
batches = [remaining[i::3] for i in range(3)]
manifest = {'prefix': started, 'batches': batches, 'total': len(selected), 'original_runner_pid': pid}
(folder / 'delta-batches.json').write_text(json.dumps(manifest, indent=2) + '\n')
env = dict(os.environ, ZZ_COMPAT_ZZ=str(root / 'target/debug/zz'), ZZ_COMPAT_TMUX=str(root / 'compat/.cache/tmux-src/tmux'), ZZ_COMPAT_CORPUS=str(root / 'compat/.cache/plugins'))

def run_batch(index):
    return subprocess.run(['python3', str(folder / 'run.py'), f'38-delta-batch-{index + 1}', 'systemd-run', '--user', '--scope', '-q', '-p', 'MemoryMax=2G', '-p', 'MemorySwapMax=1G', 'env', *[f'{key}={env[key]}' for key in ('ZZ_COMPAT_ZZ', 'ZZ_COMPAT_TMUX', 'ZZ_COMPAT_CORPUS')], 'compat/run.sh', '--delta', 'origin/main...HEAD', '--commands', 'capture-pane,lock-session,lock-client,lock-server,has-session,list-windows', '--', *batches[index]], env=env).returncode

with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
    futures = [pool.submit(run_batch, i) for i in range(3)]
    active_log = root / 'compat/results' / (started[-1] + '.log')
    while not any(line.startswith('SUMMARY ') for line in active_log.read_text().splitlines()):
        time.sleep(1)
    os.kill(pid, signal.SIGTERM)
    os.kill(pid, signal.SIGCONT)
    results = [future.result() for future in futures]
(folder / 'delta-batch-exits.json').write_text(json.dumps(results) + '\n')
print('delta batches:', results)
sys.exit(1 if any(results) else 0)
