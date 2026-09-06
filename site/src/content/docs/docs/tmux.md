---
title: tmux compatibility
description: What works, what doesn't, and how zz relates to real tmux.
---

zz reimplements tmux in Rust; it never runs or links tmux. Behavior is
checked against a pinned upstream commit. 83 canonical commands (plus
aliases) work, along with the seven named layouts and the root, prefix,
copy-mode, and copy-mode-vi key tables.

Anything unsupported is rejected with a diagnostic instead of being
half-implemented, so you find out at config load, not mid-session.

## Your config

At daemon startup zz reads each existing file in this order: `/etc/tmux.conf`,
`~/.tmux.conf`, `$XDG_CONFIG_HOME/tmux/tmux.conf`, and `~/.config/tmux/tmux.conf`.
It then reads your `zz/mux.conf` (normally `~/.config/zz/mux.conf`) so you can
keep zz-specific overrides there. Duplicate paths load once.

`zz -f <file>` replaces the tmux candidate list with that file; `zz/mux.conf`
still loads last. Multiple `-f` arguments load in their given order. The
`import-tmux-config` command now explains this behavior without copying files.
Your tmux config stays in place, so plugin managers and `source-file ~/.tmux.conf`
use the same file. `set -g prefix`, key bindings, `history-limit`, `mode-keys`,
and the rest of the supported subset apply as-is.

## Copy mode

Full copy mode, rendered natively. The selection, search matches, cursor,
and scrollbar are painted by the GUI, never emitted as escape sequences:

- vi and emacs tables, rectangle selection, marks
- incremental regex search with smart case
- `jump-to-forward` / `jump-again`
- `next-prompt` / `previous-prompt` via OSC 133 shell-integration marks
- `copy-pipe` into any shell command

Each attached client gets its own cursor, selection, and search over a
frozen view while output keeps flowing underneath.

## Navigation

- `choose-tree` and `display-panes` as native overlays
- a persistent sidebar with the session tree and your `status-left` /
  `status-right` formats, `#{...}` variables and `#()` shell substitution
  included
- a command palette (`prefix :`) with completions for commands, flags, and
  live `$session` / `@window` / `%pane` targets

IDs are stable tmux-style sigils: `$0` sessions, `@1` windows, `%2` panes, all
scriptable from the [CLI](/docs/cli/).
