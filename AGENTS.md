# AGENTS.md

zz is a tmux-superset terminal multiplexer that ships as a native GPU desktop app: a Rust workspace built on gpui (Zed's UI framework, split out into our own `demfabris/gpui`), a persistent daemon that owns sessions and PTYs, Chromium browser panes (CEF off-screen rendering), agent panes (ACP), and remote hosts over plain ssh. Targets macOS and Linux (Wayland), with experimental Windows/WSL and GPUI iOS clients, and a raw-terminal attach client.

Rust edition 2024, MSRV 1.97. Release builds on mac/windows require Zig 0.16.0 (see `mise.toml`).

## Project map

- `crates/zz` — desktop client: GPUI shell, terminal/browser/agent panes, settings, daemon client
- `crates/zz-daemon` — the daemon: session state, PTY workers, client connections
- `crates/zz-mux` — tmux-compatible model: sessions, windows, panes, key tables
- `crates/zz-protocol` — wire protocol between daemon and clients, plus the shared key contract (tables, engine, fold, command catalog)
- `crates/zz-client` — sans-IO client core: protocol reduction, chrome keymap, daemon-backed convergence simulator
- `crates/zz-config` - renderer-free application config, settings actions, preference persistence, and update checks
- `crates/zz-client-ffi` — C ABI over the client core (`include/zz-client.h`, link-verified by a C integration client)
- `crates/zz-cli` — headless `zz_cli` binary: the CLI, raw-terminal attach, and the ssh-side entry point
- `crates/zz-terminal` — terminal engine: PTY sessions, libghostty-vt state, frame snapshots
- `crates/zz-browser` — CEF off-screen-rendering browser runtime
- `crates/zz-chrome-import` — Chrome profile, cookie, and history import
- `crates/zz-ui` — widget layer: a maintained full fork of gpui-component
- `crates/zz-tui` — raw-terminal attach client as a library (the binary lives in `zz-cli`)
- `crates/zz-web` - local HTTP/WebSocket gateway for browser clients
- `clients/web` - full-page GPUI/WASM client using zz-ui and zz-client, with its own Cargo workspace
- `clients/gpui-shared` - one app shell, sidebar, status bar, settings, palette, overlays, pane entities, connection reducer, and image modules compiled by web and iOS clients
- `crates/zz-gpui-ios` and `clients/ios-gpui` - UIKit GPUI backend and iOS client with the shared session sidebar, terminal and agent panes, split layout, and thin-client settings (`just ios`); a standalone terminal example remains available (`ZZ_GPUI_DEMO=terminal`)
- `crates/zz-xtask` — build tooling: CEF bundling, packaging (`cargo xtask`)
- `compat/` — tmux compat campaign: differential harness (`run.sh`), gap registry (`tmux-gaps.json`), dispatch-board client (`board.py`), progress meter, orchestration handoff (`orchestration/`)
- `compat/tui/` — TUI parity campaign: proof ledger (`campaign.json`), validator and report generator (`tracker.py`), cycle runners (`run-N.js`); closed 2026-09-20 at 18/18
- `knowledge/` — OKF knowledge bundle for the whole system (start at `index.md`)
- `scripts/` — build, packaging, and profiling scripts
- `bench/` — terminal throughput benchmark harness
- `site/` — zzmux.sh landing page and docs (Astro)
- `third_party/` — vendored crates and pinned reference material
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
| `cargo test --workspace --all-features` | Tests (what CI runs) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Lint (what CI runs) |
| `cargo clippy -p zz-daemon --no-default-features --features daemon --all-targets -- -D warnings` | Lint the daemon without agent support (CI runs this too) |
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

<important if="you are changing gpui or moving its pin">

- `gpui`, `gpui_platform`, and `gpui_wgpu` come from `demfabris/gpui`, our own repo with the 22 GPUI crates split out of Zed. It is not a patch branch: there is nothing to rebase, and upstream Zed fixes come in by hand. GPUI changes land there first, then the `rev` moves in root `Cargo.toml` `[workspace.dependencies]`.
- Strange gpui build errors right after a dependency change usually mean `Cargo.lock` and the pinned rev are out of sync.
- `clients/web` consumes gpui's WASM renderer in an excluded workspace. Keep its rev and lockfile in step with the root, and check `just web build` after a bump.
- `knowledge/references/gpui-revision.md` has the full recipe; the `fork-rebase` skill covers the native Ghostty fork.
</important>

<important if="a test fails under cargo test --workspace">
A few `zz-daemon` tests are timing-sensitive and only fail under full-workspace parallel load. Before diagnosing, re-run the failing test alone (`cargo test -p zz-daemon <test_name>`); a solo pass points to load-induced flake, not your change. On headless machines `concurrent_default_interactive_attaches_atomically_share_session_zero` fails deterministically with `open terminal failed: not a terminal` — environmental, not a regression. Its panic is raised on a spawned thread, so libtest can attribute the failure to an innocent neighboring daemon test; a one-off daemon failure with that error text is this test misattributed.
</important>

<important if="you are debugging a running daemon or checking the CLI">

- The daemon outlives the app: after installing a new build, existing sessions keep running the old daemon binary until it restarts. Don't chase "missing" behavior in a stale daemon.
- `just run` / `just watch` build zz Dev with `ZZ_DEV_BUILD=1`, clear inherited daemon/pane context, and use `zz-dev` config/data/socket paths. The macOS bundle is `dist/zz-dev/zz Dev.app` (`dev.zz.app.dev`); Linux launches `target/debug/zz-dev`. Installed and beta packages keep their existing behavior.
- `just web run` / `web build` / `web serve` use dev assets and port 8081. Dev SSH selects `zz-dev`, linked by desktop dev runs into `~/.local/bin`. `web build --release` retains production identity. `just ios run iPad` opens the session sidebar and daemon-backed terminal/agent panes; `just ios run` (iPhone) opens the phone shell, one pane per screen, paged by the bar's pill, with the card overview (drag the bar up) as its navigator; the gear (in the overview on iPhone) opens the thin-client settings (iPhone hides Panes and Status bar); set `ZZ_GPUI_DEMO=terminal` for the terminal example. See `clients/ios-gpui/README.md` for endpoint selection.
- `ZZ_SOCKET` overrides the socket the app dials. Unix socket paths have a low length cap (`sun_path`); put test sockets directly under `/tmp`.
- Recipes live in `knowledge/playbooks/running-zz.md`.
</important>

<important if="you are building or styling UI chrome or widgets">

- UI conventions: `knowledge/configuration/ui-conventions.md` (chrome colors come from the theme; clippy rejects raw `rgb`/`hsla`).
- `crates/zz-ui` is a full fork of gpui-component, not a dependency — read `crates/zz-ui/UPSTREAM.md` before touching widget internals or trying to "update" it.
</important>

<important if="you are adding or editing documents under knowledge/">

- The bundle follows OKF v0.1: YAML frontmatter per document, and each directory's `index.md` carries a managed listing fence that mirrors frontmatter descriptions — keep both in sync when adding or renaming documents.
- Design documents under `knowledge/designs/` carry a `status:` field; update it when a design ships or dies.
- If a page contradicts the source it cites, fix the page — source is ground truth.
</important>

- No commit attribution
- Do not add comments in code
