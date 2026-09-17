import json
import os
from pathlib import Path
import shlex
import statistics
import subprocess
import sys
import tempfile
import time

output = Path(__file__).resolve().parent
paths = dict(zip(['candidate', 'main'], map(os.path.abspath, sys.argv[1:3])))
assert len(paths) == 2
samples = []
env = {k: v for k, v in os.environ.items() if k not in ['TMUX', 'TMUX_PANE', 'ZZ_SOCKET', 'ZZ_PANE', 'ZZ_SESSION']}
with tempfile.TemporaryDirectory(prefix='zzstart-') as work:
    env.update(HOME=work, XDG_CONFIG_HOME=work + '/config', LC_ALL='C')
    prefixes = {side: [binary, '-S', work + '/' + side + '.sock'] for side, binary in paths.items()}

    def run(side, *args):
        result = subprocess.run(prefixes[side] + list(args), env=env, capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, (side, args, result)
        return result.stdout

    try:
        for side in paths:
            run(side, 'new-session', '-d', '-s', 'other', 'sleep 600')
            run(side, 'new-session', '-d', '-s', 'splitwait', 'sleep 600')
        for iteration in range(42):
            for side in (['candidate', 'main'] if iteration % 2 == 0 else ['main', 'candidate']):
                ready = Path(work) / 'ready'
                release = Path(work) / 'release'
                ready.unlink(missing_ok=True)
                release.unlink(missing_ok=True)
                os.mkfifo(release)
                shell = 'date +%s%N > ' + shlex.quote(str(ready)) + '; read release < ' + shlex.quote(str(release))
                before = time.time_ns()
                child = subprocess.Popen(prefixes[side] + ['split-window', '-d', '-W', '-t', '=splitwait:', shell], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                deadline = time.monotonic() + 30
                while True:
                    timestamp = ready.read_text().strip() if ready.exists() else ''
                    if timestamp:
                        break
                    assert time.monotonic() < deadline, (side, iteration, 'child did not start')
                    time.sleep(.001)
                after = time.time_ns()
                parked = child.poll() is None
                query_start = time.time_ns()
                panes = run(side, 'list-panes', '-t', '=splitwait:', '-F', '#{session_name}:#{window_index}:#{pane_id}:#{pane_dead}').splitlines()
                query_ms = (time.time_ns() - query_start) / 1e6
                other = run(side, 'list-panes', '-t', '=other:', '-F', '#{pane_id}').splitlines()
                with release.open('w') as stream:
                    stream.write('release\n')
                stdout, stderr = child.communicate(timeout=30)
                row = dict(side=side, iteration=iteration, load_average=os.getloadavg(), warmup=iteration < 2, invocation_ns=before, child_start_ns=int(timestamp), observed_ns=after, startup_ms=(int(timestamp)-before)/1e6, ready_observation_ms=(after-before)/1e6, query_ms=query_ms, parked=parked, panes=panes, other_panes=other, exit=child.returncode, stdout=stdout.decode(), stderr=stderr.decode())
                samples.append(row)
                print(json.dumps(row), flush=True)
                (output / 'startup-samples.json').write_text(json.dumps(samples, indent=2) + '\n')
                assert parked and len(panes) == 2 and len(other) == 1 and child.returncode == 0 and not stdout and not stderr, row
                assert all(pane.startswith('splitwait:0:') and pane.endswith(':0') for pane in panes), row
    finally:
        for side in paths:
            subprocess.run(prefixes[side] + ['kill-server'], env=env, capture_output=True, timeout=30)

def distribution(values):
    ordered = sorted(values)
    return dict(n=len(values), min=min(values), median=statistics.median(values), p90=ordered[int(.9*(len(values)-1))], p95=ordered[int(.95*(len(values)-1))], max=max(values), mean=statistics.mean(values))

measured = {side: [row for row in samples if row['side'] == side and not row['warmup']] for side in paths}
summary = {side: {metric: distribution([row[metric] for row in rows]) for metric in ['startup_ms', 'query_ms']} for side, rows in measured.items()}
deltas = [candidate['startup_ms'] - main['startup_ms'] for candidate, main in zip(measured['candidate'], measured['main'])]
summary['paired_candidate_minus_main_ms'] = distribution(deltas)
summary['candidate_slower_pairs'] = sum(delta > 0 for delta in deltas)
(output / 'startup-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2), flush=True)
