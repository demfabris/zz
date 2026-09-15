# Review and gate of TUI-016, attempt-01

Two passes: an adversarial review on the alienware box of `campaign/tui-introspection` at 8d6944cf,
then a fix pass on the ubuntu box the same day that landed all four findings on the same branch, then
this gate's verification on the ubuntu box.

## The alienware review's verdict: APPROVE-WITH-FIXES

Verbatim, as the fix pass's batch carried it:

1. BLOCKER: show-messages -J prints two rows where the pin prints one as soon as a format job is
   live, so TUI-016 clause 1's -J half has asserted evidence only for the empty table. The branch's
   own last commit names the cause of the duplicate format job in TUI-016's record: read it. Read the
   pin's format job table (format.c: format_job_get and the job tree; cmd-show-messages.c's -J path)
   in the source tree beside the pin, make zz's table hold what the pin's holds (one entry per
   distinct job the way the pin keys them), and add a compat/tui-client-commands.sh case that arms a
   format job on both sides (a #() status job is the pin's usual way), lets it run, and compares
   show-messages -J exactly, with a --self-check sabotage that fails for the right reason.
2. MUST-FIX: the recorded client-naming decision (TUI-016 clause 2) describes the server log wrongly:
   the pin names any tty-bearing client by its tty, zz names none and spells one row by hostname.
   Re-measure both sides (an attached client, and a clientless CLI), rewrite the decision in
   evidence_note with the old behaviour, the pin's measured behaviour and the sentence "decided
   2026-09-14 by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible",
   then either close the difference (preferred, if it fits your zones and budget) or register it with
   the measurement.
3. NIT: two notes.md files under compat/tui/evidence cite a commit reachable from no ref. Replace each
   with the sha the measurement was really taken at, reachable from your branch (git log will tell you
   which commit the run followed).
4. NIT: the diff adds about 60 comment lines against this repo's rule against comments in code. Strip
   every comment line your branch added under crates/ (git diff origin/main...HEAD -- crates/ | grep
   '^+.*//' finds them; a fixture's header block under compat/ is documentation and stays; a doc
   comment that an existing public item already carried is not yours to remove).

## The fix pass

Landed on the same branch above 8d6944cf as `Run one status format job per attached client the way
the pin keys them`, `Name the server log's client by its tty`, `Drop the comments this branch added
under crates` and `Record the introspection review fixes in TUI-016 and TUI-017`.

## This gate's verification, ubuntu box, 2026-09-15

Rebased onto origin/main 287e3815, verified at 51c858e2. The menus half of cycle 9 was gated at the
same time and was NOT merged, so nothing here rides on it.

1. THE BLOCKER, verified. `ShellCacheScope` is gone from crates/zz-daemon/src/status.rs and the shell
   cache key is `(ClientId, FormatJobTag, String)`, one entry per client per tag per command the way
   `format_job_get` keys the pin's per-client job trees. `DaemonFormatHooks::shell` now returns early
   unless the request carries a client, which is the pin's own guard: `server_client_loop` only calls
   `server_client_check_redraw` for a client with a session. The unit test
   `a_client_with_no_attached_session_runs_no_status_job` pins both halves - one cache entry for the
   attached client, none for the clientless one. The fixture case `messages-jobs-live` arms
   `#(sleep 40; echo zzcc-job)` in status-left on BOTH sides, waits for exactly one `^Job ` row on
   each, and asserts `show-messages -J` with the fd and pid normalized by the same substitution over
   both sides and the command beside them left alone. Its sabotage `live-job-sabotage` arms that job
   on the zz side only and requires stdout to report the extra row. Measured here: the fixture's own
   summary line is `all 80 asserted comparisons identical, 29 recorded not asserted (0 for a sibling
   lane)` on three consecutive runs (exit 0 each), and `--self-check` exits 0 with the live-job
   sabotage caught in its own channel.
2. THE MUST-FIX, verified. `server_log_client_name` takes the client's tty whenever it has one. The
   decision in TUI-016's evidence_note carries the old behaviour (every row spelled by
   `ServerState::client_names`, which for an interactive client is the hostname), the pin's measured
   behaviour (any tty-bearing client by its tty, a clientless CLI by `client-<pid>`, and each command
   reprinted through `args_print`), and the required sentence `decided 2026-09-14 by the orchestrator
   under fabrico's TUI parity contract of 2026-09-09; reversible`. Checked textually in the record and
   in the fixture's own `LOG_IDENTITY` reason and DECLARED block. The tty half is closed; what stays
   registered is the clientless CLI's name and the args_print reprint, which is exactly what clause 2
   permits: "either close that difference or register it with the measurement".
3. THE COMMENT NIT, verified. `git diff origin/main...HEAD -- crates/` at this tip adds ZERO lines
   carrying a non-doc `//` comment (doc comments and URLs excluded from the count).
4. THE SHA NIT, verified. Both notes.md now cite `f0d34be7`, the last code commit before the runs they
   name, in place of the unreachable `c09ccbee`. Those shas are the branch's pre-rebase spelling and
   stay reachable from `origin/campaign/tui-introspection`; the gate's own proof block carries the
   post-rebase revision instead.

## The recorded cases, counted against this obligation

29 recorded, and the gate read which obligation each names before verifying. Seven are
`clients.interactive-refresh` (refresh-client's pan, cursor, clipboard and adjustment forms), six are
`capture.rich-transports` and belong to TUI-017's clause 1, four are `protocol.binary-streams`, four
are `commands.native-client-tools`, two are `SERVER_ACCESS`, one is `CLIENT_TREE_CLIENTLESS`, and one
is `messages-log`. Only `messages-log` is this obligation's, and it IS clause 2's registration, which
the clause asks for by name. No recorded case leaves a TUI-016 clause open, and none is a sibling
lane's (the fixture prints `0 for a sibling lane` itself).

## Verdict

Both clauses assert at 51c858e2 and every one of the review's four findings is verified above. The
obligation is NOT flipped to `verified`, and the reason is the campaign's re-measurement guard rather
than the obligation. compat/tui/verify-claims.py refuses `verified` while the obligation's mapped
fixture reports any recorded case at all. That rule is exact where a fixture serves one obligation -
compat/tui-mouse.sh and TUI-008 - and over-counts for compat/tui-client-commands.sh, which serves
TUI-011, TUI-014, TUI-016 and TUI-017 between them: 28 of its 29 recorded cases are other
obligations' or accepted gaps, and the one that is TUI-016's is clause 2's own registration. The gate
mapped TUI-016 and TUI-017 into the tool's FIXTURES table so the claim is re-measurable at all - the
tool asked for that itself - and stopped there. Teaching it per-obligation attribution is a small
named change that wants its own review; a gate does not weaken a guard to pass its own claim. The
flip is TUI-016's next_action and nothing else about the obligation is open.

TUI-017 stays active: its clause 2 is closed and its clause 1, the six rich capture transports, is
cycle 10's capture lane.
