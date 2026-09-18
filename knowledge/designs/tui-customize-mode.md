---
type: Design Plan
title: Per-pane TUI customize mode
description: Port mode-tree.c and window-customize.c onto the server pane mode stack, and share one prompt editor between customize-mode and switch-mode.
status: Implemented for the tree keys, prompts, array items and previews on 2026-09-17; gate review pending.
resource: crates/zz-mux/src/command/customize.rs
timestamp: 2026-09-17T00:00:00Z
tags: [tui, tmux, options]
---

# Ownership

Customize-mode belongs to the pane. The daemon stores `CustomizeMode` in the
`PaneModeRequest` stack, alongside clock and switch. Each snapshot carries the same
tree, selection and prompt to every viewer of that pane, and a later attach sees the
current tree. Escape pops the mode and restores the previous one. `-k` kills the pane
and `-Z` zooms it for the life of the entry, through the same stack code switch-mode uses.

The mux owns the tree and the key handling; the raw TUI only draws. `CustomizeMode`
keeps what `mode_tree_data` keeps: `current`, `offset` and `height`, the expanded and
tagged item ids, the preview size, filter, search string, help flag and the open prompt.
Rows are rebuilt from live option and key state on every call, the way
`window_customize_build` rebuilds them, and each place the pin calls `mode_tree_build`
runs `customize_build`: it finds the old current item by id, falls back to the last
line, recomputes `height` and normalises `offset`, and drops tags whose parent is
collapsed. Edits become ordinary `set-option`, `bind-key` and `unbind-key` invocations
the daemon executes, and the build that follows them runs after they land.

# Rows

The tree follows `window_customize_build`: Server Options, Session Options and Window &
Pane Options, then one Key Table root per non-empty table. Options sort by name with user
options first. Array options have no text, and each index is a child named
`name[key]` whose text is the same format expansion, so a pane-scope entry carries the
`(pane N)` marker. A key row has Command, Note and Repeat children that draw their parent's
preview. Text passes through `#[ignore]`, which `parse_styled_segments` now honours the way
`format_draw` does, so values like `pane-border-format` show their markup literally.

Previews are laid out on the daemon through a port of `screen_write_text`, including the
description, scope, `This is an array option, key N.`, the value, `This expands to:`,
choices, the `EXAMPLE` swatch for colour and style options, the default, and the window
and global values. They reach the client as `ChooserPreview::Markup` lines, with the
drawn-as-parent title in the selected item's `detail`.

# Keys

`customize_key` ports `mode_tree_key` and `window_customize_key`: movement with and
without wrap, page keys, `g`/`G`, row shortcuts, Right expanding a collapsed row and
descending an expanded one, Left collapsing or moving to the parent or up a row, `M--`
and `M-+` keeping the current line, the three-state `v` cycle, `t`/`T`/`C-t` tagging
(keyboard `t` does not move), search with `n`/`N` over the whole tree, filter and `c`,
`H`, help on `C-h`/`F1`, `s`/`w`/`S`/`W`/Enter scope selection with flag and choice
cycling, `a` for array keys, and the `d`, `u`, `D`, `U` single-key confirmations
(`-y` answers them at once).

# Prompts

`ModePrompt` in `crates/zz-mux/src/command/mode_prompt.rs` ports `prompt_key` and
`prompt_draw` for mode prompts: emacs editing, incremental notifications, single-key
prompts, and the draw that scrolls a long value so the cursor stays on the row. The
daemon sends only the drawn row and the cursor column (`PaneMode::Customize.prompt`,
`prompt_cursor`, `prompt_top`), never the whole value, so no prompt text reaches a
bounded wire field. Switch-mode uses the same editor for its `(search)` prompt.

# Switch mode

`SwitchMode` keeps `current`, `offset`, the prompt and the filter across snapshots.
`window_switch_key`'s map sends `C-p`/`C-k` up and `C-n`/`C-j` down, Enter runs the
template on the current match and does nothing when nothing matches, and every other key
edits the prompt, whose changes reset the selection to the top. The daemon ranks rows with
`fuzzy_match_columns`, which skips `#[...]` styles and returns the matched columns the
client repaints with `switch-mode-match-style`.

# zz controls extension

The mode tree is a superset surface: zz's own TUI options belong in it beside the pin's
rows (fabrico, 2026-09-16). `MuxEngine::customize_rows` is the row assembly point, so a
`zz TUI Options` root appends there after the pin's sections. It must stay out of the
default tree the pin comparison reads, reached through an explicit action that is not a
mode-tree key, and it needs its own tests as a zz extension. It is not built yet.

# The pointer

Neither mode declares a key table, so `server_client_key_callback` never swaps one in for
them; their pointer rows are reached the way `window_pane_key` reaches any mode, through
the root binding's `send -M` and, with nothing bound, through the forward that follows an
unclaimed mouse key. `MuxEngine::customize_mouse` ports `mode_tree_key`'s pointer half and
`SwitchMode::mouse` ports `window_switch_key`'s: a press selects the line under the
pointer, a double click selects and activates, the wheel steps one row in switch-mode and
is swallowed in the tree, a press outside the tree's own height does nothing, and a
button-1 press on the prompt row moves the prompt cursor through `prompt_mouse`. The raw
TUI forwards `MouseDown1Pane`, `MouseDown3Pane`, `DoubleClick1Pane`, `WheelUpPane` and
`WheelDownPane` whenever the pane it resolved holds a mode, because the daemon is the side
that runs `window_pane_key` and a name with no binding still has to arrive. The replayed
`DoubleClick`'s `m->ignore` is the pane's own input drop, not the mode's, so it applies
after the mode has had the key.

# Prompts

Every prompt either mode raises is built through `prompt_set_options`, so it keeps the
raising session's `status-keys` and `word-separators`. Under `status-keys vi` an Escape
puts the prompt in command mode with the cursor stepped back rather than cancelling it,
`ModePrompt::translate_vi` carries the whole `prompt_translate_key` table including the
three `KEYC_VI` word motions, and the daemon sends `message-command-style` in
`ChooserPresentation.prompt_style` while the prompt sits there.

# Limits

Not built: `mode_tree_display_menu` on `MouseDown3Pane` (the press selects the line on both
binaries and only the pin then opens the menu, `semantic:mode-tree-mouse-menu`), the
key-binding reset for keys whose default command changed in place, prompt history
(`Up`/`Down` in an edit prompt), `Tab` completion in a command prompt, and `C-y` pasting
the top buffer, which is also what vi `p` maps onto. Rows are rebuilt live, so an option
changed from outside shows at once where the pin shows it after its next build.

# Suspend

The daemon resolves suspend-client through the detach-client target resolver and
signals a terminal client using the PID from its handshake. The raw client pauses
its output writer, restores terminal modes, then sends itself SIGSTOP. SIGCONT
re-enters the terminal, restarts painting and checks its geometry. Control and clients
without a tty receive no process signal. The built-in SSH endpoint omits the client tty,
so a TUI connected through it takes the no-tty path; no live SSH suspension was run.
