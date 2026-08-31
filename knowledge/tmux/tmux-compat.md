---
type: Concept
title: tmux compatibility philosophy
description: "The contract for a tmux-compatible zz CLI: tmux spellings keep tmux meaning or fail loudly, native GUI behavior uses zz-only verbs, and compatibility is measured against one pinned upstream commit."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, philosophy, reimplementation, cli]
timestamp: 2026-08-24T00:00:00-03:00
last_updated: 2026-08-30
last_updated_by: Codex
---

# Overview

zz's multiplexer is a Rust reimplementation of tmux behavior. It does not compile, link, or run
tmux, and no tmux C source is copied into the Rust code. Command names, aliases, configuration
grammar, key tables, formats, options, hooks, targets, and layout arithmetic are checked against one
pinned tmux commit recorded in
[`third_party/tmux-reference/UPSTREAM.md`](/references/tmux-upstream.md).

The product target is a **compatible enough CLI plus a native superset**:

1. A tmux command spelling means what it means in tmux.
2. If zz cannot honor that meaning, it returns a loud error. It does not reuse the spelling for a
   different GUI action.
3. Native behavior uses zz-only verbs such as `split-picker`, `split-browser`, `focus-sidebar`,
   `agent-send`, and `capture-browser`.
4. zz's default bindings may call those native verbs. A binding imported from tmux that names
   `split-window` still creates a terminal split.

This boundary lets the GUI be better than a terminal-emulated tmux surface without making copied
tmux config ambiguous.

# Pinned reference

The reference commit is tmux `d77c9dc6aa021e4bc61f0da128c591af695e6466`
(`next-3.8`). Important upstream ownership areas include:

| Behavior | Upstream files consulted |
| --- | --- |
| Tokenization and config loading | `cmd-parse.y`, `arguments.c`, `cfg.c` |
| Command catalog and aliases | `cmd.c`, `cmd-*.c` |
| Root, prefix, copy, and chooser tables | `key-bindings.c`, `window-copy.c`, `mode-tree.c` |
| Targets | `cmd-find.c` |
| Layout and geometry | `layout.c`, `layout-set.c`, `resize.c` |
| Options and environments | `options-table.c`, `options.c`, `environ.c` |
| Formats and status | `format.c`, `format-draw.c`, `status.c` |
| Hooks and jobs | `hooks.c`, `cmd-run-shell.c`, `cmd-wait-for.c`, `window.c` |
| Client and control mode | `server-client.c`, `control.c`, `control-notify.c` |

The pin is an oracle, not a dependency. Updating it is a separate compatibility event.

Oracle schema 5 records 92 commands, 78 aliases, and 572 accepted command-flag shapes: 318
valueless, 246 required-value, and 8 optional-value. Each command also carries positional minimum
and maximum metadata. It parses nine custom `args_parse` callbacks used by 14 commands and reduces
them to six effective rules. The remaining inventories contain 180 options, 198 global format-table
names, 31 literal context-producer scopes with 153 scoped pairs and 108 unique names, 10 derived
context families, five propagation records, 36 format modifiers, 68 hooks, and 303 default bindings
across five tables. The 198 global names divide into 95 values resolved directly by the mux, 32
delegated to daemon `StatusHooks`, and 71 constant-backed names that remain active `format:` gaps.
The literal scoped pairs divide into 58 implemented, 54 native, and 41 active gaps. The derived
families divide into eight implemented and two active gaps; the modifier vocabulary divides into 31
implemented and five active gaps: `I`, `L`, `O`, `V`, and `w`. `formats.command-item-context`
closed on 2026-08-24 when the shared
`command` name became a command-queue-item fact that every command the mux engine runs carries.

The same command-item hooks reach the five arguments that tmux expands: both rename names, both
show-option names, and `select-pane -T`. Each handler expands after target resolution in the old
target context. Directional `select-pane -T` reads the original pane and writes the expanded title
to the destination pane.

Schema 5 registers source producers and modifier tokens. It does not establish context-value or
modifier-runtime parity. Those semantics remain with the successor groups described below.

The canonical check recaptures the inventory from a `tmux next-3.8` binary at the root of a clean
source checkout at the exact pin. The companion build stamp must also match the commit, version,
fetch recipe, and binary checksum. `ZZ_COMPAT_TMUX` may select another cache produced by that
fetcher; an unstamped checkout or an arbitrary prebuilt that reports the same version fails the
oracle check.

# Status authority

`compat/tmux-gaps.json` is the sole live TODO and status source for tmux compatibility. Schema 3
stores its update date, active gaps, and closed history. The generated
[gap report](/tmux/gaps.md) presents that registry for readers. `status` records an active gap's
product disposition as open, blocked, or accepted. `depends_on` records delivery order and does not
set status.

`just compat-check` calls `compat/check.sh`, validates the clean pinned oracle and registry, requires
eight named mux compatibility tests in the full `zz-mux` library run, then runs three named daemon
compatibility tests through `--exact`. The Rust gate reconciles upstream command and alias names,
flag arities, positional bounds, custom argument rules, option names, global formats, literal and
derived context producers, format modifiers, and hook names. It classifies native commands, native aliases, zz-only flags
on tmux command names, and every zz-only default key. It derives the guarded native-name roster from
the catalog minus the pinned oracle, then checks every pinned canonical prefix against the live
resolver. It pairs every
constant-backed format with a manifest item and tracks every missing default key across `root`,
`prefix`, `copy-mode`, `copy-mode-vi`, and `move`. For each shared default key, it reconciles the
rendered command and repeat bit or requires a named `binding:` divergence. Slice 10m pins the exact
303 pinned, 251 zz, 193 shared, 110 missing, 58 native, 51 divergent, and 142 structurally matching
counts. The structural matches divide into 49 copy-mode, 61 copy-mode-vi, and 32 prefix entries.

Slice 10l closes hook-producer discovery with a daemon-owned source invariant. It names 27 explicit
event producers and derives 37 generic `after-<command>` producers whose suffix names an implemented
command. A later pin audit classifies `after-queue` as explicit-only: ordinary queues do not
produce it, while `set-hook -R` runs it. The current partition contains those 64 automatic hooks,
the explicit-only hook, and three active gaps: `pane-focus-in`, `pane-focus-out`, and
`pane-set-clipboard`. It also rejects duplicate explicit names and produced-versus-tracked overlap. Slice 10m
closes the separate key-only runtime mismatch: bare `bind-key KEY` now preserves commands and
unspecified metadata, applies only requested `-N` and `-r` changes, and silently leaves an absent key
unbound after ensuring its table. Structural key equality still does not prove every downstream
command or copy action. Those consumers retain their existing owners. Slice 10v closes the
open-ended context and modifier registration blind spot. Slice 10w implements `R`; the gate still
does not prove context values or the five remaining modifier semantics. Slice 10ad later
source-registers option consumers, and slice 10ae closes option-name format runtime parity across
that registered roster.
`formats.context-producer-fidelity` owns context values as adopt/open,
`formats.modifier-fidelity` owns modifier semantics as adopt/open, and
`formats.native-typed-context-producers` records native typed producers as native/accepted.
Closed `tracker.semantic-coverage` owns the option-consumer source partition. Closed
`options.option-name-format-coverage` owns generic lookup, target scope, inheritance, array lookup,
and the daemon producer audit. At the
slice 10s close, the nonconstant
global-format registration partitioned the 198-name pin into 92 direct mux values, 32
daemon-delegated values, and 74 active constant-backed `format:` gaps. The mux invariant kept those
sets pairwise disjoint. The exact
`status::tests::daemon_delegated_format_consumers_match_mux_inventory` test seeds buffer, client,
and session facts and resolves every delegated name through the production `DaemonFormatHooks`
consumer.

That registration did not claim context-specific value parity. The 74 format gaps kept their
runtime owners at that checkpoint, and the oracle, protocol, snapshots, scenarios, and accepted
compatibility artifact remained unchanged. Slice 10t promotes `session_path` to direct backing from
the selected session's retained UTF-8 working directory at expansion time. The differential proof
creates two sessions, preserves lexical `/tmp/..`, reads each path through a targeted display, and
reads both through one filtered `list-sessions` query. Focused mux tests separately cover missing
retained or target state and visibility after the production `attach-session -c` command updates
one session. The 10t partition was 93 direct, 32 delegated, and 73 active format gaps. Its accepted
artifact stayed at 98 scenarios, grew to 1,522 steps, and retained an attached-client `PASS`. At the
10t checkpoint, `session_active` remained under `formats.session-runtime` for its no-client,
unattached-client, and attached-session producer audit, and
`sessions.new-session-attach-cwd` owned two cwd mutations that 10t did not change: an existing
`new-session -A -c` target skipped its cwd update, and fresh explicit-empty `-c ''` collapsed to
omitted cwd inheritance. Slice 10x closes both paths below.

Slice 10u closes
`mux.command-group-argument-parse-abort/semantic:command-group-argument-parse-abort` on 2026-08-28.
For a preparation request from a registered `ClientKind::Command`, the daemon applies the existing
static tmux grammar to every ordinary invocation with no user-alias match before the first effect.
The pass covers flags, arity, required values, and nested command blocks. Callback construction and
user-alias validation keep their prior preparation paths, while native zz names remain runtime-owned.
The sole generic-validation bypass covers exact unaliased `attach` and `attach-session` at vector
index zero, where the CLI's private parser owns the positional-session and `--restart-daemon`
extensions. Later exact spellings and every user-alias expansion to either attach name use the
ordinary catalog.

When preparation returns an error, the CLI rejects the complete local vector before stdin capture,
attach or TUI routing, or any effect. Runtime target and effect errors remain sequential, preserving
earlier effects and pruning later commands. Control preparation and framing remain unchanged. Config
and source-file replay construction stays in the residual `mux.chain-parse-abort`; remote `--host`,
replay alias snapshots, and runtime rollback remain outside the closure. Slice 10y later closes the
replay alias snapshot under `aliases.config-parse-unit`. The strict three-step
`smoke/cli-chain-parse-abort` scenario now runs six warm probes for unknown
name, invalid flag, excessive arity, missing value, later `attach`, and later `attach-session`, with
zero differential channels. Slice 10u changes no protocol or snapshot and leaves the 98-scenario,
1,522-step attached-client `PASS` artifact unchanged.

Slice 10v closes `tracker.format-vocabulary-registration` on 2026-08-28. Oracle schema 5 registers
31 literal producer scopes, 153 scoped pairs, and 108 unique names, plus 10 derived families, five
propagation records, and all 36 modifier tokens. The source-owned partitions classify literal pairs
as 58 implemented, 54 native, and 41 active gaps; derived families as eight implemented and two
active gaps; and modifiers as 30 implemented and six active gaps. The two adopt/open successors are
`formats.context-producer-fidelity` and `formats.modifier-fidelity`.
`formats.native-typed-context-producers` records the native/accepted source registrations.

Slice 10v changes no runtime modifier behavior, context values, option consumers, protocol,
snapshots, scenarios, or accepted artifact. The registry now has 91 active groups and 595 items,
with 103 closed records: 49 open, 20 blocked, and 22 accepted. Closed records plus accepted active
groups resolve 125 of 194 known groups (64.4%). The accepted artifact remains 98 scenarios and
1,522 steps with attached-client `PASS` and SHA-256
`810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`. At that checkpoint, the user
paused the campaign before the rerank selected slice 10w.

Slice 10w closes `formats.repeat-modifier` locally on 2026-08-29. `R` splits its body at the first
top-level comma before recursively expanding the value and count. Counts from 1 through 10,000
repeat the value. Leading count whitespace is accepted; trailing whitespace, invalid input, zero,
negative values, and oversized values produce an empty replacement. A missing comma follows the
replacement-failure path and stops later format output. Escaped commas, nested operands, byte-length
replacement, truncation, and post-transform order match the pin. A deterministic 40,960,000-byte
intermediate guard rejects nested amplification before allocation in place of the pin's time
budget. The shipped second and third status-row defaults indent `P:` and `S:` by the byte length of
the session name, and a daemon regression proves they emit spaces instead of literal `R` syntax.

The modifier partition now contains 31 implemented tokens and five active gaps: `I`, `L`, `O`,
`V`, and `w`. The same tracker update records the already-shipped whole-file first-diagnostic lexer
and parser abort under closed `config.parser-abort`; command construction and dispatch across config
and source replay remain open under `mux.chain-parse-abort`. The live registry has 91 active groups,
598 items, and 105 closed records: 49 open, 20 blocked, and 22 accepted. Closed records plus accepted
active groups resolve 127 of 196 known groups (64.8%). The accepted 10w artifact covers 98 scenarios
and 1,526 steps with attached-client `PASS` and SHA-256
`f2aa32e0935e8a839c0abcd43da85e0f474d6c191421776847f7a464cc7257ff`.

Slice 10x closes `sessions.new-session-attach-cwd` locally on 2026-08-29. Existing
`new-session -A -c` targets now share the attach path's retarget and cwd update. The engine expands
`-c` once in the resolved target session, window, pane, and invoking-client context, then stores the
result before a nonnested terminal-open preflight. A headless failure retains the mutation.
Clientless calls stay inert, permitted Control clients attach and update the target, and nested
Interactive, Control, and `-A -d` calls refuse before expansion, retargeting, or mutation. Fresh
creation and an `-A` miss retain an empty session cwd while leaving the initial pane cwd unset for
the donor or caller fallback. Omitted `-c` keeps its prior inheritance.

The ten-step `new-session-cwd` scenario proves one-pass expansion, escaped hashes, source-session
isolation, fresh explicit-empty creation, and an explicit-empty `-A` miss. Focused mux and daemon
tests cover the client and failure-order branches. The live registry has 90 active groups, 596
items, and 106 closed records: 48 open, 20 blocked, and 22 accepted. Closed records plus accepted
active groups resolve 128 of 196 known groups (65.3%). The accepted artifact covers 99 scenarios
and 1,536 steps with attached-client `PASS` and SHA-256
`ed1422d318298b2fee9c31c160393cc2709b9d9137705e96c2632cc700cdcd01`. Slices 10w and 10x remain
local and uncommitted, with no commit or push. No successor will be selected until the full tracker
rerank.

Slice 10y closes `aliases.config-parse-unit` locally on 2026-08-29. Each config file now stores its
original invocations beside their alias-expanded commands or preparation errors before replay. The
daemon parses the file, applies its environment assignments, and prepares every command under one
engine lock. Startup roots and top-level matched source batches finish construction before their
batch replay, while a nested source receives a fresh snapshot when its parent source command runs.
An earlier replayed alias mutation therefore cannot change a later invocation from the same parsed
file.

Stored preparation errors retain source, physical-group, and replay-position metadata. Their
Control warning-versus-guard classification is frozen during construction. `source-file -n` keeps
its no-effect behavior and suppresses stored alias preparation errors. Four focused daemon tests
cover startup roots, same-file mutation, file environment timing, multi-file batches, nested
refresh, parse-only behavior, deferred errors, and Control classification. The two-step
`smoke/config-alias-parse-unit` differential matches the pin in every channel. At the 10y
checkpoint, the live registry had 89 active groups, 595 items, and 107 closed records: 47 open, 20
blocked, and 22 accepted. Closed records plus accepted active groups resolved 129 of 196 known
groups (65.8%). The accepted artifact covered 100 scenarios and 1,538 steps with attached-client `PASS`, retained the two
registered GEO rows, and has SHA-256
`8d53288c8050e5c8cf7f19e6c81687f91544877d32ea4de9f7d40ea2934736b7`. Slices 10w, 10x, and 10y
remained local and uncommitted, with no commit or push. The post-10y rerank froze
`mux.chain-parse-abort` as slice 10z.

The rerank retired the small-slice forecast for `w`. Pinned width behavior includes leading hashes,
`#[...]` style spans, malformed style markup, controls, live `codepoint-widths[]`, tmux's 162-entry
default cache, and the host `wcwidth` policy selected by the harness build's
`--disable-utf8proc`. zz uses `unicode-width` 0.2.2. The `w` item stays later and hard until its
tests cover those platform and Unicode cases. Slice 10y closes the alias snapshot prerequisite.

Slice 10z closes `mux.chain-parse-abort` locally. Each config or source file applies permitted bare
assignments, expands aliases, and validates every command group before any command from that file
runs. The first construction failure preserves earlier bare assignments and drops every command
effect from that file. Parse-only input validates against the pre-file environment and commits no
effects. Startup roots and top-level matched files remain independent units constructed in path
order, while nested children receive fresh units and cannot suppress their parent's later physical
groups. Runtime target and effect errors retain sequential group behavior.

Control emits one located `%config-error` without a failed-command guard and delays construction
warnings until sibling replay finishes. Verbose output retains completed groups and successful
alias-subparse traces before failure. The clean two-step `smoke/config-chain-parse-abort`
differential raises the accepted artifact to 101 scenarios and 1,540 steps with attached-client
`PASS` and SHA-256
`afd1fdf9a79e06f449e8c43abd63b14a2a4968338110223750d4171889c34aaf`.

The same audit closes `hooks.queue`: pinned `after-queue` is explicit-only and the existing
three-step set-hook differential proves ordinary queue inactivity plus exact manual execution. The
10aa close then moves `session_active` into direct mux backing. An explicit `FormatClient` records
no client, an unattached client, or the attached session. Command execution keeps the raw invoking
client separate from the current or explicitly selected target client so each producer follows the
pinned source. Clientless list and filter rows remain empty; target-aware command, status, shell,
buffer, capture, popup, menu, deferred-output, Control, and display-panes formats receive their
selected client. The 198-name partition now contains 94 direct values, 32 delegated values, and 72
active format gaps. Unit, source-file, `run-shell`, `if-shell`, per-client snapshot, and
attached-client fixture proofs show that `client_*` facts and `session_active` use the same
selected client. The implementation changes no protocol or snapshot field.

The historical 10aa checkpoint covers 101 scenarios and 1,550 steps with attached-client `PASS`
and SHA-256 `bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`.

Slice 10ab closes `formats.window-activity-time/format:window_activity`. Each window stores an
optional Unix-second activity timestamp beside zz's logical window-order counter. Window creation,
parsed nonempty pane output, and pinned current-window transitions refresh both values. Same-window
selection, pane selection, pane creation, splits, and layout-only changes without output leave the
timestamp unchanged. The independent audit repaired the direct daemon `switch-client` path so it
refreshes the engine clock before selecting a different window. Plain, boolean, comparison,
list-row, and time-modified expansion use the same stored seconds. The 198-name partition now
contains 95 direct mux values, 32 daemon-delegated values, and 71 active gaps. No protocol or
snapshot field changed.

Slice 10ac closes
`jobs.command-status-environment/semantic:shell-job-clean-environment`. Shell-form `run-shell` and
shell-form `if-shell` start from an empty process environment, then receive the modeled global and
selected-session overlays in that order. Status `#()` receives the modeled global overlay only.
An explicit missing command target produces the same sessionless global-only environment. Hidden
entries, unset markers, and stale private startup variables stay absent. A visible modeled
`TMUX_PANE` survives without zz inventing one. Completed startup forces `TERM` from
`default-terminal`, `TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION=3.8-zz`, and
`COLORTERM=truecolor`; startup command jobs preserve their modeled TERM family. `TMUX` identifies
the selected session or uses `-1` for sessionless and status jobs. The private tmux executable stays
first on the modeled PATH.

Slice 10ad closes `tracker.semantic-coverage/semantic:tracker-option-consumer-registration`. The
unchanged 105-name roster now lives in `command::TMUX_OPTION_CONSUMERS` beside command and accessor
behavior. `BEHAVES` remains a public alias. The exact manifest guard proves that the pin and live
catalog share 180 unique names, the consumer roster contains 105 unique catalogued names, and 75
names retain live `option:` gaps. It rejects overlap, requires those sets to exhaust the catalog,
and verifies the closed tracker record. `copy-mode-mark-style` belongs to the roster only through
status option-variable consumption; this closure makes no visual rendering claim. The compatibility
gate passes 445 mux tests plus three daemon inventory tests. Full workspace tests and clippy,
formatting, diff, tracker, and checked-summary checks pass.

Slice 10ae closes
`options.option-name-format-coverage/semantic:option-name-format-coverage`. The 105-name roster
contains 13 server, 42 session, 40 window, and 10 pane consumers. Generic option lookup precedes
format-table names, command-item facts, and environment values. Exact names and legacy aliases use
the selected target, pinned inheritance, attached-client fallback, active children, and `S`, `W`,
and `P` loop retargeting. Command prefixes do not enter option lookup.

Flags render as `0` or `1`; other types retain their tmux spelling. `command-alias`,
`status-format`, and `update-environment` support whole-array and indexed lookup. Whole arrays put
numeric entries before named entries, normalize numeric leading zeroes, and apply whole-array local
shadowing. Malformed, missing, and overflowing indices expand empty. Mux-owned formats read live
state. Direct daemon producers use the same live resolver, while detached status shares one
all-scope snapshot across each refresh batch. Missing-target `run-shell -C` and `if-shell -F` read
global options without changing the inserted command or branch execution context.

The focused 60-step `option-name-formats` row has zero topology, geometry, format, output, or
warning differences. The attached status probe passes. Exhaustive mux and daemon tests cover the
roster, scopes, arrays, targets, loops, producer inventory, and detached refresh. The slice changes
no protocol, wire snapshot, or native GUI styling.

Slice 10af closes
`jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`.
Shell-form `run-shell` with explicit positive `-d` retains command text, target identity and numeric
session id, expanded text and numeric arguments, and the cwd string at scheduling. Child launch
reads current global state, the live original-session overlay or its retained overlay after
destruction, `default-terminal`, and the startup TERM gate. Same-name session recreation cannot
replace the original retained overlay. A missing scheduling target stays sessionless with `TMUX` id
`-1` if a matching session appears before launch. The child applies cwd existence fallback when it
starts.

Foreground daemon coverage waits for `active_shell_jobs` before mutating state. The background
three-step differential now completes twelve checks per engine across live, destroyed and
recreated, missing and later-created, and startup-crossing cases. It also proves frozen formats,
numeric arguments, target identity, and cwd, with no differing channel. Mux coverage proves retained
original-session identity and writes into an initially empty overlay. Immediate and zero-delay
ordering, `run-shell -C`, `if-shell`, cwd producer choice, `copy-pipe`, and popup jobs retain their
separate owners.

Slice 10ag closes `source-file.startup-client-cwd`. Only the cold launcher that auto-spawns a daemon
passes private `--bootstrap-client-cwd`; startup carries that bounded UTF-8 base through nested
relative sources and literal metacharacter paths, then clears it before runtime commands. The
isolated differential passes exactly on both engines without a public protocol change. The full
eight-case diagnostic then exposed queued pane output during Control exit, which slice 10ai closes.

The live registry has 83 active groups, 585 items, and 124 closed records: 41 open, 20 blocked, and
22 accepted. Closed records plus accepted active groups resolve 146 of 207 known groups (70.5%).
The persisted accepted artifact covers 106 scenarios and 1,683 steps, with attached-client `PASS`, exactly two approved GEO
rows, every other channel clean, and SHA-256
`a59c1ff951d817f00cfed37367c3e7cae8f258840876d502f12622981a1c174f`. Slice 10ai starts Control
stdin observation before initial preparation, discards queued and future pane-byte records after
EOF or blank Return, and retains all non-pane Control records plus one final exit. Shell-job cwd and
literal DEL identity are closed with their focused and aggregate proof.

`F-ALIASES-MULTI-BODY` closes executable empty and multi-command user aliases. Preparation stores one
opaque typed group in protocol v84's existing `CommandInvocation` shape: empty groups succeed
without effects, caller arguments and client-owned stdin attach only to the final child, and
nonempty groups preserve child
boundaries and physical source groups through mux, daemon, stored command, config, local CLI, and
Control execution. The wrapper is never dispatched or framed; Control emits each child with the
enclosing queue's flags. Its eight-step differential has zero mismatches. Forced-shutdown
multi-window `window-unlinked` order remains explicitly open under
`hooks.shutdown-window-unlinked-order` because tmux derives it from winlink red-black-tree history
that zz does not retain. Closure review advanced protocol v85 for typed post-admission callback
provenance and daemon-authoritative `Attached` reconnect state, not for an alias child-vector field.
Protocol v86 then closes `control-mode.diagnostic-typing`: Control receives config summaries and
located command diagnostics as typed `ConfigDiagnostic` events and selects `%config-error` without
reading the text. The three-step differential is clean. The full corpus keeps its prior accepted
artifact because two callback result-marker rows fail the same way on clean `origin/main`.
The next worker claims a dispatch-board front from published `origin/main`.

Protocol v84 closes all six runtime rules
across the 12 implemented callback commands; no command-specific `args-parse:` item remains.
`choose-client` and `switch-mode` remain covered by their unimplemented command items. `if-shell`
preserves unquoted typed branches across source-file and Control parsing, rejects typed conditions
and option values before effects, and leaves quoted braces as strings. `run-shell` accepts typed
positionals only when a leading `-C` enables command mode; option values and all positionals without
that flag remain strings. `set-option` and `set-window-option` accept typed value position 1,
expand the live mux environment and recursively print it before optional `-F` expansion, and keep
names, flag values, and extras string-only. Every `bind-key` positional accepts a typed block or
string while `-T` and `-N`
remain string-only. It stops scanning at the first positional or `--`, prints a typed key before
lookup after live mux-environment expansion, preserves typed physical-line groups, reparses one
string tail as one group, and retains the pin's empty binding for a typed first variadic tail.
Unknown typed-key commands keep their source diagnostic, while a constructed invalid key remains
a bare key error. `confirm-before` now applies the same command-or-string rule to its one command
positional while `-c`, `-p`, and `-t` stay strings. Every lexical typed block recursively
constructs before its parent's name, callback type, or arity validation. Each recursive path gets
one independent user-alias layer; alias-produced subtrees disable another user-alias expansion, and
self-recursion fails as unknown without killing the daemon. Nested `if-shell`, `run-shell`,
set-option, and confirm blocks print canonical names. Empty blocks read back as `{  }`, and physical
internal group newlines print as ` ;; `. String children construct after target lookup and
parent-format expansion as one group. Exact Control comparisons prove nested bind and confirm
construction failures are preflight parse errors. Stored `bind-key` and `set-hook` lists and typed
`if-shell`, `run-shell`, and `confirm-before` callbacks execute their constructed commands without
another user-alias lookup. Typed `if-shell` and `run-shell` callbacks preserve physical groups: a
failed group stops its remaining commands while later physical lines continue; string callbacks
stay one group. Typed `command-prompt` templates retain their structured prepared command list
through submission without re-expanding aliases. The template positional accepts a typed block or
string, while option values remain strings. Structured substitution preserves leaf-argument
boundaries against quote or semicolon injection. String templates substitute raw source before a
fresh parse and whole-result construction pass against the current alias table. Both paths replace
the first `%%` and every `%1`; a trailing `%` quotes double quotes, backslashes, dollar signs,
semicolons, and tildes. Typed callbacks retain
physical groups, while string templates and free input form one group. String failures retain the
originating source path and line. Prompt chains and multi-answer `%2` stay under their existing
prompt owner. `set-hook` and command-valued native set-option deliberately construct a second
time. Without `-B`, only `set-hook` value position 1 accepts a typed block; with `-B`, every
positional lexically accepts either type. Hook names and extra positionals remain strings without
`-B`; `-B` and `-t` values remain strings in both modes. zz still rejects `-B` during execution because format monitors remain
unsupported. Built-in hook values flatten physical groups during their second construction pass;
custom `@` typed values retain textual ` ;; ` groups. Empty and failing local appends still create
an empty local array and shadow the inherited global hook. Typed ignored `-R` values construct before the
stored hook runs. `display-menu` applies a data-dependent NAME, KEY, and ACTION state to its
positionals. Nonempty names consume a string key and a string-or-typed action; empty names are
separators and leave the next positional in NAME state. All ten valued flags stay string-only.
Typed children construct before the parent type, arity, or effects, accepted typed actions print
canonical child commands in stored bindings, and incomplete NAME or NAME-plus-KEY tails defer to
daemon runtime validation. Runtime resolves the current or `-c` target client before completeness,
so an unattached command or initial Control reports `no current client`; initial Control uses a
flag-0 `%error` and exits 1. Once attached, Control validates an incomplete group as `not enough
arguments` before its overlay no-op and returns a flag-1 `%error`; EOF after that frame exits 1.
Interactive ordering remains unchanged. The daemon drops the
structural wrapper only for typed actions before a fresh selection parse; quoted brace actions
remain literal. Broader eager whole-file source
construction, multiline inner-source placement, generic alias recursion, selected-action error
delivery, and replay-channel placement remain open. Slice 10y closes the same-file alias snapshot.
Attached menu
rendering and keyboard ownership now close for raw zz-tui under
`clients.tui-display-menu-overlay`: the client consumes the daemon-published descriptor and uses
the shared menu resolver. Action context and errors, mouse policy, paste-close ordering, queue
ordering, rendered width, resize lifecycle, shortcut display and grammar, and style refresh remain
under `display-menu.behavior-fidelity`. Raw zz-tui popup rendering and input ownership later closed
under `clients.tui-display-popup-overlay`; the six broader popup behavior classes remain under
`display-popup.behavior-fidelity`.
`display-panes` accepts an optional string or typed template while `-d` and `-t` values remain
strings. Typed children construct before parent option-type or arity validation. Aliases and
prefixes retain typed positions and canonical stored readback. Targetless routing resolves an
attached client before duration validation, producing `no current client` only when none exists.
The strict 22-check fixture closes that parser and routing boundary with zero differential channels.
Custom template execution remains parked because mux runtime rejects the positional value instead
of substituting the selected `%pane` for `%%%` and executing with the original queue state. Tmux
uses `select-pane -t "%%%"` when the template is omitted; queue blocking and presentation stay
separate.
`choose-buffer` and `choose-tree` closed together as a deliberate exception to the planned separate
10j and 10k milestones. They share one callback rule, one chooser-template execution path, and one
attached-client fixture. Each accepts zero or one string-or-typed template while `-F`, `-f`, `-K`,
`-O`, and `-t` values stay strings. Typed children construct before parent type, arity, target, or
effects. A typed template stores canonical command text before opening; a quoted template stays
raw. Selection substitutes the exact buffer name or tree target, reparses against the current alias
table, and executes in the invoking client's live context after closing the chooser. The first
`%%` and every `%1` receive the selected value, and a trailing `%` applies the pinned quoting rule.
Empty and stale buffer selections run no custom action. Attached parse and command errors begin
with an uppercase character. The strict three-step fixture completes 26 checks and ends with
`ARGS_PARSE_CHOOSERS=clean:26` on both servers with zero differential channels.
Shared command-flag diagnostics closed on 2026-08-28. One
catalog-driven parser covers all 83 implemented upstream commands and 74 built-in aliases through
mux execution, daemon preflight, and stored commands. Exact native attach shares the leading-option
diagnostics, then stops scanning at its positional-session extension. The focused differential
compares 516 probes against both zz and the pin, including unknown and invalid flags, help usage,
missing values, required-value absorption, and optional-value lookahead. Config and source-file
command-group construction stays under `mux.chain-parse-abort`. Positional bounds run after option
grammar and before recognized parked capability rejection on direct, daemon-preflight, and stored
command paths. Differential scenarios,
attached-client fixtures, unit tests, and manual GUI checks supply behavioral evidence.

The [2026-08-22 CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
preserves the measured baseline at commit `202f322`. Its counts describe that audit date. The
[divergence matrix](/tmux/divergences.md) keeps the source rationale and probe evidence behind
accepted differences. Neither document tracks live completion.

# What “compatible enough” means

The alias goal is not a percentage. It is a workload contract:

- Core session, window, pane, buffer, target, layout, and query commands used from a shell work.
- `new-session`, bare attach, reattach, read-only attach, detach, and kill preserve the calling TTY
  contract.
- A user's config and the pinned plugin smoke corpus load without a SKIP. Any SKIP fails the run.
- Script-facing stdout, stderr, exit status, formats, and errors match where the workload observes
  them.
- Ordinary `capture-pane` text follows tmux's `-p` versus named/automatic-buffer routing, clustered
  value flags, inclusive and reversed ranges, target-scoped bound expansion, and invalid-bound
  fallback. Trailing blank rows at a fallback visible end, richer raw and metadata transports, and
  saved-alternate capture remain excluded.
- Bindings explicitly declared in an imported tmux config retain tmux command meaning. Import does
  not synthesize stock bindings absent from the file; zz's own defaults remain free to use native
  verbs.
- Any accepted divergence is named, tested where possible, and excluded from the promise.
- Missing low-value models do not hold the alias hostage.

One loud error-precedence edge sits outside that workload promise. From a nested client,
`new-session -s existing` reports zz's nesting refusal before the mux sees the duplicate name;
pinned tmux reports the duplicate first. Both reject without changing state.

The compatibility gate should name the supported workload and its exclusions. “80 commands” alone
is too weak because a command can still reject flags or produce different output. “All 92 commands”
is too expensive because linked sessions, shared-server ACLs, and tmux floating panes do not fit the
zz model.

# Permanent exclusions

- **Real tmux socket protocol.** `alias tmux=zz` means zz handles the argv. zz never speaks tmux's
  private client/server wire format.
- **Linked windows and session groups.** A zz window belongs to one session. `link-window`,
  `unlink-window`, and `new-session -t` stay loud.

Floating tmux panes, client suspension, and shared-server ACLs are parked, not part of the practical
alias target. They should be revisited only if a real workload needs them and their semantics fit
the product.

# Native presentation

tmux draws status rows, prompts, choosers, copy mode, pane indicators, menus, and popups with terminal
cells. zz publishes daemon-owned state and renders it in its clients:

- `status-format[]` rows render in the TUI and in a top or bottom GUI row when a config customizes
  status. Native sidebar and titlebar presentation remain at defaults.
- The GPUI client uses native surfaces for prompts, choosers, menus, popups, copy mode, and pane
  indicators.
- The raw TUI consumes command prompts, confirmations, menus, popups, choose trees, choose buffers,
  and display-panes. `display-menu.behavior-fidelity` and `display-popup.behavior-fidelity` retain
  the broader behavior classes outside those presentation closures.
- A native surface may look different. Its command, key, target, exit, and state semantics remain
  part of the compatibility contract for every client that presents it.

This is a presentation divergence, not permission to reinterpret a tmux command.

# Config ownership

By default the daemon sources the first existing zz-owned platform candidate for `zz/mux.conf`:
XDG config, the home config directory, macOS Application Support, or Windows AppData in platform
order. One or more top-level startup `-f` files replace that default for the initial load and remain
visible through `#{config_files}`. `reload-config` returns to the first existing platform candidate
and updates the fact to that path, or to empty when no candidate exists. The import flow copies a
donor `.tmux.conf` to the first existing or first constructible candidate. The daemon does not read
`~/.tmux.conf` directly on every boot.

The config parser implements tmux grammar and reports unsupported commands. Its whole-file lexer
and parser path latches the first diagnostic, clears the commands built from that file, and stops
scanning. Slice 10y prepares each file's alias expansions before replay, with fresh snapshots for
nested sources and deferred diagnostic metadata. Slice 10z then constructs each config or source
file before effects, including parse-only validation, independent sibling units, nested-child
isolation, Control warning placement, and verbose alias traces. On Unix, the daemon gives shell and
status jobs a clean modeled environment and puts a private `tmux` executable first on the modeled
PATH so subprocess calls return to the same zz daemon. Shell-form `run-shell` and `if-shell`
receive global then selected-session overlays; status `#()` receives only the global overlay. A
user's shell alias does not cover direct process lookup, and packages do not currently install a
global executable.

# Empty boot and attach

A daemon started by a command query begins with no sessions, windows, or panes. The first explicit
`new-session` gets numeric name `0` and ids `$0`, `@0`, and `%0`, matching the pin's allocation.

The installed bare launcher rewrites an empty argv to `new-session -A`. That existing tmux-shaped
verb creates session zero on an empty daemon and attaches the current session on a live daemon.
Explicit targetless `attach` and `attach-session` still preflight the server and return tmux's exact
`no sessions` with exit 1, so `attach || new-session` keeps working. The daemon's lower-level lazy
attach remains serialized: simultaneous first attaches and a command client creating a session at
the same boundary converge instead of creating duplicates or failing. Therefore:

- bare packaged `zz` creates and attaches session zero on an empty daemon;
- bare packaged `zz` attaches the current session when one exists;
- `zz attach` and `zz attach-session` return `no sessions` on an empty daemon;
- `zz new -s NAME` creates and attaches on a TTY;
- `zz attach -t NAME` attaches an existing session;
- direct bundle launch or `zz app` opens the GUI.

Attaching `new-session` also applies the same nested-session refusal as `attach-session` before mux
state changes. An existing `new-session -A -c` target shares `attach-session`'s cwd update and
one-pass target-context expansion. A nonnested terminal-open failure keeps that session mutation,
while nested refusal happens before expansion or mutation. Fresh creation and an `-A` miss retain
an empty session cwd from `-c ''` without passing an empty cwd to the initial pane. The packaged PTY
fixture pins detached dash sizing, attached client dimensions,
read-only input rejection and output visibility, requested detach, and `attach -d` peer eviction
through the real spaced-path launcher. Both detach paths require exit zero and the tmux-shaped
`[detached (from session NAME)]` notice after terminal restoration.

# Related

- [live tmux compatibility gaps](/tmux/gaps.md)
- [tmux CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
- [tmux divergence matrix](/tmux/divergences.md)
- [tmux drop-in plan](/designs/tmux-drop-in.md)
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md)
- [tmux commands](/tmux/commands.md)
- [key tables](/tmux/key-tables.md)
- [status line and formats](/tmux/status-line.md)
- [configuration parser](/tmux/conf-parser.md)
- [compatibility harness](/playbooks/compat-harness.md)
