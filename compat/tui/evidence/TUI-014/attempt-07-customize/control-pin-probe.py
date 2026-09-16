import os, subprocess, time, signal
from pathlib import Path
root = Path.cwd()
pin = root / 'compat/.cache/tmux-src/tmux'
socket = f'/tmp/zzcp-{os.getpid()}'
env = dict(os.environ, HOME='/tmp/zz-emptyhome', XDG_CONFIG_HOME='/tmp/zz-emptyhome/config')
for name in ['TMUX', 'TMUX_PANE', 'ZZ_PANE', 'ZZ_SOCKET']:
    env.pop(name, None)
p = subprocess.Popen([str(pin), '-S', socket, '-f', '/dev/null', '-C', 'new-session', '-s', 'control', '/bin/sh'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
try:
    time.sleep(.4)
    p.stdin.write(b'suspend-client\n'); p.stdin.flush()
    time.sleep(.4)
    state = subprocess.run(['ps', '-o', 'stat=', '-p', str(p.pid)], capture_output=True, text=True)
    print('control state', state.stdout.strip(), 'alive', p.poll() is None)
    os.kill(p.pid, signal.SIGCONT)
    p.stdin.write(b'display-message -p CONTROL-ALIVE\ndetach-client\n'); p.stdin.flush()
    out, err = p.communicate(timeout=5)
    print('stdout', repr(out), 'stderr', repr(err), 'exit', p.returncode)
finally:
    subprocess.run([str(pin), '-S', socket, 'kill-server'], env=env, capture_output=True)
    if p.poll() is None:
        os.kill(p.pid, signal.SIGCONT); p.terminate(); p.wait()
