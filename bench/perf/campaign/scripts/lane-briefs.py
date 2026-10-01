import sys, json, os
ROOT=os.path.expanduser('~/dev')
MAC=sys.platform=='darwin'
HOST='macbook' if MAC else 'alienware'
INT=f'{ROOT}/zz-perf-int'
INT_BRANCH='perf/wave3'
DOC='knowledge/designs/daemon-perf-rebuild.md'
STAGE='wave3'
SCRATCH='/tmp/zzpc'
BASE_JSON=f'bench/perf/results/wave2-{HOST}-3d0fc1b0.json'
QUICK_W0=f'bench/perf/results/baseline-quick-{HOST}-17e17115.json'
PROFILER=('`sample <daemon pid> 5 -file /tmp/zzpc/<label>.txt` (or `xcrun xctrace record --template "Time Profiler" --attach <pid> --time-limit 5s --output /tmp/zzpc/<label>.trace`)' if MAC else '`perf record -g -p <daemon pid> -- sleep 5` then `perf report --stdio`')
OS_NOTE=('This host is macOS (M4 Max, the gate\'s reference host). Linux-only paths (/proc readers, the zz-pty-gather thread, epoll, THP, `tui-output-backpressure.sh`) cannot run here: name them in the report as not run instead of claiming them. The orchestrator runs the Linux checks on another host after your lane.' if MAC else 'This host is Linux (alienware). Run cargo with -j4 after `ulimit -n $(ulimit -Hn)`; macOS-only paths (posix_spawn, kqueue, the PTY spin bridge, ri_instructions, iOS) cannot run here: name them as not run. The orchestrator runs the Mac checks after your lane.')
RULES=f'''House rules (from the repo's CLAUDE.md files; they bite): no comments in Rust code; prose has no em-dashes and uses plain words; never git stash, reset --hard, or check out over work you did not write; the main checkout {ROOT}/zz belongs to other sessions: never edit or build there (reading is fine); commits carry no attribution or Co-Authored-By lines; do not push; do not bump PROTOCOL_VERSION (the campaign stays on unreleased 107 and releases are frozen until W4; non-append wire changes inside 107 are allowed); new daemon tests go in a per-lane #[cfg(test)] module file under crates/zz-daemon/src/daemon/ declared next to the lane's functions, never appended to the end of daemon.rs; find code by function name, not line number. A zz-daemon test that fails under the full workspace is re-run alone before diagnosis; `concurrent_default_interactive_attaches_atomically_share_session_zero` failing with "not a terminal" is environmental. Run long suites under a timeout (`timeout 1800 cargo test ...`) and treat any test running over 60 s as a bug to fix, not to wait on; after killing a hung run, kill its leftover test binaries (`pkill -f target/debug/deps/zz_daemon-`), they hold PTYs (macOS caps at 511). Before any build check `df -h {ROOT}`; under 20 GB free, stop and report. Prefer `cargo test -p <crate> <filter>` while iterating; another lane builds at the same time in its own worktree. Never point a build at another worktree's target directory. Test sockets go directly under /tmp (sun_path limit). compat/run.sh needs bash 4+ (use /opt/homebrew/bin/bash, not /bin/bash 3.2) and an absolute ZZ_COMPAT_ZZ. Mac-only red compat rows that tmux also fails: smoke/resurrect-save, smoke/plugin-runtime-continuum, smoke/status-background-jobs; compare any red row with the pre-lane binary before blaming the lane.
{OS_NOTE}
Always inspect the current state first (git log, git status, git diff in the worktree) and continue from it; never redo or discard finished work. Commit a checkpoint as soon as a coherent piece works.
Work alone: never call spawn_agent or any other sub-agent tool; one agent per worktree.
You have a wall-clock budget (below). Check `date` at each commit. When the budget is spent, finish the piece in hand, commit it, and write the final answer. When the done criterion (below) holds, stop: do not look for more work.
Rows outside the lane's gate groups, and rows that were already red on the base, are not yours: list them in the final answer and do not chase them.
Evidence: one scratch JSON per gate run you cite, nothing else (no receipt, provenance, audit, census or coverage files). Scratch stays in {SCRATCH}; never add gate JSONs to bench/perf/results, the orchestrator writes those.
Do not open {DOC} or bench/perf/campaign/HANDOFF.md: the lane section and a file:line map are pasted below and are all you need from them; find code from the map. Keep a state file {SCRATCH}/<slug>-<kind>-state.md under 60 lines (done, next, current numbers, budget end). After a context compaction read it and the map, nothing else.
This brief is one commit-sized step: end with one commit (at most one fixup on top). Iterate with `cargo check -p <crate>` and the focused tests; run clippy and the wider test set once, at the end. Build only into the CARGO_TARGET_DIR this brief names (default: the worktree's own target/).
A missing symbol or a moved line is NOT a reason to stop: find the equivalent and keep going; this takes precedence over any other rule. For every check you claim, give the exact command and its exit code; say SKIPPED or NOT RUN plainly for anything you did not run.
Your final answer must match the JSON schema you were given: done (true only when the done criterion holds), head, commits, gates (cmd, exit, skipped, result), deviations, open.'''

def section(lane_id):
    text=open(f'{INT}/{DOC}').read()
    start=text.index(f'\n## {lane_id}')
    end=text.find('\n## ',start+1)
    return text[start+1:end if end>0 else None].strip()

def context(l):
    parts=[f'Lane section from {DOC} at the lane base:\n\n{section(l["id"])}']
    if l.get('map'):
        parts.append('File:line map at the lane base (rg output; line numbers drift as you edit, function names do not):\n\n'+open(l['map']).read().strip())
    return '\n\n'.join(parts)

def quick(l):
    return f'`just perf-gate {STAGE} --quick --only {l["groups"]} --baseline {QUICK_W0} --json {SCRATCH}/{l["slug"]}-<label>.json` (quick against the quick W0; the full Mac wave-2 view to compare with is {BASE_JSON})'

def impl(l):
    wt=f'{ROOT}/zz-{l["slug"]}'
    return f'''# Lane {l["id"]} of the zz daemon performance rebuild: implement

{RULES}

Worktree: {wt} (branch perf/{l["slug"]}, from {INT_BRANCH} at the current main). Its target/ is already warm. Work only there; prefix every command with `cd {wt} &&`. Scratch files go in {SCRATCH} (create it).

{l.get("note","")}

{context(l)}

Budget: {l.get("budget",180)} minutes from your start. Task: {l["task"]}

Done criterion: {l["done"]}

Do:
1. Measure first: `just perf-gate baseline --quick --only {l["groups"]} --json {SCRATCH}/{l["slug"]}-before.json`.
2. Implement the whole lane scope, including its rollback knob(s) (ZZ_PERF_*) and the tests and fixtures the doc assigns to it, with tmux parity (compat/scenarios compare output byte for byte against pinned tmux).
3. Check: `cargo fmt --all`; `cargo clippy -p <touched crates and their dependents> --all-targets --all-features -- -D warnings`; `timeout 1800 cargo test -p <those crates>`; the compat scenarios at risk (`cargo build -p zz-cli && ZZ_COMPAT_ZZ={wt}/target/debug/zz_cli /opt/homebrew/bin/bash compat/run.sh <rows>`; compat/.cache already holds the pinned tmux); {quick(l)}. Instruction counts, bytes, counts, footprint and threads are the reliable signal; wall and CPU time are noisy while the other lane builds.
4. Fix your lane's section of the doc where it is wrong about the code, and add as-built notes (what was built, measured numbers against the previous merge, what is handed on).
5. Commit on perf/{l["slug"]} with a plain message. Leave the worktree in place.

{l.get("extra","")}'''

def review(l, kind, impl_report):
    wt=f'{ROOT}/zz-{l["slug"]}'
    if kind=='parity':
        body=f'''Adversarial PARITY and CORRECTNESS review of lane {l["id"]}. Diff: `cd {wt} && git log --oneline {INT_BRANCH}..HEAD && git diff {INT_BRANCH}...HEAD`. Brief: the lane section below. Try to break it: tmux parity (format output, hook ordering, alerts, command queue order, control-mode notification order, copy mode, automatic-rename, status rows), lost side effects, races and deadlocks (inner lock, status lock, actor channels, startup parking), stale state after attach/switch/resync/resize, every client kind (GUI crates/zz, TUI crates/zz-tui, CLI, control mode, web and iOS via crates/zz-client, clients/gpui-shared and zz-client-ffi, remote ssh), Linux vs macOS paths, rollback knob behaviour, hung or flaky tests. Run the lane's tests and the compat scenarios at risk. Throwaway probes go in {SCRATCH}/review-{l["slug"]} with their own CARGO_TARGET_DIR (clone {wt}/target with `/bin/cp -c -R` first).'''
    else:
        body=f'''Adversarial PERFORMANCE review of lane {l["id"]} ("no compromises"). Diff: `cd {wt} && git diff {INT_BRANCH}...HEAD`. Brief: the lane section below and bench/perf/thresholds.json. Verify the gains independently with {quick(l)}, interleaving runs of the lane binary and a {INT_BRANCH} binary (build one in a scratch worktree `git -C {INT} worktree add --detach {SCRATCH}/base-{l["slug"]} {INT_BRANCH}` with its own target cloned by `/bin/cp -c -R {INT}/target/release`; pass `--zz` to bench/perf/run.py). Profile the daemon on the lane's hot paths against an isolated daemon (env recipe in bench/perf/isolate.py; {PROFILER}) and list remaining avoidable work with sample shares. Check for regressions in rows the lane does not own and that throughput stays >= 4x tmux. Flag numbers that do not reproduce. Remove your scratch worktree at the end.'''
    return f'''# Lane {l["id"]}: {kind} review

{RULES}

Budget: {l.get("budget",60)} minutes from your start. Review commit {l.get("sha","HEAD")} (the lane may keep moving; review that commit). Done criterion: every issue you list has evidence (a command and its output, or a file:function reference); stop when the lane's risky paths are covered.

{body}

{context(l)}

Do not edit the lane's worktree. Implementer's report:

{impl_report}

Answer: done=true when the risky paths are covered; put each issue in open[] as "severity (blocker|major|minor) | where (file:function) | issue | evidence | fix"; commits stays empty.'''

def fix(l, impl_report, reviews):
    wt=f'{ROOT}/zz-{l["slug"]}'
    return f'''# Lane {l["id"]}: apply the review findings

{RULES}

Worktree {wt} (branch perf/{l["slug"]}). Budget: {l.get("budget",90)} minutes from your start. Done criterion: every blocker and major fixed or rebutted with evidence. Fix every blocker and major. Fix minors unless wrong or out of scope; say which and why. If a reviewer is wrong, show evidence instead of changing code. Re-run fmt, clippy -D warnings on touched crates, their tests (with a timeout), the compat scenarios at risk, and {quick(l)}. Update the lane's as-built notes in the doc. Commit on perf/{l["slug"]}.

Implementer's report:

{impl_report}

Reviews:

{reviews}

Answer: for each finding put "fixed <sha>" or "not fixed: <why>" in deviations[]; open[] holds what is left.'''

def focus(l):
    wt=f'{ROOT}/zz-{l["slug"]}'
    return f'''# Lane {l["id"]}: {l["title"]}

{RULES}

Worktree {wt} (branch perf/{l["slug"]}). Work only there; prefix every command with `cd {wt} &&`. Budget: {l.get("budget",90)} minutes from your start.

{context(l) if l.get("section",True) else ""}

Task: {l["task"]}

Done criterion: {l["done"]}

Checks before the final commit: `cargo fmt --all`; clippy -D warnings on the touched crates; their tests under `timeout 1800`; the compat scenarios at risk; {quick(l)}. Update the lane's as-built notes in {DOC} (keep them short: what changed, numbers against the base, what is handed on). Commit on perf/{l["slug"]}.'''

if __name__=='__main__':
    spec=json.loads(sys.argv[1]); kind=sys.argv[2]
    if kind in ('impl','slice'): print(impl(spec))
    elif kind in ('parity','perf'): print(review(spec, kind, open(sys.argv[3]).read()))
    elif kind=='focus': print(focus(spec))
    elif kind=='fix': print(fix(spec, open(sys.argv[3]).read(), '\n\n'.join(open(p).read() for p in sys.argv[4:])))
