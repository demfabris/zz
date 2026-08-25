---
type: Design Plan
title: Multi-device attach . every machine, same session
description: Agreed plan to lift the one-interactive-client-per-session rule so all of the user's devices can attach to one session at once - per-client viewports and window focus, latest-active-wins PTY sizing, presence decorations, and steal-attach falling out for free.
status: Complete
tags:
- multi-client
- fleet
- daemon
- views
- resize
- design-plan
timestamp: 2026-08-25T00:00:00-03:00
---

# Overview

> **Status: implemented 2026-07-31 . all three slices shipped** (protocol v34→v36; the
> refusal deletion, plural views, latest-active-wins arbitration, per-client focus, presence,
> and `attach-session -d` are current behavior). Implementation deltas from the plan:
> unattached command clients fan view actions out to every attached client's view; browser
> GUI requests route to the lowest attached ClientId (documented limitation); killing a
> session now sends the new `EventPayload::Detached { by: None }` to its clients . the same
> event steal-attach uses with the stealer's device name. Companion to the WAN-feel work in
> [scene-streaming remote attach](/designs/scene-streaming-remote.md) and the shipped
> [remote browser egress](/designs/remote-browser-egress.md).
>
> **Still current after the 2026-08-01 ssh-only consolidation.** This design is
> transport-agnostic . attached sets, per-client views, latest-active-wins sizing, presence, and
> steal-attach live entirely in the daemon and the envelope. Only the references below to QUIC
> and to pairing changed meaning: the devices that co-attach now reach the daemon over a unix
> socket or an `ssh://` forward. The QUIC egress splice was retired that day; ssh `-D` egress
> shipped again on 2026-08-07, see [remote browser egress](/designs/remote-browser-egress.md).

With [fleet attach](/designs/fleet-attach.md) and [fleet pairing](/designs/fleet-pairing.md)
shipped, the remaining "multiplayer" gap is that two of the user's own devices cannot look at
the same session at the same time . `SessionAlreadyAttached` refuses the second GUI. This plan
lifts that rule for the sovereign-multiplayer case: **every player is the same user on a
different machine** (desktop watching the agent window while the laptop reads logs), plus
future read-only glance clients.

The enabling observation from the code: the ban is a **policy gate, not architecture**. Key
engines, prefix state, outbound mailboxes, per-pane coalescing, and terminal view state are
already per-client maps in the daemon (`key_engines`, the per-client mailbox with its
`NeedsFull` coalescing, `TerminalViewId(client.0)` + `InactiveTerminalViews`). This design
wires up plumbing that half-exists; the wire vocabulary, terminal lanes, and patch format are
untouched.

# Scope decisions

Settled 2026-07-30; not to be re-litigated without new information.

| Decision | Choice | Why |
|----------|--------|-----|
| Players | Same user's devices; agents get their own follow-up design | Sharing with other humans (identity, permissions, guests) is Superlogical's product, not zz's |
| Approach | Extend the per-client view/mailbox machinery | A Superlogical-style raw-PTY tee would be a rewrite and forecloses thin clients (client must become a full emulator) |
| Viewports | Independent per client | Per-view scroll/selection/copy-mode/search state already exists; shared scroll is the tmux behavior everyone hates |
| Window focus | Independent per client; **pane focus and zoom stay shared** | tmux session-groups semantics built in; per-client zoom/pane-focus/layout was deliberately not chosen . revisit only with new information |
| PTY sizing | Latest-active viewer wins (settled 2026-07-31) | The "no letterbox mode needed" assumption was falsified in practice: min-of-viewers produced geometries no client owned, flapped cell-pixel dimensions, and fed the invalid-value worker crash |
| Input | Unchanged . FIFO interleave at the pane actor | Key engines and prefix state are already per-client; input messages are already pane-addressed |

# Architecture

## Policy lift

`attached: BTreeMap<SessionId, ClientId>` becomes `BTreeMap<SessionId, BTreeSet<ClientId>>`;
the `SessionAlreadyAttached` refusal (both the attach path and its command-path twin) is
deleted. The other direction stays: a client attaches to at most one session and attaching
evicts its previous attachment. The sidebar's "attached elsewhere" row becomes an
informational "also attached: laptop" decoration whose activation joins the session.
**Steal-attach falls out free**: attach plus an optional tmux-`attach -d`-style
"detach other clients" command.

## Plural views

The pane actor's `ActiveTerminalView` + `InactiveTerminalViews` split collapses into one map
of **simultaneously active** views keyed by `TerminalViewId` (already `client.0`). Each view
keeps exactly what it keeps today: scroll anchor, selection, copy-mode, search. On publish
(the 16 ms staleness gate is unchanged) the actor extracts one viewport per active view;
`watch_terminal` diffs per view stream; per-client mailboxes and per-pane coalescing are
untouched downstream. Cost is O(attached devices) per pane, N≤3 in practice.

## Resize arbitration

The daemon records each client's last-reported geometry per pane plus a global geometry-owner
sequence per client. Terminal input advances that sequence. Client FocusIn also advances it when
the server `focus-events` option is on. Absent or equal sequences tie on lowest
`ClientId`, and an owner without a report falls through to the next-ranked viewer that has one.
Resize reports and visibility changes recompute the owner.

`window-size latest` takes columns, rows, and cell metrics from the owner. Largest and smallest
aggregate columns and rows across eligible viewers, while manual retains its stored extent; all
three still take cell metrics from the owner. The inherited `aggressive-resize` option filters that
eligible set to clients viewing the window when ON. The result flows through the same guarded
window-extent write-back. A single viewer behaves exactly like
the default path.

"Terminal input" means the user reached for *that* terminal: keys, text, paste, non-motion mouse,
scrolling, selection, search, copy mode. It deliberately excludes `TerminalViewAction::Focus`,
which reports pane/application focus. Pane focus is shared mux state (see the scope table), so one
client's pane-focus change is echoed to every other client through the snapshot and re-applied to
its local focus. Counting pane focus as input therefore let each machine's focus echo claim the pty
back from the other, and two attached clients of different widths resized the shell on every focus
change. `terminal_view_action_is_input` enumerates the verdict per action rather than defaulting,
because that flicker began life as a wildcard arm quietly absorbing a newly added `Focus` variant.

Protocol v73 `ClientFocus` is separate client-window lifecycle input. When the server
`focus-events` option is on, it can refresh activity and let FocusIn claim geometry ownership
without changing shared pane focus or being echoed as pane focus through the snapshot.

## Per-client window focus

Active-window moves from session state to a per-client map. The seam already exists:
`visible_terminals` is computed per client today . it starts reading the client's own focus
instead of the session's. Knock-ons, all mechanical:

- **Command targeting**: "current window/pane" resolves via the *issuing client's* focus
  (commands already arrive per-client). Pane focus within a window stays shared mux state.
- **Status line & snapshot**: window-list highlight and active markers become per-client
  renders; `MuxSnapshot` grows "your focus" fields (it is already delivered per-client).
- **Lifecycle**: killing a window that is some client's focus reassigns that client's focus
  with the same rule the session uses today.

## Presence

Each attached client's device name and focused window ride the existing
`StatusChanged`/snapshot path . the sidebar shows "⌁ laptop → logs" under the session. No new
event type. Presence is what makes steal-attach informed rather than blind.

# Gotchas

Recorded so implementation inherits them; several are the reason for slice ordering.

| Gotcha | Detail |
|--------|--------|
| Resize arbitration | Latest-active-wins ships with the policy lift: terminal input advances one daemon-global per-client sequence, and an ownership change is the resize debounce |
| Undersized grid gutter (resolved 2026-07-31) | A non-owner can render a grid smaller than its rect; the terminal element now paints the top-or-bottom and right dead bands with a muted theme surface |
| Per-view publish cost | O(devices) viewport extraction + diff per pane per publish. Acceptable at N≤3; views sitting at live-tail with no selection produce identical frames and can Arc-share encoded bytes . optimization, not a requirement |
| Status line is per-client now | Any status renderer caching per-session output must key by client |
| Shared zoom/pane focus | Deliberate cut: device A zooming a pane zooms it for device B. Do not "fix" this ad hoc . it is per-client-layout territory, explicitly not chosen |
| Protocol bump | One `PROTOCOL_VERSION` bump (snapshot focus/presence fields, refusal-path removal). Attach handshake, terminal lanes, patch format, resync untouched |
| Fleet is orthogonal | Two devices are two `HostConnection`s to the same daemon; per-host isolation and version-skew presentation already exist |
| Disconnect = involuntary detach | Release the client's views, drop it from presence, and promote the next-ranked visible geometry owner . all riding existing `DetachView` machinery |

# Testing

New pattern, cheap: **two clients, one scratch daemon** (inverse of the fleet tests'
two-daemons-one-client). Asserts: both receive independent frame streams; scrolling one
leaves the other's viewport untouched; focus split yields disjoint `visible_terminals`;
latest-active ownership changes on terminal input and promotes a survivor on detach; steal
(`attach -d` equivalent) detaches the peer cleanly.

# Slices

Independently shippable, in order.

| # | Slice | Exit criterion |
|---|-------|----------------|
| 1 | Policy lift + plural views + latest-active-wins resize | Two GUIs on one session, shared focus, independent scroll, sane sizing |
| 2 | Per-client window focus | Desktop on the agent window, laptop on logs, same session |
| 3 | Presence + steal-attach | Sidebar shows who's where; detach-others works |

# Future work (explicitly not this design)

- **Read-only viewer tier**: a role flag at attach so a glance client (phone) never
  participates in sizing or input . the plural-view substrate is exactly what it attaches to;
  rides the [scene-streaming degraded-clients ladder](/designs/scene-streaming-remote.md).
- **Cross-machine agents**: `zz -t server:dev.1 send-keys` from any machine . the deferred
  half of [fleet attach](/designs/fleet-attach.md)'s "machines operate each other"; its two
  candidate shapes are unchanged and it deserves its own design session.
- **Per-client zoom / pane focus / layouts**: only with new information.

# Non-goals

- **Sharing with other humans** . identity, permissions, read-only guests for teammates.
- **Session durability across daemon host reboots** . still its own orthogonal feature.
- **WAN feel** . predictive echo, lossy lanes, scrollback locality live in
  [scene-streaming remote attach](/designs/scene-streaming-remote.md).

# Related

- [Fleet attach](/designs/fleet-attach.md) . the host model this composes with; its
  one-client-per-session rule is the thing this design lifts
- [Scene-streaming remote attach](/designs/scene-streaming-remote.md) . M6's multi-client
  half is superseded by this document; the TUI renderer half remains there
- [zz wire protocol](/protocol/wire-protocol.md) and
  [packed terminal lanes](/protocol/terminal-lanes.md) . unchanged except the version bump
- [Session persistence & detach](/concepts/session-persistence.md) . detach semantics reused
  for involuntary disconnect
