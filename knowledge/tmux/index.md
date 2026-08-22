# tmux

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Sidebar navigation and native choosers](choose-tree.md) - Persistent sidebar navigation for sessions and windows plus daemon-owned pane and paste-buffer choosers, with tmux-style keyboard movement and activation.
* [tmux command set (command.rs)](commands.md) - MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon.
* [tmux-grammar config parser (parser.rs)](conf-parser.md) - A single-pass tmux-style tokenizer plus the daemon replay layer that keeps stored zz/config mux overrides above the sourced zz/mux.conf configuration.
* [Copy mode and view mode](copy-mode.md) - Daemon-native tmux copy/view mode over libghostty history: vi/emacs movement tables, selection, incremental search, jumps, and copy/pipe variants, driven by send-keys -X and painted by GPUI.
* [tmux divergence matrix](divergences.md) - Every known divergence from tmux at the pinned reference commit: the 12 missing commands and why, the 29 implemented commands that still reject tmux flags, behavioral gaps on the implemented surface, the options coverage (all 180 store, 104 behave), and the protocol-level differences.
* [Key tables (key.rs)](key-tables.md) - Root/prefix/copy-mode/chooser key resolution with the default C-b prefix and optional prefix2, canonical and shifted key encoding, bind/unbind, send-prefix (-2), numeric vi counts, pending jump-key capture, and wire publication of every table.
* [tmux status line in the sidebar](status-line.md) - The daemon expands status-left and status-right from the zz-owned mux.conf into text and publishes it per client; the workspace sidebar renders it as a stacked bottom section instead of a bottom bar.
* [tmux compatibility philosophy](tmux-compat.md) - zz reimplements a deliberately-scoped subset of tmux behavior in pure Rust, checked against a pinned upstream commit; it never compiles, links, or runs tmux.
<!-- okf:listing:end -->
