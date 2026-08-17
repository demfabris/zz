---
type: Reference
title: tmux divergence matrix
description: "Every known divergence from tmux at the pinned reference commit: the 38 missing commands and why, behavioral gaps on the implemented surface, the 12-of-180 options coverage, and the protocol-level differences — the drop-in-replacement gap, enumerated."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, divergences, gaps, reference]
timestamp: 2026-08-16T00:00:00Z
---

# Overview

This is the exhaustive inventory of where zz differs from tmux at the pinned reference commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, produced by the 2026-08-16 verification sweep: four
independent passes compared each implemented behavior against the fetched tmux C sources. It
complements the [compatibility philosophy](/tmux/tmux-compat.md) (the contract) and the
[superset roadmap](/designs/tmux-superset-roadmap.md) (the plan) by enumerating the actual deltas.

**State anchor:** the "implemented surface" section reflects
[PR #4](https://github.com/demfabris/zz/pull/4) (`fix/tmux-compat-hunt-v2`, merged 2026-08-16
as `53b523e`), which corrected the hunt-claim regressions — two of them implemented backwards
(`new-window -t` bare-target order, positional targets on the kill commands) plus the
`kill-session -C`, `new-session -A`, `resize-pane` attached-adjustment, `send-keys -H`/`-l`,
`copy-mode -du`, window-step-error, and boolean-case fixes.

The one-line read: everything marked **silent** below is a bug by zz's own doctrine (tmux syntax
must mean what tmux means, or error loudly); everything loud is a choice; the "genuine gaps"
command block plus `base-index` is the actual drop-in backlog.

# Missing commands (38 of ~92)

zz's engine catalog holds 59 verbs, 13 of them zz-native, so 46 tmux verbs run in
[`MuxEngine`](/tmux/commands.md); 8 more are implemented daemon-side because they need IO
(`capture-pane` and `set-`/`show-`/`list-`/`load-`/`save-`/`delete-`/`paste-buffer`). That
leaves 38 of tmux's ~92 commands absent, in three deliberate groups.

## Refused — config must never execute programs (11)

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
| `start-server` | Explicit server start; the zz daemon has its own lifecycle. |

## Superseded by native GUI chrome (8)

| Command | What it does in tmux |
| --- | --- |
| `display-popup` | Floating popup running a shell command (also an exec vector). |
| `display-menu` | Text menu drawn on the status line. |
| `confirm-before` | y/n prompt wrapped around a command. |
| `customize-mode` | Interactive options browser. |
| `choose-client` | Chooser listing attached clients. |
| `clock-mode` | Full-pane clock. |
| `refresh-client` | Force redraw / control-mode subscriptions. |
| `suspend-client` | Ctrl-Z the attaching client. |

## Genuine gaps — buildable, nothing blocks them (18)

| Command | What it does | Weight |
| --- | --- | --- |
| `switch-client` | Move the attached client to another session; scripts use `-t` constantly. | high |
| `show-options` / `show-window-options` | Read options back — how scripts read state. | high |
| `move-window` / `swap-window` | Relocate / exchange windows. | medium |
| `set-environment` / `show-environment` | Edit the session environment new panes inherit. | medium |
| `respawn-pane` / `respawn-window` | Restart the dead command in place (`remain-on-exit` workflow). | medium |
| `find-window` | Search windows by name/title/content. | medium |
| `list-clients` | Enumerate attached clients. | low |
| `list-commands` | Print the command list (trivial: the catalog exists). | low |
| `link-window` / `unlink-window` | One window shared into several sessions; zz has no linked-window model. | low |
| `resize-window` | Manual window sizing decoupled from clients. | low |
| `show-messages` | Server message log. | low |
| `clear-prompt-history` / `show-prompt-history` | Prompt history management. | low |

Plus `switch-mode`, new in the pinned tmux alongside floating panes — unassessed.

# Divergences on the implemented surface

| Where | Divergence | Loud or silent? |
| --- | --- | --- |
| `copy-mode` | `-k -H -S -s` rejected (`-e`/`-q`/`-M` — the stock-binding trio — are implemented). | loud |
| `source-file` | No `-` stdin (refused loudly), no `-F`/`-n`/`-v`. Globbing works. | loud |
| Alerts | Bell-only: `monitor-activity`/`monitor-silence` don't exist. Matches tmux defaults, ignores those configs. | **silent** |
| `select-layout main-*` with 2 panes | The pin never sizes the lone "other" pane (layout-set.c:264-269, :458-463), leaving stale geometry that fails tmux's own `layout_check`; zz sizes it (80x24 → main 80x22 + other 80x1). Deliberate: zz refuses to reproduce an upstream bug. | **silent**, zz more correct |
| `select-layout -E` on a mixed parent | The pin spreads only leaf children (layout.c `layout_cell_is_tiled`) but divides the parent's full extent among them, so a parent mixing leaves with nested nodes gets corrupt sums (observed: 40+42+39 in an 80-wide window, last pane at xoff 84). Every later operation on that corrupted window keeps diverging: one `-E` produced four geometry divergences, three downstream, so the known scenario has one causal step but the divergence is not bounded to it. zz refuses that spread and stops the walk where the pin stops. All-leaf parents are exact (48 pin fixtures + `known/known-spread-mixed.txt`). | **silent**, zz more correct |
| Zoom vs resize/split | tmux unzooms before any non-`-Z` `resize-pane` and pops zoom on `split-window` (cmd-resize-pane.c:94, cmd-split-window.c:239); zz keeps the zoom and mutates the hidden layout. | **silent** |
| Attached-GUI `#{pane_width}` | Formats report the engine's cell allocation while PTYs are still sized by client pixel measurement, so a drawn pane's format can drift a cell from `tput cols` until the client-reported window size lands. Headless is exact. | **silent**, bounded |
| `#{window_flags}` | Only `!` bell, `*` current, `Z` zoomed are emitted (in tmux's order); `#` activity, `~` silence, `-` last, `M` marked never appear — zz doesn't model those states. | **silent** |
| `send-keys -N` (no keys) | Arms the **invoking client's** count prefix; tmux stores it on the pane mode, so another client's (or a Command client's) `-N` is a silent no-op in zz. | **silent** edge |
| `send-keys -X` | `select-line`/`copy-end-of-line` ignore counts; flags written after the verb (`-X copy-selection -C`) parse as positionals; no "not in a mode" error. | **silent** |
| `send-keys -H` | Bytes `80`–`ff` refused; tmux writes the raw byte (`KeyToken::Literal` carries UTF-8). | loud |
| `new-window` | `-S` skips tmux's target-index gating and "multiple windows named" error; `-a` onto a free index gives N+1 where tmux gives N. | **silent** |
| Session `-t` | No `fnmatch` patterns (`work*`), no `=name` exact-match escape, `-t ""` errors instead of meaning the current session. | loud |
| `set -o` | Errors where tmux's `-o`+`-u` combination silently ignores `-o`; `-q` doesn't silence invalid option names. | loud |
| Brace blocks | Empty `{}` and a trailing `\;` error; tmux accepts both. | loud |
| Unguarded commands | Closed by the [drop-in plan](/designs/tmux-drop-in.md)'s phase 0: every engine command rejects options centrally from its catalog `CommandSpec` — flags tmux has at the pin but zz lacks error as unsupported (and count in config-import skip reports); flags tmux doesn't have error as invalid. Residual: the daemon-side `capture-pane`/buffer family still hand-rolls parsing. | loud |
| `bind-key` payloads | Bind-time validation covers names and flags only; positional arity and target errors still surface at keypress, and daemon-side verbs (`capture-pane`, the buffer family) bind with no validation at all. tmux validates the full argument template at bind time. | **silent** edge |
| Error strings | Several differ (zz `index in use: 2` vs tmux `create window failed: index 2 in use`). | cosmetic |

# Options: 12 of 180

tmux's `options-table.c` holds 180 named options (plus 68 hook entries) at the pin.
Implemented tmux names: `prefix`, `mode-keys`, `history-limit`, `synchronize-panes`,
`word-separators`, `buffer-limit`, `set-clipboard`, `copy-command`, `status`,
`status-interval`, `status-left`, `status-right`. (`set-option` also accepts six zz-native
names — the agent/editor/history-trickle keys — which don't count toward tmux coverage.)
Everything else is reported-and-skipped by the [conf parser](/tmux/conf-parser.md).
The ones real dotfiles set most, roughly by frequency: `base-index`, `pane-base-index`,
`renumber-windows`, `escape-time`, `mouse`, `default-terminal`, `terminal-overrides`,
`set-titles`, `automatic-rename`, `aggressive-resize`, `remain-on-exit`, `monitor-activity`,
`display-time`, `repeat-time`, and the whole `*-style` family (styles are on the never-list).

# Protocol and process level

| Area | tmux | zz |
| --- | --- | --- |
| Env contract | `$TMUX`, `$TMUX_PANE` | `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` — every `[ -n "$TMUX" ]` script sees "not in tmux". |
| Binary argv | `-L -S -f -2 -C -u` | `--socket`, `--host`; none of tmux's binary flags. |
| Control mode `-CC` | What iTerm2 integration speaks. | Never — zz owns both ends. |
| Layout strings | `select-layout` accepts serialized layouts; tmux-resurrect depends on them. | Never — `LayoutNode` with stable `SplitId`s is richer; dump/restore, if ever, is native. |
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
