Quick review of this branch against main for one item of zz's tmux catch-up campaign
(compat/catchup/README.md). The item's ledger entry is below; its acceptance clauses are the contract.

Look for, in this order:
1. Correctness bugs in the changed code: wrong behaviour, panics, races, lost data, wrong error text.
2. Places where the changed behaviour differs from pinned tmux. The pin's source is in
   compat/.cache/tmux-src (read it; do not build or run it).
3. Acceptance clauses that are claimed but not proved by a test or a recorded command.
4. Wire mistakes: a serde type under crates/zz-protocol/src changed while PROTOCOL_VERSION still
   equals the version the newest v* tag shipped (107 shipped in v0.16.0).
5. Registry edits in compat/tmux-gaps.json that close or narrow a gap the code does not back.

Do not run cargo, the test suites or the compat harness; read the diff and the code around it.
Report at most 10 findings, most severe first, each as: severity (P0 breaks users or data, P1 wrong
behaviour, P2 worth fixing), file:line, one sentence on the defect, one on the failing case.
If nothing survives, answer exactly: LGTM.
