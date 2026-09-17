import json
from pathlib import Path
import subprocess
import tempfile

root = Path.cwd()
evidence = Path(__file__).resolve().parent
source = (root / 'compat/scenarios/smoke/fixtures/split-window-wait.sh').read_text()
start = source.index('rm -f "$work/parked.exit"')
for name in ['early-return', 'wrong-window']:
    before, parked = source[:start], source[start:]
    if name == 'early-return':
        parked = parked.replace('main_client split-window -d -W', 'main_client split-window -d $parked_wait', 1)
        before += 'parked_wait=-W\n[ "$side" != zz ] || parked_wait=\n'
    else:
        parked = parked.replace('-t "=$session:"', '-t "=$parked_session:"', 1)
        before += 'parked_session="$session"\nif [ "$side" = zz ]; then parked_session=misplaced; main_client new-session -d -s "$parked_session"; fi\n'
    with tempfile.TemporaryDirectory(prefix='capture-sabotage-', dir=root / 'compat/scenarios/smoke') as scratch:
        scratch = Path(scratch)
        (scratch / 'fixture.sh').write_text(before + parked)
        scenario = scratch / (name + '.txt')
        scenario.write_text("corpus: none\nstage: fixture.sh ~/split-window-wait.sh\nrun-shell 'sh ~/split-window-wait.sh'\nout: show-environment -g SPLIT_WINDOW_WAIT\n")
        command = ['compat/diff-scenario.sh', '--strict-geometry', str(scenario), str(root / 'target/capture-12-resumed-proof/zz'), str(root / 'compat/.cache/tmux-src/tmux')]
        result = subprocess.run(command, capture_output=True, text=True)
        relative = scenario.relative_to(root / 'compat/scenarios').with_suffix('.log')
        log = (root / 'compat/results' / relative).read_text()
        (evidence / ('sabotage-' + name + '.txt')).write_text('command: ' + json.dumps(command) + '\n' + result.stdout + result.stderr + '\n' + log + f'\nexit_status: {result.returncode}\n')
        expected = 'parked returned=[0] panes=2' if name == 'early-return' else 'parked returned=[] panes=1'
        caught = result.returncode == 1 and expected in log
        print(name, 'caught' if caught else 'NOT CAUGHT', result.returncode, flush=True)
        assert caught
