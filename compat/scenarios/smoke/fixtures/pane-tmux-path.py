import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)
work = Path(sys.argv[1])
prefix = sys.argv[2:]
work.mkdir(parents=True, exist_ok=True)


def cli(*args):
    result = subprocess.run([*prefix, *args], capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, (args, result.returncode, result.stdout, result.stderr)
    return result.stdout.strip()


def wait_for(predicate, label):
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.04)
    raise AssertionError(label)


def run_in_pane(pane, name, command):
    done = work / (name + '.done')
    out = work / (name + '.out')
    err = work / (name + '.err')
    script = work / (name + '.sh')
    script.write_text(command + '\nresult=$?\nprintf "%s\\n" "$result" > ' + shlex.quote(str(done)) + '\n')
    invocation = 'sh ' + shlex.quote(str(script)) + ' > ' + shlex.quote(str(out)) + ' 2> ' + shlex.quote(str(err))
    cli('send-keys', '-t', pane, '-l', invocation)
    cli('send-keys', '-t', pane, 'Enter')
    wait_for(lambda: done.exists() and done.read_text().strip(), name + ' completion')
    return int(done.read_text()), out.read_bytes(), err.read_bytes()


pane = cli('display-message', '-p', '-t', 'w:0', '#{pane_id}')
result, out, err = run_in_pane(pane, 'route', "timeout 15 tmux display -p '#{pane_id}'")
assert (result, out, err) == (0, (pane + '\n').encode(), b''), ('pane route', result, out, err)
print('pane-route=' + out.decode().strip())
result, out, err = run_in_pane(pane, 'nested', 'timeout 15 tmux')
assert (result, out, err) == (1, b'', b'sessions should be nested with care, unset $TMUX to force\n'), ('nested', result, out, err)
print('nested-status=' + str(result))
print(err.decode(), end='')
plugin = Path(os.environ['HOME']) / '.tmux/plugins/vim-tmux-navigator/vim-tmux-navigator.tmux'
cli('set-option', '-g', '@vim_navigator_check', 'false')
result, out, err = run_in_pane(pane, 'navigator', 'timeout 20 bash ' + shlex.quote(str(plugin)))
assert (result, out, err) == (0, b'', b''), ('navigator', result, out, err)
binding = cli('list-keys', '-T', 'root', 'C-l')
assert 'select-pane -R' in binding and 'if-shell' in binding, binding
print(binding)
nvim = shutil.which('nvim')
assert nvim, 'pane navigator proof requires nvim'
right = cli('split-window', '-h', '-t', pane, '-P', '-F', '#{pane_id}')
cli('select-pane', '-t', pane)
vim_plugin = plugin.parent / 'plugin/tmux_navigator.vim'
command = shlex.join([nvim, '--headless', '-u', 'NONE', '-i', 'NONE', '--noplugin', '-n', '-c', 'source ' + str(vim_plugin), '-c', 'TmuxNavigateRight', '-c', 'qa!'])
result, out, err = run_in_pane(pane, 'navigator-vim', 'timeout 20 ' + command)
assert (result, out, err) == (0, b'', b''), ('navigator Vim', result, out, err)
wait_for(lambda: cli('display-message', '-p', '-t', 'w:0', '#{pane_id}') == right, 'navigator selects right pane')
print('navigator-vim=right-pane')
cli('kill-pane', '-t', right)
cli('respawn-pane', '-k', '-t', pane, '/bin/sh')
result, out, err = run_in_pane(pane, 'respawn-route', "timeout 15 tmux display -p '#{pane_id}'")
assert (result, out, err) == (0, (pane + '\n').encode(), b''), ('respawn route', result, out, err)
print('respawn-route=' + out.decode().strip())
result, out, err = run_in_pane(pane, 'respawn-nested', 'timeout 15 tmux')
assert (result, out, err) == (1, b'', b'sessions should be nested with care, unset $TMUX to force\n'), ('respawn nested', result, out, err)
print('respawn-nested-status=' + str(result))
cli('set-environment', '-g', 'PANE_TMUX_PATH', 'clean')
