# AGENTS.md

zz is a tmux-superset terminal multiplexer that ships as a native GPU desktop app: a Rust workspace built on zpui (our fork of Zed's GPUI UI framework, the `crates/zpui*` crates), a persistent daemon that owns sessions and PTYs, Chromium browser panes (CEF off-screen rendering), agent panes (ACP), and remote hosts over plain ssh. Targets macOS and Linux (Wayland), with experimental Windows/WSL and iOS clients, and a raw-terminal attach client.

Rust edition 2024, MSRV 1.97. Release builds on mac/windows require Zig 0.16.0 (see `mise.toml`).

## Project map

- `crates/zpui`, `crates/zpui-*` - zpui, our GPUI: the gpui crates split out of Zed, the Zed utility crates they need (code still says `collections::`, `sum_tree::`), and `zpui-ios`, the UIKit backend
- `crates/zpui-kit` - widget kit on zpui (theme, primitives, widgets, icons): a maintained fork of gpui-component with no zz dependencies, usable by other apps
- `crates/zz` - desktop client: zpui shell, terminal/browser/agent panes, settings, daemon client
- `crates/zz-daemon` — the daemon: session state, PTY workers, client connections
- `crates/zz-daemon-client` - the client half of the daemon: local and ssh endpoints, askpass, `CommandClient`/`InteractiveClient`, transport, paths, and process facts; client-only crates depend on it instead of `zz-daemon`
- `crates/zz-mux` — tmux-compatible model: sessions, windows, panes, key tables
- `crates/zz-protocol` — wire protocol between daemon and clients, plus the shared key contract (tables, engine, fold, command catalog)
- `crates/zz-client` - sans-IO client core: protocol reduction, chrome keymap, daemon-backed convergence simulator, the browser element picker
- `crates/zz-config` - renderer-free application config, settings actions, preference persistence; update checks behind its `update` feature
- `crates/zz-cli` — headless `zz_cli` binary: the CLI, raw-terminal attach, and the ssh-side entry point
- `crates/zz-terminal` — terminal engine: PTY sessions, libghostty-vt state, frame snapshots
- `crates/zz-browser` — CEF off-screen-rendering browser runtime
- `crates/zz-chrome-import` — Chrome profile, cookie, and history import
- `crates/zz-ui` - zz's application UI on zpui-kit (panes, terminal painting, agent, palette, settings, navigation, phone shell); re-exports the kit
- `crates/zz-tui` — raw-terminal attach client as a library (the binary lives in `zz-cli`)
- `crates/zz-web` - local HTTP/WebSocket gateway for browser clients
- `crates/zz-hotreload` - Subsecond hot-reload launcher for `just hot linux`
- `clients/app` - `zz-app`: the thin-client app web and iOS share (shell, sidebar, status bar, settings, palette, overlays, panes, connection reducer, image cache, iOS transport and browser pane)
- `clients/web` - `zz-web-client`: the WASM entry point around zz-app, with its own Cargo workspace
- `clients/ios` - `zz-ios`: the iOS entry point around zz-app and `zpui-ios`, the bundle files, and a terminal example (`ZZ_GPUI_DEMO=terminal`); `just ios`
- `crates/zz-xtask` — build tooling: CEF bundling, packaging (`cargo xtask`)
- `compat/` — tmux compat campaign: differential harness (`run.sh`), gap registry (`tmux-gaps.json`), dispatch-board client (`board.py`), progress meter, orchestration handoff (`orchestration/`)
- `compat/tui/` — TUI parity campaign: proof ledger (`campaign.json`), validator and report generator (`tracker.py`), cycle runners (`run-N.js`); closed 2026-09-20 at 18/18
- `compat/catchup/` — tmux catch-up campaign (stale refusals, pin to 3.8, floating panes): rules and resume steps in `README.md`, work items in `ledger.json`
- `knowledge/` — OKF knowledge bundle for the whole system (start at `index.md`)
- `scripts/` — build, packaging, and profiling scripts
- `bench/` — terminal throughput benchmark harness
- `site/` — zzmux.sh landing page and docs (Astro)
- `third_party/` — vendored code (`ghostty/`, a trimmed Ghostty synced with `just vendor ghostty <rev>`; the libghostty-vt and CEF crates under `rust/`) and pinned reference material
- `packaging/` — Arch package, AUR/cask templates

## Knowledge bundle

`knowledge/` documents architecture, the wire protocol, tmux compat, the terminal engine, the CEF browser, designs, and operational playbooks. Read `knowledge/index.md` before digging into an unfamiliar subsystem — it beats cold grepping. The bundle is a map, not ground truth: load-bearing facts cite `resource:` source files; verify those before acting on them.

For TUI parity campaign work, start with `compat/tui/README.md` and
`knowledge/playbooks/tui-parity-campaign.md`. `python3 compat/tui/tracker.py check` validates the
ledger and its generated report; `ready` lists the dependency-ready obligations. Only verified proof
counts as TUI progress; accepted tmux gaps do not.

<important if="you need to build, run, test, lint, package, profile, or release">

Use just 1.52 or newer. Run `just` for the command groups, `just <group>` for its actions,
and `just --list --list-submodules` for the full tree. Keep related workflows under
`just <group> <action> [arguments]`; common desktop commands stay at the root.
Recipes live in `Justfile` and `scripts/just/*.just` and run from the repo root.

| Command | What it does |
|---|---|
| `cargo ci-test` | Tests (what CI runs): `cargo test --workspace --all-features` minus the zpui renderer and platform crates, whose tests need a GPU or a display (alias in `.cargo/config.toml`) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Lint (what CI runs) |
| `cargo clippy -p zz-daemon -p zz-daemon-client --no-default-features --all-targets -- -D warnings` | Lint the daemon without agent support (CI runs this too) |
| `cargo fmt --all` | Format |
| `just run <mac\|linux> [--verbose] [--features <list>]` | Launch isolated zz Dev (own daemon, config, browser, and data). Extra args are those two flags, not Cargo passthrough. No `windows` |
| `just watch <platform>` | Rebuild and relaunch on source change |
| `just build <platform>` | Release bundle into `dist/zz` (wraps `cargo xtask bundle-cef`) |
| `just install mac` | Build and swap `/Applications/zz.app`; the daemon survives the swap |
| `just ios <run\|build\|device\|bench> [iPhone\|iPad]` / `ios rig [stop]` / `ios testflight` | GPUI iOS app (iPhone by default): simulator, a paired device (release, signed, installed), a hands-free frame benchmark on the device, a throwaway daemon that `ios run` attaches to, or a signed `dev.zz.ios` TestFlight upload; `ZZ_GPUI_DEMO=terminal` selects the terminal example |
| `just site` | Docs site dev server with live reload |
| `just web run` / `web setup` / `web build [--release]` / `web serve` | Browser client dev loop / toolchain / assets / local gateway |
| `just profile <cpu\|memory\|startup\|system\|metal\|terminal> mac …` | Capture profiling data; read it back with `just profile summary <cpu\|metal\|terminal> <run>` |
| `just profile build mac` | Release-optimized bundle with dSYMs for profiling |
| `just package mac` / `package windows` / `package arch` / `install arch` / `package deb` / `install deb` | Platform packages |
| `just release bump <level-or-version> [--execute]` | Preview or publish a version bump (setup: `just release setup`) |
| `just release mac build <version>` | Full signed+notarized DMG (setup: `release mac setup`; pieces: `release mac sign`, `release mac notarize`, `release mac verify`, `release mac check`) |
| `bench/run.sh` | Terminal throughput benchmarks |
</important>

<important if="you are about to stash, reset, revert, or clean the working tree">
Multiple agent sessions often share this checkout in parallel. Never `git stash`, hard-reset, or discard uncommitted changes you did not author — you may be destroying another session's in-flight work.
</important>

<important if="you are changing zpui (our gpui)">

- zpui is `crates/zpui` plus the `crates/zpui-*` crates: our copy of the gpui crates split out of Zed and the Zed utility crates they need, renamed `zpui-*`. It is not a patch branch: there is nothing to rebase, and upstream Zed fixes come in by hand. Change zpui in the same commit as the zz code that needs it.
- They are root workspace members, so the root clippy run covers them with all features. Zed's extras zz never used (screen capture, the tracy profiler, Zed's perf test harness) are gone. `cargo ci-test` leaves out the tests of zpui-apple, zpui-linux, zpui-macos, zpui-platform, zpui-web, zpui-wgpu and zpui-windows, which need a GPU or a display server that CI runners lack; run `cargo test -p <crate>` for those when you touch them, on a machine with a GPU. The Zed-derived crates carry Zed's relaxed `[lints]` table instead of the workspace's pedantic set, and `[profile.dev.package]` in the root `Cargo.toml` keeps them at opt-level 2 so debug builds stay fast enough to use; a new zpui crate needs both. `crates/zpui-kit` is ours and takes the workspace lints.
- `clients/web` consumes zpui's WASM renderer in an excluded workspace; check `just web build` after a zpui change.
- `knowledge/references/zpui.md` has the full recipe; the `fork-rebase` skill covers pulling upstream Zed fixes and syncing the vendored Ghostty.
</important>

<important if="a test fails under cargo test --workspace">
A few `zz-daemon` tests are timing-sensitive and only fail under full-workspace parallel load. Before diagnosing, re-run the failing test alone (`cargo test -p zz-daemon <test_name>`); a solo pass points to load-induced flake, not your change. On headless machines `concurrent_default_interactive_attaches_atomically_share_session_zero` fails deterministically with `open terminal failed: not a terminal` — environmental, not a regression. Its panic is raised on a spawned thread, so libtest can attribute the failure to an innocent neighboring daemon test; a one-off daemon failure with that error text is this test misattributed.
</important>

<important if="you are debugging a running daemon or checking the CLI">

- The daemon outlives the app: after installing a new build, existing sessions keep running the old daemon binary until it restarts. Don't chase "missing" behavior in a stale daemon.
- `just run` / `just watch` build zz Dev with `ZZ_DEV_BUILD=1`, clear inherited daemon/pane context, and use `zz-dev` config/data/socket paths. The macOS bundle is `dist/zz-dev/zz Dev.app` (`dev.zz.app.dev`); Linux launches `target/debug/zz-dev`. Installed and beta packages keep their existing behavior.
- `just web run` / `web build` / `web serve` use dev assets and port 8081. Dev SSH selects `zz-dev`, linked by desktop dev runs into `~/.local/bin`. `web build --release` retains production identity. `just ios run iPad` opens the session sidebar and daemon-backed terminal/agent panes; `just ios run` (iPhone) opens the phone shell, one pane per screen, paged by the bar's pill, with the card overview (drag the bar up) as its navigator; the gear (in the overview on iPhone) opens the thin-client settings (iPhone hides Panes and Status bar); set `ZZ_GPUI_DEMO=terminal` for the terminal example. See `clients/ios/README.md` for endpoint selection.
- `ZZ_SOCKET` overrides the socket the app dials. Unix socket paths have a low length cap (`sun_path`); put test sockets directly under `/tmp`.
- Recipes live in `knowledge/playbooks/running-zz.md`.
</important>

<important if="you are building or styling UI chrome or widgets">

- UI conventions: `knowledge/configuration/ui-conventions.md` (chrome colors come from the theme; clippy rejects raw `rgb`/`hsla`).
- `crates/zpui-kit` is a full fork of gpui-component, not a dependency - read `crates/zpui-kit/UPSTREAM.md` before touching widget internals or trying to "update" it. App-specific UI belongs in `crates/zz-ui`; the kit must not depend on zz crates.
</important>

<important if="you are adding or editing documents under knowledge/">

- The bundle follows OKF v0.1: YAML frontmatter per document, and each directory's `index.md` carries a managed listing fence that mirrors frontmatter descriptions — keep both in sync when adding or renaming documents.
- Design documents under `knowledge/designs/` carry a `status:` field; update it when a design ships or dies.
- If a page contradicts the source it cites, fix the page — source is ground truth.
</important>

- No commit attribution
- Do not add comments in code
