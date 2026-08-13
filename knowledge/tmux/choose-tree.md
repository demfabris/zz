---
type: Concept
title: Sidebar navigation and native choosers
description: Persistent sidebar navigation for sessions and windows plus daemon-owned pane and paste-buffer choosers, with tmux-style keyboard movement and activation.
resource: crates/zz-mux/src/command.rs
tags: [tmux, choose-tree, choose-buffer, chooser, overlay]
timestamp: 2026-07-30T21:21:06Z
---

# Overview

Session and window selection use a persistent navigation surface instead of a floating chooser.
That surface is the workspace sidebar, which is always present: collapsing it parks a narrow rail
rather than removing it. `focus-sidebar` and the compatibility forms `choose-tree -s` and
`choose-tree -w` emit
`MuxEffect::FocusSidebar { pane }`; the server sends the invoking interactive client a reliable
`EventPayload::FocusSidebar`, and the app expands the sidebar and focuses its tree.

The remaining flagless `choose-tree` and `choose-buffer` modes are tmux-style choosers rendered as
daemon-owned native GPUI overlays, not terminal escape content. In [`crates/zz-mux`](/crates/zz-mux.md),
`choose-tree` emits `MuxEffect::ChooseTree { pane, kind: Panes }` and `choose-buffer`
emits `MuxEffect::ChooseBuffer { pane }`. The [server](/crates/zz-daemon.md) builds a snapshot
(`ChooseTreeState` / `ChooseBufferState`, defined in [protocol](/crates/zz-protocol.md)) and the
[app](/crates/zz.md) paints it above the invoking terminal *or* browser pane. The overlay blocks
input from leaking into the covered pane, follows live mux mutations, survives protocol resync, can
activate targets in another session, and closes when its underlying data empties.

# choose-tree hierarchy and flags

`choose-tree` chooses a `ChooseTreeKind`, selected by flag:

| Invocation | `ChooseTreeKind` | Shows |
| --- | --- | --- |
| `choose-tree` (default) | `Panes` | full `$session → @window → %pane` hierarchy, incl. terminal, browser, and Agent pane types |
| `choose-tree -s` | . | focus the configured persistent navigation surface; no overlay is created |
| `choose-tree -w` | . | focus the configured persistent navigation surface; no overlay is created |

Supported flags are `-s`, `-w`, `-Z` (zoom), and `-t` (target pane); `-s` and `-w` are mutually
exclusive, and positional command templates are rejected (`UnsupportedCommand`). The default bindings
are `C-b s` and `C-b w`, both mapped directly to `focus-sidebar`.

When focused, the sidebar selects and reveals the active pane. A collapsed sidebar is expanded first,
so the keyboard contract below is the only one. Arrow keys or `hjkl` move and collapse/expand tree
branches, `g`/`G` jump to the first/last visible row, Enter activates the selected target and
returns focus to the active pane, `r` opens the existing native rename prompt for a selected session
or window, and `q`/Escape returns focus without activation. Because `focus-sidebar` initially selects
the active pane, `r` on a pane row targets its containing window. Row fills appear only while the
sidebar owns focus: the **keyboard-selected** row takes a washed `background.raised(2)`, while the
**mux-active** row takes the solid one, so a row that is both reads active. Returning focus to a pane
removes both fills. The attached session, its active window, and its focused pane still use the theme
foreground for their icons and labels; all sibling nodes use the muted foreground. Indent guides
below the workspace root follow that same active hierarchy, while the root-to-session guide stays
neutral. Row labels ellipsis-truncate inside a reserved action gutter, so revealing a row's hover
actions never reflows the text.

The parked rail is pointer-only: the attached session's windows become tab groups holding their
panes as tabs, clicking a tab selects that pane, and tooltips carry the window and pane names. It is
also where the expand toggle lives, since the rail is too narrow for the sidebar's control cluster.

Pane labels are live metadata rather than manual window names: terminal OSC titles flow from the
daemon-owned viewport into `PaneSnapshot.title`, while browser `TitleChanged` events use tmux's
`select-pane -T` path. zz automatically supplies those terminal titles for zsh and modern Bash,
showing the exact command during pre-exec and the compact working directory at the prompt; native
applications remain free to replace the title with their own OSC 0/2 output. Session and window
names remain explicit and are changed only by their rename commands.

Each `ChooseTreeItem` carries a `label`, `detail`, a `ChooseTreeTarget` (`Session`/`Window`/`Pane`),
a `depth`, an optional `pane_kind` (`ChooseTreePaneKind::Terminal`/`Browser`/`Agent`), and packed `flags`: `EXPANDED`,
`HAS_CHILDREN`, `ACTIVE`. Navigation and activation are driven by `ChooseTreeAction`:

| Group | `ChooseTreeAction` variants |
| --- | --- |
| Move | `Previous`, `Next`, `PagePrevious`, `PageNext`, `First`, `Last`, `Select(index)` |
| Tree | `Collapse`, `Expand` |
| Activate | `Activate`, `ActivateIndex(index)` |
| Search | `SearchStart{reverse}`, `SearchAppend`, `SearchBackspace`, `SearchAccept`, `SearchCancel`, `SearchNext{reverse}` |
| Exit | `Close` |

In the GPUI overlay these map to: arrow keys or `hjkl` navigate and collapse/expand, Enter or
double-click activates, `/` and `?` search, `n`/`N` repeat the search, `q`/Escape closes. Custom
formats, filters, templates, tagging, previews, kill/swap actions, and `choose-client` are explicitly
unsupported.

# choose-buffer

`choose-buffer` (default `C-b =`, flags `-Z` and `-t`) opens a sibling overlay over the daemon's
global paste-buffer store. Its wire model is deliberately bounded: `ChooseBufferItem` holds only a
`name`, a single-line `preview`, `size_bytes`, and `created_unix_seconds`; full buffer contents stay
in the daemon until paste. `ChooseBufferAction` mirrors the tree's navigation plus `Paste`,
`PasteIndex`, and `Delete`. In the overlay: arrow keys or `j`/`k` navigate, Enter/`p` or double-click
pastes through the same synchronized-input path as `paste-buffer`, `d` deletes, `/` and `?` search
names and full server-side contents, and `q`/Escape closes. Buffers are newest-first; formats,
filters, sort orders, key formats, tagging, editor integration, and custom command templates are
rejected.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/command.rs` | `focus_sidebar`, `choose_tree`, and `choose_buffer` validation plus their effects. |
| `crates/zz-protocol/src/message.rs` | `FocusSidebar`, `ChooseTreeKind`, `ChooseTreeItem`/`State`/`Action`, and choose-buffer types. |
| `crates/zz/src/workspace/sidebar.rs` | Persistent tree projection, the collapsed rail, focus/reveal lifecycle, vim-style navigation, selection, and activation. |

# Related

- Opened by [commands](/tmux/commands.md); bound in the [key tables](/tmux/key-tables.md); rendered by
  the [app](/crates/zz.md) from [server](/crates/zz-daemon.md) state.
- Sibling native mode: [copy mode](/tmux/copy-mode.md). Behavior checked against
  `cmd-choose-tree.c`/`mode-tree.c`/`window-tree.c`/`window-buffer.c` in the
  [tmux upstream reference](/references/tmux-upstream.md); see [tmux compatibility](/tmux/tmux-compat.md).
