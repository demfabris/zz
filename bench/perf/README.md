# Daemon gate: zz vs tmux

`bench/perf/` measures the zz daemon against a release tmux in the same run, on
the same machine, with both servers isolated. It is the gate for the daemon
performance rebuild: every lane runs it, and every claim is a number from it.

It is Python 3 stdlib plus ctypes. It needs a built `zz_cli` and a release
tmux, nothing else.

## Running

```sh
just perf-gate                      # build zz_cli, stage baseline, full run
just perf-gate wave1 --quick        # lane loop, about 3 minutes
just perf-gate wave1 --strict --baseline bench/perf/results/<previous merge>.json
python3 bench/perf/run.py --only cli,attach --quick
python3 bench/perf/run.py --targets # the Targets table of the design doc, from thresholds.json
python3 bench/perf/test_gate.py     # threshold, noise policy and tmux choice tests
```

`just perf-gate` runs `cargo build --release -p zz-cli` first, so the binary
matches the tree. `run.py` alone does not build; it records the binary's
sha256 and mtime in `meta` and warns when the binary is older than the HEAD
commit.

Flags of `run.py`:

| flag | meaning |
| --- | --- |
| `--zz PATH` | zz binary, default `target/release/zz_cli` |
| `--headless PATH` | headless frame-sink client for `throughput.headless.*`, default `target/release/examples/perf_client` (`cargo build --release -p zz-client --example perf_client`); the row is skipped with a note when it is missing |
| `--tmux PATH` | tmux binary; default: see [tmux](#tmux) |
| `--stage S` | `baseline`, `wave1`, `wave2`, `wave3` or `final`; picks the thresholds |
| `--json PATH` | output, default `bench/perf/results/local/<stage>-<host>-<sha>-<time>.json` |
| `--baseline PATH` | the previous merge's JSON; adds `vs_baseline` and the regression check |
| `--w0 PATH` | the fixed wave 0 JSON; adds `vs_w0`, the drift check and the `vs_w0` rules. Default `results/baseline-<host>-*.json` (`baseline-quick-<host>-*.json` with `--quick`) when exactly one exists; `none` turns it off |
| `--quick` | fewer runs and fewer variants (see below) |
| `--only a,b` | run only these groups |
| `--strict` | wall clock misses fail even on a loaded host |
| `--keep` | keep the temp root (logs, configs) for inspection |
| `--rescore JSON` | measure nothing; re-evaluate an earlier result at `--stage` (with `--baseline`, `--w0`, `--strict`), write it to `--json` if given |

The servers get a scrubbed environment, except that every `ZZ_PERF_*` rollback
knob set in the caller's environment is passed to both muxes (tmux ignores
them) and listed in `meta.knobs`, so `ZZ_PERF_ATTACH_DEDUP=0 python3
bench/perf/run.py ...` measures the knob-off path on the same binary.
| `--targets` | measure nothing; print the gated metrics with their W0 values and per-stage rules as a Markdown table |

The table goes to stdout, the JSON to `--json`. Exit status is 0 when nothing
failed, 1 when a hard threshold failed or a group errored, 130 when
interrupted.

Results you want to keep go directly in `bench/perf/results/` as
`<label>-<host>-<sha8>.json` and are committed. `results/local/` is ignored.

| label | what |
| --- | --- |
| `baseline`, `baseline-quick` | wave 0, full and `--quick`: the fixed W0 reference |
| `w<wave>-<n>-<lane>` | a lane's merge run, for example `w1-2-publish`; the next lane's `--baseline` |
| `wave1` ... `final` | a wave exit run at that stage |

A `--quick` run compares against the quick W0 and a full run against the full
one; when the kinds differ the run prints a warning, because the sample counts
differ.

## tmux

The gate compares against a release tmux (Homebrew 3.7c on the reference Mac).
Without `--tmux` it tries `/opt/homebrew/bin/tmux`, `/usr/local/bin/tmux`,
`/usr/bin/tmux`, then the first `tmux` on PATH, and takes the first that
passes these checks, after resolving symlinks:

- it is a Mach-O or ELF executable, not a script;
- it is not the zz binary, and `-V` starts with `tmux ` and has no `-zz`;
- it does not link AddressSanitizer (`otool -L` / `ldd`) and has no
  `__asan_init` symbol (`nm`).

The PATH is checked last because inside a zz pane the daemon puts its tmux
wrapper first on PATH; without these checks the gate would compare zz with
itself and pass every ratio. The ASan check rules out the pinned compat build
in `compat/.cache` (an ASan debug build of tmux next-3.8 that is several times
slower and larger). The JSON records the resolved tmux path, `tmux -V` and its
linked libraries.

## Isolation

Each run makes a fresh root under `/tmp/zzpf-<pid>-*` and builds the
environment from scratch: `HOME`, every `XDG_*`, `TMPDIR`, `ZZ_DATA_DIR` and
`ZZ_LOG_DIR` point inside it; `ZZ_SOCKET=/tmp/zzpf-<pid>.sock`, `ZZ_TRAY=0`,
`ZZ_UPDATE_CHECK=0`, `SHELL=/bin/bash`, `TERM=xterm-256color`. `TMUX`,
`TMUX_PANE`, `ZZ_PANE`, `ZZ_SESSION`, `TERM_PROGRAM` and `COLORTERM` are not
passed. The empty `HOME` means bash reads no rc files. tmux runs as
`tmux -L zzpf-<pid> -f <root>/perf.conf` with `TMUX_TMPDIR` in the root, and
zz as `zz_cli -f <root>/perf.conf`.

`perf.conf` holds one line, `set -g history-limit 10000`, so both servers keep
the same scrollback from the first pane on (zz defaults to 10000, tmux to
2000). Each group checks `show-options -gv history-limit` on both servers
after its first session; a mismatch is a run error. The values land in
`meta.group_history_limit`.

Both servers are killed before and after every group, and again on exit,
including on an exception, SIGINT, SIGTERM or SIGHUP. Each kill first records
every descendant of the server (panes and whatever they started), and after
the server is gone SIGKILLs any descendant still alive; their count is
`meta.orphans_killed`. After cleanup the gate looks for processes whose
command line carries the run's tag (the zz daemon's socket path, the tmux
`-L` label, the timer panes) and for tracked descendants that survived
SIGKILL; any of them is recorded in `meta.stray_processes_after` and fails
the run.

## Probes

`probe.py` reads the server process from outside.

| probe | macOS | Linux |
| --- | --- | --- |
| CPU | `proc_pid_rusage(RUSAGE_INFO_V4)` user + system, mach timebase to ns | `clock_gettime` on `clock_getcpuclockid(pid)` (ns, includes exited threads); else the sum of `/proc/<pid>/task/*/schedstat`; else `stat` ticks |
| instructions | `ri_instructions` | `perf_event_open` `PERF_COUNT_HW_INSTRUCTIONS`, user space only, one counter per thread seen at the first sample with `inherit` + `inherit_thread`, so later threads fold into their creator's counter and forked children are not counted; none when the syscall fails (`meta.instructions_source` says why) |
| footprint | `ri_phys_footprint` | `smaps_rollup` Pss_Anon + Pss_Shmem + SwapPss |
| RSS | `ri_resident_size` | `VmRSS` |
| threads | `PROC_PIDTASKINFO` `pti_threadnum` | `Threads` in status |
| wakeups | `ri_interrupt_wkups + ri_pkg_idle_wkups` | voluntary + nonvoluntary switches summed over tasks |
| thread ids | `PROC_PIDLISTTHREADIDS` | `/proc/<pid>/task` |

Per-command daemon CPU is the server's CPU delta over N commands divided by N.
The server pid comes from `#{pid}`. zz runs all panes from one daemon process,
so the daemon's own CPU is the whole cost; job children are reported
separately. `meta.cpu_source` names the Linux CPU source used. Every CPU
metric has an instruction twin (`cli.instr.*`, `spawn.instr.*`,
`config.instr.*`, `chatty.instr_per_s.*`, `idle.instr_per_s.*`,
`attach.instr.*`, `control.instr_per_cmd`, `statusjob.instr_per_s`).

Socket bytes, frames (u32 little-endian length prefix) and connections come
from `sockproxy.py`, an in-process Unix socket relay the zz client dials
through `ZZ_SOCKET`. tmux passes the client tty over its socket, so it cannot
be relayed; these metrics are zz only.

## Noise policy

- zz and tmux alternate: per command in `cli`, per iteration elsewhere
  (A B, B A, ...), or measure over the same window when both servers run side
  by side (`chatty`, `idle`, `mem`, `statusjob`), so load drift hits both.
- `meta.loadavg_start` and `loadavg_end` are recorded. When the 1-minute load
  divided by the CPU count passes 0.5 at either end, the run is noisy.
- Hard: kinds `cpu`, `bytes`, `mem` (footprint), `threads`, `count` and
  `throughput`. A miss fails.
- Wall (`wall`: CLI wall time, spawn, cold start, config wall, attach time to
  first content, echo latency, control latency and rates): a miss fails on a
  quiet host and warns on a noisy one; `--strict` makes it fail on a noisy
  host too. Merge runs use `--strict`; if one fails on wall clock only while
  the host is loaded, run it again when the host is quiet.
- Throughput is a wall clock number and moves 10% between runs of one binary
  (215, 230 and 241 MB/s at load under 7). It is gated on the ratio to tmux in
  the same run, which cancels load, and on a floor of 0.85x the fixed W0 run,
  never on the previous merge.
- Informational: `rss`, `instr` (see regressions), `bytes_info`, `cpu_info`,
  any metric under `report_only` in `thresholds.json`, and any metric with no
  rule for the stage.
- Every metric records median, p10, p90, p99, min, max and n. Rules compare
  the median. Single measurements (a CPU delta over N commands, a footprint
  sample) have n = 1.

### Regressions

A lane must not make a metric it does not own worse. Each metric is compared
with two references: `--baseline` (the previous merge, `regressed`) and W0
(`drifted`, which catches six lanes each adding 4%). The `tolerance` block of
`thresholds.json` sets, per kind, a factor, an absolute floor in the metric's
unit (a move smaller than the floor never counts, so 0.003% idle CPU cannot
fail) and whether a miss fails the run:

| kind | factor | floor | fails |
| --- | --- | --- | --- |
| `instr` | 1.05 | 0.05 Minstr | yes |
| `bytes` | 1.05 | 256 B | yes |
| `count` | 1.05 | 1 | yes |
| `mem` | 1.10 | 0.5 MiB | yes |
| `threads` | 1.10 | 1 | yes |
| `cpu` | 1.25 | 0.05 | no, a note |
| `wall` | 1.25 | 0.5 ms | no, a note |
| `throughput` | 1.15 | - | no, a note (the W0 floor fails instead) |

CPU time is a note only: between two runs of one binary `cli.cpu.*` moved up
to 14% and `chatty.cpu_pct.flip` 61%, while `cli.instr.*` moved under 0.5%.
The 5% rule is held on instructions. A metric entry can carry its own
`tolerance`; the window metrics whose work follows the output rate
(`chatty.instr_per_s.*`, `idle.instr_per_s.p20`, `statusjob.instr_per_s`,
`chatty.tty_kibps.*`, `statusjob.tty_kibps`, `echo.wire_bytes.busy30`) have
wider ones. Report-only metrics never fail on a regression.

Linux counts user-space instructions only (`perf_event_paranoid` 2 allows no
more), so Linux and macOS instruction numbers are not comparable; ratios to
tmux and runs on the same host are. A host whose `perf_event_open` fails
(paranoid 3 or higher, a VM with no PMU) records no `instr` metrics, and the
5% rule has nothing to hold there.

## Thresholds

`thresholds.json` maps metric ids (exact or glob; exact wins, then the longest
pattern) to per-stage rules. An exact entry replaces every matching glob, so
it carries its own later stages. A stage uses the latest rule at or before
it, so `wave2` inherits `wave1` unless it has its own. A rule holds any of
`abs` (bound on zz), `ratio` (zz <= ratio x tmux + `slack`), `plus` (zz <=
tmux + plus) and `vs_w0` (zz <= factor x the W0 zz); all must hold unless
`combine` is `any`. For `better: higher` metrics the comparisons flip. A rule
with `ceiling` (the throughput rules) also passes when zz is within that
fraction of the host's pty ceiling, `throughput.ceiling.*` in the same run: a
bare Python reader of the same `cat` through a cooked 180x50 pty. It exists
because a Linux cooked tty caps any reader near 135 MB/s, below 4x tmux.

`reference` names the host the absolute rules were set on (the macOS W0). On
any other host, `abs` rules of kind `cpu` and `wall` keep their multiple of the
reference W0's tmux median (`abs@ratio`: bound = abs x tmux here / tmux there)
and `mem` rules keep their margin over it (`abs@plus`: bound = abs - tmux there
+ tmux here); other kinds, and every `ratio`, `plus` and `vs_w0` rule, are the
same everywhere. `meta.reference` records the file used.

The baseline stage records only, except the throughput rules (>= 4x tmux or
>= 0.85x the pty ceiling, and >= 0.85x W0 detached; attached <= 0.25x tmux or
the ceiling's time / 0.85, and <= 1.18x W0). `report_only`
lists the metrics that never gate: `cli.wall.version.p1` (process spawn
floor), `cold.infocmp_forks`, `attach.wire_frames.*`, `attach.wire_c2s.*`,
`echo.wire_bytes.busy30` (includes the ticker) and `statusjob.tty_kibps`.
`test_gate.py` fails when a hard or wall metric in the committed baseline has
no rule at `final` and is not on that list.

The design doc's Targets table is `run.py --targets`; change the numbers here
and regenerate the table, not the other way round.

## Groups and metric ids

Ids are stable. `<size>` is `p1` (one pane), `p20` (one session, 20 windows)
or `s20` (20 sessions).

| group | ids | what it does |
| --- | --- | --- |
| cli | `cli.wall.<verb>.<size>`, `cli.cpu.<verb>.<size>`, `cli.instr.<verb>.<size>` | posix_spawn of one CLI command, alternating zz and tmux; wall per run, daemon CPU and instructions per command. Verbs: `version` (process floor, wall only), `display` (`display-message -p '#{pane_id}'`), `list_panes`, `show_options` (`-gqv status`), `has_session`, `send_keys` (`''`), `select_pane`, `list_keys`, `chain5` (five commands joined by `;`) at p1 and p20; `list_panes_a`, `list_windows_a`, `list_sessions` at s20 |
| spawn | `spawn.wall.<v>`, `spawn.cpu.<v>`, `spawn.instr.<v>` | `split_shell` (`split-window -d`), `split_empty_P` (`split-window -d -P -F '#{pane_id}' ''`), `new_window` (`new-window -d`), `kill_pane`. CPU is the delta across the command plus 100 ms, so it includes the new shell's first output |
| cold | `cold.wall.new_session`, `cold.wall.new_session_noterm`, `cold.infocmp_forks` | `new-session -d` with no server running, with and without `TERM`. `infocmp_forks` counts infocmp runs during a cold start (a PATH wrapper); zz forks `infocmp -x -1 $TERM` on the first client hello of each TERM, synchronously on that connection, so it sits on the cold path |
| config | `config.wall.source_1000`, `config.cpu.source_1000`, `config.instr.source_1000` | `source-file` of a generated 1000-line config: 40% `set -g @plugin_opt_N`, 30% `bind-key -T <table>`, 30% real options (status-left/right, styles, history-limit, window-status formats) |
| chatty | `chatty.cpu_pct.<v>`, `chatty.instr_per_s.<v>`, `chatty.tty_kibps.<v>`, `chatty.client_cpu_pct.<v>` | 10 windows printing about 100 lines/s. `steady`: a Python printer, detached. `flip`: a shell loop typed at the prompt (`echo; sleep 0.01`, so the foreground flips between bash and sleep), detached. `hidden`: the flip load in 10 windows with a TUI attached to idle window 0. `visible`: 4 tiled panes running the flip loop in the attached window. Server CPU %, TUI tty KiB/s and TUI client CPU % over 10 s |
| idle | `idle.cpu_pct.p20`, `idle.instr_per_s.p20`, `idle.wakeups_per_s.p20` | 20 idle shells, no client, 10 s |
| mem | `mem.footprint.<p>`, `mem.rss.<p>`, `mem.threads.<p>` | `p1`, `p20` (idle shells), `tui20` (p20 with a TUI attached), `scroll180` and `scroll80` (20 panes with history-limit 10000 filled by `seq 1 12000`, sampled 5 s after the fill) |
| attach | `attach.ttfc.<p>`, `attach.tty_total.<p>`, `attach.tty_bytes.<p>`, `attach.cpu.<p>`, `attach.instr.<p>`, `attach.conns.<p>`, `attach.wire_s2c.<p>`, `attach.wire_frames.<p>`, `attach.wire_c2s.<p>` | TUI attach in a 180x50 pty. `p1`: a pane showing a marker; `p4`: four tiled panes with markers. Time from fork to all markers on the tty; `tty_total` is every tty byte until the output has been quiet for 200 ms (gated); `tty_bytes` is the bytes up to the last marker (info, it depends on draw order); daemon CPU per attach+detach. Through the proxy (zz only): connections, bytes and frames from spawn to 1 s after content |
| echo | `echo.p50.<v>`, `echo.p99.<v>`, `echo.wire_bytes.<v>` | keystroke written to the outer pty until its echo comes back, through an attached TUI, with a raw-mode echo program in the pane and status off. The pane answers key N with `§` and N as three digits, and the gate looks for that token in the tty bytes with escape sequences removed, so an SGR `m` or a redraw split across a scroll cannot pass for the echo. `idle`, and `busy30` where the same pane prints a line 30 times a second. Keys are 20 to 50 ms apart. Wire bytes per keystroke through the proxy (zz only) |
| throughput | `throughput.detached.ascii`, `throughput.detached.unicode`, `throughput.attached.ascii_ms`, `throughput.attached.tty_bytes`, `throughput.headless.ascii_ms`, `throughput.ceiling.ascii`, `throughput.ceiling.unicode`, `throughput.ceiling.ascii_ms` | `/bin/cat` of a 150 MiB seeded fixture in a detached 180x50 pane, timed inside the pane; MB/s. The attached run shows the same file in the window a TUI is looking at. The headless run (zz only, info) shows it to `perf_client`, which decodes and applies every frame with the zz-client core and renders nothing, so it measures the daemon's frames and their decoding without the TUI's painting. The ceiling rows are a bare reader of the same `cat` through a cooked 180x50 pty, once per run before the muxes (its rate sits in the `zz` column); `ascii_ms` is that rate as the time to read the file |
| control | `control.latency`, `control.cpu_per_cmd`, `control.instr_per_cmd`, `control.burst_cmds_per_s`, `control.output_mbps`, `control.output_bytes_per_byte` | `-C attach` over pipes: one `display-message -p x` at a time until `%end`, then 200 at once; `%output` throughput for a 16 MiB cat from the first `%output` line to a done marker (pane spawn time is not counted), and control bytes per pane byte |
| statusjob | `statusjob.cpu_pct`, `statusjob.instr_per_s`, `statusjob.threads_per_s`, `statusjob.child_cpu_pct`, `statusjob.tty_kibps` | three `#()` jobs in status-right at status-interval 1 with a TUI attached. Threads per second counts new thread ids seen by a 2 ms sampler, a lower bound for short-lived threads |

`--quick` runs: cli at 20 runs per verb, spawn at 4, cold at 4, config at 3,
chatty `flip` and `hidden` over 4 s, idle over 5 s, mem `p1` and `p20`,
attach at 4, echo at 100 keys, throughput ASCII detached once, control at 50
commands, statusjob over 5 s.

Fixtures live in `bench/.cache/perf/` and are generated on first use from a
fixed seed (ASCII: random printable lines of 1 to 170 columns; unicode: Latin,
CJK, emoji, combining marks and Cyrillic).

## Not built yet

These are known gaps, not silent omissions:

- TODO: a headless GUI-like client (zz-client core or the FFI native fixture)
  that subscribes like the desktop app: all sessions, history backfill, 8
  visible panes. Gate daemon CPU, attach CPU, HistoryChunk bytes and RSS with
  it attached.
- Built (W2-TERM): the frame-decoding throughput client
  (`crates/zz-client/examples/perf_client.rs`, `throughput.headless.ascii_ms`). It is info only:
  the W0 JSONs predate it, so the plan's ">= 0.85x W0" rule has no reference yet.
- TODO: RTT injection in `sockproxy.py` (for example 20 ms) and an
  ssh-localhost variant, for remote CLI latency, attach time and echo over a
  link with real round-trip time.
- TODO: agent pane streaming (the fixture ACP provider): daemon CPU, threads
  per agent pane, stream fanout bytes.
- TODO: copy-mode entry memory delta on a 10k x 180 pane.
- Done (2026-09-29): the gate runs on Linux; the Linux W0 is
  `results/baseline-alienware-17e17115.json` and its quick twin.
