---
type: Protocol
title: Mux snapshots (snapshot.rs)
description: The MuxSnapshot state tree (sessions, windows, recursive split layouts, pane descriptors, per-client focus, and viewer presence) that clients reconcile on attach and after a resync.
resource: crates/zz-protocol/src/snapshot.rs
tags: [protocol, snapshot, layout, state, presence]
timestamp: 2026-08-13T00:00:00Z
---

# Overview

A **`MuxSnapshot`** is the complete, renderer-neutral picture of the daemon's session/window/pane/split
tree at a point in time. Defined in `crates/zz-protocol/src/snapshot.rs`, it is what an interactive client
receives on attachment and whenever it must **resynchronize** after a dropped or out-of-order event.
The snapshot carries *structure and metadata* (layout, titles, active/zoomed panes, pane kind). It
deliberately does **not** carry terminal cell contents, which stream separately over the
[packed terminal lanes](/protocol/terminal-lanes.md). A monotonically increasing `generation` counter
lets clients detect staleness. The [GPUI app](/crates/zz.md) reconciles this tree into live pane
entities, reusing them by stable [`PaneId`](/protocol/ids.md).

# When it is produced and consumed

- **Attach**: the daemon replies with `ProtocolMessage::Attached { session: SessionId, snapshot }`
  (see [wire protocol](/protocol/wire-protocol.md)). Other clients already attached to that session
  stay attached.
- **Event push**: also delivered as `EventPayload::Snapshot(MuxSnapshot)` inside an ordered `Event`.
  Each subscriber gets its own copy, stamped with that client's focus and presence.
- **Resync**: an interactive client detecting a sequence gap sends `ProtocolMessage::Resync`; the
  daemon answers with a fresh full snapshot. Overlays like the command prompt, choose-tree, and buffer
  chooser *survive* a resync by re-reconciling against the new snapshot.
- Produced by [zz-daemon](/crates/zz-daemon.md) from [the mux state machine](/crates/zz-mux.md); consumed by
  every attached client.

# Schema . the snapshot tree

`MuxSnapshot` nests four levels; all types serialize through `serde` and travel via `postcard` on the
Control lane. Most types derive `Deserialize`; recursive `LayoutNode` uses a wire-compatible custom
deserializer so hostile nesting is rejected before it can exhaust the receiving thread's stack.

| Type | Fields |
|------|--------|
| `MuxSnapshot` | `generation: u64`, `sessions: Vec<SessionSnapshot>`, `focused_window: Option<WindowId>` |
| `SessionSnapshot` | `id: SessionId`, `name: String`, `active_window: WindowId`, `windows: Vec<WindowSnapshot>`, `viewers: Vec<SessionViewer>` |
| `SessionViewer` | `name: String`, `window: WindowId`, `is_self: bool` |
| `WindowSnapshot` | `id: WindowId`, `index: u32`, `name: String`, `active_pane: PaneId`, `zoomed_pane: Option<PaneId>`, `layout: LayoutNode`, `panes: BTreeMap<PaneId, PaneSnapshot>` |
| `PaneSnapshot` | `id: PaneId`, `title: String`, `kind: PaneKindSnapshot`, `synchronized_input: bool`, `bell: bool` |
| `PaneKindSnapshot` | `Picker` \| `Terminal` \| `Browser(BrowserDescriptor)` \| `Agent(AgentDescriptor)` \| `Editor(EditorDescriptor)` |
| `BrowserDescriptor` | `tabs: Vec<String>`, `active_tab: usize`, `profile: String` |
| `AgentDescriptor` | `provider: AgentProvider`, `cwd: Option<PathBuf>`, `session_id: Option<String>` |
| `EditorDescriptor` | `path: Option<String>`, `cwd: String` |

`generation` is the version stamp for the whole tree; `WindowSnapshot.index` is the tmux-style window
number; `zoomed_pane` records a temporarily maximized pane; `synchronized_input` marks panes receiving
mirrored keystrokes; `bell` is latched until the pane is read after a BEL. `BrowserDescriptor.tabs`
is the strip in order (never empty in the type's contract; `url()` returns the active tab or
`about:blank`). `WindowSnapshot.name` is the user's stable, explicit window name;
`PaneSnapshot.title` is live presentation metadata. Terminal OSC title changes are synchronized by
the daemon's terminal watcher; automatic Unix Bash/zsh hooks publish the working directory at a
prompt and the full command immediately before execution, while applications may override either
with later OSC 0/2 output. Browser document-title changes arrive through tmux-compatible
`select-pane -T`. Either title change advances `generation` and publishes a fresh snapshot without
renaming the containing window.

`Picker` is a durable, runtime-free pane state used by the native new-pane flow. It occupies a real
layout leaf and survives GUI detach/reattach, but the daemon owns no PTY and the GUI owns no CEF
session for it. `select-pane-kind` with `terminal`, `browser`, `agent`, or `editor` materializes the
same `PaneId` and advances the snapshot generation.

`Editor(EditorDescriptor)` is the daemon-owned restore metadata for an Editor pane: the open file's
absolute `path` (`None` for a scratch pane) and the pane's `cwd`. Buffer bytes, cursor position, and
dirty state stay GUI-local. Both paths are validated on deserialization as absolute, at most
`MAX_EDITOR_PATH_BYTES` (16 KiB), and free of control characters, so a hostile descriptor cannot
become mux state.

`Agent(AgentDescriptor)` stores a bounded built-in `AgentProvider` (`Codex` or `ClaudeCode`) without
transporting provider-specific ACP payloads. The daemon captures the donor terminal's live working
directory when a picker becomes an Agent pane. After the app creates or loads the ACP session, it
updates the descriptor with the actual absolute cwd and the agent's opaque session ID. These three
fields are the v24-and-later restore contract: the app owns the process and live reducer state, and the
selected provider owns conversation history behind `session/load`. Switching providers updates the
descriptor and clears its old session ID. Prompts, messages, tool calls, permissions, credentials,
and provider-specific payloads never enter the mux snapshot.

## Per-client stamping and presence

A session holds a set of attached clients, so the tree alone cannot say which window a given device
is looking at. The daemon stamps each copy of the snapshot for its recipient just before publishing
it: `MuxSnapshot.focused_window` is that client's own focused window, and every
`SessionSnapshot.viewers` entry gets `is_self` set for the receiving client. Both fields carry
`#[serde(default)]`.

`focused_window_for(session)` resolves what to render. It keeps the stamped window only while that
window still exists in the session, and otherwise falls back to `session.active_window`, which covers
both a detached snapshot and a client focus left stale by a window that closed. Removing a window
needs no snapshot repair pass.

A `SessionViewer` names one attached device: `name` is the `device_name` from its `ClientHello`
(`device-<client id>` when the hello carried none), `window` is the window that device is watching,
and `is_self` distinguishes the recipient from its peers. This is what the sidebar renders as
presence.

## Layout . the recursive `LayoutNode`

Each window's geometry is a binary tree that keeps stable `^split` IDs and ratios across resizes:

| Variant | Fields |
|---------|--------|
| `LayoutNode::Pane(PaneId)` | a leaf pane |
| `LayoutNode::Split { id, axis, ratio, first, second }` | `id: SplitId`, `axis: Axis`, `ratio: f32`, `first`/`second: Box<LayoutNode>` |

`Axis` is `Horizontal | Vertical` (default `Vertical`, `repr(u8)`). `ratio` is the first child's share
of the split's extent as an `f32` (the live [split resize](/protocol/wire-protocol.md) input
`ResizeSplit` uses fixed-point `ratio_basis_points` over `SPLIT_RATIO_BASIS = 10_000`, applied in the
mux). `LayoutNode` provides walk helpers: `contains(pane)`, `panes(&mut Vec<PaneId>)`,
`contains_split(split)`, and `splits(&mut Vec<SplitId>)`.

Decode accepts at most 256 nested layout nodes on one path and 65,535 total nodes in one layout.
Exceeding either budget returns a normal protocol decode error. Serialization and the postcard enum
layout are unchanged, so these defensive bounds do not require a protocol-version bump.

# Examples

```rust
// A window split vertically 60/40 with two terminal panes:
let layout = LayoutNode::Split {
    id: SplitId(11),
    axis: Axis::Vertical,
    ratio: 0.6,
    first:  Box::new(LayoutNode::Pane(PaneId(3))),
    second: Box::new(LayoutNode::Pane(PaneId(4))),
};

let snapshot = MuxSnapshot {
    generation: 42,
    sessions: vec![SessionSnapshot {
        id: SessionId(0),
        name: "dev".into(),
        active_window: WindowId(7),
        windows: vec![WindowSnapshot {
            id: WindowId(7), index: 0, name: "edit".into(),
            active_pane: PaneId(3), zoomed_pane: None,
            layout,
            panes: /* BTreeMap<PaneId, PaneSnapshot> */ Default::default(),
        }],
        // stamped per recipient: this copy went to "laptop"
        viewers: vec![
            SessionViewer { name: "laptop".into(),  window: WindowId(7), is_self: true },
            SessionViewer { name: "desktop".into(), window: WindowId(7), is_self: false },
        ],
    }],
    focused_window: Some(WindowId(7)),
};

// Delivered on attach:  ProtocolMessage::Attached { session: SessionId(0), snapshot }
```

A browser pane's kind carries its descriptor:
`PaneKindSnapshot::Browser(BrowserDescriptor { tabs: vec!["https://example.com".into()], active_tab: 0, profile: "default".into() })`.
An Agent pane carries restore metadata:
`PaneKindSnapshot::Agent(AgentDescriptor { provider: AgentProvider::Codex, cwd: Some("/workspace".into()), session_id: Some("opaque-session".into()) })`.
An Editor pane carries the file it reopens:
`PaneKindSnapshot::Editor(EditorDescriptor { path: Some("/workspace/src/main.rs".into()), cwd: "/workspace".into() })`.

# Related

- Carried by [the wire protocol](/protocol/wire-protocol.md) (`Attached`, `Event`/`Snapshot`, `Resync`).
- Part of [the zz-protocol crate](/crates/zz-protocol.md); identifiers from [stable IDs](/protocol/ids.md).
- Built by [the mux state machine](/crates/zz-mux.md), served by [zz-daemon](/crates/zz-daemon.md),
  reconciled by [the GPUI app](/crates/zz.md).
- Layout details: [split-pane layout](/concepts/split-pane-layout.md); durability:
  [session persistence](/concepts/session-persistence.md).
- Cell contents stream separately via [packed terminal lanes](/protocol/terminal-lanes.md).
- Agent rendering, ACP lifecycle, and restore boundary: [Native Agent pane](/concepts/agent-pane.md).
