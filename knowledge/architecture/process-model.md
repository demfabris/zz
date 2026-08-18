---
type: Architecture
title: Process & threading model
description: How zz splits work across the persistent daemon, GUI and CLI clients, CEF subprocesses, ACP agents, and per-PTY worker threads.
resource: crates/zz-daemon/src/daemon.rs
tags: [architecture, process-model, daemon, threading, ipc]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

zz runs as several cooperating processes rather than one monolith. A single persistent **daemon**
owns durable multiplexer state and PTYs; **GUI clients** and **CLI clients** attach to it over an
owner-only local IPC endpoint; the browser runs as its own tree of **CEF subprocesses**, and each
Agent pane owns an **ACP child process** inside the daemon. The separation lets terminal and agent
sessions
survive GUI detach while keeping the mux state a single source of truth.

# Processes

| Process | Owns | Lifetime |
|---------|------|----------|
| Daemon ([server](/crates/zz-daemon.md)) | mux state, PTYs, terminal frame fanout, IPC listener | persistent; survives GUI detach; exits on `kill-server`, or once it has **zero sessions and zero interactive clients**; dies on crash, logout, reboot |
| GUI client ([app](/crates/zz.md)) | GPUI windows, rendering, local CEF sessions, ACP controller/session reducers | attached to at most one session; a session takes as many clients as devices attach |
| CLI client | one short-lived command (`list-sessions`, `send-keys`, …) | request/response, then exits |
| CEF browser process | Chromium main/GPU/renderer/utility (zygote) tree | spawned inside a GUI process; not kept alive without a GUI |
| ACP agent process | Codex or Claude Code reached over stdio JSON-RPC; one process and ACP session belong to one Agent pane | spawned when the pane appears; replaced on provider/config changes; stopped with the pane or daemon |

The daemon's exit rule deliberately differs from tmux's `exit-empty`: destroying the last session is
not enough while a GUI client is still attached, because that client outlives its last pane and must
be able to show an empty workspace and make a new session. Quitting the app drops the last
subscriber and lets the empty daemon exit; live sessions keep it alive across app restarts. The
opt-in `quit-daemon-on-exit` key makes app quit send `kill-server` regardless. See
[session persistence](/concepts/session-persistence.md).

A GUI process auto-starts a daemon if none is running, then attaches as a client. On Unix the
spawned daemon gets its own process group (`process_group(0)`), so Ctrl+C or a closing tty
in the launching terminal never signals the daemon and its sessions. The daemon initially has no
session unless config created one; the GUI's actual empty-target Interactive attach lazily creates
numeric session `0`. Registration and background fleet connections do not. Neither client kind is capped:
`ServerState.attached` maps each session to a `BTreeSet<ClientId>`, so a desktop and a laptop watch
one session together while command-only clients come and go. The rule runs one way only: a client
attaches to at most one session, and attaching to a second detaches it from the first. See
[session persistence](/concepts/session-persistence.md) for attach, detach, eviction, and the state
each client owns alone.

# IPC transport

The [wire protocol](/protocol/wire-protocol.md) is a versioned, length-prefixed control protocol
carried over a platform-native, owner-only endpoint:

| Platform | Endpoint |
|----------|----------|
| Linux / macOS | Unix-domain socket |
| Windows | local named pipe |

That is the only binding the daemon has. A remote host adds no third one: the client spawns a managed
`ssh -N -L` child, which forwards the remote daemon's own socket to a local scratch path, and then
speaks the identical envelope over it. Auth, encryption, and host identity are ssh's problem. See
[wire protocol](/protocol/wire-protocol.md) and [fleet attach](/designs/fleet-attach.md).

A dead forward starts a roaming loop. The client moves that host to
`HostState::Reconnecting { attempt }`, leaves the last snapshot and frames on screen, and redials on
a 1/2/4/8/16/30-second backoff; a successful dial re-attaches the same session, or the default
session when one exists. An empty replacement daemon lazily creates the next numeric session on
that fallback attach. The attached host retries until it comes back, while
background fleet hosts give up after three attempts and keep their typed error state.

`kill-server` never starts a missing daemon, and when an older daemon cannot complete the current
protocol handshake, zz verifies the process that owns the exact local socket (identity, ownership,
executable, command line, start time) before requesting termination.

# CEF single-binary multi-process model

CEF uses the same executable for every Chromium role. `crates/zz` ships a helper entrypoint
(`zz_helper`) that CEF re-invokes for its subprocesses; [cef-runtime](/browser/cef-runtime.md)
dispatches on subprocess type. On Linux, Chromium's user-namespace sandbox stays enabled and only the
legacy setuid sandbox layer is disabled (no `--no-sandbox`).

# ACP process model

The daemon launches the AI provider. `agent::host::AgentHost` starts one ACP v1 child per Agent
`PaneId`, on its own `zz-agent-{n}` thread, using the `agent-command` mux option for Codex or
`agent-claude-code-command` for Claude Code. The daemon snapshot stores the selected provider, the
session's absolute cwd, and its opaque ID, and the daemon adopts that ID itself when the adapter
returns one. On reattach, the client asks for a replay from where its reducer stands and then tails;
daemon-side, a respawned adapter uses `session/load` against the same provider when supported and
falls back to the journal when it does not. Switching providers deliberately clears the old session
ID and starts a fresh thread. Pane removal and **daemon** shutdown cancel active turns and permission
responders before that pane's child is reaped . a GUI quit does neither, which is the point.

The desktop client keeps only the rendering half: the reducer, view, composer draft, permission
wizard, and sticky selector preferences. `zz-tui` and `zz-client-ffi` link `zz-daemon` with
`default-features = false`, so a build that never renders a transcript does not pull
`agent-client-protocol` in at all.

# Threading inside the daemon

Each terminal pane's PTY child and all of its `libghostty-vt` objects live on **one dedicated worker
thread** ([zz-terminal](/crates/zz-terminal.md)); the objects are never shared across threads. The
worker publishes one immutable [terminal frame](/concepts/terminal-frame.md) per active client view,
each rendered against that view's own scroll, selection, copy-mode, and search state. One
`zz-pane-{id}` thread per pane diffs each view stream separately and fans the patch or full viewport
to that view's client. See [PTY worker](/concepts/pty-worker.md) for the ownership boundary between
server and zz-terminal.

Agent panes use one `zz-agent-{n}` thread per pane to block on the ACP connection and one shared
`zz-agent-flush` thread to close the fanout's 25 ms coalescing windows. The flusher parks while no
pane has gathered output. The daemon builds the runtime on the first agent pane, so a daemon that
never runs an agent spawns none of these threads.

# Related

- [System overview](/architecture/overview.md)
- [Data flow](/architecture/data-flow.md)
- [Wire protocol](/protocol/wire-protocol.md) and [IDs](/protocol/ids.md)
