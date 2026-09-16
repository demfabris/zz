import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time

root = Path.cwd()
p = root / 'compat/tui/evidence/TUI-017/attempt-04'
base = 'e40a13e1c981aa9b707a5a6a54f5e0267c751de9'
main = root / 'target/capture-12-reconciled-main-src'
main_binary = root / 'target/capture-12-reconciled-main-build/debug/zz'
commands = 'capture-pane,lock-session,lock-client,lock-server,has-session,list-windows'
while not (p / 'environment.json').exists():
    time.sleep(2)

def execute(label, args):
    return subprocess.run(['python3', str(p / 'run.py'), label, *map(str, args)]).returncode

def scope(args):
    return ['systemd-run', '--user', '--scope', '-q', '-p', 'MemoryMax=2G', '-p', 'MemorySwapMax=1G', *args]

def compat_env(binary):
    return ['env', 'ZZ_COMPAT_ZZ=' + str(binary), 'ZZ_COMPAT_TMUX=' + str(root / 'compat/.cache/tmux-src/tmux'), 'ZZ_COMPAT_CORPUS=' + str(root / 'compat/.cache/plugins')]

selection = subprocess.check_output([*compat_env(root / 'target/debug/zz'), 'compat/run.sh', '--delta', base + '...HEAD', '--commands', commands, '--list'], text=True).splitlines()
(p / 'delta-selection.txt').write_text('\n'.join(selection) + '\n')
batches = [selection[i::3] for i in range(3)]
(p / 'delta-batches.json').write_text(json.dumps({'base': base, 'batches': batches, 'total': len(selection)}, indent=2) + '\n')
old_failures = json.loads((p.parent / 'attempt-03/corpus-summary.json').read_text())['failed']
stop = threading.Event()
started = time.time_ns()

def observe():
    seen = set()
    while True:
        for side, source, names in [('candidate', root, selection), ('main', main, old_failures)]:
            for name in names:
                path = source / 'compat/results' / (name.removesuffix('.txt') + '.log')
                try:
                    if path.stat().st_mtime_ns < started:
                        continue
                    text = path.read_text(errors='replace')
                except FileNotFoundError:
                    continue
                summary = next((line for line in reversed(text.splitlines()) if line.startswith('SUMMARY ')), None)
                if summary is None:
                    continue
                digest = hashlib.sha256(text.encode()).hexdigest()
                key = side, name, digest
                if key in seen:
                    continue
                seen.add(key)
                with (p / 'corpus-observations.jsonl').open('a') as output:
                    output.write(json.dumps({'side': side, 'scenario': name, 'sha256': digest, 'summary': summary, 'output': text}) + '\n')
        if stop.is_set():
            return
        time.sleep(1)

observer = threading.Thread(target=observe)
observer.start()

def batch(index):
    return execute('10-delta-' + str(index + 1), scope([*compat_env(root / 'target/debug/zz'), 'compat/run.sh', '--delta', base + '...HEAD', '--commands', commands, '--', *[root / 'compat/scenarios' / name for name in batches[index]]]))

def proofs():
    while 'exit_status:' not in (p / '02-main-build.txt').read_text():
        time.sleep(2)
    if 'exit_status: 0' not in (p / '02-main-build.txt').read_text():
        raise RuntimeError('main build failed')
    for name in ['crates/zz-mux/src/model.rs', 'crates/zz-terminal/src/session.rs', 'crates/zz-daemon/src/daemon.rs', 'crates/zz-protocol/src/message.rs']:
        assert (main / name).read_bytes() == subprocess.check_output(['git', 'show', base + ':' + name])
    (p / 'main-environment.json').write_text(json.dumps({'base': base, 'binary': str(main_binary), 'sha256': hashlib.file_digest(main_binary.open('rb'), 'sha256').hexdigest(), 'source_equality_checked': True}, indent=2) + '\n')
    results = {}
    results['main_corpus'] = execute('11-main-failures', scope([*compat_env(main_binary), main / 'compat/run.sh', *[name.removesuffix('.txt') for name in old_failures]]))
    for i in range(1, 4):
        results['fixture_' + str(i)] = execute('12-fixture-' + str(i), scope(['compat/tui-client-commands.sh']))
    for label, args in [
        ('13-self-check', ['compat/tui-client-commands.sh', '--self-check']),
        ('14-attached-client', ['compat/attached-client.sh', root / 'target/debug/zz', root / 'compat/.cache/tmux-src/tmux']),
        ('15-capture-probe', ['python3', p.parent / 'attempt-03/probe.py']),
        ('16-lock-probe', ['bash', root / 'compat/tui/evidence/TUI-015/attempt-02/lock-probe.sh']),
        ('17-rich-transports', ['bash', p.parent / 'attempt-03/rich-transports-probe.sh']),
        ('18-claims-015', ['python3', 'compat/tui/verify-claims.py', '--run', 'TUI-015']),
        ('19-claims-017', ['python3', 'compat/tui/verify-claims.py', '--run', 'TUI-017'])
    ]:
        results[label] = execute(label, scope(args))
    (p / 'proof-exits.json').write_text(json.dumps(results, indent=2) + '\n')
    return results

try:
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(batch, i) for i in range(3)]
        proof = pool.submit(proofs)
        exits = [future.result() for future in futures]
        (p / 'delta-exits.json').write_text(json.dumps(exits) + '\n')
        proof.result()
finally:
    stop.set()
    observer.join()
print('Measurement finished')
