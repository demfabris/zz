# libghostty-vt-sys snapshot for Ghostty 6301810a

This directory is a source snapshot of `libghostty-vt-sys` from
[`Uzaaft/libghostty-rs`](https://github.com/Uzaaft/libghostty-rs) commit
`359ef751c189540eafb9110b2de89ad95ce48fc3`, the head of stacked PR #99 (branch
`stack/ghostty-render-hold`) as of 2026-09-25.

- Upstream crate version: `0.2.1` (no newer release exists; the stack is unreleased)
- Upstream wrapper Ghostty pin: `56dbc4a768778753737a3b9cbe0a3f9b4e434553`
- Upstream Ghostty base: `6301810a48aaa3426887a4316668f18833a40138` (main, 2026-09-25)
- Published Ghostty pin: `e482b03688ccc9eebd6304176aa85bd5d81f0bfa` on `demfabris/ghostty` branch `zz-2026-10-04` (two commits, the render state clip `0ab7941cb98263423377627b8b17d8d090c504bd` and the trimmed row copy, on `189df4a1f6403f5bdc349fe44d1d2809741a4c1d`), pinned in `build.rs`. `zz-2026-10-02` keeps `189df4a1` (the one-call row cell copy, on `67351380b6dc30124938d809809ac0aa42813283`) and `zz-2026-09-30` keeps `67351380`. The branch fast-forwards retain copy snapshots `7823f65dd55fc9ff420d5eb5cae761cbd1995994` and trim fix `c39414175ca2aad564b74b3f52196355f2671774` in their history.
- Fork history: eight commits on upstream: the C ABI signal-stack option (`6fce227c`, still on `zz-2026-09-25`), the PageList spare-page reuse (`713374af`: line-limit pruning keeps the last pruned pool page resident for the next grow instead of decommitting and refaulting it; `compress` releases it and trims the last page), the trim fix (`c3941417`: preserves live cell blocks after history erase), owned copy snapshots (`7823f65d`), copied active pages at their used size (`67351380`), the one-call row cell copy (`189df4a1`, `ghostty_render_state_row_cells_copy`), the render state clip (`0ab7941c`, `GHOSTTY_RENDER_STATE_OPTION_CLIP`) and trimmed row copies (`e482b036`, `GHOSTTY_RENDER_STATE_ROW_CELLS_COPY_TRIM`). `zz-2026-09-29` keeps `713374af`; the previous pin `fa7986a9` stays on `codex/cabi-signal-stack`
- License: MIT OR Apache-2.0; the upstream MIT license is retained here.
- Wrapper source: [`demfabris/libghostty-rs`](https://github.com/demfabris/libghostty-rs),
  published commit `0db98a206681fd60c2b1a1719daf14049eda8c30` on new branch
  `zz-2026-10-04` (the clip and trimmed copy API), pinned in the workspace manifest.
  `f5f826018e290e776c8bc4e5969c562efe530846` (row copies `d975339f` plus the row and cell
  iteration lifetime fix) stays on `zz-2026-10-02`, `8e40135f` on `zz-2026-09-30`. Its
  parent `359ef751c189540eafb9110b2de89ad95ce48fc3` remains on `zz-2026-09-25`.
- Local override: the workspace patches the git-sourced sys package to this adjacent
  snapshot. The safe wrapper comes from the dependency fork, with owned copy
  snapshots and bounded row references. zz does not vendor the safe wrapper.

## Why an unreleased wrapper

Ghostty's C API broke after the old pin: `ghostty_terminal_mode_get/set` and
`ghostty_render_state_colors_get` are gone, `ghostty_terminal_new` takes columns and rows
instead of an options struct, and the clipboard write callback answers through an async
reply. The 0.2.1 wrapper cannot link against it. libghostty-rs master (Ghostty `22d1317`)
predates TinyIo and the scrollback line limit; the PR stack targets `56dbc4a`, ten commits
behind `6301810a`. The only header change in those ten commits is additive (render state
overscan and row ids), so the wrapper at #99 builds unchanged and its 22 unit tests, 3 sys
tests, and 20 doctests pass against `6301810a` with bindings regenerated from its headers.

The base wrapper commit lives on a PR branch that Uzaaft rebases, so the workspace fetches
the copy API from the `demfabris/libghostty-rs` fork's `zz-2026-10-04` branch.
Branch `zz-2026-09-25` keeps the base commit reachable.
Move back to upstream at the first libghostty-rs release that contains this stack (likely
0.3.0), and change both the dependency URL and the `[patch]` key in `Cargo.toml`.

## Local delta

`build.rs` is the upstream file at the wrapper commit with these changes:

- `GHOSTTY_REPO` and `GHOSTTY_COMMIT` point at the fork commit above.
- `cargo:rerun-if-changed` names this snapshot's own `build.rs`.
- The Zig mode follows Cargo's `PROFILE`, not `DEBUG`: `debug` (dev and test) builds
  `ReleaseSafe` instead of upstream's `Debug` (the unoptimized VT parser is about 6x
  slower and blows the daemon's 2 s command budgets under test load); release-family
  profiles build `ReleaseSmall` at `OPT_LEVEL` `s`/`z` and `ReleaseFast` otherwise, so
  profiles that only add debug info (`profiling`, `testflight`) build the release
  engine. `LIBGHOSTTY_VT_SYS_OPTIMIZE=Debug` still selects `Debug`.
- The upstream Windows DLL CRT source patch and its build-time `git apply` are dropped.
  zz links the static archive on every platform, where that patch does nothing, and this
  Windows patch is not applied. zz does not rewrite native sources at build time.

Upstream now covers two earlier zz deltas on its own: the default `-Dcpu=baseline`
(overridable with `LIBGHOSTTY_VT_SYS_CPU`; keep it, since the Linux x86_64 v0.6.0 release
emitted AVX-512 in a memory-fill routine that crashed with `SIGILL` on a Ryzen 7 5700X3D)
and the exact MSVC static archive name. Upstream also replaced zz's flat iOS target
mapping with an xcframework build for `aarch64-apple-ios` and `aarch64-apple-ios-sim`; no zz
iOS target links libghostty today (the GPUI iOS client renders daemon frames), so the flat
mapping and its `x86_64-apple-ios` entry were not kept.

`src/lib.rs` retains the base snapshot without patch stamps. `build.rs` uses a source
override directly and contains no source staging, patch application, reversal or hashing.
`tools/gen_bindings.rs` borrows the prefix array with `iter()` instead of `into_iter()` so
all-feature strict lint checks pass. `src/bindings.rs` comes from the native fork headers,
including `ghostty_terminal_clone_screen`, generated with the snapshot's tool:

```sh
GHOSTTY_SOURCE_DIR=<native fork checkout> cargo run --manifest-path third_party/rust/libghostty-vt-sys/Cargo.toml --features bindgen-tool --bin gen-bindings
```

The optional pkg-config path checks that the installed header declares the snapshot API
before emitting link metadata. A compatible installed archive must export it as well.

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

The normal build fetches the pinned native fork commit without rewriting source.
`GHOSTTY_SOURCE_DIR` selects a local checkout directly. An enabled `pkg-config` feature
can select an installed library with `ghostty_terminal_clone_screen`.
When comparing overrides, use distinct source paths or rebuild the sys package:
Cargo tracks the override environment value, not edits inside that directory.

This is a native dependency. Maintain it using the native Ghostty section in
`.agents/skills/fork-rebase/SKILL.md`. Preserve published commits through a
retained branch or tag before rebasing. Drop the signal-stack change when upstream provides
the same allocation behavior in ReleaseSafe, or when zz stops building
ReleaseSafe, then repin and rerun the terminal suite plus the real macOS bundle
build. No binding or safe-wrapper change is needed for this option.

## Copy snapshots

Native fork commit `7823f65dd55fc9ff420d5eb5cae761cbd1995994` adds a C ABI clone of the active screen. Copy-on-write cloning skips the
page-count pass used to preheat eager clone storage. Compressed history shares atomic
reference-counted encoded buffers when allocator identities match; other allocators receive
independent encoded copies. Each cloned compressed page starts without a private raw mapping;
first read maps it, and dropping it unread releases encoded ownership without restoring it.
Each screen owns its page metadata and cursor pins. Resident history uses software
copy-on-write; active pages are copied eagerly so cached cursor pointers stay valid. Read-only
grid getters preserve sharing. Resize reads source metadata and rows without detaching pages
that it will replace with reflowed output. The original pane can recover pooled mappings when
a shared history page becomes exclusive again, keeping spare-page reuse after leaving copy
mode.

Fork commit `67351380b6dc30124938d809809ac0aa42813283` sizes the eager copy of each active page to
its used rows when cloning copy-on-write, and always copies through `cloneFrom`. Before it, a
snapshot of a small active area allocated standard pages and read their unused tails, which
macOS charged to the footprint: copy entry on the Mac fell from 1.41 to 0.58 MiB (Linux was
already 0.59). Its regression test is "copy-on-write small active clone avoids standard-page
backing"; the full native suite passes (6508 passed, 52 skipped).

The C clone constructor initializes its owned terminal directly from the frozen ScreenSet.
It skips the four raw page mappings and pool/pin bookkeeping of a blank terminal that would
otherwise be discarded immediately. Ordinary terminal initialization keeps its existing
default-cursor setup; clone initialization keeps existing screen-clone cursor/SGR semantics,
copies the configured terminal cursor defaults, and creates fresh parser state.

The clone uses the default allocator. Compressed ownership compares allocator function tables
first, recognizes Zig allocators with undefined context pointers, and compares contexts only
for allocators that define them. This is exercised by a dense 10k x 180 default-allocator
ReleaseFast regression, in addition to custom-allocator isolation checks.

The clone retains no source callbacks and lifts its own pruning limits so a frozen resize
preserves all reflowed history. The live pane keeps its original limits. The dependency-fork safe
wrapper exposes `ScreenSnapshot` with owned metadata and borrowed grid references, plus
controlled scroll, color, compression and anchored resize operations. Frozen resize uses
primary backing with wrapping and history pulling enabled, even when the source was in the
alternate screen or had wrapping disabled. The wrapper retains the source screen identity
separately. It exposes no terminal or owned tracking handle that could escape while the
snapshot moves to a search thread. Row references check the owning page dimensions before
reading a cell, including incomplete reflow.

The native extension and its regression tests live in published Ghostty fork commits
`7823f65dd55fc9ff420d5eb5cae761cbd1995994` and `67351380b6dc30124938d809809ac0aa42813283`; the safe API and its tests live in published
libghostty-rs commit `8e40135fb20e9ed91c37c374fe1d14570c386d06`. Both forks expose their
copy commits on `zz-2026-09-30`, and zz pins those commits for fetched-source builds.
Pre-publication validation used a fresh native source path and a temporary wrapper path
patch, removed before repinning. Full native tests pass (6490 passed, 68 skipped); the wrapper
default suite passes (30 wrapper and 3 sys tests, 19 doctests, 3 doctests ignored).
Native exports include all 205 `ghostty_*` symbols, and all 199 generated function
declarations resolve in a C client that links and runs. Standalone Debug fixtures bound
rich metadata and mutation work; separate 1000-row and 10k ownership regressions remain.

## Row cell copy

Frame build used to read every cell through about nine C calls (`row_cells_next`, the style,
foreground, background, UTF-8 grapheme and raw cell getters, then the raw cell's content tag,
width and hyperlink). Native fork commit `189df4a1f6403f5bdc349fe44d1d2809741a4c1d` adds
`ghostty_render_state_row_cells_copy`, which writes a column range of the current row into
packed `GhosttyRenderStateCell` entries in one call. Each cell carries its content (codepoint,
background palette index or packed RGB, by content tag), width, hyperlink and protection flags,
semantic content, a style table index and a grapheme table index. The style table writes each
distinct style once after the default style at index 0, deduplicated by runs and a 256-slot
direct-mapped hash cache, so a collision only adds a duplicate entry. Multi-codepoint graphemes
are UTF-8 spans in a byte buffer. Short buffers return `GHOSTTY_OUT_OF_SPACE` with every needed
length, as `GhosttyBuffer` does; the call allocates nothing and starts no thread. Its tests
compare the copy with the per-cell getters on styled, grapheme, wide, hyperlink and
background-only rows, and check short buffers and clamped ranges; the full native suite passes
(6510 passed, 52 skipped).

Wrapper commit `d975339f7144b0c57c178c0eeb1ea411a99b554e` exposes it as
`CellIteration::copy_into` filling a reusable `CellsCopy`, pins its own sys crate to the native
commit, and regenerates its bindings; its suite passes (31 unit tests, 19 doctests) with a test
against the per-cell getters. This snapshot's `src/bindings.rs` is regenerated from the same
headers (additive only). Both commits are published on branches `zz-2026-10-02`, on top of
`67351380` and `8e40135` respectively; the iteration lifetime fix `f5f82601` followed on the
wrapper branch. Rebase note: the native change touches only `src/terminal/c/render.zig`, `main.zig`,
`types.zig`, `src/lib_vt.zig` and `include/ghostty/vt/render.h`, and the wrapper change only
`render.rs` plus the regenerated sys bindings and pin, so both replay on any base that keeps the
render state row cells API.

## Render state clip and trimmed row copies

A pane resized far past its clients' size (`resize-window -x 10000 -y 10000` with a 120x40
client) made every frame update copy the whole viewport into the render state: about 70 ms
and 800 MB resident at 10000x10000 on alienware, with `ghostty_render_state_row_cells_copy`
another 24% of the frame. Ghostty's own resize, page allocation and reflow were about 0.1%.

Native fork commit `0ab7941cb98263423377627b8b17d8d090c504bd` adds a clip to the render state:
`GHOSTTY_RENDER_STATE_OPTION_CLIP` takes a `GhosttyRenderStateClip` of a first viewport row, a
row count and a column count from the left edge (zero counts mean "to the edge"), and
`GHOSTTY_RENDER_STATE_DATA_CLIP` reports what an update captured. An update copies only the
clipped rows and columns; overscan is captured around the clipped rows. `ROWS`, `COLS`,
`ROW_DATA_VIEWPORT_Y` and the cursor keep viewport coordinates, and the cursor is reported only
inside the clip. A clip change forces a full redraw. The default clip is the whole viewport, so
callers that never set it see no change. Its tests compare clipped states with the matching part
of an unclipped state across the randomized incremental-update run, with and without overscan.

Native fork commit `e482b03688ccc9eebd6304176aa85bd5d81f0bfa` appends `flags` to
`GhosttyRenderStateRowCellsCopy`. With `GHOSTTY_RENDER_STATE_ROW_CELLS_COPY_TRIM` the copy stops
after the last cell of the range that is not a default (all-zero) cell, found by a backward scan
eight cells at a time, so the caller fills the blank tail itself. A struct size that ends before
`flags` still copies, without flags, so callers built against `189df4a1` keep working.

Neither change adds a symbol: the archive exports the same 206 `ghostty_*` symbols, and all 200
generated function declarations resolve in a C client that links and runs. The full native
suite passes (6525 passed, 52 skipped). This snapshot's `src/bindings.rs` is regenerated from
the `e482b036` headers with the snapshot's `gen-bindings` tool (additive only).

Wrapper commit `0db98a206681fd60c2b1a1719daf14049eda8c30` exposes them as
`RenderState::set_clip`, `Snapshot::clip` with a `Clip { y, rows, cols }`, and
`CellIteration::copy_trimmed_into`; `copy_into` keeps its behavior. It pins its own sys crate to
`e482b036` with the same regenerated bindings; its suite passes (33 unit tests, 3 sys tests, 19
doctests) with tests for the clipped rows, columns and cursor and for trimmed copies against
full ones. Rebase note: the native change touches only `src/terminal/render.zig`,
`src/terminal/c/render.zig`, `src/terminal/c/types.zig` and `include/ghostty/vt/render.h`, and
the wrapper change only `render.rs` plus the sys bindings and pin.

## Earlier grid patches

On 2026-09-18 fabrico removed the terminal grid patches; the corresponding
capture decisions remain in `knowledge/designs/tui-parity.md`. The `provenance.patch`
that retained explicit indexed foreground/background flags in spare style bits,
the ICH hunk that kept the pin's stale cells after a wide insert, the build
machinery that applied them, and the safe wrapper vendored to read those fields
are all gone. The copy snapshot extension adds ownership and row access in the dependency forks.
It does not restore the removed capture-provenance patches or safe-wrapper vendoring.

Tabs carry no provenance. The pin prints a literal tab for every cell a tab
produced, and fabrico decided on 2026-09-18 that zz captures the spaces on
screen instead (see `knowledge/designs/tui-parity.md`).
