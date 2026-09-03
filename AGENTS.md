# AGENTS.md

zz is a tmux-superset terminal multiplexer that ships as a native GPU desktop app: a Rust workspace built on gpui (Zed's UI framework, consumed through a patched fork), a persistent daemon that owns sessions and PTYs, Chromium browser panes (CEF off-screen rendering), agent panes (ACP), and remote hosts over plain ssh. Targets macOS and Linux (Wayland), with experimental Windows/WSL, a native Swift iPhone and iPad client, and a raw-terminal attach client.

Rust edition 2024, MSRV 1.97. Release builds on mac/windows require Zig 0.16.0 (see `mise.toml`).

## Project map

- `crates/zz` — desktop client: GPUI shell, terminal/browser/agent panes, settings, daemon client
- `crates/zz-daemon` — the daemon: session state, PTY workers, client connections
- `crates/zz-mux` — tmux-compatible model: sessions, windows, panes, key tables
- `crates/zz-protocol` — wire protocol between daemon and clients, plus the shared key contract (tables, engine, fold, command catalog)
- `crates/zz-client` — sans-IO client core: protocol reduction, chrome keymap, daemon-backed convergence simulator
- `crates/zz-client-ffi` — C ABI over the client core (`include/zz-client.h`, link-verified by a C integration client)
- `crates/zz-terminal` — terminal engine: PTY sessions, libghostty-vt state, frame snapshots
- `crates/zz-browser` — CEF off-screen-rendering browser runtime
- `crates/zz-chrome-import` — Chrome profile, cookie, and history import
- `crates/zz-ui` — widget layer: a maintained full fork of gpui-component
- `crates/zz-tui` — raw-terminal attach client
- `clients/ios` — adaptive SwiftUI/UIKit iPhone and iPad app over `zz-client-ffi`
- `crates/zz-xtask` — build tooling: CEF bundling, packaging (`cargo xtask`)
- `compat/` — tmux compat campaign: differential harness (`run.sh`), gap registry (`tmux-gaps.json`), dispatch-board client (`board.py`), progress meter, orchestration handoff (`orchestration/`)
- `knowledge/` — OKF knowledge bundle for the whole system (start at `index.md`)
- `scripts/` — build, packaging, profiling, and fork-maintenance scripts (`forks.conf`)
- `bench/` — terminal throughput benchmark harness
- `site/` — zzmux.sh landing page and docs (Astro)
- `examples/ui-showcase` — wasm/WebGPU showcase of zz-ui, excluded from the workspace
- `third_party/` — vendored crates and pinned reference material
- `packaging/` — Arch package, AUR/cask templates

## Knowledge bundle

`knowledge/` documents architecture, the wire protocol, tmux compat, the terminal engine, the CEF browser, designs, and operational playbooks. Read `knowledge/index.md` before digging into an unfamiliar subsystem — it beats cold grepping. The bundle is a map, not ground truth: load-bearing facts cite `resource:` source files; verify those before acting on them.

<important if="you need to build, run, test, lint, package, profile, or release">

Run `just` recipes from the repo root; `just --list` shows everything.

| Command | What it does |
|---|---|
| `cargo test --workspace --all-features` | Tests (what CI runs) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Lint (what CI runs) |
| `cargo fmt --all` | Format |
| `just run <mac\|linux> [--verbose] [--features <list>]` | Launch a fresh debug instance. Extra args are those two flags, not Cargo passthrough. No `windows` |
| `just watch <platform>` | Rebuild and relaunch on source change |
| `just build <platform>` | Release bundle into `dist/zz` (wraps `cargo xtask bundle-cef`) |
| `just install mac` | Build and swap `/Applications/zz.app`; the daemon survives the swap |
| `just ios` / `just ipad` / `just ios-build` / `just ipad-build` / `just ios-test` / `just ipad-test` / `just ios-device [name]` | Native Apple client on iPhone or iPad simulator / build only / simulator tests / physical device |
| `just forks` / `just fork-rebase <name>` | Carried-patch fork status / rebase |
| `just site` | Docs site dev server with live reload |
| `just showcase` / `showcase-setup` / `showcase-build[-release]` | wasm UI showcase dev loop / toolchain / assets |
| `just profile-cpu\|profile-system\|profile-metal\|profile-terminal-diagnostics mac …` | Instruments captures (macOS); read one back with `profile-cpu-summary`, `profile-metal-summary`, or `profile-terminal-summary` (`profile-system` has no summary recipe) |
| `just profile-build mac` | Release-optimized bundle with dSYMs for profiling |
| `just dmg` / `zip-windows` / `pacman-package` / `pacman-install` / `deb-package` / `deb-install` | Platform packages |
| `just release-mac <version>` | Full signed+notarized DMG (setup: `notary-setup-mac`; pieces: `sign-mac`, `notarize-mac`, `verify-notarized-mac`, `release-mac-check`) |
| `bench/run.sh` | Terminal throughput benchmarks |
</important>

<important if="you are about to stash, reset, revert, or clean the working tree">
Multiple agent sessions often share this checkout in parallel. Never `git stash`, hard-reset, or discard uncommitted changes you did not author — you may be destroying another session's in-flight work.
</important>

<important if="you are bumping gpui/zed or any dependency resolved through a [patch] fork">

- `gpui`/`gpui_platform` resolve to `demfabris/zed` branch `zz-patches` — a carried-patch fork listed in `scripts/forks.conf`. Bumping upstream means rebasing the patch branch: `just forks` for status, `just fork-rebase zed` to rebase.
- Strange gpui build errors right after a dependency change usually mean `Cargo.lock` and the fork branch are out of sync.
- `examples/ui-showcase` is the only consumer of gpui's wasm/WebGPU path and is workspace-excluded, so a bump can break it without failing the main build. Check with `just showcase-build`.
</important>

<important if="a test fails under cargo test --workspace">
A few `zz-daemon` tests are timing-sensitive and only fail under full-workspace parallel load. Before diagnosing, re-run the failing test alone (`cargo test -p zz-daemon <test_name>`); a solo pass points to load-induced flake, not your change. On headless machines `concurrent_default_interactive_attaches_atomically_share_session_zero` fails deterministically with `open terminal failed: not a terminal` — environmental, not a regression. Its panic is raised on a spawned thread, so libtest can attribute the failure to an innocent neighboring daemon test; a one-off daemon failure with that error text is this test misattributed.
</important>

<important if="you are debugging a running daemon or checking the CLI">

- The daemon outlives the app: after installing a new build, existing sessions keep running the old daemon binary until it restarts. Don't chase "missing" behavior in a stale daemon.
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
