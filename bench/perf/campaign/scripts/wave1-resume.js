export const meta = {
  name: 'daemon-perf-wave1-resume',
  description: 'Resume wave 1 of the daemon perf rebuild after the usage-limit cut: finish the footprint merge, merge formats, finish publish, panes, exec and attach, merge them serially',
  phases: [
    { title: 'Implement', detail: 'resume panes, start exec and attach' },
    { title: 'Review', detail: 'parity and perf reviewers per lane' },
    { title: 'Fix', detail: 'apply review findings (publish resumes here)' },
    { title: 'Merge', detail: 'serial merges into perf/wave1 through the gate' },
  ],
}

const ROOT = '/Users/demfabris/dev'
const INT = `${ROOT}/zz-perf-int`
const MAIN = `${ROOT}/zz`
const DOC = 'knowledge/designs/daemon-perf-rebuild.md'
const PRIOR = '/private/tmp/claude-501/-Users-demfabris-dev-zz/9e7e04c9-8d02-43fa-ad4b-24a1abc02bee/scratchpad/wave1'
const HOST = 'macbook'
const BASH_PATH = '/opt/homebrew/bin'
const CLONE = '/bin/cp -c -R'
const CLONE_NOTE = '(/bin/cp; GNU cp lacks -c)'

const RULES = `
House rules (from the repo's CLAUDE.md files, restated because they bite): no comments in code; prose has no em-dashes and plain words; never git stash, reset --hard, or checkout over someone else's work; the main checkout ${MAIN} belongs to other sessions, never edit or build there (reading is fine); the worktrees ${ROOT}/zz-footprint and ${ROOT}/zz-peers are in use by OTHER sessions right now: never edit, build in, remove or clean them; commits carry no attribution or Co-Authored-By lines; do not bump PROTOCOL_VERSION (the campaign stays on unreleased 107, releases are frozen until W4, non-append wire changes are allowed); new daemon tests go in a per-lane #[cfg(test)] module file under crates/zz-daemon/src/daemon/ declared next to the lane's functions, never appended to the end of daemon.rs; find code by function name; a zz-daemon test that fails under the full workspace is re-run alone before diagnosis, and concurrent_default_interactive_attaches_atomically_share_session_zero failing with "not a terminal" is environmental. Run long test suites with a timeout (for example \`timeout 1800 cargo test ...\`) and look for tests reported as running over 60 seconds: a hung test is a bug to fix, not to wait on. Before any build, check \`df -h /\`; if under 20 GB free, stop and report. Prefer \`cargo test -p <crate> <filter>\` while iterating; other lanes build at the same time.
This run resumes work that an earlier run started and a usage limit cut off. Always inspect the current state first (git log, git status, git diff in the worktree) and continue from it; never redo or discard finished work.
The owner's standard is "no compromises": the targets in the doc are floors. When a target is met but a profile still shows avoidable work on the lane's path, remove it too.
`

const IMPL = {
  type: 'object',
  properties: {
    branch: { type: 'string' }, worktree: { type: 'string' }, head: { type: 'string' },
    summary: { type: 'string' },
    rollback_knobs: { type: 'array', items: { type: 'string' } },
    tests_added: { type: 'array', items: { type: 'string' } },
    checks_run: { type: 'array', items: { type: 'string' } },
    gate: { type: 'string' },
    deviations: { type: 'array', items: { type: 'string' } },
    known_issues: { type: 'array', items: { type: 'string' } },
  },
  required: ['branch', 'worktree', 'head', 'summary', 'rollback_knobs', 'tests_added', 'checks_run', 'gate', 'deviations', 'known_issues'],
}
const REVIEW = {
  type: 'object',
  properties: {
    verdict: { type: 'string' },
    issues: { type: 'array', items: { type: 'object', properties: {
      severity: { type: 'string', enum: ['blocker', 'major', 'minor'] }, where: { type: 'string' }, issue: { type: 'string' }, evidence: { type: 'string' }, fix: { type: 'string' },
    }, required: ['severity', 'where', 'issue', 'evidence', 'fix'] } },
  },
  required: ['verdict', 'issues'],
}
const MERGE = {
  type: 'object',
  properties: {
    status: { type: 'string', enum: ['merged', 'failed'] },
    int_head: { type: 'string' }, results_json: { type: 'string' },
    gate: { type: 'string' }, tests: { type: 'string' },
    fixes_applied: { type: 'array', items: { type: 'string' } }, notes: { type: 'string' },
  },
  required: ['status', 'int_head', 'results_json', 'gate', 'tests', 'fixes_applied', 'notes'],
}

let slots = 2
const waiters = []
async function acquire() { if (slots > 0) { slots--; return } await new Promise(r => waiters.push(r)) }
function release() { const w = waiters.shift(); if (w) w(); else slots++ }

const implPrompt = (l, wt, extra) => `You are implementing lane ${l.id} of the zz daemon performance rebuild.
${RULES}
Setup if missing: \`git -C ${INT} worktree add -b perf/${l.slug} ${wt} perf/wave1\`, then \`mkdir -p ${wt}/target ${wt}/compat && ${CLONE} ${MAIN}/target/debug ${wt}/target/debug && ${CLONE} ${MAIN}/target/release ${wt}/target/release && ${CLONE} ${MAIN}/compat/.cache ${wt}/compat/.cache\` ${CLONE_NOTE}. If the worktree exists, reuse it; if its branch has no commits beyond perf/wave1 and is clean, first \`git -C ${wt} merge --ff-only perf/wave1\` so you start from the newest integration head. Work only in ${wt}; prefix every command with \`cd ${wt} &&\`.
${extra}
Read ${wt}/${DOC} fully (Outcome, Baseline, Targets, Architecture, Protocol and release policy, Waves and merge order, your lane section "${l.id}", Rollback switches, Tests and fixtures to add, Lane mechanics, Risks). The lane sections of lanes already merged into perf/wave1 carry as-built notes that may hand work to you: read them. Your lane section is the brief; every critique fix listed there is in scope.
Do:
1. Measure first: \`just perf-gate baseline --quick --only ${l.groups} --json /tmp/zz-${l.slug}-before.json\`.
2. Implement the whole lane scope, including its rollback knob(s) and the tests/fixtures the doc assigns to it, with tmux parity (compat/scenarios compare output byte for byte against pinned tmux).
3. Check: \`cargo fmt --all\`; \`cargo clippy -p <touched crates and dependents> --all-targets --all-features -- -D warnings\`; \`cargo test -p <those crates>\` with a timeout; the compat scenarios at risk (\`PATH=${BASH_PATH}:$PATH ZZ_COMPAT_ZZ=$PWD/target/debug/zz_cli compat/run.sh ...\`, bash 4+ and an absolute path are required); \`just perf-gate wave1 --quick --only ${l.groups} --baseline <newest bench/perf/results/w1-*.json in your tree, else bench/perf/results/baseline-quick-${HOST}-17e17115.json>\`. Instruction counts, bytes, counts, footprint and threads are the reliable signal; wall time is noisy while other lanes build.
4. Profile again; remove remaining avoidable work on your path.
5. Fix your lane's section of the doc where it is wrong about the code, and add as-built notes (what was built, measured numbers, what is handed on).
6. Commit on perf/${l.slug} in coherent commits with plain messages. Leave the worktree in place.
Return the structured report.`

const reviewPrompts = (l, wt, impl) => [
  () => agent(`Adversarial PARITY and CORRECTNESS review of lane ${l.id} of the zz daemon performance rebuild.
${RULES}
Worktree ${wt}, branch perf/${l.slug}; diff against perf/wave1 (\`cd ${wt} && git log --oneline perf/wave1..HEAD && git diff perf/wave1...HEAD\`). Brief: the "${l.id}" section of ${wt}/${DOC} plus Risks, Rollback switches and Tests. Implementer's report:
${JSON.stringify(impl, null, 1)}
Try to break it: tmux parity (format output, hook ordering, alerts, command queue order, control-mode notification order, copy mode, automatic-rename, status rows), lost side effects, races and deadlocks (inner lock, status lock, actor channels, startup parking), stale state after attach/switch/resync/resize, every client kind (GUI crates/zz, TUI crates/zz-tui, CLI, control mode, web/iOS via crates/zz-client and zz-client-ffi, remote ssh), Linux vs macOS paths, rollback knob behavior, hung or flaky tests. Run the lane's tests and the compat scenarios at risk; throwaway probes go in /tmp. Do not edit the lane. Report issues with evidence.`, { label: `review-parity:${l.slug}`, phase: 'Review', schema: REVIEW, effort: 'high', agentType: 'general-purpose' }),
  () => agent(`Adversarial PERFORMANCE review of lane ${l.id} of the zz daemon performance rebuild ("no compromises").
${RULES}
Worktree ${wt}, branch perf/${l.slug} (\`cd ${wt} && git diff perf/wave1...HEAD\`). Brief: the "${l.id}" section of ${wt}/${DOC}, Targets, bench/perf/thresholds.json. Implementer's report:
${JSON.stringify(impl, null, 1)}
Verify gains independently with \`just perf-gate wave1 --quick --only ${l.groups} --baseline <newest bench/perf/results/w1-*.json or baseline-quick-${HOST}-17e17115.json>\` (instruction counts, bytes, counts, footprint, threads). Sample the daemon on the lane's hot paths against an isolated daemon (env recipe in bench/perf/isolate.py) and list remaining avoidable work with sample shares. Check for regressions in metrics the lane does not own and that throughput stays >= 4x tmux. Flag numbers that do not reproduce. Do not edit the lane.`, { label: `review-perf:${l.slug}`, phase: 'Review', schema: REVIEW, effort: 'high', agentType: 'general-purpose' }),
]

const fixPrompt = (l, wt, impl, reviews, extra) => `Apply review findings to lane ${l.id} in worktree ${wt} (branch perf/${l.slug}).
${RULES}
${extra || ''}
Fix every blocker and major. Fix minors unless wrong or out of scope; say which and why. If a reviewer is wrong, show evidence instead of changing code. Re-run fmt, clippy -D warnings on touched crates, their tests (with a timeout), the compat scenarios at risk, and \`just perf-gate wave1 --quick --only ${l.groups}\` with the right --baseline. Update the lane's as-built notes in the doc. Commit on perf/${l.slug}.
Implementer's report:
${typeof impl === 'string' ? impl : JSON.stringify(impl, null, 1)}
Reviews:
${typeof reviews === 'string' ? reviews : JSON.stringify(reviews, null, 1)}
Return the same structured report shape as the implementer, updated.`

async function fullLane(l, extra) {
  await acquire()
  try {
    const wt = `${ROOT}/zz-${l.slug}`
    const impl = await agent(implPrompt(l, wt, extra), { label: `impl:${l.slug}`, phase: 'Implement', schema: IMPL, effort: 'xhigh', agentType: 'general-purpose' })
    const reviews = (await parallel(reviewPrompts(l, wt, impl))).filter(Boolean)
    const fix = await agent(fixPrompt(l, wt, impl, reviews), { label: `fix:${l.slug}`, phase: 'Fix', schema: IMPL, effort: 'high', agentType: 'general-purpose' })
    return fix
  } finally { release() }
}

const PUBLISH = { id: 'W1-PUBLISH', slug: 'publish', groups: 'cli,chatty,config,idle,attach,statusjob' }
const PANE = { id: 'W1-PANE', slug: 'pane', groups: 'spawn,chatty,mem,echo,throughput,attach' }
const EXEC = { id: 'W1-EXEC', slug: 'exec', groups: 'cli,cold,spawn,control,config' }
const ATTACH = { id: 'W1-ATTACH', slug: 'attach', groups: 'attach,echo,throughput,chatty' }

const publishDone = (async () => {
  await acquire()
  try {
    return await agent(fixPrompt(PUBLISH, `${ROOT}/zz-publish`, `Read ${PRIOR}/impl-publish.json`, `Read ${PRIOR}/review-parity-publish.json and ${PRIOR}/review-perf-publish.json`,
      `RESUME: an earlier fix run already committed fefdd5c6 "Apply W1-PUBLISH review fixes" and left uncommitted edits in crates/zz-daemon/src/daemon.rs, crates/zz-daemon/src/daemon/publish_tests.rs, crates/zz-daemon/src/daemon/timers.rs and an untracked crates/zz-terminal/src/lib.rs change before it was cut off. Read the implementer report and both reviews from the files named below, check which findings fefdd5c6 and the uncommitted edits already address, finish the rest, and verify everything.`),
      { label: 'fix:publish', phase: 'Fix', schema: IMPL, effort: 'high', agentType: 'general-purpose' })
  } finally { release() }
})()

const paneDone = fullLane(PANE, `RESUME: an earlier implementer started this lane in ${ROOT}/zz-pane (branch perf/pane, based on 157ac6a3, not on the newest perf/wave1) and was cut off with uncommitted edits in Cargo.lock, crates/zz-daemon/src/daemon.rs, crates/zz-terminal/Cargo.toml, crates/zz-terminal/src/session.rs, crates/zz-terminal/src/shell_integration.rs, crates/zz-terminal/src/terminal_core.rs and new crates/zz-daemon/src/daemon/pane_tests.rs and crates/zz-terminal/src/session/pane_tests.rs. Read that diff first and continue it; commit a checkpoint early. Known problem: with these edits, \`cargo test -p zz-daemon --lib\` hung for over 30 minutes at 12 cores in daemon::tests::mode_keys_retarget_active_command_output_and_restore_the_previous_table (spinning in zz_terminal session::mode_revision ModeRevision::first_char / clamp_point and Shared::pump_control_output_at); it had to be killed. Find and fix the cause. Do not rebase onto perf/wave1 yourself; the merge step does that.`)

const execDone = paneDone.then(() => null).catch(() => null) && fullLane(EXEC, `The worktree ${ROOT}/zz-exec (branch perf/exec) already exists at 613630ce with no work in it; reuse it and fast-forward it to the newest perf/wave1 first.`)
const attachDone = fullLane(ATTACH, '')

const mergePrompt = (l, n, report, extra) => `Merge lane ${l.id} (branch perf/${l.slug}, worktree ${ROOT}/zz-${l.slug}) into the integration branch perf/wave1 (worktree ${INT}). Merge ${n} of 6, fixed order FOOTPRINT, FORMAT, PUBLISH, PANE, EXEC, ATTACH.
${RULES}
${extra || ''}
Lane report:
${typeof report === 'string' ? report : JSON.stringify(report, null, 1)}
Steps:
1. \`cd ${ROOT}/zz-${l.slug} && git rebase perf/wave1\`; resolve conflicts by understanding both sides and never drop another lane's behaviour; rebuild and re-run the lane's own tests after a non-trivial rebase.
2. \`cd ${INT} && git merge --no-ff perf/${l.slug} -m "Merge ${l.id}: <one line>"\`.
3. In ${INT}: \`cargo fmt --all -- --check\`; \`cargo clippy --workspace --all-targets --all-features -- -D warnings\`; \`timeout 3600 cargo test --workspace --all-features\` (re-run zz-daemon failures alone; watch for hung tests); \`just compat-check\`; the full \`compat/run.sh\` corpus (PATH=${BASH_PATH} first, absolute ZZ_COMPAT_ZZ) compared against the accepted summary, with any unclean row checked against the pre-merge binary before blaming this lane; \`compat/attached-client.sh\`; then the full gate \`just perf-gate wave1 --baseline <newest bench/perf/results/w1-*.json, else bench/perf/results/baseline-${HOST}-17e17115.json> --json bench/perf/results/w1-${n}-${l.slug}-${HOST}-<sha8 of the merge commit>.json\` without --strict (other lanes compile; wall is a note). Judge the lane on metrics it owns and on the gate's regressed/drifted checks; wave targets owned by unmerged lanes are expected to fail.
4. Anything red this merge caused: fix it in ${INT} with a follow-up commit, or if unsalvageable, \`git -C ${INT} reset --hard <pre-merge sha>\` (this integration branch is yours alone) and return status failed.
5. Commit the results JSON in ${INT}. Then remove the lane worktree and branch unless the rules say another session owns it: \`git -C ${INT} worktree remove --force ${ROOT}/zz-${l.slug} && git -C ${INT} branch -D perf/${l.slug}\`.
Return the structured merge report.`

phase('Merge')
const merges = []
merges.push(await agent(mergePrompt({ id: 'W1-FOOTPRINT', slug: 'footprint' }, 1, `Read ${PRIOR}/fix-footprint.json`,
  `RESUME: the earlier merge step already merged perf/footprint into perf/wave1 as 613630ce and was cut off during the step 3 checks; no results JSON was committed. Skip steps 1-2, run step 3 on the current ${INT} head, fix what this merge broke, commit bench/perf/results/w1-1-footprint-${HOST}-<sha8>.json. Do NOT remove ${ROOT}/zz-footprint or delete branch perf/footprint: another session is working in that worktree.`),
  { label: 'merge:footprint', phase: 'Merge', schema: MERGE, effort: 'high', agentType: 'general-purpose' }))
log(`footprint: ${merges[0] && merges[0].status}`)
merges.push(await agent(mergePrompt({ id: 'W1-FORMAT', slug: 'format' }, 2, `Read ${PRIOR}/fix-format.json`, ''),
  { label: 'merge:format', phase: 'Merge', schema: MERGE, effort: 'high', agentType: 'general-purpose' }))
log(`format: ${merges[1] && merges[1].status}`)
for (const [l, n, p] of [[PUBLISH, 3, publishDone], [PANE, 4, paneDone], [EXEC, 5, execDone], [ATTACH, 6, attachDone]]) {
  const report = await p
  if (!report) { merges.push({ lane: l.id, status: 'failed', notes: 'lane produced no result' }); log(`${l.id}: no result`); continue }
  const m = await agent(mergePrompt(l, n, report, ''), { label: `merge:${l.slug}`, phase: 'Merge', schema: MERGE, effort: 'high', agentType: 'general-purpose' })
  merges.push({ lane: l.id, ...m })
  log(`${l.id}: ${m ? m.status : 'no merge result'}`)
}
return { merges }
