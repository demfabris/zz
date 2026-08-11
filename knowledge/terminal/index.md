# Terminal

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Terminal appearance and color model](appearance.md) - The renderer-neutral appearance model, the native zz/config override resolver, per-key provenance, the client-side Ghostty import loader, and embedded Ghostty/X11 colors.
* [Terminal interaction (input, selection, paste, words)](interaction.md) - The renderer-neutral pointer, keyboard, word-boundary, and paste layer that turns client gestures into libghostty encoding, native selection, and copy-mode actions, plus the client-side local scroll overlay.
* [libghostty-vt embedding](libghostty-vt.md) - How zz-terminal embeds libghostty-vt v0.2.1 over a pinned Ghostty Zig 0.16 snapshot, including terminal color-query replies and single-worker-thread ownership.
* [PTY drain topology (the IO fast path)](pty-drain.md) - How macOS keeps its tuned inline PTY actor while Linux overlaps a bounded gather stage with VT parsing; includes the probe and benchmark results behind each platform choice.
* [Zed GPUI terminal rendering parity](rendering-parity.md) - The effort to bring zz's terminal painting up to Zed's GPUI standard by mapping immutable renderer-neutral frames and dirty-row patches onto GPUI text, cursor, and overlay painting.
<!-- okf:listing:end -->
