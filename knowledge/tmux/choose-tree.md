---
type: Concept
title: Sidebar navigation and native choosers
description: Persistent sidebar navigation for sessions and windows plus daemon-owned pane and paste-buffer choosers, with tmux-style keyboard movement and activation.
resource: crates/zz-mux/src/command.rs
tags: [tmux, choose-tree, choose-buffer, chooser, overlay]
timestamp: 2026-07-30T21:21:06Z
---

# Overview

zz has two distinct navigation surfaces. `focus-sidebar` opens the persistent workspace tree. In
titlebar mode the tree leaves the layout and the native status bar appears in the title bar; focusing the
sidebar raises the same tree as a slideover. The default `C-b s` and `C-b w` bindings call this
zz-native command.

Every `choose-tree` form and a `choose-buffer` invocation with at least one stored buffer opens a
tmux-style chooser rendered as a daemon-owned native GPUI overlay, not terminal escape content. In
[`crates/zz-mux`](/crates/zz-mux.md), the effects carry the target pane, chooser shape,
filter, parsed sort order, and optional selection command. The [server](/crates/zz-daemon.md) builds a snapshot
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

Supported flags are `-s`, `-w`, `-Z` (zoom), `-t` (target pane), `-f` (format filter), `-K`
(per-row shortcut-key format), `-N` (no preview), `-O` (sort order), and `-r` (reverse). One `-N`
is already zz's only chooser layout; repeated `-NN`, tmux's large-preview mode, is rejected. The pin
accepts `-s` and `-w` together and gives `-s` precedence. These flags change only the initial
collapse depth; every form retains the complete hierarchy. A default chooser opened from a
one-pane source window initially selects that window row. The accepted sort names are case-insensitive:
`activity`, `creation`, `index`/`key`, `modifier`, `name`/`title`, `order`, `size`,
and `z`. The tree defaults to index order, so `-r` alone reverses it. The same criterion
sorts sessions, each session's windows, and each window's panes independently.

The filter is evaluated once per pane with complete session/window/pane format context. Windows
and sessions with no matching descendant are pruned. If nothing matches, zz restores the
unfiltered tree like tmux and shows `filter: no matches` above the still-selectable rows. That state
is recomputed on every full daemon rebuild and survives incremental search and selection updates.

# Selection command templates

`choose-tree [template]` and `choose-buffer [template]` accept one optional string or unquoted
command block. With no template, Enter keeps the native action: tree rows activate their target and
buffer rows paste their contents. An explicit empty template closes the chooser without running the
default action.

Command preparation constructs a typed block before the chooser opens. It resolves that block's
aliases and stores canonical command text, with ` ; ` between commands in one physical group and
` ;; ` between groups. A quoted template stays raw. Selection substitutes the chosen value and
parses the result against the current alias table, so later alias changes affect string templates
while typed aliases retain their earlier construction. The parser replaces the first `%%` and every `%1`; a trailing `%`
quotes `"`, `\`, `$`, `;`, and `~` in the inserted value.

Tree selection supplies `=name:` for a session, `=name:index.` for a window, and
`=name:index.%id` for a pane. Buffer selection supplies the exact buffer name. The selected row does
not retarget the action: the daemon executes it with the invoking client's live session, window, and
pane context. The daemon closes the chooser before it runs the action. It capitalizes the first
character of parse and command errors shown to an attached client.

`choose-buffer` opens no overlay when the buffer store is empty, so a custom action cannot run. If
another command removes the selected buffer after the overlay opens, selection closes the stale
overlay without running its action.

When focused, the sidebar selects and reveals the active pane. A collapsed sidebar is expanded first,
so the keyboard contract below is the only one. Arrow keys or `hjkl` move and collapse/expand tree
branches, `g`/`G` jump to the first/last visible row, Enter activates the selected target and
returns focus to the active pane, `r` opens the existing native rename prompt for a selected session
or window, and `q`/Escape returns focus without activation. Because `focus-sidebar` initially selects
the active pane, `r` on a pane row targets its containing window. Pointer hover, keyboard selection,
and the focused mux row share the translucent `background.washed(2)` fill; active weight and
foreground color carry the hierarchy without an opaque second signal. A 1px vertical inset leaves a
tiny seam between adjacent rounded fills. Returning focus to a pane removes the selected and active
fills. The attached session, its active window, and its focused pane still use the theme
foreground for their icons and labels; all sibling nodes use the muted foreground. Indent guides
keep the host-to-session and session-to-window rails neutral; only the active window-to-pane rail
uses the stronger foreground. Row labels ellipsis-truncate inside a reserved action gutter, so
revealing a row's hover actions never reflows the text. Host rows reveal one plus button for a new
session. The final muted Add host row opens the host dialog and highlights only its label on hover.
Window rows expose one overflow menu with Split right, Split bottom, and Delete; destructive actions
use the same Tabler Xmark as the rest of the application.

The settings and sidebar-toggle controls share the leading titlebar cluster. The toggle changes
between the full-height tree without a status bar and the full-width workspace with a status bar
in the title bar. It opens no action menu.

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
row formats (`-F`), `-G`/`-h`/`-k`/`-y`, tagging, previews beyond the
already-previewless `-N` form,
kill/swap actions, and `choose-client` are explicitly unsupported.

# choose-buffer

`choose-buffer` (default `C-b =`) accepts `-Z`, `-t`, `-f`, `-K`, `-N`, `-O`, and `-r` and opens a
sibling overlay over the daemon's global paste-buffer store. Its wire model is deliberately
bounded: `ChooseBufferItem` holds only a
`name`, a single-line `preview`, `size_bytes`, and `created_unix_seconds`; full buffer contents stay
in the daemon until paste. `ChooseBufferAction` mirrors the tree's navigation plus `Paste`,
`PasteIndex`, and `Delete`. In the overlay: arrow keys or `j`/`k` navigate, Enter/`p` or double-click
pastes through the same synchronized-input path as `paste-buffer`, `d` deletes, `/` and `?` search
names and full server-side contents, and `q`/Escape closes. Buffers default to creation order,
newest first; `-r` alone makes that oldest first. Filters receive the source
session/window/pane context plus buffer facts, and a zero-match filter falls back to the
unfiltered chooser with the same `filter: no matches` status. `-K` expands one shortcut key per row
and one `-N` selects the already-native previewless layout; repeated `-NN` remains unsupported.
Both clients reserve a shortcut gutter only when at least one rendered row has a key, so a fully
keyless list uses the full row width. Custom row formats (`-F`), `-k`/`-y`, tagging, editor
integration, and broader presentation behavior remain unsupported.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/command.rs` | `focus_sidebar`, `choose_tree`, and `choose_buffer` validation plus their effects. |
| `crates/zz-daemon/src/daemon.rs` | Chooser state, live selection, template substitution, action execution, and error delivery. |
| `crates/zz-protocol/src/message.rs` | `FocusSidebar`, `ChooseTreeKind`, `ChooseTreeItem`/`State`/`Action`, and choose-buffer types. |
| `crates/zz/src/workspace/sidebar.rs` | Persistent tree projection, titlebar-mode slideover, focus/reveal lifecycle, vim-style navigation, selection, and activation. |

# Related

- Opened by [commands](/tmux/commands.md); bound in the [key tables](/tmux/key-tables.md); rendered by
  the [app](/crates/zz.md) from [server](/crates/zz-daemon.md) state.
- Sibling native mode: [copy mode](/tmux/copy-mode.md). Behavior checked against
  `cmd-choose-tree.c`/`mode-tree.c`/`window-tree.c`/`window-buffer.c` in the
  [tmux upstream reference](/references/tmux-upstream.md); see [tmux compatibility](/tmux/tmux-compat.md).
