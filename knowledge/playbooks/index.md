# Playbooks

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Building and verifying a platform CEF bundle](build-cef-bundle.md) - Step-by-step use of cargo xtask and release recipes to assemble, sign, notarize, and validate platform CEF bundles.
* [Toolchain and system prerequisites](prerequisites.md) - The exact toolchain versions and per-platform system libraries required to build zz, pinned by rust-toolchain.toml, mise.toml, and CI.
* [Building and running zz](running-zz.md) - How to build and run the zz GPUI client and its daemon, what the first build downloads, and how to exercise the browser pane with the loopback fixture.
* [Updating the CEF pin](updating-cef.md) - The coordinated steps required to bump zz's CEF dependency, refresh its artifact reference and cache key, and run all three platform bundle smoke tests.
* [Updating the pinned tmux behavioral reference](updating-tmux-reference.md) - How to bump zz's pinned tmux upstream commit and re-verify the Rust tmux-compat reimplementation against it.
<!-- okf:listing:end -->
