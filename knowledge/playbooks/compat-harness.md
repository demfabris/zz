---
type: Playbook
title: Running the tmux compatibility harness
description: How to run the pinned tmux differential corpus, read topology and geometry results, and record known divergences.
resource: compat/run.sh
tags: [tmux, compatibility, differential-testing, geometry, playbook]
timestamp: 2026-08-16T00:00:00-03:00
---

# Overview

The harness feeds each scenario command to zz and tmux at commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. After each command, it queries both servers
with matching explicit `list-sessions`, `list-windows`, and `list-panes` formats. The
runner compares command exit classes and topology as strict results. It records geometry
separately so phase 3 can use the differences as steering data.

`compat/run.sh` builds `target/debug/zz` with your normal environment before the scenario
runner creates its scratch `HOME` and `XDG_CONFIG_HOME`. The tmux fetcher clones and builds
the pin under `compat/.cache/`, then checks for `tmux next-3.8`. Set
`ZZ_COMPAT_TMUX=/path/to/tmux` to use a prebuilt binary with that version.

# Running the corpus

Run the full corpus from the repository root:

```sh
compat/run.sh
```

Pass scenario names to run a subset. Names may include or omit `.txt`.

```sh
compat/run.sh windows panes
compat/run.sh known/known-geometry-gap.txt
```

Geometry differences do not change the default exit status. Use strict mode when you want
them to fail the run:

```sh
compat/run.sh --strict-geometry
```

Strict mode is the CI contract: the Linux workflow leg runs `compat/run.sh
--strict-geometry`, so every scenario outside `known/` must stay TOPO-clean and GEO-clean
against the pin. Since the cell-authoritative layout landed, a headless zz window is born
at tmux's 80x24 and every layout operation runs the pin's integer arithmetic, which is what
makes exact-geometry diffing possible.

# Reading results

The runner writes `compat/results/summary.md`. Each row gives the number of executed steps,
whether the scenario stayed TOPO-clean, and how many steps produced a GEO difference.

Open `compat/results/<scenario>.log` for the command status and per-step unified diffs:

- `COMMAND EXIT-CLASS` compares success with failure. Matching nonzero exits pass because
  both servers refused the command.
- `TOPO` compares session/window counts, names, active indexes, and pane indexes. Any
  difference fails a normal scenario.
- `GEO` compares window and pane cell dimensions plus each window's `#{window_layout}`
  string, normalized to its structural body: the checksum and the leaf pane numbers are
  stripped before diffing, because pane ids are opaque values both parsers ignore and the
  zz daemon's auto-session shifts id allocation by one (see the divergence matrix). Stripping
  leaf ids limits GEO to bracket structure and geometry. TOPO and GEO both miss a pane assignment
  permutation among equal-sized panes; the harness accepts that blind spot. The runner reports
  these differences by default and fails them under `--strict-geometry`.

The log also captures each step's stdout and stderr for debugging, but the comparison covers
only the exit class and the TOPO/GEO snapshots; command output itself is never diffed.

The runner starts zz on a short `/tmp/zzc-<pid>.sock` path and starts tmux with
`-L zzc-<pid> -f /dev/null`. Its exit trap stops both servers and removes both socket files.

# Adding a scenario

Add a `.txt` file under `compat/scenarios/`. Keep it to 12 commands or fewer. Put one tmux
command on each line; the runner skips blank lines and lines beginning with `#`. Use commands
and flags that both command catalogs support, and target panes by index rather than by raw
`%N` IDs.

The runner handles simple shell quoting when it turns each line into an argument array, and
rejects any line containing a shell metacharacter (`$`, backtick, `;`, `&`, `|`, `<`, `>`)
before parsing it. Prefix a command with `zz-only:` or `tmux-only:` only when a scenario needs
side-specific setup. The shared corpus should use the same command on both sides. A
side-prefixed line skips the exit-class comparison for that step, but the query trio still
runs afterward — both servers must converge to the same topology by the end of the step.

After each line, the harness runs the query trio. Scenario files should contain state changes,
not their own `list-*` assertions.

Traps that produce false divergences:

- Every `new-window` needs `-n <name>`. Default window names are process-derived in tmux —
  and refreshed by the `automatic-rename` timer roughly 500ms later — but index-derived in zz.
  The runner's prologue renames window 0 to `main` on both sides for the same reason.
- Never kill the last remaining session. tmux's server exits (`exit-empty`), while the zz CLI
  respawns a fresh daemon with a new session `0`, so every later step diverges. The prologue
  already creates session `w` before removing the auto-created session.

## Known divergences

Put a scenario with an accepted strict mismatch under `compat/scenarios/known/`. The runner
still executes every step and writes its diffs, but that scenario does not fail the corpus.
`known/known-geometry-gap.txt` records the current cell-resize gap: tmux accepts
`resize-pane -x 30`, while headless zz rejects a cell size without measured geometry.

Keep the `known/` set narrow. Move a scenario into the normal corpus when zz closes the gap.

# Geometry and phase 3

A headless zz daemon has no client-supplied cell measurements, so its
`window_width`, `window_height`, `pane_width`, and `pane_height` formats expand to empty
strings. A detached tmux session keeps its 80x24 creation geometry. GEO diffs therefore give
you a report rather than a default failure.

[Phase 3 of the tmux drop-in plan](/designs/tmux-drop-in.md#phase-3--cell-authoritative-layout-23-weeks)
makes cells authoritative in `zz-mux`. Run the corpus with `--strict-geometry` while doing
that work, then use each GEO hunk to drive the layout toward tmux.

# Key files

| File | Role |
| --- | --- |
| `compat/run.sh` | Builds both binaries, selects scenarios, and writes the summary |
| `compat/fetch-tmux.sh` | Acquires and verifies the pinned tmux binary |
| `compat/diff-scenario.sh` | Runs one scenario and emits per-step TOPO/GEO diffs |
| `compat/scenarios/` | Holds the shared and known-divergence corpora |

# Related

- [tmux drop-in plan](/designs/tmux-drop-in.md) . phase ordering and compatibility target
- [tmux divergence matrix](/tmux/divergences.md) . gaps the harness can turn into fixtures
- [updating the tmux reference](/playbooks/updating-tmux-reference.md) . how to move the pin
