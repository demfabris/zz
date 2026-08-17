---
type: Reference
title: tmux divergence matrix
description: "Every known divergence from tmux at the pinned reference commit: the 26 missing commands and why, behavioral gaps on the implemented surface, the 16-of-180 options coverage, and the protocol-level differences."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, divergences, gaps, reference]
timestamp: 2026-08-17T00:00:00-03:00
---

# Overview

This is the exhaustive inventory of where zz differs from tmux at the pinned reference commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, produced by the 2026-08-16 verification sweep: four
independent passes compared each implemented behavior against the fetched tmux C sources. It
complements the [compatibility philosophy](/tmux/tmux-compat.md) (the contract) and the
[superset roadmap](/designs/tmux-superset-roadmap.md) (the plan) by enumerating the actual deltas.

**State anchor:** the "implemented surface" section reflects
[PR #4](https://github.com/demfabris/zz/pull/4) (`fix/tmux-compat-hunt-v2`, merged 2026-08-16
as `53b523e`) plus the phase 3d layout-string and phase 4a–4e source dated 2026-08-17. PR #4 corrected the
hunt-claim regressions, including two implemented backwards
(`new-window -t` bare-target order, positional targets on the kill commands) plus the
`kill-session -C`, `new-session -A`, `resize-pane` attached-adjustment, `send-keys -H`/`-l`,
`copy-mode -du`, window-step-error, and boolean-case fixes.

The one-line read: everything marked **silent** below is a bug by zz's own doctrine (tmux syntax
must mean what tmux means, or error loudly); everything loud is a choice; the "genuine gaps"
command block plus the remaining options grind is the actual drop-in backlog.

# Missing commands (26 of ~92)

zz's shared catalog holds 71 verbs: 14 zz-native and 57 tmux. `MuxEngine` runs 54 of the tmux
verbs, while the daemon runs the cataloged `list-clients`, `refresh-client`, and `show-messages`
because they need client or message state. The daemon also implements 8 uncataloged tmux verbs:
`capture-pane` and `set-`/`show-`/`list-`/`load-`/`save-`/`delete-`/`paste-buffer`. That leaves 26
of tmux's ~92 commands absent, in three deliberate groups.

## Refused — config must never execute programs (10)

The command catalog is the security boundary: a sourced config can only invoke cataloged
commands, so these are reported and skipped, never run. See the roadmap's never-list for the
doctrine.

| Command | What it does in tmux |
| --- | --- |
| `run-shell` | Runs any shell command from config/bindings — the bootstrap for every tmux plugin (TPM). |
| `if-shell` | Shell test picks between two commands. Parse-and-skip for import compat. |
| `set-hook` / `show-hooks` | Run commands automatically on server events. |
| `wait-for` | Lets shell scripts block until another script signals. |
| `pipe-pane` | Streams pane output into a shell command. |
| `lock-client` / `lock-server` / `lock-session` | Lock by spawning an external lock program. |
| `server-access` | Per-user ACLs for a shared server socket. |

## Superseded by native GUI chrome (7)

| Command | What it does in tmux |
| --- | --- |
| `display-popup` | Floating popup running a shell command (also an exec vector). |
| `display-menu` | Text menu drawn on the status line. |
| `confirm-before` | y/n prompt wrapped around a command. |
| `customize-mode` | Interactive options browser. |
| `choose-client` | Chooser listing attached clients. |
| `clock-mode` | Full-pane clock. |
| `suspend-client` | Ctrl-Z the attaching client. |

## Genuine gaps — buildable, nothing blocks them (8)

| Command | What it does | Weight |
| --- | --- | --- |
| `switch-client` | Move the attached client to another session; scripts use `-t` constantly. | high |
| `respawn-pane` / `respawn-window` | Restart the dead command in place (`remain-on-exit` workflow). | medium |
| `link-window` / `unlink-window` | One window shared into several sessions; zz has no linked-window model. | low |
| `resize-window` | Manual window sizing decoupled from clients. | low |
| `clear-prompt-history` / `show-prompt-history` | Prompt history management. | low |

Plus `switch-mode`, new in the pinned tmux alongside floating panes — unassessed.

# Divergences on the implemented surface

| Where | Divergence | Loud or silent? |
| --- | --- | --- |
| `find-window` | Detached CLI calls validate the target and return success with no output, including for zero matches. zz does not open tmux's attached-client window-tree chooser. | **silent**, bounded |
| `list-commands` | zz lists implemented commands in tmux's line format. Each usage string reports zz's accepted flags, so affected rows differ from the pin. Unimplemented commands stay absent so feature probes can take their fallback path. | **silent**, deliberate |
| `refresh-client` | Detached command clients receive tmux's exact `no current client` error. zz does not implement attached-client redraws or control-mode subscriptions. | loud |
| `copy-mode` | `-k -H -S -s` rejected (`-e`/`-q`/`-M` — the stock-binding trio — are implemented). | loud |
| `source-file` | No `-` stdin (refused loudly), no `-F`/`-n`/`-v`. Globbing works. | loud |
| `show-options` on unimplemented names | Catalogued scalar options without zz storage have no invented value and print nothing, including under `-A`. Bare and indexed array spellings use the same empty-success path. Implemented scalars and every stored `@` user option retain normal scope and inheritance readback. | **silent**, honest omission |
| Array options | zz parses tmux's `name[index]` grammar but stores and renders none of the pin's 76 array options. Bare and indexed set/show requests succeed with no output. Indexed `@` or table scalars follow tmux: set returns `not an array`, while show reads the scalar through the indexed spelling. | **silent**, honest omission |
| `history-limit` default | zz keeps 10,000 lines for its product default; the pin keeps 2,000. `show-options -g history-limit` prints the effective 10,000 value. | **silent**, deliberate |
| Plain option listings | No-argument listings contain tmux table names and `@` user names. The six zz-native settings stay available through explicit-name queries and never appear as unknown words in tmux-parsing scripts. | **silent**, zz extension hidden from tmux listings |
| Session environment updates | Both servers seed their global environment at boot. tmux copies each `update-environment` name from the creating client's environment; zz has no client-environment field and copies from the daemon's boot environment. They differ when the daemon outlives the shell that started it. Missing names become unset markers on both. | **silent**, bounded |
| Non-UTF-8 command arguments | tmux prints a byte such as `a\377b` with octal vis escaping. zz converts argv with `to_string_lossy` before escaping and prints `a<U+FFFD>b`. | **silent**, accepted edge |
| Alerts | Bell-only: `monitor-activity`/`monitor-silence` don't exist. Matches tmux defaults, ignores those configs. | **silent** |
| `select-layout main-*` with 2 panes | The pin never sizes the lone "other" pane (layout-set.c:264-269, :458-463), leaving stale geometry that fails tmux's own `layout_check`; zz sizes it (80x24 → main 80x22 + other 80x1). Deliberate: zz refuses to reproduce an upstream bug. | **silent**, zz more correct |
| `select-layout -E` on a mixed parent | The pin spreads only leaf children (layout.c `layout_cell_is_tiled`) but divides the parent's full extent among them, so a parent mixing leaves with nested nodes gets corrupt sums (observed: 40+42+39 in an 80-wide window, last pane at xoff 84). Every later operation on that corrupted window keeps diverging: one `-E` produced four geometry divergences, three downstream, so the known scenario has one causal step but the divergence is not bounded to it. zz refuses that spread and stops the walk where the pin stops. All-leaf parents are exact (48 pin fixtures + `known/known-spread-mixed.txt`). | **silent**, zz more correct |
| `select-layout` strings with zero-sized leaves | The pin accepts a leaf with width or height zero. zz rejects it to preserve the `PANE_MINIMUM` invariant. | **loud**, zz more correct |
| `select-layout` strings with extents above `u16::MAX` | The pin accepts `70000x70000` through its `u_int` geometry. zz rejects it; `PANE_MAXIMUM` is 10000. | **loud** |
| `select-layout` strings with single-child nodes | The pin accepts a node with one child. zz requires every node to have at least two children. | **loud**, zz more correct |
| `select-layout` string validation order and depth | zz validates the whole string before trimming cells to the current pane count. The pin trims first and runs `layout_check` afterward, so a sum violation confined to a deleted cell fails only in zz. zz caps parsing at 256 levels and rejects a 300-deep string in bounded time; the live pin held 100% CPU for minutes on input around 100 levels deep. This validation-order edge is one-directional: zz never accepts a string that the pin rejects. | **loud** |
| Lone-pane `select-layout` strings | On an 80x24 one-pane window, the pin accepts `a8fd,120x30,0,0,0`, keeps `window_width=80`, and dumps `120x30` from its new root. zz adopts the encoded extent for both the window and layout. | **silent**, zz more correct |
| Detached `split-window` while zoomed | tmux pops zoom before the split (cmd-split-window.c:239). zz preserves zoom for `split-window -d` while it changes the hidden layout. A focused split and every non-`-Z` `resize-pane` unzoom first on both sides. | **silent** |
| `move-pane` on tiled panes | The pin reserves `move-pane` for floating targets and returns `pane is not floating` for a tiled target (cmd-join-pane.c:428-431). zz has no floating panes and keeps `move-pane` as an alias of `join-pane`. | loud |
| Attached-GUI `#{pane_width}` | Formats report the engine's cell allocation while PTYs are still sized by client pixel measurement, so a drawn pane's format can drift a cell from `tput cols` until the client-reported window size lands. Headless is exact. | **silent**, bounded |
| `#{window_flags}` | zz emits `!` bell, `*` current, `-` last, and `Z` zoomed in tmux order. `#` activity, `~` silence, and `M` marked remain absent because zz does not model those states. | **silent** |
| `send-keys -N` (no keys) | Arms the **invoking client's** count prefix; tmux stores it on the pane mode, so another client's (or a Command client's) `-N` is a silent no-op in zz. | **silent** edge |
| `send-keys -X` | `select-line`/`copy-end-of-line` ignore counts; flags written after the verb (`-X copy-selection -C`) parse as positionals; no "not in a mode" error. | **silent** |
| `send-keys -H` | Bytes `80`–`ff` refused; tmux writes the raw byte (`KeyToken::Literal` carries UTF-8). | loud |
| `new-window` | `-S` skips tmux's target-index gating and "multiple windows named" error. | **silent** |
| Unguarded commands | Closed by the [drop-in plan](/designs/tmux-drop-in.md)'s phase 0: every engine command rejects options centrally from its catalog `CommandSpec` — flags tmux has at the pin but zz lacks error as unsupported (and count in config-import skip reports); flags tmux doesn't have error as invalid. Residual: the daemon-side `capture-pane`/buffer family still hand-rolls parsing. | loud |
| `bind-key` payloads | Bind-time validation covers names and flags only; positional arity and target errors still surface at keypress, and daemon-side verbs (`capture-pane`, the buffer family) bind with no validation at all. tmux validates the full argument template at bind time. | **silent** edge |
| Daemon boot | The zz daemon creates a default session `0` at spawn (the GUI's empty workspace expects one); a tmux server boots empty until its first `new-session`. `tmux ls` scripts see one extra session, and every `$N`/`@N`/`%N` id is shifted by the auto-session's objects — which is why the harness diffs layout-string structure with pane numbers stripped. | **silent** |

## Format variables that remain unbacked

These names are registered so parsing matches the pinned 198-name table, but zz still returns an
empty string where tmux can return data. Each gap is separate on purpose: none of them is hidden
inside a generic “unsupported formats” claim.

| Variable | Missing backing | Loud or silent? |
| --- | --- | --- |
| `buffer_mode_format` | No tmux buffer-mode row formatter; zz's buffer chooser is native. | **silent** |
| `client_activity` | Client activity time is not fed into format expansion. | **silent** |
| `client_colours` | The attaching client's terminal color count is not fed into format expansion. | **silent** |
| `client_created` | Client creation time is not retained as a format fact. | **silent** |
| `client_flags` | tmux client flags have no zz format projection. | **silent** |
| `client_key_table` | The per-client key engine does not expose its active table as a format fact. | **silent** |
| `client_last_session` | The client's previous session is not retained as a format fact. | **silent** |
| `client_mode_format` | No tmux client-mode row formatter; zz's client surfaces are native. | **silent** |
| `client_name` | `list-clients` supplies the registry name; status-line expansion still lacks a client row context. | **silent** |
| `client_session` | `list-clients` supplies the attached session name; status-line expansion still lacks a client row context. | **silent** |
| `client_termfeatures` | Terminal feature negotiation is not represented as a tmux format string. | **silent** |
| `client_termname` | The attaching client's `TERM` name is not retained as a format fact. | **silent** |
| `client_termtype` | tmux's terminal-type classification has no zz equivalent. | **silent** |
| `client_theme` | `list-clients` supplies the retained light/dark theme; status-line expansion still lacks a client row context. | **silent** |
| `client_tty` | Native and remote zz clients do not provide a tmux attach-client TTY path. | **silent** |
| `client_user` | `list-clients` uses the daemon user because the local socket does not retain a separate attach-client user. | **silent**, bounded |
| `config_files` | The config loader does not retain tmux's printable loaded-file list. | **silent** |
| `cursor_character` | The glyph under the terminal cursor is not mirrored into mux facts. | **silent** |
| `cursor_colour` | Cursor color is not mirrored into mux facts. | **silent** |
| `mouse_hyperlink` | Command formats do not receive tmux's mouse-event hyperlink record. | **silent** |
| `mouse_line` | Command formats do not receive tmux's mouse-event line text. | **silent** |
| `mouse_pane` | Command formats do not receive tmux's mouse-event pane id. | **silent** |
| `mouse_status_line` | Command formats do not receive tmux's mouse-event status-line index. | **silent** |
| `mouse_status_range` | Command formats do not receive tmux's mouse-event status range. | **silent** |
| `mouse_word` | Command formats do not receive tmux's mouse-event word text. | **silent** |
| `pane_bg` | The terminal cell background at the cursor is not mirrored into mux facts. | **silent** |
| `pane_dead_signal` | Exited panes are removed; zz has no retained remain-on-exit signal fact. | **silent** |
| `pane_dead_status` | Exited panes are removed; zz has no retained remain-on-exit status fact. | **silent** |
| `pane_dead_time` | Exited panes are removed; zz has no retained remain-on-exit timestamp. | **silent** |
| `pane_fg` | The terminal cell foreground at the cursor is not mirrored into mux facts. | **silent** |
| `pane_key_mode` | Native copy/view mode is not projected as tmux's pane key mode. | **silent** |
| `pane_mode` | Native pane mode is not projected as tmux's mode name. | **silent** |
| `pane_pb_state` | Terminal progress-bar state is not mirrored into mux facts. | **silent** |
| `pane_search_string` | Native per-view search text is not mirrored into mux facts. | **silent** |
| `pane_start_command` | The original spawn command is not retained in the pane format record. | **silent** |
| `pane_start_command_list` | The original spawn argv is not retained in the pane format record. | **silent** |
| `pane_tabs` | Terminal tab stops are not mirrored into mux facts. | **silent** |
| `session_activity` | No activity timestamp is exposed; `S/t` still sorts by creation time. The daemon separately tracks only the most-recent session needed for targetless Command-client context. | **silent** |
| `session_group` | Session groups are unsupported, so no group name exists. | **silent** |
| `session_group_attached_list` | Session groups are unsupported, so no grouped attachment list exists. | **silent** |
| `session_group_list` | Session groups are unsupported, so no member list exists. | **silent** |
| `session_last_attached` | Last-attachment time is not tracked. | **silent** |
| `session_path` | zz has no separate per-session working directory fact. | **silent** |
| `tree_mode_format` | No tmux tree-mode row formatter; zz's tree chooser is native. | **silent** |
| `window_activity` | Window activity time is not tracked; `W/t` retains window-index order. | **silent** |
| `window_offset_x` | Client viewport X offset is not fed into window formats. | **silent** |
| `window_offset_y` | Client viewport Y offset is not fed into window formats. | **silent** |

# Options: 16 of 180

tmux's `options-table.c` holds 180 named options (plus 68 hook entries) at the pin.
Implemented tmux names: `prefix`, `mode-keys`, `history-limit`, `synchronize-panes`,
`word-separators`, `buffer-limit`, `message-limit`, `set-clipboard`, `copy-command`, `status`,
`status-interval`, `status-left`, `status-right`, `base-index`, `pane-base-index`, and
`renumber-windows`. The index trio follows tmux's session/window inheritance, allocation,
targeting, format, and close-triggered renumbering behavior. (`set-option` also accepts six
zz-native names — the agent/editor/history-trickle keys — which don't count toward tmux
coverage.) `show-options` and `show-window-options` expose implemented values with tmux's exact
string escaping, value-only and inherited forms. Their no-argument listings contain tmux names and
`@` names only; explicit queries still expose zz-native settings. Free-form `@` names are pure string storage at
server, global-session, session, global-window, window, and pane scope, including append and unset;
this is the storage seam TPM and plugins use. zz parses indexed spellings for scalars and arrays,
but array storage remains an honest empty-success omission. Global and per-session environment
overlays have `set-environment`/`show-environment` readback and are merged into new terminal PTYs,
including hidden and child-unset entries. The daemon seeds the global map from its process
environment, and `new-session` copies the fixed `update-environment` names or writes unset markers.
Everything else is reported-and-skipped by the
[conf parser](/tmux/conf-parser.md).
The ones real dotfiles set most, roughly by frequency: `escape-time`, `mouse`,
`default-terminal`, `terminal-overrides`, `set-titles`, `automatic-rename`,
`aggressive-resize`, `remain-on-exit`, `monitor-activity`, `display-time`, `repeat-time`, and
the whole `*-style` family (styles are on the never-list).
The behavior-bearing options `mouse`, `escape-time`, `automatic-rename`, `aggressive-resize`,
`remain-on-exit`, and `set-titles` remain outside this wave and are tracked for wave 4f;
readback/storage does not pretend those behaviors exist.

# Protocol and process level

| Area | tmux | zz |
| --- | --- | --- |
| Env contract | `$TMUX`, `$TMUX_PANE`, plus server-seeded global and client-updated session overlays | Daemon-seeded global and session overlays reach new PTYs, and `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` identify the workspace; every `[ -n "$TMUX" ]` script still sees "not in tmux". |
| Binary argv | `-L -S -f -2 -C -u` | `--socket`, `--host`; none of tmux's binary flags. |
| Control mode `-CC` | What iTerm2 integration speaks. | Never — zz owns both ends. |
| Session groups | `new-session -t`. | Cataloged, rejected. |
| Presentation | Status line, prompts, choosers drawn as terminal escapes. | All native GPUI — `#[…]` styles dropped, `status-style`/`-format`/`-justify`/`-position` out of scope. |

# Related

- [tmux drop-in plan](/designs/tmux-drop-in.md) — the 2026-08-16 plan that closes almost every
  row in this matrix; only linked windows/session groups and real-tmux socket interop stay.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the contract these divergences are
  measured against.
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — the tier ladder and the amended
  never-list.
- [commands](/tmux/commands.md) — the implemented verb-by-verb reference.
