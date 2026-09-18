---
type: Design Plan
title: Native status bar for GUI clients
description: The desktop status bar combines a session switcher, left-aligned window pills, and agent activity from structured state while tmux status rows remain a TUI compatibility surface.
status: Complete
resource: crates/zz/src/status_bar.rs
tags:
- status-bar
- desktop
- gpui
- client
- snapshot
- configuration
timestamp: 2026-09-17T00:00:00-03:00
---

# Decision

The desktop GUI builds its status bar from structured mux state and typed app settings. It does not
consume `StatusLine`, parse tmux styles, recognize powerline separators, or map private glyphs to
icons. The raw-terminal attach client keeps the daemon-expanded tmux rows and renders them with the
shared cell composer.

Desktop and web share the status widgets. Native Apple titlebars also use left alignment and omit
time and date; the shared session and activity menus belong to the GPUI clients.

# Data and presentation

`zz_client::StatusBarModel::from_snapshot` is the pure boundary between mux facts and GUI layout. It
selects the attached session, returns its full window list, marks the focused window, and derives
bell and activity from each window. Each window also carries its panes in layout order, with pane
identity, kind, label, and focus state for its icon deck.
Automatically named windows show the active agent's session name, or “New session” when unnamed;
explicit window names and terminal-focused window labels stay in place. The model retains the raw
window name for rename prompts. The activity list includes only non-dead Agent panes and disappears at zero. Session and
host values are optional according to settings, and the desktop supplies a host name only for a
remote attachment.

The model owns no truncation, overflow menu, click action, color, update state, or time source. The
desktop view owns those concerns:

| Item | Desktop behavior |
| --- | --- |
| Session | Session name, Layers icon, and chevron; its dropdown switches directly between sessions on the attached host |
| Windows | Always left-aligned; index and name for at most five visible windows; the visible range stays centered around the active window, and the overflow menu reaches the rest |
| Pane deck | Overlapping cards represent Terminal, Browser, Agent provider, Editor, and Picker panes; hovering lifts a card and shows pane details; clicking selects that pane |
| Window markers | Small bell and activity indicators derived from the model; daemon selection clears the latched attention state |
| Agents | Small status dot and summary; needs input takes priority over failed, running, and idle states; clicking lists agents and selects their panes |
| Host | Remote host name; absent for the local daemon |
| Update | Available version with a dot; clicking starts the existing installer |

The sidebar and status bar are mutually exclusive desktop views: an expanded sidebar hides the
status bar; retracting it shows the status bar in the title bar. There is no bottom placement.

Desktop shortcuts Cmd+1–9 on macOS and Ctrl+1–9 on Linux select windows by their
position in the attached session's displayed order, whether the sidebar or titlebar
is visible. They include windows outside the five visible pills and ignore missing
positions. The `ui` chrome actions `select-window-1` through `select-window-9`
support rebinding and take priority over pane shortcuts.

Session and agent buttons share the active window pill’s background, theme border, shadow, and
26px height, with widths sized to their contents.

Window pills are 26px high and vertically centered inside the 35px titlebar, leaving 4.5px at both edges. They
start at 240px wide regardless of the title and shrink together only when the window strip runs
out of room; titles truncate within the remaining space. Their pane cards are 20px rounded squares with 14px icons, 7px overlap, a fine
outline, and a directional theme shadow. Up to three pane cards are visible, followed by `+N`
when more panes exist. Cards stack from left to right, each above its right-hand neighbor; the
hovered card lifts above inactive cards within a fixed hit area. The focused pane always paints
last, including while another card or overflow is hovered. It stays visible and
sits 1px higher at rest. Hover tooltips show the pane name, type, identity, focus state, and available
URL, browser profile and tab count, or working path. The overflow tooltip lists hidden pane names.
Pane decks remain visible when attention badges are disabled. Clicking a card selects both its
window and pane directly, without a popup menu; disconnected cards do not select.

Browser cards use the active tab URL to retrieve a cached favicon and page title within the
browser profile. The globe and current URL are fallbacks, with “New tab” for a blank page. Browser
tabs do not increase the pane count. The web client shares the deck, tooltips, and direct selection with globe fallback.

Chrome comes from the active zz-ui theme. There is no time/date item or alignment preference.
Agent activity uses `zz_client::agent_attention_status` from published agent state; missing state
is never counted as running. `mux::client::AgentStateChanged` notifications refresh the app shell
without a periodic timer. Session and agent menu actions are disabled while disconnected.

# Settings

The settings are app-side presentation preferences in `zz/config`. The **Status bar** page under the
Appearance navigation group exposes them. A preview that scrolls with the controls uses the
same status components and sample pane decks, updating as these preferences change.

| Key | Default | Effect |
| --- | --- | --- |
| `status-show-session` | `true` | Show the session switcher |
| `status-badges` | `true` | Show bell and activity window markers |
| `status-agents` | `true` | Show activity for non-dead Agent panes |
| `status-host` | `true` | Show the remote host item |
| `status-update` | `true` | Show an available update item |

Edits use the existing comment-preserving app-config writer and the normal live reload path. None of
these keys belongs in `zz/mux.conf`.

# Protocol boundary

Wire v86 appends `activity: bool` to `WindowSnapshot`. The other bar facts already existed in the
snapshot or app state, so the desktop needs no native status payload. `StatusLine` remains on the
wire for the cell-faithful TUI and tmux compatibility; GUI clients have no presentation consumer for
it.

# Deferred

Custom text is optional and unbuilt. A future version may carry one daemon-expanded format string
as plain text with a bounded refresh cadence for `#(cmd)`. The GUI would use theme colors and honor
no tmux style. That work needs a protocol addition and does not expand the native item vocabulary.

Further FFI and mobile-client activity presentation remains separate. There is no faithful-tmux toggle in the GUI.

# Key files

- `crates/zz-client/src/status_bar.rs`: pure status-bar model and typed settings.
- `crates/zz/src/status_bar.rs`: GPUI layout, overflow, actions, update item, and agent selection.
- `crates/zz/src/config/mod.rs`: app-side values, defaults, parsing, and live projection.
- `crates/zz/src/config/settings.rs`: Status bar page controls.
- `crates/zz/src/app_shell.rs`: snapshot and agent-state redraw subscriptions.
- `crates/zz-tui/src/main.rs`: terminal client that retains the composed tmux status row.

# Related

- [tmux status line](/tmux/status-line.md)
- [Application configuration](/configuration/app-config.md)
- [Snapshot schema](/protocol/snapshots.md)
