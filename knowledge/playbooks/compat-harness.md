---
type: Playbook
title: Running the tmux compatibility harness
description: How to run the pinned tmux differential corpus, read topology, geometry, format, and query-stdout results, and record known divergences.
resource: compat/run.sh
tags: [tmux, compatibility, differential-testing, geometry, playbook]
timestamp: 2026-08-26T00:00:00-03:00
last_updated: 2026-08-28
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
and registry, requires seven named mux manifest tests, then runs the full `zz-mux` library suite.
It also requires two named daemon tests, the hook-producer partition and delegated-format consumer
tests, and runs each through `--exact`.
Linux CI runs the same command after restoring the pinned tmux cache. A full
`compat/run.sh` checks the oracle and tracker before executing scenarios.

Oracle schema 4 records 92 commands, 78 aliases, and 572 accepted command-flag shapes: 318
valueless, 246 required-value, and 8 optional-value. Every command also carries its positional
minimum and maximum. The source pass also records 14 commands that use nine custom `args_parse`
callbacks as six effective rules. The remaining inventories contain 180 options, 198 global
format-table names, 14 source-enumerated names across the selected `command-item`, `list-commands`,
and `list-keys` contexts, 68 hooks, and 303 default bindings across `root`, `prefix`, `copy-mode`,
`copy-mode-vi`, and `move`. The 198 global names divide into 93 values resolved directly by the mux,
32 delegated to daemon `StatusHooks`, and 73 constant-backed names that remain active `format:`
gaps.

The Rust gate reconciles command and alias names, flag arities, positional bounds, custom argument
rules, option names, global and selected context-format names, and hook names. It also classifies
native commands, native aliases, zz-only flags on tmux command names, and every zz-only default key.
It derives the guarded native-name roster from the catalog minus the pinned oracle and checks every
pinned canonical prefix against the resolver. It pairs every constant-backed format with a manifest
item and tracks every missing default key across all five tmux tables. For each shared default key,
it also reconciles the rendered command and repeat bit or requires a named `binding:` divergence. The
three selected context rosters contain 1 `command-item` name, 3 `list-commands` names, and 10
`list-keys` names. zz implements all 14. `formats.command-item-context` closed on 2026-08-24: the
mux dispatch chokepoint carries the canonical entry name into every command it runs, so `#{command}`
expands inside any command item and stays empty outside one. The daemon-preempted half closed under
`formats.daemon-command-item-context`; its immediate format hooks now carry the same canonical name,
and the daemon's post-spawn `new-window`/`split-window -P -F` pass retains it while adding live pane
facts. Delayed subscriptions and prompts stay outside an item.

Schema 4 does not cover the full context or modifier vocabulary. Its Python capture and Rust gate
repeat the same three-scope selection, so they can agree while omitting source producers. The
post-10u audit found 31 literal `path:function` scopes, 153 scoped name pairs, and 108 unique names
in the pin. Ninety-four names sit outside the selected 14. Pinned `format_build_modifiers`
recognizes 36 tokens; zz accepts 30 and omits `w`, `I`, `L`, `O`, `V`, and `R`.

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
exact alias and user `command-alias` precedence. Matched empty, multi-command, and unparsable alias
shadows are unit-tested at the mux and daemon dispatch seams instead: their expected zz result is a
loud `unknown command: <typed name>`, so they are not a differential claim while
`aliases.command-bodies` remains open.
Protocol v74 closes Control's former static unknown-name precheck through focused daemon and CLI
tests. The client prepares the entire initial argv unit or complete LF line before opening execution
frames, observes one daemon alias snapshot for that unit, preserves command numbering and
notifications, and executes the prepared invocation with ordinary read-only authorization. These
tests do not claim tmux-compatible empty or multi-command alias bodies. The strict
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

Only the unknown-name error shape is pinned here; malformed alias-body text remains zz-defined while
`aliases.command-bodies` is open. Focused binary coverage also exercises routing-sensitive
new-session and attach forms, `-N`, startup shadows, arbitrary startup aliases, invalid nested alias
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
neither the protocol nor the snapshot schema.

Oracle schema 4 closes callback discovery, not callback behavior. The typed Rust sidecar mirrors the
12 implemented callback commands. Protocol v84 adds zero-based lexical command-block positions to
`CommandInvocation`, and `COMMAND_ARGS_PARSE_BEHAVES` contains `bind-key`, `command-prompt`,
`choose-buffer`, `choose-tree`, `confirm-before`, `display-menu`, `display-panes`, `if-shell`,
`run-shell`, `set-hook`, `set-option`, and `set-window-option` after source-file, Control,
stored-command, parser, postcard, mux, and daemon proofs. All 12 implemented callback commands now
apply their pinned rules. The unimplemented `choose-client` and `switch-mode` callbacks need no
`args-parse:` item because their `command:` items cover the whole command.

Hook-producer discovery closed in slice 10l. The daemon-owned invariant names 27 explicit event
producers and derives 37 generic `after-<command>` producers from implemented canonical commands.
It reads the four active `hook:` items from the live tracker, rejects duplicate explicit names and
produced-versus-tracked overlap, and requires those 64 produced names plus `after-queue`,
`pane-focus-in`, `pane-focus-out`, and `pane-set-clipboard` to equal all 68 pinned names.
`just compat-check` requires the exact
`daemon::tests::pinned_hook_producer_partition_matches_the_oracle` and
`status::tests::daemon_delegated_format_consumers_match_mux_inventory` tests. The second test seeds
buffer, client, and session facts, then requires every one of the 32 delegated names to resolve
through the production `DaemonFormatHooks` consumer. At the slice 10s close, the mux manifest test
required the 92 direct, 32 delegated, and 74 tracked sets to stay pairwise disjoint and equal the
198-name pin.

That registration closed source ownership only. It did not claim context-specific value parity,
and all 74 format gaps retained their runtime owners at that checkpoint. The oracle, protocol,
snapshots, scenarios, and accepted compatibility artifact did not change. Slice 10m already closed
the shared-binding
runtime mismatch for bare key-only `bind-key`; downstream command and copy-action behavior retains
its separate owners. The post-10u rerank freezes slice 10v on
`tracker.format-vocabulary-registration`. Schema 5 will source-register all literal context
producers by `path:function`, the complete modifier token set, queue-added `current_file`, `hook`,
and `hook_arguments`, and explicit dynamic families: numbered run-shell argument keys, `hook_argument_<n>`,
`hook_flag_<char>`, `hook_flag_<char>_<n>`, `next_window_index`, `next_window_active`,
`prev_window_index`, `prev_window_active`, and `next_@*` or `prev_@*` user options. Production-owned zz inventories
must classify each entry as implemented, native, or an active gap. Those partitions must be disjoint and
exhaustive. New or stale source entries must fail `just compat-check`.

The single semantic registration owner records the six absent modifiers without creating six
runtime items or claiming runtime support. Direct scratch-socket probes already show silent
differences for `w` display-cell width, `R` repeat, `O` option loops, and `V` environment loops.
Pinned source and the manual classify `I` as client feature, termcap, and environment
interrogation; the pinned modifier regression classifies `L` as an attached-client loop. Context-specific value
parity and option `BEHAVES` consumer truth stay open. Slice 10v
changes no protocol, snapshot, differential scenario, or accepted artifact. Its frozen registry
state is 89 active groups with 594 items and 102 closed records: 48 open, 20 blocked, and 21
accepted, for 123 of 191 groups resolved (64.4%). Shared command-flag diagnostics
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
errors. The typed confirm callback executes its constructed list without another user-alias lookup;
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
hook. Both sides publish `ARGS_PARSE_SET_HOOK=clean:24`. Eager whole-file construction, same-source alias mutation,
multiline inner-source placement, monitor semantics, and broader replay placement retain their
existing owners.

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

`compat/results/summary.md` is the persisted acceptance artifact. The slice 10u closure from
2026-08-28 leaves it at 98 scenarios and 1,522 steps against pinned tmux `d77c9dc6`. Every ordinary
row is clean, and the attached-client fixture is `PASS`.
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
`session_active` tri-state producer audit remains under `formats.session-runtime`. The new
`sessions.new-session-attach-cwd` group owns two cwd mutation differences outside this slice:
existing-target `new-session -A -c` skips the target update, and fresh explicit-empty `-c ''`
collapses to omitted cwd inheritance.
Slice 10u keeps `smoke/cli-chain-parse-abort` at three harness steps and now runs six warm probes
without changing the scenario count, persisted step count, or attached result. All six finish with
zero TOPO, GEO, FMT, OUT, or WARN differences. Runtime target and effect errors retain the existing
sequential probe: the earlier effect remains and the failed command prunes the later effect.
Slice 10v is a source-registration plan freeze. It adds no canonical scenario or step and does not
change the attached-client result. The accepted artifact remains 98 scenarios and 1,522 steps with
attached-client `PASS` and SHA-256
`810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The attached-client fixture is `PASS`. The expanded
corpus pins capture routing and ranges, manual window geometry,
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
construction, same-source alias mutation, generic alias recursion, and raw-TUI overlay parity keep
their existing owners.
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
close-mid-paste ordering, same-source alias mutation, eager whole-source construction, or generic
alias recursion.
Built-in hook values flatten typed physical groups during their second construction pass. Custom
`@` values keep normalized textual groups, and typed ignored `set-hook -R` values still construct.
Prompt chaining and multi-answer `%2` remain under the prompt-fidelity owner.

The checked-in summary includes the current focused counts: `smoke/source-file-diagnostics`,
`source-file-format`, and `smoke/source-file-control` contain 12, 40, and 12 steps,
`resize-directions` contains 16, and `formats-values` contains 18. The summary SHA-256 is
`810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`.
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
requires its persisted attached-client status to be `PASS`. The check passes for the 2026-08-28
canonical checkpoint and exits before building or running either server. Linux CI first asserts that
`compat/results/summary.md` is tracked, then runs
the inventory and result check after checkout. A named partial run, a headless-only full run, a failed
run, or a run with a SKIP cannot overwrite the canonical report. After a complete strict run with
`--attached-client`, CI diffs the full tracked summary, so changes to Steps, TOPO, GEO, FMT, OUT,
WARN, or the attached proof fail the job. Per-scenario logs remain ignored scratch data, while the
canonical summary stays versionable.

`smoke/config-grammar` intentionally expects the invalid-octal `%config-error` from tmux only. The
nested zz control client still does not publish that diagnostic; the state readbacks separately
prove that both parsers abort the file at the same point.

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

`compat/startup-diagnostics.sh` is a separate seven-case gate for clientless startup causes. Run it
after building the debug binary and fetching the pinned oracle:

```sh
cargo build -p zz --bin zz
compat/startup-diagnostics.sh target/debug/zz compat/.cache/tmux-src/tmux
```

The script requires all seven cases: initial Control cold start; detached launch followed by late
Control attach; startup list-output discard; explicit-root failure ordering; multiline cause
prefixing and completion-line location; daemon-restart redelivery; and Interactive delivery with a
global drain. It compares normalized Control transcripts, checks detached streams and status, and
drives the attached Interactive view through real outer PTYs.

The oracle must be the checkout-root `tmux` executable from a clean checkout at exact commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, report `tmux next-3.8`, and match the build stamp's
commit, version, fetch-script checksum, and binary checksum. The probe requires GNU `timeout`, wraps
commands in real 15-second deadlines, uses 500 ms bounded polls, and stops readiness loops after 10
seconds. A missing case or any skip fails the run.

The final debug run passes all seven cases with no skips. This focused script does not call
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

Use the generated report to choose the next slice. The roadmap supplies dependency order, and the
divergence matrix supplies detailed rationale; neither owns live status.

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
The two current entries pin the deliberate refusals of upstream layout bugs:
`known-main-preset-two-panes.txt` (the pin never sizes the lone "other" pane) and
`known-spread-mixed.txt` (the pin's `-E` corrupts a parent mixing leaf and node children).
They use `layout.main-horizontal-upstream-bug` and `layout.spread-mixed-upstream-bug`.

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
| `compat/tmux-oracle.json` | Records schema 4 source and runtime inventories from the pin |
| `compat/tmux-oracle.py` | Captures and verifies the oracle from a clean pinned source checkout |
| `compat/tmux-tracker.py` | Validates the registry and generates the readable gap report |
| `compat/run.sh` | Builds both binaries and selects scenarios; a full run with `--attached-client` writes the canonical combined summary |
| `compat/startup-diagnostics.sh` | Runs the checksum-attested seven-case startup-cause differential without updating the canonical summary |
| `compat/fetch-tmux.sh` | Acquires tmux and validates its source-aware build stamp |
| `compat/fetch-corpus.sh` | Acquires and verifies the pinned plugin corpus |
| `compat/diff-scenario.sh` | Runs one scenario and emits per-step TOPO, GEO, FMT, OUT, and WARN diffs |
| `compat/scenarios/` | Holds the shared, smoke, and known-divergence corpora |

# Related

- [live tmux compatibility gaps](/tmux/gaps.md) . generated from the canonical registry
- [tmux drop-in plan](/designs/tmux-drop-in.md) . phase ordering and compatibility target
- [tmux divergence matrix](/tmux/divergences.md) . gaps the harness can turn into fixtures
- [updating the tmux reference](/playbooks/updating-tmux-reference.md) . how to move the pin
