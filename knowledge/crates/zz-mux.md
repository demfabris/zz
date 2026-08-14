---
type: Rust Crate
title: zz-mux crate . renderer-free mux state machine
description: The pure, UI-agnostic multiplexer core that owns sessions/windows/panes/splits, resolves tmux-style targets, executes tmux-compatible commands, holds key tables, and parses .tmux.conf.
resource: crates/zz-mux/src/lib.rs
tags: [mux, tmux, state-machine, crate, layout]
timestamp: 2026-08-01T00:00:00Z
---

# Overview

`zz-mux` is the renderer-free heart of zz's multiplexer: a pure state machine
for layouts, target resolution, tmux-style commands, and the supported `.tmux.conf`
parser. It compiles without GPUI, CEF, or PTYs and never renders anything; it takes parsed
`CommandInvocation`s and key presses and returns structured [`MuxEffect`](/tmux/commands.md) side
effects plus [`MuxSnapshot`](/protocol/snapshots.md) state for the daemon adapter to act on. All
identifiers (`$session`, `@window`, `%pane`, `^split`, `c` client) come from
[`crates/zz-protocol`](/crates/zz-protocol.md); mux never invents its own wire types. The
[`crates/zz-daemon`](/crates/zz-daemon.md) daemon owns an instance of `MuxEngine` and turns its effects
into real PTYs, browser sessions, socket fanout, and clipboard actions.

`new-session` returns `PaneCreated{Terminal}` plus `Attach { session, detach_others: false }`, and
`-d` drops the attach effect. The daemon applies the attachment only for interactive clients, so a GPUI
empty-workspace action creates and enters the session atomically while command-only clients retain
ordinary detached CLI behavior. The daemon advertises this execution contract with
`new-session-attach-v1`; clients missing that capability can preserve the same result by sending
`attach-session` after `new-session`. `attach-session -d` sets `detach_others`, which is how one
device steals a session from the user's other devices; the engine itself tracks no attachments, so
evicting the displaced clients is the daemon's half of that effect.

One global server option exists for remote attach and carries no engine behavior beyond validation
and provenance: `history-trickle` (0 to 10,000 rows, default 2,000). It is server-scoped, rejects a
window-scoped `set-window-option`, and emits `MuxEffect::MuxOptionChanged` so the daemon republishes
`MuxOptions` to every client that renders them. It used to have two siblings, `predict` and
`browser-egress`; both were deleted with the QUIC transport on 2026-08-01, as were the `pair` command
and the `listen` option.

`lib.rs` is a thin facade over four private modules: `command` (the `MuxEngine` executor +
`MuxEffect`), `model` (`MuxState`, the sessions/windows/panes/splits tree), `parser`
(`parse_config` for `.tmux.conf`), and `status` (the FORMATS subset behind the `StatusHooks` seam).
It re-exports the shared command catalog and key model from `zz-protocol`, while the daemon remains
the runtime authority that mutates and resolves those tables.

# What `model.rs` owns

`MuxState` is the single source of layout truth. It holds two flat `BTreeMap`s (`sessions:
BTreeMap<SessionId, Session>` and `windows: BTreeMap<WindowId, Window>`) plus monotonic ID
allocators (`next_session_id`, `next_window_id`, `next_pane_id`, `next_split_id`) and a `generation`
counter bumped on every mutation. Each `Window` owns its panes (`BTreeMap<PaneId, Pane>`), the
recursive [`LayoutNode`](/concepts/split-pane-layout.md) split tree, a canonical `pane_order` vector,
a most-recently-used `last_panes` history, `zoomed_pane`, and `previous_layout`/`last_layout` for
layout undo. A `Pane` is an ID, a title, a `PaneKind` . `Picker { inherit_cwd_from }`, `Terminal`,
`Browser(BrowserDescriptor)`, or `Agent(AgentDescriptor)` . and packed per-pane `InputOptions` (the
`synchronize-panes` override bits). `Picker` is the runtime-free pending state a new pane sits in
until the user chooses its kind; `Agent` carries the provider (Codex or Claude Code), working
directory, and opaque ACP session ID the GUI restores a native Agent pane from. Pane titles are live
metadata updated independently of explicit window names; tmux-compatible `select-pane -T` changes a
title without selecting the pane.

The model stays UI-agnostic by three disciplines:

- **No rendering, no coordinates persisted.** Layout is a binary tree of ratios; geometric pane rects
  are computed on demand into a bounded `1_000_000`-unit logical space only when needed (directional
  navigation), never stored.
- **Stable IDs over pointers.** Splits, panes, windows carry monotonic `u64` IDs so a divider drag or
  swap resizes exactly the grabbed `^split`, and surfaces (PTYs/browsers) survive relocation.
- **Structural invariants are checked.** `MuxState::validate()` asserts that every
  window's layout leaves match its pane map and `pane_order`, that split IDs are globally unique,
  that active/zoomed/history panes are live, and that sessions reference existing active windows.

`swapped_layout` and `joined_layout` are free functions over a `LayoutNode` that return the tree
`swap-pane` or `join-pane` would produce without touching state. The GUI renders a pane drag through
the engine's own transform instead of maintaining a second copy of it, and the model's tests assert
that the prediction matches the mutation.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/lib.rs` | Crate facade; declares four private modules and re-exports the shared catalog and key contract from `zz-protocol`. |
| `crates/zz-protocol/src/catalog.rs` | Canonical commands, aliases, descriptions, accepted options, and completion value kinds. |
| `crates/zz-mux/src/model.rs` | `MuxState`: sessions/windows/panes, the recursive `LayoutNode` split tree, target resolution, layout presets, zoom, swap/rotate/break/join, `validate()`, and the free `swapped_layout`/`joined_layout` predictors. See [split-pane layout](/concepts/split-pane-layout.md). |
| `crates/zz-mux/src/command.rs` | `MuxEngine`: executes tmux-style commands, parses options/`-t` targets, emits `MuxEffect`s, and holds server/session/window options including `history-trickle`. See [commands](/tmux/commands.md). |
| `crates/zz-protocol/src/key.rs` | `KeyTables`/`KeyEngine`: root/prefix/copy-mode and overlay tables, default prefix `C-b`, bind/unbind, key folding, and the prefix-mode state machine. See [key tables](/tmux/key-tables.md). |
| `crates/zz-mux/src/parser.rs` | `parse_config`: the `.tmux.conf` tokenizer producing `CommandInvocation`s + diagnostics. See [conf parser](/tmux/conf-parser.md). |
| `crates/zz-mux/src/status.rs` | `StatusFormats`/`StatusOption`, `StatusContext`, and `expand_status`: the tmux FORMATS subset behind the `StatusHooks` seam that keeps the clock and `#()` out of this crate. See [status line](/tmux/status-line.md). |

# Related

- Implements the philosophy in [tmux compatibility](/tmux/tmux-compat.md), checked against the pinned
  [tmux upstream reference](/references/tmux-upstream.md).
- Consumes IDs, `LayoutNode`, `CommandInvocation`, and `ServerError` from the
  [wire protocol](/crates/zz-protocol.md) and produces [snapshots](/protocol/snapshots.md).
- Driven by the [server daemon](/crates/zz-daemon.md), which converts `MuxEffect`s into real side effects.
- Command execution and effects: [commands](/tmux/commands.md); binary layout tree:
  [split-pane layout](/concepts/split-pane-layout.md).
