# Wire and comparator correction, 2026-09-16

This supplements attempt-06; it does not replace or erase those runs. The branch
was already rebased onto be5709dfcd9936d44519683e5da8dc6ab7748d96. A fresh fetch
confirmed that origin/main still points there.

The initial contract incorrectly treated protocol 103 as unreleased. It shipped
in v0.10.0. This correction sets the protocol constant, internal decimal pin,
hunt decimal pin and both hello byte pins to 104 / 0x68. PaneSnapshot.mode remains
the last field, after border_status_text, with serde(default). No field moves.
The v103 history matches origin/main exactly; the mode append is in a new v104
entry. The version gate still rejects mismatched protocol versions.

Both switch window comparators now collect session names and window indices
before sorting. Orphaned windows whose session is missing are omitted. Sorting
performs no map lookup. A regression test exercises both the row builder and
selected target with equal names, then removes sessions while retaining windows.
It checks normal alpha, cli, zulu ordering and empty results when all sessions
are gone. The outer-tmux captures independently verify alpha:win, cli:win,
zulu:win. Existing fixture assertions have not changed.

The orchestrator's added-comment count refers to the older prepush base. The
exact command against this rebased branch returns zero, as does an additional
scan including the current correction. No pre-existing comment was removed.

The frozen binary is target/debug/zz-modes-v104; binary-sha256.txt identifies it.
The recorded shell for these fixtures is /bin/sh. Cargo uses the capped wrapper;
the attempt-06 PATH adapter routes Cargo inside compat/check.sh through it and
places the wrapper's trailing --jobs before a command's -- delimiter.

TUI-014 remains active. customize-mode-open and suspend-client remain attributed
to TUI-014 for the sibling customize lane. The existing lifecycle, style, copy
ordering and other-owner records remain measured records. No new case is flipped
by this correction. The four lane flips against main remain clock-mode-open,
switch-mode, server-access-bare and server-access-user.

The complete delta corpus and other package/fixture proofs in attempt-06 were
run with protocol 103. This correction does not claim those historical runs were
performed with 104. The nine corpus failures, two known-divergence rows and
inherited-shell chooser cache limitation in attempt-06 remain unresolved. The
full delta is not rerun for the wire-number and comparator robustness correction.

The v104 outer probe passed 25 assertions with zero failures and two retained
lifecycle records. The full live verifier passed: client commands 139 asserted /
30 recorded (TUI-014=2, unattributed=0), choosers 78 asserted / zero recorded.
The client-command self-check caught every sabotage and passed both equivalences.
The protocol package passed 227 unit tests, seven bounded-payload tests and 16
hunt claims. All seven requested crates passed clippy with all targets, all
features and -D warnings. Formatting, compat/check.sh and wire-version.py passed.

The first full daemon run failed one of 938 tests: default_shell_rejects_invalid_values_and_unset_controls_the_child_shell
timed out capturing a terminal. Its isolated rerun passed in 0.48 seconds. This
matches the load-sensitive failure class described by AGENTS.md; it is evidence
of a load flake, not proof of its root cause. Both outputs are retained. A full
retry limits libtest concurrency to four threads to reduce competing PTY load.

The full daemon retry passed: 938 library tests and all package integration/doc
targets, exit zero. The implementing revision is 2740ec94a1f713a46fd4b356394a3fec2ccebf7a.

During evidence assembly, the ledger generator rejected the not-yet-created
footprint artifact. Creating the declared file resolved that validation error;
the ledger and generated report were then regenerated and checked.

The final compatibility gate after the ledger update also passed, exit zero.
OKF validation passed with zero errors and one pre-existing stale-research warning.
The footprint includes every changed path against the be5709df merge base.
