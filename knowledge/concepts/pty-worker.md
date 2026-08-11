---
type: Concept
title: Daemon-owned PTY worker model
description: How the daemon spawns and owns one PTY-backed terminal session per pane, the thread/ownership boundary between zz-daemon and the zz-terminal worker, and the paths that carry terminal frames out and send-keys in.
resource: crates/zz-daemon/src/daemon.rs
tags: [pty, daemon, terminal, threading, send-keys]
timestamp: 2026-08-06T00:00:00Z
---

# Overview

Every terminal pane is backed by a **daemon-owned PTY worker**. When [the mux state
machine](/crates/zz-mux.md) creates a terminal pane, [zz-daemon](/crates/zz-daemon.md) spawns a
`TerminalSession` from [zz-terminal](/crates/zz-terminal.md) and keeps its handle in
`ServerState.terminals: BTreeMap<PaneId, Arc<TerminalSession>>`. Each `TerminalSession` starts its
own OS thread (`zz-terminal`) that exclusively owns the PTY child process and every `libghostty-vt`
object; the daemon never touches the PTY directly. This split is what lets terminals keep running
across GUI detach (see [session persistence](/concepts/session-persistence.md)) and keeps parsing off
the daemon's connection and fan-out threads.

The boundary is a **message-passing actor**. The daemon's terminal map holds the owning `Arc` handle
and talks to the worker over channels: commands in (`send_text`, `send_key`, `view_action`, resize),
events out (`TerminalEvents`). The dedicated watcher holds a cloned event stream plus a
`Weak<TerminalSession>`; it upgrades only after an event arrives, so watching cannot extend a pane's
lifetime. Terminal frames are pulled per view from the worker's published viewports and fanned out to
each attached client; input and `send-keys` are pushed into the worker as commands.

# The ownership boundary

| Side | Owns | Thread |
|------|------|--------|
| `zz-daemon` (daemon) | `Arc<TerminalSession>` handle per pane, mux state, per-client `OutboundMailbox`, view attach/detach | `zz-client`, `zz-client-writer-{id}`, `zz-pane-{n}` |
| `zz-terminal` (zz-terminal) | The PTY child process, `libghostty-vt` grid/parser, scrollback, the authoritative `TerminalViewport` per attached view | `zz-terminal` worker (one per session) |

`TerminalSession` (in `crates/zz-terminal/src/session.rs`) is a thin handle over channels:

```rust
pub struct TerminalSession {
    commands: CommandSender,                 // daemon → worker (input, resize, view actions)
    events: TerminalEvents,                  // worker → daemon (ViewportReady, CopyReady, OpenUri…)
    latest: Arc<RwLock<PublishedViewports>>, // newest frame per view, plus a viewless fallback
    max_scrollback: usize,
    word_separators: RwLock<WordSeparators>,
}

struct PublishedViewports {
    fallback: Arc<TerminalViewport>,
    by_view: HashMap<TerminalViewId, Arc<TerminalViewport>>,
}
```

One publish carries one frame per active view. `publish_active_views` restores each view's scroll
anchor, selection, copy mode, and search state before snapshotting it, then
`Publisher::publish_viewports` swaps the whole `by_view` map in and coalesces a single
`ViewportReady`. A session with no active views takes `Publisher::publish` instead, which clears
`by_view` and stores only the fallback. Readers name a view with `latest_viewport_for(view)`, take
the whole map with `latest_viewports()`, or read the fallback with `latest_viewport()`. Details of
the snapshot itself live in [terminal frame](/concepts/terminal-frame.md).

`spawn_with_scrollback_and_appearance` starts the `zz-terminal` thread running `terminal_worker`,
which owns the PTY and libghostty for the life of the pane. The daemon interacts only through the
handle's methods; it never shares PTY or grid memory across the boundary.

# Spawning a PTY worker

Terminal spawns are driven by mux effects while `Shared::execute` holds the state lock; actor
commands and watcher startup are deferred until after that lock is released:

```text
MuxEffect::PaneCreated|PaneMaterialized { pane, kind: Terminal, inherit_cwd_from }
  → history_limit_for_pane / word_separators_for_pane        (mux options)
  → resolve the donor shell's live cwd when this is a split
  → TerminalSession::spawn_with_scrollback_and_appearance[_in](limit, appearance, cwd)
  → ServerState.terminals.insert(pane, Arc::clone(&session))
  → DeferredTerminalCommand::AttachView { view: TerminalViewId(client.0) }  (one per attached client)
  → watch_terminal(pane, &session)                           (spawns the zz-pane-{n} watcher)
```

A session holds a **set** of attached clients (`attached: BTreeMap<SessionId, BTreeSet<ClientId>>`), so
a new pane attaches one view per client already reading that session. Each view costs a scroll anchor,
a selection, and copy-mode/search state; the PTY behind them stays single.

A terminal `split-window` automatically sets `inherit_cwd_from` to its target pane. The terminal
actor exposes the live foreground process ID, queried from the PTY at call time (`tcgetpgrp` on a
dup of the master on Unix, with the shell child as a fallback and nothing cached in between);
`zz-daemon` resolves that process's current directory and passes it to the new PTY's
`CommandBuilder`. This also accepts tmux's
`split-window -c "#{pane_current_path}"` form, so existing bindings keep working. If the donor is not
a live terminal, process inspection fails, or the directory no longer exists, spawning falls back
to the normal default-shell directory instead of failing the split.

Before spawning the default shell, zz-terminal installs zz-owned title hooks for zsh and modern
Bash. The embedded scripts are materialized into a versioned user-private cache (directories `0700`,
files `0600`). zsh is bootstrapped through a temporary `ZDOTDIR` that restores and sources the
user's real `.zshenv`; Bash enters POSIX mode long enough to source an `ENV` bridge, then restores
ordinary login startup. The hooks emit OSC 2 with the exact interactive command at pre-exec and a
home-abbreviated working directory at each prompt, so the live viewport title (and therefore the
sidebar pane label) describes `npm run dev` while that command owns the PTY. Unsupported shells and
macOS's legacy `/bin/bash` use the unchanged default-shell path. `ZZ_SHELL_INTEGRATION=none` disables
the injection without preventing applications from publishing OSC 0/2 themselves.

At key-execution time, the daemon routes any configured binding whose canonical command is
`split-window` to `new-pane`, retaining its target, axis, size, and cwd arguments. The configured
prefix and key remain untouched. `new-pane` creates `PaneKind::Picker` with no `TerminalSession` and
remembers the donor pane only so a later Terminal selection can inherit the same live cwd.
`PaneMaterialized{Terminal}` follows the spawn pipeline above, while a Browser selection creates no
daemon-side runtime. Direct command requests for `split-window` remain terminal-only.

`MuxEffect::PanesRemoved` removes the map's owning `Arc<TerminalSession>`, which closes the command
side and shuts down the worker even when the shell is idle. The event sender then closes and wakes the
non-owning watcher so it can exit. The daemon also publishes `PaneRemoved`. Browser panes create
**no** PTY worker; their state is a `BrowserDescriptor` rendered by the attached GUI.

# Terminal frame fan-out (worker → clients)

Each pane gets a `zz-pane-{n}` watcher thread created by `watch_terminal`. It blocks on the worker's
event channel without holding a strong session reference. After each event it upgrades its weak
handle, verifies the pane still maps to that exact session, and then handles the event. One pane runs
one diff stream per view. The watcher keeps `previous: BTreeMap<TerminalViewId, Arc<TerminalViewport>>`
and, on every `TerminalEvent::ViewportReady`, walks `latest_viewports()` in view order, diffing each
frame against that view's own predecessor to emit either a full frame or a compact patch:

```rust
// watch_terminal, per ViewportReady
for (view, viewport) in current {                     // latest_viewports(), sorted by view id
    let payload = previous
        .get(&view)
        .and_then(|prev| TerminalViewport::diff_with_scratch(prev, &viewport, &mut diff_scratch))
        .map_or_else(|| TerminalFanout::Full, TerminalFanout::Patch);
    shared.publish_terminal_for_pane(pane, ClientId(view.0), payload, &viewport);
    previous.insert(view, viewport);
}
previous.retain(|view, _| active.contains(view));     // a view that went away drops its diff base
```

Title sync and exit detection ride whichever frames exist. Each frame's title goes through
`synchronize_pane_title`, and an `Exited` status on any of them closes the pane through
`close_exited_terminal`. When `latest_viewports()` comes back empty the session has no attached views,
so the watcher reads `latest_viewport()` instead and runs both checks off that fallback. A pane whose
last viewer detached keeps its sidebar label current and still closes when its shell exits.

`publish_terminal_for_pane` takes the view's `ClientId` and delivers only while that client is still
attached to the pane's session and the pane is currently **visible** to it (`visible_terminals`, keyed
per client, honoring active window + zoom). The frame lands in that client's `OutboundMailbox`
terminal lane (one coalesced pending frame per pane), where the watcher's `is_current_terminal` guard
and the mailbox's generation check (`delivered_terminals`) promote a stale-base patch back to a full
viewport. Full details of the mailbox lanes and backpressure live in
[terminal frame](/concepts/terminal-frame.md) and [the server crate](/crates/zz-daemon.md). The
watcher also forwards `CopyReady` (paste buffers / copy-pipe / clipboard) and `OpenUri` events, both
of which carry the originating view and route to `ClientId(view.0)`.

# Send-keys and interactive input (clients → worker)

Both CLI `send-keys` and interactive keystrokes converge on `TerminalSession::send_text` /
`send_key`, which enqueue `Command`s to the worker. The daemon resolves **synchronized-input
targets** first, so one keystroke can fan out to several panes:

| Entry | Path |
|-------|------|
| CLI `send-keys` | `CommandRequest` → `execute` → `MuxEffect::SendKeys{pane, keys}` → `resolve_input_sinks` → `DeferredTerminalCommand::SendTokens` → `keys::send_tokens` → `send_text`/`send_key` |
| Terminal interactive text | `InputMessage::Text` → `input_text` → key engine (`key_decision`, prefix tables) → `dispatch_input_text` → `input_sinks` → `send_text` |
| Terminal interactive key | `InputMessage::Key` → `input_key` → `key_decision` → `input_sinks` → `send_key` |
| Browser page input | same `input_text`/`input_key` path; `Pass` decisions reach the Browser sink |

`resolve_input_sinks` maps each target pane to a `PaneSink::Terminal(Arc<TerminalSession>)` or
`PaneSink::Browser(PaneId)`; browser targets become `EventPayload::BrowserCommand` events sent to the
attached GUI (they need a live CEF instance) rather than PTY writes. `send_tokens` (`keys.rs`)
translates tmux key spellings into `KeyInput` (`named_key`) or literal text and fans them to every
terminal sink, cloning the input for all but the **last** sink so single-target input allocates
nothing extra. CLI `send-keys` writes pane input directly and does **not** traverse the attached
client's key tables.

# Interaction & command-output surfaces

- **Views** are per-client: `attach_view` / `detach_view` / `release_view` on a `TerminalSession`
  are keyed by `TerminalViewId(client.0)`, so selection, copy-mode, and search are per-viewer without
  duplicating the PTY. `Shared::attach` attaches that view on every terminal the target session owns
  and detaches it from the session the client is leaving, so two devices reading one pane hold two
  scroll anchors over one shell. `view_action` carries copy-mode and history actions into the worker.
- **Command output** (`C-b :` results, interactive command output) uses a **PTY-free**
  `TerminalSession::spawn_output_view_with_appearance`, a frozen terminal surface with no child
  process, so the same rendering and copy-mode machinery works without a shell.

# Related

- [zz-daemon crate](/crates/zz-daemon.md) . spawns, watches, and fans out these workers.
- [zz-terminal](/crates/zz-terminal.md) . defines `TerminalSession`, the worker thread, libghostty ownership.
- [Terminal frame](/concepts/terminal-frame.md) . the coalesced fan-out unit and mailbox lanes.
- [Session persistence](/concepts/session-persistence.md) . why the worker outlives GUI detach.
- [Terminal interaction](/terminal/interaction.md) and [libghostty-vt](/terminal/libghostty-vt.md) . inside the worker.
- Input types (`KeyInput`, `KeyCode`, `Modifiers`) and `KeyToken` come from
  [zz-terminal](/crates/zz-terminal.md) and [zz-protocol](/crates/zz-protocol.md).
