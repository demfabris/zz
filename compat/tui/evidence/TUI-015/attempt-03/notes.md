# Empty exact window components before panes

The follow-up starts from capture-12 c550a396 and rebases onto main f9c52359.
The rebase required regenerating knowledge/tmux/gaps.md. Git dropped the native
command expectation commit because main already contains it.

The pinned cmd-find.c separates the session, window and pane components, removes
the exact-match prefixes, then treats empty components as absent. The previous
resolver normalized a window suffix equal to `=` but missed `=.0`, `=.%0` and
`=.`. This fix also strips the marker when the window component before the pane
separator is empty. The normalization remains inside TargetSlot::Session;
window and pane fallback lookup keeps its existing classification.

The detached probe runs all three commands against twelve targets. Before the
fix, the retained previous-tip binary differs in 30 of 36 comparisons. This is
an old-binary reproduction, not a claim about a newly built main binary. The pin
accepts cli:=.0, cli:=.%0, =cli:=.0, :=.0 and cli:=.; it accepts a pane in another
window of cli, rejects a pane in another session with `can't find pane: %2\n`,
and rejects cli:=.9 with `can't find pane: 9\n`. An omitted session permits a
global absolute pane. The exact stdout, stderr and status are in before.json.

The shared fixture adds six assertions, two targets across lock-session,
has-session and list-windows. Each has a self-check that first checks equivalence
and then injects the old erroneous rejection into the zz result. These are new
assertions, not recorded-to-asserted flips. The carried lock-client flip remains.

Main's session_targets_accept_pane_and_window_ids test and targets.txt remain
byte-identical. The existing compound-target regression now also checks the
neighbours above; the window-target containment test remains unchanged.

Residual ownership stays unchanged: TUI-015 owns three pane-base-index records.
TUI-017 owns FOUR records: capture-low-indexed-colour, capture-tab-trailing,
capture-tab-internal and capture-tab-wide. The sibling residuals lane owns their
implementation work. Neither obligation is promoted from review.

The candidate passes all 108 detached comparisons across three layouts. The
additional layouts put literal %0 outside cli's active window and in another
session, respectively. For cli:=.%0 all three commands accept the same-session
pane and reject the foreign pane with exactly `can't find pane: %0\n` and exit 1.
The three raw JSON files preserve every command result.

The full zz-mux package run passes 535 library tests and 105 integration tests.
The four filtered session-target tests pass, including main's unchanged bare-ID
pin. The full package also includes window_targets_accept_pane_forms_like_tmux.
Mux clippy with all targets/features and -D warnings, workspace formatting,
compat/check.sh, the wire guard and knowledge validation pass. Cargo always
runs through /tmp/zz-cargo.sh. One own childless build-slot waiter was cancelled
with exit 143 and requeued onto the available shared slot through the unchanged
wrapper; the cancelled wait and successful build are both retained.

All three shared-fixture runs pass: 198 asserted comparisons identical, 32
recorded, unattributed=0. TUI-015 owns three ordinary records and TUI-017 owns
four in every run. The self-check passes all 110 expectations, including the
six new rejection sabotages listed in self-check-summary.json. The separate
targets.txt run passes its 20 executable steps; its 21 source lines are unchanged.

The delta selection for the three requested commands contains 155 rows, including
the standard smoke corpus and targets.txt. HEAD..HEAD restricts selection to
those commands and the mandatory smoke set. The full-branch selection for these same three commands adds the changed
capture-pane.txt row, for 156 rows. That additional row runs separately. The
orchestrator's re-review accepted the earlier, broader 193-row selection for
all commands touched by the carried branch; this follow-up does not claim to
repeat all 193. A fresh clean-main f9c52359 binary also passes the previously failing
alias-group-forgery row, so that old failure is not carried forward here.

The command delta completes with 155 rows, 151 passing after retries, exit 1.
The additional capture-pane row passes all 34 steps, covering the full 156-row
selection for these commands and the branch diff. All 155 final transcripts
are retained. Eight rows failed on the first pass; four passed the runner's
isolated retry. cli-chain-parse-abort and default-client-command still fail
with the same complete transcript on freshly built main (apart from the source
path). plugin-runtime-resurrect-restore also fails identically against main:
the pin-side pane title assertion fails while zz restores the expected panes.

jobs-command-environment and source-replay-diagnostics initially fail with the
same transcript on main, then pass the candidate's corpus retry. control-hard-loss
passes both candidate standalone repeats and its corpus retry; main passes one
of three runs and fails the other two with differing symptoms. Its first
candidate and main transcripts are not identical. display-menu-resize-lifecycle
passes two candidate standalone repeats, main's baseline, and its corpus retry.
The causes of those two initial failures remain unestablished. Their assertions
and timing were not edited. corpus-classification.json records each observation.

status-background-jobs fails the first pass and retry. Its initial raw transcript
was overwritten by the harness retry before it was copied; delta.txt retains
the complete printed failure diff, also extracted as the first-failure excerpt.
The final raw transcript is retained. The other seven initial raw transcripts
were copied before retries. This limitation is explicit; no failure is hidden.

The status-background-jobs follow-up runs fail 5/5 on the candidate (first corpus,
retry, three standalone repeats), compared with 1/4 on clean main. Main repeat 3
has exactly the candidate corpus retry transcript apart from the source path:
DATE[] is empty at the one-second sample. Thus this failure exists on main.
These small unequal samples do not establish equal failure rates or exclude
differing timing sensitivity. No scheduling source cause is established. The
status-job implementation and fixture are unchanged; this pass changes neither.
Copies for an executable-placement experiment were prepared but not run after
main reproduced the failure. All four final corpus failures have a corresponding
clean-main reproduction; the final corpus exit is still 1, not a green run.

The source change is cf8e0b8b, two files only. The remaining commit records this
evidence and the TUI-015 ledger. No code comments or attribution trailers were
added. origin/main remained f9c52359 at the final fetch and merge-tree reports a
clean merge. Wire 104 remains inherited and unreleased; no payload or version
change is introduced by this follow-up.

GitHub push protection rejected the first evidence push because census-hooks
captured an OAuth credential through show-environment. Both occurrences are
replaced with <REDACTED_GITHUB_CREDENTIAL>; comparison outcomes are unchanged.
The unpushed evidence commit was amended and protection was not bypassed.
A subsequent key-name audit also redacted the captured messaging-token
value. All other raw measurements remain as recorded.
