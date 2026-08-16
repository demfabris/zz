---
type: Design Plan
title: tmux superset roadmap
description: The settled plan for taking zz from tmux-shaped to honest-subset-plus-superset - the compat audit verdict, the tier ladder, the host axis, and the doctrine that decides what never gets built.
status: In Progress
tags:
- tmux
- compatibility
- roadmap
- cli
- fleet
timestamp: 2026-08-09T00:00:00Z
---

# Overview

A full two-track audit (2026-08-08) of zz against tmux concluded: **muscle memory is
already ~drop-in, scripting was structurally impossible.** The interactive model —
prefix table, splits, layouts, copy mode, choose-tree, detach/reattach — matches tmux
closely enough that a tmux user lands without noticing. The scriptable surface had
three independent disqualifiers: nothing machine-readable ever came out (no `-F`, no
`display-message -p`), no terminal-attaching client, and no `$TMUX_PANE` equivalent,
so untargeted CLI commands acted on the first session regardless of where they were
typed. This plan closes the gaps worth closing and names the ones that stay open on
purpose. Builds on the [tmux compatibility philosophy](/tmux/tmux-compat.md).

# Doctrine

**zz = tmux, exactly, per daemon — plus orthogonal axes tmux doesn't have.**

tmux's syntax stays frozen at tmux's meaning. Every tmux line a user pastes must mean
what tmux meant, or error — never a third thing. zz-only power arrives as new verbs
(`split-browser`, `split-picker`, `fleet`), new pane kinds, and new selector dimensions —
never by overloading tmux grammar. Two decisions this doctrine already made:

- Pane-kind flags on `split-window` are impossible (`-t`/`-b`/`-e` are taken by
  tmux); kind lives in the verb name (`split-browser` precedent).
- The host axis never enters `-t` (`a:b` would be ambiguous host:session vs
  session:window). Host is a *server selector*: `zz --host <name>` is the
  cross-machine generalization of `tmux -L`, resolved through the fleet registry to
  an ssh endpoint. The `-t` that follows stays pure tmux.

# Tier 1 — correctness and muscle memory (landed 2026-08-09, this tree)

All engine-tested and smoke-verified against a live daemon at land time.

| Item | Where |
| --- | --- |
| `%if` blocks skipped with a diagnostic instead of executing both branches | `zz-mux/src/parser.rs` `skip_conditional_blocks` |
| `prefix ]`, `0-9`, `l` bound; `last-window` command; `Session.last_window` slot; kill-window falls back to last window | `zz-protocol/src/key.rs` defaults, `zz-mux/src/model.rs` `activate_window`/`forget_window` |
| Cell-based `resize-pane` (was ratio: `-R 5` moved the split 25%) | `command.rs` `window_cell_extent`, `model.rs` `pane_axis_fraction`; daemon feeds geometry on `ResizeTerminal` |
| `split-window`/`new-window`/`new-session [shell-command]` (was silently discarded) | `MuxEffect::PaneCreated.command` → `TerminalSpawn` (`sh -c`) |
| `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` in terminal panes (tmux's `$TMUX` contract) | `zz-terminal` `run_terminal`; daemon builds the env |
| CLI self-targeting: untargeted commands act on the invoking pane | `ClientHello.origin` (protocol v47), daemon resolves connection context from it |
| `reload-config` resets key tables then re-sources; `source-file` keeps tmux accumulate semantics | daemon `reload_user_config_with_mux_file` |
| Import/reload/source report what they skipped instead of silence | daemon `ConfigLoadReport` → ClientMessage |

Held back deliberately: `prefix d` — detach needs a GUI story first (unbound prefix
keys are already swallowed, so it is a safe noop today; leading candidate is
hide-to-tray, which the close button already does).

# Tier 2 — the scriptability layer (landed 2026-08-09, verified same day)

Formats with explicit row context (`-F` on the three list commands, engine in
`zz-mux/src/status.rs`), `display-message` (`-p` prints; flagless surfaces as a
ClientMessage toast via a new `MuxEffect::DisplayMessage`), `has-session`, compound
`session:window.pane` target resolution, `\;` chains in `bind-key`
(`Binding.commands` was always a Vec; only the parse side was missing),
`capture-pane -b`, CLI `--host <name>` (new `CommandClient::connect_endpoint`
mirroring the Interactive client's `Endpoint::Ssh` arm — `SshForward` → local
forward), and `fleet list -F`. No protocol change; version stays 47. `--host`
suppresses the `$ZZ_PANE` origin because pane ids do not cross daemons.

The composed payoff, impossible in tmux because its server is machine-local:

```
for h in $(zz fleet list -F '#{host_name}'); do
  zz --host $h list-sessions -F "$h:#{session_name}"
done | fzf
```

# Tier 3 — the TUI presentation backend

`zz attach` landed 2026-08-09 as the TUI presentation backend
([tui-client](/designs/tui-client.md) rungs 1+3). `--host attach` is still
refused. Remaining TUI work is that design's open rungs (agent panes), not a
missing verb. Shares its client seams with the [iOS client](/designs/ios-client.md).

# Never (amended 2026-08-16 by the drop-in plan)

Most of the original never-list was unwound by the [tmux drop-in plan](/designs/tmux-drop-in.md):
the exec family returns behind a consent gate, control mode becomes the differential-testing
harness, layout strings ride the cell-authoritative layout rework, and `-L`/the options sprawl
join the grind. What survives as never:

- Linked windows and session groups — one window belongs to one session; `new-session -t`
  stays a loud rejection (drop-in plan, decision 3). Named consequences: `break-pane` on a
  single-pane window keeps refusing (tmux relinks the window — linked-window machinery), and
  tmux-resurrect's grouped-session restores error loudly.
- Speaking tmux's private client-server socket protocol — the alias model (`alias tmux=zz`)
  makes real-tmux-binary interop a non-goal (drop-in plan, decision 4).
- Fleet broadcast (`--all`) — composition over features: a shell loop over
  `fleet list -F` is the unix answer. The one conceivable exception is a read-only
  `fleet status` reachability probe, which is not composable from outside.

# Decision log

- 2026-08-08: resize-cells approved knowing it changes GUI keyboard-resize feel
  (`M-arrows` were 25% per press, become 5 cells). `prefix d` deferred. Split-verb
  question dissolved — `split-window` was already terminal-pure; `new-pane` *is* the
  picker verb. Reload semantics split: `reload-config` = state-matches-file,
  `source-file` = tmux accumulate.
- 2026-08-09: superset doctrine settled; host-as-server-selector settled; TUI
  backend adopted as the Tier 3 shape. Tier 2 landed and was verified against a
  live daemon the same day (all suites green; the real ssh `--host` hop remains
  unsmoked pending a machine with fleet hosts configured). `--host` guards:
  refuses `daemon`/`proxy`/`fleet`/`attach` and conflicts with `--socket`.
- 2026-08-16: the drop-in pivot — goal upgraded from honest-subset-plus-superset
  to `alias tmux=zz`. The never-list shrank to the three items above; everything
  else moved into the [tmux drop-in plan](/designs/tmux-drop-in.md). Revised the
  same day after an adversarial review (claims verified against the tree): the
  differential harness moved off control mode onto `list-* -F`, control mode
  became a stdio front-end phase (how iTerm2 actually consumes `-CC`), consent
  narrowed to the import flow (own config is trusted — `#()` already executes
  ungated), and the TTY attach contract became a gated phase 8.

# Related

- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the subset contract this
  roadmap extends.
- [commands](/tmux/commands.md), [key tables](/tmux/key-tables.md),
  [conf parser](/tmux/conf-parser.md), [status line](/tmux/status-line.md).
- [Fleet attach](/designs/fleet-attach.md) — the host tier `--host` rides on.
- [TUI client](/designs/tui-client.md) — Tier 3.
