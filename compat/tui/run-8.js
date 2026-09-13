export const meta = {
  name: 'tui-run-8',
  description: 'TUI parity cycle 8, the closing cycle: three Opus 5 lanes at xhigh with zones drawn around obligations rather than crates, at most three agents at once, landing the pin stock mouse bindings for TUI-008, the client capability wire for TUI-009, and choose-client plus the command roster for TUI-006 and TUI-011, with the last gate flipping TUI-012',
  phases: [
    { title: 'Work', detail: 'three worktrees from origin/main: keys (TUI-008), caps (TUI-009), commands (TUI-006 and TUI-011); three agents at a time including the gates' },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh per lane, adversarial, pipelined behind its worker; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: 'one gate agent per branch in the order keys, caps, commands; each rebases, tests, writes its records, pushes main, and the commands gate also verifies TUI-012' },
  ],
}

const A = args || {}
const M = {
  root: A.root || '/home/demfabris/dev/zz',
  dev: A.dev || '/home/demfabris/dev',
  holder: A.holder || 'alienware/orchestrator',
  machine: A.machine || '16-core, 15 GB (plus 15 GB zram) CachyOS Linux box (alienware)',
  workerJobs: A.workerJobs || 3,
  workerThreads: A.workerThreads || 3,
  gateJobs: A.gateJobs || 5,
  gateThreads: A.gateThreads || 4,
  date: A.date || '2026-09-14',
  base: A.base || 'origin/main',
  protected: A.protected || "nothing at launch: no zz daemon or tmux server of the user's was running on this box when cycle 6 started; if one appears mid-run it is the user's. Never touch a server on the default sockets (/run/user/1000/zz/default.sock, /tmp/tmux-1000/default) that you did not start",
  boxNote: A.boxNote || "This box (alienware) is LINUX: CachyOS (Arch-based), 16 cores, 15 GB RAM plus 15 GB zram swap; /bin/bash is 5.x; the filesystem is btrfs; /opt/homebrew does not exist, so the PATH=/opt/homebrew/bin:$PATH prefix this prompt carries is harmless. Where a prompt says sw_vers, use uname -a plus head -2 /etc/os-release for the OS line. The zz daemon ring log lands under the scrubbed HOME at .local/state/zz/logs/. The compat caches are populated and cycles 1, 3, 4 and 5 ran here: every TUI fixture is known green at origin/main EXCEPT compat/status-row.sh under the box locale (LC_TIME=pt_BR.UTF-8 changes %b; the LC_ALL=C LC_TIME=C control exits 0), and four corpus rows are environmental here (micro-flags, show-options-hooks, lane2-store, smoke/plugin-runtime-resurrect-restore). Slow corpus rows that pass alone: command-item-format (up to 8 minutes), command-prompt-editing (about 4 minutes), copy-mode-stock-action-keys. A red row or fixture at your tip is yours only if it is green at origin/main on this box. Known fixture fragility: compat/tui-stock-keys.sh root-binding-detaches can flake on a wall-clock second boundary and its recorded count wobbles; tui-screen-diff.sh --self-check exited 2 once ('zz refused status-right') and passed solo. zz-daemon client_focus_closes_display_panes_and_preserves_chooser_modes fails about 1 run in 10 even exact-solo at BASE: the modes lane owns it this cycle, so elsewhere a single red there reruns and is named as this known flake, not chased. A single Bash call is capped at 600 seconds and a longer command is moved to the background and killed: pass timeout of at most 590000 and split long work. A cargo debug build of zz is NOT bit-reproducible here: a hash identifies an artifact, the revision plus a clean worktree attests it. The box has a real ~/.tmux.conf and ~/.config/zz/mux.conf: never read or edit them and never start a server that would load them (scrub HOME and XDG_CONFIG_HOME, both; probes use -f /dev/null). Never write 'rm -rf $HOME' or 'rm -rf ~'; put a scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz). HYGIENE, measured in cycle 5: agents left about 5 GB of zz binary copies under /tmp, which is a RAM-backed tmpfs, and filled the zram swap; a cargo test killed by a timeout leaves /tmp/zz-cli-*/ daemons running for hours; a scrubbed environment without XDG_RUNTIME_DIR autostarts a daemon on /tmp/zz-user/default.sock. So: copy a binary under /tmp only when a comparison needs a frozen copy, delete every copy before your final report, and at the end run pgrep -fa 'zz-cli-|zz-user|zzprobe' and reap only pids whose command line or environment names a socket or scratch HOME you created. After switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first, and rebuild before spawning a binary. Every git commit is 'git -c commit.gpgsign=false commit'. MEMORY, measured 2026-09-11: the first run of this cycle drove the box out of memory about twenty minutes in, with five lanes compiling and testing at once (four rustc, a 1.5 GB zz-daemon test binary, three cargo), and the desktop stayed unusable for hours. So EVERY cargo command on this box goes through a shared two-slot lock and a per-command memory cap, exactly like this: S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=CAP -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo <args> (CAP is 5G for lanes and reviewers, 7G for a gate). fabrico's rule for this box since the OOM: at most two agents work at a time, and the runner enforces it; the two lock slots keep at most two cargo commands running even so. A command that waits more than 540 seconds for a slot exits 1 without running: run it again. A command that hits its cap is killed with exit 137 instead of taking the box down: run it again with fewer --jobs or --test-threads. Neither is a test result. Fixtures and probes need no wrapper, but run one at a time per lane.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz) through the gh credential helper; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands.',
}
log(`TUI cycle 8 on ${M.machine}; three lanes from origin/main, three agents at a time, --jobs ${M.workerJobs}; one gate agent per branch at --jobs ${M.gateJobs}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`


const ORDER = ['keys', 'caps', 'commands']
const LOCK = 'F-TUI-CYCLE-8-LANES'
const OWNERSHIP = `LEDGER OWNERSHIP THIS CYCLE (compat/tui/campaign.json records; edit only yours): keys lane = TUI-008 (evidence attempt-02); caps lane = TUI-009 (attempt-05); commands lane = TUI-014, TUI-016 and TUI-017 (attempt-01 each). Cycle 7 split TUI-011 into five children, TUI-014 to TUI-018, and TUI-011 now depends on all of them: it is nobody's this cycle and no lane edits it. TUI-006 is held by one case that TUI-014's first clause closes, and the COMMANDS GATE alone verifies it; the worker never edits TUI-006's record. TUI-012's proof block is filled and its status is review, held on TUI-008 and TUI-009: nobody's worker touches it and the LAST gate of this cycle verifies it if both land. TUI-015 and TUI-018 are not this cycle's. Everything else and every earlier attempt is read-only history, as are the frozen twelve-id baseline, the milestones and the pin. In compat/tmux-gaps.json a lane may close ONLY the items its own landing makes the raw TUI honour, inside the gaps its batch names, each with a dated measurement (TMUX_OPTION_CONSUMERS and the compat_manifest_tests.rs partition move in the same commit); an accepted gap keeps its decision for the GUI and for every item not closed.`


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


const COMMON = `You are an autonomous worker on the zz TUI parity campaign (repo demfabris/zz). TWO OTHER LANES or gates run beside you on this ${M.machine} (the runner keeps at most two agents working at once, fabrico's limit for this box), and the user may code here too, so stay inside your zones and your parallelism caps. Rules that are not negotiable:

WHAT THIS CYCLE IS FOR
- Two cycles have ended with obligations one or two items short of verified. This cycle is a PUNCH LIST: your batch names exactly what cycle 5's reviewers and gate left open on your obligation, with the measurements and the fix directions. Do those items, in order, and nothing else first. An obligation counts only when every clause asserts; your lane is judged by whether its obligation can be set verified at its gate.
- THE CONTRACT DECIDES THE TUI PORTION OF ACCEPTED NATIVE GAPS (triage 2026-09-10, knowledge/designs/tui-parity.md): the raw TUI renders the pin's cells; the GUI keeps its native presentation. A case recorded only because of such a gap is a divergence to fix. Record a product decision in evidence_note with the old behaviour, the pin's measured behaviour and the sentence "${DECIDED}".
- A SCREEN STRING YOU REMOVE MAY BE ASSERTED SOMEWHERE ELSE. Cycle 6's choosers gate found a corpus scenario asserting the zz-only chooser chrome that lane had just replaced with the pin's mode tree; neither the worker nor the reviewer had run a corpus row, so the gate was the first place the two met. Before you call an item done, grep compat/scenarios for every zz-only string, prompt, label or format your landing removes or changes, and run compat/run.sh for the rows you find. The same goes for a wire field or an option name you retire.
- YOUR ZONES ARE DRAWN AROUND YOUR OBLIGATION, NOT AROUND A CRATE. This is the change cycle 8 makes. Three cycles in a row an obligation stayed open because its one remaining fix sat in a crate the lane did not hold, and each time the item closed in a single run once a lane was given those files. Your batch names every file your obligation needs, across crates. If you find one more that it truly needs, take it, name it in notes as a zone excursion with the measurement that forced it, and keep going. Do not record a case whose fix you can reach.
- FOUND SOMETHING NEW? If you find a divergence inside your obligation that no punch-list item names, fix it if it fits your zones and budget; if it does not, record it as a fixture case with its measurement and leave the obligation at active. Do not hide it and do not stop the punch list for it.

HOW YOU REPORT
- Your final act is the structured report the output schema describes. A worker that ends its turn without it is a failed agent and its lane is dropped.
- FOREGROUND ONLY: every command in the foreground, timeout at most 590000; never Monitor, run_in_background or any background task; never end your turn to wait.
- Check 'date' at the start of each item. The batch's HARD BUDGET is a ceiling. When it is spent, stop, write what asserts now and what remains into evidence_note, set the status honestly, push and report.

SETUP
- The shared checkout is ${M.root}. Read it and add worktrees from it; NEVER edit, stash, reset or clean it. Its local main branch is stale: use origin/main. knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report, never hand-merge them.
- ${M.gitNote} Fetch with GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME (never force, never main).
- BASE THIS CYCLE is origin/main, which carries every cycle-7 landing. Read what is verified with python3 compat/tui/tracker.py check and the ledger itself; do not trust a remembered count. Gates push main mid-cycle, so fetch before you branch and again before you push, and diff your work with git diff origin/main...HEAD (three dots, the merge-base), never two.
- Worktree: your WORKDIR under ${M.dev} was pre-positioned by the orchestrator at origin/main, clean, with a warm target. Confirm with git log -1 and git status --short. If origin/main has moved past it when you start, rebase onto origin/main first (resolve conflicts by keeping both sides' intent, regenerate generated reports, rebuild, and run your obligation's fixture once before any new work). Build through the wrapper (CAP 5G): S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=5G -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo build -p zz --jobs ${M.workerJobs} > build.log 2>&1, check the exit code.
- compat/attached-client.sh is green at origin/main and every lane this cycle changes input or presentation, so every lane runs it and reports anything red.
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
- Decisions not yours to reopen: width never invokes the sidebar; the raw TUI's status row uses the pin's default theme colours until a user sets theme options; the daemon loads only zz/mux.conf, and explicit -f files replace it in argument order (fabrico changed this on 2026-09-11 in 268ccd9d; the old 'layers last even under -f' rule is dead, see knowledge/configuration/app-config.md); the shell-integration pane title; the armed-prefix hint record (TUI-004).

WIRE PROTOCOL RULE: PROTOCOL_VERSION is 102 on main and STAYS 102 this cycle; 102 is unreleased (101 shipped in zz 0.8.0), so every append of this cycle folds into it. Pure appends only (a trailing field or a new variant at the end), the consumer half in the same push, and a line naming the append in the v102 entry of knowledge/protocol/wire-protocol.md's version history. Two lanes append to crates/zz-protocol/src/message.rs this cycle: append at the tail only, never renumber, and expect the gate to merge the version-history entry by union.

CODE BOUNDARIES: three lanes touch crates/zz-tui/src and the split is by FILE, named in each batch; never edit a file another lane owns, even for a one-line hook, and never reflow or reorder code you do not change. Keep new surfaces in their modules (copy_view, overlay, chooser and so on), minimal hooks in render.rs, state.rs and input.rs, never reflow or reorder code you do not change, and name every render.rs hunk by function in notes. The GUI keeps compiling with no presentation change.

MACHINE ETIQUETTE: every cargo command through the slot-and-cap wrapper in the box note (CAP 5G); cargo build/test --jobs ${M.workerJobs}, -- --test-threads=${M.workerThreads}; never workspace-scale; cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features -- -D warnings per touched crate. If the diff touches crates/, cargo test -p zz (cli_binary) before the last commit, split by test filters across calls if one call cannot hold it. Never pipe cargo test through tail or grep. Load-flake rule: fails loaded + passes exact-solo = flake.

DELIVERY: one commit per punch-list item where feasible, subject in repo style, git -c commit.gpgsign=false, NO attribution trailers, NO comments in code (a fixture's header block is documentation). Every case you flip or add has a --self-check sabotage that fails for the right reason. After your LAST commit re-run every proof you list. Push campaign/BRANCHNAME and report with the final tip sha. NO GitHub comments, NO issue 7, NO compat/board.py mutations.
`



const BATCH_KEYS = COMMON + `
YOUR BATCH: finish TUI-008. BRANCHNAME: tui-keys. WORKDIR: zz-tui-keys-8. START: origin/main. HARD BUDGET 330 MINUTES. You are FIRST in the gate order.
Zones, drawn around the obligation: crates/zz-tui/src/input.rs, app.rs and state.rs; crates/zz-client/src/chrome.rs, menu.rs and core.rs; crates/zz-protocol/src/key.rs, catalog.rs and message.rs (tail appends only); crates/zz-mux/src/command.rs (the mouse key tables, the mouse key dispatch and the -M flags) and its key-table storage wherever it lives; crates/zz-mux/src/formats.rs (the eight mouse_* formats live there, which is why cycle 7 could not close them); crates/zz-terminal/src/session.rs and interaction.rs; crates/zz-daemon/src/daemon.rs, ONLY its mouse, paste and focus routing; compat/tui-mouse.sh, compat/tui-stock-keys.sh, compat/attached-client.sh; compat/tmux-gaps.json (keys.root-native-mouse, keys.copy-mode-native-mouse, mouse.bound-context, formats.mouse-context); compat/tui/ (TUI-008, evidence attempt-02). NOT yours: tty.rs, terminal_event.rs and render.rs (the caps lane), the chooser surfaces and catalog.rs's unimplemented-command roster (the commands lane).
Read first: TUI-008's record and compat/tui/evidence/TUI-008/attempt-01/notes.md, which carry cycle 7's measurements, and the pin's key-bindings.c (the root mouse defaults at :498-522 and the copy-table ones), server-client.c (server_client_handle_key and the mouse translation) and window-copy.c.
WHAT CYCLE 7 LEFT, measured, so you do not re-derive it: your fixture compat/tui-mouse.sh exists and drives every gesture in clause 1. Custom root bindings already assert (click-user-binding, border-user-binding, status-user-binding), and so do the pane click, the application-mouse forward with mouse on and off, the status window-name click and the status wheel up. Eight cases record, for ONE cause: zz installs none of the pin's stock mouse bindings. 'list-keys -T root' answers 27 rows on the pin and 0 on zz. Two of the eight carry a second cause: input.rs still sets click_count to 0 or 1, so the double and triple click paths are unreachable. Three paste and focus cases record: a paste under copy mode and a paste under a menu reach the program behind the overlay because handle_paste writes straight to the pane, where the pin's window mode and its menu consume the key; and with focus-events off the pin still hands an arriving report to a pane that asked for it, while zz gates delivery on the option in the daemon.
PUNCH LIST:
1. INSTALL THE PIN'S STOCK MOUSE BINDINGS AND MAKE THEM RUN. All 27 root rows and all 14 copy-table rows, in the shared default key tables, so list-keys -T root and -T copy-mode(-vi) answer what the pin answers and each binding's command runs with the pin's target and the pin's visible result. This is the whole of the eight recorded cases and it is why keys.root-native-mouse and keys.copy-mode-native-mouse exist; under the 2026-09-10 triage the raw TUI runs the pin's commands and the GUI keeps its native pointer handling, so the GUI must send no mouse key and must not change. Close in those two gaps exactly the items your landing makes the raw TUI honour, each with a dated measurement.
2. THE GESTURES THE CLIENT CANNOT YET FORM. Give input.rs a real click_count (the pin's double-click and triple-click intervals, measured from the pin, not invented) so DoubleClick1Pane and TripleClick1Pane fire and zz-terminal's triple-click path is reachable; add the border hit test so MouseDrag1Border resizes; carry the scrollbar and the remaining status ranges. Assert each against the pin's own result.
3. BOUND-EVENT CONTEXT. mouse.bound-context's five items (copy-mode -S, move-pane -M, resize-pane -M, send-keys -M and display-message's bare mouse target) and formats.mouse-context's eight mouse_* formats need the invoking event carried to the command. Land what the raw TUI can honour and assert each in the fixture.
4. PASTE AND FOCUS, the three recorded cases above. Route a paste through the surface that owns the input context, the way the pin's window mode and menu consume it, and deliver a focus report to a pane that asked for it regardless of the option, matching the pin. Do not break the asserted paste cases doing it.
5. FIX THE DETACH TIMESTAMP RACE while you are in compat/tui-stock-keys.sh. Cycle 7's gate got one green in four runs there: the detach case compares the whole screen after the client exits and the remain-on-exit line 'Pane is dead (status 0, <date>)' carries a wall-clock second the two sides straddle, zz always the later. Its sibling root-binding-detaches has the same channel. Normalise that timestamp in the comparison the way the fixtures normalise other unfixable values, keeping every other cell asserted, and prove the case still fails on a real one-sided change. Three gates have now paid for this.
6. Ledger: TUI-008 to review when compat/tui-mouse.sh has zero recorded cases and attached-client.sh is green. For any case you still cannot flip, evidence_note names the case, its cause and the file that owns the fix.
Proofs at tip: ${RUN_ENV} compat/tui-mouse.sh three times plus --self-check; compat/tui-stock-keys.sh plus --self-check; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/tui-copy-mode.sh; compat/tui-screen-diff.sh plus --self-check; the delta corpus for your touched commands; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const BATCH_CAPS = COMMON + `
YOUR BATCH: finish TUI-009. BRANCHNAME: tui-caps-3. WORKDIR: zz-tui-caps-8. START: origin/main. HARD BUDGET 300 MINUTES.
Zones, drawn around the obligation: crates/zz-daemon/src/client.rs, terminal_features.rs, and daemon.rs ONLY its client-roster and capability paths; crates/zz-protocol/src/message.rs (tail appends only) and lib.rs; crates/zz-tui/src/tty.rs, terminal_event.rs, and render.rs ONLY its glyph and cell-writing path; crates/zz-terminal/src/ palette and frame code; compat/tui-caps.sh; compat/tmux-gaps.json (options.client-terminal-negotiation, options.terminal-engine-limits, formats.terminal-cells, formats.terminal-runtime); compat/tui/ (TUI-009, evidence attempt-05). NOT yours: input.rs, app.rs, state.rs, chrome.rs and the mouse paths (the keys lane), the chooser surfaces and the command roster (the commands lane).
Read first: TUI-009's record, which carries every remaining row with its cause, and compat/tui/evidence/TUI-009/attempt-04/notes.md.
WHAT CYCLE 7 LEFT, measured, so you do not re-derive it. Fifteen rows record, in three causes:
- THIRTEEN ROWS, ONE CAUSE: client_colours on facts/bare, facts/-2, facts/-u, facts/-T and facts/utf8-locale, and client_termfeatures on those five plus silent/bare, silent/-T and silent/-2. The daemon derives a client's roster from its TERM, its COLORTERM and its flags alone, because ClientHello is sent before the terminal can answer and no message carries a later answer. The fix is the v102 wire append this obligation has been waiting three cycles for: a message carrying what a client learned from its terminal AFTER the hello, plus the terminfo-derived base set per TERM, landing in crates/zz-daemon/src/client.rs, terminal_features.rs and the daemon's client roster. All three are yours this cycle. That append is also what makes -2, -u and -T take effect, which is clause 2's own subject.
- widths/non-utf8/line: tty_check_codeset draws each non-ASCII cell as width underscores for a client without CLIENT_UTF8, mapping what it can to ACS first, while render.rs's glyph path writes the grapheme. The glyph path is yours; what was missing is the client's own UTF-8 flag, which lives in crates/zz-daemon/src/client.rs and was not exported where the colour count beside it is. That file is yours this cycle too.
- silent/extended pane_key_mode: the pin writes Eneks only when the option is on AND its terminal carries the extended-keys capability. Match the condition.
PUNCH LIST:
1. THE CAPABILITY WIRE. Land the v102 append and both halves: the client reports what its terminal answered after the hello, the daemon folds it with the terminfo base set for the TERM, and client_colours and client_termfeatures answer what the pin answers on all thirteen rows. Name the append in the v102 version history. Other clients must keep working: the GUI ignores the field.
2. THE UTF-8 FLAG AND THE GLYPH PATH. Export the client's UTF-8 capability where the raw TUI can read it, then make render.rs's glyph path draw a non-ASCII cell the way tty_check_codeset does for a client without it. Assert widths/non-utf8/line.
3. THE EXTENDED-KEYS CONDITION, and then every remaining recorded row in your three clauses: fix it, or state in evidence_note the exact reason and the file that owns it.
4. Ledger: TUI-009 to review when compat/tui-caps.sh has no recorded row inside any of its three clauses.
Proofs at tip: ${RUN_ENV} compat/tui-caps.sh three times plus --self-check; compat/tui-screen-diff.sh plus --self-check; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/status-row.sh under LC_ALL=C LC_TIME=C; compat/run.sh smoke/pane-colours-palette and the delta corpus for your touched commands; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const BATCH_COMMANDS = COMMON + `
YOUR BATCH: TUI-014, then TUI-016, then TUI-017's three text residues if budget remains. Landing TUI-014's first clause is what lets your gate verify TUI-006, so it comes first. BRANCHNAME: tui-commands. WORKDIR: zz-tui-commands. START: origin/main. HARD BUDGET 330 MINUTES. You are LAST in the gate order.
Zones, drawn around the obligations: crates/zz-protocol/src/catalog.rs and message.rs (tail appends only); crates/zz-mux/src/command.rs and compat_manifest_tests.rs; crates/zz-daemon/src/daemon.rs, chooser_presentation.rs, and its capture and show-messages paths; crates/zz/src/lib.rs; crates/zz-tui/src/ the chooser and mode-tree drawing only (the module that draws choose-tree today), and NOTHING else in that crate; crates/zz-terminal/src/ the capture path only; compat/tui-client-commands.sh, compat/tui-choosers.sh, compat/scenarios/; compat/tmux-gaps.json (commands.native-client-tools, clients.interactive-refresh, capture.rich-transports); compat/tui/ (TUI-014, TUI-016, TUI-017). NOT yours: TUI-006's and TUI-011's records (your gate writes TUI-006; TUI-011 waits on all five children), input.rs, app.rs, tty.rs, terminal_event.rs, render.rs.
Read first: the records of TUI-014, TUI-016 and TUI-017, each of which carries cycle 7's measurement and a next_action naming where to start, plus compat/tui/evidence/TUI-011/attempt-02/notes.md with the roster table, and client-tree-open in compat/tui-choosers.sh.
PUNCH LIST, in this order, and do not start an item before the one above it asserts:
1. CHOOSE-CLIENT, TUI-014's first clause. It is the single case compat/tui-choosers.sh still records, so TUI-006 verifies the moment it lands, and it has already slipped one cycle. The raw TUI must draw the pin's client mode: the row's key column, the client name, the '#{t/p:client_activity}: session #{session_name}' text, the preview of that client's current pane and the info preview. The record's next_action says to build the rows in the daemon and publish them through chooser_presentation.rs, which already carries rows, sort label, filter flag and style. Remove choose-client from catalog.rs's unimplemented list. Then run compat/tui-choosers.sh yourself, flip client-tree-open to asserted with a --self-check sabotage that fails for the right reason, and say in TUI-014's evidence_note that the flip is ready for the gate. Close command:choose-client in commands.native-client-tools with a dated measurement. Do NOT edit TUI-006's record.
2. THE REST OF TUI-014: clock-mode, customize-mode and switch-mode on the same mode surface, each compared whole-screen against the pin at 80x24 and after the mode ends, plus suspend-client and server-access compared exactly, either matching the pin or keeping the refusal with a measured reason. Clause 3 wants every entry's exact stdout, stderr and exit status in compat/tui-client-commands.sh and each closed command's item closed in its gap.
3. TUI-016, which its record calls the small one. Start with show-messages -T, since the daemon already knows each client's terminal features and only the pin's print shape is missing; then -J over the format job table; and tolerate -t the way the pin does. Clause 2 is a decision to record before code: what the server log prints for the client that ran a command, given a clientless CLI is client-<pid> on the pin and device-<n> on zz. Record it with the old behaviour, the pin's measured behaviour and the sentence "${DECIDED}", then either close the difference or register it with the measurement.
4. ONLY IF BUDGET REMAINS: TUI-017's clause 2, the three text residues its record says are worth more than the six rich flags and are the smaller change, because they live in the terminal worker's capture rather than in a new transport: the default range printing every visible row instead of stopping at the last written one, -N keeping trailing spaces to the pane edge, and -M falling back to the pane. Keep compat/scenarios/capture-pane.txt green. Leave clause 1's six rich flags alone; they are not this cycle's.
5. Ledger: each obligation you touch reaches review only when its own clauses assert with no recorded case inside them, and stays honest otherwise with the cause and the owning file named. TUI-011 is nobody's this cycle: it waits on all five children.
Proofs at tip: ${RUN_ENV} compat/tui-choosers.sh three times plus --self-check; compat/tui-client-commands.sh three times plus --self-check; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/tui-screen-diff.sh plus --self-check; the delta corpus for your touched commands plus every scenario naming one of them, capture-pane.txt included; cargo test and clippy per touched crate; cargo test -p zz; python3 compat/tui/tracker.py check and write-report, python3 compat/tmux-tracker.py check and write-report, python3 -B compat/tui/tracker_test.py, compat/check.sh.
`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz TUI parity campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what its gate trusts. NEVER push, commit, touch the board or GitHub issues, or edit ${M.root}. One other agent runs beside you; keep to the caps.
HOW YOU REPORT: your final act is the structured report. FOREGROUND ONLY, timeout at most 590000, never a background task, never end your turn to wait.
SETUP: GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'. REVIEWDIR exists and is clean: checkout --detach the branch tip there (if dirty, add ${M.dev}/REVIEWDIR-2). export CARGO_TARGET_DIR=WORKERTARGET (the worker's warm target; the worker is finished); touch crates/**/*.rs and Cargo.* first, never cargo clean, rebuild before spawning a binary. ${M.boxNote}
ETIQUETTE: every cargo command through the slot-and-cap wrapper in the box note (CAP 5G); cargo test -p <pkg> --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads}; throwaway pin servers -L zzprobe-$$ -f /dev/null only; never kill servers you did not start (${M.protected}); delete any binary copy you made under /tmp before reporting.
METHOD:
1. PUNCH LIST AUDIT: for each punch-list item in the batch below, is it done at the tip, with a fixture case that asserts the pin's behaviour and a sabotage that fails for the right reason? An item neither done nor honestly recorded as open is a blocker.
2. CONTRACT AUDIT: for the obligation, take each acceptance clause and find the ASSERTED evidence at the tip. A recorded case inside a clause keeps it open, except a SIBLING:<lane> case pointing at an EARLIER lane in the gate order (${ORDER.join(', ')}). An accepted gap is never evidence. The worker may write only status, evidence_note, next_action and sources of its obligation. ${OWNERSHIP}
3. DELTA CORPUS: run ${RUN_ENV} compat/run.sh --strict-geometry --delta origin/main...HEAD --commands <the lane's touched_commands> yourself, in sequential chunks under your Bash cap. The TUI fixtures do not cover the corpus and cycle 6 twice landed a presentation change that a corpus row contradicted; you are the first reader who can catch it cheaply.
4. PROOFS AT TIP: run the claimed proofs yourself with a zz built at the tip and the pin ${PIN}; the touched crates' tests and cargo test -p zz. Any red at tip is a blocker (load-flake rule applies).
5. ORACLE SPOT-CHECKS: drive the pin yourself for every behaviour the lane changed and confirm the fixture asserts what the pin does. Every wait a bounded wait_for. Run the fixture and confirm it reaps what it starts.
6. TEST HONESTY: run every new or flipped case's sabotage, add one of your own, confirm evidence files are real runs, no .log files, git check-ignore -v clean.
7. INVARIANTS: zones, judged on git diff origin/main...HEAD (three dots: main may have moved under the lane, the merge-base is the base); the wire rule (101 shipped in 0.8.0, 102 after the caps gate, never above 102; pure appends, consumer half present, the v102 history line; read the wire diff hunk by hunk); the GUI builds with no presentation change; no added code comments; no attribution trailers; both trackers green; generated reports regenerated.
CALIBRATION: confirmed_defects only with proof; suspicion goes in notes. blocker = a claimed clause without real evidence, a fixture that cannot fail, a zone or wire violation, a red at tip; must-fix = the gate applies before merge; nit = mention. VERDICT approve / approve-with-fixes / reject; make every blocker precise (probe, expected bytes, file). Fill every_clause_asserted honestly: the gate will not verify an obligation you mark no. Bounded: well under 90 minutes after the build.
`


const REVIEW_EXTRA = {
  keys: 'LANE-SPECIFIC: run list-keys -T root and -T copy-mode and -T copy-mode-vi on both sides and diff the rows; then TYPE real SGR reports yourself for every stock binding the lane claims to have installed and confirm the command ran with the pin\'s target and the pin\'s screen, not merely that a binding exists; drive a double and a triple click inside and outside the pin\'s interval; drag a border on both sides; read every mouse_* format through a bound command; paste under copy mode and under a menu; toggle focus-events off and drive focus in. Confirm the GUI sends no mouse key and its presentation is unchanged.',
  caps: 'LANE-SPECIFIC: attach on both sides under TERM=xterm, TERM=xterm-256color and a silent terminal, bare and with -2, -u and -T, and compare client_colours, client_termfeatures, client_utf8 and client_flags on every combination; print non-ASCII, wide and combining text to a client without the UTF-8 flag and compare the drawn cells; check the v102 append is a tail append with its consumer half present and the GUI ignoring it; confirm ClientHello ordering still works when the terminal never answers.',
  commands: 'LANE-SPECIFIC: run choose-client on both sides and compare the whole screen, its keys and its exit; confirm client-tree-open now asserts and its sabotage fails for the right reason; run every roster entry yourself on both sides attached and compare stdout, stderr, exit and screen; read each new child obligation against the tracker rules (all fields, dependency direction, never in the frozen baseline) and confirm the parent depends on them; confirm no entry was dropped between the header table and the fixture.',
}

const GATE_EXTRA = {
  keys: `TUI-008 verifies only if compat/tui-mouse.sh has zero recorded cases inside its three clauses and attached-client.sh is green at your tip. It feeds TUI-012, whose flip belongs to the LAST gate of this cycle: if you verify TUI-008, say so in your board note so the commands gate can rely on it.`,
  caps: `TUI-009 verifies only if compat/tui-caps.sh has no recorded row inside any of its three clauses. Read the wire diff hunk by hunk: this lane adds a v102 append and the keys lane may have added one before you, so the version-history entry merges by union and PROTOCOL_VERSION stays 102. TUI-009 also feeds TUI-012, whose flip belongs to the last gate.`,
  commands: `THREE THINGS TURN ON YOU, IN THIS ORDER. (1) TUI-006 IS YOURS TO VERIFY, and it is the point of this lane. Its dependencies TUI-002, TUI-003 and TUI-004 are verified on main, and its one recorded case, client-tree-open, closes when choose-client lands. If compat/tui-choosers.sh runs three times plus --self-check green at your tip with zero recorded cases, write TUI-006's evidence, fill its proof block with your tip and those commands, and set it verified. If any chooser case still records, leave TUI-006 alone and name the case. (2) TUI-014, TUI-016 and TUI-017 are added-scope ids, so they move the added count and never the frozen baseline of twelve; verify each only if every one of its own clauses asserts with no recorded case inside it, and check the generated report moves the right counter. TUI-011 is not yours and stays at review: it waits on all five children, TUI-015 and TUI-018 included. (3) TUI-012 IS THE LAST GATE'S, which is you: its proof block was filled in cycle 7 and its status is review, held on TUI-008 and TUI-009. Read the ledger on origin/main after you fetch. If both are verified there, re-run compat/tui-superset.sh three times plus --self-check at your tip, append those commands to TUI-012's proof, set its proof.revision to your tip and set it verified. If either is still open, leave TUI-012 at review and name what holds it.`,
}

const LANES = [
  { key: 'keys', prompt: BATCH_KEYS, start: 'origin/main', workdir: 'zz-tui-keys-8', reviewdir: 'zz-tui-keys-8-review', target: `${M.dev}/zz-tui-keys-8/target`, extra: REVIEW_EXTRA.keys, gateExtra: GATE_EXTRA.keys },
  { key: 'caps', prompt: BATCH_CAPS, start: 'origin/main', workdir: 'zz-tui-caps-8', reviewdir: 'zz-tui-caps-8-review', target: `${M.dev}/zz-tui-caps-8/target`, extra: REVIEW_EXTRA.caps, gateExtra: GATE_EXTRA.caps },
  { key: 'commands', prompt: BATCH_COMMANDS, start: 'origin/main', workdir: 'zz-tui-commands', reviewdir: 'zz-tui-commands-review', target: `${M.dev}/zz-tui-commands/target`, extra: REVIEW_EXTRA.commands, gateExtra: GATE_EXTRA.commands },
]
const OPTS = { model: 'opus', effort: 'xhigh' }

const SLOTS = 3
let free = SLOTS
const gateWaiters = []
const laneWaiters = []
function acquire(gate) {
  return new Promise(res => {
    if (free > 0) { free--; res() } else (gate ? gateWaiters : laneWaiters).push(res)
  })
}
function release() {
  const next = gateWaiters.shift() || laneWaiters.shift()
  if (next) next(); else free++
}
async function withSlot(gate, fn) {
  await acquire(gate)
  try { return await fn() } finally { release() }
}

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
  const worker = await withSlot(false, () => agent(lane.prompt, { label: `worker:${lane.key}`, phase: 'Work', schema: WORKER_SCHEMA, ...OPTS }))
  let r = { lane, worker }
  if (!worker || !worker.branch) return r
  r.review = await withSlot(false, () => agent(reviewPrompt(r, false), { label: `review:${lane.key}`, phase: 'Review', schema: REVIEW_SCHEMA, ...OPTS }))
  if (!r.review || r.review.verdict !== 'reject') return r
  log(`${lane.key}: review REJECT, running one fix pass`)
  const fix = await withSlot(false, () => agent(fixPrompt(r), { label: `fix:${lane.key}`, phase: 'Review', schema: WORKER_SCHEMA, ...OPTS }))
  if (!fix || !fix.branch) return r
  r = { ...r, firstWorker: worker, firstReview: r.review, worker: fix, fixed: true }
  r.review = await withSlot(false, () => agent(reviewPrompt(r, true), { label: `rereview:${lane.key}`, phase: 'Review', schema: REVIEW_SCHEMA, ...OPTS }))
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
BOARD: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. claim MAIN --lease 3h before you rebase; renew MAIN --lease 2h before any long stage; after the push: note ${LOCK} --note "<lane> integrated at <sha>: verified <ids or none>; open <what and why>; review <verdict> and what you did"; then release MAIN --reason "cycle 6 <lane> pushed at <sha>". If MAIN is held by someone other than ${M.holder}, wait for it with bounded polls (at most 30 minutes), then report it as a problem and stop. BOARD FALLBACK: on a GitHub auth failure append the command to ${M.root}/compat/tui/board-replay-7.sh (uncommitted) and continue.
${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. Never use ${M.root}'s local main, never edit, stash or reset in it. Commits: git -c commit.gpgsign=false commit.
THIS BOX: ${M.boxNote} Gate worktree: ${M.dev}/zz-gate-tui7 (create it if missing: git -C ${M.root} worktree add --detach ${M.dev}/zz-gate-tui7 origin/main; if it exists and is clean, reuse it; never remove it, the next gate reuses it). export CARGO_TARGET_DIR=${M.dev}/zz-gate-target in every shell that runs cargo, compat/check.sh included; touch crates/**/*.rs and Cargo.* after every checkout. Every cargo command goes through the slot-and-cap wrapper in the box note with CAP 7G; the one other working agent may hold the other slot.
${r.lane.gateExtra || ''}
STAGES:
1. Fetch; git merge-tree --write-tree origin/main <tip> to predict conflicts; check out the lane tip on a local branch gate-${r.lane.key} (delete a stale local branch of that name first, in ${M.root} too) and rebase onto origin/main. Conflicts: generated reports regenerate; campaign.json and tmux-gaps.json merge by record and by item; render.rs, state.rs and input.rs merge by union; TMUX_OPTION_CONSUMERS and compat_manifest_tests.rs counts by union recomputed; PROTOCOL_VERSION ends at 102 and the v102 history entry keeps every line. A conflict union cannot settle inside files the lane does not own => push campaign/<branch>-gated, report, stop.
2. cargo test -p <each touched package> and cargo test -p zz --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (exit code, never piped); cargo clippy --workspace --all-targets --all-features --jobs ${M.gateJobs} -- -D warnings; ${RUN_ENV} compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <touched_commands>, in sequential chunks. CORPUS SCOPE, changed for cycle 7 to stop paying for the same rows at every gate: only the LAST gate in the order (${ORDER[ORDER.length - 1]}) also runs every keys and status scenario and smoke/tui-client-input-backpressure. An earlier gate runs the delta for its own touched commands and nothing more, so a row that only a later lane could break is caught once, at the end, rather than three times. Red that is not a load flake and not a known environmental row: fix if minutes, else push campaign/<branch>-gated, report, stop.
3. Build zz (cargo build -p zz --jobs ${M.gateJobs}) and run with the pin: compat/tui-screen-diff.sh plus --self-check, compat/tui-pane-geometry.sh, compat/tui-stock-keys.sh, compat/status-row.sh (LC_ALL=C LC_TIME=C), compat/tui-indicators.sh, compat/tui-copy-mode.sh, compat/tui-caps.sh, compat/tui-overlays.sh, compat/tui-choosers.sh, each with --self-check where it has one, and compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<the build>). Every one must exit 0 except known flakes rerun solo; a red that an earlier lane's merge would explain is still this lane's to settle before pushing.
4. SIBLING FLIPS: for this lane's sibling_cases whose owner is on main, flip each case to asserted and run its fixture plus --self-check; keep a flip only if both exit 0, otherwise revert that flip and append the failing channel to its reason.
5. RECORDS for this lane's obligation, in one commit: if every clause asserts (VERIFIED MEANS EVERY CLAUSE), its dependencies are verified on main, and stage 3 is green: write compat/tui/evidence/<ID>/<attempt>/review.md (the reviewer's verdict JSON verbatim, checks_run, your review_actions, the flips), fill the proof block (revision = this lane's final pre-records tip, tmux_commit = the pin, environment = one line plus a pointer to environment.txt, commands with exit codes, artifacts, review) and set verified with next_action naming what it unlocks (TUI-004 lets TUI-006 and TUI-007 verify; TUI-005 and TUI-007 feed TUI-008; TUI-006 and TUI-007 feed TUI-011; TUI-009 and TUI-008 feed TUI-012). ALSO: if this lane's merge makes an EARLIER-held obligation meet the bar (for example TUI-006 or TUI-007 held only by TUI-004, now verified), verify it here with its own stage-3 runs cited. Otherwise the status stays honest with the reason appended. Then both trackers' write-report and check, python3 -B compat/tui/tracker_test.py, compat/check.sh.
6. Push main (git push origin HEAD:main). Non-fast-forward: fetch, rebase, rerun stage 3's fixtures, push. Never force. Then the board note and MAIN release.
7. Sweep: pgrep -fa 'zz-cli-|zz-user|zzprobe'; reap only what this gate started; delete any binary copies you made under /tmp. Leave ${M.dev}/zz-gate-tui7 in place.
Report: branch, merged, pushed_sha, verified ids, sibling_flips, gate_summary, review_actions, flakes, fixtures (exit code and last line of each at the pushed tip), board_updates, problems.`
}



log(`TUI cycle 8: ${ORDER.join(', ')}; three agents at a time; a gate takes the next free slot before a waiting lane`)
const gates = []
const gateResolve = {}
const gateDone = {}
for (const l of LANES) gateDone[l.key] = new Promise(res => { gateResolve[l.key] = res })

async function chain(lane, i) {
  let r
  try { r = await runLane(lane) } catch (e) { r = { lane, worker: null, error: String(e) } }
  if (i > 0) await gateDone[LANES[i - 1].key]
  let g
  if (!r || !r.worker || !r.worker.branch) {
    g = { key: lane.key, skipped: (r && r.error) || 'no branch pushed' }
  } else {
    await acquire(true)
    try {
      const out = await agent(gatePrompt(r, gates), { label: `gate:${lane.key}`, phase: 'Integrate', schema: GATE_SCHEMA, ...OPTS })
      g = { key: lane.key, ...(out || { problems: 'gate agent returned nothing' }) }
      log(`gate:${lane.key} -> merged=${out && out.merged} pushed=${out && out.pushed_sha} verified=${out && JSON.stringify(out.verified)}`)
    } catch (e) {
      g = { key: lane.key, problems: `gate failed: ${String(e)}` }
    }
    gates.push(g)
    gateResolve[lane.key](g)
    for (let k = 0; k < 8; k++) await Promise.resolve()
    release()
    return r
  }
  gates.push(g)
  gateResolve[lane.key](g)
  return r
}

const results = await Promise.all(LANES.map((l, i) => chain(l, i)))
return { lanes: results.map(r => ({ key: r.lane.key, branch: r.worker && r.worker.branch, review: r.review && r.review.verdict, error: r.error || null })), gates }

