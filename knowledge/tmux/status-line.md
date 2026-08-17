---
type: Concept
title: tmux status line in the sidebar
description: The daemon expands status-left and status-right from the zz-owned mux.conf into text and publishes it per client; the workspace sidebar renders it as a stacked bottom section instead of a bottom bar.
resource: crates/zz-mux/src/formats.rs
tags: [tmux, status-line, formats, sidebar, options]
timestamp: 2026-08-17T00:00:00-03:00
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
and from `set-option` at runtime, and all four support `-u` (restore the zz default) and `-a`
(append, for the two format strings).

| Option | Default | Meaning in zz |
| --- | --- | --- |
| `status` | `on` | Whether the section renders at all. tmux's line counts (`2`..`5`) parse as on . one stacked section either way. |
| `status-interval` | `15` | Seconds between re-runs of `#()` and re-reads of the clock. `0` disables the periodic refresh, as in tmux. |
| `status-left` | `[#{session_name}] ` | First line of the section, matching the pinned tmux default. |
| `status-right` | `#{?window_bigger,[#{window_offset_x}#,#{window_offset_y}] ,}"#{=21:pane_title}" %H:%M %d-%b-%y` | Second line of the section, matching the pinned tmux default. |

A half that expands to nothing is dropped, and a section with no halves is not rendered. `status
off` costs no height rather than leaving an empty footer. The collapsed sidebar rail drops the section
too: it is too narrow for text.

# Supported format language

zz recognizes the pinned `format_table` vocabulary of 198 names. The registry in
`crates/zz-mux/src/formats.rs` records each name's scope, tmux value kind, and zz backing.
Unknown names expand to an empty string. Unknown modifiers follow tmux's fallback: zz treats the
whole body as a variable name, or expands a nested `#{...}` body as plain format text.

| Form | Meaning |
| --- | --- |
| `##` | a literal `#` |
| `%H:%M`, `%d-%b-%y`, … | strftime, applied to literal runs only |
| `#S` `#I` `#W` `#P` `#T` `#D` `#F` `#H` `#h` | single-character variable shorthands |
| `#{session_name}` | a variable by name |
| `#{==:a,b}`, `#{!=:a,b}`, `#{<:a,b}`, `#{>:a,b}`, `#{<=:a,b}`, `#{>=:a,b}` | bytewise string comparisons |
| `#{&&:a,b}`, `#{\|\|:a,b}`, `#{!:a}`, `#{!!:a}` | n-ary logic, negation, and truth-value normalization |
| `#{?condition,value,fallback}` | lazy condition and branch expansion; accepts more condition/value pairs |
| `#{b:name}`, `#{d:name}`, `#{n:name}`, `#{l:text}` | basename, dirname, byte length, and literal text |
| `#{q:name}`, `#{q/s:name}`, `#{q/h:name}`, `#{q/a:name}` | tmux shell, single-quote, hash, and argument escaping |
| `#{s/a/b/:name}` | global ERE substitution; the optional third argument uses `i` for case folding |
| `#{m:pattern,text}`, `#{m/r:pattern,text}`, `#{m/i:pattern,text}` | fnmatch, ERE, and case-folded matching; `p` and `z` select fuzzy results |
| `#{t:name}`, `#{t/f/%Y:name}`, `#{t/p:name}`, `#{t/r:name}`, `#{t/d:name}` | ctime, custom strftime, pretty, relative, and signed-difference time output |
| `#{E:name}`, `#{T:name}` | expand a value again; `T` runs one whole-value strftime pass first |
| `#{=20:name}`, `#{=-20:name}`, `#{=/20/...:name}` | display-cell truncation from either end, with an optional marker |
| `#{p20:name}`, `#{p-20:name}` | pad by display cells on the right or left |
| `#{e\|+\|:2,3}`, `#{e\|/\|f\|3:1,3}` | integer or floating-point arithmetic, numeric comparison, precision, and `m` modulo |
| `#{a:65}`, `#{c:red}`, `#{c/f:red}` | printable ASCII conversion, RGB lookup, and foreground/background SGR color output |
| `#{N:name}`, `#{N/w:name}`, `#{N/s:name}` | test a window name in the current session or a name in the global session set |
| `#{S:...}`, `#{W:...}`, `#{P:...}` | expand once per session, window, or pane; an optional second body formats the active row |
| `#{C:text}`, `#{C/ri:pattern}` | return the one-based visible pane row matching a glob substring or ERE; no match is `0` |
| `#(uptime)` | shell command output, first line only |
| `#[fg=green,bold]` | style directives, **dropped** |

The parser accepts semicolon modifier chains and nested bodies. It evaluates modifier arguments
before the body, uses tmux truthiness (empty and exact `0` are false), and stops recursion at 100
expansions. Scalar comparisons stay lexical. Arithmetic, loops, padding, color conversion, content
search, and ASCII conversion use the same recursive expansion and depth bound.

Variables resolve against the client's attached session, focused window, and active pane. A
session row backfills its active window and pane. A window row backfills its active pane. The
`session_format`, `window_format`, and `pane_format` flags retain the row type from before that
backfill. `display-message` resolves its target first and uses a pane row, matching tmux's
`FORMAT_TYPE_PANE`. The `window_layout` value is the checksummed cell tree that `select-layout <string>`
accepts. While zoomed, it remains the saved tiled tree and `window_visible_layout` reports the
one-pane zoom tree. On the status line, `window_active` and `pane_active` read `1` and `#F` includes `*`.
The four geometry variables always answer from the cell-authoritative layout tree: a headless
window is born at tmux's `default-size` 80x24 and reports its exact allocations, a drawn window
tracks the measuring client, and a zoomed pane reports the full window extent (tmux swaps in a
one-leaf layout during zoom; zz mirrors the observable).

## Variable backing and honest defaults

The table below accounts for all 198 registered names. Live values can still be empty outside their
required scope. Daemon facts cross an explicit feed or hook; format expansion itself does no runtime
discovery. The final row is not a claim of support: every name there has its own **silent** entry in
the [divergence matrix](/tmux/divergences.md#format-variables-that-remain-unbacked).

| Backing | Names |
| --- | --- |
| Live mux/server state | `active_window_index`, `history_limit`, `host`, `host_short`, `last_window_index`, `next_session_id`, `pane_active`, `pane_at_bottom`, `pane_at_left`, `pane_at_right`, `pane_at_top`, `pane_bottom`, `pane_flags`, `pane_format`, `pane_height`, `pane_id`, `pane_index`, `pane_last`, `pane_left`, `pane_right`, `pane_synchronized`, `pane_title`, `pane_top`, `pane_width`, `pane_x`, `pane_y`, `pane_z`, `pane_zoomed_flag`, `pid`, `server_sessions`, `session_alert`, `session_alerts`, `session_attached`, `session_attached_list`, `session_bell_flag`, `session_format`, `session_id`, `session_many_attached`, `session_name`, `session_stack`, `session_windows`, `socket_path`, `start_time`, `uid`, `user`, `version`, `window_active`, `window_active_clients`, `window_active_clients_list`, `window_active_sessions`, `window_active_sessions_list`, `window_bell_flag`, `window_end_flag`, `window_flags`, `window_format`, `window_height`, `window_id`, `window_index`, `window_last_flag`, `window_layout`, `window_linked`, `window_linked_sessions`, `window_linked_sessions_list`, `window_name`, `window_panes`, `window_raw_flags`, `window_stack_index`, `window_start_flag`, `window_visible_layout`, `window_width`, `window_zoomed_flag` |
| Daemon runtime feed | `pane_current_command`, `pane_current_path`, `pane_path`, `pane_start_path`, `pane_pid`, `pane_tty`, `session_created` |
| Daemon buffer hook | `buffer_created`, `buffer_full`, `buffer_name`, `buffer_sample`, `buffer_size` |
| Pinned tmux default, enabled | `cursor_flag`, `wrap_flag` |
| Pin-null without the missing client, mouse, pipe, group, or manual-size context | `client_cell_height`, `client_cell_width`, `client_control_mode`, `client_discarded`, `client_height`, `client_pid`, `client_prefix`, `client_readonly`, `client_uid`, `client_utf8`, `client_width`, `client_written`, `mouse_x`, `mouse_y`, `pane_pipe_pid`, `session_active`, `session_group_attached`, `session_group_many_attached`, `session_group_size`, `window_bigger`, `window_manual_height`, `window_manual_width` |
| Pinned inactive/default state | `alternate_on`, `alternate_saved_x`, `alternate_saved_y`, `bracket_paste_flag`, `cursor_blinking`, `cursor_shape`, `cursor_very_visible`, `cursor_x`, `cursor_y`, `history_all_bytes`, `history_bytes`, `history_size`, `insert_flag`, `keypad_cursor_flag`, `keypad_flag`, `mouse_all_flag`, `mouse_any_flag`, `mouse_button_flag`, `mouse_sgr_flag`, `mouse_standard_flag`, `mouse_utf8_flag`, `origin_flag`, `pane_dead`, `pane_floating_flag`, `pane_in_mode`, `pane_input_off`, `pane_marked`, `pane_marked_set`, `pane_pb_progress`, `pane_pipe`, `pane_unseen_changes`, `scroll_region_lower`, `scroll_region_upper`, `session_activity_flag`, `session_grouped`, `session_marked`, `session_silence_flag`, `sixel_support`, `synchronized_output_flag`, `window_activity_flag`, `window_cell_height`, `window_cell_width`, `window_marked_flag`, `window_silence_flag` |
| Unbacked, always empty | `buffer_mode_format`, `client_activity`, `client_colours`, `client_created`, `client_flags`, `client_key_table`, `client_last_session`, `client_mode_format`, `client_name`, `client_session`, `client_termfeatures`, `client_termname`, `client_termtype`, `client_theme`, `client_tty`, `client_user`, `config_files`, `cursor_character`, `cursor_colour`, `mouse_hyperlink`, `mouse_line`, `mouse_pane`, `mouse_status_line`, `mouse_status_range`, `mouse_word`, `pane_bg`, `pane_dead_signal`, `pane_dead_status`, `pane_dead_time`, `pane_fg`, `pane_key_mode`, `pane_mode`, `pane_pb_state`, `pane_search_string`, `pane_start_command`, `pane_start_command_list`, `pane_tabs`, `session_activity`, `session_group`, `session_group_attached_list`, `session_group_list`, `session_last_attached`, `session_path`, `tree_mode_format`, `window_activity`, `window_offset_x`, `window_offset_y` |

The daemon reads `pane_current_path` from the foreground process through the operating system. It
feeds `pane_path` from the terminal's reported working directory after stripping the OSC 7
`file://` scheme and host and decoding percent escapes. The two values can differ, such as
`/private/tmp` and `/tmp` on macOS.

`display-message` and status formats see only the newest automatic paste buffer, matching
`paste_get_top` in tmux. Named buffers get a buffer row through `list-buffers -F`. `buffer_full` is
implemented, but it is deliberately absent from the differential corpus because the pinned tmux
server crashes when that exact format is printed.

Two format rules preserve the status renderer's contract:

- **strftime runs per literal run, not over the whole expansion.** A `%` that arrives from a variable
  or from `#(date +%H)` stays unchanged. The `T` modifier requests the whole-value time pass.
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
| `crates/zz-mux/src/formats.rs` | The 198-name registry, scope backfill, scalar modifier parser, recursive expander, and `StatusHooks` seam. |
| `crates/zz-mux/src/status.rs` | `StatusFormats` and `StatusOption` state. |
| `crates/zz-daemon/src/status.rs` | `StatusRenderer`, strftime, bounded `#()` execution, buffer facts, visible-row search, and per-client diffing. |
| `crates/zz-daemon/src/daemon.rs` | Pane runtime-fact feeds, buffer-row hooks, `refresh_status`, the sampler thread, and status publication. |
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
