import json
import os
from pathlib import Path
import subprocess
import tempfile

root = Path.cwd()
env = {k: v for k, v in os.environ.items() if k not in ('TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_PANE', 'ZZ_SESSION')}
results = {}
with tempfile.TemporaryDirectory(prefix='zzexact-') as home:
    env.update(HOME=home, XDG_CONFIG_HOME=home + '/config', ZZ_TRAY='0')
    for side, binary in [('tmux', root / 'compat/.cache/tmux-src/tmux'), ('zz', Path(os.environ['ZZ_COMPAT_ZZ']))]:
        cmd = [str(binary), '-S', home + '/' + side + '.sock']
        if side == 'tmux':
            cmd += ['-f', '/dev/null']
        def run(*args):
            r = subprocess.run(cmd + list(args), env=env, capture_output=True, timeout=20)
            return {'exit': r.returncode, 'stdout': r.stdout.decode(), 'stderr': r.stderr.decode()}
        results[side] = {}
        try:
            setup = [
                ['new-session', '-d', '-s', 'cli', '-n', 'win', 'sleep 600'],
                ['new-window', '-d', '-t', '=cli', '-n', 'other', 'sleep 600'],
                ['new-session', '-d', '-s', 'foreign', '-n', 'foreign', 'sleep 600'],
            ]
            layout = os.environ.get('ZZ_EXACT_PANE_ZERO', 'current-window')
            if layout == 'foreign-session':
                setup = [setup[2], setup[0], setup[1]]
            if layout == 'other-window':
                setup.append(['select-window', '-t', '=cli:other'])
            for args in setup:
                r = run(*args)
                assert r['exit'] == 0, r
            for target in ['cli:=.0', 'cli:=.%0', '=cli:=.0', 'cli:=.%1', 'cli:=.%2', ':=.0', 'cli:=.9', 'cli:=.', 'cli:=', 'cli:win.0', ':=.%2', '=:=.%2']:
                for command in ['lock-session', 'has-session', 'list-windows']:
                    args = [command, '-t', target]
                    if command == 'list-windows':
                        args += ['-F', '#{window_index}:#{window_name}']
                    results[side][command + ' ' + target] = run(*args)
        finally:
            run('kill-server')
different = [key for key in results['tmux'] if results['tmux'][key] != results['zz'][key]]
print(json.dumps({'layout': layout, 'comparisons': len(results['tmux']), 'different': different, 'results': results}, indent=2))
