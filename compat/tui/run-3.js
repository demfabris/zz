export const meta = {
  name: 'tui-run-3',
  description: 'TUI parity cycle 3: three Opus 5 lanes at xhigh — TUI-003 (stock launcher/command/keys), TUI-004 (default canvas, status, borders, the sidebar decision), TUI-010 (slow output, recovery, lifecycle) — one adversarial reviewer per lane with one bounded fix pass, one serialized gate to main',
  phases: [
    { title: 'Work', detail: 'three worktrees: keys, canvas, flow' },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh per lane, adversarial, pipelined behind its worker; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: 'serialized Opus 5 MAIN gate: workspace tests and clippy per merged branch, delta corpus, the four attached fixtures, both trackers, the records commit, board ledger' },
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
  date: A.date || '2026-09-09',
  gateZz: A.gateZz || '/home/demfabris/dev/zz-gate-target/debug/zz',
  protected: A.protected || "nothing at launch: pgrep found no zz daemon and no tmux server running on this box when cycle 3 started; if one appears mid-run it is the user's — never touch a server on the default sockets (/run/user/1000/zz/default.sock, /tmp/tmux-1000/default) that you did not start",
  boxNote: A.boxNote || "This box (alienware) is LINUX: CachyOS (Arch-based), 16 cores, 15 GB RAM plus 15 GB zram swap; /bin/bash is 5.x (mapfile and associative arrays work); the filesystem is btrfs and accepts non-UTF-8 file names; /opt/homebrew does not exist, so the PATH=/opt/homebrew/bin:$PATH prefix this prompt carries is harmless. Where a prompt says sw_vers, use uname -a plus head -2 /etc/os-release for the OS line. The zz daemon ring log on Linux lands under the scrubbed HOME at .local/state/zz/logs/ ($XDG_STATE_HOME/zz/logs if set; ZZ_LOG_DIR overrides both). The compat caches are populated (pin built, corpus cloned) and cycle 1 ran here: the formats scenario, the attached fixture and all four TUI fixtures are known green at origin/main EXCEPT compat/status-row.sh, which exits 1 on this box under the box locale (LC_TIME=pt_BR.UTF-8: the pin expands %b through libc strftime, zz through locale-independent chrono; 2 of 11 rows, %b only; the LC_ALL=C LC_TIME=C control exits 0) — that red is environmental here, recorded in TUI-001's ledger record, and not yours unless your diff changes it. A red row or fixture at your tip is yours only if it is green at origin/main on this box, which you prove by building main and running it before claiming otherwise. The orchestrator pre-created your worktree at origin/main, clean, with a warm target (external deps fresh; workspace crates rebuild in a few minutes): SKIP the worktree add if your WORKDIR already exists and git status --short is empty. Cold origin/main debug build reference: 5m34s at --jobs 16 alone; you share the box with two other lanes, keep to --jobs as capped. A cargo debug build of zz is NOT bit-reproducible on this box (cycle 1 measured four distinct sha256 for identical source): a recorded hash identifies an artifact but cannot attest it; attestation is the recorded revision plus a clean worktree, and provenance is reproduction. The box has a real ~/.tmux.conf and a real ~/.config/zz/mux.conf and zz reads both in place: never read or edit them, and never start a tmux or zz server that would load them (the harness and the fixtures scrub HOME and XDG_CONFIG_HOME; your own probes use -f /dev/null or a scratch HOME plus a scratch XDG_CONFIG_HOME, both, because config discovery is first-existing-file-wins). Never write 'rm -rf $HOME' or 'rm -rf ~' even after re-exporting HOME to a scratch path; put the scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz; unix socket paths have a low length cap, keep them directly under /tmp): every server you start is -L zzprobe-$$ -f /dev/null, every client command names that socket. A binary you spawn from a build directory is whatever was built there last: rebuild in the worktree under test before spawning it; and after switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first (mtime freshness otherwise builds the wrong tree). Every git commit on this box is 'git -c commit.gpgsign=false commit' (the box's global commit.gpgsign=true would try to sign and can hang an unattended session). Sweep 'pgrep -fl zzprobe' at the end and reap only pids whose command line names a socket you created.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz) through the gh credential helper and works non-interactively on this box; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands so a credential miss fails instead of hanging.',
}
log(`TUI cycle 3 on ${M.machine}; root ${M.root}; holder ${M.holder}; three lanes at --jobs ${M.workerJobs}/--test-threads=${M.workerThreads}; gate --jobs ${M.gateJobs}/--test-threads=${M.gateThreads}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`
const OWNERSHIP = `LEDGER OWNERSHIP THIS CYCLE (compat/tui/campaign.json records; edit only yours): keys lane = TUI-003; canvas lane = TUI-004; flow lane = TUI-010. TUI-001, TUI-002 are verified records and nobody's; TUI-013 and every other obligation, the baseline list, the milestones and the tmux pin are nobody's. In compat/tmux-gaps.json this cycle: the canvas lane alone owns the TUI portion of tui.sidebar-auto-hide and, under its theme arm only, presentation items inside tui.status-row; the flow lane alone owns tui.client-output-queue-budget; nobody else closes, relocates or reopens anything. A measurement that contradicts a recorded decision outside your ownership is appended to that gap's reason as a dated TUI measurement and named in notes.`

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
    attached_client: { type: 'string', description: 'exit codes and one-line outcomes of tui-pane-geometry.sh, status-row.sh, tui-screen-diff.sh and attached-client.sh at the final merged tip' },
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
- The proof surfaces: compat/tui-screen-diff.sh (the whole-screen comparison TUI-002 verified: both binaries attached in one outer pinned tmux, every decoded cell plus the cursor tuple at named settled checkpoints, --self-check sabotages; read its header for the declared dynamic values and the settle rule), compat/tui-pane-geometry.sh, compat/status-row.sh, compat/attached-client.sh. Read the fixture you extend end to end first; every new fixture or case copies the outer-pinned-tmux driver shape (two windows in one outer pinned tmux, isolated HOME and XDG_CONFIG_HOME per side, short /tmp sockets, bounded wait_for on an observable, settle = marker on screen AND unchanged between two polls).
- THE DECODED-SCREEN RULE. The contract compares decoded terminal state, never the raw byte stream. The outer pinned tmux is the decoder: capture-pane -p -e re-emits SGR from its own grid, so attribute ORDER, batching, redundant resets and cursor-movement spelling collapse on both sides, while the colour CLASS does not (named, indexed and RGB stay distinct cells). Whole-screen equality plus display-message -p '#{cursor_x} #{cursor_y} #{cursor_flag} #{pane_width}x#{pane_height}' at a NAMED SETTLED CHECKPOINT is the assertion; a wait is always a bounded wait_for on an observable, never a sleep. Never fake a channel; a value that cannot be pinned is declared in the fixture header with its comparison rule.
- KNOWN STANDING DIVERGENCES you will see in fixtures (recorded, with owners; do not re-record, do not silently waive): cursor shape/blink/colour at every checkpoint (zz writes DECSCUSR and OSC 12, crates/zz-tui/src/render.rs:1770-1799 — the canvas lane owns fixing it this cycle); the pane body promoting named/indexed colours to RGB and the explicit default foreground under a non-default status-style (daemon frame representation, no owner this cycle); the status-row %b locale difference on this box (environmental, LC_ALL=C control).

LEDGER
- compat/tui/campaign.json is the campaign's ledger; an obligation's acceptance list IS the contract. ${OWNERSHIP}
- What YOU may write in your obligations: status (different/active while you work; review when every clause you claim has evidence on the branch; blocked with the complete diagnosis), evidence_note (rewrite it into the measurement), next_action. You NEVER write the proof block and never set verified: the gate writes proof from your evidence and the reviewer's verdict at the merged tip.
- Evidence lives at compat/tui/evidence/<YOUR-ID>/attempt-01/ on your branch: environment.txt first (both binary sha256, zz revision and clean state, the pin, OS line, TERM, shell, outer size, bash, locale), each run's stdout and stderr as .txt, captures as .txt, notes.md naming every file. *.log is gitignored globally: never name an evidence file .log, and run git check-ignore -v on the directory before committing. Text only, bounded sizes, real runs only: a file describing a check that did not run is a defect.
- python3 compat/tui/tracker.py check before each commit; regenerate the report with write-report and commit it in the same commit. If you touched compat/tmux-gaps.json, python3 compat/tmux-tracker.py check and write-report too, and only records you OWN this cycle. Script JSON edits with json.dump(..., indent=2) plus a trailing newline.
- A PRODUCT DECISION handed to you by this prompt is recorded in evidence_note with the old behaviour, the measured pin behaviour, the new stance, and the sentence "${DECIDED}". Decisions already recorded and NOT yours to reopen unless this prompt hands you the record: the raw TUI's status row uses the pin's default theme colours (presentation:tui-status-row-theme-defaults); zz/mux.conf layers last even under an explicit -f (fabrico, 2026-09-05); the shell-integration pane title (pane.runtime-facts).

WIRE PROTOCOL RULE: PROTOCOL_VERSION is 99. ONLY the canvas lane may take it to 100, ONLY for the recorded theme-colours shape under its declared theme arm, pure appends only. Outside your declared zones nothing under crates/zz-protocol, crates/zz-daemon, crates/zz-mux or crates/zz-client changes; a finding that needs the wire beyond your arm is recorded in evidence_note with the field it would need, and left.

MACHINE ETIQUETTE (three lanes share 16 cores and 15 GB)
- Cap parallelism: cargo build/test --jobs ${M.workerJobs}, test runs -- --test-threads=${M.workerThreads}. NEVER workspace-scale builds/tests; focused cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features --jobs ${M.workerJobs} -- -D warnings per touched crate only.
- INTEGRATION-TEST RULE: if your diff touches crates/zz-tui, crates/zz or crates/zz-daemon, run the touched crates' unit tests and then timeout 2400 cargo test -p zz --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} > zz-test.log 2>&1 before your LAST commit (the cli_binary integration tests) and list it in proofs. A red there is yours unless the load-flake rule says otherwise.
- Never pipe cargo test through tail/grep (masks the exit code): > log 2>&1, check exit status, read the log.
- Load-flake rule: fails loaded + passes exact-solo = flake. Known on this box and under three-lane load generally: smoke/keys-table-lifecycle, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else, wait_exit_holds_the_control_process_until_a_second_blank_line (can HANG under load; use a timeout), concurrent_default_interactive_attaches_share_session_zero (headless "not a terminal", may be misattributed to a neighbouring daemon test).

DELIVERY
- One commit per obligation task where feasible (code, fixture, evidence and ledger record together), subject matching repo style (git log --oneline -10). Commit with git -c commit.gpgsign=false. NO attribution trailers (no Co-Authored-By of any kind, whatever a CLAUDE.md suggests). NO comments in code; a shell fixture's header comment block is documentation, keep it.
- PROOFS AT TIP: after your LAST commit, re-run every proof command you list with their exit codes. Stale proofs are a reviewer blocker.
- MEASURE FIRST and write the measurement down; a measurement beats a narrowing. An unprovable acceptance clause is a finding, not a failure: record it precisely.
- Your branch gets an adversarial Opus 5 review before integration: every clause you claim proved needs evidence in your diff that asserts the PINNED behaviour, and every new fixture case needs a demonstration that it FAILS when it should (a sabotage). A fixture that only passes has proved nothing.
- When done (or stopping): push campaign/BRANCHNAME (never force, never main), report through structured output with the final tip sha in notes. Stopping early: push what runs as campaign/BRANCHNAME-wip and say what never ran.
- NO GitHub comments, NO issue 7, NO compat/board.py mutations. The orchestrator speaks for you.
`

const BATCH_KEYS = COMMON + `
YOUR BATCH: TUI-003, stock launcher, command and key behavior. BRANCHNAME: tui-stock-keys. WORKDIR: zz-tui-keys.
Your zones: compat/tui/ (the TUI-003 record and compat/tui/evidence/TUI-003/attempt-01/), new fixtures under compat/ that copy the outer-tmux driver (name every file in notes) and compat/scenarios/smoke/fixtures/ helpers; crates/zz/src/ (the launcher and CLI), crates/zz/tests/, crates/zz-tui/src/input.rs, crates/zz-client/src/chrome.rs, crates/zz-protocol/src/key.rs (binding tables and key vocabulary only, no wire messages), and crates/zz-daemon/src/ strictly bounded to key-table and binding routing. state.rs and render.rs belong to other lanes; do not touch them.
Read the TUI-003 record in compat/tui/campaign.json first: its acceptance IS your contract, its evidence_note carries the source inspection you start from (bare launcher new-session -A, stock bindings using native picker/sidebar commands, chrome shortcuts before daemon routing).
1. Clause 1, launch and command surface. HARD BUDGET 150 MINUTES. With clean config roots on BOTH sides (scratch HOME + XDG_CONFIG_HOME, -f /dev/null where a file is explicit), compare bare packaged launch, attach and new-session on empty and live servers, plus new-window and split-window: exact stdout/stderr/exit, selected targets, pane kinds, and the COMPLETE attached screen through the tui-screen-diff driver shape. Where zz's bare launch behaves as new-session -A and the pin's does not (or vice versa), that is a measured divergence to fix in crates/zz/src or record with its exact bytes — measure first.
2. Clause 2, stock bindings through actual stdin. HARD BUDGET 150 MINUTES. Drive unmodified stock split (prefix %, \"), chooser (prefix s, w), rename (prefix , and $), selection (prefix o, arrows), zoom (prefix z) and detach (prefix d) through send-keys into the ATTACHED client on both sides and compare the resulting screens and session state. Direct split-window creating a terminal does not substitute for its stock binding. A stock binding that reaches a native zz surface (picker, sidebar) where the pin shows its own surface is the divergence this obligation exists to fix: the stock binding must produce the pinned result; zz surfaces stay reachable through their own commands and user bindings ("${DECIDED}").
3. Clause 3, key ownership and config roots. HARD BUDGET 120 MINUTES. Root bindings and application keys overlapping Ctrl-\\ and Alt-s/Alt-S: bind them to observable actions on both sides and prove chrome does not consume them outside its owning context (crates/zz-client/src/chrome.rs routes before the daemon today; if that ordering eats a bound key, fixing the order in your zones is the work). Exercise explicit config roots, -f and existing zz layering as named cases; the -f stance (zz/mux.conf layers last, fabrico 2026-09-05) is recorded and NOT yours to change — a case that contradicts it is recorded against that decision, not resolved.
Ledger: status review only when every clause you claim has evidence at the tip; evidence_note rewritten as the measurement with the 80/100-column screen comparisons named; next_action for the gate.
Proofs at tip: your new fixture(s) exit 0 plus their sabotage runs, ${RUN_ENV} compat/tui-screen-diff.sh and compat/tui-pane-geometry.sh with your zz and the pin (regressions), cargo test -p zz-tui -p zz-client, clippy on every touched crate, the integration-test rule, python3 compat/tui/tracker.py check.
`

const BATCH_CANVAS = COMMON + `
YOUR BATCH: TUI-004, default canvas, status and pane borders — and the sidebar decision. BRANCHNAME: tui-canvas-sidebar. WORKDIR: zz-tui-canvas.
Your zones: compat/tui/ (the TUI-004 record and compat/tui/evidence/TUI-004/attempt-01/), compat/tui-screen-diff.sh, compat/status-row.sh and compat/tui-pane-geometry.sh (flipping record modes to asserted as behaviour converges is yours), crates/zz-tui/src/ (sidebar.rs, layout.rs, render.rs; state.rs is SHARED with the flow lane — keep any edit there minimal and confined to sidebar visibility fields), crates/zz-daemon/src/status.rs, crates/zz/tests/. THEME ARM, LAST AND BUDGET-GATED ONLY: crates/zz-mux/src/command.rs (TMUX_OPTION_CONSUMERS), the compat manifest test, one pure-append field on StatusLine in crates/zz-protocol with PROTOCOL_VERSION 99 to 100, and the client consumption. Registry ownership: the TUI portion of tui.sidebar-auto-hide, and, only if the theme arm lands, presentation:tui-status-row-theme-colours-per-client inside tui.status-row.
1. THE SIDEBAR DECISION. HARD BUDGET 150 MINUTES. PRODUCT DECISION handed to this lane by fabrico's contract (knowledge/designs/tui-parity.md): width NEVER invokes the sidebar — remove the automatic threshold (AUTO_HIDE_COLUMNS = 109 arithmetic in crates/zz-tui/src/sidebar.rs and everything that keys presentation off terminal width). The sidebar appears only through its explicit command (focus-sidebar) or a user binding, and its visibility is client-local. Record it with "${DECIDED}" and update the TUI portion of tui.sidebar-auto-hide in compat/tmux-gaps.json (the contract supersedes that accepted decision for terminal clients; retain the GUI scope). Proof: at 109x24 and 120x24 the raw TUI's screen equals the pin's cell for cell — flip compat/tui-screen-diff.sh's 109 and 120 sizes from record to same and compat/tui-pane-geometry.sh's 120 columns from record to asserted, and prove focus-sidebar still shows the sidebar (and withdrawing it restores the pinned canvas, which is TUI-012's later contract — one case here as a regression guard). Expect crates/zz/tests assertions about the 109 threshold to need updating; that is in zone.
2. Clause 1 + clause 2, the canvas matrix. HARD BUDGET 180 MINUTES. Ordinary tmux commands preserve the pinned canvas at narrow and wide dimensions: extend compat/tui-screen-diff.sh with the cases the acceptance names that it lacks — pane-border-status (top and bottom), small heights beyond 80x10 where cheap, resize restoration across the sidebar-relevant widths now asserted, and the full status matrix (default/custom formats, top/bottom/off/multiple rows already exist; add named/indexed/RGB colour cases on status-style and formats, and the terminal light/dark report cases if the pin exposes them — measure format.c/tty.c for the pin's side first). Every new case: driven identically on both sides, settled checkpoint, sabotage in --self-check where it adds a channel.
3. CURSOR ATTRIBUTES. HARD BUDGET 60 MINUTES. TUI-002 recorded zz writing DECSCUSR and OSC 12 on every cursor placement (crates/zz-tui/src/render.rs:1770-1799) where the pin writes neither; the raw TUI must match the pin's default (write neither unless the application or configuration asks). Fix it in render.rs, then flip tui-screen-diff.sh's cursor shape/blink/colour channel from recorded to asserted and let --self-check sabotage it. If the fix ripples wider than render.rs, stop, record, leave the channel recorded.
4. THEME ARM, only if every budget above held and at least 120 minutes remain. The recorded shape (cycle 18's handoff): parse_format_option in crates/zz-mux/src/command.rs gates by-name reads on TMUX_OPTION_CONSUMERS, which lacks the eleven dark-theme-*/light-theme-*/theme names (manifest count 118 to 129 in compat_manifest_tests.rs); then a TmuxColour field (not RGB — the pin keeps colour124 indexed) on StatusLine, protocol 99 to 100 as pure appends; then the raw TUI honours a user-set dark-theme-green. Measure first that the field is needed as recorded; prove with a status-row or screen-diff case; if it does not fit, record the measurement and leave the item open — a half-landed wire bump is a reviewer blocker.
Ledger: TUI-004 to review only when clauses 1 and 2 have whole-screen evidence and clause 3's covered subset is stated in evidence_note with what remains; the theme arm's outcome recorded either way.
Proofs at tip: ${RUN_ENV} compat/tui-screen-diff.sh (with your flipped sizes) twice plus --self-check, compat/tui-pane-geometry.sh three times, compat/status-row.sh (under LC_ALL=C LC_TIME=C on this box, per the box note), compat/attached-client.sh once, cargo test -p zz-tui (and -p zz-mux -p zz-protocol if the theme arm landed), clippy on every touched crate, the integration-test rule, both trackers' check.
`

const BATCH_FLOW = COMMON + `
YOUR BATCH: TUI-010, slow output, recovery and client lifecycle. BRANCHNAME: tui-client-lifecycle. WORKDIR: zz-tui-flow.
Your zones: compat/tui/ (the TUI-010 record and compat/tui/evidence/TUI-010/attempt-01/), compat/attached-client.sh (extending its lifecycle cases is yours) and a new backpressure fixture under compat/ if the smoke scenario shape does not fit (name it in notes), compat/scenarios/smoke/fixtures/; crates/zz-tui/src/ (writer.rs, tty.rs, app.rs; state.rs is SHARED with the canvas lane — keep any edit there minimal and confined to your lifecycle fields), crates/zz/tests/. Registry ownership: tui.client-output-queue-budget (you may close it only with the full proof below).
Read TUI-010's record and the tui.client-output-queue-budget group in compat/tmux-gaps.json first: the registered reproduction and the reference numbers live there (past the 4 MiB budget the paint loop parks; the pin's tty_block_maybe DROPS output and answers a bound key in 0.050s after 45s of a terminal reading nothing; zz does not answer within 12s). The recorded direction is a drop-and-redraw path, not a bigger budget.
1. Clause 1, prolonged backpressure. HARD BUDGET 180 MINUTES. MEASURE FIRST with the registered reproduction on both sides at this tip and write the numbers down (time to answer a bound key while undrained, memory bound, what the pin drops and when — read tty.c tty_block_maybe in the pinned source for the reference behaviour). Then implement the drop-and-redraw path in your zones: past the budget, drop queued output, mark the screen dirty, answer input within a declared reference-derived deadline (state the deadline and its derivation in the fixture header and evidence), and repaint from current state when the terminal drains. Prove it with a fixture that drives real prolonged backpressure (an attached client whose outer pty stops reading), delivers a bound key while undrained, and asserts the response bound and bounded memory; sabotage: with the budget disabled or the deadline halved the fixture must go red. Keep smoke/tui-client-input-backpressure green (${RUN_ENV} compat/run.sh --strict-geometry smoke/tui-client-input-backpressure). Close tui.client-output-queue-budget in compat/tmux-gaps.json only on this full proof.
2. Clause 2, resume and detach under backlog. HARD BUDGET 90 MINUTES. Resume reading after the drop and compare the FINAL screen against the pin cell for cell (the screen-diff driver shape). Detach under backlog and prove no queued paint appears after terminal restoration (capture the outer terminal after detach; a late escape sequence is the failure).
3. Clause 3, client lifecycle. HARD BUDGET 120 MINUTES. Different-sized simultaneous terminal clients (the pin sizes to the smallest; measure and match or record), read-only attach (-r: input refused, screen live), detach/reattach cycles, and supported transport recovery. Compare selected targets, geometry, focus and final screen at settled checkpoints. Extend compat/attached-client.sh or your fixture; every case sabotaged once.
Ledger: TUI-010 to review only with the clause-1 numbers in evidence_note (before and after, both sides); blocked with the complete diagnosis if the drop path needs anything outside your zones.
Proofs at tip: your fixture exit 0 plus sabotages red, ${RUN_ENV} compat/attached-client.sh, compat/tui-screen-diff.sh and compat/tui-pane-geometry.sh (regressions), smoke/tui-client-input-backpressure, cargo test -p zz-tui -p zz-terminal, clippy on touched crates, the integration-test rule, both trackers' check (you touch tmux-gaps.json).
`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz TUI parity campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit ${M.root} itself. Other lanes and reviews may run beside you; keep to the parallelism caps.
HOW YOU REPORT: your final act is the structured report the output schema describes (lane, verdict, confirmed_defects, checks_run, notes). FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long batches); never use Monitor, run_in_background or any background task; never end your turn to wait. A reviewer that ends its turn without the report is a failed agent and the gate audits the lane itself.
SETUP: GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (${M.gitNote}). Resolve the tip: git -C ${M.root} rev-parse origin/BRANCH. Scratch worktree at that tip: git -C ${M.root} worktree add ${M.dev}/REVIEWDIR <tip> (if REVIEWDIR exists and is clean, checkout --detach the tip there instead; if it is dirty, use REVIEWDIR-2). For every cargo command export CARGO_TARGET_DIR=WORKERTARGET, the worker's warm target directory (the worker is finished, its tree is clean at the same tip; the OTHER lanes' targets are NOT yours); touch crates/**/*.rs and Cargo.* in your worktree first so the shared directory rebuilds your tree, never cargo clean it, and rebuild before you spawn any binary from it. The shared checkout is ${M.root}; read it, never edit it. ${M.boxNote}
MACHINE ETIQUETTE: cargo test -p <pkg> --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} only, no workspace-scale anything, cargo output to a log file + check exit code. Throwaway pin servers -L zzprobe-$$ -f /dev/null only, kill after; never kill servers you did not start (${M.protected}), never pkill tmux or zz; never a bare tmux or zz command without -L.
METHOD, in order of value:
1. CONTRACT AUDIT: for every obligation the worker moved to review (worker report + git diff origin/main...HEAD -- compat/tui/campaign.json), take each acceptance clause it claims and find the evidence on the branch that proves it against the PIN. A fixture that only passes is not evidence: the sabotage runs must exist and must fail for the right reason. A clause claimed from a file that describes a run rather than records one is a blocker. The worker may have written status, evidence_note and next_action of ITS obligation only; a proof block, a verified status, an edit to TUI-001's or TUI-002's verified records, to another lane's obligation, to the baseline, the milestones or the pin is a must-fix or blocker. In compat/tmux-gaps.json only the records this lane OWNS may change (its batch prompt names them); anything else is a blocker. ${OWNERSHIP}
2. PROOFS AT TIP: run the worker's claimed proof commands yourself at the branch tip with a zz you built in your worktree, with the pin ${PIN}. If the diff touches crates: the touched crates' tests and timeout 2400 cargo test -p zz --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads}. Any red at tip is a blocker regardless of the report (load-flake rule applies; the known list is in the batch prompt below).
3. ORACLE SPOT-CHECKS on the pin yourself: for every behaviour the lane changed, drive the PIN in a throwaway server and confirm the branch's fixture asserts what the pin actually does (read the pinned C where the batch prompt names it). For every PRODUCT DECISION the lane recorded, confirm the recorded sentence, the old behaviour and the measured pin behaviour are all present and true. Check every wait in new fixture code is a bounded wait_for; grep for sleep.
4. TEST HONESTY: run every new fixture and its sabotages yourself; ADD one sabotage of your own on a throwaway copy and confirm the fixture reports it with a location; confirm evidence files are real captures from runs (sizes, contents), no .log files committed, git check-ignore -v flags nothing under the evidence directory, environment.txt matches a build you can reproduce from the recorded revision (hash identifies, revision attests).
5. INVARIANTS: zone discipline (git diff --stat origin/main...HEAD against the lane's declared zones; anything outside is a blocker unless notes declare it and the batch prompt allows it); the wire rule (PROTOCOL_VERSION moves only on the canvas lane's theme arm, pure appends, manifest updated — anything else touching the wire is a blocker); no code comments outside fixture headers; no attribution trailers; both trackers' check green on the branch; generated reports regenerated, not hand-edited (regenerate and diff).
6. CALIBRATION: default to refuting each claim, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = a claimed clause without real evidence, a fixture that cannot fail, a zone or wire violation, or something that would break main; must-fix = gate applies before merge; nit = mention. The load-flake rule binds you too.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes; a rejected lane gets ONE fix pass by a fresh agent and a re-review, so make every blocker precise: the probe, the expected bytes, the file). checks_run lists exact commands. Thorough but bounded: well under 90 minutes after the build.
`

const REVIEW_KEYS = `LANE-SPECIFIC SPOT-CHECKS (keys): drive every stock binding the lane claims through send-keys yourself on both sides and compare the screens; confirm the chrome-ownership cases with a root binding of your own choosing on Ctrl-\\ and Alt-s; confirm the -f cases record rather than reopen the accepted zz/mux.conf stance; read every hunk under crates/zz-daemon and crates/zz-protocol/src/key.rs — binding tables only, no wire shapes.`
const REVIEW_CANVAS = `LANE-SPECIFIC SPOT-CHECKS (canvas): confirm the sidebar never appears by width alone at 109/119/120/200 columns (drive an attach at each), that focus-sidebar still shows it, and that the flipped fixture sizes really assert (sabotage one: force the sidebar on in a throwaway build or config and confirm tui-screen-diff.sh goes red at 120); confirm tui.sidebar-auto-hide's TUI portion update retains the GUI scope; if the cursor channel flipped, print \\e[2 q from the inner shell on both sides and confirm the fixture still passes (application-requested shape is not the default channel); if the theme arm landed, confirm the manifest count moved 118 to 129, the protocol bump is pure appends with PROTOCOL_VERSION 100, and a user-set dark-theme-green reaches the raw TUI's status row against the pin's; if it did not land, confirm nothing under crates/zz-mux or crates/zz-protocol changed.`
const REVIEW_FLOW = `LANE-SPECIFIC SPOT-CHECKS (flow): reproduce the backpressure measurement yourself from the fixture (stopped outer pty, bound key, the declared deadline) on both binaries and check the numbers in evidence_note against your run; confirm the deadline's derivation from the pin's tty.c is stated and sane; run the detach-under-backlog case and inspect the captured outer terminal bytes yourself; confirm tui.client-output-queue-budget's close (if claimed) cites the fixture and the numbers; read every hunk in writer.rs, tty.rs and app.rs for the drop path — dropped DATA must never mean dropped STATE (the repaint must come from current grid state, not replayed bytes).`

const LANES = [
  { key: 'keys', prompt: BATCH_KEYS, lock: 'F-TUI-CYCLE-3-LANES', workdir: 'zz-tui-keys', reviewdir: 'zz-tui-keys-review', target: `${M.dev}/zz-tui-keys/target`, extra: REVIEW_KEYS },
  { key: 'canvas', prompt: BATCH_CANVAS, lock: 'F-TUI-CYCLE-3-LANES', workdir: 'zz-tui-canvas', reviewdir: 'zz-tui-canvas-review', target: `${M.dev}/zz-tui-canvas/target`, extra: REVIEW_CANVAS },
  { key: 'flow', prompt: BATCH_FLOW, lock: 'F-TUI-CYCLE-3-LANES', workdir: 'zz-tui-flow', reviewdir: 'zz-tui-flow-review', target: `${M.dev}/zz-tui-flow/target`, extra: REVIEW_FLOW },
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

log('TUI cycle 3: keys (TUI-003), canvas (TUI-004 + the sidebar decision), flow (TUI-010); Opus 5 at xhigh throughout, one reviewer per lane, one fix pass on a reject, one serialized gate')
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
REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on that branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge that branch; post the blockers as a board note on its lock front, push the rebased tip as campaign/<name>-gated, leave the branch, and continue with the other lanes. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a clause claimed without evidence is such a fix (drop the claim, set the status honestly, put the measurement in evidence_note). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating that lane.
BOARD IDENTITY: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (always give a unit); withdraw and front need TRIAGE held; integrated needs MAIN held. The orchestrator holds the single lock front F-TUI-CYCLE-3-LANES covering all three lanes (the board's zone model conflicts sibling raw-tui locks; expired => claim it back as ${M.holder} before the ledger step). BOARD FALLBACK: if a board command fails on GitHub authentication, append the exact command line to ${M.root}/compat/tui/board-replay-3.sh (do not commit that file), say so in board_updates, and continue; the orchestrator replays the file. Never let a board failure stop a merge.
${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. The shared checkout ${M.root} is where you start; its local main branch is stale; never use it, never edit it, never stash or reset in it. Every commit is git -c commit.gpgsign=false commit.
knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with the trackers' write-report on every conflict, never hand-merge them. compat/tui/campaign.json and compat/tmux-gaps.json merge by record id: on a conflict, take each lane's changes to the records it OWNS and nothing else.
THIS BOX: ${M.boxNote} Use ONE build directory for the gate worktree, export CARGO_TARGET_DIR=${M.dev}/zz-gate-target in every shell that runs cargo, compat/check.sh included (it is warm from cycle 1; leave it in place). Before each branch's gate, run git merge-tree --write-tree <current base> <tip>; it predicts the conflicts and costs one command.
ORDER: gate the branches in this order where present: canvas (its fixture-mode flips define the screen contract the others are judged by), then keys, then flow. Later branches rebase onto the accumulating result.
STAGES, per branch, in the gate worktree ${M.root}-gate-tui3 (create once: git -C ${M.root} worktree add ${M.root}-gate-tui3 origin/main; remove a leftover with --force first). Claim MAIN --lease 6h before the first branch; renew before any long stage; release after the records push.
1. Rebase the branch onto the current gate HEAD (origin/main for the first, the accumulated result after). A conflict inside files the lane does not own is a skip (push campaign/<name>-gated, record, continue).
2. ALWAYS for this cycle (every lane touches crates/): timeout 2400 cargo test --workspace --all-features --no-fail-fast --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (check the exit code; never pipe through tail; a wedge over 20 minutes with no output is sampled, not waited out) and cargo clippy --workspace --all-targets --all-features --jobs ${M.gateJobs} -- -D warnings, then ${RUN_ENV} compat/run.sh --strict-geometry --delta <base>..HEAD --commands <that lane's touched_commands> plus every keys and status scenario and smoke/tui-client-input-backpressure. Flake rule: fails loaded + passes exact-solo = flake (the known list is in the batch prompts); anything else red is real: fix if minutes, else SKIP the branch (no merge; push the gated tip as campaign/<name>-gated), record, continue with the next branch from the last good HEAD.
3. Fixtures at the accumulated tip after each merge: build zz (cargo build -p zz --jobs ${M.gateJobs}) and run with the pin ${PIN}: ${RUN_ENV} compat/tui-screen-diff.sh and its --self-check, compat/tui-pane-geometry.sh three times, compat/status-row.sh (LC_ALL=C LC_TIME=C on this box, plus one run under the box locale to confirm the %b record still stands), compat/attached-client.sh once; retain every exit code and the last lines for attached_client. If the canvas branch merged, the 109/120 sizes now assert: a red there after a later merge is that later branch's regression. Then python3 -B compat/tui/tracker_test.py, python3 compat/tui/tracker.py check, python3 compat/tmux-tracker.py check, and compat/check.sh. A fixture exit that contradicts a lane's claimed status is a defect: that obligation stays out of verified and the records commit says why.
4. RECORDS, after the last branch, one commit at the final tip: for each obligation whose lane merged with verdict approve or approve-with-fixes (fixes applied), re-run that obligation's proof commands at THIS tip (stage 3 already did; cite those runs), write compat/tui/evidence/<ID>/attempt-01/review.md carrying the reviewer's verdict JSON verbatim, its checks_run, and your review_actions, and fill the proof block: revision = the final pre-records tip, tmux_commit = the campaign pin, environment = one line plus a pointer to environment.txt, commands = the exact invocations with exit codes, artifacts = the evidence files, review = the review.md path; set status verified and next_action to what its closure unlocks (after TUI-003 and TUI-004: TUI-005, TUI-006, TUI-007, TUI-009 become ready; TUI-010 verified removes the last blocker TUI-012 has besides TUI-008/TUI-009). An obligation the reviewer rejected, that was skipped, or that stage 3 contradicted stays at the lane's honest status with the reason appended to evidence_note. If PROTOCOL_VERSION moved to 100, say so in problems with the fields it carries. Then python3 compat/tui/tracker.py write-report, python3 compat/tmux-tracker.py write-report (gaps changed this cycle), both checks, commit.
5. Push main (git push origin HEAD:main). Non-fast-forward: fetch; user-authored commits with a conflict-free disjoint rebase => bounded rerun (stage 3's fixtures at the new tip), push. Never force. Campaign branches stay at their old tips on origin (never force them); say so in the report.
6. Ledger against the single lock front F-TUI-CYCLE-3-LANES, once, after all branches: one candidate per merged branch posted on that front (--commit <its rebased tip> --branch <its campaign branch> --base <pre-push main> --proof per stage), then one note covering every lane (--note: obligations verified, left open and why, each reviewer verdict and what you did about each defect; a skipped lane's blockers included), one integrated (--merge <the pushed sha> --gate "workspace tests+clippy+delta corpus+fixtures green"), one release (--reason "cycle 3 integrated at <sha>"). Release MAIN.
7. Claim TRIAGE. Post one residual listing what this cycle leaves: obligations left open with why, the divergences still without registry owners (pane-body RGB promotion and the explicit default foreground, unless a lane closed them; the %b locale item; cursor attributes if the canvas lane left them recorded), the theme arm's outcome, anything skipped, each with the obligation id. Release TRIAGE.
8. python3 compat/tui/tracker.py ready and the report headline (the Fixed baseline line of knowledge/tmux/tui-parity.md) at the pushed main, into progress_after.
9. Remove only your own ${M.root}-gate-tui3 worktree (after pushing any skipped lane's gated tip). Leave the lane and review worktrees in place. Sweep pgrep -fl zzprobe and reap only pids whose command line names a socket the gate created.
Never stash/reset anything in ${M.root}. Never kill tmux or zz servers you did not start (${M.protected}). Report per branch merged/sha/gate_summary/review_actions/flakes, the progress lines, board records, the fixture outcomes, problems.`

const gate = summaries.length
  ? await agent(gatePrompt, { label: 'gate:serial', phase: 'Integrate', schema: GATE_SCHEMA, ...OPTS })
  : null
if (!gate) log('No branch to integrate: every worker came back empty.')
return { lanes: laneResults, gate }
