<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Stable object IDs ($session @window %pane ^split c)](ids.md) - The sigil-prefixed u64 newtype identifiers for sessions, windows, panes, splits, and clients: stable across the daemon lifetime and parsed/formatted with tmux-style prefixes.
* [Mux snapshots (snapshot.rs)](snapshots.md) - The MuxSnapshot state tree (sessions, windows, recursive split layouts, pane descriptors, behavior flags, per-client focus, and viewer presence) that clients reconcile on attach and after a resync.
* [Packed terminal lanes (terminal_codec.rs)](terminal-lanes.md) - The hand-packed, fixed-width Terminal envelope lane that fans immutable terminal viewports and row patches out to clients with deduplicated style and grapheme dictionaries over one ordered stream, local or ssh-forwarded.
* [zz wire protocol (v96)](wire-protocol.md) - The versioned, little-endian length-prefixed, postcard-encoded control protocol whose ProtocolMessage enum carries the entire client/daemon conversation over local IPC or an SSH tunnel.
<!-- okf:listing:end -->
