import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
for name in ['catalog-filtered-retry', 'mux-catalog-filtered']:
    result = evidence / f'{name}.result.json'
    while not result.exists():
        time.sleep(5)
    if json.loads(result.read_text())['exit'] != 0:
        sys.exit(f'{name} must pass before final checks')
env = dict(os.environ, HOME='/tmp/zz-emptyhome', XDG_CONFIG_HOME='/tmp/zz-emptyhome/config', CARGO_HOME='/home/demfabris/.cargo', RUSTUP_HOME='/home/demfabris/.rustup')
env['PATH'] = str(root / 'compat/tui/evidence/TUI-014/attempt-06/capped-bin') + ':' + env['PATH']
env['ZZ_MODES_CARGO_IN_SCOPE'] = '1'
env = {key: value for key, value in env.items() if not any(word in key.upper() for word in ['TOKEN', 'SECRET', 'PASSWORD', 'API_KEY'])}
commands = [
    ('daemon-prefix-controlled', ['/tmp/zz-cargo.sh', 'test', '-p', 'zz-daemon', '--lib', 'switch_client_key_table_and_formats_are_client_local']),
    ('packages-final', ['/tmp/zz-cargo.sh', 'test', '--no-fail-fast', '-p', 'zz-protocol', '-p', 'zz-mux', '-p', 'zz-daemon', '-p', 'zz-tui', '--', '--test-threads=4']),
    ('format-final', ['/tmp/zz-cargo.sh', 'fmt', '--all']),
    ('clippy-final', ['/tmp/zz-cargo.sh', 'clippy', '-p', 'zz-protocol', '-p', 'zz-mux', '-p', 'zz-daemon', '-p', 'zz-tui', '--all-targets', '--all-features', '--', '-D', 'warnings']),
    ('compat-check-final', ['compat/check.sh']),
    ('tracker-check-final', ['python3', 'compat/tui/tracker.py', 'check']),
    ('wire-check-final', ['python3', 'compat/wire-version.py']),
]
for name, command in commands:
    current = dict(env)
    if command[0] != '/tmp/zz-cargo.sh':
        current.pop('ZZ_MODES_CARGO_IN_SCOPE', None)
    launcher = command
    if command[0] == '/tmp/zz-cargo.sh':
        for slot, seed in [(0, 2), (1, 1)]:
            try:
                with open(f'/tmp/zz-cargo-slot-{slot}.lock') as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    fcntl.flock(lock, fcntl.LOCK_UN)
            except (BlockingIOError, FileNotFoundError):
                continue
            launcher = ['bash', '-c', 'RANDOM="$1"; shift; source /tmp/zz-cargo.sh "$@"', 'zz-cargo-slot', str(seed), *command[1:]]
            break
    start = time.monotonic()
    with (evidence / f'{name}.txt').open('w') as output:
        result = subprocess.run(launcher, env=current, stdout=output, stderr=subprocess.STDOUT)
    (evidence / f'{name}.result.json').write_text(json.dumps(dict(command=command, launcher=launcher, exit=result.returncode, seconds=round(time.monotonic()-start, 1)), indent=2)+'\n')
    if name == 'daemon-prefix-controlled' and result.returncode:
        sys.exit(result.returncode)
