import json
import os
from pathlib import Path
import subprocess
import tempfile

root = Path.cwd()
cases = {
    'refresh-missing-argument': ['refresh-client', '-r'],
    'lock-server-arity': ['lock-server', 'zzcc-extra'],
    'lock-server-unknown-flag': ['lock-server', '-t', 'zzcc-nope'],
    'lock-session-arity': ['lock-session', 'zzcc-extra'],
    'lock-client-arity': ['lock-client', 'zzcc-extra'],
    'lock-client-missing-argument': ['lock-client', '-t'],
    'client-tree-unknown-flag': ['choose-client', '-Q', '-t', '%0'],
    'client-tree-usage': ['choose-client', '-t', '%0', 'one', 'two'],
}
results = {}
with tempfile.TemporaryDirectory(prefix='zzc12usage-') as home:
    env = {key: value for key, value in os.environ.items() if key not in ['TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_PANE', 'ZZ_SESSION']}
    env.update(HOME=home, XDG_CONFIG_HOME=home + '/config', LC_ALL='C')
    for side, binary in [('pin', 'compat/.cache/tmux-src/tmux'), ('main', 'target/capture-12-reconciled-main-build/debug/zz'), ('candidate', 'target/debug/zz')]:
        cmd = [str(root / binary), '-S', home + '/' + side + '.sock']
        if side == 'pin':
            cmd += ['-f', '/dev/null']
        def run(args):
            result = subprocess.run(cmd + args, env=env, capture_output=True, timeout=20)
            return {'exit': result.returncode, 'stdout': result.stdout.decode(), 'stderr': result.stderr.decode()}
        try:
            assert run(['new-session', '-d', '-s', 'cli', 'sleep 600'])['exit'] == 0
            results[side] = {name: run(args) for name, args in cases.items()}
        finally:
            run(['kill-server'])
print(json.dumps(results, indent=2))
assert results['main'] == results['candidate']
for name in cases:
    assert results['pin'][name]['exit'] == 1
    assert results['main'][name]['exit'] == 2
    for channel in ['stdout', 'stderr']:
        assert results['main'][name][channel] == results['pin'][name][channel]
print('All eight shared-fixture failures reproduce on clean e40a13e1: exact stdout/stderr, main and candidate exit 2 versus pin exit 1')
