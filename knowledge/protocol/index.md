<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Stable object IDs ($session @window %pane ^split c)](ids.md) - The sigil-prefixed u64 newtype identifiers for sessions, windows, panes, splits, and clients: stable across the daemon lifetime and parsed/formatted with tmux-style prefixes.
* [Mux snapshots (snapshot.rs)](snapshots.md) - The MuxSnapshot state tree (sessions, windows, recursive split layouts, pane descriptors, behavior flags, per-client focus, and viewer presence) that clients reconcile on attach and after a resync.
* [PaneFrame terminal lane (pane_frame.rs)](terminal-lanes.md) - The Terminal envelope lane that carries full viewports, span patches, command-output viewports and history chunks as varint headers, changed-metadata fields and rows of style runs with UTF-8 text, decoded by every client straight into PackedCell planes.
* [zz wire protocol (v108)](wire-protocol.md) - The versioned, little-endian length-prefixed, postcard-encoded control protocol whose ProtocolMessage enum carries the entire client/daemon conversation over local IPC or an SSH tunnel.
<!-- okf:listing:end -->
