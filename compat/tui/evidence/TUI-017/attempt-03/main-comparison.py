import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path.cwd()
folder = root / 'compat/tui/evidence/TUI-017/attempt-03'
log = folder / '40-main-build.txt'
while 'exit_status:' not in log.read_text():
    time.sleep(2)
if 'exit_status: 0' not in log.read_text():
    raise SystemExit('main build failed; comparison not run')
source = root / 'target/capture-12-main-src'
binary = root / 'target/capture-12-main-build/debug/zz'
base = json.loads((folder / 'environment.txt').read_text())['base']
for path in ['crates/zz-mux/src/model.rs', 'crates/zz-terminal/src/session.rs', 'crates/zz-daemon/src/daemon.rs', 'crates/zz-protocol/src/message.rs']:
    assert (source / path).read_bytes() == subprocess.check_output(['git', 'show', base + ':' + path])
(folder / 'main-environment.txt').write_text(json.dumps({'source_revision': base, 'source': str(source), 'binary': str(binary), 'sha256': hashlib.file_digest(binary.open('rb'), 'sha256').hexdigest(), 'source_files_checked_against_git': True}, indent=2) + '\n')
result = subprocess.run(['python3', str(folder / 'run.py'), '41-main-corpus', 'systemd-run', '--user', '--scope', '-q', '-p', 'MemoryMax=2G', '-p', 'MemorySwapMax=1G', 'env', 'ZZ_COMPAT_ZZ=' + str(binary), 'ZZ_COMPAT_TMUX=' + str(root / 'compat/.cache/tmux-src/tmux'), 'ZZ_COMPAT_CORPUS=' + str(root / 'compat/.cache/plugins'), str(source / 'compat/run.sh'), 'smoke/chooser-tree-vocabulary', 'smoke/display-menu-mouse', 'smoke/alias-group-forgery'])
sys.exit(result.returncode)
