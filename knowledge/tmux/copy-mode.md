---
type: Concept
title: Copy mode and view mode
description: "Daemon-native tmux copy/view mode over libghostty history: vi/emacs movement tables, selection, incremental search, jumps, and copy/pipe variants, driven by send-keys -X and painted by GPUI."
resource: crates/zz-mux/src/command.rs
tags: [tmux, copy-mode, view-mode, selection, search]
timestamp: 2026-08-26T00:00:00-03:00
---

# Overview

Copy mode is zz's native, per-client, read-only navigation over a pane's canonical libghostty
history. **View mode** is the same machinery in read-only form for command output (e.g. `C-b ?`
`list-keys` output or output submitted through `C-b :`) and for browser panes, which reuse the copy
tables without turning browser pixels into terminal content. Entering copy mode freezes a stable
view revision while PTY output keeps flowing underneath; each attached client owns an independent mode
cursor, selection, and search state. Nothing is drawn as terminal escape
sequences: GPUI paints the cursor, selection, matches, scrollbar, and mode indicator.

[`crates/zz-mux`](/crates/zz-mux.md) is the state-machine half: the `copy-mode` command enters the mode and
every navigation/selection/copy action arrives as `send-keys -X <action>` from the
[`copy-mode`/`copy-mode-vi` key tables](/tmux/key-tables.md). `command.rs::copy_mode_action` parses
each `-X` action into a `zz_terminal::CopyModeAction` (see [zz-terminal](/crates/zz-terminal.md))
wrapped in `MuxEffect::TerminalView`. A single action uses `TerminalViewAction::CopyMode`; a counted
action uses the flat `TerminalViewAction::CopyModeCounted { action, count }` shape. The terminal
actor executes both against the frozen revision. `-X` actions never reach the PTY.

# Entry, mode-keys, and search

- **Entry:** `copy-mode` (default `C-b [`) is the only way in; `-u` immediately pages up, `-t` targets a
  pane. No pointer gesture enters the mode: a wheel just moves the live viewport offset and a drag paints
  the live selection overlay. The daemon dispatches the freeze and the client's key-table switch together
  (`enter_copy_session`), so the two can never move independently.
- **Exit:** `cancel` (`q` and `C-c` in vi; Escape, `q`, and `C-c` in emacs), any `-and-cancel`
  copy action, pane death, focusing another pane, or detaching. Stock vi Escape is
  `clear-selection`, matching the pinned tmux table; it does not exit. The separate
  `clear-selection-or-cancel` action remains available to custom bindings.
- **Reconciliation:** copy mode lives on the terminal actor while the key table lives on the client, and a
  lost edge between them used to strand a pane frozen with its client's keystrokes still reaching the PTY.
  The daemon holds the authoritative `copy_sessions` map and re-checks it on every published viewport
  (`reconcile_copy_session`): an unclaimed frozen pane is told to cancel, and a session whose pane went live
  is dropped along with its key table. Any missed edge heals within one publish. `View`-kind frozen views
  (the command-output pager) publish through their own path and are untouched.
- **mode-keys:** the active table (`copy-mode` = emacs, `copy-mode-vi` = vi) is resolved per pane via
  the window's `mode-keys` option (`copy_mode_table_for_pane`), a window option with global
  inheritance, defaulting to emacs.
- **Search:** `copy-mode-search-prompt` (bound to `/`,`?` in vi and `C-s`,`C-r` in emacs; `-b` =
  backward) opens the native GPUI search prompt via `MuxEffect::TerminalUi { BeginSearch { direction }}`;
  `search-again`/`search-reverse` repeat it, while vi `*`/`#` search forward/backward for the word
  under the copy cursor. `word-separators` (a session option) and `set-clipboard` /`copy-command`
  shape word selection and copy destinations.

# TUI command-output navigation

Command output uses the daemon's copy tables without entering an ordinary pane's copy session. The
daemon switches the attached client's active key engine to the output pane's effective `copy-mode`
or `copy-mode-vi` table. A live `mode-keys` change retargets the open view, and a live `bind-key`
change affects its next key. The TUI forwards press and repeat keys into that engine and drops
releases. Stock emacs `q` and Escape cancel the output. Stock vi `q` cancels, while Escape runs
`clear-selection` and leaves the output open.

`TerminalUiCommand::BeginSearch` opens a command-output search editor inside the TUI. Text,
Backspace, and bounded control-free paste send `SearchUpdate`. Enter submits the live query by
leaving edit mode. Escape sends `SearchClose`, and the effective copy table routes `n` and `N` to
the next and previous match. This editor belongs only to command output. An ordinary terminal pane
in TUI copy mode still has no TUI search editor.

Line movement, page movement, selection, rectangle changes, and copy actions operate on the retained
output terminal. The compatibility fixture checks that a selected match reaches a daemon paste
buffer. It does not claim an OS clipboard write.

Protocol v79 adds `output_id` to the existing `EventPayload::CommandOutput` tag. Real frames and
closes use a nonzero actor ID; ID zero with no viewport is the authoritative no-output resync.
`ClientCore` retains a watermark and ignores older actor traffic, including a late frame after its
close. The TUI keeps local search, swallowed-key, and resize-cache state while the same actor
publishes new frames, then clears it for a replacement actor, close, or reconnect. The renderer and
`ResizeCommandOutput` share one full-width content rectangle that reserves an output-header row, a
footer or message row, and the tmux status block, so the terminal actor receives the same rows and
columns the TUI paints.

`compat/attached-client.sh` creates 96 output lines and runs the same semantic checks on zz and pinned
tmux: line and page movement, search editing, `n`/`N`, selection-to-paste-buffer, a live custom
binding, and both vi and emacs tables. The fixture compares terminal state and visible markers. It
does not prove mouse behavior, SSH transport, pixel parity, or the stored canonical summary. The 29
unsupported `window-copy` actions below remain open.

# Schema: `send-keys -X` action families

| Family | Actions |
| --- | --- |
| Cursor | `cursor-left/right/up/down`, `start-of-line`, `back-to-indentation`, `end-of-line` |
| Word/para | `next-word`, `previous-word`, `next-word-end`, `next-space`, `previous-space`, `next-space-end`, `next-paragraph`, `previous-paragraph` |
| Page/history | `page-up/down`, `halfpage-up/down`, `scroll-up/down`, `scroll-middle`, `goto-line`, `history-top`, `history-bottom`, `top-line`, `middle-line`, `bottom-line` |
| Semantic prompt | `next-prompt`, `previous-prompt` (`-o` = output only, via OSC 133 marks) |
| Search | `search-again`, `search-reverse`, cursor-word forward/backward actions used by `*`/`#` |
| Selection | `begin-selection`, `select-word`, `select-line`, `selection-mode`, `clear-selection`, `stop-selection`, `clear-selection-or-cancel`, `other-end` |
| Rectangle | `rectangle-toggle`, `rectangle-on`, `rectangle-off` |
| Marks | `set-mark`, `jump-to-mark` |
| Character jump | `jump-forward`, `jump-backward`, `jump-to-forward`, `jump-to-backward` (capture one target key), `jump-again`, `jump-reverse`, `next-matching-bracket` |
| Copy | `copy-selection`, `copy-selection-no-clear`, `copy-selection-and-cancel`, `copy-end-of-line`, `copy-end-of-line-and-cancel`, `append-selection`, `append-selection-and-cancel` |
| Pipe | `copy-pipe`, `copy-pipe-no-clear`, `copy-pipe-and-cancel`, `pipe`, `pipe-no-clear`, `pipe-and-cancel` |
| Exit | `cancel`, `clear-selection-or-cancel` (exits only with no selection) |

The pinned `window-copy` command table contains 95 action names, and
[`crates/zz-mux/src/copy_actions.rs`](/crates/zz-mux.md) is the source-owned inventory of all of
them: each carries the behavior category that owns it and the pin's `WINDOW_COPY_CMD_FLAG_READONLY`
bit, while support is derived from the `send-keys -X` parser rather than stored. The categories are
the `copy-mode.action-fidelity` items: vocabulary, cursor geometry, logical-line and mode-key
behavior, goto-line, selection lifecycle, jump/page/prompt actions, and copy formatting and
destination effects. Its tests pin the mapped count, assert every mapped name reproduces the pin's
read-only classification, and list the missing names per category, so mapping one action shrinks
that list instead of drifting from the code. The table above describes the mapped surface only.

Because a detached `send-keys -X` fails on the pin with `not in a mode`, an action zz has not mapped
exits zero where tmux exits one. `compat/scenarios/copy-mode-*.txt` measure exactly that: a name in
one of those corpora is proof it reaches a typed action, not proof of its semantics.

`top-line`, `middle-line`, and `bottom-line` now set the copy cursor's column to zero and place its
row at the top, middle, or bottom of the current frozen viewport. They preserve the viewport offset
and clamp to the retained revision. The full `zz-terminal` library suite passes 197 tests for this
slice. That evidence does not cover `history-bottom`, logical-line behavior, wrapping, scrolling, or
the other missing actions.

Copy/pipe actions build a `CopyModeCopy` whose flags mirror tmux: `clipboard` is set unless `-C` or
`set-clipboard off`; a paste buffer is created unless `-P` (append variants append); `pipe` runs the
argument or the configured `copy-command`; `-no-clear` keeps the selection and `-and-cancel` leaves
the mode. Jump bindings are the reason [`KeyEngine`](/tmux/key-tables.md) has a `pending` state:
it captures exactly one following key as the jump target. Vi digits `1` through `9` similarly buffer
a numeric prefix; following digits extend it (so `10E` works), then one `send-keys -N <count> -X ...`
invocation carries the count through one mux effect and terminal action. Prefix-consuming movements,
jumps, matching brackets, and repeat-search actions execute N times without allocating N cloned
effects. `other-end` swaps only for odd N, and `select-line` selects N lines downward. Ordinary
rectangle toggles, begin-selection, clear-selection, copy-selection, cancel, and later actions in
the same binding execute once. The copy-end-of-line family extends through the end of the Nth row
and copies once, which covers direct `3D` and counted `C-k` command shapes. Resolution stops at the
first `send` or `send-keys` command whose option prefix contains `-X`. Its stored `-N` wins;
otherwise the engine inserts separate `-N <count>` arguments immediately before the option argument
containing `-X`. It does not scan onward after a stored `-N`, and a binding with no qualifying `-X`
leaves the buffered count armed. Invalid nonempty `-X` grammar restores the neutral count of one,
represented as no buffered count, so a following `5` starts at five rather than extending one to
fifteen. Bare `0` remains `start-of-line`; counted `Escape` runs clear-selection once. Empty
`send-keys -N <n> -X` still needs tmux's pane-owned prefix persistence and remains tracked under
`terminal.key-control`.

## Selection lifecycle

The pin's `selflag` is the unit a live selection extends by, and zz keeps it on the frozen mode as
`selection_mode` alongside the raw `selection_origin` the selection started from. `selection-mode`
takes `char`/`c`, `word`/`w`, and `line`/`l` case-insensitively, defaults to `char` with no
argument, and silently does nothing for a name it does not recognise. While the unit is `word` or
`line`, every cursor move re-derives both selection ends from the origin: forward of the origin the
anchor sits at the start of the origin's word or logical line and the focus at the end of the
cursor's, and behind it the two swap. `begin-selection`, `other-end`, and `clear-selection` reset
the unit to `char`; `select-word` and `select-line` arm `word` and `line`. `stop-selection` is typed
apart from `clear-selection`: it stops the selection following the cursor and resets the unit but
leaves the painted range, while `clear-selection` drops the range too. Search-driven cursor sync
still extends by cell, because the search path does not carry `word-separators`.

# Selection text

Copy mode formats text from its frozen `ModeRevision`, not from the live libghostty selection. The
formatter emits empty narrow cells as spaces when they are inside a row's real content, trims only
padding beyond the logical end of a hard row, joins soft-wrapped rows without a newline, preserves
rectangle columns, and keeps the final newline for linewise (`V`) selections. It deliberately skips
wide-character spacer cells. The resulting `String` is passed unchanged through the daemon and system
clipboard; paste encoding does not normalize spaces or tabs.

The stock emacs table includes all 13 direct navigation aliases that map to existing typed actions:
scroll lines, semantic prompts, matching brackets, line and history endpoints, half pages, top-line
positioning, and page-down. Each binding stores the pin's `send-keys -X` command with no repeat bit.

The emacs table also installs `C-[`, `C-k`, `C-w`, `N`, `R`, and `n` for cancel, both copy variants,
reverse search, rectangle toggle, and forward search. Each binding stores the pin's command and has
no repeat bit.

Seventeen keyboard defaults remain tracked. Ten require the pin's numeric-repeat or goto-line
command-prompt flow. Seven emacs or vi keys depend on five missing actions:
`previous-matching-bracket`, `recentre-top-bottom`, `cursor-centre-horizontal`, `toggle-position`,
and `refresh-toggle`. `keys.copy-mode-unsupported-default-actions` owns only those seven keys and
depends on the wider 29-action inventory in `copy-mode.action-fidelity`. zz presents the position
label in native chrome and freezes the copy revision, but the tracker keeps both tmux toggles open
until the project makes a product decision. Mouse pseudo-keys use the direct pointer route instead
of `KeyEngine` bindings.

Copy-pipe worker exit failures are silent on Control clients. Pinned tmux starts the worker without
a completion callback, so a delayed nonzero exit produces no `%message`, `%error`, or extra command
guard. zz matches that Control contract and still cancels copy mode. Its native Interactive client
keeps the existing error notification, including when a Control action falls back to an attached
Interactive viewer.

# App-initiated clipboard writes (OSC 52)

A program in the pane can reach the clipboard without copy mode: `"+y` in nvim, lazygit, a nested
tmux, any osc52 script. libghostty normalizes the OSC 52 selector, base64, multipart chunks, and the
iTerm2 OSC 1337 Copy spelling before zz sees anything, then calls the hook `zz-terminal` registers in
`session.rs::register_clipboard_write`. zz picks the `text/plain` representation (falling back to the
first one offered), refuses a payload over 8 MiB, and raises `TerminalEvent::ClipboardSet`. That
event is pane-scoped on purpose — no client view asked for it — so
`daemon.rs::deliver_clipboard_write` fans it out to every client attached to the pane's session as
the same `EventPayload::Clipboard` a copy-mode yank publishes. Remote panes work identically: the
event rides the ssh-forwarded socket and lands on the local machine's pasteboard.

This is where `set-clipboard` stops being a single on/off switch: `off` drops the write, `external`
(the default) only forwards it to the attached clients' pasteboards, and `on` additionally creates
the automatic paste buffer a yank would. Copy mode's own `copy-selection`/`copy-pipe` still only ask
whether the value is `off`, so `on` and `external` differ for app-initiated writes alone.

Clipboard *reads* stay unsupported. libghostty never forwards an OSC 52 query (`?`) to the hook, so a
program in a pane cannot read the clipboard back — deliberate, since a remote host would otherwise
get to exfiltrate whatever the local machine copied. Kitty's OSC 5522 is unimplemented.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/command.rs` | `copy_mode`, `copy_mode_search_prompt`, `copy_mode_action`, `copy_pipe_action`, `copy_selection_action`, `copy_jump_action`. |
| `crates/zz-protocol/src/key.rs` | Default `copy-mode`/`copy-mode-vi` tables and search bindings; jump-target and numeric-prefix capture. |
| `crates/zz-daemon/src/daemon.rs` | `copy_sessions` authority: `enter_copy_session`, `exit_copy_session`, `unfocused_copy_sessions`, `retarget_copy_mode_tables`, `reconcile_copy_session`; `deliver_clipboard_write` applies `set-clipboard` to OSC 52 writes. |
| `crates/zz-terminal/src/session.rs` | Native action execution plus `register_clipboard_write` / `clipboard_write_request` for OSC 52. |
| `crates/zz-terminal/src/session/mode_revision.rs` | Frozen selection text formatting, including hard/soft line structure and blank-cell spacing. |

# Related

- Actions are dispatched by [commands](/tmux/commands.md) via `send-keys -X`; keys come from the
  [key tables](/tmux/key-tables.md).
- `CopyModeAction`/`CopyModeCopy` and execution live in [zz-terminal](/crates/zz-terminal.md).
- Sibling native overlays: [choose-tree / choose-buffer](/tmux/choose-tree.md). Checked against
  `window-copy.c`/`grid-reader.c` in the [tmux upstream reference](/references/tmux-upstream.md).
