# tmux behavioral reference

zz's multiplexer is a Rust implementation; it does not compile, link, or run
tmux. Command names, aliases, key-table behavior, and configuration syntax were
checked against tmux at commit
[`d77c9dc6aa021e4bc61f0da128c591af695e6466`](https://github.com/tmux/tmux/tree/d77c9dc6aa021e4bc61f0da128c591af695e6466).

The most relevant upstream files are:

- `cmd-parse.y`, `arguments.c`, and `cfg.c` for tokenization and config loading;
- `key-bindings.c`, `cmd-bind-key.c`, `cmd-unbind-key.c`, and
  `cmd-list-keys.c` for root/prefix tables;
- `cmd-command-prompt.c`, `prompt.c`, and `status.c` for command-prompt
  options, editing keys, history, and submission behavior;
- `cmd-rename-session.c`, `cmd-rename-window.c`, `cmd-command-prompt.c`, and
  `key-bindings.c` for rename targets and aliases, duplicate-session handling,
  prompt name prefill, and the default `C-b $`/`C-b ,` bindings;
- `server-client.c` and `window-copy.c` for command output entering read-only
  view mode on the invoking pane and reusing copy-mode navigation;
- `key-bindings.c` for the default view/copy-mode exit, search, and selection
  bindings;
- `cmd-choose-tree.c`, `mode-tree.c`, and `window-tree.c` for native chooser
  hierarchy, navigation, collapse/expand, search, and activation behavior;
- `cmd-choose-tree.c`, `mode-tree.c`, `window-buffer.c`, and `key-bindings.c`
  for `choose-buffer`, newest-first buffer models, full-content search,
  paste/delete actions, empty-mode exit, and the default `C-b =` binding;
- `paste.c`, `cmd-set-buffer.c`, `cmd-load-buffer.c`, `cmd-save-buffer.c`, and
  `cmd-paste-buffer.c`, plus the `buffer-limit` entry in `options-table.c`, for
  byte-preserving buffer storage, named replacement, configurable automatic-buffer
  eviction, bounded file load/save behavior, and paste flags;
- `cmd-display-panes.c`, `server-client.c`, `key-bindings.c`, and
  `options-table.c` for pane-number overlay keys, replacement, timeout, the
  default `C-b q` binding, and unmatched-key fallthrough;
- `cmd-resize-pane.c`, `layout.c`, `cmd-select-pane.c`, `cmd-split-window.c`,
  `window.c`, and `key-bindings.c` for tiled divider dragging, nearest-axis
  pane resizing, lossless pane zoom, unzoom-on-layout-mutation,
  zoom-preserving selection, spatial pane navigation with edge wrapping and
  most-recently-active tie-breaking, `last-pane`, and the default repeatable
  arrow, `C-b ;`, and `C-b z` bindings;
- `cmd-swap-pane.c`, `window.c`, `layout.c`, and `key-bindings.c` for same- and
  cross-window pane swaps, previous/next wrapping, detached active-slot
  preservation, zoom restoration, and the default `C-b {`/`C-b }` bindings;
- `cmd-break-pane.c`, `cmd-join-pane.c`, `layout.c`, `window.c`, `session.c`,
  and `key-bindings.c` for stable-surface pane reparenting, source-window
  retirement, before/full-size split placement, detached activation, and the
  default `C-b !` binding;
- `cmd-select-layout.c`, `layout-set.c`, `layout.c`, and `key-bindings.c` for
  the seven named layouts, prefix matching, next/previous cycling, old-layout
  restoration, ancestor spreading, and the default `C-b Space` binding;
- `cmd-rotate-window.c`, `cmd-select-pane.c`, `cmd-find.c`, `window.c`, and
  `key-bindings.c` for pane-order rotation, active-slot preservation,
  zoom restoration, relative next/previous pane targets, and the default
  `C-b C-o`, `C-b M-o`, `C-b o`, `C-b E`, and `M-1` through `M-7` bindings;
- `cmd-send-keys.c` and `key-string.c` for named key behavior;
- `options-table.c`, `cmd-set-option.c`, and `window.c` for
  `history-limit` new-pane lifetime, `word-separators` defaults and session
  inheritance, `mode-keys` window scope and global inheritance,
  `synchronize-panes` scope, inheritance, and input fanout;
- `window-copy.c`, `grid-reader.c`, and `tmux.1` for the whitespace,
  separator-run, and non-separator word classes plus live vi/emacs key-table
  selection used by native copy mode;
- `cmd-find.c` for `$session`, `@window`, and `%pane` targets;
- `cmd-split-window.c`, `cmd-select-pane.c`, and `cmd-resize-pane.c` for pane
  commands;
- `server.c`, `server-client.c`, and `proc.c` for the client/server boundary.

Only the deliberately supported subset is implemented. Unsupported tmux
configuration commands are reported and skipped. No tmux C source is copied
into the Rust implementation. The upstream license is retained beside this
record for provenance.
