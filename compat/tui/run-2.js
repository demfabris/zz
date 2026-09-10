export const meta = {
  name: 'tui-run-2',
  description: 'TUI parity cycle 2: one Opus 5 lane at xhigh closing TUI-001 clause 2 with an attested macOS run of the geometry fixture, one adversarial reviewer with one bounded fix pass, one serialized gate that records TUI-001 and TUI-002 verified',
  phases: [
    { title: 'Work', detail: 'one worktree: the macOS verify lane' },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh, adversarial, pipelined behind the worker; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: 'serialized Opus 5 MAIN gate: the four fixtures, both trackers, the verification records for TUI-001 and TUI-002, the records commit, board ledger' },
  ],
}

const A = args || {}
const M = {
  root: A.root || '/Users/demfabris/dev/zz',
  dev: A.dev || '/Users/demfabris/dev',
  holder: A.holder || 'macbook/orchestrator',
  machine: A.machine || '16-core, 48 GB Apple Silicon macbook (macOS 27)',
  cores: A.cores || 16,
  workerJobs: A.workerJobs || 8,
  workerThreads: A.workerThreads || 4,
  gateJobs: A.gateJobs || 12,
  gateThreads: A.gateThreads || 8,
  shards: A.shards || 4,
  date: A.date || '2026-09-10',
  gateZz: A.gateZz || '/Users/demfabris/dev/zz-gate-target/debug/zz',
  protected: A.protected || "the user's installed zz daemon (/Applications/zz.app) on the default socket under $TMPDIR/zz-demfabris/default.sock, every dist/zz-dev daemon the user launched on /tmp/zz-*.sock, and any tmux server on /tmp/tmux-501/default",
  boxNote: A.boxNote || "This box: /bin/bash is 3.2 and has no mapfile, so prefix EVERY compat/ invocation (run.sh and each fixture) with PATH=/opt/homebrew/bin:$PATH, which puts bash 5.3 first; APFS refuses \\377 file names (smoke/client-non-utf8-cwd is environmental here). Cycle 17 measured eleven corpus rows red on this box at origin/main for environment reasons, so a red row at your tip is yours only if it is green at origin/main, which you prove by running that row against a main build before claiming it. The box has a real ~/.tmux.conf and a real ~/.config/zz/mux.conf (prefix C-a, unbind C-b) and zz reads both in place: never read or edit them, and never start a tmux or zz server that would load them (the harness and the fixtures scrub HOME and XDG_CONFIG_HOME; your own probes use -f /dev/null or a scratch HOME plus a scratch XDG_CONFIG_HOME, both, because config discovery is first-existing-file-wins). Never write 'rm -rf $HOME' or 'rm -rf ~' even after re-exporting HOME to a scratch path; put the scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz; unix socket paths have a low length cap, keep them directly under /tmp): every server you start is -L zzprobe-$$ -f /dev/null, every client command names that socket. A binary you spawn from a build directory is whatever was built there last: rebuild in the worktree under test before spawning it; and after switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first (mtime freshness otherwise builds the wrong tree). Every git commit on this box is 'git -c commit.gpgsign=false commit' (signing goes through a Touch ID enclave and hangs an unattended session). Sweep 'pgrep -fl zzprobe' at the end and reap only pids whose command line names a socket you created. PROVENANCE, learned in cycle 1: a cargo debug build of zz is NOT bit-reproducible, so a recorded sha256 identifies an artifact but cannot attest it; attestation is the recorded revision plus a clean worktree plus reproduction, and every proof names the binary it actually ran.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz.git) through the gh credential helper and works non-interactively on this box; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands so a credential miss fails instead of hanging.',
}
log(`TUI cycle 2 on ${M.machine}; root ${M.root}; holder ${M.holder}; worker --jobs ${M.workerJobs}/--test-threads=${M.workerThreads}; gate --jobs ${M.gateJobs}/--test-threads=${M.gateThreads}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`
const OWNERSHIP = `LEDGER OWNERSHIP THIS CYCLE (compat/tui/campaign.json records; edit only yours): the verify lane owns TUI-001. TUI-002's record belongs to the GATE alone this cycle (it is finished as work at review with banked proof; only the gate may flip it). Every other obligation, the baseline list, the milestones and the tmux pin are nobody's. In compat/tmux-gaps.json nobody closes, relocates or reopens anything this cycle; a measurement that contradicts a recorded decision is appended to that gap's reason as a dated TUI measurement and named in notes.`

const WORKER_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['branch', 'obligations', 'touched_commands', 'touched_packages', 'notes'],
  properties: {
    branch: { type: 'string', description: 'campaign/* branch pushed to origin, or empty string if nothing pushed' },
    obligations: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['id', 'status_after', 'clauses_proved', 'clauses_open', 'proofs', 'evidence_dir'], properties: {
      id: { type: 'string', description: 'TUI-NNN' },
      status_after: { type: 'string', enum: ['unmeasured', 'different', 'active', 'review', 'blocked'] },
      clauses_proved: { type: 'array', items: { type: 'string' }, description: 'acceptance clauses (quoted or numbered) with passing evidence at the final tip' },
      clauses_open: { type: 'array', items: { type: 'string' }, description: 'acceptance clauses not yet proved, each with why' },
      proofs: { type: 'array', items: { type: 'string' }, description: 'exact commands that ran AT THE FINAL TIP with their exit codes' },
      evidence_dir: { type: 'string', description: 'compat/tui/evidence/<ID>/<attempt> committed on the branch' },
    } } },
    touched_commands: { type: 'array', items: { type: 'string' }, description: 'tmux verb names the diff touches, for the delta corpus (usually empty for this lane)' },
    touched_packages: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string', description: 'everything reviewer and integrator must know: whether the timeout reproduced, the diagnosis or the bounded explanation, zone excursions, whether crates/ changed and why, flaky tests seen, decisions recorded, the final tip sha' },
  },
}

const REVIEW_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['lane', 'verdict', 'confirmed_defects', 'checks_run', 'notes'],
  properties: {
    lane: { type: 'string' },
    verdict: { type: 'string', enum: ['approve', 'approve-with-fixes', 'reject'] },
    confirmed_defects: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['obligation', 'severity', 'description', 'suggested_fix'], properties: {
      obligation: { type: 'string' },
      severity: { type: 'string', enum: ['blocker', 'must-fix', 'nit'] },
      description: { type: 'string' },
      suggested_fix: { type: 'string' },
    } } },
    checks_run: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
}

const GATE_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['merges', 'progress_after', 'board_updates', 'attached_client', 'problems'],
  properties: {
    merges: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['branch', 'merged', 'merge_commit', 'gate_summary', 'review_actions', 'flakes'], properties: {
      branch: { type: 'string' }, merged: { type: 'boolean' }, merge_commit: { type: 'string' },
      gate_summary: { type: 'string' }, review_actions: { type: 'string' }, flakes: { type: 'string' },
    } } },
    progress_after: { type: 'string', description: 'python3 compat/tui/tracker.py ready plus the report headline at the pushed main' },
    board_updates: { type: 'string' },
    attached_client: { type: 'string', description: 'exit codes and one-line outcomes of tui-pane-geometry.sh, status-row.sh, tui-screen-diff.sh and attached-client.sh at the merged tip' },
    problems: { type: 'string' },
  },
}

const COMMON = `You are an autonomous worker on the zz TUI parity campaign (repo demfabris/zz). You are the only worker this cycle on this ${M.machine}, but the user may code here too. Rules that are not negotiable:

HOW YOU REPORT
- Your final act is the structured report the output schema describes (branch, obligations, touched_commands, touched_packages, notes). A worker that ends its turn without that report is a failed agent and its lane is dropped.
- FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches into several calls). Never use Monitor, run_in_background, or any background task, and never end your turn to wait for anything. You are done only when the branch is pushed and the report is emitted.
- Check 'date' when you start each obligation. Every obligation carries a HARD BUDGET in minutes; it is a ceiling, not a target.

SETUP
- The shared checkout is ${M.root}. Read it, add worktrees from it, but NEVER edit, stash, reset, or clean it (other sessions' uncommitted work lives there; its local main branch is stale, always use origin/main). knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate them with python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report, never hand-merge them.
- ${M.gitNote} Fetch with GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME.
- Worktree: git -C ${M.root} worktree add ${M.dev}/WORKDIR origin/main (if it exists and git -C ${M.dev}/WORKDIR status --short prints anything, stop and use ${M.dev}/WORKDIR-2 instead). Work ONLY in your worktree; cd into it for every command. There is no warm build directory on this box: your first cargo build is cold, budget 25 minutes, start it FIRST in the foreground (cargo build -p zz --jobs ${M.workerJobs} > build.log 2>&1, check the exit code) and read code while it runs only if you split the build into its own Bash call first. The pinned tmux is built once by compat/fetch-tmux.sh into compat/.cache/ (run it from your worktree; the cache is shared).
- ${M.boxNote}

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8). Prebuilt binary: ${PIN} after compat/fetch-tmux.sh (source tree beside it, read the C freely). Probe with THROWAWAY servers only: -L zzprobe-$$ sockets and -f /dev/null; kill your servers when done. Never kill a tmux or zz server you did not start (${M.protected}); never use pkill/killall on tmux or zz.
- The proof surface this cycle is compat/tui-pane-geometry.sh (both binaries attached inside an outer pinned tmux at a size, tput cols and lines in the inner pane on each side, diffed; SIZES covers 80x24 same, 100x24 same, 120x24 record). Since cycle 1 it retains diagnostics on every timeout: the outer capture, the daemon ring log under the scrubbed HOME, the client stderr, list-clients, and which wait fired. Read it end to end before running it; read compat/status-row.sh and compat/tui-screen-diff.sh for the driver shape if you need to probe anything by hand.
- Every wait is a bounded wait-for-condition on an observable, never a sleep. A timeout is a failed check that prints what it was waiting for and what the outer screen showed at that moment.

LEDGER
- compat/tui/campaign.json is the campaign's ledger; an obligation's acceptance list IS the contract. ${OWNERSHIP}
- What YOU may write in your obligations: status (active while you work; review when every clause you claim has evidence on the branch; blocked with the complete diagnosis when a defect stops you), evidence_note (rewrite it into the measurement), next_action. You NEVER write the proof block and never set verified: the gate writes proof from your evidence and the reviewer's verdict at the merged tip, because proof.revision must name the candidate commit the gate re-ran it on.
- Evidence lives at compat/tui/evidence/TUI-001/attempt-02/ on your branch (attempt-01 is cycle 1's and is not yours to edit): environment.txt (sha256 of both binaries, zz revision and dirty state, tmux -V and the pin, sw_vers, TERM, the shell, the outer tmux's size, bash --version as resolved on PATH), each fixture run's stdout and stderr as .txt, retained timeout diagnostics if any fire, notes.md with what each file is. *.log is gitignored globally: never name an evidence file .log, and run git check-ignore -v on the directory before committing. Text only, bounded sizes, real runs only: a file describing a check that did not run is a defect.
- python3 compat/tui/tracker.py check before each commit; regenerate the report with write-report and commit it in the same commit. If you touched compat/tmux-gaps.json, python3 compat/tmux-tracker.py check and write-report too. Script JSON edits with json.dump(..., indent=2) plus a trailing newline.
- A PRODUCT DECISION handed to you by this prompt is recorded in evidence_note with the old behaviour, the measured pin behaviour, the new stance, and the sentence "${DECIDED}". Decisions already recorded and NOT yours to reopen: the raw TUI hides its sidebar below 109 columns (tui.sidebar-auto-hide; TUI-004 owns changing that); the raw TUI's status row uses the pin's default theme colours (presentation:tui-status-row-theme-defaults); zz/mux.conf layers last even under an explicit -f (fabrico, 2026-09-05); the shell-integration pane title (pane.runtime-facts).

WIRE PROTOCOL RULE: none this cycle. Current PROTOCOL_VERSION is 99 and stays 99. Nothing under crates/zz-protocol, crates/zz-daemon, crates/zz-mux or crates/zz-client may change. A finding that needs the wire or the daemon is recorded in evidence_note with the field or behaviour it would need, and left.

MACHINE ETIQUETTE
- Cap parallelism: cargo build/test --jobs ${M.workerJobs}, test runs -- --test-threads=${M.workerThreads}. NEVER workspace-scale builds/tests; focused cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features --jobs ${M.workerJobs} -- -D warnings per touched crate only.
- INTEGRATION-TEST RULE: if your diff touches crates/zz-tui, run cargo test -p zz-tui and then timeout 1500 cargo test -p zz --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} > zz-test.log 2>&1 before your LAST commit (the cli_binary integration tests; about five minutes warm) and list it in proofs. A red there is yours.
- Never pipe cargo test through tail/grep (masks the exit code): > log 2>&1, check exit status, read the log.
- Load-flake rule: fails loaded + passes exact-solo = flake. Known on this box: smoke/keys-table-lifecycle under load, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else, wait_exit_holds_the_control_process_until_a_second_blank_line (can HANG under load; use a timeout when running cli_binary tests), concurrent_default_interactive_attaches_share_session_zero (headless "not a terminal", may be misattributed to a neighbouring daemon test).

DELIVERY
- One commit per obligation (its evidence and its ledger record together), subject matching repo style (git log --oneline -10). Commit with git -c commit.gpgsign=false. NO attribution trailers (no Co-Authored-By of any kind, whatever a CLAUDE.md suggests). NO comments in code; a shell fixture's header comment block (the shape the existing fixtures use) is documentation, keep it.
- PROOFS AT TIP: after your LAST commit, re-run every proof command you list with their exit codes. Stale proofs are a reviewer blocker.
- MEASURE FIRST and write the measurement down; a measurement beats a narrowing. An unprovable acceptance clause is a finding, not a failure: record it precisely.
- Your branch gets an adversarial Opus 5 review before integration: every clause you claim proved needs evidence in your diff that asserts the measured behaviour.
- When done (or stopping): push campaign/BRANCHNAME (never force, never main), report through structured output with the final tip sha in notes. Stopping early: push what runs as campaign/BRANCHNAME-wip and say what never ran.
- NO GitHub comments, NO issue 7, NO compat/board.py mutations. The orchestrator speaks for you.
`

const BATCH_VERIFY = COMMON + `
YOUR BATCH: the single macOS measurement the whole campaign is blocked on. BRANCHNAME: tui-baseline-verify. WORKDIR: zz-tui-verify.
Your zones: compat/tui/ (the ledger record for TUI-001 and its attempt-02 evidence ONLY; attempt-01 and TUI-002's record are not yours), compat/tui-pane-geometry.sh (only if you find and fix a fixture defect). Declared excursion, under the reproduction rule below ONLY: crates/zz-tui/src/. Nothing else under crates/.
PROTOCOL: none.
1. TUI-001 clause 2 on macOS. HARD BUDGET 120 MINUTES. Context you MUST read before running anything: compat/tui/campaign.json TUI-001 (status active; its evidence_note records what cycle 1 measured on Linux and what it withdrew), compat/tui/evidence/TUI-001/attempt-01/notes.md and review.md, and the FULL resolution text of tui.client-input-backpressure in compat/tmux-gaps.json. That resolution refutes the stale-pre-fix-binary explanation twice (the fixture exited 0 at pre-fix 012b4dcc, and the outer pinned tmux drains continuously so the stalled backpressure can never build in this fixture on any platform) and hands the macOS expiry forward as unexplained. DO NOT resurrect that explanation in any form. The clause reads: "Reproduce or explain the recorded macOS geometry-report timeout using retained attach, input, daemon and outer-screen diagnostics. A timeout or missing report fails the attempt." Steps: (a) build zz in your worktree, run compat/fetch-tmux.sh, write environment.txt FIRST (both sha256, git rev-parse HEAD and git status --short of the worktree — it must be clean when the runs happen, sw_vers, TERM, shell, outer size, bash --version; per the provenance rule the attestation is revision + clean tree, the hash only identifies). (b) ${RUN_ENV} compat/tui-pane-geometry.sh <your zz> ${PIN}, three consecutive runs, all stdout and stderr retained under compat/tui/evidence/TUI-001/attempt-02/. (c) THREE EXIT-0 RUNS: the recorded 2026-09-09 expiry does not reproduce on its own platform under an attested fresh build. Write the bounded explanation the evidence now supports into evidence_note: the exploratory run used an unattested target/debug/zz that was never rebuilt (its own record says so); the one specific staleness mechanism proposed so far is refuted by the registry; an attested build at this revision passes three of three on the same platform that recorded the expiry; and the fixture since cycle 1 retains full diagnostics on any future timeout, so a recurrence self-documents. State plainly that this bounds the recorded expiry to unattested binary state without naming the exact defect that binary carried, and that the acceptance's 'explain' arm is satisfied by that bound plus the reproduction attempt; the reviewer judges whether that reading holds. Set TUI-001 status review with clause 2 in clauses_proved on exactly that argument, clauses 1 and 3 re-evidenced by these runs (the fixture asserts 80x24 and 100x24 and records 120x24 with the sidebar arithmetic; put the recorded 120-column counts in evidence_note beside cycle 1's). (d) A TIMEOUT OR EXIT 2: the fixture retains its diagnostics; KEEP THEM ALL under attempt-02 and diagnose. A fixture fault (a wait on the wrong observable, PATH or bash 3.2, a race in the driver) is fixed in the fixture and proved with three consecutive exit-0 runs. A runtime fault is fixed under the declared excursion ONLY if it is contained in crates/zz-tui/src, takes under 60 minutes, keeps every zz-tui test green, keeps smoke/tui-client-input-backpressure green (${RUN_ENV} compat/run.sh --strict-geometry smoke/tui-client-input-backpressure) and passes the integration-test rule; then clause 2 closes by REPRODUCTION with the diagnosis and the fix named in notes. Otherwise set TUI-001 blocked with the complete diagnosis in evidence_note (what times out, where in crates/zz-tui/src/app.rs, tty.rs or writer.rs, the reproduction, the retained files). (e) Either way, run compat/status-row.sh once and retain its exit code (a divergence there is a recorded measurement for the ledger's open items, not yours to fix). Ledger: evidence_note rewritten as the measurement, next_action "gate: re-run the proof commands at the merged tip, record the proof, and flip TUI-002 behind it".
Proofs at tip: ${RUN_ENV} compat/tui-pane-geometry.sh three times with your zz and the pin, exit codes pasted in notes; ${RUN_ENV} compat/status-row.sh once; python3 compat/tui/tracker.py check; if crates/zz-tui changed: cargo test -p zz-tui, cargo clippy -p zz-tui --all-targets --all-features -- -D warnings, and the integration-test rule.
`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz TUI parity campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit ${M.root} itself.
HOW YOU REPORT: your final act is the structured report the output schema describes (lane, verdict, confirmed_defects, checks_run, notes). FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches); never use Monitor, run_in_background or any background task; never end your turn to wait. A reviewer that ends its turn without the report is a failed agent and the gate audits the lane itself.
SETUP: GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (${M.gitNote}). Resolve the tip: git -C ${M.root} rev-parse origin/BRANCH. Scratch worktree at that tip: git -C ${M.root} worktree add ${M.dev}/REVIEWDIR <tip> (if REVIEWDIR exists and is clean, checkout --detach the tip there instead; if it is dirty, use REVIEWDIR-2). For every cargo command export CARGO_TARGET_DIR=WORKERTARGET, the worker's warm target directory (the worker is finished, its tree is clean at the same tip, nothing else uses it now); touch crates/**/*.rs and Cargo.* in your worktree first so the shared directory rebuilds your tree, never cargo clean it, and rebuild before you spawn any binary from it. The shared checkout is ${M.root}; read it, never edit it. ${M.boxNote}
MACHINE ETIQUETTE: cargo test -p <pkg> --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} only, no workspace-scale anything, cargo output to a log file + check exit code. Throwaway pin servers -L zzprobe-$$ -f /dev/null only, kill after; never kill servers you did not start (${M.protected}), never pkill tmux or zz; never a bare tmux or zz command without -L.
METHOD, in order of value:
1. CONTRACT AUDIT: for every obligation the worker moved (worker report + git diff origin/main...HEAD -- compat/tui/campaign.json), take each acceptance clause it claims and find the evidence on the branch that proves it. The worker may have written status, evidence_note and next_action of TUI-001 only; a proof block, a verified status, an edit to TUI-002's record, to attempt-01 evidence, to another obligation, to the baseline, the milestones or the pin is a must-fix or blocker. In compat/tmux-gaps.json only appended dated measurements are allowed; any close, relocation or reopen is a blocker. ${OWNERSHIP}
2. PROOFS AT TIP: run the worker's claimed proof commands yourself at the branch tip with a zz you built in your worktree: ${RUN_ENV} compat/tui-pane-geometry.sh three times and compat/status-row.sh once, with the pin ${PIN}. If the diff touches crates/zz-tui: cargo test -p zz-tui, and timeout 1500 cargo test -p zz --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads}. Any red at tip is a blocker regardless of the report (load-flake rule applies; the known list is in the batch prompt below).
3. THE CLAUSE-2 ARGUMENT IS THE REVIEW. Read the full resolution of tui.client-input-backpressure in compat/tmux-gaps.json yourself and hold the branch's explanation against it word by word: the refuted stale-pre-fix-binary mechanism must not reappear under any wording, and every empirical claim in evidence_note (commit timestamps, recorded exit codes, what the registry says) must check out against git log and the registry text. If the worker claims the timeout reproduced, reproduce it yourself from the retained diagnostics' recipe before trusting the diagnosis; if the worker claims it did not, confirm the three exit-0 runs are real, from an attested clean worktree at the recorded revision, and that the explanation claims no more than the evidence bounds (an explanation that names a specific defect in the old binary without evidence for it is a blocker).
4. TEST HONESTY: confirm every evidence file records a run that happened (sizes, timestamps, internal consistency with the fixture's output shape); confirm no .log file is committed and git check-ignore -v flags nothing under attempt-02; confirm environment.txt matches a build you can reproduce from the recorded revision (the hash identifies, the revision attests — the provenance rule in the box note).
5. INVARIANTS: zone discipline (git diff --stat origin/main...HEAD: only compat/tui/ (TUI-001's record and attempt-02), compat/tui-pane-geometry.sh if a fixture fix is named in notes, and, under the declared excursion, crates/zz-tui/src/; anything under crates/zz-protocol, crates/zz-daemon, crates/zz-mux or crates/zz-client is a blocker); no code comments outside a fixture's header block; no attribution trailers; python3 compat/tui/tracker.py check green on the branch; the generated report regenerated, not hand-edited (regenerate and diff).
6. CALIBRATION: default to refuting each claim, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = a claimed clause without real evidence, an explanation the registry refutes, or something that would break main; must-fix = gate applies before merge; nit = mention. The load-flake rule binds you too.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes; a rejected lane gets ONE fix pass by a fresh agent and a re-review, so make every blocker precise: the probe, the expected bytes, the file). checks_run lists exact commands. Thorough but bounded: well under an hour after the build.
`

const REVIEW_VERIFY = `LANE-SPECIFIC SPOT-CHECKS (verify): run compat/tui-pane-geometry.sh three times with the branch's zz and the pin and confirm three exit-0 runs on this box; then once with a zz built from origin/main to confirm the same there (if main behaves differently the branch must explain that difference, not skip it). Confirm the timeout-diagnostics retention still works: on a throwaway copy of the fixture, break one side's wait observable and confirm the fixture exits non-zero with the retained outer capture and the daemon ring log path named. Zone discipline: outside compat/tui/ and compat/tui-pane-geometry.sh, only crates/zz-tui/src/ may have changed, and only if the notes name the runtime fault it fixes; read every hunk there.`

const LANES = [
  { key: 'verify', prompt: BATCH_VERIFY, lock: 'F-TUI-BASELINE-VERIFY', workdir: 'zz-tui-verify', reviewdir: 'zz-tui-verify-review', target: `${M.dev}/zz-tui-verify/target`, extra: REVIEW_VERIFY },
]
const OPTS = { model: 'opus', effort: 'xhigh' }

function reviewPrompt(r, rereview) {
  const head = rereview
    ? `RE-REVIEW after a reject: the first review's confirmed defects were:
${JSON.stringify(r.firstReview.confirmed_defects, null, 2)}
A fresh agent fixed the branch in place and pushed; verify each blocker is actually fixed at the NEW tip (re-run the first review's failing probes), then run the full method again on the whole branch.
`
    : ''
  return REVIEW_COMMON + '\n' + r.lane.extra + `
${head}LANE: ${r.lane.key}. BRANCH: ${r.worker.branch}. REVIEWDIR: ${r.lane.reviewdir}. WORKERTARGET: ${r.lane.target}.
THE LANE'S BATCH PROMPT (its zones, budgets and decisions; verify against it):
${r.lane.prompt}
WORKER REPORT (verify, do not trust):
${JSON.stringify(r.worker, null, 2)}`
}

function fixPrompt(r) {
  const defects = r.review.confirmed_defects || []
  const blockers = defects.filter(d => d.severity === 'blocker')
  const others = defects.filter(d => d.severity !== 'blocker')
  return `The adversarial review of the branch ${r.worker.branch} came back REJECT. You are a fresh agent taking over that lane: read the branch first (cd ${M.dev}/${r.lane.workdir}; git status --short; git log --oneline origin/main..HEAD; git diff origin/main...HEAD) and the review below before touching anything. The worktree ${M.dev}/${r.lane.workdir} is at the branch tip with a warm target; if git status --short is not empty that is the previous worker's uncommitted residue: inspect it, keep what belongs to the fix, discard nothing blindly. Fix every BLOCKER and every must-fix below on the SAME branch, push it (never force), and emit the FULL report again through structured output: every obligation of the original batch (carrying the original entries over, statuses corrected), every proof command re-run at the NEW tip, the new tip sha in notes, and what each blocker's fix was. HARD BUDGET for this whole pass: 90 minutes plus the proofs. If a blocker is a clause claimed without evidence, drop the claim: move the clause to clauses_open, set the obligation's status honestly, and put the reviewer's measurement into evidence_note.
BLOCKERS:
${JSON.stringify(blockers, null, 2)}
MUST-FIX AND NITS (apply the must-fixes too):
${JSON.stringify(others, null, 2)}
REVIEW NOTES: ${r.review.notes}
CHECKS THE REVIEWER RAN: ${JSON.stringify(r.review.checks_run)}
ORIGINAL WORKER REPORT:
${JSON.stringify(r.worker, null, 2)}
THE ORIGINAL BATCH PROMPT AND ITS RULES FOLLOW; they still bind you (same worktree, same branch, no new worktree):
${r.lane.prompt}`
}

log('TUI cycle 2: the verify lane (TUI-001 clause 2 on macOS); Opus 5 at xhigh throughout, one reviewer, one fix pass on a reject, one serialized gate that records TUI-001 and TUI-002 verified')
const laneResults = await pipeline(
  LANES,
  lane => agent(lane.prompt, { label: `worker:${lane.key}`, phase: 'Work', schema: WORKER_SCHEMA, ...OPTS })
    .then(w => ({ lane, worker: w })),
  r => {
    if (!r || !r.worker || !r.worker.branch) return r
    return agent(reviewPrompt(r, false), { label: `review:${r.lane.key}`, phase: 'Review', schema: REVIEW_SCHEMA, ...OPTS })
      .then(review => ({ ...r, review }))
  },
  r => {
    if (!r || !r.review || r.review.verdict !== 'reject') return r
    log(`${r.lane.key}: review REJECT, running one fix pass`)
    return agent(fixPrompt(r), { label: `fix:${r.lane.key}`, phase: 'Review', schema: WORKER_SCHEMA, ...OPTS })
      .then(fix => ({ ...r, firstWorker: r.worker, firstReview: r.review, worker: fix && fix.branch ? fix : r.worker, fixed: !!(fix && fix.branch) }))
  },
  r => {
    if (!r || !r.fixed) return r
    return agent(reviewPrompt(r, true), { label: `rereview:${r.lane.key}`, phase: 'Review', schema: REVIEW_SCHEMA, ...OPTS })
      .then(review => ({ ...r, review }))
  },
)
const summaries = laneResults.filter(Boolean).filter(r => r.worker && r.worker.branch)
  .map(r => ({ key: r.lane.key, lock_front: r.lane.lock, review: r.review || null, first_review: r.firstReview || null, fixed_after_reject: !!r.fixed, ...r.worker }))
log(`Lanes complete: ${summaries.length}/${LANES.length} branches pushed and reviewed. Integrating.`)
phase('Integrate')

const gatePrompt = `You are the integration gate for the zz TUI parity campaign (repo demfabris/zz, board = GitHub issue 7). The worker and the reviewer are done; you run ALONE on this ${M.machine}, full speed. FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches into several calls); never use Monitor, run_in_background or any background task, and never end your turn to wait for one; you are done only when you have emitted the structured report (merges, progress_after, board_updates, attached_client, problems). One branch this cycle. The summary below carries the lane's worker report and its review verdict; the lane may have been rejected once and fixed by a fresh agent on the same branch, in which case review is the re-review and first_review the original.
Lane summary, worker report + Opus 5 review verdict:
${JSON.stringify(summaries, null, 2)}
REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on the branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge; post the blockers as a board note on the lock front, push the rebased tip as campaign/<name>-gated, leave the branch. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a clause claimed without evidence is such a fix (drop the claim, set the status honestly, put the measurement in evidence_note). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating.
BOARD IDENTITY: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (always give a unit); withdraw and front need TRIAGE held; integrated needs MAIN held. The orchestrator holds the lock front F-TUI-BASELINE-VERIFY (6h lease from launch; expired => claim it back as ${M.holder} before the ledger step). BOARD FALLBACK: if a board command fails on GitHub authentication, append the exact command line you meant to run to ${M.root}/compat/tui/board-replay-2.sh (do not commit that file), say so in board_updates, and continue; the orchestrator replays the file. Never let a board failure stop a merge.
${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. The shared checkout ${M.root} is where you start; its local main branch is stale; never use it, never edit it, never stash or reset in it. Every commit is git -c commit.gpgsign=false commit.
knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with the trackers' write-report on every conflict, never hand-merge them.
THIS BOX: ${M.boxNote} Use ONE build directory for the gate worktree, export CARGO_TARGET_DIR=${M.dev}/zz-gate-target in every shell that runs cargo, compat/check.sh included (cold on this box the first time; budget 25 minutes for the first build; leave the directory in place for the next cycle). Before gating, run git merge-tree --write-tree origin/main <tip>; it predicts the conflicts and costs one command. Every zz you start outside the harness reads the user's ~/.tmux.conf and ~/.config/zz/mux.conf; keep HOME and XDG_CONFIG_HOME scrubbed exactly as the fixtures do.
STAGES, in order:
1. Fresh worktree: git -C ${M.root} worktree add ${M.root}-gate-verify origin/main (remove a leftover with --force first). Rebase the branch onto origin/main. Then claim MAIN --lease 4h; renew before any long stage; release after the records push.
2. If the diff touches crates/ (git diff --stat origin/main...HEAD): timeout 2400 cargo test --workspace --all-features --no-fail-fast --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (check the exit code; never pipe through tail; a wedge over 20 minutes with no output is sampled, not waited out) and cargo clippy --workspace --all-targets --all-features -- -D warnings, then ${RUN_ENV} compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <touched_commands> plus every keys and status scenario and smoke/tui-client-input-backpressure. Flake rule: fails loaded + passes exact-solo = flake (the known list is in the batch prompt); anything else red is real: fix if minutes, else SKIP the branch (no push to main; push the gated tip as campaign/<name>-gated), record, continue to the report. If the diff touches nothing under crates/, skip this stage and say so.
3. ALWAYS: build zz in the gate worktree (cargo build -p zz --jobs ${M.gateJobs}) and run with the pin ${PIN}: ${RUN_ENV} compat/tui-pane-geometry.sh (three runs), compat/status-row.sh, compat/tui-screen-diff.sh and its --self-check, compat/attached-client.sh once; retain every exit code and the last lines for attached_client (a red row on this box is real only if it is red at origin/main too — eleven corpus rows are environmental here; prove it before blaming the branch). Then python3 -B compat/tui/tracker_test.py, python3 compat/tui/tracker.py check, python3 compat/tmux-tracker.py check, and compat/check.sh (CARGO_TARGET_DIR as above). A fixture exit that contradicts the lane's claimed status is a defect: the obligation stays out of verified and the records commit says why.
4. RECORDS, in the gate worktree at the rebased tip, one commit. FIRST TUI-001, only if the lane set review and the reviewer's verdict is approve or approve-with-fixes with the fixes applied: re-run its proof commands at THIS tip (stage 3 already did; cite those runs), write compat/tui/evidence/TUI-001/attempt-02/review.md carrying the reviewer's verdict JSON verbatim, its checks_run, and your review_actions, and fill the proof block: revision = the rebased branch tip you are recording (the commit below this records commit), tmux_commit = the campaign pin, environment = one line plus a pointer to attempt-02/environment.txt, commands = the exact fixture invocations with the pin path, artifacts = the evidence files across both attempts, review = the attempt-02 review.md path; set status verified. THEN TUI-002, only if TUI-001 just went verified: its work is finished and banked from cycle 1 (status review, proof recorded at 38c22b9e by the cycle-1 gate on Linux; attempt-01 review.md carries its independent approval). Re-run its proofs on THIS box at THIS tip: ${RUN_ENV} compat/tui-screen-diff.sh <gate zz> ${PIN} twice and --self-check once (stage 3 already did; cite), write the macOS rerun outputs as compat/tui/evidence/TUI-002/attempt-02/ (environment.txt, the run outputs; note in its notes.md that attempt-02 is the gate's macOS reconfirmation of attempt-01's reviewed work), update the proof block: revision = this records tip, commands = the reruns, artifacts = both attempts, review = attempt-01/review.md; set status verified and next_action to the obligations that now become ready (TUI-003, TUI-004, TUI-010). If the screen-diff is red here and red at origin/main too, that is an environmental record: keep TUI-002 at review, write why, and do not fake it. An obligation the reviewer rejected or that stage 3 contradicted stays at the lane's honest status with the reason appended to evidence_note. Then python3 compat/tui/tracker.py write-report, python3 compat/tui/tracker.py check (both trackers if tmux-gaps.json changed), commit.
5. Push main (git push origin HEAD:main). Non-fast-forward: fetch; user-authored commits with a conflict-free disjoint rebase => bounded rerun (stage 3's fixtures), push. Never force. The campaign branch stays at its old tip on origin (never force it); say so in the report.
6. Ledger against the lock front F-TUI-BASELINE-VERIFY: candidate (--commit <tip> --branch campaign/tui-baseline-verify --base <pre-push main> --proof per stage), note (--note: obligations verified, obligations left open and why, reviewer verdict and what you did about each defect), integrated (--merge <sha> --gate "fixtures+trackers green, tests where crates changed"), release (--reason "cycle 2 integrated at <sha>"). Release MAIN.
7. Claim TRIAGE. Post one residual on TRIAGE listing what this cycle leaves for the next: the ready set after verification (TUI-003, TUI-004, TUI-010 if both verified), the four ownerless divergences carried from cycle 1 (cursor shape/blink/colour; pane-body RGB promotion; explicit default foreground under non-default status-style; the status-row %b locale difference), and anything this cycle measured or left open, each with the obligation id. Release TRIAGE.
8. python3 compat/tui/tracker.py ready and the report headline (the Fixed baseline line of knowledge/tmux/tui-parity.md) at the pushed main, into progress_after.
9. Remove only your own ${M.root}-gate-* worktree (after pushing any skipped lane's gated tip). Leave zz-tui-verify and zz-tui-verify-review in place. Sweep pgrep -fl zzprobe and reap only pids whose command line names a socket the gate created.
Never stash/reset anything in ${M.root}. Never kill tmux or zz servers you did not start (${M.protected}). Report per branch merged/sha/gate_summary/review_actions/flakes, the progress lines, board records, the fixture outcomes, problems.`

const gate = summaries.length
  ? await agent(gatePrompt, { label: 'gate:serial', phase: 'Integrate', schema: GATE_SCHEMA, ...OPTS })
  : null
if (!gate) log('No branch to integrate: the worker came back empty.')
return { lanes: laneResults, gate }
