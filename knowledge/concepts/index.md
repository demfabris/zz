# Concepts

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Native Agent pane](agent-pane.md) - The daemon-addressable Agent pane, pane-local Codex/Claude Code ACP runtime, provider artifact profiles, nested subagent and terminal streams, sticky controls, approvals, and restore metadata.
* [Native command palette and tmux command completions](command-palette.md) - The native top-center GPUI command palette with catalog-driven tmux completions, value prompts, history, and daemon-owned execution.
* [Daemon-owned PTY worker model](pty-worker.md) - How the daemon spawns and owns one PTY-backed terminal session per pane, the thread/ownership boundary between zz-daemon and the zz-terminal worker, and the paths that carry terminal frames out and send-keys in.
* [Session persistence & daemon lifecycle](session-persistence.md) - Why mux state and terminals outlive GPUI windows, which GUI state is disk-backed, which browser/Agent descriptors can restore, and how attach, detach, eviction, per-client state, and local transport work when several devices share one session.
* [Binary split-pane layout tree](split-pane-layout.md) - How a window's panes are arranged as a recursive binary LayoutNode tree of ^split nodes with axes and ratios, plus the reconciliation, resize, directional-navigation, and preset logic in model.rs.
* [Terminal frame (TerminalViewport)](terminal-frame.md) - The immutable, renderer-neutral terminal snapshot (packed cells, interned styles, overlays, cursor, and modes) published from the worker thread and diffed into retained-grid patches.
<!-- okf:listing:end -->
