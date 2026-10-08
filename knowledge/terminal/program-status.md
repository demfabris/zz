---
type: Subsystem
title: Program status (OSC 7501)
description: How zz reads the Program Status Protocol from pane output, keeps one record per id for each pane, and shows the most urgent record in formats, the tree, @agent_state, and the desktop sidebar.
resource: crates/zz-terminal/src/program_status.rs
tags: [osc, osc-7501, program-status, agent-state, sidebar, formats]
timestamp: 2026-10-08T12:00:00-03:00
---

# Overview

The [Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status)
(OSC 7501, revision 0.3) lets a program tell the terminal what it is doing: `idle`, `working`,
`done`, `blocked` on the user, or `error`, with an optional `id`, `kind`, `progress`, `app`,
`title` and `msg`. zz treats every pane as one terminal in the spec's sense and keeps that
pane's records in the terminal engine.

```sh
printf '\e]7501;state=blocked:kind=permission:app=terraform:msg=%s\e\\' "$(printf 'Apply?' | base64)"
zz display -p -t %3 '#{pane_status} #{pane_status_app}: #{pane_status_message}'
```

# Where zz reads it

`EngineFilter` in `crates/zz-terminal/src/session.rs` sees every OSC before libghostty does,
the same place it reads OSC 9;4 and 133;D. The pinned Ghostty fork drops OSC 7501 in its parser.
Upstream libghostty-vt added a parser and an embedder callback in ghostty-org/ghostty#14560
(merge `a4aacd91`, 2026-10-06), 134 commits past the fork's base; zz does not need it.

The filter keeps at most 64 bytes of an ordinary OSC, and up to the spec's 4096-byte sequence
for one that starts `7501;`. `parse_program_status` in `program_status.rs` applies the spec's
grammar and limits: malformed pairs and unknown keys are skipped, while a missing or unknown
`state`, a bad `id`, a value over its limit, or text that does not decode discards the whole
report. Decoded `title` and `msg` text must be UTF-8 without C0, DEL or C1 controls. Bidi
controls, zero-width spaces, word joiners and the BOM are removed before the text is stored,
because every place zz shows it is outside the terminal grid.

# Records

`ProgramStatus` holds one record per id, ordered by last update. A report replaces its record,
`state=clear` removes the id and everything below it (no id removes all), and a new record past
256 evicts the least recently updated one.

| Event | Effect |
|---|---|
| OSC 133;A (a new shell prompt) | Drops `idle`, `working` and `blocked` records |
| The pane's process exits | Same, before the exit is published |
| RIS (`ESC c`) | Drops every record and forgets that the pane ever reported |

`done` and `error` survive the prompt and the exit. They stay until the program replaces or
clears them; the desktop's own badge for them clears when the user looks at the pane.

The support query `OSC 7501 ; ? ST` is answered with the same bytes and terminator. The
reply goes into the pane's `PtyEffects` queue as the filter reads the query, so it stays in
byte order with libghostty's own replies: a program that sends the query and then DA1 sees our
answer first. Surfaces without a PTY (agent pane projections and output views) leave
`EngineFilter::replies` unset and never answer.

# The headline record

`ProgramStatus::headline` picks one record per pane: the most urgent state (`blocked`, then
`error`, `working`, `done`, `idle`), the most recently updated among equals. A record without
`app` takes it from its nearest ancestor that has one.

# Where it shows

- Formats: `#{pane_status}`, `#{pane_status_kind}`, `#{pane_status_progress}`,
  `#{pane_status_app}`, `#{pane_status_title}` and `#{pane_status_message}` read the headline
  straight from the pane's terminal. They are daemon hook variables, so they also appear in
  `inspect` and `list-panes --json`. Title and message come back with every `#` doubled, so a
  status line or border format shows them as text instead of reading `#[...]` as style, the
  way tmux escapes `window_flags`. Scripts read `#{pane_status_raw_title}` and
  `#{pane_status_raw_message}`, the unescaped twins, as with `window_raw_flags`.
  `#{pane_status_reported}` is `1` once the pane has reported since its last full reset.
- The tree: the daemon's pane watcher copies the headline into `Pane::status`, which rides
  `PaneSnapshot.status` and `TreeOp::PaneStatus` (wire v108).
- `@agent_state`: `working`, `blocked`, `error` as `failed`, and `idle` for `idle`, `done` or
  no record, so `agent-send --wait`, `zz events`, `wait-for '@agent_state@%N'` and
  `agent-state-changed` work for any program that reports. After a pane's first report, the
  OSC 9;4 bridge and the Claude peer-registry sampler stop writing `@agent_state` for it until
  a full reset, as the spec asks of OSC 9;4. The daemon remembers which panes the protocol
  owns (`program_status_panes`): a full reset or `respawn-pane` on such a pane writes `idle`,
  hands it back to the heuristics, and drops the peer sampler's memo so its next sample lands.
  A peer sample waits in the loop's hook queue as
  `if-shell -F '#{?pane_status_reported,,1}' 'set-option … @agent_state …'`, so a report that
  lands while it waits still wins.
- Desktop sidebar: every terminal pane feeds the same `AgentAttentionTracker` as Agent panes,
  idle until it reports. `blocked` shows the needs-input badge and rings, even on a pane's first
  report; `error` shows failed; and `working` to `done` or `idle` rings and leaves the finished
  badge until the pane is watched.
  Chimes closer than two seconds apart are dropped, since any program can flip its state as
  fast as it writes.

Not yet covered: the shared web and iOS sidebar, the TUI sidebar, the tray, and the status bar
still read Agent panes only. zz ships no terminfo entry, so there is no `Pst` capability;
programs use the query.
