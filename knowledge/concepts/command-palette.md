---
type: Concept
title: Unified palette and tmux command completions
description: Desktop session, window, and pane navigation alongside command and host search, with shared zz-ui presentation and preserved tmux prompt behavior.
resource: crates/zz-ui/src/command.rs
tags: [gpui, command-prompt, tmux, completions, ui]
timestamp: 2026-09-16T00:17:40Z
---

# Overview

The desktop unified palette combines a session, window, and pane tree with command and host search.
`Cmd-K` and `Cmd-P` open a fresh search on macOS; `Ctrl-Shift-K` and `Ctrl-Shift-P` do so on
other desktop platforms. View > Choose Window opens the palette in Navigate mode. Daemon
`choose-tree -s` and `choose-tree -w` requests, including `prefix s`, `prefix w`, remaps, and
direct commands, use the same tree presentation with their daemon-selected expansion and row.
The desktop lets the daemon resolve prefix bindings.
The desktop and browser clients share the surface and row components in `zz-ui::command`.
The browser retains its command-prompt adapter.

# Modes and navigation

| Prefix | Content | Activation |
| --- | --- | --- |
| none | Workspace tree, three recent commands, then hosts; typed search groups Navigation, Commands, and Hosts | Attach to a session, switch window, focus pane, execute, or scope according to the selected row |
| `:` | Command catalog with descriptions, shortcuts, and highlighted-command usage | Execute a command or choose its target |
| `@` | Navigate: session/window/pane tree, with sessions expanded at first open; search uses flat breadcrumbs | Attach to a session, switch window, or focus pane |
| `%` | Panes with session/window context, pane kind, and available agent status | Focus pane |
| `~` | Hosts and their sessions; Left/Right collapse or expand | Scope an online host, connect an offline host, or attach to a session |

The default Workspace tree opens the current session to its windows and leaves other sessions
collapsed. Windows expand to panes. Session, window, and pane rows all activate their targets;
the disclosure control only toggles the branch. Left/Right collapse or expand tree branches.
The Navigate view starts with all sessions expanded and windows collapsed. The `%` mode remains
a direct pane list.

Typed search includes descendants hidden by collapsed branches and shows their session/window
breadcrumbs. The default search keeps Navigation, Commands, and Hosts in separate sections;
Navigate search stays within navigation targets. Agent panes carry their activity state in the tree
and search results.

Typing a prefix into an empty default search replaces it with a mode Tag. A host scope adds a
Tag before the mode. Choosing a target command adds its name as another Tag. Backspace on an
empty field removes one layer: command, mode, then host. Escape and clicking outside close the
palette; reopening resets the mode, query, and scope. Up/Down wrap across selectable rows and
skip headers. Moving beyond the visible rows scrolls only far enough to reveal the selected row,
including in daemon command prompts. Pointer movement selects the hovered row, and clicking activates it.

Session, window, and pane targets use stable IDs for execution; rows display names and breadcrumbs.
IDs remain searchable metadata. A host scope restricts search and execution to that host without
attaching just to browse it. Agent rows use the shared vendor icon and show Running, Waiting for
input, Idle, or Failed as text when the attached host has supplied attention state, without a
duplicate status dot. Other hosts show Agent until that state is available.
Successful local navigation and command requests use the app's existing notification surface.

The fuzzy matcher ignores query whitespace and case. It scores an ordered subsequence by the
first matching character's position plus twice the skipped characters between matches, and
returns UTF-8 byte ranges for text highlights. Each section sorts matches by score.

# Settings

Settings > Advanced > Command palette exposes three preferences:

| Config key | Values | Default |
| --- | --- | --- |
| `palette-window-layout` | `grouped`, `flat` | `grouped` |
| `palette-host-prefix` | `~`, `#` | `~` |
| `palette-show-keys` | `true`, `false` | `true` |

The `open-command-palette` chrome action lives in the UI key table and can be remapped.
Its default shortcuts replace the terminal Clear History bindings on those same keys.
Clear History remains available through the menu and custom bindings.

# Daemon prompts

`prefix :` and daemon-issued command prompts use the same desktop surface. Value prompts such as
`prefix $` rename-session and `prefix ,` rename-window retain their daemon-owned labels, initial values,
and private substitution templates. Raw-key (`-1`, `-N`, `-k`), incremental (`-i`), and backspace-exit
(`-e`) prompt modes retain their original handling. The client relays raw keys on the pane-targeted
key path and does not rewrite those prompts through unified search.

Native palette actions use existing mux navigation and execution APIs. Catalog actions submit
structured command invocations. Daemon prompts submit through `CommandPromptAction::Submit`.
Local freeform commands use a structured `if-shell -F 1` invocation with the entire command as
one argument, so the daemon retains command parsing, aliases, variables, and remote execution
context. The client does not interpret the command as an operating-system shell string.

# Daemon tree requests

`prefix s` opens collapsed sessions with the invoking session selected. `prefix w` opens
expanded sessions and collapsed windows with the invoking window selected. Both publish
`ChooseTreeKind::Windows`; the palette keeps the session, window, and pane rows supplied by the
daemon, including their depth, expansion flags, custom row text, and original source indices.
Backspace on an empty search closes the daemon chooser and returns the same palette instance
to the default Workspace view, keeping input focus. Backspace with text only edits that text.

Enter sends `ChooseTreeAction::ActivateIndex` with the daemon row index; Escape or clicking
outside sends `ChooseTreeAction::Close`. The daemon retains custom selection templates, target
validation, and zoom restoration. Returning to the default view uses the same close action.
The workspace ignores queued chooser updates until the client records the close; a later chooser
request can then open normally, including when close and open events arrive together.

The daemon publishes visible rows, so palette search expands collapsed branches through
`Select` followed by `Expand`, one branch at a time. The adapter waits for state that confirms
each expansion before choosing another branch and matches against the resulting daemon rows.
Clearing the query collapses only branches opened for search. The adapter resolves targets against
fresh source indices as rows appear or disappear; it uses the existing chooser messages without
a wire change. `CommandPaletteView::advance_chooser_search` in
`crates/zz/src/command/palette.rs` owns this sequence.

# How completions are sourced

```
zz-protocol command catalog (static, renderer-neutral)
        │  canonical name + aliases + description + option/flag completion kinds
        ▼
zz-client completion engine (token-aware, over the current MuxSnapshot)
        │  recognizes: command token / option tokens / option values / quoted values / cursor token
        ▼
Ranked suggestions: exact → canonical-prefix → alias-prefix → fuzzy → description
        │  restricted to: unused flags, required enum values, live session/window/pane targets
        ▼
CommandPaletteView renders a scrollable list of 28px row slots
```

The catalog is plain Rust data with no GPUI dependency, so it is unit-testable inside `zz-protocol`
(every canonical name unique, every alias resolves to exactly one canonical command, static enum
values valid for their command) independent of rendering. Live target completions (sessions,
windows, panes) are resolved client-side from the same `MuxSnapshot` [`AppView`](/crates/zz.md)
already reconciles panes from. A target that disappears between suggestion and submission is
rejected by the daemon's existing target validation rather than trusted client-side.

# How it is rendered in GPUI

`zz-ui::command::CommandPaletteSurface` owns the shared panel, usage line, and shortcut footer.
`command/palette.rs` builds its Input, Tag pills, ListItem rows, section headers, status dots,
and matched-label spans. `PaletteRow::icon` and `PaletteRow::expanded` carry the tree icon and
disclosure state. `command_palette_tree_entry` gives the disclosure its own toggle handler while
the row keeps its activation handler. The desktop adapter owns mode and selection state in
`crates/zz/src/command/palette/model.rs`; each client retains its own mux and input lifecycle.
`InputState` continues to handle selection, IME, clipboard, and undo. Mode changes update its
placeholder without replacing the field.

The desktop overlay starts near 12% of the viewport height, caps its width at 560px, and leaves
16px side margins. The panel uses `popover_style`, an opaque raised Chroma background, the
shared edge and shadow, and the configured theme radius plus the 8px list inset. A 40px input
row contains 12px search text and pills. Rows match dropdown typography with 12px medium labels,
10px secondary text, 16px line heights, 26px highlights, and 2px gaps; the scrolling area reaches
at most 440px. Usage and footer text are 10px. The normal UI font follows the theme; usage, prefixes, command pills, and shortcut
badges use its monospace font.

Colors derive from `cx.theme()`. Selection uses the shared opaque accent treatment with normal
foreground text. Muted text brightens when selected, matches use semibold foreground, online
hosts use success, reconnecting hosts use warning, and session/window running-agent summary dots
and mode prefixes use accent. No prototype colors enter the application code.

The desktop opening animation uses the palette entity's identity, so chooser revisions and
returning to the default view cannot restart it. The overlay keeps focus in the
single Input and does not resize the workspace. The browser and specialized daemon prompts
retain their completion contract: Tab accepts a suggestion, while Enter either accepts an
engaged suggestion or submits the current input.

# Protocol shape

`CommandPromptAction` variants live under `zz_protocol::InputMessage`:

| Variant | Purpose |
|---------|---------|
| `Update { input, cursor }` | Persist local text + Unicode-scalar cursor after every accepted edit, without the daemon publishing an echo `CommandPrompt` event back (so `InputState` selection/undo is never reset by the client's own edits) |
| `Submit { input }` | Submit the complete current value through the existing daemon parse/template/execute path, guarding against a race between a final edit and execution |
| `Close` | Cancel and remove the daemon-side prompt |

`CommandPromptState { prompt, input, cursor, kind, history }` is published on open, explicit server
change, and resync. `kind` separates command and value prompts; `history` is populated only for
command prompts. The `Update`/`Submit`/`Close` actions and this state shape entered the wire at v32;
the live version is the `PROTOCOL_VERSION` constant in `crates/zz-protocol/src/message.rs`. See
[the wire protocol](/protocol/wire-protocol.md).

# Key files

| File | Role |
| --- | --- |
| `crates/zz/src/command/palette.rs` | Desktop input, suggestion selection, pointer dismissal, and prompt synchronization |
| `clients/web/src/command_palette.rs` | Browser adapter for the same shared completion engine and palette widgets |
| `crates/zz-ui/src/command.rs` | Shared input, completion rows, badges, shortcut hints, and floating palette surface |
| `crates/zz-ui/src/command/palette.rs` | Shared tree disclosure, icon, row, pill, section, highlight, and status presentation |
| `crates/zz/src/command/palette/model.rs` | Desktop navigation tree, search modes, result grouping, target selection, and fuzzy matching |
| `crates/zz-client/src/completion.rs` | Tokenizes and ranks catalog, option, enum, and live-target completions against the current `MuxSnapshot` |
| `crates/zz-protocol/src/catalog.rs` | The renderer-free command catalog shared by execution parsing and UI completion; tests enforce unique canonical names, aliases, and options |
| `crates/zz-protocol/src/message.rs` | Prompt kind/history and the native edit/submit/close actions |
| `crates/zz-daemon/src/daemon.rs` | Prompt state, bounded history, template substitution, and final execution |

`AppView` gives the palette highest focus precedence and renders it as an overlay, so it never
resizes the pane workspace. `chooser/tree.rs` renders requests with `ChooseTreeKind::Panes` or
`ChooseTreeKind::Clients`; `ChooseTreeKind::Windows` requests render their session/window/pane
tree through the palette. `chooser/buffer.rs` and `pane/display.rs` remain
separate daemon-driven surfaces.

# Related

- [`zz` crate](/crates/zz.md) . `AppView` hosts the palette; `mux/client.rs` delivers
  `CommandPromptState` to its desktop adapter.
- [`zz-client` crate](/crates/zz-client.md) . shared completion engine.
- [`zz-protocol` crate](/crates/zz-protocol.md) . shared command catalog.
- [`zz-mux` crate](/crates/zz-mux.md) . command execution.
- [Tmux command catalog](/tmux/commands.md) . the catalog entries this palette's completions read.
- [Tmux compatibility](/tmux/tmux-compat.md) and [key tables](/tmux/key-tables.md) . the broader
  tmux-compatible surface `command-prompt` is one part of.
- [Wire protocol](/protocol/wire-protocol.md) . carries `CommandPromptState` and
  `CommandPromptAction`.
- [Split-pane layout](/concepts/split-pane-layout.md) . the workspace this overlay never resizes or
  dims.
