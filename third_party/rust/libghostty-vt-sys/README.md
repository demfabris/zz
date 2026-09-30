# libghostty-vt-sys

Raw FFI bindings for libghostty-vt.

This zz-maintained snapshot builds the copy-snapshot candidate in `demfabris/ghostty`,
local commit `7823f65dd55fc9ff420d5eb5cae761cbd1995994` on `zz-copy`, with Zig 0.16.0.
Its parent is `c39414175ca2aad564b74b3f52196355f2671774`; upstream base is
`6301810a48aaa3426887a4316668f18833a40138`. The four carried changes cover C ABI memory,
spare-page reuse, history-erase trimming and owned copy snapshots. The orchestrator must
publish the candidate and replace `GHOSTTY_COPY_SHA` in `build.rs` before ordinary builds.
The safe wrapper candidate is libghostty-rs commit
`8e40135fb20e9ed91c37c374fe1d14570c386d06`; zz vendors only this sys snapshot.
No build-time source rewriting remains. See [UPSTREAM.md](UPSTREAM.md).

- Fetches and builds `libghostty-vt.a` from ghostty sources via Zig by default.
- Exposes checked-in generated bindings in `src/bindings.rs`.
- Static linking is the baseline rather than a Cargo feature. Enable the
  additive `link-dynamic` feature to link the shared library instead.
- Set `GHOSTTY_SOURCE_DIR` to force the build to use a local Ghostty checkout.
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

The copy candidate exports 205 `ghostty_*` symbols, including
`ghostty_terminal_clone_screen`. Compare these with the generated declarations in
`src/bindings.rs`, then link a C program that takes the address of each declared function
against the archive and run it. This checks the archive as well as its headers.
