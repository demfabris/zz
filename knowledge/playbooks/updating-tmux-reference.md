---
type: Playbook
title: Updating the pinned tmux behavioral reference
description: How to bump zz's pinned tmux upstream commit and re-verify the Rust tmux-compat reimplementation against it.
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, upgrade, playbook, behavioral-reference]
timestamp: 2026-07-14T00:00:00Z
---

# Overview

zz's multiplexer ([`crates/zz-mux`](/crates/zz-mux.md)) is a from-scratch Rust reimplementation of tmux
behavior: it never compiles, links, or runs tmux, and no tmux C source is copied in. Instead,
command names, key-table behavior, and configuration syntax are checked by hand against a single
pinned upstream tmux commit recorded in
[`third_party/tmux-reference/UPSTREAM.md`](/references/tmux-upstream.md). Updating that pin means
re-diffing the relevant upstream files and porting any observed behavior change into the Rust
implementation. That is a research-and-port task, unlike the mechanical version bump in
[updating CEF](/playbooks/updating-cef.md).

# Steps

1. **Pick the new pinned commit.** Choose a specific tmux commit (not a moving branch) from
   `https://github.com/tmux/tmux`, matching the precision of the current pin
   (`d77c9dc6aa021e4bc61f0da128c591af695e6466`).
2. **Diff every relevant upstream file** between the old and new pinned commits. `UPSTREAM.md`
   groups these by feature area. Diff each group for the behavior it documents:
   - `cmd-parse.y`, `arguments.c`, `cfg.c` . tokenization and config loading
   - `key-bindings.c`, `cmd-bind-key.c`, `cmd-unbind-key.c`, `cmd-list-keys.c` . root/prefix key tables
   - `cmd-command-prompt.c`, `prompt.c`, `status.c` . command-prompt options, editing keys, history
   - `cmd-rename-session.c`, `cmd-rename-window.c` . rename targets/aliases, prompt prefill
   - `server-client.c`, `window-copy.c` . command output entering read-only view/copy-mode
   - `cmd-choose-tree.c`, `mode-tree.c`, `window-tree.c`, `window-buffer.c` . chooser hierarchy/search/activation, `choose-buffer`
   - `paste.c`, `cmd-set-buffer.c`, `cmd-load-buffer.c`, `cmd-save-buffer.c`, `cmd-paste-buffer.c`, `options-table.c` . paste-buffer storage, `buffer-limit`
   - `cmd-display-panes.c` . pane-number overlay
   - `cmd-resize-pane.c`, `layout.c`, `cmd-select-pane.c`, `cmd-split-window.c`, `window.c` . divider drag, resize, zoom, spatial navigation
   - `cmd-swap-pane.c`, `cmd-break-pane.c`, `cmd-join-pane.c` . pane reparenting/swap
   - `cmd-select-layout.c`, `layout-set.c` . named layout presets
   - `cmd-rotate-window.c`, `cmd-find.c` . rotation, relative pane targets
   - `options-table.c`, `cmd-set-option.c`, `window.c` . `history-limit`, `word-separators`, `mode-keys`, `synchronize-panes`
   - `grid-reader.c`, `tmux.1` . word-class definitions for copy mode
   - `server.c`, `server-client.c`, `proc.c` . client/server boundary semantics
   Use the full up-to-date list already recorded in `UPSTREAM.md`; add or remove files from that
   list here if the new commit's feature surface changed.
3. **Port observed behavior changes.** For each diff that changes user-visible behavior in a file
   zz already reimplements, update the corresponding Rust code path in
   [`crates/zz-mux`](/crates/zz-mux.md), always as an independent reimplementation, never by copying
   upstream C.
4. **Re-verify.** Run the workspace test suite plus a manual pass over the affected bindings/commands:
   ```sh
   cargo test --workspace
   cargo run -p zz
   ```
   Manually exercise the specific tmux-compat behaviors touched by the diff (e.g. `C-b` bindings,
   `choose-tree`, `choose-buffer`, layout presets). The supported-command list lives in
   [tmux command set](/tmux/commands.md) and the default bindings in
   [key tables](/tmux/key-tables.md).
5. **Update `UPSTREAM.md`.** Bump the pinned commit hash and its GitHub link, and adjust the
   "most relevant upstream files" list if step 2 changed it. Keep the note that only the
   deliberately supported subset is implemented and unsupported config is reported and skipped.
6. **Refresh the retained license.** The upstream tmux license kept beside `UPSTREAM.md` for
   provenance should match the newly pinned commit's license terms (rarely changes, but check).
7. **Commit the commit-hash bump, any list-of-files changes, the ported Rust changes, and their
   tests together.** A stale pin note with mismatched behavior is worse than no note.

# Key files

| File | Role |
| --- | --- |
| `third_party/tmux-reference/UPSTREAM.md` | The pinned commit and list of relevant upstream files |
| `crates/zz-mux/src/**` | The Rust reimplementation being verified against upstream behavior |
| `crates/zz-mux/src/catalog.rs` | The supported command surface to manually re-check, mirrored in [tmux command set](/tmux/commands.md) |

# Related

- [tmux upstream reference](/references/tmux-upstream.md) . the pin this playbook updates
- [tmux compat concept](/tmux/tmux-compat.md) . the Rust reimplementation being kept in sync
- [`mux` crate](/crates/zz-mux.md) . where ported behavior changes land
