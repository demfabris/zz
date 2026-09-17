import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time

root = Path.cwd()
p = Path(__file__).resolve().parent
label = sys.argv[1] if len(sys.argv) > 1 else "lifetime"
observations = []
with tempfile.TemporaryDirectory(prefix='zzlife-') as home:
    env = {key: value for key, value in os.environ.items() if key not in ['TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_PANE', 'ZZ_SESSION']}
    env.update(HOME=home, XDG_CONFIG_HOME=home + '/config', LC_ALL='C')
    for side, path in [('candidate', 'target/debug/zz'), ('main', 'target/capture-12-reconciled-main-build/debug/zz'), ('pin', 'compat/.cache/tmux-src/tmux')]:
        prefix = [str(root / path), '-S', home + '/' + side + '.sock']
        if side == 'pin': prefix += ['-f', '/dev/null']
        def run(*args):
            before = time.time_ns()
            result = subprocess.run(prefix + list(args), env=env, capture_output=True, text=True, timeout=20)
            return {'start_ns': before, 'end_ns': time.time_ns(), 'exit': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr}
        try:
            assert run('new-session', '-d', '-s', 'w', 'sleep 600')['exit'] == 0
            for retain in [False, True]:
                for iteration in range(3):
                    assert run('new-session', '-d', '-s', 'splitwait', 'sleep 600')['exit'] == 0
                    if retain: assert run('set-window-option', '-t', '=splitwait:', 'remain-on-exit', 'on')['exit'] == 0
                    start_file = Path(home) / 'start'
                    end_file = Path(home) / 'end'
                    start_file.unlink(missing_ok=True)
                    end_file.unlink(missing_ok=True)
                    shell = 'date +%s%N > ' + shlex.quote(str(start_file)) + '; sleep 1; date +%s%N > ' + shlex.quote(str(end_file))
                    split_start = time.time_ns()
                    child = subprocess.Popen(prefix + ['split-window', '-d', '-W', '-t', '=splitwait:', shell], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    if "started" in label:
                        deadline = time.monotonic() + 15
                        while not start_file.exists() and time.monotonic() < deadline:
                            time.sleep(.01)
                        assert start_file.exists()
                    time.sleep(.4)
                    queried = run('list-panes', '-t', '=splitwait:', '-F', '#{session_name}:#{window_index}:#{pane_id}:dead=#{pane_dead}')
                    everywhere = run('list-panes', '-a', '-F', '#{session_name}:#{window_index}:#{pane_id}:dead=#{pane_dead}')
                    stdout, stderr = child.communicate(timeout=20)
                    row = {'side': side, 'retain': retain, 'iteration': iteration + 1, 'split_start_ns': split_start, 'child_start_ns': int(start_file.read_text()), 'child_end_ns': int(end_file.read_text()), 'query': queried, 'all_windows': everywhere, 'split_exit': child.returncode, 'split_stdout': stdout.decode(), 'split_stderr': stderr.decode()}
                    observations.append(row)
                    print(json.dumps(row), flush=True)
                    run('kill-session', '-t', '=splitwait')
        finally:
            run('kill-server')
(p / (label + '.json')).write_text(json.dumps(observations, indent=2) + '\n')
