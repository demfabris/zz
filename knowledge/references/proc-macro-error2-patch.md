---
type: Reference
title: proc-macro-error2 patch (retired)
description: Retired on 2026-09-12 after the dependency graph stopped using proc-macro-error2.
resource: Cargo.toml
tags:
- rust
- cargo
- dependencies
- proc-macro
timestamp: 2026-09-12T23:50:00Z
---

# Retirement

The September 2026 dependency cleanup removed the unused Cargo override and
`third_party/rust/proc-macro-error2/` snapshot. Neither `proc-macro-error2` nor
`stacksafe` appears in the resolved workspace graph at the
[current GPUI revision](/references/gpui-revision.md).

The old dependency chain was `gpui → stacksafe → stacksafe-macro → proc-macro-error2`.
In zz commit `bed7d693`, `stacksafe-macro` 1.0.3 replaced its dependency on
`proc-macro-error2` with `proc-macro2`; the later GPUI rebase removed `stacksafe`.

# Historical provenance

zz vendored `proc-macro-error2` 2.0.1 to fix the
`pub_use_of_private_extern_crate` future-incompatibility warning on Rust 1.97.
The sole library change made `extern crate proc_macro` public with `#[doc(hidden)]`,
matching upstream PR `GnomedDev/proc-macro-error-2#14`, commit `53ff94b`.
The published package checksum was
`11ec05c52be0a07b08061f7dd003e7d7092e0472bc731b4af7bb1ef876109802`.
The former source, MIT and Apache licenses, and full patch remain in Git history.
