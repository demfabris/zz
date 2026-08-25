<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [NVIDIA Linux CEF accelerated OSR failure](2026-08-07-nvidia-cef-accelerated-osr.md) - Root-cause analysis of CEF 151 producing no accelerated OSR frames on NVIDIA Linux despite a complete EGL, GBM, DMA-BUF, and Vulkan stack.
* [Wayland background blur and rounded client-side corners](2026-08-09-wayland-blur-rounded-corners.md) - Why GPUI cannot match antialiased client-side window corners with ext-background-effect-v1, how the zoom and KWin coordinate bugs were corrected, and how zz removed the pane-edge backdrop seam.
* [Rendering multi-harness agent output — industry survey](2026-08-15-agent-harness-rendering-survey.md) - How comet, opencode, t3code, Zed, and other agent clients render multi-harness output, followed by zz's decision to adopt a flat ACP v1 contract.
* [Codebase Audit for Code Smells, Rust Antipatterns, and Performance Issues](2026-08-17-codebase-audit.md) - Revalidation at 758dac0 found nine confirmed issues, four qualified or latent findings, one intentional ABI contract, and one overstated impact claim.
* [tmux CLI compatibility and alias boundary](2026-08-22-tmux-cli-compatibility-audit.md) - A commit-pinned inventory of the tmux command, flag, option, format, hook, key, packaging, and native zz command surfaces, with the exact boundary around alias tmux=zz.
<!-- okf:listing:end -->
