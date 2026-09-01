export const meta = {
  name: 'opus-compat-run-5',
  description: 'Cycle 5: three Opus implementor lanes (terminal+client options, pane model basket, daemon interaction smalls), Fable reviewer per lane, Fable gate integrates to main',
  phases: [
    { title: 'Work', detail: 'terminal/client options + pane model basket + daemon interaction smalls, parallel worktrees' },
    { title: 'Review', detail: 'one Fable reviewer per lane, adversarial, pipelined behind its worker' },
    { title: 'Integrate', detail: 'serialized Fable MAIN gate: workspace tests, clippy, delta corpus, records, board ledger' },
  ],
}

const WORKER_SCHEMA = {
  type: 'object',
  required: ['branch', 'fronts_done', 'fronts_skipped', 'touched_commands', 'touched_packages', 'notes'],
  properties: {
    branch: { type: 'string', description: 'campaign/* branch pushed to origin, or empty string if nothing pushed' },
    fronts_done: { type: 'array', items: { type: 'object', required: ['front', 'items_closed', 'proofs'], properties: {
      front: { type: 'string', description: 'registry group id this entry covers' },
      items_closed: { type: 'array', items: { type: 'string' } },
      proofs: { type: 'array', items: { type: 'string' }, description: 'exact commands that ran green AT THE FINAL TIP' },
    } } },
    fronts_skipped: { type: 'array', items: { type: 'object', required: ['front', 'why'], properties: { front: { type: 'string' }, why: { type: 'string' } } } },
    touched_commands: { type: 'array', items: { type: 'string' }, description: 'tmux verb names the diff touches, for the delta corpus' },
    touched_packages: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string', description: 'everything reviewer and integrator must know: zone excursions, protocol bump done or not, flaky tests seen, registry subtleties (relocations!), slugs left open and why' },
  },
}

const REVIEW_SCHEMA = {
  type: 'object',
  required: ['lane', 'verdict', 'confirmed_defects', 'checks_run', 'notes'],
  properties: {
    lane: { type: 'string' },
    verdict: { type: 'string', enum: ['approve', 'approve-with-fixes', 'reject'] },
    confirmed_defects: { type: 'array', items: { type: 'object', required: ['front', 'severity', 'description', 'suggested_fix'], properties: {
      front: { type: 'string' },
      severity: { type: 'string', enum: ['blocker', 'must-fix', 'nit'] },
      description: { type: 'string' },
      suggested_fix: { type: 'string' },
    } } },
    checks_run: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
}

const GATE_SCHEMA = {
  type: 'object',
  required: ['merges', 'progress_after', 'problems'],
  properties: {
    merges: { type: 'array', items: { type: 'object', required: ['branch', 'merged', 'merge_commit'], properties: {
      branch: { type: 'string' }, merged: { type: 'boolean' }, merge_commit: { type: 'string' },
      gate_summary: { type: 'string' }, review_actions: { type: 'string' }, flakes: { type: 'string' },
    } } },
    progress_after: { type: 'string' },
    board_updates: { type: 'string' },
    problems: { type: 'string' },
  },
}

const COMMON = `You are an autonomous worker on the zz tmux-compat campaign (repo demfabris/zz). Up to two other workers run in parallel on this 8-core, 30 GB Linux box (Ubuntu), and the user codes here too. Rules that are not negotiable:

SETUP
- The shared checkout is /home/demfabris/dev/zz. Read it, add worktrees from it, but NEVER edit, stash, reset, or clean it (other sessions' uncommitted work lives there; its local main branch may be stale, always use origin/main). On any conflict touching knowledge/tmux/gaps.md, regenerate it with python3 compat/tmux-tracker.py write-report; never hand-merge that generated file.
- NETWORK GIT: origin is SSH (git@github.com:demfabris/zz.git) and works non-interactively here. Fetch with git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME. If a network git command hangs more than ~30s, kill it and retry once with the HTTPS URL https://github.com/demfabris/zz.git (a credential store exists).
- Worktree: git -C /home/demfabris/dev/zz worktree add /home/demfabris/dev/WORKDIR origin/main (append -2 if the path exists). Work ONLY in your worktree.

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8). Prebuilt binary: /home/demfabris/dev/zz/compat/.cache/tmux-src/tmux (source tree beside it, read the C freely). Probe with THROWAWAY servers only: -L zzprobe-$$ sockets; kill your servers when done. Never kill a tmux or zz server you did not start: other lanes and the user may have live ones; never use pkill/killall on tmux or zz.
- Differential scenarios: compat/scenarios/ (smoke under compat/scenarios/smoke/ with fixtures). Run: ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry <scenario-name>. Read 2-3 existing scenarios first to copy the format. A second window in a scenario must be created with new-window -n <name> (bare new-window flakes on automatic-rename).

REGISTRY
- Contracts live in compat/tmux-gaps.json; the gap group's acceptance list IS the contract: read every group of your batch before coding. When you PROVE an item, remove its slug from the group's items array and update reason/evidence; the removal is what the meter counts. A group whose items array empties moves from gaps[] to closed[] with closed_on. Patterns: git -C /home/demfabris/dev/zz show 9e85bc00 -- compat/tmux-gaps.json (plain close), 1f24a1f1 (RELOCATION: slugs moved into an accepted-native group that owns the stance, with the group title/acceptance/reason widened; use this for explicit native decisions), c6ce82c4 (flag promotion: catalog.rs unsupported_flag -> real option, plus the hard-coded (supported, unsupported) counters and usage_overrides length in catalog.rs, plus PINNED_TMUX_USAGE_OVERRIDES adjustments). Script edits with json.dump(..., indent=2) + trailing newline.
- crates/zz-mux/src/compat_manifest_tests.rs hard-codes partition counts (tracked == divergent for keys, constant/delegated format partitions, catalog supported/unsupported); cargo test -p zz-mux is a mandatory gate for ANY registry edit. Read python3 compat/tmux-tracker.py check rules (STATUSES open/blocked/accepted; accepted requires decision native|never; park requires blocked; priority/ease none reserved for accepted) before relocating anything.
- python3 compat/tmux-tracker.py check before each commit; if it flags report freshness, regenerate (write-report) and commit the regenerated file too. compat/run.sh --check-summary is green on main: if you add scenarios, add their summary rows so it stays green.

WIRE PROTOCOL RULE: current PROTOCOL_VERSION is 91. If your diff adds or changes ANYTHING wire-reachable (new ProtocolMessage variants, new fields anywhere in a message or snapshot, appended enum variants on any type that rides a message) bump 91 -> 92 in the same commit. All sites mandatory: crates/zz-protocol/src/message.rs constant + same-file assert test; crates/zz-protocol/tests/hunt_claims.rs version test (currently protocol_version_on_this_commit_is_ninety_one; rename to ..._ninety_two, assert 92, pinned hex hello-frame bytes 0x5B -> 0x5C; grep the hex, decimal greps miss it); knowledge/protocol/wire-protocol.md title/constant/changelog/byte rows (say inserted vs appended honestly); knowledge/protocol/index.md + knowledge/index.md (v91) mirrors; knowledge/crates/zz-protocol.md twice. Then cargo test -p zz-protocol --jobs 4. Another lane may also bump this run; that is fine, the gate reconciles; keep your changelog entry self-contained. No wire change = no bump; say which in notes either way.

MACHINE ETIQUETTE
- Cap parallelism (8 cores shared three ways): cargo build/test --jobs 4, test runs -- --test-threads=2. NEVER workspace-scale builds/tests; focused cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features --jobs 4 -- -D warnings per touched crate only.
- Never pipe cargo test through tail/grep (masks the exit code): > log 2>&1, check exit status, read the log.
- Load-flake rule: fails loaded + passes exact-solo = flake. Known: client_focus_closes_display_panes_and_preserves_chooser_modes, attached_client_extents_clamp_retained_and_default_dimensions, history_request_is_guarded_clamped_and_returns_self_contained_rows, copy-mode reconcile tests, wait_exit_holds_the_control_process_until_a_second_blank_line (can HANG under load, passes solo; use a timeout when running cli_binary tests), smoke/source-replay-diagnostics (pin-side crash under concurrent scenario load; clean solo).

DELIVERY
- One commit per registry group (or tight family), subject matching repo style (git log --oneline -10). NO attribution trailers. NO comments in code. Never insert a new fn between an existing doc block and the fn it documents (last cycle's reviewer caught a misattributed pin-derivation comment).
- PROOFS AT TIP: after your LAST commit, re-run every proof command you list and cargo test -p for every touched crate. Last cycle a worker's final commit orphaned four tests it had already "proven" green; the reviewer treats stale proofs as a blocker.
- Work groups in the order given. No clear path after ~90 honest minutes = skip with a recorded reason, move on. An unprovable acceptance clause is a finding, not a failure: record it precisely in the group reason (that is how contracts get re-scoped).
- Your branch gets an adversarial Fable review before integration: every closed slug needs a proof in your diff that asserts the PINNED behavior. A test asserting zz's current behavior without pin derivation will be flagged and the close reverted. Every relocation needs the accepted group's reason to state the measured pin behavior and the product stance.
- When done (or stopping): push campaign/BRANCHNAME over HTTPS (never force, never main), report via structured output. Stopping early: push what compiles as campaign/BRANCHNAME-wip and say what never ran.
- NO GitHub comments, NO issue 7, NO compat/board.py mutations. The orchestrator speaks for you.
`

const BATCH_TERMINAL = COMMON + `
YOUR BATCH: the terminal and client option baskets, three groups, 30 items. WORKDIR: zz-opus-termopts. BRANCHNAME: batch-terminal-options-opus.
Your zones: crates/zz-terminal (terminal engine), crates/zz-mux/src/tmux_options.rs + honest_knobs.rs (option tables), crates/zz-client, crates/zz-tui, crates/zz/src (GUI client; NOT control_mode.rs). Another worker owns crates/zz-mux/src/{command.rs,model.rs,layout.rs,formats.rs,status.rs} and crates/zz-protocol/src/catalog.rs; a third owns crates/zz-daemon and control_mode.rs. Minimum edits outside your zones (a catalog flag promotion is an allowed mechanical excursion), list every such file in notes.

Groups in priority order:
1. options.terminal-behavior (18 items): acceptance is "Each option has a traced consumer or moves to an explicit native or parked decision." This is a classification-with-proof group and every item can leave it this run. For EACH option: (a) find the pin's consumer (rg the option name in the pinned tree: options-table.c entry, then the code that reads it); (b) find zz's consumer (rg the option name across crates/); (c) settle it one of three ways. CONSUMED: zz reads the option and acts on it; write a pin-derived test (unit or differential) proving equivalent behavior and close the slug. NATIVE: zz's architecture answers this differently by design (the daemon drives no client terminal through terminfo; the GPUI client owns its own terminal; the raw TUI client in crates/zz-tui DOES drive a real terminal, so check what it honors before declaring native); relocate the slug into an accepted-native group (existing or new) whose reason states the pin's behavior and zz's stance, precedent 1f24a1f1. PARKED: real work zz should do later; move the slug into a park group (decision park, status blocked) with the recipe. A slug zz merely stores without consuming is NOT consumed. Expected mix: terminal-features, terminal-overrides, extended-keys, extended-keys-format, user-keys, xterm-keys are client-terminal negotiation; allow-rename, alternate-screen, scroll-on-clear, input-buffer-size, codepoint-widths, variation-selector-always-wide, backspace are pane VT-engine knobs (crates/zz-terminal + libghostty-vt; zz already ties ucs width mode 2027 to codepoint widths, rg mode_2027); assume-paste-time is input timing; focus-follows-mouse is client input; get-clipboard is OSC 52 read policy; editor and default-client-command are command-side. One commit per family (VT knobs, client-terminal negotiation, input/timing, command-side) is fine.
2. terminal.key-reset (1 item): send-keys -R is RIS applied to the pane from the command side while PRESERVING scrollback (pin: cmd-send-keys.c -R block = colour_palette_clear + screen_write_reset; measured 2026-09-01 on a 6-row pane with 20 lines: history 17 -> 23, visible rows pushed into history, capture-pane -S - still holds every line, default tab stops restored). libghostty's ghostty_terminal_reset drops scrollback, so build a scrollback-preserving reset in crates/zz-terminal: push the visible screen into history first, then reset the active screen state and palette (read what libghostty-vt exposes; a composed DECSTR plus scroll-up prelude cannot restore tab stops, the reason records that). Prove with terminal tests derived from the pin measurement, then promote -R in catalog.rs (mechanical excursion) and route it in command.rs with a minimal hunk (other lane's zone, list it). If the engine genuinely cannot do it, record the exact missing primitive in the reason and leave the slug.
3. options.pane-chrome (11 items, client owner): pane-border-format, pane-border-indicators, pane-border-lines, pane-border-status, pane-colours, pane-scrollbars, pane-scrollbars-position, pane-scrollbars-style, pane-scrollbars-timeout, presentation:border-style-owner-z-order, presentation:renderer-style-residue. Same trichotomy. Board recipes exist and are worth following: pane-colours = resolve stored pane-colours[] at effective global-window/window/pane scope, overlay numeric 0..255 indices on each target terminal base palette, emit TerminalKnobsChanged for affected panes on set/append/unset/-U/relocation/inheritance, preserve OSC 4 overrides above configured defaults, do not recolor native GPUI chrome. pane-border-lines (tiled portion: single/double/heavy/simple/number/spaces/none) = raw TUI renders exact live cell topology and selected-owner digits; native GPUI chrome stays theme-owned by product decision (that half is a NATIVE relocation with the stance recorded). pane-border-status/format = the TUI draws a per-pane status row from the format at top/bottom; measure the pin's border-status rendering with capture-pane on a throwaway server. pane-scrollbars* = the GUI has native scrollbars; measure whether the pin's semantics map (position/style/timeout) or record native. presentation items: read their current reason first.
Nothing here should be wire-reachable unless you add a snapshot field for border ownership; if so the 91->92 rule applies. Scenario names: compat/scenarios/option-<slug>.txt or smoke/<slug>.txt with fixtures for attached rendering; add summary rows so --check-summary stays green.`

const BATCH_PANES = COMMON + `
YOUR BATCH: the pane model basket, six groups, 21 items. WORKDIR: zz-opus-panes. BRANCHNAME: batch-pane-model-opus.
Your zones: crates/zz-mux/src/{command.rs,model.rs,layout.rs,sort.rs,formats.rs,status.rs} and crates/zz-protocol/src/catalog.rs (exclusively yours this run). crates/zz-protocol/src/snapshot.rs is wire: a marked-pane or input-disabled fact that must reach clients rides the snapshot and triggers the 91->92 bump. Another worker owns crates/zz-daemon and control_mode.rs; a third owns crates/zz-terminal, crates/zz-tui, crates/zz-client, crates/zz/src. Minimum edits there, list every file in notes.

Groups in order:
1. pane.selection-state (11 items): flag:select-pane:-m -M -d -e -g -P, format:pane_marked pane_marked_set session_marked window_marked_flag, semantic:window-marked-pane-format. Two halves with board recipes. MARKED (do first): the mark is server-global in the pin (one marked pane per server: server_check_marked, cmd-select-pane.c -m/-M, server_clear_marked, the {marked} target in cmd-find.c); implement mark and clear-mark state plus the four formats and the window-marked format semantic; a marked pane that dies or is unlinked clears the mark; prove the pin's transitions on a throwaway server (mark A, mark B moves it, -M clears, kill-pane clears, formats per session/window/pane) and a differential scenario compat/scenarios/pane-selection-marked.txt. INPUT/STYLE (second): -d and -e toggle target-pane input without selecting it (pane_input_off format readback); -P validates and sets both pane-local window-style and window-active-style atomically (a bad style changes nothing and errors like the pin); deprecated -g prints that pane-local style and returns before selection. Prove target ordering, bad-style atomicity, input readback, and -g output against the pin. Input-off enforcement lives in the daemon input path: minimal hunk, list it.
2. display-message.pane-target-grammar (1 item): CMD_FIND_PANE relative window offsets (+N/-N) and the pin's special aliases (read cmd-find.c: {last}/{next}/{previous}/{top}/{bottom}/{left}/{right}/{top-left}/{top-right}/{bottom-left}/{bottom-right}/{marked}/{active}/{current} and the single-char forms), preserving componentwise CANFAIL state after a miss. The marked aliases need group 1.
3. targets.exact-match-prefix (1 item): the group reason carries the full pin measurement of the leading = slot classification (set-option -t =alpha vs =alpha: vs -w/-p). Implement slot classification in the shared resolver; prove every row of the reason.
4. pane.spawn-flags (2 items): split-window -k and -m. Recipe: both set the new pane-local remain-on-exit value to key, retaining every successful, failed, or signaled child exit until the next non-mouse, non-paste key; -m additionally stores its argument as the pane-local remain-on-exit-format. Prove exact usage/arity, parsing, retention for both flags, mouse and paste non-dismissal, next-key dismissal, and exact pane-local format readback. Reuse the existing daemon retain and key-dismiss paths unchanged (rg remain_on_exit in crates/zz-daemon). Keep -W loudly unsupported (pane.command-completion owns it).
5. pane.break-geometry (5 items): break-pane -W -x -y -X -Y. pane.floating-model is accepted-native, so zz has its own floating model: rg floating in crates/zz-mux/src/model.rs and layout.rs, measure the pin's -W path (cmd-break-pane.c: -W enters the floating-pane path where -x/-y/-X/-Y size and place it), then either map each flag onto zz's floating model with differential geometry tests or relocate the flags into the accepted floating group with the measured decision per flag. Do not leave them open with a vague reason.
6. hooks.shutdown-window-unlinked-order (1 item): only if time remains; RB-tree emission order emulation. Otherwise record the exact data needed.
Scenario names: compat/scenarios/pane-<slug>.txt; add summary rows so --check-summary stays green. catalog.rs counters and compat_manifest_tests.rs counts move with every promotion.`

const BATCH_DAEMON = COMMON + `
YOUR BATCH: daemon interaction smalls, nine groups, ~26 items. Burn in order, skip freely with recorded reasons. WORKDIR: zz-opus-dint. BRANCHNAME: batch-daemon-interaction-opus.
Your zones: crates/zz-daemon, crates/zz/src/control_mode.rs, crates/zz-protocol/src/{message.rs,snapshot.rs,lib.rs,key.rs}, crates/zz-mux/src/parser.rs (config parser, for the tilde route). Another worker owns crates/zz-mux/src/{command.rs,model.rs,formats.rs,...} and catalog.rs; a third owns crates/zz-terminal, crates/zz-tui, crates/zz-client, crates/zz/src (GUI). Flag promotions in catalog.rs and parse hunks in command.rs are allowed mechanical excursions: keep them minimal, list every file, the gate reconciles counters.

Groups in order:
1. control-mode.local-parser-environment (1): fully designed by the last worker, execute it. Pin (measured on a throwaway server with a pty-less control client): with server-global HOME=/server/home and the control process holding HOME=/client/home, display-message -p ~ expands to /server/home; unset/empty server HOME falls back to the SERVER host passwd entry; ~root resolves from the server host. zz answers the client HOME because control_mode.rs parse_line() calls MuxEngine::parse_config_without_variable_expansion whose LiteralVariableContext returns None for HOME and falls through to user_home(Uid::current()). zz-mux's ConfigBuilder::home_directory already implements the pin rule (test resolves_bare_tildes_from_server_home_then_passwd). Route: a new client->daemon ProtocolMessage pair (HomeDirectoryRequest{request_id, users} / HomeDirectoryResponse{request_id, homes}) answered from the daemon's global_environment_variable("HOME") then the daemon host's passwd; control_mode does one recording parse pass (user_home returns Some("") so no diagnostic and no discarding), collects the tilde names, does ONE batched round trip only when the line contains ~, then re-parses with a resolving context. Invisible round trip: command numbering and guards untouched. ~4 additive hunks in crates/zz-mux (pub user_home, a resolving ConfigContext, a parse wrapper, a MuxEngine wrapper). This is wire-reachable: 91->92 bump. Prove distinct client/server HOME, empty or unset fallback, named and missing users, literal variable preservation, stale request ids; smoke scenario with a fixture.
2. clients.event-resize-context (1): root cause located: crates/zz-daemon/src/daemon.rs runs client_hook_event(&inner, "client-resized", client) inline in the InputMessage::ClientTerminalSize arm, so the hook fires from the first message of the resize batch, before the TUI's deduplicated per-pane geometry messages land. Defer the hook to the end of the input batch (or gate it on a geometry-settled signal). Acceptance: changed and unchanged Interactive outer-resize reports each emit client-resized once after visible pane and window geometry has settled; a resulting client-active still precedes it; Control refresh-client -C emits neither hook. Prove with a hook body that reads pane/window geometry formats under an outer resize, compared against the pin through the attached-client fixture pattern (compat/scenarios/smoke/fixtures/pty-drive.py and client-exit-actions.py show how to drive real pty clients on both binaries).
3. terminal.key-client-selection (1): send-keys -c. Resolve the pinned target client and supply it to direct pane input and -X mode commands across attached and Control callers. Prove names, tty targets, misses, read-only checks, and cross-client behavior. Exclude -K and -R.
4. terminal.key-control (5): send-keys -K client key-table injection (with no positional key, -K replays the invoking queue key), raw 0x80..0xff delivery (-H high hex), pane-owned no-key -N counts, no-action -N n -X behavior, copy-command shape. Cover attached and Control callers with exact guards, counts, mode transitions, and bytes. Exclude -c and -R.
5. choosers.command-flags (8): choose-buffer -F -k -y, choose-tree -F -h -k -y, semantic:chooser-key-vocabulary. Recipes: -k retains kill-on-exit in the daemon chooser session and kills exactly the source pane when that mode exits (cancel, shortcut/Enter activation, empty-after-delete, overlay replacement, disconnect cleanup; without -k the source pane remains; chosen action execution and error delivery stay ordered before teardown). -h omits the invoking source pane from pane rows while leaving its session/window ancestors visible; filtering computed before omission; fall back from the hidden source selection like pinned mode-tree. choose-buffer -y is parsed by the pin but never read for buffer mode: accept and retain it as inert syntax, prove -y alone/clustered/repeated and that paste/delete results match the no-flag control. -F expands once per visible row in that row's buffer/session/window/pane context including #{line} and makes that text the rendered row while preserving shortcut identity, order, filtering, selection, and defaults when absent; the rendering half crosses into client-core/raw-tui/zz-ui chooser code owned by another lane: keep it to the row-text path, list the files. chooser-key-vocabulary: read the group reason and the pin's mode-tree key table (mode-tree.c) and close it only with an attached-client proof of zz-deliverable key names.
6. hooks.pane-events (2): pane-focus-in and pane-focus-out emit once per transition with a defined client when multiple clients view the pane. Focus is per client; use the attached-client fixture pattern with two pty clients.
7. display-menu.behavior-fidelity (5) and display-popup.behavior-fidelity (5): pick the measurable ones (menu: queue-ordering, style-refresh, action-context-and-errors; popup: style-refresh, to-pane) and record precise measurements for the rest (mouse-policy, paste-close-ordering, border-drag, context-menu, kitty-images). Each acceptance clause names its probe; a headless proof that does not exercise a live client does not close these.
8. clients.path-encoding (2): cwd first. On Unix, replace ClientHello UTF-8-only cwd encoding with a bounded byte-preserving wire path and reconstruct the exact absolute PathBuf at registration; retain portable UTF-8 behavior elsewhere; a local client launched in a cwd containing byte 0xff must resolve a relative source-file path like the pin; preserve omission of relative or over-16-KiB facts without dropping a valid connection. Rides the same 91->92 bump. Environment bytes second if time remains.
Scenario names: compat/scenarios/smoke/<group-slug>.txt + fixtures; add summary rows for non-smoke additions. Fixture lessons from last cycle: line-buffer Python stdout (sys.stdout.reconfigure(line_buffering=True)), bounded WNOHANG reaps that report stalled instead of blocking, never hold a control client's stdin open with sleep | client -C under run-shell (adds the whole sleep to each side).`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz tmux-compat campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit /home/demfabris/dev/zz itself.

SETUP: git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (SSH origin works; HTTPS https://github.com/demfabris/zz.git is the fallback if it hangs). Scratch worktree at the branch tip: git -C /home/demfabris/dev/zz worktree add /home/demfabris/dev/REVIEWDIR <branch-tip-sha>; remove it when done (worktree remove --force).

MACHINE ETIQUETTE: others may still be working on 8 cores. cargo test -p <pkg> --jobs 4 -- --test-threads=2 only, no workspace-scale anything, cargo output to a log file + check exit code. Timeout-guard cli_binary tests (wait_exit can hang under load). Throwaway pin servers -L zzprobe-$$ only, kill after; never kill servers you did not start, never pkill tmux or zz.

METHOD, in order of value:
1. CONTRACT AUDIT: for every closed OR relocated slug (worker report + git diff origin/main...HEAD -- compat/tmux-gaps.json), find the proof in the diff that asserts the group acceptance clause. A test asserting zz behavior with no pin derivation is a defect; quote the clause. A relocation into an accepted-native group is legitimate only when that group's reason states the measured pin behavior and the product stance, and the manifest counts still enforce it.
2. PROOFS AT TIP: run the worker's claimed proof suites yourself at the branch tip (cargo test -p for every touched crate; the named scenarios). Last cycle a final commit orphaned four tests the worker had reported green; any red at tip is a blocker regardless of the report.
3. ORACLE SPOT-CHECKS: 3-5 riskiest claims (sort orders, edge cases, precedence, counts, prompts, transitions) verified against the pinned binary yourself (/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux; the pinned C source sits beside the binary, read it when subtle).
4. TEST HONESTY: run the branch's new/changed tests and its scenarios (ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry <names>). Failing/ignored/tautological = blocker. Check the durable registry resolution carries every divergence the worker disclosed in its notes (last cycle a Control-victim divergence lived only in the throwaway report).
5. INVARIANTS: zone discipline (out-of-zone files listed in notes); wire rule (wire-reachable change => complete 91->92 bump incl. hex 0x5B->0x5C fixture and knowledge mirrors, changelog says inserted vs appended honestly); no code comments; doc comments still attached to the fn they describe; no attribution trailers; registry round-trips (python3 -m json.tool); tracker check green on the branch; --check-summary green if scenarios were added.
6. CALIBRATION: default to refuting each close, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = wrong close or would break main; must-fix = gate applies before merge; nit = mention.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes). checks_run lists exact commands. Thorough but bounded: well under an hour.`

const LANES = [
  { key: 'terminal', prompt: BATCH_TERMINAL, lock: 'F-TERMINAL-CLIENT-OPTIONS', reviewdir: 'zz-review-termopts' },
  { key: 'panes', prompt: BATCH_PANES, lock: 'F-PANE-MODEL-BASKET', reviewdir: 'zz-review-panes' },
  { key: 'daemon', prompt: BATCH_DAEMON, lock: 'F-DAEMON-INTERACTION-SMALLS', reviewdir: 'zz-review-dint' },
]

log('Cycle 5: terminal/client options (30 items) + pane model basket (21) + daemon interaction smalls (~26); Fable review per lane, Fable gate')
const laneResults = await pipeline(
  LANES,
  lane => agent(lane.prompt, { label: `worker:${lane.key}`, phase: 'Work', model: 'opus', schema: WORKER_SCHEMA })
    .then(w => ({ lane, worker: w })),
  r => {
    if (!r || !r.worker || !r.worker.branch) return r
    const reviewPrompt = REVIEW_COMMON + `

LANE: ${r.lane.key}. BRANCH: ${r.worker.branch}. REVIEWDIR: ${r.lane.reviewdir}.
WORKER REPORT (verify, do not trust):
${JSON.stringify(r.worker, null, 2)}`
    return agent(reviewPrompt, { label: `review:${r.lane.key}`, phase: 'Review', model: 'fable', schema: REVIEW_SCHEMA })
      .then(review => ({ ...r, review }))
  }
)
const summaries = laneResults.filter(Boolean).filter(r => r.worker && r.worker.branch)
  .map(r => ({ key: r.lane.key, lock_front: r.lane.lock, review: r.review || null, ...r.worker }))
log(`Lanes complete: ${summaries.length}/3 branches pushed and reviewed. Integrating serially.`)

phase('Integrate')
const gatePrompt = `You are the integration gate for the zz tmux-compat campaign (repo demfabris/zz, board = GitHub issue 7). All workers and reviewers are done; you run ALONE on this 8-core, 30 GB Linux box, full speed. Integrate IN THIS ORDER: terminal first (records-heavy), panes second, daemon third. One gate per branch, and push main as soon as each lane's gate is green (if you die mid-run the orchestrator resumes from what already landed).

Lane summaries, worker report + Fable review verdict each:
${JSON.stringify(summaries, null, 2)}

REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on the branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge; post the blockers as a board note on the lock front, leave the branch, continue. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof. review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating.

BOARD IDENTITY: ZZ_BOARD_HOLDER=ubuntu/orchestrator python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h; front needs --contract --zones [--priority --kind --deps --notes]; withdraw needs TRIAGE held. The orchestrator holds the three lock fronts F-TERMINAL-CLIENT-OPTIONS, F-PANE-MODEL-BASKET, F-DAEMON-INTERACTION-SMALLS (6h leases from launch; expired => claim back as ubuntu/orchestrator before that lane's ledger step).

NETWORK GIT: origin is SSH and works non-interactively. Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. If a network command hangs ~30s, kill it and retry once over HTTPS (https://github.com/demfabris/zz.git). The shared checkout's local main branch may be stale; never use it, never touch it.
knowledge/tmux/gaps.md is generated: regenerate with tmux-tracker.py write-report on every conflict, never hand-merge it.
PROTOCOL RECONCILE: if MULTIPLE branches bumped PROTOCOL_VERSION to 92, the rebase keeps ONE constant at 92 with every changelog entry preserved as separate v92 bullets and the hunt_claims fixture updated once (0x5C); verify cargo test -p zz-protocol after reconciling. catalog.rs counters and compat_manifest_tests.rs counts conflict between lanes routinely: resolve by recounting, then cargo test -p zz-mux and -p zz-protocol.

PER BRANCH, in order:
1. Fresh worktree: git -C /home/demfabris/dev/zz worktree add /home/demfabris/dev/zz-gate-<key> origin/main (remove leftovers with --force first). Rebase branch onto origin/main; compat/tmux-gaps.json conflicts between lanes are normal: merge both sides.
2. First branch only: claim MAIN --lease 2h; hold across all, renew before each subsequent gate, release after the ledger recompute.
3. Code-branch gate stages, in order:
   a. cargo test --workspace --all-features --no-fail-fast --jobs 8 -- --test-threads=4 > log 2>&1 (check exit code; never pipe through tail). Timeout-guard: wait_exit_holds_the_control_process_until_a_second_blank_line can HANG under load; if the run wedges >20min with no output, sample the process; a lost-wakeup hang there counts as the known flake (verify solo).
   b. cargo clippy --workspace --all-targets --all-features -- -D warnings
   c. ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <lane touched_commands> (--list first). Shard up to 4 concurrent run.sh invocations over DISJOINT scenario subsets (8 cores here; proven safe at 8 shards on 16 cores); run smoke/source-replay-diagnostics SOLO after the shards (pin-side crash under load); any divergence re-runs alone before being called real.
   d. python3 compat/tmux-tracker.py check && python3 compat/board_test.py && compat/run.sh --check-summary
4. Flake rules: lone timing test failing loaded + passing exact-solo = flake, proceed (known list: copy-mode reconcile, attached_client_extents…, client_focus_closes…, history_request_is_guarded…, wait_exit… hang, concurrent_default_interactive… "not a terminal" incl. misattribution). Anything else red = real: fix if minutes, else SKIP branch (no push), record, continue.
5. Push main over HTTPS. Non-fast-forward: fetch; user-authored commits + conflict-free disjoint rebase => bounded rerun (lane package tests + its scenarios), push. Never force.
6. Ledger per successful push against that lane's lock_front: candidate (--commit tip --branch campaign/<name> --base <pre-push main> --proof per stage), note (--note: groups covered, slugs closed/relocated, reviewer verdict + what you did about defects), integrated (--merge <sha> --gate "workspace+clippy+delta green"), release (--reason "batch integrated at <sha>").
7. After ALL branches, holding MAIN: recompute TMUX_COMPAT_TRACKER.md from the merged registry: the four headline lines (Campaign delivery, Live work "<open> OPEN + <blocked> BLOCKED = <sum>", Ledger settlement "100 x (<closed> CLOSED + <accepted> ACCEPTED) / (<closed> CLOSED + <len(gaps)> LIVE)" one decimal, Exit evidence scenarios/steps from --check-summary), the Campaign dashboard table rows (Live unresolved, Latest differential, Differential SHA-256 = sha256 of compat/results/summary.md, Ledger settlement), the Ledger settlement calculation block, and a new "### <date> cycle-5 integration checkpoint" table like the cycle-4 one above it. Records commit ("Recompute the live ledger after the cycle-5 merges"), push, release MAIN.
8. Claim TRIAGE. Withdraw fronts fully mooted by the merges, slug-verified one by one against the merged registry: candidates F-PANE-SELECTION-MARKED, F-PANE-SELECTION-INPUT-STYLE, F-DISPLAY-MESSAGE-PANE-TARGETS-V2, F-PANE-SPAWN-RETAIN-V3, F-KEY-CLIENT-SELECTION-V2, F-KEY-CONTROL-V3, F-CONTROL-DAEMON-PARSER-ENV-V2, F-CHOOSER-KILL-ON-EXIT, F-CHOOSER-ROW-FORMAT-V2, F-CHOOSER-TREE-HIDE-SOURCE, F-CHOOSER-BUFFER-ACCEPT-FLAG, F-CLIENT-CWD-BYTE-PATH, F-PANE-COLOURS-PALETTE, F-PANE-BORDER-ZORDER, F-PANE-BORDER-LINES-TILED. Withdrawing F-PANE-SPAWN-RETAIN-V3 breaks F-SPLIT-MUX-01-TESTS-V4's deps: under the same TRIAGE hold remint F-SPLIT-MUX-01-TESTS-V5 with NO deps and 02..05 V5 chained onto their V5 predecessors (same contracts/zones/priority as the V4s), then withdraw the V4s. If a worker's skip reasons prove a group contract is unprovable as written, post that as a residual on the relevant front. Release TRIAGE.
9. python3 compat/progress.py, full output.
10. Remove only your own zz-gate-* worktrees. Leave zz-opus-* and zz-review-*.

Never stash/reset anything in /home/demfabris/dev/zz. Never kill tmux or zz servers you did not start (the user's may be live). Report via structured output: per branch merged/sha/gate_summary/review_actions/flakes, full progress output, board records, problems.`

const gate = summaries.length
  ? await agent(gatePrompt, { label: 'gate:serial', phase: 'Integrate', model: 'fable', schema: GATE_SCHEMA })
  : null
if (!gate) log('No branches to integrate: all workers came back empty.')

return { lanes: laneResults, gate }
