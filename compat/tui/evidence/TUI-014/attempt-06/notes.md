# Cycle 11 modes proof pass, 2026-09-16

The lane started at unproven `4d578fcf`, fetched origin, and rebased its seventeen
commits onto `be5709df`. The rebased production tip is `8b95fa47`. Catalog assertion
counts needed the union of main's usage overrides and this lane's commands. The
lane regenerated the gap report at each conflicting replay. No production change
was necessary to resolve the rebase conflicts. Runtime proofs later exposed two
additional defects, described below.

The batch authorizes the commits and push to `campaign/tui-modes-12`. TUI-014 stays
active. Fabrico assigned clause 2's last two commands, `customize-mode` and
`suspend-client`, to the sibling `customize` lane; their records stay attributed to
TUI-014 here. This pass does not claim their implementation.

## Rejected review, checked defect by defect

`defect-probe.sh` reuses the committed outer-tmux setup and comparison functions,
but supplies its own command sequence and explicit expected stack and template
values. Both inner servers and the outer decoder have isolated homes and sockets.
`defect-probe.txt` reports 18 asserted comparisons, zero failures, and two measured
records. `probe-captures/` retains each decoded screen, cursor tuple and state.

1. Injected `x` ends the clock, leaves mode `0/`, and produces equal shell contents.
   `copy-mode -q` ends the clock on both servers. These compare the three CLI
   channels as well as the screen, cursor and resulting state.
2. Clock then switch reports `2/switch-mode`. Raw Escape restores `1/clock-mode`
   and its screen. Injected `x` restores equal terminal contents.
3. `-F 'REVIEW-#{session_name}'` matches across alpha, cli and zulu. Enter executes
   the positional `set-option -g @review-command yes` template on both servers;
   both print `yes`. The `-k` lifecycle remains unbuilt: zz rejects the flag, and
   the pin kills the source pane on Escape. The probe measures that difference
   with two panes. The committed roster retains attributed records for `-k`, its
   effect, and `-Z`; these are not silent successes or parity assertions.
4. `server-access '#{?#{==:1,1},nobody,root}'` exits zero with empty stdout and
   stderr on both servers. The probe checks those values explicitly.
5. Equal window names across alpha, cli and zulu match with the plain
   `#{session_name}:#{window_name}` format. The default formatted window rows
   retain their separate measured style-tail difference; no all-cells-match
   claim covers that record.
6. `footprint.txt` contains the complete three-dot list, including production
   constructors, mechanical `mode: None` consumers, corpus helpers and generated
   reports. The lane's boundary follows the obligation, as this batch permits.
7. The rebased Rust diff adds zero comment lines. `static-checks.txt` records the
   scan. This pass adds no Rust comments.
8. The named selection-style unit test passes with `noattr` in its input.
   `noattr-mutation.py` removes only the selection's `base_cell` conversion, runs
   the exact test, and restores the file in a `finally` block. The test fails with
   exit 101 and “the row lost its dim runs.” Its output and the restored-source
   confirmation are retained in `noattr-mutation.txt`.

## Fixture changes

The self-check now sabotages the custom switch format, the Enter command's
result, tied-window labels, and an expanded identity. The existing self-check
already sabotages injected-key leakage, generic cancellation and missing stack
depth. Each sabotage checks the channel that the defect changes.

The seconds-face experiment first captured as soon as both screens changed after
a second boundary. `seconds-probe.txt` retains its failure: the 24-hour face
matched, but the 12-hour captures contained adjacent seconds. A changed screen
alone did not prove that both redraws had reached the new second. The revised
capture waits 200 ms after the boundary, requires both faces to differ from their
previous captures, and rejects a pair whose screen/cursor reads cross the next
boundary. It retries sampling windows, never retries based on screen equality,
and fails if eight windows cannot supply a pair. `seconds-probe-settled.txt`
reports four passing assertions for the two faces and their teardown. The roster
now includes both faces and a one-sided colour sabotage for each.

## Validation history

`run-fixtures.py` records each command, output, exit status and elapsed time. The
first self-check before the seconds additions passed; its output remains in
`client-commands-self-check.txt`. The seconds self-check also passed. Final totals, package tests, clippy, campaign checks and the full corpus result
are recorded in the closing sections below.

The delta includes the review's command list plus `send-keys`, `send-prefix` and
`copy-mode`, whose input routing changed. `corpus-selection.txt` lists 198 rows.
The runner uses `ZZ_COMPAT_ZZ` and the worktree's pinned tmux and plugin corpus;
it does not build zz. Every Cargo invocation uses `/tmp/zz-cargo.sh`. Full crate
tests use scrubbed HOME/XDG_CONFIG_HOME and `RUST_TEST_THREADS=3`.

The first full package-test request was cancelled while it was still waiting on
`flock` (exit 143, no tests ran). Its replacement preserves the scrubbed test home
but pins `CARGO_HOME` and `RUSTUP_HOME` to the installed toolchain. The first request
and replacement output remain separate.

`capped-bin/cargo` is a PATH adapter for `compat/check.sh`, which invokes Cargo
internally. It enters `/tmp/zz-cargo.sh` before running real Cargo. Inside that
scope it moves the wrapper's trailing `--jobs N` before a `--` argument separator;
for `fmt` it removes that unsupported option. Nested Cargo invocations inherit the
same scope. This preserves the shared two-slot lock and memory cap. Direct fmt
and Clippy calls set `ZZ_MODES_CARGO_IN_SCOPE=1` only when invoking the shared
wrapper themselves. The shared wrapper was not edited.

## Defects found by the full roster

The first full run failed 37 of 139 assertions. Three compared exit statuses:
`refresh-missing-argument`, `client-tree-unknown-flag`, and `client-tree-usage`.
Main's new CLI preflight classified those syntax errors as usage status 2, while
the pin exits 1. The CLI now identifies an error from a known tmux command's own
shared parser and keeps status 1. Native usage and the declared `--json` extension
keep status 2. The footprint therefore also includes `crates/zz/src/lib.rs`,
`crates/zz/tests/cli_binary.rs`, and `knowledge/tmux/commands.md`.

The other primary failures were `clock-injected-backtab` and
`clock-injected-keypad`. `key_token` still classified BTab and KPEnter as literal
text, so `inject_pane_mode_keys` split them into characters. The clock consumed B
or K and the shell received Tab or PEnter. Later teardown comparisons then kept
finding that same leaked shell text. The mux now consults the existing canonical
named-key table before falling back to literals. A filtered mux test checks the
named tokens and proves that explicit `-l` still preserves literal spelling.

The initial seven-package build/test request was stopped when these defects
required code changes (exit 143, before the package suites ran). The lane resumed
behind the new filtered tests. No failed full-roster output was removed.

## Wider test failures and controlled environments

The first seven-package run reached the zz library: 660 passed, two failed and
one was ignored. The palette completion test failed alone too; changing action
capture did not fix it, and that production experiment was withdrawn. An explicit
draw did not fix it either. Diagnostic output proved the completion handler ran:
the unified palette ignored its `complete` flag for command rows and executed
new-window on Tab. The fix fills the command text when completing and preserves
Enter's activation path. The existing regression now also checks `finishing` is
false. The settings test called a view's render method outside GPUI's draw phase;
it now draws the settings entity through VisualTestContext before checking preview
lifetime. Its filtered test passes. The palette production change is desktop-only
and is absent from the frozen TUI proof binary; the final package build and tests
cover it. Final package results follow.

Two chooser runs with inherited SHELL=/bin/bash failed. The first had a
`filter-cleared` sh/bash row difference. The second reproduced that row and had
a `window-tree-searched` prompt/cursor difference. The fixture itself documents
the foreground-pid cache defect: exec preserves the pid, so a cached bash name
may survive exec to sh. No output mask was added. The next runner uses
SHELL=/bin/sh for both sides, matching the fixture's explicit inner shell. Its
outputs use the `-sh` suffix. A pass in that controlled environment does not
prove the inherited-shell cache defect fixed. The original failures remain here.

The corpus's `lane2-store` and `show-options-hooks` rows expose the lock-command
default: zz stores `lock -np`, while this pinned tmux was configured with vlock.
This is related to owner client/TUI-015 and gap `options.lock-program`, whose
native-execution decision does not itself certify the default's output bytes.
The lane retains the mismatches as residuals rather than claiming them waived.

## Control wait-exit regression exposed by the full desktop suite

The desktop library passed 662 tests with one existing ignore. Its integration
suite passed 130 of 131 tests, but wait-exit never ended after the second blank
line. The lane stopped only that test's owned child after over 240 seconds so
the suite could report failure. A filtered run reproduced the hang. Both outputs
and supervision are retained. The control input thread had already ended while
the main thread waited on a futex; debugger attachment was unavailable.

`finish_control_return` drains protocol events before waiting for the exit
acknowledgement. `drain_before_exit` discarded stdin events, including the second
blank line and EOF; the later wait could therefore block forever. It now queues
those events for `wait_for_exit_input`, and a focused unit test checks that both
survive. This is another declared change in crates/zz/src/control_mode.rs, which
was already in the inherited footprint for a snapshot helper. It is unrelated
to pane mode presentation. The frozen modes binary is mapped to f884b266 in
binary-revisions.txt, and later validation will name the rebuilt control fix.

## Additional corpus residuals

The full selection also measures the rebased CLI policy outside the mode cases:
`smoke/alias-group-forgery` reports status 2 on zz versus status 1 on the pin for
`__zz-command-alias-group`; `smoke/cli-chain-parse-abort` reports broken versus
clean:6. The mode fix deliberately preserves main's unknown-verb/native-usage
policy, so neither row is claimed fixed here. The CLI/alias owner (TUI-018) needs
to reconcile that policy and cold-chain connection/parse ordering with the corpus.
These are failures, not accepted parity or unrun rows. First-pass full logs are
retained separately from the harness retries.

Other first-pass failures include a pin-side resurrect pane-title assertion and
a zz-side status-background-job timer assertion. Their exact output is retained;
final retry results determine whether either remains reproducible. The registered
known-terminal-runtime and known-pane-scrollbar-columns rows are judged by the
harness against their documented divergence counts, not described as zero-diff.

## Completed delta corpus

All 198 selected rows ran, covering 2,331 outer scenario steps. There are no
unrun rows. 187 rows have zero divergences. The two known rows retain their exact
registered counts: terminal runtime has six format divergences; pane-scrollbar
columns has two geometry and one output divergence. Nine rows fail after retry:

- lane2-store and show-options-hooks: lock-command defaults differ (client /
  TUI-015, related to options.lock-program; default bytes are not claimed waived).
- smoke/alias-group-forgery and smoke/cli-chain-parse-abort: CLI/alias policy and
  cold-chain ordering (TUI-018 / CLI).
- smoke/command-flag-errors: zz never sets COMMAND_FLAG_ERRORS=clean:535, while
  the pin does; the individual failed diagnostic probes are not isolated here
  (CLI catalog/diagnostics owner; no claim that the flag corpus is green).
- smoke/source-replay-diagnostics: the accepted nested control confirmation
  request times out at status 124; pin exits 1 with its expected event sequence
  (TUI-018 / source replay).
- smoke/plugin-runtime-resurrect-restore: pin-side restored pane titles become
  shell prompts instead of ALPHA/BETA (plugin corpus/environment owner).
- smoke/plugin-runtime-continuum: the saved first pane's process name is sh on
  zz and sleep on the pin (pane runtime facts / plugin corpus owner).
- smoke/status-background-jobs: zz's drawn DATE timer assertion fails (status
  renderer / TUI-004).

The selector's 198 names exactly match attempt-04/fix-delta-list.txt. The lane ran
two disjoint 99-row groups with the built binary and no Cargo calls. Both exit 1.
Their shared generated summary tail is not used to count coverage; the per-row
logs, group outputs and explicit selection are retained. corpus-summary.json
computes the result from every selected row's final SUMMARY, and
corpus-final-details.txt retains those complete logs. The full corpus binary maps
to f884b266, before the independent desktop palette and control wait-exit fixes.
This is a complete run with failures, not a certified clean final-tip corpus.

## Final modes checks

Three client-command runs on the frozen modes binary each passed 139 assertions,
retaining 30 records with TUI-014=2 and unattributed=0. The seconds self-check
caught every sabotage. With SHELL=/bin/sh, choosers passed 78/0 plus self-check;
copy mode passed 147/0 plus self-check; screen diff passed 147 asserted with six
existing records; attached-client compatibility passed. The inherited-shell
chooser failures remain a limitation, as described above.

The first live verifier on the rebuilt final binary reported one failed assertion
out of 139 and a passing chooser run. That verifier discarded the captured detail
and retained only its final tally, so the lane cannot identify that failure's
case. verify-claims.py now has an optional --output-dir which retains stdout,
stderr, command and exit status for each fixture. The rerun's client fixture
passes all 139 assertions, and its complete output is retained in verifier-final/.
The final verifier outcome is recorded below after the chooser finishes.

## Close-out

The final binary (production revision 5036e22f) passes the retained-output live
verifier: 139 asserted / 30 recorded in client commands, including TUI-014=2,
and 78 asserted / zero recorded in choosers. The verifier exits zero and accepts
TUI-014's active status. Its optional output capture is committed separately.

All seven requested package suites pass: six-package-tests.txt covers protocol,
mux, daemon, TUI, client and FFI, and zz-package-tests-control-fixed.txt covers
the desktop package (663 library tests plus 131 CLI integration tests, with one
existing ignored library test). The full seven-crate all-target/all-feature
Clippy run passed, and clippy-control-final.txt rechecks the final changed desktop
crate. fmt-control-final.txt and compat-check-final.txt both exit zero. Every
Cargo invocation used the capped wrapper; no requested check bypassed it.

The batch delivery is PARTIAL because the full corpus retains nine failures and
the inherited-shell chooser cache mismatch remains reproducible. TUI-014 stays
active as requested, with customize-mode and suspend-client held for the sibling
lane. The four recorded-to-asserted flips relative to main are clock-mode-open,
switch-mode, server-access-bare and server-access-user. Seconds faces are new
assertions, not falsely counted as flips of recorded cases. No claim here says
the whole alias-tmux contract is proved. Independent review is still required.

The later verifier on the frozen modes binary also exits zero (verify-claims-sh.txt): 139/30 client commands and 78/0 choosers. Both binary revisions therefore have passing live verifier runs. The earlier final-binary failure remains unclassified because the old verifier discarded its detail.

## Evidence redaction

GitHub push protection rejected the first evidence commit because the corpus environment dumps contained a GitHub OAuth token. The lane replaced every matching token in the changed evidence with [REDACTED_GITHUB_TOKEN] and amended the rejected, unpublished commit. This preserves commands, failures and result counts while removing credential values. The user was advised to rotate the token. The raw corpus detail archives are therefore credential-redacted, not byte-for-byte environment archives. No push-protection bypass was used.
