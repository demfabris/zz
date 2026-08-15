---
title: Agent panes
description: Claude Code and Codex as panes, over ACP. Experimental.
---

:::caution[Experimental]
Agent panes ship in every build, but stay off until you ask for them: set
`experimental-agent-pane = true` in `zz/config`, or flip the toggle under
**Settings → Advanced → System → Experimental**. Expect rough edges. Turning
it back off blocks new agent panes everywhere; panes that are already open
keep running.
:::

An agent pane runs Claude Code or Codex over the Agent Client Protocol, with
the conversation drawn natively rather than as terminal output: streaming
Markdown, Mermaid diagrams, reasoning, plans, tool calls with typed payloads
(a diff renders as a diff, not as JSON), live command output with exit codes,
subagent timelines, and permission approvals.

The agent process belongs to the zz daemon, not to the window you opened it in.
That is the whole design: the daemon spawns the adapter the same way it spawns
a shell, so a turn keeps running whether or not anything is looking at it.

Adapter versions are pinned and their cache is warmed when the first pane opens,
so a pane should open without waiting on the network. The daemon never runs your
shell init either, so zz repairs the agent's `PATH` from your login shell plus
any fnm, nvm, volta, bun, pnpm, or mise bin directory on disk — the agent CLIs
usually live exactly there, and a Dock launch would otherwise miss them.

## Running a turn

One button carries the whole turn. It sends when the pane is idle, **queues**
when you type while the agent is working, and **stops** the turn when the
composer is empty. Queued prompts fire in order as each turn settles, and a
chip above the composer hands them all back to the draft — text and pasted
images — if you change your mind or the turn dies.

Approvals are automatic by default. A permission request that carries a normal
allow option is answered with it, so the agent runs its tools without stopping
to ask; the tool call still lands in the transcript, so nothing happens out of
sight. Set `agent-auto-approve = false` to answer them yourself, and they
arrive as a wizard — `1`-`9` picks an option, Enter confirms the highlight,
Escape cancels.

**Changes** in the pane header shows what the turn did to your worktree: a
file list with line counts, and hunks per file. It diffs against a snapshot
taken when the prompt was sent, using a throwaway git index, so your real
staging area is never touched and files that were already untracked don't
show up as new.

If an adapter goes quiet mid-turn for two minutes with nothing outstanding,
zz parks the turn instead of killing it: the spinner settles, the pane accepts
prompts again, and the agent process is left alone.

## Getting things in

You pipe into a pane from any shell:

```sh
git diff | zz agent-send                 # into the composer
rg -n TODO | zz agent-send --submit      # send it outright
zz agent-send --context src/main.rs:10-40 < snippet.txt
```

Without a target it goes to the window's most recently focused agent pane.
`--submit` is handled by the daemon, so it works with only `zz attach` running,
or with nothing attached at all — the pane's agent has to be alive, not a
window. Without `--submit` the text is a composer draft, which is still the
window's business, so that form needs a GUI attached to the pane.

`prefix e` sends the last command **and its output** from a terminal pane to the
agent pane next door. It reads OSC 133 shell-integration marks to find the
boundaries, so you need a shell that emits them (starship, or the ghostty, kitty
and wezterm integrations all do). The capture is tail-capped, so you send the end
of a failure rather than the whole build log.

Pasted images are attached inline, normalized once so the thumbnail and the
bytes on the wire match. The element picker in a
[browser pane](/docs/browser/) produces text and a screenshot as one paste.

## Letting an agent drive zz

Every agent process inherits `ZZ_SOCKET`, so it can reach the daemon it is
running under, and `zz tools` prints a short catalog written for agents. An
agent can work the workspace it lives in through the same CLI you use, with no
MCP server in between:

```sh
zz capture-pane -t %1 -p         # read a terminal
zz capture-browser -t %4 -o /tmp/shot.png
zz set-browser-url -t %4 https://localhost:3000
zz agent-send -t %5 'check the failing test'
```

`zz tools` works in any build. The rest of that list needs a browser pane or an
agent pane to aim at.

## What survives what

Close the window and the agent keeps working. The turn runs on in the daemon,
tools keep executing, and a permission request just waits for someone to answer
it. Reopen zz and the pane replays what you missed and then tails the live
stream, so you can walk away mid-turn and come back to the finished answer. Two
devices attached to the same session see the same conversation, and either one
can answer a permission prompt — first answer wins.

Restarting the *daemon* is the line. That takes the pane with it, and there is
nothing left to replay into.

Underneath, the daemon journals every update the agent sends, one file per
session, so it can rebuild a transcript even for an adapter that cannot replay
its own history. Journals live under the daemon's application-data directory
and are kept for 30 days.

## Configuration

All four keys go in `zz/config` as usual. Three of them are settings the
*daemon* uses, since it is what spawns the adapter — zz forwards those to the
daemon for you, and they also work from `zz/mux.conf` or `zz set-option -g`:

| Key | Set on | Default | Effect |
| --- | --- | --- | --- |
| `agent-auto-approve` | daemon | `true` | Answer permission requests with the agent's preferred allow option. Off means you answer them |
| `agent-command` | daemon | pinned `codex-acp` | Command line or raw ACP stdio JSON for Codex panes |
| `agent-claude-code-command` | daemon | pinned `claude-agent-acp` | Same, for Claude Code panes |
| `agent-working-directory` | app | the donor pane's cwd | Absolute path for brand-new sessions |

Changing an adapter command does not restart the agents already running; the
next pane you open picks it up.

Environment knobs, for when the defaults get in the way. The first two are read
by the **daemon**, so set them where the daemon starts — exporting them in a
shell that only launches the GUI will not reach the agent:

| Variable | Read by | Effect |
| --- | --- | --- |
| `ZZ_AGENT_LOGIN_SHELL=0` | daemon | Skip the login-shell `PATH` probe. Use it if your shell init is slow or does something exotic |
| `ZZ_AGENT_QUIESCE_MS` | daemon | Milliseconds of silence before a turn is parked. Default 120000; `0` disables the watchdog |
| `ZZ_AGENT_SOUND=0` | app | Silence the chimes that play when a pane needs you or finishes |
