import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path.cwd()
p = root / 'compat/tui/evidence/TUI-017/attempt-04'
for name in ['01-mux-targets.txt', '03-mux-containment.txt']:
    while 'exit_status:' not in (p / name).read_text():
        time.sleep(2)
    if 'exit_status: 0' not in (p / name).read_text():
        raise SystemExit(name + ' failed')
result = subprocess.run(['python3', str(p / 'run.py'), '04-candidate-build', '/tmp/zz-cargo.sh', 'build', '-p', 'zz'])
if result.returncode:
    raise SystemExit(result.returncode)
sha = lambda path: hashlib.file_digest(Path(path).open('rb'), 'sha256').hexdigest()
(p / 'environment.json').write_text(json.dumps({
    'base': 'e40a13e1c981aa9b707a5a6a54f5e0267c751de9',
    'candidate': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
    'binary': str(root / 'target/debug/zz'),
    'binary_sha256': sha(root / 'target/debug/zz'),
    'fixture_sha256': sha(root / 'compat/tui-client-commands.sh'),
    'pin_sha256': sha(root / 'compat/.cache/tmux-src/tmux'),
    'tmux_commit': 'd77c9dc6aa021e4bc61f0da128c591af695e6466',
    'os': list(os.uname()),
    'cargo_wrapper_sha256': sha('/tmp/zz-cargo.sh'),
    'credentials_removed_from_child_environment': True
}, indent=2) + '\n')
