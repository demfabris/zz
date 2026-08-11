---
type: Rust Crate
title: zz-xtask crate
description: Workspace build task with two subcommands, bundle-cef and verify-cef-bundle, that assemble and validate platform-specific zz application bundles around the upstream cef crate's download and wrapper build.
resource: crates/zz-xtask/src/main.rs
tags: [xtask, cef, build-tooling, cli, bundling, profiling, dsym]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

`zz-xtask` is zz's build-automation crate, invoked through the `cargo xtask <subcommand>` alias. It has exactly two
subcommands . `bundle-cef` and `verify-cef-bundle` . and its only job is building and validating the
platform CEF bundle: it wraps the `cef` crate's `build_util` helpers (the code that downloads,
SHA-1-verifies, extracts, and compiles the CEF C++ wrapper against the `Cargo.lock` pin), then copies
license/credit artifacts and checks the resulting layout. It is a thin CLI. The heavy lifting
(download, hash check, wrapper compilation) lives in the upstream `cef` crate itself.
On macOS it additionally disables automatic file quarantine on the
terminal-hosting main app, signs nested code from the inside out, and strictly verifies both the
bundle setting and signature. Its named macOS profile path also preserves and UUID-validates dSYMs
for Instruments. See [the CEF bundle playbook](/playbooks/build-cef-bundle.md) for the step-by-step
flow, [the running/profiling playbook](/playbooks/running-zz.md) for capture commands, and [the CEF
artifact pin](/references/cef-artifacts.md) for exact hashes.

# Schema

Subcommands are parsed by `run()` and `parse_bundle_options()` in `crates/zz-xtask/src/main.rs`:

| Subcommand | Arguments | Behavior |
| --- | --- | --- |
| `bundle-cef` | `[--release \| --profile NAME]` `[--output DIR]` (default `dist/zz`) | Builds the platform CEF bundle, installs notices, locally signs macOS nested code, then validates it; named profiles currently apply to macOS |
| `verify-cef-bundle` | `<bundle path>` (exactly one) | Checks required paths; on macOS also requires `LSFileQuarantineEnabled = false` on the main app and performs strict recursive signature verification; does not rebuild anything |

`bundle-cef` output layout per OS:

| OS | Backend called | Notes |
| --- | --- | --- |
| Linux | `cef::build_util::linux::build_bundle` | `--release` supported |
| Windows | `cef::build_util::win::build_bundle` | `--release` supported |
| macOS | Cargo JSON artifact discovery + `cef::build_util::mac::bundle` | Debug, `--release`, and named Cargo profiles are supported; the `profiling` profile forces native Ghostty to production `ReleaseFast`, then copies and UUID-validates app/helper dSYMs before the normal inside-out signing and verification |

Required files checked by `platform_bundle_files` (also `crates/zz-xtask/src/main.rs`):

| OS | Required files (relative to bundle root) |
| --- | --- |
| Linux | executable, `libcef.so`, `icudtl.dat`, `resources.pak`, `locales/en-US.pak`, `chrome-sandbox`, `CREDITS.html`, `CEF_LICENSE.txt` |
| Windows | executable (plus its icon group at resource id 1 and a `PerMonitorV2` manifest), `zz.dll`, `libcef.dll`, `chrome_elf.dll`, `v8_context_snapshot.bin`, `icudtl.dat`, `resources.pak`, `chrome_100_percent.pak`, `chrome_200_percent.pak`, `locales/en-US.pak`, `CREDITS.html`, `CEF_LICENSE.txt` |
| macOS | Main app `Info.plist` + executable; framework executable, `Info.plist`, core `.pak` files, `icudtl.dat`, and `en.lproj/locale.pak`; `Info.plist` + executable for the generic, GPU, Renderer, Plugin, and Alerts helper apps; root CEF credits + license notices |

`bundle_root(executable)` differs by OS: the executable's parent directory on Linux/Windows, or
`<app>/Contents/Resources` on macOS.

The upstream CEF bundler writes `LSFileQuarantineEnabled = true`. `xtask` changes that key to
`false` on the main app before signing because zz's terminal descendants create local executables
and transient native modules; automatic quarantine otherwise makes Gatekeeper treat those files as
downloads. CEF helper-app policy is left unchanged. This setting does not clear quarantine from a
downloaded app or replace notarization. Local macOS signing auto-selects exactly one valid
`Apple Development` identity so TCC privacy grants remain attached to `dev.zz.app` across rebuilds.
If no unique candidate exists, it falls back to ad-hoc signing; set
`MACOS_LOCAL_SIGN_IDENTITY` to an identity name/SHA-1 or `-` to override either choice. Shipping the
app to other Macs still requires the separate Developer ID identity and Apple notarization flow.

# Examples

```sh
# Build and validate a release bundle (all platforms)
cargo xtask bundle-cef --release --output dist/zz

# Build a release-optimized, source-symbolized macOS profiling bundle
cargo xtask bundle-cef --profile profiling --output dist/zz-profile

# Launch the macOS bundle
open dist/zz/zz.app

# Validate an already-built bundle without rebuilding
cargo xtask verify-cef-bundle dist/zz/zz
cargo xtask verify-cef-bundle dist/zz/zz.app
```

CI (`.github/workflows/ci.yml`) runs the same task on every matrix OS to exercise the full
download → verify → extract → wrapper-build → bundle path:

```yaml
- name: Build and validate CEF bundle
  run: cargo xtask bundle-cef --release --output target/cef-bundle
```

On Windows, `zz.exe` is CEF's sandbox bootstrap executable, copied from the distribution's
`bootstrap.exe`: it loads the `zz.dll` beside it (the name comes from its own) and calls the
exported `RunWinMain` entry point. `xtask` builds that library with `cargo build --package zz
--lib`, so `--features` reaches it, then copies the CEF runtime, locales, `zz.dll`, and . when the
profile emitted one . `zz.pdb` around it. Upstream's `cef::build_util::win::build_bundle` is not
used: it hardcodes its cargo invocation and copies `zz.pdb` unconditionally, which no release
profile emits.

Because that executable is prebuilt, its icon and DPI manifest cannot come from the compiler.
`configure_windows_executable` writes both into the copy's PE resources with `editpe`:
`assets/windows/zz.ico` as the icon group at resource id 1 (where gpui's `LoadImage` looks for the
window icon; `editpe` files new groups under the name `MAINICON`, which that lookup never reaches),
and a per-monitor-v2 manifest . gpui scales windows by `GetDpiForWindow`, which answers 96 in a
process that never declared awareness. `verify_bundle` re-reads both. Editing resources invalidates
CEF's Authenticode signature on the file; release signing re-signs the bundle.

A direct `cargo run` still cannot open the GUI there . only the bootstrap can hand a process the
sandbox handle . but that binary answers every windowless verb.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-xtask/src/main.rs` | Entire CLI: profile-aware bundle dispatch, dSYM copying/UUID validation, CEF notices, macOS inside-out signing, and bundle verification |
| `crates/zz-xtask/Cargo.toml` | Dependencies on the workspace-pinned `cef` crate, `cargo_metadata` for artifact discovery, and (Windows only) `editpe` for the bootstrap executable's PE resources |
| `assets/windows/zz.ico` | The 16–256px icon embedded in the bundled `zz.exe`, and the file Windows installers point `SetupIconFile` at |
| `scripts/profile-macos.sh` | Isolated process launch, metadata collection, xctrace recording, and scoped cleanup for the profiling bundle |
| `third_party/cef/LICENSE.txt` | Copied into every bundle as `CEF_LICENSE.txt` by `install_cef_notices` |
| CEF distribution `CREDITS.html` | Copied into every platform bundle root by `install_cef_notices` |

# Related

- [Build/verify a CEF bundle playbook](/playbooks/build-cef-bundle.md) . the operational walkthrough this crate implements
- [CEF artifact pin reference](/references/cef-artifacts.md) . the exact version/hash table the
  `download-cef` build helper (a dependency of the upstream `cef` crate, not an `xtask` subcommand)
  verifies against before `bundle-cef` ever runs
- [Updating CEF playbook](/playbooks/updating-cef.md) . coordinated steps when bumping the CEF pin
- [Prerequisites playbook](/playbooks/prerequisites.md) . CMake/Ninja/Zig requirements this task depends on
- [CEF runtime concept](/browser/cef-runtime.md) . the runtime this bundle ships
- [`app` crate](/crates/zz.md) . the GPUI binary (`zz`) that the bundle wraps
