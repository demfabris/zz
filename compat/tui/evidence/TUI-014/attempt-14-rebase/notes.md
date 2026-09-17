Rebased campaign/tui-customize (11831758) onto origin/main eef2df94 on 2026-09-16.
The replay keeps main's native/tmux exit-code split and moves the pane-mode CLI regression
into crates/zz-cli/tests/cli_binary.rs. The wire document has one unreleased v104 entry,
including the Native* errors, PaneSnapshot.mode, Customize and ClientSuspendState.
The generated gap report was regenerated at each conflict. The ledger now names the moved CLI files.

The checkpoint uses target/debug/zz_cli, built through /tmp/zz-cargo.sh. Fixtures set
SHELL=/bin/sh and isolate their homes, configuration and sockets. The compatibility gate
uses the capped wrapper through an exported cargo function and an empty HOME.

A preliminary pinned probe of fresh switch-mode -Z in a split window exited with
"server exited unexpectedly", both without and with an attached client. The attached
attempt is retained in pin-attached-stack.txt. No success is claimed for that probe.

Checkpoint measurements: client roster exits 0 with 147 asserted and 28 recorded,
unattributed=0; its self-check exits 0. Choosers exits 1 with 77/78 asserted,
0 recorded: zoom-manual-released retains Z on zz's status row. The same failure
was recorded by the inherited customize lane. Formatting exits 0 and knowledge
validation exits 0 (one existing warning).

pin-lifetime-probe.sh exits 0. It opens -Z before splitting, zooms during the
mode, observes unzoom on Escape, then opens -kZ on an already-zoomed pane,
covers it with clock, observes the switch survive the clock's Escape, and
observes the source pane disappear on the next Escape.

compat/check.sh exits 101: 535 mux tests pass and three stale expectations fail.
The checkpoint corrects clock-mode's usage test to use -Q, updates the agent-send
usage assertion to main's catalog, and registers new-agent-session in the native
command group. Filtered reruns are queued; their results will follow this checkpoint.
