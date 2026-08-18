---
type: Playbook
title: Running the tmux compatibility harness
description: How to run the pinned tmux differential corpus, read topology, geometry, format, and query-stdout results, and record known divergences.
resource: compat/run.sh
tags: [tmux, compatibility, differential-testing, geometry, playbook]
timestamp: 2026-08-16T00:00:00-03:00
---

# Overview

The harness feeds each scenario command to zz and tmux at commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. After each command, it queries both servers
with matching explicit `list-sessions`, `list-windows`, and `list-panes` formats. The
runner compares command exit classes and topology as strict results. It also compares `fmt:` format
queries and generic `out:` command stdout as separate byte-exact strict channels. Geometry
differences fail under `--strict-geometry`, which is how CI runs the harness.

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

FMT and OUT differences fail in both modes. `--strict-geometry` changes only GEO handling.

# Reading results

The runner writes `compat/results/summary.md`. Each row gives the number of executed steps,
TOPO, FMT, and OUT status, plus the number of steps that produced a GEO difference.

Open `compat/results/<scenario>.log` for the command status and per-step unified diffs:

- `COMMAND EXIT-CLASS` compares success with failure. Matching nonzero exits pass because
  both servers refused the command.
- `TOPO` compares session/window counts, names, active indexes, and pane indexes. Any
  difference fails a normal scenario.
- `GEO` compares window and pane cell dimensions plus each window's complete raw
  `#{window_layout}` string, including its checksum and leaf pane ids. Zero-based boot allocation
  now aligns the two sides, so this catches pane assignment permutations as well as structure and
  geometry. The runner reports these differences by default and fails them under
  `--strict-geometry`.
- `FMT` compares stdout from a shared `fmt:` line byte for byte. Both `display-message -p`
  invocations must exit zero. A matching error still fails the FMT step.
- `OUT` compares stdout from any shared query command prefixed with `out:` byte for byte. Both
  commands must exit zero; matching failures still fail the OUT step.

The log captures each step's stdout and stderr. The runner ignores stdout for ordinary command
lines; `fmt:` and `out:` lines enter their respective stdout comparisons.

The runner starts zz on a short `/tmp/zzc-<pid>.sock` path and starts tmux with
`-L zzc-<pid> -f /dev/null`. Its exit trap stops both servers and removes both socket files.

# Adding a scenario

Add a `.txt` file under `compat/scenarios/`. Keep it to 12 commands or fewer. Put one tmux
command on each line; the runner skips blank lines and lines beginning with `#`. Use commands
and flags that both command catalogs support, and target panes by index rather than by raw
`%N` IDs.

The runner handles shell quoting for command lines and rejects `$`, backtick, `;`, `&`, `|`, `<`,
and `>` before parsing them. Prefix a command with `zz-only:` or `tmux-only:` when a scenario needs
side-specific setup. A side-prefixed line skips the exit-class comparison for that step, but the
query trio still runs afterward.

Use `fmt: <format>` for a shared format assertion. The runner passes the payload as one argv value
to `display-message -p` on each side, without `eval`. This path accepts `#{}`, `?`, commas, colons,
semicolons, comparison and logic operators, and `/` delimiters. It rejects an empty payload, `$`,
backticks, either quote character, and `#(`. The `#(` guard prevents a tmux format from starting a
shell command during the differential run.

Use `out: <command...>` for a shared query whose own stdout is the assertion, such as
`out: show-options -gv @plugin`. It uses the same no-eval guards as `fmt:` and splits the payload
into one argv entry per whitespace-delimited token, so quotes, `$`, backticks, and `#(` are rejected.
Put values requiring spaces into an earlier ordinary setup command, then query them by name.

After each line, the harness runs the query trio. Scenario files should contain state changes plus
explicit `fmt:` or `out:` assertions, not ordinary `list-*` assertions whose stdout is ignored.

Traps that produce false divergences:

- Every `new-window` needs `-n <name>`. Default window names are process-derived in tmux —
  and refreshed by the `automatic-rename` timer roughly 500ms later — but index-derived in zz.
  The runner's prologue renames window 0 to `main` on both sides for the same reason.
- Never kill scenario session `w`. The post-step TOPO, GEO, FMT, and OUT probes target `w`, so
  removing it turns every later probe into a fixture failure. Both sides create `w` explicitly;
  there is no auto-created session to remove.
- Never put `#{buffer_full}` in a differential scenario. `display-message -p
  '#{buffer_full}'` crashes the pinned tmux server; this is a verified pin trap, not a zz failure.
- `display-message` gets only tmux's newest automatic paste buffer. A named-only `set-buffer -b`
  setup therefore makes every `buffer_*` value empty on the pin. Add an automatic buffer for a
  `fmt:` probe; use `list-buffers -F` when the named row itself is what needs testing.

## Known divergences

Put a scenario with an accepted strict mismatch under `compat/scenarios/known/`. The runner
still executes every step and writes its diffs, but that scenario does not fail the corpus.
The two current entries pin the deliberate refusals of upstream layout bugs:
`known-main-preset-two-panes.txt` (the pin never sizes the lone "other" pane) and
`known-spread-mixed.txt` (the pin's `-E` corrupts a parent mixing leaf and node children).
Both cite their divergence-matrix rows.

Keep the `known/` set narrow. Move a scenario into the normal corpus when zz closes the gap. The
known-scenario exemption never covers an FMT or OUT difference.

`aggressive-resize.txt` covers stored option readback only. The harness has one short-lived CLI
client per side, so multi-client viewer selection belongs to daemon and convergence tests rather
than this corpus.

# Key files

| File | Role |
| --- | --- |
| `compat/run.sh` | Builds both binaries, selects scenarios, and writes the summary |
| `compat/fetch-tmux.sh` | Acquires and verifies the pinned tmux binary |
| `compat/diff-scenario.sh` | Runs one scenario and emits per-step TOPO, GEO, FMT, and OUT diffs |
| `compat/scenarios/` | Holds the shared and known-divergence corpora |

# Related

- [tmux drop-in plan](/designs/tmux-drop-in.md) . phase ordering and compatibility target
- [tmux divergence matrix](/tmux/divergences.md) . gaps the harness can turn into fixtures
- [updating the tmux reference](/playbooks/updating-tmux-reference.md) . how to move the pin
