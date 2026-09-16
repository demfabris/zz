import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

root = Path.cwd()
with tempfile.TemporaryDirectory(prefix='zzc12time-') as home:
    env = {key: value for key, value in os.environ.items() if key not in ['TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_PANE', 'ZZ_SESSION']}
    env.update(HOME=home, XDG_CONFIG_HOME=home + '/config', LC_ALL='C')
    for side, binary in [('main', 'target/capture-12-reconciled-main-build/debug/zz'), ('candidate', 'target/debug/zz')]:
        cmd = [str(root / binary), '-S', home + '/' + side + '.sock']
        def run(args):
            return subprocess.run(cmd + args, env=env, capture_output=True, text=True, timeout=20)
        try:
            assert run(['new-session', '-d', '-s', 'splitwait', 'sleep 600']).returncode == 0
            for i in range(3):
                start = time.monotonic()
                child = subprocess.Popen(cmd + ['split-window', '-d', '-W', '-t', '=splitwait:', 'sleep 1'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                time.sleep(.4)
                before = time.monotonic() - start
                panes = run(['list-panes', '-t', '=splitwait:', '-F', '#{pane_id}'])
                after = time.monotonic() - start
                stdout, stderr = child.communicate(timeout=20)
                print(json.dumps({'side': side, 'iteration': i + 1, 'list_started': before, 'list_finished': after, 'panes': panes.stdout, 'pane_exit': panes.returncode, 'pane_stderr': panes.stderr, 'split_exit': child.returncode, 'split_stdout': stdout.decode(), 'split_stderr': stderr.decode()}), flush=True)
        finally:
            run(['kill-server'])
