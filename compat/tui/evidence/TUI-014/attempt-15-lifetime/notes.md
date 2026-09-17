Lifecycle implementation and proof work for switch-mode -k and -Z, 2026-09-16.
The rebase checkpoint is bd0f76d7 on campaign/tui-customize-2.

The daemon keeps kill-on-exit on the switch entry and remembers an unzoomed -Z
entry separately from the current zoom state. Reopening the top entry does
nothing; promoting a suspended entry changes its kill flag and preserves its
original zoom lifetime. Escape removes the top entry. copy-mode -q and
clear-history drain the stack. Pane deletion uses the existing kill-pane path.

The attached fixture checks a visible source pane under a clock, stack draining,
a one-pane window, a split introduced during the mode, and an already-zoomed
window. Sabotages omit each lifecycle flag on one side and compare the resulting
pane list or zoom flag. The saved pre-fix binary is target/debug/zz-modes-before-lifetime.

The pin crashes on fresh -Z entry into a split pane, including with a client
attached. attempt-14-rebase/pin-attached-stack.txt retains that failed probe.
The deterministic comparison opens -Z in a single pane, splits and zooms during
the mode, then observes restoration on exit. Unit coverage exercises fresh
split-pane zoom on zz; no successful pin comparison is claimed for that entry.

The final verifier passes. TUI-014 remains at review: five mode cases are still parked in the interactive gap, and this lane does not verify it.

Pin hook probes: pin-kill-hook.txt uses send-keys with no attached client, which
the pin ignores for pane modes (window_pane_key requires a client). Its two panes
remain. pin-kill-hook-cancel.txt instead uses copy-mode -q; it leaves one pane
(index 0) and the after-kill-pane marker stays untouched. The implementation
preserves the client requirement for injected mode keys and skips the synthetic
after-kill-pane command hook when a mode kills its source.

The first focused candidate run passed 26 of 27 checks and every sabotage.
Its one failure was fixture setup: the detached test session had an automatic
window name (ENV= on the pin, 0 on zz). The fixture now names that window
lifetime-k explicitly. This change affects setup, not the compared command.

The pre-fix full --self-check exits 1 with two unmet equivalence expectations:
the -k and -Z acceptance controls. Its one-sided lifetime sabotages also detect
the missing behavior. focused-before.txt retains all cascading differences.
The first three cargo invocations were canceled while queued for a shared slot
(exit 143); their empty logs are retained. build-2, unit-2 and fmt-2 completed.

Stopped the auxiliary zoom probe after its two captured screen failures and a prolonged prefix wait; exit 143. The saved pre-fix auxiliary probe passed all six checks. The full candidate chooser fixture is running separately.

The first complete client roster passes: 170 asserted, 25 recorded, TUI-014
owns zero records, clients.interactive-refresh owns 13 (previously 16), and
unattributed=0. The complete chooser fixture passes all 78 comparisons,
including its six zoom cases. The auxiliary failure is not discarded.
Copy-mode passes 147 cases, screen-diff passes 147 assertions with six existing
records, and overlays passes 48 assertions with no records. Their self-checks
all pass. Two attached-client runs fail the same timed alert freeze checkpoint;
the saved pre-fix binary is being measured against that fixture too.

The seven-package build first failed on the rebased desktop tray test: its PaneSnapshot initializer lacked the inherited mode field. Added mode: None in that test only; the runtime proof binary is unchanged.

The --delta selection contains 199 rows. The serial run completed eight rows
before being stopped with exit 143 to reduce wall time. Its unfinished
command-item-format log is retained. The remaining 191 rows are partitioned
without overlap in delta-batch-{1,2,3,4}.rows and use compat/run.sh directly,
the same frozen binary and the already pinned plugin corpus. No Cargo command
runs inside these batches. The final summary must account for their union.

The desktop filtered build also rebuilt target/debug/zz_cli (hashes in binaries-after-tray-test.txt). The serial client rosters use that executable path and therefore span build configurations of the same runtime source. The verifier and delta batches use the frozen zz-modes-lifetime binary. The only retained Rust changes since freezing are test-only: mode: None in the tray snapshot initializer and clock-mode -Q in the CLI usage-error test.

The CLI integration suite also retained a bare clock-mode error expectation from main. Changed that usage-error case to clock-mode -Q, matching the already corrected mux test. This is test-only. The full package run keeps all original failures; four other CLI failures require isolated reruns.

The saved checkpoint binary passes attached-client in attached-before.txt (exit 0). Candidate attempts 1 and 2 retain their timed alert failures. A third candidate run uses the frozen binary after compilation has finished.

The first queued clippy invocation was canceled before it acquired a slot (exit 143) to test the cleanup-lock change before linting the chosen runtime.

The cleanup-lock experiment built and passed all four lifetime unit tests, but alert-cleanup.txt still fails the same alert checkpoint. It is reverted. The retained runtime is the original zz-modes-lifetime binary. Both traces place the freeze capture near or beyond the five-second alert budget; the checkpoint binary passes, and no clean candidate attached-client result was available at that stage. The final unchanged-fixture candidate run later passed. Trial logs and the focused trial run are retained.

The independent frozen-binary roster and its full self-check both pass (170/25 and every sabotage caught). The cleanup experiment also passes its focused 27 checks with zero sabotage failures, but is not retained because it did not resolve the alert failure.

After batch 1 completed command-item-format and copy-mode-previous-bracket cleanly, it was stopped with exit 143. Its remaining 46 rows are split into disjoint 23-row batches 1a and 1b; the original and interrupted scenario logs remain retained.

The serial CLI rerun also stalled in wait_exit_holds_the_control_process_until_a_second_blank_line. Its child was terminated after more than three minutes to release the unbounded wait; cli-wait-exit-stall.txt records that intervention. The earlier full seven-package run passed this case.

The final delta union completes all 199 selected rows and 2352 steps; no rows
are unrun or incomplete. Nine rows still fail after their runner retries:
lane2-store, show-options-hooks, smoke/cli-chain-parse-abort,
smoke/command-prompt-chain, smoke/control-hard-loss,
smoke/default-client-command, smoke/jobs-command-environment,
smoke/source-replay-diagnostics and smoke/status-background-jobs.
The two known rows retain their registered divergences. corpus-summary.json
contains every row's counters. corpus-logs.tar.gz contains 398 captured/final
logs checked against corpus-log-manifest.json. The runner overwrote lane2-store's
first raw log before the collector started; its first-pass divergence output
remains in delta-batch-3.txt and both archived copies of that row are the retry.

Final proof at runtime revision bd0adfb79fe19110b43e787401071ce0f9a26110:
client-1, client-2 and client-3 each pass 170 assertions with 25 records;
client-frozen and both full client self-checks pass too. The final verifier
passes 170/25 client commands and 78/0 choosers. client-1 completed before the
ordinary binary was rebuilt; that run, client-frozen and verifier-final give
three passing rosters for the frozen runtime. The final candidate attached
fixture passes unchanged, after the earlier three timed alert failures.
Choosers (78), copy-mode (147), screen-diff (147 assertions and six existing
records) and overlays (48) pass, as do their self-checks. attached-client has
no --self-check entry point; none is claimed.

The seven-package test run exits 101. Desktop, client, mux, protocol and TUI
targets pass. CLI has five failures in that run: one stale clock-mode error
expectation is fixed; attach, background-frame and menu-sizing failures pass
alone; the return-code matrix timeout recurs. The final serial CLI package
passes its other targets and 132/134 binary tests, retaining that timeout and
a wait-exit test whose unbounded child wait was terminated by the lane.
Daemon unit tests pass 984/985 and the failed callback test passes alone.
The agent slow-client soak fails both in the full run and alone: 1501 updates
instead of 1500. These unresolved tests are not claimed green.

Clippy with -D warnings on all seven crates, final formatting, compat/check.sh,
wire-version validation and OKF validation pass. OKF retains one pre-existing
research-age warning. tracker.py check is run again after sealing the proof.
The experiment that skipped empty cleanup work is fully reverted; the saved
runtime diff and runtime-diff-final.sha256 compare equal.
