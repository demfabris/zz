import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

root = Path.cwd()
env = {k: v for k, v in os.environ.items() if k not in ('TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_PANE', 'ZZ_SESSION')}
results = {}
with tempfile.TemporaryDirectory(prefix='zzcf-') as home:
    env.update(HOME=home, XDG_CONFIG_HOME=home + '/config', ZZ_TRAY='0')
    for side, binary in [('tmux', root / 'compat/.cache/tmux-src/tmux'), ('zz', root / 'target/debug/zz')]:
        cmd = [str(binary), '-S', home + '/' + side + '.sock']
        if side == 'tmux':
            cmd += ['-f', '/dev/null']
        def run(*args):
            r = subprocess.run(cmd + list(args), env=env, capture_output=True, timeout=15)
            return {'exit': r.returncode, 'stdout': r.stdout.decode(), 'stderr': r.stderr.decode()}
        def ready(marker="NEXT"):
            for _ in range(200):
                if marker in run('capture-pane', '-p', '-t', '=cli:')['stdout']:
                    return
                time.sleep(.05)
            raise RuntimeError('scene did not settle')
        results[side] = {}
        try:
            for cols, rows in [(80, 24), (100, 30)]:
                scenes = {
                    'erase-line': '\x1b[41m\x1b[2K\x1b[0m\r\nNEXT',
                    'erase-display': '\x1b[41m\x1b[2J\x1b[0m\r\nNEXT',
                    'clear': '\x1b[41m\x1b[H\x1b[2J\x1b[0m\r\nNEXT',
                    'scroll-region': '\x1b[2;4r\x1b[2;1H\x1b[41m\x1b[2K\x1b[0m\r\nNEXT\x1b[r',
                    'wide-wrap': '\x1b[31m' + 'A' * (cols - 1) + '界界\x1b[0mNEXT',
                    'tab-trailing': 'ABC\t\r\nNEXT',
                    'tab-internal': 'ABC\tDEF\r\nNEXT',
                    'tab-wide': '界\t\r\nNEXT',
                }
                for name, payload in scenes.items():
                    gate = Path(home) / 'go'
                    gate.unlink(missing_ok=True)
                    command = 'printf READY; while [ ! -f ' + shlex.quote(str(gate)) + ' ]; do sleep .05; done; printf %s ' + shlex.quote('\x1b[H\x1b[2J' + payload) + '; exec sleep 600'
                    assert run('new-session', '-d', '-s', 'cli', '-n', 'win', '-x', str(cols), '-y', str(rows), command)['exit'] == 0
                    ready('READY')
                    outer = [str(root / 'compat/.cache/tmux-src/tmux'), '-S', home + '/outer.sock', '-f', '/dev/null']
                    attach = shlex.join(cmd + ['attach-session', '-t', '=cli'])
                    subprocess.run(outer + ['new-session', '-d', '-s', 'outer', '-x', str(cols), '-y', str(rows + 1), attach], env=env, check=True, capture_output=True)
                    subprocess.run(outer + ['set-option', '-g', 'status', 'off'], env=env, check=True, capture_output=True)
                    subprocess.run(outer + ['new-window', '-d', '-t', '=outer', attach], env=env, check=True, capture_output=True)
                    run('resize-window', '-t', '=cli:', '-x', str(cols), '-y', str(rows))
                    for _ in range(100):
                        if run('display-message', '-p', '-t', '=cli:', '#{pane_width}x#{pane_height}')['stdout'].strip() == f'{cols}x{rows}':
                            break
                        time.sleep(.05)
                    else:
                        raise RuntimeError('geometry did not settle')
                    time.sleep(.2)
                    gate.touch()
                    ready()
                    key = f'{cols}x{rows}/{name}'
                    results[side][key] = {'geometry': run('display-message', '-p', '-t', '=cli:', '#{pane_width}x#{pane_height}')}
                    for label, flags in [('join', ['-L', '-e', '-J']), ('trim', ['-e', '-T']), ('escape', ['-C', '-e']), ('padding', ['-e', '-N']), ('tabs', ['-C'])]:
                        results[side][key][label] = run('capture-pane', '-p', '-t', '=cli:', *flags, '-S', '0', '-E', '4')
                    if cols == 80 and name == 'erase-line':
                        run('resize-window', '-t', '=cli:', '-x', '100', '-y', '30')
                        time.sleep(.2)
                        results[side]['resized-erase'] = {label: run('capture-pane', '-p', '-t', '=cli:', *flags, '-S', '0', '-E', '4') for label, flags in [('join', ['-L', '-e', '-J']), ('trim', ['-e', '-T'])]}
                    subprocess.run(outer + ['kill-server'], env=env, capture_output=True)
                    run('kill-session', '-t', '=cli')
            run('new-session', '-d', '-s', 'cli', '-n', 'win', 'sleep 600')
            run('new-session', '-d', '-s', 'foreign', '-n', 'foreign', 'sleep 600')
            run('set-option', '-g', 'lock-command', 'true')
            pane = run('display-message', '-p', '-t', '=foreign:', '#{pane_id}')['stdout'].strip()
            for target in ['cli:=', '=:', ':.' + pane, pane, 'foreign', 'cli:.' + pane]:
                results[side]['target/' + target] = {command: run(command, '-t', target) for command in ['lock-session', 'has-session', 'list-windows']}
        finally:
            run('kill-server')
print(json.dumps(results, indent=2, ensure_ascii=False))
for key in results['tmux'].keys() & results['zz'].keys():
    for mode in results['tmux'][key]:
        if results['tmux'][key][mode] != results['zz'][key][mode]:
            print('DIFF', key, mode, repr(results['tmux'][key][mode]), repr(results['zz'][key][mode]))
