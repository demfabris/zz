# proc-macro-error2 2.0.1 vendored patch

This directory is a source snapshot of the crates.io package `proc-macro-error2` version `2.0.1`.

- Upstream repository: <https://github.com/GnomedDev/proc-macro-error-2>
- Crates.io checksum: `11ec05c52be0a07b08061f7dd003e7d7092e0472bc731b4af7bb1ef876109802`
- License: MIT OR Apache-2.0; both upstream license files are retained here.
- Local override: the workspace root maps `proc-macro-error2` to this directory with
  `[patch.crates-io]`.

## Local delta

The only library-source change from the published 2.0.1 package is the visibility fix proposed in
upstream PR [#14](https://github.com/GnomedDev/proc-macro-error-2/pull/14), commit `53ff94b`:

```diff
-extern crate proc_macro;
+#[doc(hidden)]
+pub extern crate proc_macro;
```

This resolves Rust's `pub_use_of_private_extern_crate` future-incompatibility lint tracked in
<https://github.com/rust-lang/rust/issues/127909>. The upstream repository was archived before the
fix was merged, so zz carries the minimal patch locally to keep future Rust upgrades buildable.

When replacing this snapshot, preserve both license files, verify the upstream package checksum,
reapply the local delta if the release still needs it, refresh `Cargo.lock`, and run the workspace
build and future-incompatibility checks.
