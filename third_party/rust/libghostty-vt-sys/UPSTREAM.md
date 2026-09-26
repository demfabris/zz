# libghostty-vt-sys snapshot for Ghostty 6301810a

This directory is a source snapshot of `libghostty-vt-sys` from
[`Uzaaft/libghostty-rs`](https://github.com/Uzaaft/libghostty-rs) commit
`359ef751c189540eafb9110b2de89ad95ce48fc3`, the head of stacked PR #99 (branch
`stack/ghostty-render-hold`) as of 2026-09-25.

- Upstream crate version: `0.2.1` (no newer release exists; the stack is unreleased)
- Upstream wrapper Ghostty pin: `56dbc4a768778753737a3b9cbe0a3f9b4e434553`
- Upstream Ghostty base: `6301810a48aaa3426887a4316668f18833a40138` (main, 2026-09-25)
- Local Ghostty pin: [`demfabris/ghostty@6fce227c`](https://github.com/demfabris/ghostty/commit/6fce227c55d288e35c9fedfd090f286cc74a8ad8)
- Fork branch: `zz-2026-09-25`; the previous pin `fa7986a9` stays on `codex/cabi-signal-stack`
- License: MIT OR Apache-2.0; the upstream MIT license is retained here.
- Local override: the workspace patches the git-sourced sys package to this adjacent
  snapshot. The safe wrapper is not vendored: `libghostty-vt` resolves to upstream
  at the same commit.

## Why an unreleased wrapper

Ghostty's C API broke after the old pin: `ghostty_terminal_mode_get/set` and
`ghostty_render_state_colors_get` are gone, `ghostty_terminal_new` takes columns and rows
instead of an options struct, and the clipboard write callback answers through an async
reply. The 0.2.1 wrapper cannot link against it. libghostty-rs master (Ghostty `22d1317`)
predates TinyIo and the scrollback line limit; the PR stack targets `56dbc4a`, ten commits
behind `6301810a`. The only header change in those ten commits is additive (render state
overscan and row ids), so the wrapper at #99 builds unchanged and its 22 unit tests, 3 sys
tests, and 20 doctests pass against `6301810a` with bindings regenerated from its headers.

The wrapper commit lives on a PR branch that Uzaaft rebases. Cargo fetches it by rev,
which works while GitHub keeps the object. Move to the first libghostty-rs release that
contains this stack (likely 0.3.0) as soon as it exists.

## Local delta

`build.rs` is the upstream file at the wrapper commit with these changes:

- `GHOSTTY_REPO` and `GHOSTTY_COMMIT` point at the fork commit above.
- `cargo:rerun-if-changed` names this snapshot's own `build.rs`.
- Cargo's dev profile builds Zig `ReleaseSafe` instead of upstream's `Debug`: the
  unoptimized VT parser is about 6x slower and blows the daemon's 2 s command budgets
  under test load. `LIBGHOSTTY_VT_SYS_OPTIMIZE=Debug` still selects `Debug`.
- The upstream Windows DLL CRT source patch and its build-time `git apply` are dropped.
  zz links the static archive on every platform, where that patch does nothing, and this
  snapshot does not rewrite fetched sources.

Upstream now covers two earlier zz deltas on its own: the default `-Dcpu=baseline`
(overridable with `LIBGHOSTTY_VT_SYS_CPU`; keep it, since the Linux x86_64 v0.6.0 release
emitted AVX-512 in a memory-fill routine that crashed with `SIGILL` on a Ryzen 7 5700X3D)
and the exact MSVC static archive name. Upstream also replaced zz's flat iOS target
mapping with an xcframework build for `aarch64-apple-ios` and `aarch64-apple-ios-sim`; no zz
iOS target links libghostty today (the GPUI iOS client renders daemon frames), so the flat
mapping and its `x86_64-apple-ios` entry were not kept.

`src/lib.rs` and `tools/gen_bindings.rs` are the upstream files. `src/bindings.rs` was
regenerated from the fork commit's headers with the upstream tool, run from a checkout of
the wrapper commit: `GHOSTTY_SOURCE_DIR=<fork checkout> cargo run -p libghostty-vt-sys
--features bindgen-tool --bin gen-bindings`.

The Kitty temporary-file medium API is fixed in this wrapper
(`set_kitty_image_temp_file_dir`); `zz-terminal` still does not call it.

When replacing this snapshot, remove its git-source patch from the workspace if the
release covers the fork pin, refresh `Cargo.lock`, and run the focused terminal tests
plus the real macOS bundle build.

## Native memory patch

The 2026-09-23 performance investigation authorizes measured dependency-fork
changes. This pin adds one line to `lib_vt.zig`'s `std_options`:

```zig
if (terminal.options.c_abi) options.signal_stack_size = null;
```

The C ABI uses host-owned threads and single-threaded IO. It never registers
Zig's alternate signal stack, but the default 256 KiB TLS buffer is instantiated
for every Rust thread on macOS. This option removes that unused buffer while
preserving signal handlers, unwinding, locks, and terminal behavior. Recheck
this assumption if the C ABI starts using Zig-owned workers or startup code; at
`6301810a` the only `std.Io.Threaded` use is the single-threaded debug IO that
ReleaseSafe and Debug builds keep for panics.

Upstream's TinyIo change (`82df79ec8`, #13715) removed the buffer from
`ReleaseFast` and `ReleaseSmall` builds, so shipped release bundles no longer
need the patch. `ReleaseSafe`, which zz uses for every dev-profile build
(`just run`, `just watch`, tests), still references
`std.Thread.maybeAttachSignalStack` and still carries it. Measured on
`libghostty-vt-static_zcu.o` built with zz's flags: plain `6301810a`
ReleaseSafe has a 262,160-byte `__thread_bss`, the fork commit 16 bytes. Both
archives export the same 204 `ghostty_*` symbols.

Validation for this pin: the wrapper's own tests against the fork source,
`cargo test -p zz-terminal`, and the real macOS bundle build. Ghostty's Debug
test suite supplies its own `std_options`, so it does not exercise this option.

The normal build fetches this immutable fork commit directly. No build-time
source rewriting is used. `GHOSTTY_SOURCE_DIR` remains authoritative, and an
enabled `pkg-config` feature can select an installed library without this fix.
When comparing overrides, use distinct source paths or rebuild the sys package:
Cargo tracks the override environment value, not edits inside that directory.

This is a native dependency, outside the Cargo-only `scripts/forks.conf` and
`just forks` workflow. Maintain it using the native Ghostty section in
`.agents/skills/fork-rebase/SKILL.md`. Preserve published commits through a
retained branch or tag before rebasing. Drop the patch when upstream provides
the same allocation behavior in ReleaseSafe, or when zz stops building
ReleaseSafe, then repin and rerun the terminal suite plus the real macOS bundle
build. No binding or safe-wrapper change is needed for this option.

## Earlier grid patches

On 2026-09-18 fabrico removed the terminal grid patches; the corresponding
capture decisions remain in `knowledge/designs/tui-parity.md`. The `provenance.patch`
that retained explicit indexed foreground/background flags in spare style bits,
the ICH hunk that kept the pin's stale cells after a wide insert, the build
machinery that applied them, and the safe wrapper vendored to read those fields
are all gone. The native memory option above does not restore that machinery or
change the grid's capture semantics.

Tabs carry no provenance. The pin prints a literal tab for every cell a tab
produced, and fabrico decided on 2026-09-18 that zz captures the spaces on
screen instead (see `knowledge/designs/tui-parity.md`).
