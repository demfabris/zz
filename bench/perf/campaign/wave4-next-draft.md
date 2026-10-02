## Next session: wave 4

1. Wave 3 closed 2026-10-02 at `e9bc174c` (W3-SHARDS, W3-TUI, W3-LOOP, and twelve fix lanes run
   as Opus subagents; see "Wave 3 merge log"). Exit gates `wave3-macbook-e9bc174c.json` and
   `wave3-alienware-e9bc174c.json` (strict, full, `--baseline wave2-<host>-3d0fc1b0.json`).
   Against wave 2: threads p20 25 -> 5 Mac / 45 -> 25 Linux, footprint p20 -36% / -20%,
   status-job threads 3/s -> 0 and instructions -26% / -46%, kill-pane instructions -26% Mac,
   copy entry CPU -17 to -44%, control output +18 to +88%. No unexplained regression: see the
   exit entries in the merge log for each flagged row.
2. Wave 4 in merge order: W4-DELIVER, W4-ROWS, W4-BINARY, gate `--stage final`
   (`knowledge/designs/daemon-perf-rebuild.md`). What still loses to tmux at wave-3 exit, all
   W4-DELIVER unless noted: echo p50/p99 1.8-2.6 ms against 0.92-1.13 on Linux (Mac p50 0.59-0.90
   against 0.26-0.33), `control.latency` 1.2-1.5x, `attach.cpu`/`attach.ttfc`, `chatty.cpu_pct.visible`
   2.2x on Linux, `mem.threads.p20` 25 on Linux (one gather thread per pane; the final gate wants
   12 or fewer), `spawn.cpu.*` 1.6-1.8x on Linux (spawn path, not owned yet: give it to
   W4-BINARY or a spawn lane), `control.burst_cmds_per_s` 0.83x tmux.
3. Deferred: W3-LOOP step (d), the Mutex removal (570 production lock sites, at most 0.8% of
   instructions, uncontended after e21). Plan and census in `bench/perf/campaign/w3-loop-d-plan.txt`
   (10 slices); run it as its own lane after W4-DELIVER, whose shard delivery removes most of
   the cross-thread locking it would otherwise have to rework.
4. Open items found in wave 3 (none blocks wave 4):
   - A command handed off from inside a nested list (an `if-shell -F` body, a prompt template)
     runs after the rest of that list; tmux runs it in place (`loop_handoff.rs`).
   - `wait-for` and agent waits (`wait-pane`) bound to a key are not converted to continuations.
   - Three compat rows fail on every build here against the pinned tmux oracle
     (`smoke/control-alias-prepare`, `if-shell-background-order`, prompt history): a separate task.
   - Mac rows red on the wave-2 binary too: `census-hooks`,
     `smoke/plugin-runtime-vim-tmux-navigator`, `smoke/source-file-byte-name` (plus the known
     tmux-also-fails rows). Linux: `smoke/format-modifier-client-loop` red with the debug build
     only.
   - zz-daemon lib tests: 5-25 timing failures per parallel run on either host, all pass alone;
     `ZZ_PTY_SHARDS` 4, 16 or 0 makes no difference. zz-terminal `search_worker_tests` counts
     named threads through /proc and is not isolated like the daemon thread-count tests.
   - `endpoint::tests::remote_scripts_fall_back_to_the_mac_app_bundle_cli` returns early when a
     zz sits in /opt/homebrew/bin or /usr/local/bin.
   - Measurement: `cli.instr.display.p20` and `cli.wall.version.p1` are bimodal on every binary
     and on tmux; `chatty.tty_kibps.hidden` swings 30-50% between identical runs.
   - Cheap instruction wins seen and not taken: `after_command_hook`'s binary search (~0.9% of
     source_1000), repeated `canonical_command` lookups (~3.6% with `resolve_command`), the
     per-command `format_variables` clone in `execute_with_mux_source_inner`, Linux chain5 and
     list_keys +1 to +1.6% from `CommandTask::new_with_log`.
5. Lanes are Opus subagents now (owner decision 2026-10-02): `~/.claude/agents/lane-impl.md`
   (`model: opus`, `effort: xhigh`) with a `lane-briefs.py` brief, one worktree each, a warm
   target cloned with `/bin/cp -c -R`, Linux work over ssh. Twelve lanes took 14 min to 2 h 12 min.
   Wall-time measurements on alienware go through `~/.cache/zz-perf/quiet-gate.sh` (pauses other
   lanes' compiles under ~/dev); a binary copied to tmpfs `/tmp` there inflates the footprint
   rows. The wave-exit corpus found what 36 merge rows did not (two W3-LOOP parity regressions):
   run the full corpus before calling a wave done.
6. Release freeze until wave 4 exits; protocol stays 107.
