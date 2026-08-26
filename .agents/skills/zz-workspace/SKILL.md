---
name: zz-workspace
description: Drive the surrounding zz workspace from inside a zz Agent pane using the `zz` CLI — send text or piped output to another agent's composer, read a terminal pane's scrollback, screenshot a browser pane, navigate a browser, and route a failing command's output to an agent. Use whenever you are running inside zz (the `ZZ_PANE` environment variable is set) and the task involves another pane, a terminal's output, or a browser page.
---

# Driving zz from inside an Agent pane

zz multiplexes terminal, browser, and Agent panes over a persistent daemon. If
`ZZ_PANE` is set in your environment, you are running inside one of those Agent
panes and the `zz` CLI talks to the same daemon your pane belongs to. There is
no MCP server — the CLI *is* the tool surface.

Run `zz tools` for the canonical, always-current catalog. This file explains
when to reach for each verb.

## Environment

| Variable | Meaning |
| --- | --- |
| `ZZ_PANE` | Your own pane, e.g. `%3`. Never send to yourself. |
| `ZZ_SESSION` | The daemon session name your pane belongs to. |
| `ZZ_SOCKET` | The daemon endpoint. Already honored by `zz`; you rarely pass it. |

A value the user set in `agent-command` config wins over all three — zz never
overwrites an explicitly configured variable.

## Targets

Everything is addressed with tmux-style stable IDs: `%N` a pane, `@N` a window,
`$N` a session. Discover them:

```sh
zz list-sessions
zz list-windows
zz list-panes            # panes of the current window
zz list-panes -F '#{pane_id} #{pane_kind} #{@agent_state}'   # which pane is the agent, and what is it doing
```

`#{pane_kind}` is `terminal`, `agent`, `browser`, `editor`, or `picker`; `#{@name}` reads any user
option the way tmux does (pane, then window, session, global).

## Verbs

### `zz agent-send [-t %N] [--submit | --wait [--timeout SECS]] [--context PATH[:START[-END]]] [TEXT]`

Put text into **another** Agent pane. Default behavior appends to that pane's
composer so its user reviews before sending — that is almost always what you
want, because the other pane belongs to a person.

Targeting is forgiving: a `-t` naming a non-agent pane — or no `-t` at all —
routes to that window's most recently focused Agent pane. From a terminal
sitting next to an agent, a bare pipe needs no addressing.

```sh
zz agent-send -t %5 "can you take the frontend half of this?"

# The killer use: pipe anything. No -t: the window's agent pane gets it.
git diff | zz agent-send
cargo test 2>&1 | tail -50 | zz agent-send --context crates/zz-mux/src/command.rs

# Reference a file and line range; the payload is fenced under a header.
sed -n '10,42p' src/lib.rs | zz agent-send --context src/lib.rs:10-42
```

`--submit` sends immediately instead of drafting and prints the pane it
chose. A busy pane queues the prompt behind its running turn (up to four);
nothing is attached to the GUI for this, so it works headless. Prefer drafting
unless the user asked for a hand-off.

`--wait` submits and blocks until that prompt's turn ends, then prints the
agent's reply on stdout (the pane id goes to stderr). It exits non-zero when
the turn fails, is cancelled, the prompt is handed back, or nothing came back
within `--timeout` seconds (default 600; `0` waits forever — on timeout the
turn keeps running). This turns another agent into a function:

```sh
verdict=$(zz agent-send -t %5 --wait "review this diff for races" < diff.patch)
git diff | zz agent-send --wait --timeout 120 | zz agent-send -t %7 --submit
```

Payloads are capped at 1 MiB and must be text: control characters other than
newline, carriage return, and tab are refused.

### `zz capture-pane -p -t %N`

Read a terminal pane's text.

```sh
zz capture-pane -p -t %2              # the visible screen
zz capture-pane -p -S - -t %2         # the whole scrollback
zz capture-pane -p -J -t %2           # rejoin soft-wrapped lines
```

### `zz send-last-output -t %N`

Take a terminal pane's last completed command and its output and drop it into
the most recently focused Agent pane in the same window. This is the "explain
this error" path, and it is bound to `<prefix> e` by default.

Requires a shell that emits **OSC 133** prompt marks. Ghostty, kitty, WezTerm,
and Starship shell integrations all do; zz's own bundled bash/zsh integration
currently emits only OSC 2/7, so users who rely on it need one of the above on
top. Output is capped at the last 200 lines or 256 KiB, whichever bites first,
with a truncation note.

### Reading another agent pane

Agent panes read like terminals: `zz capture-pane -p -t %5` prints what that agent has said,
`zz show-last-output -t %5` returns its last prompt and reply, and a finished turn or a permission
request rings the pane's bell (`set-hook -g alert-bell …`). Prefer `agent-send --wait` when you are
the one asking; use these to observe a conversation you did not start.

### `zz show-last-output -t %N`

The read twin of `send-last-output`: print the terminal's last completed command and its output
(fenced under a `%N $ command` header) instead of routing it to an agent. Same OSC 133 requirement
and caps. `zz show-last-output -t %2 | grep -i error` beats capturing the screen and guessing where
the command started.

### `zz capture-browser -t %N -o /absolute/out.png`

Write a browser pane's latest rendered frame to a PNG. The path must be
absolute — the file is written by the zz window process, whose working
directory is not yours.

On Linux the default GPU compositing path has no readback, and the command says
so; restart zz with `ZZ_BROWSER_SHARED_TEXTURE=0` to capture there.

### `zz set-browser-url -t %N URL`

Navigate a browser pane. (This is the navigation verb; there is no separate
`browser navigate`.)

### `zz send-text -t %N [--no-enter] [--timeout MS] TEXT`

Deliver a prompt to a TUI agent (Claude Code, Codex, …) running in a terminal
pane. zz pastes it (bracketed when the app asked for that), waits until the
text is visibly on screen, and only then presses Enter — the Enter that
`send-keys … Enter` sends too early and the composer swallows. Exits non-zero
with nothing submitted if the text never lands (default 2000 ms). `--no-enter`
drafts without submitting.

```sh
zz send-text -t %5 "run the tests and summarize the failures"
cat prompt.md | zz send-text -t %5 --no-enter        # stdin when TEXT is omitted
zz run-shell -b -d 300 'zz send-text -t %5 continue'   # poke it in five minutes
```

### `zz send-keys -t %N 'text' Enter`

Type raw keys into a terminal pane; `-l` sends the text literally. Prefer
`send-text` for anything with a composer.

### Layout

```sh
zz split-window -h        # new terminal beside the current pane
zz split-browser -h URL   # new browser pane
zz split-picker -v        # a picker the user chooses a kind for
zz split-window -d -P -F '#{pane_id}' -e KEY=VAL -c DIR prog arg…   # argv exec, no shell, prints %N
```

### Foreign agents in terminal panes

A CLI agent running in a terminal pane (Claude Code, Codex, …) has no ACP stream, so zz does not
guess its state from the screen. Let its own lifecycle hooks stamp a pane option, and wait on it:

```sh
# in the agent's hook config (Stop / Notification hooks, or the equivalent):
zz set-option -p -t "$TMUX_PANE" @agent_state idle
zz set-option -p -t "$TMUX_PANE" @agent_state needs-approval

# an orchestrator: every write signals wait-for channel '<name>@<pane>' (sticky, race-free)
until [ "$(zz show-options -p -t %5 -v @agent_state)" = idle ]; do zz wait-for '@agent_state@%5'; done

# or get pushed: the @option-changed user hook runs on every write
zz set-hook -g @option-changed 'run-shell "notify-send zz \"#{hook_target} #{hook_option}\""'
```

## Etiquette

- Another Agent pane is somebody's conversation. Draft into it; do not
  `--submit` unless you were asked to hand work off.
- Do not send to `$ZZ_PANE`.
- Piping is cheap for you and expensive for the recipient's context: `tail` or
  `sed` the interesting part rather than the whole log.
