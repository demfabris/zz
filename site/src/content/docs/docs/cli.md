---
title: CLI
description: One binary for the GUI, the daemon, and the command line.
---

The `zz` binary is the GUI, the daemon, and the CLI. Any supported tmux
command works from any shell against the running daemon:

```sh
zz list-panes
zz send-keys -t %3 'make test' Enter
zz split-window -h
zz new-window -n logs
zz detach-client
```

Targets use tmux syntax: `$session`, `@window`, `%pane`, all stable for a
pane's lifetime, visible in `choose-tree` and `display-panes`.

## Beyond tmux

| Command | What it does |
| --- | --- |
| `zz new-browser [URL]` | new window with a browser pane |
| `zz split-browser [-p profile] [URL]` | split into a browser pane |
| `zz set-browser-url -t %N URL` | navigate a browser pane |
| `zz capture-browser -t %N -o /tmp/out.png` | screenshot a browser pane (absolute path) |
| `zz split-picker [-h\|-v]` | split into an empty pane and pick its type |
| `zz tools` | the command catalog written for agents |

Two more verbs, `zz agent-send` and `zz send-last-output`, talk to an Agent
pane. They need `experimental-agent-pane` on and a pane to aim at; see
[agent panes](/docs/agents/).

## Sessions

The daemon starts on first launch and outlives every window. Closing a
window detaches. PTYs, layouts, scrollback, and browser URLs and profiles all
survive and come back on reattach. Persistence is daemon-lifetime: a reboot
starts fresh.
