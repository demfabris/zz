---
type: Concept
title: tmux status rows and format expansion
description: The daemon expands tmux status formats per client; the TUI draws terminal rows while the GUI maps their semantic regions onto native widgets.
resource: crates/zz-mux/src/formats.rs
tags: [tmux, status-line, formats, gui, tui, options]
timestamp: 2026-08-24T00:00:00-03:00
---

# Overview

tmux draws status rows with terminal cells. zz expands the same format language in the daemon and
publishes client-specific styled data. Each client decides how to present it:

- The TUI renders the authoritative `status-format[]` block at `status-position`, shifts or shrinks
  the pane canvas, and places messages/prompts on `message-line`.
- The GUI always owns one native status surface. Sidebar mode places it across the full window
  bottom, below both the sidebar and workspace. Titlebar mode places it across the full window top
  and replaces the old session/window tab group.
- GUI placement follows the app chrome mode, not `status-position`. The TUI remains the client for
  exact terminal placement semantics.
- The GUI does not paint `status-format[]` as terminal cells. It strips every tmux style directive,
  uses Powerline separators only to divide expanded `status-left` and `status-right` into native
  content chunks, and promotes recognized terminal, branch, clock, calendar, and zoom glyphs to
  zz-ui icons. Arbitrary text and `#()` output survive without tmux-authored appearance.
- Window controls come from the attached session's snapshot index, name, zoom, focus, and bell
  state rather than `WindowSnapshot.status_label`. They retain stable-id selection, rename/close,
  truncation, focused overflow, and menus while GPUI owns every visible state. Their active and
  hover surfaces use the translucent workspace wash, so blurred chroma keeps reading through them.
- Settings and the sidebar toggle stay together in the top chrome in both modes. In titlebar mode that cluster
  and the platform window controls take space from the native rail; the bottom bar in sidebar
  mode remains tmux-only. There is no agent rollup in the status surface.
- Prompts, choosers, and copy mode remain native surfaces. Compatibility covers their state and
  input behavior, not terminal escape output.

`StatusLine.customized` records whether an explicit status-related write is active for the
recipient's scope. It does not gate GUI status rendering and has no appearance effect there.

# Who expands the format

The **daemon**. Three reasons, strongest first:

1. `#(command)` must run **once per `status-interval`**, on the host the daemon runs on. A client-side
   expander would run every user's script once per attached client, and a
   remotely attached client would run them on the wrong machine.
2. A client renders; it does not own mux state. `#S` is a daemon fact.
3. The wire then carries finished text, so the client needs no format engine.

Clients receive `StatusLine` in `ServerHello` on connect and in
`EventPayload::StatusChanged` afterwards. It carries `left`, `right`, expanded rows, title,
position, message-line selection, and the customized bit. Each client gets its own value because a
format names that client's view: two clients attached to different sessions disagree about `#S`.

# Options

The status family follows tmux's declared server/session/window scopes and inheritance. It is
writable through the zz-owned config, `source-file`, `set-option`, and `set-window-option`.

The behavior-producing set includes:

- Block ownership: `status`, `status-format[]`, `status-position`, `status-justify`,
  `status-interval`, and `message-line`.
- Native halves: `status-left`, `status-right`, their length limits, and their styles.
- Base appearance: `status-style`, `status-bg`, and `status-fg`.
- Window labels and styles: `window-status-format`, `window-status-current-format`,
  `window-status-style`, `window-status-current-style`, `window-status-last-style`, and
  `window-status-bell-style`. The default window loop expands `window-status-separator` after each
  nonfinal item in that window's context, so a per-window override, nested format, or style marker
  belongs to the item on its left. The final item emits no separator.
- Default loop rows: `pane-status-*`, `session-status-*`, and `window-pane-*-status-format`.

`status-interval` controls periodic `#()` refresh. `0` disables the timer. Sparse
`status-format[]` indices publish blank rows, and a session array overrides the global array as one
unit. `status off` removes the formatted tmux rows. In sidebar mode the GUI drops the now-empty
bottom bar; titlebar mode keeps its top chrome for settings, layout, and platform controls. The TUI
gives the terminal cells back to the pane canvas.

The GUI translates the main status options rather than emulating the terminal layout. `status-left`
and `status-right` become bounded borderless UI-font chunks, and `status-justify` aligns the native
window group. Length limits and `status-interval` remain daemon-owned. `status-style`, every
left/right/window style, and the window-status format family remain accepted and TUI-visible but
have zero visual authority in the GUI. This includes `window-status-separator`: the daemon expands
it into authoritative rows for the TUI, while the GUI builds window controls from snapshot state.
Arbitrary `status-format[]` row geometry, multiple status
rows, `message-line`, and exact `status-position` remain TUI-only.

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
| `#[fg=green,bold]` | style directives, preserved as markers (inner `#{…}`/`#()` expand like the pin); clients parse them into terminal or GPUI styled runs |

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
| Live mux/server state | `active_window_index`, `history_limit`, `host`, `host_short`, `last_window_index`, `next_session_id`, `pane_active`, `pane_at_bottom`, `pane_at_left`, `pane_at_right`, `pane_at_top`, `pane_bottom`, `pane_dead`, `pane_dead_status`, `pane_flags`, `pane_format`, `pane_height`, `pane_id`, `pane_index`, `pane_input_off`, `pane_last`, `pane_left`, `pane_right`, `pane_start_command`, `pane_start_command_list`, `pane_synchronized`, `pane_title`, `pane_top`, `pane_width`, `pane_x`, `pane_y`, `pane_z`, `pane_zoomed_flag`, `pid`, `server_sessions`, `session_activity_flag`, `session_alert`, `session_alerts`, `session_attached`, `session_attached_list`, `session_bell_flag`, `session_format`, `session_id`, `session_many_attached`, `session_name`, `session_silence_flag`, `session_stack`, `session_windows`, `socket_path`, `start_time`, `uid`, `user`, `version`, `window_active`, `window_activity_flag`, `window_active_clients`, `window_active_clients_list`, `window_active_sessions`, `window_active_sessions_list`, `window_bell_flag`, `window_end_flag`, `window_flags`, `window_format`, `window_height`, `window_id`, `window_index`, `window_last_flag`, `window_layout`, `window_linked`, `window_linked_sessions`, `window_linked_sessions_list`, `window_manual_height`, `window_manual_width`, `window_name`, `window_panes`, `window_raw_flags`, `window_silence_flag`, `window_stack_index`, `window_start_flag`, `window_visible_layout`, `window_width`, `window_zoomed_flag` |
| Daemon config-selection feed | `config_files` (the comma-joined startup selection; native reload replaces it with the selected default path or empty, while later `source-file` calls do not append) |
| Daemon runtime feed | `pane_current_command`, `pane_current_path`, `pane_path`, `pane_start_path`, `pane_pid`, `pane_tty`, `pane_dead_signal`, `pane_dead_time`, `pane_pipe`, `pane_pipe_pid`, `session_created` |
| Daemon buffer hook | `buffer_created`, `buffer_full`, `buffer_name`, `buffer_sample`, `buffer_size` |
| Pinned tmux default, enabled | `cursor_flag`, `wrap_flag` |
| Per-client daemon hook (every status request carries the recipient's `ClientFormatFacts`) | `client_flags`, `client_height` and `client_width` (`0` without a stored size), `client_name`, `client_session`, `client_theme`, `client_uid`, `client_user` |
| `list-clients`-only injection | `client_activity`, `client_key_table`, `client_last_session`, `client_readonly`, `session_last_attached` |
| Pin-null without the missing client, mouse, or group context | `client_cell_height`, `client_cell_width`, `client_control_mode`, `client_discarded`, `client_pid`, `client_prefix`, `client_utf8`, `client_written`, `mouse_x`, `mouse_y`, `session_active`, `session_group_attached`, `session_group_many_attached`, `session_group_size`, `window_bigger` |
| Pinned inactive/default state | `alternate_on`, `alternate_saved_x`, `alternate_saved_y`, `bracket_paste_flag`, `cursor_blinking`, `cursor_shape`, `cursor_very_visible`, `cursor_x`, `cursor_y`, `history_all_bytes`, `history_bytes`, `history_size`, `insert_flag`, `keypad_cursor_flag`, `keypad_flag`, `mouse_all_flag`, `mouse_any_flag`, `mouse_button_flag`, `mouse_sgr_flag`, `mouse_standard_flag`, `mouse_utf8_flag`, `origin_flag`, `pane_floating_flag`, `pane_in_mode`, `pane_marked`, `pane_marked_set`, `pane_pb_progress`, `pane_unseen_changes`, `scroll_region_lower`, `scroll_region_upper`, `session_grouped`, `session_marked`, `sixel_support`, `synchronized_output_flag`, `window_cell_height`, `window_cell_width`, `window_marked_flag` |
| Always unavailable (32; `client_termname` has an empty-valued seam) | `buffer_mode_format`, `client_colours`, `client_created`, `client_mode_format`, `client_termfeatures`, `client_termname`, `client_termtype`, `client_tty`, `cursor_character`, `cursor_colour`, `mouse_hyperlink`, `mouse_line`, `mouse_pane`, `mouse_status_line`, `mouse_status_range`, `mouse_word`, `pane_bg`, `pane_fg`, `pane_key_mode`, `pane_mode`, `pane_pb_state`, `pane_search_string`, `pane_tabs`, `session_activity`, `session_group`, `session_group_attached_list`, `session_group_list`, `session_path`, `tree_mode_format`, `window_activity`, `window_offset_x`, `window_offset_y` |

The daemon reads `pane_current_path` from the foreground process through the operating system. It
feeds `pane_path` from the terminal's reported working directory after stripping the OSC 7
`file://` scheme and host and decoding percent escapes. The two values can differ, such as
`/private/tmp` and `/tmp` on macOS.

With effective `remain-on-exit`, the daemon freezes the final viewport and marks the pane dead in
mux state. `pane_dead` then expands to `1`; `pane_dead_status` contains a normal exit code and stays
empty for a signal or worker failure; `pane_dead_time` records the exit timestamp. Revive and both
respawn paths clear all three without changing the stable pane id or layout leaf.

`display-message` and status formats see only the newest automatic paste buffer, matching
`paste_get_top` in tmux. Named buffers get a buffer row through `list-buffers -F`. `buffer_full` is
implemented, but it is deliberately absent from the differential corpus because the pinned tmux
server crashes when that exact format is printed.

Two format rules preserve the status renderer's contract:

- **strftime runs per literal run, not over the whole expansion.** A `%` that arrives from a variable
  or from `#(date +%H)` stays unchanged. The `T` modifier requests the whole-value time pass.
- **Style directives survive expansion since the 2026-08-20 status-bar wave.** The engine keeps
  `#[…]` blocks (expanding `#{…}`/`#()` inside them like the pin), the daemon wraps each half with
  the base/side style prefix in the pin's default-stack order, trims both halves left to the
  `status-*-length` display-width budgets, and ships raw marker-bearing strings; the whole
  17-option style/window-status tranche stores with pin-exact defaults, `#{`-bearing
  style values deferring validation and `-a` appending with commas like the pin. Per-window
  expanded labels ride `WindowSnapshot.status_label` (protocol v69). The TUI's shared row compositor
  combines these labels with `status-left` and `status-right`, applies tmux cell alignment and
  truncation, and emits semantic hit ranges. The GUI parses only the left/right text, removes every
  style, and takes window labels and states from the semantic snapshot instead.

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
| `crates/zz-client/src/status.rs` | Cell-accurate row composition, style runs, alignment, list truncation, and semantic hit ranges. |
| `crates/zz-ui/src/navigation.rs` | Shared GPUI status readout and window-control geometry. |
| `crates/zz/src/status_bar.rs` | Builds the native left/window/right model, actions, alignment, overflow, chrome-mode placement, and titlebar reservations. |
| `crates/zz/src/app_shell.rs` | Mounts the status surface above titlebar mode or below the full sidebar/workspace shell. |

# Related

- Options arrive through the [`.tmux.conf` parser](/tmux/conf-parser.md) and
  [`set-option`](/tmux/commands.md); the rest of the emulated surface is scoped in
  [tmux compatibility](/tmux/tmux-compat.md).
- Rendered by the [app](/crates/zz.md) around the workspace shell; the independent sidebar tree is
  described in [sidebar navigation](/tmux/choose-tree.md).
- Carried by the [wire protocol](/protocol/wire-protocol.md) as `ServerHello::status` and
  `EventPayload::StatusChanged`.
