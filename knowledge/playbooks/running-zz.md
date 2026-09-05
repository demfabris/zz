---
type: Playbook
title: Building and running zz
description: How to build and run the zz GPUI client and its daemon, what the first build downloads, and how to exercise the browser pane with the loopback fixture.
resource: crates/zz/src/lib.rs
tags: [running, cargo, cef-download, daemon, browser-fixture, pacman, profiling, instruments]
timestamp: 2026-09-05T00:00:00Z
---

# Overview

The `zz` package (crate at `crates/zz`) provides the GPUI client, daemon/CLI entrypoints, and CEF
helper executable. Linux can run the main binary directly from Cargo. macOS and Windows must launch
the platform bundle so Chromium can find its framework/resources, sandbox helpers, or bootstrap
DLL. A first native build additionally downloads and verifies a matching CEF distribution and
compiles CEF's C++ wrapper; see
[prerequisites](/playbooks/prerequisites.md) for the toolchain this requires and
[build/verify a CEF bundle](/playbooks/build-cef-bundle.md) for the packaged (non-`cargo run`)
equivalent.

# Examples

Start zz. It connects to an already-running daemon, or starts one automatically:

```sh
# Installed macOS or Linux package
zz app

# Linux
cargo run -p zz

# macOS
cargo xtask bundle-cef --release --output dist/zz
open dist/zz/zz.app

# Windows
cargo xtask bundle-cef --release --output dist/zz
dist\zz\zz.exe
```

The installed macOS and Linux `zz` command is the tmux-compatible launcher. Bare `zz` rewrites to
`new-session -A` and enters the raw-terminal client: an empty daemon creates numeric session `0`,
while a live daemon attaches its current session. Explicit `zz attach` and `zz attach-session`
remain tmux-compatible and return `no sessions` on an empty daemon. Use `zz new -s NAME` when the
first session needs a name and `zz app` for a new GUI process. Direct bundle launches and
`cargo run -p zz` keep the no-argument GUI behavior used by development tools and platform
launchers.

`zz app` starts the first session in the directory where you ran the command. A Dock or Finder
launch starts it in your home directory. The GUI includes that path when it creates a session, so
the first terminal does not inherit the process directory of an empty daemon started earlier.
The desktop window opens while a background task starts or connects to the daemon and reads its
handshake. The local host stays in `Connecting` until that task finishes; terminal content follows
the default attach. `MuxClient::new_connecting` keeps daemon startup and socket reads off the UI
thread, and preserves the stale-daemon restart prompt when the handshake reports a version mismatch.

`ZZ_SOCKET` owns daemon routing. `TMUX` carries compatibility metadata inside a pane or plugin job;
zz refuses to parse a real tmux socket from it. An explicit `-S`, `-L`, or `--socket` still selects
a zz endpoint. On Unix, daemon-spawned shell and status jobs export the exact `ZZ_SOCKET` and
prepend a private `tmux` shim to their `PATH`, so config and plugin scripts launched by zz can keep
invoking the tmux command name.

A daemon started by a CLI query can remain empty. The GUI's first default Interactive attach
materializes numeric session `0` only when no session exists, so CLI-first allocation keeps tmux's
zero-based session/window/pane ids. The installed TUI launcher now shares this behavior. First
materialization is serialized, so simultaneous default attaches and a command-side session creator
converge on one session.

The normal development loop is `just run <mac|linux>`. On Linux this runs the binary straight from
Cargo; on macOS it builds an unoptimized, locally signed bundle separately from the release output
and launches a fresh app instance. `just run` does not accept `windows`. Extra args are
`--verbose` and `--features <list>` (merged into `ZZ_CARGO_FEATURES`), not Cargo passthrough.

To verify the installed-command shape without replacing `/Applications/zz.app`, build the release
bundle and run its packaged launcher fixture:

```sh
just build mac
compat/packaged-cli.sh dist/zz/zz.app
```

The fixture clones the complete signed bundle beneath a space-containing temporary path and checks
bare, `new`, and `attach` against empty and existing isolated daemons. It never installs or notarizes
the app.

`zz attach [session]` is the raw-terminal client (`zz-tui`). It speaks the same wire protocol as
the GUI and can render kitty-graphics browser panes when CEF is available in the environment.

```sh
just run linux   # cargo run -p zz
just run mac     # dev bundle in dist/zz-dev + open a fresh instance
```

For UI iteration, `just watch <platform>` uses Cargo watch to rebuild and relaunch only after a
successful build:

```sh
just watch linux
just watch mac
```

The current client remains open across compile failures. After a successful build the watcher
starts the replacement client, closes the previous development client, and leaves the compatible
daemon plus its terminal sessions running. Rebuild and relaunch are native, so transient
client-side UI and Chromium renderer state reset; nothing hot-reloads in process.

`just build linux` / `just build mac` produce the release bundle (`cargo xtask bundle-cef --release`)
and must run on the target platform. On Linux `bundle-cef` unlinks the bundle's executable before
writing the new one: the daemon outlives the app and normally runs straight out of `dist/zz`, and
copying over a running executable fails with `ETXTBSY`. Dropping the directory entry instead leaves
that daemon on the inode it started from, so it keeps serving the previous build until restarted.

macOS local bundles auto-select a sole valid Apple Development identity. This keeps TCC privacy
grants for protected app data attached to `dev.zz.app` across rebuilds. With no unique candidate,
bundling falls back to ad-hoc signing; `MACOS_LOCAL_SIGN_IDENTITY` selects an identity by name or
SHA-1, while `MACOS_LOCAL_SIGN_IDENTITY=-` forces ad-hoc signing. Public release signing remains a
separate Developer ID/notarization step.

On Arch Linux, package that release bundle with `just pacman-package`, or build and install it in
one step with `just pacman-install`. The installed `/usr/bin/zz` symlink resolves to the `cli`
launcher beside the complete CEF runtime under `/usr/lib/zz`. On Debian and Ubuntu the pair is `just deb-package` (emitting
`dist/zz-linux.deb`) and `just deb-install`, which installs it through `apt` so the computed
dependencies resolve. The deb also installs `/etc/apparmor.d/zz`; without that profile, Ubuntu
24.04+ denies the user namespace the browser panes' zygote needs.

On macOS, `just install mac` builds the release bundle and swaps it into `/Applications/zz.app` in
one step: it quits a running GUI instance first (CEF spawns helpers by bundle path, so the bundle
must never be deleted under a live GUI), replaces the bundle with `ditto`, and relaunches. The
daemon . same binary, ` daemon`-suffixed command line . is deliberately left running so sessions
survive the swap; it serves the previous build until restarted. Local signing keeps the
`dev.zz.app` identity, so TCC grants persist across installs. The script also tries to put `zz` on
`PATH` the way the Homebrew cask does: it links the bundle's `cli` launcher (never the real binary,
which would run bundle-less) into the first existing, writable candidate already on `PATH`, checking
`/opt/homebrew/bin` then `/usr/local/bin`. It leaves any foreign `zz` untouched and prints a manual
link command if neither candidate qualifies. Run `zz app` through that launcher to open the GUI;
bare `zz` runs `new-session -A` through the raw-terminal client.

The recipe writes `dist/zz-dev/zz.app`. Its debug CEF runtime uses Chromium's mock keychain so
local rebuilds do not repeatedly prompt for Chromium Safe Storage; release bundles continue using
the macOS Keychain. A compatible daemon already listening on the default socket is reused, so stop
it separately when daemon-side changes must be exercised.

Run with continuous diagnostics (records a `.verbose.log`; treat as sensitive, since it logs raw
terminal I/O, browser URLs/input, and process environment):

```sh
# Linux
just run linux --verbose

# macOS
just run mac --verbose

# Windows
dist\zz\zz.exe --verbose
```

The same global flag works on CLI-style commands. This example uses the Linux development launcher;
on macOS use `dist/zz/zz.app/Contents/MacOS/zz`, and on Windows use `dist\zz\zz.exe`:

```sh
cargo run -p zz -- --verbose list-panes
```

`ZZ_LOG_DIR` overrides the log directory; otherwise logs default to `$XDG_STATE_HOME/zz/logs` (or
`~/.local/state/zz/logs`) on Linux, `~/Library/Logs/zz` on macOS, and `%LOCALAPPDATA%\zz\logs` on
Windows. The `just run` development launcher defaults `ZZ_LOG_DIR` to the git-ignored `logs/`
folder in the repository and executes the macOS bundle binary directly (not via `open -n`), so
`ZZ_BROWSER_*` environment flags set in the shell reach the app.

Production runs (no `--verbose`) always log too: the app and daemon roles write info-level records
to `zz.app.log` and `zz.daemon.log` in that same directory, size-capped as a two-generation ring
(8 MiB live + one `.log.old` predecessor), and CEF writes its own warnings to `cef.log` there
(previous session preserved as `cef.log.old`). After a native crash, pair the tail of those files
with the `.ips` report from `~/Library/Logs/DiagnosticReports` . the ring guarantees the last
8–16 MiB of app-side history survived. When stderr is a terminal the records are mirrored there.

When something feels off mid-session, flag it instead of trying to remember the time:
`cmd-shift-m` (`ctrl-shift-m` on Linux/Windows) stamps `user_marker seq=N` plus a state snapshot
into both ring logs and confirms with a toast, and `zz debug-marker freeze while agent was busy`
does the same from any shell. Main-thread freezes need no keypress . an always-on watchdog logs
`main_thread_stall` / `main_thread_stall_recovered duration_us=…` for any pause over 500 ms . and
every prefix-chord key logs its `key_decision` in the daemon log. Query afterwards with
`rg 'user_marker|main_thread_stall' ~/Library/Logs/zz/zz.*.log*` and read the surrounding lines.
On macOS both hooks also answer *what the app was doing*: a marker or a watchdog-detected stall
triggers `/usr/bin/sample` against the live process (2 s of 10 ms stacks, every thread . works
because the dev-installed bundle is not hardened) and writes `zz.stall-<unix-seconds>.sample.txt`
beside the ring logs, newest 8 kept, at most one per minute. The log line names the file; read
the main-thread call graph at the top of it.

CEF frames in an `.ips` report symbolicate to junk (nearest-export names like
`v8_internal_simulator_ProbeMemory`), but CEF publishes real symbols for every build. Take the
exact version string from `third_party/cef/ARTIFACTS.md`, download
`cef_binary_<version>_<platform>_release_symbols.tar.bz2` from `cef-builds.spotifycdn.com`
(URL-encode the `+`s; ~2 GB), extract the dSYM's `DWARF` file (GNU tar needs `--wildcards`), pull
each frame's `imageOffset` out of the `.ips` JSON body, and run
`atos -o "<dSYM>/Contents/Resources/DWARF/Chromium Embedded Framework" -offset <offset…>` . real
function names for the whole stack. This is how the 2026-08-03 Immersive-Reading-Mode login crash
was pinned (see the [update log](/log.md)).

## Profiling the macOS bundle

The profiling workflow deliberately uses the real CEF bundle rather than a raw Cargo binary. Build
the release-optimized `profiling` Cargo profile once:

```sh
just profile-build mac
```

This writes `dist/zz-profile/zz.app` plus matching `zz.dSYM` and `zz_helper.dSYM` bundles under
`dist/zz-profile/symbols/`. The build inherits release optimization, fat LTO, and one codegen unit,
but retains full DWARF so Instruments can resolve source lines and inlined Rust frames. `xtask`
compares each Mach-O UUID with its copied dSYM before accepting the bundle. It also forces
`libghostty-vt` to Zig `ReleaseFast`: Cargo exposes `DEBUG=true` to build scripts when this profile
emits DWARF, and the native dependency would otherwise select Zig `Debug`, retain expensive terminal
integrity assertions, and make profiling results unlike the normal release bundle.

Capture one question at a time:

```sh
just profile-cpu mac                 # Time Profiler attached to the GUI
just profile-cpu mac daemon 30s      # Time Profiler attached to the daemon
just profile-cpu mac all 20s         # broader, noisier all-process CPU trace
just profile-cpu-summary <run-dir>   # retain only the isolated owned process tree
just profile-system mac 20s          # scheduling, waits, wakeups, and IPC
just profile-metal mac 20s           # GPUI and CEF Metal work
just profile-metal-summary <run-dir> # zz-only frame and GPU summary
just profile-startup mac 8s          # launch to the first attributed displayed frame
just profile-terminal-diagnostics mac 20s
just profile-terminal-summary <run-dir>
```

`scripts/profile-macos.sh` creates a short private socket path and launches the profiling bundle
directly with `--socket`, so the GUI cannot reuse the normal persistent daemon. It
waits for the daemon's private identity file, records the owned process tree at launch, capture
start, and completion plus relevant `ZZ_BROWSER_*` controls, runs `xctrace`, then terminates only
the GUI and daemon it launched.
It refuses to start beside another copy of the profiling bundle because macOS UI automation cannot
reliably distinguish two windows with the same bundle identity. Metal capture attaches to the
isolated GUI PID; an all-process Metal trace can omit the application's command-buffer submission
rows even while recording unrelated system GPU work. Captures and metadata land in the git-ignored
`target/profiles/<run>/` directory. `scripts/summarize-macos-cpu.py` exports Time Profiler samples
and filters an all-process trace to those owned PIDs, grouping the GUI, daemon, Chromium renderer,
GPU, network, utility, and other children. It reports both leaf work and inclusive zz
call stacks so subsystem entry points are visible without manually exporting the trace.
`scripts/summarize-macos-metal.py` exports the relevant
Instruments tables into a temporary directory, filters them to the recorded GUI PID, and reports
command buffers, presents, GPU execution, channel time, and CPU-to-GPU latency using only Python's
standard library.

Measure memory without Instruments or verbose logging:

```sh
just profile-memory mac 60s
ZZ_PROFILE_APP=/Applications/zz.app just profile-memory mac 60s
python3 scripts/sample-macos-memory.py --pid GUI_PID --daemon-pid DAEMON_PID \
    --duration 60s --output target/profiles/live-memory
```

The memory recipe uses the same isolated daemon lifecycle as the other captures.
It accepts an installed bundle without dSYMs. The standalone sampler attaches to
existing processes and leaves them running; omit `--daemon-pid` to measure only
the GUI and its descendants. Both write `memory.csv` and `memory-summary.txt`,
with 250 ms samples and separate GUI, daemon, and descendant totals.

`proc_pid_rusage` supplies physical footprint and resident size (RSS) separately.
Use footprint to investigate changes in the app's memory charge, including
graphics resources that RSS can miss. Compare high and low samples with
`vmmap -summary PID` to identify changing categories. `vmmap` can pause the
target, so keep those snapshots separate from clean timing measurements.
See [Apple's memory profiling explanation](https://developer.apple.com/videos/play/wwdc2022/10106/).

Memory and attached Instruments captures start after daemon readiness and warmup,
including when warmup is zero. `profile-startup` instead launches the bundle under
Metal System Trace, with no warmup or dSYM requirement. For an existing release bundle:

```sh
ZZ_PROFILE_APP=/Applications/zz.app just profile-startup mac 8s
python3 scripts/summarize-macos-startup.py target/profiles/<startup-run>
```

`startup-summary.json` identifies the earliest displayed surface whose structured
process record matches the launched GUI PID. It resolves Instruments XML references
and reports unavailable when that attribution is absent. Launch-relative time includes
Instruments overhead; alignment with the logger clock is approximate. UI render logs
and the first render containing a terminal remain separate CPU milestones. Neither
establishes when terminal content reached the display.

The 2026-09-05 startup comparison used matching release builds and a private minimal config.
Three uninstrumented pairs reduced the median first workspace render from 169 to 123 ms with
a fresh daemon. Two alternating Metal launch pairs measured approximately 225 versus 231 ms
from logger initialization to the first displayed frame, so that sample showed no display-latency
improvement. Holding the daemon handshake for 1.5 seconds prevented any baseline workspace render;
the background-connection build rendered at 123 ms while still waiting for handshake bytes.

The 2026-09-05 macOS retention sweep grew an isolated daemon from one terminal to nine,
then closed eight. Ghostty's page mappings returned from 18 to 2 and libc live allocations
returned from 18.5 MiB to the original 8.52 MiB, while libc retained 21.7 MiB of dirty
pages versus 9.34 MiB initially. Repeated create/close cycles plateaued. This supports
working terminal teardown with allocator retention; it does not establish a terminal
object leak or justify forced allocator collection. `TerminalSession::drop` and the
PTY actor shutdown release terminal state; Ghostty's `PageList.deinit` frees its page pools.
Check VM tags against the allocator source: this build's mimalloc v3 uses tag 100,
which `vmmap` labels `IOAccelerator` without `(graphics)`. That label alone is not
evidence of daemon GPU allocations.

`profile-terminal-diagnostics` is deliberately separate from clean CPU/Metal captures. It enables
only the existing `zz::diagnostics::terminal_render` trace target and records cache hits, misses,
uncached rows, prepared text rows, and prepaint time in `app.stderr.log`; it does not run
Instruments. `profile-terminal-summary` filters prompt-only startup frames out of its cache ratio
when content-active frames exist. Logging changes frame timing, so use this mode to explain why
renderer work occurs, never as a before/after CPU benchmark.

The default five-second warmup starts after the isolated daemon is ready. Use
`ZZ_PROFILE_WARMUP_SECONDS=10 just profile-metal mac 30s` when a scenario needs more setup time.
For a steady-state CEF comparison, allow 45–60 seconds after opening the browser so helper startup
cannot dominate the sample.
Keep baseline runs quiet: do not add `--verbose` to them. Diagnostics and
Instruments captures are separate modes because trace logging changes hot-path work and can include
environment values, raw terminal data, browser URLs/input, process arguments, paths, and rendered
content. Treat the complete run directory as sensitive.

## First-build CEF download

The first native build or bundle downloads and SHA-1-verifies a matching minimal CEF distribution (see
[CEF artifact pin](/references/cef-artifacts.md)) and compiles the CEF C++ wrapper before `zz` can
run. The extracted browser runtime uses several gigabytes. Set a stable shared cache path so
separate build directories (e.g. multiple worktrees) do not each download an independent copy:

```sh
export CEF_PATH="$HOME/.cache/cef"
```

CI does the equivalent with a keyed cache instead of a fixed path:

```yaml
env:
  CEF_PATH: ${{ github.workspace }}/.cef-cache
steps:
  - name: Cache matching CEF distribution
    uses: actions/cache@v5
    with:
      path: ${{ github.workspace }}/.cef-cache
      key: cef-151.3.14-${{ runner.os }}-${{ runner.arch }}
```

Bump the cache key's version segment (`cef-151.3.14-...`) whenever the CEF pin changes; see
[updating CEF](/playbooks/updating-cef.md).

## Exercising the browser pane: the loopback fixture

`zz_browser_fixture` (a second binary in the `zz` package, `crates/zz/src/bin/zz_browser_fixture.rs`)
is a deterministic, network-free local HTTP server used as the standard example/smoke-test target
for the [CEF browser pane](/browser/cef-runtime.md). It never touches the external network and
serves a fixed page containing a color-channel proof, text input, title mutation, same-session
navigation, a long scroll area, and persistent cookie/local-storage counters. Run it, then point a
browser split at its loopback address:

```sh
# terminal 1
cargo run -p zz --bin zz_browser_fixture

# terminal 2 (Linux launcher; use the bundled executable on macOS/Windows)
cargo run -p zz -- split-browser -h http://127.0.0.1:9324
cargo run -p zz
```

CEF off-screen rendering uses the fastest attached macOS display refresh rate as
its paint ceiling; other platforms default to 60 FPS. Override either with
`ZZ_BROWSER_FPS=1..240`, for example:

```sh
ZZ_BROWSER_FPS=120 just run mac
```

The ceiling applies to the focused pane. A visible unfocused browser normally
uses at most 30 FPS, but wheel input raises it to the ceiling until one second
after the last wheel event. Hidden panes stop painting. Every ceiling is a
maximum; a static page will not necessarily produce continuous frames.

Browser panes keep Chromium's GPU process and shared-texture OSR enabled by
default. Linux/FreeBSD use the wgpu texture tier; macOS uses the
Metal-IOSurface tier. Set `ZZ_BROWSER_SHARED_TEXTURE=0` to keep WebGL, canvas,
and video acceleration while forcing readback everywhere, or set
`ZZ_BROWSER_GPU=0` to force software rendering and compositing as well. On a
GPU import or surface-wrap failure, zz recreates only the affected pane on the
universal readback tier. Linux also guards against producers that fail before
CEF emits a paint callback: if a visible shared-texture session produces no
first frame within two seconds, zz recreates that pane in readback mode. Hiding
the pane pauses the guard, and any delivered frame cancels it.

macOS drives Chromium compositor pacing with external BeginFrames by default:
hot visible sessions start at the frame-rate ceiling on a deadline-anchored
clock, cold visible sessions retain a roughly 30 Hz keepalive, and hidden
sessions receive no BeginFrames. `ZZ_BROWSER_BF_ADAPTIVE=1` opts into an
experimental adaptive divisor that selects a slower hot tier after sustained
delivery shortfall and probes faster again after stable delivery; it stays
off by default because demand-driven delivery gaps (scroll pauses) still read
as shortfalls. Set `ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=0` to restore CEF's
internal BeginFrame clock (which caps shared-texture OSR at 60 FPS).

Linux/FreeBSD keep CEF's internal BeginFrame clock by default. To exercise the
pump-driven scheduler there, opt in with the exact value `1`:

```sh
ZZ_BROWSER_EXTERNAL_BEGIN_FRAME=1 just run linux
```

The Linux/FreeBSD display ceiling remains 60 FPS unless `ZZ_BROWSER_FPS` is set;
the opt-in does not infer the display's refresh rate.

The browser-specific environment controls:

| Variable | Default | Use |
| --- | --- | --- |
| `ZZ_BROWSER_FPS=1..240` | Fastest macOS display; 60 elsewhere | Override the OSR ceiling. |
| `ZZ_BROWSER_GPU=0` | GPU on | Force software rendering/compositing and readback. |
| `ZZ_BROWSER_SHARED_TEXTURE=0` | Shared texture on | Keep the GPU process but force readback OSR. Linux also falls back per pane if a visible session produces no first frame within two seconds. |
| `ZZ_BROWSER_EXTERNAL_BEGIN_FRAME` | macOS on; Linux/FreeBSD off | macOS: exact `0` restores CEF's internal timer. Linux/FreeBSD: exact `1` enables pump-driven BeginFrames. |
| `ZZ_BROWSER_BF_ADAPTIVE=1` | Adaptive throttle off | Opt into experimental delivery-based hot-tier divisor changes on the anchored BeginFrame clock. |

## Checks

The root workspace excludes `examples/ui-showcase`. CI checks that standalone
workspace in a separate Linux step, and local release checks must include its
native and shipped WASM configurations. `just showcase-setup` installs the
nightly WASM target used by the second command.

```sh
cargo fmt --all -- --check
git diff --check
cargo test --workspace --all-features --quiet
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo check --workspace --all-targets --all-features
cargo check --manifest-path examples/ui-showcase/Cargo.toml --all-targets --locked
rustup run nightly cargo check --manifest-path examples/ui-showcase/Cargo.toml --target wasm32-unknown-unknown --all-targets --locked
```

Browser-domain tests can run without downloading or linking Chromium at all (package `zz-browser`,
crate `crates/zz-browser`):

```sh
cargo test -p zz-browser --no-default-features
```

# Key files

| File | Role |
| --- | --- |
| `crates/zz/src/main.rs` | Entry point for the `zz` binary (GPUI client + daemon bootstrap) |
| `crates/zz/src/bin/zz_browser_fixture.rs` | Loopback-only HTTP fixture used as the standard browser-pane example |
| `crates/zz/src/bin/zz_helper.rs` | CEF helper/bootstrap binary bundled alongside `zz` |
| `crates/zz/src/lib.rs` | Argument parsing, `run_command_mode` (the CLI path), daemon spawn, and the log-directory resolution documented above |
| `crates/zz-protocol/src/catalog.rs` | The canonical command list the CLI and command palette share (`list-sessions`, `split-window`, `send-keys`, …); see the [tmux command set](/tmux/commands.md) |

# Related

- [Prerequisites](/playbooks/prerequisites.md) . toolchain needed before the first build
- [Build/verify a CEF bundle](/playbooks/build-cef-bundle.md) . packaged equivalent of the first-build download
- [CEF artifact pin](/references/cef-artifacts.md) . exact version the first build downloads
- [`app` crate](/crates/zz.md) . the GPUI client this playbook runs
- [`server` crate](/crates/zz-daemon.md) . the daemon the platform launcher connects to or spawns
- [Browser CEF runtime](/browser/cef-runtime.md) . what the fixture exercises
