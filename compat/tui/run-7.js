export const meta = {
  name: 'tui-run-7',
  description: 'TUI parity cycle 7: three Opus 5 lanes at xhigh, at most two agents at once, closing TUI-008 and TUI-009 (mouse, paste, focus and the last capability rows), TUI-011 (the stock client command roster and its child obligations) and TUI-012 (superset commands beside tmux behaviour), one adversarial reviewer per lane and one gate agent per branch in order',
  phases: [
    { title: 'Work', detail: 'three worktrees from origin/main: input (TUI-008 + TUI-009), roster (TUI-011), superset (TUI-012); at most two agents at a time including the gates' },
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh per lane, adversarial, pipelined behind its worker; a rejected lane gets one fix pass and a re-review' },
    { title: 'Integrate', detail: 'one gate agent per branch in the order input, roster, superset; each rebases, tests, writes its records, pushes main' },
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
  date: A.date || '2026-09-13',
  base: A.base || 'origin/main',
  protected: A.protected || "nothing at launch: no zz daemon or tmux server of the user's was running on this box when cycle 6 started; if one appears mid-run it is the user's. Never touch a server on the default sockets (/run/user/1000/zz/default.sock, /tmp/tmux-1000/default) that you did not start",
  boxNote: A.boxNote || "This box (alienware) is LINUX: CachyOS (Arch-based), 16 cores, 15 GB RAM plus 15 GB zram swap; /bin/bash is 5.x; the filesystem is btrfs; /opt/homebrew does not exist, so the PATH=/opt/homebrew/bin:$PATH prefix this prompt carries is harmless. Where a prompt says sw_vers, use uname -a plus head -2 /etc/os-release for the OS line. The zz daemon ring log lands under the scrubbed HOME at .local/state/zz/logs/. The compat caches are populated and cycles 1, 3, 4 and 5 ran here: every TUI fixture is known green at origin/main EXCEPT compat/status-row.sh under the box locale (LC_TIME=pt_BR.UTF-8 changes %b; the LC_ALL=C LC_TIME=C control exits 0), and four corpus rows are environmental here (micro-flags, show-options-hooks, lane2-store, smoke/plugin-runtime-resurrect-restore). Slow corpus rows that pass alone: command-item-format (up to 8 minutes), command-prompt-editing (about 4 minutes), copy-mode-stock-action-keys. A red row or fixture at your tip is yours only if it is green at origin/main on this box. Known fixture fragility: compat/tui-stock-keys.sh root-binding-detaches can flake on a wall-clock second boundary and its recorded count wobbles; tui-screen-diff.sh --self-check exited 2 once ('zz refused status-right') and passed solo. zz-daemon client_focus_closes_display_panes_and_preserves_chooser_modes fails about 1 run in 10 even exact-solo at BASE: the modes lane owns it this cycle, so elsewhere a single red there reruns and is named as this known flake, not chased. A single Bash call is capped at 600 seconds and a longer command is moved to the background and killed: pass timeout of at most 590000 and split long work. A cargo debug build of zz is NOT bit-reproducible here: a hash identifies an artifact, the revision plus a clean worktree attests it. The box has a real ~/.tmux.conf and ~/.config/zz/mux.conf: never read or edit them and never start a server that would load them (scrub HOME and XDG_CONFIG_HOME, both; probes use -f /dev/null). Never write 'rm -rf $HOME' or 'rm -rf ~'; put a scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). NEVER run a bare tmux or zz command without -L <throwaway> (or --socket /tmp/<short>.sock for zz). HYGIENE, measured in cycle 5: agents left about 5 GB of zz binary copies under /tmp, which is a RAM-backed tmpfs, and filled the zram swap; a cargo test killed by a timeout leaves /tmp/zz-cli-*/ daemons running for hours; a scrubbed environment without XDG_RUNTIME_DIR autostarts a daemon on /tmp/zz-user/default.sock. So: copy a binary under /tmp only when a comparison needs a frozen copy, delete every copy before your final report, and at the end run pgrep -fa 'zz-cli-|zz-user|zzprobe' and reap only pids whose command line or environment names a socket or scratch HOME you created. After switching a shared CARGO_TARGET_DIR to another worktree, touch that worktree's crates/**/*.rs and Cargo.* first, and rebuild before spawning a binary. Every git commit is 'git -c commit.gpgsign=false commit'. MEMORY, measured 2026-09-11: the first run of this cycle drove the box out of memory about twenty minutes in, with five lanes compiling and testing at once (four rustc, a 1.5 GB zz-daemon test binary, three cargo), and the desktop stayed unusable for hours. So EVERY cargo command on this box goes through a shared two-slot lock and a per-command memory cap, exactly like this: S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=CAP -p MemorySwapMax=2G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo <args> (CAP is 5G for lanes and reviewers, 7G for a gate). fabrico's rule for this box since the OOM: at most two agents work at a time, and the runner enforces it; the two lock slots keep at most two cargo commands running even so. A command that waits more than 540 seconds for a slot exits 1 without running: run it again. A command that hits its cap is killed with exit 137 instead of taking the box down: run it again with fewer --jobs or --test-threads. Neither is a test result. Fixtures and probes need no wrapper, but run one at a time per lane.",
  gitNote: A.gitNote || 'NETWORK GIT: origin is HTTPS (https://github.com/demfabris/zz) through the gh credential helper; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands.',
}
log(`TUI cycle 7 on ${M.machine}; three lanes from origin/main, two agents at a time, --jobs ${M.workerJobs}; one gate agent per branch at --jobs ${M.gateJobs}`)

const RUN_ENV = `PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`
const PIN = `${M.root}/compat/.cache/tmux-src/tmux`
const DECIDED = `decided ${M.date} by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible`

const ORDER = ['input', 'roster', 'superset']
const LOCK = 'F-TUI-CYCLE-7-LANES'
const OWNERSHIP = `LEDGER OWNERSHIP THIS CYCLE (compat/tui/campaign.json records; edit only yours): input lane = TUI-008 (evidence attempt-01) and TUI-009 (attempt-04); roster lane = TUI-011 (attempt-01) and the child obligations it adds, and its GATE alone may verify TUI-006 once choose-client lands (the worker never edits TUI-006); superset lane = TUI-012 (attempt-01). Every other id and every earlier attempt is read-only history, as are the baseline list, the milestones and the pin. In compat/tmux-gaps.json a lane may close ONLY the items its own landing makes the raw TUI honour, inside the gaps its batch names, each with a dated measurement (TMUX_OPTION_CONSUMERS and the compat_manifest_tests.rs partition move in the same commit); an accepted gap keeps its decision for the GUI and for every item not closed.`

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
- A SCREEN STRING YOU REMOVE MAY BE ASSERTED SOMEWHERE ELSE. Cycle 6's choosers gate found a corpus scenario asserting the zz-only chooser chrome that lane had just replaced with the pin's mode tree; neither the worker nor the reviewer had run a corpus row, so the gate was the first place the two met. Before you call an item done, grep compat/scenarios for every zz-only string, prompt, label or format your landing removes or changes, and run compat/run.sh for the rows you find. The same goes for a wire field or an option name you retire.
- FOUND SOMETHING NEW? If you find a divergence inside your obligation that no punch-list item names, fix it if it fits your zones and budget; if it does not, record it as a fixture case with its measurement and leave the obligation at active. Do not hide it and do not stop the punch list for it.

HOW YOU REPORT
- Your final act is the structured report the output schema describes. A worker that ends its turn without it is a failed agent and its lane is dropped.
- FOREGROUND ONLY: every command in the foreground, timeout at most 590000; never Monitor, run_in_background or any background task; never end your turn to wait.
- Check 'date' at the start of each item. The batch's HARD BUDGET is a ceiling. When it is spent, stop, write what asserts now and what remains into evidence_note, set the status honestly, push and report.

SETUP
- The shared checkout is ${M.root}. Read it and add worktrees from it; NEVER edit, stash, reset or clean it. Its local main branch is stale: use origin/main. knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md are generated: regenerate with python3 compat/tui/tracker.py write-report and python3 compat/tmux-tracker.py write-report, never hand-merge them.
- ${M.gitNote} Fetch with GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME (never force, never main).
- BASE THIS CYCLE is origin/main, which carries every cycle-6 landing. Read what is verified with python3 compat/tui/tracker.py check and the ledger itself; do not trust a remembered count. Gates push main mid-cycle, so fetch before you branch and again before you push, and diff your work with git diff origin/main...HEAD (three dots, the merge-base), never two.
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

WIRE PROTOCOL RULE: PROTOCOL_VERSION is 102 on main and STAYS 102 this cycle. 101 shipped in zz 0.8.0 (fd3c64e4) and the cycle-6 caps gate bumped to 102, which is unreleased, so every append of this cycle folds into 102. Pure appends only (a trailing field or a new variant at the end), the consumer half in the same push, and a line naming the append in the v102 entry of knowledge/protocol/wire-protocol.md's version history. Never a version above 102 this cycle.

CODE BOUNDARIES: two lanes touch crates/zz-tui/src, and the input lane owns input.rs, app.rs, tty.rs and terminal_event.rs outright. Keep new surfaces in their modules (copy_view, overlay, chooser and so on), minimal hooks in render.rs, state.rs and input.rs, never reflow or reorder code you do not change, and name every render.rs hunk by function in notes. The GUI keeps compiling with no presentation change.

MACHINE ETIQUETTE: every cargo command through the slot-and-cap wrapper in the box note (CAP 5G); cargo build/test --jobs ${M.workerJobs}, -- --test-threads=${M.workerThreads}; never workspace-scale; cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features -- -D warnings per touched crate. If the diff touches crates/, cargo test -p zz (cli_binary) before the last commit, split by test filters across calls if one call cannot hold it. Never pipe cargo test through tail or grep. Load-flake rule: fails loaded + passes exact-solo = flake.

DELIVERY: one commit per punch-list item where feasible, subject in repo style, git -c commit.gpgsign=false, NO attribution trailers, NO comments in code (a fixture's header block is documentation). Every case you flip or add has a --self-check sabotage that fails for the right reason. After your LAST commit re-run every proof you list. Push campaign/BRANCHNAME and report with the final tip sha. NO GitHub comments, NO issue 7, NO compat/board.py mutations.
`


const BATCH_INPUT = COMMON + `
YOUR BATCH: TUI-008 (mouse, paste, focus and key ownership) and the last recorded rows of TUI-009. BRANCHNAME: tui-input. WORKDIR: zz-tui-input. START: origin/main. HARD BUDGET 330 MINUTES. You are FIRST in the gate order and both other lanes wait on your landing.
Zones: crates/zz-tui/src/ EXCEPT sidebar.rs, picker.rs and browser.rs (input.rs, app.rs, tty.rs, terminal_event.rs, state.rs, and render.rs only for the glyph path), crates/zz-client/src/menu.rs and chrome.rs, crates/zz-protocol/src/key.rs and message.rs, crates/zz-mux/src/command.rs (mouse key dispatch and the -M flags only), crates/zz-terminal/src/session.rs and interaction.rs, crates/zz-daemon/src/daemon.rs (mouse, paste and focus routing only), compat/attached-client.sh, compat/tui-caps.sh, a NEW compat/tui-mouse.sh, compat/tmux-gaps.json (only keys.root-native-mouse, keys.copy-mode-native-mouse, mouse.bound-context, formats.mouse-context, options.client-terminal-negotiation, formats.terminal-cells), compat/tui/ (TUI-008 attempt-01, TUI-009 attempt-04). NOT yours: the sidebar, picker and browser surfaces (the superset lane), catalog.rs's unimplemented-command roster (the roster lane).
Read first: TUI-008's and TUI-009's records, compat/tui/evidence/TUI-009/attempt-03/notes.md, the mouse rows in compat/tui-caps.sh, and the pin's key-bindings.c (the mouse defaults at :498-522), server-client.c (server_client_handle_key, the mouse translation), input-keys.c and tty-keys.c.
MEASURED STATE at origin/main, so you do not re-derive it (verify each before you build on it):
- The raw TUI arms mouse tracking once for the whole attach: tty.rs mouse_enable_sequence writes \\e[?1003h\\e[?1006h (plus \\e[?1016h on ghostty, kitty, wezterm and foot), armed from app.rs TerminalGuard::enter and re-armed by sync_mouse_modes. The pin arms \\e[?1002h and raises MODE_MOUSE_ALL only while a menu is up. That difference is TUI-009's eight recorded mouse_all_flag and mouse_button_flag rows in tui-caps.sh.
- A decoded pointer event never reaches a key table. input.rs mouse_route_owner sends the gesture to the popup, the menu, the sidebar column, a status click on a window range, or the pane as InputMessage::TerminalView with TerminalViewAction::Mouse or ScrollWheel; the daemon applies selection and copy-mode drag in zz-terminal session.rs route_mouse_input. zz-protocol's key catalog defines no mouse key names at all, while zz-mux DOES parse and store them (command.rs parse_mouse_key and mouse_key_identity cover the pin's nine events, nineteen locations and nine buttons, so bind -n MouseDown1Pane is accepted and then never fires).
- Never implemented client-side: a border hit test (so MouseDrag1Border cannot resize), and double and triple click (input.rs sets click_count to 0 or 1 only, so zz-terminal's triple-click path is unreachable). Status clicks reach only TmuxRange::Window.
- Refused today: send-keys -M, resize-pane -M and move-pane -M are unsupported flags in catalog.rs; copy-mode -M parses and returns an empty execution; copy-mode -S is unsupported. The eight mouse_* formats are unimplemented.
- Paste is a direct write to the active pane (input.rs handle_paste, TerminalViewAction::Paste), armed \\e[?2004h for the whole attach; a menu rejects a paste. Focus is \\e[?1004h for the whole attach, InputMessage::ClientFocus plus a pane Focus action, gated in the daemon on focus-events.
PUNCH LIST:
0. THE FIXTURE. Write compat/tui-mouse.sh in the shape of tui-overlays.sh and tui-copy-mode.sh: outer pinned tmux as decoder, isolated HOME and XDG_CONFIG_HOME per side, short /tmp sockets, bounded wait_for on observables, a trap that reaps what it starts, a --self-check with sabotages that must fail for the right reason, and a case list in the header. Drive real SGR mouse reports as bytes into each attached client through the outer tmux and compare the decoded screen, the cursor and the resulting session state (display-message -p on the outer pane). Every case names its channel. This fixture is TUI-008's proof surface.
1. THE MOUSE KEY TABLES. Under the 2026-09-10 triage the raw TUI renders the pin's cells and runs the pin's commands; the GUI keeps its native pointer handling. Give the raw TUI's decoded pointer event a path into the key engine with its context, so the pin's root and copy-table mouse bindings run their commands (select-pane -t=, send -M, resize-pane -M, copy-mode -M, the status and scrollbar names) and a user's own bind -n MouseDown1Pane fires too. Keep every gesture's observable result identical to the pin's, not merely the binding's existence. keys.root-native-mouse holds 27 items and keys.copy-mode-native-mouse 14: close only what your landing makes the raw TUI honour, each with a dated measurement.
2. BOUND-EVENT CONTEXT. mouse.bound-context's five items (copy-mode -S, move-pane -M, resize-pane -M, send-keys -M, and display-message's bare mouse target) and formats.mouse-context's eight mouse_* formats need the invoking event carried to the command. Implement what the raw TUI can honour and assert each in the fixture.
3. THE MISSING GESTURES. Border drag resize, double and triple click, the status-line ranges beyond a window name, and the scrollbar ranges. Each becomes a fixture case against the pin's own result.
4. TUI-009's LAST ROWS. Match the pin's mouse arming (\\e[?1002h plus MODE_MOUSE_ALL only while a menu is up) so tui-caps.sh's eight mouse rows assert; fix the widths/non-utf8 row in render.rs's glyph path; arm modifyOtherKeys and decode \\e[27;<mod>;<key>~ in terminal_event.rs so the extended-keys rows assert, or state precisely why a row cannot and record it with its cause and owner.
5. PASTE AND FOCUS. Compare a bracketed paste into a pane, into copy mode, into a prompt and under a menu; and focus in and out with focus-events on and off, on both sides. Fix what differs inside your zones.
6. Ledger: TUI-008 to review when tui-mouse.sh has zero recorded cases and attached-client.sh is green; TUI-009 to review when tui-caps.sh has no recorded row inside any of its three clauses. Say in evidence_note, for every row you could not flip, its cause and whose zones own it.
Proofs at tip: ${RUN_ENV} compat/tui-mouse.sh three times plus --self-check; compat/tui-caps.sh three times plus --self-check; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/tui-screen-diff.sh plus --self-check; compat/tui-stock-keys.sh; compat/tui-copy-mode.sh; compat/tui-indicators.sh; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
`

const BATCH_ROSTER = COMMON + `
YOUR BATCH: TUI-011, the remaining stock client command inventory. BRANCHNAME: tui-roster. WORKDIR: zz-tui-roster. START: origin/main. HARD BUDGET 300 MINUTES.
Zones: crates/zz-protocol/src/catalog.rs, crates/zz-mux/src/command.rs (only the commands in your roster), crates/zz-daemon/src/daemon.rs (refresh-client, the lock family, capture-pane and the buffer paths), crates/zz/src/lib.rs (the CLI's stdin paths), crates/zz-mux/src/compat_manifest_tests.rs, compat/scenarios/ (new rows), a NEW compat/tui-client-commands.sh, compat/tmux-gaps.json (only commands.native-client-tools, clients.interactive-refresh, options.lock-program, protocol.binary-streams, capture.rich-transports), compat/tui/ (TUI-011 attempt-01 and the child obligations you add). NOT yours: crates/zz-tui/src/ (the input and superset lanes own it) — a divergence that needs a raw-TUI change is a recorded case naming SIBLING:input if the input lane's landing covers it, otherwise a child obligation.
Read first: TUI-011's record and its three clauses, compat/tmux-oracle.json's command list, and the five gaps above.
MEASURED STATE at origin/main (verify each before you build on it): catalog.rs's UNIMPLEMENTED_TMUX_COMMANDS hard-rejects choose-client, clock-mode, customize-mode, suspend-client, switch-mode and server-access, so the CLI prints the unsupported error and the attached screen never changes. lock-client, lock-server and lock-session validate their target and return an empty execution, exit 0 with no output, and their hooks still run. refresh-client rejects -c -D -L -R -U -l and -r as interactive behaviour, needs a control client for -A -B -C, sets flags for -f and -F, and on -S or bare publishes a status render the raw TUI draws. capture-pane's -C -F -H -L -P -R are unsupported flags. display-message -I and split-window -I are unsupported; load-buffer - reads stdin; save-buffer - and source-file - have no dash handling; show-buffer, show-messages and show-hooks are implemented. The corpus has capture-pane, display-message and buffer rows plus flag-error fixtures, and nothing at all for choose-client, customize-mode, suspend-client, server-access or switch-mode.
PUNCH LIST:
1. CLAUSE 1, THE ROSTER. Produce the finite command-and-flag roster the clause asks for, derived from compat/tmux-oracle.json and the five gaps, as a table in the fixture's header and in evidence_note: every entry with its pin behaviour, zz's disposition today, and which of the three dispositions it takes (proved here, declared unsupported with its reason, or handed to a child obligation).
2. CHOOSE-CLIENT FIRST, BECAUSE IT HOLDS TUI-006 OPEN. Cycle 6's choosers lane left TUI-006 at active with exactly one recorded case, client-tree-open in compat/tui-choosers.sh: zz has no choose-client command at all, so the pin draws a client tree the raw TUI cannot. Every other chooser surface in that fixture asserts whole. choose-client is one of your roster's hard-rejected commands, so implement it here, before the rest of the roster: the pin's cmd-choose-tree.c defines it beside choose-tree and both end in window_pane_set_mode on the target pane, and the raw TUI already draws the chooser mode tree for choose-tree. When it lands, run compat/tui-choosers.sh yourself, flip client-tree-open to asserted with a --self-check sabotage, and say in TUI-011's evidence_note that the flip is there for the gate; do NOT edit TUI-006's record, which is not yours. Close command:choose-client in commands.native-client-tools with its dated measurement.
3. CLAUSE 2, THE COMPARISONS. Write compat/tui-client-commands.sh in the shape of the other fixtures (outer pinned tmux as decoder, isolated roots per side, bounded waits, a trap, a --self-check with real sabotages) and compare, for every entry you keep: exact stdout bytes, stderr bytes, exit status, and the attached client's screen and session state after the command. A binary-output or unsupported-platform case is DECLARED in the header with its reason and is not counted as parity. Fix inside your zones what a comparison shows and a landing can honestly close (refresh-client -S and the flag paths, the lock family's observable result, show-buffer, show-messages, show-hooks, save-buffer - and source-file - stdin, the capture-pane flags that need no rich transport).
4. CLAUSE 3, THE CHILD OBLIGATIONS. Split the independent uncovered command families into new stable ids continuing after TUI-013 (TUI-014 onward), each with title, milestone breadth, priority, depends_on, tmux_gaps, sources, acceptance clauses, evidence_note carrying today's measurement and next_action. A NEW ID IS NEVER ADDED TO campaign.json's baseline list (the twelve are frozen; additions report separately). Add each child to TUI-011's depends_on so the parent closes when they do. The natural split from the measurement is the client-side mode tools (choose-client, clock-mode, customize-mode, switch-mode), the lock family and its two options, capture-pane's rich transports, and the binary stream forms; justify whatever split you choose in evidence_note.
5. Ledger: TUI-011 reaches review only when the roster is complete, every entry not handed to a child is proved or declared, and its depends_on names the children. It cannot verify before its children do: say so in next_action rather than stretching a clause.
Proofs at tip: ${RUN_ENV} compat/tui-client-commands.sh three times plus --self-check; compat/run.sh --strict-geometry over the scenarios you touched; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); cargo test and clippy per touched crate; cargo test -p zz; python3 compat/tui/tracker.py check and write-report, python3 compat/tmux-tracker.py check and write-report, python3 -B compat/tui/tracker_test.py.
`

const BATCH_SUPERSET = COMMON + `
YOUR BATCH: TUI-012, the existing superset commands beside tmux behaviour. BRANCHNAME: tui-superset. WORKDIR: zz-tui-superset. START: origin/main. HARD BUDGET 240 MINUTES. You are LAST in the gate order.
Zones: crates/zz-tui/src/sidebar.rs, picker.rs and browser.rs, crates/zz-tui/src/render.rs ONLY the superset panes' drawing, crates/zz-client/src/chrome.rs (the native key defaults), completion.rs and core.rs, a NEW compat/tui-superset.sh, compat/tui-screen-diff.sh ONLY its sidebar cases, compat/tmux-gaps.json (only commands.native-superset, keys.native-defaults, pane.floating-model), compat/tui/ (TUI-012 attempt-01). NOT yours: crates/zz-tui/src/input.rs, app.rs, tty.rs and terminal_event.rs — the input lane owns them this cycle. Anything you need there is a recorded case naming SIBLING:input, and your gate flips it after that lane merges.
Read first: TUI-012's record, the contract's commands-and-presentation table in knowledge/designs/tui-parity.md, and the sidebar cases in compat/tui-screen-diff.sh.
MEASURED STATE at origin/main (verify before you build on it): the 25 zz-only verbs are NATIVE_COMMAND_NAMES in crates/zz-protocol/src/catalog.rs. focus-sidebar reaches the sidebar model (width 28, a minimum of 50 manual columns, and width alone never invokes it); the picker pane's keys run select-pane-kind and its cancel runs kill-pane; the browser verbs draw frames but refuse a screenshot with 'browser screenshots require the zz app'; the agent verbs refuse with 'agent commands require the zz app' and Agent and Editor panes draw only cards; show-last-output, send-last-output and tools open the command-output overlay. The only existing fixture case for any of this is tui-screen-diff.sh's sidebar-shown, sidebar-before and its withdrawal, which asserts the zz screen differs while the sidebar is up and is identical to the pin's after it withdraws. attached-client.sh has no sidebar, picker, browser or agent probe.
PUNCH LIST:
1. CLAUSE 1. Write compat/tui-superset.sh in the shape of the other fixtures and invoke every existing superset command that a raw TUI can run, both directly and through a user binding, with no compatibility profile and no activation flag. For each: what the raw TUI draws, what it refuses and with which exact message, and that the command is reachable by its declared name. A refusal is a declared case with the pin having no counterpart, not a divergence.
2. CLAUSE 2. Around each surface run the ordinary terminal commands: create, select, split, resize, detach and reattach, with the surface up and after it withdraws. Assert that a standard command keeps its meaning and its input ownership while a superset surface is up, and that withdrawing the surface restores the canvas the pin would draw, cell for cell. This is where a real divergence is most likely: fix what is inside your zones and record the rest with its cause.
3. CLAUSE 3. Client-local sidebar state across two attached clients (showing it in one must not change the other), and the browser capability and provider fallback: assert the declared behaviour and the message a client without Kitty support or a provider gets.
4. GAPS. commands.native-superset, keys.native-defaults and pane.floating-model are accepted decisions about zz's own namespace; they stay accepted. Your landing closes an item only if it makes the raw TUI honour the pin's behaviour for that item, which for these three is unlikely: say so plainly rather than closing nothing silently.
5. Ledger: TUI-012 to review when tui-superset.sh has zero recorded cases. It depends on TUI-004, TUI-008, TUI-009 and TUI-010; TUI-008 and TUI-009 land this cycle from the input lane, ahead of you.
Proofs at tip: ${RUN_ENV} compat/tui-superset.sh three times plus --self-check; compat/tui-screen-diff.sh plus --self-check; compat/attached-client.sh (TMUX_BIN=${PIN} ZZ_BIN=<your build>); compat/tui-pane-geometry.sh; compat/status-row.sh under LC_ALL=C LC_TIME=C; cargo test and clippy per touched crate; cargo test -p zz; tracker checks.
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
  input: 'LANE-SPECIFIC: type real SGR mouse reports into both attached clients yourself (press, drag, release, wheel, and a double and triple click inside the pin\'s click interval) at a pane, a border, the status row and a scrollbar, with mouse on and mouse off, and compare the decoded screen and the session state; bind -n MouseDown1Pane and a copy-table mouse key on both sides and confirm the command runs with the pin\'s target; read #{mouse_x}, #{mouse_y}, #{mouse_pane}, #{mouse_word} and #{mouse_status_range} through a bound command on both sides; check the arming bytes with the outer tmux\'s own flags while a menu is up and while it is not; paste into a pane, copy mode, a prompt and under a menu; toggle focus-events and drive focus in and out.',
  roster: 'LANE-SPECIFIC: run every roster entry yourself on both sides, attached, and compare stdout, stderr, exit status and the screen; confirm every declared case names a real reason and is not merely unimplemented; read each new child obligation against the tracker rules (fields, dependency direction, never in baseline) and confirm the parent depends on them; confirm no entry was quietly dropped from the roster between the header table and the fixture.',
  superset: 'LANE-SPECIFIC: run each superset verb on the zz side with a pinned tmux attached beside it and confirm the surrounding tmux commands keep their meaning; attach a second client and confirm the sidebar stays client-local; withdraw each surface and compare the whole screen to the pin\'s; confirm every refusal message is exact and that no case passes merely because the surface failed to open.',
}

const GATE_EXTRA = {
  input: `VERIFIED MEANS EVERY CLAUSE, and this lane carries two obligations. TUI-008 verifies only if compat/tui-mouse.sh has zero recorded cases inside its three clauses and attached-client.sh is green; TUI-009 only if tui-caps.sh has no recorded row inside any of its three clauses. Either one may verify without the other. Both feed TUI-012, whose gate runs after yours: if you verify them, say so in the board note so the superset gate can rely on it.`,
  roster: `TUI-011 CANNOT VERIFY before the child obligations it adds are verified, and they are new work: expect status review with a complete roster, and check that every new id has all the tracker's required fields, is absent from campaign.json's baseline list, and appears in TUI-011's depends_on. python3 compat/tui/tracker.py check plus python3 -B compat/tui/tracker_test.py must pass, and the generated report must show the added-scope count moving, not the baseline count. DEPENDENT VERIFICATION, yours: TUI-006 sat at active through cycle 6 with one recorded case, client-tree-open, waiting on choose-client, and this lane implements it. If TUI-006's dependencies are verified on main (TUI-002, TUI-003, TUI-004) and compat/tui-choosers.sh runs three times plus --self-check green at your tip with zero recorded cases, then TUI-006 meets the bar: write its evidence, fill its proof block with your tip as the revision and those commands, and set it verified. If any chooser case is still recorded, leave TUI-006 alone and name the case.`,
  superset: `TUI-012 depends on TUI-004, TUI-008, TUI-009 and TUI-010. The input gate ran before you: read the ledger on origin/main after you fetch. If TUI-008 and TUI-009 are verified there and this lane's clauses assert, verify TUI-012. If either is not, apply the held rule: fill the proof block, leave status at review, evidence_note leading 'proof complete at <sha>; held on <ids>', and next_action naming what must verify first. Also flip this lane's SIBLING:input cases after rebasing onto the input landing, keeping a flip only if the fixture and its --self-check stay green.`,
}

const LANES = [
  { key: 'input', prompt: BATCH_INPUT, start: 'origin/main', workdir: 'zz-tui-input', reviewdir: 'zz-tui-input-review', target: `${M.dev}/zz-tui-input/target`, extra: REVIEW_EXTRA.input, gateExtra: GATE_EXTRA.input },
  { key: 'roster', prompt: BATCH_ROSTER, start: 'origin/main', workdir: 'zz-tui-roster', reviewdir: 'zz-tui-roster-review', target: `${M.dev}/zz-tui-roster/target`, extra: REVIEW_EXTRA.roster, gateExtra: GATE_EXTRA.roster },
  { key: 'superset', prompt: BATCH_SUPERSET, start: 'origin/main', workdir: 'zz-tui-superset', reviewdir: 'zz-tui-superset-review', target: `${M.dev}/zz-tui-superset/target`, extra: REVIEW_EXTRA.superset, gateExtra: GATE_EXTRA.superset },
]
const OPTS = { model: 'opus', effort: 'xhigh' }

const SLOTS = 2
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


log(`TUI cycle 7: ${ORDER.join(', ')}; at most two agents at a time; a gate takes the next free slot before a waiting lane`)
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

