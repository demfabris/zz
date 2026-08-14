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

Adapter versions are pinned and their cache is warmed at launch, so a pane
should open without waiting on the network. A GUI launch never runs your shell
init, so zz also repairs the agent's `PATH` from your login shell plus any fnm,
nvm, volta, bun, pnpm, or mise bin directory on disk — the agent CLIs usually
live exactly there, and a Dock launch would otherwise miss them.

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

`prefix e` sends the last command **and its output** from a terminal pane to the
agent pane next door. It reads OSC 133 shell-integration marks to find the
boundaries, so you need a shell that emits them (starship, or the ghostty, kitty
and wezterm integrations all do). The capture is tail-capped, so you send the end
of a failure rather than the whole build log.

Pasted images are attached inline, normalized once so the thumbnail and the
bytes on the wire match. The element picker in a
[browser pane](/docs/browser/) produces text and a screenshot as one paste.

## Letting an agent drive zz

Every agent process inherits `ZZ_PANE`, `ZZ_SESSION`, and `ZZ_SOCKET`, and
`zz tools` prints a short catalog written for agents. An agent can work the
workspace it lives in through the same CLI you use, with no MCP server in
between:

```sh
zz capture-pane -t %1 -p         # read a terminal
zz capture-browser -t %4 -o /tmp/shot.png
zz set-browser-url -t %4 https://localhost:3000
zz agent-send -t %5 'check the failing test'
```

`zz tools` works in any build. The rest of that list needs a browser pane or an
agent pane to aim at.

## What survives a restart

The daemon keeps the pane, its provider, and its working directory, but the
agent process itself belongs to the app. Close every window and the child
stops. Open it again and zz asks the adapter to replay the conversation; if it
can't, zz replays its own journal of the session instead, so the transcript
comes back either way. Restarting the *daemon* is different — that takes the
pane with it, and there is nothing left to replay into.

Journals live under zz's application-data directory, one file per session,
kept for 30 days.

## Configuration

`zz/config` keys, all optional:

| Key | Default | Effect |
| --- | --- | --- |
| `agent-auto-approve` | `true` | Answer permission requests with the agent's preferred allow option. Off means you answer them |
| `agent-command` | pinned `codex-acp` | Command line or raw ACP stdio JSON for Codex panes |
| `agent-claude-code-command` | pinned `claude-agent-acp` | Same, for Claude Code panes |
| `agent-working-directory` | the donor pane's cwd | Absolute path for brand-new sessions |

Environment knobs, for when the defaults get in the way:

| Variable | Effect |
| --- | --- |
| `ZZ_AGENT_LOGIN_SHELL=0` | Skip the login-shell `PATH` probe. Use it if your shell init is slow or does something exotic |
| `ZZ_AGENT_QUIESCE_MS` | Milliseconds of silence before a turn is parked. Default 120000; `0` disables the watchdog |
| `ZZ_AGENT_SOUND=0` | Silence the chimes that play when a pane needs you or finishes |
