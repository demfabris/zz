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

The daemon loads `zz/mux.conf` (normally `~/.config/zz/mux.conf`). It does not read tmux
configuration on its own. `zz -f <file>` replaces the default file; multiple `-f` arguments
load in their given order. `reload-config` repeats that selection.

Run `zz import-tmux-config [path]` to copy a tmux file into `zz/mux.conf` and reload. Without
a path, zz uses the first discovered tmux file. Settings › Multiplexer also accepts any path.
The first import prepends a marked block so your existing lines keep their precedence.
Re-import replaces that block and preserves everything outside it. Unsupported commands become
comments prefixed with `# zz-unsupported:`. Your own `source-file` lines stay as written.

The Options rows and text editor both edit `zz/mux.conf`. Saving or changing a row reloads
the daemon's selected configuration. When you started with `-f`, reload still uses those files.

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
