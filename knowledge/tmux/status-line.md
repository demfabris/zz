---
type: Concept
title: tmux status line in the sidebar
description: The daemon expands status-left and status-right from the zz-owned mux.conf into text and publishes it per client; the workspace sidebar renders it as a stacked bottom section instead of a bottom bar.
resource: crates/zz-mux/src/status.rs
tags: [tmux, status-line, formats, sidebar, options]
timestamp: 2026-07-25T00:00:00Z
---

# Overview

tmux puts a status line at the bottom of the terminal. zz has no bottom bar, and does not want one:
sessions, windows, and panes are already named by the [sidebar tree](/tmux/choose-tree.md), so a
status line would restate the tree in worse typography. What a status line *also* carries (the
clock, the date, a battery percentage, `#(kubectl config current-context)`) has no other home, and
users have already written it in `~/.tmux.conf`.

So zz honors the `status-*` options and renders their **content** in a different **shape**: the two
halves stack in the workspace sidebar's bottom section, because a sidebar is tall where a status bar
is wide.

```
  status-left  ──▶ ┌──────────────┬──────────────────┐
  status-right ──▶ │ zz at tower  │                  │
                   │  ▸ work      │      panes       │
                   │    0: api    │                  │
                   ├──────────────┤                  │
                   │ [work] 1:web │                  │  ◀── the status section
                   │ 82% 09:41    │                  │
                   └──────────────┴──────────────────┘
```

# Who expands the format

The **daemon**. Three reasons, strongest first:

1. `#(command)` must run **once per `status-interval`**, on the host the daemon runs on. A client-side
   expander would run every user's script once per attached client, and a
   remotely attached client would run them on the wrong machine.
2. A client renders; it does not own mux state. `#S` is a daemon fact.
3. The wire then carries finished text, so the client needs no format engine.

Clients receive `StatusLine { left, right }` in `ServerHello` on connect, and in
`EventPayload::StatusChanged` afterwards. Each client gets **its own** status, because a format names
*that client's* view: two clients attached to different sessions disagree about `#S`.

# Options

Global only. zz renders one status section per window, so a per-session or per-window status has
nothing to attach to. All four are accepted from the zz-owned `zz/mux.conf`, from `source-file`,
and from `set-option` at runtime, and all four support `-u` (restore the tmux default) and `-a`
(append, for the two format strings).

| Option | Default | Meaning in zz |
| --- | --- | --- |
| `status` | `on` | Whether the section renders at all. tmux's line counts (`2`..`5`) parse as on . one stacked section either way. |
| `status-interval` | `15` | Seconds between re-runs of `#()` and re-reads of the clock. `0` disables the periodic refresh, as in tmux. |
| `status-left` | `[#S] ` | First line of the section. |
| `status-right` | `` "#{=21:pane_title}" %H:%M %d-%b-%y `` | Second line of the section. |

A half that expands to nothing is dropped, and a section with no halves is not rendered. `status
off` costs no height rather than leaving an empty footer. The collapsed sidebar rail drops the section
too: it is too narrow for text.

# Supported format language

A deliberate subset of tmux FORMATS, chosen from what status lines use. An unrecognized
variable expands to nothing, which is tmux's own rule.

| Form | Meaning |
| --- | --- |
| `##` | a literal `#` |
| `%H:%M`, `%d-%b-%y`, … | strftime, applied to literal runs only |
| `#S` `#I` `#W` `#P` `#T` `#D` `#F` `#H` `#h` | single-character variable shorthands |
| `#{session_name}` | a variable by name |
| `#{=20:pane_title}` | keep the first 20 characters; `=-20:` keeps the last |
| `#{?window_zoomed_flag,Z,-}` | conditional on a variable being truthy; `!` negates; branches are formats |
| `#(uptime)` | shell command output, first line only |
| `#[fg=green,bold]` | style directives, **dropped** |

Variables resolve against the client's current view (its attached session, that session's active
window, that window's active pane): `session_name`, `session_windows`, `window_index`, `window_name`,
`window_panes`, `window_width`, `window_height`, `window_active`, `window_flags`,
`window_zoomed_flag`, `pane_index`, `pane_id`, `pane_title`, `pane_width`, `pane_height`,
`pane_active`, `pane_synchronized`, `host`, `host_short`.

Two decisions:

- **strftime runs per literal run, not over the whole expansion.** A `%` that arrives from a variable
  or from `#(date +%H)`'s own output is left alone, so the command sees its `%H` intact and its output
  is not re-read as a format.
- **Style directives are dropped rather than rejected.** A config full of `#[fg=colour234]` renders
  its text in the sidebar's own muted foreground instead of failing. zz's chrome takes its colors from
  the app palette, not from per-config escape styling.

# When the status re-renders

| Trigger | `#()` commands |
| --- | --- |
| `status-interval` tick | re-run |
| a mux snapshot changes (rename, split, focus, attach) | reused from cache |
| a `status-*` option changes | reused from cache |
| a client connects | reused from cache; run once if never cached |

Only the tick spawns processes. Everything else is string expansion over cached output, so pane-title
traffic cannot turn into a process storm. Between ticks the clock is as stale as `status-interval`
allows, the same bound tmux has.

`#()` commands are bounded where tmux's are not: 2 seconds, then the child is killed and contributes
whatever it had already written. A wedged script costs one stale field instead of stalling the
daemon. The state lock is released before any command runs, and cached output for commands no format
names any more is dropped on the next tick.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/status.rs` | `StatusFormats`/`StatusOption` option state, `StatusContext` variables, and the pure `expand_status` parser behind the `StatusHooks` seam. |
| `crates/zz-daemon/src/status.rs` | `StatusRenderer` . strftime via chrono, bounded `#()` execution with an output cache, per-client diffing, and `status_context` from a snapshot. |
| `crates/zz-daemon/src/daemon.rs` | `refresh_status`, the `status-interval` sampler thread, and the `ServerHello`/`StatusChanged` publication points. |
| `crates/zz-ui/src/navigation.rs` | `workspace_sidebar_status` . the stacked, ellipsizing bottom section. |
| `crates/zz/src/workspace/sidebar.rs` | Drops empty halves, hides the section while collapsed, and repaints on `status_revision`. |

# Related

- Options arrive through the [`.tmux.conf` parser](/tmux/conf-parser.md) and
  [`set-option`](/tmux/commands.md); the rest of the emulated surface is scoped in
  [tmux compatibility](/tmux/tmux-compat.md).
- Rendered by the [app](/crates/zz.md) beneath the sidebar tree described in
  [sidebar navigation](/tmux/choose-tree.md).
- Carried by the [wire protocol](/protocol/wire-protocol.md) as `ServerHello::status` and
  `EventPayload::StatusChanged`.
