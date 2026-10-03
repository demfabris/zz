import importlib.util, json, sys, os
spec = importlib.util.spec_from_file_location('lb', os.path.expanduser('~/dev/zz-perf-int/bench/perf/campaign/scripts/lane-briefs.py'))
lb = importlib.util.module_from_spec(spec); spec.loader.exec_module(lb)
OUT = '/tmp/zzpc/w4'
START = open('/tmp/zzpc/w4-start-rows.txt').read() if os.path.exists('/tmp/zzpc/w4-start-rows.txt') else ''
FINAL = open('/tmp/zzpc/w4-final-fails.txt').read() if os.path.exists('/tmp/zzpc/w4-final-fails.txt') else ''

def header(title, wt, branch, budget):
    return f'''# {title}

{lb.RULES}

Worktree: {wt} (branch {branch}, from {lb.INT_BRANCH} = origin/main 06ea9cf1 plus a brief-generator commit; wave 3 is closed and on main). Its target/ is warm (APFS clone of the integration tree's) and compat/.cache holds the pinned tmux. Work only there; prefix every command with `cd {wt} &&`. Scratch files go in {lb.SCRATCH}/<slug>/ (create it). Budget: {budget} minutes from your start.

Wave 4 is the last wave. Its gate is `--stage final`. These rows fail that stage on the wave-3 exit build (rescored from bench/perf/results/wave3-<host>-e9bc174c.json, zz median, zz max, tmux median, ratio):

{FINAL}
Other lanes run at the same time in their own worktrees: W4-DELIVER (shard frame delivery, ~/dev/zz-deliver: publish_terminal_for_pane, OutboundState terminal slots, subscription handlers, control output taps, zz-pty-gather threads), W4-BINARY (~/dev/zz-binary: a daemon-only executable and packaging), W4-ROWS (~/dev/zz-rows: libghostty row extraction), KNOBS (~/dev/zz-knobs: deletes every ZZ_PERF_* rollback knob from waves 1 and 2 and the fallback paths behind them), SPAWN (~/dev/zz-spawn, built on alienware: pane spawn CPU), DL6 (~/dev/zz-gather, built on alienware: one Linux PTY gather thread per shard), CONTROL (~/dev/zz-control: zz_cli -C latency) and TUI-ECHO (~/dev/zz-tuiecho: the attach client's cost per key). Stay inside your own write zone; when the work you need sits in another lane's zone, report it under open instead of editing it.
'''

def knobs():
    wt = f'{lb.ROOT}/zz-knobs'
    return header('Wave 4 lane KNOBS: delete the wave-1 and wave-2 rollback knobs', wt, 'perf/knobs', 150) + f'''
The design doc's rule: "One environment knob per behaviour change ... Knobs from wave N are deleted when wave N+2 starts." Wave 4 starts now, so every knob from waves 1 and 2 goes, with the code path that only the knob reaches. These are all the ZZ_PERF_* knobs (the doc's table, verbatim):

| Knob | Lane | Set, restores |
|---|---|---|
| `ZZ_PERF_LEGACY_COMMAND=1` | EXEC | CLI uses ClientHello + PrepareCommandList + CommandRequest (daemon keeps that path through W2) |
| `ZZ_PERF_CONNECTION_THREADS=0` | EXEC | a new thread per connection, none kept idle for reuse |
| `ZZ_PERF_EAGER_FRAMES=1` | PANE | frames for every attached view plus the no-view fallback |
| `ZZ_PERF_NO_COMPRESS=1` | PANE | no idle history compression |
| `ZZ_PERF_ECHO_FASTPATH=0` | PANE | always wait `CONTENT_PUBLISH_STALENESS` |
| `ZZ_PERF_EAGER_PUBLISH=1` | PUBLISH | runtime-fact and title changes publish synchronously; no subscriber early returns; every Snapshot is sent even when a client already has it |
| `ZZ_PERF_RENAME_THROTTLE=0` | PUBLISH | no 500 ms automatic-rename throttle |
| `ZZ_PERF_PEER_SCAN=always` | PUBLISH | 1 Hz Claude peer scan reading every record each tick; the status sampler ticks with no client |
| `ZZ_PERF_EAGER_UNIVERSE=1` | FORMAT | full universe per expansion (also the differential oracle) |
| `ZZ_PERF_COMPILED_FORMATS=0` | FMT | parse and evaluate templates through the interpreter |
| `ZZ_PERF_BORROWED_FORMATS=0` | FMT | build the full owned table values for contexts and loop items |
| `ZZ_PERF_BORROWED_FACTS=0` | FMT | build owned command facts before engine execution |
| `ZZ_PERF_FORMAT_CACHE=0` | FMT | rebuild compiled templates, option snapshots and every format cache |
| `ZZ_PERF_ATTACH_DEDUP=0` | ATTACH | attach resends Snapshot and overlays, frames not held until Attached, no dedup |
| `ZZ_PERF_ATTACH_BATCH=0` | ATTACH | attach holds only terminal frames; reliable messages written as queued |
| `ZZ_PERF_ATTACH_PRESIZE=0` | ATTACH | an attaching raw-terminal client's panes keep their size until its first ResizeTerminal |
| `ZZ_PERF_WRITEV=0` | ATTACH | one write per frame, a writer thread per connection, default send buffer, unbuffered protocol reads (daemon and client) |
| `ZZ_PERF_TUI_COALESCE=0` | ATTACH | the TUI repaints without coalescing or skips (read at CLI start) |
| `ZZ_PERF_ROW_PATCHES=1` | TERM | patches replace every changed row whole |
| `ZZ_PERF_READONLY_SKIP=0` | HOOKS, CTRL | read-only commands take before/after captures and publication checks |
| `ZZ_PERF_EAGER_FACTS=1` | HOOKS, CTRL | every command builds format hook facts |
| `ZZ_PERF_HOOK_JOURNAL=0` | HOOKS | hook events from whole-mux snapshots before and after |
| `ZZ_PERF_KEY_TABLE_DELTA=0` | HOOKS | Full subscribers receive every key table |
| `ZZ_PERF_TREE_DELTA=0` | CTRL | compact subscribers receive full scoped trees instead of TreeDelta |
| `ZZ_PERF_COPY_CLONE=1` | COPY | flat ModeRevision clone |
| `ZZ_PERF_THP=1` | FOOTPRINT (Linux) | the daemon keeps transparent huge pages as the system sets them |

Where they live at the base (rg): statics in crates/zz-daemon/src/daemon.rs (ROW_PATCHES, BORROWED_FORMAT_FACTS, log_pane_perf_knobs and the startup "knobs" log lines), daemon/timers.rs (EAGER_PUBLISH, RENAME_THROTTLE, KEY_TABLE_DELTA, PEER_SCAN_ALWAYS), daemon/ctrl.rs (TREE_DELTA), daemon/exec.rs (SPAWN_PER_CONNECTION), daemon/hook_events.rs (READONLY_SKIP, EAGER_FACTS, HOOK_JOURNAL), daemon/attach.rs (ATTACH_DEDUP, ATTACH_PRESIZE, ATTACH_BATCH, BATCHED_WRITES), crates/zz-daemon/src/client.rs (LEGACY_COMMAND, BUFFERED_READS), crates/zz-terminal/src/session.rs (EAGER_FRAMES, NO_COMPRESS, NO_ECHO_FASTPATH, perf_knobs, perf_flag), session/mode_revision.rs (COPY_CLONE), session/unix_pty.rs (THP), crates/zz-mux/src/formats.rs (FORMAT_CACHE, EAGER_UNIVERSE), formats/tree.rs (BORROWED_FORMATS), formats/compiled.rs (COMPILED_FORMATS), crates/zz-tui/src/lib.rs (COALESCE); about 130 references in all. Also the ZZ_PERF_* passthrough where the CLI spawns the daemon, bench/perf/isolate.py, bench/perf/README.md, compat/rename-timing.sh, knowledge/tmux/key-tables.md, knowledge/terminal/libghostty-vt.md, knowledge/protocol/wire-protocol.md. Keep the wave-3 switches (ZZ_PTY_SHARDS, ZZ_PTY_GATHER and any other non-ZZ_PERF_ variable).

Task: delete each knob, its static, and every branch, function, type, field and wire message that only the knob's non-default side reaches, keeping the default behaviour exactly as it is. Before deleting a fallback, check it has no other caller (a test oracle, another feature, Windows code): `ZZ_PERF_EAGER_UNIVERSE` is also the format differential oracle, so a property test that compares the lazy path with the eager universe may keep calling the eager function directly (no knob) or be removed if the compiled path already has its own exact-output tests; say which. Tests that only exercise a fallback path go with it; tests that set a knob to its default value drop the setting. The LEGACY_COMMAND daemon path (ClientHello + PrepareCommandList + CommandRequest) may still serve older CLIs inside protocol 107: delete only the CLI side and the daemon side if nothing current sends those messages (check zz-client, zz-tui, zz-client-ffi, clients/, the GUI crate, the ssh entry); if a current client still sends them, keep the daemon side and say so. Update the doc's rollback table to one short paragraph (wave-1 and wave-2 knobs deleted at the start of wave 4, the commit, what remains) and remove knob mentions from the other docs listed.

Done criterion: `rg -n 'ZZ_PERF_' crates clients compat bench/perf/*.py bench/perf/README.md knowledge --hidden` prints nothing except that one paragraph in {lb.DOC}; `cargo check --workspace --all-features` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` exit 0; `timeout 1800 cargo test -p zz-daemon -p zz-mux -p zz-terminal -p zz-tui -p zz-client -p zz-cli --all-features` passes (re-run load failures alone); `cargo check -p zz-daemon --target x86_64-pc-windows-msvc` NOT required (say NOT RUN); compat rows `formats formats-values hooks smoke/own-conf smoke/config-grammar smoke/source-file-diagnostics smoke/source-replay-diagnostics smoke/oh-my-tmux smoke/tpm-init` plus every compat row whose name matches attach, copy, control, rename or status pass or match the pre-lane binary (build one from {lb.INT_BRANCH} with `git -C {lb.INT} worktree add --detach {lb.SCRATCH}/knobs/base {lb.INT_BRANCH}` and a cloned target, or compare against {lb.INT}/target/debug/zz_cli, which is the base build); `compat/attached-client.sh` passes on a quiet run or fails only as the base does; a quick gate `python3 bench/perf/run.py --zz target/release/zz_cli --stage final --quick --only cli,attach,chatty,control --w0 none --json {lb.SCRATCH}/knobs/after.json` against the same run of {lb.INT}/target/release/zz_cli shows no instruction row above 1.02x. Report the net line count removed (`git diff --stat {lb.INT_BRANCH}...HEAD | tail -1`).

Commit on perf/knobs with a plain message, one commit.'''

def deliver_plan():
    wt = f'{lb.ROOT}/zz-deliver'
    return header('Wave 4 lane W4-DELIVER: map the code and write the slice plan (no code changes)', wt, 'perf/deliver', 100) + f'''
{lb.context({"id": "W4-DELIVER"})}

That section was written before wave 3. Wave 3 changed the ground under it: W3-SHARDS put PTY reads on K = min(cpus, 4) shard threads (`PaneActor`, `on_readable`/`on_command`/`on_deadline`, `ZZ_PTY_SHARDS`), but Linux keeps one `zz-pty-gather` thread per pane by default because direct shard reads (`ZZ_PTY_GATHER=0`) lost about 10% Unicode throughput; W3-LOOP replaced the per-pane `zz-pane-N` watcher threads and relay threads with producers that call a notification sink, and the loop drains each surface 8 events per turn with one coalesced wake (c02), runs empty and output panes on the shards (c03), and owns jobs, helpers, cmdq and the accept/read/write path on one mio loop; W3-TUI runs the TUI client on one event loop. Threads at 20 panes are now 5 on the Mac and 25 on Linux (20 gather threads). Echo, measured on Linux with temporary per-stage timestamps before W3-LOOP: TUI client relay ~390 us (W3-TUI cut ~250 us since), daemon loop read/write ~235 us, shard wake and gather relay ~200 us, pane watcher to delivery ~95 us; row extraction p99 reaches 700 us when echo and scrolling coincide (that part is W4-ROWS).

Starting rows (wave-3 exit, zz / tmux medians, verdict at the wave-3 stage):

{START}

Task: read the current code for every path the section names (find them by name; some were renamed or removed by wave 3: say which), the frame path from a PTY read to a client socket write (shard -> surface events -> notification sink -> loop drain -> publish -> OutboundState -> write_ready), the control-mode %output path, the subscription and attach paths, activity/silence, edge events, and the Linux gather thread. Measure what you need with the release build at {wt}/target/release/zz_cli (instruction counts and thread counts are reliable; time is not while other lanes build): at least the thread list of a 20-pane daemon, a control client's thread delta, and frames encoded per frame with two clients on one pane. Then write {wt}/bench/perf/campaign/w4-deliver-plan.txt: first a 10-20 line as-is summary (what wave 3 already delivered of this lane, what remains, with file:function references), then the slices in merge order, one line each in this form (the W3-LOOP plans used it): `<id> | moves: ... | removes: ... | write zone: <files and functions> | risks: ... | done criterion: <numbers and named tests> | gate: <exact commands>`. Each slice is one commit of at most about 3 hours for one agent, has a numeric done criterion tied to the final-stage rows above, keeps tmux parity (compat rows named), and says which host it needs (Linux-only for the gather thread fold; say how to keep Unicode throughput within 2% of today: for example larger reads, a gather step inside the shard, or vectored reads, judged by `bench/run.sh` on Linux and `throughput.*` rows). Mark which slices can run in parallel (disjoint write zones) and which wait. Decide, with evidence, whether `control.latency` (1.41x Mac, 1.52x Linux; final rule 1.1x) and `control.burst_cmds_per_s` (0.83x Linux; rule 0.9x) belong in this lane or in a separate control lane whose write zone does not overlap (say which zone), and whether the Linux `attach.cpu.*` rows (1.4-1.48x; rule 1.25x) do. End with a short list of the risks to tmux parity and to the per-pane stream sequence barrier.

Done criterion: the plan file exists, every slice line has all seven fields, every final-stage row above that fails today is owned by a named slice, by another named lane (SPAWN owns spawn.cpu.*, W4-BINARY owns mem.rss, W4-ROWS owns row extraction) or is argued out with numbers, and it is committed on perf/deliver as the only change (plus the measurement scripts you wrote, if small, under bench/perf/campaign/). Do not edit Rust code in this brief.'''

def binary():
    l = {"id": "W4-BINARY", "slug": "binary", "groups": "mem,cold", "budget": 180,
         "task": "the whole W4-BINARY section, with these as-built facts and decisions: the CLI binary is `zz_cli` from crates/zz-cli (target name `zz_cli`, installed as `zz`); `zz daemon` and cold start (the CLI spawning the daemon when no socket answers) are where it must exec the new daemon executable that sits next to it (same directory; name it `zz-daemon`), falling back to the in-process daemon when that file is missing so dev runs and the bench harness keep working; the in-pane `tmux` wrapper keeps pointing at the CLI. Put the binary target where its dependency graph is smallest (a new small crate or a `[[bin]]` in crates/zz-daemon; say why), and make the daemon graph drop what it never runs (TLS, HTTP/ureq, the russh client, the TUI, clap if the daemon does not parse a CLI): measure with `cargo tree -e normal -p <crate>` and `size`/`otool -L` before and after. Packaging: `cargo xtask` bundles, install.sh, the cask and AUR/deb/pacman recipes under packaging/, the GUI app's bundled CLI if it ships one, and the remote ssh start script (endpoint.rs daemon start) all ship and use the new executable; `just install mac` keeps working. Promote `mem.rss.p1` to gated at the final stage in bench/perf/thresholds.json (`ratio` 2.0 against tmux) if your numbers meet it.",
         "done": "a release `zz-daemon` executable builds; `zz_cli daemon` and cold start exec it when present and fall back when it is absent (a test or a scripted check for both); `mem.rss.p1` <= 2x tmux in a quick gate run with the new executable next to the CLI (today 12.2 MB against 4.1 on the Mac); `cold.wall.*` not slower than the base CLI by more than 5% across three alternating runs; the dyld image count (`DYLD_PRINT_LIBRARIES=1` or `vmmap`) and the daemon binary size before and after are in the report; packaging and endpoint.rs updated; clippy -D warnings, the zz-cli and zz-daemon tests, and the compat rows smoke/own-conf smoke/oh-my-tmux smoke/tpm-init pass; Linux build and the ssh remote start are NOT RUN here (the orchestrator runs them).",
         "note": "Wave 4 is the last wave; its gate is `--stage final`. Other lanes run at the same time: W4-DELIVER (~/dev/zz-deliver, frame delivery in daemon.rs/session.rs), W4-ROWS (~/dev/zz-rows), KNOBS (~/dev/zz-knobs, deleting every ZZ_PERF_* knob: do not add or rely on any ZZ_PERF_* knob; if you need a rollback switch, use a plain env var like ZZ_DAEMON_IN_PROCESS=1) and SPAWN (alienware). Stay inside the startup, binary and packaging code; report anything else under open."}
    return lb.impl(l)

def rows():
    l = {"id": "W4-ROWS", "slug": "rows", "groups": "throughput,chatty", "budget": 180,
         "task": "the W4-ROWS section, profiling first. Decision for this run: profile now on the wave-4 base instead of waiting for the W4-DELIVER merge (DELIVER changes routing, not how a frame reads cells); the orchestrator re-measures on the DELIVER merge before this lane merges. Profile the daemon (and the attached TUI client for client CPU) under `throughput.attached.*`, `chatty.cpu_pct.visible` and `chatty.client_cpu_pct.visible` workloads (bench/perf/groups/throughput.py and chatty.py show the setups; bench/perf/isolate.py the isolated daemon), and the 700 us row-extraction p99 seen when echo and scrolling coincide (echo with a busy pane, bench/perf/groups/echo.py busy30). Find frame build in crates/zz-terminal (snapshot / PaneFrame / row extraction through the safe wrapper over third_party/rust/libghostty-vt-sys) and count per-cell FFI calls per frame. If row extraction is 5% or more of daemon samples in any of those workloads, add the fork API: one call that copies a row range into packed cells (styles and graphemes as dictionary indexes) in the Ghostty fork, then use it in frame build. Ghostty fork rules: the fork is demfabris/ghostty, pinned in third_party/rust/libghostty-vt-sys/build.rs (GHOSTTY_REPO, GHOSTTY_COMMIT 67351380, branch zz-2026-09-30 era; read third_party/rust/libghostty-vt-sys/UPSTREAM.md first). Clone it into {SCRATCH}/rows/ghostty at that commit, commit your C ABI change there on a new local branch zz-2026-10-02 (do NOT push; the orchestrator reviews and publishes it), and build zz against it with GHOSTTY_SOURCE_DIR pointing at the clone; edits at the same path do not trigger Cargo's native rebuild, so touch or rebuild the sys package (`cargo clean -p libghostty-vt-sys`) after each fork edit. Keep the C ABI free of Zig-owned workers and signal stacks (UPSTREAM.md says why). Leave build.rs pointing at the published pin; the orchestrator repins after publishing. If row extraction is under 5% everywhere, record the profile (sample shares, workload, commands) in the lane section's as-built notes and close the lane with that one doc commit.",
         "done": "either (a) a profile under 5% everywhere recorded in the doc and committed, or (b) the fork commit in {SCRATCH}/rows/ghostty (hash in the report), the zz side using it under GHOSTTY_SOURCE_DIR, and with it: per-cell FFI calls per frame down by at least 10x, row extraction share halved in the worst workload, `throughput.attached.ascii_ms` and `chatty.cpu_pct.visible` no worse and instruction rows of the throughput and chatty groups at most 1.0x the base binary across three alternating quick runs, zz-terminal tests and the terminal compat rows (`known/known-terminal-runtime` matches the base, every row whose name contains terminal, render, capture or copy passes or matches the base) pass. Either way the report has the profile numbers.".replace('{SCRATCH}', lb.SCRATCH),
         "note": "Wave 4 is the last wave; its gate is `--stage final`. Other lanes run at the same time: W4-DELIVER (~/dev/zz-deliver: daemon.rs publish paths, session.rs sink and shard publish path), W4-BINARY (~/dev/zz-binary), KNOBS (~/dev/zz-knobs, deleting every ZZ_PERF_* knob: do not add or rely on any) and SPAWN (alienware, pane spawn). Your write zone: frame build and row/cell extraction in crates/zz-terminal (not the shard publish or sink code), the safe wrapper and sys crate over libghostty, the fork clone in scratch, and the W4-ROWS doc section."}
    return lb.impl(l)

def spawn():
    wt = f'{lb.ROOT}/zz-spawn'
    lw = '/home/demfabris/dev/zz-spawn'
    lint = '/home/demfabris/dev/zz-perf-int'
    return header('Wave 4 lane SPAWN: pane spawn CPU at tmux parity', wt, 'perf/spawn', 180) + f'''
Hosts: you run on the Mac, but this lane is measured on Linux (alienware). Edit and commit in the Mac worktree {wt} (no target there: do not build it on the Mac except `cargo check -p <crate>` if you want; it would start cold). To build and measure, push and check out on alienware: `cd {wt} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/spawn:perf/spawn && ssh alienware 'cd {lw} && git checkout -q --detach perf/spawn && ulimit -n $(ulimit -Hn) && cargo build --release -j6 -p zz-cli'` ({lw} is a detached worktree there with a warm target; never check out a branch in it). Every Linux command goes through `ssh alienware '...'`, each call under 10 minutes (use `timeout`, or start long runs with `setsid -f ... > log 2>&1 < /dev/null` and poll the log). Linux base binary: {lint}/target/release/zz_cli (the perf/wave4 build). Linux scratch: /tmp/zzpc/spawn on alienware.

No doc section owns this; the wave-3 handoff says: "`spawn.cpu.*` 1.6-1.8x on Linux (spawn path, not owned yet: give it to W4-BINARY or a spawn lane)". Final-stage rule: `spawn.cpu.new_window`, `spawn.cpu.split_empty_P` and the other `spawn.cpu.*` rows <= 1.2x tmux, `spawn.wall.*` <= 1.05x. Today on this host (alienware): split_shell 3.35 ms against tmux 2.18, new_window 3.20 against 1.81, split_empty_P 1.38 against 1.11; on the Mac split_empty_P 0.95 against 0.45 (the others pass there). bench/perf/groups/spawn.py has the workloads; `spawn.instr.*` rows are the reliable signal on a loaded host.

Task: profile the daemon across a `split-window` / `new-window` / `split-window -d -P` + `kill-pane` loop with `perf record -e instructions:u -g` (and `-e cycles` for kernel time; `perf` is installed) on an isolated daemon (bench/perf/isolate.py has the environment recipe), and compare the call graph with tmux's for the same loop. Cut the avoidable work in the spawn path: pane creation, PTY open and child spawn (crates/zz-terminal session spawn, session/unix_pty.rs), shard registration, the daemon's split-window/new-window/respawn/kill-pane handlers, the layout and the first publication setup. Kernel time counts too (fork vs vfork/posix_spawn-style clone, fd closing, environment building, ioctl count: `strace -f -c` of one split on both muxes). Write zone: those spawn and kill paths. NOT yours: frame publication and delivery (publish_terminal_for_pane, OutboundState, subscription handlers, notification sink, gather threads: W4-DELIVER) and format/status code; if the remaining cost sits there, measure it and report it under open with numbers for DELIVER. The Mac split_empty_P row needs the Mac: say what you expect it to do there; the orchestrator checks it.

Done criterion: on this host, three alternating quick runs on alienware of `cd {lw} && python3 bench/perf/run.py --zz <bin> --stage final --quick --only spawn --w0 none --json /tmp/zzpc/spawn/<label>.json` for the base ({lint}/target/release/zz_cli) and the lane build ({lw}/target/release/zz_cli): `spawn.cpu.split_shell`, `spawn.cpu.new_window` and `spawn.cpu.split_empty_P` medians at most 1.2x the tmux medians of the same runs, or, if a row cannot get there inside your zone, at least 25% fewer instructions than the base with the remaining cost itemised by function with sample shares; `spawn.wall.*` no worse than the base; zz-terminal and zz-daemon tests that touch spawn, respawn, kill-pane, remain-on-exit and pane exit pass; compat rows whose names contain split, spawn, respawn, kill, remain, exit or new-window pass or match the base; `cargo clippy` on touched crates clean. Run Linux builds with `ulimit -n $(ulimit -Hn)` and `-j6`; when you time anything on alienware, run it through `OWN={lw} ~/.cache/zz-perf/quiet-gate.sh --exec <cmd>` (pauses other compiles under ~/dev there). Commit on perf/spawn in {wt}, one commit, and push it to alienware as above at the end.'''



PLAN_PATH = os.path.expanduser('~/dev/zz-perf-int/bench/perf/campaign/w4-deliver-plan.txt')

def plan_line(prefix):
    for line in open(PLAN_PATH):
        if line.startswith(prefix):
            return line.strip()
    raise SystemExit(f'no plan line {prefix}')

def plan_asis():
    text = open(PLAN_PATH).read()
    return text[text.index('As-is'):text.index('Slices in merge order:')].strip()

def linux_hosts(slug):
    wt = f'{lb.ROOT}/zz-{slug}'
    lw = f'/home/demfabris/dev/zz-{slug}'
    return f'''Hosts: you run on the Mac, but this slice runs only on Linux (alienware). Edit and commit in the Mac worktree {wt} (it has no target: do not build there). To build and measure: `cd {wt} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/{slug}:perf/{slug} && ssh alienware 'cd {lw} && git checkout -q --detach perf/{slug} && ulimit -n $(ulimit -Hn) && cargo build --release -j6 -p zz-cli'` ({lw} is a detached worktree there with a warm target; never check out a branch in it). Every Linux command goes through `ssh alienware '...'`, each call under 10 minutes (use `timeout`, or start long runs with `setsid -f ... > log 2>&1 < /dev/null` and poll the log). Linux base binary: /home/demfabris/dev/zz-perf-int/target/release/zz_cli (the perf/wave4 build). Linux scratch: /tmp/zzpc/{slug} on alienware. Time anything through `OWN={lw} ~/.cache/zz-perf/quiet-gate.sh --exec <cmd>` there (pauses other compiles under ~/dev). Commit on perf/{slug} in {wt}, one commit, and push it to alienware as above at the end.
'''

def gather():
    wt = f'{lb.ROOT}/zz-gather'
    return header('Wave 4 lane W4-DELIVER slice DL6: one PTY gather thread per shard (Linux)', wt, 'perf/gather', 180) + linux_hosts('gather') + f'''
The W4-DELIVER plan (bench/perf/campaign/w4-deliver-plan.txt in the worktree; read only its As-is block and the DL6 line) found this as-is state:

{plan_asis()}

Your slice, verbatim from the plan:

{plan_line('DL6 |')}

SPAWN (another lane) edits PaneActor::spawn's spawn path on Linux at the same time: keep your change to the gather branch and poll_sources, and say in the report which functions you touched so the merge can be ordered.

Done criterion: the DL6 done criterion above, measured on alienware against the base binary in the same session.'''

def control():
    l_wt = f'{lb.ROOT}/zz-control'
    return header('Wave 4 lane CONTROL (C1): control-mode command latency at tmux parity', l_wt, 'perf/control', 180) + f'''
The W4-DELIVER plan (bench/perf/campaign/w4-deliver-plan.txt in the worktree) measured this and handed it to a separate lane:

{plan_line('Decision on control.latency')}

Its as-is note on the control client: {[l for l in plan_asis().splitlines() if l.startswith('10.')][0]}

Your lane, verbatim from the plan:

{plan_line('C1 (')}

Decision for this run: try the relay variant first. If it cannot reach the done criterion, the stdio variant is allowed now (DL5 has not started and will rebase on you): the daemon receives the control client's stdin/stdout over the socket with SCM_RIGHTS and writes the control protocol bytes to that fd directly, with a plain rollback switch `ZZ_CONTROL_RELAY=1` read once at CLI start that keeps the relay path. Keep `pump_control_output_at` rendering and `publish_control_output_for_pane` unchanged (DL5's zone); change only where the rendered bytes are written and how control lines arrive. Measure with `python3 bench/perf/campaign/w4-deliver-control.py <zz_cli> /opt/homebrew/bin/tmux {lb.SCRATCH}/control/control-<label>.json` (it exists in the worktree) and the `control` group of bench/perf/run.py; on the Mac only; Linux rows are NOT RUN here (the orchestrator runs them; say what you expect there).

Done criterion: the C1 done criterion above for the Mac rows, with the Linux rows marked NOT RUN, and every listed compat row and test passing.'''

def tuiecho():
    wt = f'{lb.ROOT}/zz-tuiecho'
    return header('Wave 4 lane TUI-ECHO: the attach client\'s cost per keystroke', wt, 'perf/tuiecho', 180) + f'''
The W4-DELIVER plan (bench/perf/campaign/w4-deliver-plan.txt in the worktree) found that no wave-4 lane covers the raw-terminal attach client's share of echo latency:

{[l for l in plan_asis().splitlines() if l.startswith('11.')][0]}

{plan_line('macbook echo.p50.idle')}

Earlier Linux breakdown (before W3-TUI, which then cut about 250 us): TUI client relay ~390 us of a 2.03 ms p50; W3-TUI runs the TUI client on one thread and one event loop (crates/zz-tui; the binary entry is crates/zz-cli). The final-stage rules: echo.p50.idle <= 1.5x tmux (Mac 0.589 ms against 0.262; Linux 1.78 against 0.928).

Task: profile the zz TUI client (not the daemon) per keystroke on the Mac: `python3 bench/perf/campaign/w4-deliver-echo.py <zz_cli> {lb.SCRATCH}/tuiecho/echo-<label>.json` reports client kinstr and CPU per key; `sample <client pid> 5` during a typing loop shows where it goes. Cut the work between reading a key from the tty and writing the echoed cell to the tty: input decode and key encoding, the request to the daemon (one write, no extra wake or allocation per key), frame receipt and decode, the patch application, and the paint (only the changed cells and the cursor; no full-row or status rework for a one-cell echo; one write to the tty per frame). Write zone: crates/zz-tui and the TUI entry in crates/zz-cli; crates/zz-client only for the sans-IO reduction the TUI calls per frame (keep the GUI, web and iOS behaviour identical; their tests must pass). Not yours: the daemon (DELIVER's slices), libghostty row extraction (W4-ROWS). KNOBS deletes the `ZZ_PERF_TUI_COALESCE` knob in crates/zz-tui/src/lib.rs at the same time: do not touch that static or its uses beyond what your change needs, and keep the default (coalescing) behaviour.

Done criterion: zz TUI client instructions per echoed key at most 40 kinstr (105 today) and CPU per key at most 30 us (70 today), from w4-deliver-echo.py medians over three runs against the base binary in the same session; echo.p50.idle on the Mac at least 30 us lower than the base in three alternating quick pairs (`python3 bench/perf/run.py --zz <bin> --stage final --quick --only echo --w0 none --json ...`, then `python3 bench/perf/campaign/scripts/ab-compare.py <dir>` with ab-pre-N/ab-post-N names); `chatty.client_cpu_pct.visible` not regressed; `cargo test -p zz-tui -p zz-client -p zz-cli`, clippy -D warnings on touched crates, and compat/tui-screen-diff.sh, compat/tui-copy-mode.sh, compat/tui-overlays.sh, compat/tui-choosers.sh and compat/attached-client.sh pass (`/opt/homebrew/bin/bash compat/<f>.sh $PWD/target/debug/zz_cli $PWD/compat/.cache/tmux-src/tmux`); Linux is NOT RUN here (the orchestrator measures it).'''

def dl1():
    wt = f'{lb.ROOT}/zz-deliver'
    lw = '/home/demfabris/dev/zz-deliver'
    return header('Wave 4 lane W4-DELIVER slice DL1: current command, cwd and activity off the frame path', wt, 'perf/deliver', 180).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/wave4 at bce8d6a5, the KNOBS merge: every ZZ_PERF_* knob is gone') + f"""
The W4-DELIVER plan (bench/perf/campaign/w4-deliver-plan.txt in the worktree) found this as-is state:

{plan_asis()}

Your slice, verbatim from the plan:

{plan_line('DL1 |')}

Linux leg: the slice changes the Linux /proc readers too. Build and measure there in the detached worktree {lw} on alienware: `cd {wt} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/deliver:perf/deliver && ssh alienware 'cd {lw} && git checkout -q --detach perf/deliver && ulimit -n $(ulimit -Hn) && cargo build --release -j6 -p zz-cli'` (never check out a branch there), each ssh call under 10 minutes (`setsid -f ... > log 2>&1 < /dev/null` for long runs, then poll), timing runs through `OWN={lw} ~/.cache/zz-perf/quiet-gate.sh --exec <cmd>`, Linux base binary /home/demfabris/dev/zz-perf-int/target/release/zz_cli, scratch /tmp/zzpc/deliver on alienware. Push your final commit there too.

DL2 starts on top of your commit as soon as you finish, so keep the slice to its write zone. Done criterion: the DL1 done criterion above (Mac and Linux rows), with the rows that move only on a quiet host judged in three alternating runs against the base binary in the same session."""

def dl2():
    wt = f'{lb.ROOT}/zz-deliver'
    lw = '/home/demfabris/dev/zz-deliver'
    return header('Wave 4 lane W4-DELIVER slice DL2: per-pane stream sequence, one encode per frame', wt, 'perf/deliver', 150).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/wave4 at 37822fbf: KNOBS, ROWS, TUI-ECHO, CONTROL, DL6, PTYLEAK, TEARDOWN and DL1 merged') + f"""
The W4-DELIVER plan (bench/perf/campaign/w4-deliver-plan.txt in the worktree) found this as-is state (DL1 has since taken name checks off the frame path, DL6 folded the Linux gather threads, CONTROL hands control clients' stdio to the daemon in daemon/control_stdio.rs, and ROWS made frame build copy whole rows):

{plan_asis()}

Your slice, verbatim from the plan:

{plan_line('DL2 |')}

Before changing Event.sequence semantics, list every reader of `Event.sequence` for TerminalPatch and TerminalViewport in crates/zz-client, crates/zz-tui, crates/zz, clients/gpui-shared, clients/web, crates/zz-client-ffi and crates/zz-gpui-ios (rg) and put the list in the report; if any reader relies on the global order, keep that reader working. Linux leg: build and run the gate's Linux line in the detached worktree {lw} on alienware (`cd {wt} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/deliver:perf/deliver && ssh alienware 'cd {lw} && git checkout -q --detach perf/deliver && ulimit -n $(ulimit -Hn) && cargo build --release -j6 -p zz-cli'`; never check out a branch there; each ssh call under 10 minutes; no quiet-gate for instruction rows). DL3 starts on top of your commit, so keep to the write zone. Done criterion: the DL2 done criterion above."""

def linux_clippy_note(slug):
    return f"""Linux check before you finish (a Mac lane once merged code that failed Linux clippy): `cd {lb.ROOT}/zz-{slug} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/{slug}:perf/{slug} && ssh alienware 'cd /home/demfabris/dev/zz-{slug} && git checkout -q --detach perf/{slug} && ulimit -n $(ulimit -Hn) && timeout 580 cargo clippy -j6 -p <touched crates> --all-targets --all-features -- -D warnings'` (that worktree is detached with a warm target; never check out a branch in it); report its exit code. Linux tests and timings are NOT RUN unless the brief asks for them."""

def settle():
    wt = f'{lb.ROOT}/zz-settle'
    return header('Wave 4 lane SETTLE: no full snapshot for a pane nobody watches', wt, 'perf/settle', 120).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/wave4 at 08f56d73 (pushed to main): KNOBS, ROWS, TUI-ECHO, CONTROL, DL6, PTYLEAK, TEARDOWN and DL1 merged') + f"""
The SPAWN lane measured on Linux that `settle_unwatched` builds a full snapshot about 100 ms after a pane spawns: about 50% of daemon user instructions for `split-window -d -P` (about 0.9 Minstr and 214 us per split), and the main reason `spawn.cpu.split_empty_P` stays above the final rule (1.2x tmux): Mac 0.95 ms against tmux 0.45 (2.1x), Linux 1.38 against 1.11. ROWS has since made frame build copy rows in one call, so the cost may be lower now; measure first. Related as-is facts from the W4-DELIVER plan: unwatched panes notify the loop at most every 100 ms (session.rs `UNWATCHED_NOTIFY_INTERVAL`, `Frames::admit_notify`) and `publish_views` builds snapshots for unwatched fallbacks.

Task: find what the settle snapshot is for (who reads it: capture-pane, previews, wait-pane, status, choose-tree previews, hooks, the attach path), then stop building it when no consumer needs it, and build it on demand (or keep only the cheap part) when one asks, so every consumer sees exactly what it sees today. tmux parity: `capture-pane -p` right after `split-window -d` must match tmux (compat rows), and attaching to a pane that was never watched must paint the same first frame. Write zone: zz-terminal session.rs settle and unwatched-fallback code (`settle_unwatched`, `admit_notify`, `UNWATCHED_NOTIFY_INTERVAL` and the fallback branch of `publish_views`), session/pane_actor.rs deadline handling for that settle; tests next to them. Not yours: the daemon publish and encode path (DL2 runs now in ~/dev/zz-deliver), the Linux gather code (DL6b), unix_pty.rs spawn (SPAWN), DL1's EventQueueState fields.

Done criterion: on the Mac, three alternating quick runs of `python3 bench/perf/run.py --zz <bin> --stage final --quick --only spawn,chatty --w0 none --json {lb.SCRATCH}/settle/<label>-<n>.json` against the base build ({lb.INT}/target/release/zz_cli, rebuilt by you from perf/wave4 if older than 08f56d73): `spawn.instr.split_empty_P` at least 30% below the base and `spawn.cpu.split_empty_P` at or under 1.2x the same runs' tmux median, or the remaining cost itemised with sample shares if the rule cannot be met inside the zone; `chatty.instr_per_s.flip` and `.hidden` not above the base; `cargo test -p zz-terminal`, `cargo test -p zz-daemon` (re-run load failures alone), clippy -D warnings on touched crates; compat rows whose names contain capture, split, preview, choose, wait or attach pass or match the base; `compat/attached-client.sh` and `compat/tui-screen-diff.sh` pass (run them with LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8; the tmux side of attached-client flakes under load, so a tmux-side failure alone is not yours). {linux_clippy_note('settle')} One commit on perf/settle."""

def names():
    wt = f'{lb.ROOT}/zz-names'
    return header('Wave 4 lane NAMES: a cheap foreground name on macOS', wt, 'perf/names', 90).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/wave4 at 08f56d73 (pushed to main): KNOBS, ROWS, TUI-ECHO, CONTROL, DL6, PTYLEAK, TEARDOWN and DL1 merged') + f"""
DL1 moved pane name lookups to one check per pane per 500 ms (`run_due_name_checks`, `terminal_foreground_facts`). Its open note: on macOS each check costs about 60 us because the foreground pid changes with every short-lived command (`sleep` in the chatty workloads) and the 4-entry recent-name cache in the daemon's process-info code misses, so every check runs `KERN_PROCARGS2` (`fill_arguments`). tmux's darwin `osdep_get_name` reads `kinfo_proc.kp_proc.p_comm` with one `sysctl(KERN_PROC_PID)` for the tcgetpgrp result.

Task: make the common name lookup cheap on macOS while keeping every current result identical. zz deliberately resolves some names from argv (interpreters and agent CLIs, for `pane_current_command`, automatic-rename and agent detection): find exactly which cases need argv, keep those, and take the cheap path (one sysctl or `proc_pidinfo(PROC_PIDTBSDINFO)` for `pbi_comm`/`pbi_name`) for the rest; size the cache by what the workloads need and key it so pid reuse cannot return a stale name (pid plus start time). Linux: keep /proc behaviour unchanged unless an equivalent cut is obvious and covered by the same tests. Write zone: the daemon's process-info module (find it with `rg -n "KERN_PROCARGS2|fill_arguments|RECENT_NAMES" crates`) and its tests. Not yours: DL1's timers and `terminal_foreground_facts` callers.

Done criterion: a probe that switches the foreground job of a pane every 20 ms (a shell loop running `sleep 0.02`) and times the lookup shows the median cost per name check on macOS at or under 15 us (60 today) with identical names; existing process-info tests pass, plus new ones for the argv cases you keep and for pid reuse; `chatty.cpu_pct.flip` and `chatty.instr_per_s.flip` on the Mac not above the base in three alternating quick runs (`--only chatty`); compat rows automatic-rename, formats-values, pane-runtime-facts and every row whose name contains rename or current pass or match the base; `cargo test -p zz-daemon` (re-run load failures alone), clippy -D warnings. {linux_clippy_note('names')} One commit on perf/names."""

def tuiecho2():
    wt = f'{lb.ROOT}/zz-tuiecho2'
    return header('Wave 4 lane TUIECHO2: the attach client\'s remaining per-key work', wt, 'perf/tuiecho2', 120).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/wave4 at 08f56d73 (pushed to main): KNOBS, ROWS, TUI-ECHO, CONTROL, DL6, PTYLEAK, TEARDOWN and DL1 merged') + f"""
TUI-ECHO (merged) cut the raw-terminal attach client from 108.6 to 92.1 kinstr per echoed key (kqueue wait on macOS, one socket read per wake, 5-byte echo writes); a minimal relay with the same six syscalls costs 55.5 kinstr, so the user-space part left is about 37 kinstr. It found three costs outside its zone, which are this lane: (1) encoding the key message (`encode_protocol_message_into`, zz-protocol and the client side in crates/zz-daemon/src/client.rs) is about 12% of the client's user cycles per key; (2) `ClientCore::claims_prefix_input` (crates/zz-client) allocates through `canonical_key` on every key, about 6%; (3) every frame copies the full cell array (38 KB) because the model and the renderer each hold a clone of the cells, about 8%.

Task: remove those three: encode key input into a reused buffer without intermediate allocation, compare against prefix keys canonicalised once when the key tables or options change, and let the renderer read the model's cells without a per-frame copy (shared ownership or rendering from the model). Keep the GUI, web, iOS and FFI behaviour of zz-client identical (their tests must pass). Write zone: crates/zz-tui, crates/zz-client `ClientCore` input and prefix path, the client-side key message encode (not crates/zz-protocol terminal_codec.rs or pane_frame.rs, which DL2 owns now). Measure with `python3 bench/perf/campaign/w4-deliver-echo.py <zz_cli> {lb.SCRATCH}/tuiecho2/echo-<label>.json` (client kinstr and CPU per key).

Done criterion: client kinstr per echoed key at most 80 (92 today), medians of three alternating runs against the base build ({lb.INT}/target/release/zz_cli, rebuilt by you from perf/wave4 if older than 08f56d73); `chatty.client_cpu_pct.visible` not above the base (full `--only chatty`, two alternating pairs); `cargo test -p zz-tui -p zz-client -p zz-cli -p zz-protocol -p zz-client-ffi`, clippy -D warnings on touched crates and their dependents (`zz`, `zz-web` included); compat/tui-screen-diff.sh, tui-copy-mode.sh, tui-overlays.sh and attached-client.sh pass with LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 (tui-choosers.sh differs in 25 rows on the base too; compare with the base). {linux_clippy_note('tuiecho2')} One commit on perf/tuiecho2."""

def dl3():
    wt = f'{lb.ROOT}/zz-deliver'
    lw = '/home/demfabris/dev/zz-deliver'
    return header('Wave 4 lane W4-DELIVER slice DL3: shard sinks for foreground live views', wt, 'perf/deliver', 210).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/wave4 at 8b1393a2: KNOBS, ROWS, TUI-ECHO, CONTROL, DL6, PTYLEAK, TEARDOWN, DL1, SPAWN and DL2 merged') + f"""
The W4-DELIVER plan (bench/perf/campaign/w4-deliver-plan.txt in the worktree) found this as-is state before wave 4's merges:

{plan_asis()}

Since then: DL1 took name checks off the frame path (`TerminalWatcher::handle` no longer calls `synchronize_pane_runtime`; output frames call `note_pane_output`; a 500 ms `TimerKey::NameCheck`), DL2 made frames carry the pane's stream sequence (the current viewport's `view_generation`) and encodes each patch once per (base, current) generation in `PaneFrameFanout::enqueue` and each full frame once per (pane, generation) in `TerminalFrames::full` (on `SharedServer`; you will need it reachable from the shards), with `PendingTerminal.encoded` an `Arc<[u8]>`; ROWS made `Frames::snapshot` copy whole rows (`CellIteration::copy_into`); DL6 put Linux PTY reads on one gather thread per shard; SETTLE (a side lane running now in ~/dev/zz-settle) may change the unwatched-pane settle snapshot in `publish_views`' fallback branch: keep your sink edits to the live-view branch so the two merge cleanly.

Your slice, verbatim from the plan:

{plan_line('DL3 |')}

Linux leg: the gate's alienware line runs in the detached worktree {lw} (`cd {wt} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/deliver:perf/deliver && ssh alienware 'cd {lw} && git checkout -q --detach perf/deliver && ulimit -n $(ulimit -Hn) && cargo build --release -j6 -p zz-cli'`; never check out a branch there; each ssh call under 10 minutes; keep measured binaries on disk, not in tmpfs /tmp; no quiet-gate for instruction rows). Run `cargo clippy` for the touched crates on Linux too. Done criterion: the DL3 done criterion above."""

DL3_STATE = """DL3 (on this branch, not merged into perf/wave4 yet: `443939fa` + review fixes `41dce0ce`) delivers foreground live frames from the shard: `watch_terminal` installs a `PaneSink` per pane (crates/zz-daemon/src/daemon/shard_sink.rs) holding per (pane, view) records with the mailbox and the view's last frame, kept current by `apply_view_streams`, detach, subscribe, `enter_copy_session`/`exit_copy_session` and `retire_terminal`; the frozen flag lives on the mailbox; `publish_views` hands each frame to the sink, which diffs, encodes once per (base, current) and queues on every matching mailbox under the sink's lock; non-live views, frozen clients, kitty frames and clients whose slot still holds an unwritten frame take the loop path for that frame (`shard_sink::publish_loop_view` from `TerminalWatcher::handle`), keeping the record's base; a pane whose views all went to the sink notifies the loop only on edges (title, OSC 7 path, status, progress bar, preview) or output at most every 100 ms (every frame while an output watch from terminal_reads is alive); sink wakes ride the frame's publish and the mailbox wakes the loop only when a client's terminal slot goes from empty to non-empty. Its one miss: loop busy samples in chatty visible stayed at 15-17 (base 13-24): the loop still wakes and calls writev once per pane frame; about half the loop samples are writev, the rest per-wake turn overhead. Tests: shard_sink_tests.rs (detach during publish, window switch, freeze, NeedsFull, kitty fallback, copy mode, one encode with 2 clients, retire, title sync, the fallback base cases)."""

def dl45_common(slug, n):
    wt = f'{lb.ROOT}/zz-deliver{n}'
    lw = f'/home/demfabris/dev/zz-deliver{n}'
    return wt, lw, f"""Start by merging the integration branch into yours: `cd {wt} && git merge perf/wave4` (it adds SETTLE, NAMES, TUIECHO2, the gather fixes and doc commits that landed after DL3's base; resolve conflicts keeping both sides; commit the merge). The DL3 commits are already on your branch.

{DL3_STATE}

Linux leg: build and measure in the detached worktree {lw} on alienware (`cd {wt} && git push -q -f ssh://alienware/home/demfabris/dev/zz perf/{slug}:perf/{slug} && ssh alienware 'cd {lw} && git checkout -q --detach perf/{slug} && ulimit -n $(ulimit -Hn) && cargo build --release -j6 -p zz-cli'`; never check out a branch there; each ssh call under 10 minutes; measured binaries on disk, not tmpfs /tmp; no quiet-gate for instruction rows). Run Linux clippy for the touched crates too. The base for every A/B is DL3's head 41dce0ce built as release (Mac: build it in a scratch detached worktree with a cloned target; Linux: the same under ~/dev)."""

def dl4():
    wt, lw, common = dl45_common('deliver4', 4)
    return header('Wave 4 lane W4-DELIVER slice DL4: the shard writes an idle client socket directly', wt, 'perf/deliver4', 210).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/deliver (DL3, 41dce0ce, on perf/wave4 8b1393a2)') + f"""
{common}

Your slice, verbatim from the plan (bench/perf/campaign/w4-deliver-plan.txt):

{plan_line('DL4 |')}

Notes for this run: CONTROL (merged) hands a control client's stdin/stdout to the daemon (crates/zz-daemon/src/daemon/control_stdio.rs, `Connection::write_stdio_ready` in event_loop.rs); control clients do not receive terminal frames, so the direct path is for interactive (GUI, TUI, web, iOS) connections; leave control connections on the loop path unless it is trivial and covered by tests. DL3 left loop busy at 15-17 in visible: this slice owns that item too: the done criterion adds `w4-deliver-profile.py` loop busy samples in visible at or under 10 on the final code (three runs). Done criterion: the DL4 done criterion above plus that loop-busy item."""

def dl5():
    wt, lw, common = dl45_common('deliver5', 5)
    return header('Wave 4 lane W4-DELIVER slice DL5: control output as a shard sink, the per-pane stream barrier', wt, 'perf/deliver5', 210).replace('from perf/wave4 = origin/main 06ea9cf1 plus a brief-generator commit', 'from perf/deliver (DL3, 41dce0ce, on perf/wave4 8b1393a2)') + f"""
{common}

Your slice, verbatim from the plan (bench/perf/campaign/w4-deliver-plan.txt):

{plan_line('DL5 |')}

Notes for this run: CONTROL (merged) hands a control client's stdin/stdout to the daemon; the daemon writes plain command replies and `%output` straight to the handed stdout when the client is idle (crates/zz-daemon/src/daemon/control_stdio.rs), and the client renders the rest; `pump_control_output_at` still renders `%output`/`%extended-output`. DL2 (merged) made terminal frames carry the pane's stream sequence (the current viewport's `view_generation`), so frame and event sequences no longer compare: the barrier must use a per-pane sequence assigned where bytes are read, keyed by the terminal session (not the pane id: respawn reuses PaneId). PTYLEAK (merged) found that the tap's `DisarmRawOutputTap` could fill the actor's one-slot mailbox and drop `Shutdown` (`TerminalSession::drop` now defers it); deleting the taps removes that path, keep the deferral. DL4 runs in parallel in ~/dev/zz-deliver4 on the mailbox write path: stay off `OutboundMailbox::enqueue_terminal_with`, `notify_one` and `Connection::write_ready`. Windows: `DOCS_RS=1 cargo check -p zz-daemon --target x86_64-pc-windows-msvc` (skips the zig build) must give no new warnings against the base and no errors. Done criterion: the DL5 done criterion above."""

if __name__ == '__main__':
    which = sys.argv[1]
    text = {'knobs': knobs, 'deliver-plan': deliver_plan, 'binary': binary, 'rows': rows, 'spawn': spawn, 'gather': gather, 'control': control, 'tuiecho': tuiecho, 'dl1': dl1, 'dl2': dl2, 'settle': settle, 'names': names, 'tuiecho2': tuiecho2, 'dl3': dl3, 'dl4': dl4, 'dl5': dl5}[which]()
    out = sys.argv[2] if len(sys.argv) > 2 else f'{OUT}/{which}.md'
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, 'w').write(text)
    print(out, len(text.splitlines()), 'lines')
