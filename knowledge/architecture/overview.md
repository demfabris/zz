---
type: Architecture
title: zz system overview
description: zz is a cross-platform GPUI workspace that multiplexes native terminal, Chromium browser, and Agent panes over a persistent daemon that several of the user's devices attach to at once.
resource: crates/zz/src/lib.rs
tags: [architecture, overview, gpui, multiplexer, daemon]
timestamp: 2026-08-12T00:00:00Z
---

# Overview

`zz` is a small cross-platform GPUI workspace that multiplexes three pane surfaces under a
tmux-style model:

- a **live local terminal** powered by `libghostty-vt` and a daemon-owned PTY worker
  (see [zz-terminal](/crates/zz-terminal.md));
- a **Chromium browser** powered by CEF Alloy off-screen rendering (OSR), composited as a GPUI image
  (see [zz-browser](/crates/zz-browser.md));
- a native **Agent pane** backed by a pane-local Codex or Claude Code ACP process, with streaming
  Markdown, Mermaid, reasoning, plans, structured tool activity, slash skills/commands, dynamic
  permission, model, and effort controls, cancellation, and agent-owned session replay (see
  [Agent pane](/concepts/agent-pane.md)).

The browser is **not** a native child window. CEF publishes owned BGRA frames into a one-slot mailbox,
the app keeps only the latest frame, and one custom GPUI element paints it with normal clipping. See
[OSR rendering](/browser/osr-rendering.md). Pointer, wheel, keyboard, committed text, IME, focus,
resize, navigation, and cursor state travel through browser-neutral types.

Sessions, windows, and binary split-pane layouts live in a **persistent daemon**
([server](/crates/zz-daemon.md)) reached over an owner-only local socket . directly, or forwarded by
`ssh -L` when the daemon is on another machine. Every device the user owns can attach to one session
at the same time, each with its
own viewport, scroll position, and focused window, while the layout tree, pane focus, and PTYs stay
shared. Closing every GPUI window detaches that client without stopping
terminal processes; see [session persistence](/concepts/session-persistence.md). Browser panes
restore their last URL and zz [profile](/browser/profile.md) when a GUI reattaches; Agent panes
restore their provider, working directory, and opaque ACP session ID, then rely on that provider's
`session/load` support to replay history. Neither transient Chromium state nor ACP children remain
alive without a GUI.

# Design pillars

| Pillar | What it means | Where |
|--------|---------------|-------|
| tmux-familiar UX | prefix key tables, a `.tmux.conf` subset, stable target IDs, `send-keys` | [mux](/crates/zz-mux.md), [tmux compat](/tmux/tmux-compat.md) |
| Heterogeneous panes | terminal, browser, and Agent panes are uniform for layout/focus/targeting/lifecycle | [split-pane layout](/concepts/split-pane-layout.md) |
| Renderer-free core | mux state machine has no UI/renderer dependency | [mux](/crates/zz-mux.md) |
| Persistent daemon | PTYs + mux state outlive GUI detach; every device attaches to one session at once | [session persistence](/concepts/session-persistence.md) |
| Portable transport | Unix-domain socket on Linux/macOS, named pipe on Windows, and the same socket forwarded over ssh for remote hosts | [fleet attach](/designs/fleet-attach.md) |
| Reimplemented, not linked | tmux behavior ported to safe Rust against a pinned upstream | [tmux upstream](/references/tmux-upstream.md) |

# Crate map

| Crate | Role |
|-------|------|
| [protocol](/crates/zz-protocol.md) | stable IDs, versioned length-prefixed control protocol, packed terminal lanes |
| [mux](/crates/zz-mux.md) | renderer-free state machine: layouts, targets, commands, key tables, `.tmux.conf` |
| [server](/crates/zz-daemon.md) | persistent daemon: mux state, PTYs, frame fanout, sockets, attachment, CLI |
| [zz-terminal](/crates/zz-terminal.md) | per-PTY child + libghostty on a worker thread; publishes terminal frames |
| [zz-browser](/crates/zz-browser.md) | CEF init, subprocess dispatch, request context, input translation, frame mailboxes |
| [app](/crates/zz.md) | long-lived GPUI mux client; reconciles layouts; hosts terminal, CEF, and Agent views |
| [xtask](/crates/zz-xtask.md) | builds and validates platform CEF bundles |

# Platform status

Linux/Wayland remains the most extensively runtime-validated host. On macOS, the release CEF bundle
and daemon PTY detach/reattach lifecycle are runtime-validated; a full interactive GUI PTY/browser
smoke remains outstanding. Windows maps the protocol to a local named pipe and has CI bundle coverage,
but its full host smoke remains outstanding.

# External pins

- CEF pinned to Rust packages `151.2.0+151.3.14`, Chromium `151.0.7922.72` . see
  [CEF artifacts](/references/cef-artifacts.md).
- GPUI + `gpui_platform` come from a fixed Zed revision . see [GPUI revision](/references/gpui-revision.md).
- tmux behavior checked against a pinned commit . see [tmux upstream](/references/tmux-upstream.md).
- X11 named colors sourced from Ghostty . see [Ghostty color reference](/references/ghostty-color-reference.md).

# Related

- [Process model](/architecture/process-model.md) . daemon, GUI client, CEF subprocesses, PTY workers.
- [Data flow](/architecture/data-flow.md) . how frames and input move end to end.
- [Agent pane](/concepts/agent-pane.md) . provider-bound ACP processes, session routing, streaming
  reducer, dynamic composer controls, approvals, and restore boundary.
- [Design plans](/designs/index.md) . feature plans and decision records.
