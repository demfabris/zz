export const meta = {
  name: 'tui-run-9b',
  description: "TUI parity cycle 9, second half: one Opus 5 lane at xhigh finishing TUI-008's last three recorded mouse checks on top of the mouse branch, then its reviewer and a gate that verifies TUI-008 and flips TUI-012",
  phases: [
    { title: 'Work', detail: "one worktree from campaign/tui-mouse: the two pointer menus, the paste under a menu, and the three mouse_* formats they read" },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh, adversarial; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: "one gate: rebase onto the main the cycle 9 gates pushed, run every proof, verify TUI-008 and then TUI-012" },
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
  boxNote: A.boxNote || "This box (alienware) is LINUX: CachyOS (Arch-based), 16 cores, 15 GB RAM plus 15 GB zram swap; /bin/bash is 5.x; the filesystem is btrfs; /opt/homebrew does not exist, so the PATH=/opt/homebrew/bin:$PATH prefix this prompt carries is harmless. Where a prompt says sw_vers, use uname -a plus head -2 /etc/os-release for the OS line. The zz daemon ring log lands under the scrubbed HOME at .local/state/zz/logs/. The compat caches are populated and cycles 1, 3, 4 and 5 ran here: every TUI fixture is known green at origin/main EXCEPT compat/status-row.sh under the box locale (LC_TIME=pt_BR.UTF-8 changes %b; the LC_ALL=C LC_TIME=C control exits 0), and five corpus rows are environmental here (micro-flags, show-options-hooks, lane2-store, smoke/plugin-runtime-resurrect-restore, and smoke/status-background-jobs, which the cycle 8 keys gate measured at origin/main nine times and found 2 green to 7 red in both failure spellings: it is a timing-sensitive #(date +%s%N) status job, red at baseline on this box, so it is never a lane's). Slow corpus rows that pass alone: command-item-format (up to 8 minutes), command-prompt-editing (about 4 minutes), copy-mode-stock-action-keys. A red row or fixture at your tip is yours only if it is green at origin/main on this box. Known fixture fragility: compat/tui-stock-keys.sh root-binding-detaches can flake on a wall-clock second boundary and its recorded count wobbles; tui-screen-diff.sh --self-check exited 2 once ('zz refused status-right') and passed solo. zz-daemon client_focus_closes_display_panes_and_preserves_chooser_modes fails about 1 run in 10 even exact-solo at BASE: the modes lane owns it this cycle, so elsewhere a single red there reruns and is named as this known flake, not chased. A single Bash call is capped at 600 seconds and a longer command is moved to the background and killed: pass timeout of at most 590000 and split long work. A cargo debug build of zz is NOT bit-reproducible here: a hash identifies an artifact, the revision plus a clean worktree attests it. The box has a real ~/.tmux.conf and ~/.config/zz/mux.conf: never read or edit them and never start a server that would load them (scrub HOME and XDG_CONFIG_HOME, both; probes use -f /dev/null). Never write 'rm -rf $HOME' or 'rm -rf ~'; put a scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz). HYGIENE, measured in cycle 5: agents left about 5 GB of zz binary copies under /tmp, which is a RAM-backed tmpfs, and filled the zram swap; a cargo test killed by a timeout leaves /tmp/zz-cli-*/ daemons running for hours; a scrubbed environment without XDG_RUNTIME_DIR autostarts a daemon on /tmp/zz-user/default.sock. So: copy a binary under /tmp only when a comparison needs a frozen copy, delete every copy before your final report, and at the end run pgrep -fa 'zz-cli-|zz-user|zzprobe' and reap only pids whose command line or environment names a socket or scratch HOME you created. After switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first, and rebuild before spawning a binary. Every git commit is 'git -c commit.gpgsign=false commit'. MEMORY, measured 2026-09-11: the first run of this cycle drove the box out of memory about twenty minutes in, with five lanes compiling and testing at once (four rustc, a 1.5 GB zz-daemon test binary, three cargo), and the desktop stayed unusable for hours. So EVERY cargo command on this box goes through a shared two-slot lock and a per-command memory cap, exactly like this: S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=CAP -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo <args> (CAP is 5G for lanes and reviewers, 7G for a gate). fabrico's rule for this box since the OOM: at most two agents work at a time, and the runner enforces it; the two lock slots keep at most two cargo commands running even so. A command that waits more than 540 seconds for a slot exits 1 without running: run it again. A command that hits its cap is killed with exit 137 instead of taking the box down: run it again with fewer --jobs or --test-threads. Neither is a test result. Fixtures and probes need no wrapper, but run one at a time per lane.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz) through the gh credential helper; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands.',
}
log(`TUI cycle 9 second half on ${M.machine}; one lane from campaign/tui-mouse, one agent at a time beside the cycle 9 runner, --jobs ${M.workerJobs}; its gate at --jobs ${M.gateJobs}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`



const ORDER = ['menus']
const LOCK = 'F-TUI-CYCLE-9-LANES'
const OWNERSHIP = `LEDGER OWNERSHIP (compat/tui/campaign.json records; edit only yours): you own TUI-008 alone, evidence attempt-04. TUI-012's proof block is filled and its status is review, held on TUI-008: you do not touch it and YOUR GATE flips it once TUI-008 verifies. TUI-011 is nobody's, it waits on TUI-014 through TUI-018. TUI-006, TUI-014, TUI-016 and TUI-017 belong to the three lanes of cycle 9 running beside you; their gates push main while you work. Everything else and every earlier attempt is read-only history, as are the frozen twelve-id baseline, the milestones and the pin. In compat/tmux-gaps.json you may close ONLY the items your own landing makes the raw TUI honour, inside keys.root-native-mouse, mouse.bound-context and formats.mouse-context, each with a dated measurement (TMUX_OPTION_CONSUMERS and the compat_manifest_tests.rs partition move in the same commit); an accepted gap keeps its decision for the GUI and for every item not closed.`

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
- ITERATE BEHIND A FILTER. Measured in cycle 8: a lane checked one count assertion by running a whole
  package suite (cargo test -p zz-daemon is 893 tests) behind a memory cap and two shared cargo slots,
  so a single edit-check loop cost minutes and a lane spent its whole budget on one punch item. Your
  edit-check loop is cargo test -p <pkg> --lib <test name> (add --exact when the name is unique), or
  the one integration binary that holds the test. Run the whole package ONCE, before the commit that
  closes an item, and the workspace never. The same applies to fixtures: while you are iterating on a
  case, drive that case's probe directly rather than the whole fixture, and run the fixture three
  times only when the item is done.
- FOUND SOMETHING NEW? If you find a divergence inside your obligation that no punch-list item names, fix it if it fits your zones and budget; if it does not, record it as a fixture case with its measurement and leave the obligation at active. Do not hide it and do not stop the punch list for it.

HOW YOU REPORT
- Your final act is the structured report the output schema describes. A worker that ends its turn without it is a failed agent and its lane is dropped.
- FOREGROUND ONLY: every command in the foreground, timeout at most 590000; never Monitor, run_in_background or any background task; never end your turn to wait.
- Check 'date' at the start of each item. The batch's HARD BUDGET is a ceiling. When it is spent, stop, write what asserts now and what remains into evidence_note, set the status honestly, push and report.

SETUP
- The shared checkout is ${M.root}. Read it and add worktrees from it; NEVER edit, stash, reset or clean it. Its local main branch is stale: use origin/main. knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report, never hand-merge them.
- ${M.gitNote} Fetch with GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME (never force, never main).
- YOUR BASE IS A CAMPAIGN BRANCH, NOT MAIN. Your batch names it. Read what is verified with python3 compat/tui/tracker.py check and the ledger itself; do not trust a remembered count. Three gates of this cycle push main WHILE YOU WORK, so fetch often and diff your work with git diff <your base>...HEAD (three dots, the merge-base), never two. Your batch says exactly when to rebase and onto what.
- Worktree: your WORKDIR under ${M.dev} was pre-positioned by the orchestrator at the base your batch names, clean. Confirm with git log -1 and git status --short. Do NOT rebase at the start: your base is deliberate and your batch says when to move. Build through the wrapper (CAP 5G): S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=5G -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo build -p zz --jobs ${M.workerJobs} > build.log 2>&1, check the exit code.
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
- YOUR CLAIM IS RE-MEASURED. compat/check.sh now runs compat/tui/verify-claims.py, and a gate can run it with --run to execute an obligation's fixture and read the tally the fixture prints about itself. Cycle 7 and cycle 8 each had a lane report a clause proved while its own fixture still recorded cases against that obligation, and both times a reviewer caught it by hand. Count the recorded cases in your own fixture output before you write a status, and quote the fixture's summary line in evidence_note.
- Evidence at compat/tui/evidence/<ID>/<attempt>/: environment.txt first, each run's output as .txt, notes.md naming every file. Never name a file .log (globally ignored); run git check-ignore -v on the directory before committing. Real runs only.
- python3 compat/tui/tracker.py check before each commit; write-report in the same commit. If you touched compat/tmux-gaps.json, python3 compat/tmux-tracker.py check and write-report too. JSON via json.dump(..., indent=2) plus a trailing newline.
- Decisions not yours to reopen: width never invokes the sidebar; the raw TUI's status row uses the pin's default theme colours until a user sets theme options; the daemon loads only zz/mux.conf, and explicit -f files replace it in argument order (fabrico changed this on 2026-09-11 in 268ccd9d; the old 'layers last even under -f' rule is dead, see knowledge/configuration/app-config.md); the shell-integration pane title; the armed-prefix hint record (TUI-004).

WIRE PROTOCOL RULE: PROTOCOL_VERSION is 102 on main and STAYS 102 this cycle; 102 is unreleased (101 shipped in zz 0.8.0), so every append of this cycle folds into it. Pure appends only (a trailing field or a new variant at the end), the consumer half in the same push, and a line naming the append in the v102 entry of knowledge/protocol/wire-protocol.md's version history. Two lanes append to crates/zz-protocol/src/message.rs this cycle: append at the tail only, never renumber, and expect the gate to merge the version-history entry by union.

CODE BOUNDARIES: three lanes touch crates/zz-tui/src and the split is by FILE, named in each batch; never edit a file another lane owns, even for a one-line hook, and never reflow or reorder code you do not change. Keep new surfaces in their modules (copy_view, overlay, chooser and so on), minimal hooks in render.rs, state.rs and input.rs, never reflow or reorder code you do not change, and name every render.rs hunk by function in notes. The GUI keeps compiling with no presentation change.

MACHINE ETIQUETTE: every cargo command through the slot-and-cap wrapper in the box note (CAP 5G); cargo build/test --jobs ${M.workerJobs}, -- --test-threads=${M.workerThreads}; never workspace-scale; cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features -- -D warnings per touched crate. If the diff touches crates/, cargo test -p zz (cli_binary) before the last commit, split by test filters across calls if one call cannot hold it. Never pipe cargo test through tail or grep. Load-flake rule: fails loaded + passes exact-solo = flake.

DELIVERY: one commit per punch-list item where feasible, subject in repo style, git -c commit.gpgsign=false, NO attribution trailers, NO comments in code (a fixture's header block is documentation). Every case you flip or add has a --self-check sabotage that fails for the right reason. After your LAST commit re-run every proof you list. Push campaign/BRANCHNAME and report with the final tip sha. NO GitHub comments, NO issue 7, NO compat/board.py mutations.
`




const BATCH_MOUSE = COMMON + `
YOUR BATCH: finish TUI-008. BRANCHNAME: tui-mouse. WORKDIR: zz-tui-mouse. START: origin/main. HARD BUDGET 300 MINUTES. You are FIRST in the gate order, and TUI-012's verification waits on your landing.
Zones, drawn around the obligation: crates/zz-mux/src/formats.rs and command.rs; crates/zz-tui/src/input.rs, app.rs and state.rs; crates/zz-client/src/menu.rs, chrome.rs and core.rs; crates/zz-protocol/src/key.rs, catalog.rs and message.rs (tail appends only); crates/zz-terminal/src/session.rs and interaction.rs; crates/zz-daemon/src/daemon.rs, its mouse, paste and key-injection paths; compat/tui-mouse.sh, compat/tui-stock-keys.sh, compat/attached-client.sh; compat/tmux-gaps.json (keys.root-native-mouse, keys.copy-mode-native-mouse, mouse.bound-context, formats.mouse-context); compat/tui/ (TUI-008, evidence attempt-03). NOT yours: render.rs and the chooser surfaces (the choosers lane), the capture and show-messages paths (the introspection lane).
Read first: TUI-008's record. Its next_action names the two unlocks and the exact files, measured by the cycle 8 gate, so do not re-derive them:
1. send-keys -M must hand the invoking event to the pane the way the pin's input_key_pane does, AND #{mouse_any_flag} must answer the pane's own mode against ALL_MOUSE_MODES instead of the constant zero it is today in crates/zz-mux/src/formats.rs. Every pane-body row guards its send -M branch on that flag, so the two together install WheelUpPane, MouseDown1Pane, MouseDown2Pane and MouseDrag1Pane and flip wheel-up-pane.
2. A copy-table mouse name must be reachable from a pointer: crates/zz-tui/src/app.rs mouse_binding_names reads the root table only, so the client must also offer the names the copy tables bind while a pane is in copy mode. That is what drag-selects/buffer and the multi-click cases wait on, together with the pin's SecondClick, TripleClick and 300ms DoubleClick sequence plus copy-mode -H and send -X select-word.
PUNCH LIST:
1. The two unlocks above, in that order, each with its fixture cases flipped from recorded to asserted and a --self-check sabotage that fails for the right reason.
2. The remaining recorded checks the gate counted at ten: the multi-click sequence, the right-click window menu on the status row, and paste-under-menu, where the pin's overlay eats the paste key and hands the tail to the pane as ordinary keys while the raw TUI hands it over as a fresh bracketed paste (crates/zz-client/src/menu.rs and crates/zz-tui/src/input.rs).
3. Clause 3: thirty-four gap items stay accepted across the four mouse gaps. Close exactly the ones your landing makes the raw TUI honour, each with a dated measurement, and say in evidence_note which stay and why.
4. Ledger: TUI-008 to review when compat/tui-mouse.sh has zero recorded cases and attached-client.sh is green. Quote the fixture's own summary line in evidence_note.
Proofs at tip: ${RUN_ENV} compat/tui-mouse.sh three times plus --self-check; compat/tui-stock-keys.sh plus --self-check; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/tui-copy-mode.sh; the delta corpus for your touched commands; cargo test and clippy per touched crate; cargo test -p zz; tracker checks and compat/check.sh.
`

const BATCH_CHOOSERS = COMMON + `
YOUR BATCH: close TUI-006 and TUI-014. BRANCHNAME: tui-choosers-3. WORKDIR: zz-tui-choosers-9. START: origin/main. HARD BUDGET 330 MINUTES.
Zones, drawn around the obligations: crates/zz-tui/src/render.rs (paint_chooser, the chooser mode tree and the prompt row) and the chooser module; crates/zz-mux/src/command.rs (command-prompt and its -P form, choose-* commands); crates/zz-protocol/src/message.rs (tail appends only) and catalog.rs; crates/zz-daemon/src/daemon.rs and chooser_presentation.rs; crates/zz-terminal/src/session.rs, its copy-mode search-mark rules; compat/tui-choosers.sh, compat/tui-client-commands.sh, compat/tui-copy-mode.sh; compat/tmux-gaps.json (commands.native-client-tools, clients.interactive-refresh, clients.tui-command-output-navigation, clients.command-output-pane-prompt); compat/tui/ (TUI-006 attempt-03, TUI-014 attempt-02). NOT yours: input.rs, app.rs and the mouse paths (the mouse lane), the capture and show-messages paths (the introspection lane).
Read first: TUI-006's record, whose next_action states plainly that its three dependencies are verified and are NOT the blocker, only its recorded cases are; and TUI-014's, corrected at the cycle 8 gate.
WHAT IS LEFT, measured, so you do not re-derive it:
- TUI-006 records two causes across three cases. output-search-prompt and output-search-typed: both stock search bindings are command-prompt -P, which in the pin is window_pane_set_prompt drawn by redraw_draw_pane_prompt over the pane's LAST row (its first under status-position top), leaving the status row alone; zz raises the same prompt on the client and draws it on the status row, so the pin's row 22 carries '(search down) ' with the status row kept and zz's row 23 carries it instead. Closing it needs -P and the pane it targets carried on the wire plus a prompt row painted over that pane, and it moves real copy mode's prompt too, which is TUI-005, VERIFIED: re-run compat/tui-copy-mode.sh and keep it at zero recorded, or the landing is a regression. output-selected: window_copy_command clears data->searchmark for every command that is not search-* unless its clear column says never, so begin-selection and cursor-right drop the marks and the pin paints only the selection, while zz keeps the current-match cell painted under it; zz-terminal already carries the rule as CopyModeAction::clears_search_marks for a pane's copy mode and the retained command output does not reach it.
- TUI-014 clause 1 holds the info preview. The cycle 8 reviewer drove both sides at 80x24 with prefix D then i and found the view label, the acs separator, Terminal Type, the t/r time and twelve missing info lines all differ, and compat/tui-choosers.sh never presses i on both sides: the only place i appears is a --self-check sabotage that presses it on the PIN alone, so the fixture cannot catch a wrong zz info view. Add the two-sided case first, then fix what it shows.
- TUI-014 clause 2 is untouched: clock-mode, customize-mode, switch-mode, suspend-client and server-access are all still in UNIMPLEMENTED_TMUX_COMMANDS and answer 'unsupported command'. Implement the three mode tools on the same mode surface choose-client now uses, each compared whole-screen at 80x24 and after the mode ends; for suspend-client and server-access either match the pin exactly or keep the refusal with a measured reason.
PUNCH LIST, in this order:
1. TUI-006's three recorded cases. This closes a baseline obligation, so it comes before TUI-014's remainder.
2. TUI-014 clause 1: the two-sided info-preview case, then the divergences it exposes.
3. TUI-014 clause 2 and clause 3, closing each command's item in commands.native-client-tools with a dated measurement.
4. Ledger: TUI-006 to review when compat/tui-choosers.sh records nothing; TUI-014 likewise, with tui-client-commands.sh clean. Quote each fixture's own summary line in evidence_note.
Proofs at tip: ${RUN_ENV} compat/tui-choosers.sh three times plus --self-check; compat/tui-client-commands.sh three times plus --self-check; compat/tui-copy-mode.sh plus --self-check (it must stay at zero recorded: TUI-005 is verified); compat/tui-screen-diff.sh; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); the delta corpus for your touched commands; cargo test and clippy per touched crate; cargo test -p zz; tracker checks and compat/check.sh.
`

const BATCH_INTROSPECTION = COMMON + `
YOUR BATCH: TUI-016, then TUI-017's clause 2. BRANCHNAME: tui-introspection. WORKDIR: zz-tui-introspection. START: origin/main. HARD BUDGET 300 MINUTES. You are LAST in the gate order, so your gate also carries TUI-012's flip.
Zones, drawn around the obligations: crates/zz-daemon/src/daemon.rs (show-messages, the format job table, the client roster and the capture paths); crates/zz-terminal/src/session.rs and its capture path; crates/zz-protocol/src/catalog.rs and message.rs (tail appends only); crates/zz-mux/src/command.rs (capture-pane and show-messages only); compat/tui-client-commands.sh (only its show-messages and capture-pane cases), compat/scenarios/capture-pane.txt and the scenarios your commands touch; compat/tmux-gaps.json (clients.interactive-refresh, capture.rich-transports); compat/tui/ (TUI-016 and TUI-017, evidence attempt-01 each). NOT yours: crates/zz-tui/src/ entirely (the mouse and choosers lanes own it) - if a divergence needs a raw-TUI change, record it with its cause and the owning file.
Read first: the records of TUI-016 and TUI-017, each of which carries a next_action naming where to start.
PUNCH LIST:
1. TUI-016 clause 1, starting with show-messages -T, because the daemon already knows each client's terminal features and the only missing half is the pin's print shape; then -J over the format job table; and tolerate -t the way the pin does. Compare exact stdout, stderr and exit status on both sides.
2. TUI-016 clause 2 is a decision to record BEFORE code: what the server log prints for the client that ran a command, given a clientless CLI is client-<pid> on the pin and device-<n> on zz. Write it into evidence_note with the old behaviour, the pin's measured behaviour and the sentence "${DECIDED}", then either close the difference or register it with the measurement.
3. TUI-017 clause 2, the three text residues its record calls worth more than the six rich flags and a smaller change, because they live in the terminal worker's capture rather than in a new transport: the default range printing every visible row instead of stopping at the last written one, -N keeping trailing spaces to the pane edge, and -M falling back to the pane. Keep compat/scenarios/capture-pane.txt green; its explicit ranges already pass on both sides. Leave clause 1's six rich transports alone, they are not this cycle's.
4. Ledger: TUI-016 to review when both its clauses assert; TUI-017 stays active with clause 1 open and clause 2 closed, its evidence_note saying exactly that. Quote each fixture's own summary line.
Proofs at tip: ${RUN_ENV} compat/tui-client-commands.sh three times plus --self-check; compat/run.sh --strict-geometry capture-pane display-message and the delta corpus for your touched commands; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); cargo test and clippy per touched crate; cargo test -p zz; tracker checks and compat/check.sh.
`


const BATCH_MENUS = COMMON + `
YOUR BATCH: finish TUI-008's last three recorded checks. BRANCHNAME: tui-mouse-menus. WORKDIR: zz-tui-menus-9. START: the tip of origin/campaign/tui-mouse at 142139fd, which your worktree already holds. HARD BUDGET 240 MINUTES.
YOUR TARGET DIRECTORY IS WARM AND IT IS NOT IN YOUR WORKTREE: export CARGO_TARGET_DIR=${M.dev}/zz-tui-introspection/target in every shell that runs cargo, compat/check.sh included. That lane finished this morning off the same base and nothing is using its target; a cold build here costs you half an hour and 150 GB. Touch your worktree's crates/**/*.rs and Cargo.* before your first build, never cargo clean, and rebuild before you spawn a binary.

WHY YOU EXIST. Earlier today the mouse lane of this cycle landed both of TUI-008's unlocks and the pin's multi-click sequence, then stopped on budget with three checks still recorded, and left an exact handover. You are its second half, launched into idle capacity while its reviewer runs, so that TUI-008 and the TUI-012 waiting on it close in this cycle instead of slipping to the next. Its measurements are quoted below. Treat them as measurements, not as claims: run ${RUN_ENV} compat/tui-mouse.sh yourself before you change anything, read its own summary line, and start from what you see.

WHAT IT LEFT, in its words, with the files it named:
1. status-clicks/right-click-screen records. The pin's MouseDown3Status raises DEFAULT_WINDOW_MENU positioned with -x W and -y W. crates/zz-daemon/src/daemon.rs popup_position maps -x M, -y M, -x W and -y W to popup_mouse_centre_x, popup_mouse_top and popup_window_status_line_x, and all of those answer columns/2 and rows/2 today, so the raw TUI puts the menu at the screen centre instead of over the status range the pointer landed in. The event's cell and its status range are already carried on MouseEventTarget. This is the smaller of the two menus and needs no new format.
2. right-click-pane records, against key:root:MouseDown3Pane in keys.root-native-mouse and against mouse_word, mouse_line and mouse_hyperlink in formats.mouse-context, which the pin's DEFAULT_PANE_MENU reads. It needs the menu installed, -x M and -y M answering from the invoking event, and those three formats reading the screen under the pointer.
3. paste-under-menu/screen records. Under a menu the pin's overlay eats the paste-start key and the characters behind it, the pane sees the tail as ordinary keys, and the unmatched paste-end leaves a trailing tilde on its row. The raw TUI's EventParser in crates/zz-tui/src/input.rs turns the whole bracketed run into one Paste event before any overlay sees it and hands the tail to the pane as a fresh bracketed paste. The menu ITEM the paste selects is identical on both sides; the pane's own row is the only thing that differs.
AND ONE OBSERVATION IT NAMED AND DID NOT CHASE: under mode-keys emacs a BACKWARD word selection ends on the word's last cell and zz's exclusive emacs end drops it, so the pin copies beta where zz copies bet. No fixture or corpus row drives it.

Zones, drawn around the obligation: crates/zz-daemon/src/daemon.rs (its popup, menu, mouse, paste and key-injection paths); crates/zz-client/src/menu.rs, chrome.rs and core.rs; crates/zz-mux/src/formats.rs and command.rs (display-menu and the mouse formats); crates/zz-tui/src/input.rs, app.rs and state.rs; crates/zz-protocol/src/key.rs, catalog.rs and message.rs (tail appends only); crates/zz-terminal/src/session.rs and interaction.rs (the screen under a cell: the word, the line and the hyperlink at it); compat/tui-mouse.sh, compat/tui-stock-keys.sh, compat/attached-client.sh; compat/tmux-gaps.json (keys.root-native-mouse, mouse.bound-context, formats.mouse-context); compat/tui/ (TUI-008 only, evidence attempt-04). NOT yours: crates/zz-tui/src/render.rs and the chooser surfaces (the choosers lane is inside them right now), the capture and show-messages paths (the introspection lane's), and every ledger record but TUI-008.

THE GROUND MOVES UNDER YOU, and here is exactly how to handle it. The mouse branch you sit on is under adversarial review while you work, and the three gates of cycle 9 push main in the order mouse, choosers, introspection. So:
- Commit ONLY your own changes, on top of 142139fd. Never amend, reorder or rewrite a commit you did not write.
- Fetch every 45 minutes or so. Once the mouse gate has pushed, origin/main carries 142139fd's work, possibly rebased or with must-fix commits applied on top. When you see it (git log origin/main --oneline and look for the subject 'Hand a bound mouse gesture to the pane'), rebase YOUR commits onto origin/main and drop any of yours the gate already carries. Regenerate both reports with their trackers rather than merging them by hand, rebuild, and re-run compat/tui-mouse.sh once before continuing.
- If its reviewer rejected it and a fix pass rewrote campaign/tui-mouse under you, the same move applies: rebase your own commits onto the new tip, keep both sides' intent inside compat/tui-mouse.sh and TUI-008's record, and re-run the fixture.
- Before your final push, fetch once more and rebase onto origin/main if the mouse work has landed there. Push campaign/tui-mouse-menus.

PUNCH LIST, in this order, smallest first, one commit per item:
1. MouseDown3Status with the pin's DEFAULT_WINDOW_MENU text, and display-menu -x W and -y W answering from the invoking event rather than the screen centre. Flip status-clicks/right-click-screen from recorded to asserted, with a --self-check sabotage that fails for the right reason.
2. MouseDown3Pane with the pin's DEFAULT_PANE_MENU, -x M and -y M from the invoking event, and mouse_word, mouse_line and mouse_hyperlink answering from the cell under the pointer. Measure each format on the pin over a plain word, over a line long enough to wrap, and over an OSC 8 hyperlink, and assert what you measured. Flip right-click-pane. Then close the gap items your landing makes the raw TUI honour, each with a dated ${M.date} measurement driven from a real SGR pointer gesture rather than read off a listing, and say in evidence_note which stay and why.
3. paste-under-menu: make the raw TUI answer the pin, the pane's own row included. Flip the case.
4. ONLY IF items 1 to 3 are done with budget left: the backward emacs word selection above, with a fixture case and the pin's measurement for both directions. If the budget is gone, record it as a case with its measurement and say so; do not start it half way.
5. Ledger: TUI-008 moves to review when compat/tui-mouse.sh reports ZERO recorded cases and compat/attached-client.sh is green. Count the recorded cases in the fixture's own output before you write the status and quote its summary line in evidence_note. If a check is still recorded, TUI-008 stays active and you say which and why. Do not touch TUI-012.
Proofs at tip: ${RUN_ENV} compat/tui-mouse.sh three times plus --self-check; compat/tui-stock-keys.sh plus --self-check; compat/tui-copy-mode.sh (TUI-005 is verified and item 4 reaches it); compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/tui-caps.sh (TUI-009 is verified and asserts the mouse_any_flag this branch made real); the delta corpus for your touched commands, and grep compat/scenarios for every string your landing changes before you call an item done; cargo test and clippy per touched crate; cargo test -p zz; tracker checks and compat/check.sh.
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
  mouse: 'LANE-SPECIFIC: type real SGR reports for every pane-body binding the lane claims to have installed and confirm the command ran with the pin\'s target and the pin\'s screen; read #{mouse_any_flag} on both sides with a pane in and out of an application mouse mode; drive a double and a triple click inside and outside the pin\'s interval; drag-select in copy mode and read the buffer; paste under a menu and compare the pane\'s own screen; confirm the GUI sends no mouse key and is unchanged.',
  choosers: 'LANE-SPECIFIC: drive both stock search bindings on both sides and read which row carries the prompt and whether the status row survives; begin a selection over a search match and compare the painted cells; press prefix D then i on BOTH sides and compare the whole info view; run clock-mode, customize-mode and switch-mode on both sides and compare the screen during and after; re-run compat/tui-copy-mode.sh yourself, because TUI-005 is verified and the prompt change reaches it.',
  menus: 'LANE-SPECIFIC: right click the status row and a pane body on BOTH sides and compare the whole screen, the menu text and where the menu sits relative to the pointer, not just that a menu appeared; drive display-menu with -x M, -y M, -x W and -y W directly and read the resulting position on both sides; read mouse_word, mouse_line and mouse_hyperlink on both sides over a plain word, over a wrapped line and over an OSC 8 hyperlink, and over a cell with nothing under it; paste under a menu and compare the pane\'s own row including any trailing tilde, and confirm the menu item the paste selects is the same; confirm the GUI sends no mouse key and is unchanged; re-run compat/tui-caps.sh, because TUI-009 is verified and asserts the mouse flag this branch made real.',
  introspection: 'LANE-SPECIFIC: run show-messages bare, -J, -T and -t on both sides and diff stdout, stderr and exit status byte for byte; capture a pane with content shorter than its height and compare the default range, then -N and -M; confirm compat/scenarios/capture-pane.txt is green at the tip; confirm the client-naming decision is recorded with a measurement rather than asserted as a preference.',
}

const GATE_EXTRA = {
  mouse: `TUI-008 verifies only if compat/tui-mouse.sh reports zero recorded cases at YOUR tip, read from the fixture's own summary line, and attached-client.sh is green. Run python3 compat/tui/verify-claims.py --run TUI-008 --zz <your build> before you write the record: cycle 7 and cycle 8 each had a lane claim a clause its own fixture contradicted. TUI-008 is what TUI-012 waits on, so say in your board note whether it verified.`,
  choosers: `TWO OBLIGATIONS ARE YOURS. TUI-006 is a baseline id: verify it only if compat/tui-choosers.sh reports zero recorded cases at your tip, and check compat/tui-copy-mode.sh is still at zero recorded, because TUI-005 is verified and this lane's prompt change reaches copy mode; a regression there is a blocker, not a nit. TUI-014 is added scope and verifies on its own clauses with compat/tui-client-commands.sh clean. Run python3 compat/tui/verify-claims.py --run TUI-006 TUI-014 --zz <your build> before writing either record.`,
  menus: `TWO OBLIGATIONS TURN ON YOU, and you are the only gate of this runner.
(1) TUI-008 verifies only if compat/tui-mouse.sh reports ZERO recorded cases at YOUR tip, read from the fixture's own summary line, and compat/attached-client.sh is green. Run python3 compat/tui/verify-claims.py --run TUI-008 --zz <your build> before you write the record. Cycle 7 and cycle 8 each had a lane claim a clause its own fixture contradicted, and this lane's first half correctly refused to; do not undo that care.
(2) TUI-012 IS YOURS. Its proof block was filled in cycle 7 and its status is review, held on TUI-008. After you push, or in the same records commit, read the ledger: if TUI-008 is verified at your tip and TUI-003, TUI-004, TUI-009 and TUI-010 are verified on main, re-run compat/tui-superset.sh three times plus --self-check at your tip, append those commands to TUI-012's proof, set its proof.revision to your tip and set it verified; then run python3 compat/tui/verify-claims.py --run TUI-012 --zz <your build> and do not push if it reports a problem. If TUI-008 is still open, leave TUI-012 at review and name what holds it.
MAIN MAY BE HELD BY A CYCLE 9 GATE UNDER THE SAME HOLDER NAME. A claim on a held front fails whoever asks, so a failed claim here is a queue, not an error and not a reason to skip the board. Poll every 10 minutes for up to 120 minutes; if it is still held after that, report it as a problem and stop without pushing.
CORPUS: you are the last gate in your order, so you also run every keys and status scenario and smoke/tui-client-input-backpressure.
THE REPO RULE ON COMMENTS: cycle 9's introspection review found 60 comment lines added against this repo's 'Do not add comments in code'. Check your own lane's diff for the same thing and strip what it added.`,
  introspection: `THREE THINGS TURN ON YOU. (1) TUI-016 verifies if both its clauses assert. (2) TUI-017 stays active with clause 1 open; do not stretch it. (3) TUI-012 IS YOURS, as the last gate: its proof block was filled in cycle 7 and its status is review, held on TUI-008. Read the ledger on origin/main after you fetch. If TUI-008 is verified there, re-run compat/tui-superset.sh three times plus --self-check at your tip, append those commands to TUI-012's proof, set its proof.revision to your tip and set it verified; then run python3 compat/tui/verify-claims.py --run TUI-012 --zz <your build> and do not push if it reports a problem. If TUI-008 is still open, leave TUI-012 at review and name what holds it.`,
}

const LANES = [
  { key: 'menus', prompt: BATCH_MENUS, start: 'origin/campaign/tui-mouse', workdir: 'zz-tui-menus-9', reviewdir: 'zz-tui-menus-9-review', target: `${M.dev}/zz-tui-introspection/target`, extra: REVIEW_EXTRA.menus, gateExtra: GATE_EXTRA.menus },
]
const OPTS = { model: 'opus', effort: 'xhigh' }

const SLOTS = 1
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
  return `You are the integration gate for ONE branch of the zz TUI parity campaign (repo demfabris/zz, board = GitHub issue 7): the ${r.lane.key} lane of cycle 9, its second half. You are the only gate of this runner, but the three gates of the cycle 9 runner push main beside you, so fetch immediately before every push. Two other agents may run beside you: use --jobs ${M.gateJobs}, never run two heavy things of your own at once (cycle 5's gate manufactured flakes by overlapping cargo tests with corpus chunks), and apply the load-flake rule. FOREGROUND ONLY: every command in the foreground with a timeout of at most 590000 (a call is capped at 600 seconds; split cargo test by package and the corpus by explicit scenario names); never a background task; never end your turn to wait. You are done when you have emitted the structured report.
EARLIER GATES THIS CYCLE:
${JSON.stringify(earlier, null, 2)}
THIS LANE, worker report + review verdict:
${JSON.stringify(summary, null, 2)}
REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix in its own commit, re-running the reviewer's probe as proof. reject (after the fix pass) => do not merge; push the rebased tip as campaign/<branch>-gated, post the blockers as a board note on ${LOCK}, report, stop. A blocker you can fix in minutes may be fixed and merged with the probe as proof. Missing review => a compressed contract audit yourself first.
VERIFIED MEANS EVERY CLAUSE: read the obligation's acceptance list, its evidence_note at your tip and the reviewer's every_clause_asserted. Any clause open or partial, or holding a recorded case (a SIBLING case you did not flip green included), keeps the lane's honest status. An accepted gap is never evidence. A dependency not verified on main blocks verified (TUI-006 and TUI-007 need TUI-004).
BOARD: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. claim MAIN --lease 3h before you rebase; renew MAIN --lease 2h before any long stage; after the push: note ${LOCK} --note "<lane> integrated at <sha>: verified <ids or none>; open <what and why>; review <verdict> and what you did"; then release MAIN --reason "cycle 9 second half <lane> pushed at <sha>". A claim on a HELD front fails whoever asks, this holder included, so a cycle 9 gate holding MAIN makes your claim exit non-zero: that is a queue, not an error. Poll every 10 minutes for up to 120 minutes. BOARD FALLBACK: on a GitHub auth failure append the command to ${M.root}/compat/tui/board-replay-9b.sh (uncommitted) and continue.
${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. Never use ${M.root}'s local main, never edit, stash or reset in it. Commits: git -c commit.gpgsign=false commit.
THIS BOX: ${M.boxNote} Gate worktree: ${M.dev}/zz-gate-9b (create it: git -C ${M.root} worktree add --detach ${M.dev}/zz-gate-9b origin/main; never remove ${M.dev}/zz-gate-tui7, the cycle 9 gates are using it). export CARGO_TARGET_DIR=${M.dev}/zz-gate-target-9b in every shell that runs cargo, compat/check.sh included; touch crates/**/*.rs and Cargo.* after every checkout. Every cargo command goes through the slot-and-cap wrapper in the box note with CAP 7G; the one other working agent may hold the other slot.
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




log(`TUI cycle 9 second half: ${ORDER.join(', ')}; one agent at a time; its gate verifies TUI-008 and then TUI-012`)
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

