---
type: Playbook
title: Updating the CEF pin
description: The coordinated steps required to bump zz's CEF dependency, refresh its artifact reference and cache key, and run all three platform bundle smoke tests.
resource: third_party/cef/ARTIFACTS.md
tags: [cef, upgrade, playbook, sha1, coordinated-change]
timestamp: 2026-07-15T02:25:00Z
---

# Overview

The Rust `cef`/`cef-dll-sys` version in `Cargo.lock` selects the CEF release. During a build,
`download-cef` fetches CEF's official `index.json` and verifies the selected archive against that
published SHA-1. [`third_party/cef/ARTIFACTS.md`](/references/cef-artifacts.md) is a reviewable
in-repo mirror; the downloader never reads it. Keep the dependency, mirror, CI cache key, and
Linux/macOS/Windows bundle smoke tests coordinated so reviewers and operators see the same release
the build consumes (see [build/verify a CEF bundle](/playbooks/build-cef-bundle.md)).

# Steps

1. **Choose the new CEF release.** Find the target `cef`/`cef-dll-sys` Rust package version (format
   `<rust-pkg-version>+<cef-version>`, e.g. current `151.2.0+151.3.14` mapping to CEF
   `151.3.14+g5d67476+chromium-151.0.7922.72`).
2. **Check which wgpu major the release wants.** `accelerated_osr` hands GPUI's own
   `wgpu::Device` to `cef::osr_texture_import`, so the `cef` crate's `wgpu` dependency must be the
   *same* major as [GPUI's](/references/gpui-revision.md) . two majors in the graph is a type
   mismatch at that call, not a duplicate-crate annoyance. `cef` 150.2.1 moved to wgpu 30 while Zed
   was still on 29, which is why the 151 bump also carried a gpui patch (fork commit
   `gpui: build the wgpu renderer against wgpu 30`) and a `wgpu = "=30.0.0"` workspace bump. Check
   the crate's changelog for a `update wgpu to vN` entry before assuming a bump is dependency-only.
3. **Bump the workspace dependency.** Update `cef = "=151.2.0"` in the root `Cargo.toml` if the
   Rust package's major/minor version changed, then regenerate the lock:
   ```sh
   cargo update -p cef -p cef-dll-sys
   ```
   Confirm `Cargo.lock` now resolves to the new `<rust-pkg-version>+<cef-version>` string.
4. **Fetch the new `index.json`.** Read
   `https://cef-builds.spotifycdn.com/index.json` (see the citation format already used in
   `ARTIFACTS.md`) and collect the minimal-distribution archive name and published SHA-1 for every
   Rust target zz supports:
   - `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `arm-unknown-linux-gnueabi`
   - `x86_64-apple-darwin`, `aarch64-apple-darwin`
   - `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`, `i686-pc-windows-msvc`
5. **Rewrite `third_party/cef/ARTIFACTS.md`.** Update the CEF version string at the top, replace
   every row of the archive-name/SHA-1 table, and update the "read from `index.json` on `<date>`"
   line to the date you read it. See the
   [current pinned reference](/references/cef-artifacts.md) for the exact shape to preserve.
6. **Re-run all three platform bundle smoke tests together.** `download-cef` verifies the archive's
   SHA-1 against the live official `index.json` before extracting; `ARTIFACTS.md` does not influence
   that check.
   Exercise Linux, macOS, and Windows before merging; CI's matrix (`ubuntu-24.04`, `macos-15`,
   `windows-2025`) does this automatically via:
   ```sh
   cargo xtask bundle-cef --release --output target/cef-bundle
   ```
   See [build/verify a CEF bundle](/playbooks/build-cef-bundle.md) for what this exercises locally.
7. **Bump the CI cache key.** `.github/workflows/ci.yml` and `.github/workflows/release.yml` cache
   the downloaded distribution under `cef-151.3.14-${{ runner.os }}-${{ runner.arch }}`; update the
   version segment in both so a stale cache entry isn't reused for the new pin.
8. **Commit everything together.** `Cargo.lock`, `Cargo.toml` (if changed), `ARTIFACTS.md`, and the
   CI cache-key bump belong in one change, verified against all three platforms before merge.

# Key files

| File | Role |
| --- | --- |
| `Cargo.toml` | `cef = "=151.2.0"` workspace version constraint, and the `wgpu` pin that must match it |
| `Cargo.lock` | Exact resolved `cef`/`cef-dll-sys` version (`<rust-pkg>+<cef-version>`) |
| `third_party/cef/ARTIFACTS.md` | Reviewable mirror of the official per-target archive name + SHA-1 table |
| `.github/workflows/ci.yml`, `release.yml` | `CEF_PATH` cache keyed on the CEF version, matrix across all three OSes |
| `crates/zz-xtask/src/main.rs` | Where the download/verify/build/bundle flow is invoked from |

# Related

- [CEF artifact pin reference](/references/cef-artifacts.md) . the table this playbook rewrites
- [Build/verify a CEF bundle](/playbooks/build-cef-bundle.md) . the smoke test this playbook re-runs
- [xtask crate](/crates/zz-xtask.md) . the tool performing download/verify/build/bundle
- [Prerequisites](/playbooks/prerequisites.md) . toolchain needed to rebuild the CEF wrapper after bumping
