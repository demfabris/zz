# libghostty-vt-sys 0.2.1 Zig 0.16 snapshot

This directory is a source snapshot of `libghostty-vt-sys` from
[`Uzaaft/libghostty-rs`](https://github.com/Uzaaft/libghostty-rs) release commit
`46a9d2ac941ed600cf43c5e6299c8dfd1d3a1ef0`.

- Upstream crate version: `0.2.1`
- Upstream Ghostty pin: `a887df42c56f6de86c0fe6da9c4eeca37931e083`
- Local Ghostty pin: `20c3eae04dee606349eb21e2dd0293b203d47179`
- License: MIT OR Apache-2.0; the upstream MIT license is retained here.
- Local override: the workspace patches both git-sourced wrapper packages to adjacent
  snapshots from the same upstream v0.2.1 release commit.

## Local delta

Ghostty commit `7aa9591` is the upstream Zig 0.16.0 migration. The local pin advances through
`20c3eae`, which fixes the custom `memset` C ABI by accepting an `int` fill value and explicitly
truncating it to the low byte. The public libghostty-vt header is unchanged across that range, so the
checked-in Rust bindings remain those generated from the Zig 0.16 migration. The local
`cargo:rerun-if-changed` path names this snapshot's own `build.rs`.

The vendored build passes `-Dcpu=baseline` so Zig does not require the build machine's CPU
extensions. The Linux x86_64 v0.6.0 release emitted AVX-512 instructions in a memory-fill routine
that crashed with `SIGILL` on a Ryzen 7 5700X3D. Keep this CPU setting when updating the snapshot.

The migrated C API changes the Kitty temporary-file medium option from a boolean to a restricted
directory string. `zz-terminal` enables the wrapper's `kitty-graphics` feature but does not call
`is_kitty_image_from_temp_file_allowed` or `set_kitty_image_from_temp_file_allowed`; those v0.2.1
methods still use the old boolean ABI. Avoid those methods on this snapshot; replace the local
override with the first upstream `libghostty-rs` release that both pins Ghostty's Zig 0.16 migration
and updates the safe Kitty API.

When replacing this snapshot, remove its git-source patch from the workspace, refresh `Cargo.lock`,
and run the focused terminal tests plus the real macOS bundle build.

## Capture provenance

`provenance.patch` retains explicit indexed foreground/background flags in the
style's spare bits, including erased backgrounds, and tab spans in spare cell
bits. The cell and style allocations keep their existing size. The C style
uses its trailing padding for two booleans; cell queries append tags 12 and 13.
The adjacent safe wrapper exposes these fields. Both wrappers must accompany
this patch; a stock prebuilt libghostty-vt does not implement the new queries.

The HT path uses the cursor's cached cell pointer, validates at most 32 adjacent
cells with a packed comparison, and marks the row dirty once. It performs no
page lookup per column. A head stores the tab width; padding stores 128. Cell
moves preserve both facts, while printing clears adjacent padding and its head
where tmux would overwrite them. Capture reads the head and padding separately,
so inserting, deleting or erasing part of a tab does not erase its surviving
head. ICH clears the vacated source range, matching the pin when the insert
count exceeds the number of cells moved.

The build applies the patch to its fetched or explicitly supplied Ghostty source
and records the applied patch. On a later patch change it reverses only that
recorded patch before applying the new one; a conflicting source edit fails.
The pkg-config path accepts only packages declaring `zz_capture_provenance=1`;
it falls back to the patched source build for an unmarked library.
