---
type: Playbook
title: Building and verifying a platform CEF bundle
description: Step-by-step use of cargo xtask and release recipes to assemble, sign, notarize, and validate platform CEF bundles.
resource: crates/zz-xtask/src/main.rs
tags: [cef, xtask, bundle, playbook, sha1, codesign, pacman, profiling, dsym, homebrew, release]
timestamp: 2026-08-02T10:59:04-03:00
---

# Overview

Producing a runnable zz bundle requires downloading Chromium's CEF distribution, verifying it,
compiling its C++ wrapper, and assembling a platform-correct directory/app layout. The bundle is
the required launch path on macOS and Windows; Linux also supports direct Cargo development runs.
`cargo xtask bundle-cef` does all of this in one step; see
[the `xtask` crate](/crates/zz-xtask.md) for what the CLI itself parses and validates, and
[prerequisites](/playbooks/prerequisites.md) for the CMake/Ninja/compiler requirements this flow
needs.

On macOS, the configure step compiles `assets/zz.icon` with Xcode's `actool` . `Assets.car` plus a
regenerated `zz.icns` fallback land in `Contents/Resources` before signing, and `CFBundleIconName`
points macOS 26+ at the compiled icon so the Dock renders every appearance (light, dark, tinted,
clear) natively. `actool` ships with Xcode proper, not the Command Line Tools, so macOS bundling
requires an Xcode install. Local bundles use a sole installed Apple Development identity when
available and fall back to ad-hoc signing otherwise. `just release-mac <version-label>` layers the separate
public-distribution workflow on top: Developer ID signing from the inside out, DMG packaging and
signing, Apple notarization through a Keychain profile, ticket stapling, Gatekeeper assessment, and
a final SHA-256 checksum.

The macOS bundle also carries `Contents/MacOS/cli`, the launcher every `zz` on `PATH` points at.
macOS resolves an app bundle from the path the executable was launched with and does not follow
symlinks doing it, so a symlink straight to `Contents/MacOS/zz` starts zz with no `Info.plist` .
no bundle identifier, no icon, and none of the usage descriptions the browser pane's camera and
microphone prompts require. Rust's `current_exe` has the same shape of problem on macOS, which
would additionally send the CEF framework lookup beside the symlink. The launcher canonicalizes its
own path and execs the neighbouring `zz`, keeping one process, the same stdio and working
directory, and the bundle identity. Linux needs none of this: `/proc/self/exe` is already resolved,
so the Pacman package's `/usr/bin/zz` may point straight at the real executable.

# Examples

All supported platforms (release):

```sh
cargo xtask bundle-cef --release --output dist/zz
```

Release-optimized macOS profiling bundle with source-level dSYMs:

```sh
just profile-build mac
```

Launch the resulting macOS app bundle:

```sh
open dist/zz/zz.app
```

Validate an already-built bundle without rebuilding anything (pass the executable path on
Linux/Windows, or the `.app` path on macOS):

```sh
cargo xtask verify-cef-bundle dist/zz/zz
```

Set up and run a public macOS release:

```sh
just notary-setup-mac
just release-mac-check
just release-mac 0.1.0-beta.2
just verify-notarized-mac dist/zz-0.1.0-beta.2-macos-arm64.dmg
```

The Keychain profile defaults to `zz-notary`; override it with `MACOS_NOTARY_PROFILE`. The script
auto-selects one valid `Developer ID Application` identity, or accepts an explicit
`MACOS_SIGN_IDENTITY`. Credentials never enter command arguments or repository files after the
interactive `notarytool store-credentials` setup.

Pushing a `v*` tag runs the same sequence on `macos-15` (`.github/workflows/release.yml`) and
publishes the result: a GitHub release carrying the notarized DMG and its checksum, and a rendered
cask pushed to the `demfabris/homebrew-zz` tap, which users install with
`brew install --cask demfabris/zz/zz`. Render the cask by hand with the same script the workflow
uses:

```sh
scripts/render-cask.sh 0.1.0 dist/zz-0.1.0-macos-arm64.dmg
```

CI runs release bundles on every OS in the matrix (`.github/workflows/ci.yml`):

```sh
cargo xtask bundle-cef --release --output target/cef-bundle
```

The Linux and macOS jobs each publish two directly downloadable artifacts: the compiled release
executable(s), and a platform distribution package containing the complete runnable CEF bundle.
The workflow uploads each prebuilt artifact without GitHub's extra ZIP layer:

| Runner | Binary artifact | Runnable package |
| --- | --- | --- |
| `ubuntu-24.04` | `zz-linux-${RUNNER_ARCH}-binary.tar.gz` (`zz`) | `zz-linux-${RUNNER_ARCH}.AppImage` (complete CEF bundle in a relocatable AppDir) |
| `macos-15` | `zz-macos-${RUNNER_ARCH}-binaries.tar.gz` (`zz` + `zz_helper` + `zz_cli`) | `zz-macos-${RUNNER_ARCH}.dmg` (`zz.app` plus an `/Applications` shortcut) |

The binary-only archives expose Cargo's release outputs for inspection and integration work. For
normal launches, use the AppImage on Linux and drag `zz.app` from the DMG to Applications on macOS.
The AppImage's `AppRun` keeps the full validated CEF directory together under `usr/lib/zz`, supplies
desktop metadata and the complete hicolor icon set from `assets/linux/hicolor`, and points the
dynamic loader at the adjacent CEF libraries. CI pins
`appimagetool` 1.9.1 plus type-2 runtime `20251108`, verifies both official SHA-256 values for the
runner architecture, and passes the local runtime explicitly so packaging never falls back to the
mutable `continuous` runtime. The DMG is built as a compressed HFS+ image with `ditto`/`hdiutil`,
then mounted to verify its contents and the app's strict recursive code signature. The contained
app remains ad-hoc signed; public distribution without Gatekeeper warnings requires Developer ID
signing and notarization.

Arch developers can run `just pacman-package` to turn the same validated Linux bundle into a native
package, or `just pacman-install` to build it and install it through `makepkg`. The package keeps the
runtime together in `/usr/lib/zz`, exposes `/usr/bin/zz` as a relative symlink, and installs the
shared desktop entry and icons.

Windows continues to exercise bundle construction in CI but intentionally does not publish a
release artifact.

Artifact retention follows the repository's GitHub Actions retention setting.

# Schema

What `bundle-cef` does, in order:

1. **`download-cef`** . the `cef` crate's `build_util` resolves the Rust package pin
   (`cef = "=151.2.0"` / `cef-dll-sys` in `Cargo.lock`, currently `151.2.0+151.3.14`) to a CEF
   release, then downloads the per-target minimal distribution archive.
2. **SHA-1 verification** . the downloaded archive's SHA-1 is checked against the hash fetched from
   CEF's own `index.json` (`https://cef-builds.spotifycdn.com/index.json`) before anything is
   extracted. [`third_party/cef/ARTIFACTS.md`](/references/cef-artifacts.md) mirrors those values
   for review; the downloader never reads it.
3. **Extraction** . the verified archive is unpacked into the shared `CEF_PATH` cache (see
   [running zz](/playbooks/running-zz.md) for why a stable path matters).
4. **CEF C++ wrapper build** . `libcef_dll_wrapper` is configured with CMake (3.21+) and built with
   Ninja, per [prerequisites](/playbooks/prerequisites.md).
5. **Bundle assembly** . Linux uses upstream `bundle` after building the binary itself; Windows
   builds `zz.dll` and lays the flat bundle out around CEF's `bootstrap.exe` (copied in as
   `zz.exe`), then writes zz's icon and a per-monitor-v2 manifest into that executable's
   resources; macOS discovers Cargo's actual
   `zz`/`zz_helper`/`zz_cli` artifact paths for the selected profile and calls the public macOS
   `bundle` API. The platform backend copies CEF runtime, resources, locales, and helper/bootstrap
   artifacts into `--output` (default `dist/zz`). The upstream bundler knows nothing about the
   `PATH` launcher, so `install_macos_cli_launcher` copies `zz_cli` in afterwards as
   `Contents/MacOS/cli` . before signing, since it is Mach-O code the bundle signature must cover.
6. **macOS profiling symbols** . the named `profiling` Cargo profile inherits release optimization
   while retaining full DWARF. `install_macos_debug_symbols` copies `zz.dSYM` and
   `zz_helper.dSYM` beside `dist/zz-profile/zz.app` and requires each copied symbol bundle's
   Mach-O UUID set to match its exact executable before bundling can continue. The build command
   explicitly sets `LIBGHOSTTY_VT_SYS_OPTIMIZE=ReleaseFast`: Cargo's `DEBUG` build-script variable
   becomes true when the profile emits DWARF, which would otherwise select Zig `Debug` and retain
   terminal integrity assertions that are absent from production.
7. **macOS main-app policy** . `configure_macos_main_app` changes the upstream CEF default to
   `LSFileQuarantineEnabled = false` on the outer app. zz hosts terminal processes that create local
   executables and transient native modules; automatically tagging those outputs as downloaded code
   makes Gatekeeper block trusted tools. Helper app policy remains unchanged, and this setting does
   not remove quarantine from a downloaded DMG.
8. **Notice install** . `install_cef_notices` copies `third_party/cef/LICENSE.txt` as
   `CEF_LICENSE.txt` and the pinned distribution's `CREDITS.html` into the platform bundle root.
9. **macOS local signing** . `sign_macos_bundle` queries Keychain and uses exactly one valid
   `Apple Development` identity for the CEF framework, each helper app, the `PATH` launcher, and the
   outer app in inside-out order. Stable signing keeps macOS privacy grants valid across rebuilds. With zero or
   multiple candidates it falls back to ad-hoc signing; `MACOS_LOCAL_SIGN_IDENTITY` selects an
   identity explicitly or accepts `-` to force ad-hoc signing. Public distribution still requires
   Developer ID signing and notarization.
10. **Validation** . `verify_bundle` checks every OS-required path is a non-empty regular file (see
   the table in [the `xtask` crate doc](/crates/zz-xtask.md#schema)); missing, empty, or non-file paths
   fail the whole task with a listed diff. On macOS it also verifies the main-app quarantine policy
   and runs strict, recursive `codesign` verification.
11. **Distribution packaging** . `scripts/package-appimage.sh` maps the Linux bundle into an AppDir
   and validates the resulting type-2 AppImage by extracting it; `packaging/arch/PKGBUILD` maps that
   bundle into a dependency-declared native Pacman package; `scripts/package-dmg.sh` copies the macOS
   app with `ditto`, adds the Applications shortcut, creates/verifies the compressed disk image,
   mounts it, and re-verifies the contained app.
12. **macOS public release** . `scripts/release-macos.sh` verifies the Keychain identity and notary
    profile, re-signs every CEF library/framework/helper from the inside out with a secure timestamp,
    enables Hardened Runtime on executable bundles, gives only the GPU and Renderer helpers the JIT
    entitlement, signs the `PATH` launcher and then the main app with its device/privacy
    entitlements, signs the DMG, submits it
    with `notarytool --wait`, saves the notary log, staples the ticket, and checks both `stapler` and
    Gatekeeper. `just release-mac <version-label>` drives the complete sequence and refuses to
    overwrite an existing artifact. Its layout assertion requires the launcher, and its final sweep
    fails on any Mach-O file in the app that is not Developer ID-signed.
13. **Tag-driven publication** . `.github/workflows/release.yml` fires on `v*`, refuses a tag that
    disagrees with the workspace version (the DMG carries `CFBundleShortVersionString`, the cask
    carries the tag, and a mismatch leaves brew permanently confused), loads the Developer ID
    identity and App Store Connect notary key into one throwaway default Keychain, runs the release
    script, publishes the DMG plus its SHA-256 to a GitHub release, and pushes a rendered cask to
    the tap. Tags with a hyphen . `v0.2.0-rc.1` . become prereleases and are kept off the tap.
    Repository secrets: `MACOS_CERTIFICATE_P12_BASE64`, `MACOS_CERTIFICATE_PASSWORD`,
    `MACOS_SIGN_IDENTITY` (a SHA-1 fingerprint rather than the common name, which several Developer
    ID certificates for one team share), `APPLE_API_KEY_P8_BASE64`, `APPLE_API_KEY_ID`,
    `APPLE_API_ISSUER_ID` (Team API keys only), and `HOMEBREW_TAP_DEPLOY_KEY`. The tap push authenticates with a write
    deploy key registered on the tap repository rather than a personal access token: it is scoped to
    that one repository and does not expire. GitHub's host key is pinned in the workflow, since
    trust-on-first-use authenticates nothing on a runner that is always first use.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-xtask/src/main.rs` | Profile-aware bundle assembly, profiling dSYM UUID validation, macOS inside-out local signing, and platform validation |
| `scripts/profile-macos.sh` | Runs the symbolized profiling bundle on an isolated socket and captures CPU, System Trace, or Metal Instruments data |
| `third_party/cef/ARTIFACTS.md` | Reviewable mirror of the archive name + SHA-1 fetched from CEF's index per Rust target |
| `third_party/cef/LICENSE.txt` | Installed into every bundle as `CEF_LICENSE.txt` |
| `Cargo.toml` | `cef = "=150.0.0"` workspace pin that `download-cef` resolves |
| `crates/zz/src/bin/zz_cli.rs` | The `PATH` launcher bundled as `Contents/MacOS/cli`, with the macOS bundle-identity reasoning |
| `.github/workflows/ci.yml` | Exercises `bundle-cef` on `ubuntu-24.04`, `macos-15`, `windows-2025` |
| `.github/workflows/release.yml` | Tag-driven macOS release: keychain setup, notarized DMG, GitHub release, tap push |
| `packaging/homebrew/zz.rb` + `scripts/render-cask.sh` | Cask template for the `demfabris/homebrew-zz` tap, and the renderer that fills in version and checksum |
| `scripts/package-appimage.sh` + `packaging/linux/` + `assets/linux/hicolor/` | Builds and validates the AppDir/AppImage around a Linux CEF bundle with desktop metadata and the complete icon set |
| `packaging/arch/PKGBUILD` | Installs the validated Linux bundle, desktop entry, and icons into a native Pacman package |
| `scripts/package-dmg.sh` | Builds, mounts, and validates the macOS drag-install disk image |
| `scripts/release-macos.sh` + `packaging/macos-signing/*.plist` | Developer ID signs CEF code with the accepted entitlement split, notarizes/staples the DMG, and verifies Gatekeeper |

# Citations

- Apple, [Creating distribution-signed code for macOS](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/)
- Apple, [Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)

# Related

- [xtask crate](/crates/zz-xtask.md) . the CLI this playbook drives
- [CEF artifact pin reference](/references/cef-artifacts.md) . the version/hash table verified against
- [Updating CEF](/playbooks/updating-cef.md) . what to do when the pin changes
- [Prerequisites](/playbooks/prerequisites.md) . CMake/Ninja/compiler requirements
- [Running zz](/playbooks/running-zz.md) . the `cargo run` equivalent that triggers the same download on first build
- [CEF runtime concept](/browser/cef-runtime.md) . what the bundle ships at runtime
