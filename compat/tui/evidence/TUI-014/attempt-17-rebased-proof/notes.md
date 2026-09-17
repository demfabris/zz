# Rebased TUI-014 fix-pass measurements

Base: origin/main `38c50df4c4d51401752c49b72bb689b09e33427d`, rebased on
2026-09-17. Runtime revision: `0b72e3b242381028396272e3bea2915d39b1768c`.
Fixture revision: `76662d8d`; verifier timeout support: `7adc5e26`.
All attached and corpus measurements use the frozen `zz-customize-rebased`
binary identified in `binary.txt`, an empty home and C.UTF-8. Cargo commands
use `/tmp/zz-cargo.sh`, two jobs and the shared slots. No environment dumps
are collected. Census hooks query only their named test variable.

The final normal roster attempts are client-3, client-7 and client-8, with
client-self for the full sabotage pass. Client-6 is retained at exit 1: only
the 24-with-seconds and 12-with-seconds clock captures differ. Its other 330
assertions pass. The bounded verifier independently passes all 332 assertions;
client-8 is an additional standalone repeat, without another fixture edit. The bounded verifier writes to
verifier-bounded. Each completed command has a separate .exit file; results.json
collects the final results. Failed and interrupted attempts remain available.

Client-1 ran the earlier exact-capture-only fixture and failed two style-tail
comparisons after the preceding lifetime scenes. A focused history probe
reproduces the difference, while a fresh scene passes. Raw outer tmux capture
-R dumps show zero differences across all 1920 visible cells after excluding
internal cell flags. The history row is 41/80 allocated on the pin side and
20/20 on zz. Both have default, unstyled blanks beyond column 19; allocation
and CLEARED flags alter capture -e resets. The four .grid files and
`tail-grid-comparison.json` preserve that measurement.

The history case now compares every decoded cell (glyph, width, attributes,
foreground/background/underline colour and link), geometry, cursor, CLI
channels and state. It excludes allocation headers and internal flags only.
The two required exact capture-tail cases start fresh scenes, as do their
controls. CLI capture-pane output remains byte exact. A one-sided blank-cell
background sabotage proves that the decoded comparison catches actual style
differences. Both required fresh controls fail on rejected c0e9bd83; the
candidate passes the style controls and both sabotage types.

Old client-2 and client-5 observed incomplete pin status redraws under load.
Only this lane's process groups 2398694 and 2975939 were stopped (143), with
partial logs retained. The outdated client-4 process group 2975940 was also
stopped (143). The older verifier child 2982794 was terminated so its parent
could retain partial output. The normal equality path now takes up to 20
additional screen/cursor samples with 50 ms sleeps when the first pair differs,
except for ticking clocks. CLI output and captured state are never retried.
Persistent style-tail and blank-background sabotages exercise this same path.
The previous verifier hit its 1800-second limit and retained only its exception
log, because the old tool did not save captured output on timeout. The final invocation requests
7200 seconds. Timeout output retention has a focused mocked test.

The complete command delta selection is 204 rows. The initial 202 are split
into four disjoint batches; copy-mode-bindings and list-keys-padding add two
passing rows. The collector retains first completed logs before retry overwrite.
Final corpus logs and separate baseline measurements are also retained. The
nine-row rejected-tip and main baselines are in attempt-16's archive, with a
summary in baseline-results.json. Corpus stalls and the three explicitly
terminated run-shell invocations are documented in corpus-stalls/notes.txt.
They remain nonzero measurements; no stalled command is treated as a pass.
The environment-loop fixture's pin-side global/session cardinality assertion
fails under the candidate's minimal environment. It passed the main run; a
candidate repeat with matching explicit harness settings still failed. A subsequent candidate run with USER and LOGNAME explicitly present passes;
its raw log is retained under corpus-identity. The first failures remain
recorded as a test-environment cardinality assumption.

The full eight-crate test run exits 101 on three integration targets. The CLI
menu-disconnect test passes alone. Simulator seed 5eed fails in the full run,
the first solo run and the rebuilt candidate run, then passes unchanged. It
passes three times on main and once on rebased pre-fix a70167e3. Its cause is
unresolved, not assigned to main. The agent slow-client soak fails on candidate
and main with the same 1501 versus 1500 updates. Clippy for all eight crates,
all targets/features, passes with -D warnings. All failed tests remain in the
archive or individual logs.

The first attached invocation omitted TMUX_BIN and returned usage exit 2.
The corrected run failed the 1.8-second alert checkpoint. The main baseline
failed a later client-context assertion. attached-final is the separate
candidate repeat. It fails the same common-facts assertion as main: client_user
is empty because the minimal environment omitted USER. attached-identity adds
only USER and LOGNAME to the explicit environment and passes the full fixture.
The earlier alert failure is retained separately.

The identity-aware source-replay repeat still fails at step 60 with
pty-rc:0 and request-rc:124. This exact output also appears in main
baseline attempt-16/delta-main.txt, which later passes on retry; the
rejected-tip baseline fails with the same timeout. This supplies inherited
measurement for the confirmation timeout, while retaining the earlier
candidate broken-PTY variant separately.

TUI-014 remains review. The three copy/clock stacking records are
`decided:TUI-014` under fabrico's 2026-09-17 ruling. The two switch window-style
records are asserted. The shared interactive-refresh gap drops from 13 to 8;
its remaining records concern refresh and the previously reviewed headless
client-tree probe. The socket-ACL record remains separately owned. They do not
hide any of the five mode residuals addressed by this fix pass.

The completed full self-check catches every sabotage, including array erasure,
C-c closing customize mode, both exact switch style tails and both blank-cell
background differences. Chooser, copy-mode, screen-diff and overlay fixtures
and their self-checks pass. The bounded TUI-014 verifier exits 0.

Corpus totals: 204 selected and executed, zero unrun; 183 clean initially,
13 recovered, six inherited baseline divergences and two documented known
rows. No persistent fix-caused corpus divergence was identified. The 414 raw
first/final/identity/main-extra logs are in corpus-logs.tar.gz, each byte count
and SHA-256 recorded in corpus-manifest.json and verified before removal of
the loose copy. corpus-results.json lists every row and classification.

Final standalone roster results: client-3, client-7 and client-8 all exit 0,
each with 332 asserted comparisons identical, 30 recorded, ordinary TUI-014=0,
decided:TUI-014=3 and unattributed=0. The filled review proof and generated
report pass both trackers and the structural claim validator.
