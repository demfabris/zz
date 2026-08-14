---
type: Playbook
title: Toolchain and system prerequisites
description: The exact toolchain versions and per-platform system libraries required to build zz, pinned by rust-toolchain.toml, mise.toml, and CI.
resource: .github/workflows/ci.yml
tags: [prerequisites, toolchain, setup, rust, zig, cmake, linux, macos, windows, pacman]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

zz mixes a Rust workspace, a Zig-built terminal engine (`libghostty-vt`), and a CEF C++ wrapper
compiled from a downloaded Chromium distribution. Each has its own toolchain pin, and Linux needs
several system libraries for GPUI's Wayland/X11 backends and libghostty.

**This file is the source of truth for what to install.** Every row below is checked against a file
in the repo; when a pin moves, it moves here too. See [running zz](/playbooks/running-zz.md) for the
build/run commands and [building a CEF bundle](/playbooks/build-cef-bundle.md) for the CEF-specific
flow that consumes CMake/Ninja.

# Schema

| Requirement | Pin / version | Checked against | Why it's needed |
| --- | --- | --- | --- |
| Rust | `1.97.0`, minimal profile, `clippy` + `rustfmt` components | `rust-toolchain.toml`; `workspace.package.rust-version = "1.97"` in `Cargo.toml` | Toolchain auto-selected by `rustup` when present |
| Zig | `0.16.0` | `mise.toml`, mirrored in `.zigversion`; `mlugg/setup-zig@v2.2.1` with `version: 0.16.0` in CI | Builds `libghostty-vt` v0.2.1 with the locally patched sys crate pinned to Ghostty's Zig 0.16 migration |
| CMake | `3.21` or newer | `cmake_minimum_required(VERSION 3.21)` in the CEF distribution's own `CMakeLists.txt`; `cmake` in the CI apt list | Configures the CEF C++ wrapper build invoked by `xtask`/`cef::build_util` |
| Ninja | any recent | `ninja-build` in the CI apt list | Build backend for the CEF C++ wrapper |
| Linux system libs | see the apt line below | CI `apt-get install` list | Font discovery plus GPUI's dual Wayland/X11 backend (`gpui_platform` features `["wayland", "x11"]`) |
| Linux kernel | unprivileged user namespaces enabled | `cef_runtime.rs` appends only `disable-setuid-sandbox` | Chromium's user-namespace sandbox stays on; only the legacy setuid layer is disabled, and zz never passes `--no-sandbox` |
| Ubuntu 24.04+ | an AppArmor profile granting `userns` for the launched executable, or `kernel.apparmor_restrict_unprivileged_userns=0` | `packaging/deb/zz.apparmor`, installed as `/etc/apparmor.d/zz` | The distribution denies unprivileged user namespaces to unprofiled binaries, which kills the browser panes' zygote; the `.deb` carries a profile, an AppImage or dev bundle needs one written for its own path |
| AppImage packaging | `appimagetool` `1.9.1` + type-2 runtime `20251108`, with per-architecture SHA-256 checksums verified at download | `.github/workflows/ci.yml` (`APPIMAGETOOL_VERSION`, `APPIMAGE_RUNTIME_VERSION`, the four `*_SHA256` values) | Converts the validated Linux AppDir into a pinned-runtime type-2 AppImage; `desktop-file-utils` validates its desktop entry |
| Arch packaging | `makepkg` + Pacman; `base-devel` is the normal package-building prerequisite | `packaging/arch/PKGBUILD`, `Justfile` | Converts the validated Linux CEF bundle into a dependency-declared native package and optionally installs it |
| macOS | Xcode, plus the Metal Toolchain component on Xcode versions that ship it separately (`xcodebuild -downloadComponent MetalToolchain`) | `gpui_macos`'s build script shells out to `xcrun metal`/`metallib` | C/C++ toolchain for the CEF wrapper, and GPUI compiles its Metal shaders at build time |
| macOS packaging | system `ditto`, `hdiutil`, and `codesign` | Xcode/macOS command-line tools | Preserves the app bundle, creates/verifies the DMG, and re-checks the mounted app signature |
| Windows | MSVC Rust toolchain + Visual Studio C++ build tools | CI runs the `windows-2025` image | C/C++ toolchain for the CEF wrapper; produces `zz.exe`/`zz.dll` (see [xtask](/crates/zz-xtask.md)) |

Linux system packages, as installed in CI (`.github/workflows/ci.yml`, `ubuntu-24.04`):

```sh
sudo apt-get update
sudo apt-get install --yes \
  cmake curl desktop-file-utils ninja-build libfontconfig-dev libwayland-dev \
  libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev
```

# Examples

For local raw Cargo commands, activate mise in the shell and install the repository pin:

```sh
mise install
zig version # 0.16.0 inside this repository
```

CI installs the same Zig release explicitly:

```yaml
- uses: mlugg/setup-zig@v2.2.1
  with:
    version: 0.16.0
```

Rust toolchain requires no manual action if `rustup` is installed; `rust-toolchain.toml` at the
repo root pins the channel automatically:

```toml
[toolchain]
channel = "1.97.0"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

# Key files

| File | Role |
| --- | --- |
| `rust-toolchain.toml` | Pins the Rust channel and required components |
| `mise.toml` | Selects Zig 0.16.0 for raw local Cargo commands when mise is active |
| `.zigversion` | Mirrors the Zig pin for compatible Zig-specific tooling |
| `Cargo.toml` | `workspace.package.rust-version = "1.97"`; upstream `libghostty-vt = "=0.2.1"` plus the local `libghostty-vt-sys` patch |
| `third_party/rust/libghostty-vt-sys/UPSTREAM.md` | Records the wrapper release, Ghostty `7aa9591` pin, generated bindings, and removal condition |
| `.github/workflows/ci.yml` | Authoritative list of Linux system packages and the Zig setup action, run across `ubuntu-24.04`, `macos-15`, `windows-2025` |
| `packaging/arch/PKGBUILD` | Native Arch package metadata and filesystem layout for the validated Linux bundle |

# Related

- [Running zz](/playbooks/running-zz.md) . what to run once prerequisites are satisfied
- [Build/verify a CEF bundle](/playbooks/build-cef-bundle.md) . where CMake/Ninja get exercised
- [xtask crate](/crates/zz-xtask.md) . the tool that drives the CEF build
- [CEF artifact pin](/references/cef-artifacts.md) . the CEF version these prerequisites build
