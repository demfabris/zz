# libghostty-vt-sys

Raw FFI bindings for libghostty-vt.

This zz-maintained snapshot builds the vendored Ghostty in `third_party/ghostty` with Zig
0.16.0: upstream `6301810a48aaa3426887a4316668f18833a40138`, trimmed to what this build reads,
plus eight zz commits covering C ABI memory, spare-page reuse, history-erase trimming, owned
copy snapshots, copied active pages at their used size, one-call row cell copies, render state
clips and trimmed row copies. Before 2026-10-09 the same commits were fetched from
`demfabris/ghostty` (`e482b03688ccc9eebd6304176aa85bd5d81f0bfa` on `zz-2026-10-04`).
The safe wrapper pin is published libghostty-rs commit
`0db98a206681fd60c2b1a1719daf14049eda8c30` on new branch `zz-2026-10-04` (clip and trimmed
copy on top of `f5f826018e290e776c8bc4e5969c562efe530846`, which stays on `zz-2026-10-02` with
row copies `d975339f` and the iteration lifetime fix; `8e40135fb20e9ed91c37c374fe1d14570c386d06`
stays on `zz-2026-09-30`); its parent `359ef751c189540eafb9110b2de89ad95ce48fc3` remains on
`zz-2026-09-25`.
zz vendors the wrapper too, next to this crate in `../libghostty-vt`.
No build-time source rewriting remains. See [UPSTREAM.md](UPSTREAM.md).

- Builds `libghostty-vt.a` from `third_party/ghostty` via Zig by default.
- Exposes checked-in generated bindings in `src/bindings.rs`.
- Static linking is the baseline rather than a Cargo feature. Enable the
  additive `link-dynamic` feature to link the shared library instead.
- Set `GHOSTTY_SOURCE_DIR` to build another Ghostty checkout instead.
- Set `GHOSTTY_ZIG_SYSTEM_DIR` to force Zig package resolution through a
  pre-fetched `zig build --system` directory. This is intended for Nix and other
  sandboxed package managers that cannot fetch during build scripts.
- Vendored builds target Zig's portable `baseline` CPU by default. Set
  `LIBGHOSTTY_VT_SYS_CPU` to `native`, a named CPU model such as `x86_64_v3`, or
  another Zig CPU expression to optimize for known deployment hardware.
- Set `LIBGHOSTTY_VT_SYS_OPTIMIZE` to `Debug`, `ReleaseSafe`, `ReleaseFast`, or
  `ReleaseSmall` to override the Zig optimize mode used by vendored builds.
  Without it, profiles Cargo reports as `PROFILE=debug` (dev, test) build
  `ReleaseSafe`; release-family profiles build `ReleaseSmall` at `opt-level`
  `s` or `z` and `ReleaseFast` otherwise, including release profiles that add
  debug info.
- iOS targets (`aarch64-apple-ios`, `aarch64-apple-ios-sim`) build through
  ghostty's xcframework emit instead of a flat cross build. This requires a
  macOS host with Xcode and the iOS SDK installed, and supports static linking
  only. The simulator library is arm64-only, so `x86_64-apple-ios` is not
  supported.
- If the `pkg-config` feature is enabled, the build will use an installed
  `libghostty-vt` found through `pkg-config` only when `GHOSTTY_SOURCE_DIR` is
  unset. With the default static link mode, it probes Ghostty's
  `libghostty-vt-static` pkg-config module instead.
- libghostty-vt is pre-1.0, so these bindings do not guarantee compatibility
  with arbitrary installed C API revisions.

## C ABI check

After building the native checkout, list the exports:

```sh
nm -g --defined-only <native checkout>/zig-out/lib/libghostty-vt.a
```

The published clip and trim pin exports 206 `ghostty_*` symbols, including
`ghostty_terminal_clone_screen` and `ghostty_render_state_row_cells_copy`; the clip and the
trim flag add no symbols. Compare these with the generated declarations in
`src/bindings.rs`, then link a C program that takes the address of each declared function
against the archive and run it. This checks the archive as well as its headers.
