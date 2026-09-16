---
name: zz-workspace
description: Drive the surrounding zz workspace from inside a zz Agent pane using the `zz` CLI to send text or piped output to another agent's composer, read a terminal pane's scrollback, screenshot a browser pane, navigate a browser, and route a failing command's output to an agent. Use whenever you are running inside zz (the `ZZ_PANE` environment variable is set) and the task involves another pane, a terminal's output, or a browser page.
---

# Workspace tools

Drive terminal, browser, and Agent panes through the `zz` CLI. Run `zz tools`
for the full catalog or `zz tools agent` for agent verbs. Run `zz tools --skill`
for the agent guide with skill frontmatter. Regenerate it with `just tools-skill`.
Run `zz tools terminal` for terminal verbs, `zz tools browser` for browser verbs,
and `zz tools advanced` for state subscriptions and terminal-agent integration.

## Environment

| Variable | Meaning |
| --- | --- |
| `ZZ_PANE` | Your own pane, e.g. `%3`. Do not send to yourself. |
| `ZZ_SESSION` | Your daemon session name. |
| `ZZ_SOCKET` | The daemon endpoint; the CLI honors it. |

Explicit values in `agent-command` config take precedence over these defaults.

## Targets

Start with a pane summary or selected fields:

```sh
zz inspect -t %3
zz list-panes -F '#{pane_id} #{pane_kind} #{agent_state} #{@agent_state}'
zz list-sessions
zz list-windows
```

Use `list-panes --json` for the full tmux variable set.
Use stable IDs: `%N` for a pane, `@N` for a window, `$N` for a session.
Pass the bare ID: `-t %3` works; `-t work:%3` fails with `can't find window`.
Run `zz <verb> --help` for options, or `zz list-commands [verb]` for usage.
Running `zz` without a verb launches the desktop app, even with global flags such as `-T`.
Pane options need `-p`: `zz set-option -p -t %3 @name reviewer`.

`#{pane_kind}` is `terminal`, `agent`, `browser`, `editor`, or `picker`.
`#{pane_last_command_status}` is the last completed command's exit code, or empty
when unknown; terminal and Agent panes report it from OSC 133 marks.
`#{@name}` reads a user option from pane, window, session, then global scope.

## CLI contract

The `--help` flag alone, or the `help` verb, prints the command catalog. Use
`zz <verb> --help` for a command's description, usage, options, and positional
arguments; aliases and unique prefixes work too. These help forms need no daemon and exit 0. An
unknown verb exits 1; an unknown verb requested through zz’s `--help` exits 2.
Global `-h` keeps the tmux usage banner, and command `-h`
flags keep their tmux meaning.

Add `--json` to `list-sessions`, `list-windows`, `list-panes`, or `list-clients`
for one JSON object per row in the same order as text output. Keys are the format
variable names for that entity; values are strings with the same expansion as
`#{name}`, including empty strings for unavailable values. Pane rows include
`pane_kind`, `agent_state`, `agent_pending_permission`, `browser_url`,
`pane_pb_state`, and `pane_pb_progress`. Use `show-options --json` for one object
mapping option names to value strings in the selected scope. Combining `-F` and
`--json` is a usage error.

| Exit code | Meaning |
| --- | --- |
| 0 | Success. |
| 1 | Command failure, a stopped agent turn, missing daemon, connection loss, or a tmux-compatible usage error (including an unknown command). |
| 2 | Usage error in a zz-native verb or extension: invalid flag, missing argument, or malformed value. |
| 3 | Blocked or unable to answer now, including `agent-send --on-block fail`. |
| 124 | Wait timed out, including `agent-send --timeout`. |
| 125 | Reserved for the `run-pane` timeout. |

Tmux-compatible commands keep the pin’s exit status, including 1 for parse and usage errors.
The error’s source determines the status: `list-panes -Z` exits 1, while zz’s
`list-panes --json -F x` extension conflict exits 2.
Commands that set an explicit exit code keep that code.

## Agent panes

### `zz split-agent [-h | -v] [-t %N] [-P] [-F FORMAT] [-p PROVIDER] [-c DIR]`

Split a pane to start an agent; `-t %N` chooses the pane to split and `-c DIR` sets the new pane's cwd.
Print nothing unless `-P` requests the new pane ID; `-F` changes its format.

The providers are `codex` and `claude-code` (`claude` accepted). Choose one with `zz split-agent -p <provider>`. Each provider is an ACP adapter the daemon spawns through the `agent-command` or `agent-claude-code-command` option. The bundled adapters pin `claude-agent-acp@0.76.0` and `codex-acp@1.11.0`. The model, reasoning effort, and approval policy come from the adapter's own configuration: `~/.codex/config.toml` for Codex or Claude Code's own settings. `zz` does not set them.

### `zz agent-send [-t %N] [--submit | --wait [--progress] [--timeout SECS] [--on-block wait|fail|allow|deny] [--json | --final]] [--context PATH[:START[-END]]] [TEXT]`

Draft into another Agent pane's composer for its user to review.
Print `appended to the composer in %N` when drafted. `--target` is an alias for `-t`. An omitted or non-agent target routes to that window's most recently focused Agent pane. Read stdin when TEXT is omitted: `git diff | zz agent-send`. `--context` adds a file/line header and fences the payload; text is capped at 1 MiB.

`--submit` sends now and prints the chosen pane; a busy pane queues the prompt. `--wait` submits, waits for that turn, and prints its reply on stdout (pane ID on stderr). Failure, cancellation, hand-back, or timeout exits non-zero. The timeout defaults to 600 seconds; `0` waits forever. A timeout leaves the turn running. `--json` prints one object with turn facts, `final_text`, and `transcript`. `--final` prints only the text after the last tool call or tool update. Both require `--wait`; combining them is a usage error. `--on-block wait` waits for permission. `--on-block fail` prints the pending permission JSON and exits 3 while the turn continues. `--on-block allow` answers tool permissions, preferring allow-once, and waits for user questions. `--on-block deny` rejects permissions, including user questions.

Turn facts: `final_text` contains message text after the last tool call or tool update. `tool_calls` counts tool calls the pane saw, including runtime-approved reads; permission counters count only requests that reached the pane. Each buffer keeps its tail when capped: 1 MiB for the transcript and 256 KiB for final text. The JSON object has `pane`, `stop_reason`, `duration_ms`, `tool_calls`, `permissions` (`requested`, `allowed`, `denied`), `truncated`, `final_text`, and `transcript`. A blocked reply also has a nested `permission` object. Exit 0 means `end_turn`; other stop reasons exit 1 with the reply still printed, except cancellation, which keeps its existing error output. A blocked wait exits 3 and a timeout exits 124. Stderr carries the pane ID and on-block audit lines. `--progress` requires `--wait` and an explicit `-t %N`. It prints tool calls, state changes, and a heartbeat after 60 seconds without a line to stderr. Titles in progress lines stop at 120 characters. Over `-H`, it prints `agent-send: --progress is local only` and waits without the progress stream.

### `zz new-agent-session [-t %N] [-c DIR] [--timeout SECS]`

Start a fresh conversation in an agent pane.
Use `-c` to choose its absolute working directory; otherwise use the pane's current directory. Wait until the new session can accept a prompt, then exit 0 without printing anything. Drop queued prompts from the old session. Creation failures exit 1 with a message. The timeout defaults to 60 seconds and exits 124. A timeout leaves the session change running. Other pane kinds exit 1 with `not an agent pane: %N`.

### `zz restart-agent-pane [-t %N]`

Restart the agent pane's ACP adapter and resume its current session.
Print nothing on success. Use `zz new-agent-session` for a fresh conversation.

### `zz show-agent-permission [-t %N]`

Print the oldest pending permission as `{"request_id":7,"tool_call":{...},"options":[...]}` with nested JSON values; exit 1 when none is pending.

### `zz agent-respond [-t %N] (--allow | --deny | --option ID) [REQUEST_ID]`

Answer the named or oldest pending permission.
Print the chosen option ID. `--allow` prefers allow-once; `--deny` selects a reject option. Use `--option ID` to choose an advertised option by ID, including an answer to a user question.

### Permissions

Choose `off`, `reads` (the default), or `all` for `agent-auto-approve`. With `reads`, the daemon answers ACP tool requests whose kind is `read`. With `all`, it answers requests with any tool kind. With `off`, it sends permission requests to the pane. Requests without a tool kind still need an answer when you use `reads` or `all`.

Set `@agent-auto-approve` at pane, window, session, or global scope, in that order of precedence. If no user option applies, use the server option `agent-auto-approve`. Invalid values leave the pane's effective tier unchanged and log a warning.

```sh
zz set-option -p -t %N @agent-auto-approve all
zz set-option -g agent-auto-approve reads
```

Changes apply live to running agent panes. Use `agent-send --on-block` for per-turn permission handling. Configure content-level policy, such as which shell commands may run, in the adapter's own settings: Codex `approval_policy` or Claude Code permission settings.

### `zz inspect [-a | -t %N] [--json]`

Describe a pane's kind, process, geometry, exit status, progress, agent facts, and browser URL.
Read one `key: value` line per field, or use `--json` for one object with string facts and string arrays for `verbs` and `events`. Lists in text output use spaces; unavailable facts have empty values. `verbs` lists what applies to this pane's kind; terminal panes with a nonempty `agent_state` also include `agent-send`. `events` names what `zz events` can report for the pane. Use `-a` for every pane in every session, ordered by session ID, window index, and pane index, with a blank line between text blocks or one JSON object per line; `-a` and `-t` cannot be combined. With neither, inspect the current pane.

```sh
zz inspect -a --json | jq -c 'select(.pane_kind=="agent") | {pane_id,agent_state}'
```

The keys, in text output order, are `session_id`, `session_name`, `window_id`, `window_index`, `window_name`, `window_width`, `window_height`, `window_size`, `pane_id`, `pane_index`, `pane_active`, `pane_kind`, `pane_pid`, `pane_current_command`, `pane_current_path`, `pane_title`, `pane_width`, `pane_height`, `pane_dead`, `pane_dead_status`, `pane_dead_signal`, `pane_last_command_status`, `pane_pb_state`, `pane_pb_progress`, `agent_state`, `agent_pending_permission`, `browser_url`, `verbs`, `events`.

### `zz events [-t %N]`

Stream hook events.
Print JSON lines, flushed per line: `{"seq":1,"event":"agent-state-changed","time":1750000000000,"hook_pane":"%3","agent_state":"working",...}`. Each line includes the hook's string variables. `time` is Unix milliseconds. Wait for the first line, `{"seq":0,"event":"ready","time":...}`, before starting work. On subscriber overflow, `gap` consumes the next sequence number; the client reconnects and continues counting without another `ready` line. Use `-t %N` for a pane, `-t @N` for a window, `-t '$N'` for a session ID, or `-t name` for a session name. Filters match exact hook fields; `ready` and `gap` always print. Omit `-t` to stream without filtering. `agent-state-changed` carries `agent_state` for every pane kind and `agent_pending_permission` with the permission ID or an empty string. Agent panes also emit `agent-tool-call` for new calls and completed or failed status changes. Each event carries `tool_call_id`, `tool_title`, `tool_kind`, and `tool_status`; titles collapse whitespace and stop at 200 characters: `{"seq":2,"event":"agent-tool-call","time":1750000000000,"hook_pane":"%3","tool_call_id":"call-1","tool_title":"cargo test","tool_kind":"execute","tool_status":"in_progress"}`. These agent events are stream-only; `set-hook` cannot bind them. Other `@option-changed` firings are not streamed; use a hook for those. Invalid arguments exit 2. Connection and daemon errors, or any disconnect other than overflow, exit 1, including server shutdown.

### `zz show-last-output -t %N`

Read an Agent pane's last prompt and reply.
Print them under a `%N $ command` header; an `exit: <n>` line follows when known. Use `zz capture-pane -p -t %N -S - -E -` for its full text and `-J` to rejoin soft-wrapped lines.

## Etiquette

- Draft into another person's Agent pane; use `--submit` only for an authorized hand-off.
- Do not send to `$ZZ_PANE`.
- Trim piped logs to the part the recipient needs.

### `zz debug-marker [NOTE]`

Write a `user_marker` line to the daemon log for later diagnostics.
Print nothing on success.
