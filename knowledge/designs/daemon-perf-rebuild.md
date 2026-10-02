---
type: Design Plan
title: Daemon performance rebuild
description: "The campaign to bring the zz daemon to tmux cost per command, per pane and per attach while keeping the 5x output throughput lead - a permanent zz-vs-tmux gate first, then waves that remove unrequested work (one-frame Exec commands, change-driven publication, lazy formats, frames only for watchers, a compact wire under one unreleased protocol version), then one mux loop and PTY shards; the lane brief source with targets, merge order, write zones, gates and rollback switches."
status: Approved 2026-09-28; wave 0 built; release freeze until W4 exits; wave 1 on main; wave 2 in progress (HOOKS, TERM, COPY and FMT merged on perf/wave2; CTRL review fixes committed on perf/ctrl, COPY/FMT integration checks passed except baseline alias differences, serial latency floor open and merged performance A/B pending); continued on Linux from bench/perf/campaign/HANDOFF.md
resource: crates/zz-daemon/src/daemon.rs
tags:
- performance
- daemon
- protocol
- tmux
- benchmark
- campaign
- design-plan
timestamp: 2026-10-01T00:54:00Z
---

# Campaign status

2026-09-29: wave 1 (W0, all six wave-1 lanes, three folded side branches) passed its Linux exit
and is on main (`1e0bfc6a`). Wave 2 runs on `perf/wave2`; W2-HOOKS is merge 1 (`0acd7f2a`,
`w2-1-hooks-alienware-0acd7f2a.json`) and W2-TERM merge 2 (`d7e3fc95`,
`w2-2-term-alienware-d7e3fc95.json`). W2-CTRL is built on `perf/ctrl`, with final Mac validation
ongoing and the serial control latency floor still open. Its `Batch` carries TERM's
PaneFrames through `encode_terminal_viewport_event_into` and `encode_terminal_patch_event_into`.
The campaign continues on a Linux host:
`bench/perf/campaign/HANDOFF.md` has the state, the numbers against tmux at the last gate, the
next steps in order, the macOS-only checks and the traps; `bench/perf/campaign/attach-review.json`
has the attach lane's reports; `bench/perf/campaign/scripts/` has the lane workflow template.

2026-09-30: W2-COPY merged as `2cedf6ee` and W2-FMT as `8fa427dd` on `perf/wave2`.
Their as-built sections below record gate, parity and profile evidence. CTRL review fixes
are committed on `perf/ctrl`; integration validation now includes both lanes. COPY extends
the daemon write zone for retained watcher events and popup ownership. The release freeze
remains until W4 exits.

Wave 1 merged FORMAT before PUBLISH (merges 2 and 3), against the order below: a usage-limit cut
left PUBLISH's fix pass unfinished while FORMAT was ready. Merges 1-5 ran the gate without
`--strict` because other lanes were compiling; whether they need strict reruns is an open owner
decision in the handoff.

# Outcome

The daemon does per event only the work someone asked for, at tmux cost, and keeps the parallel
libghostty parse, 64 KiB reads, time-sampled frames and latest-wins mailbox that give zz its 5x
throughput lead. Every claim is a number from `bench/perf` measured against a release tmux in the
same run.

At W0, each event did the work for all clients under one global `Mutex`, on a thread per thing.
The starting profiles put 80-95% of daemon CPU on each hot path in work nobody requested.

# Baseline

W0: `bench/perf/results/baseline-macbook-17e17115.json`, a full `--stage baseline` run of the gate
at 17e17115 on macOS (M4 Max, 16 CPUs) against Homebrew tmux 3.7c (release build,
`/opt/homebrew/bin/tmux`), isolated servers, both with history-limit 10000, 1-minute load
5.68 at the start and 4.48 at the end. `baseline-quick-macbook-17e17115.json` is the same at
`--quick`, the reference for lane loops. Values are medians; CPU is server CPU.

| Metric id | What | tmux | zz | zz/tmux |
|---|---|---|---|---|
| `cli.wall.version.p1` | process spawn floor (`-V`) | 2.94 ms | 3.61 ms | 1.23x |
| `cli.wall.display.p1` | `display-message -p` wall, 1 pane | 3.39 ms | 5 ms | 1.47x |
| `cli.cpu.display.p1` | daemon CPU per `display-message`, 1 pane | 0.078 ms | 1.08 ms | 13.9x |
| `cli.cpu.display.p20` | same, 20 panes | 0.119 ms | 3.07 ms | 25.8x |
| `cli.instr.display.p1` | daemon instructions per `display-message`, 1 pane | 0.486 Minstr | 10.8 Minstr | 22.2x |
| `control.cpu_per_cmd` | same command over an open `-C` connection | 0.0141 ms | 0.536 ms | 38.0x |
| `cli.wall.list_keys.p1` | `list-keys` wall, 1 pane | 6.07 ms | 19.0 ms | 3.13x |
| `cli.wall.list_keys.p20` | `list-keys` wall, 20 panes | 6.39 ms | 67.8 ms | 10.6x |
| `cli.cpu.list_keys.p20` | `list-keys` CPU, 20 panes | 2.81 ms | 64.1 ms | 22.8x |
| `cli.wall.list_panes_a.s20` | `list-panes -a` wall, 20 sessions | 3.71 ms | 11.6 ms | 3.13x |
| `cli.cpu.list_windows_a.s20` | `list-windows -a` CPU, 20 sessions | 0.236 ms | 8.16 ms | 34.5x |
| `cli.cpu.list_sessions.s20` | `list-sessions` CPU, 20 sessions | 1.79 ms | 8.19 ms | 4.57x |
| `spawn.wall.split_empty_P` | `split-window -d -P -F '#{pane_id}' ''` wall | 4.19 ms | 2008 ms | 478.7x |
| `spawn.wall.split_shell` | `split-window -d` (shell) wall | 5.1 ms | 10.1 ms | 1.98x |
| `spawn.cpu.split_shell` | `split-window -d` (shell) CPU | 0.901 ms | 4.76 ms | 5.28x |
| `spawn.cpu.kill_pane` | `kill-pane` CPU | 0.118 ms | 1.61 ms | 13.6x |
| `cold.wall.new_session` | cold `new-session -d` wall | 11.7 ms | 42.5 ms | 3.62x |
| `cold.infocmp_forks` | infocmp runs on a cold start | 0 | 1 | - |
| `config.wall.source_1000` | `source-file`, 1000-line config, wall | 13.0 ms | 235.0 ms | 18.0x |
| `config.cpu.source_1000` | same, server CPU | 9.2 ms | 230.1 ms | 25.0x |
| `chatty.cpu_pct.flip` | 10 panes ~100 lines/s, detached, fg flips | 1.64% | 23.9% | 14.6x |
| `chatty.cpu_pct.steady` | same, steady printer | 1.44% | 9.86% | 6.84x |
| `chatty.cpu_pct.hidden` | TUI on an idle window, 10 hidden windows print: CPU | 2.06% | 54.1% | 26.3x |
| `chatty.tty_kibps.hidden` | same: tty bytes | 0.68 KiB/s | 236.9 KiB/s | 348.5x |
| `chatty.cpu_pct.visible` | 4 visible printing panes: server CPU | 1.81% | 17.0% | 9.39x |
| `chatty.client_cpu_pct.visible` | same: TUI client CPU | 0% | 2.56% | - |
| `chatty.tty_kibps.visible` | same: tty bytes | 320.9 KiB/s | 1342 KiB/s | 4.18x |
| `idle.cpu_pct.p20` | 20 idle panes, no client: CPU | 0% | 0.0037% | - |
| `idle.wakeups_per_s.p20` | same: wakeups | 0/s | 1.2/s | - |
| `mem.footprint.p1` | footprint, 1 pane | 2.69 MiB | 7.92 MiB | 2.95x |
| `mem.footprint.p20` | footprint, 20 idle panes | 2.84 MiB | 54.3 MiB | 19.1x |
| `mem.footprint.tui20` | footprint, 20 panes, TUI attached | 3.13 MiB | 56.7 MiB | 18.1x |
| `mem.rss.p20` | RSS, 20 idle panes (info) | 4.22 MiB | 65.7 MiB | 15.6x |
| `mem.threads.p20` | threads, 20 idle panes | 1 | 89 | 89.0x |
| `mem.footprint.scroll180` | 20 panes x 10k lines, 180 cols: footprint | 61.4 MiB | 343.8 MiB | 5.6x |
| `mem.footprint.scroll80` | same, 80 cols | 35.4 MiB | 170.6 MiB | 4.82x |
| `attach.ttfc.p1` | TUI attach, time to first content, 1 pane | 7.61 ms | 18.0 ms | 2.37x |
| `attach.ttfc.p4` | same, 4 panes | 7.95 ms | 26.8 ms | 3.37x |
| `attach.tty_total.p1` | attach tty bytes until quiet, 1 pane | 972 B | 162018 B | 166.7x |
| `attach.tty_bytes.p1` | attach tty bytes before content, 1 pane (info) | 220 B | 157253 B | 714.8x |
| `attach.tty_total.p4` | attach tty bytes until quiet, 4 panes | 3653 B | 336880 B | 92.2x |
| `attach.cpu.p4` | daemon CPU per attach+detach, 4 panes | 1.7 ms | 12.1 ms | 7.08x |
| `attach.conns.p4` | daemon connections per attach | - | 4 | - |
| `attach.wire_s2c.p4` | daemon to client wire bytes per attach, 4 panes | - | 239976 B | - |
| `echo.p50.idle` | echo latency p50, idle pane | 0.0728 ms | 0.262 ms | 3.6x |
| `echo.p99.idle` | echo latency p99, idle pane | 0.158 ms | 0.489 ms | 3.11x |
| `echo.p50.busy30` | echo latency p50, pane printing at 30 Hz | 0.0715 ms | 6.38 ms | 89.2x |
| `echo.p99.busy30` | echo latency p99, pane printing at 30 Hz | 0.162 ms | 18.5 ms | 113.8x |
| `echo.wire_bytes.idle` | wire bytes per echoed key | - | 1133 B | - |
| `throughput.detached.ascii` | `cat` 150 MiB ASCII, detached 180x50 | 49.2 MB/s | 244.9 MB/s | 4.97x |
| `throughput.detached.unicode` | `cat` 150 MiB unicode, detached | 9.9 MB/s | 103.4 MB/s | 10.5x |
| `throughput.attached.ascii_ms` | 150 MiB through an attached TUI | 3821 ms | 630.7 ms | 0.165x |
| `control.latency` | `-C` command latency | 0.0201 ms | 0.549 ms | 27.3x |
| `control.burst_cmds_per_s` | `-C` burst of 200 commands | 183136 cmd/s | 1863 cmd/s | 0.01x |
| `control.output_mbps` | `-C` `%output` throughput | 40.3 MB/s | 111.6 MB/s | 2.77x |
| `statusjob.cpu_pct` | 3 `#()` jobs, interval 1, 1 TUI: CPU | 0.232% | 0.531% | 2.29x |
| `statusjob.threads_per_s` | same: threads created per second | 0/s | 3/s | - |

Design brief numbers (history). These were measured by hand before the gate existed, several at
load average ~35; the gate numbers above replace them. Where they differ (attach CPU for 4 panes,
tmux `list-windows -a` and `list-sessions` CPU, tmux attach bytes, idle CPU, RSS for 20 panes) the
gate's alternating, isolated measurement is the one to use.

| Metric | tmux 3.7c | zz now |
|---|---|---|
| Process spawn floor (`-V`) | 39.8M instr, 3.27 ms | 39.7M instr, 3.16 ms (equal) |
| CLI `display-message -p` wall, 1 pane | 4.2 ms | 5.1 ms |
| Daemon CPU per `display-message`, 1 / 20 panes | 0.10 / 0.17 ms | 1.27 / 3.3 ms |
| Same command over an open control connection | n/a | 0.22 / 0.38 ms |
| CLI round trips per command | 1 | 3 (+1 per extra chained command) |
| `list-keys` wall, 1 / 20 panes | 7 / 7 ms (5.5 ms CPU) | 21 / 68 ms (17 / 78 ms CPU) |
| `list-panes -a` wall, 20 panes | 4.3 ms | 9.1 ms |
| `list-windows -a` / `list-sessions` CPU, 20 sessions | 1.0 / ~4 ms | 12 / 10 ms |
| `split-window -d -P -F '#{pane_id}' ''` wall | 5 ms | 2008 ms |
| `split-window -d` (shell) wall / CPU | 5.5 / 0.9 ms | 9.2 / 5.6 ms |
| `kill-pane` CPU | 0.1 ms | 2.1 ms |
| Cold `new-session -d` wall | 16 ms (14.7-18.0) | 40 ms (40.7-42.3) |
| `source-file`, 1000-line config | 14.6 ms | 272 ms |
| oh-my-tmux cold start wall / server CPU | 476 / 20 ms | 630 / 110 ms |
| 10 panes ~100 lines/s, detached: flip loop / steady | 2.1% / 1.8% | 24.4% / 14.6% |
| 1 TUI on idle window, 10 hidden windows print: CPU, tty | 2.6%, 0.9 KiB/s | 49.7%, 201 KiB/s |
| 20 idle panes: CPU, wakeups | ~0%, no timer without clients | ~0.03%, 1 Hz status tick + 1 Hz Claude peer scan |
| Footprint (`phys_footprint`), 1 pane / 20 idle panes | not measured | 7.7 / 55 MB |
| RSS, 1 pane / 20 idle panes | 4.2 / 4.4 MB | 17.3 / 51.5 MB |
| Threads, 20 idle panes | 1 | 89 (9 fixed, 4 per pane, 5 on Linux) |
| Threads added by one `zz -C` on 20 panes | 0 | +23 |
| 20 panes x 10k lines, 180 / 80 cols | 65 / 39 MB RSS | 345 / 175 MB footprint (386 / 197 RSS) |
| Copy-mode entry, 10k x 180 pane | clone | +16 MB |
| TUI attach, time to first content (180x50) | 9.3 ms | 21.4 ms |
| TUI attach, tty bytes before first content | 646 B | ~158 KB (9 blank 17 KB frames) |
| Attach, 4 visible 90x24 panes: wire bytes / connections | 5.5 KB tty / 1 | 265 KB / 4 |
| Daemon CPU per attach+detach, 4 panes | 4 ms | 26 ms |
| Wire bytes per echoed keystroke | 1 B to the tty | 885-1613 B |
| TUI client CPU, 4-pane 300-line workload / tty bytes | 5 ms / 311 KiB/s | 132 ms / 1.2 MiB/s |
| `cat` 150 MiB ASCII, detached 180x50 | 57 MB/s | 266 MB/s |
| 150 MiB through an attached TUI | 3400 ms | 580 ms |

Where the time goes (sample shares, measured):

| Path | Share | Cause |
|---|---|---|
| CLI command | 47-51% | 28 KB `ServerHello` built and discarded (`Shared::register`) |
| CLI command, bound key | 11-21% | all ~370 bindings snapshotted and compared (`publish_key_tables_if_changed`) |
| CLI command | 8% | `release_view` wakes every pane actor on unregister |
| CLI command | 5-10% | 3 threads per connection, mimalloc fresh-page madvise |
| `list-keys` / `list-panes` | 95% / 90% | full format universe built per row (`FormatContext::resolve`) |
| Detached pane output | 71% | frames built with no view (`publish_active_views` fallback) |
| Pane output | 17-19% | sysinfo scan of ~1460 processes per burst (`terminal_current_command`) |
| Pane output, fg flips | 47-74% | `publish_snapshot_state` storms; 77% of Snapshots differ only in generation (default config) |
| Detached ticks | 52% | `format_option_snapshot` built with zero subscribers (`refresh_status_filtered`) |
| Config replay | 59% / 17% / 18% | key tables / status with no subscriber / option-name format expansion |
| 10 panes printing | 1506 thread-ms per 1.84 s | `lock_slow` on the global `inner` lock |

# Targets

Every gated metric with its W0 values and its rule per stage, printed by
`python3 bench/perf/run.py --targets` from `bench/perf/thresholds.json`. The json is the only
source of these numbers: change it, then regenerate this table. A blank cell keeps the rule of the
stage before it. `x` is times tmux in the same run; `W0` is the committed wave 0 value.

Memory is gated on footprint (macOS `ri_phys_footprint`, Linux `smaps_rollup`
`Pss_Anon + Pss_Shmem + SwapPss`, else `RssAnon + VmSwap`); RSS is reported, never gated, because ~9.6 MB of it is file-backed pages of the multi-role `zz_cli`
binary. Wall rules fail on a quiet host and with `--strict`, and warn on a loaded one; the others
always fail on a miss. Report-only, never gated: `cli.wall.version.p1`, `cold.infocmp_forks`,
`attach.wire_frames.*`, `attach.wire_c2s.*`, `echo.wire_bytes.busy30`, `statusjob.tty_kibps`, the
`rss`, `bytes_info` and `cpu_info` kinds, and the `instr` twins, which hold the regression rule
instead (see Waves and merge order).

| Metric id | unit | tmux | zz W0 | baseline | wave1 | wave2 | wave3 | final |
|---|---|---|---|---|---|---|---|---|
| `cli.wall.display.p1` | ms | 3.39 | 5 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.display.p1` | ms | 0.078 | 1.08 |  | <= 0.25 ms |  | <= 1.2x or <= tmux + 0.03 ms |  |
| `cli.wall.list_panes.p1` | ms | 3.39 | 4.9 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.list_panes.p1` | ms | 0.0896 | 1.06 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.show_options.p1` | ms | 3.32 | 4.84 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.show_options.p1` | ms | 0.0763 | 1.06 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.has_session.p1` | ms | 3.5 | 5.23 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.has_session.p1` | ms | 0.0672 | 1.07 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.send_keys.p1` | ms | 3.29 | 4.81 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.send_keys.p1` | ms | 0.0644 | 1.01 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.select_pane.p1` | ms | 3.25 | 4.77 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.select_pane.p1` | ms | 0.0622 | 0.978 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.list_keys.p1` | ms | 6.07 | 19.0 |  | <= 1x |  |  |  |
| `cli.cpu.list_keys.p1` | ms | 2.73 | 14.9 |  | <= 3 ms |  |  | <= 1.2x or <= tmux + 0.1 ms |
| `cli.wall.chain5.p1` | ms | 3.47 | 5.78 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.chain5.p1` | ms | 0.11 | 1.82 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.display.p20` | ms | 3.38 | 6.03 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.display.p20` | ms | 0.119 | 3.07 |  | <= 0.4 ms |  | <= 1.2x or <= tmux + 0.03 ms |  |
| `cli.wall.list_panes.p20` | ms | 3.38 | 6.03 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.list_panes.p20` | ms | 0.13 | 3.04 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.show_options.p20` | ms | 3.37 | 6.04 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.show_options.p20` | ms | 0.116 | 3.04 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.has_session.p20` | ms | 3.36 | 5.8 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.has_session.p20` | ms | 0.094 | 2.94 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.send_keys.p20` | ms | 3.47 | 5.96 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.send_keys.p20` | ms | 0.101 | 3.02 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.select_pane.p20` | ms | 3.39 | 5.87 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.select_pane.p20` | ms | 0.097 | 3.02 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.list_keys.p20` | ms | 6.39 | 67.8 |  | <= 1x |  |  |  |
| `cli.cpu.list_keys.p20` | ms | 2.81 | 64.1 |  | <= 5 ms |  |  | <= 1.2x or <= tmux + 0.1 ms |
| `cli.wall.chain5.p20` | ms | 3.52 | 7.24 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.chain5.p20` | ms | 0.146 | 4.16 |  |  |  |  | <= 1.2x or <= tmux + 0.03 ms |
| `cli.wall.list_panes_a.s20` | ms | 3.71 | 11.6 |  | <= 1x |  |  |  |
| `cli.cpu.list_panes_a.s20` | ms | 0.332 | 8.18 |  | <= 1.2x |  |  | <= 1.2x or <= tmux + 0.05 ms |
| `cli.wall.list_windows_a.s20` | ms | 3.62 | 11.5 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.list_windows_a.s20` | ms | 0.236 | 8.16 |  | <= 1.2x |  |  | <= 1.2x or <= tmux + 0.05 ms |
| `cli.wall.list_sessions.s20` | ms | 5.17 | 11.6 |  | <= 1.1x |  |  | <= 1.05x |
| `cli.cpu.list_sessions.s20` | ms | 1.79 | 8.19 |  | <= 1.2x |  |  | <= 1.2x or <= tmux + 0.05 ms |
| `spawn.wall.split_shell` | ms | 5.1 | 10.1 |  | <= 1.1x |  |  | <= 1.05x |
| `spawn.cpu.split_shell` | ms | 0.901 | 4.76 |  | <= 1.5 ms |  | <= 1.2x |  |
| `spawn.wall.split_empty_P` | ms | 4.19 | 2008 |  | <= tmux + 1.5 ms |  |  | <= 1.05x |
| `spawn.cpu.split_empty_P` | ms | 0.221 | 7.56 |  | <= 1.6x |  |  | <= 1.2x |
| `spawn.wall.new_window` | ms | 4.57 | 9.69 |  | <= 1.1x |  |  | <= 1.05x |
| `spawn.cpu.new_window` | ms | 0.748 | 4.79 |  | <= 1.6x |  |  | <= 1.2x |
| `spawn.wall.kill_pane` | ms | 3.51 | 5.49 |  | <= 1.1x |  |  | <= 1.05x |
| `spawn.cpu.kill_pane` | ms | 0.118 | 1.61 |  | <= 0.8 ms | <= 0.4 ms | <= tmux + 0.1 ms |  |
| `cold.wall.new_session` | ms | 11.7 | 42.5 |  | <= 22 ms |  |  | <= 1.15x |
| `cold.wall.new_session_noterm` | ms | 11.7 | 39.0 |  | <= 22 ms |  |  | <= 1.15x |
| `config.wall.source_1000` | ms | 13.0 | 235.0 |  | <= 25 ms | <= 1.1x |  |  |
| `config.cpu.source_1000` | ms | 9.2 | 230.1 |  | <= 25 ms | <= 1.1x + 1 ms |  |  |
| `chatty.cpu_pct.steady` | % | 1.44 | 9.86 |  | <= 1.5x + 1% |  |  | <= 1.2x + 0.5% |
| `chatty.cpu_pct.flip` | % | 1.64 | 23.9 |  | <= 1.5x + 1% |  |  | <= 1.2x + 0.5% |
| `chatty.cpu_pct.hidden` | % | 2.06 | 54.1 |  | <= 1.5x + 1% |  |  | <= 1.2x + 0.5% |
| `chatty.tty_kibps.hidden` | KiB/s | 0.68 | 236.9 |  | <= 2 KiB/s |  |  |  |
| `chatty.client_cpu_pct.hidden` | % | 0 | 1.97 |  |  | <= 3x + 1% |  |  |
| `chatty.cpu_pct.visible` | % | 1.81 | 17.0 |  | <= 1.5x + 1% |  |  | <= 1.2x + 0.5% |
| `chatty.tty_kibps.visible` | KiB/s | 320.9 | 1342 |  |  | <= 1.5x |  |  |
| `chatty.client_cpu_pct.visible` | % | 0 | 2.56 |  |  | <= 3x + 1% |  |  |
| `idle.cpu_pct.p20` | % | 0 | 0.0037 |  | <= 0.1% |  | <= 0.05% |  |
| `idle.wakeups_per_s.p20` | 1/s | 0 | 1.2 |  | <= 1/s |  | <= 0.2/s |  |
| `mem.footprint.p1` | MiB | 2.69 | 7.92 |  | <= 6.5 MiB |  |  | <= 5 MiB |
| `mem.threads.p1` | count | 1 | 13 |  |  |  |  | <= 12 |
| `mem.footprint.p20` | MiB | 2.84 | 54.3 |  | <= 38 MiB |  |  | <= 12 MiB |
| `mem.threads.p20` | count | 1 | 89 |  | <= 51 |  |  | <= 12 |
| `mem.footprint.tui20` | MiB | 3.13 | 56.7 |  | <= 1.02x W0 |  |  | <= 15 MiB |
| `mem.threads.tui20` | count | 1 | 91 |  |  |  |  | <= 12 |
| `mem.footprint.scroll180` | MiB | 61.4 | 343.8 |  | <= 1x |  |  | <= 0.65x |
| `mem.threads.scroll180` | count | 1 | 93 |  |  |  |  | <= 12 |
| `mem.footprint.scroll80` | MiB | 35.4 | 170.6 |  | <= 1.15x |  |  | <= 0.8x |
| `mem.threads.scroll80` | count | 1 | 93 |  |  |  |  | <= 12 |
| `attach.ttfc.p1` | ms | 7.61 | 18.0 |  | <= 14 ms | <= 1.1x |  |  |
| `attach.tty_total.p1` | B | 972 | 162018 |  | <= 8192 B | <= 2048 B |  |  |
| `attach.cpu.p1` | ms | 1.46 | 10.0 |  | <= 3x | <= 1.25x |  |  |
| `attach.ttfc.p4` | ms | 7.95 | 26.8 |  | <= 14 ms | <= 1.1x |  |  |
| `attach.tty_total.p4` | B | 3653 | 336880 |  | <= 16384 B | <= 1.5x |  |  |
| `attach.cpu.p4` | ms | 1.7 | 12.1 |  | <= 3x | <= 1.25x |  |  |
| `attach.conns.p1` | count | - | 4 |  | <= 2 | <= 1 |  |  |
| `attach.wire_s2c.p1` | B | - | 257146 |  | <= 133120 B | <= 8192 B |  |  |
| `attach.conns.p4` | count | - | 4 |  | <= 2 | <= 1 |  |  |
| `attach.wire_s2c.p4` | B | - | 239976 |  | <= 133120 B | <= 8192 B |  |  |
| `echo.p50.idle` | ms | 0.0728 | 0.262 |  |  |  | <= 1.5x |  |
| `echo.p99.idle` | ms | 0.158 | 0.489 |  |  |  | <= 1.5x |  |
| `echo.p50.busy30` | ms | 0.0715 | 6.38 |  |  |  | <= 1.5x |  |
| `echo.p99.busy30` | ms | 0.162 | 18.5 |  |  |  | <= 1.5x |  |
| `echo.wire_bytes.idle` | B | - | 1133 |  | <= 1700 B | <= 64 B |  |  |
| `throughput.detached.ascii` | MB/s | 49.2 | 244.9 | >= 4x or >= 0.85x the pty ceiling, >= 0.85x W0 |  |  |  |  |
| `throughput.detached.unicode` | MB/s | 9.9 | 103.4 | >= 4x or >= 0.85x the pty ceiling, >= 0.85x W0 |  |  |  |  |
| `throughput.attached.ascii_ms` | ms | 3821 | 630.7 | <= 0.25x or <= the pty ceiling's time / 0.85, <= 1.18x W0 |  |  |  |  |
| `control.latency` | ms | 0.0201 | 0.549 |  |  | <= 1.2x |  | <= 1.1x |
| `control.cpu_per_cmd` | ms | 0.0141 | 0.536 |  |  | <= 1.5x or <= tmux + 0.1 ms |  | <= 1.2x or <= tmux + 0.03 ms |
| `control.burst_cmds_per_s` | cmd/s | 183136 | 1863 |  |  | >= 0.8x |  | >= 0.9x |
| `control.output_mbps` | MB/s | 40.3 | 111.6 |  | >= 0.85x W0 |  |  | >= 1x |
| `statusjob.cpu_pct` | % | 0.232 | 0.531 |  |  |  | <= 1.5x + 0.1% |  |
| `statusjob.threads_per_s` | 1/s | 0 | 3 |  |  |  | <= 0.5/s |  |

Notes on the rules:

- Other hosts (2026-09-29, Linux leg). The rules above were set against the macOS reference host
  (`reference` in `thresholds.json`). On any other host the gate keeps each absolute rule's
  distance from tmux instead of its number: `cpu` and `wall` rules keep their multiple of the
  reference W0's tmux median (this Linux laptop runs tmux 1.3-2.8x slower than the M4 Max), and
  `mem` rules keep their margin over it (footprint definitions differ by a fixed per-process
  amount: tmux is 2.7 MiB on macOS and 0.95 MiB on Linux). The check is labelled `abs@ratio` or
  `abs@plus` with the bound used. Counts, bytes, threads and ratio rules are the same everywhere.
- Throughput rules also pass at 85% of the host's pty ceiling (`throughput.ceiling.*`: a bare
  reader of the same `cat` through a cooked 180x50 pty, measured in the same run), for hosts where
  4x tmux is above what the kernel lets any reader do: on Linux the cooked tty caps a reader near
  135 MB/s while tmux does 50, so 4x would need 200. On macOS 4x tmux stays the bar.
- Echo latency moved from wave 1 to wave 3 on 2026-09-29: no wave-1 lane owned the remaining
  hops (client reader, actor, watcher, mailbox writer, the `inner` lock in
  `publish_terminal_for_pane`), which W3-SHARDS, W3-LOOP and W4-DELIVER remove. The rule itself
  (1.5x tmux) is unchanged.

- `cli.wall.display.p1` <= 1.10x at W1 comes from Exec alone; no process spawn saving is claimed.
- `spawn.cpu.kill_pane` is staged: 0.8 ms at W1, 0.4 ms after W2-HOOKS, tmux + 0.1 ms after
  W3-LOOP.
- `idle.wakeups_per_s.p20` <= 0.2 at W3 means no client timers; `mem.threads.*` <= 12 at final
  counts agent threads apart (W4).
- `attach.cpu.*` and `mem.footprint.scroll*` are ratios to tmux because the gate measures
  different absolutes than the design brief did.
- Throughput is gated at every stage on the ratio to tmux in the same run and on 0.85x W0
  (1.18x W0 for the attached time), not on an absolute taken from the brief (250 MB/s, 610 ms);
  W0 itself measures 245 MB/s and 631 ms.
- `cold.infocmp_forks`: zz forks `infocmp -x -1 $TERM` on the first client hello of each TERM
  (`warm_terminfo_entries`); if it costs over 1 ms, parse terminfo in process or warm it after
  readiness.
- Copy-mode entry is gated in `mem` after W2-COPY: at most 1 MiB incremental footprint and
  5 ms daemon CPU and control reply wall time on a 10k x 180 pane. W2-TERM built the frame-sink
  client and `throughput.headless.ascii_ms`; that row is informational because W0 has no
  headless reference. Attached footprint with a GUI-like client, injected-RTT `remote.*` and
  `agent.*` remain ungated.

# Architecture

1. **Command clients are one-shot.** The CLI sends one `Exec` frame (facts, environment blob,
   parsed chain) and gets one `CommandResponse` per command and one `ExecExit`, written together.
   No `ServerHello`, no `PrepareCommandList`, no per-command round trip. Wave 1: one thread per
   Exec connection, reused when idle. Wave 3: the mux loop runs it. Cold start waits on a
   readiness pipe, not a 20 ms sleep; the identity file is written without `F_FULLFSYNC`.
2. **Publication is driven by changes.** Key tables, mux tree, options and status inputs carry
   generations; pane runtime facts no longer bump the tree generation. Commands and pane events
   mark dirty; one flush at the end of a command, or at most every 16 ms for pane events, builds
   only for subscribers. Bookkeeping that is not a send (window-style appearance, chooser zoom
   release, latest-client pruning) always runs. Per-client stamped labels are re-stamped on
   runtime-fact changes and sent only when the stamped bytes change. `automatic-rename` is
   throttled at tmux `NAME_INTERVAL` 500 ms (tmux.h:115, names.c:48-50). Hooks come from a change
   journal zz-mux mutations emit, not before/after copies of the world.
3. **Formats are lazy.** The format universe is built only when a loop, `O:`, `N:` or environment
   lookup reads it, and only that part. Inside the lock it is borrowed; every place a context
   escapes the lock detaches the parts its templates need. Option names resolve through a static
   index. Strings with no `#` skip expansion.
4. **Panes work only for watchers.** The PTY read/parse loop is untouched. Frames exist only for
   streamed views (foreground or preview); unwatched panes refresh a metadata record and drop
   render state. Idle history pages are LZ4-compressed with libghostty `Terminal::compress`
   (probe: 288 MB to 13-24 MB). Foreground command via `tcgetpgrp` + `KERN_PROCARGS2` exec-path
   basename, cwd via `proc_pidinfo`, `/proc` on Linux. PTY name from `ptsname`; child exit from the
   actor's kqueue/epoll set.
5. **Compact wire for interactive clients.** One `Hello` with viewport, subscriptions and optional
   attach target; reply is a ~40 B `Welcome` plus one `Batch` with the whole initial state at the
   final size. Tree deltas encoded once per scope, a small per-client `ClientView`. `PaneFrame`:
   varint header, per-pane stream sequence, changed column spans as style runs + UTF-8, trailing
   blanks dropped. Clients decode into today's `PackedCell` planes and `MuxSnapshot`, so renderers
   and `include/zz-client.h` do not change.
6. **Threads and ownership (W3-W4).** One mux loop thread owns `ServerState` with no `Mutex`
   (mio over listener, client sockets, waker, signal pipe, timer heap; blocking commands become
   continuations). K PTY shard threads (K = min(cores, 4), tuned by the gate) own PTYs and
   terminals, poll child exit, encode each pane frame once, push the bytes into every subscribed
   client queue, and post edge events to the loop. Per daemon: 1 loop + K shards + agent threads,
   against 9 fixed + 4-5 per pane + 3 per CLI command + 3+N per control client today.

Why this order: waves 1-2 remove the work that the profiles show is most CPU, latency and bytes,
with no threading surgery. Waves 3-4 remove threads and the lock, which the memory, thread-count,
idle-wakeup and final per-command targets need, and each conversion then carries less. Report
fixes that share a root cause are folded into the structural fix for that cause (Exec replaces
hello slimming, dirty publication replaces per-site early returns, the lazy universe replaces
per-caller reuse).

# Protocol and release policy

- **One protocol version for the whole campaign.** `compat/wire-version.py` reports "107 is
  unreleased (v0.14.0 shipped 106); appends are free". Every campaign wire change (Exec,
  Welcome/Batch/TreeDelta/ClientView, PaneFrame) lands in 107. No lane bumps `PROTOCOL_VERSION`.
- Non-append changes inside 107 (W2-TERM deletes the 8 B/cell `Full`/`Patch` encoders, W2-CTRL
  turns `ServerHello` into `Welcome`) are legal only while 107 is unshipped. Two campaign builds
  can then both claim 107 and disagree on bytes. Rules: after pulling a wire-changing merge,
  `kill-server` every zz Dev daemon; a lane never tests against a daemon built before its merge
  base; every wire-changing merge is noted in the campaign log.
- A pre-campaign 107 daemon cannot decode `Exec`. On a same-version first-frame decode failure or
  EOF, the CLI opens a hello connection and reruns the chain there only when that hello lacks
  `exec-v1`, so a chain an Exec daemon took is never run twice (the legacy path stays behind
  `ZZ_PERF_LEGACY_COMMAND` anyway). A released 106 daemon rejects at the envelope check with
  `CommandResponse::Error(ProtocolMismatch)`, which `classify_local_connect_error` already turns
  into today's mismatch prompt.
- **No release tags mid-campaign.** A tag freezes 107; the next wire change must then bump to 108
  and strands the in-pane `tmux` wrapper, remote ssh hosts, TestFlight builds and the web assets
  embedded in zz-web.
- **Release freeze until W4 exits** (owner, 2026-09-28: zz is not fully launched, nothing ships
  mid-campaign). No tags, no beta channel pushes, no TestFlight uploads until `--stage final`
  passes. Any wire change in any wave may land in 107, including non-append rewrites.
- **Performance before features** (owner, 2026-09-28). No plugins or other features until the
  daemon is at least as lean as tmux.
- **No compromises** (owner, 2026-09-28). A lane may not stop at "meets the target" when the
  profile still shows avoidable work on its path; targets are floors. The three deferrals the
  architect rejected as good enough are reinstated as lanes W2-FMT, W4-ROWS and W4-BINARY. Wave 3
  is committed, not optional.
- **Pin the in-pane `tmux` wrapper to the daemon's own executable.** `install_tmux_shim` pointed
  the wrapper at `current_exe()` (or `ZZ_TMUX_EXECUTABLE`), a path an app swap replaces, so panes
  of an old daemon ran a newer CLI. Built in W1-EXEC: the wrapper runs `/proc/<daemon pid>/exe`
  on Linux (the running image, no copy; a CLI whose installed image was replaced spawns through
  `/proc/self/exe`, since its `current_exe()` then names a deleted path) and a `clonefile(2)` clone beside the wrapper on macOS
  (hard link if that fails, else the installed path); the clone goes with the wrapper on daemon
  exit. `spawn_daemon` no longer exports `ZZ_TMUX_EXECUTABLE`, and an explicit one still wins
  unpinned.
- Every lane that touches the wire runs `just web-build` and `just ios-gpui iPad build` in its
  gate. `crates/zz-protocol/src/key.rs` is added to `NOT_WIRE` in `compat/wire-version.py`
  (KeyTables is not serialized), so W1-PUBLISH does not trip the guard after a future release.

# Waves and merge order

| Wave | Lanes, in merge order | Merge gate |
|---|---|---|
| 0 | W0-GATE, this document | `--stage baseline` full and `--quick` JSON committed as `bench/perf/results/baseline-<host>-17e17115.json` and `baseline-quick-<host>-17e17115.json` (W0) |
| 1 | W1-FOOTPRINT, W1-PUBLISH, W1-FORMAT, W1-PANE, W1-EXEC, W1-ATTACH | each lane: full run with `--strict`, `--baseline` = previous merge's JSON, W0 picked up automatically; wave exit: `--stage wave1` |
| 2 | W2-TERM, W2-CTRL (or one lane), W2-HOOKS, W2-FMT, W2-COPY | same chain; wave exit: `--stage wave2` |
| 3 | W3-SHARDS, W3-LOOP | same chain; wave exit: `--stage wave3` |
| 4 | W4-DELIVER, W4-ROWS, W4-BINARY | `--stage final` |

- Lanes in a wave develop in parallel from the wave base. They merge serially in the order above.
  Before merging, a lane rebases onto the previous merge, runs `just perf-gate <wave stage>
  --strict --baseline <previous merge JSON>` on a quiet host, and commits its own JSON as
  `bench/perf/results/w<wave>-<n>-<lane>-<host>-<sha8>.json`.
- A lane may not make a metric it does not own worse than the `tolerance` block of
  `thresholds.json` allows, checked against both the previous merge and W0 (so small steps cannot
  add up): 5% for instructions, bytes and counts, 10% for footprint and threads, each above an
  absolute floor. These fail the run. CPU time is held through its instruction count; its own
  moves (up to 61% between runs of one binary) are a note. Throughput is held by the ratio to tmux
  and a 0.85x floor against W0, never against the previous merge.
- Wave 1 order reasons: FOOTPRINT changes the process facts PUBLISH's runtime-fact path reads;
  PUBLISH removes the key-table and status costs every later per-command gate measures; FORMAT
  before EXEC so EXEC's CPU gate sees the lazy universe; PANE before EXEC because unregister cost
  depends on PANE's view tracking; ATTACH last because its connection and ttfc gates need EXEC.
- Wave 1 exit also runs `attach.ttfc.p1`, `compat/tui-screen-diff.sh` and `compat/attached-client.sh`
  (PANE drops frames for unwatched views, ATTACH drops duplicate frames: together they could leave
  a pane blank).
- W2-TERM merges before W2-CTRL because CTRL's `Batch` carries TERM's `PaneFrame`. They may run
  as one lane; the version is 107 either way. `message.rs` `EventPayload` terminal variants belong
  to TERM.
- If W2-COPY slips, W3-SHARDS starts without it and COPY rebases onto the shards (most copy-mode
  functions sit outside `run_terminal`).

# Lanes

Write zones name functions; line numbers drift. Find code with `rg -n "fn <name>"`.

## W0-GATE: permanent zz-vs-tmux gate (effort M)

Built in `bench/perf/` (`run.py`, `gate.py`, `isolate.py`, `probe.py`, `timing.py`, `ptyclient.py`,
`sockproxy.py`, `fixtures.py`, `groups/`, `pane/`, `thresholds.json`, `test_gate.py`), Python 3
stdlib + ctypes, headless. `bench/perf/README.md` is the reference for flags, probes, ids and the
noise policy; this section records what the lanes rely on.

- `just perf-gate [stage] [args]` builds `zz_cli` and runs `run.py --zz target/release/zz_cli
  --stage <stage>`. Flags: `--json`, `--baseline <previous merge>`, `--w0 <W0 JSON>` (found
  automatically), `--quick`, `--only`, `--strict`, `--keep`, `--rescore`, `--targets`.
- **tmux is a release build**: without `--tmux` the gate tries `/opt/homebrew/bin/tmux`,
  `/usr/local/bin/tmux`, `/usr/bin/tmux`, then PATH, resolves symlinks, and refuses scripts, the
  zz binary, any `-V` containing `-zz` (a zz pane puts the zz tmux wrapper first on PATH), and
  AddressSanitizer builds (`otool -L` / `ldd`, `__asan_init` via `nm`). The pinned `compat/.cache`
  tmux is an ASan debug build of next-3.8 and stays for compat behaviour only. `meta` records the
  resolved path, version, linked libraries, and the zz binary's sha256 and mtime.
- Isolation: fresh HOME / XDG_* / ZZ_DATA_DIR / ZZ_LOG_DIR, `ZZ_SOCKET=/tmp/zzpf-<pid>.sock`,
  `ZZ_TRAY=0`, `ZZ_UPDATE_CHECK=0`, `SHELL=/bin/bash` with no rc files; TMUX, TMUX_PANE, ZZ_PANE,
  ZZ_SESSION unset; both servers start with `-f <root>/perf.conf` holding
  `set -g history-limit 10000` (zz's default; tmux defaults to 2000), checked per group; both
  servers and every descendant killed after each group and on exit; zz and tmux runs alternate.
- Probes: CPU (macOS `RUSAGE_INFO_V4` user+system x timebase; Linux `clock_getcpuclockid` in ns,
  falling back to `/proc/<pid>/task/*/schedstat`), instructions (macOS `ri_instructions`, for every
  CPU metric; Linux user-space `perf_event_open` counters when available), footprint (`ri_phys_footprint`; Linux
  `Pss_Anon + Pss_Shmem + SwapPss`) and RSS as info, threads (`PROC_PIDTASKINFO`; `/proc` Threads), thread spawns (`PROC_PIDLISTTHREADIDS` sampled
  every ~2 ms; Linux task ids), wakeups (`ri_interrupt_wkups + ri_pkg_idle_wkups`; ctxt switches).
- `sockproxy.py`: Unix-socket relay counting bytes, u32-prefixed frames and connections (zz only;
  tmux passes the tty fd).
- Groups: `cli` (verbs at p1 and p20, list verbs at 20 sessions), `spawn`, `cold` (+ infocmp
  forks), `config`, `chatty` (steady, flip, hidden, visible with TUI CPU and tty bytes), `idle`,
  `mem` (p1, p20, tui20, scroll180, scroll80 and first copy-mode entry at 10k x 180), `attach` (ttfc, total tty bytes until quiet, CPU,
  wire), `echo` (latency idle and busy30, wire bytes), `throughput` (detached ASCII and unicode,
  attached), `control` (latency, burst rate, %output), `statusjob` (`#()` jobs).
- Thresholds per metric per stage in `thresholds.json`, the single source of the Targets table
  (`run.py --targets` prints it). Hard: CPU, bytes, footprint, threads, counts, throughput. Wall
  time fails on a quiet host (1-minute load per CPU <= 0.5 at start and end) and with `--strict`,
  and warns otherwise. Regressions against the previous merge and W0 follow the `tolerance` block.
  `report_only` names the metrics that never gate. Every metric records median, p10, p90, p99,
  min, max and n.
- JSON schema 2: `meta` (git_sha, zz_bin, zz_sha256, zz_mtime, zz_version, tmux_bin, tmux_version,
  tmux_libs, os, arch, ncpu, loadavg_start/end, noisy, stage, w0, history_limit, cpu_source,
  warnings, started_at), `metrics[]` (id, group, unit, kind, zz, tmux, ratio, vs_baseline, vs_w0,
  threshold, checks, verdict), `summary`.

Not built yet (TODO, also listed in `bench/perf/README.md`):

- TODO: the GUI-like mode of `crates/zz-client/examples/perf_client.rs`: all sessions, history
  backfill and 8 visible panes, with its `gui` group and attached footprint. W2-TERM built the
  frame-sink client and `throughput.headless.ascii_ms`; the latter needs a reference baseline.
- Built in W2-COPY: first copy-mode entry on a 10k x 180 pane, included in `mem` with tmux twins
  and wave2/final footprint, CPU and wall limits.
- TODO: injected RTT in `sockproxy.py` and the report-only `remote` group; the report-only `agent`
  group (fixture ACP provider).
- Done on 2026-09-29: the gate runs on Linux. Its W0 is
  `bench/perf/results/baseline-alienware-17e17115.json` and its quick twin.

Gate (met at 17e17115 on macOS, see Baseline): `--stage baseline` passes; `--stage wave1` and
`--stage final` fail on today's binary; full run <= 12 min (about 7), `--quick` <= 3 min (about 2.5); the
committed W0 JSONs are the Baseline table below; `test_gate.py` covers threshold evaluation, the
noise policy, the regression rules, the tmux choice and that every gated metric has a final rule.
Linux baselines and the wave-1 exit are recorded in `bench/perf/campaign/HANDOFF.md`.

## W1-FOOTPRINT: process facts, local time, allocator (effort M)

Scope:
1. `terminal_current_command`: `terminal.foreground_process_id()` then one `KERN_PROCARGS2`
   sysctl and the exec-path basename, which is what sysinfo 0.39.6 returns today. Not
   `pbsi_comm`: a `claude` symlink to `versions/2.1.99` reads `2.1.99` there (measured), which
   breaks the default `@agent-progress-commands claude` match in `synchronize_pane_progress` and
   renames windows to the version. Linux: `/proc/<pgrp>/stat` comm, what sysinfo returns today
   (tmux uses `/proc/<pgrp>/cmdline` argv0; not adopted). Record the divergence from tmux
   `osdep_get_name` (osdep-darwin.c:36-51, pbsi_comm at 48-49) in the tmux-drop-in ledger.
2. `terminal_working_directory`: `PROC_PIDVNODEPATHINFO`; Linux `/proc/<pid>/cwd`.
3. `lifecycle.rs`: `IdentityRecord::current` start time via `proc_pidinfo` `PROC_PIDTBSDINFO` or
   `/proc/<pid>/stat`; liveness via `kill(pid, 0)`; remaining sysinfo helpers. `client.rs`
   `short_device_name`: `gethostname`. `zz-cli/src/diagnostics.rs`. Drop sysinfo from zz-daemon
   and zz-cli manifests.
4. `crates/zz-mux/src/localtime.rs`: `localtime_r` -> `tm_gmtoff` -> `DateTime<FixedOffset>`,
   offset cached 1 s. Replace `chrono::Local` in status.rs (`render` now, `DaemonFormatHooks`,
   `command_with_optional_variables`, strftime helpers) and formats.rs (`CommandHooks::strftime`,
   `format_time_value`, `pretty_time`, `relative_time`, `format_datetime`). Removes the
   `/etc/localtime` re-read per new thread (3.5% of the split loop).
5. CoreFoundation: `cargo tree -p zz-cli -i core-foundation-sys` shows chrono -> iana-time-zone as
   the only path, but xtask `build_linux_binaries` and `build_macos_binaries` build
   `-p zz -p zz-cli` in one invocation, and gpui, zlog and crates/zz (`agent/view.rs` uses
   `Local`) enable chrono's clock, so features unify back. CF leaves `zz_cli` only if `zz_cli`
   builds in its own cargo invocation (xtask, deb, pacman, headless recipes) or links with
   `-Wl,-dead_strip_dylibs` for that target alone. Do it only if the dual build measures a gain;
   spawn is already equal to tmux, so no wall saving is claimed.
6. `purge_freed_memory_promptly`: mimalloc `os_tag` 241; purge delay chosen by the memory gate
   (start at 10 ms). `libghostty-vt-sys` `zig_optimize_mode`: ReleaseSafe only at OPT_LEVEL 0.

Write zone: workspace/zz-daemon/zz-cli/zz-mux `Cargo.toml` (sysinfo, chrono entries); daemon.rs
`terminal_working_directory`, `terminal_current_command`; lifecycle.rs sysinfo helpers and
`IdentityRecord::current` (not `write_identity_atomically`); client.rs `short_device_name`;
status.rs time sites; formats.rs time functions and chrono import; new `localtime.rs`;
`diagnostics.rs`; zz-cli `purge_freed_memory_promptly`; xtask build functions if item 5 goes
ahead; `zig_optimize_mode`.

Gate vs W0: `chatty.cpu_pct.flip` >= 15% lower; `mem.footprint.p1` >= 0.4 MB
lower; `mem.footprint.p20` no worse after the purge change; `cli.wall.version.p1` <= tmux + 0.3
ms; if item 5 lands, `otool -L` of the bundled binary in `dist/` lists only libSystem (+ libiconv)
and dyld image count is recorded (tmux 87, today 704). Tests: symlinked-binary fixture, compat
`pane_current_command` / `pane_current_path` rows, `compat/tui-choosers.sh`, lifecycle identity
and incompatible-daemon tests, agent peer tests, strftime tests, `cargo check` for Windows.

Expected: process lookup 37.5 us -> microseconds per burst and no longer grows with system
process count; madvise 5-7% of per-command CPU and 6.6% of config replay.

Built on `perf/footprint` (2026-09-28, revised after review). Where the build departs from the
scope above:

- Items 1-3 live in one module, `zz-daemon` `process_info.rs` (public, so zz-cli's verbose
  sampler uses it too). The macOS name lookup keeps the last four names per thread keyed on
  `PROC_PIDUNIQIDENTIFIERINFO` (unique id + exec generation `p_idversion`, 0.15 us): `p_idversion`
  moves on every exec, even an exec of the same image through another symlink, where
  `pbi_comm`, start time and executable UUID all stay put. A miss is one `KERN_PROCARGS2`
  sysctl into a shared 16 KiB buffer (a too-small buffer returns `size == capacity` with the exec
  path cut off, so only then does it ask for the size and grow to size + 1). A read that needed
  more shrinks the buffer back to 16 KiB, and a thread that finds the buffer busy reads into its
  own. Only a name read from `KERN_PROCARGS2` is cached. That sysctl fails with EIO for a moment
  at the start of an image, after `p_idversion` has already moved: a C probe that exec'd
  `bash -> claude` 200 times saw it on the `claude` image in 33 rounds. The first version cached
  the `proc_pidpath` fallback, which names the symlink target (`bash`, `2.1.99`), and kept that
  name for the life of the image on that thread. That was the cause of the flaky
  `process_facts_tests` run under a loaded crate test, and would have renamed windows to the
  version and broken `@agent_state` in real use. The fallback is now answered but not cached,
  which is what sysinfo did on every call. Windows keeps sysinfo as a Windows-only dependency
  behind the same functions, and `terminal_working_directory` stays `None` there as before (a
  PowerShell `Set-Location` does not move the process cwd); sysinfo stays a zz-daemon
  dev-dependency as the parity oracle in the `process_info` tests. Linux reads the name from
  `/proc/<pid>/comm` (the same `task->comm` as the stat field), reads `btime` from `/proc/stat`
  once per process, and parses only the stat fields it is asked for.
- Item 4: an unparseable `TZ` now formats in UTC, as the pin does; chrono `Local` used the
  system zone. glibc's `localtime_r` never re-reads the zone after its first call, while the
  pin's `localtime` re-checks `/etc/localtime` on every call, so on Linux `local_time` calls
  `tzset()` at most once per wall-clock second per thread and follows a zone change within a
  second. macOS `localtime_r` already follows zone changes. The year-long test compares
  against `localtime_r` field by field, so it holds under any `TZ`.
- Item 5 went ahead, but not through xtask: `crates/zz-cli/build.rs` links `zz_cli` with
  `-Wl,-dead_strip_dylibs`, and chrono lost its default features in the workspace (zz-mux and
  zz-daemon take `alloc` + `std`; `clock` only on non-unix and in zz-daemon's tests; crates/zz
  asks for `default`). A release `-p zz -p zz-cli` build, the xtask shape, and a lone
  `-p zz-cli` build both link only libSystem. dyld images 703 -> 83 (tmux 86); `zz_cli -V`
  3.7 -> 2.3 ms against tmux 3.5 ms in the same hyperfine run. A dev build still links
  CoreFoundation (no LTO, the dead code keeps its references).
- Item 6: the memory gate kept the purge delay at 0. Any delay (1, 3, 10, 100 ms) leaves freed
  pages of idle threads unpurged: `mem.footprint.p1` 6.53 -> 7.05 MiB at 80x24, which is past
  the wave1 6.5 MiB rule, for 1-3% less CPU per command and ~5% on `source-file`.
  `MIMALLOC_PURGE_DECOMMITS=0` was worse in a 2x A/B (`mem.footprint.p1` 7.42 -> 8.30 MiB, no
  CPU gain). So the expected madvise saving is not realized: madvise is still 7.7% of busy
  daemon samples in a `display-message` loop and 6.7% in a split/kill loop. 35 of 82 madvise
  samples come from `_pthread_tsd_cleanup` (a per-connection thread exits and its heap is purged
  at once) and most of the rest re-commit pages that delay 0 just decommitted. The structural
  fix is to stop creating a thread per connection (W1-EXEC). mimalloc (v3.3 in this build,
  purge delay default 1000 ms) reserves its 1 GiB arena before `main`, so `os_tag` 241 is set
  from a `__mod_init_func` initializer in zz-cli `main.rs` that runs before mimalloc's own; set
  inside `purge_freed_memory_promptly` it tagged nothing. It moves the daemon's heap from
  "IOAccelerator" to "App-Specific Tag 2" in `footprint`/`vmmap`; the footprint number does not
  change. `zig_optimize_mode` picks `ReleaseSafe` when cargo's `PROFILE` is `debug` (dev and
  test builds keep Zig's safety checks) and `ReleaseFast` for release-family profiles
  (`ReleaseSmall` at `s`/`z`), so `profiling` and `testflight`, which carry debug info, build
  the release engine; the xtask `profiling` override it replaced is deleted.
- The divergence is `formats.pane-current-command-exec-path` in `compat/tmux-gaps.json`.
  `compat/scenarios/smoke/plugin-runtime-resurrect-restore.txt` now restores under
  `default-shell /bin/sh`: with a faster CLI, restore.sh's `select-pane -T` lands before the
  restored bash prints its first prompt, and zz's shell integration retitles the pane at every
  prompt. The race was there before; the old CLI was slow enough to lose it most of the time.
  That tmux divergence (bash, zsh and PowerShell integrations) was recorded as the gap
  `terminal.shell-integration-prompt-title`, closed the same day: hook titles now carry a private
  `OSC 2626` mark, a `select-pane -T` title ignores marked titles until an unmarked OSC 0/2
  lands, and the row restores under the default bash again.
- The verbose sampler logs disk read and write bytes again (`proc_pid_rusage` on macOS,
  `/proc/<pid>/io` on Linux, sysinfo on Windows); the process `status` field is gone.

Measured with the quick wave1 gate (`--only cli,chatty,mem,cold,spawn`) on a loaded host (load
7-24), against the committed W0 JSONs (full / quick): `cli.wall.version.p1` 3.61 / 3.89 -> 1.8-2.2
ms (tmux 2.8-3.3), `mem.footprint.p1` 7.92 / 7.98 -> 7.42-7.47 MiB, `mem.footprint.p20` 54.3 /
53.9 -> 51.8-52.1 MiB, `mem.rss.p1` 18.9 -> 15.6 MiB. Per-command CPU is flat against W0 within
load noise (`cli.cpu.display.p1` 1.08 -> 1.04-1.26 ms, `spawn.cpu.kill_pane` 1.62 -> 1.56-1.61
ms); the per-command gain is in instructions: `cli.instr.display.p1` 10.79 -> 10.29-10.30 Minstr
(-4.6%), spawn -1 to -4%. `chatty.cpu_pct.flip` read 18.4, 19.7, 19.7, 20.4, 20.5, 20.6 and
21.8% across seven runs by three people, which is 9-23% under the full W0 value of 23.9%, so the
>= 15% rule is met in some runs and missed in others on this host; the same-run A/B against the
W0 binary is 27.2-28.6% -> 19.7-21.8% (-20 to -25%), with flip instructions 3276-3301 -> 3121-3162
Minstr/s (-4%). The `--strict` merge run on a quiet host decides the flip rule. The gate flags
`spawn.instr.split_empty_P` 11% over W0 (40.4 -> 44.8 Minstr); the W0 binary measures 46.7-46.9
in the same runs and the same new binary has read 39.9, so that is noise from the 1 ms poll in
the 2 s `wait_for_terminal_identity`, not this lane.

Left on this path after the change, as a share of busy daemon samples in a `sample` of the flip
workload: process lookups are about 5% (the `KERN_PROCARGS2` read for each new `sleep` 1.8%,
`TerminalSession::foreground_process_id` 2.0%, the cwd read 0.4%) and mimalloc 1.9%; the other
~90% is format, option and snapshot work owned by other lanes. `tcgetpgrp` is two ioctls under
the tty lock, and the watcher calls it twice per `ViewportReady` (current command, then cwd).
Both counts scale with how often the watcher asks, which W1-PUBLISH and W4-DELIVER own; one
foreground lookup per event passed to both lookups, and a bare `TIOCGPGRP` in zz-terminal
(W1-PANE), would halve the ioctls.

Linux follow-up (2026-09-29, alienware, THP `always`): the Linux fold gate read
`mem.footprint.p1` 16.1 MiB and `.p20` 103 MiB, against 6.1 and 26.1 on the Mac. The cause was
transparent huge pages in mimalloc's arena: a first touch in a 2 MiB-aligned region can fault a
whole huge page, a later partial purge splits it, and the 4K pages mimalloc never used stay
resident. It happened in about one start in four, depending on free huge pages, which is why it
looked like noise. The `no_thp` feature of `mimalloc` does not help: libmimalloc-sys 0.1.49
defines `MI_NO_THP`, which in mimalloc v3 only skips `MADV_HUGEPAGE`, while mimalloc's own CMake
also sets `MI_DEFAULT_ALLOW_THP=0`. `zz_cli` now registers a constructor in `.init_array.00100`,
ahead of mimalloc's (priority 101), that sets `PR_SET_THP_DISABLE` for the process, and the Linux
pane fork clears it again before exec, so pane programs keep the system setting (`run-shell` and
status job children still inherit it). Same-binary A/B with `ZZ_PERF_THP=1`: footprint p1
15.2 -> 3.1 MiB, p20 99-100 -> 18 MiB, detached throughput 90.6 -> 88-90 MB/s (unchanged within
noise), spawn instructions unchanged.

## W1-PUBLISH: change-driven publication (effort M)

Scope:
1. `KeyTables` generation from a process-global `AtomicU64` (whole-object replacement counts),
   bumped in `bind`, `unbind`, `update_binding_metadata`, `remove_table`, `ensure_table` (insert
   only), `set_prefix`, `set_prefix2`. Command name resolved at bind time so `snapshot()` does no
   catalog or prefix resolution. `publish_key_tables_if_changed` keeps the cheap reset pass and
   snapshots/compares only when the generation moved.
2. `refresh_status_filtered`: filter subscribers first; return before `set_format_now`,
   `state.snapshot()`, `format_hook_facts`, `format_option_snapshot` when none match.
3. Split `publish_mux_snapshots`: bookkeeping that always runs (`window_latest_clients.retain`,
   `release_chooser_zooms`, `terminal_appearance_updates` -> `set_appearance`, and
   `last_published_mux_generation`), then snapshot build and `stamp_snapshot_for_client` only when
   `subscribers` is non-empty.
4. `set_pane_runtime_facts_with_hooks` (zz-mux command.rs) bumps the tree generation only when a
   field of the unstamped snapshot changes (the automatic window name); a separate runtime-facts
   counter serves daemon readers. On a runtime-facts change, re-stamp each subscriber's labels
   (`expand_window_status_labels`, `stamp_pane_border_chrome`, `stamp_pane_border_colours`) and
   send only when the stamped bytes differ from the last send to that client (per-client hash).
   Optional shortcut: skip the re-stamp when no window-status, border or status template
   references runtime facts (scan cached per options generation). Re-stamp also on the
   status-interval tick. No "every bump changed the body" assert.
5. `refresh_automatic_window_name_for_pane`: `NAME_INTERVAL` 500 ms throttle with a Rename
   deadline for the remainder.
6. `synchronize_pane_runtime`, `synchronize_pane_title`, `rename_window_from_pane`: `mark_dirty`
   + leading-edge flush + at most one trailing flush per 16 ms (PublishFlush deadline). The
   command path still publishes at the end of each command.
7. One `zz-daemon-timers` thread with one deadline heap replaces
   `start_display_panes_deadline_dispatcher`, `start_key_table_deadline_dispatcher`,
   `start_silence_deadline_dispatcher`, `start_client_message_deadline_dispatcher`; schedule APIs
   and channel types unchanged. **The timer thread only posts expirations**: hook and command work
   (`expire_window_silence` -> `run_event_hooks` alert-silence, which can run a synchronous
   `run-shell`) runs on a worker or the command path, so a hook cannot stall repeat-time, message
   clears, display-panes, Rename or PublishFlush.
8. `start_status_sampler`: skip the tick when no status-interval client, control subscription or
   format monitor exists. `sync_claude_peer_states` runs only while some registry record's pid is
   in a pane's process tree, and detects change by per-file mtime (N stats per tick) or kqueue
   `EVFILT_VNODE` / inotify on the directory and files. Directory mtime alone misses in-place
   status rewrites.

Keep signatures of `publish_snapshot`, `publish_snapshot_state`, `publish_key_tables_if_changed`.

Write zone: zz-protocol key.rs `KeyTables`; zz-mux model.rs generation helpers; command.rs
`set_pane_runtime_facts_with_hooks`, `refresh_automatic_window_name_for_pane`; daemon.rs
`initialize_with_mux_config_files`, the four `start_*_deadline_dispatcher`,
`expire_window_silence` call path, `refresh_status_filtered`, `start_status_sampler`,
`sync_claude_peer_states`, `publish_key_tables_if_changed`, `synchronize_pane_title`,
`rename_window_from_pane`, `synchronize_pane_runtime`, `publish_snapshot`,
`publish_snapshot_state`, `publish_mux_snapshots`; new `daemon/timers.rs`; agent/claude_peers.rs
scan entry; `compat/wire-version.py` `NOT_WIRE`.

Gate vs FOOTPRINT JSON: `config.wall.source_1000` <= 90 ms; `chatty.cpu_pct.flip` <= 8%;
`cli.cpu.display` >= 11% lower; fixed service threads 9 -> 6; 4-pane 300-line TUI workload <= 12
Snapshot and <= 10 StatusChanged per 3 s (today 244 and 55). Tests: key-table tests,
`reload_user_config_resets_key_tables_to_the_file`, status / display-panes / key-table / silence /
client-message timing tests (re-run alone on failure), hook order, runtime-fact label rows, detached
window-style, timer-stall, in-place peer status. Compat: `compat/run.sh` (automatic-rename,
pane_current_command, hooks), `compat/status-row.sh`, `compat/tui-indicators.sh`,
`compat/tui-choosers.sh`.

Expected: key tables 11-21% of every command and 59% of config replay; status/snapshots with no
subscriber 52-53% with 10 panes printing; config 272 -> ~70 ms alone; flip CPU 24% -> 6-8%.

As built (branch `perf/publish`), where it departs from the scope above:

- Wire names are resolved on a binding's first snapshot and kept with the binding (`OnceLock`),
  not at bind time, so client-side tables that never snapshot (which-key, chrome keymaps) pay
  nothing. Config replay holds key-table publication on its thread
  (`KeyTablePublishHold` in `replay_config_file_in_queue`), so a file with 300 `bind-key` lines
  snapshots and publishes once, at the end of the outer command. The hello reuses the last
  published snapshot while the generation matches.
- The per-client check covers every Snapshot send, not only runtime-fact ones: the stamped
  snapshot's postcard digest plus its generation, recorded by `publish_mux_snapshots`,
  `send_attached` and `send_resync_inner`. A send whose content changed under an unchanged
  generation bumps the tree generation first, so the GUI's `AppRevision` still sees a new one,
  and moves the records of clients whose content did not change to that generation too. A
  split drag clears the dragging client's record, because the GUI drops its local layout
  prediction only on a fresh Snapshot. `publish_mux_snapshots` and `send_resync_inner` record and
  enqueue under one `snapshot_order` lock, so two publishers cannot leave a client holding an older
  Snapshot than its record says. A full publish and a runtime-fact flush build one tree snapshot
  and one set of format hook facts under one lock, shared by every client's stamped snapshot and
  every status request; before, each client's stamp and the status refresh built their own. A
  timer rename still builds facts once for the rename itself, because the publish that follows
  runs after the lock is released.
- Background publishers (a runtime-fact flush, the clock-label tick) go through
  `publish_mux_labels`, which never claims `last_published_mux_generation` for a tree change it
  did not publish. A command whose mutation lands before such a flush still runs its own
  `publish_snapshot` at the end, with the visibility, session-cleanup and chooser refreshes that
  only the full publish does; the Snapshot itself is not sent twice.
- Pane events publish with a reason. A tree change (rename, title) runs `publish_snapshot`; a
  runtime-fact-only change runs the stamped snapshot and status refresh only when
  `MuxEngine::runtime_facts_reach_presentation` finds a template that reads runtime facts
  (status, window-status, border, window-style and set-titles options; `@`, `E:`, `T:`, `O:` count
  as reads), and refreshes open choose-trees. There is no scan cache: the scan borrows the option
  tables and costs less than one status request. The status-interval tick re-stamps only when a
  window label or border template follows the clock (`window_labels_follow_the_clock`: `%`,
  `#(`, `t:`, `E:`, `T:`). A missed template degrades to tmux timing, the next tick or tree change.
- The rename throttle covers renames driven by pane runtime facts only. Command-path renames
  (select-pane, new-window, kill-pane, `set automatic-rename`) stay immediate, as the compat corpus
  expects. It is an engine flag (`set_automatic_rename_throttle`) the daemon turns on, so zz-mux
  unit tests keep immediate renames. Hook facts are built only when a rename is due; the daemon's
  "due" check and the engine's rename read the same instant (`set_pane_runtime_facts_at`), so a
  rename on the 500 ms boundary cannot run with empty hook facts. No separate runtime-facts counter
  exists: nothing read it. `compat/rename-timing.sh` compares the window-renamed hook rate and the
  settled name against the pin.
- The timer thread selects over the four unchanged deadline channels plus one `TimerCommand`
  channel (Rename, PublishFlush). Silence and rename hook events run inline when no hook has
  commands, else on a `zz-daemon-hooks` thread spawned on demand that exits when its queue is
  empty, so FIFO order holds and an idle daemon has no extra thread. Each expiry runs under
  `catch_unwind`, and a full publish clears a pending flush, so one bad handler cannot stop the
  other timers or leave publishing stuck behind a flush that never runs.
- The 1 s status tick runs only while a control client has subscriptions, a `set-hook -B` monitor
  exists, or a pane holds a Claude peer state. `status-interval` refreshes keep their own
  per-session deadlines. With none of these the sampler parks with no timeout; `subscribe`, the end
  of every command and pane output wake it when there is work.
- The peer scan is armed from the registry side. Pane output and runtime-fact changes ask for a
  probe (one atomic flag, one unpark per probe), and the sampler runs at most one
  `sync_claude_peer_states` a second while they continue and no state is recorded, plus one more
  a second after they stop, so a record the agent writes just after its last output is found. A pane whose root process is the agent
  (`split-window claude`, `exec claude`, `sh -c 'claude; ...'`) is found on its first output, the
  same as a foreground job; once a state is recorded the 1 Hz scan runs until it clears. There is
  no per-event `tcgetpgrp`. `RegistryCache` in claude_peers.rs re-reads a record only when its
  (mtime, size, inode) changes, or when it was read within 2 s of its mtime (a coarse clock can
  hide a same-length rewrite, as git's racy-clean check), and re-lists the directory the same way.
- Merged onto FOOTPRINT and FORMAT, the command path got fast enough that `display-message` right
  after `copy-mode` ran before the terminal actor entered the mode (`#{pane_in_mode}` read 0 in
  compat `smoke/copy-mode-formats`). A command that sends copy-mode view actions now ends with one
  `TerminalSession::settle` request per terminal, answered once the actor has applied them
  (`a_copy_mode_command_is_visible_to_the_next_command`).

Measured at `--quick` on a loaded host (load 7-22 on 16 CPUs). Before is the W0 quick JSON
(`baseline-quick-macbook-17e17115.json`), which a fresh 157ac6a3 build reproduces: fixed service
threads 9 -> 6 (main, async-io, signals, accept, timers, status); `mem.threads.p1` 13 -> 10;
`cli.instr.display.p1` 10.8 -> 7.4 Minstr, `.p20` 29.1 -> 25.8; `chain5.p1` 20.4 -> 9.3, `.p20`
43.8 -> 32.8; `config.instr.source_1000` 3640 -> 749 Minstr (48 ms CPU, was 228);
`chatty.instr_per_s.flip` 3206 -> 1584 Minstr/s; `chatty.instr_per_s.hidden` 7295 -> 1914;
`chatty.tty_kibps.hidden` 235 -> 8.7 KiB/s; `idle.wakeups_per_s.p20` 1.2 -> 0.2;
`attach.instr.p1` 123 -> 92. The 4-pane 300-line TUI workload stays under 12 Snapshots and 10
StatusChanged per 3 s (`a_busy_tiled_window_sends_few_snapshots_and_status_lines`). After the
second review pass (one snapshot and one facts build per publish) the same gate at load 22-30
reads `chatty.instr_per_s.flip` 1548, `.hidden` 1868, `attach.instr.p1` 87.5 and `.p4` 93.6,
`cli.instr.display.p1` 7.33, `config.instr.source_1000` 748, with 0 regressed and 0 drifted rows.
`chatty.cpu_pct.flip` reads 13-18% at load 7-22 and the 8% gate is not met by this lane alone.
A symbolized flip profile after the review fixes puts this lane's path (`synchronize_pane_runtime`
bookkeeping, `request_publish`, the peer probe) at about 3 of ~700 busy samples; the rest is
`terminal_current_command` / `terminal_working_directory` through sysinfo and `proc_pidinfo`
(W1-FOOTPRINT, ~290) and `publish_active_views` building frames with no view (W1-PANE, ~380).
The gate is met when those two lanes land, and has to be re-measured then.

## W1-FORMAT: lazy universe, option index (effort M)

Scope:
1. `tmux_options.rs`: `LazyLock` index `HashMap<&'static str, {TmuxOption, is_consumer}>` from
   `tmux_option_names()` and aliases; `exact_tmux_option`, `exact_tmux_option_name`,
   `build_tmux_option` read it; `parse_format_option` does one probe;
   `extend_format_option_values` iterates a per-scope consumer list.
2. **Universe as `Built(Arc<FormatUniverse>) | Deferred(&'e MuxEngine)`**, deferred only inside a
   lock scope. Parts (loop items, option rows per scope, environment + window user options) fill
   on first read. `StatusContext.format_universe` changes type from the owned Arc. `detach()`
   builds the parts a template scan needs at **every escape point**. Any part not built and read
   off-lock renders empty with no error, which is the bug this rule prevents. No existing lazy
   path to reuse: `FormatContextSnapshot` builds on the first `status_context()` call.
   As built, the engine borrow is the `'e` of `StatusContext<'e>`, so the compiler finds every
   escape: only the status request and the mode requests leave the lock. Border presentations,
   chooser rows, control subscriptions, format monitors and hook bodies expand inside it on one
   deferred universe per client per call.
3. `expand_format_inner`, `expand_format_with_hooks`: return input unchanged when it has no `#`
   (and no `%` when time expansion is on); lazy `option_fallback`.
4. `list_keys`, `list_commands` resolve once; `list_sessions`, `list_windows`, `list_panes`
   (incl. `-f`, `--json`) share one lazy universe per command.
5. `build_status_context`: no `WindowOptions` clone in `window_knobs`; known `WindowId` for
   history-limit and synchronize-panes lookups; layout dumps once per window per batch.
6. `match_value`: 64-entry regex cache keyed by pattern and case flag.
7. daemon.rs: `pane_format_geometry` helper for `mouse_pane_cell`, `mouse_format_variables`,
   `popup_position_variables`, `client_viewport_facts`. `client_feature_mask` /
   `client_colour_count_with` cache terminfo values per client **keyed on TERM, client features,
   `terminal-features` and `terminal-overrides`** (or an options generation).
   **`format_hook_facts` keeps terminal handles**: `inner.terminals` becomes an `Arc<BTreeMap>`
   updated copy-on-write, and facts take a refcount. `border_presentations`, `mode_request`,
   `refresh_control_subscriptions`, `run_format_monitors`, chooser `client_chooser_rows` share one
   lazy universe per client per call.
8. `seed_session_environment`, `apply_client_environment_update`: compile update-environment
   patterns once per pattern-set change.

This lane owns `status.rs` facts readers and `daemon/chooser_presentation.rs` in wave 1. Keep
signatures of `status_request`, `format_hook_facts`, `format_hook_facts_for_client`,
`client_format_facts`.

Write zone: formats.rs (except the time functions FOOTPRINT already changed); tmux_options.rs
lookup functions + index; command.rs `parse_format_option`, `StatusRowVariables::lookup`,
`format_option_snapshot`, `extend_format_option_values`, `format_option_value`,
`format_option_target`, `window_knobs`, `window_size`, `history_limit_for_pane`, `list_*`,
`format_option_rows`, `seed_session_environment`, `apply_client_environment_update`; daemon.rs
`refresh_control_subscriptions`, `run_format_monitors`, `client_feature_mask`,
`client_colour_count_with`, `client_format_facts`, `mouse_format_variables`,
`client_viewport_facts`, `status_request`, `border_presentations`, `mode_request`,
`popup_position_variables`, `mouse_pane_cell`, `format_hook_facts`, `format_hook_facts_for_client`,
`ServerState.terminals` type; status.rs facts readers; chooser_presentation.rs.

Gate vs PUBLISH JSON: `cli.*.list_keys.*` wall <= tmux at p1 and p20 with CPU <= 3 / 5 ms;
`cli.wall.list_panes_a.s20` <= tmux; `cli.cpu.list_windows_a.s20`, `cli.cpu.list_sessions.s20` CPU
<= 1.2x; `config.wall.source_1000` <= 25 ms. Tests: zz-mux format tests incl.
`snapshot_contexts_share_universe_and_match_independent_formats`,
`name_checks_and_nested_scope_loops_use_the_engine_universe`, the 198-entry vocabulary pin;
index-vs-scan equality for every name and alias; lazy-vs-eager differential over direct expansion
**and every escape point** (1 pane, 20 panes, several sessions with marked, zoomed and dead panes);
terminal-overrides refresh; `compat_manifest_tests.rs`, `format_modifier_client_loop.rs`,
`compat/run.sh`, `compat/tui-mouse.sh`, `compat/tui-pane-geometry.sh`.

Expected: universe 95% of list-keys CPU; list-keys 17 -> ~1.6 ms (1 pane), 78 -> ~4 ms (20);
list-* linear, not quadratic; option lookups 470 of 569 `format_option_snapshot` samples; regex
52 of 344 in the 20-pane status render.

As built (branch `perf/format`):

- `StatusContext<'e>` is `StatusValues` (the table values, reached through `Deref`) plus a
  `FormatUniverseRef<'e>`: an `Arc` part cache and the engine that may fill it. Loop items store
  their values once, share one empty universe handle, and loops walk them by reference.
  `StatusContext` has no `PartialEq`; tests compare values and expanded output.
- `detach(needs)` fills the parts the scan asked for over everything a loop item can reach: an
  `S` item carries its session's active window and pane, and a `W` item its window's active
  pane, so those windows' pane lists and option rows are built too. Lookups of entities that do
  not exist are cached as absent. A detached handle that misses a part it was not filled with
  trips a `debug_assert` (per part, not per kind) unless it was filled with everything.
- The template scan is `MuxEngine::format_needs`: loops, `N:`, `O:`, `V:`, and any name that
  is neither a table name nor an option with a global value (environment fallthrough). `E:` and
  `T:` on an option follow its value at every scope; on anything else, or on an option some
  context lacks, the scan asks for everything. A status refresh scans once per session, not per
  client.
- A status job's output is only known to the renderer. Each output is scanned once when it
  arrives, and `poll_jobs` records the union per client in a leaf-locked map
  (`StatusJobNeeds`) that `status_request` reads. An output that arrives between a request and
  its render is not expanded on a universe that lacks its parts: the render is not published,
  the client is marked, and the sampler's next `poll_jobs` refreshes it with the new needs.
- `pane_format_geometry` is an engine method sharing `format_target` with context resolution.
- Found by profile and removed as well: `FormatFacts` user options are shared copy-on-write
  maps, so a fact snapshot per command takes refcounts instead of copying every `@option`;
  built `TtyTerm`s are cached by their five inputs, looked up by borrowed values, and handed out
  as `Arc`s; `update-environment` patterns compile once per array (`GlobPattern`); the option
  index hashes with foldhash; a status refresh with no matching subscriber returns before it
  builds the snapshot, the facts and `format_option_snapshot`.
- `ZZ_PERF_EAGER_UNIVERSE=1` fills every part on creation and detach; `with_eager_universe`
  flips it per thread and is the oracle of the differential tests in
  `crates/zz-mux/src/format_universe_tests.rs` and
  `crates/zz-daemon/src/daemon/format_universe_tests.rs` (loop compositions inside `S`/`W`,
  status requests with partial needs, and a status job whose output needs a part the templates
  do not).
- Measured with `--quick` on the lane base (157ac6a3) and on the lane head (instructions are
  stable to 1% across hosts and loads; wall and CPU are not): `list-keys` 249 -> 25.1 Minstr at
  p1 and 1128 -> 28.6 at p20; `list-panes -a`, `list-windows -a`, `list-sessions` at s20
  ~123 -> ~14 Minstr each (tmux 4.5, 2.9, 30.6); `display-message` 10.8 -> 7.4 (p1) and
  29.2 -> 11.0 (p20); 1000-line `source-file` 3647 -> 2530 Minstr; `chatty.cpu_pct` flip
  34 -> 16%, hidden 69 -> 22.5%; `control.instr_per_cmd` 7.66 -> 2.23;
  `statusjob.instr_per_s` 59.6 -> 15.4; `attach.instr.p1` 123 -> 42.

Still failing at the lane head, on a quiet host (load 7 on 16 CPUs): `cli.wall.list_keys` 1.04x
(p1) and 1.06-1.08x (p20) tmux against 1.0; `cli.wall.list_panes_a.s20` 1.46-1.49x;
`cli.cpu.list_windows_a.s20` 7.5-8.9x and `cli.cpu.list_panes_a.s20` 6.6x against 1.2x;
`cli.cpu.list_sessions.s20` 1.14-1.26x (borderline); `config.wall.source_1000` 170 ms against
25 ms. `list-keys` CPU passes (2.15 ms at p1, 3.33 ms at p20). Format work is not gone from the profiles (shares of busy daemon samples):

- `display-message` p20: about 15%, all for the hello status of a one-shot command client
  (`register` -> `status_request` -> `render_initial`). EXEC's `Exec` path has no hello, which
  removes it.
- `list-windows -a` s20: `list_windows` 15.5% (`PreparedFormat::expand` 8.2%,
  `resolve_batched` 5.7%), about 2.2 Minstr, roughly tmux's whole 2.9 Minstr command. Key
  tables are 66-70% of the samples (PUBLISH); without them about 0.67 ms CPU remains against a
  0.36 ms budget, so the `list_*_a` rows also need W2-FMT's compiled templates and borrowed
  handles, and EXEC's one-frame command.
- `list-keys` p20: per-row template parsing is 25-30% (`expand_replacement`, `split_once_top`,
  `pad_value`, `from_modifiers`); zz is already 0.54x tmux on instructions. W2-FMT's op-list
  cache removes it.
- hidden chatty: `refresh_status_filtered` 20.5% (`status_request` 6.7%, render 6.2%,
  `format_option_snapshot` 5.3%, `format_hook_facts` 3.6%, the scan 3.1%). The refresh rate is
  PUBLISH's; the scan and the snapshot per refresh need an options generation to cache across
  refreshes, which W2-FMT introduces for its template cache.
- 1000-line config replay: key-table snapshots are 90% of what is left (PUBLISH, about 16 ms
  once it lands). A `set @x` clones that scope's user option map once, because the command's
  own fact snapshot holds a refcount while the command runs (1.4% at 400 `@options`); before
  this lane every command copied every map. W2-FMT's borrowed facts remove the snapshot.

`attach.tty_total.p1` goes over its 1.05 tolerance in 2 of 3 quick runs (170-179 KB against
W0's 162 KB). The status content is the same; the faster hello changes the order in which the
TUI paints its "waiting for frame" placeholder (11 paints instead of 9, then a full redraw).
W1-ATTACH item 5 (never paint an empty pane before its first frame) removes the placeholder
paints; until it lands, the merge run should expect this row to be order-dependent.

## W1-PANE: frames for watchers, compression, threads, spawn (effort L)

Scope:
1. No streamed view: `publish_active_views` builds no fallback snapshot, refreshes only the
   metadata of the stored latest viewport (title, OSC 7 cwd, status, size, modes, mouse, cursor,
   progress), keeps the old cell plane. `latest_viewport()` keeps its signature.
2. `Command::SetViewStream(view, Foreground|Preview|Off)` from `refresh_terminal_visibility`.
   **The streamed set includes Preview for every pane shown in a choose-tree/choose-client
   preview, display-panes, popup, GUI sidebar preview, and the pane a `#{C:}` template reads**
   (the active pane of each window in an attached status line), so `chooser_presentation.rs`,
   status.rs `pane_search` and the popup first frame read fresh cells without a round trip. A full
   frame is published when a view starts streaming. Copy-mode facts keep updating for
   non-streamed views in copy mode. `watch_terminal`, `publish_terminal_for_pane` skip diffs for
   non-streamed views.
3. `TerminalSession::fresh_viewport()` (actor round trip) exists for first frames and tests only.
   **Forbidden while holding `inner` or the status mutex**; debug-assert through a thread-local
   marker set around command execution and status render.
4. Last view deactivates: drop `RenderState`, row/cell iterators, cell pools; recreate on
   activation.
5. Idle compression in `run_terminal`'s wait block and `output_view_worker`: `compress_due` 1 s
   after `compression_activity()` changes, then `Terminal::compress(Incremental)` in <= 2 ms steps;
   re-arm after `capture-pane -S -`, HistoryChunk, `ModeRevision::capture`, search snapshots;
   `Unsupported` is a no-op. Time one page restore with the probe and record it.
   **Line-count pruning stays the only history limit**: `set_scrollback_max_bytes` (today `None`
   in `new_terminal`) is set only as a backstop at 4x the estimate history-limit x cols x 10 B,
   capped at 256 MiB.
6. Threads: `SearchWorker` spawned on first search. Child exit via kqueue `EVFILT_PROC NOTE_EXIT`
   in `wait_for_wake` (Linux: pidfd in the poll set, thread fallback before 5.3). **Register
   before the first wait; on `ESRCH` go straight to `waitpid(WNOHANG)`; remove portable_pty's
   `Child::wait` path so exactly one owner reaps.** The dead notice still follows the final output
   drain. Linux `zz-pty-gather` stays.
7. Spawn: `openpty` + `ptsname_r` / `TIOCPTYGNAME`, not portable_pty's `ttyname_r` /dev scan
   (690 vs 8.5 us). Word separators, allow-passthrough, wrap-search go into `TerminalSpawn` and a
   settings slot, not blocking sends on the capacity-1 command channel
   (`MAX_PENDING_ACTOR_COMMANDS = 1`). View setup uses `try_send` with a coalescing slot so
   `DeferredTerminalCommand::run` never parks the command thread.
8. Identity: pid and tty published through a one-shot Condvar after spawn; empty and surface
   sessions resolve immediately; `wait_for_terminal_identity` waits on it (2 s cap kept).
9. Views: `TerminalSession` records views the actor has seen; `release_view` / `detach_view` for
   an unknown view send nothing.
10. `is_current_terminal` reads a per-pane epoch without `inner`.
11. `shell_integration.rs`: `resource_root` in a `OnceLock` keyed by cache root, content-hash
    directory, no `sync_all`. `ActorWake::notify` does not log ERROR while the actor terminates.
12. **Echo fast path**: in `run_terminal`, publish at once, bypassing `CONTENT_PUBLISH_STALENESS`
    (16 ms), when PTY input was written in the last ~50 ms.

Write zone: zz-terminal session.rs `PublishedViewports`, `TerminalSession::spawn*`, identity fns,
`latest_viewport*`, view API, `Command`, `command_channel`, `CommandSender`, `ActorWake::notify`,
`new_terminal`, `run_terminal` (not the retained-pane tail), activate/deactivate/release view actor
fns, `SearchWorker`, `wait_for_wake`, `publish_active_views`, `snapshot`, `output_view_worker`;
shell_integration.rs `configure_default_shell`, `resource_root`, `materialize_resources`,
`write_resource`; daemon.rs `send_full`, `watch_terminal`, `is_current_terminal`,
`refresh_terminal_visibility`, `publish_terminal_for_pane`, `DeferredTerminalCommand::run`,
`wait_for_terminal_identity`. Not chooser_presentation.rs or status.rs (FORMAT owns them).

Gate vs FORMAT JSON: `spawn.wall.split_empty_P` <= 6 ms; `spawn.wall.split_shell` <= 6.5 ms;
`chatty.cpu_pct.steady` <= 3%; `mem.footprint.scroll180` / `scroll80` <= 1.0x / 1.15x tmux;
`mem.threads.p20` <= 51; `mem.footprint.p20` <= 45 MB; `echo.p99.busy30` <= 1.5x
tmux; throughput rows; `attach.ttfc.p1` no worse. Tests: zz-terminal history, capture, copy-mode,
snapshot tests; daemon exit / remain-on-exit / `-P` / pane_pid / pane_tty / word-separators /
allow-passthrough / wrap-search tests; exec client that ran copy-mode and capture-pane leaves no
view; styled wide lines fill history-limit exactly; `split-window 'true'` exit status; chooser
preview of a hidden printing pane is current. Compat: `compat/run.sh`, `tui-copy-mode.sh`,
`tui-screen-diff.sh`, `tui-output-backpressure.sh`, `tui-pane-geometry.sh`, `tui-choosers.sh`;
`bench/run.sh`.

Expected: snapshot builds 71% of detached-pane CPU; RenderState 0.72 MB per 180x50 pane;
`ttyname_r` 0.86 ms per spawn; 2.6 ms command-thread block per split; 2 s identity poll; threads 4
-> 2 per pane; pages 288 MB -> 13-24 MB for 20x12k lines.

Built on `perf/pane` (2026-09-28, on 157ac6a3, before the W1-FOOTPRINT merge). Where the build
departs from the scope above:

- Item 1: `publish_views` (behind `publish_active_views`) builds cells only for views whose
  stream is on, for views holding copy or view mode (their copy-mode facts keep updating, plus
  one frame when they leave it), and for the fallback when a caller forces it (exit, dead notice,
  frozen output views, `fresh_viewport`) or a preview watch is on. Otherwise
  `Publisher::refresh_fallback` copies the stored fallback with the title, OSC 7 directory,
  status, scrollbar, mouse tracking and kitty keyboard flags read again; a new size, a mode, an
  overlay, a search or a scrolled viewport makes it build instead. Cursor and progress are not
  on this path: `#{cursor_x}` and friends come from `TerminalFacts` and the progress bar from its
  own slot, both refreshed on every loop turn. The fallback of an unwatched pane is rebuilt once
  its output has been quiet for 100 ms and at least once a second under continuous output
  (`settle_unwatched`), which bounds how old the cells `#{C:}` reads can be.
- Not in the brief: a pane nobody streams (no stream, no preview watch, no view in a mode)
  publishes on the leading edge of output and then every 100 ms (`UNWATCHED_NOTIFY_INTERVAL`)
  instead of every 16 ms, and wakes its watcher on the same beat (`Frames::admit_notify`). The
  engine filter flags the sequences that change what the metadata refresh reads (OSC 0, 1, 2 and
  7, the mouse modes, the kitty keyboard stack, `DECSTR`, `RIS` and the title stack), and one of
  those, an exit, synchronized output ending or an echo publishes at once. Before review the
  actor still ran the whole publish step every 16 ms for such a pane (in the flip profile 112
  busy samples against 5 for parsing). The synchronized output deadline now lives in the
  actor's `Frames` instead of the shared lock, copy-mode facts go out with the frame under one
  write lock, and `TerminalFacts` are written only when they change. `pane_current_command`,
  activity and silence times of an unwatched pane are at most 100 ms older; bells and exits are
  separate events and are not delayed.
- Item 2: the streamed set the daemon already kept (`streamed_terminal_panes`: the visible
  window's panes as Foreground, a GUI sidebar's session panes as Preview) now drives
  `set_view_stream` through `apply_view_streams` inside `refresh_terminal_visibility`, and
  `detach_client_state` turns a leaving client's streams off. Turning a stream on for a view that
  has no published frame starts a new stream epoch, and the watcher sends the first frame of an
  epoch whole (`latest_view_frames` carries the epoch). Choosers do not attach views: the actor
  keeps its fallback cells current while `set_preview_watch` is on, and
  `refresh_preview_watches` turns it on for the panes the selected choose-tree row previews (a
  session's window active panes, a window's panes, a pane, a client's pane), re-evaluated on
  every chooser presentation and snapshot publish; once the first current frame of a newly
  watched pane lands the watcher redraws the choosers (`take_preview_ready`). Popups stream their
  own view. display-panes reads no cells. `#{C:}` is not in the streamed set: status.rs belongs to
  W1-FORMAT and a template scan would miss `#{E:}` indirection, so `pane_search` reads the settled
  fallback (at most 100 ms after output stops, 1 s under a flood; tmux reads the grid live).
- Item 3: `forbid_actor_round_trips` is set in `execute_with_mux_source_routed_for_terminal_in_queue`
  and around the status render in `refresh_status_filtered`, and the debug assertion sits in
  `CommandSender::request`, so every blocking actor call (capture, history, pointer context,
  kitty images, output taps, copy source) is checked, not only `fresh_viewport`, which has no
  production caller. The call sites that wait on an actor during a command after releasing
  `inner` lift the guard with `allow_actor_round_trips`: `capture_pane`, `pipe_pane` and the tap
  start, rearm and stop helpers, `capture_last_command_for`, `capture_screen`, the kitty image
  fetch and eviction, `pointer_format_variables` and the copy-source capture in
  `DeferredTerminalCommand::run`.
- Item 4: `Frames` owns the render state and its iterators and drops them, with the dictionary
  pools, when no view streams and no preview watch is on. A settle rebuild keeps them for 1.1 s
  after it runs, so a pane that keeps printing reuses one render state across its rebuilds, and an
  idle pane lets it go a second after its last rebuild.
- Item 5: compression as briefed, in both actors, re-armed after capture, copy-source capture,
  history chunks, semantic capture, view actions and search refreshes. The byte backstop departs
  from the brief's formula: 40 bytes a cell (4x of 10) cut history below `history-limit` for text
  with combining marks, which costs about 58 bytes a cell in ghostty's pages (history-limit 2000
  at 180 columns kept 1367 lines, tmux keeps them all). It is now history-limit x cols x 128
  bytes, at least 64 MiB and at most 1 GiB, so the line count is the only limit for any text up
  to about eight combining marks a cell; a test fills wide and narrow panes with combining marks. Measured restore: after idle
  compression, `capture-pane -S - -E -` of a 180x50 pane holding 9.9k lines of `seq` costs 74.8
  Minstr (5.3 ms daemon CPU) the first time and 64.6 Minstr after, against 64.2 Minstr with
  `ZZ_PERF_NO_COMPRESS=1`, so restoring about 46 pages is 10 Minstr (about 30 us a page).
- Item 6: the macOS child exit is a kqueue with `EVFILT_PROC NOTE_EXIT` in `wait_for_wake`'s poll
  set (`ChildExitWatch`). `NOTE_EXIT` can fire before the child is reapable, so after the event
  the actor waits for it with a blocking `waitpid` on that pid; `ESRCH` at registration goes
  straight to `waitpid`. A child that outlives its actor (a shell that ignores `SIGHUP`, a kill
  wait that ran out) is reaped by a short-lived `zz-child-reap` thread from `Drop`, where the old
  per-pane `zz-child-wait` thread reaped it. On shutdown (kill-pane drops the session) the actor,
  which is exiting anyway, waits up to 500 ms for the child it just sent `SIGHUP`, so the reap
  thread is left for children that survive it; before review nearly every kill started one. Linux watches a pidfd in `zz-pty-gather`'s poll set
  and keeps a `zz-child-wait` thread only when `pidfd_open` is missing. portable-pty's `Child` is
  gone on unix; Windows keeps it.
- Item 7: `session/unix_pty.rs` opens the pty with `posix_openpt`, `grantpt`, `unlockpt` and
  `ptsname` (rustix, `TIOCPTYGNAME` on macOS). It resolves the program and builds argv and the
  environment the way portable-pty's `spawn_command` did (same `PATH` search and errors, login
  `argv[0]`, `SHELL`), all before the child exists; the environment comes from the command
  builder's one snapshot of the process environment, with the process's own keys listed once per
  process so a value that is not UTF-8 still passes. On macOS the daemon binary is its own pane
  launcher: `posix_spawn` starts it with `--zz-pty-exec` (`POSIX_SPAWN_SETSID`, the slave on fds
  0-2, `POSIX_SPAWN_CLOEXEC_DEFAULT`, every signal reset to default, an empty mask,
  `posix_spawn_file_actions_addchdir_np`), and `run_pty_exec_mode`, which `zz_cli` calls first
  thing in `main`, resets its signals again (the Rust runtime ignores `SIGPIPE`), claims fd 0 as
  the controlling terminal with `TIOCSCTTY` and execs the program, retrying `ENOEXEC` under
  `/bin/sh`. `posix_spawn` alone cannot do it: nothing in it makes the slave the controlling
  terminal (a C probe with `POSIX_SPAWN_SETSID` and the slave opened by a file action still gets
  `ENXIO` from `/dev/tty`), and only bash grabs one on its own, so the plain `posix_spawn` build
  this lane first shipped left zsh, dash and directly exec'd programs without one (^C did
  nothing, job control was off, `/dev/tty` failed; found in review). `fork` gets the tty right but
  is worse than its 0.48 ms on macOS: memory the daemon writes after a fork stays in its footprint
  even after `madvise(FREE_REUSABLE)`, so idle compression and mimalloc purges stop returning
  anything (20 filled 180-column panes: 333 MiB with `fork`, 59 MiB with the launcher, 344 MiB
  before the lane), and each split costs about 6 Minstr more. Linux, other hosts of zz-terminal
  (tests, a GUI binary without the `cli` sibling) and a launcher that fails to start fork
  instead: the child resets every signal, calls `setsid` and `TIOCSCTTY`, puts the slave on fds
  0-2, closes inherited fds (`close_range` on Linux, a `proc_pidinfo` list on macOS) and execs.
  Signals stay blocked from before the fork until the child has reset them, and a signal to the
  pane's group that finds no group yet goes to the pid, so a terminate right after the spawn is
  not lost (a popup test read `SIGKILL` for `SIGTERM` under load). Either way the pid is known at
  once and published as the identity, and the actor waits on an exec fence (a close-on-exec pipe,
  at most 500 ms) before its first frame, so the watcher's first runtime sync names the program
  rather than the launcher or a fork of the daemon. Tests run `sleep` and `zsh -f`
  directly and check that a typed ^C ends them, a daemon test checks job control,
  `#{pane_current_command}` and `C-c` in an interactive zsh pane, and a `zz_cli` test does the
  same against the real daemon (so through the launcher) and checks the command name of five
  directly exec'd panes; it fails without `TIOCSCTTY` and without the fence. Pane settings travel in
  `TerminalSpawn`, and every setting and view change (`set_word_separators`, `set_appearance`,
  `set_allow_passthrough`, `set_wrap_search`, `set_engine_knobs`, `resize`, attach, detach,
  release, stream, preview watch) goes through `ControlSlot`, which coalesces per key and per view
  and never parks the caller. Order is kept: the handle counts control commands in flight, a
  change made while nothing is queued is applied before the next command, and a change made while
  commands are queued waits in a deferred list until those commands have run (so a copy-mode
  cancel followed by a detach cancels first, and `send -X` followed by `set word-separators` runs
  in that order).
  Empty panes take word separators and wrap-search through the slot at creation.
- Items 8-12 as briefed, with these details: views are recorded on the handle side
  (`ControlSlot::known_views`), not by the actor; `is_current_terminal` reads a per-session
  `retired` flag the daemon sets whenever it drops or replaces a pane's terminal; the resource
  directory is `v1-<FNV-1a of the scripts>`, a cache root is materialized once per process, and
  each spawn checks that the three scripts still exist (three `stat` calls) and writes them again
  when a cache purge removed them; the echo window is 50 ms and allows four immediate publishes
  per input.
- The pane watcher now sends frames before the runtime sync, so an echo no longer waits for the
  process lookup and `synchronize_pane_runtime`. Attach, detach and release publish only when
  the view streams, holds a mode or had a frame, so an attach does not build a frame that the
  stream then builds again. A pane told to terminate (kill-pane, respawn-pane -k) no longer
  forces a full exit frame nobody reads; its exit status still reaches the fallback.
- The foreground group is read with `libc::tcgetpgrp` and 0 means none; rustix asserted a
  positive pid there, which panicked the watchers of exited panes in debug builds.

Measured with the quick wave1 gate (`--only spawn,chatty,mem,echo,throughput,attach`, load 7-25)
and a full `--only chatty,mem` run, against `/tmp` before-JSON of 157ac6a3 (quick) and W0:
`spawn.cpu.split_shell` 4.71 -> 2.57 ms (36.8 -> 20.2 Minstr), `spawn.wall.split_empty_P` 2023 ->
6.3 ms (1.9 ms CPU, 45.4 -> 17.0 Minstr), `spawn.cpu.new_window` 4.94 -> 2.67 ms (in an A/B on one
isolated daemon each, daemon Minstr per command: split 37.6 -> 21.2, empty split with `-P` 47.1
-> 17.5, new-window 46.3 -> 22.9, kill-pane 16.4 -> 15.5, against 12.6 -> 11.7 for
`display-message`);
`chatty.cpu_pct.steady` 9.86 (W0) -> 3.02% (102 Minstr/s, tmux 112), `.flip` 25.6 -> 5.4% (3285
-> 351 Minstr/s), `.hidden` 51.5 -> 11.1% (6906 -> 1117 Minstr/s, tty 219 -> 43 KiB/s),
`.visible` 17.0 -> 16.6%; `mem.footprint.p20` 54.1 -> 41.9 MiB, `mem.threads.p20` 89 -> 49,
`mem.footprint.tui20` 56.7 -> 44.6 MiB, `mem.footprint.scroll180` 343.8 -> 55.9 MiB (tmux 61.5,
0.91x), `.scroll80` 170.6 -> 49.0 MiB (tmux 35.6, 1.38x); `echo.p50.idle` 0.45 -> 0.25 ms,
`echo.p99.idle` 0.68 -> 0.50 ms, `echo.p50.busy30` 3.84 -> 0.44 ms, `echo.p99.busy30` 17.7 -> 1.6
ms. Throughput and attach measured against the pre-campaign binary in the same runs: detached
ASCII 214-219 against 204-217 MB/s, unicode 101-102 against 98.5, attached 756-817 against
722-755 ms at load 7-8, and at the lane head under load 13-18 detached ASCII 210 and 174 against
179 and 152, unicode 106 and 103 against 101 and 103, attached 910 against 960 and 962 ms (one
more lane run read 1506 ms with a 6 s outlier while another fixture ran; the quick gate's single
run read 179-194 at load 14-20);
`attach.instr.p1` 120 against 122 Minstr, `.p4` 126.6 against 126.5, `attach.ttfc.p1` 18.8
against 19.7 ms, `attach.wire_s2c.p4` 257 -> 204 KB.

After review (quick gate plus a full `--only chatty,mem`, load 12-14 from other sessions, so
wall and CPU rows only warn): `spawn.instr.split_shell` 20.2 Minstr, `split_empty_P` 16.5,
`new_window` 21.2, `kill_pane` 14.6; `chatty.instr_per_s.steady` 88.7 Minstr/s (tmux 99.9) at
3.54% CPU, `.flip` 392-425 Minstr/s at 5.7-6.3%, `.hidden` 1165-1516 Minstr/s; `mem.threads.p20`
49, `mem.footprint.p20` 43.1-43.2 MiB, `.tui20` 47.1, `.scroll180` 58.8 MiB (tmux 61.5, 0.96x,
passes), `.scroll80` 51.1 MiB (1.44x); `echo.p99.busy30` 1.55 ms; `throughput.detached.ascii`
221 MB/s (4.5x tmux); `attach.instr.p1` 128, `.p4` 118.8 Minstr. The publish change moves the
actors off the top of the profile: in a 6 s `sample` of the flip workload (10 detached panes)
`publish_views` has about 40 busy samples and the watcher signal 26, against 112 for the 16 ms
publish before review, and the watchers' runtime sync is now most of what is left (see below).
`zz-child-reap` threads in a 5 s split and kill loop: 1 in 312 kills, against 99 in 380 before.

Missed or handed on:

- `mem.footprint.scroll80` (1.38x, rule 1.15x): the scrolled history now costs 7 MiB over 20 idle
  panes at 80 columns (tmux holds 33 MiB of it); the rest is the idle panes. A quiet 80x24 pane
  costs about 1.5 MiB, 0.9 MiB more than an empty one, and almost all of it is mimalloc pages:
  each pane runs two threads with their own heaps, and pages abandoned by exited connection
  threads keep the pane state they allocated (creating the same 20 panes from one chained
  command instead of 20 commands saves 5 MiB). The 64 KiB read buffer is not a factor (a 4 KiB
  buffer measured the same). Handed to W1-EXEC (connection threads) and W3-SHARDS (threads per
  pane).
- `echo.p99.busy30` (1.6 ms against 1.5x tmux, about 0.3 ms) and `echo.p50.*`: the path still
  crosses the client reader, the actor, the watcher and the mailbox writer, and
  `publish_terminal_for_pane` takes `inner`; W3-SHARDS and W4-DELIVER own those hops.
- `spawn.wall.*` and `spawn.cpu.*`: `display-message` alone costs as much wall time as a split
  here; the remaining daemon work per split beyond it is the command and publication path
  (W1-EXEC, W1-PUBLISH, W1-FORMAT) plus the actor's share (`posix_openpt`, the slave open,
  `Terminal::new`, the spawn). The lane's own gate misses on a quiet host too, as review
  measured: `split_empty_P` 6.4-6.9 ms (gate 6 ms) and `split_shell` 6.2-6.7 ms (gate 6.5 ms),
  with `display-message` alone at 4.9 ms against tmux 3.3; the rest is the command path (W1-EXEC,
  W1-PUBLISH).
- Content-only watcher wakes stay at ten a second. Review asked to stretch them to the 1 s status
  tick. In the flip profile after the 100 ms publish change the actors are a small share (about
  40 busy samples in `publish_views` and 26 in the watcher signal over 6 s for 10 panes) and the
  watchers dominate: `synchronize_pane_runtime` sees `pane_current_command` flip between `sh`
  and `sleep` on almost every wake and republishes the snapshot (`publish_snapshot_state`,
  `format_option_snapshot`, the status refresh). Stretching the wake would also make
  `synchronize_pane_runtime` stamp activity and silence up to 1 s late, since it stamps
  `last_output` with the wake time. The fix belongs in that function (read an output time the
  actor keeps, and look up the current command lazily), which W1-PUBLISH owns.
- The settle rebuild of an unwatched pane runs whether or not anything reads its cells. Only
  `#{C:}` does; skipping the rebuild unless a status template uses `#{C:}` or a preview watch is
  on needs the format side to say so (W1-FORMAT).
- W1-FOOTPRINT's hand-off: the foreground lookup is one `tcgetpgrp` on the master without
  portable-pty's mutex, and unwatched panes wake their watcher at most ten times a second; the
  second lookup per event in `terminal_current_command` and `terminal_working_directory` stays
  with whoever owns those functions after the merge.
- Linux still forks: glibc's `posix_spawn` with `addclosefrom_np` needs glibc 2.34, newer than
  the headless binary's floor, and Linux reclaims `MADV_DONTNEED` pages after a fork. The macOS
  launcher adds one exec to each pane's start (a few ms before the program runs, off the command
  path); a tiny dedicated launcher binary would halve that but needs a build step and a file to
  ship.
- Silent panes keep the command name of their first runtime sync: `zsh -c 'exec sleep 30'`
  reports `zsh` until the pane prints, on 157ac6a3 too (tmux reads it live at format time). The
  runtime sync is W1-PUBLISH's and W1-FOOTPRINT's.
- `daemon::tests::mode_keys_scope_visible_command_output_separately_from_underlying_copy_mode`
  takes 30 s on 157ac6a3 as well: Escape on the copy pane parks the command output, so the test
  only passes when its `sleep 30` pane exits just before the 30 s deadline, and it fails under a
  loaded full-crate run.

Checks on macOS: the zz-terminal and zz-daemon suites pass (the daemon suite once under
`--test-threads 8`), clippy is clean for zz-terminal, zz-daemon, zz-cli, zz-tui and zz, and for
zz-terminal on `x86_64-unknown-linux-gnu`. `compat/run.sh` over the whole corpus,
`attached-client.sh` in all three modes, `tui-copy-mode.sh` (147 cases) and
`tui-pane-geometry.sh` agree with the pin except for scenarios that diverge the same way on
157ac6a3 on this host (`census-hooks` reads the missing `/etc/hostname`, `prompt-history`,
`if-shell-background-order` under load, and smoke `plugin-runtime-vim-tmux-navigator`,
`resurrect-save`, `source-file-byte-name`, `status-background-jobs`). `tui-choosers.sh` and
`tui-screen-diff.sh` stop at the same checkpoint on 157ac6a3 (the pin's session tree never
settles; the zz wide-glyph line never settles), and `tui-output-backpressure.sh` reads
`/proc` and runs only on Linux.

Checks after review: zz-terminal (308 tests), zz-daemon lib (1053, twice under
`--test-threads 8`; `client_focus_updates_activity_and_focus_in_owns_latest_geometry` and
`popup_jobs_receive_sigterm_on_target_detach_and_kill_server` failed once under load and pass
alone, the second fixed by the signal change above) and zz-cli (all suites) pass; clippy is clean
for zz-terminal, zz-daemon, zz-cli and zz-tui, and for zz-terminal on `x86_64-unknown-linux-gnu`.
`compat/run.sh` over 31 pane, copy-mode, capture, alert, spawn and format scenarios and all 145
smoke scenarios agrees with the pin except `keys-prefix-attached`, `plugin-runtime-continuum`
(`gsleep` from Homebrew's gnubin on `PATH`), `plugin-runtime-oh-my-tmux` and the four smoke rows
listed above, all of which diverge the same way with the pre-campaign binary on this host.
`attached-client.sh` in all three modes, `tui-copy-mode.sh` (147 cases) and
`tui-pane-geometry.sh` agree.

Left on this path after review, in a 6 s `sample` of the flip workload: the pane actors are
about 40 busy samples in `publish_views` and 26 in the watcher signal, PTY parsing 12; the
watchers are the bulk (`format_option_snapshot`, `publish_snapshot_state` and the status refresh
behind `synchronize_pane_runtime`, W1-PUBLISH and W1-FORMAT, and `terminal_current_command`,
W1-FOOTPRINT).

Merged onto FOOTPRINT, FORMAT and PUBLISH (`perf/wave1`, merge `ad9c0c9b`, follow-up `04509ada`):
- The rebase kept PUBLISH's `terminals_mut()` copy-on-write map and its `Command::Settle` arm
  (now in the control-slot loop). PUBLISH's copy-mode `settle()` runs after `inner` is released,
  so it lifts the round-trip guard with `allow_actor_round_trips`.
- A respawned pane is dropped from `streamed_terminals`, and PUBLISH no longer publishes a
  snapshot for a respawn, so nothing set the new terminal's view streams and its frames never
  reached the client (`pipe_pane_survives_respawn_with_the_same_child`). A command that respawns
  a terminal without a snapshot change now runs `refresh_terminal_visibility`.
- `chatty.tty_kibps.hidden` went from 6.8 to 16.9 KiB/s. The leading-edge publish wakes the
  watcher at output time, when a `while :; do echo; sleep 0.01; done` shell is usually still in
  the foreground, so hidden windows flipped between `bash`, `sleep` and a blank name (a lookup
  that raced the exiting `sleep`), and every rename repaints the whole attached TUI. names.c
  instead arms a timer for output inside `NAME_INTERVAL` and reads the name when it fires. Output
  inside the interval now marks the window pending (`note_automatic_rename_output`), the deadline
  re-reads the active pane's command (`due_window_rename_panes` in `apply_due_window_renames`), and
  an empty lookup keeps the last command. Hidden is now 3.3 KiB/s and `rename-timing.sh` counts
  2 renames for tmux and 2 for zz.
- Still regressed against the PUBLISH merge run, bisected with non-LTO builds to this lane's first
  commit (frames only for watching views), not to the merge: `cli.instr.*.p1` +0.3 Minstr per
  command and `attach.instr` +4 to +5 Minstr, `attach.tty_total` +16 KB. The CLI part is
  allocator churn: with the daemon's purge delay of 0 (W1-FOOTPRINT), each connection thread's
  fresh pages are recommitted with `madvise`; `MIMALLOC_PURGE_DELAY=-1` brings it to 3.42 against
  3.36 Minstr. Before this lane, allocations that outlived the connection thread kept its pages
  abandoned and reusable. Per-connection threads are W1-EXEC's. The attach bytes are one more
  17 KB "waiting for frame" placeholder paint by the TUI, because the view's first frame now
  comes from the watcher after `SetViewStream` instead of being ready when the snapshot goes out;
  the TUI repainting everything on each snapshot and placeholder is W1-ATTACH's.

Merge gate (`w1-4-pane-macbook-ad9c0c9b.json`, full, load 4-8 from other sessions, so wall and
CPU rows are notes): scroll180 44.2 MiB (tmux 61.4), scroll80 37.9 MiB (tmux 35.4), p20 31.2 MiB,
tui20 34.4 MiB, 46 threads at p20; chatty steady 2.89% (tmux 2.95%), flip 1.97% (1.97%), hidden
2.14% (2.37%), instructions below tmux in all three; split_shell 6.9 Minstr and 5.3 ms,
split_empty_P 4.8 ms, new_window 7.1 Minstr; detached throughput 230 MB/s (4.4x tmux). Failing
rows this lane shares: `mem.footprint.p1` 6.6 MiB (rule 6.5), spawn CPU, echo ratios, attach.

Linux follow-up (W1-LINUX-PAGES, 2026-09-29, alienware, `perf/linux-pages`): during a detached
`cat` into a 180x50 pane the actor ran at 97-99% CPU, 61-69% of it in the kernel, on about 468k
page faults per 0.7 s. Once history is full, Ghostty's line limit prunes a page from `grow`'s
fast path, `destroyNode` decommits it back to the pool, and the next `createPage` faults the same
buffer back in 4 KiB at a time. The fix is in the fork (branch `zz/pagelist-reuse`, not pushed,
so `GHOSTTY_COMMIT` still names the old pin): limit pruning keeps the last pruned pool page as a
spare that `createPage` rebuilds with a memset; `compress` returns it to the pool and decommits
the unused rows of the last page, so an idle pane holds what it did before (without that trim,
scroll180 read 29.1 MiB). zz-terminal's escape scan (`find_escape`, a byte loop in both
`EngineFilter` and `PassthroughFilter`, 28% of the actor's cycles once the faults were gone) now
uses `memchr`. Same-host A/B against the old pin: actor 28-32% CPU, 0.6-0.8% of it kernel, 114-153
faults per 0.7 s, none in libghostty; quick gate detached ASCII 84-89 -> 119-134 MB/s; full gate
ASCII 77-84 -> 131 MB/s, unicode 43-45 -> 91 MB/s, attached ASCII 1844-1956 -> 1264 ms; footprint
p1 3.2 -> 3.2, p20 17.9-18.5 -> 18.2, tui20 19.4-20.1 -> 19.7, scroll180 23.8-24.1 -> 24.4, scroll80
22.4-22.5 -> 22.7 MiB; chatty CPU and instructions unchanged. The pane's `zz-pty-gather` thread
(67-70% CPU, nearly all kernel) and the cooked tty are now the limit. What is left on the actor:
printing 41%, the memset of the reused page 22% (the kernel's page zeroing did this work before;
`rep stosb` would cut it by about a fifth, and a `baseline` CPU build gets 16-byte SSE2 stores),
VT parsing and dispatch about 15%, `cursorScrollAbove` 7%.

Mac check and trim fix (2026-09-30, macbook): the fork landed as `713374af` (`zz-2026-09-29`,
pinned in `2166bd31`). Its trim picked the released cells as those past `size.rows * cols`, but
`eraseRows` swaps row headers without moving cells, so after a partial history erase (a shell
`clear` sends ED 3) the live rows point at the blocks the trim released: blank on Linux one idle
second later, out of the footprint on macOS. `c3941417` (`zz-2026-09-30`) starts the trim after
the highest block a live row references; zz-terminal's `a_cleared_screen_survives_idle_compression`
fails on the old pin when the discard zeroes the range (as on Linux) and passes on the fix. Gate
`wave1-macbook-2166bd31.json`: detached ASCII 243 -> 310 MB/s, unicode 104 -> 122 MB/s, scroll180
38.7 -> 35.3 MiB, scroll80 32.8 -> 28.5 MiB (the scroll rows never erase history, so the old trim
was right there).

## W1-EXEC: one-frame commands, fast cold start (effort L)

Scope:
1. zz-protocol message.rs (append only, version stays 107): `Exec(ExecRequest)` with
   `protocol_version` first, flags (UTF8, HAS_TTY, STDIN_AVAILABLE, NESTED, READ_ONLY),
   `client_instance_id`, origin pane, cwd, tty, size, TERM, features, `startup_reentry`,
   `spawned_server_id`, **`expect_server_id: Option<u64>`**, environment as one NUL-separated blob
   parsed lazily, `commands: Vec<CommandInvocation>`. `ExecOutput{stream, bytes}`.
   `ExecExit{code, server_id, resume: Option<ExecResume>}`; `ExecResume` carries exactly the
   attach/TUI classification `PreparedCommandList` carries today plus the unexecuted tail.
   Codec arms and round-trip tests in terminal_codec.rs. `PrepareCommandList` stays.
2. `handle_connection`'s first read: **envelope version check first** (the existing
   VersionMismatch branch), **then** dispatch `ClientHello` to today's path unchanged and `Exec`
   to `serve_exec`.
3. **No executor pool in wave 1: one thread per Exec connection.** That thread reads the Exec,
   runs it, writes, and reads the next frame. **An Exec connection accepts repeated Exec frames,
   one `ExecExit` each, and closes on client EOF.** Before running, the daemon checks
   `expect_server_id` and answers a mismatch with an `ExecExit` error. **A zero-command Exec is the
   liveness/mismatch probe.** Parking inside a command (hooks, `source-file`, `run-shell`,
   `if-shell`, `wait-for`, pane waits: 16 `report_command_queue_park` call sites, some reached
   only dynamically) blocks only that connection's thread; on the first park or
   `ClientFileRequest` from the connection, spawn today's reader so stdin, file responses and
   client EOF keep flowing. No static `may_park` flag.
4. **Startup**: a non-reentry Exec that arrives before startup completes is parked in a pending
   list (no thread held) and resumed by `finish_startup`; `startup_reentry` Execs (config job
   children) run at once. Test: two concurrent cold CLI commands against a config whose
   `run-shell` child calls the CLI; also `source-file` of a TPM-style config and an
   `after-select-pane` run-shell hook that calls the CLI.
5. `register_exec` inserts only the client facts formats read (origin, tty, size, pid, cwd,
   utf8, features, environment). No `ServerHello`, key snapshot, option snapshot, mux snapshot,
   status render, `published_mux_options`, `client_status_rows`, or `warm_terminfo_entries`. The
   chain goes through `prepare_command_list_with_engine` and
   `execute_command_request_with_prepared` with <= 2 `inner.lock()` takes per command; alias
   freezing and error semantics unchanged. `unregister_exec` releases views (cheap after PANE),
   skips detach hook captures and `refresh_control_output_taps` for a client that never
   attached. `spawned_server_id` replaces the `__zz-cold-start-prepare-abort` pseudo-command.
6. `Shared::unregister`, `detach_with_event_hooks`: `refresh_control_output_taps` once;
   `detach_client_state` skips `MuxHookSnapshot`, focus probe and copy-mode captures for clients
   that never attached.
7. client.rs `CommandClient`: speaks Exec. API: `connect` opens the socket; `execute` sends one
   Exec; `server_id()` is lazy (from the last `ExecExit`, else a zero-command probe);
   `server_hello()` is removed from `CommandClient`. Users to update: crates/zz
   `config/settings/file_options.rs` `execute_file_command` (passes `expect_server_id`, runs two
   commands on one client); zz-cli `connect_command_client` and
   `connect_command_client_with_spawn_provenance` (provenance from `ExecExit.server_id` vs
   `spawned_server_id`), `stored_default_client_command`, the kill-server path; crates/zz
   `config/mod.rs` `local_command_client`; daemon, zz-client simulator and `agent_soak.rs` tests
   that run many commands per connection (still one client identity per connection).
8. zz-cli lib.rs: one Exec per invocation; delete the prepare round trip and per-command
   `CommandRequest` loop (`prepare_cli_command_chain`, `execute_prepared_command`,
   `execute_command_chain`). `ExecResume` drives the attach/TUI decision. Cold start:
   `spawn_daemon` passes `--bootstrap-ready-fd N` (CLOEXEC before panes spawn); the daemon writes
   one byte right after `LocalTransport::bind`; the CLI blocks on it (6 s deadline) and connects
   once; delete the redundant failing connect in `connect_or_spawn_daemon_with_provenance`.
   Windows keeps a 0.5/1/2/5 ms backoff. Foreign-version replies keep
   `classify_local_connect_error`; same-version decode failure retries once over the legacy path.
9. lifecycle.rs `write_identity_atomically`: write + rename, no `sync_all` (3.8 ms
   `F_FULLFSYNC`).
10. `run_foreground_listener`: `claude_peers::sweep_stale_records` after readiness; prompt history
    loads lazily behind a `OnceLock` on first access (not "after readiness", which loses an early
    entry). The in-pane `tmux` wrapper installs lazily on first pane spawn, pinned to a
    daemon-owned clone of the executable (Protocol policy).

Write zone: message.rs (append), terminal_codec.rs control-lane arms; client.rs `CommandClient`,
`connect_stream_with_startup_owner` Command branch (not `short_device_name`); daemon.rs
`run_foreground_with_ready`, `run_foreground_listener`, `accept_connections`,
`install_tmux_shim`, `finish_startup`, `wait_for_startup`, `Shared::unregister`,
`execute_command_request`, `execute_command_request_with_prepared`, `prepare_command_request`,
`prepare_command_list_for_request`, `prepare_command_list_with_engine`, `detach`,
`detach_with_event_hooks`, `detach_client_state`, `handle_connection` first-frame dispatch,
`prepare_socket`, new `serve_exec` / `register_exec` / `unregister_exec`; lifecycle.rs
`write_identity_atomically`; zz-daemon lib.rs cold-start pseudo-command; zz-cli lib.rs bootstrap
args, command-mode chain, `spawn_daemon`, `detach_daemon_session`, connect fns,
`stored_default_client_command`; crates/zz `file_options.rs` `execute_file_command`,
`config/mod.rs` `local_command_client`.

Gate vs PANE JSON: `cli.cpu.display.p1` <= 0.25 ms, `.p20` <= 0.40 ms; `cli.wall.display.p1` <=
1.10x tmux; sockproxy: 1 frame up, 1 write down per single `display-message` (<= 6 KB up, <= 200 B
down); thread count back to baseline after 500 CLI commands; `cold.wall.new_session` <= 22 ms;
`spawn.cpu.kill_pane` <= 0.8 ms; throughput unchanged. Tests: workspace tests,
`crates/zz-daemon/tests/{clients,overlay,signal_shutdown,agent_soak}.rs`, zz-cli cold-start
provenance, daemon-upgrade mismatch tests, the concurrency tests in item 4, repeated-Exec and
`expect_server_id` tests. Compat: `compat/run.sh`, `packaged-cli.sh`, `startup-diagnostics.sh`,
`tui-client-commands.sh`, `tui-command-streams.sh`, control-mode fixtures.

Expected: removes the hello build (47-51%), unregister wakeups (8%), thread churn (5-10%), 2 of 3
round trips, 14.6 ms of retry slack and 3.8 ms of fsync on cold start.

Built on `perf/exec` (2026-09-28), rebased onto `perf/wave1` d5542787 (FORMAT and PUBLISH
merged) after review. Where the build departs from the scope above:

- Wire (appended to `ProtocolMessage`, new module `zz-protocol/src/exec.rs`): `Exec(ExecRequest)`
  and `ExecExit(ExecExit)`. There is no `ExecOutput`: each command's result is today's
  `CommandResponse` (request id = position in the chain, 1-based), and released `-P` lines,
  streamed stderr lines, `CommandClientExit` and `ClientFileRequest` keep their existing frames,
  so the CLI's stdout writer, its raw-claim collision rule and the last-nonzero exit fold are the
  code they were. `ExecExit { server_id, outcome }`, outcome `Ran | Resume(ExecResume) |
  Rejected(ServerError) | ServerMismatch`; `ExecResume { kind: NewSession | NativeAttach,
  commands: Vec<PreparedCommand> }` (the whole chain: an attaching chain runs nothing on the
  command connection, as before). A connection-level refusal (envelope or `protocol_version`
  mismatch, server stopping) is today's `CommandResponse::Error { request_id: 0 }`. Flags: `UTF8`,
  `STDIN_AVAILABLE`, `NESTED`, `RESUME` (the caller can take an attaching chain back; only the CLI
  sets it), `PREPARED` (run the commands as given, no alias lookup: the raw `--kill-server` path
  that `execute_prepared_streams` served), `LAST` (the CLI sends no second Exec, so the daemon
  unregisters right after the reply instead of blocking in one more read until the CLI's EOF; a
  `CommandClient` that sent one reconnects if it is used again). No `HAS_TTY`/`READ_ONLY` flag and no `TERM` field:
  nothing reads the first two, and TERM rides the environment. Features travel as the folded
  `u32` mask. The environment is one NUL-separated byte string (`ClientEnvironmentBlob`,
  serialized with `serialize_bytes`, wire-identical to a byte `Vec`).
- Daemon (`daemon/exec.rs`): `serve_exec` answers the first frame; `register_exec` inserts
  instance id, kind, origin, nested, utf8, features, tty, size, pid, cwd, activity/created/focused
  times and the environment blob, and computes the connection's `ExecutionContext` in the same
  lock. The blob is parsed only when something reads it: `ExecutionContext` and
  `client_environments` now hold `Arc<ClientEnvironmentBlob>` (zz-mux `set_client_environment`
  changed type; interactive hellos build one pre-parsed with `from_map`). The chain is prepared
  once (`prepare_command_list_with_engine`, `preflight_unaliased`), then each command runs through
  `execute_command_request_with_streams` (the old body, now also returning `client_exit`) with
  `prepared = true`; the chain stops at the first error or client exit, as the CLI loop did. That
  wrapper takes the state lock twice per command (preparation with the message-log entry, stream
  removal with the sanitize check) instead of four times. Only the wrapper changed: a
  `display-message` still takes the state lock about eight times in all on its way through
  `execute_with_mux_source_*`, `clock_modes_are_open` and the publish check, which is W3-LOOP's
  to cut. `refresh_control_output_taps`, which every command calls, returns before it walks the
  panes when there is no control client and no tap. A
  zero-command Exec is answered on the spot, before the startup wait and without registering a
  client, so it neither owns a cold daemon's bootstrap lease nor waits out a slow config.
- One write: when nothing parked, the connection thread drains the per-Exec mailbox and writes
  every frame plus the `ExecExit` in one `write_all`. That mailbox is created buffered: it has no
  256-frame cap (the byte cap stays), and between commands the connection writes it out once it
  holds 64 frames or 64 KiB, so a `\;` chain of any length streams its output (review found a
  300-command chain stopped at 257 and lost every line). The mailbox sits in `client_writers`
  only while that Exec runs, so an idle connection never holds up the shutdown drain. The first
  `report_command_queue_park` (via a thread-local exec client) or `client_file_operation` for the
  client starts a writer thread and a reader thread on that connection (`ExecLink::go_live`): the
  starter writes out what the buffered mailbox holds, switches it to the normal cap, hands the
  connection's own stream to the writer and clones only the reader's (no connection dups its
  socket up front any more). Direct writes and the starter serialize on the starter lock, so a
  go-live from another thread cannot interleave with a direct write. Later Execs on the
  connection go through the live pair; the reader's EOF cancels the queue and unregisters, as the
  old reader did. One `ResponseAdmissionGuard` spans the whole Exec, so a `kill-server` answer and
  the `ExecExit` are queued before shutdown freezes admissions and drains writers.
- Startup: an Exec that is not a startup reentry waits in `Shared.pending_execs` as a boxed job;
  `finish_startup` hands each to a connection thread and `begin_stopping` drops them (the client
  sees EOF).
- Connection threads: W1-FOOTPRINT handed over the per-connection thread cost (the heap purge at
  thread exit). Each connection still gets a thread of its own and nothing waits for one, but a
  finished connection thread parks for up to 1 s (at most two parked) and serves the next
  connection. Knob `ZZ_PERF_CONNECTION_THREADS=0`.
- Unregister: `detach_is_inert` (no session, copy mode, focus, visible views, control kind or
  latest-client mark) skips the `MuxHookSnapshot`, copy-mode and focus captures in
  `detach_client_state`, and an inert exec client skips `detach` entirely. Control output taps
  refresh only when a control client leaves an attachment. `release_view` still goes to every pane
  on unregister: W1-PANE's known-view set makes that a no-op for a client that never streamed.
  Client environments stay `Arc<ClientEnvironmentBlob>` in `FormatHookFacts` and
  `ClientFormatFacts` too; rows are built only when `#{Vc:}`, a client loop or the client terminal
  lookup reads them (before, every command parsed the blob and copied every variable).
  `client_format_facts` returns empty facts for a session that is gone instead of panicking on
  the index (review saw one such panic on a `zz-client` thread under pty exhaustion; not
  reproduced).
- CLI: one `CommandClient::exec_chain` per invocation; `ExecResume` drives the TUI and native
  attach branches (the classification moved to the daemon as `exec_resume_kind`, which the legacy
  path also uses). A daemon that never answers is the `Err` of `exec_chain` and takes the old
  "prepare failed" branch (static validation, spawn, TUI for an incompatible daemon); an answered
  chain ends in `ExecChainEnd`. Two edge semantics moved: kill-server recovery after a transport
  failure applies to the typed `kill-server` (the daemon never answered, so alias status is
  unknown), and a no-start-server command against an incompatible daemon prints the classified
  mismatch message.
- `CommandClient`: `connect` opens the socket only; `server_id()` is lazy; `server_hello()` is gone
  (the one daemon test that read it probes instead); `execute_on_server` is the settings file
  path (`expect_server_id`). `ZZ_PERF_LEGACY_COMMAND=1` runs the hello path with the old prepare
  and per-command loop, including the cold-start abort pseudo-command. The automatic fallback is
  narrower than first built: with one reply write per Exec, an EOF before the first frame also
  means "the daemon ran the chain and died before writing", and the first build then reran the
  whole chain on a freshly spawned daemon. Now the daemon lists `exec-v1` in its `ServerHello`
  capabilities; on EOF or an undecodable first frame the CLI opens a hello connection and reruns
  there only if the capability is missing (a pre-campaign 107 daemon, which reads and drops the
  frame without running it). With the capability, or with no daemon left to answer, the chain
  ends as a transport failure (`zz: daemon protocol error: ...`, as the legacy path printed for
  a daemon that died mid-command) and never reaches the spawn branch. A reset or broken pipe means
  the daemon never read the frame and is `Err` as before (a reset spawns, as it did).
- Cold start: `--bootstrap-ready-fd N` after the server id; the daemon writes one byte right after
  `bind` and closes it; the CLI polls the pipe (6 s) and dials once. The backoff (0.5/1/2/5 ms) runs
  only without the pipe (Windows) or when a racing daemon won the socket. `prepare_socket` now
  holds `<socket>.lock` (flock, the file stays beside the socket) from the probe to `bind`, as
  tmux's `client_get_lock` does: two concurrent cold commands (the new CLI test) otherwise let
  one daemon unlink the other's socket between its `bind` and `listen`, leaving a daemon nobody
  could reach. The probe is a
  zero-command Exec: a daemon that answers releases the loser's CLI through the readiness pipe and
  the loser exits at once (before, it looped for 3 s and could bind after the winner was killed,
  an orphan), and one that does not answer is still waited out as a stopping daemon. Identity
  without `sync_all`; Claude peer sweep after the ready callback; `history-file` read on first use
  (`ensure_prompt_history` at every prompt reader and writer).
- `copy-mode` from a command or control client now answers once the pane actor has applied the
  view action: entering copy mode is a view action the actor applies later, and without the old
  CLI's two extra round trips a following `display -p '#{pane_in_mode}'` read 0 every time
  (compat `smoke/copy-mode-formats`). tmux enters the mode before the command returns. The first
  build sleep-polled the published facts every 1 ms for up to 100 ms (19 ms per CLI `copy-mode`
  against 8 ms on `main`, the full 100 ms for a view the actor ignores); the lane then added a
  `TerminalSession::barrier` request (6 ms per CLI `copy-mode`, hidden window included). Rebased
  onto W1-PANE, the barrier and the poll are gone: PUBLISH's `TerminalSession::settle` is the same
  request and already runs for every client kind after copy-mode view actions, so it covers the
  Exec path and keeps a control-mode `copy-mode` then `display -p` reading like tmux's.
- The tmux wrapper is pinned (see Protocol and release policy) but still installed at startup, not
  at the first pane: a cold `new-session` spawns a pane and needs it anyway, and install plus clone
  measured 0.2 ms (clonefile 80 us, directory and script 110 us).

Measured with the quick gate (`--only cli,cold,spawn,control,config`, stage wave1) on this
macbook at load 10-37 with other lanes building, before (`613630ce`) -> after (`756498e1`), tmux
in the same run. Instructions: `cli.instr.display.p1` 10.3 -> 2.78 Minstr (tmux 0.49),
`.show_options.p1` 10.4 -> 2.80, `.has_session.p1` 9.7 -> 2.17, `.send_keys.p1` 9.77 -> 2.23,
`.select_pane.p1` 9.72 -> 2.19, `.display.p20` 28.6 -> 6.0, `.has_session.p20` 25.6 -> 3.06,
`.chain5.p1` 19.9 -> 12.0, `.chain5.p20` 43.3 -> 20.4, `spawn.instr.kill_pane` 13.8 -> 5.97,
`spawn.instr.split_empty_P` 45.6 -> 32.8 (`split_shell` and `new_window` swing 29-41 Minstr
between runs of one binary with the shell's startup output, so no gain is claimed for them).
CPU (noisy at this load): `cli.cpu.display.p1` 1.17 -> 0.31-0.34 ms, `spawn.cpu.kill_pane` 1.9 ->
0.67-1.48 ms over review's seven runs (borderline against 0.8 before the rebase).
Wall: `cli.wall.display.p1` 3.41 -> 2.52 ms (tmux 3.55), `cold.wall.new_session` 36.5 -> 11.7 ms
(tmux 12.1), `cold.wall.new_session_noterm` 31.0 -> 11.7 ms. Control-mode rows are unchanged (that
path is W2-CTRL's). Wire per `display-message -p x` through a counting proxy: 1 frame and 1 write
up (1.1 KB with the gate's environment, 3.9 KB with a 3.2 KB shell environment), 1 read of 35 B
down (the response and the exit); the hello path moved 3 frames up and 28.4 KB down in 6 reads.
The TUI's option reads at attach are command clients too: `attach.wire_s2c.p1` 257146 -> 172270
B and `.p4` 239976 -> 118970 B (W0 -> this branch; each no longer gets a 28 KB hello), with
`attach.conns.*` still 4 (W1-ATTACH) and `throughput.detached.ascii` 241 MB/s (4.9x tmux).
Daemon threads: 13 before and after 500 CLI commands; `mem.threads.p1` reads 14 because the gate's
`#{pid}` lookup runs right before the sample and its connection thread is still parked (it retires
after 1 s). Thread reuse alone is 2.95 -> 2.75 Minstr per `display-message` (A/B with the knob).

Review fixes, checked on the rebased branch: `cargo test` for zz-protocol, zz-terminal, zz-mux,
zz-daemon (lib and every test target; one russh port test failed under load and passes alone),
zz-cli (also with `ZZ_PERF_LEGACY_COMMAND=1`), zz-client, zz-tui, zz-web, zz-client-ffi; clippy
`-D warnings` on those and `zz`; `compat/wire-version.py`; `compat/run.sh` full corpus on a pinned
copy (red twice: the nine host rows below plus the two `known/` rows with their documented
divergences; `if-shell-background-order` was red once under load and passed alone;
`smoke/copy-mode-formats` passes); `tui-copy-mode.sh` 147/147; `attached-client.sh` PASS;
`startup-diagnostics.sh` 8/8; `tui-command-streams.sh` and `tui-client-commands.sh` the same 11
and 5 differences as `main` (one earlier `tui-client-commands` run timed out attaching under load);
the review's repro scripts: a 300-command `\;` chain sets 300 options and prints 300 lines, the
`kill -9 $PPID` pane runs once with exit 0, `run-shell 'kill -9 <daemon>'` prints the protocol
error and spawns nothing, `copy-mode` takes 6 ms. New tests in `exec_tests.rs`: a 640-command
chain with 160 KB of output, a proxy that runs an Exec then drops the reply (no rerun), a proxy
hiding `exec-v1` (one rerun over hello), and a `LAST` Exec closed by the daemon. Not run: Linux
(the `/proc/self/exe` fallback is compile-checked only), `packaged-cli.sh`.

Checks before review: `cargo test` for zz-protocol, zz-mux, zz-daemon (lib and every test target),
zz-cli (also with `ZZ_PERF_LEGACY_COMMAND=1`), zz-tui, zz-client, zz-client-ffi, zz-web and
zz-config; clippy `-D warnings` on those and `zz`; Linux `cargo check` of zz-daemon and zz-cli,
Windows of the client half; `just web-build`; `just ios-gpui iPad build`. `compat/run.sh` (316
rows) on this macbook: nine rows fail on both passes here, and the same nine fail with the
pre-campaign `main` binary on this host (`census-hooks` needs `/etc/hostname`, `prompt-history`,
`smoke/keys-prefix-attached`, `smoke/plugin-runtime-continuum`, `-oh-my-tmux`,
`-vim-tmux-navigator`, `smoke/resurrect-save`, `smoke/source-file-byte-name`,
`smoke/status-background-jobs`); `smoke/copy-mode-formats` failed only here and is fixed above.
`compat/tui-command-streams.sh` and `tui-client-commands.sh` differ from pinned tmux in the same
11 and 5 cases as `main` on this host; `compat/startup-diagnostics.sh` passes 8/8. Not run:
`compat/packaged-cli.sh` (needs a bundle).

After review (rebased onto d5542787, the fixes above, `LAST` and the shared stream), quick gate
against `w1-2-format-macbook-a5c5cab7.json` at load 4-5, W1-2 -> this branch, tmux in the same
run: `cli.instr.display.p1` 6.94 -> 0.217 Minstr (tmux 0.485), `.chain5.p1` 0.509 (tmux 0.929),
`.display.p20` 10.4 -> 1.08 (tmux 0.87); `cli.cpu.display.p1` 0.74 -> 0.109-0.117 ms (floor 0.25,
passes); `cli.wall.display.p1` 0.63x tmux; `cold.wall.new_session` 10.5-10.9 ms (tmux 12.5);
`spawn.cpu.kill_pane` 0.42-0.44 ms and 0.92 Minstr (was 7.66); `config.cpu.source_1000` 8.4 ms.
`control.output_mbps` read 85.5 once and 103.8-105.3 in three reruns (noise; EXEC does not touch
that path). `throughput.detached.ascii` interleaved against a build of the wave1 tip itself:
160-246 MB/s (median 222) for this branch, 203-249 (median 225) for d5542787, so the drop from
W1-2's 271 is not this lane's. Wire per `display-message -p x` through `bench/perf/sockproxy.py`:
one connection, 4.2 KB up (a full shell environment), 35 B in 2 frames down. The daemon ends at
11 threads after the gate's commands (`mem.threads.p1`).

Still red in that run, none on this lane's own path: `cli.cpu.display.p20` 1.12-1.19 ms (floor
0.40), `cli.cpu.list_panes_a.s20` / `list_windows_a.s20` 1.22-1.29 ms (1.2x tmux), all
`spawn.cpu.split_*` / `new_window`, `spawn.wall.split_empty_P` (2 s identity wait, the same in W0
and every merge since), `mem.threads.p20` / `mem.footprint.p20`, and `attach.*` (ATTACH). The
p20/s20 CPU rows are one cost: every command's unregister sends `ReleaseView` to every pane and
wakes all 20 actors (instructions are 1.08 Minstr while CPU is 1.1 ms: kernel wakeups). A
throwaway build that skipped that loop for command clients read `cli.cpu.display.p20` 0.132 ms,
`list_panes_a.s20` 0.202 ms and `list_windows_a.s20` 0.179 ms (all passing, below tmux) with p1
unchanged at 0.103 ms. That skip is not committed: W1-PANE item 9 (`known_views`, release of an
unknown view sends nothing) removes the same cost in zz-terminal for every client kind, and the
plan merges PANE before EXEC for exactly this reason. A Time Profiler trace at p1 (0.14 ms of
daemon CPU per command) leaves: the one pane actor woken by that `release_view` (12%), the
accept thread's poll wake and `accept` (13%), handing the connection to a parked worker (7%), the
final `close` of the socket (6%) and the two reads of the Exec frame (4%).

Review items not taken, and why: a leader/follower accept loop would not cut CPU. The thread
that accepts must hand the accept role on before it serves the connection, which is the same
one wakeup as today's hand-off to a parked worker, only moved; serving inline without a hand-off
would stall every new connection behind a slow command. The threading model is W3-LOOP's. Reading
the first frame in one syscall needs a buffered stream that owns any bytes read past that
frame, on both the hello and the Exec paths; with `LAST` the Exec path is already down to the
prefix read and the body read. The review's `wait-for` report (a killed waiter keeps its slot)
does not hold: its script backgrounds the shell function `c`, so `kill` hit the subshell and the
real `zz wait-for` kept running. Killing the CLI process itself drops the waiter on this branch
(threads 11 -> 10, the next `wait-for -S` sets the woken flag and a later `wait-for` returns 0). W2-CTRL: control lines can reuse `Exec` as built (results are
`CommandResponse` frames, not `ExecOutput`; `ExecResumeKind` has no in-place attach upgrade yet),
and the pinned wrapper path is `TmuxShimGuard.executable`.

Merged onto FOOTPRINT, FORMAT, PUBLISH and PANE (`perf/wave1`, merge `a26b6368`). The rebase
dropped the lane's `TerminalSession::barrier` for PUBLISH's `settle`, which is the same request
(`ce835e6d`). Merge gate (`w1-5-exec-macbook-a26b6368.json`, full, load 4-5), W1-4 -> merge, tmux
in the same run: `cli.cpu.display.p1` 0.61 -> 0.111 ms (tmux 0.100), `cli.cpu.display.p20` 0.85
-> 0.117 ms (tmux 0.143, floor 0.40 now passes), `cli.cpu.list_panes_a.s20` 1.11 -> 0.217 ms and
`list_windows_a.s20` 1.07 -> 0.175 ms (both below tmux), so PANE's `known_views` removed the
`ReleaseView` wakes as planned; `cli.instr.display.p1` 3.77 -> 0.19 Minstr (tmux 0.48),
`.display.p20` 6.85 -> 0.32 (tmux 0.87); `cold.wall.new_session` 36.3 -> 7.8 ms (tmux 12.5);
`spawn.cpu.kill_pane` 1.09 -> 0.29 ms, `split_shell` 2.18 -> 0.96, `new_window` 2.35 -> 0.98 (all
pass); `spawn.wall.split_empty_P` 4.8 -> 3.2 ms; `mem.footprint.p1` 6.63 -> 6.14 MiB (passes the
6.5 rule), `.p20` 31.2 -> 26.1 MiB; `attach.wire_s2c.p1` 186 -> 101 KB; detached throughput 243
MB/s (4.8x tmux). 59 pass, 11 fail, 0 regressed, 1 drifted. The failures are `chatty.tty_kibps.hidden`
2.8 KiB/s (PANE's row, was 3.3), `attach.*` (W1-ATTACH; `attach.tty_total.p1` is the drifted row
and reads the same 175.7 KB as at the PANE merge) and `echo.*`. `echo.p50.busy30` read 0.85 ms
against 0.50 at W1-4; three interleaved A/B runs against a build of `6a8f36a0` gave 0.49-0.61 ms
before and 0.48-0.97 ms after with tmux moving the same way (ratio 4.4-4.9 before, 4.6-5.4
after), so it is load noise, not this lane.

Merge checks: `cargo fmt --check`, workspace clippy `-D warnings`, `cargo test --workspace
--all-features` (only zz-daemon lib tests failed under full load: `history_request_is_guarded_...`
twice and `kitty_images_and_placements_...` once; each passes alone and 36 of 36 times with 12
copies in parallel on both this merge and `6a8f36a0`), `just compat-check` (with Homebrew bash
first on PATH; `/bin/bash` 3.2 trips `set -u` on an empty array in the roster tally test),
`compat/run.sh` full corpus: the four `known/` rows match the accepted summary, the other red rows
are the host rows above and are red on `6a8f36a0` too (`smoke/keys-prefix-attached` flips between
runs on both builds), and the three installed-layout rows pass on both builds once the binary is
named `zz_cli`; `compat/attached-client.sh` PASS.

## W1-ATTACH: attach path and TUI paint (effort M)

Scope:
1. `send_resync_inner`: on a fresh attach, do not resend the Snapshot `Attached` carries; send
   overlay events only for overlays that exist. Error-path Resync and RequestFull unchanged.
2. **Duplicate-Full drop against the pending slot only**, in `enqueue_terminal_with` /
   `replace_terminal_with`. Never against `delivered_terminals`: clients wipe viewports on
   `Attached` and after a failed patch (zz-client core `viewports.clear()`, remove +
   `request_full`), and `delivered_terminals` is cleared only on cancel, suspend and close. If the
   gate shows delivered-side dedup is needed for the 69 KB, it requires clearing
   `delivered_terminals` in `send_attached`, `send_resync_inner`, on RequestFull/NeedsFull and on
   popup change, tagged with an attach epoch. `newer_terminal_delivered` keeps dropping only
   strictly older frames.
3. `attach`, `attach_with_event_hooks`, `attach_collect_event_hooks`, `send_attached`: for clients
   reporting a size (`client_size_fact`), apply it and the status reservation through the resize
   policy before streaming, so the first Full has the final geometry.
4. `write_outbound` / `OutboundMailbox::recv`: `writev` up to ~256 KB of **consecutive ready
   frames**. **The reliable lane stays strictly FIFO** (kitty image chunks must precede the frame
   that places them). If bulk must yield, it gets its own lane plus a rule that holds a terminal
   frame whose `kitty_placements` reference an image until that image's chunks are written. Keep
   the `take_preview_refresh` check after each frame.
5. zz-tui: drain queued core events, paint once; on SnapshotChanged repaint only chrome whose
   layout, window list or pane set changed; never paint an empty pane before its first frame;
   read extended-keys and focus-events without two `CommandClient` connections. (Not with a
   `CommandRequest` on the `InteractiveClient`: any command with output that an interactive
   client runs opens a command-output view over its session. As built, the hello carries them.)

Write zone: daemon.rs `newer_terminal_delivered`, `OutboundMailbox::enqueue_terminal_with`,
`replace_terminal_with`, `recv`, `send_attached`, `attach`, `attach_with_event_hooks`,
`attach_collect_event_hooks`, `send_resync`, `send_resync_inner`, `write_outbound`; zz-tui
app.rs attach setup, event loop, paint fns, `handle_core_event`; tty.rs option reads; lib.rs
preflight.

Gate vs EXEC JSON: `attach.tty_total.p1` <= 8 KB; `attach.ttfc.p1` <= 14 ms; `attach.conns.p4` <= 2;
`attach.wire_s2c.p4` <= 130 KB; `attach.cpu.*` <= 3x tmux; `chatty.tty_kibps.hidden` <= 2 KiB/s;
`echo.wire_bytes.idle` <= 1700 B. Tests: switch-client A -> B -> A with no output and re-attach
deliver a Full for every visible pane; RequestFull after a bad patch repaints; kitty image then
placement order under batched writes; attach-sequence daemon tests (updated only where they asserted
the duplicates); zz-tui tests; `python3 compat/tui/tracker.py check` and every compat/tui fixture
(`attached-client.sh`, `tui-overlays.sh`, `tui-launch-diff.sh`, `tui-screen-diff.sh`,
`tui-caps.sh`).

Expected per attach: 69 KB duplicate Full, a 2.2 KB Snapshot and 9 blank repaints (4 panes), one
72 KB wrong-size Full (new-session), two extra connections; 29 write syscalls -> ~2.

As built (branch `perf/attach`, on perf/wave1 at da635845, merged with W1-PANE at ad9c0c9b,
W1-EXEC at a26b6368 and the Linux handoff head a41b1fbf; fix pass verified on Linux on 2026-09-29),
where the build departs from the scope above:

- Item 1: `send_attached` calls `send_resync_as(.., ResyncScope::Attach)`: no Snapshot, and an
  overlay event only for an overlay that exists (a command output that exists is still replayed;
  a close for none is not sent). The zz-client core clients (TUI, web, iOS) reset overlays and
  viewports on `Attached`; the GPUI desktop client keeps its retained viewports across it
  (`finish_attach` in `crates/zz/src/mux/client.rs`), so for it a Full it did not need costs only
  bytes. A requested `Resync` keeps the full scope. `send_attached` queues `Attached` and records the
  client's snapshot digest under `snapshot_order`, so no publisher can slip a Snapshot between
  them; each production caller publishes right after, which sends a Snapshot only when the tree
  moved since the attach built it.
- Item 2: the pending slot alone was not enough. The trace showed the second copy of the same
  Full arriving 0.1 ms after the first had been written, so a pending-only check missed it. The
  mailbox drops any update, patch included, whose generation is already queued or written:
  `terminal_update_redundant`, since every publish moves the view generation, except that only a
  queued Full covers an incoming Full: a queued patch never stands in for one, because the Full is
  wanted when the client may not hold the patch's base. What was written is forgotten where a
  zz-client core client drops viewports: when `Attached` is queued, on `RequestFull` for that pane
  (`Shared::request_full`) and on a requested Resync. At each of those points a queued patch for
  the affected panes is dropped too (`forget_delivered_terminal_state`,
  `forget_delivered_terminals_state`), since it was built against the base the client drops.
  With the queued patches gone, forgetting inside the lock that queues `Attached` needs no attach
  epoch: nothing queued before it can follow it with a stale base. This also drops the empty
  patch the watcher sends when a wake lists a view that did not change. A second source of duplicates was frames written before `Attached`:
  the watcher publishes a view as soon as the attach turns it on, and `send_attached` runs after
  the attach hooks. The four production attach paths (the `Attach` message, attach-session and
  new-session, switch-client, a destroyed session's survivor) hold the client's terminal lane from
  the start of the attach until `Attached` is queued (`hold_attach_terminals`); an attach error
  releases it.
- Item 3: the size alone is not enough. The TUI reports pixel sizes with each pane, and a
  pixel-only difference resizes the pty (`SIGWINCH`) and publishes again. The shared client code
  sends `client-cell-v1:WxH` next to `client-size-v1` (the 8x16 fallback moved from zz-tui into
  `zz_daemon::cell_pixel_extent`). `presize_client_terminals` seeds `terminal_geometries` for the
  attaching raw-terminal client's visible panes with the geometry the window takes at the
  client's size less its status block (`interactive_client_window_extent`), so the existing
  write-back and resize policy (window-size, aggressive-resize, ignore-size) runs as if the client
  had already reported. The attach queues the pane resizes before it turns the view streams on
  and attaches the views, and the control slot applies a resize before view commands, so the
  first frame of a stream epoch is built at the final size. An attach resync frame whose grid
  differs from the laid-out size is skipped (`attach_frame_superseded`). The cell size follows the
  client's later `ResizeTerminal` reports.
- Item 4: `recv_batch` takes ready frames in `recv`'s order up to 256 KiB, `attach::write_frames`
  writes them with `writev` and resumes a short write mid-batch, and `LocalStream` forwards
  `write_vectored` (before, the default wrote only the first buffer). `take_preview_refresh` runs
  once per written frame after the batch.
- Item 5: the TUI drains up to 256 queued protocol events and paints once (`PendingPaint`). A
  snapshot repaints everything only when `Model::paint_structure` changes (layout, dividers and
  their highlight, pane kinds and border colours, pane order, window, sidebar, status block) and
  an attach always does; a pane card keeps its own record and repaints when its text changes.
  Nothing is painted before the attach or in a terminal pane before its first frame. A forced
  paint clears with the default rendition and fills the theme background only in cells no painter
  writes (`fill_unpainted_cells`), and a pane still waiting for its first frame is remembered as
  blank, so that frame writes only its non-blank rows (before, a themed clear and then an `ECH` on
  every blank pane row, half of a one-pane attach's bytes). Borders are written as runs, one
  cursor move per run and a rendition only where it changes, instead of 45 bytes a cell; a border
  cell right below the last one is reached with a backspace and a line feed, as tmux's `cud1` is,
  unless the last one sat in the last column. A paint that only puts the cursor back where the
  last paint left it is not written. A `SIGWINCH` always sends `ClientTerminalSize` (tmux fires
  `client-resized` on every `MSG_RESIZE`; `attached-client.sh` checks it) and redraws the whole
  client as tmux does, sending pane resizes only when the size changed; a reply to the cell size
  query that leaves the grid as it was sends neither and repaints nothing (before, it sent
  `ClientTerminalSize` and so fired `client-resized` on every attach in a terminal that answers
  the query). The two terminal options: the daemon
  puts `server-option-v1:extended-keys=<v>` and `server-option-v1:focus-events=<v>` in the hello
  of an interactive client with a terminal, and the TUI arms from them. It falls back to the two
  `CommandClient` reads for a daemon that sends neither and for a new-session chain of more than
  one command, whose later commands may set them. A TUI on a remote host now arms from that
  host's options (before, the reads were local only and a remote TUI never armed).
- Knobs: `ZZ_PERF_ATTACH_DEDUP=0`, `ZZ_PERF_ATTACH_BATCH=0`, `ZZ_PERF_ATTACH_PRESIZE=0` and
  `ZZ_PERF_WRITEV=0` in the daemon (logged at startup next to the publication knobs; the client
  also reads `ZZ_PERF_WRITEV` for its buffered reads), `ZZ_PERF_TUI_COALESCE=0` in the CLI. What
  each restores is in the Rollback switches table.
- Tests: `daemon/attach_tests.rs` (18: switch away and back and re-attach, RequestFull at the
  delivered generation, first frame at the final size and none before `Attached`, kitty chunks
  before the placing frame in one `writev` and a short write, the hold and the batch, the dedup
  rules, a queued patch at each wipe point (the reviewer's three probes and a Resync), the hello
  options, the cell fact, no status render for the client a detach let go and one on its next
  attach, no status in a terminal client's hello). zz-mux: literal and glob update-environment
  names. Seven attach-sequence tests asserted the resync Snapshot or relied on it
  to overflow the mailbox; the startup-cause pressure tests now overflow on the causes, and
  `request_full_enqueues_only_the_requested_visible_pane` goes through `request_full`. zz-tui:
  paint structure, paint order, the waiting pane, border runs and the line-feed step, cursor-only
  paints, the hello options, the kitty probe sent only when something needs it. compat/tui
  fixtures on macOS with `LANG=en_US.UTF-8` (first pass): attached-client,
  overlays, launch-diff, screen-diff, caps, pane-geometry, mouse, indicators, copy-mode and
  superset pass; status-row (window name `tmux` against `bash`), client-commands (five cases,
  among them the `XT` flag), choosers and output-backpressure (the pin's side), stock-keys
  (`gcat` against `cat` with Homebrew coreutils first on `PATH`) fail the same way on the
  perf/wave1 head build, and command-streams on the pin's `select` loop with stdin closed.
  attached-client's replay check on the pin's screen timed out in two of four full runs and
  passed its command-output part three times alone on both builds. After the W1-EXEC merge the
  same fixtures give the same results, and the whole `compat/scenarios` corpus (254 scenarios)
  is clean apart from the registered `known/` tuples and six that diverge the same way on the
  W1-PANE and W1-EXEC merge builds: census-hooks, plugin-runtime-continuum,
  plugin-runtime-vim-tmux-navigator, source-file-byte-name, resurrect-save and
  status-background-jobs. A first build repainted a status row from its first changed column;
  the corpus reads the raw bytes a client writes (`display-menu-action-queue` looks for
  `Command list-p`, and the row shared its leading `C` with the message before), so a changed
  status row is written whole again. On Linux after the fix pass (`LC_ALL=en_US.UTF-8`: this
  host's `LC_TIME=pt_BR` makes the pin print Portuguese month names in every status row):
  attached-client, caps, choosers, client-commands, command-streams, indicators, launch-diff,
  output-backpressure, overlays, pane-geometry, screen-diff, stock-keys and superset pass;
  status-row (window name `tmux` against `bash`, and `/sbin` is a link to `/usr/bin` here so the
  detached `cd /sbin` step waits for `sbin` forever) and copy-mode (6 of 147 cases, the fresh-entry
  search prompt escape and cancel in vi and emacs) fail the same way on a perf/wave1 build; mouse
  timed out on the pin's side before its first case in 2 of 4 lane runs and 2 of 5 perf/wave1 runs.
  The reviewer's 21 scenarios, the six that were red on the Mac and eight environment, status and
  prefix scenarios are all clean on Linux.
- Fix pass (2026-09-29, on Linux, after the parity and perf reviews in
  `bench/perf/campaign/attach-review.json`):
  - `terminal_update_redundant`: only a queued Full covers an incoming Full; the wipe points drop
    queued patches (item 2 above).
  - `OutboundMailbox::hold_terminals` / `release_terminals` and `pop_ready_frame`: the attach holds
    the whole mailbox (`attach_batch`) until the publish after `Attached`, so `Attached`, the
    status and the frames already queued go out in one `writev`; a reliable queue half full lets
    go early. `AttachHold` releases on drop, so every attach path releases on error.
  - `Shared::register`: the hello of a raw-terminal or browser client carries no status. Neither
    paints one before it attaches, and the attach sends the real one with `Attached`; this removes
    one status render and one `format_option_snapshot` per attach.
  - `Shared::publish_snapshot_after_detach` (from `detach_with_event_hooks` and `unregister`):
    the publish after a detach leaves the detached client out of its status render and forgets
    its status record, so a later attach on the same connection gets its status again.
  - `attach::spawn_writer`: the outbound writer runs on the pooled connection threads (distinct
    daemon threads over a 6 s four-pane attach loop: 41 on perf/wave1, with 10 writer threads, 23
    on the lane), and accepted sockets get `SO_SNDBUF` = `MAX_BATCHED_WRITE_BYTES`. `attach::inbound_reader` and
    `ProtocolReceiver`: the daemon reads a connection through an 8 KiB buffer after the hello and
    the client through 64 KiB, so a frame is no longer a length read plus a body read.
  - `TerminalSession::resize` skips a geometry equal to the last one requested
    (`ControlSlot.requested_geometry`; the actor's geometry changes only through that slot) and
    `CommandSender::with_slot` wakes the pane thread once per drained slot (`wake_queued`).
  - `apply_client_environment_update`: an update-environment pattern with no glob characters
    reads its own entry (`GlobPattern::literal`, a range over the client environment's names), and
    `GlobPattern::matches` rejects on a literal prefix without allocating. Patterns were already
    compiled once per array.
  - zz-tui: the kitty graphics probe and its temp file go out at start only to terminals known to
    carry kitty graphics (`supports_kitty_graphics`: ghostty, kitty, wezterm, konsole, zz), and to
    any other terminal once an image or a browser pane needs it (`KittyProbeState::Idle`,
    `TerminalGuard::probe_kitty_graphics`, fenced by its own `DA1`). The rest of the attach query
    burst (`DA1`, `DA2`, `XTVERSION`, the theme subscription) is what tmux's `tty_start_tty` and
    `tty_send_requests` send; `CSI 16 t` stays for the cell size.
  - The per-key input diagnostics (`key_decision`, `prefix_armed_published`) log at debug instead
    of info: every attach cycle formatted two lines for the prefix and the detach key.
  - Comments the lane had added to Rust sources were removed (house rule).
- Measured on Linux (alienware, i7-11800H, tmux 3.7c, load 4-6 on 16 CPUs while another lane
  compiled; `--quick --only attach,echo,throughput,chatty`, user-space instructions). Base is a
  perf/wave1 build at a41b1fbf in the same session (it has the THP fix; the committed fold JSON
  `w1-5-fold-alienware-166b8f95.json` does not, and its `attach.cpu` carries THP page faults:
  10.54 ms at p4). Base -> lane (tmux in the same runs):
  `attach.tty_total` 175762 -> 504 B at p1 (tmux 977) and 344894 -> 1449 B at p4 (tmux 3655);
  `attach.instr` 15.9 -> 10.7 Minstr at p1 (tmux 9.09) and 16.3 -> 11.1 at p4 (tmux 12.1);
  `attach.cpu` 4.27 -> 2.96 ms at p1 (tmux 1.71, 1.74x) and 5.30 -> 4.19 ms at p4 (tmux 2.05,
  2.05x; the fold read 4.0x); `attach.ttfc` 7.65 -> 5.13 ms at p1 (tmux 4.03) and 11.9 -> 5.77 ms
  at p4 (tmux 4.89); `attach.conns` 4 -> 2; `attach.wire_frames` 20 -> 6 and 23 -> 9;
  `attach.wire_s2c` 101082 -> 100064 B and 101254 -> 100236 B (the 100 KB is the 28 KB hello and
  the Full frames, owned by W2-CTRL and W2-TERM; the drop from 257 KB at W0 came with W1-PANE and
  W1-EXEC); `chatty.tty_kibps.hidden` 2.08 -> 0.53 KiB/s (tmux 0.38-0.62 across the runs). The same
  binary with every ATTACH knob off: 175566 B and 245294 B, 4 connections, 15.1 and 15.5 Minstr,
  1.47 KiB/s hidden. Every wave1 rule in the attach and chatty groups passes. The echo rows fail at
  1.8-2.2x tmux on the lane, the base and knobs off alike (no wave-1 owner), and
  `throughput.detached.ascii` fails its 4x rule on all three (1.5x at this load; 1.71x in the quiet
  fold run): the Linux ceiling in the handoff. Measured with an attach loop against the release
  builds: the fix pass alone took the lane from 14.5 to 10.6 Minstr at p1 and from 14.9 to 11.1 at
  p4, and 662 -> 504 B and 1875 -> 1455 B. Socket calls per four-pane attach and detach, counted
  with an `LD_PRELOAD` shim: daemon writes 28 `send` -> 6-7 `writev` and 1 `send` for the same
  101 KB, daemon reads 27 -> 10, TUI reads 63 -> 9-10, TUI tty writes 16 -> 6. `SO_RCVBUF` on the
  client does nothing on Linux (an `AF_UNIX` stream queue is charged to the sender's
  `SO_SNDBUF`, 212992 B by default, so a 100 KB attach already fits one `writev`); the buffered
  read is what cut the client's calls. The Mac numbers of the first pass are in
  `attach-review.json`; its 0.375 KiB/s hidden figure did not reproduce there (0.6-1.05) and is not
  used.

The daemon's attach and detach cycle after the fix pass (four panes, `perf record -e cycles:u
--call-graph lbr` over a 60-cycle loop): pane threads building the Full frames 33% (`run_terminal`,
`Frames::snapshot`, `build_snapshot`), the status render at attach 10% plus
`format_option_snapshot` 5-10%, the detach key's command path 9% (hook captures, the detach
itself), the pane watcher 7%, the hello's key tables (clone and encode) 4.5%, `attach_target`
4.4%.

Handed on:

- One 28 KB `ServerHello` remains in every TUI attach, most of it key tables, cloned and encoded
  per hello (4.5% of the attach cycle's daemon CPU) (W2-CTRL's `Welcome`).
- Full frames are built per pane on every attach even when the pane did not change since the last
  attach (a third of the attach cycle's daemon CPU) and are 8 bytes a cell on the wire (W2-TERM,
  W4-ROWS, W4-DELIVER).
- Every status render builds a whole-server `format_option_snapshot` (W2-FMT); the attach now
  renders once per attached client and the detach not at all for the client that left.
- The detach key runs through hook captures before and after the command (W2-HOOKS); the
  watcher's foreground lookup per wake (W1-PUBLISH follow-up, W4-DELIVER).
- The first frames reach the TUI after its first paint (the chrome and the status), so an attach
  is two synchronized updates, about 24 bytes of framing; holding the batch until every visible
  pane's first frame would need a timeout for a pane that never sends one.
- The watcher still diffs every view a wake lists, changed or not; the mailbox drops the empty
  patch (W4-DELIVER).
- The TUI's status and prompt rows write runs of blanks as spaces. `ECH` would cut a status repaint
  by about half, but under `capture-pane -e` differentials an erased cell loses its foreground
  and `tui-overlays.sh` and `status-row.sh` would diverge; tmux writes the spaces too.
- A snapshot still goes to a TUI that is exiting after `Detached`; the daemon cannot tell it from a
  browser client, which stays connected and needs it.
- macOS-only, not measured here: the 8 KiB `net.local.stream.sendspace` effect on the writev
  batches (the reason for the daemon's `SO_SNDBUF`), `just ios-gpui iPad build` for the two hello
  capability constants.

Merged onto the five lanes above and the Linux THP fix (`perf/wave1`, merge `ce1b34cd`, a clean
merge). Merge gate (`w1-6-attach-alienware-ce1b34cd.json`, full, `--strict`, load under 1 on 16
CPUs), fold -> merge, tmux in the same run: `attach.tty_total` 175762 -> 504 B at p1 (tmux 977)
and 344894 -> 1435 B at p4 (tmux 3655); `attach.instr` 15.8 -> 10.7 and 16.3 -> 11.1 Minstr (tmux
9.09, 12.1); `attach.cpu` 6.20 -> 3.46 and 10.5 -> 4.99 ms (tmux 2.03, 4.07); `attach.ttfc` 11.6
-> 8.3 and 16.8 -> 9.2 ms (tmux 7.9, 9.6); `attach.conns` 4 -> 2; `chatty.tty_kibps.hidden` 1.00
-> 0.54 KiB/s (tmux 0.38). 59 pass, 11 fail, 0 regressed, 0 drifted; no failing row is owned by
this lane. The footprint rows moved with the THP fix, not this lane (the pre-merge binary reads
the same). An A/B against the pre-merge binary shows no change in `spawn.instr.*`,
`chatty.instr_per_s.*` or the echo rows (p50 idle 1.94-1.96 vs 1.97-1.99 ms, tmux 0.83-0.91).

Stale-paint fix (W1-ATTACH-PAINT, `perf/attach-paint`, Linux, 2026-09-29): at the wave-1 exit
`tui-screen-diff.sh` found 13-14 of 147 checkpoints with a stale pane row per release run. Cause:
a drained run of events can take the frame inbox more than once before it paints, and
`Renderer::note_frame` replaced the damage still pending for a pane, so a row that only the
earlier frame changed was never written (before the drain every take was painted at once). Fix:
`note_frame` folds the new damage into the pending damage with `merge_damage`, as
`FrameInbox::publish` already did; zz-tui test
`a_drained_run_of_frames_paints_every_row_any_of_them_changed`. After it screen-diff matched 147
of 147 in 5 release and 3 debug runs, and the attach and chatty gate reads the same (504 B at p1,
2 connections, 0.30 KiB/s hidden). Under a concurrent cargo build, `unzoom` at 80x10 and 80x6 can
still differ, the same way on the pre-ATTACH build: the daemon's own `list-panes` shows the wrong
geometry (a window one row short, a hidden pane left at an old size), so that one is a daemon-side
resize race and not a paint.

## W2-TERM: PaneFrame terminal lane (effort L)

Scope:
1. terminal_codec.rs `PaneFrame`: varint pane, per-pane stream sequence, base, generation; flags
   byte for metadata present (title, cwd, cursor, modes, scrollbar, search, size, dictionary
   append); rows as varint y, x0, then runs of (style id, attrs, length, UTF-8), trailing default
   blanks dropped, scroll-shift op. Full = base 0, all rows. CommandOutput, HistoryChunk and
   previews use the same rows. Delete the 8 B/cell Full and Patch encoders and decoders.
2. zz-terminal model.rs `diff_with_scratch`: changed column spans, not whole rows.
3. daemon.rs `publish_terminal_for_pane`: encode once per (pane, base), reuse for every subscriber
   at that base; per-pane stream sequence instead of `next_sequence`; `send_full`, `send_history`,
   `enqueue_terminal_viewport`, `enqueue_terminal_viewport_preview`, `replace_terminal_viewport`.
   Latest-wins and NeedsFull unchanged.
4. Clients decode into `PackedCell` planes: zz-client core apply/decode; zz-tui
   terminal_event.rs, render.rs; crates/zz mux terminal and HistoryChunk decode (keep
   `history_requests_pending` across tree changes); gpui-shared terminal pane.
5. `knowledge/protocol/terminal-lanes.md`.

Write zone: terminal_codec.rs terminal lane; message.rs `EventPayload` terminal variants;
model.rs diff and patch types; daemon.rs functions in item 3 and terminal sequence allocation;
zz-client core terminal fns; zz-tui terminal_event.rs, render.rs; crates/zz mux terminal decode;
gpui-shared terminal decode; terminal-lanes.md.

Gate vs wave-1 exit JSON: throughput rows; headless-client throughput (TODO in W0) >= 0.85x W0;
`echo.wire_bytes.idle` <= 64 B; terminal bytes in `attach.wire_s2c.p4` <= 15 KB; chatty CPU no
worse. Tests: codec round-trip and property tests (random grids, wide cells, graphemes, hyperlinks,
dictionary growth), zz-client, zz-tui, GUI mux client tests, `tui-screen-diff.sh`,
`tui-output-backpressure.sh`, `tui-copy-mode.sh`, `just web-build`, `just ios-gpui iPad build`,
`bench/run.sh`.

Expected (re-encoded captures): patches 15x smaller, Full 10x, blank screens ~140x, echo ~30-40 B.

As built (branch `perf/term`, on perf/wave2 `c65e49f0`, Linux only), where it departs from the scope
above:

- Item 1: the codec is its own module, `zz-protocol/src/pane_frame.rs`; `terminal_codec.rs` only
  routes to it. The metadata flags are a varint bitset, `zz_terminal::TerminalPatchFields`
  (15 bits), shared by full frames (absent means the default) and patches (absent means unchanged):
  rows, cursor move, scroll, scrollbar, dictionary append, overlays, cursor, presentation (title,
  cwd, hovered URI), colours, mode, search, unseen output, input modes, status, kitty placements.
  Size is always sent: a patch never changes it (the diff returns `None`), and the decoder needs
  it to bound every span. A cursor that only moved is its own bit (`CURSOR_AT`, 2 bytes), which
  is what keeps an echo small. Rows are a row step (0 ends the section), `x0 << 1 | clear`, then
  runs whose header is `count << 3 | style << 2 | kind`: text (one UTF-8 scalar a narrow cell),
  wide (one scalar a wide head plus its spacer tail), repeat (one cell, 6 or more times) and raw
  (glyph code and flags a cell, for graphemes, spacer heads and odd flags). The style carries over
  from the previous run and starts at 0 each row. Generations travel as one varint and zigzag
  deltas. `terminal-lanes.md` has the byte layout.
- Item 2: `diff_with_scratch` sends one span a changed row, from the first to the last changed
  column, or up to the last non-blank cell with `clear` when the change empties the row's tail;
  rows exposed by a scroll are sent whole (`start 0`, `clear`). A patch carries only the metadata
  that changed; `apply_patch` keeps the retained value for the rest, and a dictionary append is
  the only case that puts `style_base`/`grapheme_base` on the wire. Clients that read patch
  metadata before applying it use `carries`, `scrollbar_after` and `cursor_after`.
- Item 3: terminal frames, command-output frames and history chunks take the daemon's event
  sequence (`Shared::next_sequence`). The first cut gave each `TerminalSession` its own counter,
  which started again at 1 on `respawn-pane` under the same `PaneId`; one daemon-wide counter never
  goes back, needs no field on the session, and orders a pane's frames against every later event
  about the pane, which is what W4-DELIVER's barriers compare. **Encode once per (pane, base)**,
  as far as this lane can take it: views that are live at the bottom already share the actor's
  cell plane and dictionary (the second and later views of a publish find the render state clean
  and return the same `Arc`s); only their generations differ, because `build_snapshot` bumps
  `generations.content` and `.view` once per view. So every client's patch had the same spans and
  the same bytes after a header of about 12 bytes, and the watcher diffed, fingerprinted and
  encoded it once per client (+20.2 Minstr/s of daemon work per extra TUI on visible chatty).
  `TerminalViewport::diff_shared` keeps the row shift and spans in the watcher's scratch for the
  next view on the same two grids (keyed by the two cell planes and the two dictionaries, held
  until `release_shared` at the end of the frame, so a recycled plane can never match), and
  returns a `TerminalPatchRef` that reads the changed cells from the current plane instead of
  copying them into a boxed patch (the two allocations a patch made per client are gone). The
  encoder writes each client's header, fields and metadata, then copies the dictionary append and
  span section from the first client that encoded it (`PatchTail`, keyed by the diff). The
  watcher diffs a view only after `publish_terminal_for_pane` found the client attached, not
  frozen and streaming the pane. The fingerprint cache also serves a view whose current grid is
  cached but whose base is not. The encoders stay callable alone
  (`encode_terminal_viewport_event_into`, and `encode_terminal_patch_event_into`, which now takes
  the borrowed patch and the tail) for W2-CTRL's `Batch`. The preview mailbox no longer sizes a
  frame before encoding it (only the old fixed layout could); it encodes and checks the byte
  budget after, as the foreground path did.
- Item 4: the decoder produces the same `TerminalViewport`, `TerminalViewportPatch` and
  `HistoryChunk` values, so zz-tui (its `terminal_event.rs` is the tty input decoder and has no
  frames), gpui-shared, zz-client-ffi and the renderers did not change. zz-client core's damage and
  the GUI's retained-patch history logic now read the patch fields. HistoryChunk keeps its
  in-memory `Vec<Vec<PackedCell>>` and moved from postcard to the Terminal lane (kind 3). Chooser
  previews and a postcard-encoded `CommandOutput` carry the full-frame body as one postcard byte
  string (`pane_frame::viewport_bytes`). The GUI keeps `history_requests_pending` across tree
  changes: every `Snapshot` used to clear it and re-request, so a chunk in flight arrived with no
  request, was dropped, and was asked for again (twice the history bytes during a backfill that
  overlaps any tree change). A pane that stops being visible gets a Full when it returns, which
  resets its request; `PaneRemoved` forgets it (`a_tree_change_keeps_the_history_chunk_in_flight`).
  The daemon sends no reply when `history()` fails (a full control queue or the 2 s capture
  timeout), so a request older than 3 s counts as lost and the next backfill or scroll-up sends it
  again (`a_history_reply_that_never_came_is_requested_again`).
- Not in the brief: decoded grids are capped at `MAX_FRAME_BYTES / 8` cells (8 Mi), the bound the
  8-byte layout had implicitly, since a compact frame could otherwise declare a 65535x65535 blank
  grid in a few bytes. The diff compares rows eight cells at a time as `u64` words, hashes row
  fingerprints in four lanes, searches row shifts nearest first and stops once no shift can win
  (it scanned all `2 x rows` shifts), and skips the dictionary prefix checks when both frames share
  one dictionary; these cut the watchers' diff share of the visible chatty profile.
- Knob `ZZ_PERF_ROW_PATCHES=1` widens every span to its whole row (the pre-W2 granularity) on the
  same wire, for bisecting a span-apply bug (`TerminalDiffScratch::set_whole_rows`). It fails
  `echo.wire_bytes.idle` by design (about 80 B, the whole prompt row). The 8-byte frames have no
  knob.
- Mixed builds inside 107: the daemon's `ServerHello` names `pane-frame-v1`. An interactive
  client whose daemon does not stops at the handshake with a same-version `VersionMismatch`, which
  the TUI, the GUI and the CLI already turn into "restart the daemon" (the messages now say "an
  older build of protocol v107" when both sides report 107); command and control clients connect
  as before, so `zz kill-server` still reaches the old daemon. Clients built before the capability
  cannot tell. The web client checks nothing: the gateway relays bytes and the browser decodes the
  hello in zz-client core, so a stale daemon behind `just web-serve` still shows decode errors.
- W0 TODO, partly built: `crates/zz-client/examples/perf_client.rs` is the frame-sink client
  (attach, decode and apply every frame with `ClientCore`, render nothing) and the throughput group
  times the ASCII flood through it as `throughput.headless.ascii_ms` (zz only, info: the W0 JSONs
  predate it, so its ">= 0.85x W0" rule has no reference). The GUI-like mode (all sessions, history
  backfill, 8 visible panes) is still TODO.
- Tests: `zz-protocol` `pane_frame_tests.rs` (16: seeded random grids with ASCII, wide pairs,
  graphemes, hyperlink and classed styles, spacer heads, odd flags and repeats through full frames
  and patches with scrolls and dictionary growth; every truncation; hand-built frames that lie about
  rows, runs, styles, graphemes, counts, fields and grid size; echo and blank-screen size bounds;
  history chunks; postcard previews; cursor moves; kitty placements; every patch they build is
  also encoded from the borrowed frames, and again for a sibling view from the shared tail, and
  must match the owned encoding byte for byte), `pane_frame_fuzz_tests.rs` (from the parity
  review: 400 seeds of 80 chained steps where every metadata field, scroll, dictionary growth,
  dictionary reset and resize change at random and a client that only applies decoded frames must
  equal the daemon after every step, through the owned, the borrowed and shared and the whole-row
  paths; mutated full, patch and history frames; 200k random byte strings; edge grids),
  zz-terminal model tests for spans, whole-row widening, the chunked row scans, the shared diff
  (`views_on_the_same_two_grids_share_one_cell_diff`) and the fingerprint reuse
  (`a_view_on_the_cached_grid_fingerprints_only_its_own_base`), and `daemon/pane_frame_tests.rs`
  (4: an echo through a real pane is a patch of at most 64 bytes and the client's retained grid
  equals the daemon's frame; history chunks ride the terminal lane with blank tails dropped; a
  pane's frame sequence keeps growing across `respawn-pane`; and, from the parity review, two
  attached clients that apply every streamed frame equal the daemon's view field for field after
  typed keys, SGR, wide and combining text, OSC 8/2/7, scroll regions, insert and delete, the
  alternate screen, cursor styles, mouse mode, copy mode, split, resize, zoom and respawn). zz-daemon
  `an_interactive_client_refuses_a_daemon_without_pane_frames` covers the handshake. The old
  terminal-lane byte-layout tests went with the layout.

Measured on Linux (alienware, tmux 3.7c; `--quick --only attach,echo,throughput,chatty` against the
quick Linux W0; before is the wave-2 base `c65e49f0`, after is this branch; load 1.6-3.9 on 16 CPUs
with another lane idle or compiling, so wall and CPU rows are notes and the host changed power state
between the two runs, tmux included):

| Metric | Before | After | Rule |
|---|---|---|---|
| `echo.wire_bytes.idle` | 1,133 B | 31 B | <= 64 B, passes |
| `echo.wire_bytes.busy30` (info, includes the 30 Hz ticker) | 3,399 B | 137 B | |
| `attach.wire_s2c.p1` / `.p4` | 100,064 / 100,236 B | 29,400 / 29,674 B | 8 KB at wave 2, W2-CTRL's |
| terminal frames inside the p1 / p4 attach | 70,725 / 70,828 B (four 17-18 KB Fulls) | 62 / 265 B (four 66-67 B Fulls) | <= 15 KB, passes |
| `attach.instr.p1` / `.p4` | 10.7 / 11.1 Minstr | 10.2 / 10.6 (two reruns; one run read 14.0 at p1 with a 24 Minstr outlier) | |
| `chatty.instr_per_s.flip` / `.hidden` | 34.4 / 38.9 | 34.3 / 36.6 Minstr/s | no worse |
| `throughput.detached.ascii` | 95.3 MB/s (ceiling 125) | 88.1 MB/s (ceiling 116) | 1.75x tmux, the Linux ceiling as before |
| `throughput.headless.ascii_ms` (new) | - | 1,761-1,784 ms (detached is about 1,600) | info |

Full-group A/B against the base binary in the same session (`--only chatty,throughput`, then
`--only chatty` on the final build): `chatty.instr_per_s.visible` 391.8-392.0 -> 383.8 Minstr/s
(tmux 157-160), `chatty.client_cpu_pct.visible` 2.52 -> 1.81% in the first pair and 2.22 -> 2.27%
in the second (the TUI's CPU over 10 s moves that much between runs of one binary),
`chatty.tty_kibps.visible` unchanged at 415-418 KiB/s, `throughput.attached.ascii_ms` 2,081 ->
2,092 ms (tmux 3,922 / 3,740), `throughput.detached.unicode` 43.9 -> 42.2 MB/s (8.5-8.9x tmux).
`chatty.cpu_pct.visible` still fails its wave-1 rule (2.3-2.5x tmux) on both builds.

Profile of the daemon under the visible chatty workload (`perf record -e cycles:u` for 6 s, four
90x24 panes printing through a TUI): the pane actors are 84% of the samples, almost all of it
building frames out of libghostty (`build_snapshot` 30%, `ghostty_render_state_row_cells_get`
12%, style interning, row and cell iterators), which is W4-ROWS and W3-SHARDS. The four watchers
were 17.3% on the first cut, with this lane's diff and encode about 7.8% (`best_row_shift` 4.0%,
the row compare 2.6%, encoding 1.2%); after the diff changes above they are 13.8%, and the lane's
path is about 5% (row compare and shift search 2.1%, row fingerprints 1.5%, encoding 0.9%, the
mailbox 0.4%). Under a flood watched by the headless client the watcher is 1.2% and the actor 94%
(parsing, and page faults the unpushed PageList fork fix removes).

Checks on Linux: `cargo fmt`; clippy `-D warnings` on zz-terminal, zz-protocol, zz-daemon,
zz-client, zz-tui, zz-cli, zz-client-ffi, zz-web, zz, zz-mux and zz-config (all targets, all
features); tests of zz-terminal, zz-protocol, zz-client (with the daemon-backed simulator), zz-tui,
zz-client-ffi, zz-web, zz-cli and zz pass; zz-daemon passes except the known
`endpoint::tests::remote_scripts_fall_back_to_the_mac_app_bundle_cli` and one russh port test
that passes alone. `compat/tui-screen-diff.sh` 147/147 three times on the first release build
and three times on the final one;
`attached-client.sh`, `tui-choosers.sh` (previews ride the new postcard body),
`tui-output-backpressure.sh`, overlays, pane-geometry, caps, launch-diff, indicators, superset,
command-streams and stock-keys pass; copy-mode (the six fresh-entry search prompt cases),
status-row, mouse and client-commands (`switch-mode-duplicate-windows`, `sh` against `bash`, the
same on the base binary) fail as on the base. `compat/run.sh` over the whole corpus
(`LC_ALL=en_US.UTF-8`, debug build): only `lane2-store`, `show-options-hooks` (the pin's `vlock`
`lock-command`) and `smoke/plugin-runtime-resurrect-restore` fail twice, the three host rows the
wave-1 merge recorded, and the `known/` rows keep their documented divergences.
`compat/tui/tracker.py check` passes. `compat/wire-version.py`: 107 unreleased.
`just web-build` builds. Not run here: `just ios-gpui iPad build` (needs the Mac; gpui-shared
decodes through zz-client core and did not change), and `bench/run.sh` (it drives the packaged GUI
app in a window, not the daemon; the gate's throughput rows and the headless client cover the
wire).

Fix pass (after both reviews, on perf/wave2 `c3d48b63`, so both builds link Ghostty `713374af`):
the per-client work above, the review minors, and the reviewers' tests. Daemon user instructions
under visible chatty (four 90x25 panes printing, N attached TUIs, three 5 s windows a run, two
interleaved rounds; before is the first cut merged with the same perf/wave2):

| Clients | Before (Minstr/s) | After (Minstr/s) |
|---|---|---|
| 1 | 384.7 / 383.9 | 380.6 / 383.7 |
| 2 | 404.7 / 404.2 | 389.0 / 389.0 |
| 4 | 444.7 / 443.1 | 398.7 / 399.0 |
| each extra client | +19.9 | +5.6 |

A profile of the same workload with 1 and 4 TUIs (`cycles:u` at 15 kHz, profiling build): the
watchers go from 12.2% to 14.3% of the daemon's cycles (the review measured 15.4% to 22.9% on
the first cut); `row_fingerprint`, `encode_runs`, `push_scalar` and `check_cell` do not grow with
the client count (the review had `row_fingerprint` at 0.95% then 4.71%); what a view still adds on
the watcher is its metadata compare (`diff_shared` +0.3 points), its header, metadata and tail copy,
and the mailbox and `publish_terminal_for_pane` lock traffic that W4-DELIVER deletes. The actors'
share of the extra cost is a snapshot per view (W3-SHARDS).

Quick gate (`--quick --only attach,echo,throughput,chatty` against the quick Linux W0,
`fixed.json` in the lane's report directory): `echo.wire_bytes.idle` 32 B (31 on the first cut:
the event sequence of a fresh daemon is a byte longer than a fresh per-terminal counter; both
grow with uptime), `echo.wire_bytes.busy30` 137 B, `attach.wire_s2c.p1` / `.p4` 29,415 / 29,692 B
(+15 and +18 B for `pane-frame-v1`), `attach.instr` 10.2 / 10.6 Minstr and `chatty.instr_per_s`
34.3 / 35.6 Minstr/s unchanged, `throughput.detached.ascii` 134.1 MB/s against a same-run pty
ceiling of 130.1 (tmux 55.0; it passes now because perf/wave2 brought the PageList pin),
`throughput.headless.ascii_ms` 1,306 ms. 10 pass, 6 fail, 0 regressed, 0 drifted; the failures
are `attach.conns` and `attach.wire_s2c` (W2-CTRL) and `attach.ttfc.p4` / `attach.cpu.p4`, wall
and CPU rows that failed the same way on the wave-2 base.

Fix-pass checks on Linux: `cargo fmt`; clippy `-D warnings` on every crate listed above; tests of
zz-terminal (313), zz-protocol (252), zz-client (167 and the daemon-backed simulator), zz-tui (229),
zz-mux (586), zz-client-ffi, zz-web, zz-cli and zz (583) pass; zz-daemon 1129 of 1133, the known
`remote_scripts_fall_back_to_the_mac_app_bundle_cli` plus two `process_info` exec-name tests and
the russh port test, which pass alone 3 of 3. The two-client daemon test passes three times plain
and twice with `ZZ_PERF_ROW_PATCHES=1` (the knob reads 80.5 B on `echo.wire_bytes.idle`, as the
rollback table says). Release build, with other trees' compiles paused: `tui-screen-diff.sh`
147/147 three times, `attached-client.sh` PASS, choosers 78/78, overlays 48/48,
output-backpressure 9/9, copy-mode 141/147 (the six fresh-entry search prompt cases of the base);
`diff-scenario.sh` on capture-pane, the eight copy-mode scenarios, zoom, resize, resize-window,
switch-client, alerts, panes, display-panes-format, pane-dead-time and windows: 18/18.
`just web-build` builds. `compat/tui/tracker.py check`, `compat/wire-version.py` (107 unreleased),
`bench/perf/test_gate.py` and the OKF validator pass. Not run: `just ios-gpui iPad build` and
`bench/run.sh`, for the reasons above.

Handed on:

- W2-CTRL: the 28 KB `ServerHello` is now 95% of an attach's bytes. `Batch` can carry the
  enveloped PaneFrames the two public encoders write, or the bare full-frame body
  (`pane_frame::encode_viewport_body`, crate-private today).
- W3-SHARDS: one generation per publish for every plain live view of a pane. The actor already
  shares the cell plane and dictionary between those views; only `build_snapshot`'s per-view
  `generations.content += 1` / `.view += 1` keeps their bases apart, so each client's patch still
  needs its own header (base and generation deltas) and W4's per-(pane, base) cache could never
  hit. With one generation per publish the views' frames become equal, and the actor can skip
  building a second snapshot for them at all (the 263 M cycles the three extra views cost the
  actors in the perf review's 4-client profile). A clean-row hint from `build_snapshot` would let
  the diff skip rows the actor did not re-extract (the row compare is most of the diff for in-place
  updates); W3-SHARDS or W4-ROWS owns both ends of that.
- W4-DELIVER: frames carry the daemon's event sequence, which only grows, also across a respawn,
  and orders a pane's frames against every later event about the pane, ready for "after seq N"
  barriers. Each client's frame takes its own number today; a shard that encodes one
  `Arc<[u8]>` per (pane, base) for every sink needs one number per pane frame instead (the
  sequence sits in the header). Until then `diff_shared` and `PatchTail` are the shared part: one
  diff and one span encoding per pane per frame.
- W4-ROWS: the profile above is the "does row extraction show" evidence: it does, at about 60% of
  the actor's samples in visible chatty.

Merged into `perf/wave2` as `d7e3fc95` (w2-2, Linux) after merging perf/wave2 into the lane
(`ebdf8591`: the knob log lines of both lanes kept, the rollback and test tables joined), plus
`8f18e4d7`, which fixes two tests that only failed under `cargo test --workspace`: the layout
contract assumed smallvec without its `union` feature, which the workspace turns on
(`TerminalPatchRowData` is 56 bytes there, 64 alone), and the two-client daemon test compared
panes a client no longer streamed (zoom hides a pane while its actor is mid-publish; the actor
keeps that view's last snapshot, which the daemon rightly never sends, until its next publish).
The strict gate `w2-2-term-alienware-d7e3fc95.json` against w2-1 ran on a loaded host (a game
used two cores, load 3.3 to 7.1), so its CPU, wall and throughput rows moved for both muxes; the
owned byte and instruction rows are exact: `echo.wire_bytes.idle` 1,133 -> 33 B (rule 64),
`echo.wire_bytes.busy30` 3,399 -> 137 B, `attach.wire_s2c.p1` / `.p4` 100,064 / 100,237 ->
29,417 / 29,691 B (the rest is the `ServerHello`, W2-CTRL), `attach.instr.p1` / `.p4` 10.62 /
11.08 -> 10.09 / 10.56 Minstr, `chatty.instr_per_s.visible` 389 -> 381 Minstr/s. Alternating
runs against the pre-merge binary on the same host put `chatty.cpu_pct.visible` at 10.8% against
11.2% and every other CPU row within noise; `throughput.detached.ascii` stayed at the pty ceiling
measured in the same run (56.5 MB/s against a 56.1 MB/s bare reader, tmux 24.1).

## W2-CTRL: control plane v2 (effort XL)

Scope:
- Wire (in 107): `Hello` with viewport, subscriptions {tree: None|Attached|All, status, options
  bitset, keys: None|Hash|Full, default pane stream}, optional Attach op, environment blob;
  `MuxOptionKey` appends ExtendedKeys, FocusEvents; `ServerHello` -> `Welcome {version,
  server_id, client_id, instance_id, caps u64}`; `Batch`, `TreeDelta{base, version, ops}` +
  `TreeSync` on base mismatch, `ClientView`, `KeyTablesChanged{hash, mouse bitset}`,
  `GetKeyTables`; in-place upgrade after `ExecExit{resume: Attach}`.
- **Key subscriptions**: GUI, iOS and web subscribe `keys=Full` and receive per-table deltas
  (bind-key touches one table); they read `prefix_bindings` on every input route. Hash-only (+
  mouse bitset) is for TUI. Command-only and control clients request no key state.
- **Control mode**: `%layout-change` and window notifications stay hook-driven (W2-HOOKS owns the
  source; `DEFERRED_CONTROL_NOTIFICATIONS` ordering after `%end` preserved). New: control lines go
  as one `ExecRequest` body over the interactive connection, answered by `CommandResponse` frames
  and one `ExecExit` (W1-EXEC built no `ExecOutput`), instead of `prepare_commands` + execute (1
  round trip per line).
- Daemon: `Shared::register` / `register_subscribed` build only `Welcome`; `send_attached` +
  `send_resync_inner` produce one `Batch` (tree, ClientView, status, options, each visible pane's
  Full at final size); `publish`, `publish_to_control_clients`, `enqueue_reliable`,
  `enqueue_encoded_reliable` encode once and push `Arc<[u8]>`; `publish_mux_snapshots` sends a
  per-scope TreeDelta and nothing on an empty diff; `stamp_snapshot_for_client` produces only a
  ClientView; status output joins the flush Batch; agent stream fanout encodes once.
- Clients: zz-daemon `InteractiveClient` (connect + attach in one round trip); zz-client core
  (Welcome, TreeDelta into `MuxSnapshot`, Batch as one event group, key tables by subscription);
  zz-tui lib.rs, app.rs (delete tty.rs option reads and the preflight); zz-cli `ExecResume`
  upgrade and control lines; crates/zz mux/client.rs, lib.rs, config/mod.rs caps,
  workspace/which_key.rs; gpui-shared connection.rs; zz-client-ffi hello and caps;
  web and iOS rebuild; `knowledge/protocol/wire-protocol.md`, `snapshots.md`.

Write zone: message.rs hello/Welcome/Snapshot/KeyTablesChanged/new messages/MuxOptionKey;
tree_delta.rs TreeDelta ops; daemon.rs functions above, `handle_connection` interactive branch,
`serve_exec` upgrade hand-off; client.rs `InteractiveClient`; zz-client core; zz-tui lib.rs,
app.rs, tty.rs; zz-cli lib.rs, control_mode.rs command submission (not notification building);
crates/zz mux/client.rs, lib.rs, config/mod.rs, workspace/which_key.rs, workspace/view.rs,
workspace/new_session.rs, command/palette.rs, config/settings/multiplexer.rs; gpui-shared
connection.rs, command_palette.rs; zz-client-ffi ffi.rs, settings.rs, settings/mobile.rs;
agent/fanout.rs encode path; protocol docs.

Gate vs TERM JSON: `attach.ttfc.p1` <= 1.1x tmux; `attach.conns.p4` = 1; `attach.cpu.*` <= 1.25x
tmux; `attach.wire_s2c.p4` <= 8 KB; bind-key <= 64 B for hash subscribers; rename in an unattached
session <= 100 B per client; `control.latency` <= 1.2x, `control.burst_cmds_per_s` >= 0.8x tmux;
`chatty.client_cpu_pct.visible` <= 3x + 1% (1% on the Mac where tmux reads zero),
`chatty.tty_kibps.visible` <= 1.5x; cli and throughput no
regression. Tests: zz-client simulator convergence, zz-client-ffi + C integration client, daemon
clients and overlay tests, `cargo test -p zz`, every compat/tui fixture, control-mode fixtures
(`%begin`/`%end`, `%layout-change`), `compat/run.sh`, `just web-build`, `just ios-gpui iPad build`.

Expected: attach ~10 round trips -> 1; hello 28.3 KB -> ~40 B + subscribed state;
KeyTablesChanged 26 KB -> ~20 B for hash subscribers; tree change 20-60 B, encoded once;
attach CPU 26 -> <= 5 ms.

As built on `perf/ctrl`, 2026-09-30:

- `Hello` carries viewport, subscriptions, an optional session or command attach, and the
  raw NUL-separated environment once. `Welcome` carries identities and capability bits.
  Both `pane-frame-v1` and `control-plane-v2` are required inside unreleased protocol 107.
- `send_attached` and `send_resync_inner` collect one flat `Batch` containing the scoped
  tree, `ClientView`, requested status/options/keys, image chunks, and each visible pane's
  Full at final size. Typed queued groups expand into the collector without decoding
  TERM bodies or copying them into intermediate child buffers. Image chunks remain
  ahead of their placement frame.
- `publish_compact_trees` retains raw trees per scope, publishes small `TreeDelta`
  operations, and emits nothing for an empty diff. Per-client overlays stay in
  `ClientView`; attachment epochs reset frame state once, and forced state shares the
  same publication lock and atomic tree/view group as ordinary publication.
  Scoped tree children keep their shared encoded allocation through each client's
  group and collector. The Batch encoder borrows child bytes and writes each slice
  at once, with the same frame and count bounds and output as the owned encoder.
  Quiet queries retain their response/exit children until that final encoding,
  avoiding an intermediate Batch allocation and copy. Admission still charges the
  exact encoded bytes and one reliable queue item; buffered execution keeps its
  encoded path. Ordinary status publication and session-wide refreshes skip clients
  still attaching, while the explicit final resync renders their final status.
- Hash subscribers receive revision plus exact root/copy mouse bits. Full subscribers
  receive per-table patches. A first Full subscriber cannot consume a pending Hash
  revision. Control requests no keys because its frontend does not use them; TUI requests
  Hash, and desktop, web, iOS, and FFI request Full.
- `InteractiveClient` attaches with Hello; `CommandClient::into_interactive` retains its
  existing transport after `ExecResume`. With `ZZ_PERF_LEGACY_COMMAND=1`, it closes the
  command transport and reconnects through the saved route before sending interactive
  Hello. It preserves endpoint facts and transfers the existing SSH forward to the
  interactive client. The rollback test checks the old connection closes, the new
  connection receives the requested attachment, and remote routes omit local cwd/origin.
  TUI no longer opens option/preflight clients.
  Options include `ExtendedKeys` and `FocusEvents`, bringing the shared catalog to 20.
- Control sends each raw line through one `ExecRequest`. Parsing, daemon-host variable
  expansion, and alias freezing happen once in the daemon. `ControlCommandStarted`
  opens native parent guards before callback output; responses close them. Raw callback
  guards preserve bytes. Cancellation is checked before dispatch and between commands,
  preventing disconnected queues from acting on later clients. Hook notification order
  is unchanged. Synchronous read-only queries without hooks collect Started, Response and
  ExecExit in one flat Batch. The existing collector field records Attach or Quiet ownership,
  so a query cannot release a newer resync collector. Hook dispatch and parking release the
  quiet collection before blocking, including hooks installed after the eligibility check.
- `ClientCore` reduces a whole Batch before publishing grouped events and requests one
  `TreeSync` on a base mismatch. Desktop keeps its retained-history terminal path and a
  separate tree-only mirror for inactive hosts, without a second terminal grid.
- V2 size reports carry the rendered layout generation. Compact clients' stale reports
  are rejected after unzoom; retained legacy ClientHello clients keep generation-zero
  resize behavior. Native and shared reports capture generation synchronously; the C
  API also accepts an explicitly captured generation.
  Already-presized compact panes also skip identical grid and cell-pixel reports,
  after checking the layout generation. Changed geometry and legacy reports retain
  the normal resize path.
  Pane and client-grid reports keep their supplied generation through normalization
  and revalidate under the same lock that applies geometry. A rejected report also
  skips the input publication/hook tail. Two-phase proofs admit a report, change
  the zoom layout, and reject its application without changing stored or actor size.
  The TUI clears its sent-geometry cache on a new layout generation and immediately
  reports the newly projected grid, including when its dimensions are unchanged.
  Same-connection attachment refreshes clear that cache before assigning the new
  generation and retain their existing first-paint sequence. Desktop and shared
  web/iOS caches include the generation alongside the measured grid. A new view
  notifies those panes; their next real prepaint sends the exact captured V2 tag.
  Generation-only notifications retain shared mouse bounds until the next measure.
  Real-window proofs keep grid and pixels fixed, change `ClientView`, require a
  second tagged report, and then verify deduplication at that generation.
- The control frontend buffers payload/end writes and joins a ready Started guard to its
  response, flushing before it waits. Hook names and effective hook bodies are borrowed
  from the existing registry and option arrays. The existing literal-format predicate
  skips unused facts while preserving target selection and formatted after-hooks. Read-only
  commands skip key publication and tap refresh; their mutating after-hooks still update both.
  The final control response and ExecExit share one existing reliable queue group; quiet
  queries also join Started through the existing collector. The receiver retains all remaining
  Batch children in order. On Unix, `DirectControl` selects stdin and the existing socket on
  the control thread, removing both forwarding threads. `ProtocolReceiver::try_recv` keeps
  incomplete frames, initial pending messages and buffered bytes; per-call `DONTWAIT` reads
  leave blocking clients' descriptor flags intact. Split UTF-8 lines, EOF tails and signal
  polling retain their prior behavior. Buffered events bypass readiness polling. The decoder
  returns the same box through the ready-receive layers, avoiding repeated large enum copies.
  The initialized receiver buffer is reused for reads.
  After an empty readiness probe and output flush, the next wait skips the repeated
  socket probe once while still selecting fresh stdin and socket readiness.
  The private buffered probe also defers an empty zero-time poll to that timed
  select. Pending protocol with fresh-stdin priority still polls immediately;
  ordinary receive calls keep their existing polling behavior.
  Between completed guards with no cached input line, that private probe decodes
  only messages already held by the receiver before waiting for readiness.
  Cached input keeps the socket peek on the protocol's turn, so a ready reply
  still precedes another buffered line. An open guard keeps that peek so a
  ready response can join its buffered start in one physical write.
  Partial bytes, decode-repair state and EOF retain their ordinary readiness path.
  A native guard formats its time, command number and flags once into a fixed
  46-byte body. Begin and end or error reuse that body, including the newline.
  Zero, maximum values, saturating numbers, raw bytes and partial sink errors
  retain the existing output. This increases the private copied Frame from
  24 to 47 bytes and avoids formatting the same metadata at completion.
  Final quiet-query completion can send its admitted group through the existing Unix
  socket when the queue has no other ready work and the writer owns no bytes. One
  nonblocking send either completes or leaves the group for the existing writer.
  A partially sent group retains its original allocation and an offset, stays ahead
  of later traffic, and remains outside later Batch collectors. Overflow preserves
  that remainder before ControlExit within the queue bounds, or closes the transport.
  Writer cleanup tracks dequeued bytes through success, error and panic.
  `ZZ_PERF_WRITEV=0` disables this completion path; hook and park releases retain
  their ordinary writer wake.
  The retained `ServerHello` payload is boxed, reducing `ProtocolMessage` from
  1,432 to 312 bytes on this host. Its encoded greeting is unchanged; a saved
  pre-change frame decodes and re-encodes to the same 1,265 bytes and SHA-256.
  Batch children and RawText use bulk byte decoding with the existing owned types
  and serialization. A saved 318-byte Batch covers every byte value, invalid UTF-8,
  JSON sequence fallback and unchanged encoding; malformed bounds remain rejected.
  TUI startup reduces the connection's already-held Batch synchronously and lays out
  that complete state before the first paint. The same reducer forwards attachment
  reset, image delivery and placing frames in stream order for later attachments.
  Image resets share the existing metadata/frame drain, so initial attachment paints
  each pane once. The physical-output proof covers one and four panes and leaves
  unrelated input outside that drain.
  A compact startup command-output actor is installed without an early reliable
  viewport. Final resync emits its populated viewport after the attachment
  `ClientView`, preserving the core's output-ID watermark across attachment resets.
  The real initial wire Batch contains the located direct and nested startup rows
  once, and the TUI forwarding/drain proof paints that actor over the base pane
  in the first physical paint. Legacy startup admission keeps its existing path.
  Consumed frame maps return their capacity to the inbox. Frames published during
  consumption retain their latest viewport, merged damage and pending wake.
  The TUI reuses unchanged rows, borders, status and message composition, then limits narrow-cell
  incremental painting to changed columns. Full-frame and scroll damage compare retained
  rows; equal dictionary contents also retain the painted cache. Wide cells, overlays and
  changed dictionaries keep full-row drawing.
  Repainting takes the previous cached viewport by ownership and restores the current
  viewport after drawing, avoiding a temporary clone of its fixed Arc handles.
  The unchanged fast path retains its cache without taking that entry.
  The existing output writer returns one cleared buffer, bounded by its queue budget, to the
  next paint instead of regrowing a buffer every frame. One cached terminal style reuses the
  existing formatter's exact ANSI bytes instead of formatting the same RGB style on every row.
- `status_sampler_sessions` filters compact clients that request no status. Legacy clients
  retain their default status subscription. Format monitors, control subscriptions, peer
  checks and rename deadlines keep their independent timers; repeated read-only queries
  leave an idle sampler parked when none of that work exists.
- `config_expansion_names` skips discovery when raw input contains neither `$` nor `~`.
  Empty environment/home lookup lists return before locking. The actual parser still checks
  the whole line before mutation, and stored aliases keep their existing expansion and
  same-line freezing behavior.
  Caller-input discovery skips the flag parser only when no argument contains `I`;
  display-message alias discovery skips it only when neither `@` nor `{` is present.
  Packed and escaped flags, marker false positives, errors and target precedence
  keep the existing parser and execution paths.
  Read-only classification recognizes the exact two-argument `display-message -p`
  form when its payload does not begin with a dash. Other forms retain the option
  parser. Quiet admission and execution still classify their own current invocation,
  so stdin, mouse and client-route rewrites cannot reuse an earlier classification.
  The command's actual validation and after-hook checks remain separate.
- The command worker uses the existing Crossbeam Select API to park directly instead
  of yielding through the empty receive retry. Its queue, context, cancellation and
  thread ownership stay the same. Completed groups return owned child and outer buffers
  to the existing bounded pool after the writer finishes; shared children are released.
  Completion takes its response and exit buffers under one pool lock. The final
  collector reserves the counted children once, within the existing frame limit.
- Broad validation exposed an existing cold-start agent projection bug: SessionReset
  dropped text queued before readiness. The projection now retains that text until its
  turn and clears it on explicit restart or reclaim. The original capture assertion and
  new cold/reset/reclaim/restart proofs pass.

`ZZ_PERF_TREE_DELTA=0` publishes full scoped trees on the new wire. The existing
`ZZ_PERF_EAGER_FACTS=1` restores eager facts for the literal-output optimization.
`ZZ_PERF_READONLY_SKIP=0` restores read-only key/tap work and eager Started wakes.
`ZZ_PERF_TUI_COALESCE=0` restores eager full-row paints and uncached border/status work.
Full wire rollback requires reverting matching daemon and clients.


Review corrections, 2026-09-30:

- Fixed the legacy command attachment hang by reconnecting only legacy command links;
  Exec resume still upgrades its existing connection.
- The standalone iOS terminal example now caches `(GridSize, layout_generation)` and
  reports unchanged geometry again after the generation changes, matching shared clients.
- Fixes are checkpoint `870b2b1e`; `a7e5d62a` names explicit defaults in their test.
  Integration brings `perf/wave2` at `8fa427dd` into `perf/ctrl`, including COPY and FMT.
  Conflict resolution retains CTRL's literal display-message fact skip and compact
  subscription/tree/view state, FMT's clientless borrowed row seeds and presentation
  caches, and both classifiers and test sets for display-message/list-keys/source-file.
- The eight-crate serial all-feature suite exits 0: 3,454 top-level tests passed,
  two existing tests ignored, and the documented headless attach test filtered.
  Nested subprocess test summaries do not count toward that total. The 60-second test
  guard did not trigger. Host all-target/all-feature lint for daemon, mux, terminal,
  client, TUI, protocol, CLI and iOS exits 0 with `-D warnings`.
- The rebuilt CLI's normal and legacy attachments both report the expected terminal
  error without a PTY and paint `LEGACY_ATTACH_READY` inside a PTY. PRE does the same.
  The new route test checks old-connection EOF, a fresh interactive Hello with the
  requested attachment, and omission of local cwd/origin for remote route facts.
  Existing SSH-forward ownership transfers in source; live SSH is not tested here.
- The standalone iOS terminal example simulator build and its simulator-target lint
  exit 0. These establish compilation, not a physical device resize observation.
- Fourteen of the 15 selected compat rows pass. `smoke/control-alias-prepare` exits 1
  with two output differences; both reproduce on PRE, including the isolated retry.
  Attached-client parity exits 0. Normal TUI screen parity exits 0 with 147 asserted
  checkpoints and six recorded cursor-style observations that the fixture does not
  assert. The TUI rollback screen run also exits 0 with the same checkpoint counts.
- Formatting exits 0. OKF validation exits 0 with three existing warnings outside
  this lane. Initial test-only type/EOF/socket-cleanup assumptions caused three
  focused-test exits 101 before correction; the final focused test exits 0. The first
  lint exited 101 for implicit default constructors; the corrected lint exits 0.
  Two scratch compat setup attempts exited 1 before scenarios ran because the copied
  harness lacked its expected directory layout and evidence references.
- The performance gate is SKIPPED by the integration instruction. The orchestrator
  runs the merged A/B; the measurements below still describe the earlier frozen source.
  Linux `/proc`, PTY gathering, epoll, THP and `tui-output-backpressure.sh`, Windows,
  live SSH, physical devices, the packaged launcher and production behavior are NOT RUN.
  Full-workspace/corpus, web/shared/FFI/native rebuilds and the supplied baseline-red
  resurrect-save/plugin-runtime-continuum/status-background-jobs rows are SKIPPED.

Exact review/integration commands, exits and captured output stay in scratch JSONs:

| Check | Command | Exit | Scratch JSON |
| --- | --- | ---: | --- |

| Focused rollback attachment | `timeout 1800 cargo test -p zz-daemon legacy_command_attach_reconnects_with_the_original_route -- --test-threads=1` | 0 | `/tmp/zzpc/ctrl-fix-legacy-unit-passed.json` |
| Merged formatting | `cargo fmt --all -- --check` | 0 | `/tmp/zzpc/ctrl-fix-merge-fmt.json` |
| Merged lint | `timeout 1800 cargo clippy -p zz-daemon -p zz-mux -p zz-terminal -p zz-client -p zz-tui -p zz-protocol -p zz-cli -p zz-gpui-ios --all-targets --all-features -- -D warnings` | 0 | `/tmp/zzpc/ctrl-fix-merge-lint.json` |
| Merged serial suite | `timeout 1800 cargo test -p zz-daemon -p zz-client -p zz-tui -p zz-protocol -p zz-mux -p zz-terminal -p zz-gpui-ios -p zz-cli --all-features -- --test-threads=1 --skip concurrent_default_interactive_attaches_atomically_share_session_zero` | 0 | `/tmp/zzpc/ctrl-fix-merge-tests.json` |
| CLI build | `timeout 1800 cargo build -p zz-cli --bin zz_cli` | 0 | `/tmp/zzpc/ctrl-fix-cli-build.json` |
| Non-TTY attachment pair | `timeout 1800 python3 /tmp/zzpc/ctrl-fix-legacy-probe.py` | 0 | `/tmp/zzpc/ctrl-fix-legacy-probe-built.json` |
| PTY attachment pair | `timeout 1800 python3 /tmp/zzpc/ctrl-fix-legacy-pty-probe.py` | 0 | `/tmp/zzpc/ctrl-fix-legacy-pty-probe-built.json` |
| iOS example build | `timeout 1800 env ZZ_GPUI_DEMO=terminal just ios-gpui iPad build` | 0 | `/tmp/zzpc/ctrl-fix-ios-terminal-build.json` |
| iOS example lint | `timeout 1800 env IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo clippy -p zz-gpui-ios --target aarch64-apple-ios-sim --example terminal -- -D warnings` | 0 | `/tmp/zzpc/ctrl-fix-ios-terminal-lint.json` |
| Selected compat rows | `timeout 1800 env PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin ZZ_COMPAT_TMUX=/Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_ZZ=/Users/demfabris/dev/zz-ctrl/target/debug/zz_cli /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-harness/compat/run.sh smoke/control-notify smoke/control-eof-drain smoke/control-hard-loss smoke/control-alias-prepare smoke/control-tilde-environment smoke/source-file-control smoke/send-keys-control smoke/hooks-pane-focus smoke/hooks-pane-focus-clients smoke/format-monitor-hooks smoke/refresh-status smoke/copy-mode-refresh smoke/copy-mode-formats smoke/copy-mode-mode-keys-tail smoke/pane-border-status` | 1 | `/tmp/zzpc/ctrl-fix-compat-risk-final.json` |
| PRE alias comparison | `timeout 1800 env PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin ZZ_COMPAT_TMUX=/Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_ZZ=/tmp/zzpc/ctrl-base/zz_cli /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-harness/compat/run.sh smoke/control-alias-prepare` | 1 | `/tmp/zzpc/ctrl-fix-compat-alias-pre.json` |
| Attached-client parity | `timeout 1800 env PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-harness/compat/attached-client.sh /Users/demfabris/dev/zz-ctrl/target/debug/zz_cli /Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux` | 0 | `/tmp/zzpc/ctrl-fix-attached-client.json` |
| TUI screen parity | `timeout 1800 env PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-harness/compat/tui-screen-diff.sh /Users/demfabris/dev/zz-ctrl/target/debug/zz_cli /Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux` | 0 | `/tmp/zzpc/ctrl-fix-tui-screen.json` |
| TUI screen rollback | `timeout 1800 env ZZ_PERF_TUI_COALESCE=0 PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-harness/compat/tui-screen-diff.sh /Users/demfabris/dev/zz-ctrl/target/debug/zz_cli /Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux` | 0 | `/tmp/zzpc/ctrl-fix-tui-screen-rollback.json` |
| Knowledge validation | `python3 .agents/skills/okf/scripts/okf.py validate` | 0 | `/tmp/zzpc/ctrl-fix-final-okf.json` |
| Initial focused test: type correction | `timeout 1800 cargo test -p zz-daemon legacy_command_attach_reconnects_with_the_original_route -- --test-threads=1` | 101 | `/tmp/zzpc/ctrl-fix-legacy-unit.json` |
| Initial focused test: EOF correction | `timeout 1800 cargo test -p zz-daemon legacy_command_attach_reconnects_with_the_original_route -- --test-threads=1` | 101 | `/tmp/zzpc/ctrl-fix-legacy-unit-rerun.json` |
| Initial focused test: socket cleanup correction | `timeout 1800 cargo test -p zz-daemon legacy_command_attach_reconnects_with_the_original_route -- --test-threads=1` | 101 | `/tmp/zzpc/ctrl-fix-legacy-unit-final.json` |
| Initial lint: explicit defaults needed | `timeout 1800 cargo clippy -p zz-daemon -p zz-gpui-ios --all-targets --all-features -- -D warnings` | 101 | `/tmp/zzpc/ctrl-fix-lint.json` |
| Corrected premerge lint | `timeout 1800 cargo clippy -p zz-daemon -p zz-gpui-ios --all-targets --all-features -- -D warnings` | 0 | `/tmp/zzpc/ctrl-fix-lint-final.json` |
| Initial scratch compat setup | `timeout 1800 env PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin ZZ_COMPAT_TMUX=/Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_ZZ=/Users/demfabris/dev/zz-ctrl/target/debug/zz_cli /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-compat/run.sh smoke/control-notify smoke/control-eof-drain smoke/control-hard-loss smoke/control-alias-prepare smoke/control-tilde-environment smoke/source-file-control smoke/send-keys-control smoke/hooks-pane-focus smoke/hooks-pane-focus-clients smoke/format-monitor-hooks smoke/refresh-status smoke/copy-mode-refresh smoke/copy-mode-formats smoke/copy-mode-mode-keys-tail smoke/pane-border-status` | 1 | `/tmp/zzpc/ctrl-fix-compat-risk.json` |
| Second scratch compat setup | `timeout 1800 env PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin ZZ_COMPAT_TMUX=/Users/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_ZZ=/Users/demfabris/dev/zz-ctrl/target/debug/zz_cli /opt/homebrew/bin/bash /tmp/zzpc/ctrl-fix-harness/compat/run.sh smoke/control-notify smoke/control-eof-drain smoke/control-hard-loss smoke/control-alias-prepare smoke/control-tilde-environment smoke/source-file-control smoke/send-keys-control smoke/hooks-pane-focus smoke/hooks-pane-focus-clients smoke/format-monitor-hooks smoke/refresh-status smoke/copy-mode-refresh smoke/copy-mode-formats smoke/copy-mode-mode-keys-tail smoke/pane-border-status` | 1 | `/tmp/zzpc/ctrl-fix-compat-risk-rerun.json` |

Mac measurements on frozen source `8c49034f1b19b023d85008cf0e0b2d4809533d06`:

| Row | Pre-lane quick | Previous TERM full | CTRL full | CTRL / same-run tmux |
| --- | ---: | ---: | ---: | ---: |
| Attach connections p1 / p4 | 2 / 2 | 2 / 2 | 1 / 1 | fixed count |
| Attach server bytes p1 / p4 | 29,391 / 29,667 B | 29,391 / 29,666 B | 2,328 / 2,847 B | fixed byte bound |
| Attach first paint p1 / p4 | 7.5802 / 7.2905 ms | 19.4232 / 20.4470 ms | 7.6291 / 7.9861 ms | 0.880 / 0.877 |
| Attach daemon CPU p1 / p4 | 1.3849 / 2.5286 ms | 3.3401 / 4.9842 ms | 1.3401 / 2.2943 ms | 0.878 / 1.221 |
| Serial control latency | 0.0629 ms | 0.0781 ms | 0.0269 ms | **1.439, fails 1.2 limit** |
| Control daemon CPU / command | 0.0379 ms | 0.0452 ms | 0.0124 ms | 0.954 |
| Control instructions / command | 0.2236 M | 0.2209 M | 0.0729 M | 0.457 |
| Burst commands / second | 18,603.8 | 11,500.8 | 275,909.6 | 1.572 |
| Control output | 124.5421 MB/s | 131.5761 MB/s | 131.6183 MB/s | 3.302 |
| Visible TUI CPU | 1.5187% (separate full) | 1.6735% | 0.9334% | passes 1% limit |
| Visible TTY output | 393.5864 KiB/s (separate full) | 384.4557 KiB/s | 244.0791 KiB/s | 0.737 |

Quick and full use different sample counts; the historical columns provide context,
not paired timing estimates. The exact required quick gate exits 1 with 43 passes
and two failures: p4 attach CPU 1.327 times tmux and serial latency 1.487 times.
The full attach/control gate exits 1 with 13 passes and one failure, serial latency;
all its attach floors pass. The separate clean full chatty gate exits 0 with eight
passes. A profiled visible run measured 1.1724% and remains separate informational
evidence. The passing clean run does not erase that outcome.

The clean full throughput run exits 0: ASCII 300.4309 MB/s, Unicode 122.5457 MB/s,
and attached ASCII 498.192 ms, all three inherited floors passing. ASCII is about
13% below the previous TERM full result of 345.1791 MB/s. A later same-harness
pre-lane/final pair is red for both attached W0 rows: 893.6 versus 766.1 ms. ASCII
is 293.2 versus 279.8 MB/s and Unicode 121.3 versus 119.1 MB/s. The final headless
information row rises from 554.8 to 4,535.6 ms. Host load moves from 7.1 to 36.11
in PRE and 29.28 to 28.70 in final; these conditions do not establish a cause.
Throughput stability and the headless change remain unresolved, rather than
being waived by the earlier passing result. Both full raw outcomes are retained
under `bench/perf/results/w2-3-ctrl-*-macbook-8c49034f.json` and the pre-lane
results identify their preserved binary separately from the harness git revision.

Final five-second daemon, control frontend and visible TUI samples retain identities
and raw stacks. The simple display classifier no longer enters the generic option
parser in either daemon control sample. Actual command validation, hook predicates,
worker dispatch and socket/stdio operations remain. Native guard formatting occurs
once at allocation; the old begin/end formatting branches are absent. The visible
sample contains no recurring frame-map growth, output-buffer growth or repeated RGB
formatting. Sparse samples do not prove those costs absent in every workload.

A separate 1,000-command process assay records client and daemon CPU/instructions
with 100 warmup commands, alternating zz and tmux. zz uses 9.706292 microseconds of
frontend CPU plus 11.778 daemon CPU per command; tmux uses zero measured client delta
plus 12.857542 daemon CPU. Both complete all guards. Median latency is 0.0274 versus
0.0192 ms. This supports investigation of the frontend relay, without predicting
savings from moving its formatter or proving the latency floor.

Final source validation: the 16-package serial test command exits 0 with 4,573
passed, five existing ignored and the single documented headless test filtered.
The preceding eight-thread command stopped at the daemon with 24 failures; all
24 unchanged solo reruns pass in 0.01 to 1.06 seconds. The failed run remains
recorded, and the solos and serial pass do not establish its cause. The 16-package
all-target/all-feature lint exits 0. Shared host capability runs one actual test,
its full library passes 58, and its lint exits 0. The linked C rollback integration
passes with `ZZ_PERF_TREE_DELTA=0`; one Rust test launches both C clients. Web and
iPad build recipes exit 0. Those builds establish compilation rather than physical
iPad or packaged desktop behavior. Detailed commands and exits are retained in
`bench/perf/results/w2-3-ctrl-validation-macbook-8c49034f.md` and its raw JSON.

The serial floor remains open. A local Unix stdin/stdout descriptor handoff could
retain the existing worker/FIFO and remove the command-data relay, but changes the
negotiated transport and native stdout owner beyond this lane's compact message
model. It needs a separate design decision and actual bounded-output, cancellation,
callback and ordering proofs. Reader-side execution instead requires an explicit
execution-context ownership transition; queue emptiness is not an idle claim.
No such ownership change is implemented here. Linux readiness, PTY gathering,
THP and output-backpressure remain coordinator checks on a Linux host.


Follow-ups after the merge (2026-09-30 to 10-01). `f6a25887`: on Linux the control client's
`%layout-change` lost the monitor-activity flag (`-` for `#-`, 7 of 8 runs of
`smoke/control-notify`) because the raw output tap published output before the viewport worker
raised activity; activity is now noted on the tap first, layout hooks publish compact trees under
`snapshot_order` before the hook event, and `WindowSnapshot` carries the missing `silence` flag
(tests in `daemon/ctrl_flags_tests.rs`). `9e634cd8`: the wave-2 exit corpus found ten config and
argument rows red (own-conf, config-grammar, source-file and source-replay diagnostics,
`args-parse-*`, oh-my-tmux) plus four Mac plugin-init rows: control stdin now starts only after
the initial command replay completes, typed preparation errors keep `parse error:`, and the
early option preflight applies to command clients only (tests in `daemon/ctrl_config_tests.rs`
and `zz-cli` `cli_binary.rs`). Merge-time A/B numbers are in `bench/perf/campaign/HANDOFF.md`
(wave 2 merge log).
## W2-HOOKS: read-only skip, then change journal (effort L)

Two commits, the first mergeable alone.

1. `mutates` is **zz's own predicate over arguments**, not tmux `CMD_READONLY` (tmux.h:1999 is a
   read-only-client permission flag, set on attach-session, send-keys and others that mutate).
   `capture-pane` without `-p` (it writes a paste buffer, `-b` or a new one, as
   cmd-capture-pane.c does), `display-message -I`/`-d` count as mutating. Read-only commands skip
   `MuxHookSnapshot::capture`, `capture_pane_focus_probe`, copy-mode / active window / pane /
   bell / focused-window captures and the after-diff (debug-assert generation unchanged).
   **`after-<cmd>` hooks still fire for read-only commands** (pinned tmux `CMD_AFTERHOOK`,
   cmd-queue.c:653, covers list-keys, list-panes, show-options, capture-pane, display-message).
   `format_hook_facts_for_client` is built only when a hook body or format needs client facts.
2. zz-mux `MuxState.journal: Vec<Change>` pushed by every mutating method (SessionCreated,
   Renamed, Closed, WindowLinked, Unlinked, Renamed, LayoutChanged, ActivePaneChanged, PaneFocus,
   PaneTitle, PaneExited, OptionChanged{scope, name}, KeyTablesChanged, EnvironmentChanged,
   BellRaised, ClientSessionChanged). **Each Change carries the names, ids and indexes hook
   formats need, captured at emission** (hooks for closed sessions read what
   `MuxHookSnapshot` / `HookSessionState` hold today). Route every direct `get_mut` on the pub
   `sessions` / `windows` fields through methods; `drain_journal()`. Mutating commands and
   `synchronize_pane_runtime`, `synchronize_pane_title`, `close_exited_terminal`,
   `rename_window_from_pane` take hook events from the journal in tmux `notify_*` order. Then
   delete `MuxHookSnapshot` and the diff in `mux_hook_events`. This lane owns the source of
   control-mode notifications.

Write zone: catalog.rs `CommandSpec.mutates`; zz-mux model.rs journal and mutators; command.rs
mutation sites (not list or format fns); daemon.rs `MuxHookSnapshot`, `mux_hook_events`,
`execute_with_mux_source_inner`, `capture_pane_focus_probe`, `close_exited_terminal`,
`synchronize_pane_title`, `rename_window_from_pane`, `synchronize_pane_runtime`,
`format_hook_facts_for_client`.

Gate vs CTRL JSON: `config.wall.source_1000` <= 1.1x tmux; `cli.cpu.display.p20` 5-15% lower;
`spawn.cpu.kill_pane` <= 0.4 ms; oh-my-tmux cold server CPU <= 30 ms (tmux 20). Tests: hook-order
tests (`splice_pane_focus_events`, window-layout-changed order, client-detached on session loss),
after-list-keys and other read-only after-hooks, debug assert "generation moved implies journal
entry" over the whole daemon suite, control-mode `%begin`/`%end` ordering fixtures,
`compat/run.sh` hooks and source-file scenarios.

As built (branch `perf/hooks`, 2026-09-29, from `perf/wave1` `0e590636`, Linux only). Where it
departs from the scope above:

- Item 1 (commit `fbb70f6b`, mergeable alone): `CommandSpec::mutates(args)` in catalog.rs is
  false for list-*, show-*, has-session, list-commands, start-server, `display-message` without
  `-I`/`-d` and `capture-pane -p`; a spelling the option parser rejects counts as mutating.
  Only engine commands reach the captures (`capture-pane`, the buffer commands and
  `list-clients` are daemon-dispatched and never took them). A read-only command in a release
  build takes no hook snapshot, focus probe, copy-mode, active window, active pane, bell or
  focused-window capture. Debug builds still take them and assert that the command moved no
  generation, selected no pane, changed no window and raised no hook event, so the whole daemon
  suite and every debug compat run check the predicate. After-hooks are untouched: they fire
  from `execute_with_mux_source_routed_for_terminal_in_queue` for every command.
- The facts rule is a second predicate, `hook_events::format_facts_unread`: bind-key,
  unbind-key, has-session, show-options with no `#` in its arguments, and set-option or
  set-window-option unless the option name holds a `#`, `-F` expands a value with a `#`, or the
  name is a prefix of `automatic-rename` (the only setter that takes format hooks). Those run
  with a shared empty `FormatHookFacts`, so a config line no longer holds refcounts on the user
  option maps, and `set -g @x` stopped cloning the whole map and dropping the copy (27% of a
  1000-line `source-file` with 400 `@options`). `DaemonFormatHooks::withhold_facts` makes every
  fact-reading `StatusHooks` method debug-assert. `synchronize_pane_runtime` uses the same empty
  facts when no rename is due. Knob `ZZ_PERF_EAGER_FACTS=1`.
- Item 2 is a journal of pre-images, not of semantic events. `MuxState.sessions` and `.windows`
  are `Tracked` maps (zz-mux `journal.rs`): reads go through `Deref` to the `BTreeMap`, and the
  only mutable accessors (`get_mut`, `insert`, `remove` inside zz-mux; `session_mut`,
  `window_mut`, `remove_session` for other crates) hand `MuxState.journal` the entry's state
  first, so no mutation can bypass it; the compiler listed the 120 or so sites to route, and
  `pane_mut` now touches only the pane's window. A record happens only while a `ChangeWindow`
  is open and only on an entity's first touch after the newest open mark; a window is a
  refcounted token, so one dropped on an early return closes itself, and the entries go when
  the last window closes. The daemon opens
  `hook_events::HookScope` where it used to capture `MuxHookSnapshot` twice (the command path,
  attach, detach, input resizes and copy-mode switches, control client sizes, switch-client,
  exited panes, the rename timer) and `FocusProbeScope` where it captured the focus probe; the
  probe now takes only its client half up front. `mux_hook_events` is unchanged in order and
  output: it reads through a `HookView`, which the snapshot (rollback and oracle), the journal
  overlaid on live state (before) and the live state (after) implement. Lookups answer for any
  id; iteration covers only the touched sessions and windows and their panes, the only ones
  whose hooks can differ. The command path reads its active window, active pane and bell
  pre-images from the same journal. Single-pane events (alerts, title, mode, clipboard, agent
  state, pane-exited/died) look their pane up instead of snapshotting everything. Why not the
  event enum: the current diff's order is what the compat corpus pins against tmux, and events
  derived from the same diff over pre-images keep that order exactly; option, key-table,
  environment and client changes raise their hooks through effects today and needed no entries.
- `MuxHookSnapshot` and the full diff stay as the rollback path (`ZZ_PERF_HOOK_JOURNAL=0`) and
  as the oracle: debug builds capture both and assert the journal gives the same events, the
  same focus candidates and the same moved active windows, panes and bells. That replaces the
  "generation moved implies journal entry" assert, which could not see a missed pre-image.
  Delete both with the wave-2 knobs.
- Found by profile on the lane's path and removed: `resolve_command`, `command_spec` and
  `catalog_command_spec` scanned the catalogue about eight times per command (10% of config
  replay); exact spellings now go through hash maps built in table order. The after-hook lookup
  built `hook_arguments` and the flag variables for hook arrays with no commands.
  `close_agent_panes` took three locks with nothing to close.
- Review fixes (same day, `perf/hooks` after the parity and perf reviews):
  - Key tables carry a generation per table (`KeyTables::table_generations`). A CLI bind-key
    snapshotted and compared every binding of every table on each command: to nobody while no
    client was attached (60% of its cost), and to the attached clients otherwise (0.75 Minstr,
    4.6x tmux). The daemon now remembers the generation it last published per table and sends
    the appended `KeyTablesPatched { tables, removed }` with only the changed tables;
    `ClientCore` merges it by name. A rebind of an identical binding moves nothing. With no
    subscriber nothing is built and the change stays pending for the next publication; the first
    version of the skip also left the published content stale, so a subscriber that arrived
    after an unbind and then saw the binding restored was never told (parity review, now a
    test). The hello always snapshots fresh. `ZZ_PERF_EAGER_PUBLISH=1` covers the skip,
    `ZZ_PERF_KEY_TABLE_DELTA=0` sends every table in `KeyTablesChanged`.
  - `MuxEngine::execute_without_alias_expansion_inner` swept 37 session, window and pane keyed
    maps after every command, read-only ones included, with a scan of every window per pane
    entry (18.6% of config replay at 20 windows, 36% of `has-session` at 100). The sweep now
    runs only when a session, window or pane was removed since the last one, against a set of
    live panes built once. The removal count lives in the journal: `Tracked::remove` bumps it
    (break, join and move take the source window out of the map first) and so does
    `kill_pane`, the one place a pane leaves a window that stays in the map. A generation
    trigger was tried first and still swept on 5.9% of config replay at 20 windows, since most
    option sets move the generation. Debug builds run the sweep anyway when it is skipped and
    assert it removed nothing.
  - `run_shell_job` slept 20 ms between checks of the job's output and exit for the job's whole
    life (2131 voluntary switches while sourcing oh-my-tmux, about 80% of the daemon's CPU). It
    now blocks in `poll` on the output socket and a pidfd (Linux) or a kqueue `EVFILT_PROC`
    (macOS, type-checked for `aarch64-apple-darwin` but not run); without either it keeps a
    20 ms poll of the output alone. After EOF it waits on the same descriptor.
  - The pane focus candidates return at once while no pane has focus and no attached client is
    focused (every connection, the invoking CLI included, registers as focused, so the first
    early return never fired on the command path). The command's focus probe shares the hook
    scope's change window.
  - `input` kept its focus probe's change window open across the whole dispatch, so a key
    binding that blocks (`run-shell` without `-b`, a menu) made the journal keep one image per
    command every other client ran meanwhile. The probe now sits in a thread-local slot, and
    the first routed command of the dispatch turns it into a full probe from the journal's
    pre-images and the live state (the same maps the old whole-state probe held) and closes the
    window; a keystroke that runs no command keeps the journal path.
  - Config replay tests the command name before parsing a line for `source-file -`.

Measured on alienware (Linux, battery with the powersave governor: CPU and wall swing up to 3x
between runs of one binary, so instructions are the signal), lane head against the lane base,
tmux 3.7c in the same run. Quick gate (`before.json` / `after.json` in the lane cache):
`config.instr.source_1000` 139.8 -> 44.4 Minstr (tmux 82.6); `cli.instr.display.p1` 0.109 ->
0.080 (tmux 0.135), `.display.p20` 0.248 -> 0.157 (tmux 0.260), `.has_session.p20` 0.208 ->
0.075, `.show_options.p20` 0.248 -> 0.080, `.select_pane.p20` 0.234 -> 0.139, `.send_keys.p20`
0.245 -> 0.143, `.chain5.p20` 1.10 -> 0.53 (tmux 0.49); `spawn.instr.kill_pane` 0.205 -> 0.184
(tmux 0.179); `cli.cpu.display.p20` 0.103 -> 0.078 ms. Three alternating full config runs per
binary: `config.wall.source_1000` 1.74-1.78x tmux -> 0.80-0.91x, CPU 1.94-1.99x -> 0.78-0.92x.
Daemon user instructions per command from `perf stat` A/B loops: CLI bind-key 0.777 -> 0.066
Minstr; config at 20 panes 211 -> 51 Minstr.

After the review fixes, same host, quick gate `fixed.json` (lane cache) against the pre-fix lane
binary: `config.instr.source_1000` 44.4 -> 41.7 Minstr (tmux 82.5); `cli.instr.display.p20`
0.157 -> 0.136 (tmux 0.260), `.has_session.p20` 0.075 -> 0.054, `.show_options.p20` 0.080 ->
0.058, `.select_pane.p20` 0.139 -> 0.117, `.send_keys.p20` 0.143 -> 0.121, `.list_panes.p20`
0.198 -> 0.176, `.chain5.p20` 0.53 -> 0.42 (tmux 0.49, now below it); `config.wall.source_1000`
0.83x tmux, CPU 0.77x; 33 pass, 3 fail (`spawn.cpu.split_shell`, `split_empty_P`, `new_window`,
red on the base too, W3), 0 regressed. With 100 windows (`meas.py`, daemon instructions per
command): `display-message` 0.890 -> 0.433 Minstr (tmux 0.783), `has-session` 0.529 -> 0.074
(tmux 0.613); config replay at 20 windows 64.8 -> 41.9 (tmux 98.7), the same as at one window.
A CLI bind-key with a control client attached 0.753 -> 0.072 Minstr (tmux 0.164). oh-my-tmux
cold, server CPU from exec until idle, three quiet runs: 72-95 -> 19-22 ms (tmux 18.6-18.9;
user time 10-11 ms against tmux 12, kernel 9-10 ms against 6-7); interleaved again while another
lane compiled (load 20): 21-33 ms against tmux 18-32 and the pre-fix lane 81-136. While sourcing
it the run-shell threads' voluntary switches go 2131 -> 20-24.

Handed on:

- `spawn.cpu.kill_pane` <= 0.4 ms is not met here: user instructions are 1.03x tmux, and the gap
  is kernel time (the pane's actor and pty-gather threads exiting, PTY close); tmux itself read
  0.24-0.48 ms across runs (W3-SHARDS).
- oh-my-tmux cold server CPU <= 30 ms is met since the review fixes (above). The first
  handoff note here had its measure wrong: zz's user time was 1.6-1.8x tmux, not one tick for
  both, and the polling run-shell threads were 80% of the daemon's CPU, not 44% of samples.
- At the HOOKS merge, per-command `format_hook_facts_for_client` was still 11% of
  `display-message` and `select-pane` at 20 panes: it copied pane kinds, pane windows and user
  option refcounts into a snapshot. W2-FMT replaces that capture with borrowed providers;
  `withhold_facts` remains a debug guard for paths that must not ask for formats.
- A select-pane in a 20-pane window still clones that window's layout and 20 titles into the
  journal. A layout and title generation per window would let the image skip them, but only
  once `Window.layout` and pane titles are private behind journaling setters; today any
  `window_mut` caller can change them.
- Every CLI command's `Shared::unregister` is 11.5% of `select-pane` at 20 windows and 10.6% of
  `display-message` at 100; half of the latter is `release_view` on every terminal for a
  client that never had a view (EXEC).
- `resolve_command` is still about 2.8% of config replay: eight exact-name lookups per line,
  now hashed; resolving once and passing the name down is EXEC's.
- Other 20 ms pollers of the `run_shell_job` kind remain: `run_copy_pipe_with_timeout`,
  `run_control_output_tap` (a `recv_timeout` per control client and pane) and `run_pane_pipe`
  (W3-LOOP; none is on this lane's gate rows).
- Commands run from a hook body raise no control-mode notifications (`%window-renamed`,
  `%layout-change`), because hook bodies run with `no_hooks` and open no hook scope; tmux
  suppresses only on the global queue. Base and lane alike (W2-CTRL or a follow-up here).
- With `destroy_sessions`, shutdown still takes a whole-mux snapshot per session (O(sessions
  squared)). One snapshot before the loop is not the same when grouped sessions hand a window
  to the next session, so it stays until the journal covers shutdown.
- A key table change published between a client's `register` (its hello) and its `subscribe`
  never reaches it, for every hello-carried state; the base had the same gap.

Merged into `perf/wave2` as `0acd7f2a` (w2-1, Linux), plus `74f35b65` for the web client
lockfile. Strict gate `w2-1-hooks-alienware-0acd7f2a.json` against the wave-1 exit:
`config.wall.source_1000` 0.74x tmux (rule 1.1x), `config.cpu.source_1000` 0.70x,
`config.instr.source_1000` 41.7 Minstr (tmux 82.4); `cli.cpu.display.p20` 16-33% under the
pre-merge binary in alternating A/B runs (target 5-15%); `spawn.cpu.kill_pane` passes the
Linux-scaled bound (1.04 ms, bound 1.52) but is 2.3x tmux in raw time, the kernel-side gap handed
to W3-SHARDS above. No instruction, byte, footprint or thread row regressed.

## W2-COPY: copy mode without flat clones (effort L)

Scope: replace `ModeRevision`'s flat clone (PackedCell + semantics + search text + 12 B offsets
per cell, 128 MiB cap) while keeping tmux clone semantics. tmux clones backing at entry and on
an enabled refresh (`window_copy_clone_screen` in window-copy.c). Source pruning and ED3 do not
change frozen content. Resize reflows the frozen backing, maps the logical cursor, clears
selection and recalculates existing search marks. The `clear-history` command exits copy mode;
ED3 leaves it open. Clone covered pages copy-on-write, keeping compressed pages compressed, and
render only visible rows per frame. `HistorySearchSnapshot` searches frozen rows lazily, joining
physical wraps into logical lines. An enabled refresh takes another page snapshot only when
there is unseen output and no selection; it retains tmux's follow and suppression rules.
Retained dead panes keep their compressed `Terminal` instead of flat `FrozenHistory`
(`publish_frozen_history`, the `run_terminal` retained-pane tail). Remove `ModeRevisionTooLarge`.

Write zone: `session/mode_revision.rs`, `session/copy_grid.rs`; session.rs `FrozenHistory`,
`publish_frozen_history`, `run_terminal` retained-pane tail, copy-mode refresh,
`HistorySearchSnapshot` and `copy_mode_snapshot`; the native page snapshot and safe row APIs.
The retained copy completion path also needs `Shared::watch_terminal`; see the as-built notes.

Gate against lane base `fecaaa43`: first copy-mode entry on a 10k x 180 pane is included in `mem`,
with bounds of 1 MiB incremental footprint and 5 ms daemon CPU and control reply wall time.
Throughput must remain unchanged. Tests: tmux differentials for copy mode under continuous
output past history-limit and resize in copy mode; `compat/tui-copy-mode.sh`, `compat/run.sh`
copy-mode rows, `tracker.py check`, terminal copy/search tests and daemon copy-session tests.

As built (branch `perf/copy`, 2026-09-30, lane base `fecaaa43`, Linux only; review-fix
checkpoints `1540b318` and `62e7dbfd`):

`ModeRevision::capture` freezes the native active screen instead of flattening its whole
history. Copy-on-write cloning skips the eager clone preheat-count pass, confirmed as
surviving in optimized assembly; eager clones retain that preheating. Complete resident
history pages share immutable storage with atomic ownership; writes detach a shared page.
Compressed pages share encoded bytes when allocator identities match and allocate a private
restore mapping only when read. Differing allocators receive independent encoded copies.
Dropping unread pages never restores them. Active pages are copied because native cursor
caches hold pointers into them. Resize reads source page metadata and rows without detaching
pages it will replace. The native delta lives in published Ghostty fork commit
`7823f65dd55fc9ff420d5eb5cae761cbd1995994` on `zz-2026-09-30`, with parent
`c39414175ca2aad564b74b3f52196355f2671774` retained in the branch's history.
`zz-2026-09-29` keeps spare-page pin `713374af`. The safe wrapper's
owned `ScreenSnapshot` and borrowed bounded `GridRow` APIs live in one published
libghostty-rs fork commit `8e40135fb20e9ed91c37c374fe1d14570c386d06` on new branch
`zz-2026-09-30`, with parent `359ef751c189540eafb9110b2de89ad95ce48fc3` still on
`zz-2026-09-25`. The lane vendors only the sys snapshot, with regenerated
bindings and the installed-header API check. Native source staging, patch application,
reversal, hashing and stamps are gone, as are `copy-mode.patch` and the safe-wrapper directory.
The manifest and native pin select these published commits for fetched-source builds.
Pre-publication validation used a temporary wrapper path patch and fresh native source
override, removed before repinning. This implements the binding packaging
decision of 2026-09-30; the earlier rationale about a fork push freeze no longer applies.

The C clone initializes its owned terminal directly from frozen backing, skipping four blank
page mappings and their pool/pin bookkeeping. Ordinary terminal initialization retains its
default-cursor setup; existing cloned cursor/SGR semantics, configured terminal cursor defaults
and fresh parser state remain intact.

`CopyGrid` converts requested rows and retains at most 64. Its compacting style/grapheme
dictionary clears cached rows when ids change; published frames keep their own storage.
Visible cells resolve one native row reference per row. Consecutive cells with the same
page-local style id and hyperlink state reuse one converted style. Single-scalar glyphs skip
UTF-8 encoding and decoding. `ModeRevisionReader` pairs a cached row with its dictionary so
cursor line, word and search-match facts avoid repeated cell-cache locks and remain valid
through dictionary compaction. Plain capture, VT capture and selection formatting now also
hold one row and its matching dictionary for their row loop; they append glyphs to the
destination without per-cell strings or repeated style-dictionary fetches. `HistorySearchSnapshot` stores native backing rather than
whole-history text and offsets. Search reuses buffers for one logical wrapped line, maps
Unicode matches to physical start/end rows, checks cancellation between rows, and recompresses
after 512-row batches and at completion. Empty and single-scalar glyphs skip the general
iterator; longer graphemes retain the scratch path. The existing match-count limit remains.

Frozen backing survives source output, pruning, ED3 and source destruction. It uses primary
backing with unlimited snapshot pruning, wrapping and history pulling enabled, and shell
prompt redraw disabled; source screen identity stays separate. Resize maps the logical cursor,
clears selection and rebuilds search marks. Appearance changes recolor frozen content, while
an enabled refresh captures the current source. `ZZ_PERF_COPY_CLONE=1` restores flat
mode/search snapshots and their original entry-geometry limit; retained actor behavior applies
with either choice. A test-only search gate now covers pending retained search direction in
both backends without unwrapping absent native state. Resize tests assert the supported
backend geometry while still checking the live terminal resize.
`ZZ_PERF_NO_COMPRESS=1` also suppresses copy/search recompression.

Retained dead panes keep their compressed terminal actor instead of `FrozenHistory`. The actor
answers copy reads immediately during the five-second retention decision and completes their
in-flight/deferred bookkeeping. It carries the original search worker/results into its
retained surface loop, preserving pending search policies and completed cursor placement.
Exited panes that are discarded cancel their active frozen searches. Classified PTY input is
closed and drained before completion publication; queued payloads and permits are released
even while the sender survives. PTY handles and parsing/input scratch are dropped before the
retained loop. Linux exercises this transition; the macOS duplicate drain descriptor cleanup
was checked in source but cannot be run here. `Shared::watch_terminal` handles completion once
and continues delivering retained copy events. `finish_popup` sends the retained-popup
decision. Those two functions are the forced daemon write-zone extensions; CTRL/FMT register,
attach, publication, format, client and protocol code were not changed. Protocol remains
unreleased 107.

The gate is in `mem`: each fresh server gets exactly 10000 dense history rows in a 180x50
pane, five seconds to idle, and one entry through a persistent control client. It gates
incremental footprint at 1 MiB and daemon CPU/control wall time at 5 ms, with Linux
instruction counts and a tmux twin. RSS remains informational. `before.json` was measured at
gate-only `116483ae`, with production code unchanged from `fecaaa43`. The first release
attempt exposed an undefined Zig builtin allocator-context read, fixed by comparing vtables
before any defined custom context; the dense default-allocator regression failed before the
fix and passed afterward. Failed attempts are preserved as diagnostics and contribute no COPY
measurements.

Pre-review release at `827ec2ab`, SHA-256
`304746910e6a78d4818ae748cb8539cd60102cc69a82445939434f9201944c75`:

| Metric | Base zz | Final zz | Final tmux | Rollback zz |
|---|---:|---:|---:|---:|
| Copy footprint (MiB) | 52.6875 | 0.5898 | 9.168 | 52.7695 |
| Copy RSS (MiB, informational) | 52.75 | 0.6523 | 9.168 | 52.832 |
| Copy daemon CPU (ms) | 101.7425 | 1.2123 | 7.0696 | 100.3023 |
| Copy reply wall (ms) | 101.819 | 1.3061 | 7.1979 | 100.4032 |
| Copy instructions (Minstr) | 1535.0302 | 4.3218 | 10.1793 | 1527.7527 |
| Detached ASCII (MB/s) | 133.7675 | 132.0379 | 39.0785 | n/a |
| Cooked PTY ceiling (MB/s) | 130.7389 | 133.3491 | n/a | n/a |
| Headless ASCII (ms, informational) | 1194.765 | 1203.954 | n/a | n/a |

The three COPY floors pass, including every sample below their bounds. The prescribed
`wave2 --quick --only mem,throughput` run has six passes, one known failure and seven
informational rows, with no errors, strays or killed orphans. The failure is unchanged
`mem.threads.p20`: 66 against 51, owned by W3-SHARDS. Detached throughput is 0.987x the lane
base and passes the Linux cooked-PTY/W0 bounds. Final load was `[0.28, 1.03, 0.93]` before and
`[0.58, 1.01, 0.93]` after. Rollback explicitly records `ZZ_PERF_COPY_CLONE=1` and intentionally
fails the three COPY floors; retained actor fixes remain active. Artifacts are
`/home/demfabris/.cache/zz-perf/copy/{before,after,rollback-copy-clone}.json` and matching logs.
The final gate records dirty documentation only; the measured production code is committed.

Final profiling attaches only to the daemon: `perf record -e cycles:u -F 999 -g -p
700400 -- sleep 5`, then `perf report --stdio --no-children`. The capture has 5507 samples,
zero lost, and 4926 repeated entry/cancel cycles over 4.955375 seconds. Both record and main
report exit successfully. The persistent control client and producer stay outside the sampled
process. Warm entry median is 0.4560715 ms; the separate unprofiled diagnostic first entry is
0.5859375 MiB, 0.914623 ms CPU and 0.844651 ms wall. Fresh-server gate medians above remain
the entry result.

| Daemon self-cost leaf during entry/cancel | First valid profile | Final profile |
|---|---:|---:|
| `CopyGrid::row` | 17.30% | 6.38% |
| `ghostty_grid_ref_style` | 2.27% | 0.03% |
| `ModeRevision::cell` / `ModeRevisionReader::cell` | 2.81% | 0.18% |
| `ViewportDictionary::encode_glyph` | 3.12% | 2.23% |
| `ViewportDictionary::intern_style` | 4.48% | 3.79% |
| Live `build_snapshot` on cancel | 17.93% | 19.99% |

These are cycle shares for both entry and cancellation, rather than isolated entry timings.
Final independent native grapheme and grid-cell getters account for 6.52% and 5.65%; live
row-cell extraction accounts for 8.55%. Native clone is 0.38%, page map 0.01%, and no blank
constructor leaf appears. Source and optimized DWARF confirm that the direct constructor and
early COW branch remove the dummy pages and eager counting. The later independent review found repeated COPY row-cache access in capture and a general
scalar iterator in search, outside this entry/cancel profile. The review fixes remove both
caller costs; independent cell/grapheme reads and live cancel extraction go to W4-ROWS. Artifacts are `entry-final.perf.data`, `entry-final.profile.json`,
`entry-final.perf-report.txt` and the supplemental flat report under the lane cache. Cleanup
reports daemon exit zero and no strays.

Final linked parity at `827ec2ab`: 27 scenarios and 300 steps pass with zero divergences or
retries. Copy TUI passes 141/147 with its full transcript byte-identical to the baseline; tracker
and binary hash guards pass, and cleanup finds no fixture processes or sockets. An intermediate
run exposed a refresh-fixture race: live output and an earlier cursor change could precede the
next frozen refresh. The fixture now waits for the producer marker in frozen backing before
disabling refresh and keeps the independent final cursor/content assertion.

Validation on this host: the Python gate has 31 passing tests. Final `zz-terminal` has 330
passing tests and one ignored. Native clone/constructor tests pass 116/116 in both
ReleaseSafe and ReleaseFast; the excluded safe wrapper passes 34 unit and 20 doc tests with
three doc tests ignored, and sys passes three tests. The five adjacent daemon COPY tests pass.
Root workspace and both excluded manifests pass strict all-target/all-feature clippy. Rust/Zig
formatting and both static C integration clients pass. The all-feature workspace test run
finishes without hangs. Its known endpoint test fails because this host has zz on PATH, and
the known UI caps test fails. The daemon large-argument and same-image-exec tests fail under
workspace load and pass when rerun alone. Three new input fixtures initially used
control-channel `RawInput`; they now use classified PTY input and the whole terminal package
passes. This changes the fixture, not RawInput routing.

W3-SHARDS must preserve the retained native actor, its search worker/results and pending
policy, classified input close/drain, released PTY scratch, and immediate EOF read/accounting
contract. The 64-row cache is bounded; search recompresses in 512-row batches. W4-ROWS owns
remaining independent cell/grapheme metadata queries and live frame extraction on copy
cancellation; keep the safe snapshot ownership and borrowed row bounds when adding a bulk API.
Integrate the two retained-event daemon edits with CTRL. The native and safe-wrapper copy
changes now live in published fork commits, pinned for ordinary fetched-source builds.

Review fixes (2026-09-30): the published copy fork pins are
Ghostty `7823f65dd55fc9ff420d5eb5cae761cbd1995994` and libghostty-rs
`8e40135fb20e9ed91c37c374fe1d14570c386d06`, each one commit on its required parent.
The default optimized release hash is
`c03d88d26c893b19446282177818e0fe099056454c0f32c31c268c5bde49247b`.
`/home/demfabris/.cache/zz-perf/copy/fixed.json` records the prescribed quick
`mem,throughput` run: 0.5859 MiB entry footprint, 1.2130 ms CPU, 1.2426 ms reply wall,
4.3228 Minstr, 129.5623 MB/s detached ASCII and 130.6971 MB/s cooked-PTY ceiling.
All entry samples meet the floors (max CPU 2.4375 ms, wall 2.8486 ms). The run exits 1
with six passes, one unchanged `mem.threads.p20` failure at 66 threads and seven
informational rows. It has no errors, regressions, strays or killed orphans.
`--only copy` now selects entry measurements alone; selecting it with `mem` avoids
repeating entry. Scratch follows `TMPDIR`, while sockets remain directly under `/tmp`.
The standalone copy run exits 0 with three passes and two informational rows:
0.5859 MiB footprint, 1.1842 ms CPU, 1.2065 ms wall and 4.3227 Minstr. Its evidence is
`fixed-copy.json`, with no errors, strays or orphans.

Four-request operation medians, same dense frozen backing:

| Operation | Review lane Minstr | Fixed Minstr | Reduction |
|---|---:|---:|---:|
| search | 545.309 | 487.421 | 10.62% |
| capture | 1322.107 | 915.366 | 30.76% |
| capture_vt | 1718.592 | 988.553 | 42.48% |

Output byte counts remain unchanged. Search still decodes native rows on each query;
captures still decode rows with bounded storage. Their deferred read cost remains above
the flat base; the fixes remove repeated row-cache access and the scalar iterator without
restoring whole-history arrays. The instruction evidence is in `fixed-ops.profile.json`.

The ordinary release strips symbols. A supplemental optimized build with
`CARGO_PROFILE_RELEASE_STRIP=none` exits 0 and retains symbols for attribution. Its text
section differs by 2112 bytes (0.0123%); repeated operation instructions differ by less
than 0.002%, with the same output byte counts. The default stripped binary remains the
gate artifact. Five-second search and capture profiles record 4917 and 4599 samples,
with no losses and clean daemon exits. `String::extend<&char>`, `ModeRevision::cell` and
per-cell hash construction no longer appear above the 0.5% reporting threshold. Search
spends 28.87% in native grapheme reads and 21.72% in grid-cell reads; capture spends
20.63% and 11.95% respectively, plus 15.88% in bounded row conversion. The independent
getter and page-cooling work remains assigned to W4. Reports are
`fixed-search-symbols.perf-report.txt`, `fixed-capture-symbols.perf-report.txt` and
`fixed-perf-validation.json` under the lane cache.

Validation after review: full native suite 6490 passed/68 skipped, wrapper default suite
30 wrapper and three sys tests plus 19 doctests passed/three doctests ignored, terminal
332 passed/one ignored with both default and flat rollback storage, and 36 daemon copy
filter tests passed. All five lane daemon tests pass with rollback and with compression
suppressed. Full daemon runs retain the known PATH endpoint failure; four different load
failures pass alone. All 35 daemon integration tests pass, with one ignored. Strict
all-target/all-feature clippy passes on touched crates and the wrapper; fmt and OKF
validation pass. Native exports number 205, all 199 generated function declarations
resolve, and the strict C ABI client links and runs.

The final compat suite passes 27 scenarios/252 steps with zero divergences or retries.
Lane and recorded base TUI copy fixtures each pass 141/147 and their logs match byte for
byte, including the earlier base transcript. Tracker validation and cleanup pass. The
wrapper's costly standalone Debug fixtures bound rich metadata and mutation work while
separate 1000-row and 10k ownership cases retain shared-page coverage. Fork checkouts stay
clean and unpublished; the orchestrator must publish both commits, replace both literal
pin placeholders and regenerate `Cargo.lock` before ordinary builds.

The six copy TUI fresh-entry prompt Escape/cancel failures are byte-identical to the baseline.
Their first divergence is that pinned tmux retains the prompt after Escape while zz closes it;
then tmux consumes `q` in that prompt while zz cancels copy mode. The relevant daemon
functions are `command_prompt_key`, `prompt_translate_key` and `command_prompt_edit_key`,
outside the COPY write zone. The cause remains unproven. macOS launch, clonefile, kqueue, PTY
bridge, ri_instructions, GUI profiling and iOS builds cannot be checked on this Linux host.

Merged 2026-09-30 (`2cedf6ee`, Linux gate `w2-3-copy-alienware-2cedf6ee.json`): both fork
commits are published and pinned, entry 52.70 to 0.55 MiB and 104.8 to 1.02 ms on Linux. The
Mac entry read 1.41 MiB against the 1.0 MiB rule because a copy-on-write snapshot of a small
active area allocated standard pages and read their unused tails. Ghostty `67351380` copies
active pages at their used size and `ModeRevision` builds viewport cells directly into the
`Arc` (no temporary vector): Mac entry 0.58 MiB, 5.58 to 5.00 Minstr.

## W3-SHARDS: PTY shard threads inside zz-terminal (effort XL)

macOS echo fairness follow-up, 2026-10-01, perf/echo: `Shard::run` handles
input before PTY output and dispatches input and echo windows first. Shared read
turns yield only for peer input or an echo window; output readiness keeps full
256 KiB / 1 ms turns. Bridge retries check input and echo work every 32 attempts,
without peer fd polling. The existing shared-shard test streams 32 echoes with two floods.
Five alternating quick pairs against `c59ee369`'s merged binary measured checkpoint
`1bf5a79a`: median per-run zz/tmux busy p50 ratios 2.331 -> 2.653 (1.138x),
busy p99 1.877 -> 2.886 (1.538x), above both 0.93x limits. Idle p50/p99 factors
were 0.897x/0.670x. Chatty flip/hidden medians were 63.080 -> 63.169 and
77.591 -> 78.927 Minstr/s (1.001x/1.017x); detached ASCII was
302.066 -> 306.191 MB/s (1.014x). All ten runs were load-marked.
The chatty and throughput limits pass, but busy echo does not; this follow-up is not gate-ready.
Both terminal modes passed 353 tests with one ignored; terminal clippy passed.
All four requested compat scenarios passed 16 steps without divergences.
The quick W0 rescore of the fifth candidate run exited 0 with four existing echo
warnings and one idle-p99 regression flag. It took no extra measurements.
Quick mode omits Unicode and attached ASCII throughput; the headless client was absent.
Busy30 uses a ticker in the echo pane rather than shared-shard floods. Linux throughput,
gather, epoll, THP and TUI backpressure remain with the orchestrator.
Evidence stays in `/tmp/zzpc/echo2-{merge,fair2}-[1-5].json`.

macOS chatty follow-up, 2026-10-01: `session/shard.rs` `Shard::poll` now retains
PTY, wake-pipe and child PID watches in one kqueue per shard. Level-triggered
PTY reads keep the turn caps; real notifications and newly writable queued input
trigger channel checks. Parse scratch allocates on first use. Three alternating
quick runs against `shards-s1-mac-cli` gave flip medians 67.7897 -> 62.2157 Minstr/s
(0.918x) and hidden 79.2885 -> 77.7577 (0.981x). The p20 footprint median was
17.4224 MiB, below the lane base's roughly 17.7 MiB, with 30 threads in each run.
Both terminal test modes passed 339 tests with one ignored; clippy and all six
requested compat scenarios passed without divergences.
The four echo timing rows remain red on s1 and this build. Linux throughput,
gather, epoll, THP and TUI backpressure checks stay with the orchestrator.

Scope, keeping the `TerminalSession` public API: K shard threads (K = min(available_parallelism,
4), `ZZ_PTY_SHARDS`, chosen by the gate) own PTY fds and terminals, never migrating; each polls
PTY fds, child exit (`EVFILT_PROC` / pidfd) and a wake fd; never `waitpid(-1)` (run-shell,
pipe-pane, ssh children are reaped elsewhere). The per-pane actor becomes a state machine
(`on_readable`, `on_command`, `on_deadline`) keeping 64 KiB reads, 256 KiB / 1 ms turn caps,
16 ms time-sampled frames and the echo fast path; `PTY_BRIDGE_SPIN_MAX` spins only while the pane
is its shard's only ready pane. One lazy search thread per daemon. Fold Linux `zz-pty-gather` only
if `bench/run.sh` on Linux matches. Own fork: allocation-free child with prebuilt argv/envp, then
setsid, `TIOCSCTTY`, dup2, closefrom, chdir, execve. One live frame per pane per publish shared
by all live views; per-view frames only for copy mode or scrollback. Windows ConPTY keeps a
reader thread per pane feeding its shard. Shard -> loop channels are unbounded or coalescing, so
a shard never blocks on the loop.

Write zone: session.rs spawn fns, `run_terminal`, `wait_for_wake`, wake-pipe drain, child wait,
pty gather, `SearchWorker`, `publish_active_views`, `output_view_worker`, view activation; new
`session/shard.rs`; shell_integration.rs spawn command building; portable-pty removal in
manifests.

Gate vs W2 exit JSON: zz-terminal threads = K; `mem.footprint.p20` <= 20 MB; one flood >= 4x tmux
and >= 0.85x W0, and 4 concurrent floods >= base aggregate; `echo.p99.busy30` p99 in an idle pane
while 4 floods run <= 1.5x tmux; `throughput.attached.ascii_ms` <= 1.18x W0. Tests: zz-terminal
tests, daemon exit / remain-on-exit / respawn / job control (Ctrl-Z, SIGWINCH, foreground pgid),
`compat/run.sh`, compat/tui fixtures, `bench/run.sh` on macOS and Linux.

As built (slice s1, 2026-10-01): `crates/zz-terminal/src/session/pane_actor.rs` holds the pane state and event handlers; `run_terminal` waits and dispatches on the pane thread.
The handlers retain the read and parse turn caps, frame sampling, echo fast path, bridge spin, child exit and retained-pane handoff.
Linux checks: 332 terminal tests passed; terminal clippy and six compatibility scenarios passed. The known daemon endpoint test stayed red; three daemon load failures passed alone, and 35 integration tests passed.
The orchestrator owns the perf A/B and Mac checks.

As built (slice s3, 2026-10-01): Unix uses crate-local command, exit-status and PTY-size types in `crates/zz-terminal/src/pty_types.rs`; the existing spawn path stays unchanged.
The normal dependency trees contain no `portable-pty` on Linux or macOS; Windows keeps it as a target dependency.
Linux checks: clippy passed, both terminal modes passed 337 tests with one ignored, and seven compatibility rows passed. The known daemon endpoint test stayed red; four load failures passed alone, and 35 integration tests passed with one ignored.
Windows builds, Mac runtime checks and the perf gate were not run for this slice.

As built (slice s4, 2026-10-01): `session.rs` `SEARCH_SCHEDULER` starts one process-wide `zz-terminal-search` thread on the first submitted search.
Each actor keeps its own coalescing job and result mailbox, view cancellation tokens, and wake; dropped actors cancel their outstanding jobs.
The shared worker preserves view ordering, wrap selection and match scratch, drops stale jobs, and sends each completed view result to its actor.
Linux tests count one named search thread across three panes and cover overlapping view IDs, stale requests, actor wakes and output-pane results.
Clippy passed; both terminal modes passed 350 tests with one ignored; five compatibility rows passed; 35 daemon integration tests passed with one ignored.
The known daemon endpoint test stayed red; four load failures passed alone. Mac checks and the perf gate were not run for this slice.


## W3-LOOP: single-owner mux loop (effort XL)

As built, e19fix (2026-10-02, base `6e7d954e3`): empty peer updates skip format-fact
snapshots, and drained peer-probe workers exit immediately. Other helpers reuse workers for
250 ms. Path acknowledgements and discovery output wait for events; the loop owns job
deadlines and shares path cancellation flags. A full helper queue refuses loop submissions.
The p20 counter test reports zero helper tasks and zero starts per steady `send-keys`.
The pre-commit Mac quick W0 gate passed 36 scored rows: `send_keys.p20` was 0.1805 Minstr
against e19's historical median 0.2242; idle CPU/instructions/wakeups were zero, with 2/5
threads at p1/p20. Live 100-command samples matched e12 at about 0.165/0.167 Minstr for
p1/p20. Final three-pair comparisons belong in `/tmp/zzpc/e19fix-{pre,post}-{1,2,3}.json`.
Linux process readers, PTY gathering, epoll, THP, and TUI backpressure remain with the
orchestrator; this Mac did not run them.

**b3fix as built (2026-10-01, on b3 `53982eb8`).** Exec prepares each chain once. Proven queries, plain input without attached clients or hooks, and unchanged unzoomed pane selection run on the mux loop. Parking commands, relative pane selectors, and cleanup that can run hooks keep workers. Output pressure yields the remaining commands with response admission and reply IDs intact. Repeated chains retain registration data; LAST/ExecExit, Resume, and client-file replies keep their existing order.

Three alternating quick `cli,control,mem` pairs against b2fix give 19/19 CLI instruction medians at 0.895-0.988x; display p1/p20 is 0.901/0.920x, control instructions 0.984x, and threads 7/45 versus 8/45. Serial daemon tests pass: 1360 unit and 35 integration, including 20 Exec and 34 event-loop tests; one existing test is ignored. Fmt, Clippy, and all eight selected compat scenarios pass. The quick W0 gate has 35 passes and one failure on control latency. Control latency and burst throughput also fail on b2fix, so those rows are left outside this fix.

Linux `/proc`, zz-pty-gather, epoll, THP, and `tui-output-backpressure.sh` were not run on this Mac; the orchestrator owns those checks.

### b2 fixes as built (2026-10-01)

- Initialization checks cancellation before work and between attach commands. Clean Interactive EOF
  runs admitted input and drains replies; aborted compact Hello releases held Attach output.
- Pending input is limited to 4096 messages and 16 MiB; the count allows pasted-input bursts. GUI and client-file responses bypass a
  parked command. Acceptance yields after 32 connections, schedules continuation, and checks shutdown.
  Failed worker starts post completion. Bulk image limits include reliable messages held by the writer.
- Read-only queries without pending or command hooks run on the loop, in request order. Actor-backed
  `capture-pane`, prepared legacy requests, and control lines requiring expansion stay on workers.
  Direct quiet writes require the loop owner. Loop-owned replies skip self-wakes and writer
  condition-variable notifications. Raw output readers
  start when bytes arrive, retaining the existing streaming path.
- Three alternating quick `control,cli` runs against b1 `dd97bbd3`: control instructions median
  0.0724 -> 0.0684 Minstr (0.9448x); all 19 CLI instruction medians <=1.0108x (limits 1.05x/1.10x).
  Raw output median 138.5609 -> 141.2872 MB/s. `control.latency` and
  `control.burst_cmds_per_s` thresholds against tmux were red on both binaries; no errors.
- Validation: 1347 daemon unit tests and 35 integration tests passed, one existing test ignored;
  clippy, formatting, all seven requested compat rows, and the full attached-client fixture passed.
  Quick W0 `control,cli`: 29 pass, one pre-existing latency failure, 34 informational rows,
  no regressions or errors. Perf `attach` was omitted under the brief's explicit group restriction.
- Handed on: first-frame routing and Exec connections still use the existing worker pool;
  registration, initialization, parkable commands, cleanup, preview work, and active raw output
  still use workers. Mutex removal and park-point continuations remain later W3 work. Linux `/proc`,
  PTY gather, epoll, THP, and TUI output backpressure were not run on this Mac.


Commits that each keep tests green:
- (e0) Move the per-request thread_locals into a per-cmdq-item context:
  `CLIENT_KEY_INJECTION_DEPTH`, `DEFERRED_CONTROL_NOTIFICATIONS`, `COMMAND_QUEUE_PARK`. List every
  synchronous actor round trip reachable from command or lock-holding paths (`capture_screen` ->
  `terminal.capture`, `fresh_viewport`, identity waits, search) in the PR; each becomes a
  continuation.
- (a) `BTreeMap<ClientId, Client>` replaces the ClientId-keyed maps in `ServerState`.
- (b) One `zz-mux` thread on mio: listener, nonblocking client sockets (per-client read buffer,
  `writev` on writable), Waker, signal self-pipe (signal-hook replaces async-signal, async-io,
  futures-lite, `DaemonSignalGuard`), timer heap absorbing the W1 timer thread and the status
  sampler. The status tick runs only while a shown status needs it; control subscriptions and
  format monitors get their own timers. Per-connection threads go away; the Mutex stays, taken
  only on the loop.
- (c) Pane watcher threads post `ViewportReady` and events through a channel + Waker.
- (d) Delete the Mutex; `inner.lock()` sites become `&mut`.
- (e) tmux-style cmdq with continuations for the 16 park points and sleep loops: wait-for,
  run-shell and if-shell jobs (pipes and child exit on the loop), confirm-before, command-prompt,
  menus, popups, pane `-W` waits, client stdin and file reads, `agent-send --wait`, identity waits.
  **`#()` status jobs join the loop**: job pipe and child exit registered with it; the
  `zz-status-job` reader thread and the terminate-on-drop thread in status.rs go away. A small
  helper pool only for items not yet converted, listed in the PR.

Write zone: all production code in daemon.rs (no other lane edits it in wave 3), `daemon/*.rs`,
transport.rs, status.rs, `agent/*` posting, zz-daemon `Cargo.toml`.

Gate vs SHARDS JSON: `cli.cpu.display` <= max(1.2x, tmux + 0.03 ms) at p1 and p20;
`spawn.cpu.split_shell` <= 1.2x; `spawn.cpu.kill_pane` <= tmux + 0.1 ms; `idle.wakeups_per_s.p20` <=
0.2/s with no client timers; fixed daemon threads = 1 loop + agent threads;
`statusjob.threads_per_s` <= 0.5 and `statusjob.cpu_pct` <= 1.5x + 0.1 pt; chatty no worse; 0
`lock_slow` samples under chatty; 20-pane `list-keys` and a 20-pane `capture-pane -S -` on large
history delay another pane's echo p99 no more than tmux does. Tests: workspace (flaky daemon tests
re-run alone), `compat/run.sh`, compat/tui, control-mode fixtures, `cargo check` Windows, full bench
at `wave3`.

### e0 as built (2026-10-01)

Each queue item now owns one `CommandItemContext` on its `Shared` execution handle; nested commands, hooks and injected keys keep that handle.
Independent workers and agent publishers keep the server owner. `SharedServer` holds the existing server fields and sampler cleanup; this slice adds no threads, dependencies or wire fields.
Checks passed: formatting, daemon clippy, 1,327 unit tests and 35 integration tests (one existing soak ignored), plus all eight required compat rows. Two parallel-suite failures passed alone and the full suite passed with `RUST_TEST_THREADS=1`; child compat scripts needed Homebrew Bash first in `PATH`.
The table records the waits that step (e) must replace with command-item continuations; e0 keeps their current behavior.
Functions live in `crates/zz-daemon/src/daemon.rs` unless a row names another file; terminal actor methods live in `crates/zz-terminal/src/session.rs`.

| Function | What it waits for | Continuation in step (e) |
| --- | --- | --- |
| `execute_with_mux_source_inner` -> `wait_for_terminal_identity` | Spawned pane publishes its PID and TTY, or the two-second identity deadline expires | Identity reply or timer; rebuild spawn-format facts and resume |
| `execute_with_mux_source_inner` (copy-mode tail) | `terminal.settle()` acknowledges prior copy-mode actions, including synchronous copy search | Settled reply; finish the command and its after hook |
| `run_copy_mode_search` (`zz-terminal/src/session.rs`) | History scan on the actor delays the caller's settle reply | Search result; apply cursor and selection updates before completing the settle continuation |
| `capture_pane` | `terminal.capture()` returns live or retained-pane content | Capture reply; print output or store the paste buffer |
| `capture_screen` (`wait_pane`, `run_pane`, `paste_and_submit`) | Capture replies for pattern, command-output and paste-echo polling | Capture reply; match content, then resume or park on pane output and the deadline |
| `capture_last_command_for` (`send_last_output`, `show_last_output`) | `terminal.capture_last_command()` returns shell-integration marks and output | Semantic-capture reply; deliver or display the result |
| `send_compact_resync` (`daemon/ctrl.rs`) | `terminal.fresh_viewport()` publishes a fresh frame | Viewport reply; send the resync frame |
| `pipe_pane` | Raw-output tap arm acknowledgement; replacement also calls `stop_pane_pipe` while pipe serialization is held | Tap reply and old-pipe cleanup; finish installing the pipe |
| `rearm_pane_pipe` | Raw-output tap arm acknowledgement after pane replacement | Tap reply; retain the pipe or stop it on failure |
| `start_control_output_tap` | Raw-output tap arm acknowledgement, with one retry on timeout | Tap reply or retry timer; install the control reader |
| `stop_pane_pipe` | Raw-output tap disarm acknowledgement after child and reader cleanup | Tap-disarmed reply plus child/reader completion; finish pipe cleanup |
| `stop_control_output_tap` | Raw-output tap disarm acknowledgement followed by reader join | Tap-disarmed reply and reader completion; finish control-tap cleanup |
| `kitty_image_frames` | `terminal.kitty_image()` returns image pixels and generation | Image reply; build and cache the outbound image frames |
| `evict_absent_kitty_images` | `terminal.kitty_image_generation()` answers once per cached candidate | Generation reply; advance the candidate cursor and evict stale frames |
| `pointer_format_variables` | `terminal.pointer_context()` returns the word, line and hyperlink under a cell | Pointer-context reply; expand mouse formats and dispatch the command |
| `DeferredTerminalCommand::run` (`ArmCopySource`) | `source.capture_copy_source()` clones the source screen on its actor | Copy-source reply; set the target's pending source before entering copy mode |
| `TerminalSession::capture_frozen_frame` (`zz-terminal/src/session.rs`) | Live capture request before falling back to the cached viewport when the actor has stopped | Capture reply or stopped-actor fallback; resume the retained-pane capture |
| `send_history` (input path) | `terminal.history()` returns history rows and their dictionary | History reply; enqueue `HistoryChunk` |

Native `SearchBegin`/`SearchUpdate` already use asynchronous search results; `DaemonFormatHooks::pane_search` in `status.rs` scans a cached viewport under command-format callers and has no actor round trip.

**a1 as built (2026-10-01):** `Client` in `crates/zz-daemon/src/daemon.rs` holds the 22 identity and terminal facts in `ServerState::clients`.
`register_identity` and `daemon/exec.rs` `register_exec` create the record; `unregister` removes it once. Detach clears activity and last-session facts while retaining identity.
`ClientFormatFields` and `BorrowedFormatHookFacts` borrow the client map. The other ClientId-keyed fields remain in `ServerState` for a2 and a3.
Checks passed: formatting, daemon clippy, 1,327 unit tests and 35 integration tests (one existing soak ignored), and all ten required compat rows with zero divergences; child scripts needed Homebrew Bash first in `PATH`.

**a2 as built (2026-10-01):** `Client` owns the 28 per-client mode, key, visibility, path and status fields listed for this slice; none remain in `ServerState`.
`unregister` drops those fields with the client record after detach handles mode cleanup. Delivery and control maps remain for a3; fields keyed by other ids stay in place.
Format readers borrow the client fields, and diagnostics collect the same mode snapshots from them. This slice adds no threads, dependencies or wire changes.
Checks passed: formatting, daemon clippy, 1,327 unit tests and 35 integration tests (one existing soak ignored), and all 12 required compat rows with zero divergences. The terminal-peer test failed in the first suite, passed alone, and passed in the repeated full suite.

**a3 as built (2026-10-01, `3560dece`):** `Client` owns delivery and control state, and `ServerState::clients` stores `Box<Client>` records.
The lane's a3 measurement put CLI instructions within 1.4% of a1.

**a4 as built (2026-10-01):** `ServerState::client`, `client_mut` and `client_entry` shorten client lookups; `Shared::read_client` reads client fields under the existing lock.
After `cargo fmt --all`, `daemon.rs` has 116,416 lines, 2,575 fewer than a3, with no formatting skips, new macros or behavior changes.
Checks passed: daemon clippy, 1,327 unit tests and 35 integration tests (one existing soak ignored), and all six required compat groups with zero divergences. The perf gate was not run.

**b1 as built (2026-10-01):** `crates/zz-daemon/src/daemon/event_loop.rs` `EventLoop::run` accepts Unix sockets on the foreground mio loop; connection workers and their mutex access remain.
Startup replay runs on a temporary worker. Its channel and Waker return completion to the foreground thread, which joins the worker and invokes readiness; shutdown wakes the same loop.
The two macOS memory runs measured settled threads against `b47ffeae`: p1 fell from 9 to 8 and p20 from 46 to 45. Native thread inspection found no `zz-daemon-accept` or startup worker.
Checks passed: formatting, daemon clippy, 1,330 unit tests and 35 integration tests (one existing soak ignored), and all seven required compat scenarios with 175 steps and zero divergences.
Windows named pipes retain the blocking transport. The Windows target check was unavailable here; Linux-only checks remain for the orchestrator.

### As-built: writer shutdown follow-up (2026-10-01, Linux)

In `crates/zz-daemon/src/daemon/event_loop.rs`, `disconnect` retains client writer registrations
through partial-output drain. `remove` drops the matching registration before signaling writer
completion; disconnect workers release session state. Response admission still precedes writer
shutdown, with the same deadlines.
At `8129cf07`, the original kill-server test passed 0/1 and two deterministic regressions passed
0/2. With this fix, they pass 20/20 and 2/2. Formatting and daemon clippy pass; all four selected
control/client-exit compat scenarios report zero divergences. The daemon suite passes 1362/1364:
the known macOS CLI fallback test stays red alone, and the process-info large-argv test passes
alone after failing under load. All 35 integration tests pass, with one soak ignored and no doc
tests. The orchestrator owns macOS checks; this step runs no perf gate.

### b5 command-cost fix (2026-10-01)

`ResponseAdmissionGuard::finish` notifies response waiters and the mux loop only after
admissions freeze. The existing admission mutex protects the count and freeze together.
Three alternating quick `cli,mem` runs against b4 measured instruction median ratios of
0.9836..1.0167 across all 19 CLI rows, below the 1.02 limit. Thread counts stay 6 -> 4 at p1
and 44 -> 42 at p20. The poll-wake test covers normal completion, frozen admissions, and
the final outstanding response. Final fmt, clippy with warnings denied, the serial daemon
suite, all four requested compatibility scenarios, and the quick W0 gate passed. The W0
gate reports 32 pass, 0 fail, and 37 info rows. Later loop slices retain the b5 signal and
drain phases; the orchestrator runs Linux-only checks on its Linux host.

**c02 as built (2026-10-01):** `TerminalEvents::install_notification_sink` posts coalesced readiness to `daemon/watchers.rs` `LoopWatchers`, which owns pane, command-output and popup receivers; their relay threads are gone.
Each surface has one readiness entry, bounded event drains and weak terminal ownership; the four reliable slots and one viewport slot remain. The last producer closes and notifies the stream even without a final event.
Three alternating quick pairs against `1518f03d` measured threads 3 -> 2 at p1 and 25 -> 5 at p20; all 22 instruction medians stay within 1.0256x B, hidden chatty is 0.9328x B, and idle instructions and wakeups are zero.
Checks passed: 360 terminal tests, 1,389 daemon unit tests, 35 integration tests, Clippy and all seven compat fixtures (102 steps, zero divergences); one existing test per crate remains ignored. Linux-only checks were not run on this Mac.

**e02 as built (2026-10-02, Linux):** Hello, Control and Exec root items retain their command queues; converted queries run on the mux loop, and each legacy command leaf returns its remaining cursor after one worker turn.
Socket writes resume output continuations without `LoopExec::wait_for_output` or an output-only worker fallback; LAST, RESUME, first errors, ExecExit and file/GUI reply bypass retain their order. Twenty idle Exec connections followed by a 200-query chain use zero execution workers.
Registration, startup/terminfo, legacy attach/resync, expansion resolution, nonconverted command leaves, other client messages and hook-capable cleanup still use workers; this slice keeps the helper pool and wire version 107.
Checks: 48 loop tests, 68 control tests, the 640-command chain, 35 integration tests (one soak ignored), formatting, daemon Clippy, six compat groups (186 steps, zero divergences) and nine TUI backpressure assertions pass. The serial daemon suite passes 1,415 tests with two known host failures; status-job EINTR passes alone, and the macOS CLI fallback stays red on Linux.
Three alternating quick `cli,control,chatty,mem` pairs against the supplied `loop-ed5ba3aa-cli` give all 19 CLI instruction medians <=1.00365x; display p1/p20 and chain5 p20 are 1.00140x/0.99080x/0.99301x. Control instructions are 0.0457/0.0462 Minstr (0.98918x), and thread medians remain 3/25. Control latency is red on both binaries; burst throughput is also red in one baseline run, with no new or outside-lane reds.
macOS posix_spawn, kqueue, PTY spin bridge, ri_instructions and iOS checks were not run on this Linux host; the orchestrator owns them.

**e03b as built (2026-10-02, Linux):** `crates/zz-daemon/src/daemon.rs` keeps source replay in owned queue frames through `replay_config_file_in_queue_in_item`, preserving source depth, `current_file`, source-relative cwd, frozen aliases and warning order.
`run_hook_commands_with_policy` uses owned frames for after-command, event and shutdown hooks; continuation tokens resume parents after their child work and retain output and error ordering.
Event frames keep formats captured at mutation and repair targets at execution; source and hook child boundaries release input change windows before suspension.
Three alternating quick pairs against `loop-9f31b80d-cli` pass all 23 required rows: the largest instruction ratio is 1.004324x, chain5 p20 is 0.997947x, and thread medians remain 3/25. Control latency is red on both binaries with no harness errors.
Formatting, daemon Clippy, 3 new frame tests, 35 integration tests and 8 selected compat scenarios pass with zero divergences; the serial daemon suite passes 1,422 tests with three known failures, including the status-job test that passes alone. macOS and Windows checks were not run on this Linux host.

**e06 as built (2026-10-02, Linux):** Both `wait-for` receiver parks now register cmdq continuation IDs; signal wakes all waiters, lock grants transfer FIFO, and cancellation after a grant transfers the lock to the next waiter.
Owned root and inserted queue frames preserve nested aliases, callbacks, hooks, reply order and sticky signals. Twenty signal, lock and inserted waiters add zero workers and complete each continuation once.
Seven continuation tests, named wait-for/shutdown tests, formatting, daemon Clippy and five compat groups (62 steps, zero divergences) pass. The serial suite passes 1,427/1,432; four failures pass alone, and the known macOS-bundle fallback failure persists on Linux. All 35 integration tests pass, with one existing soak ignored.
Three alternating quick `cli,control,mem` pairs against `loop-b626aa02-cli` put all 19 CLI instruction medians at 0.99814–1.01563x; `chain5.p20` is 0.2425 -> 0.2454 Minstr (1.01196x), and thread medians remain 3/25.
Control latency is red in all six runs; burst throughput is red only in new-2. The runs have zero harness errors; these tmux threshold rows are outside this slice's instruction/thread comparison.
Shell jobs and other unconverted leaves, registration/startup and hook-capable cleanup still use workers. The Windows/test synchronous adapter remains; macOS, Windows, iOS, full-workspace, TUI and full-bench checks were not run for this slice.

**e05 as built (2026-10-02, Linux):** `daemon/terminal_reads.rs` and `daemon/terminal_requests.rs` resume capture, semantic output, send-text, wait-pane and run-pane reads through actor reply tokens and cmdq continuations; synchronous terminal APIs remain for off-loop callers.
History and mouse pointer reads enter on the mux loop; completions recheck the client and terminal identity. Mouse bindings retain the existing worker adapter after the pointer reply, with connection input ordering preserved.
Kitty pixel and generation replies resume publication asynchronously; terminal-bound cache entries and atomic delivered-generation checks reject stale respawn and eviction results.
Checks: 372 terminal tests, 1,456 daemon unit tests and 37 integration tests pass (one soak ignored); seven stale-result/loop-dispatch tests, the three named capture/history/frame tests, fmt, Clippy and five compat scenarios (73 steps, zero divergences) pass. The known macOS-bundle fixture fails on Linux; the cwd fixture passes alone and in the serial rerun.
macOS, Windows, iOS, full-workspace and full-bench checks were not run; the final six quick instruction/thread measurements remain in `/tmp/zzpc/e05-{base,final}-{1,2,3}.json`.

## W4-DELIVER: frames straight from shards (effort L)

Scope: the loop pushes per-pane subscriber sinks (queue handle, stream kind, delivered base,
frozen flag) to the owning shard on subscription change. The shard encodes each `PaneFrame` once
per (pane, base) as `Arc<[u8]>`, enqueues on every matching sink (latest-wins, NeedsFull kept),
and wakes the loop only when a queue goes empty -> non-empty. Delete `watch_terminal`, the
`zz-pane-N` threads and per-frame lock traffic in `publish_terminal_for_pane`. Edge events post to
the loop (title, OSC 7 cwd, bell, exit, clipboard, OSC rename, copy results, kitty images,
placeholder binds, activity edge, foreground change, checked at most every 500 ms per pane with
output). `#{pane_current_command}` / `#{pane_current_path}` resolve at expansion through the W1
direct calls. Activity and silence use an atomic last-output timestamp and loop timers.
Control-mode `%output` becomes a shard-delivered view kind; delete `refresh_control_output_taps`,
`run_control_output_tap` and the `zz-control-output-N` threads. **Per-pane stream sequence
barriers**: every loop notification about a pane (`%exit`, `%window-close`, mode changes, pane
removal) carries "after seq N" and the client queue holds it until that pane's frames and
`%output` up to N are queued, so final output precedes exit (`control-mode.exit-pane-output`,
wire-protocol.md).

Write zone: daemon.rs `watch_terminal`, `close_exited_terminal`, `publish_terminal_for_pane`,
`synchronize_pane_runtime`, `refresh_control_output_taps`, `run_control_output_tap`,
`OutboundState` terminal slots, subscription handlers; session.rs sink API, `EventQueueState`,
shard publish path; formats.rs current command/path callbacks.

Gate `--stage final`: `mem.threads.p20` <= 12; `mem.footprint.p20` <= 12 MB, `.p1` <= 5 MB;
a control client adds 0 threads (today +23); `chatty.*` <= 1.2x tmux + 0.5 pt; 1 encode per frame
with 2 clients on a pane (counter); throughput rows. Tests: `compat/run.sh` (monitor-activity,
monitor-silence, automatic-rename, pane_current_command), compat/tui, control-mode `%output` and
exit-ordering fixtures, workspace tests.

## W2-FMT: tmux format.c model, no materialized context (effort L)

Reinstated under the no-compromise rule. W1-FORMAT makes the universe lazy but still resolves
through `StatusContext`, a ~100-field struct of owned strings built per item.

Scope: resolve through a tree of borrowed engine handles and typed client, session, window and
pane ids. Variable lookup goes options first, then the sorted static callback table, then
per-command variables, then the environment. Loops (`#{S:}`, `#{W:}`,
`#{P:}`) push a borrowed child tree. Parsed template operations are cached by source text;
they hold syntax, never option values. Options generation keys the option snapshots, needs
scan and indirect reference closure instead. Command facts borrow daemon maps and build
derived maps only on demand. Status and mode requests leave the engine lock through
`detach_with_references()`, which captures referenced values and reachable loop contexts.
Owned facts remain necessary for those requests. Unknown dynamic job output uses the full
capture path. The old `StatusValues` object is boxed and initialized only for explicit legacy
field access or rollback.

Write zone: formats.rs (all but the time helpers), the `StatusContext` users in status.rs and
command.rs `StatusRowVariables`, daemon.rs `format_hook_facts*`.

Gate `--stage wave2`: format code under 3% of samples in `cli.list_keys.p20`, status render and
`config.source_1000` profiles; `cli.cpu.list_keys.*` and `cli.cpu.list_*_a.s20` <= tmux CPU;
`chatty.cpu_pct.visible` improves on the W2-HOOKS merge. Tests: the format differential rows in
`compat/run.sh` (formats, formats-values, status rows, choosers) with exact output, plus a
property test that the compiled path and the W1 lazy path agree on every pinned format name.

Handed over from W1-FORMAT (see its As-built block): per-row template parsing is 25-30% of
`list-keys` p20; `list_windows` costs about 2.2 Minstr at s20 against tmux's 2.9 for the whole
command; the status template scan and `format_option_snapshot` run on every refresh because
nothing tells them options changed, so this lane keys the option and dependency caches by an
options generation; the per-command `FormatHookFacts` snapshot makes every `set @x` clone
that scope's user option map.

Original as-built on `perf/fmt`, 2026-09-30, measured source
`4c5b0b39c6536ada322894ee4aab461bcff3ef27`. Source checks and the three profile floors pass;
five owned listing CPU rows beat same-run tmux, and visible daemon CPU fell in the observed
scoped runs. Compatibility preserves all 44 pre-lane final scenario tuples. The full-chatty
command still exits 1 on client CPU. Current full strict exits 1 with 11 failed rows; paired
regression/ownership assessment is PENDING. Complete lane/merge acceptance is not claimed.

The implementation removes repeated context construction, template parsing, fact snapshots
and stable status expansion while preserving the full formatter and its rollback interpreter:

- `StatusContext` holds borrowed engine handles, typed scope ids and command variables. The
  sorted 198-entry table has a callback per name; lookup remains options, table, command,
  environment. Row adapters borrow strings. The legacy values/universe keep their leading
  field order, and explicit legacy access alone initializes boxed `StatusValues`. Borrowed
  daemon facts can reach the engine through the legacy universe when only borrowed-variable
  mode is disabled; nested children preserve engine, scope, client, variables and clock.
- Parsed templates hold nested operations, references and clock/loop metadata. A process-wide
  FIFO retains at most 512 entries and 1 MiB, charging actual source/operation capacities and
  a 64 KiB container reserve. Oversized entries bypass retention; cache-off parses fresh
  before lookup. Syntax keys use source alone because they contain no option values, a
  deviation from the original source-plus-options-generation plan. The container proof uses
  the current toolchain/hashbrown layout and needs review when either changes.
- Option snapshots use an immutable Arc keyed by options/mux revisions. Needs/reference
  caches check exact sources, hold at most 128 entries and invalidate on relevant option
  mutations, including arrays, hooks, user options, unset and default setters. A separate
  format-data revision covers environment, runtime facts, identity, start-command and activity
  writes. Only enabled status rows enter the dependency union. Static E/T follows all option
  scopes/arrays; cycles, dynamic indirection, implicit fact modifiers and shell output request
  conservative capture. Removing a scope may retain extra dependencies until an option write.
- Six one-entry result caches have individual 1 MiB admission bounds: default key listing,
  detached capture, status parameters, prepared request, border presentations and completed
  status. Arbitrary custom listing formats remain live. Default listings require exact flags,
  filters and key generation plus stable-option opt-in; custom collisions bypass reuse. The
  default-false native-options contract skips probes only for proven non-option row names.
  Cache hits defer context preparation and still create fresh `list-keys -1` effects. Literal
  set-option names borrow through `plain_format`; formatted names retain their RawText owner.
- Detached captures retain only referenced values and reachable loop parts. Early hits compare
  exact target/client/needs/overrides/revisions and reference identity or equality. Clock-only
  reuse overlays a fresh root and every S/W/P child clock; old contexts remain unchanged.
  `same_detached_data` requires detached captures, uninitialized legacy values, identical
  variable/universe allocations and every non-clock input. Whole output still keys the clock.
  RawText estimates charge both lossy text and retained raw bytes.
- Read-only fact views borrow daemon maps and derive client/buffer/mode/window maps on demand.
  Ordinary status capture selects dependency groups and client fields, retaining operational
  width plus referenced geometry prerequisites. Modes, jobs, unknown dependencies, Control
  clients and rollback keep full facts. A shared empty record avoids empty-map allocation.
  Live status contexts and direct row/key-table/message getters avoid model/option clones.
  Config files are captured and compared only when the closed status/border union needs them;
  dynamic/full/rollback paths retain the complete override and existing child/border answers.
- `StatusParameters` shares raw templates, environment, option snapshot, references and fact
  selection plans. Prepared requests share context/facts and match engine revisions, option
  Arc, scope/focus, client kind/terminal/features, size/viewport, scheme, config and startup.
  Weak engine/environment identity prevents replacement or address reuse from aliasing old
  inputs. Only width/colours/viewport callbacks qualify for preparation reuse; other client
  callbacks, live modes and jobs take the fresh path. Clock overlays still require exact
  capture-data identity and fresh borders. Lazy option selection uses the existing mutex.
- Border reuse retains immutable presentations and resolved owner ids for linked windows.
  Every hit checks headers and fresh referenced values; scalar pane-mode counts read live
  maps/terminal handles without derived maps. A miss pins one count for both style and guard.
  Proven clock-independent styles reuse across seconds without querying system time; dynamic,
  timed, missing or loop sources fall back. The producer passes its resolved window/revision
  into the hit probe. Borders do not replace the engine's detached-status entry.
- Completed/published output shares an Arc; the owned wire boundary still copies. Weak request
  identity allows a same-second hit before touched-job allocation, with the same publication
  tail. Job polling remains outside forced rendering. Static top-level portions reuse across
  clocks only after exact templates/options/capture data, scope, environment, scheme, layout
  and fresh callback guards. Whole completed output still requires the current second.
  The renderer marks clients only after completed hits; a maximum-ever-rendered id lets
  `forget` skip absent high ids without assuming monotonic allocation. Lower ids use ownership.
- Portion plans follow native E/T raw sources across captured option scopes, arrays and indices
  before strftime/nested expansion. User/data E/T remains fresh because child values are not
  fully proven. Raw `%` stays whole; ordinary substituted `%` remains data. Timed/dynamic
  dependencies stay fresh. Top W/P requires actual capture availability; malformed/early-stop
  cases use whole expansion. Adjacent raw values stay together so UTF-8 fragments join before
  conversion. Compiled/cache/borrowed-variable rollback disables segmented rendering.
  Fixed Copy theme colours and message-style Strings reuse only under the same static proof;
  dynamic palettes/styles remain fresh. Trimming/layout/title paths retain their behavior.
  Style wrapping uses checked direct writes, and base style revalidates only after an append.
- Admission counts initialized portions, capacities, Arc/Weak metadata, callbacks, options,
  captures and output. Option-byte memoization requires the same retained strong option Arc;
  `Arc::make_mut` invalidates it. Post-render context memoization requires exact capture data
  and all three public id capacities; preparation's byte memo also requires border Arc identity.
  Changed storage or legacy materialization recounts. Appended scalar/OnceLock payloads are
  charged by struct size. These are logical allocation bounds, not measured process RSS.
- Literal Command/Absent paths classify format needs once through private calls; rewritten
  streamed/mouse/alternate commands discard that local result. Default list-keys/source-file
  without `-F` withhold unused facts only after successful option parsing. Formatted variants,
  aliases, errors and immediate/after/error hooks retain fresh context. Refresh-client skips
  only unused executor formatter setup; its handler still resolves targets/options/facts.
  Source cwd is read only for actual source/reload effects. Default no-F list-keys also skips
  unused client selection; custom/formatted/malformed/eager/control paths stay fresh.
  `finish_pane_command` returns on success before an unused attachment lookup; nonzero
  completion routing is unchanged. Non-control clients return before an unused query.
  Detach names are captured before removal only when attached event hooks can use them. No persistent execution-context memo was added.

Rollback is independent and read once at startup:

| Setting | Restores |
|---|---|
| `ZZ_PERF_COMPILED_FORMATS=0` | W1 interpretation; no segmented status rendering |
| `ZZ_PERF_BORROWED_FORMATS=0` | Legacy owned values/universe with provider engine preserved |
| `ZZ_PERF_BORROWED_FACTS=0` | Complete owned daemon fact snapshots |
| `ZZ_PERF_FORMAT_CACHE=0` | Fresh compiled templates, options/dependencies and result captures |

Current source verification, all completed exits 0:

| Source/check | Result | Evidence |
|---|---|---|
| 4c5b nine-crate clippy, all targets/features | 22.30s, no lint errors | `/tmp/zzpc/fmt-final-unused-clippy.log` |
| 4c5b routing / eager routing | 9 / 9 | `/tmp/zzpc/fmt-final-unused-{routing,eager-routing}-tests.log` |
| 4c5b completion / withholding filters | Four completion cases and one withholding case | `/tmp/zzpc/fmt-final-unused-*-tests.log` |
| 4c5b formatting, diff and independent audit | Passed; no findings | Root pipeline |
| 4c5b nine-crate serial suite | 3,359 distinct passed, 2 ignored, 0 failed | `/tmp/zzpc/fmt-tests-final-unused-final.log` |
| 4c5b normal/all-four format sweeps | 299 each (149 daemon, 150 mux) | `/tmp/zzpc/fmt-final-unused-{format,rollback}-tests.log` |
| 4c5b each independent rollback switch | 127 each | `/tmp/zzpc/fmt-final-unused-ZZ_PERF_*-tests.log` |
| 4c5b profiling CLI build and identity | 4m49s; validated | `/tmp/zzpc/fmt-profile-build-4c5b0b39.log` |
| 4c5b debug / release CLI / release headless client | 37.23s / 3m32s / 1m16s | `/tmp/zzpc/fmt-{debug,release,headless}-build-4c5b0b39.log` |

The current full-suite count excludes seven nested subprocess summaries: 47 test targets,
9 doc targets and 3,361 identities. No individual >60s warning occurred; the whole daemon
target took 121.84s. Exact commands/exits and earlier check history are in
`/tmp/zzpc/fmt-final-check-ledger.md`; counts are `/tmp/zzpc/fmt-tests-4c5b0b39-final-counts.json`.
The current arm64 profiling binary UUID is `1F3EB5ED-A962-3973-A587-AFE3EA0C1C7E`, SHA256
`dbbb213ea1a737888efe9e707f338977f1107cfe7c27bc96ba3d594d774fc43d`, 21,043,832 bytes.
Identity validation exited 0; actual capture authorization retains the 60/180/60-second plan.
Current profile, scoped benchmark and compatibility results are below. The full strict
command exited 1; only its final paired assessment remains pending. Profile and scoped quick/chatty artifacts
are portable; measured source identity remains distinct from later documentation/results
HEAD. The benchmark release CLI SHA256 is
`79e77aaa61a5951ec7622399f83fcc7ba311df555b94e6bc3f0cadeffa3c76b1`.

The diff adds 160 test declarations (101 daemon, 59 mux), not 160 executed/generated cases.
Tests compare every pinned name/modifier against W1 over attached, detached, marked, zoomed,
dead and null fixtures; selective detach and nested loops/indirection; lookup precedence;
option/inheritance/unset invalidation; raw UTF-8/style/error parity; all bounds/rollback modes;
clock overlays and retained old contexts; twenty-window static portions with a ticking right
clock; timed E/T/palette fallback; fresh client/mode/terminal/viewport inputs; linked borders;
request/fact identity, public-capacity mutation, cleanup, routing/provenance and hooks.

Historical attribution counts each matching Running stack once, including formatter owners,
preparation, predicates, copies, serialization and drop costs. Inclusive weights are not added.
Matcher SHA256: `91a263c0c02e3e07e6e825c5e8aa9c9f4e79aa66cd8aa905421cd147fe91ab8f`.
All 42 valid phases below are symbolicated with zero unresolved weight and passing capture/
analysis commands. Source qualifiers matter: early captures were 5 seconds, the 251/8e/83
captures 20 seconds, 0b/4d captures 60 seconds, and later captures 60/180/60 seconds.

| Source | Keys matched/Running ms (%) | Status ms (%) | Config ms (%) | Result |
|---|---:|---:|---:|---|
| `e33198c3` | 483/700 (69.000000) | 432/496 (87.096774) | 53/1873 (2.829685) | Keys/status failed |
| `4e3192dd` | 3/145 (2.068966) | 30/93 (32.258065) | 12/1714 (0.700117) | Status failed |
| `ee71938f` | 1/136 (0.735294) | 14/68 (20.588235) | 19/1780 (1.067416) | Status failed |
| `1fde980c` | 3/180 (1.666667) | 8/92 (8.695652) | 14/1785 (0.784314) | Status failed |
| `25121456` | 13/610 (2.131148) | 27/299 (9.030100) | 66/6890 (0.957910) | Status failed |
| `8eac8457` | 12/543 (2.209945) | 12/265 (4.528302) | 83/7026 (1.181327) | Status failed |
| `83d5f420` | 10/546 (1.831502) | 12/258 (4.651163) | 69/7122 (0.968829) | Status failed |
| `0b9b90c0` | 20/1456 (1.373626) | 24/669 (3.587444) | 168/21137 (0.794815) | Status failed |
| `4d87bb90` | 9/1459 (0.616861) | 28/731 (3.830369) | 35/20584 (0.170035) | Status failed |
| `020491b2` | 17/1612 (1.054591) | 108/2815 (3.836590) | 47/19605 (0.239735) | Status failed |
| `f234e908` | 16/1433 (1.116539) | 51/2148 (2.374302) | 33/20254 (0.162931) | Floors passed; further avoidable work found |
| `534de43e` | 15/1496 (1.002674) | 70/2408 (2.906977) | 613/21738 (2.819947) | Floors passed; duplicate classification fixed in 5d |
| `5d6ba30e` | 17/1452 (1.170799) | 58/2198 (2.638763) | 340/21568 (1.576410) | Floors passed; two residual queries fixed in 4c5b |
| `4c5b0b39` | 14/1479 (0.946586) | 58/2295 (2.527233) | 354/20393 (1.735890) | Profile floors/audit accepted; full strict assessment pending |

Portable evidence:
`bench/perf/results/w2-4-fmt-profiles-macbook-4c5b0b39.json`, generated and validated with
exit 0, SHA256 `b4363a018676c31a8a4e502b9fc2ba70ee133ff283356a629b8d8447842d27d0`.
It preserves all 42 valid phases, three unresolved baseline phases as N/A and the original
020 infrastructure failure. Current reports/projections are
`/tmp/zzpc/fmt-after-4c5b0b39-{profile-report,stack-projections}.json`; all export/symbol/
analysis stages, full-stack exports and end-identity checks exited 0. Independent audit
inspected all 79 disjoint chains and found no remaining concrete duplicate; unassigned PCs
stay counted, without a claim of per-instruction irreducibility. Audit:
`/tmp/zzpc/fmt-after-4c5b0b39-independent-avoidable-audit.json`. Both discarded-query chains
are absent; config classification is outer 317 + replay 8 ms, with no inner/raw duplicate.
Host load was elevated: status ended at 36.11, config ran from 28.05 to 28.46. These were
not globally quiet captures. Additive matcher revisions retain all owners; hook-predicate
reanalysis added zero weight across 33 prior phases. Increased inline visibility at 534 is
not CPU-regression evidence; its 579 ms classification group included necessary work.

Baseline quick/chatty artifacts remain under
`bench/perf/results/w2-4-fmt-before-{quick,chatty}-macbook-d317e171.json`. Original runs were
noisy. Supplemental `-quiet-` replays preserve harness HEAD `83d5f420` but identify the actual saved
pre-lane `d317e171` executable through `meta.saved_binary_provenance`; quick was not noisy, chatty
was noisy despite the filename. Their binary SHA is
`3ab0861c73db60d18e08298f44929a380c3257a20ba12dff406d822db22a00e5`.
Stripped baseline profile shares are N/A, not 0%. Original lane CPU values are keys p1/p20
1.056/0.996ms; panes/windows/sessions list-all s20 0.1529/0.111/0.1338ms; config1000 2.5126ms;
visible chatty 4.9978%. Current scoped measurements are:

| Metric | Original before zz | After zz | Same-run tmux |
|---|---:|---:|---:|
| list-keys p1 / p20 CPU, ms | 1.056 / 0.996 | 0.1337 / 0.1230 | 2.6732 / 2.7514 |
| panes-all / windows-all / sessions s20 CPU, ms | 0.1529 / 0.1110 / 0.1338 | 0.0881 / 0.0746 / 0.0842 | 0.3224 / 0.2307 / 1.8361 |
| config1000 CPU, ms | 2.5126 | 2.3513 | 9.3689 |
| config1000 instructions, millions | 39.5848 | 38.6508 | 198.545 |
| config1000 wall, ms | 4.7133 | 4.9115 | 13.0238 |
| visible chatty daemon CPU, % | 4.9978 | 3.4317 | 2.2967 |
| visible chatty instructions, millions/s | 375.0454 | 390.5270 | 192.8621 |
| visible chatty terminal output, KiB/s | 377.3640 | 393.9418 | 339.3428 |

Scoped quick exited 0: 32 pass, 40 info, zero failures/errors/warnings/regressions/drift.
Full chatty exited 1: 7 pass, 1 fail, 4 info; visible client CPU was 1.4097% against a 1% rule.
The observed daemon CPU improvement is not a causal estimate. Instructions/KiB were nearly
unchanged (993,856 before, 991,332 after); throughput increased. Both scoped runs were noisy:
quick load 25.66 to 20.72, chatty 13.02 to 7.8. Supplemental baselines stay separate.
Portable current inputs:
`bench/perf/results/w2-4-fmt-{quick,chatty}-macbook-4c5b0b39.json`; comparator exit 0:
`/tmp/zzpc/fmt-4c5b0b39-metric-comparison.{json,md}`.

The required all-12-group full strict saved-old baseline exited 0: 149 metrics, 3 pass and
146 info, no failures/errors/warnings/regressions/drift, 374.2s, `meta.noisy=false`, load 5.19
to 6.16. Its portable file is
`bench/perf/results/w2-4-fmt-before-full-strict-macbook-d317e171.json`. Captured harness HEAD
remains 4c5b; only `meta.saved_binary_provenance` identifies the saved d317 executable.
The intentional old-binary-mtime warning remains in metadata. Provenance receipt:
`/tmp/zzpc/fmt-4c5b0b39-full-pair-provenance.json`. Current full strict exited 1: 63 pass, 11 fail, 75 info, 0 errors, 2 regressed, 0 drifted,
369.6s. Raw input: `/tmp/zzpc/fmt-4c5b0b39-full-strict.json`. Final paired ownership/quiet
metadata assessment is PENDING. Attach instructions rose from 10.4492 to 14.2440 million
at p1 (+36.3%) and 12.3259 to 16.0091 million at p4 (+29.9%). Their mechanism/ownership is
resolved by the attach follow-up below: connection threads repeated syntax compilation.
The original full strict result remains historical; the follow-up uses scoped quick runs.
Both full inputs report `meta.noisy=false`. Separate full-pair owned values are config CPU
2.5655 to 2.3598ms, instructions 39.2139 to 38.4788 million, wall 5.6306 to 4.5984ms; visible
daemon CPU 3.1407 to 2.8369%; status-job CPU 0.1873 to 0.1801%, instructions 13.8277 to
7.0966 million/s. These full values are not mixed with the earlier scoped/noisy results.

Meaningful limitations and handoff:

- Source-only syntax keys and conservative native-only E/T portion proof are deliberate.
  Arbitrary user/data indirection keeps full fresh formatting. Allocation bounds are logical,
  not process RSS; no client/protocol behavior is traded for them.
- The first 020 status capture exited 1 while saving after its 180-second recording limit and
  225-second harness deadline. Its hash-verified archive stays N/A. A 360-second completion
  allowance kept recording at 180 seconds; the valid retry is the failed-floor row above.
  Evidence: `/tmp/zzpc/fmt-after-020491b2-status-infrastructure-failure.json`.
- The 0b full/solo agent-capture checks failed. Fixture commit `adb53fa7` waits for SessionReady
  without a fixed sleep and preserves all capture assertions. Startup-queued prompt echo loss
  remains unresolved in local/source evidence outside FMT; production incidence was not
  measured. The fixture change does not establish a production fix. Handoff:
  `/tmp/zzpc/fmt-agent-startup-reset-proof.md`.
- Current Mac compatibility batch exited 1: 44 final scenarios exactly match saved pre-lane
  tuples, FMT0, 43 clean and the unchanged `smoke/status-background-jobs` OUT1/WARN1 red.
  All 46 attempts are retained. An initial command-item failure coincided with tmux fork/
  Device-not-configured errors; archived first-pass logs remain, and automatic retry plus
  explicit current/pre-lane solos were clean (0/0). Status-jobs solos remain 1/1 with the
  same assertion signature. Coverage generator/validator exited 0:
  `/tmp/zzpc/fmt-compat-4c5b0b39-final-coverage.json`. This accepts unchanged coverage, not
  an all-green batch. The external attached-client fixture is NOT RUN.
- Linux /proc, zz-pty-gather, epoll, THP and tui-output-backpressure.sh are NOT RUN here;
  the Linux orchestrator owns them. iOS/WASM are NOT RUN; this lane changes no wire.
  Protocol remains unreleased 107. The committed 4c5b audit records 23 Rust files/545 hunks, all 53
  protected CTRL bodies unchanged, zero comments/forbidden edits/append exceptions. Audit:
  `/tmp/zzpc/fmt-source-scope-audit-4c5b0b39.json` and
  `/tmp/zzpc/fmt-4c5b0b39-final-source-audit-report.json`; bounds:
  `/tmp/zzpc/fmt-field-bound-audit.json`, key `committed_4c5b0b39_followup`.
  CTRL owns registration/publication/transport changes and can use cached option snapshots,
  borrowed fact providers and live status-context construction. The worktree stays in place.

### Attach follow-up, 2026-09-30

Measured source `5279acf7` includes `5e9996d4`. Bisect repeats put the attach regression
in the compiled-template path: `824f7c03` 15.0/17.3, `81b1ebef` 13.0/15.2 and
`3475ce1e` 12.0/14.2 Minstr at p1/p4. The 824f measurement needed a temporary trait-signature
correction, removed after that run. Connection threads repeated syntax compilation.
`formats/compiled.rs` `get` now shares the bounded syntax FIFO between threads, parses misses
outside its mutex and rechecks insertion. Bounds and cache-off interpretation stay unchanged.
`formats.rs` `FormatUniverseRef::detach` now reuses the owned loop records when borrowed
formats are disabled. The new tests prove cross-thread syntax reuse and frozen rollback loops.

Three alternating saved `d317e171` / final-binary quick attach pairs:

| Pair | Base p1 | Final p1 | Change | Base p4 | Final p4 | Change |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 10.5351 | 10.6379 | +0.98% | 12.4930 | 12.6177 | +1.00% |
| 2 | 10.6654 | 10.7960 | +1.22% | 12.6701 | 12.8305 | +1.27% |
| 3 | 10.5423 | 10.6403 | +0.93% | 12.6508 | 12.7301 | +0.63% |

All values are Minstr. The supplied 4c5b repeat was 14.3362/16.2205. All-four rollback now
measures 10.4007/12.2589 against the supplied base 10.4432/12.3313. Scratch inputs are
`/tmp/zzpc/fmt-focus-final-{base,lane}-{1,2,3}.json` and `fmt-focus-rollback-owned.json`.

Fresh saved 4c5b / final owned-group runs both exit 0, with 32 passing gates:

| Instructions | Saved 4c5b | Final |
|---|---:|---:|
| CLI list-keys p1 / p20, Minstr | 0.7468 / 0.7556 | 0.7459 / 0.7514 |
| Config source1000, Minstr | 38.6450 | 38.7055 |
| Chatty flip / hidden, Minstr/s | 67.1377 / 83.9632 | 67.2269 / 79.4812 |
| Status jobs, Minstr/s | 6.9030 | 6.8969 |

The instruction gains remain within repeat variation. Raw CPU varies on this loaded host;
config CPU is 2.4873/2.5105ms, and chatty flip's same-run tmux ratio is 0.886/0.887.
Inputs: `/tmp/zzpc/fmt-focus-final-{reference,owned}.json`. The required combined quick gate
against quick W0 exits 1: 36 pass, 5 fail, 1 wall warning, no regression/drift/error, input
`/tmp/zzpc/fmt-focus-final-verified.json`. Four connection/byte failures and the intermittent
p4 CPU failure also occur on the base. The full wave-2 adbc5407 file remains a separate view.

Final checks: fmt and mux all-target/all-feature clippy exit 0; the full mux suite passes
651 unit tests plus integrations, the all-four format sweep passes 152 plus 4, and the normal
daemon format sweep passes 149 plus 6, all under timeout 1800 and exit 0. At the shared-cache
checkpoint, 36 at-risk compat scenarios have FMT0, 35 clean and the known Mac status-job
OUT1/WARN1 (exit 1), also
reproduced on the saved base. The client-loop first-pass tty-order failure passes its solo
retry. Four all-four rollback format/option-loop compat scenarios exit 0. Four all-four daemon
preparation metadata assertions fail identically at starting `b0c7a22f` and remain handed on.
Linux /proc, zz-pty-gather, epoll, THP and tui-output-backpressure.sh are NOT RUN on this Mac;
iOS/WASM and the external attached-client fixture are NOT RUN. Protocol stays at 107.

## W4-ROWS: bulk row extraction from libghostty (effort M)

Reinstated under the no-compromise rule. Frame build reads cells through per-cell calls into
libghostty-vt.

Scope: first profile `throughput.attached.*`, `chatty.cpu_pct.visible` and
`chatty.client_cpu_pct.visible` after W4-DELIVER. If row extraction shows in any of them, add a
fork API that copies a row range into packed cells in one call (styles and graphemes as dictionary
indexes), carried in the Ghostty fork pinned in
`third_party/rust/libghostty-vt-sys/build.rs` and the libghostty-rs fork (`scripts/forks.conf`,
`just forks`). If it does not show, record the profile in this doc and close the lane.

Gate: attached throughput and visible chatty CPU improve on the W4-DELIVER merge; fork patch
listed in forks.conf with a rebase note.

## W4-BINARY: daemon-only executable (effort M)

Reinstated under the no-compromise rule. The daemon runs from the multi-role `zz_cli`
(CLI, TUI, daemon, libghostty, russh, ureq), so about 9.6 MB of RSS is file-backed pages of code
the daemon never runs.

Scope: a `zz-daemon` binary target holding only the daemon, mux and terminal engine. `zz_cli`
execs it for `zz daemon` and for cold start (fall back to the in-process daemon when it is
missing, for dev runs); the in-pane `tmux` wrapper keeps pointing at the CLI clone. Package it in
`cargo xtask` bundles, `install.sh`, the cask and AUR/deb/pacman recipes, and the remote ssh start
script (`endpoint.rs` daemon start). Measure `mem.rss.*` and `mem.footprint.*` and dyld image
count before and after; strip unused features (TLS, http, russh client) from the daemon graph.

Gate `--stage final`: `mem.rss.p1` <= 2x tmux (informational metric promoted to gated for this
lane); cold start not slower; remote start works against an ssh host in the fleet tests.

# Rollback switches

One environment knob per behaviour change, read once at daemon start (the CLI one at CLI start),
logged at startup, so a parity regression can be bisected on a live build without a revert. Each
knob is a `static LazyLock<bool>` next to the code it guards. Check `spawn_daemon` passes
`ZZ_PERF_*` through to the daemon. Knobs from wave N are deleted when wave N+2 starts (W3-LOOP
deletes most wave-1 fallback paths anyway).

| Knob | Lane | Set, restores |
|---|---|---|
| `ZZ_PERF_LEGACY_COMMAND=1` | EXEC | CLI uses ClientHello + PrepareCommandList + CommandRequest (daemon keeps that path through W2) |
| `ZZ_PERF_CONNECTION_THREADS=0` | EXEC | a new thread per connection, none kept idle for reuse |
| `ZZ_PERF_EAGER_FRAMES=1` | PANE | frames for every attached view plus the no-view fallback |
| `ZZ_PERF_NO_COMPRESS=1` | PANE | no idle history compression |
| `ZZ_PERF_ECHO_FASTPATH=0` | PANE | always wait `CONTENT_PUBLISH_STALENESS` |
| `ZZ_PERF_EAGER_PUBLISH=1` | PUBLISH | runtime-fact and title changes publish synchronously; no subscriber early returns (key tables included); every Snapshot is sent even when a client already has it |
| `ZZ_PERF_RENAME_THROTTLE=0` | PUBLISH | no 500 ms automatic-rename throttle |
| `ZZ_PERF_PEER_SCAN=always` | PUBLISH | 1 Hz Claude peer scan as today, reading every record each tick; the status sampler ticks with no client |
| `ZZ_PERF_EAGER_UNIVERSE=1` | FORMAT | full universe per expansion (also the differential oracle) |
| `ZZ_PERF_COMPILED_FORMATS=0` | FMT | parse and evaluate templates through the interpreter |
| `ZZ_PERF_BORROWED_FORMATS=0` | FMT | build the full owned table values for contexts and loop items |
| `ZZ_PERF_BORROWED_FACTS=0` | FMT | build owned command facts before engine execution |
| `ZZ_PERF_FORMAT_CACHE=0` | FMT | rebuild compiled templates, option snapshots, needs, reference closures and unions, detached contexts, default key listings, status parameters, prepared requests, border presentations and completed status output |
| `ZZ_PERF_ATTACH_DEDUP=0` | ATTACH | resync and Full enqueue as today: an attach resends the Snapshot and every overlay, frames are not held until `Attached`, no update is dropped for a generation already queued or written, and the publish after a detach renders the detaching client's status |
| `ZZ_PERF_ATTACH_BATCH=0` | ATTACH | an attach holds only its terminal frames until `Attached`; `Attached`, the status and the other reliable messages are written as they are queued instead of as one batch after the publish that follows the attach, and the hello of a raw-terminal or browser client carries a status rendered before it attached |
| `ZZ_PERF_ATTACH_PRESIZE=0` | ATTACH | an attaching raw-terminal client's panes keep their size until its first `ResizeTerminal` |
| `ZZ_PERF_WRITEV=0` | ATTACH | one write per outbound frame, a writer thread of its own per connection, the default socket send buffer, and unbuffered protocol reads in the daemon and in the client (the client reads it at start) |
| `ZZ_PERF_TUI_COALESCE=0` | ATTACH | the TUI (read at CLI start) disables event-paint coalescing and retained-row and narrow-column skips, repaints on every snapshot and unchanged resize, paints before draining queued events and draws a waiting card in a pane with no frame, writes cursor-only paints, clears to the theme colour and erases blank pane rows, performs uncached border/status work, sends the initial Kitty graphics probe regardless of terminal-support detection, reports client grid size for a cell-size reply, and uses absolute cursor positioning for successive vertical border cells. Terminal options come from Hello and attachment uses the same single connection |
| `ZZ_PERF_ROW_PATCHES=1` | TERM | patches replace every changed row whole (from column 0, the rest of the row cleared), the pre-W2 row granularity; the frames stay PaneFrames. Fails `echo.wire_bytes.idle` (about 80 B against the 64 B rule) by design: a key echo resends its whole prompt row |
| `ZZ_PERF_READONLY_SKIP=0` | HOOKS, CTRL | read-only commands take the before/after captures and perform key-table publication checks and control tap refresh |
| `ZZ_PERF_EAGER_FACTS=1` | HOOKS, CTRL | every command builds format hook facts, literal display commands resolve format targets before execution, and a pane runtime fact change builds facts with no rename due |
| `ZZ_PERF_HOOK_JOURNAL=0` | HOOKS | hook events come from whole-mux snapshots before and after, the command path captures every active window, active pane and bell, and the focus probe captures every window and session |
| `ZZ_PERF_KEY_TABLE_DELTA=0` | HOOKS | Full subscribers receive every key table instead of the changed and removed tables; Hash subscribers retain their compact revision and mouse bindings |
| `ZZ_PERF_TREE_DELTA=0` | CTRL | compact subscribers receive full scoped trees for changes instead of TreeDelta; Hello, Welcome and Batch stay on the new wire |
| `ZZ_PERF_COPY_CLONE=1` | COPY | flat `ModeRevision` clone |
| `ZZ_PERF_THP=1` | FOOTPRINT (Linux) | the daemon keeps transparent huge pages as the system sets them |

Wire changes (W2-TERM, W2-CTRL) and the thread model (W3, W4) require a revert for rollback.
`ZZ_PERF_TREE_DELTA=0` changes tree publication within the new wire only. The 8-byte-a-cell frames cannot come back behind a knob: both ends speak one format. `ZZ_PTY_SHARDS=N` is a tuning knob, not a rollback. W2-HOOKS changes that keep behaviour
have no knob either: the catalogue name index, the skipped lookup for empty hook arrays, the sweep
that runs only after a removal (debug builds assert a skipped one would remove nothing), the
focus early return and the shared change window (debug builds diff the focus candidates against
whole-state probes), and the blocking run-shell wait.

# Tests and fixtures to add

| Test | Lane | Asserts |
|---|---|---|
| Runtime-fact format rows (tmux differential) | PUBLISH | `window-status-format '#I:#{b:pane_current_path}'`, `pane-border-format '#{pane_current_command}'`, a `status-format` using both: labels update after `cd` and exec, attached and detached |
| Cold start + run-shell child CLI | EXEC | two concurrent cold CLI commands with a config whose `run-shell` child calls the CLI finish; same for TPM-style `source-file` and an `after-select-pane` run-shell hook |
| Repeated Exec, `expect_server_id`, zero-command probe | EXEC | settings file path runs two commands on one client; mismatch returns an error before running |
| Copy mode past history-limit, resize in copy mode (tmux differential) | COPY | content under the cursor matches tmux |
| Styled wide lines fill history-limit | PANE | `#{history_size}` equals history-limit |
| Symlinked binary name | FOOTPRINT | `claude` -> `versions/X.Y.Z`: `#{pane_current_command}` is `claude`, `agent_state` updates; macOS and Linux |
| Detached window-style appearance | PUBLISH | `set -w window-style bg=red` and select-pane with `window-active-style` in a detached session reach the terminal (OSC 11 reply, `capture-pane -e`) |
| Control-mode ordering | HOOKS, DELIVER | `%begin`/`%end` with deferred notifications; final `%output` before `%exit` / `%window-close`; `%layout-change` |
| Attach dedup | ATTACH | switch-client away and back with no output, re-attach, RequestFull after a bad patch: every visible pane gets a Full, none twice at one generation (`daemon/attach_tests.rs`) |
| Kitty ordering under batched writes | ATTACH | image chunks precede the placing frame in one `writev`; a short write resumes mid-batch (`daemon/attach_tests.rs`) |
| First frame at the final size | ATTACH | a client that named its size and cell gets one Full per pane, at the laid-out size, and none before `Attached` (`daemon/attach_tests.rs`) |
| Timer stall | PUBLISH | alert-silence hook `run-shell 'sleep 2'` while a repeat-time key table expires on time |
| In-place peer status | PUBLISH | rewriting `<pid>.json` in place changes `agent_state` |
| Lazy universe at escape points | FORMAT | lazy and eager give equal output for status, border, mode, chooser, control subscription, format monitor, hook |
| Borrowed and compiled formats | FMT | every pinned name and modifier agrees with the W1 interpreter over attached, detached, marked, zoomed, dead and null contexts; nested loops and indirect references agree after selective detach; option changes invalidate dependency and snapshot caches |
| terminal-overrides change | FORMAT | `set -as terminal-overrides` changes the feature mask without reconnect |
| Early prompt history | EXEC | `command-prompt` right after cold start sees history; a new entry survives |
| Immediate child exit | PANE | `split-window 'true'` reports its status once |
| Chooser preview freshness | PANE | choose-tree preview of a hidden printing pane shows current content |
| Journal completeness | HOOKS | debug builds diff the journal against whole-mux snapshots at every hook scope (events, focus candidates, active windows, panes and bells) over the whole daemon suite and every debug compat run; after-list-keys fires (`daemon/hook_events_tests.rs`, zz-mux `journal_tests.rs`) |
| Key table deltas | HOOKS | a subscriber's tables, folded from its hello and every publication, equal a fresh snapshot after binds, unbinds, `unbind -a`, prefix changes and a revert made while nobody listened; an identical rebind publishes nothing (`daemon/hook_events_tests.rs`, zz-protocol `key/generation_tests.rs`, zz-client `core.rs`) |
| Blocking work holds no journal | HOOKS | a `run-shell 'sleep 1'` key binding keeps the journal at a few entries while other commands run; a run-shell job thread wakes fewer than 10 times over 0.6 s (Linux, `daemon/hook_events_tests.rs`) |
| PaneFrame codec | TERM | random grids (ASCII, wide pairs, graphemes, hyperlink and classed styles, spacer heads, odd flags, repeats) round-trip through full frames and apply through patches with dictionary growth and scrolls; every truncation is rejected, and a lying count of styles, graphemes, overlays or placements before allocating (the grid is bounded by the 8 Mi cell cap); echo patch <= 40 B, blank 180x50 frame <= 64 B (`zz-protocol` `pane_frame_tests.rs`) |
| Frames through the daemon | TERM | a typed key reaches an attached client as a patch of at most 64 B and the retained grid equals the daemon's frame; history chunks ride the terminal lane; a pane's frame sequence keeps growing across respawn-pane; two attached clients rebuild every view exactly from the streamed frames (`daemon/pane_frame_tests.rs`) |
| Echo under load | W3 | `capture-pane -S -` on a 20-pane large history while another pane echoes: p99 no worse than tmux |

# Lane mechanics

- Each lane in its own worktree: `git worktree add ~/dev/zz-<lane> -b perf/<lane> <wave base>`,
  then an APFS clone of a warm target: `/bin/cp -c -R ~/dev/zz/target ~/dev/zz-<lane>/target`.
  Never the harness `isolation=worktree` (fails on the `.claude` symlink). Never `git stash`,
  reset or checkout in a shared tree.
- New daemon tests go in a per-lane `#[cfg(test)]` module file under
  `crates/zz-daemon/src/daemon/` declared next to the lane's functions, never appended to the end
  of daemon.rs (production ends near line 44.7k, `mod tests` holds the remaining ~65k lines).
  Helpers needed from `mod tests` get `pub(super)` in place.
- New struct fields are appended, never reordered. Find code by function name.
- Develop with `just perf-gate` `--quick`; run the full gate for the wave stage before merging,
  on a quiet machine for wall metrics. `compat/.cache` is not in a fresh worktree: run
  `compat/fetch-tmux.sh` there for compat scripts (the gate uses the release tmux).
- A failing zz-daemon test under the full workspace is re-run alone before diagnosis.
- Merge JSON goes to `bench/perf/results/w<wave>-<n>-<lane>-<host>-<sha8>.json`; the next lane's
  `--baseline` points at it. W0 stays `results/baseline-<host>-17e17115.json` for the whole
  campaign.

# Risks

| Risk | Guard |
|---|---|
| Exec parity: attach mid-chain, nested attach refusal, kill-server recovery, cold-start abort, stdin via ClientFileRequest | `ExecResume` mirrors `PreparedCommandList`; compat CLI scripts; provenance tests |
| Same-version 107 builds disagree on bytes during the campaign | kill-server dev daemons after wire merges; legacy retry on first-frame failure; no tags |
| Deferred publish and the rename throttle change event timing | timing tests updated; hook order checked against pinned tmux |
| PANE + ATTACH leave a pane blank at attach | pending-only dedup; wave-1 exit checks |
| LZ4 page restore latency unmeasured | PANE times one restore; re-arm after full scans |
| `#{pane_current_command}` diverges from tmux (exec path vs `pbsi_comm`) | deliberate, recorded in the ledger; agent detection depends on it |
| CoreFoundation stays linked through feature unification | gate `otool -L` on the `dist/` binary; no wall claim |
| Wave 2 moves every interactive client at once | `PackedCell` planes and `MuxSnapshot` stay the client model; web and iOS builds in every wire gate |
| Compact rows or shared shards cost throughput | throughput and headless-client throughput are hard gates at every merge; spin bridge bounded |
| A journal miss silently drops a hook | generation-implies-journal debug assert over the whole suite |
| W3-LOOP size (~44.7k production lines, 447 lock sites, 16 park points) | green commits (e0) to (e); Windows pipes via adapter threads |
| Non-zero mimalloc purge delay raises idle footprint | chosen by the memory gate |
| Wall-time noise from parallel agents | wall misses fail on a quiet host or with `--strict`; CPU time held through instruction counts; throughput held by the in-run tmux ratio and a wide W0 floor |
| The gate compares zz with itself (a zz pane puts the zz tmux wrapper first on PATH) | `run.py` prefers Homebrew and system paths, resolves symlinks, refuses scripts, the zz binary and `-V` with `-zz` |

# Rejected

| Idea | Why |
|---|---|
| Slimming the hello for command clients | Exec sends no hello |
| Folding the hello into a command response | a second command wire format; Exec covers it |
| socketpair / listener-fd handoff on cold start | a readiness pipe gives the same latency with less code |
| Separate config batch-replay mode | the per-line costs are removed at their source; a second mode duplicates hook semantics |
| `posix_spawn` of the pane program itself | on XNU nothing in it makes the slave the controlling tty (W1-PANE tried it: zsh, dash and exec'd programs got none), so macOS spawns the daemon binary as a launcher that claims the tty and execs; on Linux glibc's `addclosefrom_np` is newer than the headless binary's floor and fork is fine |
| Daemon-owned layout | GUI pixel gaps unresolved; sizing before the first frame removes the TUI round trip |
| Version preamble and slimmer envelope | ~3 B per frame does not justify changing zz-web framing |
| Shrinking the GUI HistoryRing | outside the daemon; revisit after W2-TERM |
| A 2-thread Exec executor pool in wave 1 | deadlocks on startup and on config/hook run-shell children; one thread per connection until W3 |
| `pbsi_comm` for the process name | reads the version string for symlinked `claude` |
| Duplicate-Full drop against delivered frames | clients wipe viewports on attach and bad patches; panes go blank |
| Per-lane protocol bumps (107 -> 110) | each released bump strands panes, remote hosts, TestFlight and web assets |
| RSS as the memory gate | file-backed binary pages put 6 / 10 MB below the floor |
| TreeDelta for control-mode `%layout-change` | bypasses the hook-driven `%end` ordering |

Reinstated on 2026-09-28 under the no-compromise rule, and no longer rejected: the borrow-based
format resolver with a template cache (W2-FMT), bulk row extraction (W4-ROWS), and a daemon-only
binary for RSS (W4-BINARY). W2-FMT also caches option snapshots behind option and mux-tree
generations, with invalidation tests for the mutation paths.

# Open questions

- Remote ssh version skew after the campaign: what the GUI shows when a host's zz speaks an older
  version, and whether it offers an update.
- Local TUI server-side render fast path (TUI passes its tty fd, daemon writes escapes) if
  `chatty.client_cpu_pct.visible` or echo latency miss after W2; cannot work over ssh.
- Shard count and fairness: panes per shard before latency regresses (bench with 1, 4, 16 floods).
- Can system libmalloc replace mimalloc after W3-SHARDS without losing the 245 MB/s?
- Which zz hooks (`@option-changed`, wait-for signals) must still flush mid-file during config
  load, once publication is deferred?
- Attached footprint with a GUI-like client awaits its W0 TODO group. Headless throughput has
  an informational frame-sink row but still needs a reference baseline. Copy-mode entry has
  footprint and time gates from W2-COPY.


### W3-LOOP b6fix as built (2026-10-01)

- Client timers now follow status option effects, monitor presence, attachment changes, and control subscriptions. Unattached subscription changes and unregister after detach no longer send redundant timer messages. The mux loop skips timer dispatch while no input, completion, or deadline needs work.
- Three alternating quick pairs against `/tmp/zzpc/w3/loop-b5fix-cli`: 18 of 21 CLI/chatty instruction medians meet 1.02x. Remaining misses: `cli.instr.has_session.p20` 1.0218x, `cli.instr.select_pane.p20` 1.0702x, `chatty.instr_per_s.hidden` 1.0553x. Thread counts stay 4 to 3 at p1 and 42 to 41 at p20. Status-job instructions fall 2.7%.
- Checks: 1382 daemon unit tests and 35 integration tests pass, one ignored; 17 timer tests pass after the clippy fix; fmt, clippy, and all four requested compat scenarios pass. The final quick W0 gate has 37 pass, 1 fail, and 42 info rows, with no harness errors or regressions. The failure is the pre-existing status-job thread rate near 3/s against a 0.5/s limit.
- Handed on: the three instruction misses above; idle wakeups remain unmeasured because the permitted commands omit `idle`. Linux `/proc`, PTY gather, epoll, THP, and `tui-output-backpressure.sh` checks did not run on this Mac. Scratch results stay in `/tmp/zzpc/b6fix-{b5fix,b6fix}-{1,2,3}.json` and `/tmp/zzpc/loop-b6fix.json`. This step does not meet its done criterion.

## W3-SHARDS TUI attach follow-up, 2026-10-01

As built: `crates/zz-tui/src/app/event_loop.rs` `EventLoop::receive` reads tty keys,
daemon frames and signal notifications on the attach thread. `TerminalWriter::flush`
uses nonblocking tty writes, partial-write offsets and the existing 100 ms output
recovery. The CLI already calls the TUI on its calling thread. Linux thread snapshots
show seven attach threads on the base and one, `zz_cli`, on this build.

Five alternating untraced runs against `w3/base-cli` gave idle p50 medians
1.9660 -> 1.7255 ms and busy p50 2.0500 -> 1.7925 ms, with no missed echoes.
The 240.5 us idle reduction misses the 250 us target by 9.5 us; busy improves
257.5 us. tmux idle medians were 0.8935 -> 0.9300 ms and busy 0.9115 -> 0.9180 ms.
The quick chatty/attach comparison gave attach instruction ratios 1.0034x for
both sizes. Hidden-client CPU was 0.0000 -> 0.0080%, so the literal 1.05x bound
against zero does not pass. These limits keep the TUI step open. The base already
fails attach time and daemon CPU rows for both sizes. Mac checks stay with the
orchestrator; Linux validation follows below.

Linux validation: clippy passed and 524 crate tests passed. The attached-client
fixture passed, screen-diff matched 147 asserted checkpoints, output backpressure
passed nine assertions, and all four requested smoke scenarios had zero divergences.
The final quick W0 gate had 14 passes, five failures and zero regressions: the four
echo timing rows and `attach.cpu.p4` remain red, as on the base. No extra perf runs
followed the required series. Mac runtime checks, iOS and Windows builds were not run.


### W3-LOOP e01 instruction fix (2026-10-02)

Remove two write-only `ExecutionContext` snapshots in `Shared::execute_command_request_with_streams_in_item` and `Shared::execute_with_mux_source_routed_for_terminal_in_queue_in_item`, plus their item field. Keep context in `LoopExec` or the command worker. Keep item and queue ids, continuation tokens, scope isolation, cached results and duplicate completion guards. Move the owner's context when adding future parked continuations instead of copying it on each command.

The instruction profiles put context cloning at 0.91% before and 0.60% after, and `memcpy` at 10.17% before and 9.47% after. Three alternating quick runs against the 1518f03d loop-00 binary pass 18/19 CLI instruction medians: chain5 p1/p20 ratios are 1.0131/1.0164; select-pane p20 is 1.02046, above the 1.02 limit. Chatty hidden is 51.0584 Minstr/s against B's maximum 52.8874; flip is 32.9205 against 32.9090 and misses by 0.035%. The sixth run also flags send-keys p20 wall time, outside this step's instruction criterion. The instruction gate remains open; hand on the select-pane and flip misses with the six scratch results under `/tmp/zzpc/e01fix-{B,C}-{1,2,3}.json`.

Validation: formatting and daemon clippy with all targets/features and warnings denied pass. The serial daemon unit suite passes 1393/1395; `status_job_output_reaches_clients_without_a_periodic_deadline` passes alone, and `remote_scripts_fall_back_to_the_mac_app_bundle_cli` also fails on 71d59edb. The plain-input worker test passes. The remaining integration targets pass 35 tests with one ignored soak; doc tests contain no cases. All six selected compat scenarios pass 219 steps with zero divergences. Mac, iOS, Windows, full-workspace and full-compat checks remain for orchestration.

## W3-LOOP e16 Linux fix, as built (2026-10-02)

Linux resumes bounded Control tap delivery after the loop writer frees client output capacity.
Linux copy-pipe jobs retain the child's exit status when stdin closes early; the loop still reaps
and releases the permit. The pipe thread test waits for terminal startup, including Linux's
`zz-pty-gather`, before sampling. Four-chunk backpressure and the macOS production paths stay intact.
The three named tests passed 10 times each with gather and direct reads. Daemon checks passed
1,444 library and 35 integration tests after excluding the known Mac bundle lookup test;
terminal checks passed 364 tests. Clippy, formatting, 99 compat steps, and nine TUI backpressure
assertions passed. An initial mode-keys timeout passed alone and in the serial recheck.
Diagnostics against `loop-pre-e16fix-cli`: the base stalled the 16 MiB Control transfer for 120 s;
the fix delivered 126.8 to 134.6 MB/s. Control instructions stayed at 0.0469 M/cmd, and threads
stayed at 3/25 for one/twenty panes. Detached throughput was 122.8 to 124.5 MB/s against the
base's 130.2; hidden chatty instructions varied from 0.914x to 1.043x the base. The final three
alternating pairs run after this commit and decide those strict bounds. The tmux-relative
Control burst gate was already red on the base; latency also varied. Mac checks go to the orchestrator.
