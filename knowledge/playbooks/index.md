# Playbooks

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Running the browser client](browser-client.md) - Build the shared GPUI browser client and connect it to a zz daemon through the local WebSocket gateway.
* [Building and verifying a platform CEF bundle](build-cef-bundle.md) - Step-by-step use of cargo xtask and release recipes to assemble, sign, notarize, and validate platform CEF bundles.
* [Running the tmux compatibility harness](compat-harness.md) - How to run the pinned tmux differential corpus, read topology, geometry, format, and query-stdout results, and record known divergences.
* [Native macOS terminal client](native-macos-client.md) - Build the Swift terminal client, connect it to a zz daemon, and verify native input, layout, and reconnect.
* [Native macOS component gallery](native-macos-gallery.md) - Build, run, and verify the native Swift component library, with zz-ui coverage and the Rust client boundary.
* [Toolchain and system prerequisites](prerequisites.md) - The exact toolchain versions and per-platform system libraries required to build zz, pinned by rust-toolchain.toml, mise.toml, and CI.
* [Building and running zz](running-zz.md) - How to build and run the zz GPUI client and its daemon, what the first build downloads, and how to exercise the browser pane with the loopback fixture.
* [Running tmux compatibility cohorts](tmux-compat-cohorts.md) - A bounded, parallel workflow for closing the practical alias tmux=zz gap without letting new oracle findings extend one campaign forever.
* [Updating the CEF pin](updating-cef.md) - The coordinated steps required to bump zz's CEF dependency, refresh its artifact reference and cache key, and run all three platform bundle smoke tests.
* [Updating the pinned tmux behavioral reference](updating-tmux-reference.md) - How to bump zz's pinned tmux upstream commit and re-verify the Rust tmux-compat reimplementation against it.
<!-- okf:listing:end -->
