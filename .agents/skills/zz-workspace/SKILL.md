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
guesses fail with `can't find window`. Discover verbs with `zz list-commands` (add
a verb name for its usage line); the `--help` flag prints only the tmux usage
banner. Never run the binary without a verb (`zz` alone, or with only global flags
such as `-T`): that launches the desktop app. Pane options need `-p`:
`zz set-option -p -t %3 @name reviewer`.

```sh
zz list-sessions
zz list-windows
zz list-panes -F '#{pane_id} #{pane_kind} #{agent_state} #{@agent_state}'
```

`#{pane_kind}` is `terminal`, `agent`, `browser`, `editor`, or `picker`.
`#{@name}` reads a user option from pane, window, session, then global scope.

## Verbs

### `zz agent-send [-t %N] [--submit | --wait [--timeout SECS] [--on-block wait|fail]] [--context PATH[:START[-END]]] [TEXT]`

Draft into another Agent pane's composer for its user to review. An omitted or
non-agent target routes to that window's most recently focused Agent pane,
except for Claude Code terminal peers described below.
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
`--wait` is not supported for these targets.

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

Read native Agent state and permission presence through formats:

```sh
zz list-panes -F '#{pane_id} #{agent_state} #{agent_pending_permission}'
until [ "$(zz display-message -p -t %5 '#{agent_state}')" = idle ]; do zz wait-for agent_state@%5; done
```

The `agent_state@%N` channel is sticky: a signal before the wait still wakes it.
Recheck the state after waking. A completed turn or permission request also rings
the pane bell; use `zz set-hook -g alert-bell 'display-message "agent needs attention"'`.

For foreign agents, read `@agent_state` with `zz show-options -p -t %5 -v @agent_state`
and wait on `zz wait-for '@agent_state@%5'`. Use the lifecycle hooks below to write it.
`zz set-hook -g @option-changed` observes user-option writes.

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
`SendMessage` queues a prompt into the pane while its adapter runs.

## Foreign agents in terminal panes

A terminal CLI agent has no ACP stream. Configure its lifecycle hooks (Stop,
Notification, or equivalent) to write a pane option:

```sh
zz set-option -p -t "$TMUX_PANE" @agent_state idle
zz set-option -p -t "$TMUX_PANE" @agent_state needs-approval
until [ "$(zz show-options -p -t %5 -v @agent_state)" = idle ]; do zz wait-for '@agent_state@%5'; done
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
