# Daemon perf campaign: handoff

Entry point for a fresh session continuing the zz daemon performance rebuild. Written 2026-09-29 on
the macbook, continued the same day on the Linux host alienware (see "Linux leg"). State on
2026-10-02: waves 0 to 3 are on `main` and pushed. Wave 3 (W3-SHARDS, W3-TUI, W3-LOOP and nine
Opus fix lanes) closed at `e9bc174c`; its exit gates are `wave3-macbook-e9bc174c.json` and
`wave3-alienware-e9bc174c.json`. The Ghostty fork pin is `189df4a1` (row cell copy, branch
`zz-2026-10-02`, on `67351380`: trim fix `c3941417`, copy snapshots `7823f65d`, used-size active
page copies); the libghostty-rs pin is `f5f82601`. No lane in flight, no lane worktree left.
Wave 4 started 2026-10-02 (see "Wave 4 merge log"); read "Lane brief rules" before launching anything.

## Next session: wave 4

1. Wave 3 closed 2026-10-02 at `e9bc174c` (W3-SHARDS, W3-TUI, W3-LOOP, and nine fix lanes run
   as Opus subagents; see "Wave 3 merge log"). Exit gates `wave3-macbook-e9bc174c.json` and
   `wave3-alienware-e9bc174c.json` (strict, full, `--baseline wave2-<host>-3d0fc1b0.json`).
   Against wave 2: threads p20 25 -> 5 Mac / 45 -> 25 Linux, footprint p20 -36% / -20%,
   status-job threads 3/s -> 0 and instructions -26% / -46%, kill-pane instructions -26% Mac,
   copy entry CPU -17 to -44%, control output +18 to +88%. No unexplained regression: see the
   two exit entries in the merge log for each flagged row. Pushed to main the same day.
2. Wave 4 in merge order: W4-DELIVER, W4-ROWS, W4-BINARY (closed unmerged, see the wave-4 merge log), gate `--stage final`
   (`knowledge/designs/daemon-perf-rebuild.md`). What still loses to tmux at wave-3 exit, all
   W4-DELIVER unless noted: echo p50/p99 1.8-2.6 ms against 0.92-1.13 on Linux (Mac p50 0.59-0.90
   against 0.26-0.33), `control.latency` 1.2-1.5x, `attach.cpu`/`attach.ttfc`, `chatty.cpu_pct.visible`
   2.2x on Linux, `mem.threads.p20` 25 on Linux (one gather thread per pane; the final gate wants
   12 or fewer), `spawn.cpu.*` 1.6-1.8x on Linux (spawn path, not owned yet: give it to
   W4-BINARY or a spawn lane), `control.burst_cmds_per_s` 0.83x tmux. Watch in W4-DELIVER: Mac
   `chatty.cpu_pct.steady` (ten hidden printing panes) is about 11% above loop-00 after
   normalizing by tmux, at +4% instructions, under both tolerances.
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
   - Mac `compat/attached-client.sh` fails on the tmux side under load: the copy-mode position
     indicator is captured half drawn (`[` with no `N/M]`), so the first transcript line does not
     match. It passes alone on a quiet Mac. Killing the fixture mid-run leaks its `zzai-*` tmux
     server; kill those by hand.
   - Binaries before `writestall` (loop-00 included) hang in a full-history `capture-pane -S -` of
     a 20-pane session: use `wave3-gate-cli` (`2ea96e66`) or later as an A/B base for that row.
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
   target cloned with `/bin/cp -c -R`, Linux work over ssh. Nine lanes took 14 min to 2 h 12 min.
   Wall-time measurements on alienware go through `~/.cache/zz-perf/quiet-gate.sh` (pauses other
   lanes' compiles under ~/dev); a binary copied to tmpfs `/tmp` there inflates the footprint
   rows. The wave-exit corpus found what 36 merge rows did not (two W3-LOOP parity regressions):
   run the full corpus before calling a wave done.
6. Release freeze until wave 4 exits; protocol stays 107.

## Wave 4 merge log (from 2026-10-02)

- Start, 2026-10-02 20:15. `perf/wave4` from main `06ea9cf1` in `~/dev/zz-perf-int` on both hosts
  (alienware gets it through `perf/wave4-mac`, then `--ff-only`: pushing a branch that a worktree
  has checked out is refused). `lane-briefs.py` points at wave 4 (`--stage final`, base
  `wave3-<host>-e9bc174c.json`, wave-3 reds listed, plain rollback switches instead of
  `ZZ_PERF_*`). Final-stage rescore of the wave-3 exit gates (`run.py --rescore <json> --stage
  final`): Mac 8 fails (`spawn.cpu.split_empty_P` 2.1x, `chatty.cpu_pct.visible`, five echo rows,
  `control.latency` 1.41x), Linux 20 (spawn CPU x3, chatty flip/visible, `mem.threads.*` x4 at
  25-26 against 12, `attach.cpu.p1`/`.p4` 1.4-1.48x, six echo rows, `control.latency` 1.52x,
  `control.burst_cmds_per_s` 0.83x, and the noisy `cli.wall.capture_history.p20`).
- Lanes launched at once as Opus subagents (`lane-impl`), briefs from
  `bench/perf/campaign/scripts/wave4-briefs.py` (copied from scratch `/tmp/zzpc/w4/`), lane check
  `/tmp/zzpc/w4/lanecheck.sh` every 15 min:
  - KNOBS (`~/dev/zz-knobs`, Mac): delete every `ZZ_PERF_*` knob and the code only it reaches.
    Decision: all 26 go now. The doc's rule deletes wave-N knobs when wave N+2 starts; the wave-1
    knobs were missed at the start of wave 3, so wave 1 and wave 2 go together. Merges first: it
    touches every lane's files and is the smallest.
  - W4-DELIVER plan (`~/dev/zz-deliver`, Mac): the section predates wave 3, so the first brief maps
    the as-built code and writes `bench/perf/campaign/w4-deliver-plan.txt` (slices, zones, done
    criteria, parallel groups, and whether control latency/burst and Linux attach CPU belong here
    or in a separate control lane). Slices follow the plan.
  - W4-BINARY (`~/dev/zz-binary`, Mac): the daemon-only executable and packaging; Linux build and
    ssh remote start are mine after the lane.
  - W4-ROWS (`~/dev/zz-rows`, Mac). Decision: profile now on the wave-4 base instead of after the
    DELIVER merge (DELIVER changes routing, not how a frame reads cells); it is re-measured on the
    DELIVER merge before it merges. A fork change is committed in a scratch Ghostty clone, not
    pushed; it gets one review, then I publish a dated branch and repin.
  - SPAWN (new lane, no doc section; the wave-3 handoff left `spawn.cpu.*` unowned): edits in the
    Mac worktree `~/dev/zz-spawn`, builds and measures in a detached alienware worktree of the same
    name (`git checkout --detach perf/spawn` after each push).
  - Decision: five lanes at once, four compiling on the Mac (owner, 2026-10-02: "bump the gas, we
    have usage"). Instruction, byte, count and thread rows are the signal while they build; wall
    and CPU rows are judged in quiet A/Bs at merge time.
- W4-DELIVER plan done in 21 min (`a1f6fe3a` on perf/wave4, cherry-picked from the plan lane):
  `bench/perf/campaign/w4-deliver-plan.txt` plus four probe scripts (`w4-deliver-*.py`). Wave 3
  already removed the `zz-pane-N` threads and (on unix) the control tap threads; a second client
  on a pane costs +6 Minstr/5 s against +515 for the first. Slices DL1 (current command, cwd and
  activity off the frame path), DL2 (per-pane stream sequence, encode once), DL3/DL3b (shard
  sinks), DL4 (shard writes the socket directly when the queue is empty), DL5 (%output as a sink
  kind and the per-pane sequence barrier), DL6 (Linux: one gather thread per shard, threads
  25 -> 9). Owners for all 28 final-stage fails. Two new lanes from it: C1 CONTROL
  (`control.latency`: the daemon is already cheaper than tmux per command, the gap is the
  `zz_cli -C` process, 9.5 us and 64 kinstr per command) and TUI-ECHO (the TUI client spends 105
  kinstr and 70 us per key; DELIVER alone gives 60-100 us of the 196 us the Mac echo p50 needs).
  Order: DL1, DL2 and DL4 start after KNOBS merges (it deletes knobs inside their functions); DL3
  and DL3b rebase on W4-ROWS; DL6 merges after SPAWN.
- Launched at 20:50: DL6 (`~/dev/zz-gather`, Mac edits, detached alienware worktree for builds),
  CONTROL (`~/dev/zz-control`, Mac; decision: relay variant first, the SCM_RIGHTS stdio variant
  allowed if the relay misses, behind `ZZ_CONTROL_RELAY=1`, since DL5 has not started and will
  rebase on it) and TUI-ECHO (`~/dev/zz-tuiecho`, Mac). Seven lanes in flight: five compiling on
  the Mac (load about 28 on 16 cores, 64% memory free), two building on alienware.
- KNOBS merged 2026-10-02 21:45 as `bce8d6a5` (lane 82 min, `539b41f9`, plus my `339d3c4a`):
  every `ZZ_PERF_*` knob gone, 49 files, +522/-1739. Kept on purpose: the daemon's legacy
  command path (gpui-shared `connection.rs` and `InteractiveClient` still send ClientHello,
  PrepareCommandList and CommandRequest, and the CLI falls back to it when a daemon refuses
  Exec), the format interpreter (format tracing uses it) and three test-only oracle switches with
  no env var (`with_eager_universe`, `with_borrowed_formats`, `compiled::with_enabled`), the mux
  engine's `set_automatic_rename_throttle` (the daemon always sets it on), `KeyTablesChanged`.
  Deleted fallbacks include the flat copy-mode capture (a paged grid always), whole-row diffs
  (`TerminalDiffScratch` 136 -> 128 bytes), `FormatHookFactsView`, the TUI waiting card and the
  per-connection threads on Windows; `run.py` no longer writes `meta.knobs`. Windows check:
  `DOCS_RS=1 cargo check -p zz-daemon --target x86_64-pc-windows-msvc` (skips the zig build) gave
  one new dead-code warning, `OutboundFrame::into_vec`, now `cfg(test)`: 17 warnings, as on the
  base. The lane's quick gate ran at load 25-30: `chatty.instr_per_s.hidden` 1.075x raw, 1.013x
  normalized by tmux over nine pairs; every cli, control and attach instruction row <= 1.017 at
  the minimum of three. Merge checks on both hosts follow (Linux A/B through quiet-gate).
- DL1 launched 21:50 on `perf/deliver` from `bce8d6a5` (`~/dev/zz-deliver` on the Mac, detached
  worktree of the same name on alienware).
- W4-BINARY closed, not merged (lane 93 min, `perf/binary` `b08a3565`, branch and worktree
  removed; the commit stays in the Mac reflog). The split works but moves `mem.rss.p1` only 12.20
  -> 11.72 MiB (2.87x tmux; gate 2x): the CLI-only code is 1.3 MB, the resident text is the
  daemon's own. Decision: about 1,000 net lines in 39 files (two executables in every recipe, a
  build-id handshake, a `/proc` fd pin for the tmux wrapper) is not worth 0.48 MiB of clean text
  pages while the footprint rows pass; `mem.rss.*` stays informational. The design doc's W4-BINARY
  section has the numbers and the order-file lever (8.3 MB idle, 2.10x) if RSS ever matters.
- W4-ROWS merged 2026-10-02 23:00 as `ebf23f86` (lane 102 min, `2a0e7d72`, plus my repin
  `15643e1b`). Row extraction was 54% of daemon samples in chatty visible, 25% in echo busy30:
  the fork call `ghostty_render_state_row_cells_copy` (Ghostty `189df4a1`, new branch
  `zz-2026-10-02` on `67351380`) copies a row into packed 12-byte cells with a style table and
  UTF-8 grapheme spans, no allocation, no thread; the wrapper exposes `CellIteration::copy_into`
  (libghostty-rs `zz-2026-10-02`: `d975339f` plus `f5f82601`). FFI calls per 180x50 frame 81,250
  -> 250; frame build 53.9% -> 24.7% of chatty-visible samples; `chatty.cpu_pct.visible` 3.48 ->
  2.18% on the Mac (now passes the final rule), `chatty.instr_per_s.visible` 0.33x, flip/hidden/
  steady 0.75-0.81x. Fork review (codex ultra, 13 min after a watchdog false start: pass the lane
  head, not the lane base, as the review's base): the C ABI is clean (6510 native tests, 73
  ReleaseSafe probes on limits, buffers and bad input); one blocker in the wrapper, inherited from
  `8e40135`: `RowIterator::update` and `CellIterator::update` did not tie their return lifetimes
  to the snapshot and row, so safe Rust could copy from freed render state (reproduced). Fixed in
  `f5f82601` (`'s` on both), then both branches published and repinned; zz-terminal 375 pass on
  the fetched pins. Open: copy-mode history capture (`ModeRevision::capture_flat`) still reads per
  cell and could use the same copy.
- TUI-ECHO merged as `127f09b1` in one batch with ROWS (lane 105 min, `ffcfc2c4`; done=false): the
  client waits on kqueue on macOS (select elsewhere and as the fallback), reads the socket once per
  wake, and the renderer tracks the terminal cursor and default style so an echoed key is 5 bytes
  instead of 63 (tmux writes 5). Per key 108.6 -> 92.1 kinstr and 64.5 -> 56.8 us; the 40 kinstr
  target is below the OS floor here (a minimal relay with the six syscalls per key costs 55.5
  kinstr, 25-29 us; the counter includes kernel work). echo.p50.idle did not move beyond noise.
  Out of its zone, left open: key message encoding in zz-protocol (12-15% of client user CPU per
  key), `ClientCore::claims_prefix_input` allocating a String per key (6%), and each frame copying
  the full 38 KB cell array (8%). Going below the floor needs tmux's shape (the server writes the
  client's terminal), the TUI analogue of CONTROL's stdio handoff.
- Merge checks run per batch from here (ROWS + TUI-ECHO as `batch1`): serial 35-minute suites
  per lane were the bottleneck with seven lanes; each lane already carries its own A/B.
- CONTROL (lane 73 min, `986abe33`, done for the Mac): `zz_cli -C` hands its stdin/stdout to the
  daemon over SCM_RIGHTS and the daemon writes plain command replies and `%output` straight to
  them; everything else goes back through the client's renderer. Mac `control.latency` 0.0338 ->
  0.0113 ms (0.81x tmux), burst 291k (1.69x), client 0 kinstr per command. A zero-work relay alone
  costs 47 kinstr, so the relay variant was skipped. Parity review running; Linux leg: cli tests
  280 pass, control_stdio tests and startup diagnostics pass, compat 6 of 7 (the known red).
- Incident 22:05-22:32: the CONTROL reviewer's probe daemon held 462 of the Mac's 511 PTYs (`cat`
  panes), so every other lane's tests failed with "PTY error: Device not configured"; stopped by
  message, lanes told to rerun. Lane briefs should cap probes at 50 panes.
- Incident on alienware: quiet-gate pauses every foreign compile, and a paused cargo holds the
  shared package-cache lock, so with four lanes plus my checks the solo reruns of merge checks
  (600 s including compile) timed out (KNOBS: `an_attach_repaints_a_dead_pane_kept_by_remain_on_exit`,
  `formatted_split_wait_resumes_its_pane_wait_on_the_loop`) and my CONTROL release build died.
  Instruction rows do not need quiet-gate; use it only for wall rows, one at a time.
- CONTROL parity review (Opus, 42 min): block integrity, escaping, kill-server, detach,
  pause-after, 17 EOF/blank-Return inputs and the relay fallbacks match; 2 majors (a SIGKILLed
  client with a full stdout pipe blocks the loop up to 1 s in `ControlStdio::close` ->
  `flush_blocking`; after a client is killed the daemon keeps its fds and keeps writing
  `%output`, so the reader never sees EOF) and 5 minors (O_NONBLOCK left on the client's stdio if
  the daemon dies, a 5 s ack deadline, stderr diagnostic order, a new client against an older 107
  daemon fails instead of falling back, stdout WRITABLE left registered). Fix round launched.
  Two bugs found that predate wave 4: `kill-window` from a control client leaks the pane's PTY
  (3 ptmx fds per pane; this is what took the 462 PTYs) -> lane PTYLEAK (`~/dev/zz-ptyleak`);
  `refresh-client -f wait-exit`, blank line, EOF hangs (tmux exits 0) -> the CONTROL fix round.
- SPAWN (lane 141 min, `4d79a6d2`, done=false): Linux panes start with `clone(CLONE_VM |
  CLONE_VFORK)` on a 64 KiB stack instead of `fork()` from the multithreaded daemon (copy-on-write
  faults on the shard per pane 250 -> 4), the exec fence pipe is gone on Linux, `TIOCGPTPEER` for
  the slave, empty panes skip the spawn environment. alienware: `spawn.cpu.split_shell` 2.23-2.35
  -> 1.80-1.83 ms (tmux 1.29-1.35), `new_window` 2.40-2.52 -> 1.61-1.69 (tmux 1.20-1.40),
  `split_empty_P` 0.85-0.91 -> 0.82-0.87 (tmux 0.63-0.71); kernel time, not instructions
  (`split_shell` user instructions +7%, unexplained). Left outside its zone: the per-pane gather
  thread (DL6), `terminal_current_command` per publish (DL1), and `settle_unwatched`, which builds a
  full snapshot about 100 ms after spawn: about 50% of `split_empty_P` instructions (0.9 Minstr,
  214 us per split), owner DL3 (sinks) unless a smaller slice takes it first. A safety review of
  the CLONE_VM child (Opus: codex's cyber filter stops process-spawn reviews) runs before merging.
- batch1 Mac merge checks (ROWS + TUI-ECHO on `127f09b1`, A/B against the post-KNOBS binary): fmt,
  clippy, compat-check, web, iPad, tui-screen-diff, tui-copy-mode, tui-overlays pass; 5 workspace
  load failures pass alone; compat 31/35 (the four `known/*`); attached-client failed on the tmux
  side again (the load flake, load 25). Quick A/B: `attach.instr.p1`/`.p4` -50.6%/-44.6%,
  `chatty.instr_per_s.flip`/`.hidden` -28.7%/-26.4%, `chatty.tty_kibps.hidden` -23%; echo rows
  lower but the base runs were load spikes (p50 idle 1.27 ms), so they are not evidence.
- KNOBS on Linux (`bce8d6a5`): 44 solo reruns pass; compat 99/101 (`show-options-hooks`,
  `smoke/control-alias-prepare`, both red on wave 3); one solo failure to settle:
  `mode_keys_scope_visible_command_output_separately_from_underlying_copy_mode` ("output view 1
  did not close" at 30 s, alone, at load 6-7).
- CONTROL merged as `59db7c3e` (fix round 36 min, `70e4f1ef`): both majors fixed (remove never
  blocks, the buffer is dropped; socket EOF closes the handed stdio and outbound at once), the five
  minors (flags restored at exit and before the client writes stdout itself, ack wait with no
  deadline, the stderr diagnostic after the reply, stdout WRITABLE deregistered once drained, and
  a `control-stdio-v1` capability bit in the compact Welcome so a new client stays on the relay
  against an older daemon, no new message), and the wait-exit hang (`capture_pending_return`
  dropped a second Return or EOF that arrived during a command). Mac after the fixes: latency
  0.0125 ms, burst 211k, instructions per command unchanged. Windows check equals the base.
  Known gap kept: kill-server against a handoff client whose consumer stopped reading drops the
  final `%exit` after the 2 s writer timeout (the relay client would write it later). DL5 rebases
  on it. Linux checks for the merged tree (`59db7c3e`) run in alienware `~/dev/zz-control`.
- DL6 merged as `d408426e` (lane 162 min, `71e04876`, done=false): one Linux gather thread per
  shard (`gather_pty_linux` over every PTY of its shard plus a wake pipe for launches, exits and
  buffer returns; a pane with no free buffer leaves the poll set; started lazily, so shards with
  only empty or output panes add no thread). `mem.threads.p20`/`tui20`/`scroll180`/`scroll80`
  25/25/26/26 -> 9, the final rule (12) met. zz-terminal 378 pass, tui-output-backpressure 9/9.
  Throughput floors not judged: the owner was gaming on alienware (about 3 cores, 8 GB swap;
  a bare `cat` ceiling ran 2.5x slower), and `bench/run.sh` needs a `dist/zz` GUI bundle there.
  Single-shard busy probe: 1 busy pane flat, 4 busy unicode +1%, 4 busy ascii 120 -> 105 MB/s
  (-13%): follow-up DL6b (`perf/gather2`, same worktrees). Attach on Linux: kernel time is
  0.25/0.5 ms of 2.7/3.55 ms per attach and the gather threads do nothing during it, so
  `attach.cpu.*` is not a gather lever (DL1 and later slices). The lane killed CONTROL's Linux
  release build with `pkill -f "cargo build --release -j6 -p zz-cli"` (briefs now forbid broad
  pkill patterns). Merging it landed under the running batch2 Mac checks (mostly Linux-only
  code); merge checks now take `WT=<worktree>` so they run on a fixed snapshot.
- PTYLEAK (lane 46 min, `d492c144`, done): the leak was a lost `Shutdown`, not a held session.
  `TerminalSession::drop` sends `Shutdown` with `try_send` into the actor's one-slot mailbox;
  with a control client attached, `stop_control_output_tap` queues `DisarmRawOutputTap` just
  before, the slot is full and the shutdown was dropped, so the child and 3 ptmx fds lived on
  (kill-window/kill-pane 3 -> 63 fds after 20 pairs, respawn-pane -k 3 -> 87, kill-session 3 ->
  60). Now `Drop` defers `Shutdown` behind the queued command; `ptyleak_tests.rs` fails on the
  base. Merges after batch2's Mac checks finish (they run in `zz-perf-int`).
- TEARDOWN (launched 23:40 from `perf/ptyleak`): `mode_keys_scope_visible_command_output_
  separately_from_underlying_copy_mode` takes 30 s on both hosts because teardown waits for its
  `sleep 30` pane to exit on its own (3.08 s with `sleep 3`): a shard actor is not woken when its
  terminal is dropped. The lane makes a kill reach an idle silent child within 100 ms.
- Linux checks of the merged tree run in alienware `~/dev/zz-control` (`~/.cache/zz-perf/batch2`);
  the old KNOBS Linux check was stopped after its release build timed out at 40 min on the loaded
  host (its fmt, clippy, 44 solos and compat 99/101 stand; the three solo stragglers pass alone,
  `formatted_split_wait_resumes_its_pane_wait_on_the_loop` 2 of 3).
- SPAWN safety review (Opus, 56 min): the CLONE_VM child is sound (signals blocked across clone
  and reset in the child, no allocation or lock, BIND_NOW, fds 0-3 only, exec failure statuses
  and pidfd reaping unchanged). Two majors: the 64 KiB child stack came from mimalloc and nearly
  doubled daemon footprint per live pane (Linux `mem.footprint.p20` 6.3-6.8 -> 11.05 MiB; the
  lane never ran `--only mem`), and clearing the THP flag on the shared mm opened a daemon-wide
  THP window that raced other shards' spawns (2 of 8 runs got a huge page; the lane's own THP
  test failed 4 of 25). Decisions for the fix round: an mmap'd stack with a guard page; Linux
  panes inherit the daemon's THP-off setting (already the case under every CLI- or ssh-started
  daemon; a recorded difference from tmux); `ZZ_PTY_FORK=1` selects the old fork path; a shard
  blocking until exec is accepted (glibc posix_spawn does the same). Unchecked parity question:
  `respawn-pane` reuses the pane's previous shell, not the current `default-shell`.
- TEARDOWN merged (lane 19 min, `b048ed0f` + my `c72a7323`): my hypothesis was wrong. Teardown is
  prompt on the base (every kill path reaps an idle child in about 1.5 ms in process, 16 ms
  through the CLI under load, with or without a control client, both shard settings; `Shutdown`
  already wakes the shard). The test sent Escape to the copy pane, which parks the output since
  `fec8e107` (09-10, "Keep the view surface to its pane's keys"), so the output only closed when
  its `sleep 30` window died, racing the 30 s wait deadline. The key now goes to the output
  pane (0.07 s) and the pane sleeps 300 s, so a regression fails at the deadline instead of
  passing by luck. Noted, not fixed: a sharded actor whose session drops after `terminate()` gets
  no wake when its channel closes; its own grace and SIGKILL timers cover the child.
- DL1 merged as `37822fbf` (lane 135 min, `dd322400`): `TerminalWatcher::handle` stops calling
  `terminal_current_command`/`synchronize_pane_runtime` per frame; the shard keeps a last-output
  instant (`AtomicU32` ms, `EventQueueState` stays 8 words) and an output-since-check bit; one name
  check per pane per 500 ms (`TimerKey::NameCheck`, `run_due_name_checks`) reads the foreground pid,
  command and cwd once and feeds `set_pane_runtime_facts_at`, automatic-rename and the peer probe;
  silence re-arms from the last output; `wait-pane` idle reads the instant. Mac
  `chatty.instr_per_s.flip` 59.5-67.1 -> 43.1-46.4, `chatty.cpu_pct.visible` 3.14-3.52 -> 2.35-3.31;
  Linux `chatty.cpu_pct.flip` -0.6 pt; loop busy samples in visible 36 -> 13, the lookups at 0.
  rename-timing.sh matches tmux. Open: a macOS name check costs about 60 us because the 4-entry
  name cache in `process_info.rs` misses whenever the foreground pid changes (`KERN_PROCARGS2`);
  the choose-tree activity sort still refreshes per output frame while such a chooser is open.
- DL2 launched 00:05 (10-03) on `perf/deliver` reset to `37822fbf` (same worktrees as DL1).
- Pushed mid-wave to main 2026-10-03 00:15 (no tag; the freeze holds until wave 4 exits): KNOBS,
  ROWS, TUI-ECHO, CONTROL, DL6, PTYLEAK, TEARDOWN, DL1. Checks on the merged head: Mac batch1 and
  batch2 merge checks (above), Linux functional checks of `59db7c3e` (1378 tests, control_stdio,
  compat 37/42 with the known reds, startup diagnostics, backpressure) and Linux clippy of
  `300a2f7d` after one fix (`RawFd` imported only for the macOS kqueue loop: TUI-ECHO never ran a
  Linux build). Rule from this: every Mac lane's merge waits for a Linux `cargo clippy` of the
  merged head. Wave-exit gates and the full corpus still to come.
- Side lanes as one workflow from 00:25 (ultracode is on; workflow `w4-side-lanes`, run
  `wf_602a147e-e5e`, script under the session's `workflows/scripts/`): each lane implement ->
  Opus review (structured findings) -> fix when there are findings. SETTLE (`~/dev/zz-settle`:
  no full settle snapshot for an unwatched pane, the `split_empty_P` lever), NAMES
  (`~/dev/zz-names`: cheap macOS foreground names, 60 us -> 15 us per check), TUIECHO2
  (`~/dev/zz-tuiecho2`: key encode, prefix canonicalisation and the per-frame cell copy in the TUI
  client). Every brief now ends with a Linux clippy on a detached alienware worktree of the same
  name. Briefs in `wave4-briefs.py`.
- SPAWN merged as `706d08b3` (fix round 57 min, `45dae962`): the child stack is a 64 KiB mmap with a
  PROT_NONE guard page, unmapped when clone returns; Linux panes inherit the daemon's THP setting
  (child prctl, parent reset and `THP_DISABLED_HERE` gone; the test, renamed
  `a_pane_keeps_transparent_huge_pages_off_when_zz_turned_them_off`, checks both spawn paths);
  `ZZ_PTY_FORK=1` selects the old fork path with the exec fence. alienware, pairs at load 2-8:
  `spawn.cpu.split_shell` 1.83 -> 1.30 ms and `new_window` 1.60 -> 1.13 ms (-29% each), footprint
  p1 +0.3%, p20 -0.9% over five mem-only pairs (the 20-shell probe 5.23 vs 5.24 MiB); THP probe 0
  huge pages and every pane THP-off in 8/8. Still red against tmux on Linux: the three
  `spawn.cpu.*` rows (SETTLE takes `split_empty_P`); on the Mac `split_empty_P`. Flaky on Linux on
  base and lane alike: `an_attach_repaints_a_dead_pane_kept_by_remain_on_exit` (5 of 10 alone).
- DL6b (lane 76 min, `4ac0a4c5` on `perf/gather2`, done=false, not merged): busy panes read on into
  their next buffer and refills are bridged with a shared zero-timeout poll; 1 busy pane +3%, but 4
  busy ascii panes on one shard still 169 MB/s against 221 on the pre-DL6 per-pane readers (the
  DL6 probe's 120 -> 105 was a loaded run). Cause, from `/proc/<tid>/io` and `wchan`: one gather
  thread now does every pane's n_tty reads (435k reads/s at 414 B) and spends 13% in `flush_work`
  waiting for the kernel to refill an empty 4 KiB tty buffer, even on a nonblocking fd, starving
  the shard (68% vs 95% before). DL6c (01:00, same branch): skip panes with nothing queued
  (`TIOCINQ`), or a second gather thread only while two or more panes of a shard are busy; target
  0.9x pre-DL6 at 4 busy panes. Its footprint rows "failed" only because binaries sat on tmpfs
  `/tmp` (the wave-3 trap); briefs now say to keep measured binaries on disk.
- Pushed to main again at 00:45: `2918280f` (SPAWN), after Linux clippy, zz-terminal and the new
  daemon test modules passed on that head.
- DL2 merged as `8b1393a2` (lane 60 min, `229549e5`): frames carry the pane's stream sequence (the
  current viewport's `view_generation`; terminals start 2^40 above the previous one, so it grows
  across respawn); `PaneFrameFanout::enqueue` reuses one encoded patch per (base, current) per
  frame; `TerminalFrames::full` caches encoded full frames per (pane, generation, cells), at most 16
  frames and 1 MiB; `PendingTerminal.encoded` is `Arc<[u8]>`. No client reads `Event.sequence` for
  terminal frames (zz-client ignores it). Second client on a pane 2.36 Minstr/5 s (target 3.0);
  Mac gate 0 fail, 0 regressed, `attach.cpu.p4` 1.2. Linux: `attach.instr.p4` equal to the base,
  but `attach.ttfc.p4` failed in 2 of 3 lane runs and 0 of 3 base runs on the loaded host: the
  row to watch at wave exit. Frames are 1-3 bytes larger (6-byte varint sequence). DL5 must order
  by per-pane stream sequence, since frame and event sequences no longer compare.
- DL3 launched 01:05 as workflow `w4-slices` (run `wf_a3e8d9c5-200`): implement -> parity and perf
  reviews in parallel -> fix; the script takes a list of slices and is reused for DL3b/DL4/DL5.
- DL6b/DL6c merged as `d548f3c8` (DL6c lane 55 min, `5b75f2e7`): FIONREAD gating was measured in a C
  model (`/tmp/zzpc/gather3/ptybench.c`): the ioctl never waits, but it spins about 2.5M ioctls/s
  and reads shrink, 124-152 MB/s; one reader thread tops out near 200 MB/s whatever the strategy
  (2 threads 194-215, 4 threads 230-244, per-pane readers 235-284). So the gather lends every busy
  pane but one to a thread of its own (up to three per shard) when two panes filled a 64 KiB
  buffer within 10 ms; the pane keeps its buffers, partial batch and bridge state, buffer returns
  and exits follow it through a swappable wake target, and it goes back after 100 ms without a
  full buffer. Single-shard probe: 4 busy ascii 240.5 MB/s vs pre-DL6 247.6 (0.97x; DL6 174.9),
  1 busy 1.02x, 4 busy unicode 1.00x; default shards over 8 pairs `throughput.detached.ascii`
  0.987x; `mem.threads.*` 9. Open: thread churn at the lending threshold (each hand-off spawns a
  thread). Race and parity review of the merged gather runs as workflow `w4-review-fix`
  (`wf_5b1b76de-3cc`).
- Gather review (two Opus lenses, 78 min): no blocker or major; fd lifetime, wakeups, byte order
  across hand-offs, the thread bound and respawn during floods all held under scratch stress
  tests (707 spawn/kill cycles, 200 respawn rounds). Four minors fixed in `7bc80444` (merged): a
  lent thread lingers 1 s and is reused (distinct gather threads in 10 s at the threshold 91 -> 2),
  dropping the last `PtyGather` wakes and stops the home, a stopped gather is replaced on the next
  spawn, and a lent thread that cannot hand back no longer acts as a home. One busy pane read
  0.974x pre-DL6 against 0.98 in that round, with the untouched DL6c build at 0.982 in the same
  minutes: noise, accepted.
- Linux quick A/B of `59db7c3e` (KNOBS+ROWS+TUI-ECHO+CONTROL) against the pre-KNOBS binary, three
  pairs on the loaded host (`~/.cache/zz-perf/batch2`): `attach.instr.p1`/`.p4` -50%/-47%,
  `chatty.instr_per_s.flip`/`.hidden` -57%/-39%, `control.latency` -59%, `control.burst_cmds_per_s`
  +67% (86k against 51k), `control.cpu_per_cmd` -39%, `control.instr_per_cmd` +21% (the daemon now
  writes what the client used to). Chatty flip/hidden CPU rows failed on the lane in quick mode at
  39-57% fewer instructions: load, judged at wave exit.
- Side lanes merged 01:55 (workflow `w4-side-lanes`, 9 agents, 2.9 h):
  - SETTLE `e703eb64` (lane `99c07f48` + fix `a2bbc675`): an empty pane (`split-window ''`) is now
    created at its real size with its modes set before the actor starts (`spawn_empty_pane`), so
    no settle build runs for it; `#{cursor_flag}` is backed by `TerminalFacts::cursor_hidden`
    (removed from the `formats.terminal-runtime` gap). Fixes a parity bug that predates wave 4:
    `capture-pane -p` on such a pane printed 24 rows where tmux prints 11, and cursor_flag read 1.
    Mac `spawn.instr.split_empty_P` -45%, `spawn.cpu.split_empty_P` 0.92-1.00x tmux (rule 1.2x,
    was 2.1x). Open: the settle after real output in an unwatched pane (needs an async settle
    before `#{C:}` filters in status.rs `pane_search` and a metadata-only `FrameSnapshot::capture`);
    remaining split+kill cost is command execution 39% (source pane tcgetpgrp and cwd, automatic
    rename on kill), loop turn 25%, shard wake 23%.
  - NAMES `3352c904` (lane `ed3640c8` + fix `14931bf1`): the lane's launch-key guess named sibling
    symlinks wrongly (`vi` -> `vim`; review blocker) and was dropped; what merged is the exact
    lookup with a 64-entry cache keyed by (pid, unique_id, id_version), safe against pid reuse.
    Per-check cost 22.4 us against a 15 us target; the saving is inside run-to-run noise. Kept
    because the old 4-entry pid-keyed cache could return a stale name after pid reuse.
  - TUIECHO2 `52f8e3ab` (lane `4ae4b030` + fix `f3af5d52`, done): key input encodes straight into a
    reused buffer (`zz-protocol/src/key_frame.rs`), prefix keys are canonicalised once, and the
    client reuses a spare cell buffer instead of copying the 38 KB array per frame (seeded test
    `reused_cell_buffers_follow_patches_while_clones_are_held`). Client per key 92 -> 76.6 kinstr.
- batch3 checks from 02:00 on `52f8e3ab` (everything since batch2): Mac full suite in the snapshot
  worktree `~/dev/zz-check` plus the full corpus, A/B against the wave-3 exit binary; Linux the
  same in alienware `~/dev/zz-perf-int` (`~/.cache/zz-perf/batch3`, no quiet-gate).
- Trap: a SendMessage to an agent that already finished resumes it. Wait for its next completion
  notice before removing its worktree (TUI-ECHO lost its worktree mid-rerun this way; its commit
  was already merged).

## Wave 3 merge log (from 2026-10-01)

- W3-LOOP (c)/(e) slices, 2026-10-01 evening. Two lanes at a time, one per host, each on the
  other's latest commit, cherry-picked onto the Mac `perf/loop` (Linux pushes go to
  `perf/loop-mac` in alienware's repo; its worktree is reused per slice with `git worktree move`, so
  the build cache comes along). Slices that share daemon.rs but not the same functions run in
  parallel; each brief names the other lane's area as off limits.
  - c01 (`5369b274`, Mac): watcher threads relay `DeferredTerminalEvent`s to the loop, 0
    `inner.lock()` in watcher bodies, 1385 daemon tests green. `chatty.instr_per_s.hidden` 1.09x B
    (B 78-82, c01 87-96 Minstr/s): every relayed event and effect completion calls
    `AcceptWake::wake`, one syscall each. Decision: accepted, with the fix folded into c02 (which
    rewrites that path) instead of a separate fix round; c02's budget is cumulative against B.
    `cli.instr.select_pane.p20` 1.056x was noise (B itself spans 0.137-0.150).
  - c03 (`43a32883`, Linux): empty and output panes run on the shard threads; 40 surfaces add
    only the 4 shard threads; `spawn.instr.split_empty_P` 0.98x. Hidden chatty 1.13x B is
    within B's spread on alienware (B 38.1-41.8, c03 41.8-45.1) and c03 does not touch that
    path: accepted, recheck on the combined build. Its 4 serial daemon failures pass alone; the
    base fails 7 environmental ones on that host (process_info, endpoint, russh_socks).
  - c02 (`810dd4d3`, Mac): relay threads gone; producers call a notification sink after
    enqueueing (and on close), the loop drains each surface 8 events per turn with one coalesced
    wake. Threads p1/p20 3/25 -> 2/5 on the Mac; hidden chatty 0.93x B, all 22 instruction
    medians at most 1.026x B, idle 0. Linux check rides on the e01 merge.
  - e01 (`7ce2d11e`, Linux): command items own queue state, scopes and control-event capture
    (new `daemon/cmdq.rs`); no command guard keyed by ThreadId. Correct, but short commands pay
    3.2-4.1% more user instructions than B on alienware (chain5 p1 0.222 -> 0.230 Minstr): a
    bounded fix lane (`e01fix`) profiles it with `perf record -e instructions:u` and removes it.
  - c04 (`4fa8958b`, Mac): agent publication goes through a message-only handle
    (`daemon/agent_publisher.rs`, `agent_inbox.rs`) applied on the loop; 0 agent threads added,
    retired runtime messages dropped. Hidden chatty read 1.19x B, but c02 vs c04 directly is
    70-80 vs 75-80 Minstr/s and an idle inbox turn is one atomic swap: noise, accepted.
  - e12 (`5156d733`, Mac): loop-owned job registry (`daemon/jobs.rs`): pipe/socket ports on
    loop tokens, CHILD_SIGNAL checks only registered children, deadlines in the loop timeout; no
    family migrated yet (e14-e16). 20 jobs add 0 threads, each child reaped once. `chain5.p1`
    read 1.037x c04, but that row is bimodal on the Mac (c04 alone 0.2687 / 0.2689 / 0.278
    Minstr) and the other 23 rows are flat: accepted.
  - e01fix (`3c21e82c`, Linux): `perf record -e instructions:u` showed command entry and
    completion copying a request context nobody read; removing it leaves short commands 1.2-2.0%
    above B on alienware (was 3.2-4.1%). Accepted.
  - Linux leg of c04 and e12 on the combined branch: c04 tests and the slow-client soak pass;
    five e12 registry tests failed in the serial suite only (EINTR from a SIGCHLD handler an
    earlier test installed); fixed in the test pump (`78c906b4`). The loop itself retries.
  - e19 (`6e7d954e`, Mac): bounded demand-started helper pool (`daemon/helpers.rs`) for path
    listing, git/catalog discovery, prompt history, terminfo warming and peer probes; 1439 daemon
    tests. My A/B of its final HEAD vs e12: `send_keys.p20` +28.7% and `mem.threads.p1` 2 -> 3 (its
    own runs predated its last fixes). Fix lane `e19fix` (helper thread start per command with a
    50 ms idle exit, 10 ms polling waits).
  - e19fix (`999439ae`, Mac): idle helpers linger 250 ms instead of 50 ms, empty peer probes
    retire their worker, discovery waits block on the reply (the registry enforces the deadline),
    and the loop thread never blocks on a full helper queue. send-keys p20 back to 0.168 Minstr
    (e12 0.168), threads 2/5, idle 0, 1442 daemon tests. e19's Linux tests pass.
  - e02 (`c16fd4e2`, Linux): Hello, Control and Exec requests run as cmdq queues resumed on the
    loop; output pressure parks a continuation until the socket drains (`wait_for_output` gone).
    19/19 CLI medians at most 1.004x its base, control 0.989x, tui-output-backpressure passes.
  - Both hosts now carry the same slices (cherry-picked in different orders): Mac `perf/loop`
    `fdf67a9d`, alienware `perf/loop-e01` `397d7da8`. The e01 Mac leg is green (1411 lib tests).
  - e02 Mac leg (`fdf67a9d`): 1419 lib tests, compat clean, no instruction row moved 3%;
    `control.burst_cmds_per_s` ranges overlap (pre 83-134k, post 72-107k cmds/s).
  - e03a (`9f31b80d`, Linux): aliases and foreground inserted callbacks run as owned parent/child
    queue frames; a 3-level alias suspension keeps output order while another queue runs. CLI
    within 1.006x, control instructions +0.44% (noise, accepted). `smoke/control-alias-prepare`
    has 2 OUT divergences on alienware on every binary back to the wave-3 start (`base-cli`): not
    a LOOP regression; check it on the Mac before calling it a Linux gap.
  - e15 (`6fdb9049`, Mac): status `#()` jobs run on the job registry (`daemon/status_jobs.rs`),
    the first family to use it; no reader or drop-cleanup threads. `statusjob.threads_per_s` 3 -> 0,
    status CPU 0.21% (limit about 0.7%), status instructions 7.14 -> 6.94 Minstr/s, CLI within
    1.015x, 1458 daemon tests. `smoke/status-background-jobs` fails its clock assertion on the base
    and on tmux on this Mac (not e15).
  - e03a Mac leg (`07fb0a4c`): 1424 lib tests, alias compat clean. `smoke/control-alias-prepare`
    also fails on the Mac with the wave-3 start binary: zz answers frozen/sequential as the
    2026-09-06 closure recorded, the fetched tmux oracle now answers broken. Oracle drift, not
    LOOP; handed to a separate task.
  - e03b (`b626aa02`, Linux): sourced config replay, after-command, event and shutdown hooks run
    as owned queue frames; 23/23 perf rows at most 1.004x, compat clean.
  - `plain_input_and_unchanged_selection_need_no_worker` fails on Linux from c01 on (warm runs
    20/20, the first run after a build passes): pane startup raises hook events, c01 runs
    `run_event_hooks` as a watcher effect on a worker, and that worker start takes the test's
    injected failure. A test race plus one worker hop per title/rename event; e20 (event hooks
    as cmdq work on the loop) owns the fix and must pass it 20/20 on Linux.
  - Order change: e04 (tap arm/disarm, rearm_pane_pipe) would collide with e16 on the Mac, so
    alienware took e06 (wait-for continuations) first; e04 follows e16.
  - e16 (`d640ed4b`, Mac): pane pipes, copy-pipe and raw Control output run on the loop and
    the job registry (`daemon/pipe_jobs.rs`), bounded tap delivery with capacity notifications;
    0 reader threads, 1468 daemon and 361 terminal tests. Flagged rows were noise:
    `cli.instr.list_panes.p1` is bimodal on the Mac (base 0.149/0.151/0.163) and hidden chatty's
    median fell. Mac p1 CLI rows sit in two modes about 7% apart; judge them on min/max.
  - e03b Mac leg (`66cc31a4`): 1430 lib tests, source/hook compat clean.
  - e06 (`576a9d13`, Linux): wait-for signal and lock waiters are cmdq continuations
    (`daemon/wait_queue.rs`); 20 waiters add 0 workers and complete once; new
    `smoke/wait-for-loop` matches tmux; instructions within 1.016x.
  - e20 (`65defe00`, Mac): timer transitions, monitors and queued hooks run on the loop
    (`daemon/hook_queue.rs`); zz-copy-refresh, zz-clock-mode and zz-daemon-hooks gone;
    `plain_input_and_unchanged_selection_need_no_worker` 20/20. Only the bimodal p1 rows flagged.
  - Linux leg of e15/e16: status jobs, tui-output-backpressure and pipe compat pass, but two e16
    tests fail deterministically on Linux with or without e06:
    `control_output_loop_delivers_bytes_without_readers` (5 s timeout) and
    `copy_pipe_failure_is_a_reliable_client_message_and_releases_its_permit`. A Linux fix lane
    follows. e20 conflicts with e06 in daemon.rs; resolved once on the Linux branch, then the Mac
    branch takes the Linux tree through a merge (no force-push of `perf/loop`).
  - Branches converged 2026-10-02: the e20 cherry-pick onto e06 was resolved on Linux (`036dc942`),
    and the Mac `perf/loop` merged it taking the Linux tree (`e122e38d`; trees identical, no
    force-push). Mac leg of e06 and the resolution: 1442 lib tests, compat clean. `perf/loop` is
    backed up on origin (not merged to main).
  - Mid-lane strict Mac gate (`gate-mac-65defe00.json` in /tmp/zzpc/w3, against B): 74 pass,
    9 fail. Fixed vs B: `spawn.cpu.kill_pane` 0.373 -> 0.288 ms, `statusjob.threads_per_s` 3 -> 0.
    Threads 3/25 -> 2/5, footprint p20 16.9 -> 10.9 MB, echo p99 busy 2.17 -> 1.13 ms (tmux 0.38),
    idle 1.70 -> 0.87 ms. Still red: echo x4 and control.latency (ratio to tmux), attach.ttfc.p4
    (13.3 vs tmux 11.8, improved from 19.1), client CPU visible. `spawn.instr.split_empty_P` is
    bimodal (B itself reads 1.09 or 2.06). Only real regression vs the wave-2 exit baseline:
    `mem.copy_instr.scroll180` 4.92 -> 5.5 Minstr, added by c01 (bisected: B 5.03-5.12, c01
    5.51-5.56); fix lane `c01copy` on the Mac.
  - e16 Linux fix (`fa29d51d`): Control output resumes when the bounded tap drains and copy-pipe
    exits keep their status on Linux; the three e16 tests pass 10/10. On the base every Control
    transfer in the throughput probe timed out (a real Linux stall), now 131-137 MB/s. Detached
    throughput median 134 vs 136 MB/s (timing), hidden chatty median 1.006x.
  - c01copy (`8affa1f7`, Mac): c01 had moved a cold mode-format expansion (~0.95 Minstr) for a
    Control client with status subscriptions off ahead of the copy-mode reply; `status_targets`
    now skips it. `mem.copy_instr.scroll180` 5.55 -> 4.97 Minstr (wave-2 exit value 4.92).
  - e16 Linux fix Mac leg (`ffcef23e`): 1442 lib tests, the three e16 tests, pipe compat clean.
  - e13 (`326de470`, Mac): run-shell delays and `-C` callbacks on the timer heap with a ready-queue
    activation for zero delay; 20 delays add 0 threads and 20 deadlines; 1479 daemon tests.
  - e04 (`6dc8d3bb`, Linux): spawn identity, tap arm/disarm, settle and copy-source acquisition
    resume through loop request tokens (`daemon/terminal_requests.rs`); a respawn rejects stale
    replies; 1450 daemon and 369 terminal tests. `spawn.instr.new_window` 0.718 -> 0.737 Minstr
    (1.026x, token registration per spawn); split_shell 1.000x, kill_pane 0.998x: accepted.
  - e14 (`6c163955`, Mac): foreground/background run-shell and if-shell jobs on the registry, 14/16
    explicit park sites; 20 jobs add 0 workers, admission capped at 256; 1445 daemon tests. p20
    medians show_options/has_session/send_keys 1.020-1.024x, inside the base's spread.
  - `if-shell-background-order` fails with every binary back to before wave 3 (`base-cli-mac`): zz
    keeps file order for equal-cost background jobs, the fetched tmux answers
    never-file-order-in-8. Oracle drift like control-alias-prepare; both handed to one separate
    task.
  - e05 (`6afbb9f5`, Linux): capture, history, semantic capture, pointer-context and Kitty reads
    resume through actor completions (`daemon/terminal_reads`); stale client, respawn and image
    generation results dropped; 22/22 perf rows at most 1.006x; 1456 daemon and 372 terminal tests.
  - Second sync: Mac slices c01copy, e13, e14 go onto Linux; e13 conflicts with e04/e05 in
    hook_queue.rs/wait_queue.rs, resolved by a pick lane. Design-doc conflicts are unions of both
    as-built notes.
  - e07 (`705ca175`, Mac): display-panes, command-prompt and confirm-before park as queue
    continuations; 20 parked overlays per type add 0 workers; 1492 daemon tests.
  - e08 (`ab94c8d4`, Mac): display-menu parks as a continuation; 20 blocking menus add 0 workers;
    19/19 CLI rows under 1.02x.
  - Third sync: the e07 pick on Linux needed both sides' new `RegisteredWait` fields (`terminal`
    from e04, `overlay` from e07) and a union of the queue-command classifiers (`aba5b662`). The Mac
    branch took a scratch tree of the Linux tip plus e08 (`c0f35c97`) through a merge (`f29c3622`):
    pick onto the other host's tip in a scratch worktree, then merge that tree, instead of merging
    two branches that carry the same patches as different cherry-picks.
  - compat `prompt-history` fails on alienware with every binary back to the wave-3 start and
    passes on the Mac: a Linux environment issue, not LOOP.
  - e09 (`095151f1`, Linux): split-window -W and wait-pane --exit park as terminal-incarnation
    continuations; 20 exit waits add 0 workers and 0 poll deadlines; 23/23 instruction rows pass
    (split_shell 0.987x). e08 picked onto Linux after it (`da24448e`).
  - Order change: e10 (send_text, paste_and_submit) waits for e18 (agent-send waits go through
    them); alienware took e11 (client file waits) meanwhile.
  - e18 (`ed40399a`, Mac): agent-send terminal and native turn waits, peer, permission,
    new-agent-session and GUI replies are typed continuations; 20 parked waits add 0 workers; a
    timed-out turn keeps running; 1474 daemon tests. `select_pane.p20` is bimodal (base
    0.140/0.140/0.150, new 0.151/0.151/0.139): accepted.
  - Windows build broke at some slice before e18 (`wait_queue::wait_needs_worker` called from
    code Windows compiles while `wait_queue` is `#[cfg(unix)]`); fixed by moving the pure helper
    next to `InsertedCommandStep` (`7a555180`). Briefs on the Mac now include
    `cargo check -p zz-daemon --target x86_64-pc-windows-msvc`.
  - e10 (`93f06e08`, Mac): wait-pane condition scans, run-pane marker scans and paste_and_submit
    echo waits resume on output changes and deadlines (`daemon/terminal_reads.rs`); 20 waits add
    0 workers and 0 scan timers; 19/19 CLI and 6/6 chatty rows pass. Windows check passes with 16
    warnings (dead code on that target), left for the d steps.
  - e11 (`e4330884`, Linux): client file requests, caller stdin and sourced replay resume through
    continuations (`daemon/source_queue.rs`); 20 outstanding operations add 0 workers; wrong-owner
    and duplicate replies rejected; 1479 daemon tests. `config.instr.source_1000` 41.4 -> 44.2
    Minstr (+6.8%, outside the base's range): fix lane `e11fix` profiles it on alienware.
  - e17 (`8501b400`, Mac): display-popup waits are continuations completed from loop-owned popup
    events; popup PTY children stay with their shard; 20 popup waits add 0 workers; 1483 daemon
    tests. `show_options.p1` 1.027x is one of the bimodal Mac p1 rows.
  - Order change: e21 (startup replay, registration, shutdown) touches the source replay e11's fix
    is rewriting, so the Mac took e22 (cooperative list-keys and full-history capture) first.
  - e11fix (`64e475f5`, Linux): per-line hook eligibility searches, repeated command-name
    resolution and finish-state copies in source replay; the replay now runs a line first and keeps
    a frame only for waits or children. `config.instr.source_1000` 1.020x the base (was 1.068x).
  - Fourth sync onto Linux: e18 picked (`f797df18`); the Windows-build fix does not apply there
    (e11 removed `wait_needs_worker` on that branch), so the Windows check reruns on the Mac after
    convergence; e10 and e17 go through a pick lane (conflicts with e09/e11).
  - e22 (`9e4b0160`, Mac): uncached list-keys and full-history capture yield between bounded
    steps (64 binding rows, 512 history rows); new paired echo-fairness probes pass 12/12 against
    tmux's increase (also with K=1); capture instructions 0.68x, list-keys 1.012x.
  - Fifth sync: e10/e17 picked onto Linux by a lane (`bf401683`), e22 picked onto that in a scratch
    worktree (`ba4d2afa`, classifier union again), Mac `perf/loop` merged that tree (`362c83bc`).
    The Mac's Windows-build fix is subsumed: e11 removed `wait_needs_worker` on the Linux side.
  - Mac leg of the fifth sync (`362c83bc`): clippy, Windows check, zz-mux 655, zz-terminal 371,
    zz-daemon 1493 lib tests (one serial-suite failure of
    `twenty_registered_jobs_add_no_monitor_threads_and_reap_once`, 3/3 alone), 16 compat scenarios
    clean except `prompt-history`, which now fails on the Mac for every binary back to the wave-3
    start although e07's lane passed it hours earlier with the same pinned tmux: state between
    runs, not code. Added to the compat-drift task with the other two scenarios.
  - Step (d) decision (2026-10-02): a read-only ultra plan (`bench/perf/campaign/w3-loop-d-plan.txt`)
    counts 570 production `inner.lock()` sites (the rest of the ~2300 matches are tests) and 18
    ConnectionThreads submissions, and splits (d) into d01a/d01b plus d02a-h, about 12-15 hours.
    Its value estimate: a short command takes about 27 uncontended model lock/unlock pairs, 540-1080
    instructions, 0.4-0.8% of `cli.instr.display.p1`; a chatty frame 2+V. After e21 the loop is the
    only lock taker on Unix, so the lock is uncontended. Decision: W3-LOOP's perf work ends at
    e21; (d) becomes a separate ownership-cleanup lane after W4-DELIVER (the plan file is its
    input). Instead of the post-(d) parity review, an ultra source review of the whole c/e series
    runs before the lane merge.
  - e21 (`b9ab5478`, Linux): startup replay, registration, default attach, disconnect/unregister,
    destroy-unattached and shutdown run on the loop (`daemon/lifecycle.rs`); no ConnectionThreads
    in production Unix builds (Windows keeps accepts, writers, deferred exec, timers and helper
    dispatch on them); 1497 daemon tests, startup diagnostics 8/8. Short commands +3.5-4.1% user
    instructions (12/19 CLI rows over): fix lane `e21fix` profiles the per-connection lifecycle.
  - Ultra review of the c/e series (`362c83bc`): codex's cyber filter stopped it after 21 min;
    before that it confirmed one P1 (helper replies awaited synchronously on the loop:
    ensure_prompt_history, warm_terminfo, history save, peer sync, HelperPool::read/import_source;
    two slow source-file reads stall every client) and one candidate (macOS copy-pipe EPIPE
    cancels the job and force-stops the child, unlike Linux). Fix lane `helperwait` on the Mac;
    the remaining review areas (wake flags, client-loss cleanup, platform guards, output order) go to
    an Opus subagent per brief rule 7.
  - e21fix (`ff4e3335`, Linux): an empty retirement allocation, an extra loop wake and empty
    lifecycle locking per command (1.8k instructions); 19/19 CLI rows at most 1.016x. Linux leg of
    e22 on top (`c8beb4f9`): 1499 daemon, 374 terminal, 656 mux tests, backpressure passes.
  - Opus review of the rest (`bench/perf/campaign/w3-loop-review-findings.txt`): P1 a waiting
    event hook after a queued task panics the daemon (`set-hook -g window-layout-changed 'run-shell
    true'` + split-window); P2 on Linux a fully accepted 256 KiB write batch leaves the rest of a
    client's mailbox waiting for an unrelated event; P3 the Windows test build, a peer-scan panic
    that stops peer probes, a client-file waiter leak race. Fix lanes: `hookpanic` and
    `reviewfixes` (with the macOS copy-pipe EPIPE case) on the Mac, `writestall` on Linux.
  - helperwait (`e3a2a1fd`, Mac): prompt history load/save resume through helper continuations
    (`daemon/history_io.rs`); no helper reply is awaited on the loop thread (the remaining
    synchronous callers are off the loop); 1533 daemon tests; 21/21 perf rows pass.
  - writestall (`0ca40354`, Linux): when write_ready stops at its batch limit with data queued, the
    connection stays scheduled for another pass; the new test stalled at 262 KB of 786 KB on the
    base and passes 10/10. The base also hung a `capture-pane` over 60 s in a perf run (large
    output to a CLI client): same bug. Instructions within 1.004x, throughput/attach within range.
  - hookpanic (`fbd1ffbe`, Mac): event hooks raised by a finishing loop task queue on the loop's
    hook queue after the command's output; both reproductions are tests; CLI 0.987-1.002x.
  - helper-wait picked onto e21 on Linux (`600a5295`; config hooks reload-config/import-tmux-config
    keep a worker there). Final convergence: hookpanic picked on the Linux tip (`3c425efa`, module
    union), Linux fast-forwarded to it, Mac merged its tree (`534c99ee`). Both branches now hold the
    same tree.
  - reviewfixes (`8b01d408` Mac, `880f37bb` Linux, same tree): Windows test build links again
    (Unix-only tests gated), peer-scan panics still complete the probe, client-file waiter insert
    re-checks the client, EPIPE on a job's input ends the input instead of cancelling the job, the
    hookpanic worker test no longer asserts a worker, and two tests that raced queued loop work
    (`first_command_session_uses_zero_ids_and_arms_last_session_shutdown`,
    `shell_formats_use_the_selected_current_client`, 4/15 failures alone) drive the loop until the
    condition. Both hosts green (1509/1510 daemon tests).
  - Lane merge 2026-10-02: `perf/loop` (`8b01d408`) merged into `perf/wave3` as `6701b511` (clean,
    Cargo.lock auto-merged with main's GPUI-lab pin). Merge checks running on both hosts
    (`/tmp/zzpc/merge-w3-loop`, `~/.cache/zz-perf/merge-w3-loop`), WIRE=1 on the Mac, PRE =
    `loop-00-cli` (main's daemon code is unchanged since the lane's base).
  - Merge checks on `6701b511`: fmt and clippy clean on both hosts. Workspace tests under load: Mac
    28 failures (26 pass alone; most are timeouts and the process-wide thread-count asserts of
    e07/e08/e12/e14/e16), Linux 11. Failing alone on Linux: `endpoint::tests::remote_scripts_fall_back_to_the_mac_app_bundle_cli`
    (environmental: the Arch package's `/usr/bin/zz` wins over the bundle fallback) and zz-ui
    `which_key::tests::caps_fold_families_and_ranges` (main's `b8270500` test expects macOS glyphs).
    The two `cli_binary` failures never ran alone: the scripts passed the test target as a package.
    Compat rows 31/32 on both hosts: `smoke/display-menu-action-queue` is red with the merge and
    with the lane tip `8b01d408`, clean with `loop-00-cli`. Mac `attached-client.sh` fails: a
    key-binding `source-file -F` glob runs on the mux loop and hits the helper-wait guard
    ("synchronous helper reply on mux loop"). web-build, release, tui-screen-diff and the iOS
    build pass. A/B against `loop-00-cli` (3 alternating quick pairs per host): threads p20
    25 -> 5 Mac / 45 -> 25 Linux, footprint p20 -36% / -20%, statusjob threads/s 3 -> 0, Mac
    kill-pane instructions -36%, copy CPU -33 to -38%, Linux control output +88%; regressions
    that block the strict gate: `statusjob.instr_per_s` +42% Linux / +10.5% Mac (hard),
    `config.{cpu,wall}.source_1000` +36 to +55% with instructions +3 to +5%; also Linux CLI
    instructions +4 to +8% per command, `control.instr_per_cmd` +5.5%, Linux kill-pane
    instructions +9.2%.
  - Fix lanes from 2026-10-02 12:17, the first Opus lanes: `perf/loopfix` (`~/dev/zz-loop`:
    helper replies on the loop, the menu row, source_1000 timing), `perf/testfix`
    (`~/dev/zz-testfix`: thread-count tests re-run alone in a child process, the which-key and
    endpoint tests, the solo re-run of integration tests in both merge-check scripts),
    `perf/perffix` (`~/dev/zz-perffix`, Linux builds in alienware `~/dev/zz-loop-e01`: status-job
    and CLI instructions). All from `ebc4f781`; merge them, rerun the merge checks, then the
    wave-3 exit gates.
  - Fix lanes merged 2026-10-02 (wall time, all Opus): testfix `42c8dc06` (40 min; a shared
    `solo_tests::rerun_alone` runs the process-wide thread-count tests alone in a child process,
    the which-key test expects spelled key names off macOS, the endpoint test runs with a PATH
    of three symlinked tools, the merge-check scripts re-run integration tests with
    `--workspace --test <target>`). testfix also found two more regressions: `zz reload-config`
    hit the helper-wait guard, and a late client during a draining kill-server alias got EOF.
    drainfix `363cf238` (14 min): `b9ab5478` had widened the accept gate in
    `EventLoop::accept_ready` to `shutdown_pending`, so no connection was accepted during the
    drain; back to `stopping`, with a real-socket test in `drainfix_tests.rs`. perffix
    `f9f34b33` (90 min): status renders only when a job's output changes or it can restart this
    second (1.375 renders per job before), one read buffer kept on the loop (an 8 KiB zeroing per
    command), `Box<Client>` built in place, `task_command` evaluated once, the response clone in
    `CommandTask::finish` only when someone else holds the item, no child frame for empty hook
    lists. Linux statusjob instr/s 2.31 -> 1.25 Minstr/s (-46% against wave 2), Mac -26%, every
    Linux CLI row within 2% of wave 2 (chain5 and list_keys +1 to +1.6%). loopfix `b333fc79`
    (2 h 12 min): `loop_handoff.rs` hands a key binding that contains a parking command
    (source-file, foreground run-shell, shell if-shell, load/save-buffer, prompt history while it
    loads) to one queued task, keeping its order, and runs `reload-config` and
    `import-tmux-config` on a connection worker; a queued caller parks on a CommandWait. Two of
    those paths had hung the whole daemon (key-bound `run-shell`, including `-C`, parked forever).
    The menu row was a TUI bug from W3-TUI (`8548e684`): a menu closed and a nested one opened in
    the same batch left the old choice pending and ate every later key; `ClientCore` now counts
    menu opens and `Model::sync_menu` drops the stale choice. Known gap: a command handed off
    from inside a nested list (an `if-shell -F` body, a prompt template) runs after the rest of
    that list, where tmux runs it in place. `cli.instr.display.p20` reads 0.140 or 0.151 on both
    binaries (one sample right after 19 shells start); judge it settled, not by a quick median.
  - Second merge checks on `2ea96e66` (all four fix lanes): Linux green (5 load failures pass
    alone, 36/36 compat rows, release). Mac: the zz-daemon lib binary hung on a quiet host (load
    2.6) with 50 failures; a `sample` showed `zz-pty-shard-1` blocked in a blocking `waitpid`
    (`PaneActor::poll_child` -> `ChildExitWatch::on_readable` -> `reap_child(pid, true)`: a
    shard-owned watch has no kqueue, so `on_readable` assumes the child exited) while every
    other pane on that shard waited on its full command channel. It came with W3-SHARDS
    `6c7e10f0` and is on main: a macOS daemon can freeze one shard's panes until some child
    exits. Lane `perf/shardwait`. The 50 pass alone; compat 36/36, attached-client, tui-screen,
    web, iOS and release pass on the Mac.
  - Linux strict wave-3 gate on `2ea96e66` (`~/.cache/zz-perf/w3/wave3-alienware-2ea96e66.json`,
    not committed: code is still changing): 73 pass, 16 fail, 4 regressed. The fails are rows
    that also failed at wave 2 against tmux and improved (spawn CPU x3, chatty visible CPU,
    attach ttfc/cpu, control.latency) plus the echo rows, which the wave-3 stage now judges at
    1.5x tmux (1.81-2.53 ms against 0.92-1.13; W4-DELIVER). Full-mode triage, alternating
    loop-00 and the gate binary twice: `control.burst_cmds_per_s` 40.6k/42.5k vs 40.8k/39.1k (no
    regression: the wave-2 JSON's 141k was an outlier); `attach.wire_frames.p4` 3 -> 7 and p1
    3 -> 4 at the same bytes (the SHARDS and TUI merge binaries still send 3; Mac the same);
    `mem.copy_wall.scroll180` 1.29/1.42 -> 1.71/2.28 ms with CPU and instructions lower (Linux
    only; Mac -29%); `config.instr.source_1000` +3.7% against loop-00, +5.2% against the wave-2
    JSON. Lanes `perf/attachframes`, `perf/copywall`, `perf/sourceinstr` from `2ea96e66`.
  - Merged 2026-10-02 (Opus lanes): shardwait `d88f2074` (36 min): W3-SHARDS registers the
    child watch at the shard's first poll, after exec; for a shell already exiting XNU answers the
    EVFILT_PROC add with ESRCH (the process is marked dead at the start of exit), the shard took it
    as "exited" and did a blocking `waitpid` before reading the PTY, and a session leader exiting
    with unread output waits in exit for the master to drain (the ttywait in `kern_exit.c`): the
    only reader was the blocked shard. Shard mode now reaps with WNOHANG and re-registers after
    10 ms (an ESRCH re-add on every poll made kevent return only the changelist errors and starved
    the shard). C repro in `/tmp/zzpc/shardwait-repro/exitrace.c` at the time; Linux unchanged.
    sourceinstr `ff7334a6` (71 min): after-command hook context built only when a hook table has
    commands, parsed commands moved instead of cloned, no state lock or path clone per line; Linux
    `config.instr.source_1000` 42.18 -> 40.21 Minstr (loop-00 40.66), Mac -6%. attachframes
    `1ac403a5` (2 h with a follow-up): `send_compact_resync` flushed the attach batch before the
    panes' async settle callbacks queued their viewports (3 -> 4/7 frames); the batch now waits
    for the round's settles or a 100 ms loop timer, and a failed settle (actor stopped) still
    sends the latest viewport as before W3-LOOP; frames 3/3 again, Mac ttfc p4 20.3 -> 16.8 ms.
    copywall `495bcb89` (96 min): a terminal request that met a full one-slot actor channel
    (the copy-mode settle right behind its ViewAction) parked and retried on a 1 ms poll, which
    epoll rounds to a whole extra sleep; it now joins the control slot's deferred list behind the
    in-flight commands (a Wake when nothing is in flight); Linux `mem.copy_wall.scroll180` 2.03 ->
    1.05 ms (loop-00 1.06). Its one miss: Mac `mem.copy_footprint.scroll180` +16 KiB (one page,
    0.547 vs 0.531 MiB, under the 0.5 MiB floor and below wave 2's 0.64), accepted.
  - Not a bug: a pane whose command prints and exits at once (`new-window 'echo MARK'` with
    remain-on-exit) shows only "Pane is dead" on macOS for zz and for tmux alike (10 of 10 on
    pinned tmux): the macOS PTY drops output the reader has not taken when the last slave closes.
  - Wave-3 exit run on `fe57a839` (`/tmp/zzpc/wave3-exit`, `~/.cache/zz-perf/wave3-exit`): fmt,
    clippy, compat-check, builds, web, iOS and tui-screen pass; workspace tests Mac 25 / Linux few
    load failures, all pass alone (an A/B with `ZZ_PTY_SHARDS` 4, 16 and 0 gives 1-8 failures in
    any setting, so the shared shards are not the cause); Mac attached-client failed on the tmux
    side ("command-output view did not show one ordered replay transcript", a known load flake).
    Linux gate (load 1.1): 76 pass, 12 fail (the known tmux reds plus echo), 2 regressed, both
    explained: `control.burst_cmds_per_s` (the wave-2 JSON's 141k is an outlier; loop-00 reads
    40-42k) and `cli.wall.version.p1`, which reads 0.78 or 2.0-2.5 ms on loop-00, this build and
    tmux alike in a quiet-gate A/B. The Mac gate started at load 25 (my shard experiment), so it
    is rerun. Corpus: Linux adds three reds to wave 2's known eight; Mac adds five to its known
    ones. `smoke/chooser-kill-keys` and `smoke/hooks-pane-focus-clients` (both hosts) are W3-LOOP
    regressions, clean on every binary before the W3-LOOP merge: lane `perf/focusfix`
    `16fb164b` (41 min; the overlay's `pane-focus-out` waited in `pending_event_hooks` until the
    queue finished, now parked queues run their pending hooks at the park as tmux's global queue
    does; the chooser's kill refresh ran before the queued kill, now it runs after it).
    `smoke/format-modifier-client-loop` is red only with the Linux debug build (clean release).
    `census-hooks`, `smoke/plugin-runtime-vim-tmux-navigator` and `smoke/source-file-byte-name`
    are red on the Mac with loop-00 too (pre-existing, not wave 3).
  - Wave-3 exit run 2 on `e9bc174c` (focusfix merged; `/tmp/zzpc/wave3-exit2`,
    `~/.cache/zz-perf/wave3-exit2`), the wave-3 close. fmt, clippy, compat-check, debug and
    release builds pass on both hosts; workspace tests 7 (Mac) and 8 (Linux) load failures, all
    pass alone. Corpus: only the expected reds. Linux: `lane2-store`, `show-options-hooks`,
    `smoke/control-alias-prepare`, `smoke/plugin-runtime-resurrect-restore`,
    `smoke/format-modifier-client-loop` (debug build only) and the four `known/*`. Mac: the four
    `known/*`, `census-hooks`, `if-shell-background-order`, `smoke/control-alias-prepare`,
    `smoke/plugin-runtime-continuum`, `smoke/plugin-runtime-vim-tmux-navigator`,
    `smoke/resurrect-save`, `smoke/source-file-byte-name`, `smoke/status-background-jobs`.
    `smoke/chooser-kill-keys` and `smoke/hooks-pane-focus-clients` pass on both. Mac
    attached-client failed on the tmux side in both exit runs under load (the copy-mode position
    indicator captured half drawn, a lone `[` at column 196 of 200) and passed alone on a quiet
    Mac with this build and with loop-00.
  - Gates committed as `bench/perf/results/wave3-<host>-e9bc174c.json`. Linux: 75 pass, 13 fail,
    1 regressed (`control.burst_cmds_per_s`, the wave-2 outlier). The fails are wave-2 tmux reds
    that improved (spawn CPU x2, chatty visible, attach CPU x2, `control.latency`), the six echo
    rows (1.5x rule, W4-DELIVER) and `cli.wall.capture_history.p20` at 1.127x tmux, a new row:
    six quiet-gate runs alternating `2ea96e66` and this build read 0.96-1.04x tmux for both at
    the same 138.67 Minstr, so the gate read a noisy sample. Mac: 81 pass, 7 fail, 6 regressed
    against the wave-2 JSON, all explained by a full-mode spawn/chatty/attach A/B alternating
    loop-00 and this build twice (`/tmp/zzpc/w3/exit-ab`, about 1.3 cores of desktop load):
    `spawn.wall.split_shell`, `spawn.cpu.split_shell` and `spawn.wall.split_empty_P` read the
    same or better than loop-00 (split_shell CPU 1.63/1.75 against 1.58/1.60 ms, wall 9.5/10.5
    against 9.2/7.8 ms), so the wave-2 JSON came from a quieter Mac; `attach.wire_c2s.p1`/`.p4`
    +264 B is the caller's environment in the Hello (loop-00 sends 4738/4785 B from the same
    shell); `chatty.cpu_pct.steady` 6.55/7.40% against loop-00's 5.44/5.81% with tmux at 4.7-6.5%
    in the same runs is +11% once normalized by tmux, at +4% instructions (66.0/67.9 against
    63.8/64.6 Minstr/s), under both tolerances: accepted and watched in W4-DELIVER, which
    rewrites hidden-pane delivery. `echo.p99.idle` shows as drifted against W0 but passes at
    0.87x tmux.
  - loop-00 hung on Linux in `capture-pane -p -S -` of the cli group's 20-pane session (the loop
    idle in `ep_poll`, the client waiting on its reply). That is the `writestall` bug, introduced
    and fixed inside W3-LOOP (loop-00 already carries the b slices), never on main.
  - Decision: wave 3 is closed. Both exit gates, the HANDOFF and the design status go to main
    with the code; no tag (release freeze until wave 4).
  - `chatty.instr_per_s.*` is a rate and its quick-mode spread is 5-15% per host (Mac hidden
    chatty B alone spans 70-84): judge it on the min/max of three alternating runs, or against
    the previous slice's binary, not the median ratio alone. Later briefs compare against the
    previous slice's binary at 1.02x instead of B at 1.03x.
- W3-LOOP in progress on `perf/loop` (worktree `~/dev/zz-loop` on the Mac; not merged): e0, a1-a4,
  b1-b6 with fixes, then main merged in (slice 00, `1518f03d`). Step (b) plan:
  `bench/perf/campaign/w3-loop-b-plan.txt`; steps (c)/(d)/(e), 30 slices in order:
  `bench/perf/campaign/w3-loop-cde-plan.txt`. Baseline B for those slices:
  `loop-B-macbook.json` (74 pass, 7 fail) and `loop-B-alienware.json` (65 pass, 16 fail), kept in
  `/tmp/zzpc/w3` and `~/.cache/zz-perf/w3`. Against the SHARDS merge: threads p1/p20 10 -> 4 / 50
  -> 45 on Linux, 3 / 25 on the Mac; `cli.cpu.display.p1` 0.082 -> 0.051 ms (Linux, tmux 0.074)
  and 0.039 ms on the Mac (tmux 0.070); `control.cpu_per_cmd` near tmux on both. Linux user
  instructions per short command are 7-18% higher (mio per-connection bookkeeping) while CPU
  falls 20-40%: the gate rows are CPU, so this is accepted. Each b slice first added 4-10%
  per-command instructions (worker round trips, admission wakes, status re-arming) and needed a
  bounded fix; expect the same pattern in (e). Open gate gaps owned by (c)/(e): spawn
  (`kill_pane` 0.37 vs tmux 0.19 ms on the Mac), `statusjob.threads_per_s` 3/s, echo.
- W3-SHARDS merged 2026-10-01 as merge 1 (`99aef76b`), slices s1 `edd445fc` (PaneActor state
  machine), s2 `7ff4e681`+`c42a605e`+`3736bf67` (K shard threads, lazy start, coalesced wakes),
  s3 `00314f96` (portable-pty off Unix; the allocation-free spawn already existed), s6
  `3ef90061` (direct shard reads, opt-in `ZZ_PTY_GATHER=0`: they lose Linux Unicode throughput
  ~10%, so gather stays the default per the plan), macOS readiness fix `6c7e10f0`, s5 `2f8d787e`
  (one snapshot per distinct live view state), s4 `024d811f` (one lazy search thread), s7
  `2321297c` (Windows ConPTY readers feed shards; Windows `cargo check` only). Linux gate
  `w3-1-shards-alienware-99aef76b.json`: 65 pass, 16 fail, all owned elsewhere (LOOP: spawn x4,
  statusjob threads, control latency; W4: chatty visible, attach) or the open echo target; its 2
  regressed rows are timing at flat instructions. Merge A/B against the wave-2 binary: threads
  at 20 panes 66 -> 50 (Linux) and 46 -> 30 (Mac), footprint -40% / -33%, RSS -20%, chatty flip
  instructions -6.8% on the Mac. Open: echo latency. Linux echo is unchanged since wave 2 (p50
  2.0 ms vs tmux 0.9) and the wave-3 rule (<= 1.5x tmux) starts here; on the Mac busy-pane echo
  p50 rose about 15-25% with shards (a flooding pane shares a shard with the idle one). Owner:
  a shard fairness brief (echo-ready panes ahead of a busy pane's turn), then a cross-lane echo
  breakdown (input path, actor, delivery). Six Windows-only dead-code warnings in zz-daemon
  (daemon.rs `BootstrapReady`/`start_lock`, timers.rs `PeerProbe`, status.rs `set_tmux_shim`,
  user_data.rs `fs`) are handed to LOOP. Mac strict view at wave exit.

- Echo work, 2026-10-01. A Linux breakdown with temporary per-stage timestamps (no commits)
  found no fixed waits: zz's echo p50 2.03 ms against tmux 0.92 is a chain of thread handoffs of
  90-140 us each: TUI client relays ~390 us, daemon loop read/write ~235 us (LOOP b2/b3), shard
  wake and gather relay ~200 us, pane watcher to delivery ~95 us (W4-DELIVER); row extraction
  p99 reaches 700 us when echo and scrolling coincide (W4-ROWS). Merged into perf/wave3:
  `16ec41a2` (shard fairness `a8ee2ecf`: serve fresh input and pending echo ahead of a busy
  pane's turn, never shorten output-only turns; a first version that preempted on any ready
  pane cost chatty 16-21% instructions) and `8548e684` (new W3-TUI slice `953fa4e0`, not in the
  plan: the TUI attach client runs on one thread and one event loop, echo p50 -240/-257 us idle
  /busy on Linux), plus `a93bff74` (the TUI slice was built only on Linux and used rustix
  `pipe_with`, which macOS lacks; now `pipe()` plus fcntl). Merge A/B against the SHARDS merge:
  Linux echo zz/tmux p50 idle 2.19 -> 1.82, busy 2.25 -> 1.99; Mac busy p50 -16%, busy p99 -50%,
  idle p99 -30%; no instruction regressions. Echo is still above the 1.5x rule on both hosts:
  the rest is LOOP (b2/b3 already cut the loop handoffs on perf/loop) and W4-DELIVER.

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
- 2026-10-02: implementation lanes run as Opus 5.5 subagents at xhigh effort, not codex (too slow for implementation). Deeper reviews are unchanged.
- 2026-10-03: codex only when a lane is stuck ("invoke gpt 6 astra xhigh for review if and only if we feeling stuck"): `lane-run.sh` defaults to `gpt-6-astra` (override with `CODEX_MODEL`), effort `xhigh`; routine parity, perf and fork reviews go to Opus subagents.

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

Opus lanes (from 2026-10-02): the agent definition `~/.claude/agents/lane-impl.md` (`model: opus`,
`effort: xhigh`) takes the same `lane-briefs.py` brief and runs in the background through the
orchestrator's Agent tool, one per worktree, building on the Mac and reaching Linux over ssh.
`lane-run.sh` and its watchdog do not apply: the orchestrator checks each lane's commits every
15 minutes and stops a stuck agent itself.

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
   SHARDS s2 fix was stopped mid-series with all its work uncommitted); the gate series itself runs once on the final code, because e19 measured, changed code and ran out of allowed runs; a helper agent appears (a subagent rollout in
   `~/.codex/sessions` for that cwd; `features.multi_agent=false` does not remove the tools in
   codex 0.159, so the brief also forbids them); a file changes outside the write-zone globs (plus
   the design doc; add `bench/results/*` when a brief runs `bench/run.sh`, which writes there,
   or the watchdog stops the run, as it stopped SHARDS s6 at 130 min); a file is added under
   `third_party/` or a vendored crate; more than 50 or 100
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

- Tests that call `Poll::poll` or other blocking syscalls must retry `Interrupted`: on Linux an
  earlier test in the same process can leave a SIGCHLD handler installed, so the call fails
  only in the serial suite. macOS did not show it (e12's registry tests).
- Launching a lane over ssh with `nohup ... &` keeps the ssh call open until its timeout (the
  lane survives). Use `setsid -f lane-run.sh ... > log 2>&1 < /dev/null`.
- macOS caps PTYs at 511: never run workspace tests and perf runs on the Mac at the same time.
  Overlapping them failed six LOOP b6 perf runs ("tmux PTY allocation failed") and 166
  parallel daemon tests (all passed alone).
- A slice built on one host only needs a `cargo check` on the other before acceptance: the
  W3-TUI slice (Linux) used a rustix API that macOS does not have.
- `pkill -f` and `pgrep -f` over ssh match their own shell's command line: write patterns as
  `[c]argo` / `[m]erge-checks`. A merge-check script must get a saved copy of the pre-merge
  binary, not `target/release/zz_cli`, which the script rebuilds before its A/B.

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
