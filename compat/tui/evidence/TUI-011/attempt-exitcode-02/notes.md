# Exit-code follow-up, cycle 11

Lane `exitcode`, branch `campaign/tui-exitcode`. Rebased cleanly onto fetched
`origin/main` at `e40a13e1c981aa9b707a5a6a54f5e0267c751de9` on 2026-09-16.
This attempt supplements attempt-exitcode-01; it does not erase any earlier failed run.

The user resolved the earlier help-output scope question explicitly. The exit-code table
and accompanying explanation now live in the `WORKSPACE_TOOLS` generator in
`crates/zz-daemon/src/daemon.rs`. Before regeneration, the hand-edited skill was restored
from origin/main. `just tools-skill` then rebuilt zz, started its throwaway daemon, and
regenerated `.agents/skills/zz-workspace/SKILL.md`. No edit followed generation. The resulting
file has the same contents as the intended skill text already on this branch. The focused
`tools_skill_matches_workspace_skill` test passes (1 passed, 0 failed, 950 filtered out).
This closes the generator synchronization failure and supersedes the pending clarification
in attempt-exitcode-01. The explicitly authorized catalog-help change is the only new
printed-text change in this follow-up; command error diagnostics remain unchanged.

The superset fixture fails on both the rebuilt candidate and a fresh build of plain
origin/main: `error: the sidebar withdrawn did not happen within 12 seconds`, exit 2.
Each fixture ran after its build finished, with no other build, test, lint, or fixture
from this turn in flight. Other lanes remained active on the shared host; load snapshots
are retained. Main was checked out as a clean detached worktree inside this worktree's
ignored target directory and built into a separate target directory. Source identity,
build logs, binary hashes, both fixture outputs, and both diagnostic sets are retained.

The failure is a reproduced baseline failure, not an exit-code regression. It is not
classified as load-induced: neither isolated run passed. Both stop in the sidebar group
after `still-drawn-while-unfocused`. The decoded zz and tmux screens, active-pane output,
and pane listings are byte-identical at the timeout. Both show `q` reaching the shell
while the sidebar stays drawn. Both last command results are status 1 with
`focus-sidebar requires an interactive client`. Later fixture groups were not reached.
`superset-comparison.json` records these comparisons without normalizing the captures.
The residual remains owned by TUI-012.

Every cargo invocation, including those inside `just tools-skill` and compat/check.sh,
runs through `/tmp/zz-cargo.sh`. The retained adapters route nested cargo calls into the
wrapper, move its appended jobs argument before the Rust-test/clippy separator, and
remove it for rustfmt. The queue adapter may retry only before cargo starts, terminating
only its own childless queued scope. The initial direct skill-test invocation selected a
busy slot and was terminated before cargo started (signal 15); the successful retry and
the canceled invocation are both retained. Actual compilation and tests keep the wrapper's
4 GiB memory / 2 GiB swap / two-job cap and one of the two shared locks.

The full zz-daemon package passes with scrubbed HOME/XDG and one test thread: 951 unit
tests and 34 integration tests pass, zero fail, one pre-existing soak test is ignored,
and doc tests pass (zero cases). Exit status is 0. Both previously reported main failures,
`default_shell_rejects_invalid_values_and_unset_controls_the_child_shell` and
`switch_client_key_table_and_formats_are_client_local`, pass in this run. The generated
skill synchronization test also passes again in the full run. No baseline daemon test
rerun was needed because the candidate package has no failures to compare.

The final code commit is `755ebd6d1a253206bf622ae843181b7cb3c1ee74`. Formatting,
compat/check.sh, the widened wire-version guard, the TUI tracker, and clippy for zz,
zz-protocol, zz-mux, and zz-daemon (all targets/features, warnings denied) pass.
compat/check.sh includes all 534 mux unit tests and the three daemon manifest checks.
Protocol stays at 104 with both hello pins at 0x68; this follow-up changes no wire shape.

No fixture or assertion was changed in this follow-up, and no recorded case was flipped.
The earlier three client-command passes, six new sabotaged assertions, attached-client
proof, superset self-check, full delta corpus, and other package tests remain in attempt-01.
Those runs were not repeated here. The inherited 25 client-command records keep their
owners and unattributed=0. The earlier corpus and unrelated zz test residuals remain
as documented there; neither attempt marks TUI-011 verified.

`measurements.json` indexes every follow-up command and result. `tests.txt` and
`fixtures.txt` are readable summaries; raw-evidence.tar.gz retains every original log,
including the canceled queued command, both failed fixture runs, diagnostics, source
identity, load snapshots, source hashes, and the cargo adapters. All archived file bytes
were compared against their originals before packing the final evidence layout.
