# TUI-006 attempt-02 — cycle 6 choosers punch list

Every file here is a real run on the alienware box (CachyOS, 16 cores, 15 GB) against pinned
tmux d77c9dc6 at `compat/.cache/tmux-src/tmux`. `environment.txt` is the host, the repo revision,
the pin, the toolchain and the hash of the `zz` binary under test, written before the first run.

## the runs

| file | command | result |
|---|---|---|
| `environment.txt` | host, revision, pin, toolchain, binary hash | — |
| `run-01-tip.txt` | `compat/tui-choosers.sh` | exit 0, 53 asserted identical, 4 recorded |
| `run-02-tip.txt` | `compat/tui-choosers.sh` | exit 0, same |
| `run-03-tip.txt` | `compat/tui-choosers.sh` | exit 0, same |
| `run-04-tip.txt` | `compat/tui-choosers.sh` at the final tip | exit 0, same |
| `self-check-01-tip.txt` | `compat/tui-choosers.sh --self-check` | exit 0, six sabotages caught, two equivalences quiet |
| `self-check-02-tip.txt` | `compat/tui-choosers.sh --self-check` at the final tip | exit 0, same |
| `stock-keys-regression.txt` | `compat/tui-stock-keys.sh` | exit 0, 50 cases agree |
| `screen-diff-regression.txt` | `compat/tui-screen-diff.sh` | exit 0, 125 checkpoints identical |
| `attached-client-regression.txt` | `compat/attached-client.sh target/debug/zz <pin>` | exit 0, PASS |
| `copy-mode-regression.txt` | `compat/tui-copy-mode.sh` before the message-row rule was narrowed | exit 1, four search-prompt cases differ |
| `copy-mode-regression-2.txt` | `compat/tui-copy-mode.sh` after it | exit 0, 147 cases agree |
| `run-05-rebased-tip.txt` | `compat/tui-choosers.sh` after the rebase onto the caps landing | exit 0, same |
| `self-check-03-rebased-tip.txt` | `compat/tui-choosers.sh --self-check` after that rebase | exit 0, same |
| `pin-filter-clear.txt` | pin-only probe: `prefix w`, `f`, a window filter, `Enter`, `c` | the rebuild rule |
| `pin-filter-clear-sessions.txt` | pin-only probe: `prefix s`, the same, plus a session created while the tree is open | the same rule under `-s` |
| `zz-protocol-unit-tests.txt` | `cargo test -p zz-protocol` | exit 0 |
| `zz-mux-unit-tests.txt` | `cargo test -p zz-mux` | exit 0 |
| `zz-tui-unit-tests.txt` | `cargo test -p zz-tui` | exit 0 |
| `zz-client-unit-tests.txt` | `cargo test -p zz-client` | exit 0 |
| `zz-daemon-unit-tests.txt` | `cargo test -p zz-daemon` | exit 0 |
| `zz-integration-tests.txt` | `cargo test -p zz` | exit 0 |
| `clippy.txt` | `cargo clippy -p <crate> --all-targets --all-features -- -D warnings`, one block per touched crate | exit 0 each |

Every cargo command ran through the box's two-slot lock under `MemoryMax=5G`, `--jobs 4` and
`--test-threads=3`.

The branch was rebased onto `origin/main` once more after the cycle-6 caps gate pushed the wire to
102 and the colour-class frames with it. Everything above except `run-01`..`run-04`,
`self-check-01`, `self-check-02` and `copy-mode-regression.txt` was re-run at that rebased tip and
is the run recorded here; `run-05` and `self-check-03` are the obligation's fixture at it. The two
v102 entries were merged by hand into one, keeping both lanes' text. `screen-diff-regression.txt`
reads 137 asserted and 16 recorded there against 125 and 28 before it, which is the caps landing's
colour-class work closing twelve of its own records.

## the two pin probes

`pin-filter-clear.txt` and `pin-filter-clear-sessions.txt` are the measurement the daemon's rebuild
rule rests on. Both drive one pinned tmux server inside an outer pinned tmux, on throwaway sockets
under a scrubbed HOME, and capture the mode screen at each step. Under `choose-tree -Zw` and again
under `choose-tree -Zs`, a filter that hides rows and a `c` that clears it bring every hidden row
back EXPANDED while the row that survived the filter keeps the state it had, and a session created
while the tree is open arrives expanded on the next rebuild. That is `mode_tree_build` moving the
last build's rows to `mtd->saved` and `mode_tree_add` reading each row's expansion out of it.

## what the runs do not cover

`compat/tui-choosers.sh` records four cases and says why in the run output: the two command-output
pane-prompt rows, the search marks a selection leaves painted, and `choose-client`, which zz does
not implement. The ledger note carries the same measurements.
