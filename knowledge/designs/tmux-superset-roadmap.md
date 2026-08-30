---
type: Design Plan
title: tmux-compatible CLI and native superset roadmap
description: The dependency plan and delivery history for making alias tmux=zz practical while keeping picker, browser, agent, editor, and fleet behavior on explicit zz-only commands.
status: In Progress; parallel wave 2 frozen at 0 of 3
tags:
- tmux
- compatibility
- roadmap
- cli
- fleet
- native-superset
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-08-30
last_updated_by: Codex
---

# Outcome

Build a zz CLI that is compatible with the tmux workloads people actually carry, then add native
commands that make the GUI better than tmux:

```text
tmux names        -> tmux semantics or a loud error
zz-only names     -> picker, browser, agent, editor, GUI, and fleet semantics
zz default keys   -> may call zz-only names
imported bindings -> preserve the command they name
```

The goal is not all tmux internals. It is a reliable `alias tmux=zz` for daily shell use, imported
config, the pinned plugin corpus, and common automation. Linked sessions and a real tmux socket stay
out permanently; multi-user ACLs are parked outside the practical alias gate.

The schema 3 [tmux compatibility gap report](/tmux/gaps.md) owns live status, ordering, priorities,
and closed history. The
[2026-08-22 tmux CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
records the source-anchored baseline used to build this plan.

# Current checkpoint, 2026-08-29

Slices 10w through 10ah plus the Config and Key fronts form the local 2026-08-29 checkpoint. The
three-front trial closed all 3 frozen chunks and registered 3 residual groups. Unresolved work
stayed at 65: 45 open and 20 blocked. The tracker has 87 active groups, 590 classified
active items, 119 closed groups, and 22 accepted active groups. Its secondary ledger settlement is
141 of 206 known groups (68.4%). The persisted accepted artifact covers 104 scenarios and 1,672 steps, with attached-client
`PASS`, exactly two approved GEO rows, every other channel clean, and SHA-256
`8365f95b9297641a7f4462d7b337d4a711a9edf34c41fc7ab4d8ec4818700a5c`. Slice 10af closes
`jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`.
Slice 10ag closes `source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd`
without a public protocol change. The integrated mux suite passes 531 tests, the focused strict-key
run passes 40 steps plus 161 fixture checks on both engines, and formatting and mux clippy pass.
Slice 10ah closes
`control-mode.kill-server-response-order`; slice 10ai is now the sole `next` group under
`control-mode.exit-pane-output`.
The Config front closes `config.parser-edge-cases` for UTF-8 daemon parser contexts. The parser now
matches closing-quote expansion, hidden token-state transitions, daemon `HOME`, passwd fallback,
named users, failed lookup, and the 1,022-byte username limit. Direct Control environment provenance
and non-UTF-8 passwd home paths remain in separate active groups.
The Key front closes `keys.strict-validation`. Its tmux command parser matches short modifiers,
named and caret forms, exact function-key bounds, and the pin's prefix-consuming User and hex number
grammar. It rejects invalid names before state changes and keeps printable ASCII hex keys distinct
from literal keys. Literal DEL, caret plus DEL, and `0x7f` remain under
`keys.literal-delete-identity`.
The three-front trial is positive: all three bounded chunks reached `main`, their changed paths did
not intersect, and integration had no merge conflicts. Six independent review repairs were needed,
so the next wave uses two active editors, one permanent oracle and reviewer, and the root as
coordinator. Full corpus and workspace gates remain centralized in one warm integration lane.
Wave 2 freezes Control exit pane-output discard, shell-job cwd, and literal DEL identity as three
independent chunks. The first two start with editors. The DEL front probes and reviews before it
rotates into editing.
The final workspace run passed every non-daemon package. Three daemon tests failed only under the
parallel load and each passed when rerun alone, matching the repository's documented load-flake
class. Strict workspace clippy, formatting, tracker, stored-summary, and OKF validation pass.
The Alert cohort closed without a protocol bump. Bell, Activity, and Silence
messages now share the daemon-owned status-message identity, timer, replacement, dismissal,
terminal-publication freeze, and full-viewport thaw. Repair requests, resync, and popup viewports
obey the same gate. Each eligible attached Interactive client
appends an exact `<client> message: <text>` entry to the bounded server log, using its registered
name or `device-<id>` fallback. Control clients receive no alert status message or alert log entry.
`TerminalSession` emits one reliable Bell event per occurrence while the mux owns the visible flag,
so repeated BELs from one unvisited monitored pane still notify while that flag remains set.
The attached PTY fixture replaces a 1,500 ms sticky message with a 5,000 ms alert, proves 1.8
seconds of freeze, repeats the same-pane Bell, drains the pin's old timer for 5.2 seconds, and proves
zero-duration persistence and input dismissal on zz and pinned tmux. Ordinary publication, repair
requests, resync, and popup viewports remain frozen until the message clears. The pin's stale-timer
bug remains a deliberate correctness divergence because zz cancels and identity-checks old timers.

The accepted runtime artifact for the authorized 2026-08-29 checkpoint through the three-front
closures contains 104 scenarios and 1,672 steps.
Every ordinary row is clean. `known/known-main-preset-two-panes` and
`known/known-spread-mixed` each retain their one approved GEO divergence with every other channel
clean. The expanded attached-client fixture and `compat/run.sh --check-summary` both pass. The
persisted summary SHA-256 is
`8365f95b9297641a7f4462d7b337d4a711a9edf34c41fc7ab4d8ec4818700a5c`. Requested flags, attached
sizing, and client environments extend the attached fixture, while the daemon invalid-flag closure
and both positional-bound closures each add one fail-closed three-step canonical scenario. The
three-step shared flag scenario passes 516 focused probes on zz and the pin inside that full run.
The combined chooser row contributes three harness steps and 26 internal checks with zero TOPO,
GEO, FMT, OUT, or WARN differences.
Slice 10l registers the pinned hook-producer partition in source. Slice 10m then makes the full
default-key structural partition explicit and matches tmux's bare key-only `bind-key` mutation.
Slice 10n closes raw-TUI confirmation presentation and input handling. None adds a scenario step,
so the historical 10s artifact remained 98 scenarios and 1,517 steps at SHA-256
`9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`. The bind-key row runs
17 internal checks, while the attached fixture now exercises seven confirmation replies with a pane
sentinel that proves reply keys do not leak. Slice 10o closes raw-TUI consumption of the
daemon-published menu descriptor and shared keyboard ownership. Its attached cases cover a title,
shortcut precedence, unusable-row skipping, cancel, an unusable PageUp landing with stay-open
Enter, nonactivating paste, and pane-input isolation without adding a differential row. Focused
resolver coverage pins exact raw-row-zero and all-disabled boundary behavior. Popup presentation
closed in slice 10p. Its attached A/B/C cases cover live title-only modification, bracketed paste,
exact content-relative pointer and wheel input, dead `-k` retention, live external focus
suppression, dead focus-close, and pane-input isolation. Six broader popup contracts remain under
`display-popup.behavior-fidelity`, while broader menu behavior stays under
`display-menu.behavior-fidelity`.
Slice 10q closes the per-client `no-detach-on-destroy` fallback with two real raw clients and no new
differential row. Slice 10r closes the cold local CLI parse-abort contract. An alias-free raw pass
validates the complete vector against all 83 implemented and nine recognized parked tmux verbs,
including exact native attach tails and `-N`, before routing, stdin, TUI handoff, or daemon spawn. A
startup alias name cannot trigger autospawn, while a canonical spelling can be shadowed in the
daemon pass. A successful pass starts a generation-tagged daemon, which prepares the complete vector under one
post-config alias snapshot. Its exclusive bootstrap lease survives startup reentry, becomes
sticky when a second external client connects, commits before a pipelined command can race the
worker, and shuts down after a failed preparation only when the owner disconnects uncontested.
The attached-client fixture now also compares nested validation status, stderr, session roster,
client count, aliases, and command-list stop behavior on both servers.
Slice 10s closes the nonconstant global-format behavior registration without changing runtime, the
pinned oracle, protocol, scenario inventory, or accepted artifact. Source-owned inventories
partition all 198 pinned global names into 92 values resolved directly by the mux, 32 delegated
through `StatusHooks`, and 74 constant-backed names that remain live `format:` gaps. The three sets
are pairwise disjoint and their union equals the pinned oracle. A required exact daemon test seeds
buffer, client, and session facts, then proves that the production `DaemonFormatHooks` consumer
resolves every delegated name. This registration makes no context-specific value-parity claim.
Slice 10t closes `format:session_path`. The mux expands the selected session's retained cwd at use
time, so target changes and `attach-session -c` updates appear without another cache or protocol
field. The `formats-values` differential grows by five steps to 18. It proves two creates with
distinct cwd facts, two explicit targets, filtered `list-sessions` output, and lexical `..`
preservation. Mux tests cover absent retained state and the production attach update. The format
partition is now 93 direct mux values, 32 daemon-delegated values, and 73 live gaps.
`format:session_active` remains open for a tri-state producer audit. At the 10t checkpoint, the new
`sessions.new-session-attach-cwd` group owned two adjacent mismatches: zz lacked the pinned
`new-session -A -c` cwd mutation, and fresh `new-session -c ''` inherited a cwd instead of retaining
the pin's explicit empty value. Slice 10x closes both paths below.
Slice 10u closes `mux.command-group-argument-parse-abort` on 2026-08-28. Warm local Command-client
vectors now validate ordinary unaliased tmux grammar across the complete vector before any effect,
while runtime target and effect errors keep sequential queue ordering. Only vector index 0 when it
is exact unaliased `attach` or `attach-session` retains the private positional parser; later exact
spellings and aliases use the catalog. Control, remote `--host`, config and source replay, native zz
grammar, alias snapshots, and runtime rollback remain excluded. Six warm fixture probes and the
focused three-step scenario report zero differences. The accepted artifact remains the 98-scenario,
1,522-step 10u checkpoint with attached-client `PASS` and SHA-256
`810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`.

Slice 10v closes `tracker.format-vocabulary-registration` with oracle schema 5. The source-backed
inventory records 31 literal `path:function` producer scopes, 153 scoped pairs, 108 unique names,
10 derived families, five propagation records, and 36 modifier tokens. The literal partition is 58
implemented pairs, 54 native pairs, and 41 active gaps. The derived partition is eight implemented
families and two active gaps. The modifier partition is 30 implemented tokens and six active gaps.
The required gate runs eight exact mux tests and three exact daemon tests.

This is source registration, not a runtime or context-value-parity claim. Runtime and option-consumer
work remains under `formats.context-producer-fidelity` (`adopt`, open) and
`formats.modifier-fidelity` (`adopt`, open). Native typed context producers remain accepted under
`formats.native-typed-context-producers` (`native`, accepted). Protocol, snapshots, scenarios, and
the accepted artifact remain unchanged. At the 10v delivery checkpoint, the campaign was paused
before a registry rerank or next-slice selection.

The 2026-08-29 rerank corrected the stale parser-abort ledger item: zz already clears a config
file's commands, stops at the first parser diagnostic, retains only assignments reduced before the
error, and suppresses later diagnostics. That behavior is now closed under `config.parser-abort`.
The later `config.parser-edge-cases` closure matches post-closing-quote expansion, the pin's hidden
token-state transitions, nonempty daemon `HOME`, passwd fallback, named users, failed lookup, and
the 1,022-byte username limit. Direct Control environment provenance and non-UTF-8 passwd home paths
remain under their explicit groups.

Slice 10w closes `formats.repeat-modifier` locally. `R` splits at the first top-level comma,
recursively expands its value and count, accepts counts from 1 through 10,000, and matches the pin's
empty or replacement-failure behavior for invalid, missing, zero, negative, and oversized counts.
Escaped commas, nesting, byte-length, truncation, and post-transform ordering are covered. A
deterministic 40,960,000-byte intermediate guard rejects nested amplification before allocation,
replacing the pin's elapsed-time budget with a bounded result. The shipped `P:` and `S:` status
rows prove production use by indenting with the session name's `n` byte length without exposing
literal `R` syntax. The modifier partition is now 31 implemented tokens and five active gaps:
`I`, `L`, `O`, `V`, and `w`. The strict 16-step `formats` row, the full 98-scenario and 1,526-step
run, and the attached-client fixture pass with SHA-256
`f2aa32e0935e8a839c0abcd43da85e0f474d6c191421776847f7a464cc7257ff`. No commit or push is part of
this local closure, and no successor is selected until rerank.

Slice 10x closes `sessions.new-session-attach-cwd` locally. Existing `new-session -A -c` targets
now use the attach path's retarget and cwd update. The engine expands `-c` once in the resolved
target session, window, pane, and invoking-client context, then stores it before a nonnested
terminal-open preflight. A headless open failure therefore retains the target cwd mutation.
Clientless calls remain inert, a permitted Control client attaches and updates the target, and
nested Interactive, Control, and `-A -d` calls refuse before expansion, retargeting, or mutation.
Fresh creation and an `-A` miss retain an empty session cwd when the command supplies `-c ''`, while
the initial pane keeps its donor or caller fallback. Omitted `-c` retains its prior inheritance.

The ten-step `new-session-cwd` scenario proves one-pass expansion, escaped hashes, target isolation,
fresh explicit-empty creation, and an explicit-empty `-A` miss. Focused mux and daemon tests cover
the client and failure-order branches. The full 99-scenario, 1,536-step strict run and attached
fixture pass with SHA-256
`ed1422d318298b2fee9c31c160393cc2709b9d9137705e96c2632cc700cdcd01`. The tracker now has 90
active groups and 596 active items, with 106 closed records: 48 open, 20 blocked, and 22 accepted.
Closed history plus accepted groups resolve 128 of 196 groups (65.3%). The closure remains local
and uncommitted.

Slice 10y closes `aliases.config-parse-unit` locally. Each config file now stores its original
invocations beside their alias-expanded commands or preparation errors before replay. Parsing,
file-local environment assignments, and alias preparation happen under one engine lock, so an
earlier replayed alias mutation cannot change a later command from that file. Startup roots finish
construction before startup replay; a top-level `source-file` invocation constructs every matched
file before batch replay; and a nested source receives a fresh snapshot when its parent source
command runs.

Stored preparation errors retain source, physical-group, and replay-position metadata. Control
warning-versus-guard classification is frozen with the stored error, while `source-file -n` keeps
its no-effect behavior and suppresses those stored alias errors. Four focused daemon tests cover
startup roots, file and batch timing, nested refresh, parse-only behavior, deferred errors, and
Control classification. The two-step `smoke/config-alias-parse-unit` differential is clean. The
full 100-scenario, 1,538-step run and attached fixture pass with SHA-256
`8d53288c8050e5c8cf7f19e6c81687f91544877d32ea4de9f7d40ea2934736b7`. The tracker now has 89
active groups and 595 active items, with 107 closed records: 47 open, 20 blocked, and 22 accepted.
Closed history plus accepted groups resolve 129 of 196 groups (65.8%). The closure remains local
and uncommitted.

The 10x rerank also rejected the old small-slice forecast for `w`. Pinned width expansion parses
leading hashes and `#[...]` styles, returns zero for malformed style markup, skips controls, and
uses live `codepoint-widths[]` overrides over a 162-entry default cache. The harness builds the pin
with `--disable-utf8proc`, so the host `wcwidth` policy supplies uncached widths; zz uses
`unicode-width` 0.2.2. The later `w` contract must pin those platform and Unicode rules before any
runtime change. Slice 10y closes the alias snapshot prerequisite. The post-10y rerank freezes
`mux.chain-parse-abort` as slice 10z for eager config and source construction before effects.

Slice 10z closes `mux.chain-parse-abort` locally. Each config file now applies permitted bare
assignments, expands aliases, and validates every command group before any command from that file
runs. The first construction failure preserves earlier bare assignments and drops every command
effect from that file. `source-file -n` validates against the pre-file environment and commits no
assignments or commands. Startup roots and files matched by one invocation remain independent file
units built in path order before replay. A failed file loses its own commands while later siblings
continue, and a failed nested child does not stop its parent's later physical groups. Runtime target
and effect errors retain sequential group behavior.

Control emits one located `%config-error` without a failed-command guard and defers construction
warnings until the sibling batch finishes replay. Verbose output retains completed groups and
successful alias-subparse traces before the failure. Parser, mux, and daemon tests plus the clean
two-step `smoke/config-chain-parse-abort` differential cover those boundaries. The complete
101-scenario, 1,540-step run and attached fixture pass with SHA-256
`afd1fdf9a79e06f449e8c43abd63b14a2a4968338110223750d4171889c34aaf`.

The same audit closes `hooks.queue`. Pinned tmux stores `after-queue`, but ordinary queues do not
produce it; explicit `set-hook -R after-queue` runs the stored hook once. The 68-name partition now
contains 64 automatic producers, explicit-only `after-queue`, and three pane-event gaps. Direct pin
checks also correct two open premises. Bare tilde expansion uses a nonempty server-global `HOME`,
falls back through the current user's passwd entry, resolves named users through `getpwnam`, and
reports a located syntax error on required lookup failure; a tilde after either closing quote
expands. The sourced-hook cwd mismatch applies to Control replay, while Command replay already
retains the caller cwd.

Slice 10aa closes `formats.session-runtime/format:session_active`. `FormatClient` records no client,
an unattached client, or one attached session. Command execution keeps the raw invoking client
separate from the current or explicitly selected target client. Clientless lists and filters stay
empty while target-aware commands, status rows, deferred output, shell callbacks, buffer and
capture paths, overlays, Control subscriptions, and display-panes labels use their selected
client. The 198-name partition now has 94 direct mux values, 32 daemon-delegated values, and 72
active gaps. Unit, source-file, `run-shell`, `if-shell`, per-client snapshot, and attached-client
fixture proofs show that `client_*` facts and `session_active` use the same selected client. No
protocol or snapshot field changes.

Slice 10ab closes `formats.window-activity-time/format:window_activity`. Each window stores an
optional Unix-second timestamp beside the logical MRU counter. Creation, parsed nonempty pane
output, and pinned current-window transitions refresh both values. Same-window selection, pane
selection, pane creation, splits, and layout-only changes without output leave the timestamp
unchanged. The independent audit repaired the direct daemon `switch-client` path so it refreshes
the engine clock before selection. Plain, boolean, comparison, list-row, and time-modified forms
read the same stored seconds. The 198-name partition now contains 95 direct mux values, 32
daemon-delegated values, and 71 active gaps. No protocol or snapshot field changes.

Slice 10ac closes
`jobs.command-status-environment/semantic:shell-job-clean-environment`. Shell-form `run-shell` and
`if-shell` start from an empty process environment, then apply modeled global and resolved-session
values. Status `#()` applies global values only. Hidden and unset values stay absent; an explicit
missing target becomes sessionless; visible modeled `TMUX_PANE` survives without synthesis.
Startup command jobs preserve modeled TERM-family values, while completed startup forces the tmux
terminal identity. The private tmux launcher uses modeled PATH. The three-step differential runs
eight assertions per engine, and the attached fixture proves the global-only status path. Delayed
`run-shell` sampling, `copy-pipe`, popup jobs, and status cwd remain active.

Slice 10ad closes
`tracker.semantic-coverage/semantic:tracker-option-consumer-registration`. The unchanged 105-name
roster moved from the option definitions to `command::TMUX_OPTION_CONSUMERS`, while `BEHAVES`
remains its public alias. The exact guard proves that 180 pinned options equal 105 consumers plus 75
live option gaps, with no overlap, and confirms the tracker closure. `copy-mode-mark-style` records
status option-variable consumption only, not visual mark rendering. The compatibility gate passes
445 mux tests plus three daemon inventory tests. Full workspace tests and clippy, formatting, diff,
tracker, and checked-summary checks pass. No runtime behavior, oracle, protocol, snapshot, scenario,
or accepted artifact changes.

Slice 10ae closes
`options.option-name-format-coverage/semantic:option-name-format-coverage`. Generic lookup now
precedes format-table, command-item, and environment values across 13 server, 42 session, 40 window,
and 10 pane consumers. Exact names and legacy aliases follow selected targets, inheritance,
attached fallback, active children, and `S`, `W`, and `P` loop retargeting. Command prefixes do not
match.

Flags emit `0` or `1`; other types retain their tmux spelling. `command-alias`, `status-format`, and
`update-environment` support whole-array and indexed lookup with numeric-before-named order,
leading-zero normalization, empty invalid results, and whole-array local shadowing. Mux formats read
live state. Direct daemon producers use the same resolver; detached status shares one all-scope
snapshot across a refresh batch. Missing-target `run-shell -C` and `if-shell -F` read global options
while inserted work keeps the caller context.

The focused 60-step differential has zero topology, geometry, format, output, or warning
differences, and the attached status probe passes. Exhaustive mux and daemon coverage includes the
roster, arrays, targets, loops, producer inventory, and detached refresh. No protocol, wire snapshot,
or native GUI styling changed.

Slice 10af closes
`jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`.
Shell-form `run-shell` with explicit numeric `-d > 0` retains command text, target identity and
numeric session id, expanded text and numeric arguments, and the cwd string at scheduling. Child
launch reads current global state, the live original-session overlay or its retained overlay after
destruction, `default-terminal`, and the startup TERM gate. A missing scheduling target remains
global-only with `TMUX` id `-1` after a matching session appears. Cwd existence fallback runs when
the child starts.

Foreground daemon coverage waits for `active_shell_jobs` before it mutates state. The background
three-step differential completes twelve checks per engine across live, destroyed and recreated,
missing and later-created, and startup-crossing cases, including frozen formats, numeric arguments,
target identity, and cwd. It reports no differing channel. `run-shell -C`, `if-shell`, absent `-d`,
`-d 0`, immediate background ordering, cwd producer selection, `copy-pipe`, and popup jobs stay
outside 10af.

Slice 10ag closes
`source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd`. A cold launcher
passes a bounded valid UTF-8 cwd through private `--bootstrap-client-cwd` only when it auto-spawns
the daemon. Startup replay gives that base first priority, carries it through nested relative
sources and literal metacharacter paths, then clears it before runtime commands on success or error.
A direct daemon launch has no bootstrap base, and later sources use the registered client cwd. The
isolated startup-client-cwd differential passes exactly on both engines without a public
protocol change.

The full eight-case startup diagnostic now reaches `control-mode.exit-pane-output`. zz may drain
three to five queued shell-prompt `%output %0` rows after a flags-0 `%end` and before `%exit`; ten
equivalent pinned-tmux probes emitted none. Slice 10ai owns pending and later pane-byte discard on
EOF or blank Return. Hard-disconnect cancellation, command output, retained status, and flow control
remain outside that slice.

Slice 10ah closes `control-mode.kill-server-response-order`. Shutdown freezes response admission
under the active-request lock, waits a bounded interval for admitted replies, publishes
`ServerStopping`, and drains every registered writer through one deadline. The foreground thread
retains the listener through that drain, removes the endpoint while it still owns the listener, and
then drops it. Deterministic tests cover the old race, late Control and Command requests, stalled and
disconnected writers, replacement binding during cleanup, and immediate fresh-daemon startup.
Pane-output discard follows as slice 10ai.

The registry now has 87 active groups and 590 active items, with 119 closed records: 45 open, 20
blocked, and 22 accepted. Closed history plus accepted groups resolve 141 of 206 groups (68.4%).
Slices 10w through 10ah plus the Config and Key fronts form the local 2026-08-29 checkpoint. The
persisted accepted artifact covers 104 scenarios and 1,672 steps with attached-client `PASS`,
exactly two approved GEO rows, every other channel clean, and SHA-256
`8365f95b9297641a7f4462d7b337d4a711a9edf34c41fc7ab4d8ec4818700a5c`. Focused mux and
compatibility gates pass. The final workspace run passed every non-daemon package; three daemon
tests failed only under parallel load and passed alone. Strict workspace clippy, formatting,
tracker, stored-summary, and OKF validation pass.
The historical 10i checkpoint remains 97 scenarios and 1,514 steps at SHA-256
`3b728eb8f0d30cae1bf1fe9c09100188279aaf8c80c0b33b30cd15b617f75d70`.
The historical 10h checkpoint remains 96 scenarios and 1,511 steps at SHA-256
`75aee7176d3ed3cf1886d4f4c697062089b87644036e85f0230f355fac7d4217`.
The historical 10f checkpoint remains 94 scenarios and 1,505 steps at SHA-256
`31b03805b5701aff0555ebe4d4b40a0116b8525130d4d3406963e9a1c8f1919c`.
The historical 10e checkpoint remains 93 scenarios and 1,502 steps at SHA-256
`e0783568fc5845eaaa9ff4b84256d43a046ced996fbf8b664bc65d9bf0d9578a`.
The historical 10d checkpoint remains 92 scenarios and 1,499 steps at SHA-256
`afea2249cd62402fe00dc8c54ea60662eb616ef584806f4774cd77723746144e`.
The historical 10i `display-panes` row contributed three harness steps and 22 internal checks with
zero TOPO, GEO, FMT, OUT, or WARN differences.

Protocol v82 appends one bounded UTF-8 client-environment snapshot to `ClientHello`. Local and
SSH-forwarded clients now seed fresh sessions and refresh existing sessions through the effective
`update-environment` patterns. `-E`, `-A`, Control, native attach, and targeted `switch-client`
follow the pinned selection and ordering rules. Existing processes keep their startup environment;
future panes read the refreshed session map. Non-UTF-8 Unix entries remain under
`clients.path-encoding` rather than being substituted.

Protocol v83 appends `ClientHello.process_id` and closes `clients.context-formats`. The daemon builds
one retained client-fact record for list rows, ordinary commands, foreground inserted commands,
status recipients, and `display-message`. The attached fixture covers Interactive, status,
bound-command, implicit target-client, and Control contexts against the pin.

Protocol v84 appends zero-based lexical command-block positions to `CommandInvocation` and now
closes `tracker.args-parse-if-shell`, `tracker.args-parse-run-shell`,
`tracker.args-parse-set-option`, `tracker.args-parse-display-menu`,
`tracker.args-parse-display-panes`, `tracker.args-parse-set-hook`,
`tracker.args-parse-choose-buffer`, and `tracker.args-parse-choose-tree`, plus the `bind-key`,
`command-prompt`, and `confirm-before` members of the shared commands-or-string rule.
Source-file and Control
parsing preserve unquoted typed arguments through wire transport, aliases, bindings, and hooks;
quoted braces remain strings. `if-shell` accepts typed branch positions while rejecting typed
conditions, option values, and extra positionals. `run-shell` accepts typed positionals only when a
leading `-C` enables command mode, keeps option values string-only, and stops scanning flags at the
first positional or `--`. Its strict three-step scenario runs 21 internal checks on both servers
and finishes with `ARGS_PARSE_RUN_SHELL=clean:21`. `set-option` and `set-window-option` share the
`SetOptionValue` rule: only positional 1 accepts a typed block; option names, flag values, and extra
positionals remain strings. Recursive command printing preserves same-line `;` and physical-line
`;;`, empty blocks become empty values, quoted braces stay literal, and `-F` expands after typed
normalization. Typed values use the live mux environment during that construction. Their strict
three-step scenario runs 21 internal checks on both servers and finishes
with `ARGS_PARSE_SET_OPTION=clean:21`.
`bind-key` accepts strings or typed blocks in every positional while keeping `-T` and `-N` values
string-only. Option scanning stops at the first positional or `--`. Typed keys expand the live mux
environment and print recursively before lookup. One typed tail preserves physical source groups,
one string tail reparses as one group, and a typed first tail with extra arguments stores an empty
binding. Stored child failures leave the prior binding intact. Bare key-only mutation preserves
commands and unspecified metadata, applies only requested note and repeat changes, and leaves an
absent key unbound. Its strict three-step scenario runs 17 internal checks through a real attached
client and finishes `ARGS_PARSE_BIND_KEY=clean:17` on both servers.
`confirm-before` accepts a typed block or string at its one command positional while `-c`, `-p`,
and `-t` values stay strings. Every lexical typed block constructs recursively before its parent's
name, callback type, or arity validation. Recursive paths carry independent one-layer user-alias
budgets; alias-produced subtrees disable further user aliases, direct self-recursion fails unknown
without killing the daemon, and siblings remain independent. Nested `if-shell`, `run-shell`,
set-option, and confirm blocks print canonical names. Empty blocks read back as `{  }`, while
physical internal group newlines print as ` ;; `. String children construct after target lookup and
parent-format expansion as one group. Its strict three-step scenario runs 19 internal construction,
parser, readback, alias, and exact channel checks and finishes
`ARGS_PARSE_CONFIRM_BEFORE=clean:19` on both servers. Nested bind and confirm failures are preflight
parse errors. Reply and `-y` Enter-default paths have daemon and GPUI unit proof. Raw zz-tui now
renders confirmations and handles their replies with unit and attached-client coverage. It also
consumes the daemon-published menu and popup descriptors; menu keyboard resolution is shared with
GPUI. The broader menu and popup behavior classes stay under `display-menu.behavior-fidelity` and
`display-popup.behavior-fidelity`. Eager whole-file source construction and the broader
replay-channel placement difference remain open.
Stored `bind-key` and `set-hook` commands execute their constructed lists without another
user-alias lookup. Typed `if-shell`, `run-shell`, and `confirm-before` callbacks stay frozen after
lexical construction. Typed `if-shell` and `run-shell` callbacks preserve physical groups: a failed
group stops its remaining commands while later physical lines continue; string callbacks stay one
group. Typed `command-prompt` templates retain their structured prepared command list through
submission without re-expanding aliases. Its template positional accepts a typed block or string
while `-I`, `-p`, `-t`, and `-T` values remain strings. Structured substitution preserves
leaf-argument boundaries against quote or semicolon injection. String templates substitute raw
source before a fresh parse and complete construction pass against the current alias table. Both
paths replace the first `%%` and every `%1`, with trailing-percent quoting. Typed callbacks retain
physical groups, while string templates and free input form one group. String failures retain the
originating source path and line. Prompt chaining and multi-answer `%2` retain their prompt owner.
`set-hook` and command-valued native set-option retain their intentional second
construction stage. Without `-B`, only `set-hook` value position 1 accepts a typed block; the hook
name and extra positionals remain strings. With `-B`, every positional lexically accepts either
type. Option values remain strings in both modes. zz still rejects `-B` because format monitors
remain unsupported. Built-in hooks flatten
physical groups during their second pass, while custom `@` typed values retain textual ` ;; `
groups. Unindexed malformed runtime replacement clears first, indexed replacement preserves its entry, and
an empty or failing local append creates an empty local array that shadows the inherited global
hook. Typed ignored `-R` values still construct. `display-menu` walks repeated NAME, KEY, and ACTION
fields. Nonempty names consume a string key and a string-or-typed action; empty names are separators
and leave the parser in NAME state. Its ten valued flags stay string-only. Typed children construct
before parent type, arity, or effects. Accepted typed actions print canonical child commands in
stored bindings; incomplete NAME and NAME-plus-KEY tails reach daemon runtime validation. Runtime
resolves the current or `-c` target client before completeness, so an unattached command or initial
Control reports `no current client`; initial Control uses a flag-0 `%error` and exits 1. Once
attached, Control validates an incomplete group as `not enough arguments` before its overlay no-op
and returns a flag-1 `%error`; EOF after that frame exits 1. Interactive ordering remains
unchanged. The daemon drops a typed action's structural
wrapper before the fresh selection parse, while quoted brace actions remain literal. `display-panes`
accepts an optional string or typed selection template while `-d` and `-t` values remain strings.
Every typed child constructs before parent option-type or arity validation. Aliases and
prefixes retain typed positions and canonical stored readback. Targetless routing resolves an
attached client before duration validation. Its strict three-step fixture runs 22 internal checks
with zero differential channels. Custom selection-template execution remains parked because mux
runtime rejects the positional value instead of substituting the selected `%pane` for `%%%` and
executing with the original queue state. Tmux uses `select-pane -t "%%%"` when the template is
omitted; queue blocking and presentation stay separate. `choose-buffer` and `choose-tree` accept
zero or one string-or-typed template while their valued options stay strings. Typed children
construct before parent type, arity, target, or effects. Typed templates freeze constructed aliases
before opening; string templates parse against the current alias table after selection. The daemon
closes the chooser, substitutes the exact buffer name or tree target, and executes against the
invoking client's live context. The shared 26-check attached-client fixture covers substitution,
alias timing, stale and empty buffers, uppercase errors, and direct plus stored arity precedence
over recognized parked flags. All 12 implemented callback commands now apply their pinned rules,
so no command-specific `args-parse:` item remains. Eager whole-file
construction, same-source alias mutation, multiline inner-source placement, generic alias recursion,
selected-action runtime errors, and broader replay placement retain their owners. Attached menu
descriptor consumption and shared keyboard ownership now close for raw zz-tui. Geometry
construction, action context and errors, mouse policy, paste-close ordering, queue ordering,
rendered width, resize lifecycle, shortcut display and grammar, and style refresh remain separate
under `display-menu.behavior-fidelity`.

Protocol v81 closes `control-mode.async-command-output`. Targetless and
invalid-target foreground shell output reaches the exact originating Control client raw after its
empty flags-1 guard; direct and sourced same-line continuation keeps separate guards. Embedded LF
and percent-prefixed lines stay literal, a missing trailing LF is supplied, and a shell's nonzero
status does not change Control retval. Foreground `run-shell -C` remains synchronous. Resolved `-t`
and ordinary `run-shell -b` open zz's native per-Interactive command-output view for attached pane
viewers without raw Control text or `%pane-mode-changed`, preserving deliberate GUI ownership.

The later asynchronous copy-pipe slice needs no protocol or runtime change. Pinned tmux starts the
worker without a completion callback, and a delayed exit-7 Control probe observes successful copy
mode cancellation with no message, error frame, or extra command guard. zz keeps the same silent
Control contract while retaining its native Interactive error notification.

Protocol v80 closed `config.startup-diagnostic-delivery`. Startup parses all roots
before replay, retains normalized root and nested read failures, parser diagnostics, unsupported and
runtime failures, and successful `display-message -p` output, and discards list-style output. Root
causes precede replay causes; replay stays root-ordered and nested depth-first. Successful physical
multiline commands use their completion line.

A detached Command launch stays rc 0 with empty stdout and stderr and cannot drain the causes. Only
the post-spawn Control advertises `startup-config-owner-v1`; it receives the raw bounded vector after
`ServerHello` and before its first `%begin`. A late Control receives it after `Attached` inside the
attach frame. An attached Interactive winner opens a PTY-free `configuration errors` view with an
ordered, control-sanitized, UTF-8-safe 64 KiB preview. The explicit truncation line directs full
recovery through a Control-mode restart. This preview is a deliberate product boundary: an
Interactive client cannot recover the exact retained 1 MiB vector.

Eligible delivery is linearized globally. The daemon commits the one-shot only after the complete
attach sequence remains admitted; failure retains the set and retires only the startup actor by
exact ID. A restart builds a fresh set. The checksum-attested seven-case differential against pinned
tmux `d77c9dc6` passes with no skips and leaves the canonical scenario summary untouched.

# Baseline captured 2026-08-22

These counts preserve the audit baseline. Use the generated gap report for current totals.

- 83 of 92 tmux commands execute; 9 are recognized but unimplemented.
- 70 tmux-valid flags are rejected across 20 implemented commands.
- All 180 named options stored at the audit baseline; 104 behaved and 76 were storage-only. The
  2026-08-24 separator slice moved the current split to 105 behaving and 75 storage-only.
- The audit used a flat format ledger. Schema 3 supersedes that snapshot with 198 global
  format-table names and source-enumerated rosters for selected contexts; use the generated report
  for current limitations. `config_files` retains the active top-level config selection and
  `pane_dead_time` tracks retained exits.
- All 68 hooks store; 10 lack an automatic producer.
- 71 differential scenarios and 1,058 executable steps were represented by the audit inventory.
- Bare packaged `zz` now creates session zero on an empty daemon and attaches it; simultaneous first
  attaches and a racing command-side session creator converge on the existing session.
- The default zz prefix table intentionally favors picker/sidebar behavior over complete stock tmux
  parity.

# Doctrine

## Keep tmux syntax frozen

Every tmux line a user pastes must mean what tmux meant or fail. Do not use spare-looking tmux flags
for pane kinds, hosts, browser profiles, agent providers, or GUI layout.

- Pane kind belongs in the verb: `split-window` creates a terminal, while `split-picker` and
  `split-browser` are native commands.
- Host belongs before the command: `zz --host NAME ...`. It never enters tmux's `-t` target grammar.
- A native default binding can call a native command. Config that binds `%` to `split-window -h`
  keeps terminal-split behavior.

## One native command catalog

The shared catalog is now the source for command discovery, prefix resolution, `list-commands`,
stored-command rendering, and palette completion. Its 102 canonical verbs comprise 83 tmux verbs
and 19 zz-native verbs. Exact names and aliases resolve first. Non-exact lookup searches tmux names
before native names, so new GUI verbs cannot change a pinned tmux abbreviation. `tools`,
`agent-send`, `send-last-output`, `capture-browser`, and `debug-marker` joined the catalog on
2026-08-22. The remaining duplicate is the long-form prose in `zz tools`; it is a documentation
cleanup, not a discovery gap.

Native command families should remain small and composable:

- Pane creation: `split-picker`, `split-browser`, and a direct agent form if automation needs it.
- Pane materialization: `select-pane-kind terminal|browser|agent|editor`.
- Browser control: `set-browser-url`, `set-browser-tabs`, `set-browser-profile`, `capture-browser`.
- Agent control: `agent-send`, `send-last-output`, `set-agent-session`, `set-agent-provider`,
  `restart-agent-pane`.
- Editor control: `set-editor-path`.
- Workspace navigation and operations: `focus-sidebar`, `tools`, `debug-marker`, and fleet verbs.

Do not add both `new-X` and `split-X` for every pane kind without evidence. The picker plus one
script-friendly direct form is the smaller primitive.

# Definition of compatible enough

The alias milestone is met when all of these are true:

1. Bare packaged `zz` creates and attaches on an empty daemon, attaches on a live daemon, and
   `new-session`/`attach-session` preserve TTY, nested-session, read-only, and detach semantics.
2. The current pinned config/plugin corpus runs without a SKIP, and the checked-in report proves
   every current scenario. Any SKIP exits nonzero.
3. A short published workload covering create, attach, list, target, split, resize, move, capture,
   buffer, option, environment, hook, source, and kill operations is differential-clean for exit
   status, stdout, stderr, topology, and geometry.
4. An attached-client harness covers copy mode, choosers, prompts, key bindings, and launcher paths
   that the headless corpus cannot see.
5. Every remaining accepted divergence is explicit. No unsupported tmux syntax silently changes
   GUI state.
6. Migration documents the one-time config import and the limit of a shell alias. Unix shell and
   status jobs spawned by the daemon receive zz's private shim; arbitrary programs that require an
   executable named `tmux` use a separate opt-in shim.

This gate does not require closing every registered gap. Pull work from the supported workload and
real config or plugin hits.

# Implementation-ease assessment captured 2026-08-22

This table records the audit's initial complexity assessment. The registry owns current rank,
status, `depends_on` ordering, and acceptance evidence. The delivery plan below groups related work
by dependency rather than raw ease.

| Ease rank | Gap | Why it falls here | Target |
| ---: | --- | --- | --- |
| 1 | Empty-daemon bare launch | The daemon and `new-session -A` already had the needed create-or-attach behavior; the launcher needed to select it without changing explicit attach semantics. | **Shipped 2026-08-22.** Bare `zz` creates-or-attaches, explicit attach preserves `no sessions`, nested `new-session` is guarded, and both first-session races are covered. |
| 2 | Prompt history commands | The existing separate rings and file policy needed command handlers plus serialized persistence. | **Shipped 2026-08-22.** Both commands, aliases, `-T`, output, errors, clears, and persistence are covered. |
| 3 | Native command catalog cleanup | Five daemon-only verbs needed shared specs and consumer convergence. | **Shipped 2026-08-22.** All 19 native verbs are discoverable; no direct agent split was justified. |
| 4 | Local parser and no-model flags | `unbind-key -a/-q`, `new-window -b`, and `kill-*-a -f` use state and formats the mux already owns. | **Two slices shipped 2026-08-22.** The 22-step local-flag fixture and 17-step kill-filter fixture are clean; pull further flags by corpus hit. |
| 5 | Small state and format facts | Bare `list-keys` padding, `pane_dead_time`, `config_files`, client timestamps, missing hook producers with an existing event seam, and straightforward output formatting. | **Three pulls shipped.** The 2026-08-22 pull covered bare `list-keys`, explicit-startup `config_files`, and retained `pane_dead_time`. The 2026-08-24 pull added pin-ordered `show-options -H` hook rows and item-scoped `window-status-separator` expansion. The 2026-08-25 pull exposed retained session activity and corrected logical MRU ordering. |
| 6 | Manual geometry | `resize-window` and `window-size manual` need a durable manual size plus clear precedence against per-client measurements. The command is small; the policy is not. | **Shipped 2026-08-22.** Absolute and relative practical forms, target/error precedence, manual formats, per-client precedence, and daemon PTY resize behavior are pinned. The later 2026-08-27 `clients.attach-sizing` slice closed client-derived `-A`/`-a`. |
| 7 | Capture, chooser, prompt, and list fidelity | `capture-pane` routing/ranges, chooser formats, command-prompt chains, and exact `list-keys` rendering need attached-client and output fixtures. | **List and chooser presentation fidelity completed 2026-08-24.** The list selectors, positional key filter, stock repeat metadata, canonical Space spelling, and `-1` attached-client status route are pinned by a 46-step differential plus the attached fixture. Chooser static-filter fallback state now survives deltas, both clients show `filter: no matches`, and fully keyless lists omit the shortcut gutter; the attached fixture proves tree and buffer fallback on zz and tmux. Ordinary capture was extended 2026-08-23; trailing blank viewport rows and richer capture transports remain. |
| 8 | Spawn and attach context | Attached cwd, client flags, sizes, environment refresh, client targeting, and exit actions cross different state owners. | **Thirteen bounded slices have shipped.** Protocol v72 carries caller cwd; later slices closed client targeting, nested intent, supported tty selectors, local Control identity, session cwd, requested flags, retained sizing, and protocol v82 environment refresh. Protocol v83 closed `clients.context-formats`: one retained client-fact record covers list rows, ordinary and inserted commands, status recipients, and `display-message`, with pinned Interactive and Control empty behavior. The client lifecycle slice now produces all six report hooks with pinned duplicate, ordering, client-kind, and target-context rules. Per-client `no-detach-on-destroy` fallback now matches the pin's two-tier survivor choice. The fresh attached-client differential passes against tmux `d77c9dc6`. `detach-client -E`, active-pane consumption, changed-resize post-geometry hook context, parent-HUP exit actions, non-UTF-8 path bytes, read-only/focus policy, and interactive refresh remain in separate groups. |
| 9 | Interactive client behavior | Full `refresh-client`, `switch-mode`, mouse-targeted forms, pane marking, mode state, focus hooks, and client fanout cross daemon, protocol, TUI, and GUI ownership. | Implement only for named workloads. |
| 10 | Binary streams and process control | `display-message -I`, `split-window -I`, buffer/source `-`, and lock execution require bounded transport, backpressure, cancellation, and process lifetime rules. | Separate design approval. |
| 11 | tmux floating panes | `new-pane` and the parked `move-pane`/placement flags need a new mux-state model that is distinct from current native floating UI. | Park. |
| 12 | Linked sessions, ACLs, and tmux socket interop | These require changing core ownership or implementing unrelated security/wire protocols. | Linked sessions and socket interop are permanent non-goals; park ACLs. |

The 21 theme/palette options and four tree-mode options are easy to store but not necessarily easy
to make meaningful across native clients. They remain demand-driven rather than inflating an option
percentage.

# Implementation progress: 2026-08-25

The first eight ease ranks have shipped at least one evidence-driven slice:

- Bare installed `zz` routes through `new-session -A`, preserving product-friendly create-or-attach
  without weakening explicit `attach`/`attach-session`: those verbs return tmux's `no sessions` on
  an empty daemon, and a literal `attach || new-session` fallback is PTY-tested. Empty-daemon
  materialization remains atomic, including two simultaneous lower-level attaches and an ordinary
  command client creating the first session at the same boundary. Attaching `new-session` uses the
  same nested-session refusal as `attach-session` before changing mux state.
- `show-prompt-history`/`showphist` and `clear-prompt-history`/`clearphist` match the pin's two rings,
  `-T command|search`, output shape, invalid-type error, selective/all clear, and persistence. Save
  ordering is serialized so a racing record or clear cannot restore stale disk state.
- The shared catalog contains all 19 native verbs. A review-caught long-option rendering bug is
  pinned: storing `agent-send --submit --context=...` no longer turns the long flag into a short
  cluster when rendered by `list-keys` or hooks.
- `unbind-key -a/-q` and `new-window -b` match the pin in a 22-step differential scenario.
  `kill-session -a -f`, `kill-window -a -f`, and `kill-pane -a -f` share the existing contextual
  format engine and match in another 17 steps. The unsupported ledger fell from 113 pairs across
  29 commands to 107 across 26 before manual geometry made `resize-window -A`/`-a` explicit,
  bringing the then-current ledger to 109 across 27. Later `display-panes`, `join-pane`, and pane-spawn
  slices brought the ledger to 102 pairs across 24 commands. `last-pane -d/-e` then brought it to
  100 pairs across 23 commands. Four micro flags, three `list-keys` selectors, and creation-time
  `new-session -e/-E` brought the ledger to 91 pairs across 23 commands; the following
  `set-buffer -n`, `source-file -F`, `split-window -Z`, and `break-pane -a/-b` slices left the
  ledger at 85 pairs across the same 23 commands after `move-pane -l` joined the supported surface.
- `list-keys` now shares the pin's global padding facts, optional key filter, `-1`, `-O`, `-r`,
  error precedence, and stock copy-table repeat metadata. Its `-1` result is stdout for Command and
  Control clients and a frozen timed status for Interactive clients. `config_files` reaches command,
  status, list, label, and renderer-style contexts and changes from startup selection to the file
  selected by native reload; retained panes expose `pane_dead_time` and clear it on revive/respawn.
- `session_activity` now exposes retained Unix seconds initialized from session creation and refreshed
  by the shared attach and terminal-input funnels. `S/t` and `list-sessions -O activity` use a
  separate logical counter, preserving deterministic same-second MRU order. Sessions now retain an
  internal cwd. Public `session_path` now has the separate `formats.session-path` owner, while the
  format-client-specific `session_active` remains under `formats.session-runtime`. Every attach now
  advances latest geometry independently of `focus-events`; enabled FocusIn uses the same owner
  seam. Read-only rejected native input updates activity and latest geometry without clearing bells,
  while writable chooser input counts once, advances latest geometry, and preserves bells. Chooser
  routing stays client-scoped. Read-only-safe local view actions bypass retained chooser and
  display-panes surfaces without dismissing them. Display-panes valid selection and bare hover
  consumption, unmatched key/Escape/non-hover mouse fallthrough, and timeout accounting are
  explicit. Typed `send-keys -X` authorization, all-or-nothing binding preflight, and pane-focus
  blocking are closed under `clients.read-only-local-view-actions`. Committed text now uses one
  bounded ordered queue per client whose entries record pane and input lane: a matching Key-plus-Text
  pair takes the Key result and contributes at most one activity/latest update, standalone read-only
  terminal text accounts without PTY input or a bell clear, and writable modal consumption can
  contribute zero. Cleanup and synchronous switch behavior are closed under
  `formats.session-activity-text-input`; tmux's inapplicable suspended-client wake path is accepted
  under `formats.session-activity-wake-lifecycle`.
- `resize-window`/`resizew` now resize the durable layout extent, select a window-local manual
  sizing policy, expose the two manual-size formats, and outrank later client measurements. The
  16-step strict-geometry scenario covers absolute and relative sizes, option transitions, output,
  bounds, and missing-target error precedence against the pin. The later `clients.attach-sizing`
  slice closed client-derived `-A`/`-a` aggregation.
- `new-window` and `split-window` apply repeated `-e NAME=VALUE` entries only to the new pane,
  ignore malformed entries, and let the last value win without changing `show-environment`.
  Their `-E` forms create live panes with no child process, reject nonempty commands after target
  resolution, and match the pin throughout a 25-step strict-geometry scenario.
- Creation-time `new-session -e` overlays the normal `update-environment` seed, persists on the
  session, and reaches its first pane; later entries win and malformed entries are ignored.
  Creation-time `-E` skips that normal seed while retaining explicit overlays, and `-A` ignores
  `-e` when the session already exists. An 18-step differential fixture pins the behavior. Protocol
  v82 later closed bounded UTF-8 client-sourced values and attach-time reseeding.

Focused protocol, mux, daemon, TUI, completion, strict-Clippy, formatting, and differential tests
cover the tranche. Gate 0's mechanism and current canonical summary are complete. The 2026-08-26
checkpoint covers 84 scenarios and 1,475 steps; every ordinary row is clean, and the two registered
known rows each retain exactly one documented GEO divergence with every other channel clean. The
attached-client fixture is part of the strict Linux CI run and drives real zz and pinned-tmux
attaches through outer PTYs. The packaged CLI fixture clones
a verified macOS bundle through a path containing spaces and passes bare/new/attach against empty
and existing daemons. It now also pins detached `-x`/`-y`, attached client dimensions, read-only
input rejection with visible pane output, requested detach notices, and `attach -d` eviction notices.
The attached-client fixture also targets the live zz and tmux clients by their real outer PTYs,
requires their attached-client counts to reach zero, and thereby proves normal local TUI tty publication.
It now also refuses attach and `new-session -A` with inherited `$TMUX`, then repeats both through
`env -u TMUX` on the same retained tty and requires them to attach.
The current attached fixture also runs local Control from each outer PTY. It requires terminal-backed
`attach-session` and `new-session -A` refusal against existing sessions, permits a fresh `-A` miss,
and proves piped stdin does not acquire a tty identity. The daemon unit matrix covers
`new-session -Ad`; the attached fixture does not. The complete attached differential passed for zz
and pinned tmux. The 2026-08-26 canonical run persists that result as `PASS` below the scenario rows.
The attached proof also exposed and closed a copy-mode ordering race where a queued yank could be
canceled before the terminal processed it.

The first evidence-ordered tranche was:

1. Make bare `list-keys` compute the pin's global repeat, key, and table widths. **Complete:** the
   pinned `tmux-sensible` runtime path calls this form, and the expanded 19-step scenario also pins
   `-N`, `-a`, and `-P` with deterministic prefix/root ordering. **Extended 2026-08-24:** the
   46-step scenario covers the remaining selectors, positional filtering, reverse orders,
   canonical Space spellings, and post-`-1` aggregate facts; the attached fixture covers the timed
   status route.
2. Add a shell-level attached-client driver. **Complete and integrated:** a pinned tmux outer pane
   supplies the PTY for an inner
   zz or tmux attach; the fixture compares semantic queries and small mode markers, not native
   presentation pixels. It covers readiness, root/prefix/prefix2 bindings, copy mode, command
   prompt rename, tree row-key switching, buffer paste/deletion, and nested attach.
3. Back `config_files` from the active top-level selection and `pane_dead_time` from the
   retained-pane exit seam. **Complete:** harness startup is symmetric for explicit `/dev/null`,
   native reload replaces the retained config selection, style conditionals receive the same fact,
   retained death stamps the timestamp, and revive/respawn clear it.
4. Implement manual window geometry without letting a later client measurement overwrite it.
   **Complete:** `resize-window` uses the existing durable layout extent, `window-size manual`
   freezes that extent, and the supported forms are strict-geometry clean.

# Recommended delivery order

This section records sequencing decisions and dated completion evidence. It does not serve as the
work queue. Select exact gap IDs from the generated report before starting a slice.

## Gate 0: make the evidence current

1. Revalidate `smoke/config-grammar`: the current tmux-only warning expectation is correct; the
   nested zz control client still does not emit `%config-error`.
2. Run all 84 differential scenarios and persist the 1,475-step summary. **Complete 2026-08-26:**
   every ordinary row is clean. `known/known-main-preset-two-panes` and
   `known/known-spread-mixed` each retain exactly one documented GEO divergence and no other
   difference. The attached-client fixture is `PASS`, and `compat/run.sh --check-summary` passes.
   The summary SHA-256 is
   `5de67222bc2ebb99c57963be14c865ddfdddc387da34ee32dd86962cef8336c9`. CI checks scenario paths,
   step counts, every stored result cell, and the attached `PASS` before the run, then diffs every
   result column after a complete strict run.
3. Add a drift check that fails when scenario files and checked-in result rows differ. **Complete:**
   `compat/run.sh --check-summary` compares exact scenario paths, step counts, and all seven stored
   row cells against the ordinary clean tuple or the tracker's registered known tuple. It also
   requires a persisted attached-client `PASS`. Partial and headless-only runs cannot overwrite the
   combined report, and only the summary is versionable under `compat/results/`.
4. Add an attached TUI fixture for copy mode, choose-tree, choose-buffer, command prompt, prefix
   tables, and nested attach. **Complete as `compat/attached-client.sh`:** both sides run through
   real 80x24 PTYs with bounded semantic polling, diagnostic screen dumps, and deterministic
   cleanup. `compat/run.sh --attached-client` includes it in overall success without mixing its
   result into the headless scenario counts; strict Linux CI runs that combined contract.
5. Add packaged-launcher smoke tests for bare, `new`, `attach`, empty daemon, existing daemon, and a
   path containing spaces. **Complete:** the Cargo launcher matrix covers the cheap seam, while
   `compat/packaged-cli.sh` verifies a freshly built CEF bundle and development signature, clones
   the whole app under a spaced path, then passes all six command/server cases through the real
   `Contents/MacOS/cli`. Four PTY cases additionally pin detached and attached sizing, read-only
   input/output, requested detach, and peer eviction. The macOS CI leg repeats that smoke after
   bundle creation. Release notarization and `/Applications` installation remain packaging checks,
   not tmux compatibility gates.
6. Put every compatibility TODO and accepted difference in one repo-owned registry. **Schema 3
   review complete 2026-08-23:** `compat/tmux-gaps.json` owns stable IDs, product status,
   `depends_on` ordering, priorities, evidence, `updated_on`, and closed history. Oracle schema 5
   captures 92 commands, 78 aliases, 572 flag shapes split into 318 valueless, 246 required-value,
   and 8 optional-value shapes, plus positional minimum and maximum metadata. It parses nine custom
   `args_parse` callbacks used by 14 commands and reduces them to six effective rules. It also
   captures 180 options, 198 global formats, 31 literal producer scopes with 153 scoped pairs and
   108 unique names, 10 derived families, five propagation records, 36 modifier tokens, 68 hooks,
   and 303 default bindings across five tables from an attested clean build at the exact pin.
   `just compat-check` runs the oracle and registry checks, the full `zz-mux` library suite, eight
   required exact mux tests, and three required exact daemon tests.

   Slice 10v partitions the literals into 58 implemented pairs, 54 native pairs, and 41 active
   gaps. It partitions the derived families into eight implemented and two active gaps, and the
   modifiers into 30 implemented and six active gaps. These production-owned registrations close
   discovery without claiming runtime behavior or context-value parity.

   `mux.resize-pane-optional-values` closed on 2026-08-25 as a catalog-only reconciliation.
   Runtime already accepted bare direction flags with amount 1 and attached or separated integer
   amounts. The four direction entries now expose optional values to the manifest gate. Nine focused
   resize tests, 175 protocol unit tests, 14 protocol framing tests, and the strict 16-step
   `resize-directions` differential pass. No runtime path or wire version changed. `resize-pane -M`
   and `-T` remain open under their existing owners.

   The Rust gate reconciles names, flag arities, positional bounds, native extensions, the guarded
   native-name roster, every pinned canonical prefix, zz-only defaults, every constant-backed
   format gap, every missing default key, and rendered command plus repeat metadata for every
   shared default binding. It also reconciles the complete schema 5 literal, derived, propagation,
   and modifier inventories. The earlier selected-context closures remain part of that inventory:
   `formats.command-item-context` closed on 2026-08-24 once the mux dispatch chokepoint started
   carrying the canonical entry name into every command it runs.
   `formats.daemon-command-item-context` closed the same day by carrying that resolved name only
   through daemon-owned item expansion, including the post-spawn `new-window`/`split-window -P -F`
   pass that adds live pane facts. Typed blocks and delayed formats retain their own or empty item
   context instead of inheriting a parent command.

   `formats.command-argument-expansion` closed five paths on 2026-08-24. Both rename names, both
   optional show-option names, and `select-pane -T` expand in their resolved target contexts. The
   differential fixture covers canonical names, tmux aliases, permitted unique prefixes, old
   session/window facts, and directional pane-title application. `formats.new-session-name-expansion`
   closed on 2026-08-25: `new-session -s` now expands once before attach-or-create lookup, carries
   only a genuinely attached client's target facts, preserves explicit command-item precedence,
   and refuses a nested formatted `-A` attach before applying its effect while leaving a formatted
   detached miss intact. `formats.name-validation-cleaning` then closed the adjacent pinned name
   pipeline: `new-session -n` expands, validates, and vis-cleans before `-s`; `new-window -n`
   expands once in its destination session context with session format type before the same helper;
   both rename commands expand through their resolved active pane with pane format type and then use
   that helper; and `break-pane -n` deliberately stays literal before validation and cleaning. Empty
   names and valid Unicode survive, ASCII controls fail before
   mutation, cleaned backslashes determine identity and collision or reuse behavior, and a detached
   formatted `-A` miss is no longer refused merely because its raw format text names another
   session. `formats.creation-name-edges` closed the last two edges on 2026-08-25. An unindexed
   `new-window -S` performs the pin's second format pass over the cleaned first-pass value for lookup
   while creation keeps the first-pass name. An explicit `break-pane -n` pins window-local
   `automatic-rename off` on both placement paths. The full mux suite passes 379 tests, and the
   125-step `command-item-format` plus 30-step `break-pane` differentials report zero differences and
   no skips.
   `formats.buffer-path-expansion` closed on 2026-08-25: `load-buffer` and `save-buffer` now expand
   paths once through the shared daemon command hooks before home-directory handling and file I/O.
   Client selection supplies the target session, focused window, and active pane; aliases and unique
   prefixes retain the canonical command name, explicit item state wins, and replacement text is not
   expanded again. `protocol.binary-streams`, `buffers.clipboard-write`, and
   `buffers.client-file-context` retain the stream, clipboard, relative-path, attached-session,
   and remote transport work. The separate 29-step
   `native-prefix-isolation` fixture closes all 25 unique prefixes that native names had changed
   and checks ambiguous `list-commands` exit parity. The daemon authorizes one expanded alias
   invocation and dispatches that same value. Stored bind-key and set-hook lists execute their
   constructed commands without another user-alias lookup. Read-only clients authorize that frozen
   chain before any effect; writable execution uses the same alias boundary. Typed `if-shell`,
   `run-shell`, and `confirm-before` callbacks remain frozen after lexical construction. Typed
   `if-shell` and `run-shell` callbacks preserve physical groups: a failed group stops its remaining
   commands while later physical lines continue; string callbacks remain one group. Typed
   `command-prompt` templates retain their structured prepared command list through submission
   without re-expanding aliases. Structured substitution preserves leaf-argument boundaries. String
   templates substitute raw source before a fresh parse and complete construction pass against the
   current alias table. Both paths replace the first `%%` and every `%1`, with trailing-percent
   quoting. Typed callbacks retain physical groups, while string templates and free input form one
   group. Prompt chaining and multi-answer `%2` retain their prompt owner. `set-hook` and
   command-valued native set-option retain their intentional second construction stage. Built-in
   hook values flatten physical groups during that pass, custom `@` values retain normalized
   textual groups, and typed ignored `-R` values still construct. A typed `display-menu` action
   drops its structural wrapper before the fresh selection parse, while a quoted brace string stays
   literal. A
   typed alias result now keeps an exact empty, multi-command, or unparsable match from falling
   through to the canonical or catalog-alias command it shadows. Actual empty and multi-command
   execution remains under `aliases.command-bodies`.
   Protocol v74 closes Control's static unknown-name precheck by preparing each complete input unit
   under one daemon lock before framing. Prepared execution freezes only that alias lookup and still
   reauthorizes normally. A local CLI with a compatible daemon prepares the complete vector before
   exact attach, TUI, stdin, and kill-recovery routing, then carries the immutable vector across a TUI
   reconnect. Slice 10r closes the missing-daemon seam with a raw alias-free syntax pass over the 83
   implemented and nine recognized parked tmux verbs. Canonical names, built-in aliases, unique
   prefixes, flags, arity, typed callbacks, exact native attach tails, and `-N` validate before
   routing or spawn. An arbitrary startup alias cannot trigger autospawn, while a canonical spelling
   remains eligible for startup-config shadowing. A generation tag then binds the spawned daemon to one exclusive first-external
   bootstrap lease. The daemon prepares the full vector under one post-config alias snapshot before
   effects; a failed preparation stops it after the owner's response and disconnect only while the
   lease remains uncontested. Startup reentry does not contest it, a second external client contests
   it permanently, a pipelined command commits it before worker scheduling, and a stopping daemon
   rejects registration. Runtime failures retain tmux queue ordering. Remote `--host` routing remains
   under `aliases.remote-client-preflight`. Slice 10y closes config replay alias snapshots, slice
   10u closes warm unaliased argument groups, and slice 10z closes config and source file-unit
   construction under `mux.chain-parse-abort`.

   `tracker.args-parse-inventory` closed callback discovery on 2026-08-25. The oracle rejects an
   unknown callback body, the Rust catalog carries typed rules for all 12 implemented callback
   commands, and the third manifest test requires one `args-parse:` item for each implemented
   callback command absent from `COMMAND_ARGS_PARSE_BEHAVES`. The behaving roster now contains
   `bind-key`, `choose-buffer`, `choose-tree`, `command-prompt`, `confirm-before`, `display-menu`,
   `display-panes`, `if-shell`, `run-shell`, `set-hook`, `set-option`, and `set-window-option`.
   Every implemented callback command now appears in the behaving roster, so no command-specific
   item remains. The
   unimplemented `choose-client` and `switch-mode` callbacks stay covered by their command items.

   Slice 10l closed hook-producer discovery with a daemon-owned partition of all 68 pinned hook
   names. The source roster names 27 explicit event producers, while the test derives 37 generic
   `after-<command>` producers from implemented command names. A later pin audit classified
   `after-queue` as explicit-only: ordinary queues never produce it, while `set-hook -R` runs it.
   The current partition contains 64 automatic producers, that explicit-only hook, and three active
   gaps: `pane-focus-in`, `pane-focus-out`, and `pane-set-clipboard`. Slice 10m makes the default-key structural partition
   explicit: 303 pinned bindings and 251 zz defaults yield 193 shared keys, 110 tracked omissions,
   58 tracked native keys, 51 tracked command-or-repeat divergences, and 142 structural matches.
   The matches split into 49 copy-mode, 61 copy-mode-vi, and 32 prefix entries without claiming
   complete runtime parity. Bare key-only `bind-key` now also preserves commands and unspecified
   metadata, replaces a note only with `-N`, sets repeat with `-r`, and leaves an absent key
   unbound while creating its table. Slice 10n then closes raw-TUI confirmation presentation and
   input handling, including exact key case, modifier reduction, Enter defaults, pending-reply capture, and seven attached
   reply paths that prove the response does not reach the pane. Slice 10s partitions all 198 pinned
   global format names into 92 direct mux values, 32 values delegated through `StatusHooks`, and 74
   constant-backed live gaps. The required daemon test resolves each delegated name through the
   production consumer. Slice 10v closes the open-ended and dynamic context-format discovery blind
   spot with schema 5 source ownership. Option `BEHAVES` consumer truth remains separate under the
   open successor groups.
   Daemon invalid-flag
   coverage first closed on 2026-08-27 with
   a 24-command production-dispatch roster. The shared flag closure on 2026-08-28 removed that
   partial roster and routed daemon preflight through the catalog parser used by mux execution.
   The first eight positional maximum mismatches closed later that day, followed by all fourteen
   required minima. The full shared arity closure then removed the partial maximum roster: all 72
   implemented finite upstream commands now validate their catalog maximum after option grammar and
   minima but before unsupported-capability rejection, targets, or effects. Stored binding and hook
   children use the same bounds before unsupported-capability rejection or prior-state replacement.
   The three-step `positional-maximums` fixture checks 71 generic-CLI-routed
   command-drivable canonical names and 62 aliases with exact stderr and unchanged pane, buffer, and
   file state; an exhaustive daemon test covers all 72 engine paths and aliases. The minimum fixture
   retains its exact canonical and alias proof. The shared option parser now covers all 83
   implemented upstream commands and 74 aliases, including stored commands and exact native attach.
   Its three-step differential compares 516 probes against zz and the pin with exact diagnostics,
   required-value absorption, optional-value lookahead, and unchanged sentinels. The final
   `mux.error-shapes` item closed on 2026-08-28 when nested `new-session` adopted the pin's exact
   validation order ahead of its mutation-free nesting refusal.
   `knowledge/tmux/gaps.md` remains generated from the registry.

Without this gate, easy compatibility fixes can land while the persisted proof quietly goes stale.

## Milestone 1: close the literal alias path

1. Remove the empty-daemon `has-session` dead end and make bare packaged `zz` create and attach.
   **Complete.**
2. Apply the same nested-session refusal to attaching `new-session`, not only `attach-session`.
   **Complete.**
3. Pin the terminal size and read-only/detach variants in packaged PTY tests. **Complete:** a
   detached `new -d -x 93 -y 29` retains exact window/pane geometry; an attached 97x31 outer PTY is
   published as the client's dimensions; a read-only attach drops ordered terminal input while
   continuing to display externally produced output; and requested detach plus `attach -d` eviction
   both exit zero with `[detached (from session NAME)]` after terminal restoration.
4. Document that `alias tmux=zz` is a shell boundary. Keep the global executable shim opt-in so zz
   does not steal an installed tmux binary. **Complete:** the compatibility guide documents the
   shell-alias limit and the daemon's private shim for shell and status jobs.

This is the smallest milestone that changes the answer from “the alias fails on first run” to “the
alias starts and reconnects correctly.”

## Milestone 2: close the cheap, high-frequency surface

1. Implement prompt history commands. **Complete.**
2. Consolidate native command discovery. **Complete.**
3. Mine the pinned corpus and a small personal-config corpus for actual rejected flags.
   **Complete for the current pin:** bare `list-keys` formatting is the sole remaining rank-5
   runtime hit; `client_last_session` is deferred with client-context work and `new-session -t`
   stays parked with linked sessions.
4. Implement only the rank 4 and rank 5 items those fixtures hit. **Current slice complete:** bare
   `list-keys` default padding, explicit `config_files`, and `pane_dead_time` are differential-clean.
   Further state facts remain demand-driven.
5. Differential-test every landed flag, update its registry entry, and regenerate the report before
   merging the slice.

Do not sweep flags alphabetically. A config-hit flag is worth more than ten unused palette knobs.

## Milestone 3: make script output trustworthy

Close accepted divergences that scripts observe even though the command catalog says the syntax is
supported:

- `capture-pane` stdout/buffer routing and range selection. **Complete for ordinary retained text:**
  `-p` prints without touching `-b`; without `-p`, named and automatic buffers receive the capture
  with the pin's trailing newline. The 23-step differential fixture covers clustered value flags,
  inclusive and reversed bounds, target-scoped format expansion, and silent invalid/out-of-range
  fallback. A fallback `-E` over trailing blank viewport rows still omits tmux's blank newlines;
  saved alternate screens and the six richer capture transports remain explicit gaps.
- `show-buffer` binary policy.
- `list-keys` note rendering and `-N`/`-a`/`-P` selectors. **Complete:** the 19-step fixture pins
  exact rows and deterministic root/prefix ordering. The 2026-08-24 extension closes positional
  filtering plus `-1`/`-O`/`-r`, while keeping only tmux's non-total comparator ties as a bounded
  accepted divergence.
- `source-file` client-relative path resolution. **Complete for registered clients:**
  protocol v72 retains one local caller cwd, SSH omits it, and CLI coverage separates caller and
  daemon cwd. The daemon now keeps that selected base through nested replay, including after an
  ordinary sourced command clears the mutable context cwd and when runtime `source-file` loads the
  active default `zz/mux.conf` through the ordinary path. A direct zz-native `reload-config` carries
  the same selected base for registered clients. The 2026-08-26 session-cwd slice retains one cwd
  per session, selects compound `attach-session` targets before pane-context `-c` expansion, and makes attached source selection
  prefer the invoking client's session cwd. The full attached-client fixture separates command cwd
  from session cwd with decoys, and a focused daemon test adds a third `source-file -t` target cwd
  decoy. Command replay already retains the caller cwd for sourced hooks. Control sourced-hook cwd,
  deferred event hooks, and initial startup replay retain separate gaps.
- command-prompt only where a real automation uses it.
- exact exit, stdout, and stderr on the published workload.

This milestone matters more than raising the command count. Scripts consume bytes, not catalog
totals.

## Milestone 4: add one client-context model

The first tracked slice, `clients.cwd-context`, completed on 2026-08-23. Protocol v72 appends one
bounded `ClientHello.working_directory`; local endpoints publish an absolute cwd and SSH endpoints
omit their caller-host path. Non-UTF-8 or oversized local paths are omitted rather than breaking the
connection. The daemon retains the accepted fact per client and resolves relative top-level
`source-file` paths after `-F` expansion and before globbing. It snapshots that selected base for
registered clients and carries it through nested replay, so a sourced ordinary command cannot erase
the next nested source's cwd. Sourcing the active default config through the ordinary runtime loader
forwards the same snapshot. A direct zz-native `reload-config` forwards that snapshot for registered
clients. Startup keeps its separate clientless bootstrap gap. CLI coverage pins literal cwd glob
metacharacters, glob order, declared-path order, quiet continuation, and declared missing-file
diagnostics independently of the daemon cwd. The `clients.attach-session-cwd` slice closed on
2026-08-26: each session retains a cwd seeded from explicit `new-session -c`, its attached source
session, or caller cwd; `attach-session -c` selects a resolved compound target's window and pane,
then expands and stores before terminal validation; attached `source-file` and `reload-config` prefer the invoking client's session
cwd while `source-file -t` remains a separate target. This uses internal state without a protocol or
snapshot-schema change. The 2026-08-27 `clients.attach-flags` slice closes `attach-session -f`,
attaching `new-session -f`, and durable client attach context without changing protocol v81. Typed
daemon state retains tmux's comma mutation grammar across switches, detach, native attach, and TUI
reconnect, and clears it on client teardown. Common requested flags and Control-only flags report
through `#{client_flags}`; `pause-after` follows tmux's unsigned prefix and wrap behavior. The fresh
attached differential covers missing targets, fresh and detached creation, switching, reattach,
teardown, and the deliberate `-r` difference; Rust tests cover terminal-open ordering and `-A`.
zz `-r` adds read-only only, while tmux also adds ignore-size. The completed
`clients.attach-sizing` slice consumes retained client size and `ignore-size` state without a wire
or snapshot-schema change. `resize-window -A` and `-a` choose the largest or smallest width and
height independently, then store that one-shot result as a durable manual extent. Any attached
unignored client globally suppresses ignored candidates; if every attached client is ignored, the
ignored candidates become eligible. Control participates only after explicit `-C`; a per-window
override beats its global size and hard-clamps each final dimension. An empty target candidate set
uses `default-size`, and the final extent clamps to 10,000. Later client size updates do not move
the manual window. The 2026-08-27 `clients.attach-environment` slice appends a bounded UTF-8
environment snapshot in protocol v82. Fresh sessions and later attaches copy the invoking or selected client's
exact and wildcard-matched `update-environment` values. Missing names become unset markers, empty
values stay set, and selected hidden values become ordinary. Existing `new-session -A` follows
attach behavior and ignores `-e`; `-E` preserves the session map. Control, PTY, native attach, and
targeted switch use the same rules, while `switch-client -T` returns before refresh. The session
map survives client disconnect, affects future panes, and does not rewrite existing process
environments. Non-UTF-8 Unix entries remain under `clients.path-encoding`. The full attached
differential passes against pinned tmux. `no-detach-on-destroy` now applies the newest-session
fallback per client under `on` and only when `no-detached` lacks a detached primary. Mixed-client
daemon tests and the attached fixture cover its fallback and exit paths without a wire change.
`active-pane` retains state but stays open under its own consumer gap. Deferred event-hook
client selection remains under
`source-file.event-hook-client-cwd`. Slice 10ag carries the cold launcher's bounded cwd through a
private daemon argument while startup configuration runs, matching tmux's initial `cfg_client` cwd
rule without changing the public protocol. `source-file.sourced-hook-client-cwd` tracks hooks raised during
Control replay because that path clears the replay client before the hook runs. Command replay
already carries the caller cwd. The Unix POSIX glob
dialect slice is closed: source matching now uses `glob(3)` with tmux's bytewise cwd quoting,
backslash rules, leading-dot exclusion, repeated-star behavior, malformed-pattern handling, and
per-pattern order. The tilde slice is also closed: `source-file` leaves a literal leading tilde for
normal relative-path resolution, while tildes expanded by the config parser already arrive as
absolute paths. The nested declared-path slice is closed: loud no-match and glob errors reach the
invoking client with the post-`-F` declared argument, while a quiet no-match stays silent. A direct
Control all-miss aborts its line; a direct partial match ends with `%end` and continues; matched
parser errors remain `%config-error`. Protocol v76 now gives each parser-owned replayed command that
survives command-name resolution a tail-tag-47 `SourcedCommandGuard`. An alias resolved to
`source-file` before replay retains the same recursion path. Unknown or ambiguous command names and
malformed alias names publish a located Warning that Control renders as `%config-error`, without a
guard. Ordinary success and quiet all-miss use an empty flags-1 `%end`; a mixed hit and miss keeps the
declared-path diagnostic inside `%end`; and all-miss, flag or arity failure, runtime failure, or depth
refusal ends `%error`. Runtime failures alone set `client_failure`, and the Control writer defers
guards FIFO until the direct outer frame closes. Matched OS and path read failures follow as typed
standalone Error events, including numeric OS errors and colon-space paths. Invalid UTF-8 config
content remains under `config.non-utf8-file-bytes`: pinned tmux accepts the measured lone-`0xff`
file without a visible diagnostic where zz emits a typed Error and status 1. Config and lexer
Warning prose remains under `control-mode.diagnostic-typing`. The existing loader preflights every
declared path for one source command before recursion. A focused regression and the then-six-step
Control differential prove root missing-path guard, then middle missing-path guard, then leaf output
guard, each exactly once.
That closes `source-file.nested-control-queue` with no production change. The later
`control-mode.source-file-exit-status` closure completes the long-lived Control matrix. Direct and
parser-owned sourced runtime errors plus nonruntime source failures set retval 1. Generic nonzero successes and
flags-1 parse or preparation errors do not set or change it, so a fresh client stays at 0 while a
prior sticky failure stays at 1. A blank line or EOF snapshots the current value. A Return captured
while a preceding non-detach command waits precedes later queued stdin commands, including detach;
a Return observed while self-detach itself waits is discarded on the caller's `Detached` event. Only
a caller-targeted `Detached` event exits 0, so nonself and no-victim detach forms keep the loop alive.
The command response closes before `%exit`.

The synchronous inserted-list slice now retains flags-1 Control identity through foreground
shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C`.
Per-client and per-thread capture publishes the containing replay command before each inserted
command, and an inserted source before its children. Output, failures, status, and nested ordering
remain command-scoped without folding or leakage. An unsupported zz-only inserted command gets an
empty success guard and later siblings continue, but it does not join `ConfigLoadReport`'s skipped
summary. An unknown child command produces successful parent and source guards, then `%config-error`
without its own guard, matching the pin. The closure reuses protocol v76. The later protocol v77
closures give immediate hook commands and shell-evaluated `if-shell -b` or `run-shell -bC` callbacks
flags 0. Protocol v78 later closed source-read placement and completion numbering.
Hard-disconnect queue cancellation remains under its active Control group. Protocol v81 later closed
foreground Control shell-output placement; ordinary `run-shell -b` now uses zz's native
per-Interactive command-output view.
The nesting limit is closed for guard
placement, depth wording, count, and
later-line continuation: both sides run 50 concurrent source invocations counting the initial
`source-file` as invocation 1, and refuse invocation 51 with `too many nested files` on the
client-specific channel. A malformed invocation at that depth is diagnosed as malformed rather than
refused for depth on both sides, because the pin rejects it while parsing the containing file and
never consults its depth guard, and zz now runs its depth guard after the command's own flag and
positional validation. The later shared arity and flag closures also matched the malformed text.
The pin still abandons the rest of the containing file where zz continues it, which
`config.parser-edge-cases` owns. The refused nested command now uses the same flags-1
`%begin`/`%error` guard as the pin.
Same-line replay
grouping is closed independently: synchronous invalid/runtime errors, depth refusal, and a loud
zero-file source miss or glob error drop only later siblings on the same parser-owned source line,
while later physical lines continue. Matched sources and asynchronous commands do not propagate
child failures into their invoking line. The daemon retains a matched child OS or path read failure
in the load report without using it to prune the parent group. Quiet zero-file misses succeed. In this
slice zz-classified unsupported capability gaps changed from pruning later same-line siblings to
skip-and-continue. That continuation is desirable for zz import capability gaps but remains
pin-unproven because the corresponding commands are unsupported in zz. The synchronous inserted
path shares the continuation policy but does not add its unsupported commands to the load report's
skipped summary. Replayed error delivery is
closed for the pinned target and set-option failures on Command, parser-owned Control, and attached
clients. Successful output plus command-name and parser diagnostics share one per-invocation
transcript. Each invocation
appends its complete `-v` batch, replays every parsed match, then appends buffered command-name,
and parser diagnostics. Source no-match, glob, and actual OS or path read failures retain their
existing error channels.
A nested source inserts its own complete frame at the parent command's replay position, so nested
frames are depth-first. This does not claim physical verbose and replay interleaving. Command clients
receive the transcript once on stdout. For valid successful replay and `-v` output, Interactive
clients open one command-output view without duplicate Info or Warning events. Parser diagnostics
may still publish their existing Warning summary. Successful output leaves stderr empty and status
zero. A runtime failure retains stderr and status 1 while stdout before and after it remains ordered.
Cross-depth parser-owned Control ordering, synchronous inserted flags-1 framing, and
return-versus-detach precedence are closed. The later protocol-v77 slice also closes immediate command
hook flags-0 frames. The later background slice closes callback frames, and protocol v78 closes
parser and hook-source raw read placement plus completion numbering. Config byte input, parser abort,
and error shapes remain with their named groups; the same-line close did not cover those contracts.
Startup accounting is closed: one
budget spans every startup root, the roots do not count, source commands 1 through 50 run, and later
source commands retain their declaring file and line while runtime sequential sources stay
unbounded. Protocol v80 later closes retained delivery and placement. Startup parses every root
before replay, retains normalized root and nested read failures, parser diagnostics, unsupported and
runtime failures, and successful `display-message -p` output, and discards list-style output. Root
causes precede replay causes; replay stays root-ordered and nested depth-first. The detached launch
stays silent and cannot drain the set. Control receives the raw bounded vector with pinned
`ServerHello` or attach-frame placement. An attached Interactive winner receives an ordered,
UTF-8-safe 64 KiB preview that replaces every Unicode control except LF and TAB. Its explicit
Control-mode recovery notice reflects the product decision not to promise exact Interactive
recovery of the retained 1 MiB vector. The startup view alone uses a pinned Ghostty 64 MiB byte
history cap; ordinary output retains its 100,000-byte setting. Attached delivery commits only after
`Attached`, the diagnostic, resync, and mux options remain admitted; admission failure retains the
causes and retires only the startup actor by exact ID. Replayed runtime failures
retain encounter order, use the
invoking client's error channel, set the Command status and parser-owned Control status, capitalize
the attached warning, and continue later physical lines through an outer source. The config parser
group separately retains tmux's first-error file abort and its unusual tilde expansion immediately
after a closing quote.
The `source-file.flags` slice is closed through the existing effect and replay-loader seam. One
ordinary invocation parses every declared and globbed match before replay. A bare assignment in an
earlier file applies during parsing, affects a later file's conditional, and persists. A replayed
`set-environment` runs after the later file was parsed, so it cannot change that branch but persists
after replay. `-n` applies neither assignment nor command effects, later parse-only files see the
assignment as absent, and `-v` still reports the selected branch. `-t` resolves one pane context for path formats and replay, with a quiet empty context on a
miss and no change to the invoking client cwd. `-v` preserves file and line order, inherits through
nested sources, and stays suppressed for Control. Full tmux command, flag, and arity validation
during parse remains under the parser, error-shape, and chain-abort groups. Command and Interactive
transcript presentation and ordering are closed under `config.replayed-command-output`. Protocol
v79 closes the TUI output view's local keyboard contract: live copy tables, line and page movement,
search editing and repetition, selection-to-paste-buffer, and vi/emacs exits. Mouse, OS clipboard,
ordinary TUI pane copy-search editing, and the wider 29-action vocabulary stay outside that closure.
Runtime `source-file` now treats the active default config like every other matched path: one
invocation parses all matches in declared-path and glob order, then replays them in the same order.
Declared default, after, and default files apply as `DAD`; a loud miss
returns status 1 without stopping later matches; and ordinary diagnostics plus `-v` lines retain
declared path and match order. Explicit native `reload-config`, startup first-existing discovery,
and ordered explicit `-f` roots keep their separate behavior. Parse-only and nested paths are
unchanged. Focused CLI and daemon tests, strict clippy, fmt, and the 12-step diagnostics, 40-step
format, and then-six-step Control differentials pass with zero differences and no skips. The later
Control return-status close grows that focused row to eight. Neither focused run refreshes the stored
canonical row, which remains at three steps.
Control source diagnostics now use the existing Error kind and reach standalone `%error` frames
without text classification. Config summaries still use Warning events, so
`control-mode.diagnostic-typing` retains only future or localized config wording.
The new client accepts the old daemon's known source Warning families. The reverse version mix can
hide source diagnostics because the old client ignores Error events; downgrading the app requires a
matching daemon restart.
Byte-preserving non-UTF-8 Unix cwd transport remains separately visible under
`clients.path-encoding` instead of making such a path a connection failure.

Keep the remaining client work split by the state it needs. Session cwd, requested client flags,
attached resize aggregation, bounded UTF-8 environment seeding and refresh, and retained client
format facts have shipped.
`detach-client -E` and the
parent-HUP trio (`attach-session -x`, attaching `new-session -X`, `detach-client -P`) need typed
client exit actions and are not prerequisites for targeting or ordinary detach. Creation-time
`new-session -e/-E` works without a wire change: explicit overlays persist and reach the first pane,
while `-E` suppresses client-sourced update seeding. Existing-session `new-session -A -E` and
`attach-session -E` now preserve the destination session environment.
The separate `new-window`/`split-window` pane-local `-e` and empty-pane `-E` pair belongs to the
daemon-owned spawn effect and has shipped.

## Confirmed terminal-owned blockers

Two small-looking flags are not mux-only work:

- `clear-history -H` makes the pin clear normal history and reset the active screen's hyperlink
  registry. zz's current clear action emits ED3 and clears copy, search, selection, hover, and view
  state, but the terminal API exposes no distinct VT hyperlink-registry reset. This needs one
  terminal-owned mutation action before the mux can accept the flag honestly.
- `resize-pane -T` is not a layout resize. The pin no-ops in an active pane mode; otherwise it
  removes a cursor-derived, history-capped number of history rows, advances the cursor by the same
  amount, and redraws. The mux owns neither live cursor/history state nor terminal mode, and the
  terminal API has no atomic action for the operation. It needs a terminal-owned action and result
  contract, not another layout branch.

Requested flag retention, attached sizing, and bounded environment refresh are closed. The remaining
attach work keeps client exit actions separate; per-client active panes and destruction fallback are
separate consumers of the retained flags. Reuse existing facts within
each contract, but do not make one an artificial dependency of the others.

## Milestone 5: decide whether streams earn their cost

Only after the practical alias gate is green, measure demand for binary stdin/stdout and lock
processes. If required, design one bounded command-stream channel for `display-message -I`,
`split-window -I`, `load-buffer -`, `save-buffer -`, and `source-file -`. Do not build five bespoke
transports.

# Native GUI command direction

The current surface already has the right shape. Improve it by composition:

```sh
# tmux-compatible terminal split
zz split-window -h

# native picker and browser split
zz split-picker -h
zz split-browser -h https://example.com

# materialize a pending pane explicitly
zz select-pane-kind -t %7 agent

# control existing native panes
zz agent-send -t %7 --submit 'review the failing test'
zz set-browser-url -t %8 https://example.com/docs
zz capture-browser -t %8 -o /tmp/browser.png
```

If direct agent creation becomes necessary, prefer one thin `split-agent` command that lowers into
the same pane-kind operation as the picker. Do not give `split-window` an agent flag.

# What stays intentionally different

- `%` and `"` may open `split-picker` in the zz default table.
- `s` and `w` may focus the native sidebar instead of drawing tmux's tree.
- The GPUI client uses native presentation for prompts, menus, popups, copy mode, status, and
  choosers. Raw zz-tui handles confirmations plus daemon-published menu and popup descriptors.
  `display-menu.behavior-fidelity` and `display-popup.behavior-fidelity` own the remaining behavior
  classes.
- Current window may remain per client rather than per session.
- GUI defaults may keep the persistent-daemon lifecycle until a config explicitly selects tmux
  lifecycle behavior.

Each difference needs a stable registry ID. Use the divergence matrix for detailed rationale and
probe evidence. Imported tmux commands still keep their tmux semantics.

# Permanent exclusions

- `link-window`, `unlink-window`, and grouped `new-session -t`.
- Speaking tmux's private socket protocol.
- Fleet broadcast as a special command; compose `fleet list -F` with a shell loop.

`server-access` and multi-user socket ACLs are parked outside the practical alias target, but no
permanent product decision has been recorded for them.

# Decision log

- 2026-08-08: cell-based resize approved; host selected outside tmux target grammar.
- 2026-08-09: scriptability and TUI tiers defined; `--host` adopted as the server axis.
- 2026-08-16: target upgraded to `alias tmux=zz`; linked sessions and real socket interop excluded.
- 2026-08-20: native GUI defaults may diverge while explicit tmux commands stay exact.
- 2026-08-22: current source re-audited. The target changed from percentage completion to a written
  workload gate, with native commands kept on a separate namespace and missing work ranked by ease.
- 2026-08-23: `compat/tmux-gaps.json` became the sole live status source. Schema 3 separates product
  status from `depends_on` ordering, keeps manifest-owned freshness and closed history, and expands
  the pinned oracle to flag arities, positional bounds, global formats, selected context formats,
  and the existing command, option, hook, and default-binding inventories. The only missing name in
  the selected context rosters is `command`; custom `args_parse` callbacks and open-ended context
  names remain semantic work. The generated report owns the work queue; this roadmap keeps dated
  sequencing and delivery evidence.
- 2026-08-26: agent-backbone superset surface landed as additive daemon verbs and hook-seam formats:
  `agent-send --wait` (request/reply parked on the host's turn waiter), `show-last-output` (read
  twin of `send-last-output`), `#{pane_kind}` and `#{@name}` through `DaemonFormatHooks` rather than
  the pinned format table, and user-option writes signalling `wait-for` `<name>@<target>`. No tmux
  verb changed meaning and nothing new appears in unadorned `-C`/`-CC` output. Phase 2 added
  `send-text` (paste, wait for the echo, then Enter — the only multiplexer-level fix for the
  paste/Enter race every tmux orchestrator works around) and the `@option-changed` user hook;
  then agent panes learned to speak bytes: a PTY-free shadow terminal per Agent pane fed with the
  transcript projection ([design](/designs/agent-pane-projection.md)), so `capture-pane`,
  `show-last-output`, `pipe-pane`, and the alerts work on `%agent` — excluded from client frames
  and from control-mode `%output`.
- 2026-08-24: `send-keys` adopted tmux's two parser boundaries. The command parser rejects outer
  `-C`, `-P`, and `-o`; the window-copy parser recognizes them on their action-specific tables.
  Invalid local syntax stays silent and resets the copy-mode repeat prefix. Four unimplemented
  `copy-line*` actions and the parser-failure redraw remain under `terminal.key-control`.
- 2026-08-24: `list-keys` completed its remaining grammar and presentation surface. Sorting and key
  filtering precede `-1`, facts follow truncation, Interactive clients receive a timed frozen status,
  and Command and Control clients keep stdout. Stock copy tables now expose the pin's zero repeat
  metadata without changing runtime copy repetition. zz uses a documented total order where the
  pin's truncated comparator is non-total.
- 2026-08-25: Oracle schema 4 added a fail-closed inventory for custom command argument callbacks.
  Nine callback bodies reduce to six rules across 14 commands. The protocol catalog mirrors the 12
  implemented commands, while `COMMAND_ARGS_PARSE_BEHAVES` and command-specific tracker items record
  behavior adoption. The two unimplemented callback commands keep their command-level gaps.
- 2026-08-25: vi numeric counts moved onto one flat protocol-v75 terminal action. The first `send`
  or `send-keys` command whose option prefix contains `-X` consumes the count. Its stored `-N` wins;
  otherwise zz inserts separate `-N <count>` arguments before the option argument containing `-X`.
  A list with no qualifying `-X` preserves the pending value. Raw terminal sends stop on
  backpressure. Native browser sends clamp repeats to 9,999 on both sides of the wire.
- 2026-08-26: Protocol v77 renamed tail-tag-47 `SourcedCommandGuard` in place to
  `ControlCommandGuard`, adding explicit frame flags and an independent `sticky_failure` bit.
  Immediate `after-*` and `command-error` hooks now retain the originating Control recipient at flags
  0 without copying parser replay state. Hook arrays, source descendants, failures, unknown commands,
  alias resolution, and status retention match the pin. Background inserted frames and raw matched
  hook-source read placement remained separate at that checkpoint.
- 2026-08-26: Protocol v78 appended `ControlSourceFile` at event tail tag 48. Typed `ReadError`
  events render as raw unframed lines after parser flags-1 or immediate-hook flags-0 source guards
  and retain status 1. Invisible `Complete` events consume one command number after every
  depth-admitted invocation's descendants. Depth refusals and dispatch-time syntax, arity, and flag
  rejections consume none. Invalid UTF-8, source stdin transport, parser abort, hook cwd, deferred
  event hooks, and hard-disconnect queue cancellation retain separate gaps.
- 2026-08-26: Protocol v79 added a nonzero actor ID to every real command-output frame and close,
  with zero plus no viewport reserved for an authoritative no-output resync. The client watermark
  rejects stale traffic. TUI search and resize state now belong to the actor, and the local attached
  fixture closes keyboard navigation over a 96-line output on both mode-key tables. At that
  checkpoint, startup config cause delivery became the next ease-ranked slice.
- 2026-08-26: Protocol v80 appended `StartupConfigCauses` at event tail tag 49. The post-spawn
  Control owner and late attach path now match pinned placement. Attached Interactive clients
  receive an ordered, control-sanitized 64 KiB preview rather than exact recovery of the retained
  1 MiB vector. Startup parsing, ordering, completion-line locations, one-shot admission, and
  restart behavior pass the checksum-attested seven-case pinned probe.
- 2026-08-26: Protocol v81 appended `ControlCommandOutput` at event tail tag 50. Targetless and
  invalid-target foreground shell output now follows its direct or sourced guard as raw text for
  the exact Control recipient without changing retval. Resolved-target and background output use
  zz's native attached-viewer command-output surface. The strict 12-step pinned Control differential
  has no differences or skips.
- 2026-08-26: The full Alert cohort closed on the existing timed-message protocol. Bell, Activity,
  and Silence now share per-client identity, exact bounded `<client> message: <text>` logging,
  replacement, expiry, zero-duration, input dismissal, terminal-publication freeze, and
  full-viewport thaw with ordinary status messages. Repair requests, resync, and popup viewports
  obey the same gate. Control remains outside alert message delivery
  and logging. The terminal publishes one reliable Bell event per occurrence while the mux owns the
  visible flag, closing repeated pre-visit delivery. The attached PTY fixture passed the 1,500 ms
  sticky, 5,000 ms alert, 1.8-second freeze, repeated same-pane Bell, 5.2-second stale-timer drain,
  and zero-duration sequence on zz and pinned tmux. zz keeps identity-safe timer cancellation.
- 2026-08-27: The ninth attach-context slice closed `clients.attach-sizing` without a protocol or
  snapshot-schema change. Valueless `resize-window -A` and `-a` perform one-shot componentwise
  largest or smallest client aggregation and store the result as a durable manual extent. The
  global ignore-size fallback, explicit Control ceilings, `default-size` fallback, and final
  10,000-cell clamp match the pinned contract; later client updates leave manual geometry frozen.
  The expanded attached differential passes. The canonical corpus remains the requested-flags
  checkpoint's 84 scenarios and 1,475 steps with SHA-256
  `5de67222bc2ebb99c57963be14c865ddfdddc387da34ee32dd86962cef8336c9`.
- 2026-08-27: Protocol v82 closed `clients.attach-environment` for bounded UTF-8 environments.
  `ClientHello` now carries one per-connection snapshot for local and SSH-forwarded clients. Fresh sessions,
  existing attach, native attach, Control attach, and targeted switch apply the pin's effective
  `update-environment`, wildcard, missing, empty, hidden, `-A`, `-e`, `-E`, and `-T` rules. Values
  persist in the session after disconnect; future panes see updates and existing processes do not.
  Non-UTF-8 Unix entries remain under `clients.path-encoding`. The full attached differential passes
  for zz and pinned tmux.
- 2026-08-27: Protocol v83 closed `clients.context-formats`. `ClientHello.process_id` supplies the
  last missing process fact. The daemon retains creation, activity, focus, flags, key table,
  attachment, terminal, environment, geometry, and mailbox counters, then expands the same record
  through list, ordinary command, foreground inserted-command, status, and display contexts.
  Defined Interactive and Control empties match the pin. The full attached differential passes for
  zz and pinned tmux, including an attached key binding and implicit `display-message` selection.
- 2026-08-27: The positional-maximum slice closed eight `mux.error-shapes` items without a protocol
  change. Seven commands now accept at most one positional and `select-pane` accepts none. The shared
  catalog emits the pin's exact canonical error before target resolution, buffer mutation, or file
  I/O. Focused mux and daemon tests cover canonical and alias routes; the three-step differential is
  clean. At that checkpoint required positional bounds and the broad arity, flag, and nested-session
  families remained open.
- 2026-08-27: The positional-minimum slice closed the remaining fourteen positional-bound items
  without a protocol change. One exact catalog sidecar supplies minimum one for thirteen commands
  and minimum two for `if-shell`; mux, shared daemon, menu, and confirmation parsers validate it
  after flags but before upper bounds, targets, callbacks, files, buffers, or other effects. The
  three-step differential covers all fourteen canonical names and aliases with exact canonical
  errors and unchanged state. Integration validation also restored Control `%session-changed`
  delivery by keeping hook command variables client-only while adding session identity to the
  Control publication copy and using one tmux-facing client-name ladder for Control snapshot self
  identity.
  At that checkpoint shared arity, flag diagnostics, and nested-session precedence remained open.
- 2026-08-27: The shared command-arity slice removed the partial maximum roster without a protocol
  change. All 72 implemented finite upstream commands now validate the catalog maximum after flags
  and minima but before targets or effects. Stored binding and hook children use the same bounds
  before replacing state. The strict fixture covers 71 generic-CLI-routed canonical names
  and 62 aliases, while an exhaustive daemon test covers all 72 engine paths and aliases. Flag
  At that checkpoint, flag diagnostics and nested-session precedence remained open.
- 2026-08-27: The complete CLI binary and app-library gates exposed stale assertions and two production edges
  after the client-context work. Exact native `attach-session -E` now enters daemon command
  execution for its initial attach and preserves the session environment; automatic reconnect
  behavior is unchanged. The shared attached-client selector accepts the exact published
  `#{client_name}`, including the `client-PID` fallback used by nameless Control clients. Control
  menu targeting and nonself detach therefore consume the same identity that `list-clients`
  reports. Client-flag assertions now accept the pinned `focused` and `UTF-8` facts while preserving
  the deliberate `-r`/`ignore-size` difference, and Control height remains intentionally empty.
  The command palette follows the catalog's zero positional maximum for `select-pane` and offers
  live pane targets only after `-t`. The complete CLI binary and app-library suites pass all 102
  and 639 tests.
- 2026-08-28: The shared command-flag slice replaced the partial daemon roster with one
  catalog-driven parser across all 83 implemented upstream commands and 74 built-in aliases. Mux
  execution, daemon preflight, stored binding and hook children, and exact native attach now agree
  on canonical unknown and invalid flags, pinned help usage, missing required values, required-value
  absorption, optional-value lookahead, and syntax-before-unsupported ordering. Product usage stays
  truthful in `list-commands` and completion through a separate pinned diagnostic accessor. The
  strict three-step fixture reports `COMMAND_FLAG_ERRORS=clean:516` on zz and the pin. At that
  checkpoint, parser-group atomicity, callback-specific grammar, and nested `new-session`
  precedence remained under their existing owners. No wire protocol or version change was needed.
- 2026-08-28: The final `mux.error-shapes` item closed without a protocol or catalog change. A
  mutation-free nested `new-session` preflight now follows the pin through flag and arity parsing,
  target conflicts, expanded window and session names, `-A`, duplicate detection, unresolved
  session-group name validation, and start-directory expansion before refusing nesting. The
  refusal still precedes terminal and size validation. A narrow `-t` routing path exposes this
  order without implementing session groups. Canonical names, built-in aliases, prefixes, user
  aliases, command lists, detached creation, Control clients, and already-attached clients are
  covered by focused tests and the real attached-client differential.
- 2026-08-28: Protocol v84 closed `tracker.args-parse-if-shell`, the first custom callback runtime
  rule. `CommandInvocation` carries zero-based unquoted command-block positions through source-file
  and Control parsing, postcard transport, aliases, bindings, hooks, and stored printing. The
  shared validator accepts typed true and false branches, rejects typed conditions, option values,
  and extra positionals before effects, and keeps quoted braces on the string path. The strict
  three-step scenario finishes `ARGS_PARSE_IF_SHELL=clean:12` on zz and the pin. Five callback rules
  across 11 implemented commands remain.
- 2026-08-28: The existing protocol v84 metadata closed `tracker.args-parse-run-shell` without a
  wire change. A leading `-C`, including combined forms, accepts typed blocks in every positional;
  without it every positional and every option value remains a string. Option scanning stops at
  the first positional or `--`. Under `-C`, only positional 0 executes and valid extras are
  accepted and ignored. Exact failures preserve stored bindings and hooks. The strict three-step scenario runs
  21 source-file and Control checks and finishes `ARGS_PARSE_RUN_SHELL=clean:21` on zz and the pin.
  Four callback rules across 10 implemented commands remain.
- 2026-08-28: The existing protocol v84 metadata closed `tracker.args-parse-set-option` without a
  wire change. `set-option` and `set-window-option` accept typed command blocks only at positional
  1. Option names, option values attached to flags, and extra positionals remain strings, with exact
  type failures preceding arity, target lookup, and effects. Recursive typed printing preserves
  same-line and physical-line command groups before `-F` expansion; empty blocks become empty
  values and quoted braces stay literal. The strict three-step scenario runs 21 source-file and
  Control checks and finishes `ARGS_PARSE_SET_OPTION=clean:21` on zz and the pin. Three callback
  rules across eight implemented commands remain.
- 2026-08-28: The existing protocol v84 metadata closed `tracker.args-parse-bind-key` without a
  wire change. Every positional accepts a string or typed command block while `-T` and `-N` values
  remain strings, and option scanning stops at the first positional or `--`. Typed keys expand the
  live mux environment and print recursively before lookup. Exact typed and string tails preserve
  their distinct grouping rules; a typed first tail with extras stores an empty binding. Child
  failures preserve the prior binding, and failed typed physical groups do not suppress later
  physical groups. The strict three-step scenario now drives a real attached client, runs 17
  internal checks including bare-key mutation, and finishes `ARGS_PARSE_BIND_KEY=clean:17` on zz
  and the pin. Three callback rules across
  seven implemented commands remain.
- 2026-08-28: The existing protocol v84 metadata closed `tracker.args-parse-confirm-before`
  without a wire change. Its one command positional accepts a typed block or string while `-c`,
  `-p`, and `-t` values remain strings. Every lexical typed block recursively constructs before
  its parent name, callback type, or arity validation. Each recursive path carries one independent
  user-alias layer, alias-produced subtrees disable further user aliases, self-recursion fails
  unknown safely, and nested callbacks print canonically. Empty blocks print as `{  }`; physical
  internal group newlines print as ` ;; `. String construction follows target lookup and
  parent-format expansion as one group. The strict three-step scenario runs 19 construction,
  parser, readback, alias, and exact channel checks and finishes
  `ARGS_PARSE_CONFIRM_BEFORE=clean:19` on zz and the pin. Nested bind and confirm failures are
  preflight parse errors. Eager whole-file source construction and the broader replay-channel
  placement difference remain open. Three callback rules across six implemented commands remain.
  The same audit registered `clients.tui-overlay-consumption` because raw zz-tui then dropped
  confirmation, menu, and popup state changes. Later closures shipped confirmation handling in
  10n, menu descriptor consumption in 10o, and popup consumption in 10p.
- 2026-08-28: Protocol v84 closed `tracker.args-parse-command-prompt` without another wire change.
  The command accepts zero or one typed-block-or-string template while `-I`, `-p`, `-t`, and `-T`
  values remain strings. Typed templates preserve their recursively constructed command list and
  alias snapshot through submission. String templates preserve raw source, substitute the answer,
  then parse and construct the complete result against the current alias table before effects.
  Both paths replace the first `%%` and every `%1`, with trailing-percent quoting. Typed
  substitution preserves argument boundaries against quote and semicolon injection. Typed
  callbacks retain physical source groups, while string templates and free input form one group.
  Stored source paths and lines survive the string path. The strict three-step scenario drives a
  real attached client, runs 43 internal checks, and finishes
  `ARGS_PARSE_COMMAND_PROMPT=clean:43` on both servers. Prompt chaining and multi-answer `%2`, the
  remaining prompt UI and queue contracts, eager whole-file source construction, and broader replay
  placement keep their existing owners. Three callback rules across five implemented commands
  remain.
- 2026-08-28: Protocol v84 closed `tracker.args-parse-set-hook` without another wire change.
  Without `-B`, only value position 1 accepts a typed block or string; the hook name and extra
  positionals remain strings. With `-B`, every positional lexically accepts either type, while
  `-B` and `-t` values remain strings. zz still rejects `-B` during execution because format
  monitors remain unsupported. Every typed child constructs before its parent's type, arity, or
  effects. Typed values normalize for built-in hooks, custom `@` options, and named-option
  forwarding. Built-in hooks flatten physical groups during a second construction pass, while
  custom `@` values retain textual ` ;; ` groups. Quoted braces are runtime syntax for built-in
  hooks and literal deferred storage for custom hooks. An empty or failing local append creates an
  empty local array that shadows the inherited global hook. The strict three-step scenario runs 24
  internal checks and finishes `ARGS_PARSE_SET_HOOK=clean:24` on both servers. Eager whole-file
  construction, same-source alias mutation, multiline inner-source placement, `-B` monitor
  semantics, and broader replay placement retain their owners. Two callback rules across four
  implemented commands remain.
- 2026-08-28: Protocol v84 closed `tracker.args-parse-display-menu` without another wire change.
  The parser walks positional data through repeated NAME, KEY, and ACTION states. A nonempty NAME
  consumes a string KEY and a string-or-typed ACTION before resetting to NAME; an empty NAME is a
  separator that consumes no KEY or ACTION. All ten valued flags remain strings. Typed children
  construct before parent type, arity, or effects. Accepted typed actions print canonical child
  commands in stored bindings, quoted actions remain strings, and incomplete NAME or NAME-plus-KEY
  tails defer to daemon runtime validation. Runtime resolves the current or `-c` target client
  before completeness, so an unattached command or initial Control reports `no current client`;
  initial Control uses a flag-0 `%error` and exits 1. Once attached, Control validates an incomplete
  group as `not enough arguments` before its overlay no-op and returns a flag-1 `%error`; EOF after
  that frame exits 1. Interactive ordering remains unchanged. The daemon
  already removes the structural wrapper only from a typed action before its fresh selection parse.
  The strict three-step scenario runs 34 internal checks through a PID-unique FIFO, finishes
  `ARGS_PARSE_DISPLAY_MENU=clean:34` on both servers, and reports zero differences. Attached-client
  menu rendering and input, geometry, styles, targets, formats, selected-action runtime errors,
  same-source alias mutation, eager whole-source construction, generic alias recursion, and raw-TUI
  overlay parity retain their owners. One callback rule across three implemented commands remains.
- 2026-08-28: Protocol v84 closed `tracker.args-parse-display-panes` without another wire change.
  Its optional template positional accepts a string or typed block, while `-d` and `-t` values remain
  strings. Every typed child constructs before parent option-type or arity validation.
  Aliases and prefixes retain typed positions and canonical stored readback. Daemon targetless
  routing resolves an attached client before duration validation, so a Command client can select an
  attached Interactive client while a truly clientless command reports `no current client`. The
  strict three-step fixture runs 22 internal checks and reports zero TOPO, GEO, FMT, OUT, or WARN
  differences. The runtime custom action remains visibly parked under
  `display-panes.command-template`: tmux substitutes the selected `%pane` for `%%%` and executes with
  the retained original queue state, with `select-pane -t "%%%"` as the omitted default. Mux
  execution rejects the positional value and the native overlay has a fixed select-pane action.
  Queue blocking and presentation retain separate owners. Two callback commands under one effective
  rule remain.
- 2026-08-28: The combined 10j/10k chooser milestone closes
  `tracker.args-parse-choose-buffer` and `tracker.args-parse-choose-tree` without another wire
  change. This deliberate exception to the planned one-command split uses one shared
  commands-or-string callback, one chooser-template execution path, and one attached-client proof.
  Both commands accept zero or one string-or-typed template while `-F`, `-f`, `-K`, `-O`, and `-t`
  values stay strings. Typed templates freeze constructed aliases before opening; string templates
  parse against the current alias table after selection. The daemon closes the chooser, substitutes
  the exact buffer name or tree target, and executes against the invoking client's live context.
  Empty and stale buffers run no custom action, and attached parse or runtime errors start with an
  uppercase character. The strict three-step fixture runs 26 internal checks and finishes
  `ARGS_PARSE_CHOOSERS=clean:26` with zero differential channels on both servers. The accompanying
  parser fix makes direct and stored positional bounds outrank recognized parked capabilities.
  All 12 implemented callback commands now apply their pinned rules; no command-specific
  `args-parse:` item remains.
- 2026-08-28: Slice 10l closed `tracker.hook-producer-partition` without a protocol or runtime
  change. A daemon-owned invariant names 27 explicit event producers and derives 37 generic
  `after-<command>` producers whose suffix names an implemented canonical command. It reads the four
  active `hook:` items from the tracker, rejects duplicates and produced-versus-tracked overlap, and
  requires the 64 produced names plus `after-queue`, `pane-focus-in`, `pane-focus-out`, and
  `pane-set-clipboard` to equal all 68 pinned hooks. `just compat-check` requires the named daemon
  test and runs it through `--exact`. The source-registration closure leaves the canonical 98
  scenarios, 1,517 steps, attached-client `PASS`, two registered GEO rows, and summary digest
  unchanged.
- 2026-08-28: Slice 10m closed `tracker.key-binding-behavior` without a protocol change. The
  manifest test pins the 303/251/193/110/58/51/142 default-binding partition and the 49/61/32 table
  split for structural matches. Those matches remain structural evidence only; their command and
  action consumers keep their existing owners. Runtime handling now accepts bare `bind-key KEY`,
  ensures the selected table, mutates only an existing binding's requested note and repeat fields,
  and silently leaves an absent key unbound. The strict three-step bind-key row grows from 16 to 17
  internal checks and reports zero differences. Its harness step count and the canonical summary
  digest remain unchanged.
- 2026-08-28: Slice 10n closed `clients.tui-confirm-before-overlay` without a protocol or daemon
  change. Raw zz-tui now seeds and resets retained confirmation state with its connection, renders
  the prompt in the status or sidebar message area, hides the cursor, and intercepts input before
  normal shortcuts and pane delivery. Exact ASCII case, tmux-style Meta and Enter modifier handling,
  custom confirmation keys, Escape, pending replies, key repeat and release, paste, pointer input,
  and named nontext keys are covered by focused tests. The attached fixture compares default and
  Meta accept, default reject, custom-case reject and accept, default-no Enter, and `-y` Enter on zz
  and pinned tmux. A one-byte pane sentinel
  proves every reply is consumed. The tracker now has 87 active groups, 581 classified active
  items, and 95 closed entries. Menu and popup presentation remain active under
  `clients.tui-overlay-consumption`; menu is slice 10o.
- 2026-08-28: Slice 10o closed `clients.tui-display-menu-overlay` without a protocol or daemon
  change. Raw zz-tui now retains and clears menu state with its connection, renders the
  daemon-published descriptor after chooser and command-output bases, and resolves menu keys through
  the same renderer-free client helper as GPUI. Input capture runs before confirmation, global
  shortcuts, and pane delivery; paste and pointer input do not activate an item. The attached
  fixture covers a title, shortcut-before-cancel behavior, disabled and separator skipping, Escape,
  an unusable PageUp landing plus stay-open Enter, nonactivating paste, and a pane sentinel. Focused
  resolver coverage separately pins exact raw-row-zero and all-disabled boundary behavior. This closes the
  raw-TUI descriptor-consumption boundary only. Daemon-side geometry construction, mouse `-M`, full
  shortcut grammar and display, live style and resize refresh, Interactive queue ordering,
  selected-action target and error ordering, and close-mid-paste ordering remain under
  `display-menu.behavior-fidelity`. Popup consumption remains under
  `clients.tui-overlay-consumption`. The tracker now has 88 active groups, 589 classified active
  items, and 96 closed entries. The stored acceptance artifact remains 98 scenarios and 1,517 steps
  with attached-client `PASS` and SHA-256
  `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.
- 2026-08-28: Slice 10p closed `clients.tui-display-popup-overlay` without a protocol change. Raw
  zz-tui now retains popup descriptors and synthetic terminal frames, centres and clamps the
  published grid, renders every border family and style between the workspace and higher overlays,
  and purges popup caches across close, replacement, attachment change, and reconnect. Popup keys,
  held-key lifecycles, paste, and tracked content-relative pointer and wheel input resolve before
  chrome or pane input. External focus retains its client-state update, stays out of live popup
  terminals, and closes a dead `-k` popup on FocusOut, matching the pin. The daemon mouse gate now admits tracked popup input when the
  global mouse option is off, and one decoded tracked wheel notch produces one application report.
  Attached cases A/B/C prove unchanged live job and geometry, exact click and wheel reports at cell
  `3,3`, dead `-k` retention, and a final decimal-122 underlay sentinel. Pinned tmux emits three
  internal underlay focus pairs; zz emits none. Focus-reporting live popup applications prove
  external focus is swallowed on both, the dead case proves FocusOut closes it, and the complete
  fixture passes under `LC_ALL=C` with ACS borders.
  `display-popup.behavior-fidelity` retains resize, style, pointer-affordance, popup-to-pane, and
  Kitty-image work. The tracker now has 88 active groups, 594 classified active items, and 97 closed
  entries. The acceptance artifact remains 98 scenarios and 1,517 steps with attached-client
  `PASS` and SHA-256
  `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.
- 2026-08-28: Slice 10q closed `clients.no-detach-on-destroy` without a protocol or snapshot change.
  The daemon freezes the configured primary and newest-session fallback before moving any client.
  The primary applies to every client, while only a client retaining `no-detach-on-destroy` may use
  the fallback under `on`, or under `no-detached` when no detached primary exists. The attached
  fixture gives two real raw clients one victim session, flags one by exact tty, and proves that it
  survives on the newest fallback while its unflagged peer exits on zz and pinned tmux. The accepted
  artifact remains 98 scenarios and 1,517 steps with the same attached `PASS` and digest.
- 2026-08-28: Slice 10r closed `semantic:local-cli-autospawn-parse-abort` without a protocol or
  snapshot change. The raw gate validates canonical names, built-in aliases, unique prefixes, flag
  grammar, arity, and typed callbacks across all 83 implemented and nine recognized parked tmux
  verbs before a cold local route can read stdin, enter an exact native attach or TUI path, or spawn.
  Exact attach validates its later commands, and `-N` cannot autospawn. The spawned daemon receives a
  private generation identifier and prepares the complete vector under one post-config alias
  snapshot. Its first external client owns a one-shot exclusive bootstrap lease. Startup reentry is
  ignored, a competing external client makes contention sticky, a pipelined command commits
  it before worker scheduling, and an uncontested preparation failure stops the daemon only after the
  owner receives the result and disconnects. An arbitrary startup alias cannot trigger autospawn,
  while a canonical spelling remains eligible for startup-config shadowing. The 11 cold fixture probes pass on zz and pinned tmux.
  The 111 CLI, 640 app-library, 711 daemon, 422 mux, and 206 protocol tests pass, as does strict
  affected-crate clippy. The full strict and attached checkpoint remains 98 scenarios and 1,517 steps
  with attached-client `PASS` and SHA-256
  `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`. Warm unaliased argument groups
  and config or source-file replay remain under `mux.chain-parse-abort`.
- 2026-08-28: Slice 10s closed `semantic:tracker-nonconstant-format-behavior` without changing
  runtime, the pinned oracle, protocol, scenario inventory, or accepted artifact. Source-owned
  registries partition all 198 pinned global format names into 92 values resolved directly by the
  mux, 32 delegated through `StatusHooks`, and 74 constant-backed names retained as live `format:`
  gaps. The partitions are pairwise disjoint and exhaustive. A required exact daemon test seeds
  buffer, client, and session facts and proves that the production `DaemonFormatHooks` consumer
  resolves all 32 delegated names. Context-specific value parity remains outside this registration.
  The tracker now has 87 active groups, 593 classified active items, and 100 closed entries; 121 of
  187 groups are resolved (64.7%). Open context formats and option consumers remain the two
  discovery blind spots. The accepted artifact stays at 98 scenarios and 1,517 steps with
  attached-client `PASS` and SHA-256
  `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.
- 2026-08-28: Slice 10t closed `formats.session-path/format:session_path`. The mux now expands the
  selected session's retained cwd at use time. The five added `formats-values` steps prove two
  creates, two explicit targets, filtered list output, and lexical `..`; mux tests cover missing
  retained state and production `attach-session -c` updates. The source partition is now 93 direct,
  32 delegated, and 73 live gaps. `format:session_active` remains open for a tri-state producer
  audit. The tracker also classifies the adjacent `new-session -A -c` mutation and fresh explicit
  empty `new-session -c ''` mismatch under `sessions.new-session-attach-cwd`. It now has 88 active
  groups, 594 items, and 101 closed entries; 122 of 189 groups are resolved (64.6%). The accepted
  artifact covers 98 scenarios and 1,522 steps, including an 18-step `formats-values` row, with
  attached-client `PASS`, two registered GEO rows, and SHA-256
  `810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`.
- 2026-08-28: The post-10t rerank froze slice 10u on
  `mux.command-group-argument-parse-abort`. It owns warm local unaliased whole-vector argument
  preflight before effects while preserving runtime error ordering and exact native attach's
  dedicated parser. Config and source replay remain in `mux.chain-parse-abort`; alias snapshotting
  stays outside the slice. The registry split leaves 89 active groups with 594 items and 101 closed
  entries: 48 open, 20 blocked, and 21 accepted, with 122 of 190 groups resolved (64.2%). No runtime
  or artifact claim has shipped. `sessions.new-session-attach-cwd` is the first alternate, now rated
  small-medium because both cwd mutations must preserve nested-refusal ordering.
- 2026-08-28: Slice 10u closed `mux.command-group-argument-parse-abort`. Warm local Command-client
  vectors now prepare ordinary unaliased tmux grammar across the complete vector before effects,
  while runtime target and effect errors remain sequential. Only vector-index-0 exact unaliased
  `attach` or `attach-session` retains the private positional parser; later exact spellings and
  aliases use the catalog. Control, remote `--host`, config and source replay, native zz grammar,
  alias snapshots, and runtime rollback remain excluded. Six warm fixture probes and the focused
  three-step scenario report zero differences. The registry now has 88 active groups, 593 active
  items, and 102 closed entries: 47 open, 20 blocked, and 21 accepted, with 123 of 190 groups
  resolved (64.7%). The accepted artifact remains 98 scenarios and 1,522 steps with attached-client
  `PASS` and SHA-256
  `810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`. All 112 CLI tests, the
  all-feature workspace suite excluding `zz-daemon`, strict workspace clippy, formatting, and the
  focused daemon test pass. One parallel daemon run hit two unrelated failures that both pass alone.
  A sequential run passed 711 of 712 tests; its unrelated viewport-queue assertion also passed
  immediately alone. No uninterrupted full-daemon result is claimed here. Slice 10u is delivered,
  and the live tracker must be reranked before selecting the next slice.
- 2026-08-28: Slice 10v closes `tracker.format-vocabulary-registration` with oracle schema 5. The
  source-backed inventory records 31 literal producer scopes, 153 scoped pairs, 108 unique names,
  10 derived families, five propagation records, and 36 modifier tokens. The literal partition is
  58 implemented, 54 native, and 41 active gaps; the derived partition is eight implemented and two
  active gaps; the modifier partition is 30 implemented and six active gaps. Runtime and
  option-consumer fidelity remain under `formats.context-producer-fidelity` (`adopt`, open) and
  `formats.modifier-fidelity` (`adopt`, open). Native typed producers remain accepted under
  `formats.native-typed-context-producers` (`native`, accepted). This is a source-registration
  closure without runtime, context-value, protocol, snapshot, scenario, or artifact changes. The
  tracker has 91 active groups and 595 active items, with 103 closed entries: 49 open, 20 blocked,
  and 22 accepted. Closed history plus accepted groups resolve 125 of 194 groups (64.4%). The
  accepted artifact remains 98 scenarios and 1,522 steps with attached-client `PASS` and SHA-256
  `810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`. The campaign is paused by
  user request after 10v, so no rerank or next slice is selected.
- 2026-08-29: The resumed campaign rerank corrected the stale config parser-abort item into closed
  history, then slice 10w closed `formats.repeat-modifier` locally. `R` now matches the pinned split,
  recursive operand expansion, count bounds, invalid-count behavior, nesting, and post-transform
  order. A deterministic 40,960,000-byte intermediate guard replaces the pin's elapsed-time
  budget. Production status tests prove the default `P:` and `S:` rows consume `R` correctly. The
  modifier partition is 31 implemented tokens and five active gaps: `I`, `L`, `O`, `V`, and `w`.
  The registry has 91 active groups and 598 active items, with 105 closed entries: 49 open, 20
  blocked, and 22 accepted. Closed history plus accepted groups resolve 127 of 196 groups (64.8%).
  The accepted artifact has 98 scenarios and 1,526 steps with attached-client `PASS` and SHA-256
  `f2aa32e0935e8a839c0abcd43da85e0f474d6c191421776847f7a464cc7257ff`. The closure is uncommitted,
  no push is authorized, and a fresh rerank must select any successor.
- 2026-08-29: Slice 10x closed `sessions.new-session-attach-cwd` locally. Existing
  `new-session -A -c` now shares the attach path's one-pass target-context expansion and cwd update,
  including retained mutation after a nonnested headless terminal-open failure. Clientless calls
  remain inert, permitted Control clients attach, and nested Interactive, Control, and `-A -d`
  calls refuse before expansion or mutation. Fresh creation and an `-A` miss retain an empty
  session cwd without sending an empty cwd to the initial pane. The ten-step `new-session-cwd`
  scenario and focused mux and daemon tests cover the contract. The registry has 90 active groups
  and 596 active items, with 106 closed entries: 48 open, 20 blocked, and 22 accepted. Closed
  history plus accepted groups resolve 128 of 196 groups (65.3%). The accepted artifact has 99
  scenarios and 1,536 steps with attached-client `PASS` and SHA-256
  `ed1422d318298b2fee9c31c160393cc2709b9d9137705e96c2632cc700cdcd01`. The closure is uncommitted,
  no push is authorized, and a fresh rerank must select any successor. The rerank must treat `w`
  as a platform-sensitive width contract and place `aliases.config-parse-unit` before
  `mux.chain-parse-abort`.
- 2026-08-29: Slice 10y closed `aliases.config-parse-unit` locally. Each config file now prepares
  its alias expansions under one engine lock after its parse-time environment assignments and
  before replay. Startup roots and top-level matched source batches complete construction before
  their batch replay, while nested sources receive a fresh snapshot when their parent command
  runs. Stored preparation errors keep their source, physical group, and replay position. Their
  Control warning-versus-guard classification is frozen before earlier replayed alias mutations can
  affect it. `source-file -n` keeps its no-effect behavior and suppresses stored alias preparation
  errors. Four focused daemon tests and the clean two-step `smoke/config-alias-parse-unit`
  differential cover the contract. The registry has 89 active groups and 595 active items, with
  107 closed entries: 47 open, 20 blocked, and 22 accepted. Closed history plus accepted groups
  resolve 129 of 196 groups (65.8%). The accepted artifact has 100 scenarios and 1,538 steps with
  attached-client `PASS` and SHA-256
  `8d53288c8050e5c8cf7f19e6c81687f91544877d32ea4de9f7d40ea2934736b7`. The closure is uncommitted,
  no push is authorized, and the post-10y rerank freezes `mux.chain-parse-abort` as slice 10z.
- 2026-08-29: Slice 10z closed `mux.chain-parse-abort` locally. Config and source files now
  construct as file units before effects, including parse-only validation, independent sibling and
  startup units, nested-child isolation, Control warning placement, and verbose alias traces. The
  clean two-step differential raises the accepted artifact to 101 scenarios and 1,540 steps with
  attached-client `PASS` and SHA-256
  `afd1fdf9a79e06f449e8c43abd63b14a2a4968338110223750d4171889c34aaf`.
  The same audit closes `hooks.queue` because pinned `after-queue` is explicit-only. The registry
  has 87 active groups, 593 active items, and 109 closed entries: 45 open, 20 blocked, and 22
  accepted, resolving 131 of 196 groups (66.8%). The closure is uncommitted, no push is authorized,
  and the post-10z rerank freezes `formats.session-runtime/format:session_active` as slice 10aa.
- 2026-08-29: Slice 10aa closed `formats.session-runtime` locally. The three-state
  `FormatClient` backing separates a command's raw invoking client from its selected target client.
  Clientless producers expand `session_active` to empty; an unattached or differently attached
  client expands it to `0`; a client attached to the target session expands it to `1`. Deferred
  output carries the selected state, while fresh session cwd and pane cwd expansion retain their
  independent raw-client rules. Focused mux and daemon tests cover each reachable branch, and the
  28-step `formats-values` row passes. Unit, source-file, `run-shell`, `if-shell`, per-client
  snapshot, and attached-client fixture proofs show that `client_*` facts and `session_active` use
  the same selected client. The 198-name partition now has 94 direct mux
  values, 32 daemon-delegated values, and 72 active gaps. The registry has 86 active groups, 592
  active items, and 110 closed entries: 44 open, 20 blocked, and 22 accepted, resolving 132 of 196
  groups (67.3%). The accepted artifact covers 101 scenarios and 1,550 steps with attached-client
  `PASS` and SHA-256
  `bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`. The closure changes no
  protocol or snapshot field, remains uncommitted, and has no push authorization. The
  post-10aa rerank freezes `format:window_activity` as slice 10ab, pending its
  `formats.window-activity-time` split from `formats.window-runtime`.
- 2026-08-29: Slice 10ab closed `formats.window-activity-time` locally. Windows now keep a
  Unix-second `activity_time` separate from logical MRU ordering. Creation, parsed nonempty pane
  output, and pinned current-window transitions refresh it, while same-window and output-free
  mutations leave it unchanged. The independent audit repaired the direct daemon `switch-client`
  clock refresh. Focused mux and daemon coverage plus the 45-step `formats-values` row prove plain,
  boolean, comparison, list-row, time-modified, target-isolated, and output-driven behavior. The
  198-name partition now has 95 direct mux values, 32 daemon-delegated values, and 71 active gaps.
  The registry has 86 active groups, 591 active items, and 111 closed entries: 44 open, 20 blocked,
  and 22 accepted, resolving 133 of 197 groups (67.5%). The accepted artifact covers 101 scenarios
  and 1,567 steps with attached-client `PASS` and SHA-256
  `309aed0df108abd93e50f2073af7df5991d266c25e55dd266f0c8fc7f412ad72`. The closure changes no
  protocol or snapshot field, remains uncommitted, and has no push authorization. The post-10ab
  rerank freezes slice 10ac on the planned
  `jobs.command-status-environment/semantic:shell-job-clean-environment` split.
- 2026-08-29: Slice 10ac closed `jobs.command-status-environment` locally. Shell-form `run-shell`
  and `if-shell` now start from an empty process environment, apply global then resolved-session
  state, remove hidden and unset values, and preserve visible modeled `TMUX_PANE` without creating
  one. Status `#()` uses the same clean construction with global-only state and a `TMUX` suffix of
  `-1`. Startup jobs preserve modeled TERM-family values; completed startup forces the tmux
  terminal identity, and the private launcher uses modeled PATH. The three-step differential runs
  eight assertions per engine, and the attached fixture proves status scope. Delayed `run-shell`
  timing, `copy-pipe`, popup jobs, and status cwd remain active. The registry has 86 active groups,
  593 active items, and 112 closed entries: 44 open, 20 blocked, and 22 accepted, resolving 134 of
  198 groups (67.7%). The accepted artifact covers 102 scenarios and 1,570 steps with
  attached-client `PASS`, exactly two registered GEO rows, and SHA-256
  `542f7187cb0600c1e28df592c0497aaa90aa8c71c9f07ae3bf76030e54964016`. The complete workspace
  tests and clippy, 729 serialized daemon tests plus two integration passes and one ignored case,
  desktop build, formatting, tracker, summary, and diff checks passed. The closure remains
  uncommitted, and no push is authorized. At that checkpoint, the post-10ac rerank froze slice 10ad on
  `tracker.semantic-coverage/semantic:tracker-option-consumer-registration`, a runtime-neutral
  source registration across 180 pinned options and 75 live option gaps.
- 2026-08-29: Slice 10ad closed
  `tracker.semantic-coverage/semantic:tracker-option-consumer-registration` locally. The unchanged
  105-name roster moved to `command::TMUX_OPTION_CONSUMERS`, `BEHAVES` remains its public alias,
  and an exact guard proves the 180 = 105 consumers + 75 live gaps partition and the closed tracker
  record. `copy-mode-mark-style` names status option-variable consumption only, not visual mark
  rendering. The compatibility gate passes 445 mux tests plus three daemon inventory tests. Full
  workspace tests and clippy, formatting, diff, tracker, and checked-summary checks pass. The source
  move is runtime-neutral, so the accepted artifact remains the 102-scenario, 1,570-step slice 10ac
  run with attached-client `PASS`, exactly two registered GEO rows, and SHA-256
  `542f7187cb0600c1e28df592c0497aaa90aa8c71c9f07ae3bf76030e54964016`. The tracker has 85 active
  groups, 592 active items, and 113 closed entries: 43 open, 20 blocked, and 22 accepted, resolving
  135 of 198 groups (68.2%). The closure remains uncommitted, and no push is authorized. The
  post-10ad rerank freezes slice 10ae on
  `options.option-name-format-coverage/semantic:option-name-format-coverage`: generic lookup
  precedence, selected target scope and inheritance, whole and indexed array access,
  selected-target display, and the active attached status chain across the exact 105-name roster.
  Exhaustive unit coverage plus a focused differential own the proof. Protocol, snapshots, and
  native GUI styling stay unchanged. Projected closure is 84 active groups, 591 items, and 114
  closed records: 42 open, 20 blocked, and 22 accepted, resolving 136 of 198 groups (68.7%). Delayed
  `run-shell` timing is queued next, then startup initial-client cwd.
- 2026-08-29: Slice 10ae closed
  `options.option-name-format-coverage/semantic:option-name-format-coverage` locally. Generic option
  lookup now precedes format-table, command-item, and environment values across the 105-name roster
  and its 13 server, 42 session, 40 window, and 10 pane scopes. Exact names, legacy aliases,
  inheritance, selected and missing targets, attached fallback, active children, `S`, `W`, and `P`
  loops, three array families, direct daemon producers, and detached status refresh use the pinned
  contract. The focused 60-step differential has zero differences, the attached status probe
  passes, and `just compat-check` passes 452 mux tests plus three daemon inventory tests. No
  protocol, wire snapshot, or native GUI styling changed. New job and cwd splits leave 87 active
  groups, 594 items, and 114 closed records: 45 open, 20 blocked, and 22 accepted, resolving 136 of
  201 groups (67.7%). The persisted full artifact covers 103 scenarios and 1,630 steps through
  10ae, with attached-client `PASS`, exactly two registered GEO rows, and SHA-256
  `46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`. Slice 10af is frozen next under
  `jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`;
  startup initial-client cwd follows.
- 2026-08-29: Slice 10af closed
  `jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`
  locally. Shell-form `run-shell` with explicit numeric `-d > 0` retains command, target identity and
  numeric session id, expanded text and numeric arguments, and cwd at scheduling. Child launch reads
  current global state, the live or retained original-session state, `default-terminal`, startup
  TERM state, and cwd existence. Same-name recreation cannot replace the retained original session;
  a target missing at scheduling stays sessionless after a matching session appears. Deterministic
  foreground daemon coverage waits for `active_shell_jobs`. The background three-step differential
  completes twelve checks per engine across live, destroyed and recreated, missing and later-created,
  and startup-crossing cases with no differing channel. The tracker has 86 active groups, 593 items,
  and 115 closed records: 44 open, 20 blocked, and 22 accepted, resolving 137 of 201 groups (68.2%).
  The accepted full artifact covers 103 scenarios and 1,630 steps through slice 10af. The
  attached-client fixture passes, the two registered GEO rows retain their exact tuples, every other
  channel is clean, and the SHA-256 remains
  `46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`. All 734 serialized daemon
  tests and two active agent integrations pass; one long soak remains ignored. The all-feature
  workspace run excluding the daemon and the prior full workspace clippy gate pass. The closure
  remains uncommitted, no push is authorized, and startup initial-client cwd follows.
- 2026-08-29: Slice 10ag closed
  `source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd` locally. Only a
  cold launcher that auto-spawns the daemon passes private `--bootstrap-client-cwd`. Startup gives
  the bounded valid UTF-8 path first priority, carries it through nested relative sources and
  literal metacharacter paths, then clears it on success or error. A direct daemon starts without
  that value, and later runtime sources use the registered client cwd. The isolated differential
  passes exactly on both engines. The full eight-case startup diagnostic reaches a separately
  registered Control exit difference in which zz can drain queued shell-prompt pane output after a
  flags-0 guard. The rerank also registers the higher-priority kill-server response-admission race
  as slice 10ah and moves pane-output discard to 10ai. The tracker has 87 active groups, 594 items,
  and 116 closed records: 45 open, 20 blocked, and 22 accepted, resolving 138 of 203 groups (68.0%).
  Priority is one next, 64 later, and 22 none. Full zz validation passes 653 unit tests plus 113 CLI
  tests. Serialized daemon validation passes 736 unit tests plus two active agent integrations, with
  one soak ignored. The
  all-feature workspace run excluding the daemon, full workspace clippy, formatting, tracker,
  summary, and diff checks pass. The persisted accepted artifact covers 103 scenarios and 1,630
  steps through slice 10ag, with attached-client `PASS`, exactly two approved GEO rows, every other
  channel clean, and SHA-256
  `46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`. Cumulative slices 10w
  through 10ag form the authorized 2026-08-29 checkpoint.

# Related

- [live tmux compatibility gaps](/tmux/gaps.md)
- [tmux CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
- [tmux compatibility philosophy](/tmux/tmux-compat.md)
- [tmux divergence matrix](/tmux/divergences.md)
- [tmux drop-in plan](/designs/tmux-drop-in.md)
- [tmux commands](/tmux/commands.md)
- [key tables](/tmux/key-tables.md)
- [compatibility harness](/playbooks/compat-harness.md)
- [fleet attach](/designs/fleet-attach.md)
- [TUI client](/designs/tui-client.md)
