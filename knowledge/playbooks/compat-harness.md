---
type: Playbook
title: Running the tmux compatibility harness
description: How to run the pinned tmux differential corpus, read topology, geometry, format, and query-stdout results, and record known divergences.
resource: compat/run.sh
tags: [tmux, compatibility, differential-testing, geometry, playbook]
timestamp: 2026-08-26T00:00:00-03:00
last_updated: 2026-08-31
last_updated_by: Codex
---

# Overview

The harness feeds each scenario command to zz and tmux at commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. After each command, it queries both servers
with matching explicit `list-sessions`, `list-windows`, and `list-panes` formats. The
runner compares command exit classes and topology as strict results. It also compares `fmt:` format
queries and generic `out:` command stdout as separate byte-exact strict channels. Geometry
differences fail under `--strict-geometry`, which is how CI runs the harness.

`compat/run.sh` builds `target/debug/zz` with your normal environment before the scenario
runner creates its scratch `HOME` and `XDG_CONFIG_HOME`. The tmux fetcher clones and builds
the pin under `compat/.cache/`. The canonical oracle check accepts `tmux next-3.8` only when the
binary lives at the root of a clean source checkout at the exact pin and its companion build stamp
matches the commit, version, fetch recipe, and binary checksum. `ZZ_COMPAT_TMUX` can select another
cache built by the same fetcher, but a version-matching prebuilt or an unstamped clean checkout
cannot satisfy the oracle.

The cache stays valid while its source HEAD is clean at the pin and its build stamp matches the pin,
version, fetch script, and binary. A mismatch rebuilds tmux before the gate runs; a dirty checkout
is refused rather than attested.

# Tracker and generated report

`compat/tmux-gaps.json` is the sole live TODO and status source. Schema 3 assigns stable IDs to
active gaps, stores the manifest date in `updated_on`, and keeps completed work in `closed`. The
generated [tmux compatibility gap report](/tmux/gaps.md) is the readable view. Do not maintain
counts or open-item rosters in the philosophy, roadmap, divergence matrix, or research snapshots.

Run the fast gate before choosing or landing a compatibility slice:

```sh
just compat-check
```

The recipe calls `compat/check.sh`, which fetches the pinned tmux binary once, validates the oracle
and registry, requires nine named mux manifest tests, then runs the full `zz-mux` library suite.
It also requires three named daemon tests: the hook-producer partition, delegated-format consumer,
and scoped-context registration tests. Each required test runs through `--exact`.
Linux CI runs the same command after restoring the pinned tmux cache. A full
`compat/run.sh` checks the oracle and tracker before executing scenarios.

Oracle schema 5 records 92 commands, 78 aliases, and 572 accepted command-flag shapes: 318
valueless, 246 required-value, and 8 optional-value. Every command also carries its positional
minimum and maximum. The source pass also records 14 commands that use nine custom `args_parse`
callbacks as six effective rules. The remaining inventories contain 180 options, 198 global
format-table names, 31 literal context-producer scopes with 153 scoped pairs and 108 unique names,
10 derived families, five propagation records, 36 modifier tokens, 68 hooks, and 303 default
bindings across `root`, `prefix`, `copy-mode`, `copy-mode-vi`, and `move`. The 198 global names
divide into 99 values resolved directly by the mux, 44 delegated to daemon `StatusHooks`, and 55
constant-backed names, all of which are now carried as accepted `format:` items rather than open
gaps.

The Rust gate reconciles command and alias names, flag arities, positional bounds, custom argument
rules, option names, global formats, the complete schema 5 context and modifier inventories, and
hook names. It also classifies
native commands, native aliases, zz-only flags on tmux command names, and every zz-only default key.
It derives the guarded native-name roster from the catalog minus the pinned oracle and checks every
pinned canonical prefix against the resolver. It pairs every constant-backed format with a manifest
item and tracks every missing default key across all five tmux tables. For each shared default key,
it also reconciles the rendered command and repeat bit or requires a named `binding:` divergence. The
earlier selected context rosters contain 1 `command-item` name, 3 `list-commands` names, and 10
`list-keys` names. zz implements all 14. `formats.command-item-context` closed on 2026-08-24: the
mux dispatch chokepoint carries the canonical entry name into every command it runs, so `#{command}`
expands inside any command item and stays empty outside one. The daemon-preempted half closed under
`formats.daemon-command-item-context`; its immediate format hooks now carry the same canonical name,
and the daemon's post-spawn `new-window`/`split-window -P -F` pass retains it while adding live pane
facts. Delayed subscriptions and prompts stay outside an item.

Slice 10v closes the former selected-context blind spot. The literal partition is 58 implemented
pairs, 54 native pairs, and 41 active gaps. The derived partition is eight implemented families and
two active gaps. At that checkpoint, the modifier partition was 30 implemented tokens and six
active gaps. These are source-registration facts, not runtime or context-value-parity claims.

Local slice 10w closes `formats.repeat-modifier`. `R` splits at the first top-level comma,
recursively expands both operands, accepts counts from 1 through 10,000, and matches the pin's empty
or replacement-failure behavior for invalid, missing, zero, negative, and oversized counts. Escaped
commas, nesting, byte-length, truncation, and post-transform order match. A deterministic
40,960,000-byte intermediate guard rejects nested amplification before allocation, replacing the
pin's elapsed-time budget. Focused mux tests cover the contract, and the daemon row test proves the
default `P:` and `S:` rows consume `R` without exposing literal modifier syntax. The partition is
now 31 implemented tokens and five active gaps: `I`, `L`, `O`, `V`, and `w`.

Local slice 10x closes `sessions.new-session-attach-cwd`. The engine routes an existing
`new-session -A -c` target through the attach cwd path, expands once in the resolved target and
invoking-client context, and stores the result before a nonnested terminal-open preflight. Fresh
creation and an `-A` miss retain an empty session cwd without sending an empty cwd to the initial
pane. Clientless calls remain inert, and nested refusal still precedes expansion and mutation. The
ten-step `new-session-cwd` row provides the pinned differential proof.

Local slice 10y closes `aliases.config-parse-unit`. Each parsed config file prepares all alias
expansions under one engine lock after applying that file's environment assignments and before
replay. Startup roots and top-level matched source batches finish construction before batch replay.
A nested source receives a fresh snapshot when its parent source command runs. Stored preparation
errors retain source, physical-group, and replay-position metadata, including the Control
warning-versus-guard classification selected during construction. `source-file -n` retains its
no-effect behavior and suppresses stored alias preparation errors. The two-step
`smoke/config-alias-parse-unit` row provides the pinned differential proof.

Local slice 10z closes `mux.chain-parse-abort`. Each config or source file applies permitted bare
assignments, expands aliases, and validates every command group before any command from that file
runs. A construction failure preserves earlier bare assignments and drops every command effect from
that file. Parse-only input validates with the environment from before the file and commits no
assignments or commands. Startup roots, top-level matched siblings, and nested children remain
independent file units; one invalid unit does not suppress valid siblings or a parent's later
physical groups. Control emits one located `%config-error` without a failed-command guard and delays
construction warnings until sibling replay finishes. The two-step
`smoke/config-chain-parse-abort` row provides the pinned differential proof.

For the 2026-08-30 `aliases.command-bodies` closure, the mux expands one immutable user-alias layer
into an opaque prepared group. Valid multi-command bodies execute every child, append caller
arguments and client-owned stdin only to the final child, and preserve option boundaries, empty
arguments, typed blocks, and physical source groups. Empty bodies succeed without effects. Control
emits no wrapper guard for the opaque group: each child keeps the originating flags, while an empty
body emits no guard. The daemon preserves ordinary failure, source-yield, structural-hook,
foreground and detached job, and deferred-shutdown ordering across the queue boundary. The focused
eight-step `aliases-multi-body` run reports zero differences in every channel; focused mux, daemon,
Control, CLI, and binary regressions back it. Exact forced-shutdown multi-window `window-unlinked`
hook order is accepted as a permanent divergence under `hooks.shutdown-window-unlinked-order`
(`decision: never`): tmux derives it from retained winlink RB-tree history, while zz retains only
the final index-ordered map. The alias group itself reuses protocol v84's `CommandInvocation` shape.
Closure review advanced v85 for typed callback provenance and daemon-authoritative `Attached`
reconnect state; no alias child-vector field or snapshot field is added.
The persisted aggregate now covers 218 scenarios and 2,644 steps with attached-client `PASS`,
three registered known rows carrying GEO differences, every other channel clean, and SHA-256
`c72aa5e1cd782cf8d2cae4c2d0c6ed62c1e3a7bd4637c0c362439170ba6b13b2`.

The 2026-08-31 Source Replay V3 close keeps each syntax or command diagnostic on its physical
source path and line. Command stderr, Control flags, command-error hooks, later siblings, and
physical command groups retain pinned ownership. Detached `run-shell -bC` shutdown drains kill,
nested-source, hook, and outer callback guards before one final `%exit`; foreground queues keep
their prior exit order. The focused `smoke/source-replay-diagnostics` row passes all 60 steps with
zero TOPO, GEO, FMT, OUT, or WARN differences.

The same checkpoint closes `F-PANE-BORDER-SPANS-V2` in raw zz-tui. The renderer divides a shared
border into adjacent pane spans and applies the active owner only where that pane touches the span.
Directional ownership falls back through top, bottom, left, then right; ordinary split-built
same-side ties choose the lower `PaneId`. The focused `pane-border-span-owner` row passes all ten
steps with zero TOPO, GEO, FMT, OUT, or WARN differences. Protocol, snapshots, and GPUI retain their
prior contracts, and GPUI continues to take pane colors from its theme. Mutable tiled order after
`join-pane`, `swap-pane`, or serialized `select-layout` remains under `F-PANE-BORDER-ZORDER`. The
live registry has 44 active groups holding 457 items, with 2 groups open, none blocked, 42 accepted,
and 172 closed records; only 4 of those items sit in an open group. Two groups remain unresolved;
closed records plus accepted active groups resolve 214 of 216 known groups (99.1%).

The remaining `w` modifier needs a wider proof than the earlier forecast recorded. Pinned
`format_width` handles leading hashes, `#[...]` style spans, malformed markup, controls,
`codepoint-widths[]`, a 162-entry default cache, and the host `wcwidth` policy selected by the
harness build's `--disable-utf8proc`. zz uses `unicode-width` 0.2.2. Keep `w` under the hard later
group until those cases have a fixed contract. Slice 10y closes the replay alias snapshot, and
slice 10z closes file-unit construction. Slice 10aa closes the `session_active` client-context
audit. Slice 10ab closes `formats.window-activity-time/format:window_activity` with a Unix-second
timestamp separate from logical window ordering. Window creation, parsed nonempty pane output, and
pinned current-window transitions refresh it; same-window and output-free mutations do not. The
independent audit repaired the direct daemon `switch-client` path so it refreshes the engine clock
before selection. Slice 10ac closes
`jobs.command-status-environment/semantic:shell-job-clean-environment`. The three-step
`smoke/jobs-command-environment` proof runs eight assertions per engine across shell-form
`run-shell`, shell-form `if-shell`, and status `#()`. It covers clean inherited state, overlay
order, hidden and unset values, explicit target loss, TERM identity, modeled `TMUX_PANE`, and cold
versus completed startup. The attached fixture proves status jobs receive global-only state.
Command-form `-C`, format-condition `-F`, delayed callbacks, `copy-pipe`, and popup jobs remain
outside that fixture. The 2026-08-30 shell-job cwd closure below handles status and shell-form
command working directories separately.

Slice 10ad closes
`tracker.semantic-coverage/semantic:tracker-option-consumer-registration`. The unchanged 105-name
roster now belongs to `command::TMUX_OPTION_CONSUMERS`, while `BEHAVES` remains its public alias.
An exact guard proves that the 180 pinned options equal those 105 consumers plus the 75 live option
gaps, with no overlap, and confirms the tracker closure. `copy-mode-mark-style` records status
option-variable consumption only, not visual mark rendering. The source move changes no runtime
behavior, oracle data, protocol or snapshot field, scenario, or artifact step.

Slice 10ae closes
`options.option-name-format-coverage/semantic:option-name-format-coverage`. The source roster has
105 names across 13 server, 42 session, 40 window, and 10 pane scopes. Generic lookup runs before
the format table, command item, or environment. Exact names and legacy aliases follow selected
targets, inheritance, attached fallback, active children, and `S`, `W`, and `P` loops; command
prefixes do not match. Flags emit `0` or `1`, and other types retain their tmux spelling.

Whole-array and indexed lookup covers `command-alias`, `status-format`, and `update-environment`.
Whole arrays put numeric entries before named entries and apply whole-array local shadowing. Numeric
indices normalize leading zeroes; malformed, missing, and overflowing indices expand empty. Mux
formats read live state. Direct daemon producers use the same live resolver, while detached status
shares one all-scope snapshot across each refresh batch. Missing-target `run-shell -C` and
`if-shell -F` read global options while their inserted work keeps the caller context.

Exhaustive mux and daemon tests cover the roster, scopes, arrays, targets, loops, producer inventory,
and detached refresh. The focused 60-step `option-name-formats` row has zero differential channels,
and the attached status probe passes. Protocol, wire snapshots, and native GUI styling stay outside
the change.

Slice 10af closes
`jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`.
Shell-form `run-shell` with explicit numeric `-d > 0` retains command text, target identity and
numeric session id, expanded text and numeric arguments, and the cwd string at scheduling. Child
launch reads current global state, the live original-session overlay or its retained overlay after
destruction, `default-terminal`, and the startup TERM gate. A target missing at scheduling stays
global-only with `TMUX` id `-1` after a matching session appears. Cwd existence fallback runs when
the child starts.

Foreground daemon coverage waits for `active_shell_jobs` before it mutates the model. The background
three-step fixture completes twelve checks per engine across live, destroyed and recreated, missing
and later-created, and startup-crossing cases. Keep `run-shell -C`, `if-shell`, absent `-d`, `-d 0`,
immediate background ordering, `copy-pipe`, and popup jobs outside 10af. The 2026-08-30 cwd closure
handles cwd selection separately.

The 2026-08-30 `jobs.shell-job-cwd` closure selects shell-form command cwd in pinned order:
`run-shell -c`, startup client cwd, cwd from an unattached provenance client, selected target
session, invoking client's attached session, `HOME`, then `/`. Positive-delay jobs retain the
selection before the timer and check path existence when the child starts. Status `#()` uses the
attached session path. Attached clients keep independent command caches, while unattached query
clients share entries by effective cwd. Ten focused daemon shell-job tests and 32 status tests
pass. The three-step `smoke/jobs-shell-job-cwd` row completes eight checks per
engine with no differing channel. The attached fixture covers 24 real cases across Interactive and
Control clients, `run-shell` and `if-shell`, and valid, missing, and omitted targets. No protocol or
snapshot field changed. The full 105-scenario, 1,675-step strict and attached aggregate passes with
two approved GEO rows, every other channel clean, and SHA-256
`a1e4ca86326006c5f06c77859219772b97fe7e6ac86dd703b127fced4ca0cd7e`.
Slice 10ag closes
`source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd`. Only a cold
launcher that auto-spawns the daemon passes a bounded valid UTF-8 cwd through private
`--bootstrap-client-cwd`. Startup carries it through nested
relative sources and literal metacharacter paths, then clears it before runtime source selection on
success or error. A direct daemon starts without the value. Run the isolated startup-client-cwd
differential from `compat/startup-diagnostics.sh` to prove exact selection on zz and pinned tmux.
The full eight-case script currently reaches the separately registered
`control-mode.exit-pane-output` difference during its first Control exit case: zz may drain queued
shell-prompt `%output` rows after a flags-0 guard, while pinned tmux discards them before `%exit`.

`formats.command-argument-expansion` closed five target-sensitive paths on 2026-08-24. The current
`command-item-format` scenario covers the positional names for `rename-session` and
`rename-window`, optional option names for both show commands, `select-pane -T`, both
`new-session` names, formatted `new-window -n`, literal `break-pane -n`, and shared name cleaning.
Its fixtures use exact non-current targets and cover Unicode, backslash identity, clean-name reuse,
literal format tokens, and the pin's pane format type for both rename commands. Control-byte
rejection and expansion-count assertions stay in focused
Rust tests because this line-oriented fixture cannot carry those values. The
focused run prints the authoritative step count; this playbook does not duplicate that moving
number. `formats.new-session-name-expansion` closed `new-session -s` on 2026-08-25, and
`formats.name-validation-cleaning` then closed the shared `new-session`, `new-window`, rename, and
literal `break-pane` name pipeline. `formats.creation-name-edges` closed the pin's second
`new-window -S` lookup expansion and `break-pane -n` automatic-rename side effect on 2026-08-25.
`formats.buffer-path-expansion` closed both buffer paths the same day; the focused
`buffer-path-format` scenario covers one-pass expansion, format-before-home ordering, canonical,
alias, unique-prefix, and user-alias command identity, and load/save file effects.
`native-prefix-isolation` covers the 25 unique tmux prefixes that native names had changed, plus
exact alias and user `command-alias` precedence. The eight-step `aliases-multi-body` scenario adds
the valid body contract: caller arguments reach only the last command of a multiline alias, stored
binding readback preserves the group, an empty body ignores caller arguments and succeeds, and an
earlier child's `--` boundary survives the opaque round trip. Matched unparsable alias bodies remain
unit-tested at the mux and daemon dispatch seams; they fail loudly as
`unknown command: <typed name>` and never fall through to a shadowed command.
Protocol v74 closes Control's former static unknown-name precheck through focused daemon and CLI
tests. The client prepares the entire initial argv unit or complete LF line before opening execution
frames, observes one daemon alias snapshot for that unit, preserves command numbering and
notifications, and executes the prepared invocation with ordinary read-only authorization. At that
checkpoint these tests did not claim tmux-compatible empty or multi-command bodies. The daemon now
prepares those forms through the same boundary and emits only their normal per-child Control guards,
with no synthetic parent frame. The strict
`smoke/control-alias-prepare` fixture adds pinned proof for one whole-line alias snapshot and a
whole-line preparation error that aborts before either surrounding effect. The strict
`smoke/cli-chain-parse-abort` fixture proves that a local CLI preparation failure aborts before an
earlier mutation while a runtime command failure keeps the earlier effect and prunes the later
command. Its three harness steps now run six warm probes and eleven cold probes on each engine. The
warm set covers an unknown name, an invalid flag, excessive arity, a missing required value, a later
exact `attach`, and a later exact `attach-session`. The cold set covers implemented and parked
command syntax through canonical and alias spellings, exact native attach tails, and `-N` no-spawn
routing.

Against a missing local socket, the CLI validates the complete raw vector without user aliases
before routing, stdin capture, TUI handoff, daemon spawn, startup config, or effects. The pass covers
canonical names, built-in aliases, unique prefixes, flags, arity, callback argument types, and nested
typed blocks for all 83 implemented plus nine parked upstream commands. It parses exact native
attach first and validates the tail, which still executes after attach. Arbitrary user-alias names
cannot trigger autospawn, while startup config may shadow a canonical spelling. A successful raw
pass generation-identifies the spawned daemon, then prepares the full vector under one post-config
alias snapshot before execution. A failed preparation and owner disconnect stop only the
exclusively owned new daemon. Startup reentry cannot claim or contest the lease; another external
client or any command commits it.

Only the unknown-name error shape is pinned here; zz still defines malformed alias-body text and
reports it loudly after valid empty and multi-command bodies closed. Focused binary coverage also
exercises routing-sensitive new-session and attach forms, `-N`, startup shadows, arbitrary startup
aliases, invalid nested alias
bodies, contender and pipeline races, and parked syntax. Slice 10u closes warm ordinary argument
preflight under `mux.command-group-argument-parse-abort`. The daemon applies the existing static
grammar to ordinary invocations with no user-alias match for a registered `ClientKind::Command`.
Callback construction and user-alias validation retain their prior paths, while native zz names
remain runtime-owned. The sole generic-validation bypass covers exact unaliased `attach` and
`attach-session` at vector index zero, where the CLI's private positional-session and
`--restart-daemon` parser owns them. Later exact spellings and every user-alias expansion to either
attach name use the ordinary catalog. Control preparation and framing remain unchanged. Config and
source-file replay stay under the residual `mux.chain-parse-abort`; remote `--host`, replay alias
snapshots, runtime rollback, and native zz grammar remain outside the closure. Slice 10u changes
neither the protocol nor the snapshot schema. Slice 10y later closes the replay alias snapshot;
the other exclusions retain their owners.

Oracle schema 4 closes callback discovery, not callback behavior. The typed Rust sidecar mirrors the
12 implemented callback commands. Protocol v84 adds zero-based lexical command-block positions to
`CommandInvocation`, and `COMMAND_ARGS_PARSE_BEHAVES` contains `bind-key`, `command-prompt`,
`choose-buffer`, `choose-tree`, `confirm-before`, `display-menu`, `display-panes`, `if-shell`,
`run-shell`, `set-hook`, `set-option`, and `set-window-option` after source-file, Control,
stored-command, parser, postcard, mux, and daemon proofs. All 12 implemented callback commands now
apply their pinned rules. The unimplemented `choose-client` and `switch-mode` callbacks need no
`args-parse:` item because their `command:` items cover the whole command.

Hook-producer discovery closed in slice 10l. The daemon-owned invariant names 30 explicit event
producers and derives 37 generic `after-<command>` producers from implemented command names. A
later pin audit classifies `after-queue` as explicit-only: ordinary queues do not produce it, while
`set-hook -R` runs it. The current partition contains those 67 automatic producers and the
explicit-only hook, with no `hook:` gaps left in the registry: `hooks.pane-events`, which held
`pane-focus-in`, `pane-focus-out`, and `pane-set-clipboard`, closed on 2026-09-02.
`just compat-check` requires the exact
`daemon::tests::pinned_hook_producer_partition_matches_the_oracle` and
`status::tests::daemon_delegated_format_consumers_match_mux_inventory` tests. The second test seeds
buffer, client, and session facts, then requires every one of the 44 delegated names to resolve
through the production `DaemonFormatHooks` consumer. At the slice 10s close, the mux manifest test
required the 92 direct, 32 delegated, and 74 tracked sets to stay pairwise disjoint and equal the
198-name pin.

That registration closed source ownership only. It did not claim context-specific value parity,
and all 74 format gaps retained their runtime owners at that checkpoint. The oracle, protocol,
snapshots, scenarios, and accepted compatibility artifact did not change. Slice 10m already closed
the shared-binding
runtime mismatch for bare key-only `bind-key`; downstream command and copy-action behavior retains
its separate owners. Slice 10v closes `tracker.format-vocabulary-registration` with the schema 5
source inventory and disjoint, exhaustive production-owned partitions. New or stale literal,
derived, propagation, or modifier entries fail `just compat-check`.

`formats.context-producer-fidelity` closed on 2026-09-04 with the `set-hook -B` monitor subsystem,
and `formats.modifier-fidelity` closed on 2026-09-02. Native typed producers remain accepted under
`formats.native-typed-context-producers` (`native`, accepted). Slice 10v changes no protocol,
snapshot, differential scenario, or accepted artifact. The resumed rerank corrected the stale
parser-abort ledger item because first-diagnostic whole-file abort was already implemented and
tested. Commit `562b950c` contains slices 10w through 10ag. At that checkpoint, the tracker had 87
active groups with 594 items and 116 closed records: 45 open, 20 blocked, and 22 accepted, for 138
of 203 groups resolved (68.0%). The persisted accepted slice 10ag artifact covers 103
scenarios and 1,630 steps with attached-client `PASS`, exactly two approved GEO rows, every other
channel clean, and SHA-256
`46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`. Slice 10ag closes startup
initial-client cwd. Kill-server response order is next in 10ah, followed by exit pane output in
10ai.
Shared command-flag diagnostics
closed on 2026-08-28 without retaining the earlier partial
daemon roster. The catalog parser covers 83 implemented upstream canonical commands and 74 aliases
through mux execution, daemon preflight, and stored commands. Exact native attach shares the
leading-option diagnostics while keeping its positional-session boundary and extensions. After
option grammar, the common paths validate positional minima and maxima before rejecting a
recognized parked capability. A too-short or too-long command therefore reports the pin's arity
diagnostic even when it also names an unsupported flag. The
`smoke/command-flag-errors` fixture byte-compares 516 probes on each server: 513 failures covering
unknown and invalid flags, help usage, missing required values, and unsupported-before-unknown
ordering, plus three successes proving required-value absorption. It checks pane, buffer, file,
binding, and hook sentinels. Differential scenarios, attached-client fixtures, unit tests, and
manual GUI checks remain the behavioral evidence.
The strict three-step `smoke/args-parse-if-shell` scenario runs 12 internal checks. It distinguishes
typed and quoted branches, covers format and shell conditions, rejects typed conditions, option
values, and extra positionals before effects, preserves a valid stored binding after an invalid
replacement, and exercises both canonical source-file and built-in-alias Control paths. Both sides
publish `ARGS_PARSE_IF_SHELL=clean:12`.
The strict three-step `smoke/args-parse-run-shell` scenario runs 21 internal checks. It distinguishes
typed blocks from quoted brace text, tests leading and late `-C`, combined flags, every string-only
option value, `--` and first-positional boundaries, accepted ignored positionals, background work,
stored replacement preservation, and source-file plus Control alias paths. Both sides publish
`ARGS_PARSE_RUN_SHELL=clean:21`.
The strict three-step `smoke/args-parse-set-option` scenario runs 21 internal checks across
`set-option` and `set-window-option`. It requires positional 1 to accept either a string or typed
block while option names, flag values, and extra positionals stay strings; exact type failures
precede arity, targets, and effects. Recursive printing preserves same-line `;` and physical-line
`;;`, empty blocks become empty values, quoted braces remain literal, and `-F` runs after typed
normalization. Canonical names, built-in aliases, unique prefixes, preexisting user aliases, inner
aliases, `--`, late flags, a real command option, stored bindings, source-file, and direct Control
paths finish `ARGS_PARSE_SET_OPTION=clean:21` on both servers.
The strict three-step `smoke/args-parse-bind-key` scenario runs 17 internal checks. Every positional
accepts strings or typed blocks while `-T` and `-N` values remain strings, with option scanning
ending at the first positional or `--`. It covers exact typed and string tails, empty and variadic
typed tails, aliases, boundary flags, child-failure preservation,
Control routing, bare-key metadata mutation, absent-key table creation, command replacement, and
physical-group execution through a real attached client. Both sides publish
`ARGS_PARSE_BIND_KEY=clean:17`. The fixture does not invoke or claim `send-keys -K` behavior.
The strict three-step `smoke/args-parse-confirm-before` scenario runs 19 internal checks. It covers
the one command positional as a typed block or string, string-only `-c`, `-p`, and `-t` values,
the first-positional and `--` boundaries, canonical readback, aliases, target and child parser
errors, invalid replacement preservation, parent-format expansion, and exact source-file plus
Control channel placement. Every lexical typed block constructs recursively before parent name,
callback type, or arity validation. Recursive paths carry one independent user-alias layer;
alias-produced subtrees disable further user aliases, self-recursion fails unknown without killing
the daemon, and siblings stay independent. Nested `if-shell`, `run-shell`, set-option, and confirm
blocks print canonical names; empty readback is `{  }`, and physical internal group newlines print
as ` ;; `. Exact Control comparisons prove nested bind and confirm failures are preflight parse
errors. Each Control probe waits for that error frame before sending `detach-client`, so immediate
EOF cannot race the expected successful detach status. The typed confirm callback executes its
constructed list without another user-alias lookup;
stored `bind-key` and `set-hook` lists have the same frozen execution boundary. `set-hook` and
command-valued native set-option deliberately construct again. Built-in hook values flatten
physical groups during that second pass, while custom `@` typed values retain textual ` ;; `
groups. A typed ignored `set-hook -R` value still constructs. `display-menu` now walks repeated
NAME, KEY, and ACTION fields, treats an empty NAME as a separator, keeps NAME, KEY, and all ten
valued flag arguments string-only, and accepts strings or typed blocks for ACTION. Typed children
construct before the parent type boundary. Incomplete item groups reach daemon runtime validation.
Runtime resolves the current or `-c` target client before completeness, so an unattached command or
initial Control reports `no current client`; initial Control uses a flag-0 `%error` and exits 1.
Once attached, Control validates an incomplete group as `not enough arguments` before its overlay
no-op and emits a flag-1 `%error`; EOF after that frame exits 1. Interactive menu ordering is
unchanged. Typed `command-prompt` templates retain their structured prepared command list through
submission without re-expanding aliases. String templates substitute raw source before a
fresh parse and complete construction pass against the current alias table. Both paths replace the
first `%%` and every `%1`, with trailing-percent quoting. Typed callbacks retain physical groups,
while string templates and free input form one group. Both sides publish
`ARGS_PARSE_CONFIRM_BEFORE=clean:19`. The scenario proves construction, parsing, readback, and
output channels. Accept, reject, `-y` Enter-default, blocking, and background replies have daemon
and GPUI unit coverage. Raw zz-tui confirmation replies later closed under
`clients.tui-confirm-before-overlay` with focused and attached-client proof. Bounded raw-TUI menu
descriptor consumption later closed under `clients.tui-display-menu-overlay`, and popup
consumption later closed under `clients.tui-display-popup-overlay`. This row does not close eager
whole-file source construction or the broader replay-channel placement difference.

The strict three-step `smoke/args-parse-command-prompt` scenario drives a real attached client and
runs 43 internal checks. It covers zero, typed, string, and empty templates; string-only `-I`,
`-p`, `-t`, and `-T` values; child-before-parent construction errors; canonical readback;
independent recursive alias paths; exact Control frames; frozen typed aliases versus fresh string
aliases; the first `%%`, every `%1`, `%%%`, and `%1%`; structured injection resistance; and typed
versus string physical groups. String templates substitute before parsing, preflight the complete
result before effects, and retain the stored source path and line for failures. Both sides publish
`ARGS_PARSE_COMMAND_PROMPT=clean:43`. Prompt chains and multi-answer `%2`, format and target flags,
labels, key spelling, pass order, vi editing, and freeze behavior retain their existing owners.

The strict three-step `smoke/args-parse-set-hook` scenario runs 24 internal checks. Without `-B`,
only value position 1 accepts a typed block or string; hook names and extra typed positionals remain
strings. With `-B`, every positional lexically accepts either type, while `-B` and `-t` values
remain strings. zz still rejects `-B` because format monitors remain unsupported. The fixture
covers child-before-parent construction, canonical readback, preexisting aliases, same-line and
physical groups, built-in versus custom storage, quoted braces, replacement, empty-value, and
local-inheritance order, `-R`, named-option forwarding, stored bindings, and exact Control frames.
An empty or failing local append creates an empty local array that shadows the inherited global
hook. Both sides publish `ARGS_PARSE_SET_HOOK=clean:24`. Eager whole-file construction, multiline
inner-source placement, monitor semantics, and broader replay placement retain their existing
owners. Slice 10y later closes the same-file alias snapshot.

Regenerate the readable report after changing the manifest:

```sh
python3 compat/tmux-tracker.py write-report
python3 compat/tmux-tracker.py check
```

Use the registry vocabulary consistently:

- `decision` is `adopt` for tmux behavior zz will implement, `native` for a zz presentation or
  ownership choice, `park` for work without current product demand, or `never` for a permanent
  exclusion.
- `status` records product disposition as `open`, `blocked`, or `accepted`. It does not describe
  dependency readiness.
- `depends_on` records delivery order between active gaps. An open gap may depend on another gap,
  while a blocked gap may have no tracked dependency.
- `priority` is `now`, `next`, `later`, or `none`; `ease` is `easy`, `medium`, `hard`, `hardest`, or
  `none`. Accepted items use `none` for both.
- `items` holds normalized upstream, arity, positional-bound, `args-parse`, selected context-format,
  native-extension, semantic, presentation, and protocol identifiers. The source gate reconciles
  structural identifiers where code exposes an inventory. `evidence` points to source, tests, or
  scenarios; `acceptance` states the condition that closes or accepts the gap.
- `updated_on` changes with the manifest. Completed adopt work moves from `gaps` to `closed` with
  the same ID, a closure date, evidence, and a short resolution.

## Coverage freshness

`compat/results/summary.md` is the persisted acceptance artifact. It records 218 scenarios and 2,644
steps against pinned tmux `d77c9dc6`. Every ordinary row is clean, the attached-client
fixture is `PASS`, and exactly three registered `known/` rows carry GEO differences, one of them an
approved OUT difference as well. Its SHA-256 is
`c72aa5e1cd782cf8d2cae4c2d0c6ed62c1e3a7bd4637c0c362439170ba6b13b2`.

Slice 10ag extends `compat/startup-diagnostics.sh` to eight cases. Its startup-client-cwd case builds
distinct initial-client, top-level-config, containing-directory, runtime-client, and glob-decoy
trees. It proves direct and nested startup sources select the launch cwd, then proves a later
runtime source selects the current registered-client cwd. The paths contain spaces, brackets,
stars, and question marks so accidental glob expansion cannot pass. The isolated
startup-client-cwd differential passes exactly on both engines. The complete script reaches
`control-mode.exit-pane-output` when queued shell-prompt pane bytes appear during zz's Control exit
drain. The separately registered difference does not invalidate the accepted slice 10ag corpus.
The rerank also registers `control-mode.kill-server-response-order`: shutdown can close a mailbox
before the successful response is admitted. Slice 10ah must force that old ordering with daemon
synchronization and prove the Control `%end` precedes `%exit`, ordinary Command receives success,
and stalled or disconnected drains remain bounded. Pane-output discard follows as slice 10ai.
Full zz validation passes 653 unit tests plus 113 CLI binary tests. The serialized daemon package
passes 736 unit tests plus two active agent integrations; one soak remains ignored. The full
workspace excluding the daemon, full workspace clippy, and `cargo fmt --check` pass.
Slices 10l and 10m add no differential scenario or step. Slice 10n adds seven confirmation cases and
a pane-input sentinel to the attached fixture. Slice 10o adds bounded menu cases for a visible
title, shortcut precedence, unusable-row skipping, cancel, an unusable PageUp landing with stay-open
Enter, nonactivating paste, and pane-input isolation. Slice 10p adds three bounded popup cases for
live modification, terminal input, dead retention, live focus suppression, dead focus-close, and
pane-input isolation. Slice 10q adds two temporary raw clients on one destroyed session: the
flagged client survives on the newest fallback, while its unflagged peer exits.
Slice 10r keeps `smoke/cli-chain-parse-abort` at three harness steps and adds eleven cold probes on
each engine for the two-pass local autospawn contract. Focused resolver coverage pins exact
raw-row-zero and all-disabled menu behavior. Slice 10s adds no differential row or step. Slice 10t
extends `formats-values` from 13 to 18 steps without adding a scenario: two sessions retain `/tmp`
and lexical `/tmp/..` paths, two targeted displays read each value, and one filtered
`list-sessions` query reads both. Focused mux tests separately cover missing retained or target
state and visibility after the production `attach-session -c` path updates one session. The
`session_active` tri-state producer audit remains under `formats.session-runtime`. At the 10t
checkpoint, `sessions.new-session-attach-cwd` owned two cwd mutation differences: existing-target
`new-session -A -c` skipped the target update, and fresh explicit-empty `-c ''` collapsed to omitted
cwd inheritance. Slice 10x closes both paths.
Slice 10u keeps `smoke/cli-chain-parse-abort` at three harness steps and now runs six warm probes
without changing the scenario count, persisted step count, or attached result. All six finish with
zero TOPO, GEO, FMT, OUT, or WARN differences. Runtime target and effect errors retain the existing
sequential probe: the earlier effect remains and the failed command prunes the later effect.
Slice 10v is a delivered source-registration closure. It adds no canonical scenario or step and
does not change the attached-client result. Slice 10w extends `formats` from 12 to 16 steps without
adding a scenario. Its four probes cover valid and nested repetition, invalid and out-of-range
counts, post-repeat byte-length and truncation, and the missing-comma failure path. The accepted
10w artifact had 98 scenarios and 1,526 steps with attached-client `PASS` and SHA-256
`f2aa32e0935e8a839c0abcd43da85e0f474d6c191421776847f7a464cc7257ff`.
Slice 10x adds the ten-step `new-session-cwd` scenario. It proves one-pass expansion against the
resolved target session, window, pane, and invoking-client context; escaped-hash retention;
source-session isolation; fresh explicit-empty state; and an explicit-empty `-A` miss. Focused mux
and daemon tests cover clientless calls, permitted Control attach, retained cwd after a nonnested
headless terminal-open failure, and nested Interactive, Control, and `-A -d` refusal before
expansion or mutation. The accepted artifact now has 99 scenarios and 1,536 steps with
attached-client `PASS` and SHA-256
`ed1422d318298b2fee9c31c160393cc2709b9d9137705e96c2632cc700cdcd01`.
Slice 10y adds the two-step `smoke/config-alias-parse-unit` scenario. It proves that same-line and
later-line invocations use the alias table captured before replay, top-level matched files finish
construction before batch replay, and a nested source obtains a fresh snapshot when its parent
command runs. Focused daemon tests cover startup-root timing, file environment assignments,
parse-only behavior, deferred preparation errors, source and physical-group retention, and frozen
Control diagnostic classification. At the 10y checkpoint, empty and multi-command alias execution
remained under `aliases.command-bodies`, while eager name, flag, arity, callback, and nested-child
construction, including `source-file -n` validation, remained under `mux.chain-parse-abort`. The accepted
artifact now has 100 scenarios and 1,538 steps with attached-client `PASS` and SHA-256
`8d53288c8050e5c8cf7f19e6c81687f91544877d32ea4de9f7d40ea2934736b7`.
Slice 10z adds the two-step `smoke/config-chain-parse-abort` scenario. It proves whole-file command
construction before effects, parse-only validation against the pre-file environment, independent
top-level sibling and startup units, nested-child isolation, Control warning ordering, verbose
alias traces, and the runtime-error contrast. The accepted artifact now has 101 scenarios and 1,540
steps with attached-client `PASS` and SHA-256
`afd1fdf9a79e06f449e8c43abd63b14a2a4968338110223750d4171889c34aaf`.
The later 2026-08-30 command-body closure adds the focused eight-step `aliases-multi-body` scenario
without changing either historical checkpoint above.

Slice 10aa extends `formats-values` from 26 to 28 steps without adding a scenario. The row proves
that a target-aware `display-message` and `list-keys` receive an attached format client, while
`list-commands`, session, window, and pane list rows plus their filters remain clientless. Focused
mux and daemon tests cover no-client, unattached, same-session, other-session, deferred pane
output, shell, buffer, capture, popup, menu, status, Control, and display-panes producers. Unit,
source-file, `run-shell`, `if-shell`, per-client snapshot, and attached-client fixture proofs show
that `client_*` facts and `session_active` use the same selected client. The 28-step row passes
inside the accepted 101-scenario, 1,550-step artifact with attached-client `PASS` and SHA-256
`bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`.

Slice 10ab extends `formats-values` from 28 to 45 steps without adding a scenario. The row proves
deterministic creation timestamps, target isolation, actual current-window changes, same-window and
output-free no-op paths, and parsed nonempty output. Focused model, command, format, and daemon
tests cover empty context, plain, boolean, comparison, list-row, and time-modified expansion plus
the pinned move-window and swap-window transitions. The independent audit repaired the direct
daemon `switch-client` path by refreshing the injected engine clock before selection. The 45-step
row passes inside the accepted 101-scenario, 1,567-step artifact with attached-client `PASS` and
SHA-256 `309aed0df108abd93e50f2073af7df5991d266c25e55dd266f0c8fc7f412ad72`.

Slice 10ac adds the three-step `smoke/jobs-command-environment` scenario. Its fixture runs eight
internal assertions on each engine for inherited canary removal, global and session precedence,
hidden and unset handling, an explicit missing target, TERM identity, modeled `TMUX_PANE`, and the
startup gate. The attached fixture adds the global-only status proof. Focused daemon tests cover the
same command and status paths. The complete accepted artifact has 102 scenarios and 1,570 steps,
attached-client `PASS`, exactly the two registered GEO rows, and SHA-256
`542f7187cb0600c1e28df592c0497aaa90aa8c71c9f07ae3bf76030e54964016`.

Slice 10ad adds no differential scenario or step. Its source move preserves the 105-name roster and
the `BEHAVES` alias, while the exact inventory guard proves 180 pinned options equal 105 consumers
plus 75 live option gaps and records the tracker item as closed. The compatibility gate passes 445
mux tests plus three daemon inventory tests. Full workspace tests and clippy, formatting, diff,
tracker, and checked-summary checks pass. Because the slice is runtime-neutral, the accepted artifact
remains the 102-scenario, 1,570-step slice 10ac run above.

Slice 10ae adds the focused 60-step `option-name-formats` scenario. It proves the complete 105-name
roster, four option scopes, lookup precedence, exact and legacy names, arrays, selected and missing
targets, attached fallback, `S`, `W`, and `P` loops, direct daemon producers, and detached status
refresh. The row reports zero topology, geometry, format, output, or warning differences. The
attached fixture passes its status option probe. `just compat-check` passes 452 mux tests plus the
three required daemon inventory tests. This slice first produced the 103-scenario, 1,630-step
artifact with attached-client `PASS`, exactly the two registered GEO rows, and SHA-256
`46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`.

Slice 10af keeps `smoke/jobs-command-environment` at three harness steps and expands its fixture from
eight to twelve background checks per engine. It proves live original-session writes, retained state
after destruction and same-name recreation, a missing target that stays sessionless after later
creation, and launch after startup completion. It also freezes formats, numeric arguments, target
identity, and cwd at scheduling while reading global state, original-session state,
`default-terminal`, and the startup TERM gate at launch. The row reports zero topology, geometry,
format, output, or warning differences. Foreground behavior stays in deterministic daemon coverage
that waits for `active_shell_jobs` before mutating state. The destroyed and initially missing target
cases use a four-second launch delay so their separate CLI mutation chains finish before launch-time
sampling. The full 103-scenario, 1,630-step corpus
and attached-client fixture pass on the final 10af runtime. The two registered GEO rows retain their
exact tuples, every other channel is clean, and the accepted SHA-256 remains
`46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`.

The `jobs.shell-job-cwd` front adds the three-step `smoke/jobs-shell-job-cwd` row with eight
checks per engine. Its focused daemon and status tests pass, and the attached fixture passes its
24-case cwd matrix. The full strict and attached aggregate passes at 105 scenarios and 1,675 steps,
replacing the persisted artifact with SHA-256
`a1e4ca86326006c5f06c77859219772b97fe7e6ac86dd703b127fced4ca0cd7e`.

`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The accepted 10af attached-client fixture is
`PASS`. The expanded corpus pins capture routing and ranges, manual window geometry,
join and break placement, pane-local and creation-time environments, empty panes, post-split zoom,
last-pane input gating, buffer rename, source path formatting, and the small accepted-flag cluster.
`list-keys-padding` contributes 46 byte-exact checks for default padding, note selectors, ordering,
positional filtering, `-1` aggregates, stock repeat metadata, and canonical Space spellings;
`smoke/cheap-flags` contributes 22 checks for `new-window -b` and `unbind-key -a/-q`; and
`smoke/kill-filters` contributes 17 contextual `kill-session`/`kill-window`/`kill-pane -a -f`
checks. `smoke/source-file-depth` contributes 4 command-client checks for the 50-invocation source
limit and the refused 51st. `smoke/source-file-diagnostics` contributes 12 checks for parser and
path diagnostics plus replayed runtime failures, continuation, and outer propagation. Its final
check sources the active default config, a loud missing middle path, an after file, and the default
again. It pins rc 1, declared `-v` order, later-file continuation, and final `DAD` state.
`source-file-format` contributes 40 checks for parse-only, target, target-format, quiet miss,
verbose order, and final state. `smoke/source-file-control` contributes 12 focused checks including
Control verbose suppression, replayed runtime error delivery, the three-level root-miss,
middle-miss, leaf-output guard order, the full return-status matrix, queued Return precedence,
immediate hook flags-0 frames, background inserted-command frames, and parser flags-1 plus hook
flags-0 read-error placement and hidden numbering. The source-read check covers multiple matched
read failures before replay, one completion after descendants, raw unframed diagnostics, retained
status, and later-line continuation. Its status coverage includes actual self-detach, nonself and
no-victim detach, alias targeting, sticky background failures, and `%end` before `%exit`; a manual
`detach-client -a` probe also matches the pin. Protocol v81 extends the same 12-check row with direct
and sourced foreground `run-shell` output after an empty flags-1 guard, exact-recipient raw delivery,
same-line continuation, and unchanged Control retval. The row also requires resolved `-t` and
ordinary `run-shell -b` output to stay off raw Control. It excludes pane-view notifications because
tmux enters a shared pane view while zz opens its native per-Interactive command-output view and
emits no `%pane-mode-changed`. The pinned foreground-disconnect server crash is an intentional
non-parity and stays outside the scenario. `resize-directions`
contributes 16 checks for bare direction flags with
the default amount 1, attached amounts such as `-L2`, separated amounts such as `-L 2`, and the
existing absolute resize forms. `formats-values` also proves explicit startup `config_files` and
the selected session's retained UTF-8 `session_path`; both servers start with `-f /dev/null` so the
config fact is symmetric. `native-prefix-isolation` contributes
29 steps: 28 byte-exact command-name queries plus one alias setup, without plugin-corpus
dependencies.
`smoke/daemon-invalid-flags` contributes three checks: it first removes any inherited sentinel,
then proves representative daemon-dispatched flags reject before callbacks or buffer mutation, and
finally requires the fixture to publish its clean marker.
`smoke/positional-maximums` contributes three checks: it clears inherited state, then requires exact
canonical maximum errors for all 71 generic-CLI-routed finite commands and 62 aliases,
and finally requires unchanged pane, buffer, and file state. The exhaustive daemon test also covers
the exact attach engine path, which the native CLI intentionally extends with a positional session.
`smoke/positional-minimums` contributes three checks: it clears inherited state, then requires exact
canonical minimum errors for all fourteen commands and aliases before missing-target resolution,
and finally requires unchanged pane, buffer, and file state. Focused daemon tests separately prove
that rejected commands do not change menu, confirmation, or wait state.
`smoke/args-parse-if-shell` contributes three harness steps around 12 internal checks for lexical
branch types, exact rejection and output channels, foreground execution, Control transport, and
plain stored-binding validation.
`smoke/args-parse-run-shell` contributes three harness steps around 21 internal checks for lexical
command mode, option and positional boundaries, exact rejection and output channels, foreground
and background execution, aliases, Control transport, and stored-command preservation.
`smoke/args-parse-set-option` contributes three harness steps around 21 internal checks for typed
option values, exact rejection and state preservation, recursive command printing, format order,
aliases, source-file, and direct Control behavior across both set-option commands.
`smoke/args-parse-bind-key` contributes three harness steps around 17 internal checks for typed
option and key positions, exact typed and string tails, aliases, flag boundaries, child rejection,
Control routing, bare-key metadata mutation, and physical-group execution through a real attached
client.
`smoke/args-parse-confirm-before` contributes three harness steps around 19 internal checks for
recursive typed and string construction, string-only option values, nested canonical readback,
per-path alias budgets, self-recursion safety, physical groups, invalid replacement preservation,
and exact source-file plus Control diagnostics.
`smoke/args-parse-command-prompt` contributes three harness steps around 43 internal checks for
template types, recursive construction precedence, alias timing, placeholder substitution,
injection resistance, physical groups, source-file diagnostics, exact Control framing, and real
attached prompt submission.
`smoke/args-parse-set-hook` contributes three harness steps around 24 internal checks for lexical
types, child-before-parent construction, aliases, group normalization, built-in and custom storage,
replacement, empty-value, and local-inheritance order, `-R`, named-option forwarding, stored
bindings, and exact Control framing.
`smoke/args-parse-display-menu` contributes three harness steps around 34 internal checks for the
data-dependent NAME, KEY, and ACTION state, empty-name separators, typed and quoted actions, all
ten string-only valued flags, child construction precedence, canonical and alias readback, invalid
binding preservation, client-before-completeness precedence, incomplete runtime groups,
source-file diagnostics, and exact initial flag-0 plus attached flag-1 Control framing. A
PID-unique FIFO holds the attached command stream through the error frame and proves exit 1 after
EOF. Both servers finish `ARGS_PARSE_DISPLAY_MENU=clean:34` with zero differences.
`smoke/args-parse-display-panes` contributes three harness steps around 22 internal checks for its
optional string-or-typed template, string-only `-d` and `-t` values, child-before-option-type and
arity validation, canonical and alias readback, targetless client routing before duration,
source-file, and direct Command-client runtime behavior. A Command client with an attached
Interactive client resolves to it; a truly
clientless path reports `no current client`. Both servers finish `ARGS_PARSE_DISPLAY_PANES=clean:22`
with zero TOPO, GEO, FMT, OUT, or WARN differences. The fixture closes parsing only: mux runtime
still rejects a positional selection template instead of substituting the selected `%pane` for
`%%%` and executing with the original queue state. Tmux uses `select-pane -t "%%%"` when the
template is omitted. This stays tracked under `display-panes.command-template`; queue blocking and
presentation remain separate.
The strict three-step `smoke/args-parse-choosers` scenario runs 26 internal checks across
`choose-buffer` and `choose-tree`. Both commands accept zero or one string-or-typed template while
`-F`, `-f`, `-K`, `-O`, and `-t` values stay strings. Typed children construct before parent type,
arity, target, or effects. Typed templates freeze their constructed aliases before the chooser
opens; string templates parse against the current alias table after selection. The daemon closes
the chooser, substitutes the exact selected buffer or tree target, and executes against the
invoking client's live context. The fixture covers placeholder quoting, stale and empty buffer
behavior, uppercase attached-client errors, and direct plus stored arity precedence over
recognized parked flags. Both sides publish `ARGS_PARSE_CHOOSERS=clean:26` with zero TOPO, GEO,
FMT, OUT, or WARN differences. Broader chooser flags, presentation, eager whole-source
construction, generic alias recursion, and raw-TUI overlay parity keep their existing owners.
Slice 10y later closes the same-file alias snapshot.
Typed `if-shell`, `run-shell`, and structured `command-prompt` callbacks stop the failed physical
group and continue later physical lines, while string callbacks stay one group. Structured prompt
substitution preserves leaf-argument boundaries against quote or semicolon injection. Raw string
templates substitute before parsing and whole-result construction. Both paths replace the first
`%%` and every `%1`, with trailing-percent quoting. Typed `display-menu` actions retain canonical
child printing in stored bindings, then lose their structural wrapper before the fresh selection
parse; quoted brace strings stay literal. Raw zz-tui now consumes the daemon-published descriptor
and uses the shared keyboard resolver for the bounded attached cases. The fixture does not claim
daemon-side geometry construction, mouse `-M` policy, full shortcut grammar or display, live style
or resize refresh, Interactive queue ordering, selected-action target or error ordering,
close-mid-paste ordering, eager whole-source construction, or generic alias recursion. Slice 10y
later closes the same-file alias snapshot.
Built-in hook values flatten typed physical groups during their second construction pass. Custom
`@` values keep normalized textual groups, and typed ignored `set-hook -R` values still construct.
Prompt chaining and multi-answer `%2` remain under the prompt-fidelity owner.

The checked-in summary includes the current focused counts: `smoke/source-file-diagnostics`,
`source-file-format`, and `smoke/source-file-control` contain 12, 40, and 12 steps,
`smoke/source-replay-diagnostics` contains 60 steps, and `pane-border-span-owner` contains 10.
`resize-directions` and `formats` contain 16, `formats-values` contains 45, and `new-session-cwd`
contains 10. `smoke/config-alias-parse-unit` and `smoke/config-chain-parse-abort` contain 2 each,
`smoke/jobs-command-environment` contains 3, `aliases-multi-body` contains 8, and
`option-name-formats` contains 60.
The summary SHA-256 is
`fdb38caafb85b65b4649b88198231371815c3738741511862bff9a50cb49bcec`.
The historical 10r and 10s checkpoints remain 98 scenarios and 1,517 steps at SHA-256
`9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.
The combined chooser row contributes three harness steps and 26 internal checks with zero TOPO,
GEO, FMT, OUT, or WARN differences to the complete differential and attached artifact.
The historical 10i checkpoint remains 97 scenarios and 1,514 steps at SHA-256
`3b728eb8f0d30cae1bf1fe9c09100188279aaf8c80c0b33b30cd15b617f75d70`; its display-panes row
contributed three harness steps and 22 internal checks with the same clean channels.
The historical 10h checkpoint remains 96 scenarios and 1,511 steps at SHA-256
`75aee7176d3ed3cf1886d4f4c697062089b87644036e85f0230f355fac7d4217`.
The historical 10g checkpoint remains 95 scenarios and 1,508 steps at SHA-256
`15385526cd2098f35276c27cd8edfef338569cd6a6c87ffe80d8f919701f042a`.
The historical 10f checkpoint remains 94 scenarios and 1,505 steps at SHA-256
`31b03805b5701aff0555ebe4d4b40a0116b8525130d4d3406963e9a1c8f1919c`.
The historical 10e checkpoint remains 93 scenarios and 1,502 steps at SHA-256
`e0783568fc5845eaaa9ff4b84256d43a046ced996fbf8b664bc65d9bf0d9578a`.
The historical 10d checkpoint remains 92 scenarios and 1,499 steps at SHA-256
`afea2249cd62402fe00dc8c54ea60662eb616ef584806f4774cd77723746144e`.

`compat/run.sh --check-summary` compares the exact current scenario paths, static step counts, and
all seven stored row cells against the ordinary clean tuple or each registered known tuple. It also
requires its persisted attached-client status to be `PASS`. The check passes for the 2026-08-31
accepted checkpoint and exits before building or running either server. Linux CI first asserts that
`compat/results/summary.md` is tracked, then runs
the inventory and result check after checkout. A named partial run, a headless-only full run, a failed
run, or a run with a SKIP cannot overwrite the canonical report. After a complete strict run with
`--attached-client`, CI diffs the full tracked summary, so changes to Steps, TOPO, GEO, FMT, OUT,
WARN, or the attached proof fail the job. Per-scenario logs remain ignored scratch data, while the
canonical summary stays versionable.

`smoke/config-grammar` intentionally expects the invalid-octal `%config-error` from tmux only. The
nested zz control client still does not publish that diagnostic; the state readbacks separately
prove that both parsers abort the file at the same point. The 2026-08-29 ledger correction records
that already-shipped behavior under closed `config.parser-abort`; slice 10z closes file-unit command
construction. `config.parser-edge-cases` retains post-closing-quote expansion and passwd-backed bare
or named-user tilde lookup. Pinned tmux prefers a nonempty server-global `HOME`, then the current
user's passwd entry, and reports a located syntax error only when the required lookup fails.

# Running the corpus

Run the strict corpus and attached-client contract from the repository root:

```sh
just compat --strict-geometry --attached-client
```

`compat/run.sh` without flags remains the non-strict headless-only form. It prints the temporary
report but leaves the canonical combined summary unchanged.

Pass scenario names to run a subset. Names may include or omit `.txt`.

```sh
compat/run.sh windows panes
compat/run.sh known/known-geometry-gap.txt
```

Check whether the persisted inventory and attached proof are current:

```sh
compat/run.sh --check-summary
```

## Startup diagnostic differential

`compat/startup-diagnostics.sh` is a separate eight-case gate for clientless startup causes. Run it
after building the debug binary and fetching the pinned oracle:

```sh
cargo build -p zz --bin zz
compat/startup-diagnostics.sh target/debug/zz compat/.cache/tmux-src/tmux
```

The script requires all eight cases: initial Control cold start; detached launch followed by late
Control attach; startup list-output discard; explicit-root failure ordering; multiline cause
prefixing and completion-line location; daemon-restart redelivery; startup initial-client cwd; and
Interactive delivery with a global drain. It compares normalized Control transcripts, checks
detached streams and status, and drives the attached Interactive view through real outer PTYs.

The oracle must be the checkout-root `tmux` executable from a clean checkout at exact commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, report `tmux next-3.8`, and match the build stamp's
commit, version, fetch-script checksum, and binary checksum. The probe requires GNU `timeout`, wraps
commands in real 15-second deadlines, uses 500 ms bounded polls, and stops readiness loops after 10
seconds. A missing case or any skip fails the run.

The final debug run passes all eight cases with no skips. This focused script does not call
`compat/run.sh` or regenerate the current `compat/results/summary.md`.

Run the real attached-client fixture separately after building zz and fetching the pin when
debugging it in isolation:

```sh
compat/attached-client.sh target/debug/zz compat/.cache/tmux-src/tmux
```

Pinned tmux owns two isolated outer PTYs and drives an inner zz attach beside an inner tmux attach.
The fixture polls semantic state rather than comparing native presentation. It covers readiness,
root/prefix/prefix2 bindings, copy-mode entry/exit, prompt-driven window rename, choose-tree row
keys, choose-buffer paste/deletion, exact nested-attach refusal, and the attached status message
for a refused 51st `source-file` invocation. It also checks that `list-keys -1` shows a timed status
without replacing the terminal with command output; the short result marker comes from the binding
note and does not appear in the typed prompt. The local Control probe runs `-C` from each outer PTY,
requires existing-session refusal for `attach-session` and `new-session -A`, permits a fresh `-A`
miss, and pipes stdin through a final attach to prove a nonterminal stdin does not publish tty
identity. The daemon unit matrix covers `new-session -Ad`; the attached fixture does not. The
shell-job cwd probe keeps the command, attached-session, and target-session paths distinct. For
each engine it runs Interactive and Control clients through `run-shell` and `if-shell` with a valid,
missing, or omitted target. The resulting 24 real cases require the target session cwd for a valid
target and the attached session cwd for missing or omitted targets. The
command-output probe builds a 96-line local transcript and runs on both sides. It checks line and
page movement, vi Escape selection clearing without exit, search cancel, search editing and submit,
`n`/`N`, selection-to-paste-buffer, a live custom `copy-mode-vi` binding, a live switch to the emacs
table, vi `q` cancel, and emacs Escape cancel. It verifies the created paste buffer contains the
selected match and then removes it. The current full fixture passed for zz and pinned tmux after
independent review of the fresh-session marker.

The confirmation probe runs seven fresh `confirm-before` cases on each server: default-key accept,
Meta plus default-key accept, default-key reject, custom uppercase-key lowercase rejection, custom
uppercase-key acceptance, default-no Enter rejection, and `-y` Enter acceptance. Each case blocks behind a one-byte read in
the underlying pane. The expected reply controls whether the callback writes its marker, and a
final sentinel byte releases the pane while proving the response never reached terminal input.

The menu probe opens a titled short menu on each server. It proves that `q` activates its unique
shortcut before the ordinary `q` cancel rule, Down skips a separator and a disabled row before
Enter selects the enabled row, and Escape cancels without mutation. A `-O` case uses PageUp to land
on an unusable row, keeps the menu open after Enter, then navigates and selects. A
nonactivating paste is consumed while the menu remains visible. This attached case proves an
unusable landing, while focused resolver coverage pins exact raw row zero and all-disabled boundary
behavior. Each ordinary input case blocks an underlying pane behind a one-byte sentinel, proving
those bytes do not reach the pane. This closes
raw-TUI consumption of the daemon-published descriptor and shared keyboard ownership for those
cases. It does not cover daemon-side geometry construction, mouse `-M`, complete shortcut grammar
or display, live style or resize refresh, Interactive queue ordering, selected-action target or
error ordering, or close-mid-paste ordering. The raw renderer places menus after chooser and
command-output bases.

The popup probe runs three cases against each attached client. Case A opens a bordered `-E` popup,
modifies its title while requesting different geometry and a replacement command, then requires the
original terminal body, job, and complete captured frame to remain before a scratch marker proves
`q` reached that popup. Case B requires bracketed paste, one physical SGR left-button press/release pair, and one
tracked wheel event to arrive at exact content-relative cell `3,3`; outer cursor coordinates keep
the proof independent from native sidebar geometry and locale width rules. Case C retains a dead
`-k` popup until a key closes it. Pinned tmux emits three internal underlay FocusOut/FocusIn pairs
around those lifecycles, while zz emits none. Cases A and B enable application focus reporting;
explicit external OUT/IN probes are swallowed by both live overlays, making `q` and bracketed paste
the next application bytes while the underlay stays isolated. Case C uses FocusOut to close the
dead `-k` popup. Unicode and C-locale ACS borders share the same full-frame proof, and the complete
fixture passes under `LC_ALL=C`. A final `z` must be the
underlay's only ordinary byte, reported as decimal 122. The closure covers raw-TUI rendering, state, cleanup, and
bounded input ownership. Live resize, style refresh, context menu, border drag, popup-to-pane,
popup Kitty images, and real mouse or status format facts remain outside it.

The alert-lifecycle probe uses fresh non-current monitored windows. It replaces a 1,500 ms sticky
message with a 5,000 ms Bell alert, writes new terminal output behind it, and proves the current
screen stays frozen for 1.8 seconds across the old deadline. One elapsed endpoint capture requires
the alert to remain visible while the terminal marker remains hidden, so capture-pane process cost
cannot stretch a poll-count clock past the alert expiry. F12 plus Enter then proves one key
dismisses the alert, reaches the pane, and releases the latest viewport well before the alert's own
expiry. The alert window remains unvisited with `#{window_bell_flag}` equal to 1. The probe rings
that same pane again, sees a second Bell message, and repeats the 1.8-second freeze and dismissal
proof while the flag remains set. It then waits 5.2 seconds for the pin's stale positive timer to
drain, changes `display-time` to zero, and repeats the hidden-output and input-release check on
another fresh window. Match the stable `Bell in window` prefix: at 80 columns, the TUI status
surface can truncate the trailing index beside its detach hint. The probe covers ordinary
incremental TTY freeze. A forced structural redraw may expose the latest parsed state.

This focused proof does not cover command-output
mouse behavior, OS clipboard delivery, ordinary TUI pane copy-search editing, SSH transport, pixel
comparison, or the 29 unsupported window-copy actions. It does not update the canonical summary on
its own. The 2026-08-28 strict-plus-attached run persisted this fixture as `PASS`.
Failure output includes both
outer screens and zz daemon stderr; cleanup removes outer servers before inner servers.
`--attached-client` runs it after the headless scenarios and includes it in the overall exit status
without adding a fake row or step count to the canonical summary. Its `PASS` status is persisted
below the scenario rows. A fixture failure or an omitted fixture prevents that full run from
replacing the prior combined summary.

Geometry differences do not change the default exit status. Use strict mode when you want
them to fail the run:

```sh
compat/run.sh --strict-geometry
```

Strict mode is the CI contract: the Linux workflow leg runs `compat/run.sh
--strict-geometry --attached-client`, so every scenario outside `known/` must stay TOPO-clean and GEO-clean
against the pin. Since the cell-authoritative layout landed, a headless zz window is born
at tmux's 80x24 and every layout operation runs the pin's integer arithmetic, which is what
makes exact-geometry diffing possible.

FMT and OUT differences fail in both modes. `--strict-geometry` changes only GEO handling.

Smoke scenarios under `compat/scenarios/smoke/` are part of the default corpus. Each declares
`corpus: none` or `corpus: required`; placement controls smoke-mode byte-exact stdout/stderr checks,
while this metadata alone controls plugin acquisition and offline eligibility. When the pinned
plugin cache is absent or cannot be fetched, the run executes corpus-independent smoke scenarios
and prints a visible SKIP for each plugin-dependent scenario. A skipped smoke is never reported as
a pass. Any SKIP makes the run exit nonzero, discards its temporary report, and leaves the last
complete canonical summary unchanged.

# Reading results

The combined full runner writes `compat/results/summary.md` only after the attached-client fixture
passes. Each row gives the number of executed steps, TOPO, FMT, OUT, and WARN status, plus the number
of steps that produced a GEO difference. The final section preserves the attached-client `PASS`.

Open `compat/results/<scenario>.log` for the command status and per-step unified diffs:

- `COMMAND EXIT-CLASS` compares success with failure. Matching nonzero exits pass because
  both servers refused the command.
- `TOPO` compares session/window counts, names, active indexes, and pane indexes. Any
  difference fails a normal scenario.
- `GEO` compares window and pane cell dimensions plus each window's complete raw
  `#{window_layout}` string, including its checksum and leaf pane ids. Zero-based boot allocation
  now aligns the two sides, so this catches pane assignment permutations as well as structure and
  geometry. The runner reports these differences by default and fails them under
  `--strict-geometry`.
- `FMT` compares stdout from a shared `fmt:` line byte for byte. Both `display-message -p`
  invocations must exit zero. A matching error still fails the FMT step.
- `OUT` compares stdout from any shared query command prefixed with `out:` byte for byte. Both
  commands must exit zero; matching failures still fail the OUT step.
- `WARN` is the smoke-only config channel. It checks each side's expected `%config-error` lines
  and independently checks whether the `source-file` control block ended with `%end` or `%error`.
  The pin does not emit `%config-error` for every execution-time config failure, so both signals
  are required.

The log captures each step's stdout and stderr. In normal scenarios the runner ignores stdout for
ordinary command lines; `fmt:` and `out:` lines enter their respective comparisons. Smoke scenarios
also compare ordinary command stdout byte for byte.

The runner starts zz on a short `/tmp/zzc-<pid>.sock` path and starts tmux with
`-L zzc-<pid> -f /dev/null`. Its exit trap stops both servers and removes both socket files.

The headless scenario rows do not prove copy mode, choose-tree, choose-buffer, command-prompt,
default prefix behavior, packaged launcher attach, or native GUI rendering. The combined strict run
adds the attached-client proof. On macOS, build and exercise the real app launcher separately:

```sh
just build mac
compat/packaged-cli.sh dist/zz/zz.app
```

That fixture verifies CEF resources and the bundle signature, clones the whole app under a path
containing spaces, and drives bare/new/attach against empty and existing daemons. Its PTY cases also
pin detached `new-session -x`/`-y` geometry, attached client dimensions, read-only input rejection
and output visibility, native detach, and `attach -d` peer eviction. The detach paths require exit
status zero plus `[detached (from session NAME)]`; the read-only path processes a later copy-mode
transition before checking that earlier typed input never reached the pane, avoiding a sleep-based
negative assertion. It does not install or notarize the app. The macOS CI leg runs it after producing
`target/cef-bundle/zz.app`. A local run proves the bundle currently in `dist/`; rebuild at the repo
root with `just build mac` after production changes before treating it as fresh evidence. Native GUI
rendering still needs visual smoke evidence; a clean headless summary must not be used as evidence
for that surface.

# Adding a scenario

Add a `.txt` file under `compat/scenarios/`. Keep each scenario focused on one behavior or one
stateful command family, and split it when independent setup or assertions could fail for unrelated
reasons. Long scenarios are appropriate when later assertions genuinely depend on the earlier
state. Put one tmux command on each line; the runner skips blank lines and lines beginning with `#`.
Use commands and flags that both command catalogs support, and target panes by index rather than by
raw `%N` IDs.

The runner handles shell quoting for command lines and rejects `$`, backtick, `;`, `&`, `|`, `<`,
and `>` before parsing them. Prefix a command with `zz-only:` or `tmux-only:` when a scenario needs
side-specific setup. A side-prefixed line skips the exit-class comparison for that step, but the
query trio still runs afterward.

Use `fmt: <format>` for a shared format assertion. The runner passes the payload as one argv value
to `display-message -p` on each side, without `eval`. This path accepts `#{}`, `?`, commas, colons,
semicolons, comparison and logic operators, and `/` delimiters. It rejects an empty payload, `$`,
backticks, either quote character, and `#(`. The `#(` guard prevents a tmux format from starting a
shell command during the differential run.

Use `out: <command...>` for a shared query whose own stdout is the assertion, such as
`out: show-options -gv @plugin`. It uses the same no-eval guards as `fmt:` and splits the payload
into one argv entry per whitespace-delimited token, so quotes, `$`, backticks, and `#(` are rejected.
Put values requiring spaces into an earlier ordinary setup command, then query them by name.

After each line, the harness runs the query trio. Scenario files should contain state changes plus
explicit `fmt:` or `out:` assertions, not ordinary `list-*` assertions whose stdout is ignored.

## Registering a discovered gap

Register a gap before implementing it:

1. Reproduce the behavior against the fetched pinned binary and identify the upstream command,
   option, format, hook, key, presentation rule, or model that owns it.
2. Add one stable ID to `compat/tmux-gaps.json`. Follow the existing entry shape and record the
   decision, status, priority and ease, owning subsystem, affected workflow, `depends_on` ordering,
   source evidence, and acceptance evidence. Keep the ID when status changes and update
   `updated_on` with the manifest.
3. Add the smallest failing test or differential scenario that proves the observation. Use a
   `known/` scenario only for an accepted exact mismatch. Its first metadata comment must be
   `# gap: <stable-gap-id>`, and the registry entry must declare the expected
   `TOPO GEO FMT OUT WARN` tuple.
4. Run `just compat-check`. Fix unclassified structural gaps, stale manifest entries, broken
   evidence, and tuple mismatches before changing behavior.
5. Implement the slice and run its focused evidence. Run the full strict corpus when the change can
   affect shared command, topology, geometry, format, output, config, or attached-client behavior.
6. If the implementation closes an adopt gap, pass its acceptance checks, then move the ID from
   `gaps` to `closed`. Record its title, `closed_on`, evidence, and resolution. If work remains,
   update the same active ID and its evidence. Regenerate `knowledge/tmux/gaps.md`, then run
   `just compat-check` again.

When the user resumes the campaign, use the generated report to rerank and choose a slice. The
roadmap supplies dependency order, and the divergence matrix supplies detailed rationale; neither
owns live status.

## Adding a smoke scenario

Add smoke configs and fixtures under `compat/scenarios/smoke/`. The smoke class boots both daemons
with a scratch HOME and prepends a generated executable `tmux` wrapper to PATH. The pin wrapper
executes the reference binary with `-L <label>`; the zz wrapper executes `zz --socket <path>`.
This makes literal `tmux` calls inside plugins hit the intended server on both sides.

The smoke directives are:

- `corpus: none` marks a self-contained smoke; `corpus: required` permits fixtures to use the eight
  pinned plugin checkouts. Every smoke scenario declares exactly one of these values.
- `conf: <path>` stages and sources a config after linking cached plugins into
  `~/.tmux/plugins/<name>`. A `~/`-prefixed path resolves against the scratch HOME, so a
  corpus file can be staged verbatim (`conf: ~/.tmux/plugins/oh-my-tmux/.tmux.conf`) —
  needed when a config locates itself as `~/.tmux.conf`, as Oh My Tmux does.
- `stage: <source> <destination>` copies one file into the scratch HOME before sourcing
  (both paths accept the same `~/` resolution; the destination must be under `~/`). Oh My
  Tmux uses it for its stock `.tmux.conf.local`.
- `expect-warn: zz <text>` and `expect-warn: tmux <text>` pin each side's
  `%config-error` set. Do not cross-diff skip summaries: they intentionally have no pin analogue.
  The harness separately requires the current tier-1 config loads to finish with `%end` on both
  sides and fails if either source-file block ends with `%error`.
- `keys: <table> <key>` compares only that binding through
  `list-keys -F '#{key_table}|#{key_string}|#{key_repeat}|#{key_command}'`. Stock tables differ,
  so whole-table comparison is invalid.
- Existing `out:` and `fmt:` directives remain available for option, environment, and format
  readback.

Capture stdout and stderr separately for every smoke command. Merging them with `2>&1` introduces
pipe-buffering order artifacts. The harness exports `ZZ_SMOKE_CANARY` into both daemon environments;
scenarios must never read it, which keeps the known clean-environment divergence from becoming an
implicit dependency.

Traps that produce false divergences:

- Every `new-window` needs `-n <name>`. Default window names are process-derived in tmux —
  and refreshed by the `automatic-rename` timer roughly 500ms later — but index-derived in zz.
  The runner's prologue renames window 0 to `main` on both sides for the same reason.
- Never kill scenario session `w`. The post-step TOPO, GEO, FMT, and OUT probes target `w`, so
  removing it turns every later probe into a fixture failure. Both sides create `w` explicitly;
  there is no auto-created session to remove.
- Never put `#{buffer_full}` in a differential scenario. `display-message -p
  '#{buffer_full}'` crashes the pinned tmux server; this is a verified pin trap, not a zz failure.
- `display-message` gets only tmux's newest automatic paste buffer. A named-only `set-buffer -b`
  setup therefore makes every `buffer_*` value empty on the pin. Add an automatic buffer for a
  `fmt:` probe; use `list-buffers -F` when the named row itself is what needs testing.

## Known divergences

Put a scenario with an accepted strict mismatch under `compat/scenarios/known/`. The runner
still executes every step and writes its diffs. It accepts the result only when the scenario's gap
ID resolves to the exact registered `TOPO GEO FMT OUT WARN` tuple. An unregistered known scenario,
a missing tuple, or any tuple drift fails the run.
The three current entries pin two deliberate refusals of upstream layout bugs plus one native
presentation choice: `known-main-preset-two-panes.txt` (the pin never sizes the lone "other" pane),
`known-spread-mixed.txt` (the pin's `-E` corrupts a parent mixing leaf and node children), and
`known-pane-scrollbar-columns.txt` (the pin builds its pane scrollbar from grid cells, so it costs
each pane a column, while zz draws one in client chrome outside the cell grid). They use
`layout.main-horizontal-upstream-bug`, `layout.spread-mixed-upstream-bug`, and
`options.native-pane-scrollbars`.

Inspect a registered tuple directly with:

```sh
python3 compat/tmux-tracker.py known-tuple known/known-main-preset-two-panes.txt
```

Keep the `known/` set narrow. Move a scenario into the normal corpus when zz closes the gap. The
tracker rejects known-scenario evidence that does not match its registry entry.

`aggressive-resize.txt` covers stored option readback only. The harness has one short-lived CLI
client per side, so multi-client viewer selection belongs to daemon and convergence tests rather
than this corpus.

# Key files

| File | Role |
| --- | --- |
| `compat/check.sh` | Runs the oracle, registry, and full `zz-mux` library gate |
| `compat/tmux-gaps.json` | Owns active gaps, product status, ordering, evidence, and closed history |
| `compat/tmux-oracle.json` | Records schema 5 source and runtime inventories from the pin |
| `compat/tmux-oracle.py` | Captures and verifies the oracle from a clean pinned source checkout |
| `compat/tmux-tracker.py` | Validates the registry and generates the readable gap report |
| `compat/run.sh` | Builds both binaries and selects scenarios; a full run with `--attached-client` writes the canonical combined summary |
| `compat/startup-diagnostics.sh` | Runs the checksum-attested eight-case startup-cause differential without updating the persisted summary |
| `compat/fetch-tmux.sh` | Acquires tmux and validates its source-aware build stamp |
| `compat/fetch-corpus.sh` | Acquires and verifies the pinned plugin corpus |
| `compat/diff-scenario.sh` | Runs one scenario and emits per-step TOPO, GEO, FMT, OUT, and WARN diffs |
| `compat/scenarios/` | Holds the shared, smoke, and known-divergence corpora |

# Related

- [live tmux compatibility gaps](/tmux/gaps.md) . generated from the canonical registry
- [tmux drop-in plan](/designs/tmux-drop-in.md) . phase ordering and compatibility target
- [tmux divergence matrix](/tmux/divergences.md) . gaps the harness can turn into fixtures
- [updating the tmux reference](/playbooks/updating-tmux-reference.md) . how to move the pin
