# TUI-011 close-out, cycle 11 gate 7

TUI-011 is a parent obligation. Its written condition was that once TUI-014,
TUI-015, TUI-016, TUI-017 and TUI-018 verify, its roster carries no recorded
entry left and it verifies on the same fixture. This gate is that run.

## The roster tally, three consecutive runs, byte for byte the same

    all 536 asserted comparisons identical, 42 recorded not asserted
    (0 for a sibling lane, owners decided:TUI-014=3 decided:TUI-015=4
     decided:TUI-016=1 decided:TUI-017=25 gap:clients.interactive-refresh=8
     gap:protocol.socket-acl=1 unattributed=0)

Every remaining entry is either a dated decision or an accepted gap. No
obligation holds an ordinary owner record, and `unattributed=0` means no case
is recorded without an owner. That is the condition, met.

## The rest of the suite at the same revision

mouse three runs at 67 asserted identical; client-commands and mouse
`--self-check` each caught every sabotage in its own channel; command-streams
208 asserted identical with zero recorded; copy-mode 147; screen-diff 147 with
its own six records; choosers 78; overlays 48; superset 197; attached-client
PASS against pinned tmux next-3.8; verify-claims answered `every verified
obligation holds up` for the whole ledger and again for TUI-015, TUI-017 and
TUI-018 individually; workspace clippy with `-D warnings` clean; `cargo fmt
--check` clean; `compat/check.sh` exit 0.

## Honest limits

`check.sh` first came back 101 on this tree. The failure was not in the lane
work: the orchestrator had filed the pane_current_command divergence as
`format:pane_current_command`, and compat_manifest_tests asserts that every
tracked `format:` item names a constant-backed placeholder variable. That
format is genuinely backed, so the item was refiled as
`semantic:pane-current-command-empty` and the suite was re-run to exit 0. The
tally above is from the run whose check.sh exited 0.

TUI-013 remains unmeasured. It is a recorded macOS geometry-report timeout and
is outside this baseline; nothing here depends on it.
