# tmux

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Concepts

* [Sidebar navigation and native choosers](choose-tree.md) - Persistent sidebar navigation for sessions and windows plus daemon-owned pane and paste-buffer choosers, with tmux-style keyboard movement and activation.
* [tmux command set (command.rs)](commands.md) - MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon.
* [tmux-grammar config parser (parser.rs)](conf-parser.md) - A single-pass tmux-style tokenizer plus the daemon replay layer that keeps stored zz/config mux overrides above the sourced zz/mux.conf configuration.
* [Copy mode and view mode](copy-mode.md) - Daemon-native tmux copy/view mode over libghostty history: vi/emacs movement tables, selection, incremental search, jumps, and copy/pipe variants, driven by send-keys -X and painted by GPUI.
* [tmux divergence matrix](divergences.md) - Dated rationale and source evidence for measured tmux divergences, including command, flag, behavior, option, format, hook, and protocol differences.
* [tmux compatibility gap report](gaps.md) - Live TODO and status report for tmux compatibility gaps, decisions, evidence, and acceptance gates.
* [Key tables (key.rs)](key-tables.md) - Root/prefix/copy-mode/chooser key resolution with the default C-b prefix and optional prefix2, canonical and shifted key encoding, bind/unbind, send-prefix (-2), numeric vi counts, pending jump-key capture, and wire publication of every table.
* [tmux status rows and format expansion](status-line.md) - The daemon expands tmux status formats per client for the cell-faithful TUI; GUI clients build native bars from snapshots and app settings without consuming StatusLine.
* [tmux compatibility philosophy](tmux-compat.md) - The contract for a tmux-compatible zz CLI: tmux spellings keep tmux meaning or fail loudly, native GUI behavior uses zz-only verbs, and compatibility is measured against one pinned upstream commit.
<!-- okf:listing:end -->
