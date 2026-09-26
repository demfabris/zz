# libghostty-vt-sys

Raw FFI bindings for libghostty-vt.

This zz-maintained snapshot builds `demfabris/ghostty` commit `6fce227c55d288e35c9fedfd090f286cc74a8ad8` with Zig 0.16.0. The fork adds one C ABI memory option on upstream `6301810a48aaa3426887a4316668f18833a40138`; see [UPSTREAM.md](UPSTREAM.md).

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
  Without it, Cargo's dev profile builds `ReleaseSafe` and optimized profiles
  build `ReleaseFast`.
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
