---
type: Design Plan
title: Agent pane text projection
description: Every Agent pane owns a PTY-free shadow TerminalSession fed with a text projection of its transcript (prompt between OSC 133 A/B and C, reply as output, D plus BEL at turn end), so capture-pane, show-last-output, pipe-pane, and activity/bell alerts work on agent panes with no new verbs; clients never see its frames.
status: Shipped
tags:
- agent
- terminal
- tmux-compat
- daemon
timestamp: 2026-08-26T00:00:00-03:00
---

# Why

Every tmux-based agent orchestrator surveyed on 2026-08-25 reads agents back by scraping a
terminal. zz's Agent panes were not terminals, so `capture-pane -p -t %agent` answered
`PaneExited`, `pipe-pane` had nothing to tap, and `alert-bell`/`monitor-activity` never fired for
a turn that finished. `agent-send --wait` covers request/reply; this makes the whole tmux read
grammar work on an Agent pane too.

# Mechanism (shipped 2026-08-26)

- **A PTY-free terminal per agent pane.** The `PaneKindSnapshot::Agent` arm of the daemon effect
  loop (`resource: crates/zz-daemon/src/daemon.rs`) creates
  `TerminalSession::spawn_empty_with_appearance` (the `-E` surface), sizes it from the layout
  cell (`MuxEngine::pane_geometry`), inserts it into `inner.terminals`, and watches it like any
  other terminal. It publishes `SessionStatus::Running` forever, so the watcher never closes the
  pane; `kill-pane` removes it with the pane like any terminal.
- **The feed.** `TerminalSession::feed(bytes)` (`resource: crates/zz-terminal/src/session.rs`) is
  a control-queue `Command::Output`; the empty-pane worker writes it to the VT, marks output
  activity, publishes a content frame, forwards it to an armed raw-output tap, and — because the
  worker now registers the bell callback — a fed BEL raises `TerminalEvent::Bell`. Sessions with
  a real child ignore the command.
- **The projection.** `PaneLane::project` in `AgentFanout::accept`
  (`resource: crates/zz-daemon/src/agent/fanout.rs`) turns stream items into bytes in order:
  `TurnStarted` emits `OSC 133;A` `> ` `OSC 133;B` *prompt* `OSC 133;C` (the prompt text was
  remembered by `prompt_with_waiter` and is popped when its turn actually starts, so a queued
  prompt lands after the running turn's output); `agent_message_chunk` text is output with
  LF→CRLF; `PromptFinished` emits `OSC 133;D;<0|1>` and a BEL; `PermissionRequested` prints a
  `[zz] permission requested` line and rings. Thoughts and tool calls are not projected. The
  bytes leave `accept` after the lane lock drops, through `AgentPublisher::feed_agent_pane_text`,
  which resizes the shadow to the current layout cell when it changed.

# What it buys

| Surface | Status |
| --- | --- |
| `capture-pane` (any flags, copy-mode, search) | Works — gated only on `inner.terminals` |
| `show-last-output` / `send-last-output` | Works — the kind guard accepts Agent panes; a turn is a command |
| `monitor-activity`, `monitor-silence`, `alert-activity` | Works via `mark_output_activity` |
| `alert-bell` | Works — BEL at turn end and on permission requests |
| `pipe-pane` | Works — the surface worker now arms taps; `-I` has nothing to write to |
| `send-keys` / `send-text` into an agent pane | Refused as before (`resolve_input_sinks` checks kind before the terminal map; `send-text` keeps its Terminal-only guard) |

# Hazards and how they were closed

1. **Frame publication.** `visible_terminal_panes` now excludes Agent panes, so clients never
   receive `TerminalViewport`/`TerminalPatch` frames for the shadow and never attach a view to it.
2. **Geometry.** The daemon resizes the shadow from `pane_geometry` at creation and before each
   feed when the cell changed; pixel sizes are zero (only kitty graphics care).
3. **Control mode.** `refresh_control_output_taps` skips Agent panes, so unadorned `-C`/`-CC`
   output does not grow.
4. **Memory.** One libghostty grid plus the session's `history-limit` per agent pane, holding
   only assistant text — small next to the replay ring and journal. The standing decision in
   [session persistence](/concepts/session-persistence.md) stands: the daemon keeps raw items; the
   projection is a render, not a transcript store.
5. **Empty facts.** `pane_pid`/`pane_tty`/`pane_current_command` stay empty for agent panes.
6. **The TUI card.** The TUI still paints its static Agent card; rendering the projection there
   is the obvious follow-up now that it exists.

# Related

- [Agent pane](/concepts/agent-pane.md)
- [Daemon-owned agent runtime](/designs/agent-daemon-runtime.md)
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md)
