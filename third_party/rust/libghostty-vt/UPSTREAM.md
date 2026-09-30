# libghostty-vt snapshot

This is the safe wrapper from `demfabris/libghostty-rs` at
`359ef751c189540eafb9110b2de89ad95ce48fc3`, version 0.2.1. The workspace patches the
original git dependency to this source directory; the original pin remains in
`Cargo.toml`. The package declares MIT OR Apache-2.0; the upstream MIT license is retained here.

The wrapper is excluded from the root workspace, like the sys crate. Its tests and
all-feature checks run separately. This keeps workspace feature unification from
enabling its optional dynamic link mode in zz's static C client archive.

Other local changes pass strict all-feature lint checks and use `advance` for the lending
Kitty placement iterator instead of a standard iterator `next` method.

Local additions are `Terminal::clone_screen`, the owned `ScreenSnapshot`, and
`GridRow` for resolving a row once before reading its cells. A snapshot is Send and
has no public terminal access, callbacks or owned tracking handles. Grid references
borrow it, so resize and destruction cannot race a row reader. Anchored resize keeps
its tracking handle inside the operation. Color refresh copies the live appearance
without copying live content.

The native implementation is in the adjacent sys crate's `copy-mode.patch`; see its
`UPSTREAM.md` for allocation, sharing, pruning and build details. Snapshot tests cover
source destruction, concurrent live mutation, compressed nested clones, styled
Unicode and hyperlinks, source callback isolation, appearance changes and narrowing
history beyond the live pruning limit.

When updating the wrapper, preserve these extensions and tests until upstream offers
an equivalent safe API. Keep the root Cargo patch and lockfile consistent.
