---
type: Research Report
title: tmux CLI compatibility and alias boundary
description: A commit-pinned inventory of the tmux command, flag, option, format, hook, key, packaging, and native zz command surfaces, with the exact boundary around alias tmux=zz.
date: 2026-08-22T16:12:36-03:00
researcher: Codex
git_commit: 202f322304a6b1717411f2bfb3e99e6c14e55e8f
branch: main
repository: demfabris/zz
topic: How close zz is to supporting alias tmux=zz while retaining a richer native GUI CLI
tags: [tmux, compatibility, cli, alias, commands, options, formats, hooks, key-tables, research]
status: complete
timestamp: 2026-08-22T16:12:36-03:00
last_updated: 2026-08-23
last_updated_by: Codex
---

# Research question

How much of pinned tmux is available through the current zz CLI, what is deliberately skipped,
what is missing, and what still prevents a practical `alias tmux=zz`?

The reference is tmux commit `d77c9dc6aa021e4bc61f0da128c591af695e6466`
(`next-3.8`). The zz source anchor is commit
[`202f322`](https://github.com/demfabris/zz/tree/202f322304a6b1717411f2bfb3e99e6c14e55e8f).

# Post-anchor implementation update

The audit below remains a reproducible snapshot of `202f322`. The implementation tranche completed
later on 2026-08-22 changed the live ledger without rewriting that historical baseline:

- Executable tmux verbs increased from 80 to 83: `show-prompt-history`,
  `clear-prompt-history` plus their aliases now execute with pin-matched rings, output, errors,
  selective clearing, and serialized file persistence; `resize-window`/`resizew` now own manual
  geometry. The recognized-unimplemented set is nine canonical verbs and four aliases; 157 of 170
  tmux spellings execute.
- All five formerly daemon-only workspace verbs have shared specs. The executable catalog is now
  102 canonical verbs: 83 tmux and 19 zz-native. Resolution, `list-commands`, completion,
  bind-time flag validation, and stored-command rendering consume that catalog.
- The installed bare `zz` launcher rewrites an empty argv to `new-session -A`, so an empty daemon
  creates and attaches `$0`/`@0`/`%0` while a live daemon attaches its current session. Explicit
  targetless `attach` and `attach-session` retain tmux's `no sessions`, exit-1 contract, including
  the standard `attach || new-session` fallback. Deterministic tests also cover simultaneous first
  attaches and a command client creating the first session at the same boundary. Attaching
  `new-session` receives the nested-session refusal before state changes.
- `new-window -b`, `unbind-key -a/-q`, and contextual `-f` filters for `kill-session -a`,
  `kill-window -a`, and `kill-pane -a` are implemented. The exact unsupported ledger fell from
  113 pairs across 29 commands to 107 across 26; the two differential slices contribute 39 clean
  steps.
- Bare `list-keys` now uses the pin's global repeat, key, and table widths. Its expanded 19-step
  differential scenario also covers `-N`, `-a`, and `-P` with deterministic prefix/root ordering.
  `#{config_files}` retains and joins startup-selected paths,
  switches to the current default selection on native reload, and reaches renderer-style
  conditionals. `#{pane_dead_time}` records retained exits and clears on revive/respawn. The live
  format inventory has 32 always-unavailable names rather than 34.
- `capture-pane` now gives `-p` sole ownership of stdout and otherwise writes the pin's named or
  automatic paste-buffer bytes, including the final newline. Its clean 23-step fixture covers
  clustered value flags, inclusive and reversed `-S`/`-E` ranges, target-scoped format expansion,
  and tmux's silent invalid/out-of-range fallback. `-T`, saved-alternate capture, and the six raw or
  metadata transports remain ledgered gaps. A fallback `-E` that reaches trailing blank viewport
  rows still differs: the pin emits those rows as newlines and zz stops at retained content.
- `resize-window` supports absolute and relative practical forms, selects a local manual sizing
  policy, exposes `window_manual_width`/`window_manual_height`, and prevents later client
  measurements from overwriting the durable layout extent. Its 16-step strict-geometry fixture
  matches the pin, including bounds and missing-target error precedence. Client-derived `-A`/`-a`
  remain loud, bringing the live unsupported ledger to 109 pairs across 27 commands.
- `join-pane -l` accepts cell and percent sizes, expands formats in the destination pane context,
  and switches from the destination pane to the whole-window sizing basis under `-f`. Its 11-step
  strict-geometry fixture matches the pin.
- `new-window` and `split-window` now accept repeated pane-local `-e NAME=VALUE` overlays and
  create live PID-less panes with `-E`. Malformed overlays are ignored, later values win, and
  `show-environment` remains unchanged. Exact refusal and target precedence match the pin. The
  25-step strict-geometry fixture is clean. This slice removed four rejected pairs; the concurrent
  `last-pane -d/-e` slice removed two more. Four micro flags and three `list-keys` selectors landed
  next. Creation-time `new-session -e` now persists last-wins overlays and reaches the first pane;
  `-E` suppresses normal `update-environment` seeding while retaining explicit overlays, and
  `new-session -A` ignores `-e` on an existing session. Its 18-step differential fixture is clean.
  That slice removed two more rejected pairs, leaving 91 pairs across 23 commands. Client-sourced
  creation and attach-time reseeding remain open, so the existing-session half of `new-session -E`
  is not claimed complete. `split-window -Z` now zooms the post-spawn active pane, including the
  existing pane under `-d`, while a successful split without `-Z` clears any prior zoom. Its
  11-step strict fixture is clean for active and zoom state. The subsequent `set-buffer -n`,
  `source-file -F`, `split-window -Z`, and `break-pane -a/-b` slices, followed by `move-pane -l`, leave the live ledger at 85
  pairs across the same 23 commands.
- The canonical differential inventory now covers all 71 scenarios and 1,058 executable steps.
  The complete strict run leaves every ordinary row clean; each known row has exactly one GEO
  difference and no TOPO, FMT, OUT, or WARN difference, and the attached-client fixture passes.
  `compat/run.sh --check-summary` detects path or step drift; partial and failed runs cannot replace
  the full report.
- `compat/attached-client.sh` supplies the previously missing real-client proof and now runs in the
  strict Linux CI contract. Pinned tmux drives
  an inner zz attach and an inner tmux attach through isolated PTYs; the live fixture proves root,
  prefix, prefix2, copy mode, rename prompt, tree row keys, buffer paste/deletion, and exact nested
  attach refusal with bounded semantic polling.
- The Cargo launcher pair passes bare/new/attach across empty/existing daemons from a path with
  spaces. `compat/packaged-cli.sh` separately verifies a freshly built CEF app and development
  signature, clones the whole bundle beneath a spaced path, and passes the same six cases through
  the real `Contents/MacOS/cli`; the macOS CI leg repeats that packaged smoke. The attached fixture
  also exposed a queued-yank/cancel ordering
  race; exiting copy sessions now remain owned until the terminal confirms live mode.

The remaining live plan and ease ranking are maintained in the
[tmux-compatible CLI and native superset roadmap](/designs/tmux-superset-roadmap.md). The sections
below deliberately retain the commit-pinned counts so later readers can distinguish evidence at the
anchor from work completed afterward.

# Verdict

zz already has a broad tmux-shaped CLI, but it is not a drop-in alias yet.

- 80 of 92 tmux commands execute.
- All 78 tmux aliases resolve. Seventy-one lead to executable commands and seven lead to
  recognized-but-unimplemented commands. All 170 canonical-plus-alias spellings are recognized;
  151 execute and 19 reject as unsupported.
- 51 of those 80 commands have no catalog-declared missing tmux flag. This is not a claim of
  byte-exact behavior; several accepted forms still diverge.
- 29 commands reject 113 flag pairs that the pin accepts.
- All 180 named options store and read back. 104 have a behavior consumer; 76 are storage-only.
- All 198 registered format names parse. 34 are always unavailable, 13 have usable backing only in
  a client/list context, and the remaining 151 are generally backed in their intended context.
- All 68 hook names store. 57 have an automatic producer; 11 do not.
- The checked-in differential report proves 48 scenarios and 678 steps. The current tree defines
  56 scenarios and 816 executable steps, so eight scenarios are not represented in the persisted
  report.
- The installed macOS and Linux launcher makes bare `zz` mean `attach`. On an empty daemon that
  path currently stops at `no sessions`; it does not create the first session. Bare attach works
  once a session exists, and `zz new -s NAME` creates and attaches on a TTY.
- A shell alias is shell expansion state. It can be enabled in a noninteractive shell, but it does
  not affect direct process lookup such as `execvp("tmux", ...)`. Packages do not install a `tmux`
  executable. On Unix, daemon-spawned shell and status jobs receive a private PATH shim, which
  covers config and plugin jobs that zz launches itself.

The shortest honest description is: **the common command model is present, the plugin/config floor
is credible, but the launcher edge, script-visible output semantics, 113 flags, stock bindings,
and several client/process models still separate zz from a general tmux replacement.**

# Command surface

The executable catalog contains 94 verbs: 80 tmux verbs and 14 zz-native verbs. The tmux pin
contains 92 canonical verbs and 78 aliases. A separate resolver table recognizes the 12
unimplemented canonical names and their seven aliases. The source keeps that boundary explicit and
keeps unsupported flags in a build-checked ledger
([catalog](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-protocol/src/catalog.rs#L189-L209),
[ledger](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-mux/tests/catalog_floor.rs#L10-L82)).

## Executable tmux verbs (80)

`attach-session`, `bind-key`, `break-pane`, `capture-pane`, `choose-buffer`, `choose-tree`,
`clear-history`, `command-prompt`, `confirm-before`, `copy-mode`, `delete-buffer`,
`detach-client`, `display-menu`, `display-message`, `display-panes`, `display-popup`,
`find-window`, `has-session`, `if-shell`, `join-pane`, `kill-pane`, `kill-server`,
`kill-session`, `kill-window`, `last-pane`, `last-window`, `list-buffers`, `list-clients`,
`list-commands`, `list-keys`, `list-panes`, `list-sessions`, `list-windows`, `load-buffer`,
`lock-client`, `lock-server`, `lock-session`, `move-pane`, `move-window`, `new-session`,
`new-window`, `next-layout`, `next-window`, `paste-buffer`, `pipe-pane`, `previous-layout`,
`previous-window`, `refresh-client`, `rename-session`, `rename-window`, `resize-pane`,
`respawn-pane`, `respawn-window`, `rotate-window`, `run-shell`, `save-buffer`, `select-layout`,
`select-pane`, `select-window`, `send-keys`, `send-prefix`, `set-buffer`, `set-environment`,
`set-hook`, `set-option`, `set-window-option`, `show-buffer`, `show-environment`, `show-hooks`,
`show-messages`, `show-options`, `show-window-options`, `source-file`, `split-window`,
`start-server`, `swap-pane`, `swap-window`, `switch-client`, `unbind-key`, and `wait-for`.

Every alias belonging to one of these implemented verbs resolves to the same canonical command as
the pin. Command prefix resolution also follows the pin's exact-name, exact-alias, unique-prefix,
ambiguous-prefix order.

## No catalog-declared missing flags (51)

These commands have no option marked unsupported in the command catalog:

`bind-key`, `confirm-before`, `delete-buffer`, `display-menu`, `display-popup`, `find-window`,
`has-session`, `if-shell`, `kill-server`, `last-window`, `list-buffers`, `list-clients`,
`list-commands`, `list-panes`, `list-sessions`, `list-windows`, `lock-client`, `lock-server`,
`lock-session`, `move-window`, `next-layout`, `next-window`, `paste-buffer`, `pipe-pane`,
`previous-layout`, `previous-window`, `refresh-client`, `rename-session`, `rename-window`,
`respawn-pane`, `respawn-window`, `rotate-window`, `run-shell`, `save-buffer`, `select-layout`,
`select-window`, `send-prefix`, `set-environment`, `set-hook`, `set-option`,
`set-window-option`, `show-buffer`, `show-environment`, `show-hooks`, `show-options`,
`show-window-options`, `start-server`, `swap-pane`, `swap-window`, `switch-client`, and
`wait-for`.

This is a grammar statement only. `refresh-client`, `switch-client`, `show-buffer`, `bind-key`, and
some output/error paths still have accepted semantic divergences listed below.

## Recognized but not executable (12)

| Class | Commands | Current reason |
| --- | --- | --- |
| Small buildable gaps | `resize-window`, `clear-prompt-history`, `show-prompt-history` | Required state mostly exists; the command handlers do not. |
| Replaced by native UI | `choose-client`, `clock-mode`, `customize-mode`, `suspend-client` | zz has native client and configuration surfaces; suspending a persistent GUI client is not a useful product action. |
| Missing floating-pane model | `new-pane` | tmux creates a floating pane by default. zz has native floating surfaces, not tmux floating panes in mux state. |
| Missing pane-mode transition | `switch-mode` | tmux targets an ordinary pane and switches its active mode. zz's native mode ownership does not expose that command transition. |
| Permanently parked model | `link-window`, `unlink-window` | A zz window belongs to one session. Linked windows and session groups are an explicit non-goal. |
| Parked security model | `server-access` | zz is a single-user daemon and has no shared-socket ACL model; unlike linked sessions and socket interop, this has not been declared a permanent exclusion. |

The table is source-owned by `UNIMPLEMENTED_TMUX_COMMANDS`. All 19 canonical-plus-alias spellings
resolve to `Unimplemented`, not `Unknown`, but remain absent from handlers and `list-commands` so
probes can use a fallback. No pinned command spelling is missing at the resolver layer.

# Rejected tmux flags

The catalog and `the_unsupported_flag_ledger_matches_the_catalog` test enforce this exact set:

| Command | Rejected flags |
| --- | --- |
| `attach-session` | `-E -c -f -x` |
| `break-pane` | `-W -X -Y -a -b -x -y` |
| `capture-pane` | `-C -F -H -L -P -R` |
| `choose-buffer` | `-F -k -y` |
| `choose-tree` | `-F -G -h -k -y` |
| `clear-history` | `-H` |
| `command-prompt` | `-F -P -l -t` |
| `copy-mode` | `-S -k -s` |
| `detach-client` | `-E -P -t` |
| `display-message` | `-I -N -a -c -l -v` |
| `display-panes` | `-N -t` |
| `join-pane` | `-l` |
| `kill-pane` | `-f` |
| `kill-session` | `-f -g` |
| `kill-window` | `-f` |
| `last-pane` | `-d -e` |
| `list-keys` | `-1 -N -O -P -a -r` |
| `load-buffer` | `-t -w` |
| `move-pane` | `-D -L -M -P -R -U -X -Y -l -z` |
| `new-session` | `-E -X -e -f -t` |
| `new-window` | `-E -b -e` |
| `resize-pane` | `-M -T` |
| `select-pane` | `-M -P -d -e -g -m` |
| `send-keys` | `-F -K -M -R -c` |
| `set-buffer` | `-n -t -w` |
| `show-messages` | `-J -T -t` |
| `source-file` | `-F -n -t -v` |
| `split-window` | `-E -I -R -S -T -W -Z -e -k -m -s` |
| `unbind-key` | `-a -q` |

Thirty of these pairs are coupled to absent client, environment, queue, pane-prompt, floating-pane,
or linked-session models. The machine ledger does not encode that planning label, so this report
treats the distinction as a design classification rather than a tested fact:

- Client-environment reseeding: `attach-session -E`, `new-session -E`.
- Empty-pane spawning: `new-window -E`, `split-window -E`.
- Client/process policy: `detach-client -E`.
- Binary stdin: `split-window -I`.
- Linked-session or floating-pane context: all ten `move-pane` flags;
  `break-pane -W/-X/-Y/-x/-y`; `new-session -t`; `kill-session -g`; `choose-tree -G`.
- Pane prompt state: `command-prompt -P`.
- Scrollbar, mouse, pane-input, or terminal-diagnostic context: `copy-mode -S`, `send-keys -M`,
  `display-message -I`, and `show-messages -T/-t`.

`command-prompt -F/-l/-t` are also intentionally loud until zz has the corresponding multi-prompt,
full-format, and client-fanout contracts. The remaining 80 pairs are ordinary implementation work.

# Accepted syntax with different behavior

These gaps do not appear in the 113-pair ledger because the syntax is accepted, handled elsewhere,
or has no catalog marker.

- Bare `refresh-client`, `-S`, and its redraw/scroll family are unavailable. The control-client
  subscription and size forms work.
- `load-buffer -`, `save-buffer -`, and `source-file -` lack binary stdin/stdout transport.
- `show-buffer` refuses non-UTF-8 data.
- `show-options -H` is accepted but does not include stored hook rows; `show-hooks` does.
- `set-hook -B` and `show-hooks -B` are catalog-accepted but reject at execution because zz has no
  tmux format-monitor model.
- At the `202f322` source anchor, `capture-pane -p` and `-T` were inert, and
  buffer-versus-stdout routing differed; the post-anchor update above records the routing repair.
- Copy mode and both choosers require an attached interactive client. A headless tmux pane can enter
  a mode, but a headless zz pane cannot because zz owns mode state per client view.
- The native chooser has no tmux preview pane and accepts a narrower key-name vocabulary.
- `command-prompt` lacks comma-chained prompts, full `-F` expansion, client fanout, and exact label
  spacing/order in a few paths.
- `list-keys` lacks the pin's list-width padding and several selector/output modes.
- `send-keys -H` cannot inject bytes `80` through `ff` as raw bytes.
- `bind-key` does not validate all positional arity, targets, and daemon-owned payloads at bind time.
- A relative `source-file` path resolves against the daemon's cwd instead of the client's cwd.
- Config parse failure now aborts the whole file like the pin. Environment assignments reduced
  before the failure survive on both sides.
- Current window is per client in zz and per session in tmux. This is a deliberate multi-client
  extension.
- Error ordering, shell environment synthesis, version text, and a few layout guards still differ.
- Lock commands store and fire hooks but do not launch a lock program.

# Options

The option table has 180 non-hook names, and `BEHAVES` is a tested 104-name roster
([source](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-mux/src/tmux_options.rs#L761-L866),
[tests](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-mux/src/tmux_options.rs#L1335-L1370)).

## Behavior consumers (104)

`base-index`, `pane-base-index`, `renumber-windows`, `default-size`, `window-size`,
`aggressive-resize`, `history-limit`, `detach-on-destroy`, `prefix`, `mode-keys`, `key-table`,
`prefix-timeout`, `repeat-time`, `initial-repeat-time`, `prompt-history-limit`, `history-file`,
`word-separators`, `wrap-search`, `default-shell`, `default-command`, `default-terminal`,
`remain-on-exit`, `focus-events`, `allow-passthrough`, `allow-set-title`, `cursor-style`,
`cursor-colour`, `synchronize-panes`, `automatic-rename`, `automatic-rename-format`,
`bell-action`, `visual-bell`, `display-time`, `display-panes-time`, `message-limit`,
`buffer-limit`, `set-clipboard`, `copy-command`, `menu-border-lines`, `menu-border-style`,
`menu-selected-style`, `menu-style`, `popup-border-lines`, `popup-border-style`, `popup-style`,
`main-pane-width`, `main-pane-height`, `other-pane-width`, `other-pane-height`,
`tiled-layout-max-columns`, `status`, `status-interval`, `status-left`, `status-right`,
`status-left-length`, `status-right-length`, `status-left-style`, `status-right-style`,
`status-style`, `status-bg`, `status-fg`, `status-format`, `status-justify`, `status-position`,
`message-line`, `pane-status-style`, `pane-status-current-style`, `session-status-style`,
`session-status-current-style`, `window-pane-status-format`, `window-pane-current-status-format`,
`window-status-format`, `window-status-current-format`, `window-status-style`,
`window-status-current-style`, `window-status-last-style`, `window-status-bell-style`, `mouse`,
`escape-time`, `set-titles`, `set-titles-string`, `command-alias`, `update-environment`,
`exit-empty`, `exit-unattached`, `destroy-unattached`, `monitor-activity`, `monitor-bell`,
`monitor-silence`, `activity-action`, `silence-action`, `visual-activity`, `visual-silence`,
`window-status-activity-style`, `prefix2`, `display-panes-format`, `window-style`,
`window-active-style`, `mode-style`, `pane-border-style`, `pane-active-border-style`,
`copy-mode-match-style`, `copy-mode-current-match-style`, and `copy-mode-mark-style`.

Some consumers are bounded rather than exact. Examples include `window-size manual` without
`resize-window`, presentation-only automatic rename, global publication of some copy-mode styles,
and color-only handling for several style options.

## Storage-only (76)

`allow-rename`, `alternate-screen`, `assume-paste-time`, `backspace`, `clock-mode-colour`,
`clock-mode-style`, `codepoint-widths`, `copy-mode-current-line-number-style`,
`copy-mode-line-number-style`, `copy-mode-line-numbers`, `copy-mode-position-format`,
`copy-mode-position-style`, `copy-mode-selection-style`, `dark-theme-black`, `dark-theme-blue`,
`dark-theme-cyan`, `dark-theme-dark-grey`, `dark-theme-green`, `dark-theme-light-grey`,
`dark-theme-magenta`, `dark-theme-red`, `dark-theme-white`, `dark-theme-yellow`,
`default-client-command`, `display-panes-active-colour`, `display-panes-colour`, `editor`,
`extended-keys`, `extended-keys-format`, `fill-character`, `focus-follows-mouse`, `get-clipboard`,
`input-buffer-size`, `light-theme-black`, `light-theme-blue`, `light-theme-cyan`,
`light-theme-dark-grey`, `light-theme-green`, `light-theme-light-grey`, `light-theme-magenta`,
`light-theme-red`, `light-theme-white`, `light-theme-yellow`, `lock-after-time`, `lock-command`,
`message-command-style`, `message-format`, `message-style`, `pane-border-format`,
`pane-border-indicators`, `pane-border-lines`, `pane-border-status`, `pane-colours`,
`pane-scrollbars`, `pane-scrollbars-position`, `pane-scrollbars-style`, `pane-scrollbars-timeout`,
`prompt-command-cursor-colour`, `prompt-command-cursor-style`, `prompt-cursor-colour`,
`prompt-cursor-style`, `remain-on-exit-format`, `scroll-on-clear`, `status-keys`,
`switch-mode-match-style`, `terminal-features`, `terminal-overrides`, `theme`,
`tree-mode-border-style`, `tree-mode-preview-format`, `tree-mode-preview-style`,
`tree-mode-selection-style`, `user-keys`, `variation-selector-always-wide`,
`window-status-separator`, and `xterm-keys`.

Twenty-nine are explicitly low-demand or parked: the 21 theme/palette keys, `lock-after-time`,
`lock-command`, `remain-on-exit-format`, `status-keys`, and the four `tree-mode-*` keys.

# Formats

The registry contains the pin's 198 names
([registry](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-mux/src/formats.rs#L289-L500)).

Thirty-four are always unavailable. Thirty-three have no backing seam; `client_termname` has a
per-client seam whose retained value is always empty because the attaching terminal name is not
stored:

`buffer_mode_format`, `client_colours`, `client_created`, `client_mode_format`,
`client_termfeatures`, `client_termname`, `client_termtype`, `client_tty`, `config_files`,
`cursor_character`, `cursor_colour`, `mouse_hyperlink`, `mouse_line`, `mouse_pane`, `mouse_status_line`,
`mouse_status_range`, `mouse_word`, `pane_bg`, `pane_dead_time`, `pane_fg`, `pane_key_mode`,
`pane_mode`, `pane_pb_state`, `pane_search_string`, `pane_tabs`, `session_activity`,
`session_group`, `session_group_attached_list`, `session_group_list`, `session_path`,
`tree_mode_format`, `window_activity`, `window_offset_x`, and `window_offset_y`.

Thirteen more have usable backing only in a specific client/list context and may be empty in an
ordinary pane format:

`client_activity`, `client_flags`, `client_height`, `client_key_table`, `client_last_session`,
`client_name`, `client_readonly`, `client_session`, `client_theme`, `client_uid`, `client_user`,
`client_width`, and `session_last_attached`.

Of these, `client_flags`, `client_height`, `client_name`, `client_session`, `client_theme`,
`client_uid`, `client_user`, and `client_width` also receive per-recipient facts in status/title
rendering. The other five are currently injected only by `list-clients`. `pane_dead_signal` is
backed and must not appear in an unavailable list.

The other 151 names have a state value, runtime feed, defined pin-null/default value, or an explicit
daemon hook. Empty output can still be correct when the required session, window, pane, buffer, or
client context is absent.

# Hooks

All 68 pinned hook names store at their declared scope. Fifty-seven have a daemon or mux producer.
The eleven names without an automatic producer are:

- Queue/resize: `after-queue`, `after-resize-window`.
- Client events: `client-active`, `client-focus-in`, `client-focus-out`, `client-resized`,
  `client-light-theme`, `client-dark-theme`.
- Pane events: `pane-focus-in`, `pane-focus-out`, `pane-set-clipboard`.

This is nine missing event seams plus two missing after-command producers. `alert-activity` and
`alert-silence` now fire and are not part of this list.

# Default key compatibility

The default zz prefix table has 60 bindings. The pin has 92. Fifty-nine keys overlap. zz adds
`e -> send-last-output` and omits 33 stock keys:

`#`, `'`, `(`, `)`, `*`, `-`, `.`, `/`, `<`, `>`, `@`, `BTab`, `C`, `C-z`, `D`, `DC`, `L`,
`M`, `M-n`, `M-p`, `PPage`, `S-Down`, `S-Left`, `S-Right`, `S-Up`, `Tab`, `d`, `f`, `g`, `i`,
`m`, `t`, and `~`.

The important shared-key differences are:

| Keys | tmux | zz default |
| --- | --- | --- |
| `%` | `split-window -h` | `split-picker -h` |
| `"` | `split-window` (vertical by default) | `split-picker -v` |
| `&`, `x` | `confirm-before` around kill | immediate kill |
| `]` | `paste-buffer -p` | `paste-buffer` |
| `?` | `list-keys -N` | `list-keys` |
| `r` | `refresh-client` | `reload-config` |
| `s`, `w` | `choose-tree` | `focus-sidebar` |
| `M-Up`, `M-Left`, `C-Up`, `C-Left` | floating-aware `if-shell` resize | direct tiled-pane `resize-pane` |

The source deliberately binds the picker and sidebar verbs
([defaults](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-protocol/src/key.rs#L46-L89)).
An imported binding that names `split-window` still creates a terminal pane. The compatibility
decision is therefore not “copy all stock defaults”; it is “tmux command names retain tmux meaning,
while zz defaults may call zz-only commands.”

# Alias and packaging boundary

The installed launcher rewrites no arguments to `attach`
([launcher](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz/src/bin/zz_cli.rs#L28-L40)).
The raw-terminal client first executes `has-session`; an empty daemon produces `no sessions` before
the lower-level interactive attach can perform its lazy creation
([preflight](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-tui/src/lib.rs#L436-L471),
[lazy attach](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-daemon/src/daemon.rs#L9321-L9363)).

Invocation behavior at the `202f322` source anchor:

| Invocation | Current result |
| --- | --- |
| bare packaged `zz` with an existing session | Attaches the TTY. |
| bare packaged `zz` with an empty daemon | Fails with `no sessions`; no first session is created. |
| `zz new -s foo` on a TTY | Creates and attaches. |
| `zz new -s foo` without a TTY | Refuses before creating. |
| `zz attach [-d] [-r] [-t target]` | Attaches, supports detach-others and read-only mode. |
| direct inner `zz` binary with no arguments | Opens the GPUI app. |
| `zz app` through the installed launcher | Opens the GPUI app. |

At that source anchor, nested attach refusal is only checked for a command named `attach-session`
before the engine runs.
An attaching `new-session` produces the same attach effect later without that check, so nested
`attach` refuses but nested attaching `new-session` remains permissive
([dispatch check](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-daemon/src/daemon.rs#L4453-L4468),
[effect application](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-daemon/src/daemon.rs#L5700-L5715)).

Top-level `-L`, `-S`, `-f`, `-c`, `-l`, `-N`, `-C`, `-CC`, and `-V` exist. `-2`, `-q`, `-u`, and
`-v` are accepted no-ops; `-T` consumes a value and is a no-op; `-D` is rejected. Real tmux socket
protocol compatibility is a permanent non-goal.

The default config remains zz-owned. Unless startup receives one or more top-level `-f` files, the
daemon reads the first existing platform candidate:
`$XDG_CONFIG_HOME/zz/mux.conf`, the home config directory, macOS Application Support, or Windows
AppData candidates in platform order. Import copies a donor `.tmux.conf` to the first existing or
first constructible candidate. Explicit startup `-f` files replace that default for the initial
load; `reload-config` later returns to the platform candidate because the startup list is not
retained. A shell alias does not help direct process lookup, and current packages provide no global
`tmux` shim. Windows packaging does not install the CLI on PATH.

# Existing zz-native CLI surface

Fourteen native commands share the main catalog:

`copy-mode-search-prompt`, `focus-sidebar`, `new-browser`, `reload-config`,
`restart-agent-pane`, `select-pane-kind`, `set-agent-provider`, `set-agent-session`,
`set-browser-profile`, `set-browser-tabs`, `set-browser-url`, `set-editor-path`,
`split-browser`, and `split-picker`.

Five more daemon-owned workspace commands are dispatched outside that catalog:

`tools`, `agent-send`, `send-last-output`, `capture-browser`, and `debug-marker`.

Together they already cover the main superset direction: pane-kind picking, browser creation and
control, agent prompt delivery and runtime selection, editor selection, GUI navigation, browser
capture, and diagnostic markers. The daemon's `tools` output documents the agent-facing subset
([workspace tools](https://github.com/demfabris/zz/blob/202f322304a6b1717411f2bfb3e99e6c14e55e8f/crates/zz-daemon/src/daemon.rs#L21839-L21880)).

The native CLI itself has two structural gaps:

1. The five daemon-only verbs are outside `COMMAND_SPECS`, so discovery, `list-commands`, command
   palette completion, and validation do not share one source of truth.
2. There is no direct `split-agent` or `new-agent` spelling. An automation must create a picker and
   select its pane kind, or depend on GUI state.

# Differential proof and blind spots

`compat/run.sh` compares command exit class, topology, exact geometry, `fmt:` and `out:` query
stdout, smoke-scenario ordinary stdout, and plugin warning streams against the pin. Normal ordinary
command stdout is captured in the log but is not compared. The current scenario tree contains:

- 42 normal scenarios.
- 2 accepted known-divergence scenarios.
- 12 plugin/config smoke scenarios.
- 816 executable steps in total.

The checked-in `compat/results/summary.md` contains 48 scenario rows and 678 steps. It omits
`alerts`, `command-alias`, `display-message`, `display-panes-format`, `prefix2`,
`renderer-styles`, `smoke/source-file-diagnostics`, and `update-environment`. A later run may have
been green, but that evidence is not persisted at this source anchor.

The `smoke/config-grammar` fixture intentionally keeps a one-sided warning expectation. The nested
tmux control client publishes the invalid-octal `%config-error`; the nested zz control client does
not. Its state readbacks still prove matching whole-file abort semantics. Gate 0 must preserve this
explicit warning divergence rather than teaching the fixture to expect output zz does not emit.

The harness is also headless. It cannot prove copy mode, choose-tree, choose-buffer, prompts, stock
key behavior, packaged launcher behavior, or native GUI rendering. Those need an attached-client
fixture and packaged CLI smoke tests before an alias claim can rely on them.

# Implications

The project is closer to a practical tmux-compatible CLI than the raw 113 flags suggest. Most
plugins and daily session/window/pane commands already have the required nouns, targets, formats,
options, hooks, and shell execution. The remaining work is uneven:

- A small launcher fix blocks the most visible bare alias path.
- A finite group of local handlers and output flags can close cheaply.
- Client environment, binary streams, process control, interactive modes, floating panes, linked
  sessions, and ACLs require new ownership or wire models.
- Chasing every storage-only theme option or every floating/session-group feature would spend far
  more complexity than it returns for the stated CLI goal.

The design target should be **compatible enough by a written workload**, not “all 92 commands and
all flags.” The tmux namespace stays exact. Native GUI improvements stay on native verbs, and zz's
default bindings are free to call those verbs.

# Related

- [tmux divergence matrix](/tmux/divergences.md)
- [tmux compatibility philosophy](/tmux/tmux-compat.md)
- [tmux drop-in plan](/designs/tmux-drop-in.md)
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md)
- [tmux command set](/tmux/commands.md)
- [key tables](/tmux/key-tables.md)
- [status formats](/tmux/status-line.md)
- [compatibility harness](/playbooks/compat-harness.md)
