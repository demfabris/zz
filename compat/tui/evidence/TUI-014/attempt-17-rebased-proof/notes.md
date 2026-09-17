Final proof pass after rebasing onto origin/main 38c50df4 on 2026-09-17.

Runtime and fixture revision: 0b72e3b2. All attached and corpus runs use
one frozen zz-customize-rebased binary (SHA-256 in binary.txt), an empty
home, and explicit C.UTF-8 locale. Cargo always uses /tmp/zz-cargo.sh.
The earlier failed, interrupted and pre-rebase probes remain in attempt-16.
No environment dumps are collected. The census hook trigger reads only
its named test variable. A credential-pattern scan of all TUI-014 evidence,
including archive members, reported zero hits before this final pass.

The fix-command delta selection is 202 rows, split into four disjoint batches.
Expanding the command roster to clock-mode, suspend-client, server-access,
choose-client and copy-mode adds copy-mode-bindings and list-keys-padding.
Both extra rows pass; the complete selection contains 204 rows.
The file-only collector retains the first completed log before retries;
final logs will also be retained. The nine-row rejected-tip and current-main
baselines in attempt-16 support inherited-versus-caused attribution.

The first complete roster reports two capture-tail mismatches: switch-mode-windows
and the extra 20-column background row. A focused lifetime-then-switch probe
reproduces the first; a fresh scene passes. Raw outer tmux capture -R dumps show
zero differences across all 1920 visible cells in both probes, after excluding
internal cell flags. The history row is 41/80 allocated on the pin side and
20/20 on the zz side. Both have default, unstyled blank cells beyond column 19;
CLEARED allocation flags explain the extra reset in capture -e. This is retained
in tail-history.txt, four .grid files and tail-grid-comparison.json.

The fixture now keeps a history case comparing every decoded cell (glyph/width,
attributes, foreground/background/underline colour and link), geometry, cursor,
CLI channels and state. It excludes only grid allocation headers and internal
cell flags for that case. The two required exact capture-tail cases start with
fresh scenes, as do their preexisting controls. The style-tail sabotage still
changes exact capture serialization; a separate blank-cell background sabotage
proves the cell comparison detects a real style difference. CLI capture-pane
stdout remains byte-exact in every case. No recorded case is waived by this
fixture adjustment. The first two old-fixture roster runs are retained.

Fixture revision ede74f2d keeps the Rust runtime frozen at 0b72e3b2. The final
normal roster runs are client-3, client-4 and client-5; client-1 and client-2
loaded the earlier fixture and remain failed-run evidence. The final verifier
uses verifier-final and verify-claims-final. Both required fresh window controls
fail on the rejected c0e9bd83 binary (tail-rejected-controls.exit=1, two unmet
control expectations); the candidate passes every style control and both kinds
of sabotage (tail-isolated-controls.exit=0).

The eight-crate full test run exits 101 on three integration targets. The CLI
menu-disconnect test passes alone. The simulator seed 5eed fails in the full
run, the first solo run and the first rebuilt candidate run, then passes on the
unchanged candidate. It passes three times on current main and once on the
rebased pre-fix source a70167e3. Its cause is unresolved; this is reported as an
intermittent candidate failure, not silently assigned to main. The agent soak
slow-client test fails on both candidate and current main with the same 1501
versus 1500 update count. All failures and comparison runs are retained.

Stopped only this lane's failed old-fixture roster process groups 2398694 and
2975939 after retaining the partial pin status redraw mismatches (client-2 and
client-5, exit 143). This also stops the old loop before its third run and
self-check. Fresh runs will use the bounded redraw comparison. No other lane's
process was signaled. The normal equality path now permits up to 20 additional
screen/cursor samples, sleeping 50 ms between them, only if the first pair
mismatches and the case is not a ticking clock. CLI outputs and captured state
are never retried or changed. Persistent capture-tail and cell-background
sabotages run through the same bounded retry path and must still fail.

Stopped the older strict-settling client-4 process group 2975940 (143) and sent TERM to the older verifier child fixture 2982794 so its parent retains the partial stdout. Fresh final rosters will be client-3, client-6 and client-7, all at fixture revision 76662d8d. The final verifier will use verifier-latest.
