# Customize and suspend, 2026-09-16

This lane starts at pinned 2e2dc584, independently of the running modes lane.
Every Cargo invocation goes through /tmp/zz-cargo.sh. The inherited capped-bin
adapter moves its trailing jobs argument before Cargo's argument separator and
removes it for fmt; it does not bypass the memory scope or shared slot.

## Ownership and extension

CustomizeMode lives in the existing per-pane PaneModeRequest stack. Selection,
expansion, tags, search and edit prompts survive snapshots. ChooseTreeState and
ChooserPresentation carry the tree to the existing chooser grid painter. It is
not a per-client ChooseTreeSession. The default roots match the pinned options
and key tables; the mode's format name is options-mode.

The design in knowledge/designs/tui-customize-mode.md gives the zz-knobs sibling
an explicit reveal action and a new root after the pin's sections. No zz section
is hidden by a comparison mask. This lane supplies no zz-specific knobs.

TmuxOption metadata now includes kind, description, unit and choices copied from
the locally pinned options-table.c. String, number, colour, style and key options
use text prompts; flags toggle and choices cycle. The scoped edit is executed as
a normal set-option so existing daemon option effects apply.

The pin has a move key table root. zz previously lacked the nineteen bindings.
Their registration is restored from the oracle; the floating actions remain a
registered gap. This is not evidence that zz implements floating placement.

## Suspend and the fixture's foreground job

The daemon resolves the client target, sends TSTP to a tty client, and excludes
suspended clients from list-clients and session attachment facts. The TUI pauses
output, restores termios and leaves alternate-screen, mouse and keyboard modes,
then stops itself with SIGSTOP. SIGCONT re-enters and repaints; ClientSuspendState
reports the transition while preserving the connection. Both wire additions use
unreleased version 104. A failed signal does not mark the client suspended.

The original fixture exec'ed the inner client as the outer pane leader. That
places it in an orphaned process group: pin SIGTSTP did not stop it, and the outer
tmux immediately continued zz's SIGSTOP (server_child_stopped). The fixture now
runs both clients under the same small foreground-job supervisor. The child has
its own process group, with a live parent in the same session; both inner clients
report T and return to the same screen after explicit CONT. This fixes the test
terminal's job-control arrangement, not its comparison. The final suspend case
stays last. The cleanup resumes zz before terminating the isolated servers.

The pin control-client probe returns empty command output and success while the
control client remains responsive. Nonterminal zz clients take the same no-op
path. No actual GUI, browser or remote-SSH client has been launched in this lane.

## Retained failures and scope of the proof

Focused probe 1 failed while the move table, border style and description wrapping
were missing. Probe 2 passed every customize checkpoint but exposed the foreground
job problem. Probe 3 passed all twelve comparisons, including a buffer-limit edit
to 7 and suspend/resume. Failed compilation and test outputs remain beside them.
The canceled build-2 request never acquired a compile slot and has an empty log.

The client-command self-check catches both new sabotages: one-sided expansion
changes the decoded tree; one-sided suspend changes the screen and session/client
state. Both equivalences pass after the changes are undone.

The opening-screen proof is not an exhaustive customize interaction proof. Key
binding editing, reset/unset and tagged bulk mutations, array insertion, help,
mouse interaction, kill-on-exit (-k), and zoom restoration (-Z) are not implemented
by this attempt. The two flags are explicitly refused. Incremental search,
filtering, navigation and arbitrary option edits have not all received pin screen
comparisons. These are limitations, not claimed parity. The final report must
retain them even if the two named fixture cases pass.

The attached-client fixture has no --self-check entry point; the supported full
fixture runs instead. Other requested regression fixtures run both modes.

The third filtered manifest and catalog requests were canceled while waiting for
the same worktree build lock after the full package request acquired a slot first.
Neither third request ran tests. This released the second slot for other lanes;
the full package result covers both test names. Their empty/waiting logs remain.

First full client-command result: 141 asserted comparisons identical, 28 recorded;
owners TUI-015=4 TUI-017=6 decided:TUI-016=1
gap:clients.interactive-refresh=16 gap:protocol.socket-acl=1 unattributed=0.
The first capture archive preserves every decoded screen file byte for byte.
Clippy across zz-protocol, zz-mux, zz-daemon and zz-tui, all targets and features,
passed with -D warnings after fixing two customize style lints.
The first chooser regression failed only zoom-manual-released (77/78 passed);
its output remains pending a controlled rerun.

The four-package run completed. TUI: 212 library tests passed. Daemon: 936/938
library tests passed; the delayed shell-job timeout and repeat-prefix deadline
failed under parallel load and are being rerun alone. Its other targets completed.
Mux library and protocol wire tests passed; remaining failures were stale catalog
counts in mux hunt_claims and protocol catalog tests, plus customize-mode's hidden
-y flag needing a separate pinned usage override. These test inventories are
updated, with filtered reruns retained.

The second full roster passed both new cases but failed the 12-with-seconds
clock checkpoint (140/141); clock-repeat.sh subsequently passed both seconds
styles and their teardowns (4/4). This does not erase the retained clock failure.
The controlled SHELL=/bin/sh chooser run passed 78/78. The claim verifier's
client-command run passed 141/141 with no TUI-014 records, but its chooser run
failed zoom-tree-open (77/78). The original chooser failure was
zoom-manual-released. Therefore verify-claims exited 1 despite the separate
passing chooser run. The lane does not claim that the broader zoom issue is fixed.
A source inspection found that refresh_status_filtered snapshots the engine
before acquiring the status renderer and publishes after releasing that renderer;
concurrent refreshes can therefore publish out of snapshot order. That is a
candidate explanation for a stale zoom marker, not a proven diagnosis here.

The rebased production commits are 70b7174c and 0ae39523, on top of b2c00bd2.
origin/main did not contain the modes tip at rebase time. Existing fixture and
corpus processes continue using the initial built binary; the final rustc request
uses a distinct output name so its provenance can be measured independently.

The delayed-job failure passed alone. The client-scope test failed alone too:
KeyEngine::active_table returns None once the repeat deadline expires, and this
unit test used the default 500 ms while asserting client ownership after the
switch. Its setup now requests repeat-time 60000, keeping the production deadline
and all ownership/format assertions unchanged. This test-helper change is part
of the declared daemon.rs footprint.

The catalog filter waited 712.9 seconds on slot 0 while slot 1 was idle. Its
waiting scope was canceled, its result retained, and a fresh wrapper request
passed all 33 catalog tests. The mux catalog filter also passed. final-checks.py
probes the existing locks without blocking and, when one is free, seeds Bash's
RANDOM before sourcing the unchanged /tmp/zz-cargo.sh. The wrapper still performs
systemd-run with its memory/swap cap, flock, and jobs argument for every command.
No shared wrapper or lock file is rewritten. The actual launcher is recorded in
each result JSON. This avoids needlessly queueing behind the busy slot; a race
still falls back to the wrapper's normal wait on that same lock.

The controlled client-scope test passed (2.10 seconds), with its 60-second
repeat interval and every existing ownership/format assertion intact. The final
package pass uses four test threads to reduce timing contention. The rebased
binary passed all twelve focused edit/suspend/resume checks and the full
client-command self-check. Original target/debug/zz remains byte-identical to
binaries-initial.json; final rustc -o warnings only describe the alternate
output filename. The final binary hash and exact output path are in
binaries-final.json.

Final four-crate tests: mux 536 library tests plus all integration targets pass;
protocol 227 library tests plus its catalog/wire integration targets pass; TUI
212 library tests pass. Daemon has 938 passing library tests and one retained
delayed shell-job cwd failure; all its other targets pass (one soak is ignored).
The isolated rerun is recorded separately. The repeat-ownership helper now passes
in the full run. Final formatting and four-crate all-target/all-feature clippy
with -D warnings pass. Formatting only reflowed the customize usage override.

Screen-diff passes 147 assertions with its six existing records; its self-check
catches every sabotage and passes every equivalence. Overlays pass 48 assertions,
zero records, and their self-check passes. The final binary's full client-command
roster passes 141 assertions with the same 28 attributed records and none owned by
TUI-014. This is the fourth full roster in this attempt, counting both live
verifier runs. The clock failure in the second roster remains retained.

The final delayed-job test passes alone (0.45 seconds). Its initial isolated
request and compat/check.sh's first internal Cargo request waited on busy slot 0
while slot 1 was free; both waiting flock processes were terminated before tests
ran. The canceled results are retained. The retry uses the same unchanged wrapper
with its free-slot seed. The attempt-local capped-bin/cargo adapter applies that
same selection to compat/check.sh's internal requests, preserving the wrapper's
scope, locks and jobs. Tracker validation and wire-version.py pass; version 104
is correctly reported unreleased after v0.10.0's 103.

The free-slot compat/check.sh retry passes (18.9 seconds), including its internal
Cargo checks. The original canceled wait is not counted as a passing check.

The final rebuilt-binary live verifier passes: client commands 141 assertions/28
attributed records with none owned by TUI-014, choosers 78 assertions/zero records,
exit 0 after 1346.5 seconds. This does not erase the earlier verifier failure.

Attached-client completes successfully with its supported full fixture (no
self-check option exists). A final-binary corpus check exposed another stale
fixture count: command-flag-errors.tsv contains 89 commands, 75 aliases and 84
required-flag rows after this lane's additions, while its shell driver still
required 87/74/82. It now requires 543 failure probes and 3 success probes, total
546. The first log where both sides lacked COMMAND_FLAG_ERRORS is preserved in
final-command-flag-errors-first.txt. This shell test-helper change is part of the
complete footprint. The broader corpus also retains lane2-store's two
lock-command default differences (lock -np versus vlock).

The broad command-flag fixture also finds an inherited attach-session failure: zz
returns status 2 for invalid attach flags while the pin returns 1, with the same
stderr. attach-flag-diagnostic-with-server.json measures that exact difference;
the earlier no-pin-server probe is retained separately and does not prove it.
The full fixture's automatic retry staged the old count script before the count
correction was written. The delta run will stage the corrected script. A focused
final-binary probe compares all eleven added customize/suspend diagnostic cases
(stdout, stderr and exit status), and all eleven pass. This proves the added rows
without claiming that the broader attach-status defect has been repaired.

A final source check refines the SSH limit: connect_endpoint_with_prompts_and_terminal
uses PortableTerminalSize for an SSH terminal surface, and that facts scope omits
the tty. The new handler therefore performs its no-tty no-op for the built-in SSH
transport; it does not signal a PID on the other host. This is source inspection,
not a live SSH proof. Local raw-TUI suspend/resume remains the measured contract
case. Remote suspension needs a client-directed action in a follow-up.

The delta first pass also measures seven show-options-hooks output differences
from the same lock-command default, alias-group-forgery's status 2 versus 1,
and cli-chain-parse-abort's broken versus clean:6 marker. Those scenario names
already failed in the inherited attempt-06/corpus-summary.json and have raw
measurements in attempt-06/corpus-final-details.txt. The current failures and
retries are still retained; prior failure does not turn them into passes.

The direct compat-check-free-slot and compat-check-counts retries passed but
did not explicitly set the scrubbed home. compat-check-scrubbed repeats the gate
with HOME=/tmp/zz-emptyhome and XDG_CONFIG_HOME=/tmp/zz-emptyhome/config, plus the
explicit Cargo/Rustup homes and the capped PATH adapter. Its result is the final
gate environment claim; the earlier results remain as measured. All full package
and standalone daemon test invocations used the scrubbed home.

## Final base selection

Fetched both refs before delivery. `origin/campaign/tui-modes-12` remains `b2c00bd26091523a167343b1d39f36c036662d07`; `origin/main` is `d1694e65da7552e8ebd53da2ce596ee8ffa748e9` and does not contain that modes head. This lane is already rebased onto the current modes head, so no further rebase is required. The complete own contribution and inherited main comparison are recorded separately in footprint.json and footprint-from-main.txt.

## Completed delta measurement

The full selected corpus completed 199 rows and 2352 steps with zero unrun rows, exit 1. Failed after the isolated retry: lane2-store, show-options-hooks, smoke/alias-group-forgery, smoke/cli-chain-parse-abort, smoke/command-flag-errors, smoke/jobs-command-environment, smoke/plugin-runtime-resurrect-restore, smoke/source-replay-diagnostics, smoke/status-background-jobs. Recovered on retry: smoke/command-prompt-chain, smoke/command-prompt-editing, smoke/plugin-runtime-continuum. Both first and final logs are in corpus-logs.tar.gz; corpus-log-manifest.json records their SHA-256 hashes. The archive was verified against every source byte before removing the temporary copied directory. The harness footer can say Nothing failed on the first pass when none recovered; corpus-summary.json uses its actual per-row results and retry warnings instead.

Source and document whitespace checks pass. Raw captured evidence retains trailing spaces and blank final lines; the full staged whitespace checker consequently reports those raw-output locations. They are not normalized because decoded cell and diagnostic evidence must remain verbatim.
