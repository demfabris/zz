---
type: Reference
title: proc-macro-error2 future-compatibility patch
description: Provenance and maintenance policy for zz's vendored proc-macro-error2 2.0.1 visibility fix.
resource: third_party/rust/proc-macro-error2/UPSTREAM.md
tags:
- rust
- cargo
- dependencies
- proc-macro
timestamp: 2026-07-15T02:27:21Z
---

# Overview

`gpui` depends on `stacksafe`, whose `stacksafe-macro` crate uses `proc-macro-error2`. Version 2.0.1
publicly re-exports a private `extern crate proc_macro`; Rust 1.97 accepts that code but reports the
`pub_use_of_private_extern_crate` future incompatibility. The upstream project was archived before
its one-line fix was merged, so zz owns a minimal source patch under `third_party/rust/` rather than
depending on an unmerged fork.

# Patch schema

| Field | Value |
| --- | --- |
| Package | `proc-macro-error2 2.0.1` |
| Consumer chain | `zz → gpui → stacksafe → stacksafe-macro → proc-macro-error2` |
| Vendored source | `third_party/rust/proc-macro-error2/` |
| Cargo override | `[patch.crates-io]` in the workspace `Cargo.toml` |
| Published checksum | `11ec05c52be0a07b08061f7dd003e7d7092e0472bc731b4af7bb1ef876109802` |
| Upstream fix | PR `GnomedDev/proc-macro-error-2#14`, commit `53ff94b` |
| Rust tracking issue | `rust-lang/rust#127909` |

The only library-source delta is:

```diff
-extern crate proc_macro;
+#[doc(hidden)]
+pub extern crate proc_macro;
```

Both upstream licenses and the package metadata remain beside the source. Provenance details and
refresh instructions live in `third_party/rust/proc-macro-error2/UPSTREAM.md`, the in-repo source of
truth named by this concept's `resource` field.

# Verification and retirement

- `cargo tree -i proc-macro-error2 --locked` must resolve the package from the in-repo path.
- A clean `cargo check -p zz --bin zz --locked` must compile the patched crate without a future-
  incompatibility footer.
- A working tree that compiled the registry package before this override may retain an old generated
  report in `target/`; a fresh checkout or fully clean patched build has no current report.
- Remove the override and vendored source when the [pinned GPUI revision](/references/gpui-revision.md)
  no longer reaches this crate or a maintained compatible release contains the fix.

# Related

- [Toolchain prerequisites](/playbooks/prerequisites.md) . Rust version that currently emits the lint.
- [Pinned GPUI revision](/references/gpui-revision.md) . the direct workspace dependency at the head
  of the transitive chain.
