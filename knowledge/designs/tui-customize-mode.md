---
type: Design Plan
title: Per-pane TUI customize mode
description: Reuse the pane mode stack and chooser grid for tmux option editing, with a separate entry point for future zz controls.
status: Implemented for opening and scoped option editing on 2026-09-16; gate review pending.
resource: crates/zz-mux/src/command/customize.rs
timestamp: 2026-09-17T00:00:00Z
tags: [tui, tmux, options]
---

# Ownership

Customize-mode belongs to the pane. The daemon stores `CustomizeMode` in the existing
`PaneModeRequest` stack, alongside clock and switch. Each snapshot carries the same
selection, expansion state and prompt to every viewer of that pane. A later attach sees
the current tree. Escape pops the mode and restores the previous mode.

The implementation reuses `ChooseTreeState`, `ChooseTreeItem`, `ChooserPresentation`
and the TUI chooser grid. It does not enter a per-client `ChooseTreeSession`.
The renderer reads item depth and flags; the mux owns option identities, scopes and
edits. Tree items carry the source pane as their wire target. The mux resolves the
selected row through its stable option identity before constructing a normal
`set-option` command, so the daemon runs the existing option effects.

The pin calls this mode `options-mode` in formats. It opens collapsed Server Options,
Session Options, Window & Pane Options and key table sections. Its description box
uses the bottom twelve rows. The option metadata comes from the pinned
`compat/.cache/tmux-src/options-table.c`: type, description, units and choices.
`window-customize.c` and `mode-tree.c` define the presentation and edit behavior.

# zz controls extension

The sibling zz-knobs lane can add an explicit `z` action that reveals a collapsed
`zz TUI Options` root after the pin's sections. `MuxEngine::customize_rows` is the
row assembly point; `CustomizeMode` owns the visibility flag and `customize_key`
owns the action. The pin assigns row shortcuts to digits and Meta-letters; plain `z` is not a
customize or mode-tree action. The default tree has only the pin's roots. This keeps the decoded
pin comparison exact at every default checkpoint, even when the sibling adds its
rows. The sibling must make the entry discoverable in help and test the revealed
tree as a zz extension. It must not mask extra rows in the pin comparison.

# Suspend

The daemon resolves suspend-client through the detach-client target resolver and
signals a terminal client using the PID from its handshake. The raw client pauses
its output writer, restores terminal modes, then sends itself SIGSTOP. SIGCONT
re-enters the terminal, restarts painting and checks its geometry. The guard keeps
its original termios throughout. Control and clients without a tty receive no
process signal; the pin measurement for these classes belongs in the evidence.

# Wire and proof

The pane tree extends the same unreleased v104 as the base pane mode field.
Suspend uses process signals plus the tail `ClientSuspendState` input variant to restore
the client's attachment accounting after SIGCONT. The evidence under
`compat/tui/evidence/TUI-014/attempt-07-customize/` records exact opening-screen
comparisons, a numeric option edit, terminal stop/resume, and regression results.
The two newly asserted cases have one-sided sabotages in the fixture self-check.

The 2026-09-17 fix pass covers all eight editable array options. Root edits insert
at the first unused index when submitted; child edits replace that index. Both
preserve other entries and the existing owner scope. Hook arrays stay out of the
customize tree, matching the pin. C-c and the measured unbound mode-tree keys
leave the mode open; q, Escape and C-g close it.

The opening-screen proof does not cover every customize interaction. Key binding
editing, reset/unset and tagged bulk mutations, help, mouse,
kill-on-exit (`-k`) and zoom restoration (`-Z`) remain unimplemented; the two flags
are explicitly refused. Search, filtering, navigation and arbitrary option edits
have not all received pin screen comparisons. The zz section belongs to the
sibling lane. Actual GUI, web and remote SSH suspend behavior was not exercised;
the no-tty policy was tested in the daemon and compared with a pin control client.

Source inspection also identifies a remote limitation. The built-in SSH endpoint
uses `EndpointFactsScope::PortableTerminalSize` in
`crates/zz-daemon/src/client.rs`, which omits the client tty. It therefore takes
the suspend handler's no-tty no-op path. The process-signal implementation covers
a TUI connected to a local daemon; it does not suspend a local TUI through the
built-in SSH transport. No live SSH suspension proof was run.
