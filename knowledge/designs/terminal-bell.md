---
type: Design Plan
title: Terminal bell . Ghostty-parity notifications
description: Shipped plan to stop swallowing BEL - register libghostty's on_bell in the daemon, carry a per-pane bell flag in mux state and the snapshot plus an appended EventPayload edge event, badge the pane's sidebar icon, render a `!` window flag, request window attention and play the macOS system beep on the ring edge, clear on pane input, selection, or window activation; no OS notification popup, no config in v1.
status: Complete
tags:
- terminal
- bell
- notifications
- protocol
- daemon
- design-plan
timestamp: 2026-08-07T20:06:22Z
---

# Overview

zz swallows BEL today. libghostty exposes `Terminal::on_bell`
(`third_party/rust/libghostty-vt/src/terminal.rs`), and the session layer registers five
other callbacks right next to where it would go (`crates/zz-terminal/src/session.rs`,
`on_pty_write` / `on_clipboard_write` / etc.) . the bell callback is the one nobody wired.
This plan wires it, end to end, copying Ghostty 1.2's default bell behavior because that is
the least intrusive design that still tells you which of your twelve panes rang.

Ghostty's defaults: a bell puts an indicator in the title, bounces the dock icon once
(macOS) or sets the WM urgency hint (GTK), plays the system beep (the `system` bell
feature, default since 1.3.0 on macOS . 1.2 had no macOS audio), and clears when the
terminal gains focus or receives input. No OS notification popup. zz v1 matches that and
adds nothing.

Scope guard: agent-pane lifecycle notifications (`RuntimeEvent::PromptFinished` and
friends) are out of scope while the `agent-pane` feature stays gated. OSC 9 / OSC 777
program notifications are blocked upstream . libghostty routes OSC 9 to `PWD_CHANGED` and
has no notify callback in its C ABI.

# Behavior

A program rings BEL in a pane:

* The pane's sidebar row grows a small warning-colored dot on the top-right corner of its
  kind icon. The dot rides the marker, not the trailing slot, because every non-host row's
  trailing controls only surface on hover . a notification the pointer has to find is no
  notification.
* `window_flags()` in `crates/zz-mux/src/status.rs` appends `!` for a window containing a
  belled pane, next to the existing `*` and `Z`. tmux semantics, tmux glyph.
* The workspace window showing that pane calls `Window::request_attention()` . dock bounce
  on macOS, urgency hint on Linux, taskbar flash on Windows. gpui already no-ops the call
  when the window is key, so a bell in the window you are typing in stays silent at the OS
  level.
* The flag clears when the pane receives input (any PTY write from a client), becomes
  the active pane, or its window becomes the active window (tmux's activation-clear). Both events are daemon-visible, so clearing needs no knowledge of
  client focus. This approximates Ghostty's clear-on-focus-or-input with the two signals a
  multiplexer daemon actually has.

* macOS also plays the system beep (`objc2_app_kit::NSBeep`, one call, no new deps) on
  the same edge. Unlike the bounce it sounds while the window is key . readline's
  backspace-on-empty BEL is the canonical case. The latch still applies, but interactive
  bells re-ring naturally: the keystroke that provokes the BEL is input, so it clears the
  latch the reply immediately re-raises. A non-interactive flood stays one ding.

No OS notification popup, no config key. Ghostty ships these defaults to everyone and
they hold up; a `bell-features`-style opt-out can land later if someone asks.

# Where the state lives

The bell flag is daemon state, not client state . the tmux answer, for the tmux reasons: a
flag held by the client dies on detach, and two attached clients would disagree about
which panes rang. The daemon owns the PTYs, sees BEL first, and already publishes
pane-scoped program events to clients (clipboard writes are the precedent).

Flow:

1. `crates/zz-terminal/src/session.rs` registers `on_bell` beside the existing callbacks
   and surfaces it as a `TerminalSessionEvent`, riding the same actor/publisher path as
   clipboard writes.
2. The daemon sets `bell: true` on the pane in mux state and publishes the edge event to every
   subscribed interactive client, including clients currently attached to another session.
3. The daemon clears the flag on PTY input to that pane, on pane-selection change, or on
   window activation (including `kill-session -C`), releasing the terminal bell latch on the
   same transition, and the next snapshot reflects it.

Flood safety: the bell latches at the session layer. When the pane's flag is already
pending, the `on_bell` callback returns after one bool check, and the daemon publishes the
edge event only on the false-to-true transition. A `\x07` flood (`cat /dev/urandom` rings
roughly one bell per 256 bytes) therefore collapses to a single event, one snapshot delta,
one redraw, and one `request_attention` until the pane is visited. Non-BEL bytes never
touch any of this . libghostty parses BEL as a C0 control with or without a callback, so
the throughput hot path and the bench/ numbers are unchanged.

# Protocol (one version bump)

* `EventPayload::Bell { pane_id }`, appended to the enum . postcard encodes variants by
  index, so append-only, per the standing comment in `zz-protocol/src/message.rs`. This is
  the edge trigger; the client uses it for `request_attention` only.
* A per-pane `bell` flag on the snapshot. This is the level trigger the sidebar and
  `window_flags()` render from, and it is what makes a reattaching client see bells that
  rang while it was gone.

Fleet delivery crosses two ownership boundaries. The remote daemon broadcasts both the snapshot
level and bell edge to clients attached elsewhere, and `MuxClient` accepts those two event kinds
from every host connection instead of applying the attached-host event filter. The cached snapshot
drives sidebar dots while the edge increments the shared attention revision, so `printf '\a'` on a
background fleet host bounces the local dock without switching hosts.

# Client

Two small pieces, no client-held bell state:

* `crates/zz/src/mux/client.rs`: accept `EventPayload::Bell` from attached and background hosts and
  advance the shared bell revision; `AppView` turns a new revision into `request_attention()` and
  the macOS system beep.
* `crates/zz/src/workspace/sidebar.rs` and `zz-mux/src/status.rs`: render from the snapshot flag.
  Sidebar dots bubble from panes through collapsed window, session, and host rows so hidden remote
  descendants still advertise attention.

# Testing

* `zz-terminal`: feed `\x07` through a session, assert the bell event surfaces . mirror of
  the clipboard-write tests.
* Daemon: set/clear transitions (bell then input clears; bell then select-pane clears; bell
  then window activation clears and releases the latch), snapshot
  carriage of the flag, and edge delivery to an un-attached subscriber.
* Client/sidebar: background-host edges advance attention and cached remote snapshot flags bubble
  through collapsed ancestors.
* Smoke, per the headless daemon recipe: `printf '\a'` in a pane, read `!` out of
  `window_flags`; repeat against an ssh fleet host for wire propagation.

# Deferred

* macOS dock badge. Ghostty shows one; gpui has no badge API. zz already depends on
  `objc2-app-kit`, so a local `NSDockTile` call is viable without touching the fork.
* Window-title bell suffix.
* The ding on Linux/Windows, custom audio files (`bell-audio-path` style), border flash,
  and a `bell-features` config key.
* Activity/silence monitoring (tmux `#` / `~`) . different feature, same plumbing; this
  plan's `EventPayload` + snapshot-flag shape is the template for it.
