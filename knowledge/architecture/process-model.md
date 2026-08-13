---
type: Architecture
title: Process & threading model
description: How zz splits work across the persistent daemon, GUI and CLI clients, CEF subprocesses, ACP agents, and per-PTY worker threads.
resource: crates/zz-daemon/src/daemon.rs
tags: [architecture, process-model, daemon, threading, ipc]
timestamp: 2026-08-01T00:00:00Z
---

# Overview

zz runs as several cooperating processes rather than one monolith. A single persistent **daemon**
owns durable multiplexer state and PTYs; **GUI clients** and **CLI clients** attach to it over an
owner-only local IPC endpoint; the browser runs as its own tree of **CEF subprocesses**, and each
Agent pane owns an **ACP child process** inside its GUI process. The separation lets terminals
survive GUI detach while keeping the mux state a single source of truth.

# Processes

| Process | Owns | Lifetime |
|---------|------|----------|
| Daemon ([server](/crates/zz-daemon.md)) | mux state, PTYs, terminal frame fanout, IPC listener | persistent; survives GUI detach; exits on `kill-server`, or once it has **zero sessions and zero interactive clients**; dies on crash, logout, reboot |
| GUI client ([app](/crates/zz.md)) | GPUI windows, rendering, local CEF sessions, ACP controller/session reducers | attached to at most one session; a session takes as many clients as devices attach |
| CLI client | one short-lived command (`list-sessions`, `send-keys`, …) | request/response, then exits |
| CEF browser process | Chromium main/GPU/renderer/utility (zygote) tree | spawned inside a GUI process; not kept alive without a GUI |
| ACP agent process | Codex or Claude Code reached over stdio JSON-RPC; one process and ACP session belong to one Agent pane | spawned when the pane appears; replaced on provider/config changes; stopped with the pane or GUI |

The daemon's exit rule deliberately differs from tmux's `exit-empty`: destroying the last session is
not enough while a GUI client is still attached, because that client outlives its last pane and must
be able to show an empty workspace and make a new session. Quitting the app drops the last
subscriber and lets the empty daemon exit; live sessions keep it alive across app restarts. The
opt-in `quit-daemon-on-exit` key makes app quit send `kill-server` regardless. See
[session persistence](/concepts/session-persistence.md).

A GUI process auto-starts a daemon if none is running, then attaches as a client. On Unix the
spawned daemon gets its own process group (`process_group(0)`), so Ctrl+C or a closing tty
in the launching terminal never signals the daemon and its sessions. Neither client kind is capped:
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
session when the daemon no longer has it. The attached host retries until it comes back, while
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

The daemon never launches an AI provider. `AgentController` in the GUI starts one ACP v1 child per
daemon-owned Agent `PaneId`, using `agent-command` for Codex or `agent-claude-code-command` for Claude
Code. The daemon snapshot stores the selected provider, session's absolute cwd, and opaque ID. On GUI
reattach, the replacement controller uses `session/load` against that same provider when supported;
the agent remains the authority for conversation history. Switching providers deliberately clears
the old session ID and starts a fresh thread. Pane removal and GUI shutdown cancel active turns and
permission responders before that pane's child is reaped.

# Threading inside the daemon

Each terminal pane's PTY child and all of its `libghostty-vt` objects live on **one dedicated worker
thread** ([zz-terminal](/crates/zz-terminal.md)); the objects are never shared across threads. The
worker publishes one immutable [terminal frame](/concepts/terminal-frame.md) per active client view,
each rendered against that view's own scroll, selection, copy-mode, and search state. One
`zz-pane-{id}` thread per pane diffs each view stream separately and fans the patch or full viewport
to that view's client. See [PTY worker](/concepts/pty-worker.md) for the ownership boundary between
server and zz-terminal.

# Related

- [System overview](/architecture/overview.md)
- [Data flow](/architecture/data-flow.md)
- [Wire protocol](/protocol/wire-protocol.md) and [IDs](/protocol/ids.md)
