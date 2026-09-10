export const meta = {
  name: 'tui-run-4',
  description: 'TUI parity cycle 4: three Opus 5 lanes at xhigh — canvas-close (TUI-004 clause 3 plus the whole theme landing), copy (TUI-005 pane copy mode and search), caps (TUI-009 outer-terminal capabilities) — one adversarial reviewer per lane with one bounded fix pass, one serialized gate to main',
  phases: [
    { title: 'Work', detail: 'three worktrees: canvas-close, copy, caps' },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh per lane, adversarial, pipelined behind its worker; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: 'serialized Opus 5 MAIN gate: workspace tests and clippy per merged branch, delta corpus, the attached fixtures, both trackers, the records commit, board ledger' },
  ],
}

const A = args || {}
const M = {
  root: A.root || '/home/demfabris/dev/zz',
  dev: A.dev || '/home/demfabris/dev',
  holder: A.holder || 'alienware/orchestrator',
  machine: A.machine || '16-core, 15 GB (plus 15 GB zram) CachyOS Linux box (alienware)',
  cores: A.cores || 16,
  workerJobs: A.workerJobs || 5,
  workerThreads: A.workerThreads || 2,
  gateJobs: A.gateJobs || 10,
  gateThreads: A.gateThreads || 4,
  shards: A.shards || 4,
  date: A.date || '2026-09-10',
  gateZz: A.gateZz || '/home/demfabris/dev/zz-gate-target/debug/zz',
  protected: A.protected || "nothing at launch: pgrep found no zz daemon and no tmux server running on this box when cycle 4 started; if one appears mid-run it is the user's — never touch a server on the default sockets (/run/user/1000/zz/default.sock, /tmp/tmux-1000/default) that you did not start",
  boxNote: A.boxNote || "This box (alienware) is LINUX: CachyOS (Arch-based), 16 cores, 15 GB RAM plus 15 GB zram swap; /bin/bash is 5.x (mapfile and associative arrays work); the filesystem is btrfs and accepts non-UTF-8 file names; /opt/homebrew does not exist, so the PATH=/opt/homebrew/bin:$PATH prefix this prompt carries is harmless. Where a prompt says sw_vers, use uname -a plus head -2 /etc/os-release for the OS line. The zz daemon ring log on Linux lands under the scrubbed HOME at .local/state/zz/logs/ ($XDG_STATE_HOME/zz/logs if set; ZZ_LOG_DIR overrides both). The compat caches are populated and cycles 1 and 3 ran here: the attached fixture and the TUI fixtures are known green at origin/main EXCEPT compat/status-row.sh under the box locale (LC_TIME=pt_BR.UTF-8: the pin expands %b through libc strftime, zz through locale-independent chrono; the LC_ALL=C LC_TIME=C control exits 0), and four corpus rows are environmental here (micro-flags, show-options-hooks and lane2-store through lock-command defaulting to vlock on Arch, smoke/plugin-runtime-resurrect-restore). A red row or fixture at your tip is yours only if it is green at origin/main on this box, which you prove by building main and running it before claiming otherwise. Known fixture fragility: compat/tui-stock-keys.sh root-binding-detaches compares a row carrying a timestamp and can flake on a wall-clock second boundary; a single such red reruns clean. The orchestrator pre-created your worktree at origin/main, clean, with a warm target (external deps fresh; workspace crates rebuild in a few minutes): SKIP the worktree add if your WORKDIR already exists and git status --short is empty. You share the box with two other lanes; keep to --jobs as capped. A cargo debug build of zz is NOT bit-reproducible on this box: a recorded hash identifies an artifact but cannot attest it; attestation is the recorded revision plus a clean worktree, and provenance is reproduction. The box has a real ~/.tmux.conf and a real ~/.config/zz/mux.conf and zz reads both in place: never read or edit them, and never start a tmux or zz server that would load them (the harness and the fixtures scrub HOME and XDG_CONFIG_HOME; your own probes use -f /dev/null or a scratch HOME plus a scratch XDG_CONFIG_HOME, both, because config discovery is first-existing-file-wins). Never write 'rm -rf $HOME' or 'rm -rf ~' even after re-exporting HOME to a scratch path; put the scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz; unix socket paths have a low length cap, keep them directly under /tmp): every server you start is -L zzprobe-$$ -f /dev/null, every client command names that socket. A binary you spawn from a build directory is whatever was built there last: rebuild in the worktree under test before spawning it; and after switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first (mtime freshness otherwise builds the wrong tree). Every git commit on this box is 'git -c commit.gpgsign=false commit' (the box's global commit.gpgsign=true would try to sign and can hang an unattended session). Sweep 'pgrep -fl zzprobe' at the end and reap only pids whose command line names a socket you created.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz) through the gh credential helper and works non-interactively on this box; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands so a credential miss fails instead of hanging.',
}
log(`TUI cycle 4 on ${M.machine}; root ${M.root}; holder ${M.holder}; three lanes at --jobs ${M.workerJobs}/--test-threads=${M.workerThreads}; gate --jobs ${M.gateJobs}/--test-threads=${M.gateThreads}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`
const OWNERSHIP = `LEDGER OWNERSHIP THIS CYCLE (compat/tui/campaign.json records; edit only yours): canvas-close lane = TUI-004 (its record and evidence attempt-02; attempt-01 is cycle 3's, read-only); copy lane = TUI-005; caps lane = TUI-009. TUI-001, TUI-002, TUI-003, TUI-010 are verified records and nobody's; TUI-013 and every other obligation, the baseline list, the milestones and the tmux pin are nobody's. In compat/tmux-gaps.json this cycle: the canvas-close lane alone owns options.theme-palette (its 21 items close on the consumer half of the theme landing) and the item presentation:tui-status-row-theme-colours-per-client inside tui.status-row; nobody else closes, relocates or reopens anything. A measurement that contradicts a recorded decision outside your ownership is appended to that gap's reason as a dated TUI measurement and named in notes.`

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
    touched_commands: { type: 'array', items: { type: 'string' }, description: 'tmux verb names the diff touches, for the delta corpus' },
    touched_packages: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string', description: 'everything reviewer and integrator must know: zone excursions, protocol bump done or not and what it carries, whether crates/ changed and why, flaky tests seen, decisions recorded, registry records you own that you closed or updated, the final tip sha' },
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
    attached_client: { type: 'string', description: 'exit codes and one-line outcomes of the attached fixtures at the final merged tip' },
    problems: { type: 'string' },
  },
}

const COMMON = `You are an autonomous worker on the zz TUI parity campaign (repo demfabris/zz). TWO OTHER LANES run beside you on this ${M.machine} and the user may code here too, so stay inside your zones and your parallelism caps. Rules that are not negotiable:

HOW YOU REPORT
- Your final act is the structured report the output schema describes (branch, obligations, touched_commands, touched_packages, notes). A worker that ends its turn without that report is a failed agent and its lane is dropped.
- FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches into several calls). Never use Monitor, run_in_background, or any background task, and never end your turn to wait for anything. You are done only when the branch is pushed and the report is emitted.
- Check 'date' when you start each obligation. Every task below carries a HARD BUDGET in minutes; it is a ceiling, not a target. When a budget is spent, STOP that task, write what you measured and what remains into evidence_note, set the status honestly, and move on.

SETUP
- The shared checkout is ${M.root}. Read it, add worktrees from it, but NEVER edit, stash, reset, or clean it (other sessions' uncommitted work lives there; its local main branch is stale, always use origin/main). knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate them with python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report, never hand-merge them.
- ${M.gitNote} Fetch with GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME.
- Worktree: your WORKDIR under ${M.dev} was pre-created at origin/main with a warm target (see the box note); if it is missing, git -C ${M.root} worktree add ${M.dev}/WORKDIR origin/main; if it exists and git status --short prints anything, stop and use ${M.dev}/WORKDIR-2. Work ONLY in your worktree; cd into it for every command. Build first (cargo build -p zz --jobs ${M.workerJobs} > build.log 2>&1, check the exit code).
- ${M.boxNote}

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8). Prebuilt binary: ${PIN} (source tree beside it, read the C freely). Probe with THROWAWAY servers only: -L zzprobe-$$ sockets and -f /dev/null; kill your servers when done. Never kill a tmux or zz server you did not start (${M.protected}); never use pkill/killall on tmux or zz.
- The proof surfaces: compat/tui-screen-diff.sh (the verified whole-screen comparison: both binaries attached in one outer pinned tmux, every decoded cell plus the cursor tuple at named settled checkpoints, --self-check sabotages; read its header for the declared dynamic values and the settle rule), compat/tui-pane-geometry.sh, compat/status-row.sh, compat/attached-client.sh, compat/tui-stock-keys.sh. Read the fixture you extend end to end first; every NEW fixture copies the outer-pinned-tmux driver shape (two windows in one outer pinned tmux, isolated HOME and XDG_CONFIG_HOME per side, short /tmp sockets, bounded wait_for on an observable, settle = marker on screen AND unchanged between two polls). THIS CYCLE EACH LANE WRITES ITS OWN NEW FIXTURE FILE and does not edit compat/tui-screen-diff.sh (three lanes sharing one fixture file is a rebase trap); the canvas-close lane's status-row.sh flip is the one exception, declared in its batch.
- THE DECODED-SCREEN RULE. The contract compares decoded terminal state, never the raw byte stream. The outer pinned tmux is the decoder: capture-pane -p -e re-emits SGR from its own grid, so attribute ORDER, batching, redundant resets and cursor-movement spelling collapse on both sides, while the colour CLASS does not (named, indexed and RGB stay distinct cells). Whole-screen equality plus display-message -p '#{cursor_x} #{cursor_y} #{cursor_flag} #{pane_width}x#{pane_height}' at a NAMED SETTLED CHECKPOINT is the assertion; a wait is always a bounded wait_for on an observable, never a sleep. Never fake a channel; a value that cannot be pinned is declared in the fixture header with its comparison rule.
- KNOWN STANDING DIVERGENCES (recorded; do not re-record, do not silently waive): the pane body promotes named/indexed colours to RGB and emits an explicit default foreground under a non-default status-style (the caps lane measures where the class is lost this cycle); the three theme status rows in compat/status-row.sh stay recorded until the canvas-close lane lands the theme; the status-row %b difference under this box's locale is environmental. Cursor attributes were FIXED in cycle 3 and now assert: a red on that channel is a real regression, not a standing divergence.

LEDGER
- compat/tui/campaign.json is the campaign's ledger; an obligation's acceptance list IS the contract. ${OWNERSHIP}
- What YOU may write in your obligations: status (different/active while you work; review when every clause you claim has evidence on the branch; blocked with the complete diagnosis), evidence_note (rewrite it into the measurement), next_action. You NEVER write the proof block and never set verified: the gate writes proof from your evidence and the reviewer's verdict at the merged tip.
- Evidence lives at compat/tui/evidence/<YOUR-ID>/<YOUR-ATTEMPT>/ on your branch: environment.txt first (both binary sha256, zz revision and clean state, the pin, OS line, TERM, shell, outer size, bash, locale), each run's stdout and stderr as .txt, captures as .txt, notes.md naming every file. *.log is gitignored globally: never name an evidence file .log, and run git check-ignore -v on the directory before committing. Text only, bounded sizes, real runs only: a file describing a check that did not run is a defect.
- python3 compat/tui/tracker.py check before each commit; regenerate the report with write-report and commit it in the same commit. If you touched compat/tmux-gaps.json, python3 compat/tmux-tracker.py check and write-report too, and only records you OWN this cycle. Script JSON edits with json.dump(..., indent=2) plus a trailing newline.
- A PRODUCT DECISION handed to you by this prompt is recorded in evidence_note with the old behaviour, the measured pin behaviour, the new stance, and the sentence "${DECIDED}". Decisions already recorded and NOT yours to reopen: width never invokes the sidebar (cycle 3, fabrico's contract); the raw TUI's status row uses the pin's default theme colours until a user sets theme options (presentation:tui-status-row-theme-defaults); zz/mux.conf layers last even under an explicit -f (fabrico, 2026-09-05); the shell-integration pane title (pane.runtime-facts).

WIRE PROTOCOL RULE: PROTOCOL_VERSION is 99. ONLY the canvas-close lane may take it to 100, ONLY for the theme landing under its declared arm, pure appends only, and the consumer half lands in the same push (a half-landed arm is a reviewer blocker). Outside your declared zones nothing under crates/zz-protocol, crates/zz-daemon, crates/zz-mux or crates/zz-client changes; a finding that needs the wire beyond your arm is recorded in evidence_note with the field it would need, and left.

MACHINE ETIQUETTE (three lanes share 16 cores and 15 GB)
- Cap parallelism: cargo build/test --jobs ${M.workerJobs}, test runs -- --test-threads=${M.workerThreads}. NEVER workspace-scale builds/tests; focused cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features --jobs ${M.workerJobs} -- -D warnings per touched crate only.
- INTEGRATION-TEST RULE: if your diff touches crates/, run the touched crates' unit tests and then timeout 2400 cargo test -p zz --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} > zz-test.log 2>&1 before your LAST commit (the cli_binary integration tests) and list it in proofs. A red there is yours unless the load-flake rule says otherwise.
- Never pipe cargo test through tail/grep (masks the exit code): > log 2>&1, check exit status, read the log.
- Load-flake rule: fails loaded + passes exact-solo = flake. Known on this box and under three-lane load generally: smoke/keys-table-lifecycle, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else, wait_exit_holds_the_control_process_until_a_second_blank_line (can HANG under load; use a timeout), concurrent_default_interactive_attaches_share_session_zero (headless "not a terminal", may be misattributed), zz cli_binary daemon_autostart::nested_attach_inside_a_pane_prints_the_pinned_refusal (one loaded failure in cycle 3, clean solo).

DELIVERY
- One commit per obligation task where feasible (code, fixture, evidence and ledger record together), subject matching repo style (git log --oneline -10). Commit with git -c commit.gpgsign=false. NO attribution trailers (no Co-Authored-By of any kind, whatever a CLAUDE.md suggests). NO comments in code; a shell fixture's header comment block is documentation, keep it.
- PROOFS AT TIP: after your LAST commit, re-run every proof command you list with their exit codes. Stale proofs are a reviewer blocker.
- MEASURE FIRST and write the measurement down; a measurement beats a narrowing. An unprovable acceptance clause is a finding, not a failure: record it precisely.
- Your branch gets an adversarial Opus 5 review before integration: every clause you claim proved needs evidence in your diff that asserts the PINNED behaviour, and every new fixture case needs a demonstration that it FAILS when it should (a sabotage). A fixture that only passes has proved nothing.
- When done (or stopping): push campaign/BRANCHNAME (never force, never main), report through structured output with the final tip sha in notes. Stopping early: push what runs as campaign/BRANCHNAME-wip and say what never ran.
- NO GitHub comments, NO issue 7, NO compat/board.py mutations. The orchestrator speaks for you.
`

const BATCH_CANVAS_CLOSE = COMMON + `
YOUR BATCH: TUI-004 clause 3, the piece cycle 3 left open — and the theme landing, whole or not at all. BRANCHNAME: tui-canvas-close. WORKDIR: zz-tui-canvas2.
Your zones: compat/tui/ (the TUI-004 record and compat/tui/evidence/TUI-004/attempt-02/), the new fixture compat/tui-indicators.sh, compat/status-row.sh (ONLY the flip of its three recorded theme rows to asserted, after the theme lands), crates/zz-tui/src/ (render.rs, state.rs — state.rs minimal), crates/zz-daemon/src/status.rs, and UNDER THE THEME ARM ONLY: crates/zz-mux/src/command.rs, crates/zz-mux/src/compat_manifest_tests.rs, crates/zz-protocol/src/ (the StatusLine append and PROTOCOL_VERSION), crates/zz-client/src/ bounded to StatusLine consumption, plus crates/zz/tests/.
Read first: TUI-004's record (its evidence_note carries cycle 3's whole story, the reopen, and the gate's theme measurement), compat/tui/evidence/TUI-004/attempt-01/, and the options.theme-palette and tui.status-row groups in compat/tmux-gaps.json.
1. CLAUSE 3'S FIXTURE. HARD BUDGET 150 MINUTES. Write compat/tui-indicators.sh (executable, header block, usage [ZZ_BIN [TMUX_BIN]], exit 2 on usage, the outer-tmux driver copied from compat/status-row.sh): drive both sides identically and compare whole screens at settled checkpoints for (a) copy-mode entry — the pin paints its position indicator top-right; compare the WHOLE screen including it, then exit copy mode and compare the restoration; (b) the view surface the pin reaches with copy-mode -e or view-mode equivalents — measure what the pin actually offers before writing the case; (c) the prefix state — press and hold the prefix context on both sides and compare; the pin shows NOTHING by default, so any zz hint cell on the screen is the divergence this clause exists to catch, never a waived cell (fixing zz's presentation in render.rs is in-zone); (d) display-message — compare during display and after its timeout restores the screen (message-time is a dynamic value: pin it low on both sides and declare it); (e) command-prompt — open, type, cancel; compare during and after. --self-check: one sabotage per channel (an extra hint cell, a message that differs, a prompt that leaves residue) plus one equivalence that must not fail. Three consecutive exit-0 runs at your tip.
2. THE THEME LANDING. HARD BUDGET 200 MINUTES, ONE LANDING OR NONE. The gate's cycle-3 measurement, verify then execute: the roster is 21 names (theme plus ten dark-theme-* and ten light-theme-*; read server_client_update_theme_colours for the per-client set), TMUX_OPTION_CONSUMERS in crates/zz-mux/src/command.rs goes 118 to 139, and compat_manifest_tests.rs holds consumers and tracked option items in a partition, so the consumer half CLOSES the 21 items of options.theme-palette — you own that gap this cycle: close those items with the dated measurement. The wire half: a TmuxColour field on StatusLine (NOT an RGB triple — the pin keeps colour124 indexed and the colour class is part of the contract), PROTOCOL_VERSION 99 to 100, pure appends, whatever protocol tests pin the version updated. The client half: the raw TUI honours a user-set dark-theme-green (and the light set under a light terminal report) in its status row. Then flip compat/status-row.sh's three recorded theme rows to asserted and prove the fixture exit 0. If ANY half does not fit the budget, land NONE of it: leave the measured record as it stands, keep the gap items open, say exactly what remains in evidence_note and notes. A half-landed arm is a reviewer blocker.
3. Ledger: TUI-004 to review only when clause 3 has fixture evidence at your tip; clauses 1 and 2 keep cycle 3's evidence (state that, do not re-run their whole matrix — the gate re-runs the fixtures anyway). If the theme did not land, clause 3's theme sentence stays open and TUI-004 stays review with that stated; the indicators half still counts as progress. Honest statuses beat optimistic ones: cycle 3's over-promotion of this exact record cost a reopen.
Proofs at tip: ${RUN_ENV} compat/tui-indicators.sh three times plus --self-check; compat/status-row.sh under LC_ALL=C LC_TIME=C (asserted rows if the theme landed); compat/tui-screen-diff.sh and compat/tui-pane-geometry.sh once each (regressions); cargo test on every touched crate; clippy; the integration-test rule; both trackers' check (you touch tmux-gaps.json only if the theme landed).
`

const BATCH_COPY = COMMON + `
YOUR BATCH: TUI-005, pane copy mode and search. BRANCHNAME: tui-copy-mode. WORKDIR: zz-tui-copy.
Your zones: compat/tui/ (the TUI-005 record and compat/tui/evidence/TUI-005/attempt-01/), the new fixture compat/tui-copy-mode.sh, compat/scenarios/smoke/fixtures/ helpers, crates/zz-tui/src/app.rs and input.rs (render.rs is SHARED with the canvas-close lane — touch it only if a copy-mode presentation fix demands it, keep the hunk minimal and name it in notes), crates/zz/tests/.
Read first: TUI-005's record (evidence_note: ordinary pane TerminalUiCommand search reports "terminal search is unsupported here" — that is the known defect this obligation opens on), and the copy-mode.action-fidelity, copy-mode.command-fidelity, keys.copy-mode-native-numeric-prefix and options.native-mode-styles groups in compat/tmux-gaps.json (their closures are prior evidence with narrower contracts; their accepted decisions are not yours to reopen — a contradicting measurement is appended, dated).
1. THE COPY TABLES THROUGH ACTUAL INPUT. HARD BUDGET 180 MINUTES. Write compat/tui-copy-mode.sh (the driver shape, controlled pane content seeded identically on both sides — a numbered line file both shells cat, declared in the header): stock emacs AND vi tables driven by send-keys into the attached client — enter copy mode, line/page movement (up/down/half/full), numeric counts (5j, C-u), selection start, rectangle toggle, copy, cancel; forward and backward search with editing (type, backspace, submit), repeat search (n/N). Per case: whole decoded screen plus cursor at a settled checkpoint, and after every copy the paste buffer BYTES compared (show-buffer both sides). mode-keys set explicitly per table on both sides.
2. PRESENTATION AND LIFECYCLE. HARD BUDGET 150 MINUTES. Mode and search prompts as cells, selection styles, cursor through the whole flow; a live mode-keys change mid-session picked up by the next copy-mode entry; return to terminal input proved by typing into the shell after cancel and comparing. Selection style options (mode-style) set identically and compared under the colour-class rule.
3. THE KNOWN SEARCH DEFECT AND THE INVENTORY. HARD BUDGET 120 MINUTES. Reproduce stock forward/backward search in an ORDINARY pane (not command output) on both sides; zz's recorded failure is "terminal search is unsupported here" from TerminalUiCommand. If the fix is contained in crates/zz-tui/src/app.rs or input.rs, fix it and prove with the clause-1 search cases; if it reaches the daemon or the wire, record exactly what it needs and leave it, status honest. Keep ordinary pane search proof separate from command-output search (a case name says which). Then inventory the remaining copy actions against their existing authority: list in evidence_note which stock copy-mode commands the fixture now covers and which remain, so the claim is a covered subset, never "complete table coverage".
Ledger: TUI-005 to review only when clauses 1 and 2 have fixture evidence and clause 3's subset statement is in evidence_note; active with the same honesty otherwise.
Proofs at tip: ${RUN_ENV} compat/tui-copy-mode.sh three times plus --self-check (sabotages: a one-sided extra movement key, a one-sided mode-style, a buffer byte difference); compat/tui-screen-diff.sh and compat/tui-stock-keys.sh once each (regressions); cargo test -p zz-tui; clippy; the integration-test rule; tracker check.
`

const BATCH_CAPS = COMMON + `
YOUR BATCH: TUI-009, outer-terminal capabilities and fidelity. BRANCHNAME: tui-caps. WORKDIR: zz-tui-caps.
Your zones: compat/tui/ (the TUI-009 record and compat/tui/evidence/TUI-009/attempt-01/), the new fixture compat/tui-caps.sh, crates/zz-tui/src/tty.rs and input.rs (render.rs SHARED with canvas-close — minimal, named), crates/zz/src/lib.rs (the client terminal flags), crates/zz/tests/.
Read first: TUI-009's record and the options.client-terminal-negotiation, options.terminal-engine-limits, formats.terminal-cells and formats.terminal-runtime groups in compat/tmux-gaps.json (accepted negotiation/engine decisions get bounded remeasurement, not reopening).
1. THE CAPABILITY MATRIX. HARD BUDGET 120 MINUTES. Declare the finite matrix from both sides' sources (the pin's tty.c/tty-keys.c/tty-features.c and zz's tty.rs): legacy versus extended keys, user keys, bracketed paste, focus, mouse, colour/theme replies (OSC 10/11, DA), Unicode widths, alternate-screen restoration on detach. Write it into the fixture header and evidence as the declared coverage list — covered cases driven, uncovered channels named, never silently absent. Drive at least: one legacy-terminal case (outer TERM without extended keys) and one extended-key case, comparing what each side EMITS to the outer terminal and what facts each daemon records for the client.
2. THE CLIENT FLAGS. HARD BUDGET 150 MINUTES. -2, -u and -T on both binaries: exact stdout/stderr/exit, the client facts each side records (client_termname, client_termfeatures through display-message -p), and the screen consequence where one exists. TUI-009's evidence note records zz's CLI consuming several of these without applying them: measure each, fix inside crates/zz/src or crates/zz-tui where contained, and record any ignored flag as a visible obligation in evidence_note (an ignored flag is a finding, not a pass).
3. COLOUR CLASSES UNDER PALETTE CHANGES. HARD BUDGET 150 MINUTES. The two recorded pane-body divergences: named/indexed colours promoted to RGB, and an explicit default foreground under a non-default status-style. MEASURE where the class is lost first — the TUI reconstructs cells and styles client-side, so trace one named-colour cell from daemon frame to emitted bytes. If the loss is inside crates/zz-tui, fix it and prove with palette-change cases (set a palette entry, compare the pin's re-render against zz's under the colour-class rule; equivalent escape spellings must keep collapsing); the screen-diff colour-classes case then asserts fully and you say so in notes for the gate. If the class is already lost in the daemon's frame or the wire, record exactly the field or behaviour it would need and leave it — the wire is not yours this cycle.
Ledger: TUI-009 to review only when the matrix is declared with its covered subset driven, the three flags are measured with their dispositions, and clause 3 is either fixed-and-proved or precisely recorded; active otherwise.
Proofs at tip: ${RUN_ENV} compat/tui-caps.sh three times plus --self-check (sabotages: a one-sided flag, a one-sided palette entry, a legacy-terminal case driven with extended keys); compat/tui-screen-diff.sh once (regression); cargo test -p zz-tui -p zz as touched; clippy; the integration-test rule; tracker check.
`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz TUI parity campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit ${M.root} itself. Other lanes and reviews may run beside you; keep to the parallelism caps.
HOW YOU REPORT: your final act is the structured report the output schema describes (lane, verdict, confirmed_defects, checks_run, notes). FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches); never use Monitor, run_in_background or any background task; never end your turn to wait. A reviewer that ends its turn without the report is a failed agent and the gate audits the lane itself.
SETUP: GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (${M.gitNote}). Resolve the tip: git -C ${M.root} rev-parse origin/BRANCH. Scratch worktree at that tip: git -C ${M.root} worktree add ${M.dev}/REVIEWDIR <tip> (if REVIEWDIR exists and is clean, checkout --detach the tip there instead; if it is dirty, use REVIEWDIR-2). For every cargo command export CARGO_TARGET_DIR=WORKERTARGET, the worker's warm target directory (the worker is finished, its tree is clean at the same tip; the OTHER lanes' targets are NOT yours); touch crates/**/*.rs and Cargo.* in your worktree first so the shared directory rebuilds your tree, never cargo clean it, and rebuild before you spawn any binary from it. The shared checkout is ${M.root}; read it, never edit it. ${M.boxNote}
MACHINE ETIQUETTE: cargo test -p <pkg> --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} only, no workspace-scale anything, cargo output to a log file + check exit code. Throwaway pin servers -L zzprobe-$$ -f /dev/null only, kill after; never kill servers you did not start (${M.protected}), never pkill tmux or zz; never a bare tmux or zz command without -L.
METHOD, in order of value:
1. CONTRACT AUDIT: for every obligation the worker moved to review (worker report + git diff origin/main...HEAD -- compat/tui/campaign.json), take each acceptance clause it claims and find the evidence on the branch that proves it against the PIN. A fixture that only passes is not evidence: the sabotage runs must exist and must fail for the right reason. A clause claimed from a file that describes a run rather than records one is a blocker. The worker may have written status, evidence_note and next_action of ITS obligation only; a proof block, a verified status, an edit to a verified record, to another lane's obligation, to the baseline, the milestones or the pin is a must-fix or blocker. In compat/tmux-gaps.json only the records this lane OWNS may change (its batch prompt names them); anything else is a blocker. ${OWNERSHIP}
2. PROOFS AT TIP: run the worker's claimed proof commands yourself at the branch tip with a zz you built in your worktree, with the pin ${PIN}. If the diff touches crates: the touched crates' tests and timeout 2400 cargo test -p zz --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads}. Any red at tip is a blocker regardless of the report (load-flake rule applies; the known list is in the batch prompt below).
3. ORACLE SPOT-CHECKS on the pin yourself: for every behaviour the lane changed or asserted, drive the PIN in a throwaway server and confirm the branch's fixture asserts what the pin actually does (read the pinned C where the batch prompt names it). For every PRODUCT DECISION recorded, confirm the sentence, the old behaviour and the measured pin behaviour are present and true. Check every wait in new fixture code is a bounded wait_for; grep for sleep.
4. TEST HONESTY: run every new fixture and its sabotages yourself; ADD one sabotage of your own on a throwaway copy and confirm the fixture reports it with a location; confirm evidence files are real captures from runs (sizes, contents), no .log files committed, git check-ignore -v flags nothing under the evidence directory, environment.txt matches a build you can reproduce from the recorded revision (hash identifies, revision attests).
5. INVARIANTS: zone discipline (git diff --stat origin/main...HEAD against the lane's declared zones; anything outside is a blocker unless notes declare it and the batch prompt allows it); the wire rule (PROTOCOL_VERSION moves only on the canvas-close lane's theme arm, pure appends, consumer half in the same landing, manifest updated — anything else touching the wire is a blocker; a HALF-landed theme arm is a blocker in both directions); no code comments outside fixture headers; no attribution trailers; both trackers' check green on the branch; generated reports regenerated, not hand-edited (regenerate and diff).
6. CALIBRATION: default to refuting each claim, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = a claimed clause without real evidence, a fixture that cannot fail, a zone or wire violation, or something that would break main; must-fix = gate applies before merge; nit = mention. The load-flake rule binds you too.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes; a rejected lane gets ONE fix pass by a fresh agent and a re-review, so make every blocker precise: the probe, the expected bytes, the file). checks_run lists exact commands. Thorough but bounded: well under 90 minutes after the build.
`

const REVIEW_CANVAS_CLOSE = `LANE-SPECIFIC SPOT-CHECKS (canvas-close): drive the pin's copy-mode indicator, display-message and command-prompt yourself and confirm the fixture asserts the pin's presentation, not zz's; press the prefix on an attached zz client and confirm no hint cell survives anywhere on the screen (the clause forbids waived cells). If the theme landed: confirm PROTOCOL_VERSION is exactly 100 with pure appends (read the wire diff hunk by hunk), the manifest counts moved 118 to 139 with the partition test green, options.theme-palette's 21 items closed with dated measurements, a user-set dark-theme-green reaching the raw TUI against the pin's same-option behaviour, and the three status-row rows now asserting (run the fixture; sabotage one theme colour one-sided and confirm it goes red). If the theme did NOT land: confirm nothing under crates/zz-mux, crates/zz-protocol or crates/zz-client changed and the gap items are untouched. THE CYCLE-3 LESSON BINDS THE GATE THROUGH YOU: state in notes, explicitly, whether EVERY clause of TUI-004 now has evidence — the gate promoted this record once with a clause open and the orchestrator reopened it.`
const REVIEW_COPY = `LANE-SPECIFIC SPOT-CHECKS (copy): drive five stock copy-mode sequences of your own choosing (mixed emacs/vi, at least one numeric count and one backward-search edit) through send-keys on both sides and compare against the fixture's claims; compare paste-buffer bytes yourself after a rectangle copy; confirm the ordinary-pane search case really runs in an ordinary pane (not command output) and that its disposition matches the diff (fixed in zz-tui, or recorded with what it needs); confirm the inventory in evidence_note lists covered and uncovered actions rather than claiming the whole table.`
const REVIEW_CAPS = `LANE-SPECIFIC SPOT-CHECKS (caps): pick one declared-covered capability and one declared-uncovered channel and confirm the fixture drives the first and names the second; run the -2/-u/-T cases yourself and check the recorded client facts against display-message on both sides; for clause 3, trace one named-colour cell yourself at the tip (print \\e[31mX, capture through the outer tmux, read the class) and confirm the branch's story — fixed with the class preserved, or recorded with the exact loss point named; confirm accepted negotiation decisions were remeasured, not reopened.`

const LANES = [
  { key: 'canvas-close', prompt: BATCH_CANVAS_CLOSE, lock: 'F-TUI-CYCLE-4-LANES', workdir: 'zz-tui-canvas2', reviewdir: 'zz-tui-canvas2-review', target: `${M.dev}/zz-tui-canvas2/target`, extra: REVIEW_CANVAS_CLOSE },
  { key: 'copy', prompt: BATCH_COPY, lock: 'F-TUI-CYCLE-4-LANES', workdir: 'zz-tui-copy', reviewdir: 'zz-tui-copy-review', target: `${M.dev}/zz-tui-copy/target`, extra: REVIEW_COPY },
  { key: 'caps', prompt: BATCH_CAPS, lock: 'F-TUI-CYCLE-4-LANES', workdir: 'zz-tui-caps', reviewdir: 'zz-tui-caps-review', target: `${M.dev}/zz-tui-caps/target`, extra: REVIEW_CAPS },
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
  return `The adversarial review of the branch ${r.worker.branch} came back REJECT. You are a fresh agent taking over that lane: read the branch first (cd ${M.dev}/${r.lane.workdir}; git status --short; git log --oneline origin/main..HEAD; git diff origin/main...HEAD) and the review below before touching anything. The worktree ${M.dev}/${r.lane.workdir} is at the branch tip with a warm target; if git status --short is not empty that is the previous worker's uncommitted residue: inspect it, keep what belongs to the fix, discard nothing blindly. Fix every BLOCKER and every must-fix below on the SAME branch, push it (never force), and emit the FULL report again through structured output: every obligation of the original batch (carrying the original entries over, statuses corrected), every proof command re-run at the NEW tip, the new tip sha in notes, and what each blocker's fix was. HARD BUDGET for this whole pass: 120 minutes plus the proofs. If a blocker is a clause claimed without evidence, drop the claim: move the clause to clauses_open, set the obligation's status honestly, and put the reviewer's measurement into evidence_note.
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

log('TUI cycle 4: canvas-close (TUI-004 clause 3 + the theme landing), copy (TUI-005), caps (TUI-009); Opus 5 at xhigh throughout, one reviewer per lane, one fix pass on a reject, one serialized gate')
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

const gatePrompt = `You are the integration gate for the zz TUI parity campaign (repo demfabris/zz, board = GitHub issue 7). The workers and reviewers are done; you run ALONE on this ${M.machine}, full speed. FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches into several calls); never use Monitor, run_in_background or any background task, and never end your turn to wait for one; you are done only when you have emitted the structured report (merges, progress_after, board_updates, attached_client, problems). Up to three branches this cycle, serialized through ONE gate worktree. The summaries below carry each lane's worker report and review verdict; a lane may have been rejected once and fixed on the same branch, in which case review is the re-review and first_review the original.
Lane summaries, worker reports + Opus 5 review verdicts:
${JSON.stringify(summaries, null, 2)}
REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on that branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge that branch; post the blockers as a board note on the lock front, push the rebased tip as campaign/<name>-gated, leave the branch, and continue with the other lanes. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a clause claimed without evidence is such a fix (drop the claim, set the status honestly, put the measurement in evidence_note). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating that lane.
VERIFIED MEANS EVERY CLAUSE. Cycle 3's gate set TUI-004 verified while its own evidence_note said clause 3 was open, and the orchestrator had to reopen it in a records commit. Before you set ANY obligation verified: read its acceptance list and its evidence_note at your tip, and if any clause is stated open, partial, or covered-subset, the status stays at the lane's honest value and the records commit says why. There is no partial verified.
BOARD IDENTITY: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (always give a unit); withdraw and front need TRIAGE held; integrated needs MAIN held. The orchestrator holds the single lock front F-TUI-CYCLE-4-LANES covering all three lanes (expired => claim it back as ${M.holder} before the ledger step). BOARD FALLBACK: if a board command fails on GitHub authentication, append the exact command line to ${M.root}/compat/tui/board-replay-4.sh (do not commit that file), say so in board_updates, and continue; the orchestrator replays the file. Never let a board failure stop a merge.
${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. The shared checkout ${M.root} is where you start; its local main branch is stale; never use it, never edit it, never stash or reset in it. Every commit is git -c commit.gpgsign=false commit.
knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with the trackers' write-report on every conflict, never hand-merge them. compat/tui/campaign.json and compat/tmux-gaps.json merge by record id: on a conflict, take each lane's changes to the records it OWNS and nothing else.
THIS BOX: ${M.boxNote} Use ONE build directory for the gate worktree, export CARGO_TARGET_DIR=${M.dev}/zz-gate-target in every shell that runs cargo, compat/check.sh included (warm from cycle 3; leave it in place). Before each branch's gate, run git merge-tree --write-tree <current base> <tip>; it predicts the conflicts and costs one command.
ORDER: gate the branches in this order where present: canvas-close (its status-row flips and any protocol bump define what the others are judged against), then copy, then caps. Later branches rebase onto the accumulating result.
STAGES, per branch, in the gate worktree ${M.root}-gate-tui4 (create once: git -C ${M.root} worktree add ${M.root}-gate-tui4 origin/main; remove a leftover with --force first). Claim MAIN --lease 6h before the first branch; renew before any long stage; release after the records push.
1. Rebase the branch onto the current gate HEAD (origin/main for the first, the accumulated result after). A conflict inside files the lane does not own is a skip (push campaign/<name>-gated, record, continue).
2. ALWAYS for this cycle (every lane touches crates/): timeout 2400 cargo test --workspace --all-features --no-fail-fast --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (check the exit code; never pipe through tail; a wedge over 20 minutes with no output is sampled, not waited out) and cargo clippy --workspace --all-targets --all-features --jobs ${M.gateJobs} -- -D warnings, then ${RUN_ENV} compat/run.sh --strict-geometry --delta <base>..HEAD --commands <that lane's touched_commands> plus every keys and status scenario and smoke/tui-client-input-backpressure. Flake rule: fails loaded + passes exact-solo = flake (the known list is in the batch prompts); anything else red is real: fix if minutes, else SKIP the branch (no merge; push the gated tip as campaign/<name>-gated), record, continue with the next branch from the last good HEAD.
3. Fixtures at the accumulated tip after each merge: build zz (cargo build -p zz --jobs ${M.gateJobs}) and run with the pin ${PIN}: ${RUN_ENV} compat/tui-screen-diff.sh and its --self-check, compat/tui-pane-geometry.sh, compat/tui-stock-keys.sh, compat/status-row.sh (LC_ALL=C LC_TIME=C, plus one run under the box locale to confirm the %b record still stands), each lane's new fixture (tui-indicators.sh, tui-copy-mode.sh, tui-caps.sh) with its --self-check once the owning branch is merged, and compat/attached-client.sh once at the FINAL tip; retain every exit code and the last lines for attached_client. Then python3 -B compat/tui/tracker_test.py, python3 compat/tui/tracker.py check, python3 compat/tmux-tracker.py check, and compat/check.sh. A fixture exit that contradicts a lane's claimed status is a defect: the obligation stays out of verified and the records commit says why.
4. RECORDS, after the last branch, one commit at the final tip: for each obligation whose lane merged with verdict approve or approve-with-fixes (fixes applied) AND whose every clause has evidence (the VERIFIED MEANS EVERY CLAUSE rule above): re-run that obligation's proof commands at THIS tip (stage 3 already did; cite those runs), write compat/tui/evidence/<ID>/<attempt>/review.md carrying the reviewer's verdict JSON verbatim, its checks_run, and your review_actions, and fill the proof block: revision = the final pre-records tip, tmux_commit = the campaign pin, environment = one line plus a pointer to environment.txt, commands = the exact invocations with exit codes, artifacts = the evidence files, review = the review.md path; set status verified and next_action to what its closure unlocks (TUI-004 verified => TUI-006 and TUI-007 become ready; TUI-005 and TUI-007 later feed TUI-008; TUI-009 feeds TUI-012). An obligation with any open clause, a rejected review, a skip, or a stage-3 contradiction stays at the lane's honest status with the reason appended to evidence_note. If PROTOCOL_VERSION moved to 100, say so in problems with exactly what it carries. Then python3 compat/tui/tracker.py write-report, python3 compat/tmux-tracker.py write-report (if gaps changed), both checks, commit.
5. Push main (git push origin HEAD:main). Non-fast-forward: fetch; user-authored commits with a conflict-free disjoint rebase => bounded rerun (stage 3's fixtures at the new tip), push. Never force. Campaign branches stay at their old tips on origin (never force them); say so in the report.
6. Ledger against the single lock front F-TUI-CYCLE-4-LANES, once, after all branches: one candidate per merged branch posted on that front (--commit <its rebased tip> --branch <its campaign branch> --base <pre-push main> --proof per stage), then one note covering every lane (--note: obligations verified, left open and why, each reviewer verdict and what you did about each defect; a skipped lane's blockers included), one integrated (--merge <the pushed sha> --gate "workspace tests+clippy+delta corpus+fixtures green"), one release (--reason "cycle 4 integrated at <sha>"). Release MAIN.
7. Claim TRIAGE. Post one residual listing what this cycle leaves: obligations left open with why, the divergences still without owners (pane-body colour classes if the caps lane recorded rather than fixed; anything the theme landing left), the ready set at the pushed main, anything skipped, each with the obligation id. Release TRIAGE.
8. python3 compat/tui/tracker.py ready and the report headline (the Fixed baseline line of knowledge/tmux/tui-parity.md) at the pushed main, into progress_after.
9. Remove only your own ${M.root}-gate-tui4 worktree (after pushing any skipped lane's gated tip). Leave the lane and review worktrees in place. Sweep pgrep -fl zzprobe and reap only pids whose command line names a socket the gate created.
Never stash/reset anything in ${M.root}. Never kill tmux or zz servers you did not start (${M.protected}). Report per branch merged/sha/gate_summary/review_actions/flakes, the progress lines, board records, the fixture outcomes, problems.`

const gate = summaries.length
  ? await agent(gatePrompt, { label: 'gate:serial', phase: 'Integrate', schema: GATE_SCHEMA, ...OPTS })
  : null
if (!gate) log('No branch to integrate: every worker came back empty.')
return { lanes: laneResults, gate }
