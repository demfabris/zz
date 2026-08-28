---
type: Subsystem
title: tmux-grammar config parser (parser.rs)
description: A single-pass tmux-style tokenizer plus the daemon replay layer that keeps stored zz/config mux overrides above the sourced zz/mux.conf configuration.
resource: crates/zz-mux/src/parser.rs
tags: [tmux, parser, config, tokenizer, mux-conf]
timestamp: 2026-08-26T00:00:00-03:00
last_updated: 2026-08-28
last_updated_by: Codex
---

# Overview

`parser.rs` implements `parse_config(source, input) -> ParsedConfig`, the lexer used for startup
config, `source-file`, and each `command-prompt` submission. By default the daemon reads the first
existing zz-owned platform candidate for `zz/mux.conf`: XDG config, the home config directory,
macOS Application Support, or Windows AppData in platform order. One or more top-level startup `-f`
files replace that default for the initial load. `#{config_files}` retains that ordered startup
selection until `reload-config` returns to the first existing platform candidate; reload then
replaces the fact with that selected path, or empties it when no candidate exists. Later
`source-file` calls do not append to the fact. The daemon does not read `~/.tmux.conf`; the client's
import flow copies a user's tmux config to the first existing or first constructible candidate. See
[Application configuration](/configuration/app-config.md).

The lexer is a single character-by-character state machine (modeled on tmux's `cmd-parse.y` /
`arguments.c`) that splits input into words, groups words into commands, and records a
[`SourceSpan`](/crates/zz-protocol.md) (`source`, `line`, `column`) for each command so diagnostics and
`list-keys` output can point back at the origin. It produces `CommandInvocation`s only. It does
**not** validate command names or arguments; that happens later in
[`MuxEngine::execute`](/tmux/commands.md).
Protocol v84 also retains every standalone, balanced, unquoted `{ ... }` argument as a zero-based
position on `CommandInvocation`. The brace-bearing text stays in `args`; the position says it was a
typed command block. Single- or double-quoted brace text remains an ordinary string, as do braces
inside another word. Control mode uses the same lexer before transport. Invocations built directly
from argv strings have no typed positions because shell quoting has already been erased.
Parser diagnostics keep the location where the invalid statement began. A successfully parsed
physical multiline command instead records the line where that command completes, matching tmux's
startup cause location.

`parse_config` never panics, but since the 2026-08-19 grammar wave it follows the pin's
whole-file abort: the FIRST diagnostic stops the scan and drops every command in the file
(`commands` comes back empty), while environment assignments already reduced before the
error point survive — tmux's `environ_put` runs during parse and is never rolled back
(`cmd-parse.y:221-244`, `cfg.c:123-128`). The full entry point is
`parse_config_with(source, input, ctx)` where the context supplies `variable(name)`
lookups for `$VAR` expansion and a `condition(format)` evaluator for `%if`; the daemon
routes through `MuxEngine::parse_config` (engine-backed globals, hidden entries visible,
`#()` in conditions renders empty — never spawns), and control mode uses
`MuxEngine::parse_config_without_variable_expansion` so `$VAR` stays literal there (an
accepted divergence; the pin expands control-mode input server-side).

## Daemon replay and zz/config overrides

The parser itself remains unaware of `zz/config`. The app sends its daemon-owned entries as ordered
raw pairs in `SetConfigOverrides`; `daemon.rs` partitions the ten mux keys and stores that subset.
Each valid entry is dispatched as `set-option -g -- KEY VALUE` through `MuxEngine::execute`, retaining
the option grammar and normal effects (`ModeKeysChanged`, `WordSeparatorsChanged`,
`BufferLimitChanged`, and snapshot publication) in the command path. Invalid values log a diagnostic
and are skipped while later entries continue.

Replay resolution is `built-in default < zz/mux.conf < zz/config override`. `load_config_file`
labels successful option writes as `tmux-config` (the wire tier kept its name; it now means "from
the sourced mux config file"); after the complete startup, `reload-config`, or
interactive `source-file` replay, the daemon reapplies its stored mux overrides and labels them
`override`. Interactive `set-option` calls are `runtime-command`, untouched values remain `default`,
and the last successful writer is what `ServerHello` and `MuxOptionsChanged` report. Thus a
`zz/mux.conf` assignment such as `set -g prefix C-b` cannot revert a stored `prefix = C-a` when the
configuration is reloaded.

`source-file -F` expands every positional path in the command's current pane context before the
daemon resolves relative paths and globs. Paths remain in declared order and matches within each
path remain in glob order. On Unix, matching uses `glob(3)` with flags zero, like tmux: backslash
quotes the next character, a wildcard does not include a leading dot, repeated stars are ordinary
wildcards rather than recursive traversal, and an unmatched bracket can be a literal. A pattern
that finds nothing follows the ordinary missing-file path, including malformed forms that libc
treats as no match. A `-q` miss is silent without stopping later paths. Nested loud no-match and
glob errors retain the post-`-F` declared argument; a nested `-q` no-match stays silent. For a
registered client, the daemon snapshots the top-level source base before replay and passes it through
each recursive load. An ordinary sourced command still executes through `ClientId(u64::MAX)` and
clears the mutable `ExecutionContext` cwd, but the next nested source continues to use the invoking
client's stable base. Runtime `source-file` treats the active default `zz/mux.conf` as an ordinary
matched path and forwards that same base into nested replay. A direct `reload-config` from a
registered client snapshots and forwards the base through its separate native reset path. Startup
replay and sentinel-client reloads pass no source base, so startup remains under
`source-file.startup-client-cwd`. Deferred event hooks still use the sentinel client under
`source-file.event-hook-client-cwd`. A hook raised by an ordinary sourced command also starts from
the sentinel replay client, outside this recursion base; `source-file.sourced-hook-client-cwd`
tracks that path.
For an attached client, the daemon now prefers the invoking client's retained session cwd over the
process cwd in its hello. `attach-session -c` selects a compound target's window and pane, then
mutates that session cwd after pane-context format expansion. `source-file -t` remains only the
format and replay target. Command and Interactive
clients receive those diagnostics directly. Protocol v77 gives Control one
`ControlCommandGuard { output, error, sticky_failure, flags }` for each parser-owned replay command
that survives command-name resolution. An alias resolved to `source-file` before replay stays on
this path. Unknown or ambiguous command names and malformed alias names publish a located Warning
that Control renders as `%config-error`, without a guard. Ordinary success and a quiet all-miss
produce an empty flags-1 `%end` guard. A nested hit plus miss carries its declared-path diagnostic
inside `%end`; an all-miss, flag or arity failure, runtime failure, or depth refusal ends `%error`.
`sticky_failure` is separate from the terminator, so runtime failures set Control retval 1 while
parse and source-command diagnostics can remain nonsticky.

Synchronous inserted lists reached during parser-owned Control replay now retain the same flags-1
recipient. This covers foreground shell-evaluated `if-shell`, immediate `if-shell -F` including
`-bF`, and foreground `run-shell -C`. Control closes the direct outer frame, then publishes the
containing replay command before each inserted command. An inserted `source-file` guard precedes
its sourced child guards. Success output, diagnostics, terminators, and `sticky_failure` remain on
the command that produced them. Per-client and per-thread capture prevents nested frames from
folding into a parent, intercepting another thread's event, or leaking into the next input command.
The parser-owned path now uses the v77 event shape; its command frames remain flags 1.

An unsupported zz-only command in an inserted list receives an empty success guard and later
inserted siblings continue. It does not join `ConfigLoadReport`'s skipped summary, so existing
unimplemented command and semantic groups still own reporting parity. An unknown command in a child
file follows the pin: the parent and source guards succeed, then a `%config-error` Warning appears
without its own command guard.

Immediate `after-*` and `command-error` hooks retain the originating Control recipient but clear the
parser replay client and enter a no-hooks context. Every hook command, hook source, and sourced child
gets its own flags-0 frame. Hook array entries remain ordered. One failure stops only the current
command list, later entries continue, and output never folds into the triggering frame. Unknown or
ambiguous sourced names publish `%config-error` without a guard or `command-error`; aliases use one
frozen resolution for classification and execution. A mixed missing-and-matched hook source ends
`%end` but sets `sticky_failure`, retaining process status 1 while later hook commands continue.
Protocol v78 gives matched parser and hook-source OS or path read failures a typed
`ControlSourceFileEvent::ReadError`. The Control writer prints that text raw after the source guard,
without allocating a frame, and retains process status 1. Each source invocation that passes depth
checking also publishes one `Complete` after its descendants. The writer renders nothing for that
event and consumes one command number. Empty files, loud and quiet misses, matched parser errors,
nested and hook-owned sources, and `source-file -` all take that depth-admitted path. One or several
matched read failures print before replay and still share the invocation's single completion after
all descendants. The refused 51st invocation and dispatch-time syntax, arity, or flag errors consume
none.

Shell-evaluated `if-shell -b` and `run-shell -bC` run later with flags 0, retain the exact originating
Control recipient through callback entry, and cancel before any callback work when that origin is
gone. Hard disconnect after an immediate hook or source queue has already started stays under
`control-mode.disconnect-cancels-command-queue`. Protocol v81 supersedes the earlier
`control-mode.async-command-output` boundary: ordinary `run-shell -b` now opens zz's native
per-Interactive command-output view for attached viewers of the selected recent pane.

The Control writer defers flags-1 guards FIFO until the direct outer frame closes. The loader
preflights every declared path for one source command before recursion. A focused regression and
the then-six-step Control differential prove the root-miss guard, middle-miss guard, then
leaf-output guard order, each exactly once; no production change was required. This closes
`source-file.nested-control-queue` for ordering only. The later
`control-mode.source-file-exit-status` closure retains direct and parser-owned sourced runtime
failures plus nonruntime source failures as retval 1. A Return captured while a preceding non-detach
command waits keeps its arrival-time snapshot ahead of later queued stdin. A Return observed while
self-detach waits is discarded when the caller's `Detached` event arrives. Nonself and no-victim
detach commands keep the Control loop alive.

Matched child OS and path read failures follow their parent source guard through the v78 typed
source-read event, which Control renders as raw unframed text. Numeric OS errors and colon-space
paths use that path without text classification. A pinned
config containing only byte `0xff` behaves differently: direct Command and Control sources succeed
without a visible diagnostic, Control consumes an extra hidden empty-command item plus the ordinary
source completion, and a synchronous `if-shell` source emits successful parent and source guards
before later root commands run. zz currently rejects that byte during
`read_to_string`, emits `stream did not contain valid UTF-8`, and returns status 1.
`config.non-utf8-file-bytes` owns the byte-input matrix and fix.
The `source-file -` completion number agrees, but caller stdin transport remains open under
`protocol.binary-streams`. Parser abort, sourced-hook cwd, deferred event-hook cwd and routing, and
hard-disconnect queue cancellation also remain under their named gaps.
No-match, glob, and located depth diagnostics stay inside the source command's guard. Config
summaries and lexer-owned diagnostics remain generic Warning events
behind the prose classifier in `control-mode.diagnostic-typing`. The known-family Warning fallback
remains for legacy producers, while the exact protocol handshake rejects v76 and v77 client-daemon
skew before either event path can mix. Protocol v78 peers reject both older shapes, and protocol
v79 peers also reject the pre-actor-ID command-output shape. Counting
the initial `source-file` as invocation 1, both sides run 50 concurrent
source invocations and refuse invocation 51 with `too many nested files` before any of its
paths are matched or loaded: Command stderr at rc 1, the same lowercase text on the Control
error channel while the outer typed line continues, and the capitalized `Too many nested
files` on an attached status line. `-q` does not suppress it, one diagnostic covers a refused
command rather than each of its paths, and the containing file keeps running its later
physical lines. Both Control implementations place the refusal inside the rejected nested
command's own flags-1 `%begin`/`%error` guard. The closed nested queue proof covers its cross-depth
placement without widening the depth slice. Same-line replay grouping now matches the pin: the refused
source's later `;` siblings are dropped, the next physical line runs, and a matched parent source
still runs its own same-line sibling. Matched child runtime, parser, and OS or path read failures do
not prune that parent group; zz retains those child failures in `ConfigLoadReport`. Whole-file abort
and the remaining source-read semantics stay under their existing gaps. A malformed
invocation at the refused depth is diagnosed as malformed rather than as depth on both sides,
because the pin rejects it while parsing the containing file and never consults its depth guard, and
zz runs its depth guard after the command's own flag and positional validation for the same reason.
Precedence, the stdout stream, the rc-1 exit, and the malformed text now agree through the shared
arity and flag parsers. The pin's abandonment of the rest of the containing file remains tracked
under `config.parser-edge-cases`. Startup configuration now uses
one cumulative 50-command source budget across every top-level config. Top-level roots do not consume
slots, quiet misses do, and one command with many paths consumes one slot. Invocation 51 and later
retain `<file>:<line>: too many nested files` in the startup report while later ordinary commands
continue. Runtime sequential sources stay unbounded.

The v80 startup path first reads and parses every root, then replays those parsed roots in declared
order. Root read or parse causes therefore precede replay causes, while nested replay remains
depth-first at its parent command. The report retains normalized explicit-root read failures,
non-`NotFound` default-root failures, parser and unknown-command diagnostics, unsupported and runtime
replay failures, nested-source failures, and successful `display-message -p` output. Startup
discards list-style output. A missing implicit default remains silent. A detached Command launch
exits 0 with empty stdout and stderr and does not consume the report.

The first eligible Control or attached Interactive client consumes the report after successful
delivery. Control receives the full bounded vector through v80 `StartupConfigCauses`; each element
gets one `%config-error` prefix and keeps raw continuation lines. Interactive receives a sanitized,
ordered 64 KiB preview in a `configuration errors` command-output view. A daemon restart parses a
fresh set.
Runtime replay errors now follow
the invoking client. A missing `kill-session` target and a semantic failure from a syntactically
valid `set-option` use the pin's bare text on Command stderr at rc 1, as typed Control errors with
the client's final status retained at 1, and as capitalized attached warnings. Later physical lines
still run, and a containing `source-file` propagates the inner error and status without blocking
inner or outer continuation. Unknown command names and malformed set-option syntax retain the
existing file-prefixed parse-diagnostic path. Protocol v77 carries parser-owned runtime failures
inside their own Control command guards. Synchronous foreground inserted lists use the same flags-1
path. Immediate command hooks and background inserted lists use independent flags-0 guards.
Parser and hook-source read placement plus completion numbering now use the v78 source event.
Hard-disconnect queue cancellation remains under its active Control group. Successful stdout before and after a failure remains in the
invocation transcript; the original stderr and status 1 remain separate. Clientless startup delivery
stays separate.
`source-file -n` runs the same lexer and condition evaluation, retains syntax diagnostics and
optional verbose output, and applies neither parser environment assignments nor parsed commands.
One invocation parses all of its top-level matches before replay. A bare assignment applies during
that parse, affects conditionals in later files, and persists. A parsed `set-environment` command
runs during replay, so it cannot change a later file's branch after that file has parsed, though the
environment change persists after replay. Under `-n`, a bare assignment does not affect a later file
and neither kind of assignment persists.
This is no-effect source parsing, not full tmux parse validation: tmux validates command names and
arguments while building its command list, while zz performs those checks during replay. Shared flag
and arity diagnostics match at dispatch. `config.parser-edge-cases` and `mux.chain-parse-abort`
retain the parse-unit boundary.
The protocol-v84 callback closures do not claim tmux's eager nested-command construction. Pinned
tmux recursively builds `{ ... }` bodies before it applies the outer callback, including under
`source-file -n`. zz records lexical block positions during config parsing and applies callback and
child-command validation only during replay. Consequently, `source-file -n` still does not reject
an otherwise invalid typed `run-shell`, set-option value, or `bind-key` child in zz, and an invalid
nested body can take diagnostic precedence on the pin. Alias definitions earlier in the same source are also
unavailable during the pin's whole-file construction. The outer-user-alias plus nested-user-alias
case retains tmux's `NOALIAS` parse state, which protocol v84 does not carry. `config.parser-edge-cases`,
`mux.chain-parse-abort`, `aliases.config-parse-unit`, and the existing alias owners retain those
boundaries.
`-t` resolves one pane target before path expansion and replay. A missing target follows tmux's
`CMD_FIND_CANFAIL` path: the file still loads with an empty target context, while the invoking client
cwd remains the source base. `-F` reads the resolved target context. `-v` emits canonical parsed
command groups as `path:line: command`, preserves declared-path and glob order, and carries into
nested sources. Control clients suppress explicit and inherited verbose lines. Command clients get
one stdout transcript, and Interactive clients open one command-output view. Each invocation emits
its complete verbose batch, then replay output, then buffered command-name and parser diagnostics.
Source no-match, glob, and actual OS or path read failures retain their existing error channels. A nested source
inserts its own complete frame at the parent command's replay position, so recursion is depth-first.
This is per-invocation batching, not a claim of physical verbose and replay interleaving. Valid
successful replay and `-v` output produce no duplicate Info or Warning event; parser diagnostics may
still publish their existing Warning summary. Protocol v79 closes the TUI keyboard-navigation
contract for the output view: live copy tables, line and page movement, search editing plus `n`/`N`,
selection-to-paste-buffer, and stock vi/emacs exit behavior. The local attached fixture drives these
semantics over 96 output lines on zz and the pinned tmux. It does not claim mouse behavior, an OS
clipboard write, ordinary TUI pane copy-search editing, SSH transport, or presentation pixels.
Every runtime invocation,
including one that names the active native `zz/mux.conf`, parses its matches in declared-path and
glob order before replaying them in the same order. Declared default, after, and default paths
therefore apply as `DAD`; a loud miss returns status 1 without stopping later matches, and diagnostics
plus `-v` lines retain declared path and glob order. Explicit `reload-config` keeps its native default rediscovery,
key-table reset, appearance rebuild, and stored-override replay. Startup still chooses the first
existing zz-owned candidate, while ordered explicit `-f` files remain its roots. Parse-only and
nested source paths keep their existing behavior.
`source-file` does not expand tildes again during path resolution.
Leading tildes that the config lexer expands already arrive as absolute paths; a quoted literal
tilde or a tilde passed through direct argv remains relative and follows the command's normal base
selection. One parser edge remains tracked separately: tmux expands a tilde immediately after a
closing quote, while zz leaves it literal.

# Syntax and tokenization rules

The lexer processes only real input characters, then performs explicit EOF finalization: a valid
final command is flushed without requiring a newline, and an open single or double quote at EOF
finishes the current token like the pin. A trailing escape still produces a diagnostic and omits
that invalid final command. Its rules:

| Feature | Behavior |
| --- | --- |
| Word separators | Any Unicode whitespace ends the current word (outside quotes). |
| Command separators | `;` and newline end the current command; empty commands are dropped. |
| Comments | `#` starts a comment **only** when no word has started; it runs to end of line. A `#` inside/after a word is literal. |
| Single quotes `'…'` | Literal; backslashes are **not** escapes and `$` does **not** expand inside single quotes. |
| Double quotes `"…"` | Grouping; the full pin escape set applies (below); a newline inside the quote strips following indentation and `#` comment lines like the pin. |
| Escapes (bare words + double quotes) | `\NNN` (exactly three octal digits), `\a \b \e \f \s \v \r \n \t`, `\uXXXX`, `\UXXXXXXXX`, `\$` → literal `$`, any other `\x` → `x`. Invalid forms (`\4`, `\400`, short/overlong `\u`, surrogates) are file-aborting diagnostics. `\377`/`\000` land as UTF-8 U+00FF / embedded NUL — the pin stores raw bytes; accepted `String` divergence, pinned by test. |
| `$VAR` / `${VAR}` expansion | Bare words and double quotes, not single quotes; undefined → empty string; unbraced names are alpha/underscore-led (`$9` stays literal, `${9}` expands). Lookup = same-file assignments overlay, then the context (daemon: engine global environment, hidden included). |
| `NAME=value` / `%hidden NAME=value` | Line-leading assignment (one per statement, pin grammar) applied at word completion — visible to later tokens on the same line and to later lines; `%hidden` sets the hidden flag. Assignments flow to the daemon in `ParsedConfig.environment` and are applied to the engine's global environment before any command of the file executes. |
| `%if / %elif / %else / %endif` | EVALUATED at parse time: the condition format-expands through the context (no jobs — `#()` is empty) and truth-tests with the pin's `format_true` (false = empty or exactly `"0"`). Same-line and nested forms per the pin's `condition1` grammar; a condition's `#{…}` scans balanced through whitespace; `#{` right after `%else`/`%endif` is a `syntax error` like the pin. |
| Backslash escape | Outside single quotes, `\` escapes the next char; `\`+newline is line continuation (joins lines). |
| Quoted empty / concatenation | `""` preserves an empty argument; `""suffix` concatenates into one word (adjacent quoted+bare text is a single token). |
| Open quote at EOF | The current single- or double-quoted token and final command finish without an unterminated-quote diagnostic. |
| Command blocks `{ … }` | A standalone unquoted balanced block is one argument whose zero-based position is retained on `CommandInvocation`; quoted brace text is a string. A block cannot be the command name. |
| First-word command name | The first word of a command becomes `CommandInvocation.name`; the rest become `args`. |

Only tokenization lives here. What the resulting commands *mean* (supported names, aliases, flags,
`-t` targets) is [the command layer](/tmux/commands.md); supported config directives are whatever
`MuxEngine::execute` accepts (bindings, `set-option`/`set-window-option`, `source-file`, prefix
changes, and mux commands). Unsupported directives parse fine here and are rejected/skipped downstream
with a source-span diagnostic.

For `bind-key`, every positional accepts either a string or balanced typed `{ … }` block while
`-T` and `-N` values remain strings. A typed key expands the live mux environment and prints its
recursive command tree before key lookup. One typed tail preserves parsed physical groups, one
string tail reparses as one group, and longer tails use argument-list parsing. An empty typed tail
is a valid empty list. Outside a block, an escaped `\;` separates bound commands; one final
separator is discarded, while leading and doubled separators still report an empty command.

Callback construction walks every lexical typed block recursively before validating the parent
command's name, callback type, or arity. One user-alias layer is tracked per recursive path, so
siblings remain independent; an alias-produced subtree disables another user-alias expansion, and
self-recursion becomes an unknown-command parse error without killing the daemon. Nested
`if-shell`, `run-shell`, set-option, and `confirm-before` blocks print canonical names. An empty
block reads back as `{  }`, and physical internal group newlines print as ` ;; `. These rules cover
each constructed command tree. Stored `bind-key` and `set-hook` lists and typed `if-shell`,
`run-shell`, and `confirm-before` callbacks execute that constructed form without another
user-alias lookup. Typed `if-shell` and `run-shell` callbacks preserve physical groups: a failed
group stops its remaining commands while later physical lines continue; string callbacks remain
one group. Typed `command-prompt` templates retain their structured prepared command list through
submission without re-expanding aliases. The command accepts zero or one template positional as a
typed block or string, while `-I`, `-p`, `-t`, and `-T` values stay strings. Structured substitution
edits leaf arguments in place and preserves their boundaries against quote or semicolon injection.
A string template substitutes its raw source before a fresh parse and whole-result construction
pass. Both paths replace the first `%%` and every `%1`; a trailing `%` quotes double quotes,
backslashes, dollar signs, semicolons, and tildes.
Typed templates retain their physical groups, while string templates and free input form one group.
The string path retains the original source path and line for parse or construction failures.
Prompt chains and multi-answer `%2` stay under their existing prompt owner. `set-hook` and
command-valued native set-option deliberately construct again.
A typed `display-menu` action drops its structural wrapper before the fresh selection parse, while a
quoted brace string remains literal. These rules do not yet make source-file parse and construction
atomic for the whole file or close the broader replay-channel difference.

# Schema

`ConfigDiagnostic` cases emitted directly by the lexer:

| Diagnostic message | Cause |
| --- | --- |
| `syntax error` | The pin's generic yacc message: trailing `\` at EOF, stray/short `%if` family directives, unterminated `%if` (reported at the EOF detection line, like the pin), a second leading assignment, `#{` after `%else`/`%endif`. |
| `invalid octal escape` | `\` followed by a bad octal form (`\4`, `\400`, `\12x`). |
| `invalid \u argument` / `invalid \U argument` | Bad or out-of-range unicode escape (surrogates included). |
| `invalid environment variable` | `${` never closed. |

Any of these aborts the whole file (commands dropped, prior assignments kept, exactly one
diagnostic — no cascade).

`ParsedConfig` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `commands` | `Vec<CommandInvocation>` | Parsed commands in order, each with an attached `SourceSpan` and any lexical command-block positions. Command and argument validation still occurs during replay rather than as one eager whole-file unit. |
| `environment` | `Vec<ConfigEnvironmentAssignment>` | Ordered `NAME=value` assignments (`name`, `value`, `hidden`) reduced during parse; the daemon applies them to the global environment before the file's commands run. |
| `diagnostics` | `Vec<ConfigDiagnostic>` | Lexer-level errors (`source`, `line`, `column`, `message`). |

# Examples

```text
# a comment (whole line, since no word started yet)
set -g prefix C-a
bind c new-window -n 'my window'; bind -n F2 splitw -h   # two commands on one line
bind x send-keys "hello world" Enter                     # "hello world" is one arg
bind c new-\
window                                                   # \<newline> continues → "new-window"
set -g word-separators ""                                # preserves an empty-string argument
```

Parsing the first block yields commands `set -g prefix C-a`, `bind c new-window -n "my window"`,
`bind -n F2 splitw -h`, and `bind x send-keys "hello world" Enter`; the `# ...` tail is ignored.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/parser.rs` | The `parse_config` lexer, `ParsedConfig`, and `ConfigDiagnostic`. |
| `crates/zz-protocol/src/message.rs` | `CommandInvocation` and `SourceSpan` produced by the parser. |

# Related

- Output feeds [the command layer](/tmux/commands.md) (`MuxEngine::execute`) and the
  [key tables](/tmux/key-tables.md) via `bind-key`.
- Part of the broader [tmux compatibility](/tmux/tmux-compat.md) effort, checked against
  `cmd-parse.y`/`arguments.c`/`cfg.c` in the [tmux upstream reference](/references/tmux-upstream.md).
- Lives in [crates/zz-mux](/crates/zz-mux.md).
