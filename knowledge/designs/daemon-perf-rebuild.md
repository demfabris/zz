---
type: Design Plan
title: Daemon performance rebuild
description: "The campaign to bring the zz daemon to tmux cost per command, per pane and per attach while keeping the 5x output throughput lead - a permanent zz-vs-tmux gate first, then waves that remove unrequested work (one-frame Exec commands, change-driven publication, lazy formats, frames only for watchers, a compact wire under one unreleased protocol version), then one mux loop and PTY shards; the lane brief source with targets, merge order, write zones, gates and rollback switches."
status: Approved 2026-09-28; wave 0 (gate and this plan) built; release freeze until W4 exits; wave 1 in progress on perf/wave1 (FOOTPRINT, FORMAT, PUBLISH and PANE merged)
resource: crates/zz-daemon/src/daemon.rs
tags:
- performance
- daemon
- protocol
- tmux
- benchmark
- campaign
- design-plan
timestamp: 2026-09-28T15:56:52Z
---

# Outcome

The daemon does per event only the work someone asked for, at tmux cost, and keeps the parallel
libghostty parse, 64 KiB reads, time-sampled frames and latest-wins mailbox that give zz its 5x
throughput lead. Every claim is a number from `bench/perf` measured against a release tmux in the
same run.

Today: every event does all the work for everyone, eagerly, under one global `Mutex`, on a thread
per thing. Profiles put 80-95% of daemon CPU on each hot path in work nobody requested.

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

Memory is gated on footprint (macOS `ri_phys_footprint`, Linux `smaps_rollup` `Pss_Anon`); RSS is
reported, never gated, because ~9.6 MB of it is file-backed pages of the multi-role `zz_cli`
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
| `echo.p50.idle` | ms | 0.0728 | 0.262 |  | <= 1.5x |  |  |  |
| `echo.p99.idle` | ms | 0.158 | 0.489 |  | <= 1.5x |  |  |  |
| `echo.p50.busy30` | ms | 0.0715 | 6.38 |  | <= 1.5x |  |  |  |
| `echo.p99.busy30` | ms | 0.162 | 18.5 |  | <= 1.5x |  |  |  |
| `echo.wire_bytes.idle` | B | - | 1133 |  | <= 1700 B | <= 64 B |  |  |
| `throughput.detached.ascii` | MB/s | 49.2 | 244.9 | >= 4x, >= 0.85x W0 |  |  |  |  |
| `throughput.detached.unicode` | MB/s | 9.9 | 103.4 | >= 4x, >= 0.85x W0 |  |  |  |  |
| `throughput.attached.ascii_ms` | ms | 3821 | 630.7 | <= 0.25x, <= 1.18x W0 |  |  |  |  |
| `control.latency` | ms | 0.0201 | 0.549 |  |  | <= 1.2x |  | <= 1.1x |
| `control.cpu_per_cmd` | ms | 0.0141 | 0.536 |  |  | <= 1.5x or <= tmux + 0.1 ms |  | <= 1.2x or <= tmux + 0.03 ms |
| `control.burst_cmds_per_s` | cmd/s | 183136 | 1863 |  |  | >= 0.8x |  | >= 0.9x |
| `control.output_mbps` | MB/s | 40.3 | 111.6 |  | >= 0.85x W0 |  |  | >= 1x |
| `statusjob.cpu_pct` | % | 0.232 | 0.531 |  |  |  | <= 1.5x + 0.1% |  |
| `statusjob.threads_per_s` | 1/s | 0 | 3 |  |  |  | <= 0.5/s |  |

Notes on the rules:

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
- Not gated yet, waiting for the W0 TODO groups: copy-mode entry memory (<= 1 MB, <= 5 ms after
  W2-COPY), attached footprint with a GUI-like client, headless-client throughput (>= 0.85x
  W0), `remote.*` with injected RTT and `agent.*`.

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
  EOF, the CLI retries once over the legacy hello path (kept behind `ZZ_PERF_LEGACY_COMMAND`
  anyway). A released 106 daemon rejects at the envelope check with
  `CommandResponse::Error(ProtocolMismatch)`, which `classify_local_connect_error` already turns
  into today's mismatch prompt.
- **No release tags mid-campaign.** A tag freezes 107; the next wire change must then bump to 108
  and strands the in-pane `tmux` wrapper, remote ssh hosts, TestFlight builds and the web assets
  embedded in zz-web.
- **Release freeze until W4 exits** (owner, 2026-09-28: zz is not fully launched, nothing ships
  mid-campaign). No tags, no beta channel pushes, no TestFlight uploads until `--stage final`
  passes. Any wire change in any wave may land in 107, including non-append rewrites.
- **No compromises** (owner, 2026-09-28). A lane may not stop at "meets the target" when the
  profile still shows avoidable work on its path; targets are floors. The three deferrals the
  architect rejected as good enough are reinstated as lanes W2-FMT, W4-ROWS and W4-BINARY.
- **Pin the in-pane `tmux` wrapper to the daemon's own executable.** `install_tmux_shim` pointed
  the wrapper at `current_exe()` (or `ZZ_TMUX_EXECUTABLE`), a path an app swap replaces, so panes
  of an old daemon ran a newer CLI. Built in W1-EXEC: the wrapper runs `/proc/<daemon pid>/exe`
  on Linux (the running image, no copy) and a `clonefile(2)` clone beside the wrapper on macOS
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
  CPU metric; none on Linux), footprint (`ri_phys_footprint`; Linux `Pss_Anon`) and RSS as info,
  threads (`PROC_PIDTASKINFO`; `/proc` Threads), thread spawns (`PROC_PIDLISTTHREADIDS` sampled
  every ~2 ms; Linux task ids), wakeups (`ri_interrupt_wkups + ri_pkg_idle_wkups`; ctxt switches).
- `sockproxy.py`: Unix-socket relay counting bytes, u32-prefixed frames and connections (zz only;
  tmux passes the tty fd).
- Groups: `cli` (verbs at p1 and p20, list verbs at 20 sessions), `spawn`, `cold` (+ infocmp
  forks), `config`, `chatty` (steady, flip, hidden, visible with TUI CPU and tty bytes), `idle`,
  `mem` (p1, p20, tui20, scroll180, scroll80), `attach` (ttfc, total tty bytes until quiet, CPU,
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

- TODO: `crates/zz-client/examples/perf_client.rs`, a headless client on the zz-client core with
  a GUI-like mode (all-sessions tree, history backfill, 8 visible panes) and a frame-sink mode
  (decode, no render), with its `gui` group and headless-client throughput.
- TODO: copy-mode entry memory on a 10k x 180 pane; attached footprint with a GUI-like client.
- TODO: injected RTT in `sockproxy.py` and the report-only `remote` group; the report-only `agent`
  group (fixture ACP provider).
- TODO: a first run on Linux.

Gate (met at 17e17115 on macOS, see Baseline): `--stage baseline` passes; `--stage wave1` and
`--stage final` fail on today's binary; full run <= 12 min (about 7), `--quick` <= 3 min (about 2.5); the
committed W0 JSONs are the Baseline table below; `test_gate.py` covers threshold evaluation, the
noise policy, the regression rules, the tmux choice and that every gated metric has a final rule.
Open: the same run on Linux.

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
  That tmux divergence (bash, zsh and PowerShell integrations) is recorded as the open gap
  `terminal.shell-integration-prompt-title`.
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

Built on `perf/exec` (2026-09-28). Where the build departs from the scope above:

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
  that `execute_prepared_streams` served). No `HAS_TTY`/`READ_ONLY` flag and no `TERM` field:
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
  removal with the sanitize check) instead of four times; the engine's own locks are W3-LOOP's. A
  zero-command Exec is answered on the spot, before the startup wait and without registering a
  client, so it neither owns a cold daemon's bootstrap lease nor waits out a slow config.
- One write: when nothing parked, the connection thread drains the per-Exec mailbox and writes
  every frame plus the `ExecExit` in one `write_all`. The mailbox sits in `client_writers` only
  while that Exec runs, so an idle connection never holds up the shutdown drain. The first
  `report_command_queue_park` (via a thread-local exec client) or `client_file_operation` for the
  client starts a writer thread and a reader thread on that connection (`ExecLink::go_live`), and
  later Execs on it go through them; the reader's EOF cancels the queue and unregisters, as the
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
  path (`expect_server_id`). `ZZ_PERF_LEGACY_COMMAND=1` and the once-only fallback (EOF, reset or an
  undecodable frame before the daemon answered) run the hello path with the old prepare and
  per-command loop, including the cold-start abort pseudo-command.
- Cold start: `--bootstrap-ready-fd N` after the server id; the daemon writes one byte right after
  `bind` and closes it; the CLI polls the pipe (6 s) and dials once. The backoff (0.5/1/2/5 ms) runs
  only without the pipe (Windows) or when a racing daemon won the socket. `prepare_socket` now
  holds `<socket>.lock` (flock) from the probe to `bind`, as tmux's `client_get_lock` does: two
  concurrent cold commands (the new CLI test) otherwise let one daemon unlink the other's socket
  between its `bind` and `listen`, leaving a daemon nobody could reach. The probe is a
  zero-command Exec: a daemon that answers releases the loser's CLI through the readiness pipe and
  the loser exits at once (before, it looped for 3 s and could bind after the winner was killed,
  an orphan), and one that does not answer is still waited out as a stopping daemon. Identity
  without `sync_all`; Claude peer sweep after the ready callback; `history-file` read on first use
  (`ensure_prompt_history` at every prompt reader and writer).
- `copy-mode` from a command or control client now answers once the target view publishes its
  copy facts (polled, at most 100 ms): entering copy mode is a view action the pane actor applies
  later, and without the old CLI's two extra round trips a following `display -p
  '#{pane_in_mode}'` read 0 every time (compat `smoke/copy-mode-formats`). tmux enters the mode
  before the command returns.
- The tmux wrapper is pinned (see Protocol and release policy) but still installed at startup, not
  at the first pane: a cold `new-session` spawns a pane and needs it anyway, and install plus clone
  measured 0.2 ms (clonefile 80 us, directory and script 110 us).

Measured with the quick gate (`--only cli,cold,spawn,control,config`, stage wave1) on this
macbook at load 10-37 with other lanes building, before (`613630ce`) -> after (`756498e1`), tmux
in the same run. Instructions: `cli.instr.display.p1` 10.3 -> 2.78 Minstr (tmux 0.49),
`.show_options.p1` 10.4 -> 2.80, `.has_session.p1` 9.7 -> 2.17, `.send_keys.p1` 9.77 -> 2.23,
`.select_pane.p1` 9.72 -> 2.19, `.display.p20` 28.6 -> 6.0, `.has_session.p20` 25.6 -> 3.06,
`.chain5.p1` 19.9 -> 12.0, `.chain5.p20` 43.3 -> 20.4, `spawn.instr.kill_pane` 13.8 -> 5.97,
`spawn.instr.split_shell` 36.0 -> 28.1, `spawn.instr.split_empty_P` 45.6 -> 32.8. CPU (noisy at
this load): `cli.cpu.display.p1` 1.17 -> 0.31-0.34 ms, `spawn.cpu.kill_pane` 1.9 -> 0.74-0.86 ms.
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

Checks on this branch: `cargo test` for zz-protocol, zz-mux, zz-daemon (lib and every test target),
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

Handed on: the `cli.cpu.display.*` floors (0.25 / 0.40 ms) and `spawn.cpu.split_*` wait on the
other wave-1 lanes. A `sample` of 3000 `display-message -p x` on this branch puts 49% of the
connection thread's CPU in `publish_key_tables_if_changed` (the 370-binding snapshot and its
catalog lookups, W1-PUBLISH item 1) and 23% in the format universe built for a template with no
`#` (W1-FORMAT items 2-3); p20 adds the per-pane universe rows and one `release_view` per pane on
unregister (W1-PANE item 9). W2-CTRL: control lines can reuse `Exec` as built (results are
`CommandResponse` frames, not `ExecOutput`; `ExecResumeKind` has no in-place attach upgrade yet),
and the pinned wrapper path is `TmuxShimGuard.executable`.

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
   read extended-keys and focus-events with `CommandRequest` on the existing `InteractiveClient`,
   pipelined with the has-session preflight, instead of two `CommandClient` connections.

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
  mouse bitset) is for TUI, CLI and control clients.
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
snapshot.rs TreeDelta ops; daemon.rs functions above, `handle_connection` interactive branch,
`serve_exec` upgrade hand-off; client.rs `InteractiveClient`; zz-client core; zz-tui lib.rs,
app.rs, tty.rs; zz-cli lib.rs, control_mode.rs command submission (not notification building);
crates/zz mux/client.rs, lib.rs, config/mod.rs, workspace/which_key.rs, workspace/view.rs,
workspace/new_session.rs, command/palette.rs, config/settings/multiplexer.rs; gpui-shared
connection.rs, command_palette.rs; zz-client-ffi ffi.rs, settings.rs, settings/mobile.rs;
agent/fanout.rs encode path; protocol docs.

Gate vs TERM JSON: `attach.ttfc.p1` <= 1.1x tmux; `attach.conns.p4` = 1; `attach.cpu.*` <= 1.25x
tmux; `attach.wire_s2c.p4` <= 8 KB; bind-key <= 64 B for hash subscribers; rename in an unattached
session <= 100 B per client; `control.latency` <= 1.2x, `control.burst_cmds_per_s` >= 0.8x tmux;
`chatty.client_cpu_pct.visible` <= 3x, `chatty.tty_kibps.visible` <= 1.5x; cli and throughput no
regression. Tests: zz-client simulator convergence, zz-client-ffi + C integration client, daemon
clients and overlay tests, `cargo test -p zz`, every compat/tui fixture, control-mode fixtures
(`%begin`/`%end`, `%layout-change`), `compat/run.sh`, `just web-build`, `just ios-gpui iPad build`.

Expected: attach ~10 round trips -> 1; hello 28.3 KB -> ~40 B + subscribed state;
KeyTablesChanged 26 KB -> ~20 B for hash subscribers; tree change 20-60 B, encoded once;
attach CPU 26 -> <= 5 ms.

## W2-HOOKS: read-only skip, then change journal (effort L)

Two commits, the first mergeable alone.

1. `mutates` is **zz's own predicate over arguments**, not tmux `CMD_READONLY` (tmux.h:1999 is a
   read-only-client permission flag, set on attach-session, send-keys and others that mutate).
   `capture-pane -b`, `display-message -I`/`-d` count as mutating. Read-only commands skip
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

## W2-COPY: copy mode without flat clones (effort L)

Scope: replace `ModeRevision`'s flat clone (PackedCell + semantics + search text + 12 B offsets
per cell, 128 MiB cap) while **keeping tmux clone semantics**: tmux clones the screen at entry and
on refresh (`window_copy_clone_screen` in window-copy.c), so pruning, reflow and clear-history do
not move content under the cursor. Either clone the covered pages copy-on-write (compressed pages
stay compressed) or hold a pin that blocks pruning and reflow of the pinned range while in copy
mode. Render only visible rows per frame. `HistorySearchSnapshot` becomes page-wise search over the
PageList (libghostty `src/terminal/search`) or a lazy per-page text index. The copy-mode refresh
stops recapturing. Retained dead panes keep their compressed `Terminal` instead of
`FrozenHistory` (`publish_frozen_history`, the `run_terminal` retained-pane tail). Remove the
`ModeRevisionTooLarge` ceiling.

Write zone: `session/mode_revision.rs`; session.rs `FrozenHistory`, `publish_frozen_history`,
`run_terminal` retained-pane tail, copy-mode refresh fn, `HistorySearchSnapshot` and impls,
`copy_mode_snapshot`.

Gate vs HOOKS JSON: copy-mode entry (TODO in W0) <= 1 MB and <= 5 ms on a 10k x 180 pane; throughput
unchanged. Tests: tmux differential fixtures for copy mode under continuous output past
history-limit and for resize in copy mode; `compat/tui-copy-mode.sh`, `compat/run.sh` copy-mode
rows, `tracker.py check` copy-mode obligations, zz-terminal copy-mode and search tests, daemon
copy-session tests (re-run alone; known reconcile race).

## W3-SHARDS: PTY shard threads inside zz-terminal (effort XL)

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

## W3-LOOP: single-owner mux loop (effort XL)

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

Scope: replace it with a format tree of borrowed handles (client, session, window, pane ids plus
`&MuxEngine` and the daemon facts), the way tmux `format_defaults` records pointers. Variable
lookup goes options first, then a static sorted callback table (one function per variable,
binary search or `phf`), then the per-command tree, then the environment, as `format_find` does.
Loops (`#{S:}`, `#{W:}`, `#{P:}`) push a child tree per item. Templates are parsed once into an
op list cached by (template string, options generation), so status, borders and list commands
stop re-parsing. Daemon facts are borrowed, never cloned into the context. The two escape
points W1-FORMAT found (status and mode requests) keep `detach()`, now producing only the values a compiled template references.

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
nothing tells them options changed, so the options generation this lane adds for its template
cache should key them too; the per-command `FormatHookFacts` snapshot makes every `set @x` clone
that scope's user option map.

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
| `ZZ_PERF_EAGER_PUBLISH=1` | PUBLISH | runtime-fact and title changes publish synchronously; no subscriber early returns; every Snapshot is sent even when a client already has it |
| `ZZ_PERF_RENAME_THROTTLE=0` | PUBLISH | no 500 ms automatic-rename throttle |
| `ZZ_PERF_PEER_SCAN=always` | PUBLISH | 1 Hz Claude peer scan as today, reading every record each tick; the status sampler ticks with no client |
| `ZZ_PERF_EAGER_UNIVERSE=1` | FORMAT | full universe per expansion (also the differential oracle) |
| `ZZ_PERF_ATTACH_DEDUP=0` | ATTACH | resync and Full enqueue as today |
| `ZZ_PERF_READONLY_SKIP=0` | HOOKS | read-only commands take the before/after captures |
| `ZZ_PERF_COPY_CLONE=1` | COPY | flat `ModeRevision` clone |

Wire changes (W2-TERM, W2-CTRL) and the thread model (W3, W4) have no runtime switch; rollback is a
revert. `ZZ_PTY_SHARDS=N` is a tuning knob, not a rollback.

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
| Attach dedup | ATTACH | switch-client away and back with no output, re-attach, RequestFull after a bad patch: every visible pane gets a Full |
| Kitty ordering under batched writes | ATTACH | image chunks precede the placing frame |
| Timer stall | PUBLISH | alert-silence hook `run-shell 'sleep 2'` while a repeat-time key table expires on time |
| In-place peer status | PUBLISH | rewriting `<pid>.json` in place changes `agent_state` |
| Lazy universe at escape points | FORMAT | lazy and eager give equal output for status, border, mode, chooser, control subscription, format monitor, hook |
| terminal-overrides change | FORMAT | `set -as terminal-overrides` changes the feature mask without reconnect |
| Early prompt history | EXEC | `command-prompt` right after cold start sees history; a new entry survives |
| Immediate child exit | PANE | `split-window 'true'` reports its status once |
| Chooser preview freshness | PANE | choose-tree preview of a hidden printing pane shows current content |
| Journal completeness | HOOKS | debug assert over the whole daemon suite; after-list-keys fires |
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
| Caching the option snapshot behind a generation | removing the calls plus the option index works without invalidation risk |
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
binary for RSS (W4-BINARY).

# Open questions

- Remote ssh version skew after the campaign: what the GUI shows when a host's zz speaks an older
  version, and whether it offers an update.
- Local TUI server-side render fast path (TUI passes its tty fd, daemon writes escapes) if
  `chatty.client_cpu_pct.visible` or echo latency miss after W2; cannot work over ssh.
- Shard count and fairness: panes per shard before latency regresses (bench with 1, 4, 16 floods).
- Can system libmalloc replace mimalloc after W3-SHARDS without losing the 245 MB/s?
- Which zz hooks (`@option-changed`, wait-for signals) must still flush mid-file during config
  load, once publication is deferred?
- Goals for the attached footprint with a GUI-like client, copy-mode entry memory and
  headless-client throughput wait for the W0 TODO groups that measure them.
