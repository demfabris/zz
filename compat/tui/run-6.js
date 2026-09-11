export const meta = {
  name: 'tui-run-6',
  description: 'TUI parity cycle 6: five Opus 5 continuation lanes at xhigh, at most two agents working at once, working the punch lists cycle 5 left (modes TUI-004, copy TUI-005, caps TUI-009, overlays TUI-007, choosers TUI-006), one adversarial reviewer per lane with one bounded fix pass, then one gate agent per branch in order, each pushing main when green',
  phases: [
    { title: 'Work', detail: 'five worktrees, each starting where cycle 5 stopped; two lanes at a time' },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh per lane, adversarial, pipelined behind its worker; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: 'one Opus 5 gate agent per branch, in the order modes, copy, caps, overlays, choosers; each rebases, tests, flips its sibling cases, writes its records, pushes main' },
  ],
}

const A = args || {}
const M = {
  root: A.root || '/home/demfabris/dev/zz',
  dev: A.dev || '/home/demfabris/dev',
  holder: A.holder || 'alienware/orchestrator',
  machine: A.machine || '16-core, 15 GB (plus 15 GB zram) CachyOS Linux box (alienware)',
  workerJobs: A.workerJobs || 4,
  workerThreads: A.workerThreads || 3,
  gateJobs: A.gateJobs || 6,
  gateThreads: A.gateThreads || 4,
  date: A.date || '2026-09-11',
  base: A.base || 'origin/campaign/tui-cycle5-gated',
  protected: A.protected || "nothing at launch: no zz daemon or tmux server of the user's was running on this box when cycle 6 started; if one appears mid-run it is the user's. Never touch a server on the default sockets (/run/user/1000/zz/default.sock, /tmp/tmux-1000/default) that you did not start",
  boxNote: A.boxNote || "This box (alienware) is LINUX: CachyOS (Arch-based), 16 cores, 15 GB RAM plus 15 GB zram swap; /bin/bash is 5.x; the filesystem is btrfs; /opt/homebrew does not exist, so the PATH=/opt/homebrew/bin:$PATH prefix this prompt carries is harmless. Where a prompt says sw_vers, use uname -a plus head -2 /etc/os-release for the OS line. The zz daemon ring log lands under the scrubbed HOME at .local/state/zz/logs/. The compat caches are populated and cycles 1, 3, 4 and 5 ran here: every TUI fixture is known green at origin/main EXCEPT compat/status-row.sh under the box locale (LC_TIME=pt_BR.UTF-8 changes %b; the LC_ALL=C LC_TIME=C control exits 0), and four corpus rows are environmental here (micro-flags, show-options-hooks, lane2-store, smoke/plugin-runtime-resurrect-restore). Slow corpus rows that pass alone: command-item-format (up to 8 minutes), command-prompt-editing (about 4 minutes), copy-mode-stock-action-keys. A red row or fixture at your tip is yours only if it is green at origin/main on this box. Known fixture fragility: compat/tui-stock-keys.sh root-binding-detaches can flake on a wall-clock second boundary and its recorded count wobbles; tui-screen-diff.sh --self-check exited 2 once ('zz refused status-right') and passed solo. zz-daemon client_focus_closes_display_panes_and_preserves_chooser_modes fails about 1 run in 10 even exact-solo at BASE: the modes lane owns it this cycle, so elsewhere a single red there reruns and is named as this known flake, not chased. A single Bash call is capped at 600 seconds and a longer command is moved to the background and killed: pass timeout of at most 590000 and split long work. A cargo debug build of zz is NOT bit-reproducible here: a hash identifies an artifact, the revision plus a clean worktree attests it. The box has a real ~/.tmux.conf and ~/.config/zz/mux.conf: never read or edit them and never start a server that would load them (scrub HOME and XDG_CONFIG_HOME, both; probes use -f /dev/null). Never write 'rm -rf $HOME' or 'rm -rf ~'; put a scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz). HYGIENE, measured in cycle 5: agents left about 5 GB of zz binary copies under /tmp, which is a RAM-backed tmpfs, and filled the zram swap; a cargo test killed by a timeout leaves /tmp/zz-cli-*/ daemons running for hours; a scrubbed environment without XDG_RUNTIME_DIR autostarts a daemon on /tmp/zz-user/default.sock. So: copy a binary under /tmp only when a comparison needs a frozen copy, delete every copy before your final report, and at the end run pgrep -fa 'zz-cli-|zz-user|zzprobe' and reap only pids whose command line or environment names a socket or scratch HOME you created. After switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first, and rebuild before spawning a binary. Every git commit is 'git -c commit.gpgsign=false commit'. MEMORY, measured 2026-09-11: the first run of this cycle drove the box out of memory about twenty minutes in, with five lanes compiling and testing at once (four rustc, a 1.5 GB zz-daemon test binary, three cargo), and the desktop stayed unusable for hours. So EVERY cargo command on this box goes through a shared two-slot lock and a per-command memory cap, exactly like this: S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=CAP -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo <args> (CAP is 5G for lanes and reviewers, 7G for a gate). fabrico's rule for this box since the OOM: at most two agents work at a time, and the runner enforces it; the two lock slots keep at most two cargo commands running even so. A command that waits more than 540 seconds for a slot exits 1 without running: run it again. A command that hits its cap is killed with exit 137 instead of taking the box down: run it again with fewer --jobs or --test-threads. Neither is a test result. Fixtures and probes need no wrapper, but run one at a time per lane.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz) through the gh credential helper; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands.',
}
log(`TUI cycle 6 on ${M.machine}; five continuation lanes, two agents at a time, --jobs ${M.workerJobs}; one gate agent per branch at --jobs ${M.gateJobs}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`
const ORDER = ['modes', 'copy', 'caps', 'overlays', 'choosers']
const LOCK = 'F-TUI-CYCLE-6-LANES'
const OWNERSHIP = `LEDGER OWNERSHIP THIS CYCLE (compat/tui/campaign.json records; edit only yours): modes lane = TUI-004 (evidence attempt-04); copy lane = TUI-005 (attempt-03); caps lane = TUI-009 (attempt-03); overlays lane = TUI-007 (attempt-02); choosers lane = TUI-006 (attempt-02). Earlier attempts are read-only history. TUI-001, TUI-002, TUI-003, TUI-010 are verified and nobody's; TUI-008, TUI-011, TUI-012, TUI-013, the baseline, the milestones and the pin are nobody's. In compat/tmux-gaps.json a lane may close ONLY the items its own landing makes the raw TUI honour, inside the gaps its batch names, each with a dated measurement (TMUX_OPTION_CONSUMERS and the compat_manifest_tests.rs partition move in the same commit); an accepted gap keeps its decision for the GUI and for every item not closed.`

const WORKER_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['branch', 'obligations', 'sibling_cases', 'touched_commands', 'touched_packages', 'notes'],
  properties: {
    branch: { type: 'string', description: 'campaign/* branch pushed to origin, or empty string if nothing pushed' },
    obligations: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['id', 'status_after', 'clauses_proved', 'clauses_open', 'proofs', 'evidence_dir'], properties: {
      id: { type: 'string' },
      status_after: { type: 'string', enum: ['unmeasured', 'different', 'active', 'review', 'blocked'] },
      clauses_proved: { type: 'array', items: { type: 'string' } },
      clauses_open: { type: 'array', items: { type: 'string' }, description: 'each with why; a clause holding any recorded case is open' },
      proofs: { type: 'array', items: { type: 'string' }, description: 'exact commands run AT THE FINAL TIP with exit codes' },
      evidence_dir: { type: 'string' },
    } } },
    sibling_cases: { type: 'array', items: { type: 'string' }, description: '"<fixture>:<case> -> <lane key>: <what the sibling must land>", or empty' },
    touched_commands: { type: 'array', items: { type: 'string' } },
    touched_packages: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string', description: 'zone excursions, every render.rs hunk by function, wire appends, decisions recorded, gap items closed, punch-list items done and not done, the final tip sha' },
  },
}

const REVIEW_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['lane', 'verdict', 'confirmed_defects', 'checks_run', 'every_clause_asserted', 'notes'],
  properties: {
    lane: { type: 'string' },
    verdict: { type: 'string', enum: ['approve', 'approve-with-fixes', 'reject'] },
    confirmed_defects: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['obligation', 'severity', 'description', 'suggested_fix'], properties: {
      obligation: { type: 'string' }, severity: { type: 'string', enum: ['blocker', 'must-fix', 'nit'] },
      description: { type: 'string' }, suggested_fix: { type: 'string' },
    } } },
    checks_run: { type: 'array', items: { type: 'string' } },
    every_clause_asserted: { type: 'string' },
    notes: { type: 'string' },
  },
}

const GATE_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['branch', 'merged', 'pushed_sha', 'verified', 'sibling_flips', 'gate_summary', 'review_actions', 'flakes', 'fixtures', 'board_updates', 'problems'],
  properties: {
    branch: { type: 'string' },
    merged: { type: 'boolean' },
    pushed_sha: { type: 'string', description: 'origin/main after this gate, or empty if nothing pushed' },
    verified: { type: 'array', items: { type: 'string' }, description: 'obligation ids this gate set verified' },
    sibling_flips: { type: 'string' },
    gate_summary: { type: 'string' },
    review_actions: { type: 'string' },
    flakes: { type: 'string' },
    fixtures: { type: 'string', description: 'exit code and last line of every fixture at the pushed tip' },
    board_updates: { type: 'string' },
    problems: { type: 'string' },
  },
}

const COMMON = `You are an autonomous worker on the zz TUI parity campaign (repo demfabris/zz). ONE OTHER LANE or gate runs beside you on this ${M.machine} (the runner keeps at most two agents working at once, fabrico's limit for this box), and the user may code here too, so stay inside your zones and your parallelism caps. Rules that are not negotiable:

WHAT THIS CYCLE IS FOR
- Two cycles have ended with obligations one or two items short of verified. This cycle is a PUNCH LIST: your batch names exactly what cycle 5's reviewers and gate left open on your obligation, with the measurements and the fix directions. Do those items, in order, and nothing else first. An obligation counts only when every clause asserts; your lane is judged by whether its obligation can be set verified at its gate.
- THE CONTRACT DECIDES THE TUI PORTION OF ACCEPTED NATIVE GAPS (triage 2026-09-10, knowledge/designs/tui-parity.md): the raw TUI renders the pin's cells; the GUI keeps its native presentation. A case recorded only because of such a gap is a divergence to fix. Record a product decision in evidence_note with the old behaviour, the pin's measured behaviour and the sentence "${DECIDED}".
- FOUND SOMETHING NEW? If you find a divergence inside your obligation that no punch-list item names, fix it if it fits your zones and budget; if it does not, record it as a fixture case with its measurement and leave the obligation at active. Do not hide it and do not stop the punch list for it.

HOW YOU REPORT
- Your final act is the structured report the output schema describes. A worker that ends its turn without it is a failed agent and its lane is dropped.
- FOREGROUND ONLY: every command in the foreground, timeout at most 590000; never Monitor, run_in_background or any background task; never end your turn to wait.
- Check 'date' at the start of each item. The batch's HARD BUDGET is a ceiling. When it is spent, stop, write what asserts now and what remains into evidence_note, set the status honestly, push and report.

SETUP
- The shared checkout is ${M.root}. Read it and add worktrees from it; NEVER edit, stash, reset or clean it. Its local main branch is stale: use origin/main. knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report, never hand-merge them.
- ${M.gitNote} Fetch with GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME (never force, never main).
- BASE THIS CYCLE is ${M.base} (5d9bf198): cycle 5's modes and copy landings, gated green on tests, clippy, the delta corpus and every TUI fixture, but NOT yet on main, because compat/attached-client.sh fails at it in one step: its copy-mode wait still expects zz's old COPY status badge, which the modes landing replaced with the pin's in-pane position indicator. The modes lane fixes that first, and the modes gate lands BASE on main together with its own work. Until then origin/main (3319ceba) lacks BASE: build on BASE, never on origin/main, and diff your work against BASE.
- Worktree: your WORKDIR under ${M.dev} was pre-positioned by the orchestrator at your batch's START commit, clean, with a warm target. Confirm with git log -1 and git status --short; RESUMING AFTER THE OOM: the first run of this batch was killed about twenty minutes in, and its work is in your WORKDIR (commits after START plus uncommitted edits). Start with git status, git log --oneline START..HEAD and git diff: keep what serves your punch list, finish or redo what was cut off mid-edit (an unfinished rebase, a half-written hunk), and continue from there. Never discard it blindly and never switch to a second worktree. If your START is not BASE, rebase onto BASE FIRST (git rebase ${M.base}; resolve conflicts by keeping both sides' intent, regenerate generated reports, rebuild, and run your obligation's fixture once before any new work). Build through the wrapper (CAP 5G): S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=5G -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo build -p zz --jobs ${M.workerJobs} > build.log 2>&1, check the exit code.
- compat/attached-client.sh at BASE stops at 'zz screen did not visibly become copy-mode'. That one red is known and the modes lane's to fix; every other lane runs it and reports anything past that step.
- ${M.boxNote}

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8) at ${PIN}; its source tree is beside it. Probe with throwaway servers only (-L zzprobe-$$ -f /dev/null), kill them when done. Never kill a tmux or zz server you did not start (${M.protected}); never pkill/killall tmux or zz.
- Proof surfaces: compat/tui-screen-diff.sh, tui-pane-geometry.sh, status-row.sh, attached-client.sh, tui-stock-keys.sh, tui-indicators.sh, tui-copy-mode.sh, tui-caps.sh, and cycle 5's tui-overlays.sh and tui-choosers.sh. Read a fixture end to end before you extend it. New cases copy the driver shape (outer pinned tmux, isolated HOME and XDG_CONFIG_HOME per side, short /tmp sockets, bounded wait_for on an observable, settle = marker on screen AND unchanged between two polls, a trap that reaps everything it started).
- THE DECODED-SCREEN RULE: compare decoded state through the outer pinned tmux (capture-pane -p -e): attribute order, batching and cursor-movement spelling collapse; the colour class (named, indexed, RGB) does not. A wait is a bounded wait_for, never a sleep. Never fake a channel.
- SIBLING CASES: the gate merges in the order ${ORDER.join(', ')}, one gate agent per branch, each pushing main before the next starts. A case of yours that can pass only after an EARLIER lane in that order merges stays recorded with a reason starting 'SIBLING:<lane key> ' and is listed in sibling_cases; your own gate flips it after rebasing onto the main that carries the sibling. Never point a SIBLING reason at a lane later than yours in the order. A case recorded for any other reason keeps its clause open.

LEDGER
- compat/tui/campaign.json: an obligation's acceptance list IS the contract. ${OWNERSHIP}
- You may write in your obligation: status (active while you work; review when every clause asserts on the branch, SIBLING cases aside; blocked with a complete diagnosis), evidence_note (the current measurement: keep it readable, lead with what asserts and what remains; move history into the attempt's notes.md rather than growing the note forever), next_action, sources. Never the proof block, never verified.
- Evidence at compat/tui/evidence/<ID>/<attempt>/: environment.txt first, each run's output as .txt, notes.md naming every file. Never name a file .log (globally ignored); run git check-ignore -v on the directory before committing. Real runs only.
- python3 compat/tui/tracker.py check before each commit; write-report in the same commit. If you touched compat/tmux-gaps.json, python3 compat/tmux-tracker.py check and write-report too. JSON via json.dump(..., indent=2) plus a trailing newline.
- Decisions not yours to reopen: width never invokes the sidebar; the raw TUI's status row uses the pin's default theme colours until a user sets theme options; zz/mux.conf layers last even under -f; the shell-integration pane title; the armed-prefix hint record (TUI-004).

WIRE PROTOCOL RULE: PROTOCOL_VERSION is 101 on main and STAYS 101 this cycle: 100 and 101 are unreleased (zz 0.7.0 shipped 99), so every append of this cycle folds into 101. Pure appends only (a trailing field or a new variant at the end), consumer half in the same push, and a line added to the v101 entry of knowledge/protocol/wire-protocol.md's version history naming the append. The copy lane changes nothing on the wire.

CODE BOUNDARIES: five lanes touch crates/zz-tui/src. Keep new surfaces in their modules (copy_view, overlay, chooser and so on), minimal hooks in render.rs, state.rs and input.rs, never reflow or reorder code you do not change, and name every render.rs hunk by function in notes. The GUI keeps compiling with no presentation change.

MACHINE ETIQUETTE: every cargo command through the slot-and-cap wrapper in the box note (CAP 5G); cargo build/test --jobs ${M.workerJobs}, -- --test-threads=${M.workerThreads}; never workspace-scale; cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features -- -D warnings per touched crate. If the diff touches crates/, cargo test -p zz (cli_binary) before the last commit, split by test filters across calls if one call cannot hold it. Never pipe cargo test through tail or grep. Load-flake rule: fails loaded + passes exact-solo = flake.

DELIVERY: one commit per punch-list item where feasible, subject in repo style, git -c commit.gpgsign=false, NO attribution trailers, NO comments in code (a fixture's header block is documentation). Every case you flip or add has a --self-check sabotage that fails for the right reason. After your LAST commit re-run every proof you list. Push campaign/BRANCHNAME and report with the final tip sha. NO GitHub comments, NO issue 7, NO compat/board.py mutations.
`

const BATCH_MODES = COMMON + `
YOUR BATCH: finish TUI-004. BRANCHNAME: tui-modes-2. WORKDIR: zz-tui-modes. START: BASE (it carries cycle 5's modes and copy landings). You are FIRST in the gate order; every other lane's SIBLING:modes cases wait on you, and your gate lands BASE on main. HARD BUDGET 270 MINUTES.
Zones: compat/tui/ (TUI-004 and evidence attempt-04), compat/tui-indicators.sh, compat/attached-client.sh (ONLY its zz-side mode expectations), compat/tui-screen-diff.sh (ONLY TUI-004's border checkpoints), crates/zz-tui/src/ (render.rs border and mode drawing, copy_view and state), crates/zz-client/src/, crates/zz-daemon/src/ status publication, crates/zz-protocol/src/ (appends under the wire rule), crates/zz-mux/src/ consumer lists and compat_manifest_tests.rs, crates/zz/tests/, and in compat/tmux-gaps.json the items of options.native-mode-styles and options.native-overlay-styles your landing makes the raw TUI honour.
Read first: TUI-004's record, compat/tui/evidence/TUI-004/attempt-03/notes.md, the BORDER_STYLE_REASON checkpoints in compat/tui-screen-diff.sh, and MATCH_REASON in compat/tui-copy-mode.sh.
PUNCH LIST:
0. ATTACHED-CLIENT AT BASE. HARD BUDGET 45 MINUTES. compat/attached-client.sh exits 1 at BASE with 'zz screen did not visibly become copy-mode within 10 seconds': wait_for_visible_mode (near line 774) still gives zz its own pattern, the old COPY status badge, while zz now draws the pin's [n/m] position indicator in the pane, which the tmux side's pattern already matches. Give both sides the pin's pattern, then run the fixture to the end: any later zz-only expectation that your cycle-5 landing made false (the view surface, the prompt, the command-output overlay) gets the same treatment, the pin's observable on both sides, never a loosened check. The fixture must exit 0 at your tip ('attached-client compatibility: PASS'), run with ZZ_BIN=<your build> TMUX_BIN=${PIN}. SECOND, the flaky daemon test: zz-daemon daemon::tests::client_focus_closes_display_panes_and_preserves_chooser_modes failed 1 of 10 exact-solo runs at BASE and once in the full workspace suite there (the assertion at daemon.rs near line 43415 prints an event list led by a Snapshot, so an event arrives in an order the test does not expect). Run it 20 times exact-solo at 3319ceba (origin/main) and at BASE. If BASE introduced the instability (cycle 5's status sampler and mode refresh changes are the suspects), fix the cause in the code; if it predates BASE, make the test deterministic by waiting on the event it means rather than its position. Either way, 20 of 20 solo passes at your tip, recorded in evidence.
1. CLAUSE 2 BORDERS. The cycle-5 gate held TUI-004 because tui-screen-diff.sh still records the pane-border-top and pane-border-bottom checkpoints (BORDER_STYLE_REASON), and the divider row in tui-indicators.sh's view-inactive cases differs on a plain split with no record covering it (the gate's measurement and the border bytes at 3319ceba and 2911d88c are in attempt-03's notes and the tui-indicators.sh header). Read the pin's border drawing (screen-redraw.c, pane-border-status, pane-border-style/pane-active-border-style, pane-border-lines, pane-border-indicators), fix the raw TUI, flip every BORDER_STYLE_REASON checkpoint to asserted, and make the divider row asserted by cells and style, not glyph only. A sabotage each.
2. MATCH HIGHLIGHT PAINTING. The raw TUI does not paint copy-mode-match-style and copy-mode-current-match-style: the 12 MATCH_REASON cases and emacs-ordinary-pane-search-backspace in compat/tui-copy-mode.sh stay red on the rows channel only (text, cursor, facts, view and buffer already agree). The fix is presentation, which is yours. Read window-copy.c's match drawing (window_copy_update_style, the searchmark and current-match rules), publish what the client needs (the resolved styles and the match spans, extending cycle 5's ModePresentation append), paint them, and close the two option items in options.native-mode-styles. Do NOT edit compat/tui-copy-mode.sh: the copy lane marks those 13 cases SIBLING:modes and its gate flips them. Prove it here with a throwaway copy of that fixture with the 13 cases flipped, committed as evidence and labelled a throwaway flip.
3. STILL-UNPRODUCED FORMATS. Your attempt-03 note names #{top_line_time} (zz-terminal keeps no line times; the pin's default copy-mode-position-format begins with it) and #{copy_position}. Measure whether either reaches a cell any fixture asserts at a default setting; if one does, make it match; if neither does, state that in evidence_note with the measurement.
4. Ledger: TUI-004 to review only when tui-screen-diff.sh has no recorded checkpoint inside clause 2, tui-indicators.sh has zero recorded cases and asserts the divider row whole, and status-row.sh is 14/14 under LC_ALL=C LC_TIME=C.
Proofs at tip: ${RUN_ENV} compat/attached-client.sh (exit 0); compat/tui-indicators.sh three times plus --self-check; compat/tui-screen-diff.sh plus --self-check; compat/tui-pane-geometry.sh; compat/status-row.sh under LC_ALL=C LC_TIME=C; compat/tui-stock-keys.sh; the throwaway copy-mode flip; cargo test and clippy per touched crate; cargo test -p zz; both trackers' check.
`

const BATCH_COPY = COMMON + `
YOUR BATCH: finish TUI-005. BRANCHNAME: tui-copy-2. WORKDIR: zz-tui-copy. START: BASE. HARD BUDGET 240 MINUTES.
Zones: compat/tui/ (TUI-005 and evidence attempt-03), compat/tui-copy-mode.sh, crates/zz-terminal/src/ copy-mode code (not PackedStyle or the frame cell), crates/zz-mux/src/command.rs send-prefix and send-keys paths, crates/zz-daemon/src/ SendKeys effect dispatch and copy-format expansion, crates/zz-protocol/src/ key tables and engine (no wire message change), crates/zz-client/src/ key handling, crates/zz-tui/src/input.rs and app.rs (render.rs is the modes lane's), crates/zz/tests/, and in compat/tmux-gaps.json the items of copy-mode.action-fidelity, copy-mode.command-fidelity, keys.copy-mode-native-numeric-prefix and formats.pane-runtime (the pane_search_string item only) your landing makes match.
Read first: TUI-005's record (next_action carries both measurements with pin source, bytes and fix direction), compat/tui/evidence/TUI-005/attempt-02/gate-review-probe2.txt, and compat/tui-copy-mode.sh.
PUNCH LIST:
1. VI PAGE CLAMP. Measured by the cycle-5 review's probe2 (82 seeded lines, vi, 80x24: C-b [, 5 k, C-u C-u, C-d C-d C-d; and 5 k then NPage): the pin ends at cursor 1,23, zz at 0,23. Fix in page_copy_cursor as next_action describes, with a unit test, one fixture case per sequence per table, and a sabotage.
2. SEND-PREFIX AND SEND-KEYS INTO A PANE IN COPY MODE. C-b C-b in copy-mode-vi pages up once on the pin (server-client.c:1417 gives the prefix precedence; the second C-b runs send-prefix, and cmd_send_keys_inject_key, cmd-send-keys.c:94-100, dispatches the key through the pane mode's own table); zz's send_prefix emits MuxEffect::SendKeys to the pane and never consults the copy table, so nothing moves (pin 6,21 scroll 22, zz 0,21 scroll 0). Make send-prefix and plain send-keys of a key without -X on a pane in copy mode dispatch through that mode's table as the pin does; add fixture cases for C-b C-b in both tables and for send-keys -t into a copy-mode pane, with sabotages.
3. SIBLING ACCOUNTING. The 12 MATCH_REASON cases and emacs-ordinary-pane-search-backspace are the modes lane's this cycle (match-highlight painting): rewrite their reasons to start with 'SIBLING:modes ' followed by the existing measurement and list them in sibling_cases. Your gate flips them after rebasing onto the main that carries modes.
4. THE GAP EDIT CYCLE 5 MADE OUTSIDE ITS ZONE. Cycle 5 removed format:pane_search_string from formats.pane-runtime, and its re-review found the raw TUI does not yet honour it in every stock state (the answer after re-entering copy mode). Make pane_search_string answer as the pin in each state the fixture drives, with the fixture asserting it; if you cannot, restore the item to formats.pane-runtime with the measurement appended. Either way the record ends true.
5. Ledger: TUI-005 to review when the only recorded cases left in tui-copy-mode.sh are the SIBLING:modes ones and clause 3's inventory stands.
Proofs at tip: ${RUN_ENV} compat/tui-copy-mode.sh three times plus --self-check; compat/tui-screen-diff.sh and compat/tui-stock-keys.sh; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const BATCH_CAPS = COMMON + `
YOUR BATCH: finish TUI-009. BRANCHNAME: tui-caps-2. WORKDIR: zz-tui-caps. START: origin/campaign/tui-caps-close-gated (9c050ebf: cycle 5's caps lane plus the gate's must-fix commit, already on BASE, so no rebase is needed). HARD BUDGET 240 MINUTES.
Zones: compat/tui/ (TUI-009 and evidence attempt-03), compat/tui-caps.sh, compat/tui-screen-diff.sh (ONLY its colour checkpoints), crates/zz-terminal/src/ (model.rs PackedStyle, session.rs palette handling, the frame), crates/zz-daemon/src/ (pane palette and attach/client facts), crates/zz-protocol/src/ (appends under the wire rule), crates/zz-tui/src/tty.rs, input.rs, terminal_event.rs and render.rs ONLY in cell colour emission, crates/zz/src/lib.rs and crates/zz/ only for GUI compile consequences, crates/zz/tests/, and in compat/tmux-gaps.json the items of options.client-terminal-negotiation, formats.terminal-cells and formats.terminal-runtime your landing makes match.
Read first: TUI-009's record, compat/tui/evidence/TUI-009/attempt-02/, and compat/scenarios/smoke/pane-colours-palette (the corpus row that made the cycle-5 gate skip this branch).
PUNCH LIST:
1. THE REGRESSION THAT SKIPPED THIS BRANCH. smoke/pane-colours-palette is green at origin/main and red at your branch (7 of 18 zz-side checks: the attached client never writes the pane-colours RGB). Root cause measured by the gate: crates/zz-terminal/src/session.rs palette_class classes a cell against terminal.default_color_palette(), and crates/zz-daemon/src/daemon.rs pane_palette writes pane-colours into that same default palette, so the cell stays Palette and goes out as \\e[31m where the pin writes 38;2;18;52;86. Carry the base palette separately from the per-pane override so a pane-colours entry produces the pin's RGB while an untouched named cell keeps its class. Prove with compat/run.sh on that scenario (exit 0) and a tui-caps.sh case per direction, then re-prove clause 3 whole.
2. WHAT THE CYCLE-5 RECORD LEAVES OPEN. Extended keys: the measured script(1) result in TUI-009's evidence_note says what zz arms and what the pin arms; make them match under the fixture's extended-keys case, or state the exact remaining difference. The -T RGB delta and the missing -2 screen effect named in next_action: fix and assert, each with a sabotage. OSC 10/11 and window-style, named as not driven: drive them, or keep them named in clause 1's declared-uncovered list with why.
3. Ledger: TUI-009 to review only when all three clauses assert with no recorded row inside them and the corpus row is green.
Proofs at tip: ${RUN_ENV} compat/tui-caps.sh three times plus --self-check; compat/tui-screen-diff.sh plus --self-check; compat/run.sh smoke/pane-colours-palette; compat/status-row.sh under LC_ALL=C LC_TIME=C; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const BATCH_OVERLAYS = COMMON + `
YOUR BATCH: finish TUI-007. BRANCHNAME: tui-overlays-2. WORKDIR: zz-tui-overlays. START: origin/campaign/tui-overlays-gated (7c692222: cycle 5's overlays lane rebased by the gate onto the modes and copy landings, with the conflicts resolved: TMUX_OPTION_CONSUMERS 144, the prompt row fed through modes' message painting; it already sits on BASE, so no rebase is needed). HARD BUDGET 240 MINUTES.
Zones: compat/tui/ (TUI-007 and evidence attempt-02), compat/tui-overlays.sh, crates/zz-tui/src/ (overlay module plus minimal hooks), crates/zz-client/src/ overlay reduction, crates/zz-daemon/src/ and crates/zz-protocol/src/ only for appends under the wire rule, crates/zz-mux/src/ consumer lists and compat_manifest_tests.rs, crates/zz/ only for GUI compile consequences, crates/zz/tests/, and in compat/tmux-gaps.json the items of options.native-overlay-styles (message-style and message-command-style included, now that modes paints them), clients.tui-confirm-before-overlay, clients.tui-display-menu-overlay and clients.tui-display-popup-overlay your landing makes match.
PUNCH LIST:
1. THE POPUP RED AT THE MERGED TIP. At 7c692222 the unflipped tui-overlays.sh exits 1: 8 of 47 asserted comparisons differ, all popup cases (popup-opened, popup-typed, popup-under-message, popup-resized, centre-popup-fg, centre-popup-fg-typed, centre-popup-fgbg, centre-popup-fgbg-typed), while the same fixture was 47/47 three times at the lane tip 2606c87d. Suspect: modes' render.rs blit_row trailing-clear (ECH for a trailing blank run of ten or more default cells) against the popup's default-style cells. Diagnose from the DIFF bytes (build fresh first so a stale binary is ruled out), fix, and make all 47 assert again.
2. THE SIBLING:modes CASES. The modes landing is on main. At 7c692222 nine of your thirteen SIBLING:modes cases already match (prompt-opened, prompt-typed, prompt-wrapped, prompt-cursor-home, prompt-resized, prompt-resized-back, prompt-status-top, menu-under-message, menu-resized): flip them to asserted yourself, with sabotages. Four still differ (confirm-opened, confirm-resized, popup-under-message, popup-resized): they are yours now; fix and assert them.
3. Ledger: TUI-007 to review when tui-overlays.sh has zero recorded cases. Its record notes it verifies only once TUI-004 is verified.
Proofs at tip: ${RUN_ENV} compat/tui-overlays.sh three times plus --self-check; compat/tui-screen-diff.sh; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const BATCH_CHOOSERS = COMMON + `
YOUR BATCH: finish TUI-006. BRANCHNAME: tui-choosers-2. WORKDIR: zz-tui-choosers. START: origin/campaign/tui-choosers (069143ba, on the cycle-4 base). The killed first run already rebased it onto BASE in your worktree (local branch tui-choosers-2) and was partway through its edits; the cycle-5 gate's helper scripts were lost with the reboot. The conflict map, for checking that rebase: hunt_claims.rs keeps the ..._one_hundred_and_one name; render.rs hunk 1 keeps choosers' paint_mode_tree match without the command-output branch modes deleted; render.rs hunk 2 keeps the menu/confirm cursor, else restore_mode_tree_cursor. HARD BUDGET 240 MINUTES.
Zones: compat/tui/ (TUI-006 and evidence attempt-02), compat/tui-choosers.sh, crates/zz-tui/src/ (chooser module plus minimal hooks), crates/zz-client/src/ chooser reduction, crates/zz-daemon/src/ and crates/zz-protocol/src/ only for appends under the wire rule, crates/zz-mux/src/ consumer lists and compat_manifest_tests.rs, crates/zz/ only for GUI compile consequences, crates/zz/tests/, and in compat/tmux-gaps.json the items of choosers.native-presentation, clients.tui-command-output-navigation and clients.command-output-pane-prompt your landing makes match.
PUNCH LIST:
1. THE REVIEW'S THREE UNRECORDED DIVERGENCES in clause 1: choose-tree -Z zoom, choose-buffer f (filter), and choose-tree c. The cycle-5 review's bytes and fix direction are in its record (TUI-006's evidence_note after the gate's planned edits, or the review in the gate scratch notes). Add a fixture case for each, fix each, with sabotages.
2. SIBLING CASES. output-shown rides modes' view surface, now on main: flip it to asserted. find-window-prompt and find-window-typed name SIBLING:overlays, which is wrong: the prompt row's style is modes', now on main, so flip them to asserted if they match, or fix what still differs.
3. The choosers.native-presentation resolution phrase gains 'or its first under status-position top' (the gate's planned edit); declare the GUI key change the review noted.
4. Ledger: TUI-006 to review when tui-choosers.sh has zero recorded cases. Its record notes it verifies only once TUI-004 is verified.
Proofs at tip: ${RUN_ENV} compat/tui-choosers.sh three times plus --self-check; compat/tui-stock-keys.sh and compat/tui-screen-diff.sh; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz TUI parity campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what its gate trusts. NEVER push, commit, touch the board or GitHub issues, or edit ${M.root}. One other agent runs beside you; keep to the caps.
HOW YOU REPORT: your final act is the structured report. FOREGROUND ONLY, timeout at most 590000, never a background task, never end your turn to wait.
SETUP: GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'. REVIEWDIR exists and is clean: checkout --detach the branch tip there (if dirty, add ${M.dev}/REVIEWDIR-2). export CARGO_TARGET_DIR=WORKERTARGET (the worker's warm target; the worker is finished); touch crates/**/*.rs and Cargo.* first, never cargo clean, rebuild before spawning a binary. ${M.boxNote}
ETIQUETTE: every cargo command through the slot-and-cap wrapper in the box note (CAP 5G); cargo test -p <pkg> --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads}; throwaway pin servers -L zzprobe-$$ -f /dev/null only; never kill servers you did not start (${M.protected}); delete any binary copy you made under /tmp before reporting.
METHOD:
1. PUNCH LIST AUDIT: for each punch-list item in the batch below, is it done at the tip, with a fixture case that asserts the pin's behaviour and a sabotage that fails for the right reason? An item neither done nor honestly recorded as open is a blocker.
2. CONTRACT AUDIT: for the obligation, take each acceptance clause and find the ASSERTED evidence at the tip. A recorded case inside a clause keeps it open, except a SIBLING:<lane> case pointing at an EARLIER lane in the gate order (${ORDER.join(', ')}). An accepted gap is never evidence. The worker may write only status, evidence_note, next_action and sources of its obligation. ${OWNERSHIP}
3. PROOFS AT TIP: run the claimed proofs yourself with a zz built at the tip and the pin ${PIN}; the touched crates' tests and cargo test -p zz. Any red at tip is a blocker (load-flake rule applies).
4. ORACLE SPOT-CHECKS: drive the pin yourself for every behaviour the lane changed and confirm the fixture asserts what the pin does. Every wait a bounded wait_for. Run the fixture and confirm it reaps what it starts.
5. TEST HONESTY: run every new or flipped case's sabotage, add one of your own, confirm evidence files are real runs, no .log files, git check-ignore -v clean.
6. INVARIANTS: zones, judged on git diff ${M.base}...HEAD (BASE is cycle 5's gated chain, not this lane's work; the lane's branch sits on it); the wire rule (PROTOCOL_VERSION stays 101, pure appends, consumer half present, the v101 history line added; read the wire diff hunk by hunk); the GUI builds with no presentation change; no added code comments; no attribution trailers; both trackers green; generated reports regenerated.
CALIBRATION: confirmed_defects only with proof; suspicion goes in notes. blocker = a claimed clause without real evidence, a fixture that cannot fail, a zone or wire violation, a red at tip; must-fix = the gate applies before merge; nit = mention. VERDICT approve / approve-with-fixes / reject; make every blocker precise (probe, expected bytes, file). Fill every_clause_asserted honestly: the gate will not verify an obligation you mark no. Bounded: well under 90 minutes after the build.
`
const REVIEW_EXTRA = {
  modes: 'LANE-SPECIFIC: set pane-border-status top and bottom, pane-border-lines heavy and double, and a user pane-active-border-style on both sides and compare the border rows yourself; search in copy mode and read the current-match and other-match cells on both sides; run the lane\'s throwaway copy-mode flip.',
  copy: 'LANE-SPECIFIC: re-run the cycle-5 probe2 sequences (compat/tui/evidence/TUI-005/attempt-02/gate-review-probe2.txt) at the tip; press C-b C-b in both copy tables and send-keys -t a key into a copy-mode pane on both sides; query #{pane_search_string} after re-entering copy mode on both sides.',
  caps: 'LANE-SPECIFIC: run compat/run.sh smoke/pane-colours-palette yourself at the tip; print \\e[31mX, \\e[38;5;42mY and \\e[38;2;1;2;3mZ, then set pane-colours on the pane, and read the classes through the outer tmux on both sides; confirm the GUI painting path resolves the same RGB as before.',
  overlays: 'LANE-SPECIFIC: open display-popup centred at an odd height with and without a message shown, and confirm-before at a resize, on both sides; confirm the popup red at 7c692222 is explained by a named cause and fixed, not re-recorded.',
  choosers: 'LANE-SPECIFIC: drive choose-tree -Z, choose-buffer then f, and choose-tree then c on both sides; confirm the rebase kept modes\' command-output removal and the cursor rules the gate mapped.',
}

const START = {
  modes: M.base,
  copy: M.base,
  caps: 'origin/campaign/tui-caps-close-gated',
  overlays: 'origin/campaign/tui-overlays-gated',
  choosers: 'origin/campaign/tui-choosers',
}
const PROMPTS = { modes: BATCH_MODES, copy: BATCH_COPY, caps: BATCH_CAPS, overlays: BATCH_OVERLAYS, choosers: BATCH_CHOOSERS }
const LANES = ORDER.map(key => ({ key, prompt: PROMPTS[key], start: START[key], workdir: `zz-tui-${key}`, reviewdir: `zz-tui-${key}-review`, target: `${M.dev}/zz-tui-${key}/target`, extra: REVIEW_EXTRA[key] }))
const OPTS = { model: 'opus', effort: 'xhigh' }

function reviewPrompt(r, rereview) {
  const head = rereview
    ? `RE-REVIEW after a reject: the first review's confirmed defects were:
${JSON.stringify(r.firstReview.confirmed_defects, null, 2)}
A fresh agent fixed the branch in place and pushed; verify each blocker at the NEW tip (re-run the failing probes), then run the full method again.
`
    : ''
  return REVIEW_COMMON + '\n' + r.lane.extra + `
${head}LANE: ${r.lane.key}. BRANCH: ${r.worker.branch}. REVIEWDIR: ${r.lane.reviewdir}. WORKERTARGET: ${r.lane.target}.
THE LANE'S BATCH PROMPT:
${r.lane.prompt}
WORKER REPORT (verify, do not trust):
${JSON.stringify(r.worker, null, 2)}`
}

function fixPrompt(r) {
  const defects = r.review.confirmed_defects || []
  return `The adversarial review of ${r.worker.branch} came back REJECT. You are a fresh agent taking over that lane: cd ${M.dev}/${r.lane.workdir}; git status --short; git log --oneline origin/main..HEAD; read the diff and the review before touching anything. Uncommitted residue belongs to the previous worker: inspect it, keep what belongs to the fix, discard nothing blindly. Fix every blocker and must-fix on the SAME branch, push (never force), and emit the FULL report again: every obligation with statuses corrected, sibling_cases, every proof re-run at the NEW tip, the new tip sha and what each fix was. HARD BUDGET 120 minutes plus the proofs. Prefer fixing the code; if a blocker does not fit, move the clause to clauses_open, set the status honestly and put the reviewer's measurement into evidence_note.
CONFIRMED DEFECTS:
${JSON.stringify(defects, null, 2)}
REVIEW NOTES: ${r.review.notes}
EVERY CLAUSE ASSERTED (reviewer): ${r.review.every_clause_asserted}
CHECKS THE REVIEWER RAN: ${JSON.stringify(r.review.checks_run)}
ORIGINAL WORKER REPORT:
${JSON.stringify(r.worker, null, 2)}
THE ORIGINAL BATCH PROMPT (still binds you; same worktree, same branch):
${r.lane.prompt}`
}

async function runLane(lane) {
  const worker = await agent(lane.prompt, { label: `worker:${lane.key}`, phase: 'Work', schema: WORKER_SCHEMA, ...OPTS })
  let r = { lane, worker }
  if (!worker || !worker.branch) return r
  r.review = await agent(reviewPrompt(r, false), { label: `review:${lane.key}`, phase: 'Review', schema: REVIEW_SCHEMA, ...OPTS })
  if (!r.review || r.review.verdict !== 'reject') return r
  log(`${lane.key}: review REJECT, running one fix pass`)
  const fix = await agent(fixPrompt(r), { label: `fix:${lane.key}`, phase: 'Review', schema: WORKER_SCHEMA, ...OPTS })
  if (!fix || !fix.branch) return r
  r = { ...r, firstWorker: worker, firstReview: r.review, worker: fix, fixed: true }
  r.review = await agent(reviewPrompt(r, true), { label: `rereview:${lane.key}`, phase: 'Review', schema: REVIEW_SCHEMA, ...OPTS })
  return r
}

function gatePrompt(r, earlier) {
  const summary = { key: r.lane.key, review: r.review || null, first_review: r.firstReview || null, fixed_after_reject: !!r.fixed, ...r.worker }
  return `You are the integration gate for ONE branch of the zz TUI parity campaign (repo demfabris/zz, board = GitHub issue 7): the ${r.lane.key} lane of cycle 6. Gates run one per branch in the order ${ORDER.join(', ')}; each pushes main before the next starts, so origin/main already carries every earlier lane that merged. One other lane's worker or reviewer may run beside you: use --jobs ${M.gateJobs}, never run two heavy things of your own at once (cycle 5's gate manufactured flakes by overlapping cargo tests with corpus chunks), and apply the load-flake rule. FOREGROUND ONLY: every command in the foreground with a timeout of at most 590000 (a call is capped at 600 seconds; split cargo test by package and the corpus by explicit scenario names); never a background task; never end your turn to wait. You are done when you have emitted the structured report.
EARLIER GATES THIS CYCLE:
${JSON.stringify(earlier, null, 2)}
THIS LANE, worker report + review verdict:
${JSON.stringify(summary, null, 2)}
REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix in its own commit, re-running the reviewer's probe as proof. reject (after the fix pass) => do not merge; push the rebased tip as campaign/<branch>-gated, post the blockers as a board note on ${LOCK}, report, stop. A blocker you can fix in minutes may be fixed and merged with the probe as proof. Missing review => a compressed contract audit yourself first.
VERIFIED MEANS EVERY CLAUSE: read the obligation's acceptance list, its evidence_note at your tip and the reviewer's every_clause_asserted. Any clause open or partial, or holding a recorded case (a SIBLING case you did not flip green included), keeps the lane's honest status. An accepted gap is never evidence. A dependency not verified on main blocks verified (TUI-006 and TUI-007 need TUI-004).
BOARD: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. claim MAIN --lease 3h before you rebase; renew MAIN --lease 2h before any long stage; after the push: note ${LOCK} --note "<lane> integrated at <sha>: verified <ids or none>; open <what and why>; review <verdict> and what you did"; then release MAIN --reason "cycle 6 <lane> pushed at <sha>". If MAIN is held by someone other than ${M.holder}, wait for it with bounded polls (at most 30 minutes), then report it as a problem and stop. BOARD FALLBACK: on a GitHub auth failure append the command to ${M.root}/compat/tui/board-replay-6.sh (uncommitted) and continue.
${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. Never use ${M.root}'s local main, never edit, stash or reset in it. Commits: git -c commit.gpgsign=false commit.
THIS BOX: ${M.boxNote} Gate worktree: ${M.dev}/zz-gate-tui6 (create it if missing: git -C ${M.root} worktree add --detach ${M.dev}/zz-gate-tui6 origin/main; if it exists and is clean, reuse it; never remove it, the next gate reuses it). export CARGO_TARGET_DIR=${M.dev}/zz-gate-target in every shell that runs cargo, compat/check.sh included; touch crates/**/*.rs and Cargo.* after every checkout. Every cargo command goes through the slot-and-cap wrapper in the box note with CAP 7G; the one other working agent may hold the other slot.
STAGES:
1. Fetch; git merge-tree --write-tree origin/main <tip> to predict conflicts; check out the lane tip on a local branch gate-${r.lane.key} and rebase onto origin/main. BASE (${M.base}, cycle 5's gated modes and copy chain) reaches main through the first gate that pushes: if origin/main does not contain it yet, your branch carries it, and it lands only with stage 3 fully green, compat/attached-client.sh included. If attached-client.sh fails only at 'zz screen did not visibly become copy-mode' (the modes lane's punch item 0, not landed because that lane did not merge), give its zz side the pin's [n/m] pattern in wait_for_visible_mode as a gate commit and rerun it to the end. Conflicts: generated reports regenerate; campaign.json and tmux-gaps.json merge by record and by item; render.rs, state.rs and input.rs merge by union; TMUX_OPTION_CONSUMERS and compat_manifest_tests.rs counts by union recomputed; PROTOCOL_VERSION stays 101 and the v101 history entry keeps every line. A conflict union cannot settle inside files the lane does not own => push campaign/<branch>-gated, report, stop.
2. cargo test -p <each touched package> and cargo test -p zz --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (exit code, never piped); cargo clippy --workspace --all-targets --all-features --jobs ${M.gateJobs} -- -D warnings; ${RUN_ENV} compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <touched_commands> plus every keys and status scenario and smoke/tui-client-input-backpressure, in sequential chunks. Red that is not a load flake and not a known environmental row: fix if minutes, else push campaign/<branch>-gated, report, stop.
3. Build zz (cargo build -p zz --jobs ${M.gateJobs}) and run with the pin: compat/tui-screen-diff.sh plus --self-check, compat/tui-pane-geometry.sh, compat/tui-stock-keys.sh, compat/status-row.sh (LC_ALL=C LC_TIME=C), compat/tui-indicators.sh, compat/tui-copy-mode.sh, compat/tui-caps.sh, compat/tui-overlays.sh, compat/tui-choosers.sh, each with --self-check where it has one, and compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<the build>). Every one must exit 0 except known flakes rerun solo; a red that an earlier lane's merge would explain is still this lane's to settle before pushing.
4. SIBLING FLIPS: for this lane's sibling_cases whose owner is on main, flip each case to asserted and run its fixture plus --self-check; keep a flip only if both exit 0, otherwise revert that flip and append the failing channel to its reason.
5. RECORDS for this lane's obligation, in one commit: if every clause asserts (VERIFIED MEANS EVERY CLAUSE), its dependencies are verified on main, and stage 3 is green: write compat/tui/evidence/<ID>/<attempt>/review.md (the reviewer's verdict JSON verbatim, checks_run, your review_actions, the flips), fill the proof block (revision = this lane's final pre-records tip, tmux_commit = the pin, environment = one line plus a pointer to environment.txt, commands with exit codes, artifacts, review) and set verified with next_action naming what it unlocks (TUI-004 lets TUI-006 and TUI-007 verify; TUI-005 and TUI-007 feed TUI-008; TUI-006 and TUI-007 feed TUI-011; TUI-009 and TUI-008 feed TUI-012). ALSO: if this lane's merge makes an EARLIER-held obligation meet the bar (for example TUI-006 or TUI-007 held only by TUI-004, now verified), verify it here with its own stage-3 runs cited. Otherwise the status stays honest with the reason appended. Then both trackers' write-report and check, python3 -B compat/tui/tracker_test.py, compat/check.sh.
6. Push main (git push origin HEAD:main). Non-fast-forward: fetch, rebase, rerun stage 3's fixtures, push. Never force. Then the board note and MAIN release.
7. Sweep: pgrep -fa 'zz-cli-|zz-user|zzprobe'; reap only what this gate started; delete any binary copies you made under /tmp. Leave ${M.dev}/zz-gate-tui6 in place.
Report: branch, merged, pushed_sha, verified ids, sibling_flips, gate_summary, review_actions, flakes, fixtures (exit code and last line of each at the pushed tip), board_updates, problems.`
}

log(`TUI cycle 6: ${ORDER.join(', ')}; at most two agents at a time; each lane's gate follows its review, gates in order`)
const SLOTS = 2
const queue = LANES.slice()
const lanes = {}
const gates = []
const gateResolve = {}
const gateDone = {}
for (const l of LANES) gateDone[l.key] = new Promise(res => { gateResolve[l.key] = res })

async function gateLane(r, i) {
  if (i > 0) await gateDone[LANES[i - 1].key]
  let g
  if (!r || !r.worker || !r.worker.branch) {
    g = { key: LANES[i].key, skipped: (r && r.error) || 'no branch pushed' }
  } else {
    const out = await agent(gatePrompt(r, gates), { label: `gate:${r.lane.key}`, phase: 'Integrate', schema: GATE_SCHEMA, ...OPTS })
    g = { key: r.lane.key, ...(out || { problems: 'gate agent returned nothing' }) }
    log(`gate:${r.lane.key} -> merged=${out && out.merged} pushed=${out && out.pushed_sha} verified=${out && JSON.stringify(out.verified)}`)
  }
  gates.push(g)
  gateResolve[LANES[i].key](g)
}

async function slot() {
  while (queue.length) {
    const lane = queue.shift()
    const i = LANES.indexOf(lane)
    let r
    try {
      r = await runLane(lane)
    } catch (e) {
      r = { lane, worker: null, error: String(e) }
    }
    lanes[lane.key] = r
    try {
      await gateLane(r, i)
    } catch (e) {
      gates.push({ key: lane.key, problems: `gate failed: ${String(e)}` })
      gateResolve[lane.key]({ key: lane.key, problems: String(e) })
    }
  }
}

await Promise.all(Array.from({ length: SLOTS }, () => slot()))
return { lanes: ORDER.map(k => lanes[k]), gates }
