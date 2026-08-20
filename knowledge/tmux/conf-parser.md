---
type: Subsystem
title: tmux-grammar config parser (parser.rs)
description: A single-pass tmux-style tokenizer plus the daemon replay layer that keeps stored zz/config mux overrides above the sourced zz/mux.conf configuration.
resource: crates/zz-mux/src/parser.rs
tags: [tmux, parser, config, tokenizer, mux-conf]
timestamp: 2026-08-19T00:00:00Z
---

# Overview

`parser.rs` implements `parse_config(source, input) -> ParsedConfig`, the lexer that the daemon uses
to read the zz-owned `~/.config/zz/mux.conf` on startup (the daemon does not read `~/.tmux.conf`;
the client's import flow copies a user's tmux config there verbatim; see
[Application configuration](/configuration/app-config.md)), to handle `source-file`, and to parse
each `command-prompt` submission. It is a single character-by-character state machine (modeled on tmux's `cmd-parse.y` /
`arguments.c`) that splits input into words, groups words into commands, and records a
[`SourceSpan`](/crates/zz-protocol.md) (`source`, `line`, `column`) for each command so diagnostics and
`list-keys` output can point back at the origin. It produces `CommandInvocation`s only. It does
**not** validate command names or arguments; that happens later in
[`MuxEngine::execute`](/tmux/commands.md).

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

# Syntax and tokenization rules

The lexer processes only real input characters, then performs explicit EOF finalization: a valid
final command is flushed without requiring a newline, while an unfinished quote or escape produces a
diagnostic and omits only that invalid final command. Its rules:

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
| First-word command name | The first word of a command becomes `CommandInvocation.name`; the rest become `args`. |

Only tokenization lives here. What the resulting commands *mean* (supported names, aliases, flags,
`-t` targets) is [the command layer](/tmux/commands.md); supported config directives are whatever
`MuxEngine::execute` accepts (bindings, `set-option`/`set-window-option`, `source-file`, prefix
changes, and mux commands). Unsupported directives parse fine here and are rejected/skipped downstream
with a source-span diagnostic.

For `bind-key`, one balanced `{ … }` argument is reparsed as a command list. An empty `{}` is a
valid empty list. Outside a block, an escaped `\;` separates bound commands; one final separator is
discarded, while leading and doubled separators still report an empty command.

# Schema

`ConfigDiagnostic` cases emitted directly by the lexer:

| Diagnostic message | Cause |
| --- | --- |
| `unterminated quote` | Input ends while inside a `'` or `"` quote. |
| `syntax error` | The pin's generic yacc message: trailing `\` at EOF, stray/short `%if` family directives, unterminated `%if` (reported at the EOF detection line, like the pin), a second leading assignment, `#{` after `%else`/`%endif`. |
| `invalid octal escape` | `\` followed by a bad octal form (`\4`, `\400`, `\12x`). |
| `invalid \u argument` / `invalid \U argument` | Bad or out-of-range unicode escape (surrogates included). |
| `invalid environment variable` | `${` never closed. |

Any of these aborts the whole file (commands dropped, prior assignments kept, exactly one
diagnostic — no cascade).

`ParsedConfig` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `commands` | `Vec<CommandInvocation>` | Parsed commands in order, each with an attached `SourceSpan`. |
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
