# Concepts

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Native Agent pane](agent-pane.md) - The daemon-addressable Agent pane, its daemon-owned ACP v1 runtime, flat transcript, approvals, session controls, and restore metadata.
* [Native command palette and tmux command completions](command-palette.md) - The native top-center GPUI command palette with catalog-driven tmux completions, value prompts, history, and daemon-owned execution.
* [Daemon-owned PTY worker model](pty-worker.md) - How the daemon spawns and owns one PTY-backed terminal session per pane, the thread/ownership boundary between zz-daemon and the zz-terminal worker, and the paths that carry terminal frames out and send-keys in.
* [Session persistence & daemon lifecycle](session-persistence.md) - Why mux state and terminals outlive GPUI windows, which GUI state is disk-backed, which browser/Agent descriptors can restore, and how attach, detach, eviction, per-client state, and local transport work when several devices share one session.
* [Cell-authoritative split-pane layout](split-pane-layout.md) - How a window's panes are arranged as an n-ary cell tree ported from tmux's layout.c — split/resize/preset algorithms in layout.rs, the derived binary wire projection with stable ^split ids, measurement-driven window extent, and the invariants model.rs enforces.
* [Terminal frame (TerminalViewport)](terminal-frame.md) - The immutable, renderer-neutral terminal snapshot (packed cells, interned styles, overlays, cursor, and modes) published from the worker thread and diffed into retained-grid patches.
<!-- okf:listing:end -->
