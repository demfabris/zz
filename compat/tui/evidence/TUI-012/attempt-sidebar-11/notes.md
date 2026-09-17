# TUI-012: wait for sidebar refocus before withdrawing it

The lane started at `d1694e65da7552e8ebd53da2ce596ee8ffa748e9` on alienware on
2026-09-16. The branch is `campaign/tui-sidebar`. No production Rust code, desktop or web
behavior, wire field, acceptance clause, or obligation status changes in this lane.

## The before/after control disproves the proposed culprit

I built current main and `936e15ee33b3c51d692b52381a7c757f6ab89c31`, the parent of
`bb4a4288c0f9fbb195a899d460fb84d88b1079f5`, through `/tmp/zz-cargo.sh`. I checked out the parent
in this worktree and returned to the lane branch after measuring it. The preserved binaries
live under `target/sidebar-bisect/{main,parent}/zz`; `environment.txt` records their SHA-256 hashes.

The unchanged full fixture fails on both binaries with exit 2 at `the sidebar withdrawn`.
Both runs pass the first withdrawal by `q`. Both fail after Escape has returned the keyboard
to the pane and F8 requests focus again. Their pane captures contain `$ q`. A second unchanged
main run restricted to the sidebar reproduces the same failure.

Only four documentation commits separate the parent from the last verified revision,
`10779511`. `verified-to-parent-stat.txt` records that comparison, and
`verified-to-parent-relevant.diff` is empty. There is no measured product regression introduced
by the palette commit. The defective synchronization entered the fixture in
`943464f5d` (`Prove the superset commands beside the tmux surfaces`), at the existing
`run_sidebar_verbs` refocus wait. Prior passing runs did not make that wait a focus barrier.

## The exact path

Escape resolves to `ChromeAction::SidebarCancel` in the TUI sidebar table.
`handle_sidebar_key` clears `model.sidebar.focused` but leaves the tree visible. F8 then
travels as `InputMessage::Key` to the daemon. The daemon executes the bound `focus-sidebar`
command and returns `EventPayload::FocusSidebar`; the TUI handles the corresponding core event
by calling `model.focus_sidebar()`.

The fixture previously waited for `sidebar_up` after F8. The marker already existed from the
unfocused tree, so the wait could finish before the focus event arrived. When `q` arrived in
that interval, `handle_key` skipped the sidebar branch and forwarded it to the pane. The later
focus event left the sidebar visible, with no later withdrawal key to close it. Waiting another
12 seconds could not fix that ordering.

`Renderer::place_active_cursor` hides the cursor while the sidebar owns input. In this fixture,
the controlled shell has a visible cursor after Escape. The corrected wait requires both the
sidebar marker and the hidden cursor before it sends `q`. It retains the original withdrawal
check, the 240 polls at 50 ms, the pane-key ownership checks, and the geometry checks.

The isolated focus-aware probe passes all eight existing sidebar cases on both unchanged
binaries. Separate xtrace output preserves stderr comparisons. `timings.txt` records main at
76.160 ms from refocus wait to `q`, then 9.793 ms to the withdrawal assertion; the parent takes
24.438 ms and 22.456 ms respectively. These are measured intervals from two runs, not latency
bounds. No timeout was raised.

## Regression coverage

The fixture adds the `sidebar refocused` assertion. Its self-check rejects an absent sidebar
and a visible but unfocused sidebar using the same focus predicate and bounded polling as the
real case. It also drives a positive focus, Escape, refocus, and `q` withdrawal control.

`sidebar_withdrawal_stays_specific_to_the_tui_profile` asserts that TUI `q` withdraws the
sidebar, desktop and Apple desktop `q` retain their cancel action, Escape cancels in all three
profiles, and ordinary `q` has no UI or terminal chrome binding. The renderer test checks that
refocusing an already visible sidebar hides the cursor while keeping its 91-column pane area,
and that hiding it restores the cursor and all 120 columns. Both new tests and five neighboring
sidebar tests passed the filtered run.

No case changes from recorded to asserted. TUI-012 already asserted these behaviors; this lane
repairs the proof's readiness condition. No screen string changes, and the scenario search finds
no `focus-sidebar` corpus row. The required delta still selects and runs all 144 smoke scenarios.

The first three frozen refocus-only runs produced two full passes (196 assertions each) and one
failure at `around-sidebar reattached-window-columns`: zz still reported 91x29 at the immediate
query, then passed the following whole-screen comparison. The final fixture also waits, under
the same 240-poll bound, for the required 120x29 size after reattach. Its self-check asks for 1x1
and requires that assertion to fail. The old runs remain under `superset-focus-only-run-*`; they
are not the final three-run proof.

The full `zz` suite exposed two pre-existing desktop test failures; both reproduced alone.
`settings_view_is_lazy_and_retained` called a render method outside GPUI's draw phase. The lane
changes only that test to draw the settings entity with `VisualTestContext::draw`, preserving
its preview, route, and entity-retention assertions. Its focused rerun passes. The palette test
`tab_accepts_completion_without_leaving_the_palette` still sees `new-w` where it expects
`new-window `. A trial initial draw did not change the failure and was removed. This lane
does not change the palette implementation or relax that expectation.

## Execution details and retained failures

Every Cargo invocation uses `/tmp/zz-cargo.sh`. The two small Cargo adapters preserve its scopes
and shared locks while moving its appended `--jobs` before a `--` separator, or removing that
unsupported option for `fmt`. `compat/check.sh` uses the entry adapter through PATH. Tests that
can start daemons use the requested empty HOME and XDG config root, with explicit CARGO_HOME and
RUSTUP_HOME to retain the installed toolchain.

One parent build attempt waited on occupied slot 0 while slot 1 was idle. I cancelled only that
attempt's childless waiting flock and retried through the unchanged wrapper. Both attempts remain
recorded. The first copied-main fixture invocation lacked adjacent `libcef.so` and failed before
starting a daemon. A link to this worktree's library fixed the loader setup; that failure and its
diagnostics remain retained.

The first timing attempt sent xtrace to stderr and therefore failed the exact CLI refusal
comparison. The clean timing runs send xtrace to a separate descriptor. The provisional full
corrected run ended without its final summary after I removed a stale fixture comment while the
shell was active. Its exit 0 is not a passing proof. It remains as
`superset-provisional-incomplete.txt`; three fresh runs use the frozen fixture and record its hash.

`attached-client.sh` has no self-check option. Its full run is the available check; the superset,
client-command, and chooser fixtures each have a separate self-check run. No independent review,
macOS run, main push, PR, or board operation belongs to this lane.


## Resume after the reboot

The lane rebased from `d1694e65` onto `eef2df94` without conflicts. The CLI now lives
in `zz-cli`, whose headless binary target is `zz_cli`. The resumed build uses
`/tmp/zz-cargo.sh build -p zz-cli --bin zz_cli`; it does not reuse a pre-split main binary.
The earlier palette-parent binary remains available for controls. Interrupted files from
before the reboot are retained; an absent summary or exit status is not a passing proof.

Five runs of the original around-sidebar group against the palette parent produced four
passes followed by the same reattach race seen on old main: run 5 queried `91x29`, then
passed the unchanged full-screen comparison. See `resume/parent-geometry-5.txt` and its
captures. This independently justifies the geometry wait on both sides of the suspected
commit. It does not establish a new runtime regression.

### Fixture hunk accounting

- The focus helpers add an assertion of rendered keyboard ownership. The real refocus
  checkpoint now waits for that ownership before sending `q`; the unchanged withdrawal
  assertion still checks the marker disappears within the original bound. Both unchanged
  binaries failed without this wait and passed the focused probe with it.
- The removed comment incorrectly said pane input was the only focus observable. The
  renderer also exposes cursor visibility, covered by the new renderer test. No assertion
  or delay was removed with the comment.
- The window-size helpers retain the exact `120x29` requirement and allow its asynchronous
  update to arrive under the existing polling bound. Old main and the palette parent both
  exhibited the immediate `91x29` result followed by the correct full canvas.
- Self-check additions reject absent focus, visible-but-unfocused chrome, and geometry that
  never reaches its expected size. Positive controls exercise Escape, refocus and `q`.

The current shared Cargo wrapper now handles `--` and `fmt` itself. The resume entry adapter
only routes Cargo invocations made by `compat/check.sh` through that wrapper; the earlier
adapters remain as evidence of the earlier commands and are not used for resumed checks.


The full unchanged fixture at the rebased headless build passed: `all 196 asserted cases
hold, 0 recorded not asserted`, exit 0 (`resume/main-original.txt`). The original script's
hash matches `git show origin/main:compat/tui-superset.sh`. This is evidence against claiming
that the old main failure still exists deterministically after the split. No production
breaking commit has been established. The measured palette-parent failure rules out the
proposed commit as the introduction; the fixture's defective wait can be traced in history,
but that is not a successful runtime bisection and is not presented as one.

All pre-reboot artifacts are in `before-reboot.tar.gz`, with per-file SHA-256 values in
`before-reboot.manifest.txt`. Every archived file was compared byte-for-byte with its source
before the loose copies were removed. Earlier paths in this note are relative to that archive;
`resume/` paths refer to the later measurements.

### Fixed binary for the final fixture runs

The combined test build replaced `target/debug/zz_cli` after the unchanged baseline and the
first corrected run had completed. Its hash changed from `09ae179d82da0015cb45e332dd00e2f881acbb87e9c4c22b030c0417be8bf2cd`
to `24a31d3925af0e4cb63aa6b405fa3100167ed897d12288e9c2f62f16febba181`.
I had already copied the first binary to `target/sidebar-bisect/rebased/zz_cli` and verified
its hash. I stopped the affected fixture process groups and restarted all final fixtures
against that copy. `resume/interruption.txt` identifies the stopped groups. The partial
outputs remain retained and are not counted as final proofs.

The final delta selection contains 144 smoke rows and no additional `focus-sidebar` row.
Its serial partial run was retained, then the exact selection was split into four disjoint
36-row lists. Each uses the existing `compat/run.sh` with explicit rows, `--delta origin/main`,
`--commands focus-sidebar`, the fixed binary, and a 1 GiB memory / 512 MiB swap scope.
The harness still retries a failing row alone within its batch; no scenario or assertion is
removed. `frozen/commands.txt` records the commands and the four list files preserve the selection.

The resumed full Cargo run completed with exit 101. The library results are 558 passed,
1 failed, 1 ignored for `zz`; 106 passed for `zz-cli`; 102 passed for `zz-client`; and 208
passed for `zz-tui`. The retained-settings test and both added sidebar tests passed. The
palette completion test still failed. The CLI integration target had 127 passes and six
failures, including the explicitly interrupted wait-exit test; the client simulator had
one pass and one failure. Individual reruns are recorded separately. These failures are
not counted as successful validation of those targets.

### Completed sidebar proof

The three final runs each report `all 197 asserted cases hold, 0 recorded not asserted`,
exit 0. The self-check reports `self-check complete: every fault was reported by the channel
that owns it`, exit 0; this includes absent focus, drawn-but-unfocused chrome, and geometry
that never reaches its expected size. `frozen/hash-check.txt` confirms the fixture and binary
hashes match the pre-run hashes for all three runs. These add one asserted readiness check,
`sidebar refocused`, and flip no recorded case.

Completed resumed and final screen captures are in `captures.tar.gz` with per-file hashes in
`captures.manifest.txt`. Every file was compared byte-for-byte before its loose copy was removed.
Their paths within the archive retain the `resume/` and `frozen/` prefixes.

### Isolated test results

The queued isolated Cargo loop was cancelled before any test started. The reruns instead use
exact filters on the test executables produced by the completed full Cargo run, with a scrubbed
HOME, a memory scope and a runtime timeout. `isolated/commands.txt` and `artifacts.sha256.txt`
record those commands and artifacts; no uncapped compilation is involved.

The later-alias attach test and refresh subscription test passed alone. Disconnect cancellation,
control-client sizing, and the return-value matrix still failed alone; the matrix finished with
its own failure after 94.26 seconds. The wait-exit test timed out at 180 seconds, exit 124, and its
remaining daemon was removed by stopping only the scope recorded in `wait-exit-cgroup.txt`.
The client convergence simulator and palette completion test also failed alone. These residuals
remain open; the lane changes no implementation in any of those paths.

### Rebase bookkeeping and the bare-client corpus probe

The first resumed `compat/check.sh` stopped at two mux tests. Commit `94f7bcf4` had added
`new-agent-session` and extended `agent-send` and `tools`, but left the native-name registry
and the exact `list-commands` test strings stale. The lane adds that existing name under
`commands.native-superset` (owner `protocol`), updates both expected usage strings and
regenerates `knowledge/tmux/gaps.md` with `compat/tmux-tracker.py write-report`. No acceptance
clause or command implementation changes. These add `zz-mux`, the registry and its generated
report to the footprint and validation scope.

The default-client corpus probe also still passed `--bootstrap-launcher-client`, which
`8d4438c0` removed when introducing the headless CLI's launcher entry point. A direct probe
of the new CLI with that flag prints usage and exits 1. Removing only that obsolete flag
preserves all nine assertions and is required by the production entry-point change on main;
it is not a weakened output comparison. Before removal six checks fail; afterwards only
the two output checks fail. `frozen/default-bare-bytes.txt` shows why: a bare CLI on a PTY
writes a `process_start` diagnostic line with `role=app` before `custom`, while the expected
output is just `custom`. The lane does not suppress or normalize that diagnostic. This
remaining launch/output defect belongs to TUI-001; it is not a passing corpus result.

### Neighbor fixtures and final corpus

The fixed-binary client-command fixture passes 90 asserted comparisons with 25 existing recorded
cases, exit 0. Its attribution summary ends `owners TUI-014=6 TUI-015=4 TUI-017=6
 decided:TUI-016=1 gap:clients.interactive-refresh=8 unattributed=0`; its self-check passes.
TUI-012 owns none of those records. The attached-client fixture passes, exit 0.

The first chooser run fails two of 78 comparisons (`window-tree-searched` and `find-window-tree`),
exit 1. The former captures the search prompt before dismissal; the latter captures bash where
the pin captures sh. The unchanged retry passes all 78, with zero recorded, exit 0; the chooser
self-check passes. The initial failure and both sets of captures remain available. The lane does
not claim to have repaired the intermittent chooser observations.

The delta list for `focus-sidebar,agent-send,tools,new-agent-session` selects the same 144 rows.
All 144 ran. The four 36-row batches exit 1, 1, 0 and 1. Eight rows remained red after their batch
retries; those eight ran again serially after all batches completed. Copy-mode refresh and
split-window wait then passed, leaving 138 passing rows and six failing rows. The serial command
exits 1. `corpus-results.json` combines the complete batch coverage with the final serial results.
Remaining failures are `smoke/cli-chain-parse-abort`, `smoke/default-client-command`,
`smoke/jobs-command-environment`, `smoke/plugin-runtime-resurrect-restore`,
`smoke/source-replay-diagnostics`, and `smoke/status-background-jobs`. These retain their raw
output differences; no normalization or registration was added to hide them. The resurrect row
fails on the tmux side. The bare-client row's remaining diagnostic output is described above.

The added mux scope passes its focused gap tests (4), full package tests (534 library plus 105
integration), and clippy with all targets and features and `-D warnings`. Clippy also passes for
`zz-client`, `zz-tui`, `zz-cli` and `zz`, with all targets and features and `-D warnings`. The final
`/tmp/zz-cargo.sh fmt --all -- --check` and `python3 compat/tui/tracker.py check` pass. Full package
test residuals described above still prevent an all-green lane report.

### Inherited wire version and final gate queue

The original batch says wire 103 is unreleased. The rebased source instead already has
`PROTOCOL_VERSION = 104`, introduced on main by `81c971f1` (`Split tmux and native usage exit
codes`). `compat/wire-version.py` reports 104 unreleased after v0.10.0 shipped 103. The lane
changes no protocol file or version; `resume/wire-baseline.txt` records the blame and empty diff.
The report therefore states the inherited 104 rather than repeating the stale batch number.

One final gate attempt was stopped while its own childless flock waited on slot 0 behind a
foreign full suite and slot 1 was idle. Its exit 143 remains in `compat-check-final.txt`.
The next gate uses `resume/cargo-entry-balanced/cargo`, which still invokes `/tmp/zz-cargo.sh`
for every Cargo command. If the wrapper chooses an occupied slot while the other is idle, the
adapter stops only its own childless flock, rechecks that no Cargo child has started, and
retries the unchanged wrapper. Running compiles, foreign processes, both shared locks and
the wrapper's memory limits are preserved. Every such queue retry is logged separately.

The completed `compat/check.sh` exits 0. It passes all static checks, 534 mux library tests,
and each of the three required daemon manifest tests. Its full output is in `checks.txt`
and the raw `resume/compat-check-completed.txt`. The queue adapter's log records every
completed Cargo command and any cancelled queue attempt; all six commands complete with
exit 0. No running compile or test was cancelled by that adapter.

### Evidence layout and disposition

`environment.txt`, `fixture-output.txt`, `probes.txt`, `checks.txt` and `corpus-output.txt`
contain the readable measurements; `corpus-results.json` accounts for every selected row.
`measurements.tar.gz` preserves all remaining raw files, including failed and interrupted
attempts, commands, captures, and the earlier `before-reboot.tar.gz` and `captures.tar.gz`
archives. `measurements.manifest.txt` records the SHA-256 of each directly archived file.
Each archive member was compared byte-for-byte against its source before loose raw copies
were removed. Paths named earlier in this note refer to members of this archive, with
pre-reboot paths inside its nested `before-reboot.tar.gz`.

The lane is partial: the sidebar proof now passes three times and all required fixture
self-checks pass, but no production breaking commit has been established. The palette-parent
control and the clean rebased baseline contradict the supplied regression hypothesis.
The six corpus failures and isolated non-sidebar test failures remain unresolved. TUI-012
stays verified in the untouched campaign ledger. No recorded case was promoted, no bound was
raised, and no production GPUI or web behavior was changed.
