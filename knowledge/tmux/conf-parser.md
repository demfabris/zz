---
type: Subsystem
title: tmux-grammar config parser (parser.rs)
description: A single-pass tmux-style tokenizer plus the daemon replay layer that keeps stored zz/config mux overrides above the sourced zz/mux.conf configuration.
resource: crates/zz-mux/src/parser.rs
tags: [tmux, parser, config, tokenizer, mux-conf]
timestamp: 2026-07-25T00:00:00Z
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

`parse_config` never fails hard: it returns `ParsedConfig { commands: Vec<CommandInvocation>,
diagnostics: Vec<ConfigDiagnostic> }`. Unterminated quotes and trailing escapes become diagnostics
while the successfully-parsed commands are still returned.

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
| Single quotes `'…'` | Literal; backslashes are **not** escapes inside single quotes. |
| Double quotes `"…"` | Grouping; `\n`, `\r`, `\t` expand to newline/CR/tab; other `\x` yields `x`. |
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
| `trailing escape` | Input ends immediately after a `\`. |

`ParsedConfig` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `commands` | `Vec<CommandInvocation>` | Parsed commands in order, each with an attached `SourceSpan`. |
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
