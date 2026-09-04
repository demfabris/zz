---
type: Playbook
title: Running tmux compatibility cohorts
description: A bounded, parallel workflow for closing the practical alias tmux=zz gap without letting new oracle findings extend one campaign forever.
tags: [tmux, compatibility, campaign, workflow, agents]
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-09-01
last_updated_by: Claude
---

# Outcome

Close the practical `alias tmux=zz` gate through bounded slices. Each slice starts with a fixed
acceptance contract, ends with one reviewed commit, and leaves later discoveries in the
[live tracker](/tmux/gaps.md).

The Alert cohort completed in commit `2e4ccf3b9b6706e44215d74ca147643e6baa3d2e`. The dedicated
campaign branch then closed session cwd in
`2468bfd8f1a11430a73b7066b022101b4048d981`, requested client flags in the next milestone, and
retained-client sizing in the third milestone. The fourth milestone closed client-environment
seeding and refresh with protocol v82. The fifth closed retained client formats with protocol v83,
the sixth closed the six client lifecycle hook producers, and the seventh proved silent Control
delivery for asynchronous copy-pipe failures. The eighth registered and executed the 24-command
daemon invalid-flag runtime roster. The ninth closed all eight pinned positional-maximum mismatches
with catalog-owned metadata and the pin's first-positional flag boundary. The tenth closed all
fourteen required positional minima through a separate catalog sidecar. The eleventh removed the
partial maximum roster and applied the catalog contract to all 72 implemented finite upstream
commands, including stored binding and hook children. The complete CLI and app-library gates then
closed native `attach-session -E` routing, published `client-PID` targeting, and stale client-format
and command-palette assertions. The twelfth error-contract milestone replaced the partial daemon
flag roster with one catalog parser across all 83 implemented upstream commands and 74 aliases.
The thirteenth closed the final `mux.error-shapes` item by matching nested `new-session`
validation precedence without implementing session groups. The fourteenth closes the first custom
argument rule with protocol v84: `CommandInvocation` retains lexical command-block positions and
`if-shell` applies them across source-file, Control, aliases, bindings, hooks, and background work.
The fifteenth reuses that v84 metadata without another wire change and applies `run-shell`'s
leading `-C` rule across every positional, option boundary, stored command, Control route, and
background callback.
The sixteenth reuses protocol v84 again and applies the shared `SetOptionValue` rule to
`set-option` and `set-window-option`, including recursive group printing, format order, aliases,
stored commands, source-file, and direct Control behavior.
The seventeenth reuses protocol v84 for `bind-key`, applying the shared commands-or-string rule to
every positional while preserving the pin's distinct typed-tail, string-tail, key-printing, and
physical-group execution contracts through a real attached client.
The eighteenth reuses protocol v84 for `confirm-before`. Its one command positional now follows
the typed-block-or-string rule, option values remain strings, typed construction precedes target
lookup, and string construction follows parent-format expansion. Every lexical typed block
constructs recursively before parent name, callback type, or arity validation. Recursive paths
carry independent one-layer user-alias budgets, alias-produced subtrees disable further user
aliases, and self-recursion fails unknown without killing the daemon. Nested callbacks print
canonically, including `{  }` for an empty block and ` ;; ` for physical internal group newlines.
Stored `bind-key` and `set-hook` lists and typed `if-shell`, `run-shell`, and `confirm-before`
callbacks execute their constructed commands without another user-alias lookup. Typed `if-shell`
and `run-shell` callbacks preserve physical groups: a failed group stops its remaining commands
while later physical lines continue; string callbacks remain one group. `set-hook` and
command-valued native set-option deliberately construct again. Built-in hook values flatten
physical groups during their second pass, while custom `@` typed values retain textual ` ;; `
groups. A typed ignored `set-hook -R` value still constructs. A typed `display-menu` action drops
its structural wrapper before the fresh selection parse, while a quoted brace string remains
literal. Typed `command-prompt` templates retain their structured prepared command list through
submission without re-expanding aliases. Structured substitution preserves leaf-argument
boundaries against quote or semicolon injection. String templates substitute raw source before a
fresh parse and complete construction pass against the current alias table. Both paths replace the
first `%%` and every `%1`, with trailing-percent quoting. Typed callbacks retain physical groups,
while string templates and free input form one group. Prompt chaining and multi-answer `%2` retain
their existing prompt owner. The strict fixture proves exact construction,
parser, readback, and source-file plus Control channels. Reply and `-y` Enter-default behavior has
daemon and GPUI coverage, and slice 10n later adds raw-TUI unit and attached-client proof.

The nineteenth milestone reuses protocol v84 for `command-prompt`. The command accepts zero or one
template positional as a typed block or string while `-I`, `-p`, `-t`, and `-T` values remain
strings. Typed templates preserve their recursively constructed commands through submission.
String templates preserve raw source, substitute the answer, then parse and construct every
resulting command before effects. Frozen typed aliases and fresh string aliases match the pin.
Source paths and lines survive the string path. The strict 43-check fixture drives a real attached
client through template typing, construction precedence, aliases, placeholders, injection,
physical groups, source-file diagnostics, and exact Control frames.

The twentieth milestone reuses protocol v84 for `set-hook`. Without `-B`, only value position 1
accepts a typed block or string. With `-B`, every positional lexically accepts either type, while
option values remain strings. Without `-B`, hook names and extra positionals also remain strings.
zz still rejects `-B` because format-monitor runtime behavior remains unsupported. Every typed child constructs
before parent type, arity, or effects. Typed values normalize through built-in hook, custom `@`, and
named-option forwarding paths. Built-in hooks flatten physical groups during a second construction
pass; custom `@` typed values retain textual ` ;; ` groups. An empty or failing local append creates
an empty local array that shadows the inherited global hook. The strict 24-check fixture covers
replacement, empty-value, and local-inheritance ordering, quoted braces, `-R`, aliases, stored bindings,
`default-client-command`, source-file diagnostics, and exact Control framing.

The twenty-first milestone reuses protocol v84 for `display-menu`. Its parser walks repeated NAME,
KEY, and ACTION fields. A nonempty NAME consumes a string KEY and a string-or-typed ACTION before
resetting to NAME; an empty NAME is a separator that consumes no KEY or ACTION. All ten valued
flags remain strings. Typed children construct before parent type, arity, or effects. Stored
bindings print canonical child commands for typed actions and preserve quoted action strings.
Incomplete NAME and NAME-plus-KEY tails construct and reach daemon runtime. Runtime resolves the
current or `-c` target client before completeness, so an unattached command or initial Control
reports `no current client`; initial Control uses a flag-0 `%error` and exits 1. Once attached,
Control validates an incomplete group as `not enough arguments` before its overlay no-op and emits
an exact flag-1 `%error`; EOF after that frame exits 1. Interactive menu ordering is unchanged. The
strict three-step, 34-check fixture covers state transitions, type boundaries, all valued flags,
aliases, stored readback and preservation, client-before-completeness precedence, incomplete
runtime groups, source-file diagnostics, and PID-unique FIFO Control framing.

The twenty-second milestone reuses protocol v84 for `display-panes`. Its optional template accepts
a string or typed block while `-d` and `-t` values remain strings. Targetless routing resolves an
attached client before duration validation. The strict three-step fixture runs 22 internal checks
with zero differential channels. Custom selection-template execution remains parked under
`display-panes.command-template`.

The twenty-third milestone closes `choose-buffer` and `choose-tree` together. This is the deliberate
exception to the planned one-command 10j and 10k split: both commands use the same
commands-or-string callback, the same chooser-template preparation and selection executor, and one
26-check attached-client fixture. Splitting them would duplicate the production change and the
proof without producing an independent closure. Both commands accept zero or one string-or-typed
template. Typed templates freeze constructed aliases before opening; string templates parse
against the current alias table after selection. The daemon closes the chooser first, substitutes
the exact buffer name or tree target, and executes against the invoking client's live context.
Direct and stored commands now validate positional bounds before rejecting a recognized parked
capability. The tracker has no remaining command-specific `args-parse:` item.

The twenty-fourth milestone closes slice 10l as a source registration. A daemon-owned invariant
partitions all 68 pinned hook names into 27 explicit event producers, 37 generic
`after-<command>` producers derived from implemented command names, explicit-only `after-queue`,
and three active gaps: `pane-focus-in`, `pane-focus-out`, and `pane-set-clipboard`. A later pin audit
proved that ordinary queues do not produce `after-queue`; `set-hook -R` remains its explicit path. The test rejects
duplicate explicit names and overlap between produced and tracked names. `just compat-check`
requires the named daemon test and runs it through `--exact`. The slice changes no runtime
behavior, protocol, differential scenario, or step.

The twenty-fifth milestone closes slice 10m by pinning the full default-key structural partition
and matching bare key-only `bind-key` mutation. Structural matches stay separate from their runtime
owners. The twenty-sixth closes slice 10n: raw zz-tui retains and renders confirmation state, routes
confirmation input before normal shortcuts, and keeps the prompt active until the daemon clears it.
Focused tests cover exact key case, modifier reduction, Enter defaults, named nontext keys, key
lifecycle, paste, pointer input, pending replies, status placement, sidebar placement, and
reconnect state. Seven attached cases compare ordinary and Meta accept, reject, custom key case,
and Enter defaults against pinned tmux. A
one-byte pane sentinel proves no reply key leaks into terminal input.

The twenty-seventh milestone closes slice 10o. Raw zz-tui retains the daemon-published menu
descriptor, renders it after chooser and command-output bases, and routes keyboard input through the
same renderer-free resolver as GPUI before confirmation, global shortcuts, or pane delivery. The
attached fixture covers a title, shortcut-before-cancel behavior, disabled and separator skipping,
Escape, an unusable PageUp landing plus stay-open Enter, nonactivating paste, and a one-byte pane
sentinel. Focused resolver coverage pins exact raw-row-zero and all-disabled boundary behavior.
This closure stays bounded to descriptor consumption and shared keyboard ownership. The nine
broader menu behavior classes remain under `display-menu.behavior-fidelity`.

The twenty-eighth milestone closes slice 10p. Raw zz-tui now retains the daemon-published popup,
centers and clamps its terminal viewport, renders its border, title, styles, and cursor, and gives it
keyboard, paste, pointer, and scroll ownership before every underlay input path. External focus
updates client state, stays out of live popup terminals, and closes a dead `-k` popup on FocusOut,
matching the pin. Popup close
and replacement remove the synthetic viewport and renderer caches before repainting the underlay.
The attached fixture compares bordered update-in-place, bracketed paste and tracked mouse, retained
dead popups, live focus suppression, dead focus-close, and a pane sentinel against pinned tmux. The complete fixture also
passes under `LC_ALL=C`, including ACS-border frame proof. Broader resize, style,
context-menu, border-drag, popup-to-pane, and Kitty-image behavior remains under
`display-popup.behavior-fidelity`.

`clients.attach-context` closed as three bounded contracts. Sessions keep one internal cwd, and
attached source loading prefers it. Clients keep requested flags through attach, switch, detach,
and TUI reconnect. `resize-window -A` and `-a` now aggregate retained client geometry once and
freeze the result as manual sizing. None of the three slices changed the wire or snapshot schema.
Clients now add one bounded environment snapshot to the handshake. Fresh sessions, existing
attach, native attach, Control attach, and targeted switch apply the pinned `update-environment`,
wildcard, missing, empty, hidden, `-A`, `-e`, `-E`, and `-T` rules. Session values survive client
disconnect, future panes read updates, and existing processes keep their startup environment.
One shared retained client-fact record now serves list rows, ordinary and inserted commands, status
recipients, and `display-message`; the attached fixture covers Interactive and Control empty
behavior against the pin.
Per-window latest-client retention now drives `client-active` only on ownership changes, while
focus, theme, and positive Interactive size reports retain the pin's duplicate behavior. Hook
replay stays clientless, `hook_client` names the reporter, and the reporter's copied session,
window, and shared active pane supply ordinary target formats. Control clients originate none of
those report hooks but remain eligible for promotion after the latest client leaves.
Changed-resize post-geometry format timing remains a separate protocol-owned slice so the producer
milestone does not grow across the TUI message boundary.
`no-detach-on-destroy` now drives the per-client fallback after session destruction.
`active-pane` remains retained and reported without changing the shared selected pane.

Slice 10r closes local cold-start parse abort. The raw CLI gate validates canonical names,
built-in aliases, and prefixes for 83 implemented commands plus nine parked commands before
routing or spawn. Exact native attach tails and `-N` routes use the same gate. A spawned daemon
then prepares the full vector under one post-config alias snapshot. Its generation-owned one-shot
lease excludes startup reentry, makes contention sticky, commits on successful preparation or a
pipelined command, and shuts down only after returning a preparation error to its owner. The
stopping state rejects late registrations. Protocol and snapshot schemas stay unchanged.

Slice 10s closes nonconstant global-format discovery without changing runtime behavior. The single
198-entry production table derives 92 direct mux values, 32 daemon-delegated values, and 74 active
constant-placeholder gaps. A required exact daemon test proves every delegated name reaches the
production consumer.

Slice 10t closes `format:session_path`. Expansion reads the selected session's retained cwd at use
time and observes production `attach-session -c` changes. Five new `formats-values` steps prove two
creates, two explicit targets, filtered list output, and lexical `..`. Focused mux tests cover
missing retained state and the attach update. The 198-name source partition now contains 93 direct
values, 32 delegated values, and 73 live gaps.

Slice 10u closes `mux.command-group-argument-parse-abort` on 2026-08-28. Warm local Command-client
vectors now validate ordinary unaliased tmux grammar across the complete vector before any effect.
Runtime target and effect errors keep sequential queue ordering. Only vector index 0 when it is
exact unaliased `attach` or `attach-session` retains the private positional parser; later exact
spellings and aliases use the catalog. Control, remote `--host`, config and source replay, native zz
grammar, alias snapshots, and runtime rollback remain excluded. Six warm fixture probes and the
focused three-step scenario report zero differences. All 112 CLI tests, formatting, the all-feature
workspace suite excluding `zz-daemon`, the focused daemon test, and strict workspace clippy pass.
One parallel daemon run hit two unrelated failures that both pass alone. A sequential run passed
711 of 712 tests; its unrelated viewport-queue assertion also passed immediately alone. No
uninterrupted full-daemon result is claimed at this checkpoint.

Slice 10v closes `tracker.format-vocabulary-registration` with oracle schema 5. The source-backed
inventory records 31 literal `path:function` producer scopes, 153 scoped pairs, 108 unique names,
10 derived families, five propagation records, and 36 modifier tokens. The literal partition is 58
implemented pairs, 54 native pairs, and 41 active gaps. The derived partition is eight implemented
families and two active gaps. The modifier partition is 30 implemented tokens and six active gaps:
`w`, `I`, `L`, `O`, `V`, and `R`. Eight exact mux tests and three exact daemon tests guard the
registration. The slice changes no runtime format behavior, context-value semantics, option
consumers, protocol, snapshot, scenario, or accepted artifact.

`formats.context-producer-fidelity` closed on 2026-09-04, when the `set-hook -B` monitor subsystem
landed and produced its nine `notify_monitor_cb` names. `formats.modifier-fidelity` closed on
2026-09-02, and native typed producers remain accepted under
`formats.native-typed-context-producers` (`native`, accepted).

The resumed 2026-08-29 rerank first corrected the stale config parser-abort item. Existing parser
behavior already clears the file's command list on the first diagnostic, stops scanning, preserves
only earlier parse-time assignment effects, and suppresses later diagnostics. The corrected closure
is `config.parser-abort`; the residual `config.parser-edge-cases` group contains post-closing-quote
expansion plus passwd-backed bare and named-user lookup. Pinned tmux prefers nonempty server-global
`HOME`, then the current user's passwd entry, and reports a located syntax error when the required
lookup fails.

Slice 10w then closes `formats.repeat-modifier` locally. `R` splits at the first top-level comma,
recursively expands the value and count, accepts counts from 1 through 10,000, and matches the pin's
empty or replacement-failure behavior for invalid, missing, zero, negative, and oversized counts.
Escaped commas, nesting, byte-length, truncation, and post-transform order match. A deterministic
40,960,000-byte intermediate guard rejects nested amplification before allocation, replacing the
pin's elapsed-time budget. The default `P:` and `S:` rows prove the production path and do not leak
literal `R` syntax. The modifier partition is now 31 implemented tokens and five active gaps:
`I`, `L`, `O`, `V`, and `w`.

The accepted checkpoint for local 10w has 98 scenarios and 1,526 steps. Every ordinary row is
clean. The tracker has 91 active groups with 598 items and 105 closed records. Its status split is
49 open, 20 blocked, and 22 accepted, for 64.8% resolution (127 of 196 groups). The closure remains
uncommitted, no push is authorized, and no successor is selected until rerank.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The sizing milestone's expanded multi-client
attached fixture passes, and `compat/run.sh --check-summary` confirms the stored summary SHA-256
is
`f2aa32e0935e8a839c0abcd43da85e0f474d6c191421776847f7a464cc7257ff`.

Slice 10x closes `sessions.new-session-attach-cwd` locally. Existing `new-session -A -c` targets
now share the attach path's one-pass target-context expansion and cwd update. A nonnested headless
terminal-open failure retains that mutation. Clientless calls remain inert, permitted Control
clients attach, and nested Interactive, Control, and `-A -d` calls refuse before expansion,
retargeting, or mutation. Fresh creation and an `-A` miss retain an empty session cwd while the
initial pane keeps its donor or caller fallback. The ten-step `new-session-cwd` scenario and
focused mux and daemon tests cover the contract.

The accepted checkpoint for local 10x has 99 scenarios and 1,536 steps. Every ordinary row is
clean, the attached fixture passes, and the two registered GEO rows retain their exact tuples. The
tracker has 90 active groups with 596 items and 106 closed records. Its status split is 48 open, 20
blocked, and 22 accepted, for 65.3% resolution (128 of 196 groups). The closure remains
uncommitted, no push is authorized, and no successor is selected until rerank. The stored summary
SHA-256 is
`ed1422d318298b2fee9c31c160393cc2709b9d9137705e96c2632cc700cdcd01`.

Slice 10y closes `aliases.config-parse-unit` locally. Each config file prepares its alias-expanded
commands or stored preparation errors under one engine lock after applying that file's environment
assignments and before replay. Startup roots and top-level matched source batches finish
construction before batch replay. Nested sources obtain a fresh snapshot when their parent source
command runs. Stored failures retain source, physical-group, and replay-position metadata, and
their Control warning-versus-guard classification is frozen during construction. `source-file -n`
keeps its no-effect behavior and suppresses stored alias preparation errors. Four focused daemon
tests and the two-step `smoke/config-alias-parse-unit` differential cover the contract.

The accepted checkpoint for local 10y has 100 scenarios and 1,538 steps. Every ordinary row is
clean, the attached fixture passes, and the two registered GEO rows retain their exact tuples. The
tracker has 89 active groups with 595 items and 107 closed records. Its status split is 47 open, 20
blocked, and 22 accepted, for 65.8% resolution (129 of 196 groups). At that checkpoint the closure
remained uncommitted, no push was authorized, and slice 10z was frozen under `mux.chain-parse-abort`. The
stored summary SHA-256 is
`8d53288c8050e5c8cf7f19e6c81687f91544877d32ea4de9f7d40ea2934736b7`.

Slice 10z closes `mux.chain-parse-abort` locally. Each config or source file applies permitted bare
assignments, expands aliases, and validates every command group before any command from that file
runs. The first construction failure keeps earlier bare assignments and drops every command effect
from that file. Parse-only input validates against the pre-file environment and commits no effects.
Startup roots and matched sibling files remain independent file units constructed in path order;
nested children receive fresh units and cannot suppress a parent's later physical groups. Runtime
target and effect errors remain sequential within their physical groups.

Control reports one located `%config-error` without a failed-command guard and delays construction
warnings until the sibling batch finishes replay. Verbose output retains completed groups and
successful alias-subparse traces before failure. Parser, mux, and daemon tests plus the clean
two-step `smoke/config-chain-parse-abort` differential cover the contract.

The same audit closes `hooks.queue`. Pinned tmux stores `after-queue` but ordinary queues do not
produce it; `set-hook -R after-queue` runs it once. The hook inventory now divides 68 names into 64
automatic producers, that explicit-only hook, and three active pane-event gaps.

Slice 10aa closes `formats.session-runtime/format:session_active`. `FormatClient` records no client,
an unattached client, or one attached session. The execution context keeps the raw invoking client
beside the current or explicitly selected target client. Clientless list and filter producers stay
empty while target-aware commands, status, deferred output, shell callbacks, buffer and capture
paths, overlays, Control subscriptions, and display-panes labels use their selected client. The
28-step `formats-values` row passes. Unit, source-file, `run-shell`, `if-shell`, per-client
snapshot, and attached-client fixture proofs show that `client_*` facts and `session_active` use
the same selected client. The tracker has 86 active groups with 592 items
and 110 closed records. Its status split is 44 open, 20 blocked, and 22 accepted, for 67.3%
resolution (132 of 196 groups). The closure remains uncommitted, no push is authorized, and the
accepted artifact covers 101 scenarios and 1,550 steps with attached-client `PASS` and SHA-256
`bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`.

Slice 10ab closes `formats.window-activity-time/format:window_activity`. Windows store an optional
Unix-second timestamp beside the logical MRU counter. Creation, parsed nonempty pane output, and
pinned current-window transitions refresh both values. Same-window selection, pane selection, pane
creation, splits, and layout-only changes without output leave the timestamp unchanged. The
independent audit repaired the direct daemon `switch-client` path so it refreshes the engine clock
before selection. The 45-step `formats-values` row passes, and the 198-name partition now contains
95 direct mux values, 32 daemon-delegated values, and 71 active gaps. The tracker has 86 active
groups with 591 items and 111 closed records. Its status split remains 44 open, 20 blocked, and 22
accepted, for 67.5% resolution (133 of 197 groups). The accepted artifact covers 101 scenarios and
1,567 steps with attached-client `PASS` and SHA-256
`309aed0df108abd93e50f2073af7df5991d266c25e55dd266f0c8fc7f412ad72`.

Slice 10ac closes
`jobs.command-status-environment/semantic:shell-job-clean-environment`. Shell-form `run-shell` and
`if-shell` start from an empty process environment, then apply global and resolved-session values.
Status `#()` applies global values only. Hidden and unset values disappear, explicit missing
targets become sessionless, and visible modeled `TMUX_PANE` survives without synthesis. Startup
command jobs preserve modeled TERM-family values. Completed startup forces the pinned terminal
identity, and the private tmux launcher uses modeled PATH. The three-step differential runs eight
assertions per engine, while the attached fixture proves the global-only status path.

Slice 10ad closes
`tracker.semantic-coverage/semantic:tracker-option-consumer-registration`. The unchanged 105-name
roster moved from the option definitions to `command::TMUX_OPTION_CONSUMERS`, with `BEHAVES`
preserved as its public alias. An exact guard proves the 180 pinned options equal those 105
consumers plus the 75 live option gaps, with no overlap, and confirms the tracker closure.
`copy-mode-mark-style` records status option-variable consumption only, not visual mark rendering.
The compatibility gate passes 445 mux tests plus three daemon inventory tests. Full workspace tests
and clippy, formatting, diff, tracker, and checked-summary checks pass.

Slice 10ae closes
`options.option-name-format-coverage/semantic:option-name-format-coverage`. Generic lookup now
precedes format-table, command-item, and environment values across the 105-name roster: 13 server,
42 session, 40 window, and 10 pane consumers. Exact names and legacy aliases follow selected
targets, inheritance, attached fallback, active children, and `S`, `W`, and `P` loops. Command
prefixes do not match.

Flags emit `0` or `1`; other types retain their tmux spelling. `command-alias`, `status-format`, and
`update-environment` support whole-array and indexed lookup with numeric-before-named order,
leading-zero normalization, empty invalid results, and whole-array local shadowing. Mux formats read
live state. Direct daemon producers call the same resolver, while detached status shares one
all-scope snapshot across a refresh batch. Missing-target `run-shell -C` and `if-shell -F` read
global options while inserted work keeps the caller context.

The focused 60-step differential has zero topology, geometry, format, output, or warning
differences, and the attached status probe passes. Exhaustive mux and daemon coverage includes the
roster, arrays, targets, loops, direct producers, and detached refresh. No protocol, wire snapshot,
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
the daemon. Startup gives that base first priority, carries it through nested relative sources and
literal metacharacter paths, then clears it before runtime source selection on success or error. A
direct daemon launch has no bootstrap base. The isolated startup-client-cwd differential passes
exactly on both engines without a public protocol change.

The full eight-case startup diagnostic reached `control-mode.exit-pane-output`: zz could drain
queued shell-prompt `%output` after a flags-0 guard and before `%exit`, while ten equivalent pinned
probes emitted none. Slice 10ai now discards pending and later pane output after EOF or blank Return
while preserving admitted command responses, non-pane Control records, and one final exit.

Slice 10ah closed `control-mode.kill-server-response-order`, and slice 10ai closes the independent
pane-output discard contract. The 10ai review caught and repaired early EOF admitting a second
buffered command before integration. The second Wave 2 chunk closes `jobs.shell-job-cwd`. Shell-form
`run-shell` and `if-shell` choose cwd from literal `-c`, startup client, unattached provenance
client, explicit target session, attached invoking-client session, HOME, then root. Positive-delay
jobs freeze that choice before the timer and retain launch-time existence fallback. Status `#()`
uses the attached session path. Attached clients keep independent command caches, while unattached
query clients share entries by effective cwd. The three-step differential
completes eight checks per engine with no differing channel. The attached fixture proves status cwd
and covers 24 Interactive and Control `run-shell` and `if-shell` cases across valid, missing, and
omitted targets. Immediate background `run-shell` ordering remains later and hard.

The third Wave 2 chunk closes `keys.literal-delete-identity`. Raw DEL, caret plus DEL, and textual
`0x7f` retain separate stored identities and pinned rendering across bindings and key options.
Live prefix and configured-backspace capture proves literal DEL transport for the raw and
textual-hex values and no transport for the caret-modified value. The strict-key differential
completes 196 fixture checks per engine.

The post-10ag ledger has 87 active groups with 594 items and 116 closed records: 45 open, 20
blocked, and 22 accepted, for 68.0% resolution (138 of 203 groups). The persisted accepted slice 10ag
artifact covers 103 scenarios and 1,630 steps with attached-client `PASS`, exactly two approved GEO
rows, every other channel clean, and SHA-256
`46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`. Full zz validation passes
653 unit tests plus 113 CLI tests. Serialized daemon validation passes 736 unit tests plus two active
agent integrations; one soak remains ignored. The full workspace excluding the daemon, full
workspace clippy, and `cargo fmt --check` pass. Slices 10w through 10ag form the authorized
2026-08-29 checkpoint. Slice 10ah takes Control kill-server response order;
pane-output discard follows as 10ai.
The 10l and 10m milestones add no differential step. Slice 10n extends the attached fixture with seven
confirmation cases and a pane sentinel without adding a scenario row. Slice 10o adds the bounded menu
cases without adding a scenario row.
Slice 10p adds three popup cases and a pane sentinel to the attached fixture without adding a
scenario row. A post-close audit hardened the frame and focus assertions
and ran the complete fixture successfully under `LC_ALL=C` without changing the stored corpus.
Slice 10q adds a mixed flagged and unflagged client-destruction case to the attached fixture without
adding a scenario row.
Slice 10r adds 11 cold-socket probes per engine for implemented and parked syntax, exact native
attach tails, and `-N` routing. It adds no differential row or step. Slice 10s adds only required
source-registration tests. Their historical checkpoint remains 98
scenarios and 1,517 steps at SHA-256
`9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.
Slice 10t grows the `formats-values` row from 13 to 18 steps without adding a scenario.
Slice 10u runs six warm fixture probes through the existing focused three-step scenario with zero
differences, so the canonical scenario count, step count, attached result, and digest stay unchanged.
The attached-client result is `PASS`, every ordinary row is clean, and only the two registered GEO
rows remain. The historical 10i checkpoint remains 97 scenarios and 1,514 steps at SHA-256
`3b728eb8f0d30cae1bf1fe9c09100188279aaf8c80c0b33b30cd15b617f75d70`.
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
The combined 10j/10k artifact comes from a fresh full strict suite run. Its chooser row contributes
three harness steps and 26 internal checks with zero TOPO, GEO, FMT, OUT, or WARN differences. The
historical 10i display-panes row contributed three harness steps and 22 internal checks with the
same clean channels.
The two positional-bound scenarios prove canonical and alias diagnostics, the
first-positional flag boundary, target-error precedence, and effect suppression. The expanded
maximum fixture covers 71 generic-CLI-routed canonical names and 62 aliases; Rust coverage
also includes the exact attach engine path and stored commands. The focused three-step
`command-flag-errors` run is clean on zz and the pin with 516 byte-compared probes inside the
canonical suite. The focused three-step `args-parse-if-shell` row runs 12 internal source-file,
Control, alias, stored-command, and branch-selection checks.
The focused three-step `args-parse-run-shell` row runs 21 internal checks for lexical type,
combined and boundary flags, aliases, foreground and background execution, Control, and stored
command preservation.
The focused three-step `args-parse-set-option` row runs 21 internal checks across both set-option
commands for lexical type, recursive group printing, format order, aliases, source-file, Control,
and rejected-state preservation.
The focused three-step `args-parse-bind-key` row runs 17 internal checks for typed option and key
positions, exact typed and string tails, aliases, boundary flags, stored-child preservation,
bare-key metadata mutation, absent-key table creation, and physical-group execution through a real
attached client.
The focused three-step `args-parse-confirm-before` row runs 19 internal checks for recursive typed
and string construction, string-only option values, nested readback, per-path alias
budgets, self-recursion safety, physical groups, target and child diagnostics, exact source-file
and Control channels, and rejected-binding preservation. Nested bind and confirm failures are
preflight parse errors. The constructed confirm callback remains frozen through execution; stored
bindings and hooks likewise perform no execution-time user-alias lookup. Raw zz-tui reply handling
later closed under `clients.tui-confirm-before-overlay`; menu descriptor consumption later closed
under `clients.tui-display-menu-overlay`; popup state and input ownership later closed under
`clients.tui-display-popup-overlay`. Eager
whole-file source construction plus the broader replay-channel placement difference also remain.
Late focused regressions also prove that typed `if-shell`, `run-shell`, and structured
`command-prompt` callbacks stop the failed physical group and continue later physical lines, while
string callbacks remain one group. Structured prompt substitution preserves leaf-argument
boundaries against quote or semicolon injection. Raw string templates substitute before parsing
and complete construction. Both paths replace the first `%%` and every `%1`, with trailing-percent
quoting.
Typed `display-menu` actions lose their structural wrapper before the fresh selection parse, while
quoted brace strings remain literal. The later item-rule closure keeps incomplete NAME and
NAME-plus-KEY groups at daemon runtime. Runtime resolves the current or `-c` target client before
completeness. Initial Control returns a flag-0 `no current client` error. Attached Control validates
an incomplete group as `not enough arguments` before its overlay no-op, returns a flag-1 `%error`,
and exits 1 after EOF. Prompt chaining and multi-answer `%2` retain their existing prompt owner.
The focused three-step `args-parse-command-prompt` row drives a real attached client and runs 43
internal checks across template types, recursive construction precedence, alias timing,
substitution, injection resistance, physical groups, source-file diagnostics, and exact Control
frames. Both sides finish with `ARGS_PARSE_COMMAND_PROMPT=clean:43`.
The focused three-step `args-parse-set-hook` row runs 24 internal checks across lexical types,
child-before-parent precedence, canonical readback, preexisting aliases, physical groups,
built-in versus custom storage, replacement, empty-value, and local-inheritance ordering, `-R`, named-option
forwarding, stored bindings, and exact Control framing. Both sides finish with
`ARGS_PARSE_SET_HOOK=clean:24`. The fixture leaves eager whole-file construction, multiline
inner-source placement, `-B` monitor semantics, and broader replay placement with their existing
owners. Slice 10y later closes the same-file alias snapshot.
The focused three-step `args-parse-display-menu` row runs 34 internal checks across the repeated
NAME, KEY, and ACTION state, empty-name separators, typed and quoted actions, all ten string-only
valued flags, child construction precedence, canonical, built-in alias, prefix, and preexisting
user-alias paths, stored binding readback and preservation, incomplete runtime groups, source-file
diagnostics, and exact initial flag-0 plus attached flag-1 Control frames through a PID-unique FIFO.
Both sides finish with `ARGS_PARSE_DISPLAY_MENU=clean:34` and zero differences. Attached rendering
and shared keyboard ownership now close for raw zz-tui. Daemon-side geometry construction, action
context and errors, mouse policy, paste-close ordering, queue ordering, rendered width, resize
lifecycle, shortcut display and grammar, and style refresh remain under
`display-menu.behavior-fidelity`. Eager whole-source construction and generic alias recursion
retain their owners. Slice 10y later closes the same-file alias snapshot.
The focused three-step `args-parse-display-panes` row runs 22 internal checks across the optional
string-or-typed template, string-only `-d` and `-t` values, child-before-option-type and arity validation,
canonical, built-in alias, prefix, and preexisting user-alias readback, targetless client routing,
duration precedence, source-file, and direct Command-client runtime paths. Both sides finish with
`ARGS_PARSE_DISPLAY_PANES=clean:22` and zero TOPO, GEO, FMT, OUT, or WARN differences. Custom
template execution remains parked because mux runtime rejects a positional value instead of
substituting the selected `%pane` for `%%%` and executing with the original queue state. Tmux uses
`select-pane -t "%%%"` when the template is omitted; queue blocking and presentation keep their
existing owners.
The focused three-step `args-parse-choosers` row runs 26 internal checks across `choose-buffer` and
`choose-tree`. It covers string-only option values, typed and string alias timing, exact `%1` and
`%%` substitution with trailing-percent quoting, live invoking-client context, tree target
spellings, stale and empty buffers, chooser-close ordering, uppercase attached errors, and direct
plus stored arity precedence over recognized parked flags. Both sides finish with
`ARGS_PARSE_CHOOSERS=clean:26` and zero TOPO, GEO, FMT, OUT, or WARN differences. Broader chooser
flags, presentation, eager whole-source construction, generic alias recursion, and raw-TUI overlay
parity retain their owners. Slice 10y later closes the same-file alias snapshot.

# Cohorts

| Phase | Tracker scope | Dependency | Exit proof |
|---|---|---|---|
| Alert | Closed alert groups | Complete | Focused daemon and terminal tests, pinned alert probes, one full debug attached-client fixture, tracker and knowledge updates |
| Client foundation | Session cwd, requested flags, sizing, environment, formats, and `clients.event-hooks` closed | Complete | One written oracle contract per slice, focused differential coverage, and one full debug attached-client fixture per milestone |
| Error contracts | Async copy-pipe, shared arity, shared flag diagnostics, nested `new-session` precedence, option-consumer registration, and option-name format coverage closed | Independent of Client foundation except where a proof names client context | Every changed claim gets a pinned differential or a focused test with a named tracker item, followed by one full debug attached-client fixture |
| Copy behavior | `copy-mode.action-fidelity`, `copy-mode.command-fidelity`, `keys.copy-mode-binding-fidelity`, `keys.copy-mode-unsupported-default-actions`, `keys.copy-mode-prompt-defaults` | Command fidelity requires `clients.interactive-refresh`; prompt-backed defaults also require `prompt.command-fidelity` | Source-owned action and binding inventories, attached key-path probes, and one full debug attached-client fixture |

These phases are navigation, not commit boundaries. One persistent goal and one milestone commit own
one bounded slice. Split a tracker group before implementation when its acceptance contract crosses
unrelated production paths. Do not merge slices to save a commit.

# Selected dependency-ordered tranche

The queue separates execution order from apparent ease. A blocked medium item does not jump ahead of
the hard state contract that makes its proof meaningful. A range such as `10a-10f` means one
milestone per letter unless a row records a shared-rule exception. The combined 10j/10k chooser
closure is that exception.

| Order | Slice | Current tracker ownership | Relative effort | Why it is bounded |
|---|---|---|---|---|
| 1 | Session cwd and attached `source-file` cwd | Closed under `clients.attach-session-cwd` on 2026-08-26 | Complete | One internal session-state path; no client-environment or format vocabulary |
| 2 | Requested client flags | Closed under `clients.attach-flags` on 2026-08-27 | Complete | One attach-state contract; establishes `ignore-size` |
| 3 | Largest and smallest client sizing | Closed under `clients.attach-sizing` on 2026-08-27 | Complete | One-shot component-wise aggregation, manual freeze, global `ignore-size` fallback, Control ceilings, and default fallback; no wire change |
| 4 | Client environment seeding and refresh | Closed under `clients.attach-environment` on 2026-08-27 | Complete | Protocol v82 plus one per-connection snapshot; exact and wildcard refresh semantics remain session scoped |
| 5 | Client format facts | Closed under `clients.context-formats` on 2026-08-27 | Complete | Protocol v83 plus one retained-fact contract across list, status, Control, inserted, and targeted contexts |
| 6 | Client lifecycle hook producers | Closed under `clients.event-hooks` on 2026-08-27 | Complete | Per-window latest ownership plus five report boundaries; no protocol change |
| 7 | Interactive refresh decision | `clients.interactive-refresh` | Hard decision gate | Either justify and adopt the cross-client mode contract or keep it parked and reclassify dependent copy claims |
| 8 | Async copy or pipe error delivery | Closed under `control-mode.async-copy-pipe-errors` on 2026-08-27 | Complete | Pinned source plus a delayed exit-7 Control probe prove silent delivery and copy-mode cancellation |
| 9a | Daemon invalid-flag runtime contract | Closed under `tracker.daemon-invalid-flag-runtime` on 2026-08-27 | Complete | Initial 24-command production-dispatch proof; the later shared parser superseded the partial roster |
| 9b | Initial positional maximums | Closed under `mux.positional-maximums` on 2026-08-27 | Complete | One catalog validator across the first eight commands, with mux and daemon production-boundary proof |
| 9c | Required positional minima | Closed under `mux.positional-minimums` on 2026-08-27 | Complete | One catalog sidecar and validator after flags but before targets and effects |
| 9d | Shared arity errors | Closed under `mux.command-arity-errors` on 2026-08-27 | Complete | All implemented finite upstream commands plus stored children, without absorbing inner callback grammar |
| 9e | Shared flag errors | Closed under `mux.command-flag-errors` on 2026-08-28 | Complete | One catalog parser across 83 canonical commands and 74 aliases; 516 differential probes cover exact diagnostics and value boundaries |
| 9f | Nested `new-session` error precedence | Closed under `mux.error-shapes` on 2026-08-28 | Complete | Separate client-lifecycle path with its own oracle proof |
| 10a | `if-shell` branch argument rule | Closed under `tracker.args-parse-if-shell` on 2026-08-28 | Complete | Protocol v84 preserves typed blocks; one command and one effective rule |
| 10b | `run-shell` command-mode argument rule | Closed under `tracker.args-parse-run-shell` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; one command and one effective rule |
| 10c | Shared set-option value argument rule | Closed under `tracker.args-parse-set-option` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; two commands and one effective rule |
| 10d | `bind-key` commands-or-string argument rule | Closed under `tracker.args-parse-bind-key` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; one command within a shared rule |
| 10e | `confirm-before` commands-or-string argument rule | Closed under `tracker.args-parse-confirm-before` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; recursive construction and exact channel proof stay separate from client reply rendering and whole-file source construction |
| 10f | `command-prompt` commands-or-string argument rule | Closed under `tracker.args-parse-command-prompt` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; one typed-or-string template plus deferred substitution, alias, source, and group boundaries |
| 10g | `set-hook` monitor-or-value argument rule | Closed under `tracker.args-parse-set-hook` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; lexical `-B` typing is closed while unsupported monitor runtime behavior retains its owner |
| 10h | `display-menu` repeating item argument rule | Closed under `tracker.args-parse-display-menu` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; data-dependent NAME, KEY, and ACTION typing closes without absorbing menu presentation or selected-action execution |
| 10i | `display-panes` commands-or-string argument rule | Closed under `tracker.args-parse-display-panes` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; parsing and client-routing precedence close while custom selection-template execution remains parked |
| 10j/10k | `choose-buffer` and `choose-tree` commands-or-string rule | Closed under `tracker.args-parse-choose-buffer` and `tracker.args-parse-choose-tree` on 2026-08-28 | Complete | One deliberate combined milestone for one callback rule, one chooser-template executor, and one attached 26-check proof |
| 10l | Hook-producer source registration | Closed under `tracker.hook-producer-partition` on 2026-08-28 | Complete | 27 explicit plus 37 derived producers reconcile with four tracked hook gaps; no runtime or differential change |
| 10m | Shared key structure and bare bind mutation | Closed under `tracker.key-binding-behavior` on 2026-08-28 | Complete | Exact structural counts stay distinct from runtime proof; bare key-only bind mutation now matches the pin |
| 10n | Raw TUI confirmation | Closed under `clients.tui-confirm-before-overlay` on 2026-08-28 | Complete | State, rendering, input capture, reply lifecycle, and seven attached cases close independently |
| 10o | Raw TUI menu | Closed under `clients.tui-display-menu-overlay` on 2026-08-28 | Complete | Daemon-published descriptor consumption, rendering order, shared keyboard ownership, and bounded attached cases close without absorbing broader menu fidelity |
| 10p | Raw TUI popup | Closed under `clients.tui-display-popup-overlay` on 2026-08-28 | Complete | Popup state, rendering, input ownership, cleanup, and three attached cases close without absorbing broader popup fidelity |
| 10q | Per-client no-detach-on-destroy fallback | Closed under `clients.no-detach-on-destroy` on 2026-08-28 | Complete | The configured primary remains shared, while only flagged clients use the bounded newest-session fallback |
| 10r | Local cold-start CLI parse abort | Closed under `mux.local-cli-autospawn-parse-abort` on 2026-08-28 | Complete | Static syntax covers 83 implemented and nine parked commands; exact attach, `-N`, post-config preparation, and one-shot generation ownership close before effects |
| 10s | Nonconstant format behavior partition | Closed under `tracker.nonconstant-format-behavior` on 2026-08-28 | Complete | The single 198-name source table derives 92 mux and 32 daemon behavior registrations against 74 live gaps; an exact daemon test proves its delegated consumers |
| 10t | Target session path format | Closed under `formats.session-path/format:session_path` on 2026-08-28 | Complete | Retained target-session cwd expands at use time; five differential steps and focused mux tests prove targeting, missing state, lexical state, and production attach updates |
| 10u | Warm local whole-vector argument preflight | Closed under `mux.command-group-argument-parse-abort` on 2026-08-28 | Complete | Six warm probes and one clean three-step scenario prove ordinary unaliased tmux grammar before effects; only vector-index-0 exact unaliased attach keeps the private positional parser |
| 10v | Format vocabulary source registration | Closed under `tracker.format-vocabulary-registration` on 2026-08-28 | Complete | Schema 5 classifies all literal and derived context producers plus all modifier tokens without changing runtime behavior |
| Registry correction | Config parser abort | Closed under `config.parser-abort` on 2026-08-29 | Complete | Existing first-diagnostic whole-file abort and assignment retention were already implemented and tested |
| 10w | Repeat format modifier | Closed under `formats.repeat-modifier` on 2026-08-29 | Committed in `562b950c` | Exact `R` semantics, a deterministic 40,960,000-byte intermediate guard, default `P:` and `S:` row proof, and a clean 16-step differential |
| 10x | New-session cwd edges | Closed under `sessions.new-session-attach-cwd` on 2026-08-29 | Committed in `562b950c` | Existing-attach one-pass cwd update, explicit-empty retained state, refusal ordering, and a clean ten-step differential |
| 10y | Config and source replay alias snapshot | Closed under `aliases.config-parse-unit` on 2026-08-29 | Committed in `562b950c` | One alias snapshot per parsed file, batch-before-replay construction, nested refresh, deferred error metadata, frozen Control classification, and a clean two-step differential |
| 10z | Config and source replay construction | Closed under `mux.chain-parse-abort` on 2026-08-29 | Committed in `562b950c` | File-unit construction before effects, parse-only validation, sibling and nested isolation, Control warning order, verbose traces, and a clean two-step differential |
| Registry correction | Explicit-only queue hook | Closed under `hooks.queue` on 2026-08-29 | Committed in `562b950c` | Ordinary queues never produce `after-queue`; explicit `set-hook -R` runs it once, leaving three pane-event producer gaps |
| 10aa | Session-active client context | Closed under `formats.session-runtime/format:session_active` on 2026-08-29 | Committed in `562b950c` | Three-state client backing, raw-invoker versus selected-target routing, selected-client facts audit, deferred output, and a clean 28-step `formats-values` row |
| 10ab | Window activity timestamp | Closed under `formats.window-activity-time/format:window_activity` on 2026-08-29 | Committed in `562b950c` | Distinct Unix-second state covers creation, parsed nonempty pane output, pinned current-window transitions, direct `switch-client` clock refresh, and a clean 45-step `formats-values` row |
| 10ac | Command and status job environment | Closed under `jobs.command-status-environment/semantic:shell-job-clean-environment` on 2026-08-29 | Committed in `562b950c` | Clean command and status children, modeled overlays and PATH, startup-aware TERM identity, a three-step differential, and attached global-only status proof; delayed callbacks, copy-pipe, popup jobs, and status cwd stay separate |
| 10ad | Option-consumer source registration | Closed under `tracker.semantic-coverage/semantic:tracker-option-consumer-registration` on 2026-08-29 | Committed in `562b950c` | The unchanged 105-name roster belongs to `command::TMUX_OPTION_CONSUMERS`; `BEHAVES` remains its public alias, and an exact guard proves the 180 = 105 consumers + 75 live gaps partition and tracker closure |
| 10ae | Option-name format coverage | Closed under `options.option-name-format-coverage/semantic:option-name-format-coverage` on 2026-08-29 | Committed in `562b950c` | Generic precedence, four scopes, inheritance, array values, selected and missing targets, loops, direct daemon producers, detached status sharing, a clean 60-step differential, and attached status proof |
| 10af | Positive-delay run-shell environment timing | Closed under `jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing` on 2026-08-29 | Committed in `562b950c` | Scheduling retains command, target, expanded arguments, and cwd; child launch reads global, original-session, terminal, startup, and cwd-fallback state; focused foreground daemon, twelve-check background differential, full corpus, and attached-client proof pass |
| 10ag | Startup initial-client cwd | Closed under `source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd` on 2026-08-29 | Committed in `562b950c` | Private cold-launch cwd provenance, startup-only lifetime, nested and literal-path selection, later runtime expiry, and an exact isolated differential |
| 10ah | Control kill-server response order | Closed under `control-mode.kill-server-response-order/semantic:control-mode-kill-server-response-order` on 2026-08-30 | Committed in `4800255d` | The invoking response is admitted before shutdown closes the Control mailbox |
| 10ai | Control exit pane-output discard | Closed under `control-mode.exit-pane-output/semantic:control-mode-exit-pane-output-discard` on 2026-08-30 | Complete | EOF or blank Return discards pending and later pane bytes while draining non-pane Control records and one final exit |
| Pending rerank | Immediate background run-shell ordering | `jobs.run-shell-immediate-background-environment` | Unranked | Match absent-delay and `-d 0` queue ordering without timing races |
| Wave 2 chunk | Shell-job cwd | Closed under `jobs.shell-job-cwd` on 2026-08-30 | Complete | The three-step differential completes eight checks per engine, and the attached fixture covers 24 client, command, and target-mode cases |
| Wave 2 chunk | Literal DEL identity | Closed under `keys.literal-delete-identity` on 2026-08-30 | Complete | Raw DEL, caret plus DEL, and textual `0x7f` retain distinct identities, pinned rendering, and literal prefix and backspace transport |
| Historical Wave 3 editor | Typed Control config diagnostics | `control-mode.diagnostic-typing/semantic:control-mode-typed-config-diagnostics` | Superseded brief | Carry diagnostic identity through protocol, daemon, and Control rendering without prose matching |
| Historical Wave 3 review | Context producer runtime fidelity | `formats.context-producer-fidelity` (`adopt`, open) | Superseded brief | Producer value fidelity remains separate from source registration |
| Historical Wave 3 review | Remaining modifier runtime fidelity | `formats.modifier-fidelity` (`adopt`, open) | Superseded brief | `I`, `L`, `O`, and `V` need their context models; `w` also needs style parsing, live width overrides, the 162-entry cache, host policy, and Unicode proof |
| Accepted partition | Native typed context producers | `formats.native-typed-context-producers` (`native`, accepted) | Complete | The 54 native literal pairs are registered without pretending they are tmux runtime gaps |
| Completed slice | Startup initial-client cwd | Closed under `source-file.startup-client-cwd` in slice 10ag | Committed in `562b950c` | Cold launch provenance expires after startup without a public protocol change |
| Completed dependency | Config and source replay alias snapshot | Closed under `aliases.config-parse-unit` in slice 10y | Committed in `562b950c` | One snapshot per parsed file is ready for eager validation |
| Historical Wave 3 editor | Copy action vocabulary inventory | `semantic:copy-mode-action-vocabulary` in `copy-mode.action-fidelity` | Superseded brief | Record and classify all 95 pinned actions before behavior changes |
| Pending rerank | Copy action behavior | The other six `copy-mode.action-fidelity` semantics, one category per slice | Unranked | Cursor, logical-line, goto, selection, jump/prompt, and copy effects stay independently provable |
| Pending rerank | Unsupported stock action bindings | `keys.copy-mode-unsupported-default-actions` | Unranked | Seven keys become honest only after their five actions exist |
| Pending rerank | Copy command fidelity | `copy-mode.command-fidelity` | Unranked | Requires the interactive-refresh decision |
| Pending rerank | Shared copy binding fidelity | `keys.copy-mode-binding-fidelity` | Unranked | Follows command fidelity; owns exactly 15 divergent command shapes |
| Pending rerank | Generic prompt command fidelity | `prompt.command-fidelity` | Unranked | Requires the interactive-refresh decision and remains broader than copy mode |
| Pending rerank | Prompt-backed copy defaults | `keys.copy-mode-prompt-defaults` | Unranked | Ten defaults land only after their generic prompt contract |

Slices 9a through 9f and 10a through 10ai are closed, along with the Wave 2 shell-job cwd and
literal DEL chunks.
Commit `562b950c` contains cumulative slices 10w through 10ag; `4800255d` closes 10ah. The current
ledger has 84 active groups with 586 items and 123 closed records: 42 open, 20 blocked, and 22
accepted, resolving 145 of 207 groups (70.0%). Wave 2 is 3 of 3 complete, with no active group
marked `next`. Under
`detach-on-destroy on`, only flagged clients use
the newest remaining session; under `no-detached`, all clients use an existing detached survivor,
and only flagged clients fall back to the newest attached session when no detached survivor exists.
Flagged and unflagged clients on one destroyed session must diverge, while no remaining session
still exits both. Direct `off`, `previous`, and `next` selection stays unchanged. The slice excludes
active-pane routing, detach execution, parent-HUP exit, resize-hook ordering, client cwd, and overlay
residue.

Slice 10r moved `semantic:local-cli-autospawn-parse-abort` into closed history as
`mux.local-cli-autospawn-parse-abort`. The static pre-spawn catalog covers canonical, built-in alias,
and prefix syntax for 83 implemented commands plus nine parked commands. Exact `attach` and
`attach-session` wrappers validate their tail, and `-N` routes cannot hand off or spawn after a
later syntax error. Arbitrary startup aliases cannot trigger autospawn, while canonical spellings
remain eligible for startup shadowing.

The daemon prepares every command under one post-config alias snapshot before the first effect. A
generation-owned one-shot lease excludes startup reentry, preserves sticky contention, commits on
successful preparation or a pipelined command, returns preparation errors before disconnect-driven
shutdown, and rejects registrations once stopping begins. Runtime target and effect errors keep
their queue semantics. At the 10r checkpoint, the old chain gap retained two active siblings: warm
unaliased generic command groups and config or source-file replay. Slice 10u later closed the warm
Command-client owner, leaving config and source replay separate.

Slice 10s moved `semantic:tracker-nonconstant-format-behavior` into closed history as
`tracker.nonconstant-format-behavior`. The 198-name global format table now derives 92 direct mux
values and 32 daemon-hook values from its production backings. Those 124 behavior registrations
plus the 74 live `format:` gaps form a complete disjoint partition. A required exact daemon test
seeds buffer, client, and session facts and resolves all 32 delegated names through
`DaemonFormatHooks`. The slice changes no runtime value and claims no context-specific value parity.
Slice 10v closes source registration for context formats and modifier syntax. Slice 10w closes
exact `R` repeat behavior and leaves `I`, `L`, `O`, `V`, and `w` under modifier fidelity. Runtime
context fidelity and option consumers remain separate under the pending owners above.

The full post-10s rerank first selected `formats.session-runtime`, then independent source and oracle
audits disproved the group's shared-client premise. Slice 10t closes the resulting
`formats.session-path` group. Pinned `session_path` reads the selected session's stored cwd at
expansion time. The five added differential steps cover two creates, two explicit targets, filtered
list output, and lexical `..`; mux tests cover absent retained state and production
`attach-session -c` updates. The source partition is now 93 direct values, 32 delegated values, and
73 live gaps.

Pinned `session_active` remains empty without a target or format client, `1` for a client attached
to that target, and `0` for an unattached client or one attached elsewhere. Its open owner requires
a producer-by-producer tri-state audit. Slice 10t also exposed two `new-session` cwd edges. At that
checkpoint, zz lacked the pinned existing-session `new-session -A -c` mutation, and fresh
`new-session -c ''` inherited a cwd instead of preserving an explicit empty value. The post-10t
rerank weighed those edges against startup initial-client cwd, config and source replay, open
context formats, option consumers, and daily attach work. Slice 10x later closes both cwd paths.

The post-10t rerank selected `semantic:command-group-argument-parse-abort` and split it into
`mux.command-group-argument-parse-abort`, leaving only
`semantic:config-source-group-parse-abort` in the later `mux.chain-parse-abort`. Delivered slice 10u
now prepares ordinary unaliased tmux grammar across a warm local Command-client vector before any
effect. A later flag, arity, option-value, or other argument-preparation error prevents the vector's
earlier effect, while target and effect errors retain sequential queue behavior. Only
vector-index-0 exact unaliased `attach` or `attach-session` keeps the private positional parser;
later exact spellings and aliases use the catalog. Control, remote `--host`, config and source
replay, native zz grammar, alias snapshots, and runtime rollback remain outside the slice.

Schema 5 now source-registers the 31 literal scopes, 153 scoped pairs, 108 unique names, 10 derived
families, five propagation records, and 36 modifier tokens. At the 10v checkpoint, the literal
partition was 58 implemented, 54 native, and 41 active gaps; the derived partition was eight
implemented and two active gaps; the modifier partition was 30 implemented and six active gaps.
Local slice 10w moves `R` into the implemented modifier roster, producing 31 implemented tokens and
five active gaps. Runtime context behavior and option-consumer work remain outside that closure.
Local slice 10x closes both new-session cwd edges. Local slice 10y closes one alias snapshot per
parsed config file, including startup roots, top-level source batches, nested refresh, deferred
errors, and Control diagnostic classification. Slice 10z closes file-unit construction before
effects, parse-only validation, sibling and nested isolation, Control ordering, and verbose traces.
The earlier `w` forecast is retired: pinned width
behavior includes leading hashes, style spans, malformed markup, controls, live overrides, a
162-entry cache, and the host `wcwidth` policy chosen by `--disable-utf8proc`, while zz uses
`unicode-width` 0.2.2. A later slice must pin those cases. Slice 10aa closes the three-state
`session_active` backing by keeping raw and selected target clients separate. Slice 10ab closes
`format:window_activity` with separate Unix-second state for window creation, parsed nonempty pane
output, and pinned current-window transitions. Slice 10ac closes clean shell-form `run-shell`,
shell-form `if-shell`, and status `#()` environments. Slice 10ad moves the unchanged 105-name
option-consumer roster to `command::TMUX_OPTION_CONSUMERS`, preserves the `BEHAVES` alias, and adds
an exact 180 = 105 consumers + 75 live gaps guard without runtime changes. Slice 10ae closes
generic option-name lookup across mux and daemon format producers. Slice 10af closes positive-delay
shell-form `run-shell` environment timing. Slice 10ag closes startup initial-client cwd. Slice 10ah
closes kill-server response order, and slice 10ai then closes Control exit pane-output discard.

# Multi-front Codex pipeline

The 2026-08-29 trial starts from `562b950c`. At that base, 36 of the 45 open groups declare no
prerequisite. Use one coordinator and three front agents when their production and proof paths do
not overlap. Fall back to one active slice with oracle, implementation, and review roles when a
change crosses shared command, daemon, protocol, or tracker paths.

## Trial fronts

| Front | Worktree | Branch | Chunk | Exclusive production and proof zone |
| --- | --- | --- | --- | --- |
| Control response | `/Users/demfabris/dev/zz-tmux-control` | `codex/tmux-control-10ah` | Slice 10ah: `control-mode.kill-server-response-order` | Daemon response admission, Control client exit, and focused Control CLI tests |
| Config parser | `/Users/demfabris/dev/zz-tmux-config` | `codex/tmux-config-edges` | `config.parser-edge-cases` | Config parser plus its dedicated grammar scenario and fixture |
| Key grammar | `/Users/demfabris/dev/zz-tmux-keys` | `codex/tmux-key-validation` | `keys.strict-validation` | Protocol key parser plus a dedicated key-validation scenario and fixture |

Integrate the Control front first. The config and key candidates may finish while it runs, but the
coordinator reranks the remaining registry before accepting either candidate. Slice 10ai stays
behind 10ah because it uses the same Control paths.

## Wave 2 fronts

Wave 2 started on 2026-08-30 with three frozen chunks and 65 unresolved groups. Slice 10ai,
shell-job cwd, and literal DEL identity are closed, leaving 62 unresolved groups and the wave at 3
of 3. Shell-job cwd passed its three-step differential and coordinator-owned attached-client proof.
The DEL candidate passed independent review after repairs for the two transport failures found by
live PTY review, then passed 196 fixture checks per engine.

| Front | Starting role | Worktree | Branch | Chunk | Exclusive production and proof zone |
| --- | --- | --- | --- | --- | --- |
| Control output | Complete | `/Users/demfabris/dev/zz-tmux-control-output` | `codex/tmux-control-10ai` | `control-mode.exit-pane-output` | Accepted after an early-EOF repair; coordinator integration is slice 10ai |
| Shell-job cwd | Complete | `/Users/demfabris/dev/zz-tmux-job-cwd` | `codex/tmux-job-cwd` | `jobs.shell-job-cwd` | Daemon command-job and status-job cwd selection, a clean three-step scenario, and 24 attached-client cases |
| DEL identity | Complete | `/Users/demfabris/dev/zz-tmux-key-del` | `codex/tmux-key-del` | `keys.literal-delete-identity` | Three DEL identities plus literal prefix and configured-backspace transport are closed |

Wave 2 integrated Control output, shell-job cwd, then DEL identity. The coordinator owns every
shared campaign artifact and runs aggregate gates. A front stops when its implementation needs
another front's file zone.

## Historical Wave 3 fronts

Wave 3 was frozen from its then-published `origin/main`. The append-only dispatch board later
superseded these labels and briefs. This table is historical; workers must claim the board's live
front instead of treating any row below as ready.

| Front | Role | Contract | Owned write zone |
| --- | --- | --- | --- |
| `W3-CONTROL-DIAGNOSTICS` | Superseded editor brief | Close `control-mode.diagnostic-typing/semantic:control-mode-typed-config-diagnostics` | Protocol message and hunt-claim tests; daemon config-diagnostic publication and focused tests; Control rendering and focused CLI tests; uniquely named diagnostic scenario and fixture |
| `W3-COPY-ACTIONS-1` | Superseded editor brief | Close only `semantic:copy-mode-action-vocabulary`; leave six behavior items active | Mux command inventory and manifest test, oracle generator and JSON, relevant tracker structural code, and compatibility check script |
| `W3-FORMATS-SPLIT` | Superseded read-only brief | Split `formats.context-producer-fidelity` and `formats.modifier-fidelity` into bounded future chunks | None; return findings in Issue #7 without editing or committing |

The frozen Control brief proposed appending typed config-diagnostic identity to the existing
source-file event and advancing protocol v84 to v85. It was not executed under this brief; v85 was
later assigned to the `F-ALIASES-MULTI-BODY` closure review. Any successor must select its version
from current source rather than this historical forecast. The proposed work routed daemon config
diagnostics through typed identity, rendered `%config-error` without prose matching, and added the
smallest focused differential. Source-read placement, completion numbers, guards, parser
environment, disconnect behavior, asynchronous output, and other clients stayed outside it.

The frozen Copy brief proposed registering the pin's 95 action names and the zz partition of 66
mapped and 29 missing actions. It was inventory only: no copy behavior, terminal runtime path,
runtime scenario, registry entry, or shared knowledge page belonged to that editor.

The frozen Formats brief was read-only because it examined paths owned by the Copy editor. It asked
for exact producer and modifier partitions, dependencies, write zones, and the smallest disjoint
successors without changing source or tracker state.

## Orchestrated Opus and Fable cycles, 2026-08-31 onward

Once the append-only dispatch board in issue #7 replaced the coordinator, the campaign moved to
orchestrated cycles run from one Claude Code session with its Workflow tool. Each cycle has the
same shape:

- Three Opus implementor lanes run in parallel worktrees cut from `origin/main`. Each lane owns a
  disjoint set of board zones and an ordered batch of registry groups, closes slugs by removing
  them from `compat/tmux-gaps.json` with a pin-derived proof, and pushes a `campaign/batch-*`
  branch. A lane skips a group after about ninety honest minutes and records why in the group
  reason, which is how contracts get re-scoped.
- One Fable reviewer runs behind each lane as soon as it pushes: contract audit per closed or
  relocated slug, proof suites re-run at the branch tip, oracle spot-checks on throwaway pinned
  servers, test honesty, invariants. Its verdict (approve, approve-with-fixes, reject) and its
  confirmed defects bind the gate.
- One Fable integration gate runs alone afterwards, one lane at a time: rebase onto `origin/main`,
  apply the reviewer's must-fixes with the failing probe re-run as proof, full workspace tests and
  clippy, the delta corpus sharded eight ways over disjoint scenario sets with
  `smoke/source-replay-diagnostics` run solo, tracker and summary checks, push main, ledger the
  lane's lock front on the board, then recompute `TMUX_COMPAT_TRACKER.md` and run TRIAGE.

The orchestrator holds one lock front per lane for the cycle, settles the board when the gate
finishes, and mints the next cycle's locks and batches from a fresh registry census. The board
refuses two claims in one zone even for the same holder, so a lane claims one lock front and the
gate ledgers every front it moots at integration time.

| Cycle | Lanes | Merges | Agreed-scope meter |
| --- | --- | --- | --- |
| 1, 2026-08-31 | daemon basket, copy-mode action inventory | `a5b924ec`, `6624f042` | 1.6% to 4.6% |
| 2, 2026-09-01 | mux formats and geometry, daemon display and buffers (protocol v89), park dispositions | `9b4867ab`, `9a4129c3`, `6904747f` | 4.6% to 44.4% |
| 3, 2026-09-01 | format loops, daemon byte loaders and menu layout (protocol v90), registry hygiene | `a3562a34`, `1d4ab6b8`, `039c47b7` | 44.4% to 46.1% |
| 4, 2026-09-01 | format budgets and cell metrics, copy-mode key vocabulary, client exit actions (protocol v91) | `747acb39`, `ad539f4c`, `21ef482c` | 46.1% to 54.9% |
| 10, 2026-09-02 | Control command worker and `split-window -W`, copy-mode search and prompt bindings, the popup pointer route (protocol v96) | `fd19cce1`, `cd03bb8d`, `9ddeae0f` | 90.5% to 97.4% |
| 11, 2026-09-04 | the `set-hook -B` monitor subsystem and `display-message -v`, the copy-mode mode-keys tail and the first five chooser keys (protocol v97) | `89f36ac`, `3eda6ed` | 97.4% to 99.0% |
| 12, 2026-09-04 | the rest of `mode_tree_key`'s chooser vocabulary, the client's environment and command bytes on `RawText` (protocol v98) | `12b4776`, `595616b` | 99.0% to 99.3% |
| 13, 2026-09-04 | an attached pane's pty following the layout cell it reports, the three byte-clean consumers with the first `CLIENT_UTF8` output sanitizer (no bump, protocol stays v98) | `fd2e790`, `37e8df0` | 99.3% to 100.0% |

Cycles 5 through 9 are omitted here rather than reconstructed; their merges and meter moves live in
`compat/orchestration/CAMPAIGN-LOG.md`. The lane count dropped from three to two on 2026-09-03 when
every agent in the loop became Opus 5 at `xhigh`.

The reviewer stage earned its cost every cycle: it caught a menu width rule that ignored the
pin's title seed, four test expectations orphaned by a lane's final commit, a Control-client
divergence disclosed only in a throwaway report, a misattributed pin-derivation comment, and, in
cycle 13, an output sanitizer gated on the client's kind instead of on the sink the pin prints
through, which silently mangled `capture-pane -p` for every client shape.
Worker prompts now carry the protocol bump recipe, the relocation grammar for explicit native
decisions, the load-flake list, and the rule that proofs count only when re-run at the tip.

The current pause state, the ready next-cycle script, and the machine-move checklist live in
`compat/orchestration/HANDOFF.md` at the repository root, beside `CAMPAIGN-LOG.md` and the script.

## Ownership and handoff

The coordinator alone edits `TMUX_COMPAT_TRACKER.md`, `compat/tmux-gaps.json`, the generated gap
report, shared OKF pages, `compat/results/summary.md`, and shared attached or startup diagnostic
scripts. Each front owns only its listed code, focused tests, and uniquely named scenario files. A
front stops and reports the overlap if its proof needs a coordinator-owned or another front's path.

Each front probes the pinned tmux build, freezes its acceptance contract, implements the smallest
coherent change, runs focused proof, and creates a candidate commit. The coordinator reads and
reviews each complete candidate, applies accepted changes to the campaign branch, updates shared
artifacts, runs the integration gates, and creates the campaign milestone commit. Candidate branch
commits are transport for review; the campaign commit remains the recorded milestone.

Focused Rust tests and isolated scenarios may run in parallel from the worktrees. Keep each
worktree's default `target` and `compat/.cache` directories. Serialize cache repair, oracle writes,
tracker generation, the full corpus, attached-client diagnostics, startup diagnostics, full
workspace tests, and clippy. The full corpus also uses the fixed `/tmp/zz-c1-history` path.

After the trial, record merge conflicts, cross-front file requests, test interference, review
repairs, abandoned work, and closures delivered to the campaign branch. Keep this model only if it
raises completed, reviewed work without weakening the proof or making integration the bottleneck.

## Campaign reporting

Freeze each wave's group IDs before implementation. Report the wave and live registry with these
fields:

| Signal | Rule |
| --- | --- |
| Fixed cohort completion | Closed frozen chunks divided by frozen chunks |
| Discoveries | Residual groups registered while closing the cohort |
| Unresolved movement | Open plus blocked groups before and after the cohort |
| Practical exit gate | Current gate result and accepted differential evidence |
| Ledger settlement | Closed history plus accepted active groups divided by all known groups |

Use fixed cohort completion, discoveries, unresolved movement, and the practical exit gate as the
campaign headline. Keep ledger settlement as a secondary registry diagnostic because each group has
equal weight and new discoveries increase its denominator. Recompute every field from the live
registry and accepted artifacts when the coordinator closes a wave.

# Validation ladder

Run the cheapest proof that can fail the current edit:

1. During implementation, run focused Rust tests and the scenario or attached probe for the changed
   behavior.
2. At slice close, build the debug binary and run the full attached-client fixture against the
   pinned tmux oracle. Treat a skip or reduced scenario count as a failure.
3. At a campaign checkpoint, run `just compat --strict-geometry --attached-client`, regenerate the
   canonical summary, and run `compat/run.sh --check-summary` as a separate check.

Use campaign checkpoints after the Alert cohort, after two more completed slices, and at the
practical exit gate. Run one earlier if a change invalidates the stored checkpoint. Do not run
release builds for compatibility work.

# Discovery rule

The oracle agent records new gaps in `compat/tmux-gaps.json`. A discovery joins the active slice
only when it uses the same production path, needs no protocol or schema change, and fits the slice's
existing proof. A discovery that invalidates the slice's claimed behavior blocks closure. All other
findings wait for a later cohort.

Freeze the acceptance contract after the oracle and implementation agents agree on it. Review can
reject the implementation or proof, but it cannot expand the slice with unrelated cleanup.

# Goal boundary

By default, create one persistent Codex goal per slice and name its exact tracker items and exit
proof. When Fabrico explicitly asks for the whole campaign to continue unattended, one campaign
goal may span the practical exit gate. The slice boundary does not change: freeze, prove, review,
document, and commit one milestone before starting the next one.

# Milestone commits and worktree

Commit each slice after code review, proof, tracker generation, and OKF validation pass. Stage exact
paths or hunks because the shared checkout may contain unrelated work. Ask before the first commit
unless Fabrico has already authorized continuous campaign commits. Do not push unless he asks.

Resolve the live checkout, branch, and `$HOME/dev/zz-tmux-compat` worktree path with read-only checks
first. If no campaign worktree exists, create one only from the verified campaign base and only when
the shared checkout cannot safely host the slice. Inspect and reuse any existing path or branch;
never overwrite it. Leave unrelated shared-checkout edits intact.

# Practical exit gate

The campaign reaches compatible-enough status when all of these hold:

- Daily session, window, pane, config, plugin, Control, and attached-TUI workflows have no known
  silent semantic mismatch.
- The config and plugin corpus runs with no unexpected skip.
- The current strict differential and attached-client fixture pass at their full scenario counts.
- The canonical summary describes the current corpus rather than an older scenario set.
- Every remaining gap has an explicit `native`, `park`, or `never` decision, or produces a loud error
  that cannot corrupt state.

The tracker can retain long-tail work after this gate. It cannot retain an unclassified daily-use
surprise.

# Historical Wave 3 bootstrap prompt

This superseded prompt is retained as the launch record for Wave 3. Do not use it for a current
dispatch-board claim.

```text
Continue the tmux compatibility campaign from the published origin/main base recorded in GitHub
Issue #7. Read AGENTS.md, TMUX_COMPAT_TRACKER.md, knowledge/index.md, and
knowledge/playbooks/tmux-compat-cohorts.md first. Wave 2 is closed 3 of 3 with no new residual:
62 unresolved groups remain, and the secondary ledger is 144 of 206 groups (69.9%). The accepted
artifact covers 105 scenarios and 1,675 steps with attached-client PASS, exactly two approved GEO
rows, every other channel clean, and SHA-256
a1e4ca86326006c5f06c77859219772b97fe7e6ac86dd703b127fced4ca0cd7e.

Before editing, claim exactly one READY Wave 3 front in Issue #7 and record your machine, branch,
worktree, and exact base SHA. Do not claim an occupied front. Use a dedicated worktree. Stop and
report if the work needs a path owned by another front or the coordinator.

W3-CONTROL-DIAGNOSTICS is the protocol, daemon-config-publication, Control-rendering, focused CLI,
and uniquely named scenario front. Close only
control-mode.diagnostic-typing/semantic:control-mode-typed-config-diagnostics.

W3-COPY-ACTIONS-1 is the mux and oracle structural-inventory front. Register all 95 pinned copy
actions and the exact 66 mapped plus 29 missing partition. Close only
semantic:copy-mode-action-vocabulary. Do not implement copy behavior or edit terminal runtime paths.

W3-FORMATS-SPLIT is read-only. Return the exact split, dependencies, ownership zones, and smallest
future chunks for formats.context-producer-fidelity and formats.modifier-fidelity. Edit no file and
create no commit.

The coordinator alone edits compat/tmux-gaps.json, knowledge/tmux/gaps.md,
TMUX_COMPAT_TRACKER.md, shared knowledge pages, compat/results/summary.md, and Issue #7 state.
Editors run focused proof and commit one candidate on their branch. Do not push to main or integrate
another front.
```
