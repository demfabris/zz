export const meta = {
  name: 'opus-compat-run-6-continue',
  description: 'Cycle 6 continuation after a machine move: keys lane cached (worker + review), daemon lane cached worker + live review, client lane resumes from its wip branch, then the Fable gate',
  phases: [
    { title: 'Work', detail: 'client lane resumes from origin/campaign/batch-choosers-overlays-opus-wip; keys and daemon workers replay from embedded reports' },
    { title: 'Review', detail: 'daemon and client Fable reviews (keys review replays from its embedded verdict)' },
    { title: 'Integrate', detail: 'serialized Fable MAIN gate: workspace tests, clippy, delta corpus, records, board ledger' },
  ],
}

const A = args || {}
const M = {
  root: A.root || '/home/demfabris/dev/zz',
  dev: A.dev || '/home/demfabris/dev',
  holder: A.holder || 'ubuntu/orchestrator',
  machine: A.machine || '8-core, 30 GB Linux box (Ubuntu)',
  cores: A.cores || 8,
  workerJobs: A.workerJobs || 4,
  workerThreads: A.workerThreads || 2,
  gateJobs: A.gateJobs || 8,
  gateThreads: A.gateThreads || 4,
  shards: A.shards || 4,
}
log(`Machine: ${M.machine}; root ${M.root}; holder ${M.holder}; workers --jobs ${M.workerJobs}/--test-threads=${M.workerThreads}; gate --jobs ${M.gateJobs}/--test-threads=${M.gateThreads}; ${M.shards} corpus shards`)

const WORKER_SCHEMA = {
  type: 'object',
  required: ['branch', 'fronts_done', 'fronts_skipped', 'touched_commands', 'touched_packages', 'notes'],
  properties: {
    branch: { type: 'string', description: 'campaign/* branch pushed to origin, or empty string if nothing pushed' },
    fronts_done: { type: 'array', items: { type: 'object', required: ['front', 'items_closed', 'proofs'], properties: {
      front: { type: 'string', description: 'registry group id (or board front id for a defect front) this entry covers' },
      items_closed: { type: 'array', items: { type: 'string' } },
      proofs: { type: 'array', items: { type: 'string' }, description: 'exact commands that ran green AT THE FINAL TIP' },
    } } },
    fronts_skipped: { type: 'array', items: { type: 'object', required: ['front', 'why'], properties: { front: { type: 'string' }, why: { type: 'string' } } } },
    touched_commands: { type: 'array', items: { type: 'string' }, description: 'tmux verb names the diff touches, for the delta corpus' },
    touched_packages: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string', description: 'everything reviewer and integrator must know: zone excursions, protocol bump done or not, flaky tests seen, registry subtleties (relocations, re-scoped acceptance clauses), slugs left open and why' },
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

const COMMON = `You are an autonomous worker on the zz tmux-compat campaign (repo demfabris/zz). Up to two other workers run in parallel on this ${M.machine}, and the user codes here too. Rules that are not negotiable:

SETUP
- The shared checkout is ${M.root}. Read it, add worktrees from it, but NEVER edit, stash, reset, or clean it (other sessions' uncommitted work lives there; its local main branch may be stale, always use origin/main). On any conflict touching knowledge/tmux/gaps.md, regenerate it with python3 compat/tmux-tracker.py write-report; never hand-merge that generated file.
- NETWORK GIT: origin is SSH (git@github.com:demfabris/zz.git) and works non-interactively here. Fetch with git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME. If a network git command hangs more than ~30s, kill it and retry once with the HTTPS URL https://github.com/demfabris/zz.git (a credential store exists).
- Worktree: if ${M.dev}/WORKDIR exists from a previous cycle and git -C ${M.dev}/WORKDIR status --short prints nothing, reuse its warm build: git -C ${M.dev}/WORKDIR checkout --detach origin/main. Otherwise git -C ${M.root} worktree add ${M.dev}/WORKDIR origin/main (append -2 if a dirty one occupies the path). Work ONLY in your worktree.

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8). Prebuilt binary: ${M.root}/compat/.cache/tmux-src/tmux (source tree beside it, read the C freely). Probe with THROWAWAY servers only: -L zzprobe-$$ sockets; kill your servers when done. Never kill a tmux or zz server you did not start: other lanes and the user may have live ones; never use pkill/killall on tmux or zz.
- Differential scenarios: compat/scenarios/ (smoke under compat/scenarios/smoke/ with fixtures). Run: ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins compat/run.sh --strict-geometry <scenario-name>. Read 2-3 existing scenarios first to copy the format. A second window in a scenario must be created with new-window -n <name> (bare new-window flakes on automatic-rename). Real pty clients on both binaries: compat/scenarios/smoke/fixtures/pty-drive.py and client-exit-actions.py show the pattern; anything that needs an attached client (copy mode, prompts, choosers, menus, popups, focus) is proved that way, never through a detached differential row.
- Fixture lessons: line-buffer Python stdout (sys.stdout.reconfigure(line_buffering=True)); bounded WNOHANG reaps that report stalled instead of blocking; never hold a control client's stdin open with sleep | client -C under run-shell; set -g status-keys emacs before measuring prompts on the pin unless the probe is about vi keys (EDITOR in the environment flips it).

REGISTRY
- Contracts live in compat/tmux-gaps.json; the gap group's acceptance list IS the contract: read every group of your batch before coding. When you PROVE an item, remove its slug from the group's items array and update reason/evidence; the removal is what the meter counts. A group whose items array empties moves from gaps[] to closed[] with closed_on. Patterns: git -C ${M.root} show 9e85bc00 -- compat/tmux-gaps.json (plain close), 1f24a1f1 (RELOCATION: slugs moved into an accepted-native group that owns the stance, with the group title/acceptance/reason widened; use this for explicit native decisions), c6ce82c4 (flag promotion: catalog.rs unsupported_flag -> real option, plus the hard-coded (supported, unsupported) counters and usage_overrides length in catalog.rs, plus PINNED_TMUX_USAGE_OVERRIDES adjustments), 0fec342 and 9cab1fa on origin/main (a reverted close and the reason that records why). A slug in a blocked/park group closes the same way: remove it and leave the rest parked. Script edits with json.dump(..., indent=2) + trailing newline.
- RE-SCOPING: when your pin measurement contradicts an acceptance clause, rewriting the clause to the measured pin and closing against it is legitimate ONLY if the reason records the old clause, the measurement that refuted it, and the probe; the reviewer checks that trail.
- crates/zz-mux/src/compat_manifest_tests.rs hard-codes partition counts (tracked == divergent for keys, constant/delegated format partitions, catalog supported/unsupported); cargo test -p zz-mux is a mandatory gate for ANY registry edit. Read python3 compat/tmux-tracker.py check rules (STATUSES open/blocked/accepted; accepted requires decision native|never; park requires blocked; priority/ease none reserved for accepted) before relocating anything.
- python3 compat/tmux-tracker.py check before each commit; if it flags report freshness, regenerate (write-report) and commit the regenerated file too. compat/run.sh --check-summary is green on main: if you add scenarios, add their summary rows so it stays green.

WIRE PROTOCOL RULE: current PROTOCOL_VERSION is 92. If your diff adds or changes ANYTHING wire-reachable (new ProtocolMessage variants, new fields anywhere in a message or snapshot, appended enum variants on any type that rides a message) bump 92 -> 93 in the same commit. All sites mandatory: crates/zz-protocol/src/message.rs constant + same-file assert test; crates/zz-protocol/tests/hunt_claims.rs version test (currently protocol_version_on_this_commit_is_ninety_two; rename to ..._ninety_three, assert 93, pinned hex hello-frame bytes 0x5C -> 0x5D in every position; grep the hex, decimal greps miss it); knowledge/protocol/wire-protocol.md title/constant/changelog/byte rows (say inserted vs appended honestly); knowledge/protocol/index.md + knowledge/index.md (v92) mirrors; knowledge/crates/zz-protocol.md twice. Then cargo test -p zz-protocol --jobs ${M.workerJobs}. Another lane may also bump this run; that is fine, the gate reconciles; keep your changelog entry self-contained. No wire change = no bump; say which in notes either way. MuxEffect and CommandSpec are NOT wire (precedent: cycle 5 checked with grep); anything on ProtocolMessage, MuxSnapshot, ClientHello, or an enum they carry IS.

MACHINE ETIQUETTE
- Cap parallelism (${M.cores} cores shared three ways): cargo build/test --jobs ${M.workerJobs}, test runs -- --test-threads=${M.workerThreads}. NEVER workspace-scale builds/tests; focused cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features --jobs ${M.workerJobs} -- -D warnings per touched crate only.
- DOWNSTREAM RULE: if your diff touches crates/zz-mux/src/command.rs or model.rs target resolution, effect shapes, or anything zz-daemon calls, ALSO run cargo test -p zz-daemon --lib --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} before your last commit. Last cycle a mux-only lane shipped two deterministic zz-daemon test failures nobody had run.
- Never pipe cargo test through tail/grep (masks the exit code): > log 2>&1, check exit status, read the log.
- Load-flake rule: fails loaded + passes exact-solo = flake. Known: client_focus_closes_display_panes_and_preserves_chooser_modes, attached_client_extents_clamp_retained_and_default_dimensions, history_request_is_guarded_clamped_and_returns_self_contained_rows, copy-mode reconcile tests, daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts, nested_alias_queue_bubbles_shutdown_and_yield_to_its_parent, control_sourced_run_shell_closes_before_raw_output_and_same_line_continues, wait_exit_holds_the_control_process_until_a_second_blank_line (can HANG under load, passes solo; use a timeout when running cli_binary tests), smoke/source-replay-diagnostics (pin-side crash under concurrent scenario load; clean solo).

DELIVERY
- One commit per registry group (or tight family), subject matching repo style (git log --oneline -10). NO attribution trailers. NO comments in code. Never insert a new fn between an existing doc block and the fn it documents.
- PROOFS AT TIP: after your LAST commit, re-run every proof command you list and cargo test -p for every touched crate (plus zz-daemon under the downstream rule). Stale proofs are a reviewer blocker.
- Work groups in the order given. No clear path after ~90 honest minutes = skip with a recorded reason, move on. An unprovable acceptance clause is a finding, not a failure: record it precisely in the group reason (that is how contracts get re-scoped).
- Your branch gets an adversarial Fable review before integration: every closed slug needs a proof in your diff that asserts the PINNED behavior. A test asserting zz's current behavior without pin derivation will be flagged and the close reverted. Every relocation needs the accepted group's reason to state the measured pin behavior and the product stance. Every divergence you discover goes into the durable registry reason, not only into your report.
- When done (or stopping): push campaign/BRANCHNAME (never force, never main), report via structured output. Stopping early: push what compiles as campaign/BRANCHNAME-wip and say what never ran.
- NO GitHub comments, NO issue 7, NO compat/board.py mutations. The orchestrator speaks for you.
`

const BATCH_KEYS = COMMON + `
YOUR BATCH: the send-keys and copy-mode target-client family plus format smalls, up to eleven groups, 24 items, ordered by value. WORKDIR: zz-opus-panes (warm zz-mux/zz-protocol build). BRANCHNAME: batch-keys-copy-formats-opus.
Your zones: crates/zz-mux/src/{command.rs,model.rs,layout.rs,sort.rs,formats.rs,status.rs,copy_actions.rs}, crates/zz-protocol/src/{catalog.rs,key.rs}, crates/zz-terminal. Another worker owns crates/zz-daemon, control_mode.rs, parser.rs, daemon status.rs; a third owns crates/zz-client, crates/zz-tui, crates/zz-ui, crates/zz/src (GUI), tmux_options.rs, and protocol message.rs/snapshot.rs. Allowed excursions, minimal and listed in notes: crates/zz-daemon/src/daemon.rs ONLY in the send-keys / copy-mode effect execution and the format-hook producers (window_bigger family), plus catalog counters and message.rs if a copy-mode kill flag rides the wire.

Groups in order:
1. terminal.key-client-selection (1: flag:send-keys:-c) and terminal.key-control (5: flag:send-keys:-K, semantic:send-keys-high-hex, semantic:send-keys-no-key-count, semantic:send-keys-empty-copy-count, semantic:send-keys-copy-command-shape). ONE structural change unlocks both: the three effects send-keys produces (MuxEffect::SendKeys, MuxEffect::TerminalView, MuxEffect::CopyModeRepeat) carry no target client, so the daemon substitutes the invoking client for the view id, the read-only check, and the copy-session owner. Add an optional target client to those effects (roughly eighty construction and match sites in command.rs, mostly mechanical; keep the field name and type obvious) and have the daemon resolve it through the existing find_attached_client_with_aliases path the popup and menu commands already use, falling back to the invoking client when absent. The -c contract is fully measured in the group reason (CMD_CLIENT_CFLAG|CMD_CLIENT_CANFAIL: quiet miss exits 0 and still delivers; read-only check follows the SELECTED client and is skipped with -X; the same client reaches window_pane_key, key_bindings_dispatch, and wme->mode->command). -K injects the key into the selected client's key-table handler (server_client_handle_key) and with no positional key replays the invoking queue key; -H delivers raw 0x80..0xff bytes; a bare no-key -N count and a no-action -N n -X live on the pane mode and survive the pin's cross-client invocation rules; copy-command shape per cmd-send-keys.c. Promote -c, -K, -H in catalog.rs with counters. Prove with two real pty clients on both binaries (one read-write, one -r): names, tty targets, misses, read-only checks, cross-client behavior, exact guards, counts, mode transitions, bytes. Read the board front notes reproduced here: F-KEY-CONTROL-V3 wants compat/scenarios/smoke/send-keys-control.txt + fixture; F-KEY-CLIENT-SELECTION-V2 wants smoke/send-keys-client-selection.txt + fixture.
2. copy-mode.command-fidelity (4: flag:copy-mode:-k, flag:copy-mode:-s, semantic:copy-mode-command-counts, semantic:copy-mode-command-errors). Headless copy-mode targets are ACCEPTED NATIVE (clients.interactive-refresh: zz's copy mode lives per client), so every proof here drives an attached pty client; do not write detached differential rows for it. The group reason carries the pin measurements: -k stores wme->kill and window_pane_reset_mode kills the pane after the mode is torn down (cancel removes the pane; on the last pane it takes window, session, server); zz currently rejects -k loudly, which breaks bind-key ... copy-mode -k. Carrying the bit likely puts a kill flag on the copy-mode entry action, which rides the wire (92 -> 93), and the exit path asks the daemon to kill the pane. -s views another pane's screen (window_pane_set_mode clones the source pane's screen; window_copy_refresh_start refuses live re-sync when wme->swp differs). Counts and errors follow the same target-client plumbing as group 1.
3. display-message.format-listing (2: flag:display-message:-a, semantic:display-message-format-listing). Pin: cmd-display-message.c -a calls format_each, which walks the format tree in its RB order and prints key=value per line; measure the exact order and shape on the pin for a session, window, and pane context. zz's expander resolves named lookups but has no ordered enumeration: add one over the same 198-name table plus the context producers zz defines. Acceptance keeps unsupported families with their owners: prove the order rule and line shape match, and that the set difference between the pin's listing and zz's is exactly the still-tracked format gaps (assert it from compat/tmux-gaps.json in the test so it cannot rot).
4. formats.window-runtime (3: format:window_bigger, format:window_offset_x, format:window_offset_y). Measured blocker in the reason: the pin reads the client's cached tty viewport, so all three answer null whenever the format tree carries no client (list-windows rows are built with a null client even while attached) and follow the client's own current window. zz delegates the cell metrics to the daemon already (format hooks); do the same for these three from the format client's retained viewport, null without a client. Differential: display-message with and without a client, list-windows rows, a session-scoped format.
5. display-message.pane-target-grammar (1): finish the client slot per board front F-DISPLAY-MESSAGE-PANE-TARGETS-V3: -t @, {active}, and {current} answer 'no current client' and exit 1 from an unattached CLI and resolve the invoking client's current pane when attached (cmd-find.c reads cmdq_get_client(item)->session->curw->window->active). Choose between a client-aware resolver signature across the ~50 MuxState::resolve_pane/resolve_window call sites and daemon-side pre-resolution through ExecutionContext::attached_client_context; prove attached and unattached callers differentially (the 70-step pane-target-grammar row is already clean for every clientless form).
6. hooks.shutdown-window-unlinked-order (1). The reason records session_destroy's exact sequence (clear curw, remove from the global RB tree, session-closed, drain lastw, then RB_ROOT(&s->windows) winlinks in index order with window-unlinked each) and what the hook channel can and cannot observe. Close it only with a proof that observes the order the pin actually exposes; otherwise leave the exact missing data in the reason.
7. copy-mode.action-fidelity (2: copy-format-and-destination, logical-line-and-mode-keys) and keys.copy-mode-unsupported-default-actions (2: the r keys, gated on a daemon-owned refresh revision the reason describes; skip with the recipe if the revision does not exist).
8. formats.modifier-fidelity (1: I, grammar measured in the reason) and display-message.verbose-trace (2: -v structured trace sink). Last, only if time remains.
Scenario names: compat/scenarios/smoke/send-keys-*.txt, smoke/copy-mode-*.txt, compat/scenarios/format-*.txt; add summary rows so --check-summary stays green. catalog.rs counters and compat_manifest_tests.rs counts move with every promotion.`

const BATCH_DAEMON = COMMON + `
YOUR BATCH: a p2 defect front first, then the prompt and hook baskets and daemon smalls, up to eight groups, 18 items. WORKDIR: zz-opus-dint (warm zz-daemon build). BRANCHNAME: batch-daemon-prompt-hooks-opus.
Your zones: crates/zz-daemon (daemon.rs, status.rs, tests), crates/zz/src/control_mode.rs, crates/zz-mux/src/parser.rs. Another worker owns crates/zz-mux/src/{command.rs,model.rs,formats.rs,...}, catalog.rs, key.rs, zz-terminal; a third owns crates/zz-client, crates/zz-tui, crates/zz-ui, crates/zz/src (GUI), tmux_options.rs, message.rs/snapshot.rs. Allowed excursions, minimal and listed in notes: command.rs for the alias-group provenance and prompt flag parsing; catalog.rs promotions; message.rs when a fact must ride the wire; crates/zz-tui/src/input.rs and crates/zz/src/command/palette.rs ONLY for prompt line-editing behavior in group 2.

Groups in order:
1. F-ALIAS-GROUP-FORGERY (board front, priority 2, a DEFECT not a registry group). Reproduce on origin/main first: a user-authored '__zz-command-alias-group { ... }' block (the internal sentinel is const COMMAND_ALIAS_GROUP_NAME in crates/zz-mux/src/command.rs) must be rejected as 'unknown command: __zz-command-alias-group' exactly like pinned tmux rejects an unknown name, across the local CLI, config load, and Control source replay. The recorded defect: prepare_command_list_with_engine marks the shape Ready without alias_matched, validation accepts it before unknown-name rejection, and daemon dispatch executes its children. Fix by making the alias-group shape unforgeable (a typed internal representation produced only by alias expansion, or provenance the parser cannot mint) rather than by string-matching the name. Direct rejection tests at each entry (CLI, config, Control), plus compat/scenarios/smoke/alias-group-forgery.txt with a fixture comparing both binaries. If main already rejects it, prove that with the scenario and the tests and say so; the orchestrator withdraws the front either way. Report it under fronts_done with front 'F-ALIAS-GROUP-FORGERY' and an empty items_closed.
2. prompt.command-fidelity (11). Start with semantic:status-keys-editor-derived-default: tmux.c derives BOTH status-keys and mode-keys from the basename of VISUAL or EDITOR; zz's mode_keys_from_environment feeds only set_default_mode_keys (measured in the reason: EDITOR=vi, EDITOR=/usr/local/bin/nvim, VISUAL=vim with EDITOR=emacs each answer vi for both on the pin). Then flag:command-prompt:-F (format-expand the template before parsing), -l (single-line answer), -t (target client), semantic:command-prompt-labels, -chain (multiple prompts, %1..%N substitution order), -pass-order, -key-spelling, -vi-editing (status-keys vi selects vi line editing; the prompt is drawn by clients, so the editing state lives where the client handles the prompt: zz-tui input.rs and the GUI palette are the excursion; measure the pin's vi keys in status-prompt.c), and semantic:prompt-message-freeze. Measure each on the pin with a real pty client (the prompt draws in the status line; use the nested-multiplexer rig, inner server attached inside an outer pane, and capture the OUTER pane, because capture-pane cannot see the prompt). Read status-prompt.c and cmd-command-prompt.c. Prompt -P is accepted-native; do not touch it. Every item closes with an attached proof; unprovable clauses go into the reason.
3. hooks.pane-events (2: hook:pane-focus-in, hook:pane-focus-out). A close was reverted at the last gate on two pin measurements now in the reason and the acceptance: the pin keeps a per-pane PANE_FOCUSED flag evaluated only at its trigger points (attach, detach, tty focus keys, overlay set/clear, pane removal, and pane/window switch ONLY under focus-events on), so with focus-events at its default off a sequence attach / select-pane / new-window / select-window / kill-pane / detach emits exactly one pane-focus-in and one pane-focus-out; and with focus-events on it queues pane-focus-out/in BEFORE window-pane-changed, session-window-changed, client-session-changed, client-attached, client-detached, and AFTER window-pane-changed on kill-pane. Build that model (not a recompute-after-every-command), reuse the reverted fixture as a starting point (git show 0fec342 on origin/main shows what was removed), and run the differential with focus-events off AND on, checking hook order with a synchronous set-buffer -a hook body.
4. clients.event-resize-context (1). The reason proves the clause as written does not describe the pin: client size is current and window/pane geometry is exactly one resize behind on BOTH binaries, a changed Interactive resize emits client-resized once on both, and Control refresh-client -C emits neither hook on both. The one real divergence is that the pin's hook body carries the resized client as its format client (#{client_tty}, #{client_width} answer) while zz fills only #{hook_client}. RE-SCOPE the acceptance to the measured pin (record old clause, measurement, probe in the reason), implement the format-client half, and close with a pty differential over changed and unchanged resizes and the Control control.
5. clients.path-encoding (1: the environment item; ClientHello environment as bounded byte strings on Unix, protocol bump) and config.tilde-home-path-encoding (1: the passwd lookup returns OsString; a found user with a non-UTF-8 home must not take the missing-user syntax-error path). Same shape as the cycle-5 cwd close (git show d895596 on origin/main).
6. control-mode.disconnect-cancels-command-queue (1) and pane.command-completion (1, split-window -W two-phase queue completion, board front F-PANE-COMMAND-COMPLETION): hard architecture; only if time remains, otherwise record precisely what is missing (the reason already names the per-connection Control worker and the queue wait item).
Scenario names: compat/scenarios/smoke/<group-slug>.txt + fixtures; add summary rows for non-smoke additions.`

const BATCH_CLIENT = COMMON + `
YOUR BATCH: chooser command flags, the cheapest parked pane-chrome item, and the menu and popup behavior baskets, up to four groups, 20 items. WORKDIR: zz-opus-termopts (warm zz-tui/zz/zz-terminal build). BRANCHNAME: batch-choosers-overlays-opus.
Your zones: crates/zz-client, crates/zz-tui, crates/zz-ui, crates/zz/src (GUI; NOT control_mode.rs), crates/zz-mux/src/{tmux_options.rs,honest_knobs.rs}, crates/zz-protocol/src/{message.rs,snapshot.rs,lib.rs}. Another worker owns crates/zz-daemon (daemon.rs, status.rs), control_mode.rs, parser.rs; a third owns crates/zz-mux/src/{command.rs,model.rs,formats.rs,...}, catalog.rs, key.rs, zz-terminal. Allowed excursions, minimal and listed in notes: crates/zz-daemon/src/daemon.rs ONLY in the chooser session, menu, popup, and pane-appearance regions; catalog.rs promotions; command.rs parse hunks for the chooser flags. The daemon lane may also edit daemon.rs this run in the prompt and hook regions; keep your hunks tight so the gate can rebase.

Groups in order:
1. choosers.command-flags (8: flag:choose-buffer:-F -k -y, flag:choose-tree:-F -h -k -y, semantic:chooser-key-vocabulary). The daemon owns the chooser session (rg chooser crates/zz-daemon/src/daemon.rs), zz-ui/src/chooser.rs, zz-tui render.rs, and crates/zz/src/chooser/ render it. Board recipes, in this order: -y for choose-buffer is parsed by the pin and never read for buffer mode (deletion stays immediate): accept and retain it as inert syntax, prove -y alone, clustered, and repeated, and that paste/delete results match the no-flag control through an attached client. -k retains kill-on-exit in the daemon chooser session and kills exactly the source pane when that mode exits: cover cancel, shortcut/Enter activation, empty-after-delete, overlay replacement, disconnect cleanup; without -k the source pane remains; chosen-action execution and error delivery stay ordered before teardown as pinned (mode-tree.c, window-tree.c, window-buffer.c). -h omits the invoking source pane from pane rows while its session/window ancestors stay visible; filtering computed before omission; fall back from the hidden source selection like pinned mode-tree; prove two-pane and one-pane source windows, another window, a filter matching only the source, and absent -h. -F expands once per visible row in that row's buffer/session/window/pane context including #{line} and makes that text the rendered row while preserving shortcut identity, order, filtering, selection, and defaults when absent; the rendered row text rides the wire (92 -> 93); prove distinct custom markers for buffer and session/window/pane rows on BOTH attached surfaces (raw TUI and GUI where a GUI test exists) and activate each by its existing key. chooser-key-vocabulary: read the group reason and the pin's mode-tree key table, close only with an attached-client proof of zz-deliverable key names.
2. options.pane-border-chrome, ONLY option:pane-colours (a park group; remove just that slug). Recipe from the board residual: libghostty already takes a 256-entry default palette through the per-pane appearance the daemon publishes, and OSC 4 precedence is proven by osc_palette_override_takes_precedence_over_configured_palette; resolve pane-colours[] at effective global-window/window/pane scope into TerminalWorkerOptions, overlay numeric 0..255 indices in pane_terminal_appearance, emit TerminalKnobsChanged for affected panes on set, append, indexed/whole unset, -U, relocation, and inheritance changes; preserve unspecified theme entries, whole-array local shadowing, named keys as stored nonconsumers, live repaint without PTY replacement, OSC 4 above configured defaults; never recolor native GPUI chrome. Prove two panes, inheritance and local palettes, raw-terminal cell colors, live fallback, OSC precedence, isolation, and a clean-base negative control.
3. display-menu.behavior-fidelity (5) and display-popup.behavior-fidelity (5). The measurable ones first: menu queue-ordering (blocking vs -b command-queue continuation), menu style-refresh, menu action-context-and-errors (selected-action client context, error delivery, overlay-close order); popup style-refresh, popup to-pane conversion. Each acceptance clause names its probe and needs a live attached client on both binaries; a headless proof does not close these. For mouse-policy, paste-close-ordering, border-drag, context-menu, and kitty-images, record precise pin measurements in the reason even when you cannot close them.
4. options.pane-border-chrome option:pane-border-lines, tiled portion, raw TUI only, ONLY if it needs no tiled z-order protocol change (that dependency is the separate parked presentation item); otherwise leave it with a note.
Scenario names: compat/scenarios/smoke/chooser-*.txt, smoke/pane-colours-palette.txt, smoke/display-menu-*.txt, smoke/display-popup-*.txt with fixtures; add summary rows so --check-summary stays green.

RESUME AFTER A MACHINE MOVE: a previous attempt at this exact batch was interrupted. Its work is pushed as origin/campaign/batch-choosers-overlays-opus-wip: commits 3c75e65 (choose-buffer/choose-tree -F, -h, -k), c0d1104 (pane-colours palette), b3358bb (a menu action runs against the menu's own client, plus the display-menu action-queue scenario and fixture), and a final snapshot commit 0f1dffd of uncommitted work in crates/zz-mux/src/command.rs, crates/zz-mux/src/compat_manifest_tests.rs, and crates/zz-tui/src/render.rs. Worktree: if ${M.dev}/zz-opus-termopts exists and is clean, git -C ${M.dev}/zz-opus-termopts checkout --detach origin/campaign/batch-choosers-overlays-opus-wip; otherwise git -C ${M.root} worktree add ${M.dev}/zz-opus-termopts origin/campaign/batch-choosers-overlays-opus-wip. Then git reset --soft HEAD~1 to reopen the snapshot as working changes; read git log -p origin/main..HEAD and the registry diff to learn exactly what the earlier attempt closed and how; finish or discard the reopened display-menu work into a proper commit; continue the batch order from where it stopped; re-verify EVERY proof at the final tip (the earlier commits' proofs predate this session and count as stale until re-run); push campaign/batch-choosers-overlays-opus (not the -wip name). Report every group the earlier attempt closed as your own fronts_done with proofs re-run at tip.`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz tmux-compat campaign (repo demfabris/zz). A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit ${M.root} itself.

SETUP: git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (SSH origin works; HTTPS https://github.com/demfabris/zz.git is the fallback if it hangs). Scratch worktree at the branch tip: git -C ${M.root} worktree add ${M.dev}/REVIEWDIR <branch-tip-sha> (if REVIEWDIR exists from a previous cycle and is clean, checkout --detach the tip there instead to reuse its build); remove a worktree you created when done (worktree remove --force).

MACHINE ETIQUETTE: others may still be working on ${M.cores} cores. cargo test -p <pkg> --jobs ${M.workerJobs} -- --test-threads=${M.workerThreads} only, no workspace-scale anything, cargo output to a log file + check exit code. Timeout-guard cli_binary tests (wait_exit can hang under load). Throwaway pin servers -L zzprobe-$$ only, kill after; never kill servers you did not start, never pkill tmux or zz.

METHOD, in order of value:
1. CONTRACT AUDIT: for every closed OR relocated slug (worker report + git diff origin/main...HEAD -- compat/tmux-gaps.json), find the proof in the diff that asserts the group acceptance clause. A test asserting zz behavior with no pin derivation is a defect; quote the clause. A relocation into an accepted-native group is legitimate only when that group's reason states the measured pin behavior and the product stance, and the manifest counts still enforce it. A RE-SCOPED acceptance clause is legitimate only when the reason records the old clause, the refuting measurement, and the probe; re-run that probe on the pin yourself. Anything proved for copy mode, prompts, choosers, menus, popups, or focus must drive a real attached client; a headless or detached proof of those is a defect.
2. PROOFS AT TIP: run the worker's claimed proof suites yourself at the branch tip (cargo test -p for every touched crate; the named scenarios). ALSO run cargo test -p zz-daemon --lib whenever the diff touches crates/zz-mux/src/command.rs or model.rs, even if the worker did not claim it (last cycle a mux-only lane broke two daemon tests). Any red at tip is a blocker regardless of the report.
3. ORACLE SPOT-CHECKS: 3-5 riskiest claims (option gating, queue and hook order, precedence, counts, prompt keys, transitions) verified against the pinned binary yourself (${M.root}/compat/.cache/tmux-src/tmux; the pinned C source sits beside the binary, read it when subtle). Last cycle's best catches were pin-side gating the worker's fixture had configured away (focus-events on) and a mode reset relying on a control sequence the pinned libghostty ignores: look for the configuration that would make a fixture blind.
4. TEST HONESTY: run the branch's new/changed tests and its scenarios (ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins compat/run.sh --strict-geometry <names>). Failing/ignored/tautological = blocker. Check the durable registry resolution carries every divergence the worker disclosed in its notes.
5. INVARIANTS: zone discipline (out-of-zone files listed in notes); wire rule (wire-reachable change => complete 92->93 bump incl. hex 0x5C->0x5D fixture and knowledge mirrors, changelog says inserted vs appended honestly); no code comments; doc comments still attached to the fn they describe; no attribution trailers; registry round-trips (python3 -m json.tool); tracker check green on the branch; --check-summary green if scenarios were added.
6. CALIBRATION: default to refuting each close, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = wrong close or would break main; must-fix = gate applies before merge; nit = mention. When a blocker is a wrong close, say whether reverting that commit applies cleanly at tip and what it takes with it (that is the fix the gate will use).
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes). checks_run lists exact commands. Thorough but bounded: well under an hour.`

const KEYS_WORKER = {
  "branch": "campaign/batch-keys-copy-formats-opus",
  "fronts_done": [
    {
      "front": "terminal.key-client-selection",
      "items_closed": [
        "flag:send-keys:-c"
      ],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry smoke/send-keys-client-selection smoke/send-keys-control smoke/copy-mode-command-errors",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "cargo test -p zz-protocol --jobs 4 -- --test-threads=2",
        "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2",
        "cargo clippy -p zz-mux -p zz-protocol -p zz-daemon -p zz --all-targets --all-features --jobs 4 -- -D warnings",
        "python3 compat/tmux-tracker.py check"
      ]
    },
    {
      "front": "terminal.key-control",
      "items_closed": [
        "semantic:send-keys-high-hex"
      ],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry smoke/send-keys-control",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "cargo test -p zz-protocol --jobs 4 -- --test-threads=2",
        "python3 compat/tmux-tracker.py check"
      ]
    },
    {
      "front": "copy-mode.command-fidelity",
      "items_closed": [
        "semantic:copy-mode-command-errors"
      ],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry smoke/copy-mode-command-errors",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2",
        "cargo clippy -p zz-daemon --all-targets --all-features --jobs 4 -- -D warnings",
        "python3 compat/tmux-tracker.py check"
      ]
    }
  ],
  "fronts_skipped": [
    {
      "front": "terminal.key-control (flag:send-keys:-K, semantic:send-keys-copy-command-shape)",
      "why": "-K needs a server-side replay of the selected client's key-table handler. zz's key path is request/response: the client asks the daemon for a KeyDecision and the daemon only executes a decision inside input_client_focus and the client key handlers, all of which need the invoking client's session/window/pane and a re-entrant execute_key_commands after the state lock is dropped. Building that injection path plus its cross-client rules did not fit the batch alongside -c. copy-command-shape is scoped by commands.tmux-name-extensions to copy-line, copy-line-and-cancel, copy-pipe-line, copy-pipe-line-and-cancel execution plus the pin's post-parse-failure redraw, none of which exist in zz's copy action set."
    },
    {
      "front": "terminal.key-control (semantic:send-keys-no-key-count, semantic:send-keys-empty-copy-count)",
      "why": "Behavior landed but is not differentially observable, and I recorded exactly why in the group reason. The pin's count lives on the pane's mode entry; zz keeps it on the client key engine, and I made the daemon spend it the same way (counted_copy_mode_action) with a zz-daemon unit test pinned to the pin measurements. No shared observable exists: display-message answers pane_in_mode=0 and empty copy_cursor_y/pane_mode/scroll_position on zz even with -c naming the attached client, and the attached client's own screen is not a substitute because zz renders the pane inside a sidebar layout roughly 50 columns wide while the pin renders 80x23, so after an identical 100-line scroll no marker line is common to both (pin top row L201, zz shows its own COPY 201/301 indicator)."
    },
    {
      "front": "display-message.format-listing",
      "why": "Measured the pin fully and rewrote the reason with it: format_each walks the 198-entry format_table in alphabetical declaration order skipping NULL callbacks, then ft->tree in RB order, so display-message -a -t %0 is 143 plain key=value lines with command=display-message out of order at the end. zz's FORMAT_VARIABLES is the same 198 names in the same order, so the walk is cheap, but zz's expander models no null: a name the pin declines resolves to an empty string, so the walk would emit all 198. The acceptance clause about the set difference also does not survive: 46 of the pin's 143 names are themselves tracked format gaps and zz answers several of them (pane_in_mode, pane_pipe, wrap_flag), so the difference is not the tracked gap list. A per-variable, per-context null-versus-empty distinction is the prerequisite."
    },
    {
      "front": "formats.window-runtime",
      "why": "Zone conflict, not a technical one. The window_bigger family producers live in crates/zz-daemon/src/status.rs, which the orchestration assigned to another worker, and the recorded blocker (the daemon resolves a format client for every command expansion through current_format_client instead of per command like format_defaults) is a rework of that same file's client resolution. Left untouched to avoid colliding with that lane."
    },
    {
      "front": "display-message.pane-target-grammar",
      "why": "Re-measured with the streams separated and sharpened the reason instead of closing: the pin prints `no current client` on stderr, clears the find state, and still runs the command, so `display-message -p -t @ 'S=#{session_name}'` writes `S=` to stdout at status 1 and a chained second command still runs. zz answers the identical stdout at status 0 with no stderr, so the whole divergence is the diagnostic plus the status, not the resolution. Closing it needs a channel that emits a message and a non-zero status while still producing the command output (CommandResponse::Error's output field) on top of the client-aware resolver decision, which is a daemon-shaped change larger than one item warranted."
    },
    {
      "front": "hooks.shutdown-window-unlinked-order",
      "why": "The existing reason already records the three missing prerequisites (a control-mode fixture that holds a client open across teardown, an emulation of OpenBSD RB_INSERT/RB_REMOVE keyed by winlink index maintained across every link/unlink/kill/renumber, and differential cases reaching one index map through distinct histories). None of them exist and none is a batch-sized piece of work; nothing new to add."
    },
    {
      "front": "copy-mode.action-fidelity",
      "why": "Not reached. Both items sit behind the same clients.interactive-refresh headless-target dependency that blocks the rest of copy-mode.command-fidelity."
    },
    {
      "front": "keys.copy-mode-unsupported-default-actions",
      "why": "Not reached; gated on the daemon-owned refresh revision the reason describes."
    },
    {
      "front": "formats.modifier-fidelity",
      "why": "Not reached. The reason already measures the blocker precisely: the I modifier's c flag needs tty_term_has_name against a per-client terminfo table, zz's daemon loads no terminfo database, and answering 0 for every capability would disagree with the pin on the common case while looking like support."
    },
    {
      "front": "display-message.verbose-trace",
      "why": "Not reached. Needs a structured trace sink in the shared expander for nested conditions, modifiers, lookups, and replacements; no cheaper path exists and it was last in the ordering."
    }
  ],
  "touched_commands": [
    "send-keys",
    "send-prefix",
    "copy-mode",
    "clear-history"
  ],
  "touched_packages": [
    "zz-mux",
    "zz-protocol",
    "zz-daemon",
    "zz"
  ],
  "notes": "PROTOCOL BUMP: yes, 92 -> 93, in commit a749996. The wire change is one appended variant `KeyToken::Raw(u8)` in crates/zz-protocol/src/message.rs, which is reachable through EventPayload::BrowserCommand (BrowserCommand::SendKeys / SendKeysRepeated). All mandatory sites moved together: the constant and its same-file assert test, crates/zz-protocol/tests/hunt_claims.rs (renamed to protocol_version_on_this_commit_is_ninety_three, asserts 93, hello-frame hex 0x5C -> 0x5D in both positions; I grepped the hex and there were exactly two), knowledge/protocol/wire-protocol.md title/constant/byte-row/changelog (says \"appends\" honestly), knowledge/protocol/index.md, knowledge/index.md, and knowledge/crates/zz-protocol.md twice. The second commit (68b4324) adds NO wire change: MuxEffect::CopyModeRepeat and MuxEffect::TerminalView gained fields, and MuxEffect is daemon-local, so the copy-mode work needed no further bump. If another lane also bumped to 93 this run, my changelog paragraph is self-contained and can sit beside theirs.\n\nZONE EXCURSIONS, all minimal and deliberate:\n- crates/zz-protocol/src/message.rs (third worker's zone): the KeyToken::Raw variant plus the version constant. Nothing else.\n- crates/zz/src/browser/tui.rs and crates/zz/src/browser/view.rs (third worker's zone): four `KeyToken::Raw(_) => {}` match arms, mechanically required by the new variant. Browser panes have no raw byte sink.\n- crates/zz-daemon/src/daemon.rs (second worker's zone), inside the allowed send-keys / copy-mode effect execution only: the SendKeys/TerminalView/CopyModeRepeat arms, three new free functions (selected_effect_client, pane_carries_a_mode_command, counted_copy_mode_action), the read_only_guard_client helper feeding prepare_command_request's read-only gate, one new unit test, and one changed expectation in an existing test (see below).\n- crates/zz-daemon/src/keys.rs: the KeyToken::Raw arm in send_tokens, which routes the byte through the existing TerminalSession::send_raw_input.\n- crates/zz-mux/src/lib.rs: one added re-export (send_keys_target_client).\nI did NOT touch crates/zz-daemon/src/status.rs, control_mode.rs, parser.rs, tmux_options.rs, crates/zz-client, zz-tui, zz-ui, or zz-terminal.\n\nCHANGED PINNED EXPECTATION the integrator should look at: crates/zz-daemon/src/daemon.rs, test read_only_send_keys_and_binding_preflight_are_all_or_nothing. It asserted that a read-only client running `send-keys -t <peer pane> -X cursor-left` gets \"client is read-only\"; it now expects \"not in a mode\". This is a fidelity fix, not a regression: cmd-send-keys.c's read-only guard is `tc != NULL && CLIENT_READONLY && !args_has(args, 'X')`, so -X skips it and cmd_send_keys_exec's own `wme == NULL` check fires first. Measured on the pin with two pty clients: `send-keys -c <read-only tty> foo` answers `client is read-only` at status 1 while `send-keys -c <read-only tty> -X cursor-up` answers `not in a mode` at status 1. The comment in the test carries that measurement.\n\nREGISTRY SUBTLETIES:\n- terminal.key-client-selection emptied and moved from gaps[] into closed[] with closed_on 2026-09-01; I re-sorted closed[] by id afterwards because the tracker requires it.\n- flag:send-keys:-c is now a real catalog option (CommandOptionSpec::value(\"-c\", FreeForm, \"target client\")), so the send-keys usage string grew `[-c target-client]`; send-keys stays in PINNED_TMUX_USAGE_OVERRIDES because -K and -M are still unsupported, so usage_overrides.len() is unchanged at 22. The hard-coded counters moved: (supported, unsupported) 452/51 -> 453/50 in crates/zz-protocol/src/catalog.rs. compat_manifest_tests enforces the pairing (an implemented flag with a surviving registry item fails \"implemented flag has a stale item\"), so cargo test -p zz-mux is the gate that proves the promotion and the close agree.\n- No re-scoping of an acceptance clause was needed for the two closed slugs. One clause WAS re-scoped: copy-mode.command-fidelity's single acceptance line said \"source pane, initial scroll position, command counts, and mode errors\"; with mode errors closed the clause now reads \"source pane, initial scroll position, and command counts\", and the reason records the measurement and the probe behind the removal.\n- knowledge/tmux/gaps.md was regenerated with `python3 compat/tmux-tracker.py write-report` after every registry edit; never hand-edited.\n\nDIVERGENCES DISCOVERED AND RECORDED IN THE REGISTRY (not only here):\n- With no -c at all the pin runs cmd_find_current_client, which for an unattached command client picks the attached client with the newest activity_time, so a bare `send-keys` can be refused by a read-only peer's flag while zz uses the invoking client. Activity-ordered and untestable without a timing race; the scenario asserts only the -c forms and the closed record says so.\n- `copy-mode -q -t <pane>` from a detached zz CLI answers `pane is not attached: %0` at status 1 where the pin answers 0. That is semantic:copy-mode-headless-target under clients.interactive-refresh; it is why the copy-mode-command-errors fixture drives a real pty client instead of a detached row.\n- The pin's `-H` argument parse is a silent skip, not an error, for an empty string, a non-hex string, or a value outside 0..0xff; zz used to fail the whole command. Also `send-keys -H -1` is eaten by the flag scanner and answers \"unknown flag -1\" on both.\n- window_pane_set_mode returns before `wme->kill = args_has(args,'k')` when the pane is already in the same mode, so re-entering copy mode never updates the kill bit. Added to the copy-mode.command-fidelity reason for whoever takes -k.\n- -k does NOT need a protocol bump: MuxEffect is daemon-local so the kill can ride an effect onto the daemon's CopySession. What it needs is a kill that runs when the session actually ends, which in zz is reconcile_copy_session seeing the terminal publish live mode again, and that function has no path back into the mux to remove a pane. Recorded in the reason.\n\nFLAKES: none seen. cargo test -p zz-daemon --lib ran 807 tests green at the tip in ~122s at --test-threads=2. I did not hit any of the known load-flakes. I ran no workspace-scale build or test.\n\nSCENARIOS ADDED (three, all with summary rows in compat/results/summary.md in sorted position, each 3 steps / all-clean columns):\n- compat/scenarios/smoke/send-keys-client-selection.txt (+ fixtures/send-keys-client-selection.sh) \u2014 21 checks, two real pty clients, one read-write and one attached with -r.\n- compat/scenarios/smoke/send-keys-control.txt (+ fixtures/send-keys-control.sh) \u2014 31 checks, raw bytes into a pane running `cat >file`, closed with `-H 04`.\n- compat/scenarios/smoke/copy-mode-command-errors.txt (+ fixtures/copy-mode-command-errors.sh) \u2014 36 checks, one attached pty client.\nAll three share compat/scenarios/smoke/fixtures/send-keys-attach.py, a small `record OUT COLS ROWS CMD...` pty helper. Each fixture asserts the pin's shapes itself and publishes `clean:N` into an environment variable, so a clean row is non-vacuous: I verified both sides print the same clean:N in compat/results/smoke/*.log rather than both printing nothing. I did NOT run the full `compat/run.sh --check-summary`; it re-runs the whole corpus and would have contended with the other lanes. The three new rows carry the step counts the individual runs reported.\n\nBOARD FRONT NAMES, for the orchestrator's mapping: F-KEY-CLIENT-SELECTION-V2 asked for smoke/send-keys-client-selection.txt + fixture and got exactly that; F-KEY-CONTROL-V3 asked for smoke/send-keys-control.txt + fixture and got that, but only the high-hex slug closed against it.\n\nI started only `-L zzprobe-*` tmux servers and `--socket /tmp/zz*-<pid>.sock` zz daemons and killed every one; `ps` shows none of mine alive. I killed nothing I did not start and used no pkill/killall."
}

const KEYS_REVIEW = {
  "lane": "keys",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "front": "terminal.key-client-selection",
      "severity": "must-fix",
      "description": "The closed resolution says the read-only guard now 'follows the selected client rather than the invoker', but only prepare_command_request (crates/zz-daemon/src/daemon.rs ~5260) was rewired. The bound-key path, execute_key_commands (~16440), still preflights every command in the chain against the invoking client's flag and never consults -c. Measured with real pty clients on both binaries at the branch tip: with root bindings `bind-key -n z send-keys -c <writer tty> -t %0 -l K` and `bind-key -n y send-keys -c /dev/pts/nonexistent -t %0 -l N`, a read-only client pressing z and y delivers on the pin (pane reads KNW, W being the writer's control press; the un-flagged `send-keys -t %0 -l U` from the same client is refused, key-bindings.c admits the binding because cmd_send_keys_entry carries CMD_READONLY and cmd_send_keys_exec then tests tc), while zz delivers only the writer's W. The retired acceptance clause covered 'attached and Control callers'; an attached caller's bindings are exactly the path left on the old rule, and the closed record does not disclose it.",
      "suggested_fix": "Either (a) make execute_key_commands' preflight guard-aware: replace `commands.iter().any(|command| !command_is_read_only_safe(command))` with a check that uses the existing read_only_guard_client(&inner, client, command).is_some_and(|guarded| inner.client_flags.contains(guarded)) && !command_is_read_only_safe(command), and extend read_only_send_keys_and_binding_preflight_are_all_or_nothing with a `send-keys -c <writer>` and a `-c /dev/pts/nonexistent` binding from the read-only client that must pass; or (b) amend the terminal.key-client-selection resolution to record that bound-key chains from a read-only client still test the invoker, with the KNW-vs-W measurement above."
    },
    {
      "front": "terminal.key-client-selection",
      "severity": "nit",
      "description": "Undisclosed in the new closed record: with the read-only client selected by -c, `send-keys -c <ro> -t %0 -X begin-selection` and `-X copy-pipe 'cat >/dev/null'` exit 1 `client is read-only` on zz, while the pin exits 0 with no stderr (window_copy_command posts `client is read-only` as a status message on the client for commands lacking WINDOW_COPY_CMD_FLAG_READONLY and returns normally). This matches the stance already closed under clients.read-only-local-view-actions, so it is not a wrong close, but the resolution's 'the guard is `!args_has(args, 'X')`' sentence reads as if every -X action goes through under -c.",
      "suggested_fix": "Add one sentence to the resolution: unsafe -X actions selected onto a read-only client are refused at status 1 per clients.read-only-local-view-actions where the pin answers 0 with a client status message."
    },
    {
      "front": "copy-mode.command-fidelity",
      "severity": "nit",
      "description": "The re-scoped reason says the pin's guard 'covers every -X spelling' and zz refuses when 'no client holds a non-exiting copy session on the pane'. The pin's guard also passes for view mode: `run-shell -t %0 'echo hi'` puts %0 into view-mode (pane_mode=view-mode, pane_in_mode=1) and `send-keys -t %0 -X cursor-up` answers 0, while zz opens no view mode on the pane for run-shell -t and answers `not in a mode` at status 1. Root cause is run-shell -t output routing, umbrella'd by clients.interactive-refresh ('copy and view mode live on the per-client terminal view'), not the guard itself.",
      "suggested_fix": "Add a sentence to the copy-mode.command-fidelity reason naming the view-mode case and pointing at clients.interactive-refresh."
    },
    {
      "front": "terminal.key-control",
      "severity": "nit",
      "description": "26 new `//` comment lines in Rust test code (crates/zz-mux/src/command.rs test module, crates/zz-mux/tests/hunt_claims.rs above the two new tests, crates/zz-daemon/src/daemon.rs in a_pending_repeat_prefix_is_spent_by_one_mode_command and the changed read-only expectation) against the repo's 'Do not add comments in code' rule. daemon.rs on main already carries 67 such lines, so this is precedent-consistent, but the measurements they hold belong in the test names or the registry reason.",
      "suggested_fix": "Fold the pin measurements into the assertion messages or the registry reason and drop the `//` blocks, or leave as is if the gate treats test-module comments as tolerated."
    }
  ],
  "checks_run": [
    "git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'",
    "git -C /home/demfabris/dev/zz worktree add /home/demfabris/dev/zz-review-keys 68b4324c8a13ecc5c76c110239ad9eef36c980e3",
    "git diff origin/main...HEAD (all 25 files, incl. compat/tmux-gaps.json, both commit bodies checked for trailers)",
    "cargo test -p zz-mux --jobs 4 -- --test-threads=2 (494+9+1+1+7+5+7+3+5+4+60 passed)",
    "cargo test -p zz-protocol --jobs 4 -- --test-threads=2 (211+7+16 passed; protocol_version_on_this_commit_is_ninety_three, hello hex 0x5D)",
    "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2 (806 passed, 1 failed at load average ~36: client_focus_closes_display_panes_and_preserves_chooser_modes)",
    "cargo test -p zz-daemon --lib client_focus_closes_display_panes_and_preserves_chooser_modes --jobs 4 -- --test-threads=2 (solo pass: load-induced flake, test is display-panes/focus, untouched by the diff)",
    "cargo clippy -p zz-mux -p zz-protocol -p zz-daemon --all-targets --all-features --jobs 4 -- -D warnings (clean)",
    "cargo fmt --all -- --check (clean)",
    "cargo build -p zz --jobs 4 (tip binary for the scenario and probe runs)",
    "ZZ_COMPAT_CORPUS=.../plugins compat/diff-scenario.sh --strict-geometry compat/scenarios/smoke/{send-keys-control,send-keys-client-selection,copy-mode-command-errors}.txt <tip zz> <pin tmux> (all 0 divergences; both sides publish clean:31 / clean:21 / clean:36; run twice, once with the worker's binary and once with my own tip build)",
    "python3 -m json.tool compat/tmux-gaps.json",
    "python3 compat/tmux-tracker.py check (valid, gaps.md current); python3 compat/tmux-tracker.py write-report produced no diff",
    "compat/run.sh --check-summary (summary current: 157 scenarios, 2370 steps)",
    "git grep 0x5C origin/main -- crates/zz-protocol/tests/hunt_claims.rs (exactly two occurrences on main, none at tip)",
    "pin source read: cmd-send-keys.c (entry flags CMD_CLIENT_CFLAG|CMD_CLIENT_CANFAIL|CMD_READONLY, exec guard, -H strtol skip), cmd-find.c cmd_find_client, cmd-queue.c CFLAG prep, window-copy.c window_copy_command prefix and per-command READONLY flags, key-bindings.c key_bindings_dispatch",
    "pin probe (-L zzprobe-$$, two pty clients + one -C control client): -c client-<pid> selects the control client and -c client-99999 is quiet and delivers; -c <ro> -R refused; -N 0 / -N abc errors precede the mode guard; -N 3 -X then cursor-up moves 3 rows, -N 4 + bare -X + cursor-up moves 4; other pane answers not in a mode; run-shell -t opens view-mode",
    "same probe against zz at tip (--socket /tmp/zzprobe-<pid>.sock): identical answers except ro -X begin-selection/copy-pipe (status 1 vs 0) and view-mode after run-shell -t (not in a mode vs 0)",
    "read-only binding probe on both binaries with a pty key-press helper: pin pane KNW, zz pane W",
    "git worktree remove --force /home/demfabris/dev/zz-review-keys"
  ],
  "notes": "Contract audit. flag:send-keys:-c: proved by smoke/send-keys-client-selection with two real pty clients (rw and -r), and my own probes extend it to a -C control client selected by its listed name on both sides, a quiet miss, -R refused on the selected read-only client, and `-c <ro> -X cursor-up` answering `not in a mode` on a mode-less pane on both binaries, which is the pin-source-backed justification for the changed expectation in read_only_send_keys_and_binding_preflight_are_all_or_nothing (guard is `tc != NULL && CLIENT_READONLY && !args_has(args, 'X')`, then `wme == NULL`). The one hole is the bound-key path listed as the must-fix. semantic:send-keys-high-hex: smoke/send-keys-control lands 41 80 ff 42 43 44 45 45 45 in a `cat >file` pane on both sides and cmd_send_keys_inject_string confirms the silent skip for empty, non-hex and out-of-range arguments; a detached observation is legitimate here (raw bytes, not a mode). semantic:copy-mode-command-errors: the re-scoped clause records the old wording, the measurement and the probe; the fixture drives a real attached pty client; my probe confirmed the per-pane guard (other pane not in a mode while %0 is, and after cancel) and the -N error ordering on both binaries. Relocation of terminal.key-client-selection into closed[] is legitimate; catalog counters moved 452/51 -> 453/50 and usage_overrides stays 22 with compat_manifest_tests enforcing the pairing. Wire: the v93 bump is complete (constant, same-file assert, renamed hunt_claims test, hex 0x5C->0x5D in both hello positions, knowledge title/constant/byte-row/changelog saying 'appends', protocol index, root index, zz-protocol.md twice); KeyToken rides BrowserCommand::SendKeys/SendKeysRepeated so the bump was required. Registry round-trips, tracker check green, gaps.md current, --check-summary green (the worker's report says it re-runs the corpus; it is a file check and took seconds). Counts (semantic:send-keys-no-key-count, -empty-copy-count) were left open with an honest reason; the pin measurements in that reason match mine (a -N with no key or with an empty -X is spent by the next mode command). Flake: one daemon test failed under the full --lib run at load average ~36 and passed solo; it arms a 5000ms display-message and asserts no reliable messages, unrelated to send-keys or copy mode. Suspicions not raised as defects: zz's control-mode stream emitted five %pane-mode-changed lines to the pin's three across copy-mode/cancel/run-shell in my probe (per-client copy sessions), outside this lane's slugs; `-c client-1` is a CANFAIL miss on the pin but zz's native alias would match ClientId 1. Zone excursions match the worker's list. No attribution trailers, doc comments stay attached to their functions, no stray debug output."
}

const DAEMON_WORKER = {
  "branch": "campaign/batch-daemon-prompt-hooks-opus",
  "fronts_done": [
    {
      "front": "F-ALIAS-GROUP-FORGERY",
      "items_closed": [],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry alias-group-forgery",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2",
        "cargo test -p zz-protocol --jobs 4 -- --test-threads=2",
        "cargo test -p zz --lib --jobs 4 -- --test-threads=2"
      ]
    },
    {
      "front": "prompt.command-fidelity",
      "items_closed": [
        "semantic:status-keys-editor-derived-default",
        "flag:command-prompt:-l",
        "semantic:command-prompt-chain",
        "semantic:command-prompt-labels",
        "semantic:command-prompt-pass-order"
      ],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry status-keys-editor-default command-prompt-chain",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2",
        "cargo test -p zz --lib --jobs 4 -- --test-threads=2",
        "python3 compat/tmux-tracker.py check"
      ]
    },
    {
      "front": "clients.event-resize-context",
      "items_closed": [
        "semantic:client-resized-post-geometry-context"
      ],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry client-resize-context",
        "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "compat/run.sh --check-summary",
        "python3 compat/tmux-tracker.py check"
      ]
    }
  ],
  "fronts_skipped": [
    {
      "front": "hooks.pane-events",
      "why": "Not attempted. The pin model is now fully written into the group reason (every window_pane_update_focus call site with file:line, and which of them focus-events gates), but the ordering half needs a redesign, not an addition: zz produces window-pane-changed and session-window-changed from a pure before/after MuxHookSnapshot diff (window_hook_events) that takes only two snapshots plus the command name and has no access to ServerState, so it cannot see attached clients, client focus, overlays, or the focus-events option, and cannot place a focus transition before or after the neighbour it must sit next to. That is exactly what the last gate reverted, so a half-attempt would have been reverted again. I judged two items there worth less than the three I could finish, and spent the time on groups 2 and 4."
    },
    {
      "front": "clients.path-encoding",
      "why": "Not attempted. The client hello builder is client_environment_with in crates/zz-daemon/src/client.rs (my zone, so the excursion worry was unfounded), but the acceptance also asks for the pin's update-environment byte behaviour, and zz's session environment is BTreeMap<String, String> in zz-mux, the same String wall that blocks config.tilde-home-path-encoding. I tried to measure the pin's update-environment with a 0xff-bearing variable and got an inconclusive result (neither binary carried the variable into a detached new-session), so I deliberately did not write an unverified claim into the reason. Whoever takes this should measure the copy on attach, not on new-session -d."
    },
    {
      "front": "config.tilde-home-path-encoding",
      "why": "Blocked, unchanged. Verified the premise still holds at this tip: CommandInvocation.args is still Vec<String> in crates/zz-protocol/src/message.rs, so a passwd home with non-UTF-8 bytes has nowhere to land after expansion. The group's own reason already records this as semantic:non-utf8-command-arguments under protocol.binary-streams; nothing new to add."
    },
    {
      "front": "control-mode.disconnect-cancels-command-queue",
      "why": "Architecture, not reached. I did re-confirm the drain half from the outside and recorded it in the reason: piping refresh-client, a second refresh-client, a display-message -p and detach-client into -C attach-session runs all four on the pin and only the first on zz, which forced smoke/client-resize-context to open one Control connection per size report. Any Control differential needing two commands down one pipe is now blocked on this group, which raises its priority."
    },
    {
      "front": "F-PANE-COMMAND-COMPLETION",
      "why": "Not reached; time went to the three fronts above. Nothing new measured, so pane.command-completion's reason is untouched."
    }
  ],
  "touched_commands": [
    "command-prompt",
    "display-message",
    "set-hook",
    "set-option",
    "show-options",
    "source-file",
    "refresh-client"
  ],
  "touched_packages": [
    "zz-protocol",
    "zz-mux",
    "zz-daemon",
    "zz"
  ],
  "notes": "FIVE COMMITS on campaign/batch-daemon-prompt-hooks-opus, branched from origin/main at fdf56a1.\n\nPROTOCOL BUMP: yes, 92 -> 93, in commit c9f74b0 (the alias-group fix). CommandInvocation gains a private `expanded_alias_group: bool`, appended after `command_blocks`, set only by `into_expanded_alias_group`, which only alias expansion calls. All the mandatory sites are done: message.rs constant + same-file assert test, hunt_claims.rs renamed to protocol_version_on_this_commit_is_ninety_three with 0x5C -> 0x5D in both hello-frame positions (grepped the hex, one line, two occurrences), wire-protocol.md title/constant/byte row/CommandInvocation shape/changelog entry, knowledge/protocol/index.md, knowledge/index.md, knowledge/crates/zz-protocol.md twice. No later commit on this branch is wire-reachable, so no second bump. If another lane also bumped, my changelog entry is self-contained and the gate reconciles.\n\nZONE EXCURSIONS, all minimal and deliberate:\n- crates/zz-protocol/src/message.rs: the CommandInvocation provenance field and PROTOCOL_VERSION (a fact that has to ride the wire).\n- crates/zz-protocol/src/catalog.rs: command-prompt -l promoted from unsupported_flag to a real flag, usage string \"[-1CbeikN]\" -> \"[-1CbeiklN]\", and the hard-coded (supported, unsupported) counter 452/51 -> 453/50. usage_overrides length is unchanged at 22 and PINNED_TMUX_USAGE_OVERRIDES needed no edit because command-prompt was already an override.\n- crates/zz-mux/src/command.rs: the alias-group predicate, the command-prompt step construction and multi-answer substitution, set_default_status_keys, and ExecutionContext::hook_format_client. The last one is beyond \"prompt flag parsing\"; it is the smallest way to carry notify_client's fs.c to the hook body and it is one field plus two accessors.\n- crates/zz-mux/src/lib.rs: re-export CommandPromptStep.\n- crates/zz/src/lib.rs: ONE test adapted (prepared_cli_routing_scans_alias_groups_but_stdin_uses_the_final_command). Its \"nested alias group\" case forged the group by spelling the sentinel inside an alias body, which is the defect; it now asserts the forged child stays an ordinary unknown command. No production code in crates/zz was touched. Worth flagging to the GUI lane for a conflict.\n- crates/zz-tui/src/input.rs and crates/zz/src/command/palette.rs were NOT touched, and did not need to be: see the finding below.\n\nFINDING THAT CHANGES THE BATCH BRIEF: prompt line editing is not in the clients. The daemon owns the buffer, the cursor and the history (command_prompt_key / command_prompt_edit_key in crates/zz-daemon/src/daemon.rs); clients only render CommandPromptState and send keys. So semantic:command-prompt-vi-editing and option:status-keys are daemon work, not zz-tui or palette work. This is now in the group reason.\n\nSECOND FINDING, also in the reason: zz cannot raise a prompt from a command client at all. The MuxEffect::CommandPrompt arm rejects any client that is not the Interactive subscriber running the command, with \"command-prompt requires an interactive client\", while the pin's CMD_CLIENT_TFLAG resolves a target client and blocks the issuing command client until the prompt closes. flag:command-prompt:-t is therefore not a flag on top of working routing, it IS the routing, and until it lands every prompt differential has to go through a key binding on the attached client. That is why smoke/command-prompt-chain binds F1 rather than invoking command-prompt from the CLI.\n\nRE-SCOPING (one, with the full trail): clients.event-resize-context. The old clause (\"emits client-resized once after visible pane and window geometry has settled\") is quoted verbatim inside the new resolution, together with the pty measurement that refutes it (client size current, window geometry exactly one resize behind, on BOTH binaries) and the probe. The group moved to closed[] with a normalized single-line resolution and closed_on 2026-09-01. The differential deliberately truncates its hook log once the client is attached, because the pin reports the attaching client's first size as a client-resized of its own and zz folds that into attach; that divergence is stated in the resolution rather than hidden.\n\nREGISTRY MECHANICS: prompt.command-fidelity went 11 -> 6 items and stays open. clients.event-resize-context emptied and moved gaps[] -> closed[]; note that closed[] entries take only id/title/closed_on/evidence/resolution and must stay sorted by id, which the tracker enforces. hooks.pane-events and control-mode.disconnect-cancels-command-queue got reason additions only, no item changes. knowledge/tmux/gaps.md was regenerated with `python3 compat/tmux-tracker.py write-report` after every registry edit, never hand-edited. `cargo test -p zz-mux` (compat_manifest_tests) was run after every registry edit; it is what caught that flag:command-prompt:-l had to leave the registry in the same change that promoted it in catalog.rs.\n\nFLAKY TESTS, one worth escalating: client_focus_closes_display_panes_and_preserves_chooser_modes is NOT only a load flake. It fails roughly 6 times in 8 running exact-solo with `--test-threads=1` and a name filter, on origin/main with none of my changes applied (I stashed to check). The leftover is a late Event(Snapshot) plus a StatusChanged arriving after an ignored ClientFocus. I lost about twenty minutes chasing it as a regression of mine; the known-flake list should say \"flakes solo too\". daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts behaved as documented: failed once under --test-threads=2, clean solo. The final full daemon run at the tip was 809 passed, 0 failed.\n\nDOWNSTREAM RULE: satisfied. The diff touches zz-mux effect shapes (MuxEffect::CommandPrompt lost `prompt`/`input` and gained `steps: Vec<CommandPromptStep>`) and ExecutionContext, so `cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2` ran green at the final tip, not only earlier.\n\nFOUR NEW SMOKE SCENARIOS, all with summary rows added and `compat/run.sh --check-summary` green (158 scenarios, 2375 steps, attached-client PASS): smoke/alias-group-forgery, smoke/status-keys-editor-default, smoke/command-prompt-chain, smoke/client-resize-context. Two rig notes for whoever writes the next prompt scenario. First, smoke/command-prompt-chain uses the pinned tmux as the OUTER multiplexer on both sides (ZZ_SMOKE_TMUX_BIN is exported on the zz side too), with the binary under test attached inside an outer pane, because capture-pane cannot see the inner client's own status line; it compares the first 20 columns of the outer pane's last row so zz's own chrome to the right does not enter the comparison. Second, smoke/status-keys-editor-default starts nested `-f /dev/null` servers per case, which matters: this box's ~/.tmux.conf sets `setw -g mode-keys vi` and `set -g status-keys emacs` and completely hides the editor derivation from a bare probe.\n\nDISCRIMINATION CHECK: smoke/alias-group-forgery was verified to catch the defect, not just to pass. I temporarily reverted the predicate to the name-only test, reran, and got 4 OUT divergences, then restored. For the other three I have the before/after measurements recorded in the reasons (the prompt chain drew \"first,secondAA,BB\" and answered <AA,BB|%2> before; the resize hook answered \"tty= cw= ch=\" before).\n\nONE EXTRA FIX BEYOND THE ITEM, in commit 4b8d5b8: display-message inside a hook now prefers the notified client for its format tree, matching cmd_display_message_exec's `tc = cmdq_get_target_client(item)` with `-c` still overriding. Without it the run-shell path was right and the display-message path still picked best_display_message_format_client. The new daemon test client_hooks_expand_the_notified_client covers it with two attached clients.\n\nMACHINE ETIQUETTE: every build and test used --jobs 4 and --test-threads=2, no workspace-scale runs, no output piped through tail or grep without a log file and an exit check. Every tmux and zz server I started was a throwaway on its own -L label or --socket path, killed by a shell trap; I verified at the end that none of mine were still live before removing their stale socket files. One self-inflicted hazard worth passing on: `pkill -f \"zzouter-\"` matched the bash-tool's own command line and killed my shell mid-heredoc, which silently left an old script in place and cost a confusing debug cycle. Do not pkill on a pattern that appears in your own argv."
}

const LANES = [
  { key: 'keys', lock: 'F-MUX-KEYS-COPY-FORMATS', worker: KEYS_WORKER, review: KEYS_REVIEW },
  { key: 'daemon', lock: 'F-DAEMON-PROMPT-HOOKS', worker: DAEMON_WORKER, reviewdir: 'zz-review-dint' },
  { key: 'client', lock: 'F-CLIENT-CHOOSERS-OVERLAYS', prompt: BATCH_CLIENT, reviewdir: 'zz-review-client' },
]

log('Cycle 6 continuation: keys lane replays its worker report and review; daemon lane replays its worker report and runs its review; client lane resumes from origin/campaign/batch-choosers-overlays-opus-wip')
const laneResults = await pipeline(
  LANES,
  lane => lane.worker
    ? Promise.resolve({ lane, worker: lane.worker })
    : agent(lane.prompt, { label: `worker:${lane.key}`, phase: 'Work', model: 'opus', schema: WORKER_SCHEMA }).then(w => ({ lane, worker: w })),
  r => {
    if (!r || !r.worker || !r.worker.branch) return r
    if (r.lane.review) return Promise.resolve({ ...r, review: r.lane.review })
    const reviewPrompt = REVIEW_COMMON + `

LANE: ${r.lane.key}. BRANCH: ${r.worker.branch}. REVIEWDIR: ${r.lane.reviewdir}.
WORKER REPORT (verify, do not trust; it was written on another machine, so its literal paths mean the worktree parent here):
${JSON.stringify(r.worker, null, 2)}`
    return agent(reviewPrompt, { label: `review:${r.lane.key}`, phase: 'Review', model: 'fable', schema: REVIEW_SCHEMA })
      .then(review => ({ ...r, review }))
  }
)
const summaries = laneResults.filter(Boolean).filter(r => r.worker && r.worker.branch)
  .map(r => ({ key: r.lane.key, lock_front: r.lane.lock, review: r.review || null, ...r.worker }))
log(`Lanes complete: ${summaries.length}/3 branches reviewed. Integrating serially.`)

phase('Integrate')
const gatePrompt = `You are the integration gate for the zz tmux-compat campaign (repo demfabris/zz, board = GitHub issue 7). All workers and reviewers are done; you run ALONE on this ${M.machine}, full speed. Integrate IN THIS ORDER: keys first (it carries the large command.rs refactor the other two lanes made excursions into), daemon second, client third. One gate per branch, and push main as soon as each lane's gate is green (if you die mid-run the orchestrator resumes from what already landed).

Lane summaries, worker report + Fable review verdict each (the keys and daemon reports and the keys verdict were written on the previous machine, so literal /home/demfabris/dev paths inside them mean ${M.dev} here; re-derive every command from this prompt, never paste theirs):
${JSON.stringify(summaries, null, 2)}

REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on the branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge; post the blockers as a board note on the lock front, leave the branch, continue. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a wrong close whose revert applies cleanly is such a fix (precedent 0fec342 + 9cab1fa: revert, then a records commit that puts the reviewer's measurement into the group reason and acceptance). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating.

BOARD IDENTITY: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (renew sets expiry to now + lease); front needs --contract --zones [--priority --kind --deps --notes]; withdraw needs TRIAGE held. The orchestrator holds the three lock fronts F-MUX-KEYS-COPY-FORMATS, F-DAEMON-PROMPT-HOOKS, F-CLIENT-CHOOSERS-OVERLAYS (6h leases from launch, the orchestrator renews them; expired => claim back as ${M.holder} before that lane's ledger step).

NETWORK GIT: origin is SSH and works non-interactively. Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. If a network command hangs ~30s, kill it and retry once over HTTPS (https://github.com/demfabris/zz.git). The shared checkout's local main branch may be stale; never use it, never touch it.
knowledge/tmux/gaps.md is generated: regenerate with tmux-tracker.py write-report on every conflict, never hand-merge it.
PROTOCOL RECONCILE: if MULTIPLE branches bumped PROTOCOL_VERSION to 93, the rebase keeps ONE constant at 93 with every changelog entry preserved as separate v93 bullets and the hunt_claims fixture updated once (0x5D); verify cargo test -p zz-protocol after reconciling. catalog.rs counters and compat_manifest_tests.rs counts conflict between lanes routinely: resolve by recounting, then cargo test -p zz-mux and -p zz-protocol. Two lanes may have touched crates/zz-daemon/src/daemon.rs in different regions; rebase conflicts there are resolved by keeping both hunks and letting the workspace run judge.

PER BRANCH, in order:
1. Fresh worktree: git -C ${M.root} worktree add ${M.root}-gate-<key> origin/main (remove leftovers with --force first). Rebase branch onto origin/main; compat/tmux-gaps.json conflicts between lanes are normal: merge both sides.
2. First branch only: claim MAIN --lease 2h; hold across all, renew before each subsequent gate, release after the ledger recompute.
3. Code-branch gate stages, in order:
   a. cargo test --workspace --all-features --no-fail-fast --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (check exit code; never pipe through tail). Timeout-guard: wait_exit_holds_the_control_process_until_a_second_blank_line can HANG under load; if the run wedges >20min with no output, sample the process; a lost-wakeup hang there counts as the known flake (verify solo).
   b. cargo clippy --workspace --all-targets --all-features -- -D warnings
   c. ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <lane touched_commands>. Run --list TWICE and reconcile against git diff --name-only for compat/scenarios (last cycle one --list came back stale and missed six new scenarios); shard up to ${M.shards} concurrent run.sh invocations over DISJOINT scenario subsets; run smoke/source-replay-diagnostics SOLO after the shards (pin-side crash under load); any divergence re-runs alone before being called real.
   d. python3 compat/tmux-tracker.py check && python3 compat/board_test.py && compat/run.sh --check-summary
4. Flake rules: lone timing test failing loaded + passing exact-solo = flake, proceed (known list: copy-mode reconcile, attached_client_extents…, client_focus_closes…, history_request_is_guarded…, daemon_native_split_resize_commits_exactly…, nested_alias_queue_bubbles_shutdown…, control_sourced_run_shell_closes_before_raw_output…, wait_exit… hang, concurrent_default_interactive… "not a terminal" incl. misattribution). Anything else red = real: fix if minutes, else SKIP branch (no push), record, continue.
5. Push main. Non-fast-forward: fetch; user-authored commits + conflict-free disjoint rebase => bounded rerun (lane package tests + its scenarios), push. Never force. Campaign branches rewritten by the rebase stay at their old tips on origin (never force them); say so in the report.
6. Ledger per successful push against that lane's lock_front: candidate (--commit tip --branch campaign/<name> --base <pre-push main> --proof per stage), note (--note: groups covered, slugs closed/relocated, reviewer verdict + what you did about defects), integrated (--merge <sha> --gate "workspace+clippy+delta green"), release (--reason "batch integrated at <sha>").
7. After ALL branches, holding MAIN: recompute TMUX_COMPAT_TRACKER.md from the merged registry: the four headline lines (Campaign delivery, Live work "<open> OPEN + <blocked> BLOCKED = <sum>", Ledger settlement "100 x (<closed> CLOSED + <accepted> ACCEPTED) / (<closed> CLOSED + <len(gaps)> LIVE)" one decimal, Exit evidence scenarios/steps from --check-summary), the Current checkpoint rows, the Campaign dashboard table rows (Live unresolved, Latest differential, Differential SHA-256 = sha256 of compat/results/summary.md, Ledger settlement), the Ledger settlement calculation block, and a new "### <date> cycle-6 integration checkpoint" table like the cycle-5 one above it. Records commit ("Recompute the live ledger after the cycle-6 merges"), push, release MAIN.
8. Claim TRIAGE. Withdraw fronts fully mooted by the merges, slug-verified one by one against the merged registry (a defect front is mooted when its scenario is on main): candidates F-ALIAS-GROUP-FORGERY, F-KEY-CLIENT-SELECTION-V2, F-KEY-CONTROL-V3, F-CHOOSER-KILL-ON-EXIT, F-CHOOSER-ROW-FORMAT-V2, F-CHOOSER-TREE-HIDE-SOURCE, F-CHOOSER-BUFFER-ACCEPT-FLAG, F-PANE-COLOURS-PALETTE, F-DISPLAY-MESSAGE-PANE-TARGETS-V3, F-PANE-COMMAND-COMPLETION, F-PANE-BORDER-LINES-TILED. None of those is a dependency of another READY front except F-PANE-BORDER-LINES-TILED (depends on F-PANE-BORDER-ZORDER, which stays). If a worker's skip reasons prove a group contract is unprovable as written, post that as a residual on the relevant front. Release TRIAGE.
9. python3 compat/progress.py, full output.
10. Remove only your own zz-gate-* worktrees. Leave zz-opus-* and zz-review-*.

Never stash/reset anything in ${M.root}. Never kill tmux or zz servers you did not start (the user's may be live). Report via structured output: per branch merged/sha/gate_summary/review_actions/flakes, full progress output, board records, problems.`

const gate = summaries.length
  ? await agent(gatePrompt, { label: 'gate:serial', phase: 'Integrate', model: 'fable', schema: GATE_SCHEMA })
  : null
if (!gate) log('No branches to integrate: all workers came back empty.')

return { lanes: laneResults, gate }
