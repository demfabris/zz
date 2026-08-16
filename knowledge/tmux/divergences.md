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
| `copy-mode` | `-e -q -M -H -S -s` rejected; tmux's **own default bindings** use `-e`/`-q`/`-M`, so pasted stock configs hard-error. | loud |
| `source-file` | No globbing (`conf.d/*.conf` matches nothing), no `-` stdin, no `-F`/`-n`. | glob is **silent** under `-q` |
| `next/previous-window -a` | Bells never clear on window activation, so a second `-a` re-picks the same window where tmux errors. | **silent** |
| Alerts | Bell-only: `monitor-activity`/`monitor-silence` don't exist. Matches tmux defaults, ignores those configs. | **silent** |
| `resize-pane` (nested) | The ratio tree rescales sibling panes; tmux moves exactly one boundary and preserves other panes' cells. | **silent** drift |
| Pane sizes | Splits and percentages clamp to 10–90%; tmux allows down to `PANE_MINIMUM` (1 cell). | loud on `%`, silent clamp |
| `send-keys -N` (no keys) | Doesn't arm tmux's copy-mode count prefix, so tmux's stock vi digit bindings no-op. | **silent** |
| `send-keys -X` | `select-line`/`copy-end-of-line` ignore counts; flags written after the verb (`-X copy-selection -C`) parse as positionals; no "not in a mode" error. | **silent** |
| `send-keys -H` | Bytes `80`–`ff` refused; tmux writes the raw byte (`KeyToken::Literal` carries UTF-8). | loud |
| `split-window -f` | New pane numbered adjacent to the target; tmux numbers a full-size pane first/last. | **silent** |
| `new-window` | `-S` skips tmux's target-index gating and "multiple windows named" error; `-a` onto a free index gives N+1 where tmux gives N. | **silent** |
| `break-pane` | Refuses breaking a single-pane window; tmux relinks it into the destination. | loud |
| Session `-t` | No `fnmatch` patterns (`work*`), no `=name` exact-match escape, `-t ""` errors instead of meaning the current session. | loud |
| `select-window` | Accepts a positional target tmux bounds at zero arguments. | zz-lax |
| `set -o` | Errors where tmux's `-o`+`-u` combination silently ignores `-o`; `-q` doesn't silence invalid option names. | loud |
| Brace blocks | Empty `{}` and a trailing `\;` error; tmux accepts both. | loud |
| Unguarded commands | Unknown flags are still swallowed wherever no hand-rolled allowlist exists (36 allowlist sites in `command.rs` today); the systemic catalog-driven rejection is the [drop-in plan](/designs/tmux-drop-in.md)'s phase 0. | **silent** |
| `bind-key` payloads | Bound commands are stored without validation; an unsupported command inside a binding fails only at keypress and never counts as a skipped config line. | **silent** |
| Error strings | Several differ (zz `index in use: 2` vs tmux `create window failed: index 2 in use`). | cosmetic |
| `new-pane` | The pinned tmux ships its own `new-pane`/`newp` (floating panes); zz's picker verb sits on the name with different semantics — the one remaining silent third-meaning, pending a product decision. | **silent** |

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
