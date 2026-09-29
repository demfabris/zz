export const meta = {
  name: 'daemon-perf-wave',
  description: 'One wave of the daemon perf rebuild: lanes implemented, reviewed and fixed in their own worktrees, then merged serially into the integration branch through the gate',
  phases: [
    { title: 'Implement', detail: 'one agent per lane in its own worktree, SLOTS lanes at a time' },
    { title: 'Review', detail: 'parity and perf reviewers per lane' },
    { title: 'Fix', detail: 'apply review findings on the lane branch' },
    { title: 'Merge', detail: 'serial rebase, full checks and a gate JSON per merge' },
  ],
}

const ROOT = '/home/demfabris/dev'
const MAIN = `${ROOT}/zz`
const INT = `${ROOT}/zz-perf-int`
const INT_BRANCH = 'perf/wave1'
const DOC = 'knowledge/designs/daemon-perf-rebuild.md'
const HOST = 'CHANGE_ME'
const WAVE = 1
const STAGE = 'wave1'
const BASE_JSON = `bench/perf/results/w1-5-fold-${HOST}-CHANGE_ME.json`
const QUICK_BASE_JSON = `bench/perf/results/baseline-quick-${HOST}-17e17115.json`
const SCRATCH = '/tmp/zz-perf-campaign'
const BASH_ENV = ''
const CLONE = 'cp -R --reflink=always'
const PROFILER = '`perf record -F 999 -g -p <daemon pid> -- sleep 5` then `perf report --stdio --no-children` (needs kernel.perf_event_paranoid <= 1 for kernel stacks)'
const OS_NOTE = 'This host is Linux. macOS-only paths (the posix_spawn pane launcher, clonefile tmux wrapper pin, kqueue, the macOS PTY spin bridge, ri_instructions, `just ios-gpui`) cannot be run here: say so in the report instead of claiming them.'
const SLOTS = 2
const LANES = [
  {
    id: 'W1-ATTACH', slug: 'attach', n: 6, groups: 'attach,echo,throughput,chatty', start: 'fix',
    reports: 'bench/perf/campaign/attach-review.json',
    note: 'RESUME: the lane is built and reviewed, and a fix pass was cut off mid-verification. Its commits 46510b07, 6e475f57, 08186223 and the WIP commit 73eafb94 are on perf/attach, which holds perf/wave1 only up to 9eb5888b. First run `git merge perf/wave1` in the worktree so the lane has the Linux instruction probe and the Linux W0 JSONs; without them the quick gates have no instr twins and no W0, and the vs-W0 rules and drift check do not run. findings_status in the reports file says which findings those commits look to address; verify each one, finish the rest, re-run every lane check, and replace the WIP commit message with a real one only by adding a follow-up commit (never rewrite pushed history).',
  },
]

const RULES = `
House rules (from the repo's CLAUDE.md files, restated because they bite): no comments in code; prose has no em-dashes and plain words; never git stash, reset --hard, or checkout over someone else's work; the main checkout ${MAIN} belongs to other sessions, never edit or build there (reading is fine); commits carry no attribution or Co-Authored-By lines; do not push; do not bump PROTOCOL_VERSION (the campaign stays on unreleased 107, releases are frozen until W4, non-append wire changes are allowed); new daemon tests go in a per-lane #[cfg(test)] module file under crates/zz-daemon/src/daemon/ declared next to the lane's functions, never appended to the end of daemon.rs; find code by function name; a zz-daemon test that fails under the full workspace is re-run alone before diagnosis, and concurrent_default_interactive_attaches_atomically_share_session_zero failing with "not a terminal" is environmental. Run long test suites with a timeout (for example \`timeout 1800 cargo test ...\`) and look for tests reported as running over 60 seconds: a hung test is a bug to fix, not to wait on; after killing a hung run, kill its leftover test binaries (\`pgrep -f target/debug/deps/zz_daemon-\`) because they hold PTYs. Before any build, check \`df -h ${ROOT}\`; if under 20 GB free, stop and report. Prefer \`cargo test -p <crate> <filter>\` while iterating; other lanes build at the same time. Never point a second build (yours or a probe's) at another lane's target directory: cargo reuses artifact hashes and overwrites that lane's binaries. compat/run.sh needs bash 4+ and an absolute ZZ_COMPAT_ZZ (a relative path breaks the in-pane tmux wrapper).
${OS_NOTE}
Always inspect the current state first (git log, git status, git diff in the worktree) and continue from it; never redo or discard finished work. Commit a checkpoint as soon as a coherent piece works, so a usage-limit cut never strands uncommitted edits.
The owner's standard is "no compromises": the targets in the doc are floors. When a target is met but a profile still shows avoidable work on the lane's path, remove it too.
`

const IMPL = {
  type: 'object',
  properties: {
    branch: { type: 'string' }, worktree: { type: 'string' }, head: { type: 'string' },
    summary: { type: 'string', description: 'what changed, by function, 10-30 lines' },
    rollback_knobs: { type: 'array', items: { type: 'string' } },
    tests_added: { type: 'array', items: { type: 'string' } },
    checks_run: { type: 'array', items: { type: 'string' }, description: 'command and result' },
    gate: { type: 'string', description: 'lane metrics before and after, from --quick runs' },
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

let slots = SLOTS
const waiters = []
async function acquire() { if (slots > 0) { slots--; return } await new Promise(r => waiters.push(r)) }
function release() { const w = waiters.shift(); if (w) w(); else slots++ }

const laneWT = l => `${ROOT}/zz-${l.slug}`
const newestBase = `the newest bench/perf/results/w${WAVE}-*-${HOST}-*.json in the tree, else ${BASE_JSON}`
const quickGate = l => `\`just perf-gate ${STAGE} --quick --only ${l.groups} --baseline <${newestBase}; for a quick-vs-quick comparison use ${QUICK_BASE_JSON}> --json ${SCRATCH}/${l.slug}-<label>.json\``
const reportText = v => typeof v === 'string' ? v : JSON.stringify(v, null, 1)

const implPrompt = (l, wt) => `You are implementing lane ${l.id} of the zz daemon performance rebuild.
${RULES}
${l.note || ''}
Setup if missing: \`git -C ${INT} worktree add -b perf/${l.slug} ${wt} ${INT_BRANCH}\`, then warm the build: \`mkdir -p ${wt}/target ${wt}/compat && ${CLONE} ${INT}/target/debug ${wt}/target/debug && ${CLONE} ${INT}/target/release ${wt}/target/release && ${CLONE} ${INT}/compat/.cache ${wt}/compat/.cache\`. If the clone fails because the filesystem has no reflinks, skip it and build fresh. If the worktree exists, reuse it; if its branch has no commits beyond ${INT_BRANCH} and is clean, first \`git -C ${wt} merge --ff-only ${INT_BRANCH}\`. Work only in ${wt}; prefix every command with \`cd ${wt} &&\`.
Read ${wt}/${DOC} fully (Campaign status, Outcome, Baseline, Targets, Architecture, Protocol and release policy, Waves and merge order, your lane section "${l.id}", Rollback switches, Tests and fixtures to add, Lane mechanics, Risks). The lane sections of lanes already merged carry as-built notes that may hand work to you: read them. Your lane section is the brief; every critique fix listed there is in scope. Also read bench/perf/README.md and bench/perf/campaign/HANDOFF.md.
Do:
1. Measure first: \`just perf-gate baseline --quick --only ${l.groups} --json ${SCRATCH}/${l.slug}-before.json\`.
2. Implement the whole lane scope, including its rollback knob(s) and the tests/fixtures the doc assigns to it, with tmux parity (compat/scenarios compare output byte for byte against pinned tmux).
3. Check: \`cargo fmt --all\`; \`cargo clippy -p <touched crates and dependents> --all-targets --all-features -- -D warnings\`; \`timeout 1800 cargo test -p <those crates>\`; the compat scenarios at risk (\`cargo build -p zz-cli && ${BASH_ENV}ZZ_COMPAT_ZZ=${wt}/target/debug/zz_cli compat/run.sh ...\`, run \`compat/fetch-tmux.sh\` first if compat/.cache is missing); ${quickGate(l)}. Instruction counts (when the probe has them), bytes, counts, footprint and threads are the reliable signal; wall and CPU time are noisy while other lanes build.
4. Profile again (${PROFILER}); remove remaining avoidable work on your path.
5. Fix your lane's section of the doc where it is wrong about the code, and add as-built notes (what was built, measured numbers against the previous merge, what is handed on).
6. Commit on perf/${l.slug} in coherent commits with plain messages. Leave the worktree in place.
Return the structured report.`

const reviewPrompts = (l, wt, impl) => [
  () => agent(`Adversarial PARITY and CORRECTNESS review of lane ${l.id} of the zz daemon performance rebuild.
${RULES}
Worktree ${wt}, branch perf/${l.slug}; diff against ${INT_BRANCH} (\`cd ${wt} && git log --oneline ${INT_BRANCH}..HEAD && git diff ${INT_BRANCH}...HEAD\`). Brief: the "${l.id}" section of ${wt}/${DOC} plus Risks, Rollback switches and Tests. Implementer's report:
${reportText(impl)}
Try to break it: tmux parity (format output, hook ordering, alerts, command queue order, control-mode notification order, copy mode, automatic-rename, status rows), lost side effects, races and deadlocks (inner lock, status lock, actor channels, startup parking), stale state after attach/switch/resync/resize, every client kind (GUI crates/zz, TUI crates/zz-tui, CLI, control mode, web/iOS via crates/zz-client and zz-client-ffi, remote ssh), Linux vs macOS paths, rollback knob behavior, hung or flaky tests. Run the lane's tests and the compat scenarios at risk; throwaway probes go in ${SCRATCH} with their own CARGO_TARGET_DIR. Do not edit the lane. Report issues with evidence.`, { label: `review-parity:${l.slug}`, phase: 'Review', schema: REVIEW, effort: 'high', agentType: 'general-purpose' }),
  () => agent(`Adversarial PERFORMANCE review of lane ${l.id} of the zz daemon performance rebuild ("no compromises").
${RULES}
Worktree ${wt}, branch perf/${l.slug} (\`cd ${wt} && git diff ${INT_BRANCH}...HEAD\`). Brief: the "${l.id}" section of ${wt}/${DOC}, Targets, bench/perf/thresholds.json. Implementer's report:
${reportText(impl)}
Verify gains independently with ${quickGate(l)}, and interleave runs of the lane and of a build of ${INT_BRANCH} in a scratch worktree with its own target. Profile the daemon on the lane's hot paths against an isolated daemon (env recipe in bench/perf/isolate.py; ${PROFILER}) and list remaining avoidable work with sample shares. Check for regressions in metrics the lane does not own and that throughput stays >= 4x tmux. Flag numbers that do not reproduce. Do not edit the lane.`, { label: `review-perf:${l.slug}`, phase: 'Review', schema: REVIEW, effort: 'high', agentType: 'general-purpose' }),
]

const fixPrompt = (l, wt, impl, reviews) => `Apply review findings to lane ${l.id} in worktree ${wt} (branch perf/${l.slug}).
${RULES}
${l.note || ''}
Fix every blocker and major. Fix minors unless wrong or out of scope; say which and why. If a reviewer is wrong, show evidence instead of changing code. Re-run fmt, clippy -D warnings on touched crates, their tests (with a timeout), the compat scenarios at risk, and ${quickGate(l)}. Update the lane's as-built notes in the doc. Commit on perf/${l.slug}.
Implementer's report:
${reportText(impl)}
Reviews:
${reportText(reviews)}
Return the same structured report shape as the implementer, updated.`

const mergePrompt = (l, report) => `Merge lane ${l.id} (branch perf/${l.slug}, worktree ${laneWT(l)}) into the integration branch ${INT_BRANCH} (worktree ${INT}). This is merge ${l.n} of wave ${WAVE}; the order is ${LANES.map(x => x.id).join(', ')} after the lanes already merged.
${RULES}
Lane report:
${reportText(report)}
Steps:
1. \`cd ${laneWT(l)} && git merge ${INT_BRANCH}\` (merge, not rebase: lane branches may already be shared); resolve conflicts by understanding both sides and never drop another lane's behaviour; rebuild and re-run the lane's own tests after a non-trivial merge.
2. \`cd ${INT} && git merge --no-ff perf/${l.slug} -m "Merge ${l.id}: <one line>"\`.
3. In ${INT}: \`cargo fmt --all -- --check\`; \`cargo clippy --workspace --all-targets --all-features -- -D warnings\`; \`timeout 3600 cargo test --workspace --all-features --no-fail-fast\` (re-run zz-daemon failures alone; watch for hung tests); \`just compat-check\`; \`cargo build -p zz-cli\` then the full corpus \`${BASH_ENV}ZZ_COMPAT_ZZ=${INT}/target/debug/zz_cli compat/run.sh\` compared against the accepted summary, with any unclean row checked against the pre-merge binary before blaming this lane; \`compat/attached-client.sh\`; then the full gate \`just perf-gate ${STAGE} --baseline <${newestBase}> --json bench/perf/results/w${WAVE}-${l.n}-${l.slug}-${HOST}-<sha8 of the merge commit>.json\`, with \`--strict\`: first wait until nothing else builds and the 1-minute load per CPU is under 0.5 (\`uptime\`, \`nproc\`), up to 30 minutes; if the host never gets there, run without \`--strict\` and say so in the report so the owner can rerun it. Judge the lane on metrics it owns and on the gate's regressed/drifted checks; stage targets owned by unmerged lanes are expected to fail.
4. Anything red this merge caused: fix it in ${INT} with a follow-up commit, or if unsalvageable, \`git -C ${INT} reset --hard <pre-merge sha>\` (the integration branch belongs to this workflow alone while it runs) and return status failed.
5. Commit the results JSON in ${INT}. Then remove the lane worktree and branch: \`git -C ${INT} worktree remove --force ${laneWT(l)} && git -C ${INT} branch -D perf/${l.slug}\`.
Return the structured merge report.`

const merged = {}
for (const l of LANES) { let resolve; const done = new Promise(r => { resolve = r }); merged[l.slug] = { done, resolve } }

async function runLane(l) {
  if (l.after) await merged[l.after].done
  await acquire()
  try {
    const wt = laneWT(l)
    const start = l.start || 'impl'
    let impl = l.reports ? `Read ${INT}/${l.reports} (implementer report, reviews and findings_status).` : null
    let reviews = impl
    if (start === 'impl') impl = await agent(implPrompt(l, wt), { label: `impl:${l.slug}`, phase: 'Implement', schema: IMPL, effort: 'xhigh', agentType: 'general-purpose' })
    if (!impl) return null
    if (start === 'merge') return impl
    if (start === 'impl' || start === 'review') reviews = (await parallel(reviewPrompts(l, wt, impl))).filter(Boolean)
    return await agent(fixPrompt(l, wt, impl, reviews), { label: `fix:${l.slug}`, phase: 'Fix', schema: IMPL, effort: 'high', agentType: 'general-purpose' })
  } finally { release() }
}

const lanePromises = LANES.map(l => runLane(l).catch(e => { log(`${l.id}: ${e}`); return null }))
phase('Merge')
const merges = []
for (let i = 0; i < LANES.length; i++) {
  const l = LANES[i]
  const report = await lanePromises[i]
  if (!report) {
    merges.push({ lane: l.id, status: 'failed', notes: 'lane produced no result' })
    log(`${l.id}: no result, not merged`)
    merged[l.slug].resolve()
    continue
  }
  const m = await agent(mergePrompt(l, report), { label: `merge:${l.slug}`, phase: 'Merge', schema: MERGE, effort: 'high', agentType: 'general-purpose' })
  merges.push({ lane: l.id, ...m })
  log(`${l.id}: ${m ? m.status : 'no merge result'}`)
  merged[l.slug].resolve()
}
return { merges }
