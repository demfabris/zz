# TUI-014 fix-pass handoff, 2026-09-17

Branch: `campaign/tui-customize-3`. Rebase base: `38c50df4c4d51401752c49b72bb689b09e33427d`.
The runtime was built at `0b72e3b242381028396272e3bea2915d39b1768c`;
fixture revision `76662d8d` adds bounded redraw sampling. The frozen binary and
its SHA-256 are recorded in `binary.txt`. The lane leaves TUI-014 at `review`.

Customize root edits choose the first free array index at submission time;
indexed edits retain the selected index and both retain the existing scope.
The attached fixture compares the complete resulting option listing for all
eight editable array options, including seeded sparse index 100. Hook arrays
are excluded from the customize tree, matching the pin. C-c no longer exits
the mode; the key audit and unbound-key comparisons are in `key-audit.md`.
The pre-rebase development evidence and failed probes remain in attempt-16.

The two switch window-row cases now assert exact capture serialization in fresh
scenes. Both fail on the rejected c0e9bd83 binary. Style controls cover dim,
foreground, background, underline and an allocation-boundary width. The history
case compares every decoded visible cell and the cursor: internal tmux grid
allocation flags can alter capture serialization while all 1920 cells remain
identical. The retained raw grids and `tail-grid-comparison.json` measure this
limit. A one-sided blank-cell background sabotage proves that the decoded
comparison does not discard visible trailing-cell attributes. Exact CLI
capture output remains byte compared.

The three copy/clock stacking cases follow fabrico's 2026-09-17 decision:
copy mode stays per client so two clients can scroll independently. They are
owned by `decided:TUI-014`; the design amendment and ledger acceptance sentence
name that decision. The interactive-refresh gap loses exactly those three
records and the two asserted window-style cases, going from 13 to 8.

The full eight-crate test command has three failing integration targets.
The CLI menu-disconnect test passes alone. The simulator seed 5eed fails in
three candidate runs and then passes unchanged; it passes three main-baseline
runs and the rebased pre-fix baseline. Its cause remains unresolved and is not
claimed inherited. The agent slow-client soak fails with 1501 versus 1500
updates on both candidate and main. Clippy with all targets/features and
-D warnings passes for all eight crates. See retained test logs and exits.

No credential environment dumps were collected. The census hook now queries
only its named test variable. Corpus logs use a minimal environment. Historical
evidence was scrubbed before the rebased branch was pushed; final archive scans
must remain clean.

Fixture and corpus totals are recorded in results.json and corpus-results.json.
The full self-check catches the array-erasure, C-c mode-exit, exact style-tail
and blank-cell background sabotages. The TUI-014 verifier exits 0. The
identity-aware attached-client run passes; earlier failures with USER absent
are retained on candidate and main. Client-6 retains two ticking-clock capture
differences; every requested fix case passes in that run.

All 204 selected corpus rows ran: 183 initially clean, 13 recovered, six
inherited baseline divergences and two documented known rows. There are no
unrun rows and no persistent corpus divergence identified as caused by this
fix pass. The source-replay confirmation timeout is byte-for-byte the one
measured in main's first baseline attempt. Archives retain first and final
logs and a verified per-file SHA-256 manifest.
