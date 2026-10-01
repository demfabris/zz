# Daemon perf campaign: handoff

Entry point for a fresh session continuing the zz daemon performance rebuild. Written 2026-09-29 on
the macbook, continued the same day on the Linux host alienware (see "Linux leg"). Wave 0 and wave 1
are on `main` and pushed. State on 2026-10-01: wave 2 is closed and pushed (W2-HOOKS, W2-TERM,
W2-COPY, W2-FMT, W2-CTRL and two CTRL follow-ups); its exit gates are
`wave2-macbook-3d0fc1b0.json` and `wave2-alienware-3d0fc1b0.json`. The Ghostty fork pin is
`67351380` (trim fix `c3941417`, copy snapshots `7823f65d`, used-size active page copies). No lane
in flight, no lane worktree left. Wave 3 (W3-SHARDS, W3-LOOP) starts from "Next session: wave 3";
read "Lane brief rules" before launching anything.

## Next session: wave 3

1. Started 2026-10-01: `perf/wave3` on both hosts, lanes `~/dev/zz-loop` (Mac) and
   `~/dev/zz-shards` (alienware) from `dcf05102`. Create wave and lane branches from
   `origin/main` after a fetch, never from a local `main`: alienware's clone had a stale local
   `main` (`fecaaa43`), the first SHARDS slice started on pre-wave-2 code, and its watchdog killed
   it for touching every wave-2 file.
2. Lanes and hosts: W3-SHARDS on alienware (the Linux gather fold, epoll and pidfd paths and Linux
   `bench/run.sh` live there), W3-LOOP on the Mac. Write zones do not overlap (LOOP owns daemon.rs
   production code, SHARDS owns zz-terminal session code). Merge order SHARDS, then LOOP (LOOP's
   gate is against the SHARDS JSON). Lane worktrees `~/dev/zz-shards`, `~/dev/zz-loop` from
   `perf/wave3`.
3. Slices, one commit each (see "Lane brief rules"). LOOP: e0, a, b, c, d, e as in its plan
   section. SHARDS: s1 the per-pane state machine (`PaneActor` with `on_readable`, `on_command`,
   `on_deadline`, keeping 64 KiB reads, turn caps, 16 ms frames and the echo fast path); s2 shard
   threads and wake fd owning the PTY fds (K = min(parallelism, 4), `ZZ_PTY_SHARDS`); s3 terminal process
   spawn without allocation in the child; s4 one lazy search thread; s5 one live frame per pane
   per publish; s6 the Linux gather fold, only if `bench/run.sh` on Linux matches; s7 the ConPTY
   reader feeding shards (`cargo check` for Windows). The state machine goes first (s1, same
   thread, no behaviour change), the shard threads second: a pane cannot move onto a shared
   thread while its loop blocks. Per slice: map, `lane-briefs.py '<spec>'
   slice`, `lane-run.sh <wt> <brief> <out> high 90 60 <base> <zone-globs>`, rerun its gates, merge
   into the lane branch, then the next brief. An ultra parity review (source-only) after s2, s3,
   LOOP b and LOOP d, the risky ones.
4. Gates: SHARDS against `wave2-<host>-3d0fc1b0.json` (threads = K, `mem.footprint.p20` <= 20 MB,
   floods, echo p99 under four floods); LOOP against the SHARDS merge JSON (`cli.cpu.display`,
   `spawn.cpu.*`, idle wakeups, fixed threads, statusjob). Merge checks per lane with
   `merge-checks-*.sh`, the config rows included; strict gates and the full corpus on both hosts
   at wave exit.
5. Inputs: `spawn.instr.split_empty_P` is bimodal on every binary (Mac, about 0.9 or 2.1 Minstr,
   tmux 1.1): profile a `split-window -d -P` + `kill-pane` loop with `just profile-cpu mac daemon`
   for LOOP. `mem.threads.p20` 66 on Linux (a gather thread per pane) is SHARDS'. Small follow-ups
   that fit any slot: FMT's cold template compile (`mem.copy_instr` +5.4%, Linux `attach.instr`
   +4%); `chatty.client_cpu_pct.visible` 1.08% Mac / 1.28% Linux against 1.0%. `control.latency`
   1.4-1.7x tmux goes to W4-DELIVER or the stdio handoff in CTRL's as-built notes.
6. Release freeze until wave 4; protocol stays 107.

## Wave 2 merge log (2026-09-30 to 10-01)

1. Done 2026-09-30 (from the Mac over ssh): the trim fix holds on Linux. At `d317e171`,
   `a_cleared_screen_survives_idle_compression` passes; with `GHOSTTY_SOURCE_DIR` at a `713374af`
   checkout it fails with the pane captured as seven blank lines, the bug seen for real. Three
   alternating quick pairs (`~/.cache/zz-perf/fixcheck/`): `throughput.detached.ascii` 128-130 MB/s
   old vs 128-131 new (tmux 56.7), `mem.footprint.p1` 2.78 vs 2.78-2.80 MiB, `p20` 16.9-17.5 vs
   16.9-17.7 MiB.
2. Done 2026-09-30: `w2-2-term-alienware-d7e3fc95.json` is now the quiet rerun (load 0.02 -> 1.2,
   built at `d317e171`, so with Ghostty `c3941417`; the name keeps the TERM merge). 56 pass, 16
   fail, 11 regressed against w2-1. The laptop was in its slow power state: tmux itself moved
   `spawn.cpu.split_shell` 0.80 -> 2.12 ms and `config.wall.source_1000` 7.7 -> 25.9 ms. Every
   regressed row is cpu or wall; instructions and bytes match the noisy record or w2-1
   (`attach.instr.p1` 10.08 vs 10.09 Minstr in the noisy record, `config.instr.source_1000` 41.6 vs
   41.7, `chatty.instr_per_s.visible` 384 vs 392), so `attach.cpu.*` 3.3 -> 4.7 ms at flat tmux is
   kernel and clock time, not TERM. Judge the next Linux merge on instructions and bytes.
3. W2-COPY merged on Linux 2026-09-30 as merge 3 (`2cedf6ee`, gate `w2-3-copy-alienware-2cedf6ee.json`:
   59 pass, 16 fail, the same 16 as the base but one, `cli.wall.list_panes_a.s20`, which three
   alternating A/B pairs show is timing: 1.2541 Minstr on both binaries). Linux copy entry 52.70 ->
   0.55 MiB, 104.8 -> 1.02 ms. The full `compat/run.sh` hit its 1800 s timeout after 243 rows; the
   rest ran per scenario (256/256 files covered). Raise that timeout for the wave-exit run.
   The Mac follow-up (Ghostty `67351380`) also lowers the Linux entry: 0.36 MiB, 4.33 Minstr.
   W2-FMT merged 2026-09-30 as merge 4 (`8fa427dd`), before CTRL, with the lane brief rules'
   merge checks on both hosts (`/tmp/zzpc/merge-fmt`, `~/.cache/zz-perf/merge-fmt`): Mac workspace
   4644 pass, 0 fail; Linux daemon 1291 pass with only the two known host/load failures; 37-38 of
   38 format/status/chooser compat rows clean (the others are the known `smoke/status-background-jobs`
   Mac row, `census-hooks` after-load-buffer and `show-options-hooks` lock differences, all red on
   the base). Three alternating quick A/B pairs per host against the `4263d378` binary: no new
   failure; instructions `list_keys` -96%, `list_panes_a.s20` -41..47%, `list_windows_a.s20`
   -45..49%, `statusjob.instr_per_s` -54..63%, `chain5.p20` -40..48%. `attach.instr.p1/p4` +4.3% /
   +3.4% on Linux (within 3% on the Mac): the first attach compiles status templates cold; CTRL
   rewrites that path next. No strict gate JSON for this merge (wave-exit rule).
   W2-CTRL merged 2026-09-30 as merge 5 (`87eb47c1`) after a 25 min parity review (one major:
   `ZZ_PERF_LEGACY_COMMAND=1` attach hung on a second `Hello`; one minor: the iOS example cached
   grid without layout generation) and a 15 min fix (`870b2b1e`). Merge checks on both hosts:
   workspace 4765-4771 pass with only known or solo-passing failures, attached-client, TUI screen
   diff (needs a UTF-8 locale), web and iPad builds, iOS simulator check (tree, split, live output,
   key-table patch), 33-35 of 35 control/attach/resize compat rows. A/B against the FMT binary,
   both hosts: attach connections 2 -> 1, server bytes 29.4 KB -> 2.3-2.9 KB, wire frames 6/9 -> 3,
   attach instructions -6..-11%, control instructions per command -26..-54%, latency -57..-62%,
   burst +200..470%. Linux `smoke/control-notify` then caught `%layout-change` losing the activity
   flag (`-` for `#-`, 7 of 8 runs; the Mac never showed it): `f6a25887` notes activity on the
   control output tap before raw output is published, publishes compact trees before layout
   hooks, and carries the missing silence flag (`WindowSnapshot.silence`, a wire change inside
   107). Its loop is 0 of 8 twice; `ctrl_flags_tests.rs` fails 5 tests with the fix disabled; the
   follow-up A/B leaves instructions flat on both hosts (Linux burst wall 32.6k -> 29.6k cmd/s at
   flat tmux and flat instructions: judge at the wave-exit gate). Known misses carried to W4:
   `control.latency` about 1.4-1.7x tmux (a stdio handoff design in CTRL's as-built notes),
   `attach.cpu`/`attach.ttfc` on Linux.
   Wave-2 exit (2026-10-01, `3d0fc1b0` binaries, i.e. `f6a25887`). Mac
   `wave2-macbook-3d0fc1b0.json`: 70 pass, 5 fail (`spawn.cpu.split_empty_P` and `kill_pane` for
   W3-LOOP, `chatty.client_cpu_pct.visible` 1.08% vs the 1.0% rule, down from 1.67%,
   `attach.ttfc.p1`, `control.latency` 1.5x), 15 regressed: `cli.wall.*` with tmux slower in the
   same run, `attach.wire_c2s` +30% (the Hello now carries the environment), and
   `throughput.detached.ascii` 345 -> 284 MB/s, which an alternating A/B of the COPY, FMT and CTRL
   binaries puts at 295-317 MB/s for all three (host state). Linux
   `wave2-alienware-3d0fc1b0.json`: 64 pass, 12 fail (spawn x3 and `mem.threads.p20` for wave 3,
   chatty visible for W4, `attach.cpu`/`ttfc` higher than in the COPY gate run at lower
   instructions, a host-state difference: the same-session chain is FMT flat, CTRL -24%).
   `mem.copy_instr.scroll180` +5.5% fails the 5% tolerance: A/B COPY 4.327, FMT 4.587, CTRL 4.563
   Minstr (FMT compiles status templates cold on the fresh server this row uses; still 0.45x
   tmux). Accepted at wave exit; owner: a cheaper cold compile, with the +4% Linux `attach.instr`.
   The full corpus then found ten rows clean on COPY and red after CTRL (own-conf bindings never
   applied, source-file diagnostics lost, `args-parse-*` control-typed arguments, config-grammar,
   oh-my-tmux), on both hosts, plus four Mac plugin-init rows (continuum, fpp, resurrect, tpm).
   `9e634cd8` fixes three causes: control stdin started before the initial config replay
   finished (early EOF cut it at shell waits), typed preparation errors lost `parse error:`, and
   ordinary control flags were preflighted before preceding effects and `command-error` hooks.
   After it: all 14 rows and the control rows clean on the Mac; the Linux full corpus (256 rows) is
   back to its 8 known reds (`known/*` x4, `lane2-store`, `show-options-hooks`,
   `smoke/control-alias-prepare`, `smoke/plugin-runtime-resurrect-restore`).
4. Known red on every build here, not lane regressions: `tui-screen-diff.sh` unzoom checkpoints
   under load (stale pane geometry after unzoom, open bug below), the three compat rows
   `lane2-store`, `show-options-hooks`, `smoke/plugin-runtime-resurrect-restore`, and the four
   known workspace tests.

## Mac checks after a wire change

Simulator smoke for the GPUI iOS client (the build alone does not prove the wire decodes):

```sh
target/release/zz_cli --socket /tmp/zzios.sock -f /dev/null new-session -d -s smoke -x 100 -y 30
target/release/zz_cli --socket /tmp/zzios.sock split-window -h -t smoke
ZZ_GPUI_ENDPOINT=/tmp/zzios.sock ZZ_GPUI_SESSION=smoke just ios-gpui iPad run
xcrun simctl io booted screenshot /tmp/ios.png
```

Drive it with `send-keys` (styled output, wide characters) and `bind-key` (a `KeyTablesPatched`
event); a decode failure drops the app to its connection screen. The simulator MCP tool needs a
one-time grant from the owner; `simctl` screenshots do not. `kill-server` and `xcrun simctl
shutdown booted` afterwards.

Read next, in this order: `knowledge/designs/daemon-perf-rebuild.md` (the plan: targets, lanes,
write zones, as-built notes per merged lane), `bench/perf/README.md` (the gate),
`bench/perf/thresholds.json`. Paths under `/Users/...` and `/private/tmp/...` in a report are on the
Mac; paths under `/home/demfabris/...` and `~/.cache/zz-perf/...` are on alienware.

## Mac leg (macbook, 2026-09-29 to 09-30): the wave-1 macOS gate

Worktree `~/dev/zz-macgate` at `2166bd31`, release build, M4 Max, tmux 3.7c. The host was not
quiet at the start (another project's `cargo test`, OrbStack, Spotlight indexing the new trees:
load 18-30); the gate started once the 1-minute load fell to 4.2 and nothing compiled.

### Gate `wave1-macbook-2166bd31.json` (strict, full, `--baseline w1-5-exec-macbook-a26b6368.json`)

62 pass, 5 fail, 20 regressed, 2 drifted, 373 s, load 4.2 -> 4.5 (5-minute load 8.2 -> 5.1).
Every W1-ATTACH rule passes unscaled on the reference host: `attach.tty_total.p1` 504 B (tmux 975),
`.p4` 1449 B (tmux 3653), `attach.conns` 2, `attach.instr.p1` 11.1 Minstr (tmux 17.4),
`chatty.tty_kibps.hidden` 0.72 KiB/s (tmux 0.75). Throughput rose with the Ghostty pin and the
memchr scan: `throughput.detached.ascii` 243 -> 310 MB/s (6.3x tmux), `.unicode` 104 -> 122 MB/s,
`throughput.attached.ascii_ms` 663 -> 525 ms.

The five failures, none a wave-1 regression:

| Row | zz / tmux (this run) | Rule | Why |
|---|---|---|---|
| `attach.ttfc.p1` | 16.9 / 20.7 ms | abs 14 ms | host state: tmux itself read 11.2 ms at w1-5. Same-host A/B (below): 2166bd31 17-22 ms, a26b6368 25-29 ms, tmux 19-23. zz is ahead of tmux; W2-CTRL owns the 1.1x-tmux wave2 rule |
| `attach.ttfc.p4` | 14.7 / 15.8 ms | abs 14 ms | same; A/B 21-22 ms vs a26b6368 40-46 ms, tmux 21-22 |
| `spawn.cpu.split_shell` | 1.72 / 1.79 ms | abs 1.5 ms | host state: instructions held (3.29 vs 3.41 Minstr); A/B a26b6368 1.7-2.1 ms vs 1.8-1.9. W3 |
| `spawn.cpu.split_empty_P` | 0.89 / 0.44 ms | ratio 1.6 | bimodal, see next row |
| `spawn.instr.split_empty_P` | 2.11 / 1.17 Minstr (hard) | +5% vs baseline | bimodal on both binaries: samples sit near 0.9 or 2.1 Minstr (this run min 0.89, median 2.11); A/B a26b6368 1.74 / 2.11 / 2.13, 2166bd31 2.12 / 2.15 / 2.14. w1-5 caught the low mode (1.15). Not the macOS pty spin bridge (it arms only after a read of 1 KiB or more). The extra ~1.2 Minstr is unexplained: W3-LOOP, which owns spawn CPU, should find it |

The 20 regressed rows are cpu/wall kinds (spawn, cold, config, chatty steady/visible, control
burst) and the four echo rows, with tmux slower by the same or more in the same run
(`spawn.wall.split_shell` tmux 5.8 -> 12.3 ms, `echo.p50.idle` tmux 0.15 -> 0.41 ms). The two
drifted rows are `echo.p50.idle` / `.p99.idle` against W0 (tmux moved 5.6x on the same rows);
echo rules are wave 3 since the Linux leg. Instructions, bytes, counts, footprint and threads did
not regress, except `spawn.instr.split_empty_P` above.

A/B: `bench/perf/run.py --only spawn,attach --w0 none`, three alternating pairs of the a26b6368
and 2166bd31 binaries at load 3.6-4.6 (scratch results, not committed).

### Darwin path of the PageList fix (`713374af`)

- `zig build test-lib-vt -Demit-lib-vt=true -Dtest-filter=PageList` at `713374af`: 347 of 347 pass.
  Those tests prove little about the Darwin path: `mem.zig` skips the OS under `builtin.is_test`.
- Footprint against w1-5: `mem.footprint.scroll180` 38.7 -> 35.3 MiB (tmux 61.7), `scroll80`
  32.8 -> 28.5 MiB (tmux 35.7), RSS flat (342.8 / 165.7 MiB). A probe on this Mac (16 KiB pages)
  showed why a missed recommit would read as a gain: pages written after `MADV_FREE_REUSABLE`
  without `MADV_FREE_REUSE` stay out of `phys_footprint` (64 MiB written, +64 KiB counted).
  The scroll rows never erase history, so their drop is the real trim plus the spare release.
- A codex review of the Darwin path found a data-loss bug on both platforms: `trimLastPage`
  released the cells past `size.rows * cols`, but `eraseRows` swaps row headers without moving
  cells, so after a partial history erase (a shell `clear` sends ED 3) the live rows point at
  the blocks the trim released. Linux discards them (`MADV_DONTNEED`): a unit test that models
  the discard reads the live rows back as zero, which should show as a blank pane one idle second
  after `clear` (not yet reproduced on a Linux host). macOS drops them out of the footprint and can
  lose them under memory pressure. The regression test fails at `713374af`.
- Fix: `demfabris/ghostty@c3941417` on the new branch `zz-2026-09-30` (`zz-2026-09-29` keeps
  `713374af`): the trim starts after the highest cell block a live row references, and test builds
  discard the whole range (as `MADV_DONTNEED` does) instead of only the dirty prefix. Full
  `zig build test-lib-vt -Demit-lib-vt=true` on the Mac: 6488 pass, 52 skipped, 0 fail.

### Gate `w2-2-term-macbook-adbc5407.json` (strict, full, `--stage wave2`, baseline the wave-1 Mac JSON)

`main` after the repin: W2-HOOKS, W2-TERM and Ghostty `c3941417`. 62 pass, 10 fail, 6 regressed,
2 drifted, 367 s, load 2.8 -> 2.2. The Mac view of the two wave-2 merges, on rows that do not
depend on host speed:

| Row | wave1 Mac | wave2 Mac | tmux | Lane |
|---|---|---|---|---|
| `config.instr.source_1000` | 133.4 | 39.3 Minstr | 198.5 | HOOKS |
| `cli.instr.chain5.p20` | 1.085 | 0.479 Minstr | 1.296 | HOOKS |
| `cli.instr.show_options.p20` | 0.311 | 0.144 Minstr | 0.888 | HOOKS |
| `echo.wire_bytes.idle` / `.busy30` | 1133 / 3399 | 33 / 137 B | - | TERM |
| `attach.wire_s2c.p1` | 100,040 | 29,391 B | - | TERM (the rest is the 28 KB hello, W2-CTRL) |
| `attach.instr.p1` | 11.1 | 10.4 Minstr | 17.4 | TERM |
| `chatty.instr_per_s.visible` | 396 | 378 Minstr/s | 171 | TERM |
| `throughput.detached.ascii` | 310 | 345 MB/s | 50.8 | TERM |
| `mem.footprint.scroll180` / `scroll80` | 35.3 / 28.5 | 33.8 / 27.0 MiB | 61.3 / 35.9 | both, on the fixed pin |
| `mem.footprint.p1` / `p20` | 6.19 / 26.4 | 5.88 / 25.1 MiB | 2.69 / 2.84 | both |

Failing rows and owners: W2-CTRL `attach.conns.p1` / `.p4` (2, rule 1), `attach.wire_s2c.p1` /
`.p4` (29.4 / 29.7 KB, rule 8 KiB), `control.latency` (3.3x, rule 1.2x), `control.burst_cmds_per_s`
(0.28x, rule 0.8x); `chatty.client_cpu_pct.visible` 1.67% (tmux 0, rule 1.0%; wave1 Mac 1.86%, so
TERM did not move it here; W2-CTRL or W4-DELIVER must find the client's per-frame work);
`spawn.cpu.split_shell` 2.13 ms (abs 1.5, tmux 1.76) and `spawn.cpu.split_empty_P` (bimodal, above),
W3; `spawn.cpu.kill_pane` 0.45 ms (wave2 abs 0.4, tmux 0.17, instructions 0.66 -> 0.64 Minstr), W3-LOOP.
The six regressed rows are cpu/wall rows whose tmux moved the same way (`attach.ttfc.p4` 14.7 ->
20.4 ms with tmux 15.8 -> 20.9, `chatty.cpu_pct.steady` 3.7 -> 7.1% with tmux 3.4 -> 5.6%) or
bimodal (`split_empty_P`), plus `echo.p50.busy30` 1.76 -> 2.47 ms (tmux 0.38 -> 0.44; a wave-3
row) and `statusjob.cpu_pct` 0.37 -> 0.50% at +3% instructions. The two
drifted rows are the echo idle rows again. Attach ttfc passes the wave2 ratio rule (0.90x and 0.98x
tmux).

### macOS-only checks

| Check | Result |
|---|---|
| CoreFoundation-free link | `just build mac` at `main` (`ae63c577`): `otool -L dist/zz/zz.app/Contents/MacOS/cli` lists only `libSystem.B.dylib`; 81 dyld images at `cli -V` (tmux 86) |
| `just ios-gpui iPad build` at `main` | builds clean, no warnings: W1-ATTACH hello capabilities, W2-HOOKS `KeyTablesPatched` and W2-TERM PaneFrames compile for `aarch64-apple-ios-sim` |
| Ghostty PageList on Darwin | see above |

## Where things stand

| Branch | Head | Contents | State |
|---|---|---|---|
| `main` (origin) | fast-forwarded to `perf/wave2` after each merge | W0, wave 1 (merged `1e0bfc6a`), the Ghostty pin, W2-HOOKS, W2-TERM, the Mac leg (gate JSONs, Ghostty repin `f822301d`) | pushed |
| `perf/wave2` | behind `main` by the Mac leg's commits | integration branch in `~/dev/zz-perf-int` | fast-forward it to `main` before the next lane |
| `perf/wave1` | deleted after merging into `main` | W0 gate + plan and all of wave 1 | history kept in `main` |
| `perf/attach` | `9a72b53b` (was `00831246` at handoff) | W1-ATTACH | fix pass finished on Linux, merged into `perf/wave1` as `ce1b34cd` (w1-6); branch and worktree removed |

Wave 1 merge log on `perf/wave1` (gate JSON per merge in `bench/perf/results/`):

| # | Lane | Merge | Gate JSON |
|---|---|---|---|
| W0 | gate + plan | `157ac6a3` (local main) | `baseline-macbook-17e17115.json`, `baseline-quick-macbook-17e17115.json` |
| 1 | FOOTPRINT | `613630ce` | `w1-1-footprint-macbook-613630ce.json` |
| 2 | FORMAT (before PUBLISH, see below) | `a5c5cab7` | `w1-2-format-macbook-a5c5cab7.json` |
| 3 | PUBLISH | `d5542787` | `w1-3-publish-macbook-d5542787.json` |
| 4 | PANE | `ad9c0c9b` | `w1-4-pane-macbook-ad9c0c9b.json` |
| 5 | EXEC | `a26b6368` | `w1-5-exec-macbook-a26b6368.json` (last full gate, load 4-5 on 16 CPUs, not noisy, not `--strict`) |
| fold | `perf/peers-process-info` | `d617fec9` | none; Claude peer helpers folded into `process_info` |
| fold | `fix/pty-bridge-adaptive-spin` | `c7c93042` | quick throughput only: 261.0 MB/s vs tmux 47.4 (5.51x), W0 quick 233.9; not committed (`results/local/`) |
| fold | `perf/footprint` (title fix) | `cd1ce6eb` | none; `select-pane -T` titles survive shell-integration prompt retitles |
| extra | registry | `9eb5888b` | closes `terminal.shell-integration-prompt-title` in `compat/tmux-gaps.json`; owner may drop it |
| 6 | ATTACH | `ce1b34cd` | `w1-6-attach-alienware-ce1b34cd.json` (Linux, `--strict`, full, quiet: 59 pass, 11 fail, 0 regressed, 0 drifted) |

FORMAT merged before PUBLISH, against the plan's order: a usage-limit cut left PUBLISH's fix pass
unfinished while FORMAT was ready, and the resume run (`scripts/wave1-resume.js`) merged it first.
PUBLISH's as-built notes are written against FOOTPRINT and FORMAT. None of merges 1-5 ran the gate
with `--strict` (other lanes were compiling); see the open decisions.

Fold checks on the Mac: fmt and clippy `-D warnings` clean; zz-terminal 309, zz-mux 584 pass;
`cargo test --workspace --all-features --no-fail-fast` green on rerun (first run: 3 zz-daemon
load flakes that pass alone); `just compat-check` green with Homebrew bash first.

Wave 2 merge log on `perf/wave2` (Linux, alienware; gate JSON per merge in `bench/perf/results/`,
each `--baseline` is the row above, the first one's is `wave1-alienware-aaaa8195.json`):

| # | Lane | Merge | Follow-ups | Gate JSON |
|---|---|---|---|---|
| 1 | HOOKS | `0acd7f2a` | `74f35b65` (foldhash in `clients/web/Cargo.lock`, or `just web-build` stops at `--locked`) | `w2-1-hooks-alienware-0acd7f2a.json` (`--strict`, full, quiet, load 0.8-1.7: 55 pass, 17 fail, 7 regressed, 1 drifted) |
| 2 | TERM | `d7e3fc95` | `8f18e4d7` (two TERM tests that only failed under `cargo test --workspace`) | `w2-2-term-alienware-d7e3fc95.json` (`--strict`, full, NOT quiet: a Steam game used two cores, load 3.3-7.1: 52 pass, 20 fail, 52 regressed, 5 drifted; read its CPU, wall and throughput rows as noise) |

Merge 1 (HOOKS): every instruction row dropped or held (`config.instr.source_1000` 139.4 -> 41.7
Minstr, tmux 82.4; `cli.instr.display.p20` 0.248 -> 0.136, tmux 0.262; `cli.instr.chain5.p20`
1.106 -> 0.425, tmux 0.493; `control.instr_per_cmd` 0.157 -> 0.128); bytes, footprint and threads
held. The 7 regressed rows are cpu/wall (`cli.wall.version.p1`, `cli.wall.display.p20`,
`cli.cpu.display.p20`, `spawn.wall/cpu.split_shell`, `cold.wall.new_session_noterm`,
`control.burst_cmds_per_s`) with tmux moving the same way or the pre-merge binary reading the same
in alternating A/B runs (`~/.cache/zz-perf/hooks/merge/ab*.json`); A/B medians put
`cli.cpu.display.p20` 16-33% under the pre-merge binary and `control.burst_cmds_per_s` at 16.8k
against 15.2k. Stage wave2 rules for unmerged lanes fail as expected (attach, echo wire, control
latency and burst, `chatty.*.visible`), plus the W3 rows (spawn CPU, `mem.threads.p20`). Both
throughput rows pass now because of the Ghostty pin (`2166bd31`), not HOOKS.

Merge 2 (TERM): the wire rows moved as planned and are exact on any host: `echo.wire_bytes.idle`
1,133 -> 33 B (rule 64, pass), `echo.wire_bytes.busy30` 3,399 -> 137 B, `attach.wire_s2c.p1` /
`.p4` 100,064 / 100,237 -> 29,417 / 29,691 B (still over the 8 KiB rule: the rest is the 28 KB
`ServerHello`, W2-CTRL), `attach.instr.p1` / `.p4` 10.62 / 11.08 -> 10.09 / 10.56 Minstr,
`chatty.instr_per_s.visible` 389 -> 381 Minstr/s (tmux 149). The gate ran while a game used two
cores (the quiet wait gave up after 10 minutes), so both muxes lost half their throughput
(`throughput.detached.ascii` 56.5 MB/s, tmux 24.1, bare pty reader 56.1) and most CPU and wall
rows read as regressed against w2-1; `mem.footprint.p20` 17.4 -> 19.1 MiB tripped the strict
mem rule, but the pre-merge binary reads 19.2-20.0 MiB on the same host. Alternating runs of the
pre-merge and merge binaries on that host (`~/.cache/zz-perf/term/merge/ab-*.json`, 3 to 7
pairs) show no regression: `chatty.cpu_pct.visible` 11.2% -> 10.8%, `attach.cpu.p1` 3.53 -> 3.39
ms, the other chatty, attach, echo and mem rows within noise. `chatty.instr_per_s.hidden` read
45.7 against 43.4 Minstr/s, tracking more status redraws in the same 10 s window
(`chatty.tty_kibps.hidden` 0.72 against 0.57 KiB/s, bimodal on both binaries, tmux 0.73); TERM
does not touch that path, but merge 3 should watch it. **For merge 3, compare CPU, wall and
throughput rows against w2-1 or a quiet rerun, not this JSON.**

### Numbers at the last full gate (`w1-5-exec`, macOS M4 Max, tmux 3.7c, medians)

| Metric | tmux | zz W0 | zz now | now/tmux | wave1 rule | verdict |
|---|---|---|---|---|---|---|
| `cli.cpu.display.p1` (ms) | 0.10 | 1.08 | 0.111 | 1.11 | <= 0.25 ms | pass |
| `cli.cpu.display.p20` (ms) | 0.143 | 3.07 | 0.117 | 0.82 | <= 0.4 ms | pass |
| `cli.wall.display.p1` (ms) | 4.12 | 5.00 | 2.62 | 0.64 | <= 1.1x | pass |
| `cli.cpu.list_keys.p20` (ms) | 2.91 | 64.1 | 1.23 | 0.42 | <= 5 ms | pass |
| `cli.cpu.list_windows_a.s20` (ms) | 0.264 | 8.16 | 0.175 | 0.66 | <= 1.2x | pass |
| `cold.wall.new_session` (ms) | 12.5 | 42.5 | 7.81 | 0.63 | <= 22 ms | pass |
| `config.cpu.source_1000` (ms) | 9.22 | 230 | 8.58 | 0.93 | <= 25 ms | pass |
| `spawn.wall.split_empty_P` (ms) | 4.63 | 2008 | 3.20 | 0.69 | <= tmux + 1.5 | pass |
| `spawn.cpu.kill_pane` (ms) | 0.174 | 1.61 | 0.294 | 1.69 | <= 0.8 ms | pass |
| `chatty.cpu_pct.flip` (%) | 2.01 | 23.9 | 1.96 | 0.97 | <= 1.5x + 1 | pass |
| `chatty.cpu_pct.hidden` (%) | 2.26 | 54.1 | 1.99 | 0.88 | <= 1.5x + 1 | pass |
| `chatty.cpu_pct.visible` (%) | 2.26 | 17.0 | 3.64 | 1.61 | <= 1.5x + 1 | pass |
| `chatty.tty_kibps.hidden` (KiB/s) | 0.77 | 237 | 2.82 | 3.68 | <= 2 | **fail** |
| `idle.wakeups_per_s.p20` | 0 | 1.2 | 0.3 | - | <= 1 | pass |
| `mem.footprint.p1` (MiB) | 2.67 | 7.92 | 6.14 | 2.30 | <= 6.5 | pass |
| `mem.footprint.p20` (MiB) | 2.81 | 54.3 | 26.1 | 9.29 | <= 38 | pass |
| `mem.threads.p20` | 1 | 89 | 46 | 46 | <= 51 | pass |
| `mem.footprint.scroll180` (MiB) | 61.2 | 344 | 38.7 | 0.63 | <= 1x | pass |
| `attach.ttfc.p1` (ms) | 11.2 | 18.0 | 15.0 | 1.34 | <= 14 ms | **fail** |
| `attach.ttfc.p4` (ms) | 10.3 | 26.8 | 20.4 | 1.99 | <= 14 ms | **fail** |
| `attach.tty_total.p1` (B) | 972 | 162018 | 175738 | 181 | <= 8192 | **fail** (drifted +8% vs W0) |
| `attach.tty_total.p4` (B) | 3653 | 336880 | 344818 | 94 | <= 16384 | **fail** |
| `attach.conns.p1` / `.p4` | - | 4 / 4 | 4 / 4 | - | <= 2 | **fail** |
| `attach.wire_s2c.p4` (B) | - | 239976 | 101218 | - | <= 133120 | pass |
| `echo.p50.idle` (ms) | 0.151 | 0.262 | 0.333 | 2.20 | <= 1.5x | **fail** |
| `echo.p99.idle` (ms) | 0.384 | 0.489 | 0.607 | 1.58 | <= 1.5x | **fail** |
| `echo.p50.busy30` (ms) | 0.202 | 6.38 | 0.846 | 4.19 | <= 1.5x | **fail** |
| `echo.p99.busy30` (ms) | 0.543 | 18.5 | 3.47 | 6.39 | <= 1.5x | **fail** |
| `throughput.detached.ascii` (MB/s) | 50.5 | 245 | 243 | 4.81 | >= 4x, >= 0.85x W0 | pass |
| `throughput.detached.unicode` (MB/s) | 9.74 | 103 | 104 | 10.6 | same | pass |
| `throughput.attached.ascii_ms` (ms) | 3853 | 631 | 663 | 0.17 | <= 0.25x | pass |

Totals: 59 pass, 11 fail, 0 regressed, 1 drifted. All 11 failures are the attach, echo and
`chatty.tty_kibps.hidden` rows above. `python3 bench/perf/run.py --rescore <json> --stage <s>`
re-evaluates any JSON at another stage.

### What still loses to tmux, and who owns it

| Area | Now (zz vs tmux) | Rule | Owner |
|---|---|---|---|
| Attach bytes, conns, ttfc | 176 KB vs 972 B tty (p1); 4 conns; ttfc 1.3-2x | wave1, then wave2 (1 conn, <= 2 KB, 1.1x) | W1-ATTACH (lane measured 1.9 KB, 2 conns, ttfc at parity when quiet), then W2-CTRL |
| `chatty.tty_kibps.hidden` | 2.82 vs 0.77 KiB/s | wave1 <= 2 | W1-ATTACH (reviewer measured 0.6-1.05) |
| Echo latency | 2.2x idle p50, 4-6x busy30 | wave1 <= 1.5x | **no wave-1 owner**: W1-PANE's notes hand the remaining hops to W3-SHARDS and W4-DELIVER |
| Echo wire bytes | 1133 B per key | wave2 <= 64 B | W2-TERM |
| Control mode | latency 3.3x, burst 0.14x (14.6k vs 106k cmd/s), CPU/cmd 2.7x | wave2 | W2-CTRL |
| Visible chatty | server CPU 1.61x, tty 1.33x, client CPU 1.67% vs 0 | wave2 tty/client, final CPU <= 1.2x + 0.5 | W2-TERM, W4-DELIVER, W4-ROWS |
| `spawn.cpu.kill_pane` | 1.69x | wave2 <= 0.4 ms, wave3 tmux + 0.1 | W2-HOOKS, W3-LOOP |
| `spawn.cpu.split_empty_P`, `cli.cpu.send_keys.p1` | 1.30x, 1.35x | final <= 1.2x | W3-LOOP |
| Threads | 46 vs 1 (p20), 9 vs 1 (p1) | final <= 12 | W3-SHARDS, W3-LOOP, W4-DELIVER |
| Footprint | 26.1 vs 2.8 MiB (p20), 6.1 vs 2.7 (p1) | final <= 12 / <= 5 MiB | W3, W4 |
| RSS | 13.0 vs 4.1 MiB (p1) | info; W4-BINARY gates `mem.rss.p1` <= 2x | W4-BINARY |
| Status jobs, idle wakeups | 3 threads/s vs 0; 0.3/s vs 0 | wave3 <= 0.5/s, <= 0.2/s | W3-LOOP |
| Format code in list/status paths | per-row template parse 25-30% of `list-keys` p20 | wave2 | W2-FMT |
| Copy-mode entry memory | not measured (gate TODO) | W2-COPY <= 1 MB, <= 5 ms | W2-COPY adds the group |

Already ahead of tmux: every `cli.wall.*` (0.47-0.67x), cold start, config replay, list CPU,
scrollback footprint (0.63x at 180 cols), throughput (4.8x ASCII, 10.6x unicode).

## Linux leg (alienware, from 2026-09-29)

Host: Tiger Lake i7-11800H, 8 cores / 16 threads, 15 GB RAM, btrfs, CachyOS kernel 7.2 with THP
`always`, tmux 3.7c at `/usr/bin/tmux`, `perf_event_paranoid` 2 (user-space counters only).
At most two compiling lanes at a time on this box.

Done so far on `perf/wave1`:

| Commit | What |
|---|---|
| `658e640d` | Linux instruction counts in `probe.py` (`perf_event_open`, user space, `inherit` + `inherit_thread`: later threads fold in, forked panes stay out) |
| `166b8f95` | `compat/run.sh` lists rows that failed twice |
| `f50c434e` | Linux W0 (`baseline-alienware-17e17115.json`, quick twin) and the strict fold gate `w1-5-fold-alienware-166b8f95.json` (45 pass, 25 fail, 1 drifted) |
| `5d41ad4b` | the gate passes `ZZ_PERF_*` knobs to the servers (before this, knob-off gate runs silently measured the default path) |
| `ad4ee99c` | merge of `perf/linux-thp`: `zz_cli` turns THP off for itself (constructor ahead of mimalloc's), panes get it back; knob `ZZ_PERF_THP=1` |

What Linux showed that the Mac did not (fold gate, zz vs tmux medians):

| Area | Linux | Mac (w1-5-exec) | Cause and owner |
|---|---|---|---|
| Footprint p1 / p20 | 16.1 / 103 MiB (tmux 0.95 / 1.06) | 6.1 / 26.1 | THP in mimalloc's arena; fixed in `ad4ee99c`: same-binary A/B 15.2 -> 3.1 and 99 -> 18 MiB. libmimalloc-sys 0.1.49's `no_thp` feature does not set `MI_DEFAULT_ALLOW_THP=0` (upstream bug worth reporting) |
| `throughput.detached.ascii` | 90.9 MB/s, 1.71x tmux (W0 87.4, 1.64x) | 243, 4.81x | Kernel ceiling: a bare forkpty reader gets 125-139 MB/s through a cooked tty (714 MB/s raw), and the master's read buffer is 4 KiB, so a reader that naps 100 us drops to 25 MB/s. zz can reach at most ~2.6x tmux here; the 4x rule is Mac-only. Inside zz: the pane actor spends 63-68% of its time in the kernel on ~485k page faults per 0.7 s, all from libghostty `PageList.grow -> createPageExt -> memset` after the line-limit path decommits retired pages. Lane W1-LINUX-PAGES (fork fix, below) |
| `spawn.cpu.*` | 2-2.6x tmux (tmux itself 2.2 ms per split here, 0.9 on the Mac) | ~1x | ~3 ms of kernel time per new window (user space is 0.76 Minstr); 503 daemon page faults per window; per-pane thread creation and `Terminal::new` page setup are the likely bulk. W3-SHARDS / W3-LOOP; a `CLONE_VM` spawn on Linux is a possible side item |
| `mem.threads.p20` | 66 | 46 | Linux runs one `zz-pty-gather` thread per pane on top of the Mac set. W3-SHARDS |
| Echo | 2.0-3.0x tmux, both slower (tmux idle p50 0.94 ms vs 0.15 on the Mac) | 1.6-6.4x | each thread hop costs a wakeup on this laptop; W3 / W4 own the hops |
| Abs rules | `config.*.source_1000` 30 / 26 ms vs rule 25 (tmux itself 25.6 / 21.1) | pass | Mac-calibrated absolute ms rules do not transfer to a slower CPU; see decisions |

Base-branch test status on Linux (not regressions of any lane): `which_key::tests::caps_fold_families_and_ranges`
(zz-ui) fails every run and main's CI is red too; `endpoint::tests::remote_scripts_fall_back_to_the_mac_app_bundle_cli`
fails on any host with `zz` on PATH (this one has it); `process_info::tests::the_current_process_matches_sysinfo`
is flaky because Linux CPU time comes in 10 ms ticks (test bug in W1-FOOTPRINT code, fix queued). A test
run from a shell with `ulimit -n` 1024 fails ~114 zz-daemon tests with EMFILE: run suites from a shell
with a high fd limit.

Lanes in flight:

| Lane | Where | State |
|---|---|---|
| W1-ATTACH | was `~/dev/zz-attach`, `perf/attach` | merged 2026-09-29 as `ce1b34cd` (w1-6), worktree and branch removed; reports in `~/.cache/zz-perf/attach/` (`report.md`, `merge-report.md`), statuses in `attach-review.json` |
| W1-LINUX-PAGES | fork `demfabris/ghostty`, now branch `zz-2026-09-30` at `c3941417` (`zz-2026-09-29` keeps `713374af`) | landed: zz side `0e590636` (memchr escape scan), fork pin `2166bd31` (detached ASCII 85 -> 120-134 MB/s, unicode 45 -> 91, actor 98% -> 30% CPU); the Mac leg found and fixed its trim bug, repin `f822301d` |
| W1-ATTACH-PAINT | was `~/dev/zz-attach-paint`, `perf/attach-paint` | merged `aaaa8195`: `Renderer::note_frame` merged, instead of replacing, a pane's unpainted damage when a drain took a second frame; regression test in zz-tui app.rs |
| W2-HOOKS | was `~/dev/zz-hooks`, `perf/hooks` | merged into `perf/wave2` as `0acd7f2a` (w2-1), worktree and branch removed; reports in `~/.cache/zz-perf/hooks/` (`fix-report.md`, `merge-report.md`) |
| W2-TERM | was `~/dev/zz-term`, `perf/term` | merged into `perf/wave2` as `d7e3fc95` (w2-2) plus `8f18e4d7`, worktree and branch removed; reports in `~/.cache/zz-perf/term/` (`impl-report.md`, `fix-report.md`, `merge-report.md`). After pulling it, `kill-server` every zz daemon built before it: the wire changed inside 107 |
| W2-CTRL | not started | can start now from `perf/wave2`: its `Batch` carries TERM's PaneFrames; the encoder entry points are `encode_terminal_viewport_event_into` and `encode_terminal_patch_event_into` (zz-protocol `terminal_codec.rs`, the patch one takes a `TerminalPatchRef` and a `PatchTail`) |

## Decisions taken on the Mac leg (owner away; revisit if you disagree)

- **Gate of record at `2166bd31`, as asked, with no rerun.** Its five failures are explained by
  a same-host A/B against the w1-5 binary (`a26b6368`); none is a wave-1 regression.
- **`spawn.instr.split_empty_P` (a hard instruction row) accepted as bimodal.** The old binary
  reads the same on this host; owner W3-LOOP (next Mac session, item 4).
- **CoreFoundation check and bundle at `main`, not `2166bd31`.** W1-FOOTPRINT's link change is in
  both; checking `main` also covers wave 2 and, after the repin, the new pin (`f822301d`).
- **The Ghostty fix is pushed and pinned.** This revisits the Linux leg's "Ghostty fork: nothing
  is pushed": the pin on `main` could blank a Linux pane after `clear`, and the native-fork process
  in `.agents/skills/fork-rebase/SKILL.md` publishes a new dated branch (`zz-2026-09-30`) without
  touching the published one. Validation: full `zig build test-lib-vt` (6488 pass, 52 skipped),
  zz-terminal 314 pass, clippy, a bundle build that fetched the commit from GitHub, 204 exported
  `ghostty_*` symbols as before.
- **The fork's test discard zeroes the whole range** (as `MADV_DONTNEED` does), not only the dirty
  prefix, so its unit tests can see a trim that loses live cells. The zz test
  `a_cleared_screen_survives_idle_compression` passes on macOS on either pin (the discard keeps the
  contents there); proven against both pins with a Linux-like discard patched into a local checkout.
- **The first wave-2 run at `ae63c577` was stopped and discarded**: a zig build of mine overlapped
  its cli and spawn groups. The record is `w2-2-term-macbook-adbc5407.json`, after the repin, with
  the Mac wave-1 JSON as its baseline.
- **No echo A/B.** Echo rules are wave 3 since the Linux leg, and tmux moved as much in both runs.

- **W2-COPY merged before W2-CTRL and W2-FMT** (plan order CTRL, FMT, COPY): COPY finished its
  fix pass first while the other two were still implementing; it shares no write zone with them
  except two retained-event daemon edits handed to CTRL in its as-built notes.
- **COPY's packaging overruled.** The lane vendored the safe wrapper (~12k lines) and added a
  build-time `copy-mode.patch`; owner rules say no build-time source rewriting and no vendored
  wrapper. Its native delta became Ghostty `7823f65d`, its wrapper delta libghostty-rs `8e40135f`
  (new branch `zz-2026-09-30`), both published by me after review; the lane went from 53 files
  +16.4k to 28 files +3.8k.
- **COW review limits accepted as minor.** Codex's copy-on-write lifetime review of `7823f65d` was
  cut by the cyber filter after clearing the main paths (88/88 COW tests, concurrent read probe);
  two Zig-API-only cases (a later-row clone sharing an active page; the ownership record in the
  snapshot allocator) cannot be reached from the C entry (default allocator, full-screen clone).
- **Mac copy entry fixed after the merge.** 1.41 MiB on the Mac against the 1.0 rule (Linux 0.59):
  Ghostty `67351380` copies active pages at their used size and `ModeRevision` builds viewport
  cells straight into the `Arc`; Mac 0.58 MiB. The Linux gate is COPY's merge of record; the Mac
  strict view waits for wave-2 exit (the Mac was compiling two lanes).
- **CTRL and FMT implementation stopped by the orchestrator** after 13.5 h (at their next commit or
  21:15): see "Lane brief rules". FMT's own rows pass; its one real regression,
  `attach.instr.p1/p4` 10.44 -> 14.34 / 12.33 -> 16.22 Minstr, went to a focused fix brief.

## Owner decisions (binding)

- Performance before features: no plugins or other features until the daemon is at least as lean as tmux.
- The wire protocol may be rebuilt from scratch; zz is not widely deployed.
- Release freeze until W4 exits: no tags, beta pushes or TestFlight uploads until `--stage final` passes.
- One unreleased protocol version, 107, for the whole campaign. No lane bumps `PROTOCOL_VERSION`; non-append changes inside 107 are allowed.
- "No compromises": targets are floors. A lane that meets its target but still shows avoidable work on its path removes that work too.
- Wave 3 (single-owner loop and PTY shards) is committed, not optional. W2-FMT, W4-ROWS and W4-BINARY are reinstated lanes.

## Decisions taken on the Linux leg (owner away; revisit if you disagree)

- **Merges of record on Linux: yes.** Lane merges run the strict gate here against the Linux W0;
  each wave exit still gets one strict Mac run (`<stage>-macbook-<sha8>.json`).
- **Merges 1-5 without `--strict`: no reruns.** The strict Linux fold gate (`w1-5-fold-alienware-166b8f95.json`)
  covers all five merged lanes at once and is the baseline for merge 6.
- **`9eb5888b` (gap registry close): kept.** The behaviour it records is real and tested.
- **Ghostty fork: nothing is pushed.** Fork fixes are developed on a local clone and wait for the
  owner to push the branch and bump `GHOSTTY_COMMIT`.
- **Echo rows move from wave 1 to wave 3** (unchanged 1.5x tmux). No wave-1 lane owned the
  remaining hops; W1-ATTACH left echo unchanged against its pre-merge binary (2.0-3.0x tmux on
  Linux). W3-SHARDS, W3-LOOP and W4-DELIVER remove those hops. In `thresholds.json`, the Targets
  table and its notes.
- **Mac-calibrated rules on other hosts: scaled, not overridden.** `thresholds.json` names the
  macOS W0 as `reference`; elsewhere `abs` rules of kind `cpu`/`wall` keep their multiple of the
  reference tmux and `mem` rules keep their margin over it (`abs@ratio`, `abs@plus` in the
  checks). The throughput rules also pass at 85% of a pty ceiling measured in the same run
  (`throughput.ceiling.*`). Counts, bytes, threads and ratio rules are unchanged. Rescored this
  way, w1-6 reads 59 pass, 7 fail (spawn CPU x3, `chatty.cpu_pct.visible`, `mem.threads.p20`,
  both throughput rows, whose ceiling pass needs the unpushed PageList fork fix).

## Next steps, in order

### 0. On the Mac, before leaving (owner)

Nothing is pushed. From `~/dev/zz-perf-int`: `git push origin perf/wave1 perf/attach`.
Merged side worktrees still on the Mac: `~/dev/zz-footprint`, `~/dev/zz-peers`, `~/dev/zz-tpcollapse`
(branches `perf/footprint`, `perf/peers-process-info`, `fix/pty-bridge-adaptive-spin`, all in
`perf/wave1`); remove them when their sessions are done.

### 1. Fetch

```sh
git clone https://github.com/demfabris/zz.git ~/dev/zz    # or fetch in an existing clone
git -C ~/dev/zz fetch origin perf/wave1 perf/attach
git -C ~/dev/zz worktree add ~/dev/zz-perf-int perf/wave1
git -C ~/dev/zz worktree add ~/dev/zz-attach perf/attach
```

Work in worktrees, never in the main checkout if other sessions use it.

### 2. Linux setup

| Need | How | Why |
|---|---|---|
| Rust 1.97.0 + clippy, rustfmt | `rust-toolchain.toml` (rustup picks it up) | MSRV |
| Zig 0.16.0 | `mise install` (reads `mise.toml`) or a tarball on PATH | every build: `libghostty-vt-sys/build.rs` runs `zig build`, not only release builds |
| System packages | Ubuntu: the `apt-get install` list in `.github/workflows/ci.yml` (autoconf automake bison cmake libevent-dev libncurses-dev libfontconfig-dev libwayland-dev libxkbcommon-dev pkg-config ninja-build ...); Arch: the equivalents plus `base-devel`; see `knowledge/playbooks/prerequisites.md` | GUI crates in `--workspace`, the pinned tmux build |
| CEF | `export CEF_PATH=$HOME/.cache/cef` (as in `knowledge/playbooks/running-zz.md`; CI points it at a shared `.cef-cache` directory); `cef-dll-sys` downloads 154.0.23 there once instead of into every target dir | `cargo clippy/test --workspace` builds `zz-browser` |
| Release tmux for the gate | distro package (`pacman -S tmux` / `apt install tmux`) at `/usr/bin/tmux` | the gate refuses ASan builds, scripts and zz's own tmux wrapper; record `tmux -V` (the Mac used 3.7c, distros ship older; ratios are per run) |
| Pinned tmux oracle | `compat/fetch-tmux.sh` in each worktree (builds next-3.8 `d77c9dc6` into `compat/.cache`) | `compat/run.sh`, `just compat-check`; not for the gate |
| bash >= 4 | default on Linux | `compat/run.sh` uses `declare -A`, `diff-scenario.sh` uses `mapfile` |
| `just` | distro package or `cargo install just` | every recipe (`perf-gate`, `compat-check`, `web-build`) |
| Linux `perf` | Arch `perf`; Ubuntu `linux-tools-$(uname -r)` | the profiler in the lane prompts (`PROFILER` in `scripts/wave-lanes.js`) |
| perf counters | `sudo sysctl kernel.perf_event_paranoid=1` (Ubuntu defaults to 4, Arch to 2) | 2 is enough for step 3's user-only counters on your own processes; `perf record -g` with kernel stacks needs 1. VMs may expose no PMU at all |
| Web toolchain | `just web-setup` (nightly with `wasm32-unknown-unknown`, wasm-bindgen-cli 0.2.128) | `just web-build` in steps 5 and 9 |
| Disk | >= 20 GB free before any build; each worktree target grows to 40-50 GB | |

First build in `~/dev/zz-perf-int`: `cargo build -p zz-cli && cargo build --release -p zz-cli`, then
`cargo test --workspace --all-features --no-run` (the cold GUI and CEF compile, untimed), then run
the Linux-only tests nobody has run yet: `cargo test -p zz-daemon -- process_info process_facts`,
`cargo test -p zz-mux localtime`, `timeout 3600 cargo test --workspace --all-features --no-fail-fast`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, `just compat-check`.
Code written on the Mac for Linux and never run: `process_info.rs` `/proc` paths, zz-mux
`localtime.rs`, zz-terminal `unix_pty.rs`, the `/proc/<daemon pid>/exe` tmux wrapper pin (W1-EXEC),
the `zz-pty-gather` path. `bench/perf` itself has never run on Linux (README "Not built yet").

### 3. Instruction counts in `bench/perf/probe.py` for Linux (done, `658e640d`)

Do this before recording baselines, so the Linux W0 carries `instr` twins and the 5% regression
rule has something to hold (CPU time moved up to 61% between runs of one binary while lanes built).

- The Linux branch sets `HAS_INSTRUCTIONS = False` and `instructions=0`, and `run.py` `add_instr` then drops every `instr` metric.
- Use `perf_event_open` through `ctypes.CDLL(None).syscall` (nr 298 on x86_64, 241 on aarch64), `PERF_TYPE_HARDWARE`/`PERF_COUNT_HW_INSTRUCTIONS`, `inherit=1`, `exclude_kernel=1`, `exclude_hv=1`.
- A counter attaches to one thread. When `sample()` first sees a pid, open one counter per tid in `/proc/<pid>/task` and cache the fds; never open counters for tids that appear later. `inherit=1` folds every thread created after that by a counted thread (so every later thread of the process) into its parent's counter, and a counter of an exited thread stays readable. Opening counters for new tids as well would count those threads twice.
- Linux counts user space only, so Linux and Mac instruction numbers are not comparable; ratios to tmux are.
- Fall back to no instructions (and say so in `meta`) when the syscall fails (paranoid level, no PMU).
- Run `python3 bench/perf/test_gate.py` and update the README Probes table.

### 4. Linux baselines (done, `f50c434e`)

W0 is the gate run against a binary built at `17e17115` (the gate did not exist there), from a tree
whose HEAD is `17e17115`, so `meta.git_sha` names the W0 commit:

```sh
H=$(python3 -c 'import socket; print(socket.gethostname().split(".")[0])')
git -C ~/dev/zz worktree add --detach ~/dev/zz-w0 17e17115
cp -R ~/dev/zz-perf-int/bench/perf ~/dev/zz-w0/bench/
cd ~/dev/zz-w0 && cargo build --release -p zz-cli
python3 bench/perf/run.py --zz target/release/zz_cli --stage baseline --w0 none --json /tmp/baseline-$H-17e17115.json
python3 bench/perf/run.py --zz target/release/zz_cli --stage baseline --w0 none --quick --json /tmp/baseline-quick-$H-17e17115.json
cp /tmp/baseline-*$H-17e17115.json ~/dev/zz-perf-int/bench/perf/results/
cd ~/dev/zz-perf-int && just perf-gate wave1 --strict --json bench/perf/results/w1-5-fold-$H-$(git rev-parse --short=8 HEAD).json
```

Run on a quiet host (1-minute load / CPUs under 0.5), nothing else building. The last line is the
`--baseline` for the attach merge. Commit the three JSONs on `perf/wave1`, then remove `~/dev/zz-w0`.
Footprint on Linux is `smaps_rollup` `Pss_Anon + Pss_Shmem + SwapPss` (falling back to `/proc/<pid>/status`
`RssAnon + VmSwap`), not `phys_footprint`: the `abs` MiB rules were set on the Mac and may read
differently here.

### 5. Finish W1-ATTACH (`~/dev/zz-attach`, branch `perf/attach`)

First bring the lane up to the integration head so its quick gates use the Linux probe (step 3)
and find the Linux W0 (step 4): `git -C ~/dev/zz-attach merge perf/wave1`. Without it the lane has
the old probe (no `instr` twins) and no `baseline-quick-<host>` W0, so the `--w0` default finds
nothing and the vs-W0 rules and the drift check do not run.

`attach-review.json` holds the implementer report, both reviews and `findings_status`. Status is
judged from the fix-pass diffs (`46510b07`, `6e475f57`, `08186223`, `73eafb94`); none was verified.

| # | Review | Sev | Finding | Status |
|---|---|---|---|---|
| 1 | parity | major | queued patch stands in for a Full after a viewport wipe; pane blank or stale | fixed `46510b07`, verified on Linux (+4 tests) |
| 2 | parity | minor | same-size SIGWINCH no longer repaints | fixed `6e475f57`; a cell-size reply no longer fires `client-resized` |
| 3 | parity | minor | as-built notes: only zz-client core wipes viewports on Attached; "no epoch needed" is wrong | fixed (doc) |
| 4 | perf | major | themed clear + ECH on every blank row is half the p1 attach bytes | fixed: p1 504 B, p4 1449 B on Linux |
| 5 | perf | major | placeholder status row and borders painted, then repainted 0.2 ms later | fixed, tty captures show no byte painted twice; the hello no longer carries a pre-attach status |
| 6 | perf | minor | 8 KiB macOS AF_UNIX send buffer caps writev batches | fixed on Linux (6-7 writev per attach, buffered reads both ends; `SO_RCVBUF` does nothing here); macOS unmeasured |
| 7 | perf | minor | 7 pane wakes per attach+detach, redundant same-geometry resize | fixed `08186223`, cannot go stale (read) |
| 8 | perf | minor | writer thread spawned per TUI connection | fixed: 41 -> 23 distinct daemon threads over a 6 s attach loop |
| 9 | perf | minor | update-environment glob match on every attach | fixed: literal names read by lookup |
| 10 | perf | minor | status render for the detaching TUI and other-lane work in the attach cycle | fixed for the lane; the rest is in the plan's hand-on list |
| 11 | perf | minor | kitty graphics probe + query burst, 282 B per attach | fixed: probe only for known kitty terminals or on first need |
| 12 | perf | minor | headline numbers misattributed (wire_s2c came from PANE/EXEC, 0.375 KiB/s did not reproduce) | fixed (doc, Linux numbers) |

Also: `ZZ_PERF_ATTACH_BATCH` is missing from the Rollback switches table; the lane adds ~73 new
comment lines in `.rs` files (house rule: no comments); web build never run (`just web-build`);
iOS build is Mac-only; the full `--strict` gate was never run for this lane (only a quick gate
against the full w1-4 JSON, which warns that sample counts differ).

Then: lane tests (`cargo test -p zz-daemon attach_tests`, `-p zz-tui -p zz-client -p zz-cli`),
`python3 compat/tui/tracker.py check`, every `compat/tui-*.sh` fixture, `compat/attached-client.sh`,
the scenarios the parity reviewer listed, and `just perf-gate wave1 --quick --only
attach,echo,throughput,chatty --baseline bench/perf/results/baseline-quick-<host>-17e17115.json`
(quick against quick; a quick run against the full w1-5-fold JSON warns that sample counts differ).
`tui-output-backpressure.sh` reads `/proc` and runs only on Linux. These failed on the base build
on the Mac too, so compare each against the pre-merge binary before blaming the lane: fixtures
`status-row`, `tui-choosers` (stops at the same checkpoint on `157ac6a3`), `tui-client-commands`,
`tui-stock-keys`, `tui-output-backpressure`, `tui-command-streams`; corpus `census-hooks`,
`plugin-runtime-continuum`, `plugin-runtime-vim-tmux-navigator`, `source-file-byte-name`,
`resurrect-save`, `status-background-jobs`.
`scripts/wave-lanes.js` is preset for exactly this (fix from the reports, then merge).

### 6. Merge attach as w1-6 (done, `ce1b34cd`)

Merge `perf/wave1` into `perf/attach` again if it moved since step 5, then merge `perf/attach`
`--no-ff` into `perf/wave1`, full checks (fmt, clippy, workspace tests under `timeout`,
`just compat-check`, full `compat/run.sh`, `compat/attached-client.sh`), gate JSON
`just perf-gate wave1 --strict --baseline <w1-5-fold JSON> --json
bench/perf/results/w1-6-attach-<host>-<sha8>.json` on a quiet host. Remove `~/dev/zz-attach` and the branch right after.

Done 2026-09-29 on alienware. The merge was clean (`perf/attach` already held `a41b1fbf`) and its
tree equals the lane's. fmt, clippy, `just compat-check`, `just web-build` and
`compat/attached-client.sh` pass. Workspace tests: only the known base failures plus load victims
that pass alone (`process_info` exec-name tests, a `russh_socks` port test,
`mode_keys_scope_visible_command_output_separately_from_underlying_copy_mode`, see Known open
bugs). Full `compat/run.sh`: three rows red twice, all identical on the pre-merge binary
(`lane2-store` and `show-options-hooks`: zz's `lock-command` default is `lock -np`, the oracle
tmux built here has `vlock`; `smoke/plugin-runtime-resurrect-restore` fails for tmux too, bash
prompt retitles).

Gate `w1-6-attach-alienware-ce1b34cd.json` against `w1-5-fold`: every attach rule passes
(`tty_total` 175762 -> 504 B at p1, 344894 -> 1435 B at p4; `instr` 15.8 -> 10.7 and 16.3 -> 11.1
Minstr; `ttfc` 8.3 / 9.2 ms vs tmux 7.9 / 9.6; `conns` 4 -> 2), `chatty.tty_kibps.hidden` 1.00 ->
0.54 KiB/s. Rows that moved for reasons outside the lane: every `mem.footprint.*` row passes now
because of the THP fix (`ad4ee99c`, after the fold JSON): the pre-merge `a41b1fbf` binary reads
3.18 / 17.9 MiB against the merge's 3.14 / 18.1. The `spawn`, `cold`, `config` and `cli` CPU and
wall rows dropped 2-3x with tmux dropping the same (tmux `spawn.cpu.split_shell` 2.18 -> 0.79 ms,
`config.wall.source_1000` 25.6 -> 14.5 ms) and their instruction counts unchanged: the host ran in
a faster state than during the fold run, so `spawn.cpu.kill_pane` and `config.*.source_1000`
passing is not a lane result, and both can fail again in the slow state. Still failing, none owned
by ATTACH: `spawn.cpu.split_shell`, `split_empty_P`, `new_window` (Linux kernel time per pane,
W3), `chatty.cpu_pct.visible` (W2-TERM, W4), `mem.threads.p20` 66 (W3-SHARDS), the four echo rows
(no wave-1 owner, same on the pre-merge binary in an A/B), `throughput.detached.ascii` (Linux
ceiling, W1-LINUX-PAGES) and `throughput.attached.ascii_ms` 0.53x (rule 0.25x).

### 7. Wave-1 exit (done on Linux 2026-09-29, `wave1-alienware-aaaa8195.json`)

Strict, full, quiet (load 1.4-1.5): 59 pass, 7 fail, 0 drifted; the 10 regressed rows are cpu/wall
kinds whose tmux moved the same way (this laptop flips between a fast and a slow power state, and
cold-start samples are bimodal, 4-5 ms and 13-15 ms, for both muxes); instructions, bytes,
memory and threads did not regress. Failing rows and owners: `spawn.cpu.split_empty_P`,
`spawn.cpu.new_window` (about 2x tmux in kernel time; W3), `cold.wall.new_session` (bimodal noise:
two reruns pass at 0.68-0.83x tmux), `chatty.cpu_pct.visible` 2.5x (W2-TERM, W4),
`mem.threads.p20` 66 (the per-pane Linux gather thread; W3-SHARDS), both throughput rows (pass
the ceiling rule only with the unpushed PageList fork fix). `tui-screen-diff.sh`: 147/147 in three
release runs and one debug run after W1-ATTACH-PAINT; `attached-client.sh` passes. The Mac strict
run (`wave1-macbook-<sha8>.json`) is still owed by the owner.


On a quiet machine: `just perf-gate wave1 --strict --baseline <w1-6 JSON> --json
bench/perf/results/wave1-<host>-<sha8>.json`, plus `compat/tui-screen-diff.sh` and
`compat/attached-client.sh` (PANE drops unwatched frames, ATTACH drops duplicates: together they
could blank a pane). Settle the echo decision first. The owner runs the same strict gate once on
the Mac for `wave1-macbook-<sha8>.json`.

### 8. Merge `perf/wave1` into `main` (done 2026-09-29, `1e0bfc6a`, local only)

`git merge --no-ff perf/wave1` in a main worktree nobody else is using. Then remove
`~/dev/zz-perf-int`. Do not push unless the owner says so; do not tag (freeze).

### 9. Wave 2, in the plan's order

Create the integration worktree again from the new main (the script hardcodes
`INT = ${ROOT}/zz-perf-int`): `git -C ~/dev/zz worktree add -b perf/wave2 ~/dev/zz-perf-int main`.
Lanes: W2-TERM, W2-CTRL (after TERM, or one lane with
TERM), W2-HOOKS, W2-FMT, W2-COPY. `LANES` for `scripts/wave-lanes.js`:

```js
const INT_BRANCH = 'perf/wave2', WAVE = 2, STAGE = 'wave2'
const BASE_JSON = `bench/perf/results/wave1-${HOST}-<sha8>.json`
const LANES = [
  { id: 'W2-TERM', slug: 'term', n: 1, groups: 'attach,echo,throughput,chatty' },
  { id: 'W2-CTRL', slug: 'ctrl', n: 2, groups: 'attach,control,cli,chatty,echo', after: 'term' },
  { id: 'W2-HOOKS', slug: 'hooks', n: 3, groups: 'cli,spawn,config' },
  { id: 'W2-FMT', slug: 'fmt', n: 4, groups: 'cli,config,chatty,statusjob' },
  { id: 'W2-COPY', slug: 'copy', n: 5, groups: 'mem,throughput' },
]
```

HOOKS and FMT both edit `format_hook_facts*` in daemon.rs; expect a conflict at merge 4. COPY must
first add the copy-mode entry group to the gate (README "Not built yet"). TERM and CTRL change the
wire: after pulling either merge, `kill-server` every dev daemon, and run `just web-build` here and
`just ios-gpui iPad build` on the Mac.

## macOS-only checks

| Check | Why it needs the Mac |
|---|---|
| posix_spawn pane launcher (W1-PANE) | XNU needs the tty-claiming launcher; Linux forks |
| `clonefile` tmux wrapper pin (W1-EXEC) | Linux uses `/proc/<pid>/exe` |
| macOS PTY spin bridge (`fix/pty-bridge-adaptive-spin`), 1 KiB PTY reads | Darwin PTY behaviour |
| CoreFoundation-free link (`otool -L` on the `dist/` binary) | Mach-O |
| kqueue / `EVFILT_PROC` paths in W3-SHARDS and W3-LOOP | Linux uses epoll / pidfd |
| `ri_instructions`, `ri_phys_footprint` numbers comparable with W0 | Mac probe |
| 8 KiB AF_UNIX send buffer effects (attach writev) | `net.local.stream.sendspace` |
| `just ios-gpui iPad build` in every wire lane gate | Xcode |

Rule: every wave exit gets one `--strict` gate run on the Mac, committed as
`<stage>-macbook-<sha8>.json`, next to the Linux one.

## Orchestration that worked

### Lane brief rules (from 2026-10-01)

Why: the first wave-2 impl runs (W2-CTRL, W2-FMT) ran 13.5 h each and never finished on their own.
Codex session timestamps: 87-92% of the wall was model time (1,580-1,800 steps per main thread,
median 10 s, p90 55-70 s, about 105k input tokens re-read per step, 12-18 compactions, the design
doc re-read 64-70 times); each lane's codex forked 3 helper agents into the same worktree and
target/ (10-28 cargo lock waits each, 34 dirty files at stop); the lanes wrote about 3,000
receipt/audit/provenance files (4 GB); FMT spent its last 1.5 h on rows owned by W2-CTRL. Under the
rules below, reviews and fixes took 15-36 min each.

Tooling in `bench/perf/campaign/scripts/`: `lane-briefs.py` (kinds `slice`/`impl`, `parity`, `perf`,
`fix`, `focus`; host-aware; pastes the lane section and a map), `lane-run.sh` (launcher: codex with
`--json`, `--output-schema lane-answer.schema.json`, `-o`, `< /dev/null`, the watchdog, a cost
summary in `<out>.cost.json`, `<out>.failed` when no valid answer), `lane-watchdog.py`,
`lane-cost.py`, `merge-checks-mac.sh`, `merge-checks-linux.sh`, `ab-compare.py`.

1. One commit per brief. W3-LOOP gets a brief per step e0, a, b, c, d, e of its plan; W3-SHARDS is
   sliced the same way before launch (shard threads and wake fd; actor state machine; own process
   spawn; search thread; Linux gather fold; ConPTY reader). Each brief has a numeric done
   criterion, the exact gate commands and a 90 min budget (impl at most 3 h). Review and merge each
   slice into the lane branch before writing the next brief.
2. Small context. Before each brief build a file:line map at the lane base (`rg -n "fn <name>"`
   over the write zone, or an Explore subagent) and pass it as `map`; the generator pastes the
   lane section. Briefs tell codex not to open `daemon-perf-rebuild.md` or this file. State file
   under 60 lines.
3. Structured answers: every run goes through `lane-run.sh`. A run without a valid answer counts as
   failed: write its report from git log and send the leftovers as a fresh `focus` brief with the
   full fix list. Never `--resume`.
4. Watchdog per lane (`lane-run.sh` starts it; every 5 min) kills the codex tree and every process
   whose cwd is inside the worktree when: the wall budget is spent; no new commit for the idle
   limit (60 min impl, 30 min fix/focus, 45 min for a fix whose done criterion includes perf
   runs; briefs say to commit a checkpoint before each measurement series, because the first
   SHARDS s2 fix was stopped mid-series with all its work uncommitted); a helper agent appears (a subagent rollout in
   `~/.codex/sessions` for that cwd; `features.multi_agent=false` does not remove the tools in
   codex 0.159, so the brief also forbids them); a file changes outside the write-zone globs (plus
   the design doc); a file is added under `third_party/` or a vendored crate; more than 50 or 100
   MB of untracked files. It writes the reason to `<out>.watchdog`. Launch `lane-run.sh` from
   outside the worktree.
5. Cheaper iteration: briefs say `cargo check -p <crate>` and focused tests while iterating,
   clippy and the wider tests once at the end; every agent builds into its own target (a review
   clones one with `cp -c -R` / `cp -a --reflink=auto`). One compiling lane per host, both hosts
   busy: SHARDS on alienware, LOOP on the Mac.
6. Effort: ultra for parity, perf and fork reviews (source-only, so they can run beside a
   compiling lane); high for impl and fix; medium for repins, doc updates and renames.
7. OpenAI's cyber filter cut runs that wrote madvise/footprint probes. SHARDS (fork, setsid,
   controlling terminal, closefrom, execve) and LOOP (signal handling) are likely to trip it:
   phrase those briefs as terminal emulator process spawning, never ask codex for raw-syscall
   probes (run probes yourself and paste the numbers), and if a slice is flagged once, give it to
   an Opus subagent instead of retrying codex.
8. The orchestrator never ends a turn while a lane runs: wait on the report file (`<out>.exit`) with
   a background `until` loop, never on `pgrep -f <brief>` over ssh (it matches itself; write
   `[c]odex`). `lane-run.sh` records wall time, steps and tokens per run.
9. Kept from before: one ultra review per fork change (it caught the Ghostty trim bug); rerun
   every gate codex claims; a fresh brief per fix round; rows outside the lane's groups or red on
   the base are listed, not chased; one scratch JSON per cited gate run, nothing in
   `bench/perf/results` from lanes.
10. Merge checks per lane (`merge-checks-*.sh <name> <pre-binary> <A/B groups> <compat rows>`,
    `WIRE=1` for wire lanes on the Mac): fmt, workspace clippy and tests with solo reruns, `just
    compat-check`, the lane's compat rows, web build, attached-client, TUI screen diff and the iPad
    build for wire lanes, three alternating quick A/B pairs (`ab-compare.py <dir>`). Wire and
    command-path lanes add the config rows (`smoke/own-conf smoke/config-grammar
    smoke/source-file-diagnostics smoke/source-replay-diagnostics smoke/args-parse-*
    smoke/oh-my-tmux smoke/tpm-init`): CTRL's 35 control rows missed ten config regressions that
    only the wave-exit corpus found. Full corpus (5400 s timeout) and strict gates on both hosts
    at wave exit.

- Linux leg: lanes run as Agent-tool subagents from brief files generated by
  `~/.cache/zz-perf/prompts/gen.py` (impl, parity and perf reviews, fix, merge); reports go to
  `~/.cache/zz-perf/<slug>/`. `~/.cache/zz-perf/quiet-gate.sh` pauses other trees' compiles
  (SIGSTOP, always resumed) while a gate or timing fixture runs. The integration worktree
  `~/dev/zz-perf-int` stays put and switches branch per wave (`perf/wave2` from `main`).
- Owner rule (2026-09-29): commit and merge finished work back to `main` right away; no ghost work
  left in worktrees or side branches. After each wave-2 merge into `perf/wave2`, `main` is
  fast-forwarded to it. Nothing is pushed without the owner.

- Mac leg: codex (`gpt-6.1-sol`, reasoning `ultra`, `service_tier` `priority`) did the Ghostty
  review, the fix and the repin from brief files, and I reviewed and reran every gate. Run it as
  `codex exec -m gpt-6.1-sol -c model_reasoning_effort='"ultra"' -c service_tier='"priority"' -s
  <sandbox> -C <dir> -o <report.md> "Read <brief> and do exactly what it says." < /dev/null`: without
  the `/dev/null` stdin a backgrounded run waits on stdin forever. OpenAI's cyber filter ended a
  review that wrote an madvise / `phys_footprint` C probe; a source-only review with the
  measurements pasted into the brief went through. The review of the fork commit found the
  blocker that every test had missed: give each fork change one such review.
- One integration worktree (`~/dev/zz-perf-int`, branch `perf/waveN`), one worktree per lane (`~/dev/zz-<slug>`, branch `perf/<slug>`) from the integration head.
- Per lane: implementer at `xhigh` effort, then a parity reviewer and a perf reviewer in parallel at `high`, then a fix agent at `high`. All return structured JSON (schemas in the script); keep those reports outside the repo, they are the resume point.
- Merges strictly serial in the plan's order (wave 1 swapped FORMAT and PUBLISH, see above): merge the integration head into the lane, `--no-ff` into integration, fmt, clippy, workspace tests, `just compat-check`, full `compat/run.sh`, `compat/attached-client.sh`, then a full `--strict` gate JSON `w<wave>-<n>-<slug>-<host>-<sha8>.json` on a quiet host whose `--baseline` is the previous merge's JSON.
- 2 lane slots at a time spread token use (the first wave-1 run used 3 and hit the usage limit); remove a lane worktree and branch right after its merge.
- Template: `bench/perf/campaign/scripts/wave-lanes.js`. Edit only the constants block at the top (`ROOT`, `MAIN`, `INT`, `INT_BRANCH`, `HOST`, `WAVE`, `STAGE`, `BASE_JSON`, `QUICK_BASE_JSON`, `CLONE`, `PROFILER`, `SLOTS`, `LANES`). Lanes take `start: 'impl' | 'review' | 'fix' | 'merge'`, `reports` (a file in the integration tree) and `after` (a lane slug whose merge must land first). Run it with the Workflow tool (load the `workflow-authoring` skill first).
- `scripts/wave1-resume.js` is the run that resumed wave 1 after the usage-limit cut, kept as the example of RESUME notes per lane (paths moved into constants, logic unchanged; its `execDone` line uses `&&` on a promise, so EXEC did not actually wait for PANE).

## Traps

- Waiting on a remote agent with `ssh host 'pgrep -f <brief>'` never ends: the pattern matches the
  ssh shell's own command line. Wait on the report file, or write the pattern as `[c]odex`.
  This cost a night (2026-09-30 23:38 to 10-01 08:57).
- `compat/tui-screen-diff.sh` and the attached checks need a UTF-8 locale: without `LANG`/`LC_ALL`
  the pinned tmux draws ACS borders and the run reports diffs and an unsettled wide-glyph screen.
- `compat/attached-client.sh` can fail on the tmux side ("tmux command-output view did not show
  one ordered replay transcript") on a loaded Mac; it passed 1 of 2 reruns at `f6a25887`.
- The Mac lane worktrees left an orphaned `run-tui-fixtures.py` from a killed codex run alive for
  6 h 40 min; after killing a lane, sweep `ps` for its `/tmp/zzpc` runners too.

- `just compat-check` on the Mac fails `verify_claims_test.py` (unbound `unattributed`) when
  `/bin/bash` 3.2 comes first in PATH, on main too: run it with `/opt/homebrew/bin` first.
- A Linux check started with `setsid nohup` over ssh gets `ulimit -n` 2048: the daemon suite then
  fails about 140 tests with "Too many open files". Raise it to the hard limit (2097152) first.

- **Stale paint slips past a single fixture run.** At the wave-1 exit, `compat/tui-screen-diff.sh`
  on the release build showed 13-14 of 147 checkpoints with stale pane or status rows per run
  (1-6 on a debug build; the pre-ATTACH release build matched all 147 every time), while the lane
  had reported it green. `ZZ_PERF_TUI_COALESCE=0` clears it, so it sits in zz-tui's coalesced
  paint path. Merge checks now run screen-diff three times on the release build (`gen.py`).
  Fixed by W1-ATTACH-PAINT (`aaaa8195`). Under a concurrent cargo build a few `unzoom`
  checkpoints can still differ on both the pre- and post-ATTACH builds: the daemon holds a stale
  pane geometry after unzoom (likely a client size report landing after the layout change,
  `InputMessage::ResizeTerminal` -> `set_pane_geometry` with no layout generation). Open bug below.

- **Ghostty's unit tests never reach the OS.** `mem.zig` skips madvise under `builtin.is_test`,
  so a green PageList suite says nothing about the decommit and recommit paths. Rows own cell
  blocks by offset, not by index: `eraseRows` swaps row headers, so no code may derive a cell range
  from `size.rows`. The repro for a trim that loses cells is zz-terminal's
  `a_cleared_screen_survives_idle_compression` on Linux.
- **Spotlight on the Mac.** A new worktree or an APFS clone of a 5-30 GB `target/` sets `mds`
  indexing for about ten minutes (load 18-30). `touch <worktree>/target/.metadata_never_index`
  before cloning into it.
- **Never `git stash`**, `reset --hard`, or path checkouts in a tree another session uses. To revert your own edit, re-edit. Keep the index empty.
- **`isolation: worktree` fails** in this repo (`.claude -> .agents` is a committed symlink). Create worktrees by hand with `git worktree add`.
- **Warm targets.** macOS cloned `target/` with APFS `cp -c`. On Linux `cp -R --reflink=always` works on btrfs, XFS with reflink, bcachefs; on ext4 it fails, so build fresh (about 10 min) or use sccache. Never let two builds share one `CARGO_TARGET_DIR`: the W1-ATTACH parity reviewer's probe overwrote the lane's test binary that way.
- **compat/run.sh** needs bash 4+ and an absolute `ZZ_COMPAT_ZZ`; a relative path breaks the in-pane tmux wrapper (7 false failures). Its summary used to print "Nothing failed on the first pass" even when a row failed twice; since 2026-09-29 it lists rows that failed twice on their own line.
- **Flaky daemon tests.** A zz-daemon failure under the full workspace is re-run alone first. Known load victims: `russh_socks` port tests, `terminal_agent_and_editor_panes_inherit_live_working_directory` (hostname), copy-mode `mode_keys_*` and `configured_v_e_y_*`, `history_request_*`, `kitty_images_and_placements_*`, `client::tests::stdin_read_restores_signal_disposition_*`. `concurrent_default_interactive_attaches_atomically_share_session_zero` fails with "not a terminal" on headless hosts, and its panic can be blamed on a neighbour. Never run the daemon suite while a live probe daemon runs.
- **Hung tests are bugs.** In W1-PANE `cargo test -p zz-daemon --lib` hung 30+ min in `mode_keys_retarget_active_command_output_and_restore_the_previous_table` (spinning in `ModeRevision::first_char`/`clamp_point`); it was a real ordering bug in the lane. Run suites under `timeout`, look at anything over 60 s, and after killing a run kill leftover `target/debug/deps/zz_daemon-*` binaries: they hold PTYs (macOS caps at 511; check `/proc/sys/kernel/pty/nr` against `pty/max` here).
- **Usage-limit cuts.** The first wave-1 run died mid-merge and mid-lane with uncommitted work. Resume from worktree state: `git log`, `git status`, `git diff` per worktree; pass what is done as a RESUME note; never redo finished work. Commit checkpoints early (that is why `73eafb94` exists).
- **Stale daemons.** The daemon outlives clients; after a wire-changing merge, `kill-server` every dev daemon. Test sockets go directly under `/tmp` (`sun_path` limit).
- **Wall and CPU noise.** Parallel lanes make wall and CPU time noisy; judge on instructions, bytes, counts, footprint and threads, and run merge-of-record gates with `--strict` on a quiet host.
- **Compat rows red on the Mac only**: `smoke/resurrect-save` (macOS `/bin/sh` runs as bash, tmux fails it too), `smoke/plugin-runtime-continuum` (gnubin `sleep` is `gsleep`), `smoke/status-background-jobs` (BSD `date` lacks `%N`), `verify_claims_test.py` under `/bin/bash` 3.2. Expect them to pass here; if not, compare with the pre-merge binary before blaming a lane.
- **The web client has its own lockfile.** A lane that adds a dependency to a crate `clients/web` builds (zz-protocol, zz-client, zz-mux, zz-ui) must refresh `clients/web/Cargo.lock` too (`cargo metadata --offline --manifest-path clients/web/Cargo.toml`), or `just web-build` fails at `cargo metadata --locked` with a Python JSON traceback. W2-HOOKS missed it (`74f35b65`).
- **`-p` and `--workspace` build different features.** smallvec's `union` feature is on in the
  workspace and off for `cargo test -p zz-terminal`, so a `size_of` contract over a `SmallVec`
  holds in one and fails in the other (W2-TERM's `TerminalPatchRowData`, fixed in `8f18e4d7`).
  Lanes that add layout asserts should run the crate's tests both ways.
- **A hidden view keeps its last snapshot.** `TerminalSession::latest_viewport_for` still returns
  the frame the actor built before the daemon turned the view's stream off, until the actor's next
  publish. The daemon never sends it (the pane is not streamed), so a test that compares a
  client's retained frames with it must skip panes the client does not stream
  (`daemon/pane_frame_tests.rs` `settle`).
- **Pane-title rows under compile load.** `command-item-format` and `new-session-cwd` in `compat/run.sh` read `#{pane_title}` right after `new-session`; with a cargo build running they can see the shell's first title (zz `bash`, tmux `user@host:cwd`) and fail once. They pass alone; W2-HOOKS merge saw both clean in three quiet reruns and on the pre-merge binary.

## Known open bugs

| Bug | Where | Fix idea |
|---|---|---|
| Stale pane geometry after unzoom under load: a client's size report can land after the layout change. At the W2-TERM merge, with a game loading the host, `tui-screen-diff.sh` on the release build showed 1-4 differing `unzoom` / `resized` / `restored` checkpoints in about a third of the runs on the pre-merge, lane and merge binaries alike (merge 6 of 18, pre-merge 4 of 14, lane 2 of 5) (`~/.cache/zz-perf/term/merge/sdab*.log`) | daemon `InputMessage::ResizeTerminal` -> `set_pane_geometry` | tag size reports with a layout generation and drop stale ones (protocol change; W2-CTRL or its own lane) |
| Title race: the pane watcher can see a program's new title before `program_title_writes` moves, so a `select-pane -T` title then hides it until the next viewport publish | zz-terminal `run_terminal` publishes facts at the top of each pass, after the viewport; daemon `watch_terminal` | set facts right before `publish_active_views` |
| `verify_claims_test.py` "unattributed: unbound variable" under bash 3.2 | `compat/tui/` | Mac-only; empty array under `set -u` |
| `mode_keys_scope_visible_command_output_separately_from_underlying_copy_mode` takes 30.05 s alone and fails under load: the Escape never closes the output view, it closes when the window's `sleep 30` exits, just inside the test's 30 s wait (with `sleep 50` it fails at 30 s). Same on `a41b1fbf` | daemon.rs test and the command-output Escape path | find why the emacs-table Escape does not cancel the output view; then the test stops depending on the sleep |
| `lock-command` defaults to `lock -np` on every platform; tmux's configure picks `vlock` on Linux when `vlock` is installed at build time (it is on alienware), so `lane2-store` and `show-options-hooks` are red in `compat/run.sh` here | zz-mux `tmux_options.rs`, `command.rs` | decide whether zz follows tmux's build-time probe; the pinned oracle's answer depends on the build host |
