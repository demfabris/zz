# Terminal IO throughput benchmark

Measures how fast a terminal consumes real PTY output. Everything is timed
inside the terminal under test: `run.sh` opens each terminal and has it run
`inner.sh`, which appends one JSON object per test to `results/results.jsonl`
and drops `results/<label>.done` when finished. `summarize.sh` renders the
JSONL as `results/summary.md`; the last record wins per (label, test) pair, so
re-running one terminal updates the table in place.

## Tests

| test | what runs | metric |
| --- | --- | --- |
| `cat-ascii` | `/bin/cat` of a 150 MiB ASCII fixture | median wall time → MB/s |
| `cat-unicode` | `/bin/cat` of a 150 MiB mixed-UTF-8 fixture | median wall time → MB/s |
| `doom-fire` | DOOM-fire-zig for a fixed duration | frames per second |

Timing uses hyperfine (per-run times from its JSON export) when hyperfine and
jq are present, else a `/usr/bin/time` loop; the `timer` column in the summary
records which.

## Running

```sh
bench/gen-fixtures.sh    # build ghostty-gen + DOOM-fire, materialise fixtures
bench/run.sh             # every detected terminal
bench/run.sh --terminals zz,ghostty+tmux --fresh --runs 7
```

Known terminals: `zz`, `ghostty`, `ghostty+tmux`, `ghostty-tip` (staged
nightly, see the comment atop `run.sh`), `kitty`, `alacritty`. Anything else —
or a zz build `run.sh` can't find — goes through manual mode: open the
terminal yourself, make it frontmost, and paste the `bash bench/inner.sh
<label>` line `run.sh` prints. To automate a new terminal, add a case to the
dispatch at the bottom of `run.sh` with a `run_<name>` function that opens the
terminal running `inner.sh <label>` and waits on `results/<label>.done`.

## Before trusting a number

- **Release bundles only.** zz must be a release bundle — `run.sh` looks under
  `dist/zz` and `dist/zz-profile` (`cargo xtask bundle-cef --release`, or
  `just profile-build mac`). A dev-profile build measures the build profile,
  not the terminal: unoptimized draw code and an unoptimized VT engine each
  cost multiples of the real number.
- **Same grid or no comparison.** Throughput scales with the grid the terminal
  had when the test ran; the summary records a `grid` column and rows with
  different grids are not comparable. `inner.sh` waits for the pty size to
  settle and requires two agreeing reads before it trusts the grid.
- **The producer is pinned to `/bin/cat`.** Never benchmark by typing `cat` in
  an interactive shell: an alias like `cat='bat'` measures the alias, and
  GNU-coreutils PATHs shadow `cat` with `gcat`.
- **Throughput is partly batching policy.** A terminal that batches frames
  aggressively while draining its pty posts higher MB/s at identical parse
  speed; treat cross-terminal deltas as drain-pipeline numbers, not pure
  parser numbers.
- **Fixture identity.** The UTF-8 fixture is seed-pinned and byte-identical
  everywhere; ghostty-gen's `+ascii` mode has no seed upstream, so the ASCII
  fixture is time-seeded — self-consistent per machine via its recorded
  sha256, but not byte-equal across machines.

## Out of scope

`gen-fixtures.sh --extra` also emits `+osc` and `+kitty` fixtures. No test
drives them yet; they exist so an escape-sequence-heavy or graphics-heavy test
can be added without touching the generator.

## Daemon gate

`bench/perf/` is a separate benchmark: the zz daemon against a release tmux,
both headless and isolated, covering CLI round trips, spawn, cold start,
config replay, chatty panes, idle cost, memory, attach, echo latency,
throughput, control mode and status jobs. `just perf-gate [stage]` runs it;
see `bench/perf/README.md`.

# GPUI frame cost (macOS)

`bench/gpui/frames.py` compares zz app bundles frame by frame. Each launch gets its own `HOME`
and socket, a 1280x900 window, and five scenarios driven through the CLI (idle, stream, a
nine-pane grid with one pane streaming, copy-mode scrolling, and 25 sessions with the palette
open). It records main-thread CPU, footprint, and per-phase frame costs from `GPUI_FRAME_STATS`,
which needs a fork with the frame recorder (`cc9e4d1804` or later).

```sh
bench/gpui/frames.py --app A=dist/a/zz.app --app B=dist/b/zz.app --out /tmp/frames --repeats 3
bench/gpui/frames.py summarize /tmp/frames/results.jsonl A
```

Build each side with `cargo xtask bundle-cef --profile profiling --output dist/<name>`.
`--app NAME=PATH,KEY=VALUE` adds environment for one side. It waits for a quiet machine before each
window and repeats windows that overlapped a compile, a perf gate or a compat run; set
`ZZ_FRAMES_FOREIGN` to colon-separated target directories of other worktrees whose test binaries
should also count, and `ZZ_FRAMES_HOLD` to a file it creates while measuring.
