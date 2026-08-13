---
type: Concept
title: Session persistence & daemon lifecycle
description: Why mux state and terminals outlive GPUI windows, which GUI state is disk-backed, which browser/Agent descriptors can restore, and how attach, detach, eviction, per-client state, and local transport work when several devices share one session.
resource: crates/zz-daemon/src/daemon.rs
tags: [daemon, persistence, attach, detach, transport, lifecycle, multi-client]
timestamp: 2026-08-01T00:00:00Z
---

# Overview

Session persistence is the property that makes zz a multiplexer rather than a terminal window: the
[zz-daemon daemon](/crates/zz-daemon.md) is the **sole authority for mux state** and owns every terminal
pane's [PTY worker](/concepts/pty-worker.md), so sessions, windows, binary split-pane layouts, and
the processes running inside them survive after every GPUI window is closed. Closing the last window
merely **detaches** that client; it never stops the terminals, and it never disturbs another device
attached to the same session.

For mux sessions, persistence means daemon-lifetime, in-memory persistence. There is **no on-disk
session restoration**: a daemon crash, `kill-server`, logout, or reboot destroys PTYs, and a fresh
daemon starts empty. Client-owned state such as browser storage, recent pages, Agent choices, and
main-window bounds is separately persisted to disk as called out below.

# Daemon lifecycle

The daemon is a single long-lived process per user, started on demand by the GUI or by any
state-creating CLI command, and bound to one owner-only endpoint. On Unix the auto-spawned daemon
is detached into its own process group (`process_group(0)` in `spawn_daemon`, `crates/zz/src/lib.rs`),
so terminal signals aimed at the launching client (Ctrl+C, tty hangup) never reach the daemon:

| Phase | Mechanism |
|-------|-----------|
| Start | `Daemon::run_foreground` → `prepare_socket` (probe/remove stale endpoint) → `LocalListener::bind` |
| Single instance | A live endpoint makes `bind` return `AddrInUse` → `DaemonError::AlreadyRunning`; a dead one is unlinked and rebound |
| Owner-only | `restrict_socket_permissions` sets Unix `0o600`; the Windows pipe is user-SID scoped |
| Stay alive | Serves connections until `request_shutdown` sets `stopping`; **outlives GUI detach** while sessions exist |
| Stop | `kill-server` (never auto-starts a daemon), or nothing-left-to-serve: zero sessions **and** zero interactive clients (`subscribers`), armed only after startup seeds the default session; `SocketGuard::Drop` unlinks the Unix socket |
| Recover | `terminate_incompatible_daemon` safely stops a daemon whose protocol version no longer matches |

zz deliberately diverges from tmux's `exit-empty` here. tmux can key on sessions alone because its
last client exits at the same instant, dropping the user back in a shell; zz's GUI window outlives
its last pane. Keying on sessions alone therefore meant closing the last pane killed the daemon and
left the window on an unrecoverable "zz daemon stopped" screen. Now:

- **Closing the last pane** leaves the daemon up with zero sessions and reveals the existing
  `NewSessionView` **New Session** card. That is the steady state of an empty daemon, not a
  teardown race.
- **Quitting the app** drops the last subscriber, so the daemon exits then, provided no sessions
  remain.
- **Live sessions still win.** A GUI quit with sessions open leaves the daemon and its PTYs running,
  preserving the multiplexer guarantee that sessions are there on relaunch.
- The check lives in `Shared::request_shutdown_if_empty` and is called from `initialize`, the
  `execute` effect loop, and `Shared::unregister` (the funnel every disconnect path reaches through
  `ClientRegistrationGuard::Drop`).

The client-local `quit-daemon-on-exit = true` opts out: app quit then sends `kill-server` instead of
`detach()`, stopping the daemon even with live sessions. It defaults to `false`.

`MuxClient` also clears its client handle when its reader loop ends, so `is_connected()` no longer
reports a live daemon over a dead socket and action buttons cannot stay enabled while their writes
silently fail.

On macOS, the GUI changes its AppKit activation policy to `prohibited` during `on_app_quit` before
the process exits. This keeps the intentionally persistent daemon and its PTYs alive without leaving
zz's background-activity indicator in the Dock; macOS may still report that activity in its system
background-app UI.

## Guarded recovery of an incompatible daemon

When a client handshakes with a daemon speaking a different `PROTOCOL_VERSION`, `zz-daemon` can stop
the old one before rebinding. This is deliberately paranoid so it never kills an unrelated process:

- `Daemon::run_foreground` installs a `DaemonIdentityGuard` that atomically writes a private
  `<socket>.identity` file (magic `zz-daemon-identity-v1`, `pid` + process `start_time`, Unix mode
  `0o600`).
- `terminate_incompatible_daemon` cross-checks the **kernel-reported socket peer PID**
  (`LocalStream::peer_credentials`), the identity file, the process owner, and the executable/command
  line (`zz … daemon`) and start time before sending a single non-escalating `SIGTERM` (Unix) or
  kill (Windows). It refuses on any mismatch (`DaemonRecoveryError::UnsafeTarget`) and verifies the
  endpoint was not replaced mid-shutdown (`EndpointReplaced`).
- Cargo rebuilds replace `target/debug/zz` atomically, so Unix may report a still-running daemon's
  executable basename as exactly `zz (deleted)`. Recovery accepts that one name while still requiring
  the socket peer PID, private identity PID/start time, owner, and `daemon` command-line checks; missing
  executable metadata and deleted-name lookalikes remain unsafe.

# Attach and detach

A session holds as many interactive clients as the user has devices: `attached` is a
`BTreeMap<SessionId, BTreeSet<ClientId>>`. The constraint runs the other way: a client attaches to
at most one session, and attaching to a second one detaches it from the first. Command-only CLI
clients connect concurrently and never attach. The GUI is a long-lived subscribed client; the CLI
sends a request and disconnects.

| Step | Server action (`daemon.rs`) |
|------|-----------------------------|
| Connect | `handle_connection` reads `ClientHello` (protocol version plus an optional `device_name`), `validate_hello` checks the version |
| Register | `register` mints a `ClientId`, records the device name, adds interactive clients to `subscribers` (their `OutboundMailbox`), returns `ServerHello` + capabilities |
| Attach | `Attach{session}` → `attach_target` → `attach`: inserts the client into `attached[session]` and removes it from every other session's set, seeds `visible_terminals`, calls `TerminalSession::attach_view(TerminalViewId(client.0))` on every terminal in the session, recomputes PTY sizes, and returns a `MuxSnapshot` stamped for that client; then a full resync of visible panes |
| Steal | `attach-session -d` attaches, then `evict_other_clients` sends each peer `EventPayload::Detached { session, by: Some(device) }` and tears its per-client state down. A command-only client cannot attach, but its `-d` still evicts |
| Detach | `Detach` → `detach` → `detach_client_state`: clears the client's attachment, visible terminals, focus, geometry, overlays, key state, and in-flight GUI requests, then calls `TerminalSession::detach_view` for its views, **keeping the connection, subscriber, and `Arc<TerminalSession>` alive** for a later `Attach` |
| Disconnect | EOF/reset, a protocol error, or a panic drops `ClientRegistrationGuard` → `unregister`: performs the detach cleanup, removes the client from `subscribers`, and permanently releases its terminal views |
| Session destroyed | `detach_removed_sessions` runs at the head of every snapshot publish and sends `Detached { session, by: None }` to the clients of a session that no longer exists, so `kill-session` leaves no stale attachment behind |

Because the daemon holds the terminal `Arc`, `detach_view` only removes that client's view of the
pane; the PTY worker keeps running. Reattaching seeds a fresh snapshot and full terminal viewports,
so output produced while detached is present.

A client that receives `Detached` for the session it is attached to clears its attachment and shows
a notice naming the device that took it, or "session ended" when `by` is `None`.

## What each client owns alone

Attaching more devices forks what each person looks through. The session underneath stays single.

| Per client | Shared by the session |
|------------|-----------------------|
| Terminal view state under `TerminalViewId(client.0)`: scroll anchor, selection, copy mode, search | Layout tree, pane focus, zoom |
| Focused window (`focused_windows: BTreeMap<ClientId, WindowId>`, falling back to `session.active_window` when the entry names a window that is gone) | `session.active_window`, window and pane names |
| Reported pane geometry, key engine and prefix state, command prompt, choosers, command output | The PTY, its scrollback, and every process inside it |

PTY size is arbitrated rather than last-writer-wins. `terminal_resize_for_pane` takes `min(columns)`
and `min(rows)` across the clients **currently viewing** the pane, breaking ties on the lowest
`ClientId`, and recomputes on a resize report, a visibility change, an attach, and a detach.
Navigating the laptop to another window drops it out of that set, and the pane re-expands for the
desktop still watching.

Presence rides the snapshot. `snapshot_presence` turns the attached set into
`SessionViewer { name, window, is_self }` rows named from `ClientHello.device_name` (`device-{id}`
when a client sent none), and `stamp_snapshot_for_client` writes each subscriber's own
`focused_window` and viewer list into the copy it receives, so one snapshot computation serves every
client. A view action from an attached client steers that client's view; one from an unattached
command client fans out to every attached client's view of the pane, because a CLI caller has no view
of its own.

## Involuntary detach and roaming

A network drop is an involuntary detach, and the client hides it. A remote host whose ssh forward
dies moves to `HostState::Reconnecting { attempt }`: the last snapshot and terminal frames
stay on screen, `MuxClient` redials on a 1/2/4/8/16/30-second backoff, and a successful dial
re-attaches the same session, or the default session when the daemon no longer has it. The daemon
sees an ordinary disconnect followed by an ordinary attach, so PTYs, scrollback, and the other
devices attached to that session never notice.

# What is persisted vs transient

| Category | Survives GUI detach? | Survives daemon exit/crash? | Notes |
|----------|:---:|:---:|-------|
| Sessions / windows / layout trees / pane IDs | Yes | No | Held in `ServerState.engine` (`MuxEngine`); IDs are monotonic and never reused |
| Session/window names + last pane titles | Yes | No | Explicit names and the latest terminal OSC/browser document titles live in mux state and are included in attach snapshots |
| Terminal PTY process + scrollback + screen | Yes | No | Owned by the [zz-terminal worker](/concepts/pty-worker.md); the daemon holds the `Arc<TerminalSession>` |
| Key bindings, options, mode-keys, prefix | Yes | No | Parsed from the zz-owned `zz/mux.conf` [in tmux grammar](/tmux/conf-parser.md) at startup, mutable via commands |
| Paste buffers, command-prompt history | Yes | No | `ServerState.paste_buffers`, `command_history` |
| Browser pane last URL + named zz profile | Yes | No | Stored as `BrowserDescriptor` in mux state; profile switches update the descriptor. Panes created without a URL carry `about:blank` and render the client's recently-visited empty state |
| Named profile cookies/cache/local storage | Yes | **Yes** | CEF persists each profile in a separate zz-owned immediate child of the browser root; explicit source-profile import writes normalized Chrome cookies into the current zz store without mounting Chrome's request context |
| App-owned browser history list | Yes | **Yes** | Client-side, not daemon state: the app persists up to 5,000 live or imported Chrome visits in `<data dir>/zz/browser/recent-pages` (see [browser profile](/browser/profile.md)) |
| Main-window bounds and mode | **Yes** | **Yes** | Client-side, not daemon state: bounded `<data dir>/zz/window-state.json` remembers size, position, display, and windowed/maximized/full-screen mode; restore clamps to a usable current display |
| Chromium renderer runtime (JS heap, scroll, unsubmitted forms, media) | **No** | No | Not kept alive with no GUI attached; on reattach the GUI builds a fresh CEF instance from the descriptor |
| Agent provider + cwd + opaque ACP session ID | Yes | No | Stored as `AgentDescriptor` in mux state; the replacement GUI launches that provider and passes the metadata to `session/load` when supported |
| ACP processes and reduced conversation state | **No** | No | GUI-owned per pane; children stop on GUI shutdown and each provider is responsible for history replay from its session ID |
| Terminal frames in flight | No | No | Coalesced per-pane in each client's `OutboundMailbox`; newest replaces stale |

Browser panes are the key asymmetry: the daemon persists only the **restorable descriptor**, not the
live browser. Live browser input while detached returns `PaneNotAttached` and is not queued for
replay. See [browser lifecycle](/browser/lifecycle.md) and [browser profile](/browser/profile.md).

Agent panes follow the same descriptor/runtime split. The daemon retains the selected provider,
absolute working directory, and opaque ACP session ID, while `AgentController`, streamed entries,
pending approvals, and the pane-local child process are GUI-owned. Reattach reconstructs the
timeline only if that provider supports `session/load` and replays it; otherwise zz creates a fresh
session and updates the descriptor. See [Native Agent pane](/concepts/agent-pane.md).

# Transport per platform

`transport.rs` wraps the `interprocess` crate into `LocalListener`/`LocalStream` behind one
interface (connect, bind, accept, framed read/write, peer-credential capture). Endpoints are
owner-only; `default_socket_path` resolves per platform:

| Platform | Endpoint (`default_socket_path`) | Kind |
|----------|----------------------------------|------|
| Linux | `$XDG_RUNTIME_DIR/zz/default.sock` (falls back to `$TMPDIR/zz-$USER/default.sock`) | Unix-domain socket |
| macOS | owner-only directory beneath `$TMPDIR` | Unix-domain socket |
| Windows | `\\.\pipe\zz-{sanitized-username}-default` | Named pipe |

A remote daemon adds no fourth kind. `ssh://[user@]host[:port]` forwards that host's own
`default_socket_path` to a local scratch socket, so the client is still talking to a Unix socket and
the envelope is unchanged. See [wire protocol](/protocol/wire-protocol.md) and
[fleet attach](/designs/fleet-attach.md).

The accept loop itself is nonblocking, but each accepted stream is explicitly restored to blocking
mode before protocol handling. That normalization is needed on BSD-derived Unix hosts such as macOS,
where an accepted socket may otherwise inherit `O_NONBLOCK` and surface a normal empty read as
`WouldBlock`/`EAGAIN`.

Peer identity is validated via `LocalStream::peer_credentials` (`PeerCredentials { pid, euid }` on
Unix; PID on Windows). A protocol-version mismatch, invalid framing, oversized frame, or bad enum
value closes only that connection with a typed error, so **one malformed client never takes down the
daemon** (`handle_connection` treats `UnexpectedEof`/`ConnectionReset`/`BrokenPipe` as a clean
disconnect).

# Examples

The requirement this satisfies: closing every GPUI window must leave the PTYs running, and
reattaching must show everything they produced while nothing was attached.

```text
Detach without stopping work:
  1. Split terminals, run `cargo test` in one pane.
  2. Close every GPUI window  → InteractiveClient drops → unregister → detach
                              → detach_view(client), PTY workers keep running.
  3. Reopen zz → InteractiveClient::connect + attach → fresh MuxSnapshot
                              → attach_view + full viewports → test output is intact.
```

```rust
// The interactive client detach path (client.rs).
pub fn detach(&self) -> Result<(), DaemonError> {
    self.send(&ProtocolMessage::Detach)
}
// Simply dropping the connection is equivalent: the server's read loop hits EOF
// and runs `unregister` → `detach`, which keeps every Arc<TerminalSession> alive.
```

# Related

- [zz-daemon crate](/crates/zz-daemon.md) . the daemon that implements all of this.
- [PTY worker model](/concepts/pty-worker.md) . why terminals keep running while detached.
- [Terminal frame](/concepts/terminal-frame.md) . the coalesced unit resynced on attach.
- [Split-pane layout](/concepts/split-pane-layout.md) . the persisted layout trees.
- [Wire protocol](/protocol/wire-protocol.md) . `Attach`/`Detach`/`ServerHello`, protocol versioning.
- Browser side: [browser lifecycle](/browser/lifecycle.md), [browser profile](/browser/profile.md).
- Running the daemon: [running zz](/playbooks/running-zz.md).
