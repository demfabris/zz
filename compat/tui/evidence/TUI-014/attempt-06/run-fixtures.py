from pathlib import Path
import os
import subprocess
import time

root = Path(__file__).resolve().parents[5]
os.chdir(root)
evidence = Path(__file__).resolve().parent
environment = dict(os.environ, ZZ_BIN=str(root / 'target/debug/zz-modes-proof'), TMUX_BIN=str(root / 'compat/.cache/tmux-src/tmux'), ZZ_COMPAT_ZZ=str(root / 'target/debug/zz-modes-proof'), ZZ_COMPAT_TMUX=str(root / 'compat/.cache/tmux-src/tmux'))
commands = [('client-commands-self-check-seconds', ['compat/tui-client-commands.sh', '--self-check'])]
commands += [(f'client-commands-run-{n}', ['compat/tui-client-commands.sh']) for n in range(1, 4)]
commands += [
    ('choosers', ['compat/tui-choosers.sh']),
    ('choosers-self-check', ['compat/tui-choosers.sh', '--self-check']),
    ('copy-mode', ['compat/tui-copy-mode.sh']),
    ('copy-mode-self-check', ['compat/tui-copy-mode.sh', '--self-check']),
    ('screen-diff', ['compat/tui-screen-diff.sh']),
    ('attached-client', ['compat/attached-client.sh']),
    ('verify-claims', ['python3', 'compat/tui/verify-claims.py', '--run', 'TUI-014', '--zz', str(root / 'target/debug/zz-modes-proof')]),
]
start = os.environ.get('ZZ_MODES_PROOF_START', '')
if start:
    commands = commands[next(index for index, (name, _) in enumerate(commands) if name == start):]
suffix = os.environ.get('ZZ_MODES_PROOF_SUFFIX', '')
for name, command in commands:
    name += suffix
    started = time.time()
    print('START', name, time.strftime('%FT%T%z'), flush=True)
    with (evidence / f'{name}.txt').open('w') as output:
        output.write('COMMAND ' + ' '.join(command) + '\n')
        output.flush()
        result = subprocess.run(command, env=environment, stdout=output, stderr=subprocess.STDOUT)
        output.write(f'\nEXIT {result.returncode}\nELAPSED {time.time() - started:.2f}s\n')
    print('END', name, result.returncode, flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
