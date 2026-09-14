#!/usr/bin/env python3
"""Tell a spinning agent from a slow one, on a timer, for a whole cycle.

    python3 compat/tui/agentwatch.py

Reads every workflow run still moving and prints a line only when something is
wrong. Run it from a 15-minute monitor for the length of a cycle. fabrico asked
for this in cycle 8 after two cycles ran a day longer than they should have and
nobody could say which agent was stuck.

What it judges, and why each rule is shaped the way it is:

  SPINNING   the identical FULL command four times in the last sixty calls. The
             first version truncated commands at 180 characters, which made a
             series of different file edits look like one repeated command.
  STUCK      the identical FAILURE signature four times, read off tool RESULTS
             rather than command text. Different failures in a row are a fix
             loop making progress and must never be flagged. The first version
             matched "timeout 580" in a command and called it a timeout.
  SILENT     no transcript write in twenty minutes. A label whose live attempt
             already returned is dropped whole: the harness leaves a dead
             earlier attempt behind under the same label, and without that rule
             the finished lane reappears as the newest unfinished transcript.
  NO COMMIT  many new calls and nothing landed in three hours, workers only.
  SECOND HALF  a worker finished with its obligation still active. That is an
             opportunity, not a failure. A runner's gates are serial, so while
             they run every slot but one is idle; if the worker's report names
             the files its remaining items need, a second-half lane closes the
             obligation in the same cycle. Cycle 9 earned this one: the mouse
             lane stopped on budget with three checks recorded and an exact
             handover, and the follow-on lane took the idle capacity.

Set ROOT to the session's workflow directory before running.
"""

import json, os, glob, sys, time, re, subprocess, hashlib, collections

ROOT = os.environ.get('ZZ_WATCH_ROOT') or ''
STATE = os.environ.get('ZZ_WATCH_STATE') or '/tmp/zz-agentwatch-state.json'
if not ROOT:
    cands = sorted(glob.glob(os.path.expanduser('~/.claude/projects/*/*/subagents/workflows')),
                   key=os.path.getmtime)
    if not cands:
        sys.exit(0)
    ROOT = cands[-1]
DEV = os.environ.get('ZZ_WATCH_DEV') or os.path.expanduser('~/dev')
NOW = time.time()
TAIL = 60           # tool calls to look back over
REPEAT = 4          # identical full command this many times = spinning
FAILRUN = 4         # this many failing results in the window = thrash

def jload(p, d=None):
    try: return json.load(open(p))
    except Exception: return d if d is not None else {}

# Watch every run still moving, not just the newest: a second runner launched
# beside a cycle (run-9b beside run-9) would otherwise silence the first.
allruns = sorted(glob.glob(ROOT + '/wf_*'), key=os.path.getmtime)
if not allruns: sys.exit(0)
runs = [r for r in allruns if NOW - os.path.getmtime(r) < 6 * 3600] or allruns[-1:]

st = jload(STATE)
flags, pulse = [], []
for run in runs:
    finished = set()
    shortfall = []
    try:
        for line in open(run + '/journal.jsonl'):
            e = json.loads(line)
            if e.get('type') != 'result': continue
            finished.add(e.get('agentId'))
            r = e.get('result')
            if not isinstance(r, dict): continue
            for ob in r.get('obligations') or []:
                if ob.get('status_after') in ('active', 'different', 'blocked'):
                    shortfall.append((e.get('agentId'), ob.get('id'), ob.get('status_after'),
                                      len(ob.get('clauses_open') or [])))
    except Exception: pass

    meta = {}
    for m in glob.glob(run + '/*.meta.json'):
        i = os.path.basename(m).replace('agent-', '').replace('.meta.json', '')
        meta[i] = jload(m, {}).get('description', i)

    FAIL = re.compile(r'exit code[: ]*(1[0-9]{2}|[1-9][0-9]?)\b|exit status (1[0-9]{2}|[1-9][0-9]?)\b'
                      r'|error\[E\d+\]|panicked at|Killed|Out of memory|No such file|command not found'
                      r'|timed out after|assertion .*failed|FAILED|Traceback', re.I)
    OK = re.compile(r'\bexit(?: code)?[: =]*0\b|PASS\b|all \d+ .*identical|test result: ok', re.I)

    def scan(path):
        """Walk one agent transcript, pairing tool calls with their results."""
        calls, results = [], []
        try: lines = open(path, errors='replace').read().splitlines()
        except Exception: return calls, results, 0
        for line in lines:
            try: e = json.loads(line)
            except Exception: continue
            msg = e.get('message', e)
            content = msg.get('content') if isinstance(msg, dict) else None
            if not isinstance(content, list): continue
            for p in content:
                if not isinstance(p, dict): continue
                if p.get('type') == 'tool_use':
                    inp = p.get('input') or {}
                    cmd = inp.get('command') or inp.get('file_path') or json.dumps(inp, sort_keys=True)
                    calls.append(str(cmd))
                elif p.get('type') == 'tool_result':
                    c = p.get('content')
                    if isinstance(c, list):
                        c = ' '.join(x.get('text', '') for x in c if isinstance(x, dict))
                    results.append(str(c)[:4000])
        return calls, results, len(lines)

    prev = st.get(run, {})
    cur = {}

    # The harness restarts an agent that dies on a terminal error, leaving the dead
    # attempt's transcript behind under the same label. Only the newest transcript
    # per label is live; an older one is superseded, not silent.
    # A label whose live attempt already returned is done. Its dead earlier attempt
    # is still unfinished on paper, so without this the finished lane reappears as
    # the newest unfinished transcript and gets flagged SILENT forever.
    done_labels = {label for aid, label in meta.items() if aid in finished}

    live, superseded = {}, []
    for aid, label in meta.items():
        if aid in finished or label in done_labels: continue
        path = f'{run}/agent-{aid}.jsonl'
        if not os.path.exists(path): continue
        prior = live.get(label)
        if prior is None or os.path.getmtime(path) > os.path.getmtime(f'{run}/agent-{prior}.jsonl'):
            if prior is not None: superseded.append((label, prior))
            live[label] = aid
        else:
            superseded.append((label, aid))
    for label, aid in superseded:
        if not prev.get('seen_superseded', {}).get(aid):
            flags.append(f'{label}: an earlier attempt ({aid[:8]}) died and the harness restarted it; '
                         f'watching the live one only')
    cur['seen_superseded'] = {aid: True for _, aid in superseded}

    # A worker that finished with its obligation still short is a SECOND-HALF
    # opportunity, not a failure: its report names the remaining items and the
    # files that own them. Cycle 9 earned this. A runner's gates are serial, so
    # while they run, every slot but one is idle and a second-half lane costs
    # nothing but tokens. Announce each obligation once.
    seen_short = dict(prev.get('seen_short', {}))
    for aid, oid, status, nopen in shortfall:
        k = f'{aid}:{oid}'
        if seen_short.get(k): continue
        seen_short[k] = True
        lane = meta.get(aid, aid[:8])
        flags.append(f'SECOND HALF: {lane} finished with {oid} at {status}, {nopen} clause(s) open. '
                     f'{len(live)} agent(s) still live. Read its report: if the remaining items name '
                     f'their files, launch a second-half lane into the idle gate slots instead of '
                     f'carrying {oid} to the next cycle.')
    cur['seen_short'] = seen_short
    st[run] = cur

    for label, aid in sorted((l, a) for l, a in live.items()):
        path = f'{run}/agent-{aid}.jsonl'
        calls, results, nrec = scan(path)
        idle = (NOW - os.path.getmtime(path)) / 60.0
        d = nrec - prev.get(aid, {}).get('rec', nrec)
        cur[aid] = {'rec': nrec}

        if idle > 20:
            flags.append(f'{label}: SILENT {idle:.0f}m, no transcript write ({nrec} records)')

        # identical FULL command repeated -> real spinning
        win = calls[-TAIL:]
        h = collections.Counter(hashlib.sha1(re.sub(r'\s+', ' ', c).strip().encode()).hexdigest() for c in win)
        if h:
            top, k = h.most_common(1)[0]
            if k >= REPEAT:
                sample = next(c for c in reversed(win)
                              if hashlib.sha1(re.sub(r'\s+', ' ', c).strip().encode()).hexdigest() == top)
                flags.append(f'{label}: SPINNING, ran an identical command {k}x in its last {len(win)} calls -> '
                             + re.sub(r'\s+', ' ', sample)[:180])

        # STUCK: the SAME failure signature over and over. Different signatures in a row
        # are a fix loop making progress, which is normal and must not be flagged.
        rw = results[-TAIL:]
        sigs = []
        for r in rw:
            m = FAIL.search(r)
            if not m or OK.search(r[:400]): continue
            start = max(0, m.start() - 40)
            sigs.append(re.sub(r'\s+', ' ', r[start:m.end() + 90]).strip())
        if sigs:
            sc = collections.Counter(sigs)
            sig, k = sc.most_common(1)[0]
            if k >= FAILRUN:
                flags.append(f'{label}: STUCK, the identical failure {k}x in its last {len(rw)} calls -> {sig[:170]}')
        kills = sum(1 for r in rw if re.search(r'exit code[: ]*137|Killed|Out of memory', r, re.I))
        touts = sum(1 for r in rw if re.search(r'exit code[: ]*124|timed out after', r, re.I))
        if kills >= 2: flags.append(f'{label}: {kills} memory kills in recent results (cap too low or --jobs too high)')
        if touts >= 4: flags.append(f'{label}: {touts} command timeouts in recent results (work does not fit the 600s cap)')

        # burning calls with nothing landing
        key = label.split(':')[-1]
        cands = [c for c in glob.glob(DEV + '/zz-tui-*') + glob.glob(DEV + '/zz-gate-*')
                 if not os.path.basename(c).endswith('-review')
                 and re.fullmatch(r'zz-(?:tui|gate)-' + re.escape(key) + r'(?:-\d+)?', os.path.basename(c))]
        # a lane worktree carrying a suffix (zz-tui-keys-8) is this cycle's; bare is an older cycle's
        cands.sort(key=lambda c: (len(os.path.basename(c)), os.path.getmtime(c)), reverse=True)
        wd = cands[0] if cands else None
        age = None
        if wd:
            try:
                ts = subprocess.run(['git', '-C', wd, 'log', '-1', '--format=%ct'],
                                    capture_output=True, text=True, timeout=20).stdout.strip()
                if ts: age = (NOW - int(ts)) / 60.0
            except Exception: pass
        if age is not None and age > 180 and d > 40 and label.startswith(('worker', 'fix')):
            flags.append(f'{label}: {d} new calls since last check, no commit in {age:.0f}m ({os.path.basename(wd)})')

        pulse.append(f'{label} {nrec}rec(+{d}) idle{idle:.0f}m' + (f' commit{age:.0f}m' if age is not None else ''))

st['ticks'] = st.get('ticks', 0) + 1
try: json.dump(st, open(STATE, 'w'))
except Exception: pass

for f in flags: print('AGENT WATCH: ' + f)
if st['ticks'] % 4 == 0 and pulse: print('agent pulse: ' + ' | '.join(pulse))
