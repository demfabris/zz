---
type: Subsystem
title: tmux command set (command.rs)
description: "MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon."
resource: crates/zz-mux/src/command.rs
tags: [tmux, commands, mux-engine, targets, effects]
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-09-03
last_updated_by: Claude
---

# Overview

`command.rs` defines `MuxEngine`, the executor that turns a parsed
[`CommandInvocation`](/crates/zz-protocol.md) into mutations on [`MuxState`](/crates/zz-mux.md) plus a list
of [`MuxEffect`](#effects) side effects the daemon acts on. `MuxEngine` owns the `MuxState`, the
[`KeyTables`](/tmux/key-tables.md), and the tmux options that are not per-window layout: `mode-keys`
(global + per-window), `set-clipboard`, `copy-command`, `buffer-limit`, `message-limit`, `history-limit`
(global + per-session), `word-separators` (global + per-session), and the phase-4f behavior-option
overrides. It also owns free-form `@`
option maps at every tmux scope and global/per-session process-environment overlays. `execute` canonicalizes the
command name, dispatches to a handler, and, if the state `generation` advanced, appends a
`SnapshotChanged` effect and repairs the `ExecutionContext` (`session`/`window`/`pane`) so the
current target stays valid after the mutation.

Errors are structured `ServerError`s. Target lookup uses `SessionNotFound`, `WindowNotFound`, and
`PaneNotFound`, displayed as tmux's `can't find TYPE: COMPONENT`; internal stable-ID failures retain
`MissingTarget`. Other command errors use `UnsupportedCommand`, `InvalidCommand`, and the v76
`CommandParse` tail variant. `CommandParse` identifies command-name, flag, arity, and preparation
failures before effects; target lookup and semantic or runtime failures keep their existing variants.
Most handlers validate before mutation. tmux orders some mutations before later failures;
`select-layout`, for example, unzooms its resolved window before it parses a custom layout. The
daemon publishes any generation change at the command boundary even when the handler returns an
error.
`catalog.rs` is the shared renderer-free source for canonical names, aliases, descriptions,
accepted usage strings, flags/options, and completion value kinds; `canonical_command` and the native
[command palette](/concepts/command-palette.md) both consume it.

# Argument parsing and `-t` targets

`parse_tmux_options(spec, args)` is the shared parser for every implemented upstream command. It
uses catalog flag arity, stops at the first positional or bare `--`, splits short clusters, consumes
attached or separated required values, and applies the pin's lookahead to optional values. It emits
the canonical command name in unknown-flag, invalid-flag, usage, and missing-value errors before a
handler reports an unsupported capability. After option grammar, those consumers validate
positional minima and maxima before unsupported-capability rejection. A recognized parked flag
therefore cannot hide the pin's arity diagnostic. Mux execution, daemon-preempted commands, stored
bindings, and hooks use this contract. Exact native attach uses the same leading-option diagnostics,
then stops scanning at its positional-session extension while still accepting trailing
`--restart-daemon`. zz-native commands keep their local parsers. `CommandSpec::pinned_tmux_usage`
supplies the diagnostic text for 24 commands whose honest
zz `list-commands` and completion usage differs from the pin. The three-step
`smoke/command-flag-errors` differential checks 83 canonical commands, 74 aliases, and 516 exact
probes. Protocol v84 adds zero-based unquoted command-block positions to `CommandInvocation`.
`parse_tmux_command_options(spec, command)` applies the command-aware callback rule for commands in
`COMMAND_ARGS_PARSE_BEHAVES` while retaining the shared option diagnostics. Target resolution lives
in `MuxState`:

`validate_static_command_chain` applies this grammar without a live mux or user aliases. The cold
local CLI runs it over the complete raw vector before routing, stdin capture, TUI handoff, daemon
spawn, startup config, or effects. It resolves canonical names, built-in aliases, and unique
prefixes; validates flags, arity, callback argument types, and nested typed blocks; and covers all 83
implemented plus nine parked upstream commands. Exact native `attach` and `attach-session` use the
native parser for their first invocation and validate the tail, which still executes after attach.
`-N` cannot start a daemon. Arbitrary user-alias names cannot trigger autospawn, while startup config
may shadow a canonical spelling.

A successful raw pass generation-identifies the spawned daemon. That daemon prepares the complete
vector under one post-config alias snapshot, including recursive callback construction and static
syntax validation for alias expansions. The CLI checks every result before execution. A failed
bootstrap preparation and owner disconnect retire only the exclusively owned new daemon. Startup
reentry cannot claim or contest the lease; another external client or any command commits it.
Runtime errors keep sequential queue behavior. Slice 10u closes warm unaliased argument groups, and
slice 10z closes config or source file-unit construction. Remote `--host`, Control mode, and
rollback fall outside the cold local contract.

Oracle schema 4 extracts tmux's custom `args_parse` callbacks from the pinned source. The extractor
accepts six rules and fails when a callback body falls outside them:

| Rule | Pinned command positions |
| --- | --- |
| `commands-or-string` | Every positional on `bind-key`, `choose-buffer`, `choose-client`, `choose-tree`, `command-prompt`, `confirm-before`, `display-panes`, and `switch-mode` |
| `display-menu-items` | Each nonempty menu name has a string key followed by a command-or-string action; an empty name consumes no key or action |
| `if-shell-branches` | Positions 1 and 2 are command-or-string branches; position 0 stays a string |
| `run-shell-command-flag` | Every positional becomes command-or-string when `-C` is present and stays a string without it |
| `set-option-value` | Position 1 is command-or-string for `set-option` and `set-window-option` |
| `set-hook-monitor-or-value` | `set-hook -B` makes every positional command-or-string; without `-B`, position 1 uses that type |

`COMMAND_ARGS_PARSE_SPECS` mirrors the 12 implemented commands. `choose-client` and `switch-mode`
remain unimplemented and need no sidecar entry. `COMMAND_ARGS_PARSE_BEHAVES` now contains
`bind-key`, `choose-buffer`, `choose-tree`, `command-prompt`, `confirm-before`, `display-menu`,
`display-panes`, `if-shell`, `run-shell`, `set-hook`, `set-option`, and `set-window-option`. Every `bind-key`
positional accepts a string or typed block while `-T` and `-N` values remain strings. Scanning
stops at the first positional or `--`; a typed key expands the live mux environment and is
recursively printed before key lookup. Unknown typed-key commands keep their source diagnostic.
One typed tail retains its parsed command groups, one string tail is reparsed as one group, and a
typed first variadic tail yields the pin's empty binding. For `if-shell`, zero-based condition
position 0 and option values must be strings, while branch positions 1 and 2 accept either strings
or typed command blocks. For `run-shell`, a leading `-C`, including a combined short-option form,
makes every positional command-or-string; without it every positional must be a string. Option
values stay strings, option scanning stops at the first positional or `--`, and a later `-C` is
positional. Command mode executes positional 0 and accepts but ignores later positionals.
For both set commands, option names, flag values, and extra positionals stay strings while position
1 accepts either a string or a typed block. Typed values expand the live mux environment, then
recursively print canonical command names, same-line `;` groups, physical-line `;;` groups, and
nested blocks before `-F` expansion. Empty
blocks become empty values, while quoted brace text stays a string. The parser preserves lexical
types through source files, Control transport, aliases, bindings, and hooks; validation rejects a
forbidden type before execution or stored-command replacement. `confirm-before` accepts one typed
block or string command while its option values remain strings. Every lexical typed block
constructs recursively before its parent's name, callback type, or arity validation. One
user-alias layer is independent per recursive path; siblings do not consume each other's layer,
alias-produced subtrees disable further user aliases, and direct self-recursion fails as an unknown
command without killing the daemon. Nested `if-shell`, `run-shell`, set-option, and
`confirm-before` blocks print canonical names. Empty blocks print as `{  }`, while physical
internal group newlines print as ` ;; `. String children construct after target lookup and
parent-format expansion as one group. Nested bind and confirm construction failures are preflight
parse errors. `command-prompt` accepts zero or one template positional as a typed block or string;
`-I`, `-p`, `-t`, and `-T` values remain strings. A typed template retains its recursively
constructed command list through submission without another user-alias lookup. A string template
retains raw source, substitutes the answer, then parses and constructs the complete result against
the current alias table before any command executes. Both paths replace the first `%%` and every
`%1`, and a trailing `%` quotes double quotes, backslashes, dollar signs, semicolons, and tildes.
Structured substitution does not reparse the
answer, so quotes and semicolons cannot create new arguments or commands. Typed callbacks retain
physical source groups; a failed group stops while later lines continue. String templates and free
input form one group. String-template diagnostics retain the originating source path and line.
Prompt chaining and multi-answer `%2` remain under `prompt.command-fidelity`. Slice 10z closes
whole-file source construction; multiline replay-channel placement remains open. Without `-B`, `set-hook`
accepts a typed block only at value position 1; its hook name and extra positionals remain strings.
With `-B`, every positional lexically accepts either type, while `-B` and `-t` values remain
strings. zz rejects `-B` at runtime because format monitors remain unsupported. Every typed child
constructs before the parent type, arity, and effects. Typed values normalize before storage as a
built-in hook, a custom `@` option, or a forwarded named option. Built-in hooks construct the value
again and flatten physical groups; custom `@` typed values keep textual ` ;; ` groups. Quoted braces
are runtime syntax for a built-in hook and literal deferred storage for a custom hook. Unindexed
built-in replacement clears before a malformed runtime value fails, while indexed replacement preserves
the prior entry. An unindexed empty value clears without adding an entry, `-a` with an empty value
does nothing, and an indexed empty value remains present. Local array creation precedes empty append
and runtime parsing, so an empty or failing local append creates an empty local array that shadows
the inherited global hook. `-R` constructs a typed supplied value
before it runs the stored hook but ignores a supplied quoted string. `display-menu` tracks repeated
NAME, KEY, and ACTION fields from the positional data. A nonempty NAME advances through a
string-only KEY to an ACTION, which accepts a string or typed block and resets the state to NAME.
An empty NAME is a separator that consumes no KEY or ACTION. Its ten valued flags remain strings.
Typed children construct before the parent type, arity, or effect boundary, so child failures
precede rejected NAME, KEY, and option-value types. Stored bindings print accepted typed actions
with canonical child command names and retain quoted actions as strings. Incomplete NAME and
NAME-plus-KEY tails construct successfully and reach daemon runtime. Runtime resolves the current
or `-c` target client before completeness, so an unattached command or initial Control reports `no
current client`; initial Control uses a flag-0 `%error` and exits 1. Once attached, Control validates
an incomplete group as `not enough arguments` before its overlay no-op and returns a flag-1
`%error`; EOF after that frame exits 1.
Interactive ordering remains unchanged. `display-panes` accepts zero or one positional template as
a string or typed block, while `-d` and `-t` values remain strings. Every typed child constructs
before parent option-type or arity validation. Canonical, built-in alias,
unique-prefix, and preexisting user-alias forms retain typed positions and canonical stored
readback. Targetless daemon routing resolves an attached client before duration validation: no
attached client reports `no current client`, while a Command client can select an attached
Interactive client. This closes the parser rule, not the custom action: mux execution still rejects
a positional template instead of substituting the selected `%pane` for `%%%` and executing with
the original queue state. Tmux uses `select-pane -t "%%%"` when the template is omitted. This is tracked under
`display-panes.command-template`. `choose-buffer` and `choose-tree` each accept zero or one
string-or-typed template while `-F`, `-f`, `-K`, `-O`, and `-t` values stay strings. Every typed
child constructs before parent type, arity, target, or effects. A typed template stores constructed
canonical command text before opening; a string template stays raw. Selection substitutes the
exact buffer name or tree target, parses against the current alias table, and executes in the
invoking client's live context after closing the chooser. Tree values use `=name:`, `=name:index.`,
or `=name:index.%id`. The first `%%` and every `%1` receive the value; a trailing `%` applies the
pin's quoting. Empty and stale buffers run no custom action, and attached parse or command errors
start with an uppercase character. All 12 implemented callback commands now apply their pinned
rule, leaving no command-specific `args-parse:` item.

| Target | Resolver | Accepts |
| --- | --- | --- |
| `-t $N` / session name | `resolve_session` | `$id`, exact name, `=name` exact-only escape, unique prefix, unique `fnmatch` pattern (`*`, `?`, `[…]`, with `/` ordinary), or the current/first session. An empty string means current. |
| Leading `=` | slot classification | The equals sign is stripped into an exact-match flag only where the token reaches the session or window slot, and a colon-less token reaches the slot the command's own target type names. `set-option` is pane-targeted, so `-t =name` is a pane name that matches nothing and the option scope reports `no such session: =name` with the raw argument, while `-t =name:` succeeds and `set-window-option -t =name` and `has-session -t =name` succeed on their own slots. Target-parser diagnostics report the stripped name; option-scope diagnostics echo the raw `-t`. |
| `-t @N` / `:index` / name | `resolve_window` | `@id`; wrapped `+N`/`-N`; `{start}`/`^`, `{end}`/`$`, `{last}`/`!`, `{next}`/`+`, `{previous}`/`-`; numeric index; exact name; `=name` exact-only escape; unique prefix; unique `fnmatch`; or current. The first session/window component that fails names itself in the error. |
| `-t %N` / pane index | `resolve_pane` | `%id`, pane index, wrapped `+N`/`-N`, `!`/`{last}`, `{next}`/`{previous}`, `{up-of}`/`{down-of}`/`{left-of}`/`{right-of}` searched from the window's own active pane, the case-insensitive compass words `top`, `bottom`, `left`, `right`, `top-left`, `top-right`, `bottom-left`, `bottom-right` and their braced spellings, a window target's active pane, or current. Compound failures report only the failing pane component. |
| `-t ~` / `-t {marked}` | `resolve_session`, `resolve_window`, `resolve_pane` | The server's one marked pane, resolved as a whole target before any session, window, or pane splitting. Without a mark every slot answers `no marked target` and exits 1, and `display-message` swallows that under CANFAIL and retains nothing. A split component such as `{marked}.0` is an ordinary name that matches nothing, exactly as in the pin. |
| `-t:.+` / `-t:.-` | `select-pane` | canonical next/previous pane in `pane_order`. |
| `-t` on `new-window` / `new-browser` / `break-pane` | `resolve_window_index_target` | The same centralized grammar in tmux's target-window-with-index mode. A numeric or `+N`/`-N` destination may name an unused index; existing names, patterns, and tokens contribute their resolved index. A taken `new-window` index fails with `create window failed: index N in use` unless `-k` replaces it. With `break-pane -a` or `-b`, an unused numeric destination falls back to the destination session's current window before placement. |

# Schema: commands (name | aliases | purpose)

When `new-session` omits `-s`, `next_session_name` selects the first unused canonical decimal name
(`0`, `1`, …). It marks numeric names in a bounded `n + 1` occupancy bitmap and materializes only the
winning string; noncanonical lookalikes such as `00` do not reserve `0`.
Because a Command-spawned daemon starts empty, its first `new-session` receives name `0` and the
first session, window, and pane ids are `$0`, `@0`, and `%0`.

Implemented client-selector flags share one attached-client matcher. `detach-client -t`,
`switch-client -c`, `display-message -c`, `display-panes -t`, `display-popup -c`,
`display-menu -c`, `confirm-before -t`, `refresh-client -t`, `lock-client -t`,
`load-buffer -t`, and `set-buffer -t` accept an exact registered name, the exact published
`#{client_name}`, a full tty,
or the tty after removing exactly one leading `/dev/` prefix, with exactly one optional trailing
colon. Published names follow the tty, registered-name, `client-PID`, and `device-N` ladder, so a
nameless Control client can be targeted by the value returned from `list-clients`. They do not
accept a final pathname basename: `/dev/pts/3` admits `pts/3`, but not `3` unless `3` is the exact
client name.
Collisions choose the globally oldest attached client regardless of session. The shared native
`device-N` alias remains; popup, menu, confirm, refresh, and lock also retain numeric `N` and
`client-N` aliases. Unsupported `command-prompt -t`, `show-messages -t`, `send-keys -c`, and
`suspend-client -t` stay with their existing gaps. A local Control client contributes a tty to this matcher only when stdin is a
terminal; piped Control stdin contributes none.

| Command | Aliases | Purpose |
| --- | --- | --- |
| `new-session` | `new` | Create a session with its first window and terminal pane, then request attachment for an interactive caller; command-only callers remain detached. A targetless Command client starts from the most-recent session, so `-c '#{pane_current_path}'` expands against the same origin the pin selects. An explicit `-n` expands, validates, and vis-cleans exactly once before `-s`; `-s` then takes the same path before `-A`, duplicate lookup, and creation. Both use the invoking command item's canonical name and a genuinely attached client's actual session, focused window, and active pane, while explicit item state still wins. Fresh Interactive and Command clients contribute no attached defaults even when their mutable execution context carries the most-recent target. Empty and valid Unicode results remain valid, ASCII controls are rejected, and backslashes receive the pin's stored-name escaping. A supplied `-n` installs window-local `automatic-rename off` on creation; an existing-session `-A` still expands and validates `-n` first, then ignores it. Nested non-detached creation preserves the pin's error order: target conflict, expanded `-n`, expanded `-s`, `-A`, duplicate lookup, unresolved `-t` group-name validation, and expanded `-c` all precede the mutation-free nesting refusal, which still precedes terminal and size validation. Fresh creation copies the invoking client's exact and wildcard-matched `update-environment` values, including empty values and unset markers for selected missing names. Repeated `-e NAME=VALUE` entries then overlay that seed, persist on the session, and reach its first pane; later values win and entries without `=` are ignored. An `=VALUE` entry remains visible in the session environment as `=VALUE`, but is not exported to the child, matching the pin's split between `environ_put` and `environ_push`. On the creation path, `-E` skips the client seed but still applies explicit `-e` values. When `-A` finds an existing session, it follows attach refresh, honors `-E`, and ignores `-e`; bare `-A` resolves the current session, `-D` detaches other clients, and `-d` is ignored. `-f` applies requested client flags only on an attaching path. `-P`/`-F` print the created pane, and `-x`/`-y` size detached creation. `-X` is `-D` with the parent-hangup exit action, so an attaching `-A` evicts peers that then print the SIGHUP notice and signal their own parents; without `-A` there are no peers to evict. A narrow nested-routing preflight accepts `-t` only to match the pin's refusal precedence; normal execution still rejects session groups. |
| `list-sessions` | `ls` | List sessions with `-f` format filtering, `-O` sort order, and `-r` reversal. Filters run after sorting, and `#{line}` remains the pre-filter sorted index. Without `-O`, `-r` is a no-op like tmux. |
| `rename-session` | `rename` | Resolve the target, expand its new name exactly once through that target's active pane, reject ASCII controls, and apply the shared tmux vis-cleaning path before same-name and duplicate lookup. The pin therefore exposes `session_format=0` and `pane_format=1`, while the target's old session, window, and pane facts remain available. Empty and valid Unicode names remain valid; backslashes are stored escaped. |
| `kill-session` | . | Remove a session and its windows/panes. Attached clients follow that session's `detach-on-destroy` policy: `off` selects the most recently active survivor, `on` exits, `no-detached` selects the newest unattached survivor, and `previous`/`next` walk session names with wrapping. When `on` has no primary, or `no-detached` finds no unattached primary, only a client retaining `no-detach-on-destroy` falls back to the newest remaining session. `-a` keeps the target and kills every other session; `-f` filters those candidates in their session contexts. `-C` clears the session's pane bells and kills nothing, outranking `-a` as in tmux. Positional targets are refused — tmux's bound is zero arguments, and the kill commands are too destructive to guess. |
| `attach-session` | `attach` | Attach the client to a session (`Attach` effect); `-d` detaches the session's other clients, `-r` marks this client read-only, and `-f` applies the final requested-client flag mutation. `-c` sets the session cwd after format expansion in the resolved target pane; a compound `-t` selects that window and pane first, and both selection and cwd mutation survive terminal-open failure like the pin. After target and terminal preflight, attach refreshes the session from the client's effective `update-environment` patterns; `-E` preserves it. The direct native attach and Control path use the same refresh. `-x` selects the same peers as `-d` and evicts them with the parent-hangup exit action, so each victim prints `[detached and SIGHUP (from session NAME)]` and signals its own parent. The nested check applies here and to an attaching `new-session` before mux state changes only when the hello carries `client-nested-v1` and its independently retained tty matches a pane. Local terminal surfaces and Command clients retain a discoverable tty for client targeting; `env -u TMUX` omits only the nested marker and forces either attach path like the pin. Local Control publishes the same two identity facts only from terminal stdin and a nonempty `$TMUX`; piped stdin omits its tty. Existing `attach-session`, `new-session -A`, and `new-session -Ad` refusal paths require both facts, while fresh `new-session` and `-A` misses still create and attach and duplicate or validation errors keep their precedence. Control sends no size fact or resize message. Its explicit `refresh-client -C` geometry is separate from the TUI-only `ClientTerminalSize` update. |
| `detach-client` | `detach` | Detach through a typed `DetachRequest`. `-t` uses the supported attached-client selector above. Without `-t`, an attached Interactive or Control caller selects itself; a Command caller prefers the best client on its origin pane's session, then the best client on the newest attached session. Bare detach removes that client, `-a` removes every attached peer except it, and `-s` wins over `-a` and removes every client on the resolved session. Explicit client lookup precedes source-session lookup; a missing `-s` source is a quiet no-op. Read-only clients may detach only themselves. A Control process exits only when it receives its own `Detached` event. `-a`, `-t` naming another client, `-s` excluding the caller, and no-victim forms keep it alive and preserve a queued blank-line or EOF Return; canonical and user aliases follow the same victim set. Self-targeting forms exit 0, with the command's `%end` before `%exit`. Requested detach and attach stealing retain distinct Requested and Evicted reasons. `-E` takes a shell command and replaces each victim's presentation process with the victim session's `default-shell` running `-c command`, printing no notice and hanging up no parent; an empty `-E` is a no-op that leaves the client attached, and `-E` wins over `-P`. `-P` prints `[detached and SIGHUP (from session NAME)]` and then sends `SIGHUP` to the client process's parent when that parent is not init. Both ride the v91 `ClientExitAction` on the `Detached` event. |
| `switch-client` | `switchc` | Retarget an attached Interactive or Control client. `-t` accepts session, window, pane-index, and `%pane` targets; `-n`/`-p` walk sessions with wrapping and accept `-O` sort order; `-l` returns to the live previous session; `-T` switches only the client's key table; `-r` toggles read-only state and reverses an explicit `-O` walk; and `-Z` preserves zoom around a pane switch. A normal switch resets the target client's table to its session root, while a switch executed by a `bind-key -r` binding keeps that table even when `-c` selects another client. A session switch refreshes from the selected target client's environment, not from an external command caller; `-E` preserves the destination map and `-T` returns before any refresh. `-c` uses the supported attached-client selector above, and `-F` is accepted and ignored like the pin. Read-only input is enforced at the daemon input funnel; output, resize, detach, and the pin's read-only command roster remain available. |
| `list-clients` | `lsc` | List attached clients from the daemon registry. `-f` filters, `-O` sorts, `-r` reverses an explicit sort, `-t` restricts by session after the global sort, and `-F` expands the complete retained client fact record plus session last-attached time. Natural order is daemon insertion order; `#{line}` is the global pre-target/pre-filter index, so restricted output can contain gaps. Interactive clients without a retained outer size use `80x24`, missing `TERM` becomes `unknown`, and Control keeps terminal-only fields empty unless explicit state supplies them. Detached command connections do not appear. |
| `refresh-client` | `refresh` | `-A`/`-B`/`-C`/`-f`/`-F` provide control-mode flow, subscriptions, and sizing; `-t` applies those implemented paths to the supported attached-client selector above. `-C` writes Control geometry directly; Control does not use the TUI-only `ClientTerminalSize` message. A detached command client gets `no current client`; bare redraw, `-S`, and the attached-client redraw/scroll family remain unsupported. |
| `new-window` | `neww` | Create a terminal window at the `-t` destination. `-d` creates without selecting, `-a` inserts after an occupied target, and `-b` inserts before it; `-b` wins when both are supplied, while an explicitly free index stays unchanged. `-k` replaces whatever holds the index. An explicit `-n` expands once in the destination session's target context, rejects ASCII controls, and applies the shared tmux vis-cleaning path before lookup or creation. Empty and valid Unicode names survive, backslashes are stored escaped, and the cleaned value is the created window's identity. Repeated `-e NAME=VALUE` entries overlay only the new pane's child environment in order, so the last value wins; entries without `=` and empty names do not reach the child, and nothing is stored in the session environment. `-E` creates a live empty pane with no child process and accepts either no command or one empty-string argument; a nonempty command errors before creation. Without an explicit destination index, `-S` expands the cleaned name a second time for lookup, selects the unique matching existing window, errors on duplicates, and suppresses creation output and the `after-new-window` hook on reuse. A lookup miss creates with the cleaned first-pass name; an explicit index skips the second expansion. `-P` prints a created pane after spawn with the default `#{session_name}:#{window_index}.#{pane_index}` or an `-F` format, including runtime start path/command, PID, and TTY facts; `-F` without `-P` is silent. An explicit `-n` also installs the pin's window-local `automatic-rename off`. |
| `new-browser` | . | *zz-native:* create a browser window (`-p` profile, URL positional); shares `new-window`'s destination options. |
| `list-windows` | `lsw` | List windows with `-f` format filtering, `-O` sort order, and `-r` reversal. `-a` flattens all sessions into one globally sorted winlink vector; without it, only the target session is sorted. `#{line}` is tmux's total row count rather than the row index. |
| `rename-window` | `renamew` | Resolve the target, expand its new name exactly once in that window's old target context, reject ASCII controls, and apply the shared tmux vis-cleaning path. Empty and valid Unicode names remain valid, backslashes are stored escaped, duplicate window names are allowed, and a window-local `automatic-rename off` preserves the explicit name. |
| `select-window` | `selectw` | Activate a window. `-n`/`-p`/`-l` are the `next-window`/`previous-window`/`last-window` commands (`-n` wins over `-p` wins over `-l`, as in tmux); `-T` on a target that is already current behaves like `last-window`. |
| `next-window` / `previous-window` | `next` / `prev` | Step windows (wraps). A step that would land on the current window errors `no next window` / `no previous window`, tmux's strings; `-t` picks the session; `-a` steps to the next/previous window holding an alert (any pane with a latched bell) with the same error when none qualifies. |
| `kill-window` | `killw` | Remove a window; `-a` keeps the target and kills every other window in its session, with `-f` filtering those candidates in window context. |
| `move-window` | `movew` | Move a stable window to another index or session. `-a` and `-b` insert around an occupied target, `-d` avoids selecting the moved window unless `-k` replaces the current destination, `-k` replaces an occupied index, and standalone `-r` renumbers from `base-index`. An occupied destination without `-k` returns `index in use: N`. |
| `swap-window` | `swapw` | Exchange two window slots within one session or across sessions while window ids, panes, names, and layouts travel together. `-d` selects the destination slots after the swap. |
| `find-window` | `findw` | Accept tmux's `-CiNrTZ` search grammar and validate `-t`. Detached CLI calls return success with no output for both matches and zero matches; zz does not open a GUI chooser. |
| `split-picker` | . | *zz-native:* split into a runtime-free picker (`-h` horizontal, else vertical); accepts the `split-window` target/size/cwd options. By default the picker is revealed and selected, clearing an existing zoom; `-d` creates it in the background, keeps the current pane selected, and preserves zoom. Bound on zz's default `%`/`"` keys. Renamed from `new-pane` (the pinned tmux owns that name for floating panes). |
| `split-window` | `splitw` | Split a terminal pane and inherit the target pane's live cwd; key-bound, CLI, and command-prompt invocations all create a terminal, like tmux. Repeated `-e NAME=VALUE` entries overlay only the new pane's child environment, with entries lacking `=` and empty names omitted and the last named value winning. `-E` creates a live empty pane with no child process and accepts either no command or one empty-string argument; a nonempty command errors after target resolution and before layout mutation. The new pane's share comes from `-l` (cells, or `N%`) or legacy `-p N`; `-b` puts it left/above, `-f` spans the whole window, and `-d` leaves focus on the existing active pane. After a successful split, `-Z` zooms that post-spawn active pane: the new pane normally, or the existing pane under `-d`. Without `-Z`, every successful split clears an existing zoom, including detached splits. `-P`/`-F` use the same post-spawn output contract as `new-window`. `-k` and `-m` write `remain-on-exit` `key` on the new pane's own option table and leave the window and global scopes alone, so that pane alone is retained after any child exit until the next key; `-m` also stores its argument as that pane's raw, unexpanded `remain-on-exit-format`. |
| `split-browser` | . | *zz-native:* split into a browser pane (`-p` profile, URL positional). By default the browser is revealed and selected, clearing an existing zoom; `-d` creates it in the background, keeps the current pane selected, and preserves zoom. |
| `select-pane-kind` | . | *zz-native/internal:* materialize a pending picker as `terminal`, `browser`, `agent`, or `editor` (`-t` target; `-c` supplies an Agent cwd). |
| `break-pane` | `breakp` | Reparent a pane into a new one-pane window (`-n`,`-s`,`-t`,`-d`). Unlike the other explicit name paths, `-n` is literal: format tokens are not expanded. The literal rejects ASCII controls and takes the shared tmux vis-cleaning path before placement, preserving empty and valid Unicode names, escaping backslashes, and allowing duplicate window names. An explicit `-n` pins window-local `automatic-rename off` after both whole-window relinking and new-window creation. `-a` inserts after the resolved destination window and `-b` inserts before it; `-b` wins when both are supplied, and an unused indexed `-t` falls back around the destination session's current window. Without placement flags, `-t` names the new window index. `-P` prints the moved pane using the default or an `-F` format. |
| `join-pane` / `move-pane` | `joinp` / `movep` | Insert a pane beside another (`-b`,`-f`,`-h/-v`,`-l`,`-s`,`-t`,`-d`; `join-pane` also accepts legacy `-p N`). `-l` accepts cells or `N%`, expands formats in the destination pane context, and uses the destination pane as its size basis unless `-f` selects the whole window. `-p` accepts a bare number from 0 through 100; a `%` suffix is invalid. The pin rejects `move-pane -p`. |
| `set-browser-url` | . | *zz-native:* update a browser pane's URL. |
| `set-browser-profile` | . | *zz-native:* validate and switch a browser pane's persistent zz profile (`-t`, one profile name). |
| `set-agent-session` | . | *zz-native/internal:* atomically persist an Agent pane's opaque ACP session ID and optional absolute cwd (`-t`, `-c`). |
| `set-agent-provider` | . | *zz-native/internal:* persist `codex` or `claude-code` for an Agent pane, clear its provider-bound session ID, and restart its adapter (`-t`). |
| `restart-agent-pane` | . | *zz-native/internal:* replace an Agent pane's daemon-owned ACP adapter (`-t`). |
| `select-pane` | `selectp` | Select by target/direction (`-L/-R/-U/-D`), `-l` last, `-Z` keep zoom; `-T` updates only the pane title. A cross-window target changes that window's active pane without changing the session's current window. Flags run in the pin's order: `-l` first, then `-m`/`-M`, then `-P`, then `-g`, then the directional search, then `-e` before `-d`, then `-T`, then the selection. `-m` marks the target and `-M` clears the server's one mark; re-marking the holder clears it, a pane hidden by zoom refuses `-m` but still accepts `-M`, and the mark dies with its pane. `-d` disables input to the resolved pane and `-e` enables it, both without selecting; `-e` wins when both are present. `-P` writes the pane-local `window-style` and `window-active-style` without validating the string, then falls through to the selection, and the deprecated `-g` prints that pane's effective `window-style` and returns first. |
| `last-pane` | `lastp` | Return to the previously active pane (`-Z` preserves zoom). `-d` disables input to that pane and `-e` enables it without selecting it; `-e` wins when both are present. With exactly two panes and no selection history, the other pane is the last pane. |
| `swap-pane` | `swapp` | Exchange two layout leaves (`-U/-D/-s/-t`, `-d` keep slot, `-Z`). |
| `list-panes` | `lsp` | List panes with `-f` format filtering, `-O` sort order, and `-r` reversal. `-s` lists every window in the target session; `-a` lists every session. Pane sorting remains per-window rather than flattening globally, and `#{line}` is the pane count for that window. |
| `resize-pane` | `resizep` | Resize relatively with optional integer amounts on `-L`, `-R`, `-U`, and `-D`. A bare `-L` uses the default amount `1`; `-L2` and `-L 2` both request two cells, and the other directions accept the same three forms. Runtime already supported these forms; the catalog and compatibility manifest now model their values as optional. Absolute `-x`/`-y` sizes accept cells or `N%`, and `-Z` toggles zoom. Absolute percentages accept 0 through 1000, then normal layout limits clamp the result. With no direction and no `-x`/`-y` the command is a no-op, as in tmux. `-M` and `-T` remain rejected. Sizes in cells need the geometry the daemon reports per pane; without it the command errors instead of guessing. |
| `resize-window` | `resizew` | Set a durable manual window extent with `-x`/`-y`, or adjust it with `-L`/`-R`/`-U`/`-D` and an optional positive cell count. Absolute sizes accept 1 through 10,000; relative and client-derived results clamp to that range. `-A`/`-a` perform a one-shot componentwise largest/smallest resize from eligible attached Interactive and Control geometry in the target session; `-A` wins when both are present, and either aggregate overrides valid numeric adjustments. The result selects window-local `window-size manual`, preserves zoom, exposes `window_manual_width`/`window_manual_height`, and is not overwritten by later client measurements. An `ignore-size` client is excluded whenever any unignored attached sizing client exists globally; if all are ignored, ignored target-session clients re-enter. Per-window Control `refresh-client -C` geometry caps both dimensions. With no eligible target-session geometry, the session `default-size` is used. |
| `select-layout` | `selectl` | Apply a named preset or checksummed layout string, restore with `-o`, spread with `-E`, or cycle with `-n`/`-p`. The first `-o` with no saved layout succeeds silently and saves the current layout for the next restore. A layout string ignores its pane numbers, assigns the current `pane_order` through the leaves, removes extra bottom-right cells, allocates new divider ids, and adopts the encoded window extent. Too few cells fail with tmux's `have N panes but need M` error. |
| `next-layout` / `previous-layout` | `nextl` / `prevl` | Cycle the seven presets. |
| `rotate-window` | `rotatew` | Rotate surfaces through layout slots (`-D`,`-U`,`-Z`). |
| `kill-pane` | `killp` | Remove a pane (removes the window if it was the last); `-a` keeps the target and kills every other pane in its window, with `-f` filtering those candidates in pane context. |
| `respawn-pane` | `respawnp` | Restart a terminal pane in the same stable pane id and layout leaf. A live pane needs `-k`; otherwise the command reports tmux's `pane SESSION:WINDOW.PANE still active`. `-E` starts from an empty environment, `-c` replaces the stored cwd, repeated `-e NAME=VALUE` entries overlay the session environment, and an omitted command/cwd reuses the prior spawn recipe. |
| `respawn-window` | `respawnw` | Restart a terminal window in place. The first pane keeps its id, the other panes are removed, and the layout collapses to that retained leaf. Live panes need `-k`; `-E`, `-c`, repeated `-e`, and stored command/cwd reuse match `respawn-pane`. |
| `send-keys` | `send` | Send keys/text (`-l` literal, concatenating its arguments byte-for-byte like tmux; `-H` hexadecimal ASCII codes, `0x` prefix accepted; high bytes tmux would write raw are refused because `KeyToken::Literal` carries UTF-8) or `-X` copy-mode actions. `-N` expands its last value and accepts 1 through UINT_MAX with the pin's invalid, too-small, and too-large errors; attached values, short clusters, `send`, and unique command prefixes work. A whole terminal key list travels in one compact repeat field, and daemon delivery stops at the first full input queue. Browser sinks use the native `MAX_BROWSER_KEY_REPEAT` cap of 9,999 because tmux has no browser pane. Prefix-consuming copy movements, jumps, matching brackets, and repeat-search actions run N times; `other-end` swaps only for odd N; `select-line` spans N lines; the copy-end-of-line family selects through the end of row N and copies once; other toggles, selection, copy, clear-selection, and cancel run once. A bare `-N <n>` with no keys and no `-X` arms the client's native copy-mode repeat prefix, still capped at 9,999. The first `send` or `send-keys` command whose option prefix contains `-X` consumes it: a stored `-N` wins, otherwise the engine inserts separate `-N <count>` arguments immediately before the option argument containing `-X`. The engine does not scan onward after a stored `-N`; a binding with no qualifying `-X` leaves the count armed. For a read-only client, absence of `-X` is decided before full option and repeat parsing, so unsupported `-M` still answers `client is read-only`. `-X` allows the pin's read-only-safe typed movement, history, line, word, paragraph, prompt, bracket, goto-line, set-mark, jump-to-mark, and cancel actions. Selection, copying, search, jump capture, rectangle, and pin-recognized unsafe copy-line, selection-mode, scroll-exit, and search forms answer `client is read-only`. An empty or genuinely unknown `-X` action remains authorization-safe and follows the ordinary no-mode or no-op path, matching the pin's separation between command and window-copy authorization. `-F` is accepted as tmux's inert flag. The outer grammar rejects `-C`, `-P`, and `-o` with the pin's unknown-flag error. The copy-mode parser recognizes `-C` and `-P` after the action on the pin's 14 copy-family grammar entries, including a `-CP` cluster, and recognizes `-o` after `next-prompt` or `previous-prompt`. A local `--` ends flag parsing. Invalid local flags, actions, or arity run no copy action and reset the repeat prefix to 1. The `copy-line`, `copy-line-and-cancel`, `copy-pipe-line`, and `copy-pipe-line-and-cancel` handlers all ship, and `terminal.key-control` closed on 2026-09-02. zz still has no no-op redraw effect, so the pin's redraw of the first copy-mode line after a local parser failure has no zz counterpart. `-M` is the one flag with no zz model, and it is rejected rather than dropped. |
| `copy-mode` | . | Enter copy mode (`-u` page up, `-d` page down, combinable; tmux applies `-u` then `-d`). `-H` hides the native position indicator. `-e` latches exit-at-bottom on fresh entry: scroll-down/page-down/halfpage-down landing at the live bottom with no selection leaves copy mode, and `-ed` at the bottom exits instantly. `-q` pops copy mode and returns. These entry, movement, and cancel paths remain available to read-only clients and affect only their attached local view. `-M` is tmux's mouse-drag entry; without a mouse event it is a silent no-op. `-k` kills the pane when the mode is torn down, following `window_pane_set_mode`'s early return so a re-entry never clears the bit. `-s` backs the mode with another pane's screen: the source's grid is cloned with `window_copy_clone_screen`'s trim, so the cursor opens on column zero of the last used row, keys still go to the target pane's mode, `capture-pane` still reads the target's own grid, and `refresh-on` cannot arm because `window_copy_refresh_start` refuses a view of another pane; naming the target as its own source is the plain entry. `-S` is rejected. |
| `copy-mode-search-prompt` | . | *zz-native:* open the native copy-mode search prompt (`-b` backward). |
| `command-prompt` | . | Open the native command prompt (`-p`, `-I`, `%%` template). `-b` is accepted and already true: the prompt never blocks its caller. `-T command\|search` picks the history ring; the mode flags resolve in the pin's order `-1`, `-N`, `-i`, `-k`, `-e` with `-C` orthogonal. `-1` submits one key, `-k` submits that key's NAME, `-N` collects digits and lets the first non-digit both submit and reach the key tables, `-i` runs the template on every edit with an `=`/`-`/`+` prefix, `-e` exits on a backspace at an empty buffer, and `-C` keeps terminal frames flowing where a plain prompt freezes them. The template positional accepts a typed block or string. Typed templates retain their structured constructed command lists through submission; string templates substitute raw source and then parse and construct the full result against the current alias table. Both paths replace the first `%%` and every `%1`, with trailing-percent quoting. Typed substitution preserves argument boundaries against quote or semicolon injection. Typed physical groups retain their boundaries, while string templates and free input form one group. String failures keep the originating source path and line. Prompt chaining and multi-answer `%2` retain their separate owner. `-l`, `-F`, `-t` and `-P` are still rejected. |
| `show-prompt-history` / `clear-prompt-history` | `showphist` / `clearphist` | Show or clear the separate command and search prompt rings. `-T command` or `-T search` selects one ring; omitting it shows or clears both. Show output numbers entries oldest first with the pin's header and blank lines. Invalid types error, and clears rewrite the configured `history-file`. Runtime saves serialize record/clear races so stale history cannot reappear on disk. |
| `focus-sidebar` | . | *zz-native:* show and focus the workspace sidebar (`-t`). |
| `choose-tree` | . | Open the native hierarchy chooser: panes by default, windows with `-w`, sessions with `-s`. `-f` filters in pane context, `-O` sorts each hierarchy level, and `-r` reverses the default index order or the explicit sort. Zero matches restore the unfiltered tree and show `filter: no matches`. An optional string-or-typed template substitutes the selected exact session, window, or pane target and executes against the invoking client's live context; without one, selection keeps the native activation. `-Z` is accepted and already true: the full-window overlay has nothing left to zoom. The default `C-b s`/`C-b w` bindings still call zz-native `focus-sidebar` directly. |
| `choose-buffer` | . | Open the paste-buffer chooser (`-Z`,`-t`,`-f`,`-O`,`-r`). It defaults to creation order newest first, preserves source-pane context for filters, and falls back to the unfiltered list with `filter: no matches` on zero matches. An optional string-or-typed template substitutes the selected exact buffer name; without one, selection pastes. An empty store opens nothing, and a selected buffer removed while the chooser is open closes it without executing the template. `-Z` is accepted and already true, as for `choose-tree`. |
| `show-messages` | `showmsgs` | Print the daemon's message ring newest first with tmux's timestamps. The server-scoped `message-limit` bounds retention at insertion time and defaults to 1,000. Successful command-client invocations produce `command:` entries; failures produce one `message:` entry with the error. `display-message` without `-p` also adds an entry. |
| `display-message` | `display` | Expands a pane-scoped format. With no message or `-F`, ordinary calls use the pin's timestamp-bearing default and `-l` preserves it as literal text. `-p` prints to the caller; a nonprinting call delivers to the supported `-c` client and owns that destination's duration, freeze, replacement, and sticky `-N` state. The `-t` pane remains independent. Client formats use `-c` only when that client belongs to the retained target session; otherwise they use that session's highest-activity attached client. A valid unattached target widens to the highest-activity client across sessions. The oldest-created client wins either tie. Protocol v83 supplies the complete retained client fact record in explicit and implicit cases. Missing sessions and zero attached clients leave client facts empty. CANFAIL target lookup retains each valid component before a later miss. `-d` accepts `0..=4294967295`; omitted duration comes from the destination's attached session. Positive Interactive messages can freeze publication and set or clear ignore-keys; zero waits for writable input. `-C` keeps frames flowing. Read-only callers still fail authorization, while a writable caller may target a read-only client. `-a` runs `format_each` before the template is looked at, so it ignores `-l`, a message argument and a missing `-p` alike and prints one `name=value` line per format variable this target's callback answers, in the table's alphabetical order, then the command's own tree entries with `command=display-message` last; the `-F` conflict is still refused first. `-v`, `-I`, and relative or special pane targets retain their tracker groups, and a bare mouse `-t =` is settled under `mouse.bound-context` because zz installs no tmux mouse key tables, so no command is ever invoked from a mouse event. |
| `display-panes` | `displayp` | Pane-number overlay (`-d` duration). `-t` uses the supported attached-client selector above. The overlay uses that client's current window, and client lookup precedes `-d` validation. An omitted `-d` uses the target session's effective `display-panes-time`; zero installs no deadline. The optional positional selection template now accepts a string or typed block and constructs before parent option-type or arity validation. Runtime still rejects that positional value instead of substituting the selected `%pane` for `%%%` and executing with the original queue state; tmux uses `select-pane -t "%%%"` when the template is omitted. `display-panes.command-template` keeps this loud gap separate from parsing. `-N` disables pane selection: the first key closes the overlay and continues through ordinary input handling. `-b` is accepted, but zz always returns immediately; unlike tmux, omitting it does not block the command queue until the overlay closes. |
| `clear-history` | `clearhist` | Clear a pane's scrollback. `-H` parses and binds and runs the same clear: it adds `screen_reset_hyperlinks` on the pin, and zz's hyperlink identity lives inside libghostty with no registry to reset and no capture transport that would show one. |
| `bind-key` / `unbind-key` | `bind` / `unbind` | Add/remove key bindings (`-n`,`-r`,`-T`,`-N`). `unbind-key -a` removes the selected table (`-T` outranks `-n`), resets clients using it to their session's default table, and `-q` suppresses handler errors but not parser/arity errors, matching the pin. Empty `{}` installs an empty command list, and a single trailing escaped separator is ignored. Payloads validate at bind time (names + flags; tmux validates the full template): unknown names error `unknown command: X`, cataloged commands get their flags checked, and a real-but-unimplemented tmux command errors as unsupported so config import counts it. Daemon-native verbs use the same shared specs, including long-option validation and rendering. |
| `list-keys` | `lsk` | Print bindings as `bind-key` lines with tmux's global repeat, table, and key-column padding. `-T` errors after `unbind-key -a` removes `prefix`, `copy-mode`, or `copy-mode-vi`; `root` remains an implicit empty table because every command client uses it. `-N` prints the `prefix` table followed by `root`, sorts those tables independently, keeps bindings that carry a note, and falls back to the command text when a stored note is empty. `-a` with `-N` includes unnoted bindings; without `-N` it has no effect. `-T` selects one table before note filtering, and `-P` replaces the displayed prefix with a literal string. The optional `[key]` filters every selected table after options, with `--` ending option parsing; a valid absent or note-filtered key reports `unknown key`, while a malformed spelling reports `invalid key`. `-O` accepts tmux's case-insensitive sort names; `key` uses typed tmux key identity, `order` uses traversal order, repeated `-O` takes the last value, and `-r` reverses only an explicit sort. `-1` sorts and filters before selecting one row, then computes the repeat and width facts from that row. Command and Control clients receive the selected row on stdout; an attached Interactive client receives a frozen status message for the effective `display-time` without a command-output overlay. `-F` sees `notes_only`, `key_prefix`, and the per-row note/command plus post-filter repeat and width facts. `-n` is not a tmux `list-keys` flag. |
| `list-commands` | `lscm` | Print the 104 shared command specs in canonical-name order with tmux's `name (alias) usage` line shape: 83 executable tmux verbs and 21 zz-native verbs. Each usage string lists the flags zz accepts, including daemon-parsed commands. `-F` formats rows and an optional command limits the result. The list excludes tmux commands zz does not implement. |
| `set-option` / `set-window-option` | `set` / `setw` | Set typed options (see below) or exact free-form `@name` strings. The option name always expands as a format; `-F` additionally expands the value. `set` uses pane context and `setw` resolves a window target with its active pane for expansion. User options support set, append, and unset at server, global-session, session, global-window, window, and pane scope. Indexed scalars return tmux's `not an array`; table-known arrays parse and take the documented empty-success omission path. |
| `show-options` | `show` | List options stored at the resolved scope or print one named option. `-v` prints only raw values, `-A` includes inherited values with `*` after the name, `-q` suppresses unknown-name and target errors, and `-s`/`-g`/`-w`/`-p` select scope for `@` names. All 180 named options store and read back; arrays support numeric and named indices. String values use the pin's `args_escape` byte shape. On a no-name listing, `-H` retains the ordinary and `@` rows and adds hooks after the ordinary option table: none at server scope, 57 global-session hooks, 11 global-window hooks, or 7 pane hooks under `-p -A`. Empty arrays disappear under `-v`; an all-options listing prints `name` for a local empty and `name*` for an inherited empty. The pin's named-query path drops that star for an inherited empty. Populated inherited arrays carry `*` on each indexed row, and one local array shadows its parent as a unit. A named hook query works with or without `-H`. Plain listings omit hooks. Explicit-name queries still reach zz-native settings. |
| `show-window-options` | `showw` | Window-scoped spelling of `show-options` with the pin's `-g`, `-t`, and `-v` surface; `-H` is rejected. A table-known option still routes by its declared scope, so spelling this command cannot turn a session option into a window option. |
| `set-environment` | `setenv` | Store a global or per-session environment overlay. `-F` expands the value as a format, `-h` hides it from children, `-r` records a child-unset marker, and `-u` deletes the stored entry. New terminal PTYs apply global then session entries over the daemon environment. |
| `show-environment` | `showenv` | List or read the exact global/per-session overlay. The daemon seeds the global map from its boot environment. Fresh `new-session`, existing attach, and session-switch paths copy exact and wildcard-selected names from the relevant client's bounded snapshot, write unset markers for selected missing names, preserve empty values, and turn selected hidden entries into ordinary values. Creation or attach `-E` suppresses that refresh. Explicit `new-session -e` entries apply after a fresh seed. Normal output is `NAME=value` or `-NAME`; `-s` emits shell-ready export/unset statements with tmux's escaping, `-h` selects hidden entries, and an absent exact name errors `unknown variable: NAME`. A hidden exact name without `-h` succeeds with empty output. |
| `source-file` | `source` | Load config files: `-F` expands every declared path in the command's resolved pane context before globbing, each path's matches load in glob order, and declared paths remain in caller order. `-t` resolves that context once and follows tmux's quiet `CMD_FIND_CANFAIL` path on a miss, so the file still parses with no target while retaining the invoking client's source cwd. `-n` constructs each file with no environment or command effects; syntax diagnostics and `-v` output still surface, and expansion uses the pre-file environment. One invocation constructs all top-level matches as independent file units before replay. Without `-n`, a bare assignment affects later-file conditionals and persists, including before a later construction failure, while a replayed `set-environment` runs too late to affect an already constructed branch. Each file expands aliases and validates command names, flags, arity, callback arguments, and nested children before any command from that file runs. `-v` formats `path:line: command` groups plus successful alias-subparse traces in declared-path, glob, and physical-line order and carries into nested sources; Control suppresses it. Command clients receive one stdout transcript, and Interactive clients open one command-output view. Each invocation emits its complete verbose batch, then replay output, then buffered command-name and parser diagnostics. Source no-match, glob, and actual OS or path read failures retain their existing error channels. Non-UTF-8 config content is read as bytes through `parse_config_buffer_bytes` and `parse_config_file_bytes`; `config.non-utf8-file-bytes` closed on 2026-09-01. Nested invocations insert the same frame at their parent replay position, so the transcript is depth-first. This is per-invocation batching, not a claim of physical verbose and replay interleaving. Valid successful replay and `-v` output produce no duplicate Info or Warning event; parser diagnostics may still publish their existing Warning summary. Protocol v79 closes the TUI keyboard-navigation contract for that view: live copy tables, line and page movement, search, selection, paste-buffer copy, and stock vi/emacs exits. The local attached proof does not claim mouse behavior, OS clipboard delivery, ordinary TUI pane copy search, SSH transport, or presentation pixels. Unix matching uses tmux's `glob(3)` contract: backslash escaping, leading-dot exclusion, nonrecursive repeated stars, and ordinary no-match handling for malformed patterns. Without `-q` a top-level or nested path that matches nothing warns with its post-`-F` declared argument; `-q` keeps a no-match silent and later paths still run. Nested glob errors retain that declared path. Command clients receive stderr and exit 1, and Interactive clients receive a warning. A direct all-miss Control invocation ends its own frame with `%error` and stops that input line; if at least one direct path matched, its missing-path diagnostics remain inside `%end` and the line continues. During parser-owned replay, protocol v76 emits one tail-tag-47 `SourcedCommandGuard` for each command that survives command-name resolution. Aliases resolved to `source-file` before replay retain this path. Unknown or ambiguous command names and malformed alias names publish a located Warning that Control renders as `%config-error`, without a guard. An unknown command in an indirectly sourced child keeps the parent and source success guards, then publishes that Warning without another command guard. An ordinary success or quiet all-miss gets an empty flags-1 `%end`; a mixed source hit and miss keeps the declared-path diagnostic inside `%end`; and an all-miss, flag or arity failure, runtime failure, or depth refusal ends `%error`. The guard's `client_failure` bit sets Control retval 1 independently of the terminator without making parse or source-command diagnostics sticky. The Control writer defers these guards FIFO until the direct outer frame closes. Synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C` now retain flags-1 framing during parser-owned replay. Control publishes the containing command before each inserted command, and an inserted source before its children. Per-client and per-thread capture prevents folding, cross-thread interception, and leakage into the next input command. An unsupported zz-only inserted command gets an empty success guard and later siblings continue, but it does not join `ConfigLoadReport`'s skipped summary. Hook commands and shell-evaluated `if-shell -b` or `run-shell -bC` callbacks use flags 0; background callbacks retain their exact Control recipient through callback entry, while ordinary `run-shell -b` output remains under `control-mode.async-command-output`. The synchronous path reuses protocol v76 without a wire change. The loader preflights every declared path for one source command before recursion, so a three-level replay emits the root missing-path guard, the middle missing-path guard, and the leaf output guard in that order, each exactly once. The focused regression and then-six-step Control differential close that ordering without a production change. Protocol v78 appends a typed source-file event at tag 48. A matched parser or hook-source OS or path read failure closes its source guard with `%end`; `ReadError` then prints the platform diagnostic as raw unframed text and retains status 1. `Complete` renders nothing and consumes one hidden command number after that invocation's descendants. Each invocation that passes depth checking consumes one completion number, including an empty file, loud or quiet miss, matched parser error, and `source-file -`; a depth refusal or dispatch-time syntax, arity, or flag rejection consumes none. Multiple matched read failures print before replay and share the invocation's one completion. A pinned config containing only byte `0xff` instead succeeds without a visible diagnostic and consumes an additional invisible empty-command item, and zz matches that: the byte loaders apply the pin's signed-char reading rather than rejecting the file as invalid UTF-8. No-match, glob, and located depth diagnostics stay inside the source command's guard; neither path needs prose classification. Config summaries and lexer-owned diagnostics remain Warning events behind `control-mode.diagnostic-typing`; a known-prefix Warning fallback supports older daemons. Matched parser diagnostics remain `%config-error` and continue. A construction failure emits one located `%config-error` without a failed-command guard, and top-level sibling warnings wait until the batch finishes replay. `source-file` does not expand tildes again after parsing: parser-expanded leading tildes arrive as absolute paths, while literal tildes follow normal relative-path resolution. A registered client's top-level source snapshots its selected base and carries it through recursive replay, including after an ordinary sourced command clears the mutable context cwd and when the active default `zz/mux.conf` loads through the ordinary source path. Direct zz-native `reload-config` uses the same stable base through its separate reset path for registered clients; slice 10ag closes cold startup selection: an auto-spawning launcher passes a bounded UTF-8 cwd through a private daemon argument, startup carries it through nested replay, and the daemon clears it before runtime source commands. Attached clients select the invoking client's retained session cwd; `source-file -t` remains only the format and replay target. `source-file.event-hook-client-cwd` and Control-only `source-file.sourced-hook-client-cwd` own the remaining hook paths. Command replay already retains the caller cwd for sourced hooks. Counting the initial `source-file` as invocation 1, both sides run 50 concurrent source invocations and refuse invocation 51 with `too many nested files` before any of its paths are matched or loaded, so `-q` does not suppress it, a refused command emits one diagnostic rather than one per path, and the containing file keeps running its later physical lines. The refusal appears inside the rejected nested command's own flags-1 `%begin`/`%error` guard and consumes no completion number. A malformed invocation at the refused depth is diagnosed as malformed rather than as depth on both sides, because the pin rejects it while parsing the containing file and never consults its depth guard; the shared arity and flag parsers now match the malformed text, but the pin then abandons the rest of the containing file where zz continues it. The refused source drops its later same-line siblings, while a matched parent source continues its own group after child runtime, parser, or read failures; zz retains matched child read failures in `ConfigLoadReport`. Replayed runtime failures use the invoking client's error channel and nonzero status while later physical lines continue. The Control front end now retains direct runtime errors, parser-owned sourced runtime failures, synchronous inserted runtime failures, nonruntime source failures, and v78 source-read errors as retval 1. A blank line or EOF returns the value captured when it entered the queue; generic nonzero successes and flags-1 parse or preparation failures do not set it. Only a caller-targeted `Detached` event exits 0, so nonself and no-victim detach forms stay alive and preserve a queued Return. The response `%end` precedes `%exit`. Control output from parser-owned successful replay commands stays inside their sourced guards. Command stdout and the attached view now receive the per-invocation verbose, replay, and buffered command-name or parser diagnostic transcript once. Successful output stays off error channels and does not change status; runtime errors keep stdout around the failure while preserving stderr and status 1. Runtime `source-file` treats the active default `zz/mux.conf` as an ordinary matched path. One invocation constructs every matched file as an independent unit in declared-path and glob order, then replays valid units in the same order, so default, after, and default applies as `DAD`; a loud miss returns status 1 without blocking later matches; and ordinary diagnostics plus `-v` lines retain that order. Explicit native `reload-config` still rediscovers the current default, resets key tables and appearance, and reapplies stored overrides. Startup first-existing discovery, ordered explicit `-f`, parse-only, and nested paths use the same file-unit construction boundary. Startup uses one 50-command source budget across every root, with top-level roots excluded, quiet misses counted, and one command with many paths counted once. The daemon retains `<file>:<line>` in each refused startup cause, while `config.startup-diagnostic-delivery` tracks delivery and placement on Control and attached clients. A separate manual probe of pinned tmux `d77c9dc6`, outside the 12-step runtime scenario, found that startup `display-message -p` text becomes a config cause while list-style output is discarded; the detached launch stays silent. `-` (stdin) is refused loudly because the daemon has no caller stdin; only its invisible completion numbering agrees. |
| `reload-config` | . | *zz-native:* reload tmux + Ghostty config (`ReloadConfig` effect, no args). A registered caller's selected source base remains stable through relative nested sources in the default mux config. |
| `start-server` | `start` | Ensure the daemon is running, then return success with no output. The CLI's normal connection path starts a missing daemon before the no-op reaches the engine. |
| `kill-server` | . | Stop the daemon (`KillServer` effect). |

## Control source, hook, and shell-output framing in protocols v77, v78, v80, and v81

The `source-file` row above preserves the v76 parser-replay checkpoint. Protocol v77 renames its
tail-tag-47 event in place to `ControlCommandGuard { output, error, sticky_failure, flags }` and closes
immediate command-hook framing. Parser replay and synchronous foreground inserted lists keep flags 1.
Immediate `after-*` and `command-error` hooks retain the originating Control recipient in a separate
target, clear parser replay state, and give every hook command, hook source, and sourced descendant an
independent flags-0 frame. Hook array entries remain ordered, one failure stops only its current
command list, hook output does not fold into the trigger, and unknown sourced names produce only
`%config-error`. A mixed source miss and hit may end `%end` while `sticky_failure` retains status 1.

Shell-evaluated `if-shell -b` and `run-shell -bC` retain their originating Control client through
callback entry and give every inserted command and sourced descendant its own flags-0 frame. A hard
disconnect after an immediate hook or source queue starts remains under
`control-mode.disconnect-cancels-command-queue`. Protocol v78 carries matched parser and hook-source
read failures as typed `ReadError` events that render raw after their source guards. Its invisible
`Complete` event consumes the source callback's command number after descendants.

`control-mode.kill-server-response-order` owns the current shutdown race. `KillServer` can request
daemon shutdown before the client handler admits its `CommandResponse`; shutdown may close the
Control mailbox first, leaving the command guard to report an unexpected exit. Slice 10ah must admit
and drain the successful Command or Control response before `ServerStopping` and connection teardown,
with bounded behavior for stalled or disconnected clients.

Protocol v80 supersedes the startup-delivery checkpoint preserved in the `source-file` row. Startup
reads and parses all roots before replay, retains normalized root and nested read errors, parser
diagnostics, unsupported and runtime failures, and successful `display-message -p` output, then
replays roots in order with nested depth-first traversal. Startup discards list-style output, and a
successful physical multiline command uses its completion line. A detached Command launch stays
silent and cannot drain the causes. The first eligible Control client receives the raw bounded
vector once; an attached Interactive winner opens a PTY-free `configuration errors` view with an
ordered, UTF-8-safe 64 KiB preview that replaces every Unicode control except LF and TAB. Its
truncation notice points to Control mode because exact Interactive recovery of the retained 1 MiB
vector is not promised.

Slice 10ag proves the cold startup source base independently of Control exit. Its full eight-case
diagnostic exposes `control-mode.exit-pane-output`: after EOF or a blank Return, zz can render queued
shell-prompt `%output %0` rows between a flags-0 `%end` and `%exit`, while pinned tmux removes queued
pane-byte blocks and rejects later pane bytes. Slice 10ai owns that discard boundary. It excludes
hard-disconnect queue cancellation, command-output events, retained status, and pause or wait flow.

Protocol v81 appends `ControlCommandOutput` at tail tag 50 and supersedes the open
`control-mode.async-command-output` pointer in the historical `source-file` row above. A targetless
or invalid-target foreground `run-shell` publishes raw output and its exit diagnostic to the exact
originating Control client after the command's empty flags-1 guard closes. Direct input continues in
its next guard; sourced same-line siblings receive their own later guard. Embedded LF and
percent-prefixed lines stay literal, the writer supplies a missing trailing LF, and this event does
not change Control retval. Foreground `run-shell -C` retains synchronous framed output. A live
resolved `-t` and ordinary `run-shell -b` open zz's native per-Interactive command-output view for
attached pane viewers, with no raw Control text or `%pane-mode-changed` notification.

## TUI command-output navigation in protocol v79

An Interactive command transcript opens one daemon-owned view terminal. The daemon switches that
client's live key engine to the output pane's effective `copy-mode` or `copy-mode-vi` table and
retargets it when `mode-keys` or a custom binding changes. The TUI forwards press and repeat keys;
it drops releases. Movement, search-repeat, selection, rectangle, copy, and cancel commands keep
using the shared command catalog and `send-keys -X` execution path. Stock emacs `q` and Escape
cancel. Stock vi `q` cancels, while Escape clears the selection and keeps the view open.

`TerminalUiCommand::BeginSearch` opens the TUI-local command-output editor. Text edits and bounded
paste updates send `SearchUpdate`, Enter submits the current live query by leaving edit mode, Escape sends
`SearchClose`, and the active copy table supplies `n` and `N`. Copying the retained selection creates
the daemon paste buffer. This contract does not claim an OS clipboard write.

Protocol v79 keeps `EventPayload::CommandOutput` at its existing event tag and adds `output_id`.
Real frames and closes carry a nonzero actor ID; ID zero with no viewport means authoritative
no-output resync. The daemon allocates IDs monotonically for the connection, and `ClientCore` keeps
a watermark so an old coalesced frame or close cannot replace a newer output. The TUI keys its local
search, swallowed-key, and resize-cache state to the actor ID. One full-width content rectangle
drives both the painted rows and `ResizeCommandOutput`, reserving the output header, footer or
message row, and configured status block. The terminal actor and renderer use the same geometry.

The local attached fixture drives 96 output lines through line movement, page movement, search,
`n`/`N`, selection-to-paste-buffer, live custom bindings, and both mode-key tables on zz and the
pinned tmux. Mouse behavior, ordinary TUI pane copy-search editing, the one unsupported window-copy
action (`scroll-to-mouse`), SSH transport, pixel comparison, and canonical-summary freshness remain
outside that proof.

Options handled by `set-option`/`set-window-option`: `synchronize-panes` (global→window→pane scope,
`-g/-w/-p/-u/-U/-o`), `buffer-limit` (global, default 50), `message-limit` (server, default 1000),
`history-limit` (session, default 10000, 0–1,000,000), `word-separators` (session, `-a` append), `mode-keys` (`vi`→`copy-mode-vi`,
`emacs`→`copy-mode`), `prefix`, `set-clipboard` (`on`/`external`/`off`), `copy-command`, `status`,
`status-interval`, `status-left`, `status-right`, `base-index`, `pane-base-index`, and
`renumber-windows`; `mouse` (session flag, default `on`), `escape-time` (server milliseconds,
default `10`), `automatic-rename` (window flag, default `on`), `automatic-rename-format` (window
string), `remain-on-exit` (window/pane choice: `off`, `on`, `failed`, or `key`),
`default-terminal` (server string, default `tmux-256color`), `display-time` (session milliseconds,
default `750`), `repeat-time` (session milliseconds, default `500`, maximum `2000000`), and
`aggressive-resize` (global-window/window flag, default `off`). The
matcher checks exact names and unique prefixes against all 180 tmux option names plus
68 hook entries. The matched table entry chooses server, session, window, or pane scope. `set` versus
`setw` and the `-s`/`-w`/`-p` spelling cannot change that declared scope. A table entry declared as
both window and pane lets `-p` select pane scope. `-q` silences unknown or ambiguous names; config
import reports a known unimplemented name as a skip. Names beginning with `@` bypass table matching:
they are exact string keys whose scope comes from the command flags, preserving the plugin-storage
contract without pretending the stored value has behavior.

`automatic-rename` gates runtime-driven window names. Explicit window names pin a window-local
`off`; `automatic-rename-format` expands against the active pane when its runtime command changes.
Retained terminal exits keep the last viewport and expose `pane_dead`, normal-exit
`pane_dead_status`, `pane_dead_time`, and terminating `pane_dead_signal`; input is swallowed, `kill-pane` still removes the
pane, and the respawn commands replace its daemon-owned terminal session. An explicit
`default-terminal` seeds `TERM` for future spawns, with a per-spawn environment override winning;
the unset path restores `tmux-256color`.
`repeat-time` supplies the attached session's repeatable-binding deadline, including zero to disable
the repeat window. `aggressive-resize` ON restricts geometry candidates to clients viewing the
window; OFF uses the ordinary viewer set. `window-size latest` takes the geometry owner's rows,
columns, and cell metrics. Largest and smallest aggregate rows and columns across candidates, while
manual retains its stored extent; all three still take cell metrics from the owner. Terminal input
and enabled client FocusIn advance ownership. Client focus, attachment, and option changes
recompute affected windows through the normal guarded measurement write-back. `mouse` and
`escape-time` are published and consumed by the TUI; the daemon also gates terminal-surface mouse
input when the option is off.

For the index trio, `-u` and `-U` restore inheritance and ignore a trailing value, `-o` checks the
target override slot and yields to either unset flag, and the handler accepts `-a`. tmux flag values accept
`on`/`off`, `yes`/`no`, and `1`/`0`; `true` and `false` remain valid for zz-native boolean settings.
The six zz-native agent, editor, and history-trickle options keep their command and flag checks.
Buffer commands (`capture-pane`, `*-buffer`, `paste-buffer`) are handled by
[the server](/crates/zz-daemon.md), **not** here. `list-buffers` supports `-f`, `-O`,
and `-r` with newest-first natural/default creation order; unlike the other list commands,
it does not install a `#{line}` format variable. `load-buffer` and `save-buffer` expand their path
once in the selected client context before `~/` handling and file access. The expansion receives the
canonical `#{command}` value after aliases and unique prefixes, while explicit item state wins.
`load-buffer -t` uses the supported attached-client selector for session, focused-window, and active-pane facts; a
missing client stays quiet and falls back to the most-recent mux context. `-w` writes the stored
buffer to the selected client's clipboard through the same selector, quietly doing nothing when the
target does not resolve or advertises no clipboard terminal feature; like the pin it bypasses
`set-clipboard` and raises no `pane-set-clipboard` hook. `set-buffer -n`
renames the explicit `-b` source or the newest automatic buffer, replaces an existing destination,
and ignores its optional data argument on the rename path.
Daemon-preempted format arguments still receive the canonical `#{command}` item name: this includes
capture boundaries, run/if shell inputs, pipe commands, list-client/list-buffer formats and filters,
popup/menu presentation values, confirm string-command preparation, and the post-spawn
`new-window`/`split-window -P -F` pass that adds live pane facts. Command blocks keep their child
identity, and popup argv/environment values remain raw.

Slice 10aa gives `ExecutionContext` two format-client views. `format_client()` derives the raw
invoking client as no client, unattached, or attached to one session. `target_format_client()`
overlays the current or explicitly selected target client for producers that tmux expands against
that client. The daemon resolves the target without replacing the raw view, so one command can use
the invoker for name or cwd expansion and the selected client for `session_active`.
`PaneFormatOutput` stores that selected state for deferred expansion. Clientless lists and filters,
chooser rows, and `list-commands` keep no client; target-aware command formats, shell callbacks,
buffer and capture paths, popup and menu text, `list-keys`, status rows, Control subscriptions, and
display-panes labels receive the selected client. This split adds no protocol or snapshot field.
Unit, source-file, `run-shell`, `if-shell`, per-client snapshot, and attached-client fixture proofs
show that `client_*` facts and `session_active` use the same selected client.

# Command aliases

The mux resolves one exact `command-alias[]` layer before canonical lookup. It parses the complete
matched body, appends caller arguments only to its final command, and freezes the result without
recursing into another user alias. A one-command body remains one invocation. The mux wraps a
multi-command body in one opaque typed group whose children retain option boundaries, empty
arguments, nested typed blocks, and physical source groups. It treats an empty body as a successful
no-op, including when the caller supplied arguments. For an unparsable matched body, the mux reports
`unknown command: <typed name>` instead of falling through to the canonical or catalog alias it
shadows.

Construction, not execution, owns user-alias observation. Stored `bind-key` and `set-hook` command
lists execute their constructed commands without another user-alias lookup. Read-only clients
authorize the same frozen stored chain before any effect; writable and read-only execution differ
in authorization, not alias timing. Typed `if-shell`, `run-shell`, and `confirm-before` callbacks
remain frozen after lexical construction. Typed `if-shell` and `run-shell` callbacks preserve
physical groups: a failed group stops its remaining commands while later physical lines continue;
string callbacks remain one group. Typed `command-prompt` templates retain their structured prepared
command list through submission without another user-alias lookup. Structured substitution preserves
leaf-argument boundaries. String templates substitute raw source before a fresh parse and complete
construction pass against the current alias table. Both paths replace the first `%%` and every `%1`,
with trailing-percent quoting. Typed callbacks retain physical groups, while string templates and
free input form one group. Prompt chains and multi-answer `%2` remain under their prompt owner.
`set-hook` and command-valued native set-option apply their documented second construction stage
before storage or normalization. Built-in hook values flatten physical groups during that stage,
while custom `@` values retain their normalized textual groups for deferred execution. A typed
ignored `set-hook -R` value still constructs and can fail before the stored hook runs. A typed
`display-menu` action keeps its wrapper through stored binding readback; the daemon drops it before
the fresh selection parse, while a quoted brace string remains literal. Each stage resolves at most
one user-alias layer; an alias-produced subtree cannot open another layer. Generic alias recursion
and selected-action runtime errors retain their existing owners.

Protocol v74 gives Control the same daemon-owned boundary: `PrepareCommandList` resolves one
complete initial argv unit or LF line under one lock, and the client executes the immutable result
with `CommandRequest.prepared = true`. That bit skips a second user-alias lookup. The daemon still
performs ordinary read-only authorization, and it accepts no client-supplied canonical identity as
authority. Exact attach routing, client-side
stdin capture, and `kill-server` recovery now consume that identity for local CLI endpoints. The
same immutable vector crosses a TUI reconnect. Before preprocessing or execution, the local CLI
scans the whole vector for typed preparation errors, so a later invalid command cannot follow an
earlier effect. A warm endpoint covers typed name and alias-body failures; the cold static pass above
adds whole-vector upstream flag, arity, and callback validation across autospawn. Slice 10u closes
warm unaliased argument groups. Slice 10y closes config and source replay alias snapshots, and slice
10z closes file-unit construction under `mux.chain-parse-abort`.
Per-command diagnostics match the pin at dispatch. Runtime command failures retain sequential tmux
queue ordering. Raw `--kill-server`
stays unaliasable. Remote
`--host` routing remains under `aliases.remote-client-preflight` because classification must not
start SSH. Command alias shadowing is not an authorization control: every prepared command still
passes the daemon's normal read-only check.

The daemon executes each opaque-group child through the ordinary mux and daemon paths. One shared
queue boundary encloses the group. Read-only authorization validates the complete frozen chain
before its first effect. Client-owned stdin attaches only to the final child. Child failures,
physical source groups, foreground and detached shell work, nested source yields, structural hooks,
and deferred shutdown keep their queue ordering; a source yield stays local to its owning queue and
does not replay an already-fired after-hook. The daemon holds structural event hooks until the alias
children finish. During forced shutdown, it drains admitted work, suppresses event hooks that can no
longer run, and then stops. `hooks.shutdown-window-unlinked-order` tracks the remaining exact
multi-window hook-order difference because tmux derives that order from winlink RB-tree history that
zz does not retain.

Control uses the opaque group as transport structure and emits no synthetic parent `%begin`/`%end`
frame. Each real child emits its normal guard with the originating flags, and an empty alias emits
no guard. Local CLI routing scans every child for TUI handoff but only the final child for stdin;
stored binding display renders all children. Both preserve the prepared group for execution.

# Daemon-side workspace verbs

Seven zz-native verbs are handled by [the daemon](/crates/zz-daemon.md) before the engine sees them,
for the same reason `capture-pane` is: each acts on something `MuxState` does not own. They now have
shared `catalog.rs` specs, so `list-commands`, stored-command validation and rendering, and
[command-palette](/concepts/command-palette.md) completion discover them like the other 14 native
verbs. Exact native names resolve before abbreviation lookup. A native abbreviation resolves only
when no tmux canonical name starts with it, which keeps `capture-b` native while `capture` resolves
to tmux's `capture-pane`. Execution remains daemon-owned.

| Command | Purpose |
| --- | --- |
| `tools` | Print the agent-readable catalog of workspace verbs. Pure output; the self-teaching entry point for an agent running in a pane. |
| `agent-send` | `[-t %N] [--submit \| --wait [--timeout SECS]] [--context PATH[:START[-END]]] [TEXT]` . append text to a GUI-owned Agent composer, submit it as a prompt (daemon-side, no GUI needed; prints the pane it chose), or with `--wait` submit and block until that turn ends, printing the reply. A non-agent or omitted target routes to that window's most recently focused Agent pane. Reads stdin when TEXT is omitted; capped at 1 MiB. See [Agent pane](/concepts/agent-pane.md). |
| `send-last-output` | `-t %N` . route a terminal pane's last completed command and output (OSC 133 marks) into the window's most recently focused Agent pane. Bound to `<prefix> e`. |
| `show-last-output` | `-t %N` . the read twin: print that same fenced `%N $ command` block to the caller instead of routing it, so a script or an agent reads a terminal's last result without a capture-and-regex dance. Same OSC 133 requirement and 200-line / 256 KiB cap. Accepts an Agent pane too: its transcript projection frames every turn with OSC 133 marks, so the block is the last prompt and reply. |
| `send-text` | `-t %N [--no-enter] [--timeout MS] TEXT` . deliver TEXT to a TUI in a terminal pane the way `send-keys -l … Enter` cannot: paste it (bracketed iff the app enabled DECSET 2004 — the actor decides), poll `capture` until the text's tail, or a `[Pasted text` collapse marker, is on screen, then press Enter. No echo within `--timeout` (default 2000 ms) is a non-zero exit with nothing submitted. Honors `pane_input_off` and `synchronize-panes` like `paste-buffer`. |
| `capture-browser` | `-t %N -o /abs/path.png` . write a browser pane's latest rendered frame to a PNG. The path must be absolute because the GUI process writes it. |
| `debug-marker` | `[NOTE]` . stamp a `user_marker` line into the daemon's log so the moment an incident was noticed is findable later. The GUI's `DebugMark` key (`cmd-shift-m`/`ctrl-shift-m`) forwards here after stamping the app's own log. |

Agent panes answer the read verbs like terminals: each owns a PTY-free shadow terminal fed with a
projection of its transcript, so `capture-pane`, `show-last-output`, `pipe-pane`, and the
activity/bell alerts work on `%agent` with no agent-specific grammar (see
[the projection design](/designs/agent-pane-projection.md)). Input verbs still refuse them.

The composer form of `agent-send` and `capture-browser` are **round trips**: the daemon publishes
the request to the attached GUI and parks the calling command thread on
`ProtocolMessage::GuiResponse` (5 s timeout), because only the GUI owns the composer draft and only
the GUI has the CEF frame. The GUI answers from its mux observation rather than its render loop, so
a minimized window still replies. `--submit` and `--wait` never touch the GUI: the daemon's own
agent runtime takes the prompt, and `--wait` parks the command thread on the turn's reply instead
(600 s default, no lock held). `MuxState::recent_agent_pane` picks the recipient
for `send-last-output` and for any `agent-send` whose target is not itself an Agent pane, with the
same active → focus-history → layout-order rule as `cwd_donor`.

One superset event rides an existing verb: every user-option write or unset (`set-option -p/-w/-s/-g
@name …`) signals the `wait-for` channel `<name>@<target>` — `@agent_state@%5`, `@fleet@$1`,
`@x@global-session`, `@x@server` — with `wait-for -S` semantics, so a signal nobody is waiting on
parks as sticky. A foreign agent CLI's lifecycle hook can stamp
`zz set-option -p -t $TMUX_PANE @agent_state idle` and an orchestrator blocks on
`zz wait-for '@agent_state@%5'` then reads `show-options -p -t %5 -v @agent_state`; the sticky flag
makes that read-then-wait loop race-free. tmux never signals on option writes, but no tmux config
waits on such a channel, so the differential harness cannot observe the divergence. The push form
is the `@option-changed` user hook: `set-hook -g @option-changed 'run-shell "…#{hook_option} #{hook_target}…"'`
runs on every user-option write with those two variables in scope; commands already running from a
hook do not fire it again, so a hook that writes an option cannot recurse.

# Effects

`MuxEffect` variants returned to the daemon adapter include: `PaneCreated`, `PaneRespawned`, `PanesRemoved`,
`PaneRelocated`, `SendKeys`, `TerminalView` (scroll/copy-mode), `TerminalUi` (search prompt),
`CommandPrompt`, `FocusSidebar`, `ChooseTree`, `ChooseBuffer`, `DisplayMessage`, `DisplayPanes`, `BufferLimitChanged`,
`WordSeparatorsChanged`, `ModeKeysChanged`, `Attach`, `Detach(DetachRequest)` (an optional client
selector plus `Client`, `Others`, or unresolved `Session` scope for daemon resolution),
`SourceFile { path, quiet, parse_only, verbose, context }`,
`ReloadConfig`, `KillServer`, and `SnapshotChanged` (appended whenever the mux generation advanced).
Terminal split effects set `PaneCreated::inherit_cwd_from` to the resolved target pane; initial
session/window panes and browser panes leave it empty.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/command.rs` | `MuxEngine::execute`, per-command handlers, option parsing/setters, `MuxEffect`, and `copy_mode_action`. |
| `crates/zz-protocol/src/catalog.rs` | `canonical_command` plus shared canonical names, aliases, descriptions, options, and completion value kinds. |
| `crates/zz-mux/src/model.rs` | `MuxState` mutations and `resolve_session/window/pane` target resolution. |

# Related

- Mutates the [split-pane layout](/concepts/split-pane-layout.md) in [MuxState](/crates/zz-mux.md);
  `-X` actions drive [copy mode](/tmux/copy-mode.md); `choose-*` drive [choosers](/tmux/choose-tree.md).
- Bindings that invoke these come from the [key tables](/tmux/key-tables.md); text comes from the
  [conf parser](/tmux/conf-parser.md). Effects consumed by the [server daemon](/crates/zz-daemon.md).
- Names/aliases checked against the [tmux upstream reference](/references/tmux-upstream.md); see
  [tmux compatibility](/tmux/tmux-compat.md).
