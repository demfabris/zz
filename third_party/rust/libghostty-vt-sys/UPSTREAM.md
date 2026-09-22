# libghostty-vt-sys 0.2.1 Zig 0.16 snapshot

This directory is a source snapshot of `libghostty-vt-sys` from
[`Uzaaft/libghostty-rs`](https://github.com/Uzaaft/libghostty-rs) release commit
`46a9d2ac941ed600cf43c5e6299c8dfd1d3a1ef0`.

- Upstream crate version: `0.2.1`
- Upstream Ghostty pin: `a887df42c56f6de86c0fe6da9c4eeca37931e083`
- Local Ghostty pin: `20c3eae04dee606349eb21e2dd0293b203d47179`
- License: MIT OR Apache-2.0; the upstream MIT license is retained here.
- Local override: the workspace patches the git-sourced sys package to this adjacent
  snapshot from the upstream v0.2.1 release commit. The safe wrapper is not
  vendored: `libghostty-vt` resolves to upstream from the same commit.

## Local delta

Ghostty commit `7aa9591` is the upstream Zig 0.16.0 migration. The local pin advances through
`20c3eae`, which fixes the custom `memset` C ABI by accepting an `int` fill value and explicitly
truncating it to the low byte. The public libghostty-vt header is unchanged across that range, so the
checked-in Rust bindings remain those generated from the Zig 0.16 migration. The local
`cargo:rerun-if-changed` path names this snapshot's own `build.rs`.

The vendored build passes `-Dcpu=baseline` so Zig does not require the build machine's CPU
extensions. The Linux x86_64 v0.6.0 release emitted AVX-512 instructions in a memory-fill routine
that crashed with `SIGILL` on a Ryzen 7 5700X3D. Keep this CPU setting when updating the snapshot.

The build script maps arm64 iOS devices to `aarch64-ios-none` and arm64/x86_64
iOS simulators to Zig's `ios-simulator` targets for the GPUI mobile prototype.

The migrated C API changes the Kitty temporary-file medium option from a boolean to a restricted
directory string. `zz-terminal` enables the wrapper's `kitty-graphics` feature but does not call
`is_kitty_image_from_temp_file_allowed` or `set_kitty_image_from_temp_file_allowed`; those v0.2.1
methods still use the old boolean ABI. Avoid those methods on this snapshot; replace the local
override with the first upstream `libghostty-rs` release that both pins Ghostty's Zig 0.16 migration
and updates the safe Kitty API.

When replacing this snapshot, remove its git-source patch from the workspace, refresh `Cargo.lock`,
and run the focused terminal tests plus the real macOS bundle build.

## No local patch

Since 2026-09-18 zz carries no patch on the vendored terminal engine (fabrico;
see the amendment in `knowledge/designs/tui-parity.md`). The `provenance.patch`
that retained explicit indexed foreground/background flags in spare style bits,
the ICH hunk that kept the pin's stale cells after a wide insert, the build
machinery that applied them, and the safe wrapper vendored to read those fields
are all gone. `build.rs` fetches Ghostty at the pin above and builds it
pristine; the pkg-config path accepts any installed libghostty-vt.

Tabs carry no provenance. The pin prints a literal tab for every cell a tab
produced, and fabrico decided on 2026-09-18 that zz captures the spaces on
screen instead (see `knowledge/designs/tui-parity.md`).
