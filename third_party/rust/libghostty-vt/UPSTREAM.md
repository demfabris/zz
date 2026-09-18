# libghostty-vt 0.2.1

Source snapshot from Uzaaft/libghostty-rs at
`46a9d2ac941ed600cf43c5e6299c8dfd1d3a1ef0`, matching the existing sys snapshot.
The manifest uses standalone package metadata and the adjacent sys dependency.
Source comments are omitted to follow this repository's contribution rule.

The local API exposes `Style.fg_indexed`, `Style.bg_indexed` and
`Cell.bg_indexed()`. The sys crate's `provenance.patch` supplies these facts.
No raw pointer or private Rust layout access is needed by zz-terminal.
