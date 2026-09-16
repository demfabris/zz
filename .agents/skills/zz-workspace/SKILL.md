---
name: zz-workspace
description: Drive the surrounding zz workspace from inside a zz Agent pane using the `zz` CLI — send text or piped output to another agent's composer, read a terminal pane's scrollback, screenshot a browser pane, navigate a browser, and route a failing command's output to an agent. Use whenever you are running inside zz (the `ZZ_PANE` environment variable is set) and the task involves another pane, a terminal's output, or a browser page.
---

# Workspace tools

Drive terminal, browser, and Agent panes through the `zz` CLI. Run `zz tools`
for this catalog, or `zz tools --skill` for the same text with skill frontmatter.
Regenerate the skill with `just tools-skill`.

## Environment

| Variable | Meaning |
| --- | --- |
| `ZZ_PANE` | Your own pane, e.g. `%3`. Do not send to yourself. |
| `ZZ_SESSION` | Your daemon session name. |
| `ZZ_SOCKET` | The daemon endpoint; the CLI honors it. |

Explicit values in `agent-command` config take precedence over these defaults.

## Targets

Use stable IDs: `%N` for a pane, `@N` for a window, `$N` for a session. Pass the
bare ID: `-t %3` works everywhere, while `-t work:%3` and other session-prefixed
guesses fail with `can't find window`. Discover verbs with the top-level help
(the `--help` flag alone, or the `help` verb), or use `zz <verb> --help` for a
verb's options and arguments. `zz list-commands` also works (add a verb name for
its usage line). Never run the binary
without a verb (`zz` alone, or with only global flags such as `-T`): that launches
the desktop app. Pane options need `-p`:
`zz set-option -p -t %3 @name reviewer`.

```sh
zz list-sessions
zz list-windows
zz list-panes -F '#{pane_id} #{pane_kind} #{agent_state} #{@agent_state}'
```

`#{pane_kind}` is `terminal`, `agent`, `browser`, `editor`, or `picker`.
`#{pane_last_command_status}` is the last completed command's exit code, or empty
when unknown; terminal and Agent panes report it from OSC 133 marks.
`#{@name}` reads a user option from pane, window, session, then global scope.

## CLI contract

The `--help` flag alone, or the `help` verb, prints the command catalog. Use
`zz <verb> --help` for a command's description, usage, options, and positional
arguments; aliases and unique prefixes work too. These help forms need no daemon and exit 0. An
unknown verb exits 2. Global `-h` keeps the tmux usage banner, and command `-h`
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
| 1 | Command failure, missing daemon, or connection loss. |
| 2 | Usage error: unknown verb, invalid flag, missing argument, or malformed value. |
| 3 | Blocked or unable to answer now, including `agent-send --on-block fail`. |
| 124 | Wait timed out, including `agent-send --timeout`. |
| 125 | Reserved for the `run-pane` timeout. |

Commands that set an explicit exit code keep that code.

## Verbs

### `zz agent-send [-t %N] [--submit | --wait [--timeout SECS] [--on-block wait|fail]] [--context PATH[:START[-END]]] [TEXT]`

Draft into another Agent pane's composer for its user to review. An omitted or
non-agent target routes to that window's most recently focused Agent pane,
except for terminal peers, queued terminal sends, and terminal panes with
`@agent_state` using `--wait`, described below.
Read stdin when TEXT is omitted: `git diff | zz agent-send`.
`--context` adds a file/line header and fences the payload; text is capped at 1 MiB.

`--submit` sends now and prints the chosen pane; a busy pane queues the prompt.
`--wait` submits, waits for that turn, and prints its reply on stdout (pane ID on
stderr). Failure, cancellation, hand-back, or timeout exits non-zero. The timeout
defaults to 600 seconds; `0` waits forever. A timeout leaves the turn running.
`--on-block wait` waits for permission; `fail` prints the pending permission JSON
and exits 3 while the turn continues.

A terminal pane running Claude Code with cross-session messaging is a valid
target. Plain sends and `--submit` deliver through Claude Code's inbox between
its tool calls, attributed to your pane, or start a turn when it is idle.
`--wait` prints the session's reply and exits non-zero if Claude Code refuses,
expires, or drops the message, the session exits, or the timeout passes. Held
and delivered status updates keep the wait open. A plain send carries no reply
address unless you are a registered peer: a caller that needs the answer uses
`--wait`, and a pane that should be reachable sets `@name`.

A terminal pane running Codex is also a valid target. The daemon reads the session
name from the pane title and queues the text through Codex's own `codex queue`.
Codex runs it at its next idle, with polling taking up to ten seconds. Plain sends
and `--submit` both queue the message and print the pane ID for command clients.
A fresh session has no name until its first prompt; `/rename` inside Codex resolves
a name collision.

For a terminal pane without a peer reply channel, `--wait` requires a non-empty
`@agent_state` and uses the existing terminal delivery path. It waits for a
non-idle state followed by `idle`, and prints nothing on success. A pane that
starts idle must leave idle within 15 seconds; otherwise it exits 124. The overall
`--timeout` also exits 124. `failed` exits 1; `blocked` waits unless `--on-block fail`
requests exit 3. A missing `@agent_state` exits 1 before sending.

### `zz show-agent-permission [-t %N]`

Print the oldest pending permission as JSON; exit 1 when none is pending.

### `zz agent-respond [-t %N] (--allow | --deny | --option ID) [REQUEST_ID]`

Answer the named or oldest pending permission and print the chosen option ID.
`--allow` prefers allow-once.

### `zz capture-pane -p -t %N [-S -] [-E -] [-J]`

Read a terminal or Agent pane's text. `-S -`/`-E -` include the whole scrollback;
`-J` rejoins soft-wrapped lines.

### `zz send-last-output -t %N`

Send the last completed command and output to the window's most recently focused
Agent pane. The default binding is `<prefix> e`. Requires OSC 133 prompt marks;
the bundled Bash/zsh integration emits them, with best-effort PowerShell support.
Output is capped at 200 lines or 256 KiB with a truncation note.

### `zz show-last-output -t %N`

Print that last command and output under a `%N $ command` header, with the same
OSC 133 requirement and caps. For an Agent pane, read its last prompt and reply.
When known, an `exit: <n>` line follows the header.

### `zz wait-for-exit [-t %N] [--timeout SECS]`

Wait for a terminal pane's command to exit and mirror its status. Prints nothing
on success. A retained dead pane returns its exit status immediately; killing or
respawning a pane releases the wait with status 0. The timeout defaults to 0,
which waits forever; a timeout exits 124. Use a command or Control client.

### `zz inspect -t %N [--json]`

Describe a pane's kind, process, geometry, exit status, progress, agent facts, and
browser URL. Read one `key: value` line per field, or use `--json` for one object
with string facts and string arrays for `verbs` and `events`. Lists in text output
use spaces; unavailable facts have empty values. `verbs` lists what applies to
this pane's kind; terminal panes with a nonempty `agent_state` also include
`agent-send`. `events` names what `zz events` can report for the pane.
The keys, in text output order, are `session_id`, `session_name`, `window_id`, `window_index`, `window_name`, `window_width`, `window_height`, `window_size`, `pane_id`, `pane_index`, `pane_active`, `pane_kind`, `pane_pid`, `pane_current_command`, `pane_current_path`, `pane_title`, `pane_width`, `pane_height`, `pane_dead`, `pane_dead_status`, `pane_dead_signal`, `pane_last_command_status`, `pane_pb_state`, `pane_pb_progress`, `agent_state`, `agent_pending_permission`, `browser_url`, `verbs`, `events`.

### `zz events [-t %N]`

Stream hook events as JSON lines, flushed per line:
`{"seq":1,"event":"agent-state-changed","time":1750000000000,"hook_pane":"%3","agent_state":"working",...}`.
Each line includes the hook's string variables. `time` is Unix milliseconds.
Wait for the first line, `{"seq":0,"event":"ready","time":...}`, before starting work.
On subscriber overflow, `gap` consumes the next sequence number; the client
reconnects and continues counting without another `ready` line.
Use `-t %N` for a pane, `-t @N` for a window, `-t '$N'` for a session ID,
or `-t name` for a session name. Filters match exact hook fields;
`ready` and `gap` always print. Omit `-t` to stream without filtering.
`agent-state-changed` carries `agent_state` for every pane kind and
`agent_pending_permission` with the permission ID or an empty string.
Other `@option-changed` firings are not streamed; use a hook for those.
Invalid arguments exit 2. Connection and daemon errors, or any disconnect
other than overflow, exit 1, including server shutdown.

### `zz wait-pane [-t %N] [--idle MS | --until TEXT | --regex RE] [--timeout SECS] [--tail N]`

Wait for a terminal pane to stop producing output or show a matching logical line.
Choose one condition; the default is `--idle 500`, measured from this call.
Text and regex matches join wrapped screen lines and print the matching line.
`--tail N` searches only the last N logical lines. Idle success prints nothing.
The timeout defaults to 60 seconds; timeout exits 124 and names the condition.
Invalid regex syntax exits 2. Use a command or Control client.

### `zz run-pane [-t %N] [--timeout SECS] [--] COMMAND...`

Run a command in a terminal pane's POSIX shell without shell integration.
Join COMMAND words with single spaces, preserving supplied quoting; pass one
command line. Paste it, verify the echo, then press Enter. Print the output between
unique markers and return the child's exit code. Capture includes scrollback,
capped to the last 10,000 logical lines. The timeout defaults to 120 seconds;
timeout prints the output collected so far and exits 125. It leaves the command
running. Use a command or Control client.

```sh
zz run-pane -t %3 -- "sh -c 'echo hi; exit 7'"
zz wait-pane -t %3 --until 'ready' --timeout 30
```

### `zz send-text -t %N [--no-enter] [--timeout MS] [TEXT]`

Paste into a terminal TUI, wait for the text to appear, then press Enter. Read
stdin when TEXT is omitted. `--no-enter` drafts; the default timeout is 2000 ms.
If the text never appears, exit non-zero without submitting.

### `zz send-keys -t %N 'text' Enter`

Send raw keys to a terminal; `-l` sends literal text. Use `send-text` for composers
that swallow an Enter sent before the paste appears.

### `zz capture-browser -t %N -o /absolute/out.png`

Save the latest browser frame as a PNG. Use an absolute path: the window process
writes the file. On Linux, restart with `ZZ_BROWSER_SHARED_TEXTURE=0` if the GPU
path reports that readback is unavailable.

### `zz set-browser-url -t %N URL`

Navigate a browser pane. `#{browser_url}` reports the active tab's URL.

### Browser pages through CDP

Read and act on a browser pane's page with your own CDP tool. The user enables
the loopback endpoint with `browser-remote-debugging-port = 9222` in `zz/config`
(or `ZZ_BROWSER_REMOTE_DEBUGGING_PORT` on the window process); it is off by
default and listens on `127.0.0.1` only.

```sh
zz split-browser -h -P https://example.com
zz list-panes -F '#{pane_id} #{pane_kind} #{browser_url}'
curl -s http://127.0.0.1:9222/json/list
agent-browser --cdp 9222 snapshot -i
agent-browser --cdp 9222 click @e2
agent-browser --cdp 9222 wait --load load
```

Match the pane to its CDP target by comparing `#{browser_url}` with the target
list, then attach to that target. Never create pages (`Target.createTarget`,
Playwright `newPage`, a tool that opens a fresh tab on connect): CEF hosts those
as native Chromium windows outside zz. Install agent-browser instead of running it
through `npx` per call; the binary answers in about 50 ms, `npx` adds 330 ms.
Prefer `wait --load load` or `wait --text` over `networkidle`.

### `zz debug-marker [NOTE]`

Write a `user_marker` line to the daemon log for later diagnostics.

## Layout

```sh
zz split-window -h
zz split-window -d -P -F '#{pane_id}' -e KEY=VAL -c DIR prog arg
zz split-browser -h -P URL
zz split-picker -v -P
zz split-agent -h -P -p codex
```

Use `-t %N` to choose the pane to split. `-P` prints the new pane ID and `-F`
changes its format. `split-window` executes the supplied argv without a shell.
The bundled adapters pin `claude-agent-acp@0.76.0` and `codex-acp@1.11.0`.

## State and waiting

Read native Agent state and permission presence through formats.
Terminal panes running a listed agent CLI get `working`/`idle` from the OSC 9;4
progress bar through `#{agent_state}` and `@agent_state`. `@agent-progress-commands`
sets the whitespace- or comma-separated list of command basenames (default `claude`).
A terminal pane running Claude Code gets `working`/`idle` from Claude Code's own
session status in its peer registry once per second. Set `@agent-peer-state off`
at pane, window, session, or global scope to disable these updates.

```sh
zz list-panes -F '#{pane_id} #{agent_state} #{agent_pending_permission}'
zz agent-send -t %5 --wait "..."
```

A completed turn or permission request also rings
the pane bell; use `zz set-hook -g alert-bell 'display-message "agent needs attention"'`.

For foreign agents, read `@agent_state` with `zz show-options -p -t %5 -v @agent_state`.
Use `zz agent-send -t %5 --wait "..."` to send and wait for a turn, and use the
lifecycle hooks below to write the state. For transitions you did not cause,
`zz wait-for agent_state@%N` observes native Agent state and
`zz wait-for '@agent_state@%5'` observes a terminal's option writes; read the state
after waking.
`zz set-hook -g @option-changed` observes user-option writes.

The sticky channel is level-triggered like tmux's `wait-for`: a signal that happened
before the wait wakes it at once. A read-then-wait loop started right after a send
can see the pre-send idle and return early. Use `zz agent-send -t %N --wait "..."`
to wait for a turn on any pane kind. Use the channel loop to observe transitions
you did not cause.
`zz events -t %N` streams `agent-state-changed` lines for any pane kind.

```sh
zz run-shell -b -d 300 'zz send-text -t %5 continue'
zz pipe-pane -o -t %5 'cat >> /tmp/agent-output.log'
```

`run-shell -b -d SECS` schedules a background timer. `pipe-pane -o` starts an output
pipe only when none exists, including for an Agent pane's transcript.
The `zz refresh-client` verb supports `-B` subscriptions in control mode. Send this
on the control connection and read `%subscription-changed`:

```text
refresh-client -B 'agent:%5:#{agent_state}'
```

### Peers

Agent panes register as Claude Code peers under their `@name` user option
(default `zz-%N`). `ListAgents` in any Claude Code session lists them, and
`SendMessage` queues a prompt into the pane while its adapter runs. Claude
Code refuses idle subscriptions to these peers. Use
`zz agent-send -t %N --wait "..."` to send and wait for a turn.

A terminal pane becomes a peer when you set its pane `@name` option:
`zz set-option -p -t %3 @name codex-1`. A Codex pane with a session name receives
peer messages through the same Codex queue described above. Other terminal peers,
and Codex sessions without a name yet, receive pasted and submitted text, so do
not name a bare shell pane. A queue lookup or delivery failure leaves the message
unsubmitted and logs the reason. The daemon hosts no vendor's server. Claude Code
terminal sessions keep their own registration.

## Foreign agents in terminal panes

A terminal CLI agent has no ACP stream. Configure its lifecycle hooks (Stop,
Notification, or equivalent) to write a pane option:

```sh
zz set-option -p -t "$TMUX_PANE" @agent_state idle
zz set-option -p -t "$TMUX_PANE" @agent_state needs-approval
zz agent-send -t %5 --wait "..."
zz set-hook -g @option-changed 'run-shell "notify-send zz \"#{hook_target} #{hook_option}\""'
```

Each write signals the sticky channel `<option>@<pane>` and runs `@option-changed`.
Use `#{hook_target}` and `#{hook_option}` to identify the write.

## Browser through CDP

CDP is off by default. Set `browser-remote-debugging-port = 9222` in `zz/config`,
or `ZZ_BROWSER_REMOTE_DEBUGGING_PORT=9222` in the client environment (which takes
precedence). Use `0` to disable it; enabled ports range from 1024 to 65535.
Open a browser pane to initialize CEF. Restart the client to change the port after
initialization. The loopback endpoint grants local processes access to your browser
session; enable it while you intend to grant that access.

Replace PORT with the configured port in one of these attach recipes:

```sh
agent-browser --cdp PORT snapshot -i
npx @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:PORT
npx chrome-devtools-mcp@latest --browserUrl http://127.0.0.1:PORT
```

Correlate pane URLs with CDP targets:

```sh
zz list-panes -a -F '#{pane_id} #{pane_kind} #{browser_url}'
zz display-message -p -t %5 '#{browser_url}'
curl http://127.0.0.1:PORT/json/list
```

`#{browser_url}` gives the active tab URL, or an empty string for other pane kinds.
Compare it with each target's `url`; duplicate URLs need more context, and CDP may
list background tabs too. For `Page.captureScreenshot` with a clip, use `clip.scale`
of `1.0`; CEF off-screen rendering does not support custom capture scale.

## Etiquette

- Draft into another person's Agent pane; use `--submit` only for an authorized hand-off.
- Do not send to `$ZZ_PANE`.
- Trim piped logs to the part the recipient needs.
