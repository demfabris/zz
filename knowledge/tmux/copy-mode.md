---
type: Concept
title: Copy mode and view mode
description: "Daemon-native tmux copy/view mode over libghostty history: vi/emacs movement tables, selection, incremental search, jumps, and copy/pipe variants, driven by send-keys -X and painted by GPUI."
resource: crates/zz-mux/src/command.rs
tags: [tmux, copy-mode, view-mode, selection, search]
timestamp: 2026-08-10T00:00:00Z
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
wrapped in `MuxEffect::TerminalView { pane, action: TerminalViewAction::CopyMode(...) }`; the terminal
actor executes it against the frozen revision. `-X` actions never reach the PTY.

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

# Schema: `send-keys -X` action families

| Family | Actions |
| --- | --- |
| Cursor | `cursor-left/right/up/down`, `start-of-line`, `back-to-indentation`, `end-of-line` |
| Word/para | `next-word`, `previous-word`, `next-word-end`, `next-space`, `previous-space`, `next-space-end`, `next-paragraph`, `previous-paragraph` |
| Page/history | `page-up/down`, `halfpage-up/down`, `scroll-up/down`, `scroll-middle`, `goto-line`, `history-top`, `history-bottom`, `top-line`, `middle-line`, `bottom-line` |
| Semantic prompt | `next-prompt`, `previous-prompt` (`-o` = output only, via OSC 133 marks) |
| Search | `search-again`, `search-reverse`, cursor-word forward/backward actions used by `*`/`#` |
| Selection | `begin-selection`, `select-word`, `select-line`, `clear-selection`/`stop-selection`, `clear-selection-or-cancel`, `other-end` |
| Rectangle | `rectangle-toggle`, `rectangle-on`, `rectangle-off` |
| Marks | `set-mark`, `jump-to-mark` |
| Character jump | `jump-forward`, `jump-backward`, `jump-to-forward`, `jump-to-backward` (capture one target key), `jump-again`, `jump-reverse`, `next-matching-bracket` |
| Copy | `copy-selection`, `copy-selection-no-clear`, `copy-selection-and-cancel`, `copy-end-of-line`, `copy-end-of-line-and-cancel`, `append-selection`, `append-selection-and-cancel` |
| Pipe | `copy-pipe`, `copy-pipe-no-clear`, `copy-pipe-and-cancel`, `pipe`, `pipe-no-clear`, `pipe-and-cancel` |
| Exit | `cancel`, `clear-selection-or-cancel` (exits only with no selection) |

Copy/pipe actions build a `CopyModeCopy` whose flags mirror tmux: `clipboard` is set unless `-C` or
`set-clipboard off`; a paste buffer is created unless `-P` (append variants append); `pipe` runs the
argument or the configured `copy-command`; `-no-clear` keeps the selection and `-and-cancel` leaves
the mode. Jump bindings are the reason [`KeyEngine`](/tmux/key-tables.md) has a `pending` state:
it captures exactly one following key as the jump target. Vi digits `1` through `9` similarly buffer
a numeric prefix; following digits extend it (so `10E` works) and the next `send-keys -X` motion is
repeated. Bare `0` remains `start-of-line`.

# Selection text

Copy mode formats text from its frozen `ModeRevision`, not from the live libghostty selection. The
formatter emits empty narrow cells as spaces when they are inside a row's real content, trims only
padding beyond the logical end of a hard row, joins soft-wrapped rows without a newline, preserves
rectangle columns, and keeps the final newline for linewise (`V`) selections. It deliberately skips
wide-character spacer cells. The resulting `String` is passed unchanged through the daemon and system
clipboard; paste encoding does not normalize spaces or tabs.

Two pinned-tmux keyboard actions remain intentionally outside the native model: `P` toggles tmux's
position label, while zz always presents its copy-mode position in native chrome; `r` toggles live
refresh, while zz copy mode is defined as a stable frozen revision. Mouse pseudo-keys are handled by
the direct pointer route rather than `KeyEngine` bindings.

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
| `crates/zz-mux/src/key.rs` | Default `copy-mode`/`copy-mode-vi` tables and search bindings; jump-target and numeric-prefix capture. |
| `crates/zz-daemon/src/daemon.rs` | `copy_sessions` authority: `enter_copy_session`, `exit_copy_session`, `unfocused_copy_sessions`, `retarget_copy_mode_tables`, `reconcile_copy_session`; `deliver_clipboard_write` applies `set-clipboard` to OSC 52 writes. |
| `crates/zz-terminal/src/session.rs` | Native action execution plus `register_clipboard_write` / `clipboard_write_request` for OSC 52. |
| `crates/zz-terminal/src/session/mode_revision.rs` | Frozen selection text formatting, including hard/soft line structure and blank-cell spacing. |

# Related

- Actions are dispatched by [commands](/tmux/commands.md) via `send-keys -X`; keys come from the
  [key tables](/tmux/key-tables.md).
- `CopyModeAction`/`CopyModeCopy` and execution live in [zz-terminal](/crates/zz-terminal.md).
- Sibling native overlays: [choose-tree / choose-buffer](/tmux/choose-tree.md). Checked against
  `window-copy.c`/`grid-reader.c` in the [tmux upstream reference](/references/tmux-upstream.md).
