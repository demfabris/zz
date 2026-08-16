---
type: Architecture
title: End-to-end data flow
description: How terminal frames, browser pixels, ACP updates, and user input move among the daemon, GUI, PTY workers, CEF, and agents.
resource: crates/zz-daemon/src/daemon.rs
tags: [architecture, data-flow, frames, input, rendering]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

Three rendering paths coexist in a GUI client. The daemon produces terminal content and streams it
as renderer-neutral frames; CEF produces browser content locally; the daemon also runs the ACP
adapter and streams its raw items, which the GUI reduces into Agent conversation entries. Browser
pixels never cross the zz daemon protocol; ACP payloads do, since v53, as opaque JSON blobs on their
own lane.

# Terminal path (daemon → client)

```
PTY child ──bytes──▶ libghostty-vt (worker thread)  [zz-terminal]
        │
        ▼ one immutable renderer-neutral frame per active client view
   server frame fanout  [server]
        │  packed terminal lanes over the wire protocol
        ▼
   mux_client  [app] ──▶ terminal_element paints in GPUI
```

- The worker thread owns the PTY and every libghostty object and publishes one immutable
  [terminal frame](/concepts/terminal-frame.md) per active view on each update. A view is a client:
  each attached client holds `TerminalViewId(client.0)` with its own scroll anchor, selection, copy
  mode, and search, so two devices read one pane at two scroll positions.
- The [server](/crates/zz-daemon.md) diffs every (pane, view) stream against that view's previous
  frame and sends the patch or a full viewport to that view's client alone, encoded as
  [packed terminal lanes](/protocol/terminal-lanes.md) inside the
  [wire protocol](/protocol/wire-protocol.md).
- Every frame is delivered reliably and in order, local socket or ssh forward alike, and uncompressed.
  Backpressure is handled a step earlier, in the daemon's outbound mailbox: one pending frame per
  pane, newest replacing stale, so a slow client converges on the latest frame rather than draining a
  backlog. A client that cannot apply a patch repairs one pane at a time with `RequestFull { pane }`.
- The [app](/crates/zz.md) reconciles panes by [ID](/protocol/ids.md), keeping terminal entities
  stable, and paints frames with a custom GPUI element aiming for
  [Zed rendering parity](/terminal/rendering-parity.md). Rows the grid scrolls away land in that
  pane's client-side `HistoryRing`, backfilled deeper by `HistoryRequest`/`HistoryChunk`, which is
  what makes scrolling back a local read.

# Browser path (local to the GUI process)

```
CEF renderer ──shared GPU texture or BGRA──▶ one-slot mailbox (latest only)  [zz-browser]
        │
        ▼
   browser_element paints the latest frame (external_texture / paint_surface / D3D11 / BGRA)  [zz]
```

- CEF Alloy OSR publishes into a **one-slot mailbox**; only the latest frame is kept. The default
  tiers are Linux wgpu `external_texture`, macOS Metal-IOSurface, and Windows D3D11; owned BGRA
  readback is the fallback. See [OSR rendering](/browser/osr-rendering.md).
- The daemon never transports browser video frames; the browser lives entirely in the
  GUI process. The daemon only persists a browser pane's tab list + [profile](/browser/profile.md).

# Agent path (through the daemon, since v53)

```text
composer ──AgentPrompt───────▶ daemon agent host ──session/prompt──▶ ACP child   [daemon]
   ▲                                  │
   │                                  ├──journal append (JSONL, per ACP session)
   │                                  ▼
   │                          fanout: 25 ms coalesce, wire seq, 16 MiB replay ring
   │                                  │
   │  AgentUpdates (JSON items) + AgentState                                     [agent lane]
   ▼                                  ▼
AgentController reducer ──────▶ AgentTimeline + controls                         [client]
```

- The daemon owns one adapter child per `PaneId`, on its own thread, and reduces nothing: it
  journals the raw items and fans them out. `AgentController` is the reducer, rebuilding messages,
  thoughts, plans, tool calls, usage, title, available commands, and configuration from the stream
  it replays and tails.
- A replay deliberately overlaps the live tail; the client's per-pane sequence cursor is what makes
  that idempotent. A lane overflow sends `AgentLagged` rather than closing the connection, and the
  client answers with `AgentReplay` from its cursor.
- Slash completion consumes `AvailableCommandsUpdate` with one `/` sigil for every provider.
  Permission mode, model, and effort controls send the agent's exact config-option IDs and values as
  `AgentSetConfigOption`/`AgentSetMode`, falling back to legacy ACP session modes when generic config
  options are unavailable.
- Permission requests park in the daemon and ride `AgentState` out to every client on the session,
  so a late-attaching client sees the same prompt. The answer returns the exact ACP option ID through
  `AgentRespondPermission`; first answer wins. Cancel sends `AgentCancel`.
- The daemon carries `AgentDescriptor { provider, cwd, session_id }` in mux state and the transcript
  in its journal. A respawned adapter replays through `session/load` where supported and out of the
  journal where not; see [Agent pane](/concepts/agent-pane.md).

# Input path (client → target)

```
GPUI event ─┬─ prefix chord (any focus) ─▶ window-root claim ─▶ mux key tables       [server]
            ├─ chrome action ─▶ ChromeKeymap ─▶ local or protocol-backed effect      [client]
            ├─ terminal raw key ─▶ mux key tables ─▶ Pass ─▶ PTY                     [server]
            ├─ browser page key ─▶ browser-surface route ─▶ CEF sink                 [server/client]
            └─ Agent focused ─▶ composer / cancel / approval ─▶ ACP controller
```

- The renderer-free [mux](/crates/zz-mux.md) handles prefix key tables and command
  resolution; the same [commands](/tmux/commands.md) are also reachable from any shell as
  CLI clients (e.g. `send-keys -t %0 …`). The prefix is one-shot, exactly as in tmux: it arms the
  prefix table for a single following key; an unbound prefix-table key is swallowed and exits the
  table, while an unbound root key passes through to the pane.
- The tmux prefix is authoritative from **every** focus context. Terminal and Browser keys reach
  the daemon; local text widgets (composer, address bar) are covered by the window-root
  prefix claim, which forwards the chord (and the armed sequence that follows it, tracked via
  `PrefixArmed` events) with the active pane as command source.
- `zz-client::ChromeKeymap` resolves client-owned `ui`, `sidebar`, `browser`, and `terminal`
  actions after the prefix claim. The daemon publishes all of its pane tables through
  `ServerHello.key_tables` and `KeyTablesChanged`, but keeps authority over their execution.
- Browser page input uses dedicated Browser-surface messages that skip root-table resolution and
  feed the synchronized CEF sink. The prefix claim remains available from the page.
- Browser input translation (pointer, wheel, keyboard, committed text, IME, focus, resize) lives in
  [input translation](/browser/input-translation.md).
- Wheel scroll over a live pane with a warm history ring repaints from the ring on the next frame and
  tells the daemon once, through a `ScrollToOffset` debounced by 120 ms. See
  [terminal interaction](/terminal/interaction.md).

# Configuration & appearance flow

At GUI startup, the app independently resolves the first platform-appropriate
[`zz/config`](/configuration/app-config.md) file into client-local GPUI globals used by native
window rendering. A 500 ms watcher hot-reloads it; daemon-owned appearance and mux entries cross the
override channel, and the three agent adapter keys are among them . the daemon reconfigures what the
*next* pane spawns without disturbing the children already running. Only `agent-working-directory`
stays client-side.

zz's own configuration is authoritative: the daemon reads no external configs. It resolves
[appearance](/terminal/appearance.md) as built-in defaults plus the client-pushed `zz/config`
overrides, and sources the zz-owned `zz/mux.conf` (tmux grammar: options, bindings, status) at
startup and on `reload-config`/`C-b r`, then pushes the resolved state to every GUI client and live
terminal actor without killing PTYs, scrollback, selections, searches, layouts, or browser panes.
Ghostty and tmux configs are read only by the client's explicit import flow, which snapshots them
into `zz/config` and `zz/mux.conf` (first-run prompt or Settings → Import).

# Related

- [System overview](/architecture/overview.md)
- [Process model](/architecture/process-model.md)
- [Terminal frame](/concepts/terminal-frame.md) · [OSR rendering](/browser/osr-rendering.md)
