# TUI-003 attempt-01

Stock launcher, command and key behaviour, measured on alienware against pinned
tmux d77c9dc6. Every file here is the output of a run that happened; nothing is
described that did not run.

## What runs

Two new fixtures, both copying the outer-pinned-tmux driver shape
`compat/tui-screen-diff.sh` established (two windows in one outer tmux, an
isolated HOME and XDG_CONFIG_HOME per side, short `/tmp` sockets, bounded
`wait_for` on an observable, settle = the observable on screen and the screen
unchanged between two polls):

- `compat/tui-stock-keys.sh` — clause 2 and the key half of clause 3. Types the
  stock chords into the attached CLIENT'S STDIN (`send-keys` against the OUTER
  pane), then compares three channels: every decoded screen cell, the cursor
  tuple, and a server-facts line each side's own server answers. A case names
  the channels it asserts; a channel it does not name is recorded with the
  measurement that explains it.
- `compat/tui-launch-diff.sh` — clause 1 and the config-root half of clause 3.
  Runs each binary the way a user runs it and compares the exact stdout, stderr
  and exit status as one string, the server facts, and, where the case attaches,
  the screen and cursor inside the outer tmux.

Both take `--self-check`, which sabotages one channel at a time on one side and
requires the report to catch it in that channel, plus equivalences it must not
report.

## Files

| File | What it is |
| --- | --- |
| `environment.txt` | Both binaries' sha256, the pin's commit, the OS line, TERM, shell, locale, sizes, and how the config roots are scrubbed. |
| `revision.txt` | The tip that ran the proofs, plus `git status --short` at that tip. |
| `proofs.txt` | Every proof command re-run at the final tip, with its exit status and its headline result. |
| `stock-keys.run.txt` | `compat/tui-stock-keys.sh` at the final tip: stdout and stderr. |
| `stock-keys.self-check.txt` | `compat/tui-stock-keys.sh --self-check` at the final tip. |
| `launch-diff.run.txt` | `compat/tui-launch-diff.sh` at the final tip. |
| `launch-diff.self-check.txt` | `compat/tui-launch-diff.sh --self-check` at the final tip. |
| `screen-diff.run.txt` | `compat/tui-screen-diff.sh` at the final tip, the whole-screen regression. |
| `pane-geometry.run.txt` | `compat/tui-pane-geometry.sh` at the final tip, the geometry regression. |
| `cargo-tests.txt` | `cargo test -p zz-protocol -p zz-tui -p zz-client` and `cargo test -p zz` (the cli_binary integration rule) at the final tip. |
| `clippy.txt` | Clippy on every touched crate at the final tip. |
| `default-bindings.txt` | `list-keys -T prefix` for the four keys this batch changed, both sides, before and after. |
| `screen.80x24.split-horizontal.zz.txt`, `.tmux.txt` | Both screens after `prefix %` at 80x24. Everything matches except the border cell's background: the pin writes the style's `#101010`, zz writes its theme's `16,19,24`, because the border background is not on the wire. |
| `screen.80x24.chooser-sessions.zz.txt`, `.tmux.txt` | Both screens after `prefix s`. The binding now runs the pin's `choose-tree -Zs` on both sides; the surface it paints is zz's own overlay against the pin's mode tree, which is the recorded presentation divergence. |
| `facts.100x24.split-horizontal.txt` | The 100x24 split: both servers report pane 0 at 50 columns and pane 1 at 49, and the drawn border still sits one column apart. That is the `layout.rs` floored ratio, not a geometry difference. |
| `launch-log-noise.before-fix.txt` | The zz screen of `new-session -s fresh` BEFORE the diagnostics fix: seven rows of `level=INFO role=tui … process_start` where the pin shows a prompt. |
| `launch-scrollback.after-fix.txt` | The same region after the fix, both sides, with no log record on either. |
| `replacement-bindings.txt` | zz accepts `bind -n M-s focus-sidebar` and `bind -n C-\ detach-client` and lists both in the root table; the pin answers `unknown command: focus-sidebar`, which is why the sidebar half is measured here and the detach half is a differential case (`root-binding-detaches`). |
| `border-column-sweep.txt` | The drawn vertical border column against the daemon's own `#{pane_left}`/`#{pane_right}` at ten client widths, which is how the `layout.rs` rounding was pinned down. |

## What changed in the product

- `crates/zz-protocol/src/key.rs`: `prefix %` → `split-window -h`, `prefix "` →
  `split-window`, `prefix s` → `choose-tree -Zs`, `prefix w` → `choose-tree -Zw`,
  the pin's exact stored commands. `shifted_character` no longer drops the shift
  under Alt, so `M-S` stops folding to `M-s`; Control still folds, because a
  control byte carries no case.
- `crates/zz-client/src/chrome.rs` and `crates/zz-tui/src/input.rs`: the raw
  TUI's chrome keeps only the tables whose surface owns the key. `C-\`, `M-s`
  and `M-S` reach the daemon like every other key, so a user can bind them and
  an application can receive them.
- `crates/zz/src/diagnostics/mod.rs`: every spelling of the raw-terminal client
  owns the ring log, and the ring log stops mirroring to a terminal that belongs
  to the client's own screen.
- `compat/tmux-gaps.json`: the four `binding:prefix:` items that stopped
  diverging left `keys.default-prefix`'s item list, which the manifest test in
  `crates/zz-mux/src/compat_manifest_tests.rs` asserts exactly; the measurement
  behind that is appended to that group's reason, dated, and the group is
  otherwise untouched.
