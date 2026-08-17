---
type: Concept
title: tmux compatibility philosophy
description: zz reimplements a deliberately-scoped subset of tmux behavior in pure Rust, checked against a pinned upstream commit; it never compiles, links, or runs tmux.
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, philosophy, reimplementation]
timestamp: 2026-07-14T00:00:00Z
---

# Overview

zz's multiplexer is a **Rust reimplementation** of tmux behavior, not a wrapper: it does not compile,
link, or run tmux, and no tmux C source is copied into the Rust code. Command names, aliases,
key-table behavior, copy-mode actions, and configuration syntax are checked by hand against a single
pinned upstream commit recorded in [`third_party/tmux-reference/UPSTREAM.md`](/references/tmux-upstream.md).
The goal is that a user's muscle memory and existing `~/.tmux.conf` bindings work, while presentation
is native GPUI (no status line, copy-mode screen, prompt, chooser, or pane indicator is ever emitted
as terminal escape sequences; the [status line](/tmux/status-line.md) is expanded by the daemon and
rendered in the sidebar's bottom section). The behavioral model lives entirely in [`crates/zz-mux`](/crates/zz-mux.md).

# Pinned reference

The reference commit is tmux `d77c9dc6aa021e4bc61f0da128c591af695e6466`. Each behavioral area maps to
specific upstream files that were consulted, for example:

| Behavior | Upstream files consulted |
| --- | --- |
| Tokenization / config loading | `cmd-parse.y`, `arguments.c`, `cfg.c` |
| Root/prefix key tables | `key-bindings.c`, `cmd-bind-key.c`, `cmd-unbind-key.c`, `cmd-list-keys.c` |
| Command prompt | `cmd-command-prompt.c`, `prompt.c`, `status.c` |
| Copy/view mode + word classes | `window-copy.c`, `grid-reader.c`, `tmux.1` |
| Choosers | `cmd-choose-tree.c`, `mode-tree.c`, `window-tree.c`, `window-buffer.c` |
| Targets `$`/`@`/`%` | `cmd-find.c` |
| Layouts / splits / resize | `layout.c`, `layout-set.c`, `cmd-select-layout.c`, `cmd-resize-pane.c` |
| Options and environments | `options-table.c`, `cmd-set-option.c`, `cmd-show-options.c`, `options.c`, `cmd-set-environment.c`, `cmd-show-environment.c`, `environ.c` |
| Status options and FORMATS subset | `options-table.c`, `status.c`, `format.c`, `tmux.1` |

# Scope of emulation

Only a deliberately supported subset is implemented; **supported commands must not silently implement
partial semantics**, and unsupported input is reported and skipped rather than approximated or run as
shell code. Concretely:

- **Emulated:** sessions/windows/panes/splits, target resolution, 59 cataloged tmux commands and
  their aliases plus the daemon-side buffer family
  ([commands](/tmux/commands.md)), root/prefix/`copy-mode`/`copy-mode-vi` key tables with repeat
  bindings and `send-keys -X` ([key tables](/tmux/key-tables.md)), the seven named layouts, lossless
  zoom, swap/rotate/break/join, `synchronize-panes`/`history-limit`/`word-separators`/`mode-keys`
  options, the eight phase-4f behavior options, retained dead panes with in-place
  `respawn-pane`/`respawn-window`, native [copy mode](/tmux/copy-mode.md),
  [choose-tree/choose-buffer](/tmux/choose-tree.md),
  command prompt, pane-number overlay, the `status`/`status-interval`/`status-left`/`status-right`
  options with a documented [FORMATS subset](/tmux/status-line.md) (including `#()` command
  substitution), exact option readback, free-form `@` storage at every scope, global/session
  environment overlays and readback, and the [`.tmux.conf` subset](/tmux/conf-parser.md). Since
  2026-08-09 this also covers tmux's environment contract (`ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` in
  terminal panes, with the CLI resolving untargeted commands against the invoking pane the way
  `$TMUX_PANE` does and originless Command clients falling back to the most-recent session),
  `[shell-command]` positionals on `new-session`/`new-window`/`split-window`, `last-window`,
  cell-accurate `resize-pane`, and `%if` blocks skipped-with-diagnostic instead of executed; the
  scriptability layer (`-F` formats, `display-message`, compound targets) is tracked in the
  [tmux superset roadmap](/designs/tmux-superset-roadmap.md).
- **Out of scope as shipped** (this list describes current behavior; the 2026-08-16
  [drop-in plan](/designs/tmux-drop-in.md) schedules most of it — control mode,
  hooks, exec commands, styles, cell-size placement — with the permanent exclusions named
  there): binary/socket compatibility with a real tmux server, control mode, the rest of the status-line options (`status-style`, `status-justify`,
  `status-position`, `status-format`), plugins, hooks, `#[…]` styling, floating panes, and
  cell-size placement. These are rejected with diagnostics, not partially applied. The one
  exception is `#[…]`, which is dropped from a status format rather than failing it.

zz reimplements the *behavior*; the daemon sources the zz-owned `~/.config/zz/mux.conf` on startup
and applies the supported subset, logging and skipping the rest. It never reads `~/.tmux.conf`; the
client's import flow copies a user's tmux config there verbatim (see the
[conf parser](/tmux/conf-parser.md)).

# Related

- Where compatibility is going, and what is refused by doctrine:
  [tmux superset roadmap](/designs/tmux-superset-roadmap.md).
- Realized by the [mux crate](/crates/zz-mux.md) across [commands](/tmux/commands.md),
  [key tables](/tmux/key-tables.md), and the [conf parser](/tmux/conf-parser.md).
- The pinned commit and provenance: [tmux upstream reference](/references/tmux-upstream.md).
- Native (non-terminal-escape) presentation: [copy mode](/tmux/copy-mode.md) and
  [choosers](/tmux/choose-tree.md).
