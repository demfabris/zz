---
type: Concept
title: Sidebar navigation and native choosers
description: Persistent sidebar navigation for sessions and windows plus daemon-owned pane and paste-buffer choosers, with tmux-style keyboard movement and activation.
resource: crates/zz-mux/src/command.rs
tags: [tmux, choose-tree, choose-buffer, chooser, overlay]
timestamp: 2026-07-30T21:21:06Z
---

# Overview

zz has two distinct navigation surfaces. `focus-sidebar` opens the persistent workspace tree;
collapsing it parks a narrow rail rather than removing it. The default `C-b s` and `C-b w`
bindings call this zz-native command directly.

Every `choose-tree` form and `choose-buffer` opens a tmux-style chooser rendered as a
daemon-owned native GPUI overlay, not terminal escape content. In
[`crates/zz-mux`](/crates/zz-mux.md), the effects carry the target pane, chooser shape,
filter, and parsed sort order. The [server](/crates/zz-daemon.md) builds a snapshot
(`ChooseTreeState` / `ChooseBufferState`, defined in [protocol](/crates/zz-protocol.md)) and the
[app](/crates/zz.md) paints it above the invoking terminal *or* browser pane. The overlay blocks
input from leaking into the covered pane, follows live mux mutations, survives protocol resync, can
activate targets in another session, and closes when its underlying data empties.

# choose-tree hierarchy and flags

`choose-tree` chooses a `ChooseTreeKind`, selected by flag:

| Invocation | `ChooseTreeKind` | Initial view |
| --- | --- | --- |
| `choose-tree` (default) | `Panes` | full `$session → @window → %pane` hierarchy, including terminal, browser, and Agent pane types |
| `choose-tree -s` | `Windows` | session rows, each expandable through windows to panes |
| `choose-tree -w` | `Windows` | expanded sessions with window rows, each window expandable to panes |

Supported flags are `-s`, `-w`, `-Z` (zoom), `-t` (target pane), `-f` (format filter),
`-O` (sort order), and `-r` (reverse). The pin accepts `-s` and `-w` together and gives
`-s` precedence. These flags change only the initial collapse depth; every form retains the
complete hierarchy. A default chooser opened from a one-pane source window initially selects that
window row. Positional command templates are rejected. The accepted sort names are case-insensitive:
`activity`, `creation`, `index`/`key`, `modifier`, `name`/`title`, `order`, `size`,
and `z`. The tree defaults to index order, so `-r` alone reverses it. The same criterion
sorts sessions, each session's windows, and each window's panes independently.

The filter is evaluated once per pane with complete session/window/pane format context. Windows
and sessions with no matching descendant are pruned. If nothing matches, zz restores the
unfiltered tree like tmux; the native overlay does not yet show tmux's
`filter: no matches` status.

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
row formats (`-F`), command templates, `-G`/`-h`/`-K`/`-k`/`-N`/`-y`, tagging, previews,
kill/swap actions, and `choose-client` are explicitly unsupported.

# choose-buffer

`choose-buffer` (default `C-b =`) accepts `-Z`, `-t`, `-f`, `-O`, and `-r` and opens a
sibling overlay over the daemon's global paste-buffer store. Its wire model is deliberately
bounded: `ChooseBufferItem` holds only a
`name`, a single-line `preview`, `size_bytes`, and `created_unix_seconds`; full buffer contents stay
in the daemon until paste. `ChooseBufferAction` mirrors the tree's navigation plus `Paste`,
`PasteIndex`, and `Delete`. In the overlay: arrow keys or `j`/`k` navigate, Enter/`p` or double-click
pastes through the same synchronized-input path as `paste-buffer`, `d` deletes, `/` and `?` search
names and full server-side contents, and `q`/Escape closes. Buffers default to creation order,
newest first; `-r` alone makes that oldest first. Filters receive the source
session/window/pane context plus buffer facts, and a zero-match filter falls back to the
unfiltered chooser. Custom row formats (`-F`), `-K`/`-k`/`-N`/`-y`, tagging, editor
integration, and custom command templates remain rejected.

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
