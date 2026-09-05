---
type: Concept
title: tmux status rows and format expansion
description: The daemon expands tmux status formats per client for the cell-faithful TUI; GUI clients build native bars from snapshots and app settings without consuming StatusLine.
resource: crates/zz-mux/src/formats.rs
tags: [tmux, status-line, formats, gui, tui, options]
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-09-03
last_updated_by: Claude
---

# Overview

tmux draws status rows with terminal cells. zz expands the same format language in the daemon and
publishes client-specific styled data for terminal clients:

- The TUI renders the authoritative `status-format[]` block at `status-position`, shifts or shrinks
  the pane canvas, and places messages/prompts on `message-line`.
- GUI clients ignore `StatusLine`, including its title, rows, halves, styles, alignment, placement,
  and `customized` bit. They contain no tmux style parser in the status-bar path.
- The desktop builds its native bar from `zz_client::StatusBarModel`, the attached
  `MuxSnapshot`, the host tree, update state, and seven app-side settings. Sidebar mode places it
  below the workspace; titlebar mode places it above the workspace with the native controls.
- The desktop window strip uses snapshot index, name, per-client focus, bell, activity, and pane
  kind. It keeps stable-id selection, rename/close menus, five-window focus centering, and an
  overflow menu. The right side can show a live Agent-pane count, remote host, update, and clock.
- Prompts, choosers, and copy mode remain native surfaces. Compatibility covers their state and
  input behavior, not terminal escape output.

`StatusLine.customized` records whether an explicit status-related write is active for the
recipient's scope. GUI clients do not read it.

# Who expands the format

The **daemon**. Three reasons, strongest first:

1. `#(command)` must run on the daemon's host. Attached clients keep independent cache entries so
   their session and cwd contexts can differ. Unattached query clients share entries by effective
   cwd. A client-side expander would run commands on the wrong machine.
2. A client renders; it does not own mux state. `#S` is a daemon fact.
3. The wire then carries finished text, so the client needs no format engine.

The daemon sends `StatusLine` in `ServerHello` on connect and in `EventPayload::StatusChanged`
afterwards. It carries `left`, `right`, expanded rows, title, position, message-line selection, and
the customized bit. Each client gets its own value because a format names that client's view: two
clients attached to different sessions disagree about `#S`. The TUI consumes that value; GUI
clients leave it to the compatibility path.

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
unit. `status off` removes the formatted tmux rows and gives those terminal cells back to the TUI
pane canvas.

Every tmux status option affects the TUI and format consumers only. GUI status placement, items,
badges, alignment, and clock come from `zz/config`; `status off`, `status-left`, `status-right`,
`status-justify`, the style family, and the window-status family do not change the GUI bar. The
desktop still uses snapshot facts that also feed tmux formats, but it never reads the expanded
status product.

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
| `#{R:value,count}` | expand both operands through the recursive engine, then repeat the value `count` times |
| `#(uptime)` | shell command output, first line only |
| `#[fg=green,bold]` | style directives, preserved as markers (inner `#{…}`/`#()` expand like the pin); the TUI parses them into styled terminal runs |

The parser accepts semicolon modifier chains and nested bodies. It evaluates modifier arguments
before the body, uses tmux truthiness (empty and exact `0` are false), and stops recursion at 100
expansions. Scalar comparisons stay lexical. Arithmetic, loops, padding, color conversion, content
search, and ASCII conversion use the same recursive expansion and depth bound.

`R` finds the first top-level comma before the engine expands the value and count. Nested commas
stay inside their operand, and `#,` preserves a literal comma in the value. The count parser accepts
1 through 10,000 and leading whitespace, then rejects trailing whitespace or text. An invalid, zero,
negative, or oversized count produces an empty expansion. A body without a comma fails the
replacement and discards later format output. The engine runs repeat before the post
transformations, so byte length and truncation can wrap it as `#{n;R:...}` and `#{R;=/N/...:...}`.
The engine rejects a nested repeat before allocation when it would exceed 40,960,000 intermediate
bytes, which is an allocation ceiling rather than a budget.

`FORMAT_LOOP_LIMIT` is a recursion depth of 100 on both sides: 200 sibling `#{l:x}` replacements all
expand, 99 nested `#{s/x/x/:}` wrappers still reach their body, and the hundredth answers empty. The
pin's second budget, `FORMAT_TIME_LIMIT`, is 100 milliseconds of wall clock that abandons the rest of
an expansion. zz does not adopt it, so a runaway expansion runs to completion and answers the same
string every time where the pin returns a truncated result whose length moves run to run. The
[divergence matrix](/tmux/divergences.md) records the measurement behind that decision.

Nothing clamps a finished expansion. `MAX_STATUS_TEXT_BYTES` is the wire bound `StatusLine`
enforces, so the daemon applies it to the title, the base style, and each status row as it builds
that message; command-facing output never rides it and prints whole.

The shipped second and third `status-format[]` defaults use `R` with `n` to indent their `P:` and
`S:` rows. `n` returns the session name's byte length, and the expander emits that many spaces
without leaking literal `R` syntax into either row.

## Current compatibility checkpoint

At the 2026-09-02 checkpoint, zz implements all 36 of the pin's format modifiers.
`formats.modifier-fidelity` closed on 2026-09-02, and no modifier token is still tracked. The live
registry has 42 active groups, 453 active items, and 174 closed records: 0 open, 0 blocked, and 42
accepted. No group remains unresolved. Closed records plus accepted active groups resolve 216 of
216 known groups (100.0%). The persisted accepted artifact covers 220 scenarios and 2,648 steps with
three registered known rows carrying GEO differences, every other channel clean, and SHA-256
`5bef958b6945d2d07d39ab0409e47589e486e43695a458392fbae2957bfb4c1c`; its attached-client footer still reads `PASS` and is stale. The 45-step
`formats-values` row passes as part of that artifact. The source partition contains 99 direct mux
values, 44 daemon-delegated values, and 55 active gaps.

Source Replay V3 contributes a 60-step clean row for located source diagnostics, physical command
groups, Command stderr, Control frames, and detached callback shutdown order. The ten-step
`pane-border-span-owner` row proves raw zz-tui's adjacent-span ownership without changing the format
partition, protocol, snapshots, or GPUI theme ownership.

The historical 10aa checkpoint covered 101 scenarios and 1,550 steps with a 28-step
`formats-values` row, attached-client `PASS`, and SHA-256
`bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`.

`window_activity` now reads a Unix-second timestamp stored apart from zz's logical window-order
counter. Window creation, parsed nonempty pane output, and the pinned current-window transition
paths refresh it. Same-window selection, pane selection, pane creation, splits, and layout-only
changes without output leave it unchanged. An independent audit found the direct daemon
`switch-client` path and added its missing engine-clock refresh before selection. Plain, boolean,
comparison, list-row, and time-modified forms all read the same stored seconds. That format change
did not alter the protocol or snapshot schema.

Slice 10ac closes clean child environments for shell-form `run-shell`, shell-form `if-shell`, and
status `#()`. Status jobs start from an empty process environment and receive the modeled global
overlay only. Hidden values and unset markers stay absent. A visible modeled `TMUX_PANE` survives,
but zz does not create one. After startup, status jobs set `TERM` from `default-terminal`,
`TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION=3.8-zz`, `COLORTERM=truecolor`, and
`TMUX=socket,pid,-1`. The private tmux executable remains first on the modeled PATH. The attached
fixture proves this global-only path. Slice 10af later closes positive-delay `run-shell`
environment sampling. `copy-pipe` and popup job environments retain separate owners.

The 2026-08-30 `jobs.shell-job-cwd` closure aligns cwd selection for shell-form `run-shell`,
shell-form `if-shell`, and status `#()`. `run-shell -c` wins first. Both command paths then check
the startup client cwd, an unattached provenance client's cwd, the selected target session, and the
invoking client's attached session before falling back to `HOME` and then `/`. Positive-delay jobs
retain the selected path before the timer and apply the existing-path fallback when the child
starts. Status jobs use the attached session's retained path. Attached clients keep independent
command caches, while unattached query clients share entries by effective cwd. Ten focused daemon
shell-job tests and 32 status tests pass. The three-step
`smoke/jobs-shell-job-cwd` row completes eight checks per engine with no differing channel. The
attached fixture covers 24 real cases across Interactive and Control clients, `run-shell` and
`if-shell`, and valid, missing, and omitted targets. The final strict and attached aggregate passed
at 105 scenarios and 1,675 steps.

Slice 10ad closes `tracker.semantic-coverage/semantic:tracker-option-consumer-registration`. The
unchanged 105-name roster now lives in `command::TMUX_OPTION_CONSUMERS`, while `BEHAVES` remains an
alias. An exact source guard partitions all 180 pinned options into those 105 consumers and 75 live
option gaps, and verifies the closed tracker record. `copy-mode-mark-style` appears only because
status option-variable expansion consumes its value; this closure does not claim that any client
renders a visual copy-mode mark. The compatibility gate passes 445 mux tests plus three daemon
inventory tests. Full workspace tests and clippy, formatting, diff, tracker, and checked-summary
checks pass. The slice changes no runtime behavior or accepted artifact.

Slice 10ae closes
`options.option-name-format-coverage/semantic:option-name-format-coverage`. The format expander
checks all 105 behavior-consumer names before format-table, command-item, and environment values.
The roster divides into 13 server, 42 session, 40 window, and 10 pane names. Exact names and legacy
aliases resolve through selected targets, attached-client fallback, active children, inheritance,
and `S`, `W`, and `P` loop retargeting. Command prefixes stay outside option lookup.

Flags render as `0` or `1`; other types preserve their tmux spelling. Whole-array and indexed
lookup covers `command-alias`, `status-format`, and `update-environment`. Whole arrays emit numeric
entries before named entries, numeric indices normalize leading zeroes, and malformed or missing
indices expand empty. A local array shadows its inherited array as one unit.

Mux-owned expansion reads live state, and each direct daemon format producer calls the same live
resolver. Detached status creates one all-scope option snapshot per refresh batch and shares it
across client rows. Missing-target `run-shell -C` and `if-shell -F` read global options while their
inserted command or selected branch keeps the caller context. The focused 60-step
`option-name-formats` differential has zero differences in every channel, and the attached status
probe passes. Exhaustive mux and daemon tests cover the roster, arrays, targets, loops, producer
inventory, and detached refresh. The slice changes no protocol, wire snapshot, or native GUI
styling.

The post-10ae rerank freezes slice 10af on
`jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`.
`source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd` follows it.

The earlier forecast understated `w`. Pinned `format_width` parses leading hashes and `#[...]`
styles, returns zero for malformed style markup, skips controls, and consumes live
`codepoint-widths[]` overrides over a 162-entry default cache. The harness builds tmux with
`--disable-utf8proc`, so uncached codepoints use the host `wcwidth` policy; zz uses `unicode-width`
0.2.2. The later `w` slice must pin those platform and Unicode cases before changing the expander.

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

`#{command}` is separate from those row facts: it names the canonical command-queue item currently
expanding a format. Built-in aliases, unique prefixes, and one-layer user aliases therefore report
the resolved entry. Explicit item-state variables outrank that fallback. Daemon-owned commands
carry it through immediate expansion and the post-spawn `new-window`/`split-window -P -F`
re-expansion that adds live pane facts. A typed child block reports the child's command, while
confirm prompts, periodic status work, and delayed Control subscriptions expand outside an item
and leave it empty.

## Variable backing and honest defaults

The table below accounts for all 198 registered names. Live values can still be empty outside their
required scope. Daemon facts cross an explicit feed or hook; format expansion itself does no runtime
discovery. The final row is not a claim of support: every name there has its own **silent** entry in
the [divergence matrix](/tmux/divergences.md#format-variables-that-remain-unbacked).

| Backing | Names |
| --- | --- |
| Live mux/server and format-client state | `active_window_index`, `history_limit`, `host`, `host_short`, `last_window_index`, `next_session_id`, `pane_active`, `pane_at_bottom`, `pane_at_left`, `pane_at_right`, `pane_at_top`, `pane_bottom`, `pane_dead`, `pane_dead_status`, `pane_flags`, `pane_format`, `pane_height`, `pane_id`, `pane_index`, `pane_input_off`, `pane_last`, `pane_left`, `pane_marked`, `pane_marked_set`, `pane_right`, `pane_start_command`, `pane_start_command_list`, `pane_synchronized`, `pane_title`, `pane_top`, `pane_width`, `pane_x`, `pane_y`, `pane_z`, `pane_zoomed_flag`, `pid`, `server_sessions`, `session_active`, `session_activity`, `session_activity_flag`, `session_alert`, `session_alerts`, `session_attached`, `session_attached_list`, `session_bell_flag`, `session_format`, `session_id`, `session_many_attached`, `session_marked`, `session_name`, `session_path`, `session_silence_flag`, `session_stack`, `session_windows`, `socket_path`, `start_time`, `uid`, `user`, `version`, `window_active`, `window_activity`, `window_activity_flag`, `window_active_clients`, `window_active_clients_list`, `window_active_sessions`, `window_active_sessions_list`, `window_bell_flag`, `window_end_flag`, `window_flags`, `window_format`, `window_height`, `window_id`, `window_index`, `window_last_flag`, `window_layout`, `window_linked`, `window_linked_sessions`, `window_linked_sessions_list`, `window_manual_height`, `window_manual_width`, `window_marked_flag`, `window_name`, `window_panes`, `window_raw_flags`, `window_silence_flag`, `window_stack_index`, `window_start_flag`, `window_visible_layout`, `window_width`, `window_zoomed_flag` |
| Daemon config-selection feed | `config_files` (the comma-joined startup selection; native reload replaces it with the selected default path or empty, while later `source-file` calls do not append) |
| Daemon runtime feed | `pane_current_command`, `pane_current_path`, `pane_path`, `pane_start_path`, `pane_pid`, `pane_tty`, `pane_dead_signal`, `pane_dead_time`, `pane_pipe`, `pane_pipe_pid`, `pane_pb_progress`, `pane_pb_state` (the pane worker's OSC 9;4 progress bar), `session_created` |
| Terminal worker facts through daemon hooks | `history_size`, `cursor_x`, `cursor_y`, `alternate_on` (primary history survives alternate entry; pending-wrap cursor column equals pane width) |
| Daemon buffer hook | `buffer_created`, `buffer_full`, `buffer_name`, `buffer_sample`, `buffer_size` |
| Daemon superset hook (`DaemonFormatHooks::variable` answers before the pinned table, so neither name is in `FORMAT_VARIABLES` or the oracle diff) | `pane_kind` (`terminal`, `agent`, `browser`, `editor`, `picker` — what `list-panes -F` needs to find the agent), and every `@name` user option, read pane → the pane's window → session → global window → global session → server, the way tmux's `#{@name}` reads |
| Pinned tmux default, enabled | `cursor_flag`, `wrap_flag` |
| Per-client daemon hook (every attached client context carries one `ClientFormatFacts` record) | `client_activity`, `client_cell_height`, `client_cell_width`, `client_colours`, `client_control_mode`, `client_created`, `client_discarded`, `client_flags`, `client_height`, `client_key_table`, `client_last_session`, `client_name`, `client_pid`, `client_prefix`, `client_readonly`, `client_session`, `client_termfeatures`, `client_termname`, `client_termtype`, `client_theme`, `client_tty`, `client_uid`, `client_user`, `client_utf8`, `client_width`, `client_written` |
| Daemon session-attachment hook | `session_last_attached` |
| Daemon window cell-pixel hook (the format client's reported cell size, falling back to the pin's `DEFAULT_XPIXEL` 16 and `DEFAULT_YPIXEL` 32, null without a window in context) | `window_cell_height`, `window_cell_width` |
| Pin-null without the missing mouse or group context | `mouse_x`, `mouse_y`, `session_group_attached`, `session_group_many_attached`, `session_group_size`, `window_bigger` |
| Pinned inactive/default state | `alternate_saved_x`, `alternate_saved_y`, `bracket_paste_flag`, `cursor_blinking`, `cursor_shape`, `cursor_very_visible`, `history_all_bytes`, `history_bytes`, `insert_flag`, `keypad_cursor_flag`, `keypad_flag`, `mouse_all_flag`, `mouse_any_flag`, `mouse_button_flag`, `mouse_sgr_flag`, `mouse_standard_flag`, `mouse_utf8_flag`, `origin_flag`, `pane_floating_flag`, `pane_in_mode`, `pane_unseen_changes`, `scroll_region_lower`, `scroll_region_upper`, `session_grouped`, `sixel_support`, `synchronized_output_flag` |
| Always unavailable (22) | `buffer_mode_format`, `client_mode_format`, `cursor_character`, `cursor_colour`, `mouse_hyperlink`, `mouse_line`, `mouse_pane`, `mouse_status_line`, `mouse_status_range`, `mouse_word`, `pane_bg`, `pane_fg`, `pane_key_mode`, `pane_mode`, `pane_search_string`, `pane_tabs`, `session_group`, `session_group_attached_list`, `session_group_list`, `tree_mode_format`, `window_offset_x`, `window_offset_y` |

`session_path` reads the selected session's retained UTF-8 working directory at expansion time.
The differential fixture creates two sessions, preserves lexical `/tmp/..`, reads each through a
targeted display, and reads both through one filtered session list. Focused mux tests cover missing
retained or target state and the value after the production `attach-session -c` command updates one
session. Slice 10x closes the two adjacent cwd mutations. An existing `new-session -A -c` target
receives one expansion in its resolved target and invoking-client context before the attach path
stores the value. Fresh creation and an `-A` miss retain an empty session path from `-c ''` while
the initial pane keeps its donor or caller fallback.

`session_active` reads an explicit three-state `FormatClient`. A clientless producer expands it to
empty. An unattached client or a client attached to another session expands it to `0`; a client
attached to the selected session expands it to `1`. Command execution retains the raw invoking
client separately from the current or explicitly selected target client. Name and cwd expansions
can use the invoker while target-aware command formats use the selected client. Clientless lists,
filters, chooser rows, and `list-commands` remain empty. Status rows, deferred pane output, shell
callbacks, buffer and capture paths, popup and menu text, `list-keys`, Control subscriptions, and
display-panes labels carry their selected client state. The 198-name partition now contains 94
direct mux values, 32 daemon-delegated values, and 72 active constant-backed gaps. Unit,
source-file, `run-shell`, `if-shell`, per-client snapshot, and attached-client fixture proofs show
that `client_*` facts and `session_active` use the same selected client.

Eligible local terminal surfaces and Command clients retain a tty internally for attached-client
selection and nested-attach checks. Local Control retains the same identity only when stdin has a
discoverable tty; piped stdin retains none. The selected client's `ClientFormatFacts` exposes that
value to list, status, title, ordinary, inserted, and `display-message` expansion. Protocol v82's
environment snapshot supplies `TERM` when present. Control publishes no implicit size; only
explicit `refresh-client -C` state can supply Control geometry, and terminal-only fields remain
empty for piped Control.

The `ClientFocus` shape introduced in protocol v73 always retains the current focused boolean for
`client_flags`. When `focus-events` is on, it also drives activity and FocusIn geometry ownership.

`session_activity` retains Unix seconds and starts at the same timestamp as `session_created`.
Successful same- or other-session selection and queued terminal input refresh it. Every attach also
advances the terminal geometry-owner sequence and recalculates affected panes, whether or not
`focus-events` is enabled. A client with retained geometry therefore becomes `window-size latest`
on a same-session reattach; a newly attached client becomes latest as soon as its pane geometry
arrives.

Read-only `Key` messages bypass chooser, command-prompt, and `display-panes` consumption and resolve
through the ordinary root key table. Direct local scrolling and read-only-safe copy-mode navigation
refresh activity and latest geometry once without clearing the bell. Rejected shared-state actions,
including raw mouse motion, use the same accounting before rejection while retaining the modal.
Pane Focus is rejected without activity because `ClientFocus` owns the client-window transition.
Writable chooser input is different because the native choosers are pane
modes: raw keys, dedicated actions, and terminal-view input each refresh activity exactly once but
also advance latest geometry without clearing bells. A chooser activation into another session
then records the target attach as a second legitimate activity boundary. Raw chooser routing is
client-scoped, so a peer's key does not operate another client's chooser. Read-only-safe local view
actions bypass a retained chooser or `display-panes` overlay, reach the pane, and account once while
leaving the modal and bell intact. Writable `display-panes` consumes a valid selection and bare
buttonless hover Motion without activity. An unmatched key, Escape, non-hover mouse action, or wheel
closes the overlay and falls through ordinary input; timeout closes it without fabricating activity.

Client-window focus is separate from pane/application focus through the `ClientFocus` shape
introduced in protocol v73. When the server `focus-events` option is on, both directions update the retained session and client
activity facts exactly once. FocusIn additionally becomes the geometry owner and resizes visible
terminal panes; `window-size latest` takes that owner's rows, columns, and cell metrics, while
manual, largest, and smallest keep their mode-correct rows and columns but refresh the owner's cell
metrics. FocusOut does not change geometry ownership.
Read-only clients use the same activity path, but zz does not couple read-only with tmux's
`ignore-size` flag. The read-only regression proves zz accepts the notification and updates
activity; it does not prove tmux `attach -r` resize behavior. A zz-side writable two-client
regression mirrors the pinned FocusIn/latest rows-and-columns rule, but `ClientFocus` is not
CLI-drivable, so this is not a differential-harness proof. Neither direction clears a pane bell.
Writable `TerminalViewAction::Focus` only reaches the terminal application and changes neither
activity nor geometry; pairing the two signals still records one activity update. Read-only pane
Focus is rejected, while its client-window activity uses `ClientFocus`. The client-focus activity
path is inert when the server option is off. Writable focus first dismisses an active status message
and resumes any frozen terminal publication, then closes `display-panes` before either direction
reaches activity accounting. Text, Single, Incremental, and BackspaceExit prompts consume the
transition and stay open. Key prompts submit `FocusIn` or `FocusOut` and consume it; Numeric prompts
submit their buffer without recording prompt history and pass it into ordinary focus accounting.
Native chooser pane modes and read-only clients bypass the writable prequeue, so those modals and
messages stay open. The `client-focus-in` or `client-focus-out` report hook still fires when
`focus-events` is off; that option gates application focus forwarding and activity accounting, not
the retained client report. A FocusIn that changes both latest geometry and an activity-sorted chooser
publishes the snapshot and independently refreshes the chooser. Writable
command-prompt consumed key or text input does not refresh activity. Committed text uses a bounded
ordered queue per client; every entry records its pane and input lane. A press or repeat `Key` with
`text_follows: true` records the pending pair. `Text`
scans forward to the first entry for the same pane and lane, retires only the skipped prefix, and
consumes that match while preserving its suffix. The matched Key's modal and authorization result
wins without a second activity or latest-geometry update. Empty matching text is inert and retires
its dispatch suppression. If no entry matches, the queue stays intact and nonempty text is
standalone. Writable standalone text reaches chooser, prompt, and `display-panes` before activity;
terminal command-output text accounts once before it is swallowed, while browser command-output
text is consumed before activity. Standalone read-only terminal text
accounts once without clearing a bell or writing the PTY; read-only browser text retains zz's
silent drop. The queue clears on detach, unregister or reconnect, and successful wire attach, but
survives a binding-driven synchronous `switch-client` so its trailing text still belongs to the
key. Native client-theme notifications, resize, key-table-only switches, and detached commands leave
activity unchanged. A separate logical counter drives `list-sessions -O activity` and `S/t`, so
same-second touches still produce deterministic MRU order with the session name breaking exact
ties. Native browser input outside modal consumption keeps its existing superset activity behavior.
The absent tmux suspend/wake lifecycle is accepted under
`formats.session-activity-wake-lifecycle`, not left as an unnamed TODO. Synthetic focus `Any`
dispatch now follows writable modal prequeue, activity and FocusIn-only latest geometry, chooser or
copy-mode `Any`, then effective-root `Any`. An unbound transient table falls back without closing
that mode. Read-only focus retains its modal bypass and authorizes the whole selected binding before
any effect. Exact `FocusIn` and `FocusOut` remain invalid as bindable key names. Pane-rendered
`command-prompt -P` remains under `prompt.pane-rendered`.

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
  truncation, and emits semantic hit ranges. GUI clients ignore all three expanded products and
  build native window items from snapshot fields instead.

# When the status re-renders

| Trigger | `#()` commands |
| --- | --- |
| `status-interval` tick | re-run |
| a mux snapshot changes (rename, split, focus, attach) | reuse matching scope/cwd/command; run on a miss |
| a `status-*` option changes | reuse matching scope/cwd/command; run on a miss |
| a client connects | reuse the unattached scope; run on a miss |

Timer ticks refresh active command keys. Other renders expand strings and start a command only when
the scope, effective cwd, and command text have no cache entry. Attach and cwd changes can therefore
run a command before the next tick, while pane-title traffic with unchanged keys stays cached.

`#()` commands are bounded where tmux's are not: 2 seconds, then the child is killed and contributes
whatever it had already written. A wedged script costs one stale field instead of stalling the
daemon. The state lock is released before any command runs, and cached output for commands no format
names any more is dropped on the next tick.

Each status child starts from the modeled global environment rather than the daemon process
environment. The daemon removes hidden and unset values, uses the modeled PATH for its private tmux
executable, and applies the post-startup tmux terminal identity listed above. Status jobs do not
receive a session overlay. They run from the attached session's retained cwd, falling back to a
valid home directory and then `/` when that path does not exist.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/formats.rs` | The 198-name registry, scope backfill, scalar modifier parser, recursive expander, and `StatusHooks` seam. |
| `crates/zz-mux/src/status.rs` | `StatusFormats` and `StatusOption` state. |
| `crates/zz-daemon/src/status.rs` | `StatusRenderer`, strftime, bounded `#()` execution, buffer facts, visible-row search, and per-client diffing. |
| `crates/zz-daemon/src/daemon.rs` | Pane runtime-fact feeds, buffer-row hooks, `refresh_status`, the sampler thread, and status publication. |
| `crates/zz-client/src/status.rs` | Cell-accurate row composition, style runs, alignment, list truncation, and semantic hit ranges. |
| `crates/zz-client/src/status_bar.rs` | Pure native status-bar projection from snapshot data and typed settings. |
| `crates/zz-ui/src/navigation.rs` | Shared GPUI native status-item and window-control geometry. |
| `crates/zz/src/status_bar.rs` | Renders the desktop session, window, Agent, host, update, and clock items with actions, alignment, overflow, and chrome-mode placement. |
| `crates/zz/src/app_shell.rs` | Mounts the status surface above titlebar mode or below the full sidebar/workspace shell. |

# Related

- Options arrive through the [`.tmux.conf` parser](/tmux/conf-parser.md) and
  [`set-option`](/tmux/commands.md); the rest of the emulated surface is scoped in
  [tmux compatibility](/tmux/tmux-compat.md).
- The [TUI](/designs/tui-client.md) renders tmux rows. The desktop boundary lives in the
  [native status bar design](/designs/native-status-bar.md).
- The independent sidebar tree is described in [sidebar navigation](/tmux/choose-tree.md).
- Carried by the [wire protocol](/protocol/wire-protocol.md) as `ServerHello::status` and
  `EventPayload::StatusChanged`.
