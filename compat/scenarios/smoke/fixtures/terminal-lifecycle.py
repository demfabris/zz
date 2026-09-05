import argparse
import os
import pathlib
import shlex
import subprocess
import sys
import tempfile
import time

sys.stdout.reconfigure(line_buffering=True)
parser = argparse.ArgumentParser()
parser.add_argument('--zz', required=True)
parser.add_argument('--tmux', required=True)
args = parser.parse_args()


def probe(side, root):
    root.mkdir()
    env = os.environ.copy()
    for name in ['TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_SESSION', 'ZZ_PANE']:
        env.pop(name, None)
    env.update(HOME=str(root), XDG_CONFIG_HOME=str(root), TERM='xterm-256color')
    socket = f'/tmp/zzprobe-lifecycle-{os.getpid()}.sock'
    base = ([str(pathlib.Path(args.zz).resolve()), '--socket', socket, '-f', '/dev/null']
            if side == 'zz' else
            [args.tmux, '-L', f'zzprobe-lifecycle-{os.getpid()}', '-f', '/dev/null'])

    def tmux(*command, check=True):
        result = subprocess.run([*base, *command], capture_output=True, env=env, timeout=15)
        if check and result.returncode:
            raise RuntimeError((command, result.returncode, result.stderr))
        return result.stdout.decode().strip()

    fifo = root / 'input'
    os.mkfifo(fifo)
    fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
    daemon = None
    observed = []
    with (root / 'daemon.log').open('wb') as log:
        try:
            if side == 'zz':
                daemon = subprocess.Popen([*base, 'daemon'], env=env, stdout=log, stderr=log)
                deadline = time.monotonic() + 10
                while not pathlib.Path(socket).exists():
                    if daemon.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError('daemon did not start')
                    time.sleep(0.02)
            script = root / 'pane.sh'
            script.write_text('''while IFS= read -r action; do
    case "$action" in
        start) seq 1 100 ;;
        alt|alt2) printf '\\033[?1049h' ;;
        primary|primary2) printf '\\033[?1049l' ;;
        clear) printf '\\033[3J' ;;
        reset) printf '\\033c' ;;
    esac
    : > "$1/$action"
done < "$1/input"
''')
            tmux('new-session', '-d', '-s', 'life', '-n', 'lifecycle', '-x', '80', '-y', '24',
                 shlex.join(['sh', str(script), str(root)]))
            tmux('set-option', '-g', 'status', 'off')
            tmux('resize-window', '-t', 'life:0', '-x', '80', '-y', '24')
            for action in ['start', 'alt', 'clear', 'primary', 'alt2', 'reset', 'primary2']:
                os.write(fd, (action + '\n').encode())
                deadline = time.monotonic() + 10
                while not (root / action).exists():
                    if time.monotonic() >= deadline:
                        raise RuntimeError('missing marker ' + action)
                    time.sleep(0.02)
                previous = None
                stable = 0
                while time.monotonic() < deadline:
                    value = tmux('display-message', '-p', '-t', 'life:0',
                                 '#{pane_width}x#{pane_height}:#{history_size} #{cursor_x} #{cursor_y} #{alternate_on}')
                    if not value.startswith('80x24:'):
                        raise AssertionError(('equal pane geometry', value))
                    stable = stable + 1 if value == previous else 0
                    if stable == 5:
                        break
                    previous = value
                    time.sleep(0.05)
                else:
                    raise RuntimeError('unsettled facts ' + action)
                observed.append(value)
                print(side, action, value)
        finally:
            tmux('kill-server', check=False)
            if daemon is not None:
                try:
                    daemon.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait(timeout=5)
            os.close(fd)
    return observed


with tempfile.TemporaryDirectory(prefix='zzprobe-lifecycle-') as directory:
    expected = {
        'pin': ['80x24:77 0 23 0', '80x24:77 0 23 1', '80x24:0 0 23 1',
                '80x24:0 0 23 0', '80x24:0 0 23 1', '80x24:0 0 0 1', '80x24:0 0 23 0'],
        'zz': ['80x24:77 0 23 0', '80x24:77 0 23 1', '80x24:77 0 23 1',
               '80x24:77 0 23 0', '80x24:77 0 23 1', '80x24:0 0 0 0', '80x24:0 0 0 0'],
    }
    for side in ['pin', 'zz']:
        observed = probe(side, pathlib.Path(directory) / side)
        if observed != expected[side]:
            raise AssertionError((side, expected[side], observed))
