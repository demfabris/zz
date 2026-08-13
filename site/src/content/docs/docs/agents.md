---
title: Agent panes
description: Upcoming work. Claude Code and Codex as panes, over ACP.
---

:::caution[Not shipped yet]
Agent panes are upcoming work. They are compiled out of every release build,
and nothing on this page is something you can use today. It describes where the
feature is heading, so treat the details as subject to change.
:::

The plan is a pane that runs Claude Code or Codex over the Agent Client
Protocol, with the conversation drawn natively rather than as terminal output:
streaming Markdown, Mermaid diagrams, reasoning, plans, tool calls with typed
payloads (a diff renders as a diff, not as JSON), live command output with exit
codes, subagent timelines, and permission approvals.

Adapter versions are pinned and their cache is warmed at launch, so a pane
should open without waiting on the network. Detaching and reattaching replays
the conversation.

## Trying it early

Agent panes need two things turned on, and both are off by default. Build with
the cargo feature:

```sh
cargo build --features agent-pane
```

then set `experimental-agent-pane = true` in `zz/config`, or use the toggle
under **Settings → Advanced → System → Experimental**. Without the cargo
feature the config key reads false whatever the file says. While either is off,
nothing creates an agent pane: not the pane picker, not the command palette,
not the CLI.

Expect rough edges. This is the path for people who want to follow along, not a
supported configuration.

## Getting things in

Once a pane exists, you pipe into it from any shell:

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
