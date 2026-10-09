# libghostty-vt snapshot

The safe wrapper from [`demfabris/libghostty-rs`](https://github.com/demfabris/libghostty-rs)
commit `0db98a206681fd60c2b1a1719daf14049eda8c30` (branch `zz-2026-10-04`), a fork of
[`Uzaaft/libghostty-rs`](https://github.com/Uzaaft/libghostty-rs) stacked PR #99 at
`359ef751c189540eafb9110b2de89ad95ce48fc3`. The fork adds owned copy snapshots, row cell
copies, the row and cell iteration lifetime fix, render state clips and trimmed row copies.

Only `crates/libghostty-vt` is kept: `Cargo.toml`, `README.md` and `src/`. The local delta
is the manifest: workspace-inherited fields are spelled out, `libghostty-vt-sys` points at
the adjacent `../libghostty-vt-sys` snapshot, and the duplicate dev-dependency is gone.

License: MIT OR Apache-2.0; the upstream `LICENSE` is retained.

To take a newer wrapper, diff `crates/libghostty-vt` between `0db98a20` and the new commit
and apply it here, keeping the manifest delta above. Move back to crates.io at the first
libghostty-rs release that contains the stack (likely 0.3.0).
