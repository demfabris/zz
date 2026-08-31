---
type: Design Plan
title: Native status bar for GUI clients
description: The completed desktop status bar builds native session, window, Agent, host, update, and clock items from structured state and app settings while tmux status rows remain a TUI compatibility surface.
status: Complete
resource: crates/zz/src/status_bar.rs
tags:
- status-bar
- desktop
- gpui
- client
- snapshot
- configuration
timestamp: 2026-08-31T00:00:00-03:00
---

# Decision

The desktop GUI builds its status bar from structured mux state and typed app settings. It does not
consume `StatusLine`, parse tmux styles, recognize powerline separators, or map private glyphs to
icons. The raw-terminal attach client keeps the daemon-expanded tmux rows and renders them with the
shared cell composer.

This completed slice is desktop-only. It adds no FFI API or native Apple UI; that status-bar
extension remains deferred.

# Data and presentation

`zz_client::StatusBarModel::from_snapshot` is the pure boundary between mux facts and GUI layout. It
selects the attached session, returns its full window list, marks the focused window, and derives
bell, activity, and Agent flags from each window. An Agent marker means any Agent pane exists in the
window; the separate count includes only non-dead Agent panes and disappears at zero. Session and
host values are optional according to settings, and the desktop supplies a host name only for a
remote attachment.

The model owns no truncation, overflow menu, click action, color, update state, or time source. The
desktop view owns those concerns:

| Item | Desktop behavior |
| --- | --- |
| Session | Attached-session name chip; clicking it focuses the session picker in the sidebar |
| Windows | Index and name for at most five visible windows; the visible range stays centered around the active window, and the overflow menu reaches the rest |
| Window markers | Small bell, activity, and Agent indicators derived from the model; daemon selection clears the latched attention state |
| Agents | Count of live Agent panes in the attached session; hidden at zero |
| Host | Remote host name; absent for the local daemon |
| Update | Available version with a dot; clicking starts the existing installer |
| Clock | Minute-resolution app clock in 24-hour, 12-hour, time-and-date, or off mode |

Chrome comes from the active zz-ui theme. While the bar and clock are visible, the minute-aligned
app-shell task requests one redraw per minute, never from paint and never once per second.

# Settings

The settings are app-side presentation preferences in `zz/config`. The Interface page under the
Appearance navigation group exposes them in its **Status bar** group.

| Key | Default | Effect |
| --- | --- | --- |
| `status-show-session` | `true` | Show the attached-session chip |
| `status-badges` | `true` | Show bell, activity, and Agent window markers |
| `status-align` | `left` | Place the window strip at `left` or `center` |
| `status-agents` | `true` | Show the non-dead Agent count when nonzero |
| `status-host` | `true` | Show the remote host item |
| `status-update` | `true` | Show an available update item |
| `status-clock` | `24-hour` | Use `24-hour`, `12-hour`, `time-date`, or `off` |

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

The FFI and mobile-client phase remains separate. There is no faithful-tmux toggle in the GUI.

# Key files

- `crates/zz-client/src/status_bar.rs`: pure status-bar model and typed settings.
- `crates/zz/src/status_bar.rs`: GPUI layout, overflow, actions, update item, and clock formatting.
- `crates/zz/src/config/mod.rs`: app-side values, defaults, parsing, and live projection.
- `crates/zz/src/config/settings.rs`: Interface-page controls.
- `crates/zz/src/app_shell.rs`: minute-aligned redraw task.
- `crates/zz-tui/src/main.rs`: terminal client that retains the composed tmux status row.

# Related

- [tmux status line](/tmux/status-line.md)
- [Application configuration](/configuration/app-config.md)
- [Snapshot schema](/protocol/snapshots.md)
