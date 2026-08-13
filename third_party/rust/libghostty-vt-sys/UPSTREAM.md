# libghostty-vt-sys 0.2.1 Zig 0.16 snapshot

This directory is a source snapshot of `libghostty-vt-sys` from
[`Uzaaft/libghostty-rs`](https://github.com/Uzaaft/libghostty-rs) release commit
`46a9d2ac941ed600cf43c5e6299c8dfd1d3a1ef0`.

- Upstream crate version: `0.2.1`
- Upstream Ghostty pin: `a887df42c56f6de86c0fe6da9c4eeca37931e083`
- Local Ghostty pin: `7aa9591746ffa4d2eee458960c76554352832595`
- License: MIT OR Apache-2.0; the upstream MIT license is retained here.
- Local override: the workspace patches the git-sourced `libghostty-vt-sys` package to this
  directory while leaving the safe `libghostty-vt` crate on its upstream v0.2.1 release commit.

## Local delta

Ghostty commit `7aa9591` is the upstream Zig 0.16.0 migration. The build-script pin is updated to
that commit, the checked-in Rust bindings are regenerated from its `include/ghostty/vt.h`, and the
local `cargo:rerun-if-changed` path names this snapshot's own `build.rs`.

The migrated C API changes the Kitty temporary-file medium option from a boolean to a restricted
directory string. zz builds `libghostty-vt` with `default-features = false`, so the v0.2.1 safe
wrapper's old Kitty graphics method is not compiled. Do not enable that feature on this snapshot;
replace the local override with the first upstream `libghostty-rs` release that both pins Ghostty's
Zig 0.16 migration and updates the safe Kitty API.

When replacing this snapshot, remove its git-source patch from the workspace, refresh `Cargo.lock`,
and run the focused terminal tests plus the real macOS bundle build.
