---
type: Rust Crate
title: zz-daemon crate
description: The persistent local daemon. Sole authority for mux state, owner of PTY-backed terminal sessions and Agent-pane ACP adapter children, and the fan-out engine that streams coalesced terminal frames and agent transcripts to attached and short-lived clients over a socket or named pipe.
resource: crates/zz-daemon/src/daemon.rs
tags: [crate, daemon, ipc, fanout, transport, agent]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

`zz-daemon` is the **persistent local daemon** and the only authority for mux
state. It hosts [the mux state machine](/crates/zz-mux.md) (`MuxEngine`), owns every terminal pane's
[PTY worker](/crates/zz-terminal.md) as an `Arc<TerminalSession>`, listens on an owner-only local
endpoint, and fans coalesced [terminal frames](/concepts/terminal-frame.md) out to every attached
interactive client plus any number of short-lived CLI clients. Several of the user's devices can
attach to one session at once, each with its own viewport and window focus. The daemon survives GUI
detach so that sessions, windows, splits, and running terminal processes outlive every window. See
[session persistence](/concepts/session-persistence.md).

Since v53 it owns one more runtime: an [Agent pane](/concepts/agent-pane.md)'s ACP adapter child,
spawned and supervised on its own thread exactly as a PTY is, behind the default-on `agent` cargo
feature. A turn therefore survives every GUI window closing, and a reattaching client replays the
transcript out of a daemon-side journal and tails the live stream.

The daemon speaks the versioned [wire protocol](/protocol/wire-protocol.md) from
[zz-protocol](/crates/zz-protocol.md), applies commands through the serialized `MuxEngine`, and
translates the engine's `MuxEffect`s into terminal spawns, agent-pane opens, view attach/detach,
send-keys, and client events. It contains no GPUI or CEF code; live browser rendering stays in
[the app](/crates/zz.md), and the daemon only holds the browser pane's restorable
`BrowserDescriptor` (URL + profile).

# Responsibilities

| Responsibility | Where |
|----------------|-------|
| Own mux state (`MuxEngine`, sessions/windows/panes/splits) | `Shared.inner: Mutex<ServerState>` |
| Own each terminal pane's PTY worker | `ServerState.terminals: BTreeMap<PaneId, Arc<TerminalSession>>` |
| Own each agent pane's ACP adapter child | `Shared.agent: Mutex<Option<Arc<AgentRuntime>>>` → `agent::host::AgentHost`, one thread per pane |
| Bind + guard the owner-only endpoint | `Daemon::run_foreground`, `LocalListener`, `SocketGuard`, `restrict_socket_permissions` |
| Accept connections, one thread per client | `run_foreground` accept loop → `handle_connection` |
| Serialize + execute commands | `Shared::execute` / `execute_command_request` (mux engine under one lock) |
| Fan terminal frames + persist live pane titles | `watch_terminal` → `synchronize_pane_title` / `publish_terminal_for_pane` → mux snapshots + per-client `OutboundMailbox` |
| Route interactive input + `send-keys` | `input` / `input_text` / `input_key`, `execute_key_commands`, `resolve_input_sinks`, `send_tokens`; stored bindings execute exactly as written |
| Attach/detach interactive clients | `attach` / `attach_target` / `detach`, `register` / `unregister`, `evict_other_clients` for `attach-session -d` |
| Serve repair requests after a coalesced drop | `send_full` (`RequestFull`), `send_history` (`HistoryRequest` → `HistoryChunk`), `AgentRuntime::replay` (`AgentReplay` → `AgentUpdates`) |
| Journal and fan out agent transcripts | `agent::journal::AgentJournal` under `<data>/zz/daemon/agent-journal`, `agent::fanout::AgentFanout` → the mailbox's `agent` lane |
| Recover an incompatible daemon safely | `terminate_incompatible_daemon`, `DaemonIdentityGuard` (lifecycle) |

# Module map

| Module (`crates/zz-daemon/src/`) | Public surface | Role |
|-------------------------------|----------------|------|
| `lib.rs` | re-exports `Daemon`, `DaemonError`, `CommandClient`, `InteractiveClient`, `short_device_name`, `agent_send_reads_stdin`, `default_socket_path`, `default_mux_config`, `mux_config_candidates`, `mux_config_write_path`, `RecoveredDaemon`, `DaemonRecoveryError`, `terminate_incompatible_daemon`, `daemon_identity_protocol_version`, `classify_local_connect_error`, `Endpoint`, `EndpointError`, `SshEndpoint`, `SshPrompts`, `AskpassPrompt`, `AskpassPromptKind`, `AskpassReply`, `ASKPASS_SOCKET_ENV`, `run_helper`, the `user_data` module, and (feature `agent`) the agent stream vocabulary a client deserializes against . `AgentStreamItem`, `AgentStreamPayload`, `AgentPrompt`, `AgentPromptOutcome`, `AgentSessionSummary`, `AgentSessionCapabilities`, and `AgentAuthMethod` | Crate root; the whole public API |
| `daemon.rs` | `Daemon`, `DaemonError`, `agent_send_reads_stdin` | The server itself: local accept loop, `Shared`/`ServerState`, command execution, `OutboundMailbox` fan-out, terminal watching, attach/detach, input routing. It binds exactly one endpoint . the owner-only local socket |
| `transport.rs` | `default_socket_path` | Platform IPC: wraps `interprocess` into `LocalListener`/`LocalStream`, per-platform endpoint paths, peer-credential capture |
| `client.rs` | `CommandClient`, `InteractiveClient`, `short_device_name` | Client halves of the protocol: connect + handshake (`connect_endpoint` for an `ssh://` endpoint), framed `ProtocolSender`/`ProtocolReceiver`, request/response and attach/detach/input helpers |
| `paths.rs` | `default_mux_config`, `mux_config_candidates`, `mux_config_write_path` | Platform discovery of the zz-owned `zz/mux.conf` (the daemon sources this file and no external tmux config) |
| `endpoint.rs` | `Endpoint`, `SshEndpoint`, `EndpointError` | Client-half endpoint abstraction: `unix://`/bare-path/`ssh://` URI parsing (a `quic://` string is rejected with a pointer at `ssh://`), the probe → auto-start → forward ssh sequence below, a managed `ssh -N -L` child with an RAII tunnel guard, and `EndpointError::ssh_reason` turning each failure into advice for the host row |
| `askpass.rs` | `SshPrompts`, `AskpassPrompt`, `AskpassPromptKind`, `AskpassReply`, `ASKPASS_SOCKET_ENV`, `run_helper` | ssh's password and host-key prompts: the per-connect Unix socket the GUI answers on, the prompt classifier, and the helper mode `zz` re-enters when ssh runs it as `SSH_ASKPASS` |
| `lifecycle.rs` | `RecoveredDaemon`, `DaemonRecoveryError`, `terminate_incompatible_daemon` | Single-instance identity file + guarded termination of an incompatible-protocol daemon |
| `keys.rs` | (crate-internal) `input_key_name`, `send_tokens` | tmux key spelling ↔ `KeyInput`; named-key/literal fan-out for `send-keys` |
| `status.rs` | (crate-internal) `StatusRenderer`, `status_context` | Expands the [tmux status line](/tmux/status-line.md) per client: strftime, bounded `#()` execution with an output cache, change diffing |
| `user_data.rs` | `platform_data_dir`, `restrict_to_current_user`, `restrict_directory_to_current_user` | Where user-owned application data lives and how it is permission-hardened. The policy sits here because the daemon's agent journal answers to it too; `crates/zz/src/user_data.rs` is now a re-export of this module |
| `agent/` (feature `agent`) | `AgentStreamItem`/`AgentStreamPayload` and friends via the crate root | The daemon-owned Agent runtime: `host` (one thread per pane, prompt queue, permission bookkeeping), `runtime` (the ACP connection), `fanout` (coalescing, wire sequence, replay ring, pane state), `journal`, `git_summary` (bounded current-worktree totals), `environment` (ACP child PATH repair and workspace identity), `paths`, plus a test-only in-process `fixture` |

# How the daemon runs

`Daemon::run_foreground` is the entry point (invoked by the `zz daemon` subcommand). It:

1. `prepare_socket` probes a stale endpoint (connect; if dead, `remove_file`) then
   `LocalListener::bind`; an `AddrInUse` bind maps to `DaemonError::AlreadyRunning` (single instance).
2. `restrict_socket_permissions` (Unix `0o600`), installs a `SocketGuard` (Unix `Drop` unlinks the
   socket) and a `DaemonIdentityGuard` (writes an atomic `*.identity` file for recovery). New files
   use strict `zz-daemon-identity-v2` records with PID, process start time, and protocol version;
   guarded termination still accepts v1 records that contain only PID and start time.
3. Resolves [appearance](/terminal/appearance.md) from built-in defaults (external configs are
   never read; the client's import flow owns those) and sources the zz-owned `zz/mux.conf`
   [in tmux grammar](/tmux/conf-parser.md). A fresh daemon remains empty and unarmed unless config
   creates a session. The first default Interactive attach lazily creates the next numeric session.
4. Loops on a **non-blocking** `accept()`. Unix waits in `poll(2)` for listener readiness, with a
   100 ms timeout bounding shutdown detection; Windows retains the 20 ms fallback poll. Every
   accepted stream is reset to blocking mode (required on BSD-derived Unix hosts where accepted
   sockets may inherit `O_NONBLOCK`), and a `zz-client` thread is spawned per connection until
   `stopping` is set.
5. On exit, publishes `EventPayload::ServerStopping`, logs a shutdown diagnostic snapshot, and lets
   `SocketGuard::Drop` unlink the endpoint.

The daemon binds one endpoint and only one. Since the ssh-only consolidation (2026-08-01) there is no
network listener, no discovery advertisement, and no `--listen` flag: a remote client reaches this
same socket through an ssh forward, so the daemon cannot tell a remote attach from a local one.

## When the daemon stops

`Shared::request_shutdown` sets `stopping` and unwinds the accept loop. Three things call it:

| Trigger | Condition |
|---------|-----------|
| `MuxEffect::KillServer` | Unconditional. `kill-server` stops the daemon regardless of sessions or clients. |
| `Shared::request_shutdown_if_empty` | `exit_empty_armed` **and** `state.engine.state.sessions.is_empty()` **and** `state.subscribers.is_empty()`. |
| Unix `SIGTERM` / `SIGINT` listener | Unconditional. The signal thread requests the same graceful shutdown; Windows keeps its existing behavior. |

The second condition is the change from tmux's `exit-empty`. tmux can key on sessions alone because
its last client exits at the same instant; zz's GUI client outlives its last pane, so zero sessions
must mean "show the empty workspace", not "kill the server"; otherwise closing the last pane
strands the window on a dead daemon. Requiring zero interactive clients too makes the daemon exit
when the app quits instead.

`request_shutdown_if_empty` takes the whole `&ServerState` (not only `&MuxState`) and is called from
the `execute` effect loop and `Shared::unregister`. A successful command that leaves any session
arms the guard before its effects are processed; a fresh never-had-session daemon stays unarmed. Every
actual disconnect funnels through `unregister` via `ClientRegistrationGuard::Drop` (EOF/reset,
protocol error, or panic), so that is the one place that can notice the last subscriber leaving.
`ProtocolMessage::Detach` stays inside the connection loop and does not run `unregister`.

## Reaching a remote daemon over ssh

`endpoint.rs` owns the client half. `SshForward::start` runs up to **three ssh children**, in order:

| Step | Command | Why |
|------|---------|-----|
| Probe | `ssh … <host> sh -lc '<REMOTE_SOCKET_PROBE>'` | Resolve the remote default socket path and run `zz protocol-version`. This always runs before dialing, including for an explicit `remote_socket`. Output lines carry `zz-probe-` sentinels so login-profile noise is ignored; a differing number is incompatible, `unknown` (zz present but too old) asks the user to update zz first, and `missing` (no zz on the login PATH) dials anyway. |
| Auto-start | `ssh … <host> sh -lc '<start script>'` | Start the daemon detached (`setsid`, else `nohup`), then poll for its socket (50 × 100 ms, or 5 × 1 s where `sleep` refuses a fraction). `sh -lc` because `zz` usually lives on the login shell's PATH only; a missing binary exits 127 and a timeout exits 3 |
| Forward | `ssh -N -o ExitOnForwardFailure=yes -o StreamLocalBindMask=0177 -L <local>:<remote> … <host>` | The tunnel. `wait_for_socket` polls 250 × 20 ms; `Drop` kills the child, cancels the forwarding on the master, and unlinks the local socket |

The start is unconditional rather than guarded on the socket file, because a socket outlives a
daemon that was killed and `[ -S ]` reports those corpses as healthy . the state the auto-start
exists to rescue. `zz daemon` already probes the endpoint, takes over a dead one, and answers
`AlreadyRunning` within milliseconds when the daemon really is up. The script is also a single
`;`-joined line: sshd hands it to the account's login shell before any `sh` sees it, and csh cannot
hold a newline inside single quotes. Every `<host>` argument is preceded by `--`, and a host opening
with `-` is rejected at parse time, so a destination can never reach ssh's own option parser.

Every step carries `ConnectTimeout=10` and the endpoint's `-p`/`-l` options. This mirrors local
auto-spawn: a remote host with zz installed but no daemon running now just works, which is the
2026-08-01 reversal recorded in [fleet attach](/designs/fleet-attach.md). Failures classify into
typed `EndpointError` variants (`RemoteBinaryMissing`, `RemoteDaemonUnavailable`, `ForwardExited`,
`ForwardTimeout`, …), and `ssh_reason()` turns each into the sentence the unreachable host row shows;
`ssh_failure_hint` reads ssh's stderr because ssh collapses every failure of its own into exit 255.

## Connection sharing

Every step also carries `ControlMaster=auto`, `ControlPath=<private dir>/c<hash of user+host+port>`
and `ControlPersist=60`, so the probe authenticates and the other two ride its connection. This is
not an optimisation: without it a password-protected host is asked for the same password three times
for one connect, and each rung of the reconnect ladder asks again.

Two consequences of multiplexing that the code has to know about, both measured against
OpenSSH 10.3p1:

- Against a live master, `ssh -N -L …` becomes a *forward request*: the master binds the socket,
  answers, and the requesting child **exits 0 in about 16 ms**. `wait_for_socket` therefore treats a
  finished child as success whenever the socket is there, and `Drop` runs `ssh -O cancel -L <spec>`
  as well as killing the child, because the listener belongs to the master rather than to the child
  that asked for it. Forwarded traffic is an open channel, so it keeps the master alive well past
  `ControlPersist`.
- A master that died leaves its socket behind, and ssh then only warns and gives every child its own
  handshake. `discard_dead_control_master` runs `ssh -O check` first . a local round trip that costs
  about 5 ms and never touches the network . and unlinks the path when it fails.

The control socket and the askpass socket both live in one 0700 directory created per process by
`tempfile` under `$TMPDIR` (or `/tmp` when that path is too long for `sun_path`). Both are
capabilities: whoever reaches the control socket owns the ssh session, and whoever reaches the
askpass socket can phish a password out of the dialog.

**Remaining cost:** a cold connect still runs three ssh children, now over one handshake. Folding the
probe into the auto-start exec would make it two (the start script already re-checks the socket, so
it only needs to print the path it resolved).

## Password and host-key prompts

ssh prompts on the terminal that launched the process, which a window does not have, so a
password-protected or unknown host used to hang invisibly. `askpass.rs` routes those prompts to a
dialog instead.

`SshForward::start` opens a Unix socket in the private directory and hands every ssh child
`SSH_ASKPASS=<this executable>`, `SSH_ASKPASS_REQUIRE=force` and `ZZ_ASKPASS_SOCKET=<that socket>`.
ssh runs the helper with the entire prompt as `argv[1]` and reads the answer from its stdout; `zz`
sees `ZZ_ASKPASS_SOCKET` at the top of `run` . before any argument parsing, which would otherwise
take the prompt for a command name . connects, blocks for the answer, prints it, and exits.

| Constraint | Why |
|------------|-----|
| `force`, never `prefer` | `prefer` only consults the helper when ssh has no controlling TTY, so a zz started from a terminal would prompt on that terminal |
| `BatchMode` stays unset, `StrictHostKeyChecking` stays at `ask` | either one switches the prompts off entirely, which is what the helper exists to receive |
| The helper spawns nothing | `ssh_askpass` dup2s a copy of its answer pipe onto the helper's stdout and never closes the original, so any surviving child keeps ssh blocked on that pipe forever |
| Cancel is spelled three ways | empty output with a zero exit means an empty password on the secret path, `no` for a host key, and *allow* for agent key-use confirmation |
| Confirmations are matched on `(yes/no/[fingerprint])` and on the bare `Please type 'yes', 'no' or the fingerprint` re-ask | the body of the prompt varies with `VisualHostKey`, `VerifyHostKeyDNS` and which key types are already known, and a wrong answer makes ssh re-ask forever |
| A cancel latches for the rest of the attempt | cancelling a password makes ssh send an empty one, which the server rejects and ssh re-prompts for . one cancel would otherwise cost three dialogs |

The client side then parks the host: a dismissed prompt leaves it `Unreachable` with an
"authentication was cancelled" reason and no reconnect ladder, until the user picks Reconnect.

# Threading model

The daemon is thread-per-connection with dedicated writer and per-pane watcher threads.

| Thread name | Spawned by | Job |
|-------------|-----------|-----|
| main | `run_foreground` | Non-blocking accept loop |
| `zz-client` | accept loop → `handle_connection` | Read inbound protocol messages for one client, dispatch to `execute`/`attach`/`input` |
| `zz-client-writer-{id}` | `handle_connection` | Drain that client's `OutboundMailbox` via `write_outbound`, write framed bytes to the stream |
| `zz-pane-{n}` | `watch_terminal` | Consume one `TerminalSession`'s events, revalidate actor identity, persist changed OSC titles, diff viewports, fan out |
| `zz-output-{id}` | `watch_command_output` | Stream one client's command-output-view terminal events |
| `zz-display-panes-{id}` | `watch_display_panes_timeout` | Time out one client's `display-panes` overlay |
| `zz-daemon-diagnostics` | `start_diagnostic_sampler` | Periodic state snapshot logging (only when trace logging is on) |
| `zz-daemon-status` | `start_status_sampler` | Re-render the [status line](/tmux/status-line.md) every `status-interval`, re-running its `#()` commands |
| `zz-daemon-signals` (Unix) | `DaemonSignalGuard` | Wait for `SIGTERM`/`SIGINT` or ordinary shutdown cancellation, then request the same graceful stop as `kill-server` |
| `zz-copy-pipe` | `spawn_copy_pipe` | Run a `copy-pipe` child, feed selection on stdin (bounded pool) |
| `zz-agent-{n}` | `AgentHost::open` | Block on one pane's ACP connection: adapter child stdio, prompt dispatch, permission responders, Git-summary result adoption, journal appends |
| `zz-agent-git-{pane}-{refresh}` | `AgentHost::start_git_refresh` | Capture one bounded worktree summary after session readiness, a session switch, or prompt completion; stale generation, refresh, or cwd results are discarded |
| `zz-agent-flush` | `AgentFanout::ensure_flusher` | Close 25 ms coalescing windows and hand finished frames to the publisher; parks whenever no pane has anything gathered |

That is the whole inventory. The QUIC egress accept loop, the two pairing-request threads, the
discovery-setting watcher, and the knock responder are gone with the transport that needed them.
The three agent entries appear only in a build with the `agent` feature, and only after the first
agent pane opens — the runtime is built lazily.

State is shared through one `Arc<Shared>`; all mux mutations happen under `Shared.inner`
(`Mutex<ServerState>`), and effects that touch terminals are applied **after** the lock is released
(collected into `DeferredTerminalCommand`s and run at the end of `execute`).

`Shared.status` is a **second, independent mutex** for the same reason: rendering a status line runs
`#()` child processes, and no daemon operation should queue behind one. `refresh_status` collects each
client's formats and variables under `inner`, releases it, then renders under `status`. Nothing may
take `inner` while holding `status`.

After `register` inserts an interactive client into subscriber and color-scheme state,
`handle_connection` arms a `ClientRegistrationGuard`. Its `Drop` calls `unregister`, so stream-clone,
writer-thread-spawn, protocol, and normal disconnect paths all remove partially established clients;
the normal path explicitly unregisters before closing the mailbox and joining the writer. Because
every one of those paths ends in `unregister`, the shutdown check placed there sees the last client
leave no matter how the connection died.

# Terminal frame fan-out

Every **interactive** client gets one `Arc<OutboundMailbox>`, registered in
`ServerState.subscribers`. The mailbox is a four-lane coalescing queue drained by the client's
writer thread (`recv()` priority: `reliable` → `command_output` → `agent` → `terminals`):

| Lane (`OutboundState`) | Content | Coalescing |
|------------------------|---------|-----------|
| `reliable: VecDeque<Vec<u8>>` | `ServerHello`, command responses, snapshots, non-terminal events | Never dropped; bounded at `MAX_RELIABLE_MESSAGES = 256` / `MAX_OUTBOUND_BYTES = 72 MiB` |
| `command_output: Option<Vec<u8>>` | Native command-output viewport | One coalesced slot; newest replaces stale |
| `agent: BTreeMap<PaneId, PendingAgent>` | Per-pane `AgentUpdates` batches, already coalesced into 25 ms windows by the fanout | FIFO per pane, round-robin across panes (one frame per pane per turn), capped at `MAX_PENDING_AGENT_BYTES = 4 MiB` |
| `terminals: BTreeMap<PaneId, PendingTerminal>` | Per-pane `TerminalViewport`/`TerminalPatch` frames | **One pending frame per pane**; newest replaces stale under backpressure |

The agent lane is the one lane whose overflow does **not** close the mailbox. An ACP turn bursts
hundreds of small updates and a slow client should not lose its session over it, so `enqueue_agent`
clears the pane's queued frames and puts a tiny `EventPayload::AgentLagged { pane, next_seq }` on the
reliable lane instead; the client answers with `AgentReplay` from its own cursor and the fanout
serves it out of the pane's 16 MiB replay ring, or out of the journal when the ask predates the ring.
Agent frames also follow visibility like terminal frames do: `AgentUpdates` reaches only clients the
pane is visible to (`visible_agents`, derived by `visible_agent_panes`), while the small typed
`AgentState` goes to every client attached to the session so badges and permission prompts work for a
pane nobody is watching.

A command-output watcher publishes in two phases: it validates the current terminal and captures the
subscriber under `ServerState`, encodes the potentially megabyte-scale viewport after releasing that global
lock, then reacquires it to revalidate both terminal identity and mailbox identity before installing the
already-encoded frame. A close, replacement, or unregister that wins during encoding makes the stale frame
eligible only for buffer recycling; it cannot reappear after the newer state transition.

A `zz-pane-{n}` watcher walks the pane's per-view viewports (`TerminalViewId(client.0)`, one per
attached client), diffs each against that view's previous viewport, and produces either a full
`EventPayload::TerminalViewport` or a smaller `EventPayload::TerminalPatch`. It calls
`publish_terminal_for_pane` per view, which delivers **only to a client attached to the session that
owns the pane and only while the pane is visible to that client** (`visible_terminals`, honoring its
focused window and zoom). `enqueue_terminal` validates that a patch's base generation matches the
last delivered
generation (`delivered_terminals`); a mismatch returns `NeedsFull` and the patch is promoted to a
full viewport via `replace_terminal`. Completed frame buffers are recycled (bounded pool:
`MAX_RECYCLED_FRAME_BUFFERS = 8`, `MAX_RECYCLED_FRAME_CAPACITY = 8 MiB`). Overflow on any lane closes
the mailbox rather than growing unboundedly, applying backpressure up to the terminal worker.

Layout changes call `refresh_terminal_visibility`, which `suspend_terminal`s panes that left the
visible set and seeds a fresh full viewport for panes that became visible. Suspending drops the
pane's pending frame and delivered generation but **keeps its Kitty and pasted-image ledgers**, so
switching windows and coming back does not resend image pixels the client already holds;
`cancel_terminal` is the harder form that clears those ledgers too, reserved for a pane that is
really going away.

A client that misses a frame asks for repair rather than waiting: `ProtocolMessage::RequestFull`
runs `send_full`, which re-sends the pane's current viewport when that pane is still visible to the
requester, and `ProtocolMessage::HistoryRequest` runs `send_history`, which pulls a clamped
scrollback range off the terminal actor and answers with one self-contained `HistoryChunk`.

There is one writer path: the writer thread streams every frame onto the one ordered stream, whether
that stream is a unix socket, a named pipe, or the remote end of an `ssh -L` forward. Newest-wins is
a property of the mailbox above it, not of the transport, and frames are never compressed. The
per-frame QUIC uni streams and the negotiated zstd flag were deleted at protocol v43.

# Many clients, one session

`ServerState.attached` is a `BTreeMap<SessionId, BTreeSet<ClientId>>`, so a session holds as many
interactive clients as the user has devices. Per-client maps carry the rest:

- **Views.** Each attached client owns `TerminalViewId(client.0)` on every terminal it can see;
  `attach_view`/`detach_view` follow session moves, and each view keeps its own scroll, selection,
  copy mode, and search state.
- **PTY size.** `terminal_resize_for_pane` uses the viewing client with the highest daemon-global
  terminal-input sequence; absent/equal sequences tie on the lowest `ClientId`, and a viewer without
  a geometry report falls through to the next-ranked viewer. Columns, rows, and both cell-pixel
  dimensions come from that one owner. Input only resizes panes when the computed owner changes.
- **Aggressive window size.** The inherited window flag `aggressive-resize` defaults off. ON takes
  the componentwise smallest columns and rows reported by clients whose focused window is the pane's
  window; cell pixels still come from the latest-input eligible viewer. Focus, attach, detach, and
  option changes feed the result through the existing guarded window-extent write-back and resize
  the PTY. A window with one viewer is unchanged.
- **Window focus.** `focused_windows: BTreeMap<ClientId, WindowId>` is a daemon-side overlay; pane
  focus and zoom stay shared. `stamp_snapshot_for_client` writes each subscriber's own
  `focused_window` and the session's `viewers` list into the copy it sends, so one snapshot
  computation serves every client.
- **Presence.** `snapshot_presence` turns the attached set into `SessionViewer` rows carrying the
  device name from `ClientHello` (falling back to `device-{id}`) and that viewer's focused window.
- **Steal.** `attach-session -d` runs `evict_other_clients`: every other client of the target session
  gets `EventPayload::Detached { by: Some(stealer's device name) }` and loses its attachment. A
  command-only client cannot attach but can still evict. Session teardown sends the same event with
  `by: None`.
- **View actions from elsewhere.** A `TerminalView` action from a client attached to the pane steers
  only that client's view; an unattached command client's action fans out to every attached client's
  view, and an empty target set answers `ServerError::PaneNotAttached`.
- **GUI-only work.** `publish_for_pane` broadcasts `BrowserCommand`/`TerminalUiCommand` to every
  attached client, but `request_from_gui` (the blocking request/reply behind `agent-send` and
  browser screenshots) needs one answer, so it picks the session's lowest attached `ClientId`. That
  makes routing deterministic while browser surfaces lack explicit per-client ownership.

# How mux state is hosted

`execute` takes `Shared.inner`, calls `MuxEngine::execute(context, command)`, then walks the returned
`MuxEffect`s:

| `MuxEffect` | Daemon action |
|-------------|---------------|
| `PaneCreated{Terminal}` | `TerminalSession::spawn_with_scrollback_and_appearance`, insert into `terminals`, queue `watch_terminal`, attach the attached client's view |
| `PaneCreated{Browser}` | No PTY; the pane's `BrowserDescriptor` lives in mux state, rendered by the attached GUI |
| `PaneCreated{Picker}` | No PTY or browser runtime; publish the layout leaf for the GPUI picker |
| `PaneCreated{Agent}` | Capture the cwd donor, publish the default Codex `AgentDescriptor`, and `open_agent_pane` . which builds the runtime if this is the first agent pane and starts that pane's adapter thread |
| `PaneMaterialized{Terminal/Browser/Agent}` | Spawn the terminal actor using the remembered cwd donor, retain the browser descriptor, or capture the donor cwd and open the agent runtime for the pane; the pane ID/layout leaf do not change |
| `PanesRemoved` | Drop `Arc<TerminalSession>` (worker exits), close any agent pane's runtime and lane, publish `PaneRemoved` |
| `SendKeys{pane, keys}` | `resolve_input_sinks` → `send_tokens` to terminals / `BrowserCommand::SendKeys` to the GUI |
| `FocusSidebar{pane}` | Validate the invoking interactive attachment, retire competing native surfaces, and publish `EventPayload::FocusSidebar` to that client |
| `Attach { session, detach_others }` / `Detach` | `attach` / `detach` for the interactive client; detach clears per-client attachment state without closing the connection or removing its subscriber, and the same connection can attach again. A command-only client cannot attach but still runs `evict_other_clients` when `detach_others` is set |
| `AggressiveResizeChanged { window }` | Recompute the target window, or every window for a global change, through the existing measurement write-back and queue matching PTY resizes |
| `KillServer` | `request_shutdown` . set `stopping` and unwind the accept loop, unconditionally. Emitted by the `kill-server` command, which the GUI sends on quit only when `quit-daemon-on-exit = true` |
| `SnapshotChanged` | `publish_snapshot` + refresh visibility/choose-tree/choose-buffer/display-panes overlays |

`InputMessage::{Key, Text}` from any pane kind resolves the key tables first: one `KeyEngine`
cursor per interactive client, with release-swallowing and committed-text-suppression bookkeeping,
and unbound input passing to the pane through `resolve_input_sinks` (synchronized Terminal and
Browser fanout; Picker and Agent panes have no sink and drop passed keys . an agent pane's keys are
composer input the client owns, and its prompts arrive as `AgentPrompt`, not as key events). The daemon publishes
`EventPayload::PrefixArmed` transitions of that cursor to the owning client, which uses them to
claim in-flight sequence keys from focus contexts that never reach the daemon.

`execute_key_commands` executes stored `Binding`s exactly as written — a key-bound
`split-window`/`splitw` creates a terminal like tmux, from defaults and imported configs alike.
The pane-kind picker is reached only through the zz-native `split-picker` verb, which zz's default
`%`/`"` bindings name directly.

`new-session` also emits `Attach` after its initial terminal is created; `-d` suppresses it. For an interactive client,
the daemon switches that client to the new session and publishes `ProtocolMessage::Attached`
before starting the new terminal watcher or publishing its changed snapshot; this prevents a
nonempty-but-unattached intermediate client state. `ServerHello` advertises this behavior as
`new-session-attach-v1`, allowing newer GUIs to send an explicit `attach-session` fallback to older
same-protocol persistent daemons. Startup and short-lived command clients still create sessions
only when they explicitly execute `new-session`. A default Interactive attach materializes the
next numeric session when the daemon is empty.

# Examples

```rust
// lib.rs public surface
pub use client::{CommandClient, InteractiveClient};
pub use daemon::{Daemon, DaemonError};
pub use lifecycle::{DaemonRecoveryError, RecoveredDaemon, terminate_incompatible_daemon};
pub use transport::default_socket_path;

// Run the persistent listener until `kill-server`.
Daemon::new(default_socket_path()).run_foreground()?;

// Short-lived CLI client: one request, one response.
let mut client = CommandClient::connect(&default_socket_path())?;
client.execute(CommandInvocation::new("new-session", ["-d"]))?;
let output = client.execute(CommandInvocation::new("send-keys", ["-t", "%0", "ls", "Enter"]))?;

// Interactive GPUI client: an empty target creates session 0 if the daemon is empty.
let client = InteractiveClient::connect(&path)?;
client.attach("")?;
loop { let message = client.recv()?; /* reconcile snapshot / apply terminal frames */ }
```

```text
send-keys data flow (CLI → PTY):
  CommandClient::execute("send-keys …")
    → CommandRequest over the socket
      → handle_connection → Shared::execute → MuxEngine → MuxEffect::SendKeys{pane, keys}
        → resolve_input_sinks (synchronized-input targets) → PaneSink::Terminal | PaneSink::Browser
          → DeferredTerminalCommand::SendTokens → keys::send_tokens
            → TerminalSession::send_text / send_key  (crosses into the zz-terminal worker thread)
```

# Key files

| File | Role |
|------|------|
| `crates/zz-daemon/src/lib.rs` | Crate root and public re-exports |
| `crates/zz-daemon/src/daemon.rs` | `Daemon`, `run_foreground` accept loop, `Shared`/`ServerState`, `execute`, `OutboundMailbox` fan-out, `watch_terminal`, `attach`/`detach`, `input_*` routing |
| `crates/zz-daemon/src/transport.rs` | `LocalListener`/`LocalStream` over `interprocess`, blocking accepted-stream normalization, `default_socket_path`, `PeerCredentials` |
| `crates/zz-daemon/src/client.rs` | `CommandClient`, `InteractiveClient`, framed `ProtocolSender`/`ProtocolReceiver`, `connect`/`connect_endpoint` handshake, `short_device_name` |
| `crates/zz-daemon/src/endpoint.rs` | `Endpoint`/`SshEndpoint` parsing, the probe/auto-start/forward ssh commands and their shell quoting, `SshForward`'s RAII child, and the `EndpointError` → host-row advice mapping |
| `crates/zz-daemon/src/paths.rs` | Platform discovery of the zz-owned `zz/mux.conf` and its write path |
| `crates/zz-daemon/src/status.rs` | `StatusRenderer`: strftime, bounded `#()` execution with an output cache, and per-client status diffing. See [status line](/tmux/status-line.md). |
| `crates/zz-daemon/src/lifecycle.rs` | `DaemonIdentityGuard`, `terminate_incompatible_daemon`, identity-file + guarded shutdown |
| `crates/zz-daemon/src/keys.rs` | `input_key_name` (KeyInput → tmux spelling), `send_tokens` (named-key/literal fan-out) |
| `crates/zz-daemon/src/user_data.rs` | `platform_data_dir` per OS and the Unix `0o600`/`0o700` hardening helpers, shared with the GUI |
| `crates/zz-daemon/src/agent/host.rs` | `AgentHost`, `AgentPaneSpec`, `AgentPaneState`, `HostCommand`, and the per-pane `PanePump` |
| `crates/zz-daemon/src/agent/runtime.rs` | `run_agent_runtime` / `run_agent_connection`, the ACP client role, auto-approve (`is_user_question`, `preferred_allow_option`), `StderrTail`, and `load_persistent_journal` |
| `crates/zz-daemon/src/agent/fanout.rs` | `AgentRuntime` (what the daemon holds), the `AgentPublisher` trait, per-pane coalescing, wire sequencing, the replay ring, `AgentPaneWire` derivation, and first-prompt pane titles |
| `crates/zz-daemon/src/agent/journal.rs` | Per-ACP-session JSONL append/replay/prune, session-ID jailing, the 32 MiB cap |
| `crates/zz-daemon/src/agent/git_summary.rs` | Bounded branch and current-worktree file/addition/deletion capture through a throwaway Git index |
| `crates/zz-daemon/src/agent/environment.rs` | ACP child `PATH` repair (login shell + version-manager bins), `warm_adapter_cache`, workspace-identity injection |
| `crates/zz-daemon/Cargo.toml` | `default = ["daemon", "agent"]`. The daemon feature uses `async-signal`, `async-channel`, and `futures-lite` only to give the Unix signal-listener thread cancellable blocking; the core server remains thread-per-connection with no shared async runtime. The `agent` feature adds `agent-client-protocol`, `serde`/`serde_json`, and `base64`, and runs its pane threads on `futures-lite`'s `block_on` rather than any shared runtime. Clients that never render a transcript (`zz-tui`, `zz-client-ffi`) depend on this crate with `default-features = false`. |

# Related

- Serves and consumes [the wire protocol](/crates/zz-protocol.md) and its [framing](/protocol/wire-protocol.md).
- Hosts [the mux state machine](/crates/zz-mux.md) (`MuxEngine`, `MuxEffect`, key tables).
- Owns each pane's [PTY-backed terminal session](/crates/zz-terminal.md); see the
  [PTY worker model](/concepts/pty-worker.md) for the ownership boundary.
- [Session persistence](/concepts/session-persistence.md) . detach/attach, what survives, transport per platform.
- [Terminal frame](/concepts/terminal-frame.md) . the coalesced per-pane fan-out unit.
- The [GPUI app](/crates/zz.md) is the single interactive client; the CLI subcommands are short-lived clients.
- System context: [architecture overview](/architecture/overview.md), [process model](/architecture/process-model.md),
  [data flow](/architecture/data-flow.md).
- The compact fan-out encoding: [packed terminal lanes](/protocol/terminal-lanes.md).
