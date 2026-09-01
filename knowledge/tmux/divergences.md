---
type: Reference
title: tmux divergence matrix
description: "Dated rationale and source evidence for measured tmux divergences, including command, flag, behavior, option, format, hook, and protocol differences."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, divergences, gaps, reference]
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-09-01
last_updated_by: Codex
---

# Overview

This page preserves rationale and source evidence from the compatibility sweeps against tmux commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. Four independent passes produced the original
2026-08-16 record; later dated paragraphs capture follow-up measurements. The
[compatibility philosophy](/tmux/tmux-compat.md) defines the contract, and the
[superset roadmap](/designs/tmux-superset-roadmap.md) records delivery sequence.

`compat/tmux-gaps.json` owns live TODOs, classifications, and status.
Read the generated [gap report](/tmux/gaps.md) for the current inventory. Treat every count and
roster below as a dated measurement. Add or close work in the registry, then regenerate the report;
keep this page for the detailed reasoning that a compact tracker row cannot carry.

**State anchor:** the "implemented surface" section reflects
[PR #4](https://github.com/demfabris/zz/pull/4) (`fix/tmux-compat-hunt-v2`, merged 2026-08-16
as `53b523e`) plus the phase 3d layout-string and phase 4a–4f-2 source dated 2026-08-17. PR #4 corrected the
hunt-claim regressions, including two implemented backwards
(`new-window -t` bare-target order, positional targets on the kill commands) plus the
`kill-session -C`, `new-session -A`, `resize-pane` attached-adjustment, `send-keys -H`/`-l`,
`copy-mode -du`, window-step-error, and boolean-case fixes.

The counts and flag ledger were refreshed against the live source after Waves 2a and 2b on
2026-08-20. The pre-wave catalog held 159 unsupported pairs across 38 tmux commands; those
waves removed 30 pairs, Wave B's read-only slice removed `attach-session -r` on
2026-08-21, Wave C run 2 removed `send-prefix -2` the same day, and Wave D run 1 removed
`copy-mode -H` plus `-K`/`-N` on both choosers on 2026-08-22, Wave D run 3 removed
`display-message -C`/`-d` the same day, and Wave D's final run removed the seven
`command-prompt` mode flags, leaving 113 across 29. The 2026-08-22 alias tranche then implemented
`new-window -b` and `unbind-key -a/-q`, then the filtered `kill-session -a`, `kill-window -a`, and
`kill-pane -a` forms, leaving 107 across 26. Implementing `resize-window` with client-derived
`-A`/`-a` still loud brought the ledger to 109 across 27; `display-panes -N/-t` and `join-pane -l`
then reduced it to 106 across 25. Pane-local `-e` and empty-pane `-E` on `new-window` and
`split-window` removed four more pairs, and `last-pane -d/-e` removed two more. Four micro flags,
three `list-keys` selectors, and creation-time `new-session -e/-E` removed the next nine pairs.
`set-buffer -n`, `source-file -F`, `split-window -Z`, and `break-pane -a/-b` removed five more;
adding `move-pane -l` removed one more, leaving 85 across 23. The zz-only `split-picker` contributes another 19 markers to a raw catalog
grep and is deliberately excluded from tmux compatibility counts. Later bounded slices brought the
live ledger to 75; closing `attach-session -f` and `new-session -f` on 2026-08-27 leaves 73, and
closing `resize-window -A/-a` later that day leaves 71.

The doctrine requires an explicit decision for every silent difference because tmux syntax must
keep tmux meaning or fail loudly. Loud refusals can remain accepted exclusions. The registry records
that decision and whether the difference still blocks a supported workload.

# Recognized but unimplemented commands: 2026-08-22 snapshot

Counted 2026-08-20 against the pin's `cmd_table[]` (92 entries, 78 with an alias, 170
spellings). zz's shared catalog (`crates/zz-protocol/src/catalog.rs`) holds 102 verbs: 19
zz-native and 83 tmux. The split between `COMMAND_SPECS` and `DAEMON_COMMAND_SPECS` is ownership,
not discoverability: both feed resolution, `list-commands`, completion, and stored-command
rendering. `MuxEngine` runs 59 of the tmux verbs; the daemon
intercepts `list-clients`, `refresh-client`, `show-messages`, and `switch-client` ahead of it. Every
pin alias resolves to the same command it does in tmux. Nine of tmux's 92 commands have no handler
(`UNIMPLEMENTED_TMUX_COMMAND_TABLE`, 13 spellings): they resolve as recognized-unimplemented, answer
`unsupported command: <name>` rc 1, count as skips in config reports, and stay absent from
`list-commands` so feature probes take their fallback path. No pinned spelling resolves as unknown.
Three deliberate groups:

## Exec commands (doctrine revised by phase 5, 2026-08-18)

The old "config must never execute programs" doctrine died with phase 5's verified
facts: `#()` and creation-command positionals already spawned `/bin/sh` ungated, so
gating only the exec commands was theater. The user's own `mux.conf` is trusted like
`.bashrc`; the consent gate moves to the foreign-config *import flow*. `run-shell` /
`run` and `if-shell` / `if` now execute for real (wave 5a-2, `9f55f87`) with
pin-parity semantics — see the drop-in plan's phase-5 section for the accepted
divergences (overlay output routing, inherited job environment, job cap backstop).
`wait-for` / `wait` and `pipe-pane` / `pipep` shipped in wave 5b (`2d7a655`)
with pin-parity semantics (sticky signals, lock leak-on-disconnect, pre-parse
output tap, pipe survives respawn); Interactive clients cannot park on blocking
waits — they get the pin's clientless errors (scripts are faithful).

`set-hook` / `show-hooks` shipped in wave 5c (`40ddd63`): full 68-name storage
with pin scope, after-* + command-error + event hooks fire (events clientless
like the pin), `@`-user hooks share the option slot. Ledgered: `-B` monitors
are rejected. The 2026-08-27 client hook slice added the six report-driven
client producers. `client-active` fires only when the latest client changes;
focus, theme, and positive Interactive size reports fire even when repeated.
Control clients do not originate those reports but can win latest-client
promotion after a detach. Changed TUI resizes send retained outer size before
per-pane geometry, so `client-resized` can expand old pane and window dimensions;
`clients.event-resize-context` owns moving that hook after geometry without losing
unchanged-report duplicates. Pinned `after-queue` is explicit-only: ordinary queues do not produce
it, while `set-hook -R` runs it. Three names still have no automatic producer:
`pane-focus-in`, `pane-focus-out`, and `pane-set-clipboard`.
`window-layout-changed` single-fires where the pin double-fires on
resize/select-layout.

The lock trio shipped in wave 5d-2 (`01096c2`) as storage + error parity:
`lock-command` (default `lock -np`) and `lock-after-time` (default 0) store
and read back, all three commands and aliases parse with pin-exact clientless
error shapes, and `after-lock-server` fires through the hook bus — but zz
never spawns a lock program over GUI surfaces (a locked GPUI window running
`lock -np` is meaningless; revisit with the TUI client), and `lock-after-time`
drives no timer. **silent**, deliberate.

Still unimplemented and skipped from config:

| Command | What it does in tmux |
| --- | --- |
| `server-access` | Per-user ACLs for a shared server socket. |

## Superseded by native GUI chrome (4)

`display-popup`, `display-menu`, and `confirm-before` left this table in waves
5d-1/5d-2 for the GPUI client: daemon state and GPUI behavior are pinned, but the
surfaces render as native zz-design-language floating panes (`FloatingSurface`) instead of
cell-drawn overlays. This accepted presentation choice applies to the GUI. Raw zz-tui now renders
and owns all three daemon-published overlay descriptors under the closed
`clients.tui-confirm-before-overlay`, `clients.tui-display-menu-overlay`, and
`clients.tui-display-popup-overlay` records. GPUI and raw-TUI menus share one keyboard resolver,
including shortcut precedence, wrapped steps, raw-row page movement, cancel keys, and stay-open
Enter behavior.

Raw zz-tui confirmation input deliberately owns bracketed paste and pointer events until the daemon
clears the prompt. Pinned tmux routes bracketed paste to pane input before prompt dispatch, so this
is a stricter zz safety rule rather than a parity claim. Focused TUI tests cover paste and pointer
capture. The attached differential covers seven keyboard replies, including Meta-y, and uses a
one-byte pane sentinel to prove those tested keys do not leak.

Slice 10o proves raw-TUI consumption of the daemon-published menu descriptor. The attached fixture
shows a titled menu, shortcut selection, separator and disabled-row skipping, cancel, an unusable
PageUp landing with stay-open Enter, and a nonactivating paste on zz and pinned tmux. Focused
resolver coverage pins exact raw-row-zero and all-disabled boundary behavior. The underlay sentinel
proves those inputs do not reach the pane. The raw renderer layers the menu after chooser and
command-output bases. `display-menu.behavior-fidelity` keeps selected-action
context and errors, mouse policy, paste-close and command-queue ordering, rendered width, resize
lifecycle, shortcut display and grammar, and style refresh open.

Slice 10p closes raw-TUI popup consumption without claiming the broader popup feature set. The TUI
retains the current synthetic viewport, centres and clamps the published client grid, paints the
workspace then popup then higher menu or confirmation state, and purges frame and renderer caches
on close, replacement, attachment change, or reconnect. Popup keys, all held-key lifecycles,
bracketed paste, and tracked content-relative pointer and wheel events resolve before chrome or
pane input; border, outside, and nontracking pointer events are consumed. External focus still
updates client state. A live popup suppresses terminal Focus delivery to both popup and underlay;
a dead `-k` popup closes on FocusOut, matching the pin's overlay key path. The
daemon's sized-client mouse gate now checks the popup terminal's per-client viewport, and one
decoded tracked wheel notch produces one application report while local scrolling keeps its
three-line step. The attached A/B/C cases prove title-only live modification with unchanged job and
geometry, exact SGR click and wheel coordinates at content cell `3,3`, dead `-k` retention, and a
final decimal-122 underlay sentinel. Pinned tmux emits three internal underlay FocusOut/FocusIn
pairs; zz emits none. The focus-reporting live popup applications prove explicit external focus is
swallowed on both sides before `q` or bracketed paste, while the dead case proves FocusOut closes
the retained overlay. Locale-independent full-frame capture passes with C-locale ACS borders. The
six resize, style, context-menu, border-drag, popup-to-pane, and Kitty-image contracts remain under
`display-popup.behavior-fidelity`; real mouse and status formats remain under
`formats.mouse-context`.

| Command | What it does in tmux |
| --- | --- |
| `customize-mode` | Interactive options browser. |
| `choose-client` | Chooser listing attached clients. |
| `clock-mode` | Full-pane clock. |
| `suspend-client` | Ctrl-Z the attaching client. |

## Parked by decision or by model (4)

| Command | Why it stays out |
| --- | --- |
| `link-window` / `unlink-window` | Linked windows and session groups are skipped permanently (drop-in plan decision 3). One window belongs to one session. |
| `new-pane` | Creates a tmux floating pane by default. zz has no floating-pane mux model; the phase-1 picker verb was renamed off `new-pane` so the name stays tmux's. |
| `switch-mode` | Targets an ordinary pane and installs tmux's switch mode. zz's native mode ownership has no equivalent command transition. |

# Flag-level gaps on implemented commands: 2026-08-22 snapshot

Being cataloged is not the whole contract: 23 of the 83 implemented tmux commands still
reject 85 flags the pin accepts, answering `unsupported command: <cmd> -X` rc 1 (and
counting as config skips). The catalog declares the unsupported pairs and the mux/daemon
parsers enforce them. Inventory as of 2026-08-22 (flags in pin spelling; flags marked † are
gated by decision 3 or by missing context/model support, the rest are plain work):

| Command | Rejected flags |
| --- | --- |
| `attach-session` | `-c -f -x`; `-E` † |
| `break-pane` | `-W -x -y -X -Y` † |
| `capture-pane` | `-C -F -H -L -P -R` |
| `choose-buffer` | `-F -k -y` |
| `choose-tree` | `-F -h -k -y`; `-G` † |
| `clear-history` | `-H` |
| `command-prompt` | `-F -l -t`; `-P` † |
| `copy-mode` | `-k -s`; `-S` † |
| `detach-client` | `-t -P`; `-E` † |
| `display-message` | `-a -c -N -v`; `-I` † |
| `kill-session` | `-g` † |
| `list-keys` | `-1 -O -r` |
| `load-buffer` | `-w` |
| `move-pane` | `-D -L -P -R -U -X -Y -z -M` (floating-pane placement) † |
| `new-session` | `-X -f`; `-t` † |
| `resize-pane` | `-M -T` |
| `select-pane` | `-d -e -g -M -m -P` |
| `send-keys` | `-c -K -R`; `-M` † |
| `set-buffer` | `-w` |
| `show-messages` | `-J`; `-T -t` † |
| `source-file` | `-t -n -v` |
| `split-window` | `-k -m -R -s -S -T -W`; `-I` † |

`list-commands` prints each command's usage with zz's accepted flags, so the rows above are
the exact catalog-declared places its output differs from the pin's.

This table preserves the 2026-08-22 roster. Do not update it as a live ledger.
`python3 compat/tmux-tracker.py check` validates the registry and evidence;
`cargo test -p zz-mux compat_manifest` checks current Rust structural gaps against it.
`python3 compat/tmux-tracker.py write-report` publishes the current roster in the generated gap
report. `just compat-check` runs the full gate.

## Command positional bounds

All 72 implemented pinned commands with a finite upper bound now use the catalog's maximum after
leading option grammar and the required minimum, but before unsupported-capability rejection,
targets, or effects. The oracle contains 80 finite maxima; the other eight belong to explicitly
unimplemented commands. Mux and daemon paths
format the exact `command <canonical-name>: too many arguments (need at most N)` diagnostic.
`confirm-before` and the three lock commands use the daemon's common parser, while stored
`bind-key` and `set-hook` children validate both bounds before replacing prior state. Variadic
commands, zz-native commands, and inner callback grammar are unchanged.

The `positional-maximums` fixture checks all 71 generic-CLI-routed finite commands and 62
aliases at rc 1 with empty stdout, exact stderr, and unchanged pane, buffer, and file state. Exact
native attach keeps its positional-session extension outside that CLI proof. An exhaustive
daemon-path test covers all 72 canonical commands and their built-in aliases, including the exact
attach engine path, and proves the catalog error arrives before any state change.

Fourteen required bounds closed later on 2026-08-27. A sorted catalog sidecar records thirteen
minimum-one commands plus minimum-two `if-shell`, while every unlisted command defaults to zero.
Mux and daemon parser boundaries use the same canonical diagnostic after valid flags and before
targets or effects. `display-menu` uses its item parser and `confirm-before` uses the common daemon
parser before resolving clients or panes. The `positional-minimums` fixture checks
all fourteen canonical names and aliases at rc 1 with empty stdout, exact stderr, and unchanged pane,
buffer, and file state. Stored child commands now apply those same minima before their finite
maxima. Callback bodies and incomplete menu triples remain under their existing owners. Nested
`new-session` precedence closed on 2026-08-28.

## Command flag diagnostics

Shared command flag diagnostics closed on 2026-08-28. `parse_tmux_options` consumes the oracle's
flag arity for all 83 implemented upstream commands and 74 aliases. Mux execution,
daemon-preempted commands, stored `bind-key` and `set-hook` children, and exact native attach now
emit the pin's canonical unknown-flag, invalid-flag, `usage:`, and missing-value text. Required
values absorb an attached or following token even when it looks like a flag. Optional values keep
the pin's alphabetic-option lookahead. The parser finishes syntax validation before reporting a
catalogued unsupported capability. `CommandSpec::pinned_tmux_usage` carries 24 diagnostic-only
overrides so `list-commands` and completion continue to describe zz's implemented surface.

The strict three-step `smoke/command-flag-errors` fixture compares 516 probes on each server. It
contains 513 exact failures and three required-value absorption successes, then checks pane,
buffer, file, binding, and hook sentinels. Positional bounds now run after that option grammar and
before recognized parked capabilities, so direct and stored commands return the pin's arity error
for the combined case. All implemented custom `args_parse` command items have since closed.
Semantic value validation keeps its existing owners. Config and source-file command-group
construction stays under `mux.chain-parse-abort`.

## `if-shell` argument blocks

The first custom callback rule closed on 2026-08-28. Protocol v84 carries zero-based lexical
command-block positions with each `CommandInvocation`. An unquoted `{ ... }` branch remains typed
through source-file parsing, Control transport, user aliases, bindings, hooks, and stored-command
printing. Quoted brace text remains a string and is reparsed as such when selected.

For `if-shell`, the condition and option values must be strings; zero-based branch positions 1 and
2 accept either a string or a typed block. A typed condition, typed `-t` value, or typed fourth
positional produces the pin's exact diagnostic before effects. Format and foreground shell routes
execute typed true and false branches, and the background route retains the same type contract.
A typed branch preserves physical source groups: a failed group stops its remaining commands while
later physical lines continue. A string branch reparses as one group.
An invalid plain `bind-key` replacement leaves the previous binding intact, reports bare stderr
with status 1, and does not open a replacement view. Plain command tails retain typed positions
across command chains, and stored printing keeps typed branches unquoted. The strict three-step
`smoke/args-parse-if-shell` scenario runs 12 internal checks on both source-file and Control paths,
including the canonical name, built-in alias, quoted-brace stderr placement, stored printing, and
the no-mutation case. It finishes with `ARGS_PARSE_IF_SHELL=clean:12` on both servers.

This closure does not claim tmux's eager whole-file construction timing for every nested command
list. That broader parser-group contract remains separately tracked.

## `run-shell` argument blocks

The second custom callback rule closed on 2026-08-28 without another protocol change. The v84
lexical positions now distinguish a typed block from quoted brace text in `run-shell`. A leading
`-C`, including combined forms such as `-bC` and `-Cd0`, makes every positional
command-or-string. Without it, every positional must be a string. Option values always remain
strings. Leading option scanning stops at the first positional or `--`, so `-C` after either
boundary stays positional and does not enable command mode.

Only positional 0 executes in command mode. Later valid string or typed positionals are accepted
and ignored, matching the pin. A typed positional without command mode and a typed `-c`, `-d`,
`-s`, or `-t` value produce the pin's exact diagnostic before effects. Quoted braces stay strings;
under `-C` they are reparsed as command text rather than treated as a lexical block. Valid stored
bindings retain typed blocks, and an invalid replacement leaves the prior binding or hook intact.
A typed command callback preserves physical source groups: a failed group stops its remaining
commands while later physical lines continue. A string callback reparses as one group.
The strict three-step `smoke/args-parse-run-shell` scenario runs 21 internal checks on source-file
and Control paths. It covers canonical, built-in alias, unique-prefix, and user-alias resolution,
foreground and background execution, combined flags, option boundaries, stored printing, exact
errors, and rejected-effect suppression. Both servers finish with
`ARGS_PARSE_RUN_SHELL=clean:21`.

This closure does not claim eager construction of every nested callback before outer command
validation. Pinned `source-file -n` applies the callback while building its parse-only command
list; zz currently stops after lexical parsing. Those ordering differences remain under
`mux.chain-parse-abort`. The fixture also avoids the pin's
zero-positional `run-shell -C -d 0` server crash, which zz does not reproduce.

## `set-option` argument blocks

The third custom callback rule closed on 2026-08-28 without another protocol change. For
`set-option` and `set-window-option`, positional value 1 accepts either a string or a typed command
block. The option name, every flag value, and every extra positional remain strings. Typed failures
use the canonical command name and precede maximum arity, target lookup, and effects. A rejected
direct command or stored binding replacement leaves existing state unchanged.

Accepted blocks expand the live mux environment and stringify through recursive command printing
before optional `-F` expansion.
Built-in aliases, unique prefixes, and one preexisting user-alias layer become canonical names;
same-line commands retain ` ; `, physical-line groups retain ` ;; `, and nested blocks are printed
recursively. A top-level empty block becomes an empty value. Quoted brace text stays literal.
String command options keep tmux's one-group parse, so a multiline `default-client-command` uses
`;`; a typed multiline value first preserves its groups and then follows that same command-option
normalization.

The strict three-step `smoke/args-parse-set-option` scenario runs 21 internal source-file and
Control checks. It covers both commands, canonical names, built-in aliases, unique prefixes,
preexisting user aliases, typed names and targets, typed versus string extras, `--`, late flags,
single, multi, nested, multiline, empty, and quoted values, `-F` ordering, a real command option,
stored binding preservation, direct Control rejection, and Control-written readback. Both servers
finish with `ARGS_PARSE_SET_OPTION=clean:21`.

This closure does not claim eager whole-file or invalid-child construction, callback validation
under `source-file -n`, or tmux's suppressed nested user alias after an outer user-alias expansion.
Slice 10y later closes the same-file alias snapshot. Eager construction remains under
`mux.chain-parse-abort`, and nested alias recursion keeps its existing owner.

## `bind-key` argument blocks

The `bind-key` slice of the `commands-or-string` rule closed on 2026-08-28 without another wire
change. Every positional accepts either a string or typed command block while `-T` and `-N` values
remain strings. Option scanning stops at the first positional or `--`. A typed key is recursively
printed after live mux-environment expansion, with canonical command names and same-line `;` or
physical-line `;;` separators, before key lookup. Unknown or ambiguous commands discovered while
constructing that typed key retain the source path and line diagnostic; a successfully constructed
but invalid key remains a bare key error.

One typed tail retains its parsed command list and physical-line groups. One string tail is
reparsed as one group. Longer tails follow the argument-list parser, including the pin's empty
binding when the first tail value is typed and more arguments follow. Typed command-name groups in
a longer tail are omitted. Child validation completes before replacement, so exact type, syntax,
and unknown-command failures leave the previous binding unchanged.

The strict three-step `smoke/args-parse-bind-key` scenario runs 17 internal source-file and Control
checks. It covers canonical, built-in, unique-prefix, and preexisting user aliases; typed keys,
unknown-command construction, option values, exact typed and string tails, nested callbacks, empty
blocks, `--`, late flags, recursive printing, Control framing, and preserved
state. A temporary attached client selects the test key table and sends real F-keys, proving that
failure drops only the current typed physical-line group while a quoted multiline string remains
one group. It also proves that bare `bind-key KEY` preserves commands and unspecified metadata,
replaces a note only with `-N`, sets repeat with `-r`, creates an empty selected table for an absent
key, and lets a later command-bearing bind replace the metadata. Both servers finish with
`ARGS_PARSE_BIND_KEY=clean:17`.

Eager whole-file and parse-only `source-file -n` child validation plus the outer-user-alias and
nested-user-alias suppression case retain their existing owners. Slice 10y later closes the
same-file alias snapshot. The fixture does not dispatch the pin's crashing `confirm-before {}`
binding and does not claim the separately tracked `send-keys -K` behavior.

## `confirm-before` argument blocks

The `confirm-before` slice of the `commands-or-string` rule closed on 2026-08-28 without another
wire change. Its one command positional accepts either a string or typed block while `-c`, `-p`,
and `-t` values stay strings. Option scanning stops at the first positional or `--`. Typed children
construct recursively before the parent command's name, callback type, or arity validation. One
user-alias layer is carried independently along each recursive path; siblings do not consume it,
alias-produced subtrees disable further user aliases, and direct self-recursion fails as unknown
without killing the daemon. Nested `if-shell`, `run-shell`, set-option, and `confirm-before` blocks
print canonical names. An empty block reads back as `{  }`, and physical internal group newlines
print as ` ;; `. String children construct after target lookup and parent-format expansion as one
group. The two paths retain their distinct parser and runtime diagnostic channels.

The strict three-step `smoke/args-parse-confirm-before` scenario runs 19 internal source-file and
Control checks. It covers canonical, built-in, unique-prefix, and preexisting user aliases; typed,
string, and quoted construction; string-only options; `--` and late flags; canonical stored
readback; target, arity, syntax, and unknown-command diagnostics; parent-format expansion; and
rejected-binding preservation. Exact Control comparisons also cover recursive nested callbacks,
sibling alias independence, alias-produced subtree suppression, self-recursion, and nested bind
and confirm failures as preflight parse errors. Both servers finish with
`ARGS_PARSE_CONFIRM_BEFORE=clean:19`.

This fixture proves construction, parser, readback, and output channels. Accept, reject,
`-y` Enter-default, blocking, and background reply paths are covered by daemon and GPUI unit tests.
Raw zz-tui confirmation replies later closed under `clients.tui-confirm-before-overlay`; menu
descriptor consumption closed under `clients.tui-display-menu-overlay`; popup consumption later
closed under `clients.tui-display-popup-overlay`.
It also does not close eager whole-file source construction or the broader replay-channel
placement difference, which remain with the existing parser and command-chain owners.

## `command-prompt` argument blocks

The `command-prompt` slice of the `commands-or-string` rule closed on 2026-08-28 without another
wire change. The command accepts zero or one template positional as a typed block or string while
`-I`, `-p`, `-t`, and `-T` values stay strings. Typed children construct recursively before the
parent command's name, callback type, or arity validation. Recursive paths carry independent
one-layer user-alias budgets. An outer user alias disables another alias in its produced subtree,
while a sibling typed template receives its own layer. Empty typed templates remain valid.

The two template shapes keep distinct deferred-execution contracts. A typed template retains its
structured constructed command list through submission without another user-alias lookup. Answer
substitution edits leaf arguments without reparsing the answer, so quotes and semicolons cannot
create arguments or commands. A string template retains raw source and substitutes the answer
before parsing. The daemon then constructs every parsed command against the current alias table
before it executes any of them. Both shapes replace the first `%%` and every `%1`; a trailing `%`
quotes double quotes, backslashes, dollar signs, semicolons, and tildes in the inserted answer.

Typed templates retain physical source groups. A failed command stops the rest of that group while
later physical lines continue. String templates and free input form one group, so a failure stops
the rest of the submission. The string path also carries the original invocation's source path and
line through substitution, parsing, and construction. Located failures therefore point back to the
stored template rather than a synthetic prompt source.

The strict three-step `smoke/args-parse-command-prompt` scenario drives a real attached client and
runs 43 internal checks. It covers zero, typed, string, and empty templates; string-only options;
child-before-parent error precedence; canonical readback; outer and sibling aliases; exact Control
frames; fresh string aliases versus frozen typed aliases; `%%`, `%1`, `%%%`, and `%1%`
substitution; structured injection resistance; and typed versus string group failure. Both servers
finish with `ARGS_PARSE_COMMAND_PROMPT=clean:43`. Focused daemon tests cover whole-result string
preflight and stored source-line retention.

This slice does not add prompt chaining or multi-answer `%2`, `-F`, `-l`, `-t`, labels, key
spelling, queue order, vi editing, or freeze changes. Those contracts retain their existing prompt
owners. Eager whole-file source construction and the broader replay-channel placement difference
remain with the parser and command-chain owners.

## `set-hook` argument blocks

The `set-hook` slice of the `set-hook-monitor-or-value` rule closed on 2026-08-28 without another
wire change. Without `-B`, only value position 1 accepts a typed block or string. The hook name and
extra positionals remain strings. With `-B`, every positional lexically accepts either type, while
the `-B` and `-t` option values stay strings. zz still rejects `-B` during execution because it has
no format-monitor runtime. This closure covers the callback argument rule and leaves monitor
behavior under its existing owner.

Every typed child constructs before parent type, arity, or effect validation. Accepted typed values
normalize through recursive canonical printing before they enter one of three storage paths:
built-in hooks, custom `@` options, or the named-option forwarding path used by
`default-client-command`. Built-in hooks parse the normalized value again and flatten physical
groups into one command list. Custom `@` typed values store their textual ` ;; ` group markers for
deferred execution. A quoted brace string is runtime hook syntax for a built-in name, where its
parse can fail, but remains literal deferred text for a custom name.

Replacement order follows the pin. An unindexed built-in replacement clears the hook array before
parsing its runtime value, so a malformed value leaves no prior entry. An indexed replacement parses before
it writes and preserves the prior entry on failure. An unindexed empty block clears without adding
an entry, appending an empty block does nothing, and an indexed empty block remains present. Local
hook-array creation precedes empty-append and runtime-parse handling. An empty or failing local
append therefore installs an empty local array and shadows the inherited global hook. `-R` still
constructs a supplied typed value and can fail before running the stored hook. The same command
ignores a supplied quoted string value and runs the stored hook.

The strict three-step `smoke/args-parse-set-hook` scenario runs 24 internal checks. It covers
string-only hook names and option values, extra-position type and arity errors, child-before-parent
precedence, canonical readback, preexisting aliases, same-line and physical groups, empty blocks,
built-in versus custom quoted braces, replacement and local-inheritance ordering, `-R`, named-option
forwarding, stored bindings, and exact Control framing. Both servers finish with
`ARGS_PARSE_SET_HOOK=clean:24`.

Eager whole-file construction, multiline inner-source diagnostic placement, `-B` monitor
semantics, and broader replay placement retain their existing owners. Slice 10y later closes the
same-file alias snapshot.

## `display-menu` argument blocks

The `display-menu-items` rule closed on 2026-08-28 without another wire change. Its callback reads
positional data through repeated NAME, KEY, and ACTION states. A nonempty NAME advances to a string
KEY, then an ACTION accepts a string or typed block and resets the state to NAME. An empty NAME is a
separator that consumes no KEY or ACTION and leaves the parser expecting another NAME. Values for
`-b`, `-c`, `-C`, `-H`, `-s`, `-S`, `-t`, `-T`, `-x`, and `-y` stay strings.

Every lexical typed child constructs before the parent type, arity, or effect boundary. A child
failure therefore precedes a NAME, KEY, or option-value type error. Accepted typed actions print
their recursively constructed canonical commands in stored bindings. Quoted brace actions keep
their string form. The callback accepts incomplete NAME and NAME-plus-KEY tails for daemon runtime
validation, so stored bindings can retain either incomplete form. Runtime resolves the current or
`-c` target client before completeness, so an unattached command or initial Control reports `no
current client`; initial Control uses a flag-0 `%error` and exits 1. Once attached, Control validates
an incomplete group as `not enough arguments` before its overlay no-op and emits an exact flag-1
`%error`; EOF after that frame exits 1. Interactive menu ordering remains unchanged.

The daemon's existing selection path removes the structural wrapper from a typed action before its
fresh parse and leaves quoted brace strings literal. The callback closure does not absorb selected
action execution or error delivery.

The strict three-step `smoke/args-parse-display-menu` scenario runs 34 internal checks. It covers
typed NAME and KEY positions at the first and later items, empty-name separator resets, all ten
string-only valued flags, child-before-parent precedence, multiple items and separators, canonical,
built-in alias, unique-prefix, and preexisting user-alias construction, typed and quoted stored
readback, invalid-binding preservation, client-before-completeness precedence, incomplete runtime
groups, source-file diagnostics, and exact initial flag-0 plus attached flag-1 Control framing. A
PID-unique FIFO proves the attached process exits 1 after EOF. Both servers finish with
`ARGS_PARSE_DISPLAY_MENU=clean:34`, and the strict run reports zero differences.

Raw-TUI menu descriptor consumption closed under `clients.tui-display-menu-overlay`. Selected-action
context and errors, mouse policy, paste-close and command-queue ordering, rendered width, resize
lifecycle, shortcut display and grammar, and style refresh remain under
`display-menu.behavior-fidelity`. Same-source alias mutation, eager whole-source construction, and
generic alias recursion keep their existing owners.

## `display-panes` argument blocks

The `display-panes` member of the shared commands-or-string rule closed on 2026-08-28 without a wire
change. Its optional template positional accepts a string or typed block, while `-d` and `-t` values
remain strings. Every lexical typed child constructs before parent option-type or arity validation.
Canonical, built-in alias, unique-prefix, and preexisting user-alias forms
retain typed positions, and stored bindings print constructed children canonically.

Targetless daemon routing now uses `resolve_client_target` before duration validation. With no
attached client, the command reports `no current client`; an ordinary Command client uses an
available attached Interactive client. The strict three-step fixture runs 22 internal type,
arity-precedence, alias, readback, target, duration, source-file, and direct Command-client runtime checks on both servers and
reports zero TOPO, GEO, FMT, OUT, or WARN differences.

This parser closure does not implement tmux's custom selection action. On selection, tmux
substitutes the selected `%pane` for `%%%` and executes the result with the retained original queue
state; an omitted template uses `select-pane -t "%%%"`. Mux execution still rejects a positional
template, and its native overlay has a fixed select-pane action. The loud runtime gap is parked under
`display-panes.command-template`; overlay queue blocking and presentation retain separate owners.

## `choose-buffer` and `choose-tree` argument blocks

The final two implemented members of the shared commands-or-string rule closed together on
2026-08-28 without a wire change. This is a deliberate exception to the planned separate 10j and
10k milestones: both commands use the same callback rule, the same chooser-template execution path,
and one attached-client fixture. Each accepts zero or one string-or-typed template. Values for
`-F`, `-f`, `-K`, `-O`, and `-t` stay strings. Every lexical typed child constructs before parent
type, arity, target, or effects. Direct and stored commands also validate positional bounds before
rejecting a recognized parked capability.

A typed template resolves its aliases and stores canonical text before the chooser opens, with
` ; ` inside one physical group and ` ;; ` between groups. A quoted template stays raw. Selection
parses the substituted text against the current alias table, so a typed alias stays frozen while a
string alias observes later changes. The first `%%` and every `%1` receive the selected value; a
trailing `%` quotes double quotes, backslashes, dollar signs, semicolons, and tildes.

Tree rows supply `=name:` for sessions, `=name:index.` for windows, and `=name:index.%id` for panes.
Buffer rows supply the exact buffer name. The row does not retarget execution: the action uses the
invoking client's live session, window, and pane context. The daemon closes the chooser before
execution and capitalizes the first character of attached parse and command errors. An empty buffer
store opens no chooser, and a selected buffer removed after opening closes without running its
template.

The strict three-step `smoke/args-parse-choosers` fixture runs 26 internal checks and ends with
`ARGS_PARSE_CHOOSERS=clean:26` on both servers. It reports zero TOPO, GEO, FMT, OUT, or WARN
differences. Broader chooser flags, tagging, previews, editor behavior, tree kill and swap actions,
presentation, eager whole-source construction, generic alias recursion, and raw-TUI overlay parity
retain their existing owners. Slice 10y later closes the same-file alias snapshot. No command-specific `args-parse:` item
remains for an implemented command.

## Accepted grammar divergence evidence

The catalog count does not include syntax zz accepts or parses before diverging:

- The default prefix table has 60 bindings against the pin's 92, with 59 overlapping keys. zz
  adds `e -> send-last-output`, omits 33 stock keys, and deliberately maps `%`/`"` to
  `split-picker` plus `s`/`w` to `focus-sidebar`. Explicit imported commands retain tmux meaning;
  the exact default delta lives in [key tables](/tmux/key-tables.md).
- `refresh-client` rejects bare redraw, `-S`, `-c -D -L -R -U -l -r`, and the optional
  adjustment positional. `-A -B -C -f -F -t` behave.
- `load-buffer -` rejects stdin, `save-buffer -` rejects stdout, and `source-file -` rejects
  stdin.
- Relative `load-buffer` and `save-buffer` paths use the persistent daemon's cwd. tmux routes file
  access through the invoking client, using a command client's cwd or an attached session cwd. The
  remote and attached-client close remains under `buffers.client-file-context`.
- `show-buffer` refuses non-UTF-8 buffer bytes.
- `capture-pane` now routes like the pin: `-p` prints and ignores `-b`; without `-p`, `-b` selects
  a named paste buffer and bare capture creates an automatic one, including the final newline in
  stored bytes. Clustered value flags such as `-pS -3`, `-pS0`, `-pb name`, and `-pE5` follow
  tmux: a value option consumes the rest of its cluster. Numeric `-S`/`-E`, raw `-`, inclusive and
  reversed bounds, target-scoped format expansion, and tmux's silent invalid/out-of-range fallback
  match.
  One text residue remains: when `-E` falls back to visible end and the viewport has trailing blank
  rows, tmux emits those rows as newlines while zz stops after the last retained content row. `-T`
  is inert. Capture stays UTF-8 text/VT output; there is no retained saved-alternate grid, pending
  raw-byte stream, raw-grid dump, hyperlink list, line-flag prefix, or line-number prefix. The last
  six forms stay loudly rejected as `-P`/`-C`/`-R`/`-H`/`-F`/`-L` rather than being approximated.
- `copy-mode` (every flag, including bare) exits 1 with `pane is not attached: %N` when no
  client is attached to the target pane, where the pin sets the mode regardless — the mode
  lives on the pane in tmux and on the per-client terminal view in zz
  (`MuxEffect::TerminalView` resolves a target client and errors on an empty set). The same
  gate keeps `choose-tree`/`choose-buffer` at `choose-* requires an interactive client`. This
  is why no copy-mode or chooser behavior can ride the differential corpus, which drives both
  sides through a bare CLI against a headless server: every step would diverge on exit class
  before any flag mattered. Found 2026-08-22 while trying to add a `copy-mode -H` scenario.
- `choose-tree`/`choose-buffer` accept one `-N` as a no-op — zz's choosers are native
  surfaces with no preview pane, so "no preview" is already their only layout — and reject a
  repeated `-N` as `unsupported command: <cmd> -NN`, the pin's `MODE_TREE_PREVIEW_BIG`
  (`args_has(args, 'N') > 1` in `mode_tree_start`), which has no zz presentation. `-K` expands
  per row, and the two sides discard different expansions: the pin drops what
  `key_string_lookup_string` cannot PARSE (`KEYC_UNKNOWN` -> `KEYC_NONE`), never testing
  whether anything can press the result, while zz drops what falls outside its own input
  vocabulary (`zz_protocol::is_key_name`, defined as exactly the grammar `input_key_name`
  emits). zz's gate is strictly the narrower one, so a spelling tmux parses but zz has no
  keystroke for — `M-C-a` and other orderings, `Space`, key names zz does not model — is
  drawn by the pin and blank in zz. Conservative by choice: a key zz could never deliver
  would be a dead shortcut.
- `display-menu` row shortcuts run the same two-stage gate: `zz_mux::parse_tmux_key` answers
  the pin's `key_string_lookup_string` question (so `^A`, `C-M-x`, `Space`, `BTab`, `F1`-`F12`
  and the named table all resolve, and `Ctrl-Alt-x`, `F13`, `F0` and unknown words resolve to
  nothing), then `zz_protocol::is_key_name` keeps only what a client attached to the menu can
  press. `BTab` is kept beside that gate because `zz_client::resolve_menu_key` spells a shifted
  Tab `BTab` before it matches rows. What the pin parses but zz drops is the same narrowing the
  chooser `-K` rule takes: `S-` chords, keypad names, and the `0x..` spellings, none of which
  any attached client emits — and none of which the pin can press either, since
  `key_string_lookup_string` packs a `0x41` through `utf8_from_data` into a key no keystroke
  equals. Measured key by key against the pin in
  `compat/scenarios/smoke/display-menu-shortcut-grammar.txt`.
- `command-prompt` never comma-splits `-p` or `-I`, so `-p 'a,b'` raises ONE prompt labelled
  `a,b` where the pin chains two and feeds their answers to `%1` and `%2`. zz's behaviour is
  exactly the pin's `-l`, which is why `-l` stays REJECTED rather than accepted as a no-op:
  accepting it would advertise an opt-out from a chain zz cannot run. `cmd_command_prompt_exec`
  builds `cdata->prompts[]` by `strsep(&next_prompt, ",")` and
  `cmd_command_prompt_callback` walks `cdata->current` toward `cdata->count`; zz has neither.
  `-F` stays rejected for the same honesty reason: the pin expands the template through
  `format_single_from_target`, and zz's only prompt-side expander (`expand_prompt_input`)
  understands `#S` and `#W` and nothing else, so accepting `-F` would silently drop every
  other format. `-t` stays rejected with the rest of the client-fanout contract.
- `command-prompt` preserves the alias and source boundary of its template shape. A typed template
  keeps its structured constructed command list through submission. A string template substitutes
  raw source, then parses and constructs the complete result against the current alias table before
  execution. Both paths apply the pinned `%%`, `%1`, and trailing-percent quoting rules. Typed
  templates keep physical groups; string templates and free input form one group. Prompt chains,
  multi-answer `%2`, and the remaining prompt UI and queue contracts stay with
  `prompt.command-fidelity`.
- A selected typed `display-menu` action drops its structural block wrapper before the fresh parse.
  A quoted brace string remains literal. The argument-rule closure above leaves selected-action
  execution and error delivery with the menu runtime owner.
- `command-prompt` draws a different LABEL from the pin in two of three cases, found while
  measuring the mode flags and left alone because D1's brief required zero visible change for
  prompts using none of the new flags. `cmd_command_prompt_exec` appends a trailing space to
  every label it builds (`xasprintf(&tmp, "%s ", prompt)`) EXCEPT the bare `:` default, and
  when there is no `-p` but there IS a template it labels the prompt `(<first command name>) `
  rather than `:`. Measured: `-p lbl` then `q` drew `lbl q`, a bare `command-prompt
  'display-message ...'` drew `(display-message) q`, and a bare `command-prompt` drew `:q`. zz
  draws `lblq`, `:q` and `:q`. Cosmetic, three lines to fix in `MuxEngine::command_prompt`, and
  a natural pickup for the F error/label tranche.
- `command-prompt -N`'s pass-through runs in the opposite order to the pin. Both sides submit
  the collected digits AND process the non-digit key normally; the pin's cmdq runs the passed
  key's binding first (measured: a `-N` prompt fed `1`, `2`, `z` with `z` bound logged
  `ZBOUND` before `NUM[12]`, because `cmd_command_prompt_callback` uses
  `cmdq_insert_after` on the already-waiting item while the key callback is appended), and zz
  submits first because `input_command_prompt_key` completes before it answers "pass". zz has
  no command queue to reproduce the interleave, and the observable contract — both commands
  run — holds.
- `command-prompt -k` answers with `input_key_name`, which spells a space as `" "` where the
  pin's `key_string_lookup_key` spells it `Space`. Same narrow-vocabulary rule as the chooser
  `-K` gutter above.
- `command-prompt` edits with emacs keys only. The pin routes every prompt key through
  `prompt_translate_key` first when `status-keys` is `vi`, and `tmux.c:543-554` rewrites
  `status-keys` AND `mode-keys` to `vi` at startup whenever the basename of `$VISUAL`/`$EDITOR`
  contains `vi` — so a developer box with `EDITOR=nvim` silently runs a vi prompt even though
  the options table default is emacs. zz's prompt has no vi mode. Pre-existing; recorded here
  because it is why every pin measurement in this row had to `set -g status-keys emacs` first,
  and because it makes `status-keys`/`mode-keys` defaults environment-dependent in a way zz
  does not reproduce at all. The sharpest symptom: in vi mode Escape enters
  `PROMPT_COMMANDMODE` instead of closing the prompt, so `command-prompt` on such a box has no
  one-key cancel at all. Note also that `prompt_draw` branches only on `PROMPT_COMMANDMODE`
  and `PROMPT_QUOTENEXT` — none of D1's mode flags change how a prompt is DRAWN, which is why
  the TUI's status-line prompt needed no rendering change to stay faithful and only the GPUI
  palette, a zz-native modal, adjusted its affordances.
- The freeze a message raises is DERIVED in zz and LATCHED in the pin, and they part company in
  one place. `status_message_clear` only drops `TTY_FREEZE` `if (c->prompt == NULL)`, so
  clearing a message while ANY prompt is open leaves the client frozen — including a
  `command-prompt -C` or `-i` prompt that explicitly asked not to freeze. Measured on the
  nested rig: with a `-C` prompt open the view ticked `TICK62` to `TICK77`, a `display-message
  -d 0` stalled it, the dismissing key delivered exactly one catch-up frame (`TICK91`) and then
  NOTHING until the prompt closed (`TICK144`). zz's `client_terminal_publication_frozen` reads
  `message.freeze || prompt.freeze` fresh on every publication, so retiring the message
  resumes the client immediately. This is the prompt path's analogue of the
  `display-message -N` stickiness recorded for Wave D run 3, and zz diverges deliberately:
  reproducing it would mean latching a flag zz has no other reason to keep.
- `list-keys <key>` rejects the positional key filter.
- `send-keys -H` rejects bytes `80` through `ff`.
- Bind-time validation checks shared names and catalog flags, including daemon-native long options,
  but still misses complete positional arity and target validation.

## `send-keys` copy-action grammar

The outer `send-keys` parser rejects `-C`, `-P`, and `-o` with the pin's `command send-keys:
unknown flag -X` shape. The window-copy parser owns those names after the action. It recognizes `-C`
and `-P` on the pin's 14 copy-family grammar entries, recognizes `-o` on `next-prompt` and
`previous-prompt`, splits a `-CP` cluster, and honors `--`. A local parse failure returns success
without running an action and resets the copy-mode repeat prefix to 1. The catalog, mux tests, and
`micro-flags` scenario cover the two parser boundaries. `terminal.key-control` retains execution for
`copy-line`, `copy-line-and-cancel`, `copy-pipe-line`, and `copy-pipe-line-and-cancel` under
`semantic:send-keys-copy-command-shape`. The pin also redraws the first copy-mode line after a local
parser failure. zz emits the repeat reset without a no-op redraw action, so the same item owns that
presentation residue.

Read-only authorization checks whether `-X` was present before full option validation, so a
non-copy request such as unsupported `-M` answers `client is read-only` instead of exposing the later
parser error. Pin-recognized unsafe commands that zz does not implement, including the copy-line,
selection-mode, scroll-exit, and unsupported search forms, still reject at authorization. Empty and
genuinely unknown `-X` commands remain safe at this layer and follow their ordinary no-op or no-mode
path. A direct request authorizes only that invocation; a stored binding list is preflighted as one
all-or-nothing chain before any effect, matching the pinned command-list check.

## Former Wave 2e ownership (27 pairs)

The old plan grouped these commands under a TUI tranche. Source tracing shows that most of
the work belongs to the server:

| Command family | Server/core | Client/presentation | Parked on missing context/model |
| --- | --- | --- | --- |
| `command-prompt` | `-F -l -t` | ~~`-1 -C -e -i -k -N -T`~~ shipped | `-P` pane-rendered prompt |
| `copy-mode` | `-k -s` | . | `-S` bound mouse-slider context |
| `send-keys` | `-c -K -R` | . | `-M` originating mouse event |
| `display-message` | `-a -c -N -v` | . | `-I` CLI stdin/protocol stream |
| chooser residue | tree `-F -h -k -y`; buffer `-F -k -y` | . | tree `-G` session groups |
| `display-panes` | ~~`-N -t`~~ shipped | . | . |
| `show-messages` | `-J` | . | `-T -t` TTY capability model |

That leaves **20 server/core**, **0 client/presentation**, and **7 parked** pairs, and the three
numbers reconcile against the enforced roster above command by command. Wave D run 1 shipped
`-K`/`-N` on both choosers out of the middle column, run 3 shipped `display-message -d` out
of the first and `-C` out of the second, and the final run cleared `command-prompt`'s whole
middle column — the mode flags turned out to be daemon-owned rather than client work, which
is why they landed with a state machine in `zz-daemon` and only a key-relay branch in each
client. **The middle column is now empty for these 27 flag pairs**, so everything left in this
table belongs to F and G. This flag roster does not inventory consumption of daemon overlay state.
Raw zz-tui now handles `confirm-before` and consumes both `display-menu` and `display-popup`
descriptors. Broader menu behavior remains under `display-menu.behavior-fidelity`, and the six
remaining popup behavior contracts stay under `display-popup.behavior-fidelity`. None moves
daemon-owned command semantics into the client.

# Implemented-surface measurements through 2026-08-29

| Where | Divergence | Loud or silent? |
| --- | --- | --- |
| `find-window` | Detached CLI calls validate the target and return success with no output, including for zero matches. zz does not open tmux's attached-client window-tree chooser. | **silent**, bounded |
| `list-commands` | zz lists implemented commands in tmux's line format. Each usage string reports zz's accepted flags, so affected rows differ from the pin. Unimplemented commands stay absent so feature probes can take their fallback path. | **silent**, deliberate |
| `list-keys` selection and formatting | `list-keys -F` expands the pin's `notes_only`, `key_repeat`, `key_note`, `key_prefix`, `key_table`, canonical `key_string`, quoted `key_command`, repeat-set, and width facts. Literal stored space bases render as `Space` or `C-Space`; widths use that spelling, and positional filtering compares base, type, and modifiers while excluding stored spelling and flags. Bare output uses the pin's global padding. `-N` chooses `prefix` then `root` unless `-T` names one table, filters on note presence, and uses command text when the note value is empty; `-a` disables that filter and `-P` supplies a literal displayed prefix. The positional key filter, `-1`, `-O`, and `-r` follow the pinned ordering and error precedence, including per-table `-N` sorting and attached-client status routing for `-1`. Stock `copy-mode` and `copy-mode-vi` bindings now carry no repeat bits; copy-mode repetition remains runtime state. | none outside the bounded sort ties below and the separately tracked long key-modifier aliases |
| Copy-mode vi numeric prefix | The default `1` through `9` bindings keep zz's per-client `copy-mode-repeat` command shape rather than opening tmux's pane-cell `command-prompt -NP`. Digits, including a following `0`, accumulate to 9,999. The first `send` or `send-keys` command whose option prefix contains `-X` consumes the count. Its own `-N` wins; otherwise zz inserts separate `-N <count>` arguments immediately before the option argument containing `-X`. The engine does not scan onward after a stored `-N`, and a binding with no qualifying `-X` leaves the count armed. Prefix-consuming movements, jumps, matching brackets, and repeat-search run N times; `other-end` swaps only for odd N; `select-line` spans N lines; the copy-end-of-line family spans N rows and copies once; other toggles, selection, copy, clear-selection, cancel, and later actions run once. Bare `0` remains start-of-line. Direct terminal `send-keys -N` accepts the pin's full 1 through UINT_MAX range and stops on input backpressure. Native browser sinks cap repeats at 9,999 because tmux has no browser pane. | behavior closed; the visible and `list-keys` command-shape difference plus the buffered-prefix and browser caps are accepted under `keys.copy-mode-native-numeric-prefix`, `keys.copy-mode-action-and-repeat-fidelity`, and `terminal.key-control` |
| Copy-mode fixed viewport rows | `top-line`, `middle-line`, and `bottom-line` set column zero and place the cursor at the current frozen viewport's top, middle, or bottom row without moving that viewport. Targets clamp to the retained revision. | closed for these three placements; `history-bottom`, logical lines, wrapping, scrolling, and wider action semantics remain under `copy-mode.action-fidelity` |
| Copy-mode action vocabulary | The pinned `window-copy` table contains 95 action names. zz maps 66 to typed mux and terminal behavior. The remaining 29 stay classified under seven semantic items: action vocabulary, cursor geometry, logical-line and mode-key behavior, goto-line, selection lifecycle, jump/page/prompt actions, and copy formatting and destination effects. Seven absent default keys depend on five of those actions. | tracked under `copy-mode.action-fidelity`; `keys.copy-mode-unsupported-default-actions` owns only the seven default keys |
| TUI command-output navigation | Protocol v79 adds a nonzero actor ID to each real command-output frame and close. `ClientCore` retains an actor watermark, ignores stale frames and closes, and treats ID zero with no viewport as the authoritative no-output resync. The TUI ties its search editor, swallowed-key state, and resize cache to that actor ID, so same-actor frames preserve local state while replacement, close, and reconnect clear it. One content rectangle drives both paint and `ResizeCommandOutput`, reserving the output header, footer or message row, and configured status block. Press and repeat keys travel through the daemon's effective `copy-mode` or `copy-mode-vi` table; releases are inert. Live `mode-keys` and `bind-key` changes retarget the open output view. Stock emacs `q` and Escape cancel; stock vi `q` cancels while Escape clears a selection and leaves the view open. `BeginSearch` opens the TUI-local editor, Enter submits the live query by leaving edit mode, Escape closes the search, and `n`/`N` use the table's next and previous search actions. Line and page movement, selection, and copying into a paste buffer operate on the retained output. The attached fixture checks these semantics on 96 lines against zz and the pin. | closed for the named local keyboard and paste-buffer contract; no mouse or OS-clipboard claim, ordinary TUI pane copy search remains unsupported, the 29 missing window-copy actions stay open, and this attached proof covers neither SSH, pixels, nor the canonical summary |
| `list-keys` sort ties | Pinned tmux's comparator truncates key identity and returns non-total results for equal-base modifier/type ties, cross-table ties, and fields that do not apply to bindings, so libc `qsort` may reorder those rows. zz keeps a total deterministic order: `-O key` compares tmux's low-32-bit base identity first, including packed two- and three-byte UTF-8, then type, modifier bits, flags, canonical spelling, table, and original traversal index. Four-byte Unicode uses its scalar value as a stable fallback because tmux's packed value does not fit the retained low 32 bits. Stable distinct-base `key`, `order`, reverse, and per-table `-N` cases remain differential-tested. | **silent**, deliberate and bounded to comparator ties plus four-byte Unicode |
| Long key-modifier aliases | Closed 2026-08-29 for tmux commands. Bind, unbind, list filtering, `prefix`, `prefix2`, and `backspace` now accept the pin's short case-insensitive modifiers and reject `Ctrl-` and `Alt-` before state changes. The shared native key parser remains permissive for zz clients. | none on tmux command paths; native client aliases remain a zz extension |
| `#{config_files}` default discovery | Explicit startup `-f` paths are retained in order and comma-joined like the pin, and later `source-file` calls do not append. `reload-config` selects zz's current default candidate and replaces the retained fact with that path or empty. Without `-f`, pinned tmux lists every expanded default candidate whether or not it exists and does not canonicalize it; zz lists only the first existing zz-owned mux config, or empty when none exists. | **silent**, deliberate config-ownership boundary |
| `refresh-client` | `-A`/`-B`/`-C`/`-f`/`-F`/`-t` behave (phase 6: flow control, subscriptions, control-client sizing). `-C` is Control's explicit geometry path; Control does not emit the TUI-only `ClientTerminalSize` message. Bare redraw, `-S`, and the attached-client redraw/scroll family (`-c -D -L -R -U -l -r` plus the optional positional adjustment) answer `unsupported command: refresh-client interactive behavior`; detached command clients with no target get the pin's exact `no current client`. | loud |
| Supported client-selector targets | Every implemented tmux command flag that selects an attached client uses one matcher: `detach-client -t`, `switch-client -c`, `display-message -c`, `display-panes -t`, `display-popup -c`, `display-menu -c`, `confirm-before -t`, `refresh-client -t`, `lock-client -t`, and `load-buffer -t`. It accepts an exact registered name, the exact published `#{client_name}`, a full tty, or a tty after removing exactly one leading `/dev/` prefix, with exactly one optional trailing colon. The published-name path covers the tty, registered-name, `client-PID`, and `device-N` fallback ladder, so nameless Control clients can be targeted by the value returned from `list-clients`. It does not accept a final pathname basename, so `/dev/pts/3` admits `pts/3` but not `3` unless that is the exact client name. Collisions choose the globally oldest attached client by creation id, independent of session switches. The shared `device-N` alias remains a zz extension; popup, menu, confirm, refresh, and lock also retain numeric `N` and `client-N` aliases. A local terminal surface or Command client publishes `client-tty-v1:` whenever the tty is discoverable, independently from the additive `client-nested-v1` marker that a nonempty `$TMUX` enables. A local Control client publishes the same identity from terminal stdin only; piped stdin and remote endpoints omit the caller-host tty. Protocol v83 client facts expose the retained tty through `#{client_tty}` when the selected client has one. Unsupported `command-prompt -t`, `show-messages -t`, `send-keys -c`, and `suspend-client -t`, plus inert `set-buffer -t`, are outside this closure. | none on the common tmux-compatible selector shapes; native aliases remain zz extensions, and the unsupported or inert command flags keep their existing owners |
| Local Control terminal identity | Closed 2026-08-25 without a wire bump. A local Control hello carries its bounded cwd, `client-tty-v1:` only when stdin has a discoverable tty, and `client-nested-v1` only for a nonempty `$TMUX`. It never samples size, sends `client-size-v1:` or `ClientTerminalSize`, or infers geometry; `refresh-client -C` remains the explicit Control geometry path. Protocol v82's environment snapshot carries `TERM` when present, and protocol v83 client facts expose a terminal-backed Control tty while piped Control keeps it empty. The established `attach-session`, `new-session -A`, and `new-session -Ad` refusal paths require the marker plus an exact pane-tty match when they would attach an existing session. Fresh `new-session` and `-A` misses still create and attach, while duplicate and validation errors keep their existing precedence. Piped stdin is not nested-refused merely because `$TMUX` is set. Registration cleanup removes the retained facts. | none for local Control tty identity, nested intent, refusal gating, fresh-session behavior, or defined client-format empties |
| Read-only clients (`attach-session -r`, `switch-client -r`) | The daemon accounts a raw terminal `Key` and resolves it through the client's ordinary key tables, allowing the pin's `CMD_READONLY` command roster (`attach-session`, `copy-mode`, `detach-client`, `list-clients`, `send-keys`, `switch-client`) while still dropping PTY forwarding; other commands answer the pin's `client is read-only`. `send-keys` keeps the pin's second authorization layer: absence of `-X` is decided before full option and repeat parsing, so even unsupported `-M` reports read-only first. With `-X`, typed read-only-safe movement, history, line, word, paragraph, prompt, bracket, goto-line, set-mark, jump-to-mark, and cancel actions work. Selection, copying, search, rectangle, jump capture, and the pin-recognized but zz-unimplemented unsafe copy-line, selection-mode, scroll-exit, and search names reject; genuinely unknown and empty actions retain the pin's later no-op or no-mode path. A direct request authorizes its one invocation. A stored binding list uses the commands constructed for storage and preflights that frozen list as one all-or-nothing chain before any effect, without another user-alias lookup, matching the pin. A read-only local view effect cannot fan into another session's clients. Raw keys bypass retained choosers, command prompts, and `display-panes`, as tmux's writable-only prequeue does. Direct local scrolling and copy-mode entry/navigation work, update activity plus latest geometry once, and preserve bells. Paste, clear-history, raw mouse, mixed wheel, and application pane Focus remain blocked; rejected non-focus native actions, including mouse, still account once, retain the modal, and preserve the bell. Standalone terminal `Text` accounts once without writing the PTY or clearing the bell, while matching text after a key adds no second update. Browser key/text, divider resize, popup/menu/confirm actions, uploads, and agent prompts remain dropped. `client_flags` reports `read-only` without the pin's coupled `ignore-size`: explicit requested `ignore-size` now affects explicit and automatic client sizing, but `-r` deliberately does not add it. The pin's same-uid check on re-marking a read-only client is skipped by the single-user daemon. | **silent** for native dropped-input feedback; unsafe commands and bindings are loud; uncoupled `-r` ignore-size and same-uid policy are deliberate |
| Requested client flags (`attach-session -f`, `new-session -f`) | Closed 2026-08-27 without a wire bump. The daemon retains tmux's comma mutation grammar and reports common `read-only`, `ignore-size`, `active-pane`, and `no-detach-on-destroy` state; Control clients also consume `no-output`, `wait-exit`, and `pause-after`, including the pin's numeric-prefix and wrapping behavior. Unknown names are ignored, `!` clears except that read-only cannot clear itself, and the final repeated `-f` wins. Mutations follow target resolution, survive switch and detach, clear on unregister or replacement, and replay through the TUI only after a command actually succeeds and attaches. The full attached fixture covers missing targets, fresh and detached creation, switching, reattach, teardown, and the accepted `-r` difference; Rust tests cover terminal-open ordering and `new-session -A`. The later `clients.attach-sizing` slice consumes `ignore-size` for explicit and automatic client-derived sizing. Session destruction now computes the configured primary once, then applies the `on` or `no-detached` newest-session fallback only to clients retaining `no-detach-on-destroy`; focused tests cover the full policy matrix, and two real attached clients prove mixed fallback and exit behavior against the pin. `active-pane` remains retained while zz selects one shared pane per window. | retention, `ignore-size`, and `no-detach-on-destroy` consumption closed; **silent** consumer gap remains under `clients.active-pane` |
| Session activity core | `session_activity` retains Unix seconds, starts at `session_created`, and refreshes through the shared same- or other-session attach funnel and queued terminal input. Ordinary read-only `Key` messages and rejected read-only terminal-view input, including raw mouse motion, refresh before rejection and advance latest geometry without clearing bells. Writable chooser raw keys, dedicated actions, and terminal-view input refresh activity and advance latest geometry exactly once without clearing bells; activating another session then records the target attach as a second boundary. Read-only-safe local view actions bypass a retained chooser or `display-panes` overlay, reach the pane, and use the same once-only accounting. One bounded ordered queue per client correlates pane-and-lane Key-plus-Text pairs, so a match uses the Key result with no second update. Standalone writable terminal or browser text accounts after modal consumption; standalone read-only terminal text accounts without PTY input or a bell clear. Writable command-prompt consumption and valid `display-panes` selection do not refresh activity. An unmatched display-panes key, Escape, non-hover mouse action, or wheel closes the overlay and falls through ordinary input. Bare buttonless hover Motion remains consumed as a native presentation choice, and timeout fabricates no activity. Native client-theme notifications, resize, `switch-client -T`, and detached commands do not refresh activity. `S/t` and `list-sessions -O activity` use a separate logical MRU counter, so same-second activity still reorders deterministically with session name as the exact-tie break. | none for the closed core, chooser routing, committed text, modal accounting, and display-panes fallthrough edges; native browser input outside modal consumption retains zz's deliberate superset activity behavior |
| Session client focus | The `ClientFocus` shape introduced in protocol v73 separates client-window focus from pane/application focus. GPUI seeds desired focus only when construction finds an active window, leaves inactive construction unset until the first activation callback, and replays the latest value once after every successful attachment epoch; pane and sidebar transitions do not update it. The TUI assumes its outer terminal is initially foregrounded, caches focus changes while attachment is pending, and sends the latest client focus once after every successful `Attached` event. Real outer focus events additionally emit pane Focus only for an active terminal when no popup overlay is active; an active popup suppresses application Focus while retaining the client-window transition. Attachment never synthesizes pane focus. iOS sends its current scene state after the initial attach and every successful session or recovery attachment request, without replaying pane focus; scene transitions pair client focus with pane Focus when it retains a terminal input owner. Every successful attach independently advances latest geometry and recalculates affected panes, even with `focus-events` off. When that option is on, both client-focus directions update session and client activity exactly once, including read-only clients. FocusIn also becomes the geometry owner: `window-size latest` takes its rows, columns, and cell metrics, while manual, largest, and smallest keep their mode-correct rows and columns but refresh its cell metrics. FocusOut preserves the owner. Writable pane Focus alone still forwards to the application but changes neither activity nor geometry, so a paired client and pane signal touches activity once. Read-only pane Focus is rejected; its client-window transition travels through `ClientFocus` instead. Neither focus signal clears bells; the client-focus path is inert while the server option is off. A zz-side two-client regression with different retained geometries proves same-session attach ownership without focus events and mirrors the pinned FocusIn/latest rows-and-columns rule. `ClientFocus` is not CLI-drivable, so that half is not a differential-harness proof. The separate read-only fixture proves that zz accepts the notification and updates activity; it does not prove tmux `attach -r` resize behavior because tmux couples read-only with `ignore-size` and zz does not. | none for attach latest, the client signal, writable FocusIn/latest, or writable modal routing; the uncoupled read-only/ignore-size model remains under `clients.read-only-and-focus` |
| Session focus through writable overlays | With `focus-events` on, the daemon runs `ClientFocus` through the pinned writable prequeue before activity accounting. It dismisses the active status message and resumes frozen terminal publication, then closes `display-panes` and cancels its deadline. Key prompts submit `FocusIn` or `FocusOut` text and consume the transition. Numeric prompts submit without recording history and pass it; Text, Single, Incremental, and BackspaceExit prompts consume it and stay open. Choose-tree and choose-buffer keep their pane-mode routing. Read-only clients retain every modal and message while accounting both directions. FocusIn alone advances latest geometry; when that also changes an activity-sorted chooser, the daemon publishes the snapshot and independently refreshes the chooser. Neither direction clears bells. After accounting, writable focus dispatches synthetic `Any` through choose-tree, choose-buffer, active copy or command-output mode, then effective root. A transient binding wins; an unbound transient table falls back without retiring the mode. Read-only focus authorizes the complete selected binding before any effect. Disabled focus bypasses both accounting and dispatch. Exact `FocusIn` and `FocusOut` stay invalid key names. | behavior closed in focused daemon and protocol tests; pane `command-prompt -P` remains under `prompt.pane-rendered`; `ClientFocus` is not CLI-drivable, so there is no differential or canonical-suite claim |
| Session activity text edge | Closed 2026-08-25. The daemon keeps one 32-entry ordered queue per client for validated press or repeat keys whose `text_follows` bit is set. Each entry records its pane and Terminal or BrowserSurface lane. Text scans forward to the first same-pane, same-lane entry, retires only the skipped prefix, and consumes the match while preserving its suffix. Empty matching Text is inert and retires linked suppression; a no-match Text leaves the queue intact and is standalone. A two-browser-pane regression proves a skipped entry cannot retire the later bound key's suppression debt before its Text arrives, and bounded eviction retires debt on the evicted entry. Terminal command-output text accounts before it is swallowed; browser command-output text is consumed before activity. Detach, unregister or reconnect, and successful wire Attach clear the queue; a synchronous binding-driven switch preserves it. GPUI terminal standalone text and GPUI browser key-plus-text emission are source-tested. TUI keys remain unpaired; FFI exposes the explicit pair bit, and iOS uses standalone text plus unpaired key calls. | closed; a pair contributes at most one update, while writable modal consumption may contribute zero and read-only browser text retains its native silent drop |
| Session activity wake lifecycle | Pinned tmux refreshes activity when a suspended tty client receives `MSG_WAKEUP` or `MSG_UNLOCK`. zz has no suspended attached-client state or corresponding protocol message; reconnect and reattach use the ordinary attach seam. | accepted native lifecycle difference under `formats.session-activity-wake-lifecycle` |
| Client environment refresh | Closed 2026-08-27 in protocol v82 for the bounded UTF-8 snapshot. Every local or SSH-forwarded connection carries one immutable environment map. Fresh session creation, existing attach, native attach, Control attach, and targeted switch apply the effective session `update-environment` patterns against the invoking or selected client. Exact names, wildcard matches, missing-name unset markers, empty values, selected hidden values, `new-session -A`, repeated `-e`, `-E`, and the early `switch-client -T` return match pinned tmux. Attach refresh runs only after target and terminal preflight. The exact native parser accepts separate or bundled `-E` and routes that initial attach through daemon command execution; automatic TUI reconnect retains its existing local behavior. Session values survive disconnect, future panes consume them, and existing processes keep their startup environment. Debug output exposes counts rather than names or values. The full attached differential passes against pinned tmux `d77c9dc6`, and a native PTY integration test independently proves `-E` preservation followed by ordinary attach refresh. Unrepresentable Unix names or values are omitted without substitution and remain tracked under `clients.path-encoding`. | closed for UTF-8 client environments; non-UTF-8 bytes remain bounded and tracked |
| Session current window across clients | tmux stores one current window on the session, so `select-window` or `switch-client -t session:window` moves every client attached there. zz keeps `focused_windows` per client by design. One client can change windows without moving its peers, and peer rows from `list-clients -F '#{window_index}'` can differ from the pin. | **silent**, deliberate zz extension |
| `copy-mode` | `-k -S -s` rejected (`-e`/`-q`/`-M` and `-H` are implemented). | loud |
| `capture-pane` residue | Stdout versus named/automatic-buffer routing, stored trailing newlines, clustered value flags, and inclusive/reversed `-S`/`-E` ranges are differential-clean since 2026-08-23. Bounds expand in the target pane's format context; invalid or out-of-range values silently fall back to visible start/end like the pin. When an invalid `-E` reaches trailing blank viewport rows, tmux emits one newline per row while zz stops at the last retained content row. `-T` remains inert; saved-alternate capture, raw pending/grid bytes, hyperlinks, line flags, and line numbers remain outside the model. | **silent**, bounded for `-T` and trailing blank rows; loud for the six rejected transports |
| Top-level `source-file` paths and CLI diagnostics | Since protocol v72, each eligible local caller publishes one bounded daemon-host cwd; SSH callers publish none, and a non-UTF-8 or oversized cwd is omitted so the client can still connect. `-F` expands each declared path independently in the command's resolved pane context, then top-level relative paths are prefixed with a glob-escaped caller cwd before globbing. `-t` resolves that pane once and supplies it to both `-F` and replayed commands without changing the source cwd; a missing target follows `CMD_FIND_CANFAIL` and loads with an empty target context. `-n` constructs the complete file without applying bare environment assignments or replayed commands, retains lexer diagnostics and optional verbose output, and expands later tokens against the pre-file environment. Each file applies permitted bare assignments, expands aliases, and validates command names, flags, arity, callback arguments, and nested children before any command from that file runs. The first construction failure keeps earlier bare assignments and drops every command from that file. `-v` emits normalized `path:line: command` groups in declared-path, glob, and physical-line order, inherits through nested sources, and stays suppressed for Control clients. Each invocation constructs its matched files as independent units in declared-path and glob order before replay, completes its verbose batch, replays valid units in the same order, then appends buffered command-name and parser diagnostics. One invalid unit does not suppress later siblings. Source no-match, glob, and actual OS or path read failures retain their existing error channels. Nested invocations form depth-first transcript frames. Command clients receive the transcript once on stdout. For valid successful replay and `-v` output, Interactive clients open one command-output view without duplicate Info or Warning events; parser diagnostics may still publish their existing Warning summary. Protocol v79 closes the local TUI keyboard-navigation contract for that view, including live copy tables, line and page movement, search, selection, paste-buffer copy, and the stock vi/emacs exit distinction. On Unix, zz quotes the cwd bytewise and calls `glob(3)` with flags zero like the pin. Backslash escaping, leading-dot exclusion, ordinary repeated-star behavior, malformed-pattern handling, and C-locale per-pattern order therefore agree. Declared paths retain caller order, and a quiet miss does not stop later paths. This matches an unattached tmux command client with a representable cwd, including literal glob metacharacters in the cwd. `source-file` also matches the pin's tilde boundary: the config parser may expand a leading tilde before command execution, but a tilde that reaches path resolution literally remains cwd-relative even when daemon HOME contains glob metacharacters. For commands issued by an attached client, zz now prefers the invoking client's retained session cwd like the pin; `source-file -t` remains only the format and replay target. The full attached fixture separates command cwd from session cwd with decoys, and a focused daemon test adds a third `source-file -t` target cwd decoy. Invalid-line diagnostics append to STDOUT as `path:line: message` in encounter order with duplicates kept and exit 1; a loud glob miss writes `No such file or directory: <declared path>` to STDERR and exits 1; a quiet miss stays silent at rc 0; mixed input populates both streams at rc 1. The zz-only `skipped N unsupported tmux command(s): …` summary goes to STDERR at rc 0 for parser-owned replay skips. An unsupported zz-only command inside a synchronous inserted list gets an empty success guard and continues its later siblings, but does not join that `ConfigLoadReport` summary. Interactive clients receive the declared-path warning. A direct all-miss Control invocation prints its diagnostics inside `%error` and stops the rest of that input line. If at least one declared path matches, the direct errors stay inside a `%end` frame and the line continues. Matched parser diagnostics use `%config-error` and also let the line continue. A construction failure produces one located `%config-error` without a failed-command guard; the loader delays sibling construction warnings until the batch finishes replay. Native Windows keeps the Rust matcher: recursive `**` and its escaping rules are a zz platform extension because tmux has no native Windows oracle. `-` stdin is refused loudly on stderr at rc 1. | none for Unix glob matching, ordinary representable cwd prefixing, cwd escaping, literal tilde handling, declared-path order, parse-only construction, pane targeting, verbose and replayed transcript delivery, Control suppression, CLI-stream wording, direct Control framing, the named local TUI command-output navigation contract, and attached session-cwd selection; native Windows intentionally keeps its own glob dialect; non-UTF-8 cwd omission remains bounded; stdin remains loud |
| Non-UTF-8 config-file bytes | Measured against pinned tmux `d77c9dc6`. A config containing only byte `0xff` exits 0 with empty streams when sourced by a Command client. Direct Control emits one flags-1 success guard and exits 0. Synchronous `if-shell` sourcing emits successful flags-1 parent and source guards, no visible diagnostic, and still runs the later root command. zz reads config with `read_to_string`, emits `stream did not contain valid UTF-8` through a typed Error, and returns status 1. This single byte can act as EOF in the pinned build, so it does not prove general byte semantics. A pinned matrix must cover isolated and embedded bytes before zz chooses a byte-oriented parser contract. | loud and status-changing, tracked under `config.non-utf8-file-bytes`; source-path byte encoding remains separate |
| Nested `source-file` base | Pinned tmux repeats `server_client_get_cwd` for each nested `source-file`, so `a/entry.conf` containing `source-file leaf.conf` reads `<client cwd>/leaf.conf`. zz now snapshots the base selected for a registered client's top-level source and passes it through recursive replay. The nested source keeps that base after an ordinary sourced command executes through `ClientId::MAX` and clears the mutable execution-context cwd. Runtime `source-file` forwards the snapshot when it loads the active default `zz/mux.conf` through the ordinary path; direct zz-native `reload-config` forwards it through the separate native reset path. Slice 10ag closes the cold startup bootstrap path. The differential fixture includes a containing-file decoy. CLI fixtures use a caller cwd with spaces and glob metacharacters and cover ordinary replay, active-default ordinary loading, and direct native reload with a decoy beside `mux.conf`. Command, Control, and Interactive clients share this daemon path. Attached clients select the invoking client's retained session cwd before the same nested snapshot is frozen. Ordinary replay still runs through the sentinel client. Successful Command and attached output now follow the per-invocation verbose, replay, and buffered command-name or parser diagnostic transcript rule. Source no-match, glob, and actual OS or path read failures keep their existing error channels. Parser-owned Control recursion and aliases resolved to `source-file` keep the closed cross-depth guard order. Synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C` now retain flags-1 source framing. Immediate-hook and background-callback flags-0 framing closed later under `control-mode.hook-command-frames` and `control-mode.background-inserted-command-frames`; v78 later closed source-read placement and completion numbering, while Control hook cwd selection remains under `source-file.sourced-hook-client-cwd`. | none for registered-client nested rebasing, active-default ordinary loading, direct native reload, attached base selection, successful replay output, or parser-owned per-command sourced guards; hard-disconnect queue cancellation and Control sourced-hook cwd remain in those named groups |
| Control sourced-command hook `source-file` base | Pinned tmux copies the original queue client onto each command loaded from a file. A hook raised by one of those commands inherits that client and its cwd. Command replay retains the caller cwd. Control hook framing clears the replay client before the hook runs, so a source from that hook falls back to daemon HOME instead of the outer queue client's cwd. | **silent**, tracked under `source-file.sourced-hook-client-cwd` |
| Event-hook `source-file` base | Command and immediate hooks retain the invoking client on both sides. Deferred event hooks differ: zz executes them with its sentinel client and can fall back to home, while tmux selects the current or best attached client and uses that client's session cwd. | **silent**, tracked under `source-file.event-hook-client-cwd` |
| Startup `source-file` base | Closed 2026-08-29 in slice 10ag. A cold auto-spawning launcher captures an absolute valid UTF-8 cwd within 16 KiB and passes it through private `--bootstrap-client-cwd`; direct daemon starts carry none. Startup replay prefers that base over session, registered reentry-client, command-context, HOME, and root fallbacks, carries it through nested sources and literal metacharacter paths, then clears it on success or error. Later runtime sources use the registered client cwd. The isolated differential passes exactly on both engines without a public protocol change. | closed under `source-file.startup-client-cwd`; event-hook and sourced Control-hook cwd retain separate owners |
| Control kill-server response order | Closed 2026-08-29 in slice 10ah. Shutdown atomically freezes new response admissions, waits a bounded interval for active Control and Command responses to enter their registered writers, publishes `ServerStopping`, and drains those writers through one shared deadline. Late requests do not execute. The foreground thread holds the listener until the response and writer phases finish, removes the endpoint while it still owns that listener, and only then drops it. Controlled tests cover active and late admission, stalled and disconnected writers, replacement binding during cleanup, one final Control exit, and immediate fresh-daemon startup. | closed under `control-mode.kill-server-response-order`; no wire change |
| Control exit pane output | Closed 2026-08-30 in slice 10ai. Control stdin observation starts before initial preparation. EOF or a blank Return removes deferred pane-byte records and rejects later `%output` and `%extended-output`, while output written before the return remains visible. Config diagnostics, command guards and output, notifications, pause and continue records, retained return status, and one final `%exit` drain in order. Early EOF retains at most the first admitted stdin command instead of running later buffered mutations. Thirty-three focused units, the return matrix, held-command and long-lived output probes, and the full eight-case startup diagnostic pass against pinned tmux. | closed under `control-mode.exit-pane-output`; hard-disconnect queue cancellation, wait behavior, and transport pressure retain separate owners |
| Control frames during sourced config replay | Protocol v76 carries one tail-tag-47 `SourcedCommandGuard` for each parser-owned replayed command that survives command-name resolution. An alias resolved to `source-file` before replay retains this path. Unknown or ambiguous command names and malformed alias names publish a located Warning that Control renders as `%config-error`, without a guard. Control writes each guard as a flags-1 frame after the direct outer frame: ordinary success and quiet all-miss use an empty `%end`, mixed hit and miss keeps its diagnostic inside `%end`, and all-miss, flag or arity failure, runtime failure, or depth refusal ends `%error`. Runtime failures alone set `client_failure`, which sets retained retval 1 independently of the terminator. Guards defer FIFO without leaking into the next command. Protocol v78 sends matched OS and path read failures as typed `ControlSourceFileEvent::ReadError` events; the writer prints each diagnostic raw after the source guard and retains status 1. One invisible `Complete` event consumes a command number after each depth-admitted source invocation's descendants. Depth refusals and dispatch-time syntax, arity, and flag rejections consume none. Other config and lexer Warning prose still uses the config classifier. The existing loader preflights every declared path for one source command before recursion. A focused regression and the then-six-step differential prove the root missing-path guard, middle missing-path guard, then leaf output guard order, each exactly once, with no production change. Synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C` now keep that flags-1 recipient. Per-client and per-thread capture publishes the containing command before each inserted command and an inserted source before its children without folding or leakage. An unsupported zz-only inserted command gets an empty success guard and later siblings continue without joining `ConfigLoadReport`'s skipped summary. An unknown command inside the child keeps the parent and source success guards, then emits `%config-error` without its own guard. Immediate hooks and shell-evaluated `if-shell -b` or `run-shell -bC` callbacks now use flags 0 under their later closed rows. The synchronous closure reuses v76 without a wire change. The strict 12-step `smoke/source-file-control` differential covers the v78 parser and hook read timelines with zero differences and no skips. | guard shape, termination, runtime-failure retval, FIFO deferral, and cross-depth containing-before-child order behave for parser-owned resolved command names and alias recursion; synchronous foreground inserted lists behave; immediate-hook and background-callback flags-0 framing closed later; v78 source-read placement and numbering behave; return and detach precedence closed separately |
| Immediate Control command-hook frames | Closed 2026-08-26. Protocol v77 renames tail-tag-47 in place to `ControlCommandGuard { output, error, sticky_failure, flags }`. Parser replay keeps flags 1. Immediate `after-*` and `command-error` hooks retain the originating Control recipient separately from `replay_client`, enter a no-hooks state, and emit one flags-0 frame for every hook command, source command, and sourced child. Hook arrays stay ordered; one failure stops only its command-list entry, later entries continue, output does not fold into the trigger, and no hook automatically retriggers itself. Unknown or ambiguous sourced names emit only `%config-error` and do not run `command-error`. Alias classification and execution use one frozen resolution. A mixed source miss and hit ends `%end` with `sticky_failure`, so later work continues while status remains 1. At the hook closure, the strict ten-step `smoke/source-file-control` differential was clean with no skips. The v76 hook-residue clauses in nearby historical rows are superseded by this row. | closed for immediate command hooks; background `if-shell -b` and `run-shell -bC` framing closed later under `control-mode.background-inserted-command-frames`; protocol v78 later closed parser and hook-source read placement plus completion numbering; hard-disconnect queue cancellation, deferred event hooks, and hook cwd selection remain separate |
| Background Control inserted-command frames | Closed 2026-08-26. Shell-evaluated `if-shell -b` and `run-shell -bC` retain the originating Control recipient separately from `replay_client`, close their triggering flags-1 frame before later callback work, and emit one flags-0 frame for each inserted command, source command, and sourced child. Later flags-1 input may overtake the callback. Missing sources and runtime failures end `%error` and make status 1 sticky; false conditions select the else list. Malformed delayed lists stay silent and status-neutral. Ordinary `run-shell -b`, immediate `if-shell -bF`, and foreground `run-shell -C` retain their existing paths. Before callback execution begins, zz checks that the exact originating Control client is still registered; a client gone before that point gets no callback frame or side effect, and output never migrates to a replacement. The strict eleven-step `smoke/source-file-control` differential is clean with no skips. | closed for callback framing and disconnect before callback entry; hard disconnect after an immediate hook or source queue has started remains under `control-mode.disconnect-cancels-command-queue`; protocol v78 later closed source-read placement and completion numbering |
| Control return status and detach precedence | Closed 2026-08-25. Direct runtime response errors set retval 1 unless they are flags-1 parse or preparation errors. Parser-owned sourced runtime failures, nonruntime source failures represented by a nonzero `source-file` success, and v78 source-read failures and synchronous inserted runtime failures also set 1. Invalid UTF-8 config content is excluded and tracked under `config.non-utf8-file-bytes`. Generic nonzero successes such as `run-shell 'exit 3'` and parse or preparation failures do not set or change retval: a fresh client remains at 0, while a prior sticky failure stays at 1. EOF and a blank line snapshot the current value. A Return captured while a preceding non-detach command waits keeps that arrival-time snapshot and precedes later queued stdin commands, including detach. A Return observed while self-detach itself waits is discarded when the caller's `Detached` event arrives. Explicit self-detach after a completed failure and self-detach queued while another command is open exit 0 when stdin remains open. Only the caller's actual `Detached` event proves self-detach: `detach-client -a`, `-t` naming another client, `-s` excluding the caller, no-victim forms, and their aliases keep the caller alive and preserve a pending Return; self aliases exit 0. The detach command's response `%end` precedes `%exit`. Twenty-seven Control units, 34 serialized CLI tests, two five-iteration race probes, the focused eight-step differential, and a manual `detach-client -a` probe cover the matrix. | closed; no wire change; the focused eight-step run does not refresh the stored canonical row, which remains at three steps |
| Nested `source-file` depth | Measured 2026-08-24 and guard placement closed 2026-08-25. Counting the initial `source-file` as invocation 1, both sides run 50 concurrent source invocations and refuse invocation 51 before any of its paths are matched or loaded. Command stderr is `too many nested files` at rc 1, an attached tty shows the capitalized `Too many nested files`, and Control carries the same lowercase text inside the rejected nested command's own flags-1 `%begin`/`%error` guard while the outer typed line continues. `-q` does not suppress the refusal, one diagnostic covers a refused command rather than each of its paths, the refused paths are never globbed or loaded, and the containing file keeps executing its later physical lines. The refused source's own same-line `;` sibling is dropped on both sides, while the matched parent `source-file` stays on the asynchronous wait path and therefore runs its own same-line sibling on both sides. A malformed invocation at the refused depth is diagnosed as malformed rather than as depth on both sides: the pin rejects it while parsing the containing file and never consults its depth guard, and zz reaches the same precedence by running the depth guard after the command's own flag and positional validation. The missing-path form matches `command source-file: too few arguments (need at least 1)`, and the 2026-08-28 shared parser closure matches `command source-file: unknown flag -Z`. The pin's eager command construction abandons the rest of the containing file after that malformed invocation; zz constructs and dispatches replay commands one at a time, so later physical lines still run. | the depth wording, count, `-q`, per-command granularity, guard placement, malformed command text, later-line continuation, and same-line removal behave; replay command-construction atomicity remains under `mux.chain-parse-abort` |
| Startup `source-file` depth accounting and causes | Both sides share one cumulative 50-command source budget across every startup root. Top-level roots do not count; quiet misses consume slots; one command with many paths consumes one slot; command 51 and later retain `<file>:<line>: too many nested files`; later ordinary commands continue. Runtime sequential source commands stay unbounded. zz's native `reload-config` replays the whole root under one fresh startup budget of its own, so a reload lands the state a fresh start would; the pin has no reload command and its `cfg_finished` gate never re-opens the cumulative budget after startup. Protocol v80 closes retained delivery. zz reads and parses every root before replay, keeps normalized root and nested read errors, parser and unknown-command diagnostics, unsupported and runtime failures, and successful `display-message -p` output, discards list-style output, and uses the completion line for successful physical multiline commands. Root causes precede replay causes; replay stays root-ordered and nested depth-first. Detached Command startup stays rc 0 with empty streams and does not drain the set. The spawn-owner Control receives the raw bounded vector after `ServerHello` and before its first `%begin`; late Control receives it after `Attached` inside the attach frame. Each cause receives one `%config-error` prefix. An attached Interactive winner opens a PTY-free `configuration errors` view with an ordered, UTF-8-safe 64 KiB preview that replaces every Unicode control except LF and TAB and carries an explicit truncation notice. The startup view uses a pinned Ghostty 64 MiB byte history cap; ordinary output keeps its 100,000-byte setting. The post-spawn owner commits after event admission. Attached delivery commits globally only after `Attached`, the diagnostic, resync, and mux options remain admitted; failure retains the set, and daemon restart rebuilds it. | none for Control placement, ordering, detached silence, or one-shot delivery; Interactive deliberately exposes a bounded 64 KiB sanitized preview rather than exact recovery of the full retained 1 MiB vector |
| Control-mode asynchronous shell output | Closed 2026-08-26 for targetless and invalid-target foreground output. Protocol v81 appends `ControlCommandOutput` at tail tag 50. Both sides close the direct or sourced command with an empty flags-1 `%end`, print captured output plus any nonzero or signal diagnostic raw, then continue the next direct command or sourced same-line sibling in its own guard. zz sends the event to the exact originating Control client; peers receive nothing. Embedded LF and percent-prefixed lines remain literal, one missing trailing LF is supplied, and the event does not change Control retval. Foreground `run-shell -C` stays synchronous inside its frame. Measured pinned `run-shell -t <resolved-pane>` and ordinary `run-shell -b` enter tmux's shared pane view. zz instead opens its native per-Interactive command-output view for attached viewers of that pane and emits no raw Control text or `%pane-mode-changed`; GUI ownership makes this a deliberate presentation divergence. A pinned foreground Control disconnect can crash the tmux server while the shell waits. zz keeps its daemon alive and does not emulate that failure. The strict 12-step `smoke/source-file-control` differential has no differences or skips. | closed for raw targetless and invalid-target foreground placement, recipient identity, direct and sourced guard order, LF and literal-percent behavior, continuation, and retval; resolved-target and background presentation remain a deliberate native-view divergence; the pinned disconnect crash remains out of scope |
| Control-mode diagnostic identity | Protocol v77 carries source-command termination and sticky status in `ControlCommandGuard` without prose classification. Protocol v78 gives matched OS and path source-read failures typed `ControlSourceFileEvent::ReadError` identity, so daemon routing does not depend on wording or pathname shape. The Control writer renders that event as raw unframed text after the source guard and retains status 1. `ControlSourceFileEvent::Complete` renders nothing and advances the command number after the invocation's descendants. Invalid UTF-8 config content is excluded because the pinned lone-`0xff` case succeeds; `config.non-utf8-file-bytes` owns that mismatch. Background inserted-command failures use their closed flags-0 frame path; only Interactive status messages capitalize the first character. Gesture, prompt, paste, and command-output Error producers remain Interactive-only. The pinned copy-pipe closure confirms that worker failures also stay silent on Control. Config summaries and lexer-owned diagnostics still travel as generic Warning events and use the `%config-error` prose classifier. The known-family Warning fallback remains for legacy producers, while the exact protocol handshake rejects client-daemon version skew before event shapes can mix. | typed source-read identity, raw placement, invisible completion numbering, sourced-command guards, and copy-pipe silence behave; config identity remains under `control-mode.diagnostic-typing` |
| Control-mode asynchronous copy-pipe failure | Pinned tmux starts a copy-pipe job with `JOB_NOWAIT` and no completion callback. A delayed exit 7 therefore arrives after the initiating command has succeeded but emits no `%message`, `%error`, or extra command guard. The action still creates its paste buffer and cancels copy mode. zz already withholds copy-pipe Error events from Control subscribers and routes them only to an Interactive invoker or attached Interactive fallback. The attached fixture proves the silent Control transcript and completed mode transition on both engines. | none on Control; zz deliberately retains its native Interactive error notification |
| Config whole-file lexer and parser abort | zz has matched the pin's first-diagnostic file abort since the 2026-08-19 parser work. The first lexer or parser diagnostic clears every command built from the file, stops the scan, preserves environment assignments reduced before the error, and suppresses later diagnostics. The post-10v rerank moved the stale active item into closed `config.parser-abort`. Slice 10y closes the file-level alias snapshot. Slice 10z closes command construction for each config or source file before effects, including parse-only validation, sibling and nested isolation, Control warning placement, and verbose alias traces. | whole-file lexer and parser abort is closed; alias snapshotting closed in slice 10y; file-unit command construction closed in slice 10z |
| Config and source replay alias snapshots | Closed 2026-08-29 in slice 10y. For each parsed file, zz applies permitted environment assignments and prepares every original invocation into an alias-expanded command or stored preparation error under one engine lock before replay. Earlier same-line or later-line alias mutations cannot affect later commands from that file. Startup roots and top-level matched source batches finish construction before their batch replay; a nested source receives a fresh snapshot when its parent source command runs. Stored preparation errors retain source, physical-group, and replay-position metadata. Control warning-versus-guard classification is frozen with the stored error. `source-file -n` keeps its no-effect result and suppresses stored alias preparation errors. Four focused daemon tests cover startup roots, same-file mutation, file environment timing, multi-file batches, nested refresh, parse-only behavior, deferred errors, and Control classification. The two-step `smoke/config-alias-parse-unit` differential matches in every channel. At that checkpoint, eager name, flag, arity, callback, and nested-child construction remained under `mux.chain-parse-abort`, while empty and multi-command execution remained under `aliases.command-bodies`. Slice 10z later closed file-unit construction, and the 2026-08-30 command-body closure made valid empty and multi-command aliases part of the prepared replay unit. | none for the alias snapshot or valid alias bodies; closed under `aliases.config-parse-unit`, `mux.chain-parse-abort`, and `aliases.command-bodies` |
| Same-line command groups in sourced files | Measured 2026-08-24. Both sides key replay groups by the parser-owned source and physical line. A synchronous invalid or runtime command error, a depth-refused nested source, and a loud `source-file` no-match or glob error with zero files drop only the later `;` siblings from that group; the next physical line still runs, and a quiet no-match is success. A matched `source-file` takes the asynchronous wait path, so child runtime, parser, and read failures plus a mixed missing-and-matched invocation do not prune the parent line; zz retains a child read failure in its load report. A pinned directory-read probe returned rc 1 with `Input/output error: <path>` while both the parent same-line and next-line markers ran. An asynchronously failing `run-shell` also leaves its sibling. Both sides keep the same-line sibling for a `-` path, while zz's stdin transport gap remains under `protocol.binary-streams`. Equal line numbers from separate files do not collide. zz-classified unsupported capability gaps now skip and continue later same-line siblings; before this slice they pruned those siblings. That continuation helps zz config imports, but it has no pinned proof because the corresponding commands are unsupported in zz. The same policy gives an unsupported command inside a synchronous inserted list an empty success guard and continuation, but that path does not join `ConfigLoadReport`'s skipped summary. Control prepares a complete input line and aborts a preparation error before effects. Slice 10y closes replay alias observation for each parsed file. Slice 10z constructs every command group in a file before effects. Construction failure drops all commands from that file; runtime target and effect failures still prune their physical group while later lines continue. | pinned continuation behavior is proved for the supported error cases above; alias snapshotting closed in slice 10y; file-unit construction closed in slice 10z; UnsupportedCommand continuation is new and pin-unproven; sourced guard placement behaves; whole-file lexer and parser abort is closed; cross-depth order closed separately |
| Local CLI command-vector preparation | Against an already-running compatible daemon, the local CLI scans its complete prepared vector before stdin capture, attach or TUI routing, and execution. A later unknown name has the pinned error shape and prevents every earlier effect. An unparsable alias body also prevents effects and retains zz's loud `unknown command: <alias>` shape; the 2026-08-30 command-body closure covers valid empty and multi-command bodies, not malformed text. Slice 10u extends that preparation for a registered `ClientKind::Command`: the daemon applies the existing static tmux grammar to each ordinary invocation with no user-alias match. Flags, arity, required values, and nested command blocks validate across the vector before the first effect. Callback construction and user-alias validation keep their prior paths, while native zz names remain runtime-owned. The sole generic-validation bypass covers exact unaliased `attach` and `attach-session` at vector index zero, where the CLI's private parser owns the positional-session and `--restart-daemon` extensions. Later exact spellings and every user-alias expansion to either attach name use the ordinary catalog. Runtime target and effect failures remain sequential, preserving earlier effects and pruning later commands. Control preparation and framing remain unchanged. Against a missing local socket, the CLI keeps its alias-free raw-vector pass before routing, stdin capture, TUI handoff, daemon spawn, startup config, or effects. A successful cold pass generation-identifies the spawned daemon, which prepares the vector under one post-config alias snapshot; the exclusive bootstrap lease retains its existing commit and abort rules. The strict `smoke/cli-chain-parse-abort` scenario stays at three harness steps and now runs six warm probes for unknown name, invalid flag, excessive arity, missing value, later `attach`, and later `attach-session`, all with zero differences. Config and source-file replay remain under `mux.chain-parse-abort`; remote `--host`, replay alias snapshots, and runtime rollback remain excluded. Slice 10u changes neither the protocol nor the snapshot schema. | live, cold, and warm local parse atomicity is closed for the bounded paths above; native zz grammar, remote preparation, config and source replay, alias-snapshot changes, and runtime rollback remain outside the closure |
| Runtime failures of replayed commands | Closed 2026-08-25 against pinned tmux. Config replay records runtime failures in encounter order. A missing `kill-session` target and a well-formed `set-option` with an unknown name emit the pin's bare text on Command stderr at rc 1, inside the replayed command's Control `%error` guard with `client_failure` set, and as capitalized attached warnings. Later physical lines still run, and successful stdout before and after the failure remains in the invocation transcript. An outer `source-file` propagates the inner error and nonzero status without blocking inner or outer continuation. Each invocation appends only buffered command-name and parser diagnostics to the transcript after replay. Runtime, no-match, glob, and actual OS or path read failures retain their existing error channels. Non-UTF-8 config content remains under `config.non-utf8-file-bytes`. Unknown command names and malformed set-option syntax keep the existing file-prefixed parse-diagnostic path. Parser-owned Control recursion and aliases retain their guards. Synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C` now retain flags-1 guards. Immediate-hook and background-callback flags-0 framing closed later. Clientless startup remains separate. | none for the adopted Command and Interactive transcript, error channel, exit status, parser-owned sourced guard, invocation ordering, and continuation; v78 later closed Control source-read placement and completion numbering; startup delivery remains under `config.startup-diagnostic-delivery` |
| Output of successful replayed commands | Closed 2026-08-25. Runtime `source-file` retains one transcript per invocation. It parses every declared and globbed match before replay, appends the complete `-v` batch in declared-path, glob, and physical-line order, replays parsed files in match order, then appends buffered command-name and parser diagnostics. Source no-match, glob, and actual OS or path read failures retain their existing error channels. Non-UTF-8 config content remains under `config.non-utf8-file-bytes`. A nested source inserts its own complete verbose, replay, command-diagnostic frame at the parent command's replay position, so nested frames are depth-first. This is per-invocation batching, not a claim of physical verbose and replay interleaving. Command clients receive sourced `display-message -p`, `list-sessions`, hook, and continuation output once on stdout. For valid successful replay and `-v` output, Interactive clients open one command-output view rendered from that transcript, subject to the existing command-output size bound, without duplicate Info or Warning events. Parser diagnostics may still publish their existing Warning summary. Successful output leaves stderr empty and status zero. A runtime failure keeps its stderr and status 1 while stdout before it, hook output, later output, and list output remain ordered. A bare assignment in an earlier top-level file runs while the invocation parses, affects a later file's conditional, and persists. A replayed `set-environment` runs after every top-level match has been parsed, so it persists after replay but cannot change a later file's selected branch. With `-n`, neither assignment nor command effects apply, later parse-only files see the assignment as absent, and `-v` still reports the selected branch. Direct parser-owned Control replay and aliases resolved to `source-file` retain their flags-1 guards. Synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C` now use the same flags-1 path. Immediate-hook and background-callback flags-0 framing closed later. Clientless startup creates no replay transcript, so the detached launcher has empty stdout and stderr; a separate manual probe of pinned tmux `d77c9dc6`, outside the 12-step runtime scenario, found that startup `display-message -p` becomes a located config cause while list output is discarded. | transcript delivery, per-invocation ordering, nested depth-first framing, channel and status separation, assignment timing, and parse-only behavior are closed; protocol v79 later closed the local TUI output-navigation contract; config byte input and retained startup-cause delivery remain under their named groups; v78 later closed Control source-read placement and completion numbering |
| Active default config in a multi-file `source-file` | Closed 2026-08-25. Runtime `source-file` parses every active-default match in declared-path and glob order, then replays those matches in the same order instead of entering native reload. Declared default, after, and default paths apply as `DAD`; a loud miss returns status 1 without stopping later matches; and ordinary diagnostics plus `-v` lines retain declared path and glob order. The invocation presents one complete verbose batch, then replay, then buffered command-name and parser diagnostics; source no-match, glob, and actual OS or path read failures retain their existing error channels; it does not physically interleave verbose rows with replay output. Explicit zz-native `reload-config` still rediscovers the first existing candidate, replaces `#{config_files}`, resets key tables, rebuilds appearance, and reapplies stored mux overrides. Startup first-existing discovery and ordered explicit `-f` roots remain intentional; parse-only and nested paths are unchanged. Focused CLI and daemon tests, strict clippy, fmt, and the 12-step diagnostics, 40-step format, and six-step Control differential pass with zero differences and no skips. This makes no canonical-suite claim. | none on runtime active-default ordering; native reload and startup retain their documented zz-owned behavior |
| `mouse` / `escape-time` | Behaving since 2026-08-21 (Wave B2/B3). zz-tui gates the outer-terminal mouse modes (`?1003h`/`?1006h`/`?1016h`) on the session-effective `Mouse` value from the v71 publication, emits/retracts them live on `MuxOptionsChanged` (the pin's default is on: the reference builds with `-DTMUX_MOUSE=1`), and the daemon drops mouse-originated `TerminalView` input from terminal-surface clients when the effective value is off; the GUI's native mouse stays ungated per decision 6. With the option off, an application inside a pane can still use the mouse exactly as the pin documents (`options-table.c` mouse help; `server-client.c` forward_key): the outer modes also follow the active pane's own `mouse_tracking`, events forward straight to the tracking pane under the cursor with every chrome branch skipped, and the daemon admits them for panes whose app requested tracking. Chrome mouse (status clicks, sidebar, dividers, focus clicks) remains available only while the option is on — matching the pin, whose mouse key bindings also fire only then. `escape-time` replaces the TUI's old 25 ms escape fold timeout (pin default 10 ms, 0 clamps to 1 like `tty_keys_next`). Both keys are config-writable through `MuxOptionKey::from_config_key` with the standard reload-reapply semantics. | none — behaving |
| `set-titles` empty expansion | With `set-titles on` and a `set-titles-string` that expands to the empty string, zz publishes an empty `StatusLine.title`: the GUI reverts to its native title and zz-tui writes no OSC, where the pin's `server_client_set_title` would set an empty terminal title. Empty doubles as the "option off" wire state, so this narrow edge is deliberate. | **silent**, narrow and deliberate |
| `automatic-rename` / `automatic-rename-format` | Runtime command changes update `Window.name` while automatic rename is on, and the configured format is expanded with pane facts. Explicit `rename-window`, `new-window -n`, or a named first window pins a window-local `off`. zz refreshes when its runtime fact changes rather than on tmux's 500 ms timer, so a process transition that the sampler has not observed can lag differently. | **silent**, bounded timing residue |
| `aggressive-resize` + `window-size` | Since 2026-08-20 `aggressive-resize` is a candidate FILTER (ON = clients focused on the window; OFF = zz's viewer set, a per-client-focus stand-in for the pin's linked-window `session_has`) and `window-size` is the AGGREGATION policy. `latest` picks the most-recent-input owner, while `largest`/`smallest` aggregate componentwise. Manual sizing now has higher precedence than either: `resize-window` stores the durable layout extent, selects a local `window-size manual`, and client measurements cannot overwrite it. Switching an already resized window away from manual and back uses its then-current layout in zz; tmux retains a separate last-manual extent. | **silent**, bounded only across a manual → automatic → manual transition |
| `resize-window` client-derived and out-of-range forms | Closed for the practical 1..=10,000-cell surface on 2026-08-27. `-A` and `-a` validate target and numeric forms first, then choose component-wise largest or smallest retained geometry; `-A` wins when both are present. Eligible Interactive and GUI clients are attached to the target session, with status rows removed and GUI projection used when no outer size exists. Control clients count only after `refresh-client -C`; per-window Control sizes override global sizes and cap each aggregate dimension. `ignore-size` follows tmux's global fallback: ignored clients are excluded while any attached unignored client exists anywhere, then count when all attached clients are ignored. An empty candidate set uses the target session's effective `default-size`. The final size clamps to 1..=10,000, discards cell metrics, and becomes a durable manual extent. The full multi-client attached differential proves crossed extrema, precedence, unrelated exclusion, manual freeze, ignore fallback, and default fallback; focused daemon tests cover status, GUI, Control, automatic modes, and bounds. No protocol, client, wire, or snapshot-schema change was needed. A relative adjustment outside the practical range still differs only in the hidden request fact: both clamp effective geometry, but tmux may expose its separate unclamped manual request while zz reports the durable effective layout extent. | none for `-A`/`-a` or effective practical geometry; **silent**, bounded only in the hidden out-of-range manual request fact |
| Client targeting and requested detach | Closed 2026-08-25 against pinned `cmd-find.c`, `cmd-detach-client.c`, and `server-client.c`. One daemon resolver serves `detach-client` and `switch-client`: explicit targets use the supported matcher above before any `-s` lookup. Targetless Interactive and Control commands select themselves; a Command client first uses the best client on its origin pane's session, then the best client on the most recently active attached session. `detach-client -s` wins over `-a` and a missing source session quietly does nothing; `-a` detaches every peer except the resolved target. Explicit `-t` resolves before `-s`, including its error. Read-only clients may detach only themselves. Requested detach carries the existing Requested reason without a by-client; `attach-session -d` keeps Evicted. Local terminal surfaces publish their real tty even outside nested tmux, while SSH clients omit the caller-host tty. The later `clients.tty-basename-targeting` closure aligned every supported selector caller, removed final-basename matching, and fixed global creation-order collision precedence. The later local-Control closure extended only stdin-backed tty identity and nonempty-`$TMUX` intent to that client kind. Sequential daemon coverage passed 598/598. Focused selector tests, a debug build, strict daemon clippy, and fmt passed. Scoped zz and pinned-tmux tty guards passed, but the full attached-client harness later blocked on unrelated nested-attach interleaving, so this is not a full-harness or canonical-suite claim. `detach-client -E` and the parent-HUP actions `attach-session -x`, attaching `new-session -X`, and `detach-client -P` remain separate gaps. | none on bare, `-a`, `-s`, supported selectors, requested event classification, or eviction classification |
| `display-message` client selection and format fallback | `cmd-display-message.c` uses the explicit `-c` destination for formats only when that client belongs to the retained target session. Otherwise a valid target selects its highest-activity client, widening across attached sessions only when that target has none. The oldest-created client wins an activity tie. `client_session` comes from the selected client's attachment; session, window, and pane facts stay target-scoped. Zero clients and a missing target session leave client facts empty. Protocol v83's shared client-fact path now covers the formerly missing no-`-c` attached-target case. Nonprinting delivery, destination-owned duration and message state, CANFAIL target fallback, and printing behavior keep their existing contracts. The full attached-client fixture passes explicit, implicit attached, and Control format selection against pinned tmux `d77c9dc6`. Bare `-t =` and `{mouse}` still need bound mouse context, and relative or special pane targets remain incomplete. | none on client selection for supported targets; mouse target context and relative or special grammar remain tracked |
| `display-time` | Status-message toasts consume the configured milliseconds, and since Wave D run 3 (2026-08-22) the daemon owns that timer: `display-message` without `-d` reads the destination client's attached session value exactly like the pin's `status_message_set` with `delay == -1`, independently from the pane session selected by `-t`; `-d` overrides it per invocation. A zero installs no deadline and waits for writable input. A non-release key press, a nonempty bulk `Text` packet that survives bound-key suppression, an explicit paste, non-hover mouse or wheel input, or enabled writable `ClientFocus` retires the active message before downstream dispatch. Bare hover deliberately remains a zz presentation-only no-op, and every read-only input leaves the message armed. Suppressed trailing text from a binding cannot erase the message that binding raised. `display-message -N` now matches the pin's sticky client flag: a positive-effective-duration ordinary Interactive message writes it, with `-N` setting the bit and a plain message clearing it. A positive-duration Interactive `PrintOrMessage` producer such as `list-keys -1` also clears it. Explicit or inherited zero duration, clear, expiry, printing, Control clients, and a missing destination leave it unchanged. While the bit and an active message coexist, writable terminal Key, standalone or paired `Text`, Paste, non-hover mouse and wheel input, and `ClientFocus` stop before message dismissal, display-panes teardown, prompt handling, dispatch, or activity accounting. An ignored release retires a swallowed press decision without forwarding. The committed-text queue matches the first entry with the same pane and input lane, uses the committed character from the key, retires the skipped queue prefix and its linked debt, then retires the matched debt while preserving the later suffix. Browser-before-terminal ordering therefore preserves the later terminal debt, while terminal-before-browser ordering retires the skipped terminal debt as stale. Read-only input and native browser-surface input keep their prior paths. Alert-produced visual messages now use the same `ActiveClientMessage` record. Each eligible Interactive recipient gets its own identity. A positive inherited duration clears sticky ignore-keys and arms a cancellable deadline; zero duration waits for writable input; replacement, dismissal, and expiry use the ordinary identity-specific clear. The freeze suppresses ordinary updates, full-frame repair, resync, and popup viewport publication. Control clients never arm message state. Pinned `status_message_clear` leaves an old positive timer live, which can clear a newer zero-duration message; zz cancels and identity-checks stale deadlines instead of reproducing that bug. Since 2026-08-20 the omitted `display-panes -d` duration comes from `display-panes-time` like the pin. | ordinary and alert message lifecycles behave; bare hover is an intentional native adaptation; stale timer cancellation is a deliberate correctness divergence |
| `respawn-pane` / `respawn-window` | Dead panes revive with stable pane identity; `respawn-window` keeps its first pane and removes the rest. `-E`, `-k`, `-c`, repeated `-e NAME=VALUE`, and stored command/cwd reuse are implemented. | none known on the cataloged surface |
| Array options | Since the 2026-08-20 Lane-2 sweep all eight real array options (`command-alias`, `codepoint-widths`, `user-keys`, `terminal-overrides`, `terminal-features`, `status-format`, `pane-colours`, `update-environment`) store with the pin's separators, hole reuse, and `name[N]`/`-u name[N]` semantics, and the 68 hook names route to the hook table. Since the B1 server slice (2026-08-21) `status-format[]` drives the daemon's personalized `StatusLine.rows` production (sparse indices publish blank rows, a session array overrides the global one whole, scoped writes refresh that session's attached clients). Wave C added two more consumers (2026-08-21): `command-alias[]` expands one layer before canonical lookup at both construction entry points (`MuxEngine::resolve_command_alias`), and, like the pin's parse-time expansion, `bind-key`/`set-hook`/`default-client-command` store the expansion, so `list-keys` and `show-hooks` print `list-windows` for an aliased `lsw` on both servers (differentially pinned). Stored bind-key and set-hook lists execute their constructed commands without another user-alias lookup; read-only clients authorize the same frozen chain before any effect. Typed `if-shell`, `run-shell`, and `confirm-before` callbacks remain frozen after lexical construction. Set-hook and command-valued native set-option retain their intentional second construction stage, while display-menu selection begins a fresh stage. A failure uses the ordinary command-output and `key_command_failed` warning path. Protocol v74 Control and the local ordinary CLI prepare each complete argv unit under one daemon lock and freeze that one alias layer. The CLI uses the returned canonical identity and alias-match bit for attach, stdin, and kill recovery routing and carries the vector unchanged across a TUI reconnect. Every prepared command is reauthorized during execution, so alias shadowing is not an authorization control. Remote `--host` preparation remains under `aliases.remote-client-preflight`. At the Wave C checkpoint only single-command bodies executed; the 2026-08-30 `aliases.command-bodies` closure supersedes that limitation. The mux now wraps valid multi-command bodies in one opaque prepared group, caller arguments and client-owned stdin attach only to its final child, and empty bodies succeed without effects. The group renderer preserves option boundaries, empty arguments, typed blocks, and physical source groups across stored commands, source replay, local routing, and Control. The daemon runs each child through its normal queue path and preserves failures, nested source yields, structural hooks, shell work, and deferred shutdown across the shared boundary. Control emits inherited-flags child guards without a synthetic parent frame; an empty alias emits no frame. For an unparsable matched body, the mux reports a loud unknown command and never falls through to the canonical or catalog alias it shadows. Alias lookup is exact on the typed name in both (a command prefix like `ls` never reaches the alias table). `update-environment[]` drives `seed_session_environment` plus its own readback. The remaining five still drive nothing. Indexed `@`/table scalars follow tmux (`not an array` on set; indexed show reads the scalar). | valid `command-alias[]` execution is closed; **silent**, store-only for `codepoint-widths`, `user-keys`, `terminal-overrides`, `terminal-features`, and `pane-colours` |
| Option-name format lookup | Closed 2026-08-29 in slice 10ae. Generic option lookup now precedes format-table, command-item, and environment values for the source-registered 105-name roster: 13 server, 42 session, 40 window, and 10 pane consumers. Exact names and legacy aliases follow selected-target scope, inheritance, attached-client fallback, active children, and `S`, `W`, and `P` loop retargeting; command prefixes do not match. Flags render as `0` or `1`, and other types retain their tmux spelling. `command-alias`, `status-format`, and `update-environment` support whole-array and indexed access with numeric-before-named order, leading-zero normalization, empty malformed or missing results, and whole-array local shadowing. Mux formats read live state. Direct daemon producers use the same live resolver, while detached status shares one all-scope snapshot across a refresh batch. Missing-target `run-shell -C` and `if-shell -F` read global options while their inserted work keeps the caller context. The 60-step `option-name-formats` differential has no differing channel, and the attached status probe passes. | none for the complete registered roster; no protocol, wire snapshot, or native GUI styling change |
| Renderer-style residue (C9) | Only the COLOUR halves behave: `window-style`/`window-active-style` patch each pane's default fg/bg (attributes, `dim`, and the styles' `#()` shell branches stay inert; the appearance seam expands conditionals with context-only hooks), and `pane-border-style`/`pane-active-border-style` publish one fg colour per pane for the raw TUI (`None` selects its normal fallback; non-colour border attributes and `bg` fills stay ledgered per the v71 contract). The GPUI client ignores those border fields and derives pane chrome from its local theme. `mode-style` colours the copy-mode selection (the pin's `copy-mode-selection-style` default chain) and the copy-mode match styles colour the GUI's search overlays through the published appearance. That appearance channel carries one global value, so `setw -t` per-window copy-mode/mode styles store but do not recolour. zz's copy-mode position indicator keeps its theme chrome (`copy-mode-position-style`/`-format` are store-only), the TUI flattens all overlays to reverse video, and `copy-mode-mark-style` resolves but paints nothing because zz renders no mark element. | **silent**, bounded |
| Border style owner z-order | Closed 2026-08-31 for per-span partitioning. The raw TUI now resolves every shared divider span from its adjacent panes. With `A | (B / C)` and C active, the A/B span is inactive while the junction and A/C span are active; fallback is `top`→`bottom`→`left`→`right`, and aligned same-side ties created only by splits use lower `PaneId` creation order. The 10-step `LC_ALL=C` raw-client scenario matches pinned tmux in every channel, while the exact base fails its renderer marker. The pin's final same-side overwrite order after `join-pane`, `swap-pane`, or serialized `select-layout` follows mutable tiled pane z-order. `MuxSnapshot` does not transport that order, so those cases remain open. Floating panes, GPUI chrome, and unrelated indicators are excluded. | **silent**, bounded, opt-in; mutable z-order open |
| `display-panes` custom selection template | The parser accepts and constructs the optional string-or-typed template with the pinned callback rule, but mux execution still rejects a positional value. Tmux substitutes the selected `%pane` for `%%%` and executes with the retained original queue state; an omitted template uses `select-pane -t "%%%"`. The gap is parked under `display-panes.command-template`, separate from queue blocking and presentation. | **loud**, bounded |
| `display-panes` label presentation | The pin paints big numerals plus the expanded `display-panes-format` across the pane's top row in the `display-panes-colour` cell. zz expands the same format per pane into `PaneIndicator.label` (1 KiB cap) and paints it through the shared styled-segment path: the TUI composes it across the pane header row right of the selection-key badge (alignment and exact-width clipping via `compose_status_row`), the GUI as an alignment-bucketed top strip inside the indicator overlay clipped at the pane edge. The label's base colours stay theme-derived — `display-panes-colour`/`display-panes-active-colour` remain store-only — and zz keeps its native badge/card instead of the pin's numerals. | **silent**, bounded |
| `display-panes` queue blocking | With no `-b`, tmux returns `CMD_RETURN_WAIT` and resumes that client's command queue when the overlay closes. zz accepts `-b` but always returns immediately, so a command sequence continues while the overlay is still open. `-N`, client targeting, duration, and key fallthrough do not change this retained difference. | **silent**, bounded |
| Status-block suppression threshold | tmux hides the status line when `tty.sy <= statuslines` (resize.c `CLIENT_STATUSOFF`), so a 3-row terminal with `status 2` still shows both status rows plus one window row. zz panes carry a header row, so the TUI suppresses the block when `rows < statuslines + 2` (one header plus one content row must survive) — in that same 3-row terminal zz shows no status block and gives all rows to the pane. The GUI mirrors the rule against its measured canvas in line-height units. | **silent**, bounded |
| `history-limit` default | zz keeps 10,000 lines for its product default; the pin keeps 2,000. `show-options -g history-limit` prints the effective 10,000 value. | **silent**, deliberate |
| Plain option listings | No-argument listings contain tmux table names and `@` user names. The six zz-native settings stay available through explicit-name queries and never appear as unknown words in tmux-parsing scripts. | **silent**, zz extension hidden from tmux listings |
| Nested `new-session` validation precedence | Closed 2026-08-28 against pinned `cmd-new-session.c`. Nested non-detached creation now completes generic option and arity parsing; rejects a `-t` target combined with a command or `-n`; expands and validates `-n`, then `-s`; tries existing-session `-A`; checks for a duplicate; validates an unresolved `-t` as a session-group name; and expands `-c` before the nesting refusal. The refusal still precedes terminal and size validation, and every early path leaves mux state unchanged. A narrow routing parser accepts catalogued `-t` only while selecting the terminal client and running this preflight. Normal mux execution still reports `new-session -t` as unsupported because session groups remain excluded. Canonical, alias, prefix, user-alias, and later command-list forms share the path. Detached creation, Control fresh creation and `-A` misses, and already-attached fresh creation remain allowed; existing `-A` and `-Ad` hits still refuse, while `-Ad` misses stay detached. The real attached-client differential matches exact status, stderr, session roster, client count, and command-list stop behavior on zz and the pin. | closed for nested refusal; session groups remain unsupported |
| `new-session` cwd edges | Closed 2026-08-29 in slice 10x. Existing `new-session -A -c` targets now share the attach path's retarget and cwd update. The engine expands `-c` once in the resolved target session, window, pane, and invoking-client context, then stores it before a nonnested terminal-open preflight. A headless open failure retains the target mutation. Clientless calls remain inert, permitted Control clients attach and update the target, and nested Interactive, Control, and `-A -d` calls refuse before window or pane selection, expansion, retargeting, or mutation. Fresh creation and an `-A` miss retain an empty session cwd when `-c` expands to empty, while the initial pane keeps the donor or caller fallback; omitted `-c` keeps normal inheritance. The ten-step `new-session-cwd` differential has zero topology, geometry, format, output, or warning differences. Focused mux and daemon tests cover one-pass expansion, target isolation, client classes, headless failure ordering, nested refusal, and empty state. | none; closed under `sessions.new-session-attach-cwd` |
| Session environment updates | Closed 2026-08-27 for representable client entries. Both servers seed their global environment at boot. Protocol v82 adds the per-connection snapshot needed to apply the effective session `update-environment` array from the invoking or selected client. Exact names and `fnmatch` patterns copy present values; selected missing names create unset markers; empty values remain set; selected hidden entries become ordinary. A wildcard with no match creates its literal unset marker and leaves old expanded names in place. Fresh `new-session` seeds before repeated `-e NAME=VALUE` overlays, while `-E` skips only that seed. Existing `new-session -A` follows attach behavior, honors `-E`, and ignores `-e`. Attach refresh runs after target and terminal preflight. Direct native attach and Control use the same path. Targeted `switch-client -c` uses the selected client's snapshot, `-E` preserves the destination, and `-T` returns before refresh. Updated session values survive disconnect and reach later panes; already running children remain unchanged. Like the pin, an empty-name `=VALUE` entry remains visible in `show-environment` but is discarded at terminal spawn, and pane-local `new-window`/`split-window` overlays discard it at the same boundary. | closed for UTF-8 client environments; non-UTF-8 bytes remain under `clients.path-encoding` |
| Lifecycle trio | `exit-empty`, `exit-unattached`, and `destroy-unattached` are inert until a config EXPLICITLY sets them (presence in the stored-scalar map, not the effective value): unset, zz keeps its persistent-daemon rule, `armed ∧ zero sessions ∧ zero subscribers`. Explicitly set, the pin's `server_loop` (`server.c:281-292`, whose client loop at `:289-292` is the check the subscriber clause below contrasts against) and `server_check_unattached` (`server-fn.c:481`) policies take over — enforced on client departure and command execution, where the pin re-evaluates every loop iteration — with one permanent divergence: the `zero subscribers` conjunct is LOAD-BEARING and survives every policy, because a zz GUI/TUI client can outlive its session where a tmux client cannot, so an attached client must never have the daemon die under it. "Attached" means present in `ServerState::attached` (a client bound to a session); "subscriber" means an Interactive or Control client holding an outbound mailbox. `exit-unattached on` therefore exits when no client is bound to a session AND no client is subscribed, where the pin needs only the former. Policies are also dormant inside the startup bracket so a boot config cannot kill the daemon it is configuring. `destroy-unattached=keep-last`/`keep-group` are decided by linked session groups in the pin (`session_group_contains`); zz has no session groups, so `keep-last` never destroys (every session is effectively the last of its group) and `keep-group` always destroys — both are exact for the ungrouped case, which is every zz session. Session groups stay the permanent compatibility skip. | **silent**, bounded, opt-in |
| Forced-shutdown multi-window hook order | Pinned tmux emits `session-closed`, then repeatedly removes the root of each session's winlink RB tree to produce `window-unlinked`; insertion, deletion, and rebalance history can therefore change the hook order for an identical final window map. zz retains only the index-ordered `Session.windows` map and snapshots that order during forced shutdown. For the final mapping `@0:0,@2:3,@1:9`, the pin emits windows `@2,@1,@0`, while zz emits `@0,@2,@1`. The difference is externally visible because the draining shutdown queue may run only the first admitted hook. The alias-body closure covers the surrounding queue, source-yield, structural-hook, and shutdown behavior but cannot reconstruct discarded tree history. | open under `hooks.shutdown-window-unlinked-order`; requires retained tmux-compatible winlink tree history rather than sorting the final map |
| Client-exit notices | Closed for zz-tui in protocol v70: requested/evicted detaches print `[detached (from session X)]` rc 0, a destroyed session with no survivor prints `[exited]` rc 0, shutdown prints `[server exited]` rc 1, and a lost connection prints `[server exited unexpectedly]` rc 1, all after terminal restoration. Native GUI and control-mode surfaces keep their existing presentation. | closed |
| In-UI error text width | Command errors surfaced inside the TUI render in the sidebar's 28-column status row, so a long tmux message truncates (`can't find window: 99` shows as `can't find win`). The message text itself is now the pin's, via one shared renderer. Collapsed-sidebar mode uses the full width. | **silent**, cosmetic |
| Command and status shell-job cwd | Closed 2026-08-30 under `jobs.shell-job-cwd`. `run-shell -c` wins first. Shell-form `run-shell` and `if-shell` then select the startup client cwd, an unattached provenance client's cwd, the target session cwd, or the invoking client's attached-session cwd before falling back to `HOME` and `/`. Positive-delay jobs retain that selection before the timer and check path existence when the child starts. Status `#()` uses the attached session cwd instead of `pane_current_path`. Attached clients keep independent command caches, while unattached query clients share entries by effective cwd. Ten focused daemon shell-job tests and 32 status tests pass. The three-step differential completes eight checks per engine with no differing channel. The attached fixture keeps pane, session, and target paths distinct and covers 24 Interactive and Control cases across `run-shell`, `if-shell`, and valid, missing, and omitted targets. The full 105-scenario, 1,675-step aggregate passes with attached-client `PASS` and only the two approved GEO rows. | closed; no protocol or snapshot field changed |
| Command and status shell job environment | Closed 2026-08-29 in slice 10ac. Shell-form `run-shell` and `if-shell` start from an empty process environment, then apply global and resolved-session values in order. Status `#()` applies global values only. Hidden and unset values disappear; explicit missing targets become sessionless; visible modeled `TMUX_PANE` survives without synthesis. During startup, command jobs preserve modeled TERM-family values. After startup, every path forces `TERM` from `default-terminal`, `TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION=3.8-zz`, and `COLORTERM=truecolor`. `TMUX` carries the resolved session id or `-1`; status uses `-1`. The private tmux launcher uses modeled PATH and replaces stale private startup values. The three-step scenario runs eight assertions per engine, and the attached fixture proves the global-only status path. Slice 10af closes positive-delay sampling, and the 2026-08-30 cwd closure covers command and status working directories. Immediate background ordering, `copy-pipe`, and popup jobs have separate owners. | closed under `jobs.command-status-environment`; producer-specific residues remain split across their named groups |
| Buffer file client path context | Closed 2026-09-01 under `buffers.client-file-context`. The daemon expands the path once, in its own format context: a leading `~/` against the server's own home, anything else relative against `server_client_get_cwd(c, NULL)` — the config client during startup, an unattached client's own working directory, then that client's attached session cwd, then home. An unattached command client then does the bounded read, or the create-truncate or `-a` append, itself over the v89 `ClientFileRequest`/`ClientFileResponse` pair, correlated by request id and released when the connection ends, the way `file_read` and `file_write` hand `MSG_READ_OPEN` and `MSG_WRITE_OPEN` to a client instead of touching the server's filesystem, so a client that does not share a filesystem with the daemon still moves its own bytes. Failures read `strerror: path` the way cmd-load-buffer.c and cmd-save-buffer.c print them, and a client-side read that fails after the open reports `Input/output error`, which is what the pin's bufferevent path reports. The client now reports its working directory the way `find_cwd` does, preferring `PWD` when it resolves to the same place as `getcwd`. Attached Interactive and Control clients keep daemon-side IO. | closed for both buffer commands |
| `display-popup` job environment | Closed 2026-08-31 under `jobs.environment`. A popup PTY starts from the global overlay and the target pane session's overlay with hidden and child-unset entries dropped, then the multiplexer identity — `TERM` from `default-terminal`, `TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION=3.8-zz`, `COLORTERM=truecolor` — then `TMUX` carrying that session's id, and only then the `-e` assignments, so a repeated `-e` wins by its last occurrence and an `-e` can override `TMUX`, `TERM` or a hidden name. An `-e` with no `=` or an empty name is dropped in silence, matching `environ_put`. The working directory owns `PWD` last, so `-e PWD` loses to a `-d` or target-client cwd. `TMUX_PANE` is no longer synthesized: the pin's popup is a job, not a pane, so only a modeled `TMUX_PANE` reaches the child. Pane spawns keep zz's own `TERM_PROGRAM=zz` identity; the terminal worker stamps it before the spawn environment now, and the session scope drops the TERM family so an overlay cannot leak into it. The two-popup differential runs four checks over twenty assertions per engine. | closed; no protocol or snapshot field changed |
| Positive-delay `run-shell` environment timing | Closed 2026-08-29 in slice 10af under `jobs.run-shell-positive-delay-environment`. Shell-form `run-shell` with explicit numeric `-d > 0` retains scheduling-time command text, resolved target identity and numeric session id, expanded text and numeric arguments, and cwd string. Child launch reads current global state. A live original session contributes its current overlay; a destroyed original session contributes its retained overlay after same-name recreation. A missing scheduling target remains global-only with `TMUX` id `-1` if a matching session appears before launch. Child launch reads `default-terminal` and the startup TERM gate, then applies cwd existence fallback to the retained cwd string. Foreground daemon coverage waits for `active_shell_jobs` before mutating state. The background three-step differential completes twelve checks per engine across live, destroyed and recreated, missing and later-created, and startup-crossing cases with no differing channel. | closed under `semantic:run-shell-positive-delay-environment-timing`; `run-shell -C`, format-condition `if-shell -F`, absent `-d`, `-d 0`, immediate background ordering, `copy-pipe`, and popup jobs retain separate owners; `jobs.shell-job-cwd` closes cwd selection |
| `#{version}` | zz reports `3.8-zz`, sharing the compatibility-version source used by `zz -V` (`tmux 3.8-zz`); the pin reports `next-3.8`. The suffix is deliberate so scripts can identify the compatible implementation without confusing it with upstream tmux. | **silent**, deliberate |
| Non-UTF-8 command arguments | tmux prints a byte such as `a\377b` with octal vis escaping. zz converts argv with `to_string_lossy` before escaping and prints `a<U+FFFD>b`. | **silent**, accepted edge |
| Config `~` expansion | Closed for UTF-8 daemon parser contexts on 2026-08-29. Leading `~` at an unquoted word boundary, inside an opening double quote, or immediately after an empty or nonempty closing quote expands at parse time. Tildes inside single quotes, escaped tildes, ordinary mid-word tildes, prefixed continuations, and tildes after empty variable expansion remain literal. Invisible continuations, raw quoted newlines, stripped quoted comments, and typed command blocks follow the pinned state transitions. Bare tilde prefers a nonempty parser-context `HOME`, then the current passwd home; named users use passwd lookup; failure is a located syntax error; usernames stop at 1,022 bytes. The 17-step differential runs 26 internal checks with no differing channel. Direct Control still parses with its local environment, and a non-UTF-8 passwd home is still treated as lookup failure. | closed under `config.parser-edge-cases` for representable daemon parser paths; tracked under `control-mode.local-parser-environment` and `config.tilde-home-path-encoding` for the two retained contexts |
| Command-name abbreviation | Closed 2026-08-24. Exact canonical names and aliases resolve first. Non-exact lookup searches the pinned tmux canonical namespace before the guarded 19-name native roster, so every pinned prefix keeps its tmux result while native names stay exact and noncolliding native abbreviations such as `capture-b` remain available. The manifest gate derives the roster from catalog minus oracle and checks every prefix of all 92 pinned names. A strict 29-step differential scenario covers the 25 prefixes that native names had changed, exact tmux aliases, a user `command-alias` named `split`, and ambiguous `list-commands` exit parity. The daemon resolves one alias layer for a direct invocation before read-only authorization and reuses that invocation through dispatch and hooks. Stored bind-key and set-hook commands execute their constructed lists without another user-alias lookup, and read-only clients authorize that frozen chain before any effect. Typed `if-shell`, `run-shell`, and `confirm-before` callbacks remain frozen after lexical construction. Set-hook and command-valued native set-option keep their documented second construction stage; display-menu selection begins a fresh stage. Matched unsupported bodies refuse without falling through to canonical or catalog-alias lookup. Protocol v74 closes Control's former static unknown-name precheck: the client asks the daemon to prepare the whole initial argv unit or LF line under one lock before allocating frames, then executes the returned invocations without a second alias lookup and with normal authorization. Local ordinary CLI commands now use the same prepared canonical identity and alias-match state for attach, new-session, stdin, and kill recovery routing, including immutable TUI handoff. Remote `--host` preprocessing remains static under `aliases.remote-client-preflight`. Prefixes resolving to catalogued-but-unimplemented commands still answer `unsupported command: <canonical>`. | closed |
| `set prefix` key validation | Closed 2026-08-29, with its DEL residual closed 2026-08-30. The tmux command parser covers the pin's short modifiers, caret and named forms, exact `F1` through `F12` range, and prefix-consuming 32-bit `User` and hex numbers. Literal controls 1 through 31 and printable ASCII hex keys retain pinned behavior across binding, filtering, options, and send-prefix. Raw DEL, caret plus DEL, and textual `0x7f` retain separate identities and pinned rendering across bindings and key options; raw and textual-hex forms send a literal DEL while the caret-modified form sends nothing. | closed under `keys.strict-validation` and `keys.literal-delete-identity` |
| `resize-pane` direction amount metadata | Closed 2026-08-25 as a catalog-only reconciliation. Runtime already accepted bare `-D`/`-L`/`-R`/`-U` with amount 1 plus attached and separated integer amounts. The four catalog entries now mark their values optional, and the manifest compares that shape with the pin. No handler, effect, or wire contract changed. The strict 16-step `resize-directions` differential is clean with no skips. `-M` and `-T` remain open under their existing owners. | closed metadata gap; runtime unchanged |
| Error-shape residue (post-7b) | Grep-facing error classes are pin-bare and byte-exact since wave 7b (2026-08-18): the twelve `options-values.sh` regress strings, `can't find session/window/pane:`, `unknown command:`, `already set:`, `open terminal failed: not a terminal`, show-messages pairs, `%config-error <file>:<line>:`. Catalogued-but-unimplemented commands/options answer `unsupported command: <name>`, a zz-only condition the pin would instead run. Positional minimum and maximum diagnostics closed on 2026-08-27 across all 72 implemented finite commands, their built-in aliases, and stored `bind-key` and `set-hook` children. Shared flag diagnostics and usage fallbacks closed on 2026-08-28 across all 83 implemented upstream commands and 74 aliases, replacing the earlier 24-command daemon-only rejection roster. The chooser close then made positional bounds outrank recognized parked capabilities on direct, daemon-preflight, and stored-command paths. Nested non-detached `new-session` precedence also closed on 2026-08-28, leaving no active `mux.error-shapes` item. Command-specific semantic value diagnostics retain their existing owners. | arity, shared flag text, parked-capability precedence, and nested `new-session` precedence closed; semantic value families remain |
| Alerts | Full alerts.c behavior closed on 2026-08-26. `monitor-bell` gates the bell path, `monitor-activity` raises the activity flag from PTY output, and `monitor-silence` arms per-window daemon deadlines that re-arm on output and expiry. Every successful silence-option write, including a same-value write or repeated global reset, resets every live window timer; a missing local `-u` and a rejected `-o` do not. Window selection clears alerts and requeues activity like `session_set_current`. Attach clears the mux-visible bell, activity, and silence flags only on the session's active window before snapshotting. The alert action and label are evaluated once against that active window, then the same ring or message decision fans to every eligible Interactive client. Flags surface through `#{window_flags}` (`#`/`!`/`~` in pin order), the window flag formats, `session_alert`/`session_alerts`, and `session_activity_flag`/`session_silence_flag`, whose misleading names mirror the resolved target window. Visual alerts use the daemon-owned message lifecycle. Every eligible attached Interactive client owns its message identity, replacement record, timer, terminal-publication freeze, and full-viewport thaw. Full-frame repair, resync, and popup viewport publication obey the same gate. Its exact bounded log entry is `<client> message: <text>`, using the registered name or `device-<id>` fallback. Control clients can receive pane BEL bytes through `%output` but receive no alert message or alert log entry. `TerminalSession` publishes one reliable `TerminalEvent::Bell` per BEL callback, matching pinned `alerts.c:196-215`; the mux owns the visible alert flag, so a repeated Bell can notify while `#{window_bell_flag}` remains 1 until selection or attach clears it. The attached fixture replaces a 1,500 ms sticky message with a 5,000 ms alert, proves 1.8 seconds of freeze, repeats the Bell on the same unvisited pane, waits 5.2 seconds for the pin's stale timer, and proves zero-duration dismissal. zz deliberately cancels and identity-checks old timers instead of letting one clear a newer zero-duration alert. The narrow TUI status surface may show only the stable `Bell in window` prefix because its detach hint truncates the trailing index. | closed; stale-timer cancellation remains deliberately safer than the pin |
| `select-layout main-*` with 2 panes | The pin never sizes the lone "other" pane (layout-set.c:264-269, :458-463), leaving stale geometry that fails tmux's own `layout_check`; zz sizes it (80x24 → main 80x22 + other 80x1). Deliberate: zz refuses to reproduce an upstream bug. | **silent**, zz more correct |
| `select-layout -E` on a mixed parent | The pin spreads only leaf children (layout.c `layout_cell_is_tiled`) but divides the parent's full extent among them, so a parent mixing leaves with nested nodes gets corrupt sums (observed: 40+42+39 in an 80-wide window, last pane at xoff 84). Every later operation on that corrupted window keeps diverging: one `-E` produced four geometry divergences, three downstream, so the known scenario has one causal step but the divergence is not bounded to it. zz refuses that spread and stops the walk where the pin stops. All-leaf parents are exact (48 pin fixtures + `known/known-spread-mixed.txt`). | **silent**, zz more correct |
| `select-layout` strings with zero-sized leaves | The pin accepts a leaf with width or height zero. zz rejects it to preserve the `PANE_MINIMUM` invariant. | **loud**, zz more correct |
| `select-layout` strings with extents above `u16::MAX` | The pin accepts `70000x70000` through its `u_int` geometry. zz rejects it; `PANE_MAXIMUM` is 10000. | **loud** |
| `select-layout` strings with single-child nodes | The pin accepts a node with one child. zz requires every node to have at least two children. | **loud**, zz more correct |
| `select-layout` string validation order and depth | zz validates the whole string before trimming cells to the current pane count. The pin trims first and runs `layout_check` afterward, so a sum violation confined to a deleted cell fails only in zz. zz caps parsing at 256 levels and rejects a 300-deep string in bounded time; the live pin held 100% CPU for minutes on input around 100 levels deep. This validation-order edge is one-directional: zz never accepts a string that the pin rejects. | **loud** |
| Lone-pane `select-layout` strings | On an 80x24 one-pane window, the pin accepts `a8fd,120x30,0,0,0`, keeps `window_width=80`, and dumps `120x30` from its new root. zz adopts the encoded extent for both the window and layout. | **silent**, zz more correct |
| Detached `split-window -Z` hidden-pane width | Both servers zoom the post-spawn active pane, including the existing pane under `-d`, and a successful split without `-Z` clears zoom. Immediately after `split-window -dZ`, the pin also reports the newly created hidden pane at the full window width until the window unzooms (`layout_assign_pane` receives `SPAWN_ZOOM` before `window_pop_zoom`). zz reports that hidden pane's saved layout allocation while reporting only the active zoomed pane at full size. Active-pane and zoom flags match. | **silent**, bounded |
| `move-pane` on tiled panes | The pin reserves `move-pane` for floating targets and returns `pane is not floating` for a tiled target (cmd-join-pane.c:428-431). zz has no floating panes and keeps `move-pane` as an alias of `join-pane`. The accepted `-l` grammar, format expansion, and `-f` sizing basis match the shared pinned tiled-layout path, but zz still performs the move where the pin refuses it. | loud |
| Attached-GUI `#{pane_width}` | Formats report the engine's cell allocation while PTYs are still sized by client pixel measurement, so a drawn pane's format can drift a cell from `tput cols` until the client-reported window size lands. Headless is exact. | **silent**, bounded |
| `#{window_flags}` | zz emits `#` activity, `!` bell, `~` silence, `*` current, `-` last, and `Z` zoomed in tmux order (Wave C run 2 backed activity and silence). `M` marked remains absent because zz does not model the marked pane. | **silent**, `M` only |
| `#{command}` in command items | Closed 2026-08-24. Both sides treat `#{command}` as a command-queue-item fact rather than a list-row fact. tmux adds it in `cmdq_merge_formats` from the running item's `cmd_entry` name and then merges the item state over it; zz carries the canonical entry after alias and prefix resolution through the mux dispatch hooks and daemon-owned expansion, consulting explicit item state first. The daemon path covers run-shell shell and string `-C`, if-shell conditions, capture and pipe arguments, client and buffer list formats and filters, popup and menu presentation values, confirm string preparation, and the post-spawn `new-window`/`split-window -P -F` pass that adds live pane facts. Typed blocks skip the parent expansion, hook bodies report their own command beside the trigger `#{hook}`, and confirm prompts plus delayed Control subscriptions remain outside an item with an empty value. Popup argv and environment assignments remain raw. | closed; pinned by `command-item-format.txt` and the 24-step `daemon-command-item-format.txt` |
| Command-argument format and name expansion | The 2026-08-24 close covers five target-sensitive paths: `rename-session` and `rename-window` positional names, direct `show-options` and `show-window-options` optional names including `show-hooks` forwarding, and directional `select-pane -T`. The 2026-08-25 follow-ups close both `new-session` names, `new-window -n`, and both buffer file paths. `new-session -n` expands, validates, and vis-cleans before `-s`; `new-window -n` expands once in its destination session context with session format type before the same validation and cleaning; both rename paths use their resolved active pane and pane format type before applying that helper after one expansion. `break-pane -n` follows the pin's different rule: it stays literal, then validates and cleans before placement. Rust hooks and pinned source establish ASCII-control rejection and exact ordering, while the strict differential covers valid Unicode, empty expansion, backslash cleaning, cleaned collision or reuse, literal break-pane tokens, and rename parity. An unindexed `new-window -S` repeats format expansion over the cleaned first-pass value for lookup while preserving that first-pass value for creation, and an explicit `break-pane -n` disables window-local `automatic-rename` on both placement paths. `load-buffer` and `save-buffer` expand their path once before `~/` handling and I/O, with canonical command identity, explicit item-state precedence, and no second pass over replacement text. A valid `load-buffer -t` supplies its attached session, focused window, and active pane; a miss is quiet and falls back to the most-recent mux context. Targetless buffer paths select the invoking or best attached client before the same context fallback. Raw or produced `-` streams remain unsupported, and relative, attached-session, and remote file ownership remains under `buffers.client-file-context`. | ten expanded paths, literal break-pane naming, and both creation-name edges closed; client file context active |
| Format modifier and context vocabulary | Slice 10v delivered schema 5 source registration for 31 literal producer scopes, 153 scoped pairs, and 108 unique names, plus 10 derived families, five propagation records, and 36 modifier tokens. At that checkpoint, the literal partition held 58 implemented pairs, 54 native pairs, and 41 active gaps; the derived partition held eight implemented families and two active gaps; and the modifier partition held 30 implemented tokens and six active gaps. The 10v artifact covered 98 scenarios and 1,522 steps with SHA-256 `810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`. Slice 10w closes `formats.repeat-modifier` locally. `R` splits at the first top-level comma before recursive operand expansion, repeats counts from 1 through 10,000, accepts leading count whitespace, and rejects trailing whitespace. Invalid, zero, negative, and oversized counts produce an empty replacement; a missing comma stops later format output. Escaped commas, nesting, byte-length replacement, truncation, and post-transform order match the pin. A deterministic 40,960,000-byte intermediate guard replaces the pin's time budget, and the daemon regression proves the default `P:` and `S:` rows emit their computed indentation without literal `R` syntax. The modifier partition now holds 31 implemented tokens and five active gaps: `I`, `L`, `O`, `V`, and `w`. The old forecast understated `w`: pinned `format_width` handles leading hashes, `#[...]` style spans, malformed markup, controls, live `codepoint-widths[]`, a 162-entry default cache, and the host `wcwidth` policy selected by the harness build's `--disable-utf8proc`; zz uses `unicode-width` 0.2.2. The `w` item remains later and hard until tests pin those platform and Unicode cases. The live registry after slice 10z has 87 active groups, 593 items, and 109 closed records: 45 open, 20 blocked, and 22 accepted. Closed records plus accepted active groups resolve 131 of 196 known groups (66.8%). The accepted artifact covers 101 scenarios and 1,540 steps with attached-client `PASS`, retains the two registered GEO rows, and has SHA-256 `afd1fdf9a79e06f449e8c43abd63b14a2a4968338110223750d4171889c34aaf`. Slices 10w through 10z remain local and uncommitted, with no commit or push. The post-10z rerank freezes `formats.session-runtime/format:session_active` as slice 10aa. | **silent** for `w`, `O`, and `V`; `I` and `L` remain source-classified without a live-runtime claim; `R` closed in slice 10w |
| `session_active` format-client context | Closed 2026-08-29 in slice 10aa. `FormatClient` records no client, an unattached client, or the attached session. Expansion returns empty without a target session or format client, `1` when that client is attached to the target session, and `0` when it is unattached or attached elsewhere. Command execution keeps the raw invoking client separate from the current or explicitly selected target client because one command can use both. Clientless lists, filters, chooser rows, and `list-commands` remain empty. Target-aware command formats, deferred pane output, shell callbacks, buffer paths, capture boundaries, popup and menu text, `list-keys`, status rows, Control subscriptions, and display-panes labels receive their selected client state. Fresh `new-session -c` retains independent stored-session and initial-pane cwd expansions, and non-detached `new-session -P` expands after attachment. Unit, source-file, `run-shell`, `if-shell`, per-client snapshot, and attached-client fixture proofs show that `client_*` facts and `session_active` use the same selected client. The 28-step `formats-values` row passes inside the accepted 101-scenario, 1,550-step artifact with attached-client `PASS` and SHA-256 `bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`. The change adds no protocol or snapshot field. | closed under `formats.session-runtime`; accepted in the full 10aa checkpoint |
| `window_activity` timestamp | Closed 2026-08-29 in slice 10ab. Each window stores an optional Unix-second `activity_time` beside zz's logical MRU counter. Creation, parsed nonempty pane output, and pinned current-window transitions refresh both values. Same-window selection, pane selection, pane creation, splits, and layout-only changes without output leave the timestamp unchanged. The independent audit repaired the direct daemon `switch-client` path so it refreshes the engine clock before selection. The direct Time backing expands empty without a window and preserves the same seconds through plain, boolean, comparison, list-row, and time-modified forms. Move-window and swap-window keep their pinned transition details. The 45-step `formats-values` row passes inside the accepted 101-scenario, 1,567-step artifact with attached-client `PASS` and SHA-256 `309aed0df108abd93e50f2073af7df5991d266c25e55dd266f0c8fc7f412ad72`. The change adds no protocol or snapshot field. | closed under `formats.window-activity-time`; accepted in the full 10ab checkpoint |
| `send-keys -N` (no keys) | Arms the **invoking client's** count prefix; tmux stores it on the pane mode, so another client's (or a Command client's) `-N` is a silent no-op in zz. | **silent** edge |
| `send-keys -X` | The action-local `-C`/`-P`/`-o` grammar, `--`, parser-failure behavior, and repeat-prefix reset match the pin. zz still has no pane-owned mode entry, so it cannot return the pin's `not in a mode` error when no client copy view exists. | **silent** |
| `send-keys -H` | Bytes `80`–`ff` refused; tmux writes the raw byte (`KeyToken::Literal` carries UTF-8). | loud |
| Unguarded commands | Closed by the [drop-in plan](/designs/tmux-drop-in.md)'s phase 0 and the 2026-08-28 shared flag parser. Every implemented upstream command rejects options from its catalog `CommandSpec`: absent ASCII alphanumeric flags are unknown, punctuation is invalid, known unsupported capabilities stay unsupported, and the full syntax scan precedes capability rejection. Mux execution covers all 83 canonical commands and 74 built-in aliases. Daemon preflight covers its 24 dispatch-owned upstream commands plus `display-panes`; zz-native commands retain their local parsers. | closed shared grammar; unsupported capabilities remain loud by design |
| `bind-key` payloads | Bind-time validation covers shared names and flags, including daemon-native long options. Positional arity and target errors can still surface at keypress. tmux validates the full argument template at bind time. | **silent** edge |
| Empty-daemon command-query startup | zz autostarts its persistent daemon and `list-sessions` succeeds with empty output, while tmux reports `no server running on ...` when its server is absent. Once either server exists without sessions, explicit targetless `attach`/`attach-session` return `no sessions`, exit 1, and the first `new-session` gets name `0` and ids `$0`/`@0`/`%0` on both. | **silent** lifecycle difference |
| Bare launcher with a non-empty server | tmux with empty argv defaults to `new-session`, so it creates and attaches another numbered session. The installed zz launcher maps empty argv to `new-session -A`, so it attaches the current session instead. Empty-server behavior matches because both create and attach session zero. | **silent**, deliberate product-friendly launcher behavior |

## Format variables that remain unbacked

This snapshot was refreshed on 2026-08-29. The parser registers the pinned 198-name table. Protocol
v83 and the daemon's shared client-fact
path close the 26 retained `client_*` facts plus `session_last_attached` across `list-clients`,
ordinary attached commands, foreground inserted commands, status and title recipients, and
`display-message`. Interactive and Control clients keep the pin's defined empty fields when their
terminal state cannot supply a value. Slice 10t removes `session_path` from this list. Its
differential proof creates two sessions, preserves lexical `/tmp/..`, reads each through a targeted
display, and reads both through one filtered session list. Focused mux tests separately cover
missing retained or target state and the value after the real `attach-session -c` command updates
one session. Slice 10ab removes `window_activity` after adding its direct mux timestamp backing. The
table below lists the remaining missing or deliberate format differences.

| Variable | Missing backing | Loud or silent? |
| --- | --- | --- |
| `buffer_mode_format` | No tmux buffer-mode row formatter; zz's buffer chooser is native. | **silent** |
| `client_mode_format` | No tmux client-mode row formatter; zz's client surfaces are native. | **silent** |
| `cursor_character` | The glyph under the terminal cursor is not mirrored into mux facts. | **silent** |
| `cursor_colour` | Cursor color is not mirrored into mux facts. | **silent** |
| `mouse_hyperlink` | Command formats do not receive tmux's mouse-event hyperlink record. | **silent** |
| `mouse_line` | Command formats do not receive tmux's mouse-event line text. | **silent** |
| `mouse_pane` | Command formats do not receive tmux's mouse-event pane id. | **silent** |
| `mouse_status_line` | Command formats do not receive tmux's mouse-event status-line index. | **silent** |
| `mouse_status_range` | Command formats do not receive tmux's mouse-event status range. | **silent** |
| `mouse_word` | Command formats do not receive tmux's mouse-event word text. | **silent** |
| `pane_bg` | The terminal cell background at the cursor is not mirrored into mux facts. | **silent** |
| `pane_fg` | The terminal cell foreground at the cursor is not mirrored into mux facts. | **silent** |
| `pane_key_mode` | Native copy/view mode is not projected as tmux's pane key mode. | **silent** |
| `pane_mode` | Native pane mode is not projected as tmux's mode name. | **silent** |
| `pane_pb_state` | Terminal progress-bar state is not mirrored into mux facts. | **silent** |
| `pane_search_string` | Native per-view search text is not mirrored into mux facts. | **silent** |
| `pane_tabs` | Terminal tab stops are not mirrored into mux facts. | **silent** |
| `session_group` | Session groups are unsupported, so no group name exists. | **silent** |
| `session_group_attached_list` | Session groups are unsupported, so no grouped attachment list exists. | **silent** |
| `session_group_list` | Session groups are unsupported, so no member list exists. | **silent** |
| `tree_mode_format` | No tmux tree-mode row formatter; zz's tree chooser is native. | **silent** |
| `window_offset_x` | Client viewport X offset is not fed into window formats. | **silent** |
| `window_offset_y` | Client viewport Y offset is not fed into window formats. | **silent** |

# Options coverage: 2026-08-22 snapshot

tmux's `options-table.c` holds 180 named options (plus 68 hook entries) at the pin. Since
the 2026-08-20 Lane-2 sweep **every one of the 180 stores** with the pin's exact default,
type validation, scope, inheritance, `-a`/`-u`/toggle semantics, and listing shape, so bare
`show-options -s`/`-g`/`-gw` byte-match the pin and a real `tmux.conf` imports with **zero
skipped lines**. That is test-enforced: `tmux_options.rs`
(`listing_order_covers_every_non_hook_table_option_once`,
`every_named_option_has_storage_metadata`) and `command.rs`
(`every_remaining_named_scalar_stores_and_bare_listings_cover_the_pin_table`) pin the 180,
the eight array options store with indexed semantics, and `set-option <hook-name>` writes
the hook table — the two paths that used to silently succeed while doing nothing are dead.

**105 behave**, meaning a value change is consumed somewhere outside set/show/inherit/
readback (consumer-traced 2026-08-20; nine status-production names joined in the B1 server
slice on 2026-08-21, `status-position` joined with B1's client half the same day,
`mouse`, `escape-time`, `set-titles`, and `set-titles-string` joined with the B2/B3/title
slice, and `command-alias`, `update-environment`, `exit-empty`, `exit-unattached`, and
`destroy-unattached` joined with the Wave C alias/environment/lifecycle slice, the
nine alert/prefix2 names joined with Wave C run 2, all 2026-08-21; `display-panes-format`
and the eight renderer styles joined with Wave C run 3 on 2026-08-22; and
`window-status-separator` joined through the daemon status-row renderer on 2026-08-24).
**75 are store-only.**
The earlier "78 behave" counted
options given a typed home in the honest-knobs/status structs, twelve of which nothing
read. Slice 10ad moves the unchanged roster into `command::TMUX_OPTION_CONSUMERS` beside
mux behavior and preserves `BEHAVES` as a public alias. The exact manifest guard proves
that the pin and live catalog share 180 unique names, the consumer roster contains 105
unique catalogued names, and the 75 live `option:` gaps form the disjoint remainder. It
also requires the closed `tracker.semantic-coverage` record. This registration changes no
runtime behavior. The compatibility gate passes 445 mux tests plus three daemon inventory tests;
full workspace tests and clippy, formatting, diff, tracker, and checked-summary checks pass.
`tmux_stored_scalar` and `tmux_stored_array` storage is store-only by construction until a
consumer wave wires a name up and moves it into `BEHAVES` (B1 moved six stored scalars and
the `status-format` array).

Slice 10ae closes `options.option-name-format-coverage`. The generic resolver covers all 105
registered consumer names across 13 server, 42 session, 40 window, and 10 pane scopes before
format-table, command-item, and environment values. Selected targets, inheritance, active children,
attached fallback, loops, arrays, missing targets, direct daemon producers, and detached status use
the same contract described in the table above. The focused 60-step differential and attached
status proof pass. Protocol, wire snapshots, and native GUI styling remain unchanged.

The completed Wave 2 has 84 active groups, 586 active items, and 122 closed records: 42 open, 20
blocked, and 22 accepted, resolving 144 of 206 groups (69.9%). The persisted accepted artifact
covers 105 scenarios and 1,675 steps. The attached-client fixture passes, exactly two approved GEO
rows retain their exact tuples, every other channel is clean, and the SHA-256 is
`a1e4ca86326006c5f06c77859219772b97fe7e6ac86dd703b127fced4ca0cd7e`. Slice 10ai, 10ah, the Config
front, shell-job cwd, and literal DEL identity are closed locally. The shell-job cwd aggregate and
the DEL strict-key differential pass.

**Behaving (105):**

- Indexing and sessions: `base-index`, `pane-base-index`, `renumber-windows`,
  `default-size`, `window-size`, `aggressive-resize`, `history-limit` (10,000 product
  default; the pin keeps 2,000 — the one fenced default divergence), `detach-on-destroy`.
- Keys and prompts: `prefix`, `mode-keys`, `key-table`, `prefix-timeout`, `repeat-time`,
  `initial-repeat-time`, `prompt-history-limit`, `history-file` (saved on submission, not
  at shutdown), `word-separators`, `wrap-search`.
- Spawn and terminal: `default-shell`, `default-command`, `default-terminal`,
  `remain-on-exit`, `focus-events`, `allow-passthrough` (`on` behaves as `all` — the
  worker lacks visibility state; payloads cap at 1 MiB then discard-until-ST; nested
  `\ePtmux;` is not recursively unwrapped), `allow-set-title`, `cursor-style`/
  `cursor-colour` (per-pane appearance clones; a zz-config `cursor-blink` override still
  outranks the blink half), `synchronize-panes`.
- Names and alerts: `automatic-rename`, `automatic-rename-format`, `bell-action`,
  `visual-bell`; since Wave C run 2 (2026-08-21) the full alert set: `monitor-bell`,
  `monitor-activity`, `monitor-silence` (per-window daemon deadlines),
  `activity-action`/`silence-action`, `visual-activity`/`visual-silence`, and
  `window-status-activity-style` (see the Alerts row).
- Overlays and buffers: `display-time`, `display-panes-time`, `message-limit`,
  `buffer-limit`, `set-clipboard`, `copy-command`, and the seven `menu-*`/`popup-*`
  style and border options.
- Layout: `main-pane-width`/`-height`, `other-pane-width`/`-height`,
  `tiled-layout-max-columns`.
- Status bar (GUI titlebar strip and tabs; see the Presentation row): `status`,
  `status-interval`, `status-left`/`-right`, `status-left-length`/`-right-length`,
  `status-left-style`/`-right-style`, `status-style`, `status-bg`, `status-fg`,
  `window-status-format`, `window-status-current-format`, `window-status-style`,
  `window-status-current-style`, `window-status-last-style`, `window-status-bell-style`.
- Status rows (B1, 2026-08-21 — the daemon expands these into the personalized v71
  `StatusLine` per client; the TUI paints the authoritative rows, while the GUI uses
  their list alignment as one input to native snapshot-backed window controls; see the
  Presentation row): `status-format[]` (sparse indices publish blank rows, session arrays
  override whole), `status-justify` (resolved inside the expanded row formats),
  `message-line` (published clamped to the row count; selects the row messages and the TUI
  prompt replace), `status-position` (the TUI shifts or shrinks its canvas for the block;
  GUI placement follows the app's chrome mode), `pane-status-style`,
  `pane-status-current-style`, `session-status-style`, `session-status-current-style`,
  `window-pane-status-format`, `window-pane-current-status-format` (all six resolve
  inside the default pane/session list rows), and `window-status-separator` (each nonfinal
  window item resolves its separator in item scope; exact row output remains TUI-only).
- Terminal surface and titles (B2/B3 + the C3 title source, 2026-08-21): `mouse` and
  `escape-time` (zz-tui consumes the session-effective values; the daemon rejects mouse
  input from terminal-surface clients when off — see the `mouse` / `escape-time` row),
  `set-titles` and `set-titles-string` (the daemon expands the title per client into
  `StatusLine.title`, publishing even with `status off`; zz-tui writes OSC 2 on non-empty
  changes and the GUI adopts the window title only when the option is explicitly on).
- Command aliasing, environment, and lifecycle (Wave C, 2026-08-21): `command-alias`
  (one layer before canonical lookup in standalone mux execution; daemon execution prepares
  one immutable layer before authorization and reuses it through dispatch and hooks; stored
  bindings prepare each command when it is about to run; bind-key/set-hook/option-command
  validation uses the same non-recursive rule as the pin's `CMD_PARSE_NOALIAS`),
  `update-environment` (drives `seed_session_environment` and its
  own readback), and the lifecycle trio `exit-empty`, `exit-unattached`,
  `destroy-unattached` — all three inert until EXPLICITLY set, and the
  "zero subscribers" guard survives every policy (see the Lifecycle trio row).
- Keys (Wave C run 2, 2026-08-21): `prefix2` — stored as a global-session scalar,
  synced into the shared `KeyTables` so either prefix arms the prefix table, published
  through `MuxOptionKey::Prefix2` and config-writable via `from_config_key`;
  `send-prefix -2` sends it and is a silent success while it is unset, like the pin.
- Renderer styles and the display-panes label (Wave C run 3, 2026-08-22):
  `display-panes-format` (the daemon expands it separately in each pane's context into
  `PaneIndicator.label`, format-expanded but not strftime-expanded like the pin's
  `format_single`; both clients parse the styled segments, honor `#[align=…]`, and clip),
  `window-style`/`window-active-style` (colour halves feed the per-pane appearance bridge:
  the daemon patches each pane's terminal fg/bg defaults with the pin's per-channel
  active-over-base fallback, re-resolving on option writes, selection, and relocation),
  `pane-border-style`/`pane-active-border-style` (explicit colours resolve pane, window, then
  global during personalized snapshot stamping into the v71 `PaneSnapshot` fields; the TUI
  styles divider spans and pane headers, while the GPUI client keeps its pane chrome under the
  zz theme), `mode-style` (copy-mode selection colours,
  matching the pin's `copy-mode-selection-style` default chain), and
  `copy-mode-match-style`/`copy-mode-current-match-style` (search overlay colours through
  the published appearance), plus `copy-mode-mark-style`. The status option-variable path
  consumes `copy-mode-mark-style`, which is enough for the 10ad source roster. No client
  renders a visual mark. These names resolve through the selected status-row and
  `display-message -p` injection paths; slice 10ae extends generic option-name format lookup to the
  complete registered roster.

**Store-only (75):**

- Typed storage that nothing reads (31): `lock-after-time`,
  `lock-command` (the lock commands are no-ops); `allow-rename`, `alternate-screen`,
  `scroll-on-clear`, `extended-keys`, `extended-keys-format`, `xterm-keys`, `backspace`,
  `editor`, `assume-paste-time`, `input-buffer-size`, `get-clipboard`,
  `default-client-command`, `fill-character`, `variation-selector-always-wide`;
  `message-style`, `message-command-style`, `message-format`;
  `pane-border-lines`, `pane-border-indicators`, the four `pane-scrollbars*`; the four
  `prompt-*cursor-*`; `clock-mode-colour`, `clock-mode-style`.
- Generic scalar storage (39 of the 63 scalar-backed names) plus five of the eight
  arrays: everything else in the table,
  including `remain-on-exit-format`, `status-keys`,
  `copy-mode-selection-style`, `copy-mode-position-style`,
  `display-panes-colour`/`display-panes-active-colour`,
  `terminal-overrides[]`, `terminal-features[]`, `user-keys[]`,
  `pane-colours[]`, `codepoint-widths[]`, the 21 theme-palette options, and the
  four `tree-mode-*` options. Lane assignments live in the drop-in plan's "options residue"
  section.

The index trio follows tmux's session/window inheritance, allocation, targeting, format,
and close-triggered renumbering behavior. `set-option` also accepts six zz-native names —
the agent/editor/history-trickle keys — which don't count toward tmux coverage and never
appear in the no-argument listings (those contain tmux table names and `@` names only).
`show-options` and `show-window-options` expose values with tmux's exact string escaping,
value-only and inherited forms. A no-name `show-options -H` keeps those rows and appends the
scope's stored hook arrays in option-table order; named hook queries do not require `-H`.
Free-form `@` names are pure string storage at server,
global-session, session, global-window, window, and pane scope, including append and unset;
this is the storage seam TPM and plugins use. Global and per-session environment overlays
have `set-environment`/`show-environment` readback and are merged into new terminal PTYs,
including hidden and child-unset entries; the daemon seeds the global map from its process
environment, and `new-session` copies the currently stored global `update-environment[]` names,
whose initial value is the pin default, or writes unset markers; creation-time `-e` overlays that
session map and `-E` skips the array seed. `automatic-rename` gates the desktop's active-pane label and explicit
window names install the pin's window-local `off`; its format string is evaluated by the
daemon-side label path only. `remain-on-exit` retains a frozen dead pane with live
`pane_dead` and normal-exit `pane_dead_status` facts, and the respawn commands revive that
stable pane slot. `default-terminal`, `display-time`, and `repeat-time` feed new PTYs,
client message/overlay timers, and each attached session's repeat-key window.
`list-keys` now matches the pin's selectors, key filtering, aggregate facts, stock copy-table repeat
metadata, global flags, table, and key-column padding. Only the deterministic comparator boundary
described above and the separately tracked long key-modifier spelling overacceptance remain.

# Protocol and process level

| Area | tmux | zz |
| --- | --- | --- |
| Env contract | `$TMUX`, `$TMUX_PANE`, plus server-seeded global and client-updated session overlays | Panes get `$TMUX` in tmux's exact `socket,pid,session` shape plus `TMUX_PANE=%N`. Slice 10ac gives shell-form `run-shell` and `if-shell` clean global-then-session environments and status `#()` a clean global-only environment. Visible modeled `TMUX_PANE` survives, while zz never invents one for jobs. Post-startup TERM identity and the private tmux PATH match the contract above. `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` ride alongside panes. Slice 10af closes positive-delay launch sampling, and the 2026-08-30 shell-job cwd closure aligns command and status paths. Immediate ordering, copy-pipe, and popup jobs remain listed above. |
| Binary argv | `-L -S -f -2 -C -u -V -N -c -l` | Closed by 7a (2026-08-18): `-V` (`tmux 3.8-zz`), `-L`/`-S`/`-f`/`-c`/`-N`/`-l`/`-2`/`-u`, tmux-shaped usage and unknown-option lines, pin CMD_STARTSERVER autostart. `-C`/`-CC` are the phase-6 control-mode front-end (row below). |
| Control mode `-CC` | What iTerm2 integration speaks. | SHIPPED (phase 6 complete 2026-08-18): a stdio front-end speaking the full CC protocol — framing, notifications, `%output` with flow control (pause/age-kill/pacing), `refresh-client -A/-B/-C/-f`. Deliberate divergences, all reviewer-endorsed: blocks are COMPLETE (WAIT commands keep output in-block; after-hooks add no extra block; `%pause`/`%continue` land after the triggering block, not inside); per-client monotonic `n`; zz-lax unquoted `%`-words on the control stdin; automatic-rename transients single-fire. |
| Session groups | `new-session -t`. | Cataloged, rejected. |
| `StatusLine.customized` | No equivalent — tmux has no wire and no explicit-write ledger. | zz-native v71 field: true while any explicit `status`, `status-*`, or `status-format` write is in force for the recipient's scope (even when the value equals the default); scalar and whole-array unsets clear their mark, an indexed `status-format[N]` unset keeps it. It gates only the TUI's `Ctrl-\ detach` hint. GUI visibility instead follows whether the native status model is empty, so `customized` has no GUI appearance effect. |
| Presentation | Status line, prompts, choosers drawn as terminal escapes. | The TUI renders the daemon's personalized `status-format[]` rows through the shared `zz-client` compositor that reproduces `format-draw.c` alignment sections, `fill=`, list focus/truncation, blank-row base style, and hit ranges. It places that authoritative block at `status-position`, replaces the selected `message_line` row with messages or a prompt, and routes window-range clicks. The GUI never paints those rows as tmux-authored cells: its always-native surface uses `status.left`/`status.right`, snapshot-backed window controls, and only the row list-alignment directive. Its top or bottom placement follows the app chrome mode, visibility does not depend on `customized`, and `status-position` has no GUI placement authority. Prompts and choosers stay native on both where implemented. Raw zz-tui handles command prompts, confirmations, menus, popups, choose trees, choose buffers, and display-panes. `display-menu.behavior-fidelity` and `display-popup.behavior-fidelity` own the broader behavior classes outside those presentation closures. |

## Park dispositions (2026-09-01)

The frozen campaign scope carried fifteen groups whose decision was `park`: work deferred by
agreement rather than blocked on a measurement. A park disposition changes no behavior. It records
which side of the model each difference falls on and retires the group, so nothing sits in a queue
nobody intends to drain. `accepted` plus `native` means zz's own surface serves the intent;
`accepted` plus `never` means the capability is deliberately outside zz. Live status stays in
`compat/tmux-gaps.json`; the one-line rationale is here.

- `capture.rich-transports` (native): `-R`, `-P`, `-C`, `-F`, `-H`, and `-L` read tmux's grid and
  input-parser internals, while zz's capture surface is the terminal worker's retained UTF-8 text
  snapshot plus the zz-native output verbs; the six forms stay loudly refused.
- `clients.interactive-refresh` (native): the pin keeps mode and redraw ownership on the server,
  while zz's clients render themselves and copy or view mode lives on the per-client terminal view,
  so `switch-mode` and the interactive redraw and pan family have no zz counterpart.
- `formats.mouse-context` (native): the eight `mouse_*` names are filled only from the mouse key
  event that invoked a command, and zz installs no tmux mouse key tables, so no command ever carries
  one.
- `formats.pane-runtime` (native): the pin's mode names read one shared pane mode stack, while zz's
  copy, view, and search state is per client, so a pane with two viewers has no single answer.
- `formats.terminal-cells` (native): the cursor cell, tab stops, and progress state live in the
  terminal worker's VT, and the mux format engine models topology rather than a grid.
- `formats.terminal-runtime` (native): the 28 VT runtime names keep the pin's inactive or default
  value, which the [status line reference](/tmux/status-line.md) already records as default state
  rather than support.
- `messages.tty-model` (never): `show-messages -T` dumps the server's per-terminal terminfo
  capability table and `-J` its internal job fds; zz's daemon drives no client terminal through
  terminfo and publishes no job registry, so both reports describe a server zz is not.
- `mouse.bound-context` (native): `copy-mode -S`, `resize-pane -M`, `move-pane -M`, and
  `send-keys -M` consume the mouse event that invoked them, and zz's pointer gestures belong to the
  rendering client rather than to a key table.
- `prompt.pane-rendered` (native): `command-prompt -P` paints into pane cells, while zz's prompts
  are client surfaces and its copy-mode numeric prefix already uses the native per-client repeat
  shape.
- `options.lock-program` (native): the pin spawns `lock -np` onto a client's tty because its server
  owns that terminal; zz's daemon publishes frames, so a zz lock is a client-rendered surface and
  the two options stay store-only.
- `options.theme-palette` (native): zz already resolves the pin's ten `theme*` style colour
  names into zz theme tokens in the GUI and through the pin's own fallback indices in the daemon and
  raw TUI, while the twenty-one options that would override those slots stay store-only.
- `pane.floating-model` (native): a tmux floating pane is a mux object placed by `new-pane` and
  `move-pane`, while zz's floating things are presentation objects its clients draw and its panes
  are layout-tree leaves.
- `protocol.binary-streams` (native): zz's client protocol carries typed UTF-8 messages and its CLI
  already reads caller stdin as one bounded payload for its own verbs, so the five tmux `-` forms
  stay loudly refused; the roadmap's milestone 5 still owns the single bounded channel that would
  replace all five at once, so this is the reversible reading rather than an exclusion.
- `protocol.socket-acl` (never): `server-access` hands other Unix users a shared server socket,
  while zz's daemon binds at 0600, keeps no peer identity, and holds one account's PTYs, ssh
  sessions, browser profiles, and agent sessions.

The fifteenth group, `display-panes.command-template`, is deliberately not settled. Its selection
template is ordinary undone work rather than a model difference: the chooser template execution path
that substitutes the selected value and runs the result already shipped on 2026-08-28, so wiring the
overlay's selection to it is a delivery question, not a disposition. It stays blocked.

## Format expansion budgets settled (2026-09-01)

The two budgets registered on 2026-09-01 both resolved toward the pin, in opposite directions.

zz used to clamp every finished expansion to `MAX_STATUS_TEXT_BYTES` in `truncate_output`, which ran
at all three entry points, so command-facing output lost everything past 4096 bytes even though
`#{n:}` of the same format already reported the full length. `truncate_output` is gone. The bound
now lives where the wire enforces it: `clamp_status_text` in `crates/zz-daemon/src/status.rs` bounds
the status title, the base style, and every status row as `StatusLine` is built, beside the left and
right sides `wrap_status_style` already bounded. Measured on a strict differential, both sides print
9000 bytes for `#{p-9000:#{l:tail}}`, 10000 for `#{R:x,10000}`, and 6000 for a 6000-byte user option
read back through `#{E:}`.

Going the other way, the pin's `FORMAT_TIME_LIMIT` is refused, and the refusal is measured rather
than assumed. The budget was implemented first: one deadline stamped at the outermost entry point and
copied into every nested expander, checked on entry to `Expander::expand` exactly where
`format_expand1` calls `format_check_time`. It reproduced the pin's shape, where after
`#{n:#{Ogs:#{Ogs:#{l:x}}}}` burns the budget the trailing literal text still lands and `#{l:lit}`
still expands, because `format_unescape` only samples the clock every 10,000 characters, while
`#{?session_name,yes,no}` beside them expands empty.

It also made zz's command semantics load-sensitive. zz expands option values and command arguments
through the same engine, where an emptied argument is a failure rather than a truncated string. With
the budget in place, `daemon::tests::attached_client_extents_clamp_retained_and_default_dimensions`
failed 5 of 5 runs under eight spinning cores and passed 6 of 6 idle; the same test under the same
load passed 3 of 3 without it, and forcing the budget spent failed it deterministically inside a
`set-option status off`. Trading a benign output difference for a load-dependent command failure is
the wrong side of that bargain, so zz keeps a deterministic engine: a runaway expansion runs to
completion and answers the same string every time, where the pin returns a truncated result whose
length moves run to run. The 40,960,000-byte repeat ceiling remains the one allocation guard.

`FORMAT_LOOP_LIMIT` was never part of the divergence and did not change. Measured on the pin, 200
sibling `#{l:x}` replacements all expand, 99 nested `#{s/x/x/:}` wrappers still reach their body, and
the hundredth answers empty: a recursion depth of 100 on both sides, not a running total.

# Related

- [live tmux compatibility gaps](/tmux/gaps.md) — generated TODO, decision, and status report.
- [tmux drop-in plan](/designs/tmux-drop-in.md) — the 2026-08-16 campaign plan and delivery record.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the contract these divergences are
  measured against.
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — the tier ladder and the amended
  never-list.
- [commands](/tmux/commands.md) — the implemented verb-by-verb reference.
