# TUI-016 attempt-01, cycle 9 introspection lane

Base `origin/main` b6a57af1. Branch `campaign/tui-introspection`. Box: alienware,
CachyOS Linux, pin d77c9dc6.

## Files

- `environment.txt` — the box, the pin, the zz binary's sha256, the toolchain,
  the cargo memory wrapper. Written first.
- `show-messages-probe.txt` — the whole clause-1 measurement, one throwaway zz
  daemon and one throwaway pinned tmux server, a client each attached inside one
  outer pinned tmux at 80x24 with TERM=tmux-256color. Holds: `-T` on both sides,
  the normalized line-for-line diff of the two 234-line dumps, both sides'
  `client_termfeatures` and `client_colours`, `-J` with no job and with a live
  `#(sleep 30; echo hi)` in status-left, `-JT`, `-T -t <client>`, `-T -t` a name
  that matches nothing, and `-t` alone.
- `log-client-identity.txt` — clause 2's measurement. The same four commands on
  both sides, then each side's own `show-messages`.
- `client-commands-run-1.txt`, `-2`, `-3` — `compat/tui-client-commands.sh` three
  times at f0d34be7.
- `client-commands-self-check.txt` — the same fixture's `--self-check`.
- `corpus-messages.txt` — `compat/run.sh --strict-geometry` over every corpus row
  that names show-messages or reads the terminal interrogate the landing moves.
- `format-job-duplicate.txt` — the finding below, on the shipped build.
- `tui-caps.txt` — `compat/tui-caps.sh`, because this landing changes the
  `TtyTerm` behind `#{I/c:}` and `#{I/f:}` and TUI-009 is verified on that
  fixture: 366 asserted rows, all identical, 0 recorded.
- `status-row.txt` — `compat/status-row.sh` under `LC_ALL=C LC_TIME=C`, the
  control this box needs, because the status shell job grew an fd, a pid and a
  serial: 14 comparisons identical, none recorded.
- `compat-check.txt` — `compat/check.sh`, whose verify-claims pass says every
  verified obligation holds up.

## What the runs say

`show-messages -T` is 234 lines on both sides and the normalized diff is empty:
the header `Terminal 0: tmux-256color for /dev/pts/N, flags=0x31:` and then
`tty_term_describe` for each of the 233 codes, indices and spellings included.
Only the pts number is normalized, and the same substitution runs over both
sides. Before the landing zz answered `unsupported command: show-messages -T` at
exit 1.

`show-messages -J` is empty on both with no job running. With one live format job
the pin prints `Job 0: sleep 30; echo hi [fd=10, pid=N, status=0]` and zz prints
the same line shape twice, once per shell-cache scope — see the finding below.

`-t` is tolerated: `-T -t <client>` is that client's 234 lines on both sides and
`-T -t /dev/zzcc-nope` is every terminal on both, each at exit 0 with empty
stderr, which is `CMD_CLIENT_CANFAIL` leaving the target unset.

The fixture's own summary line, all three runs:

    all 78 asserted comparisons identical, 29 recorded not asserted (0 for a sibling lane)

At BASE the same fixture printed `all 61 asserted comparisons identical, 37
recorded not asserted`. Of the 29 that remain, none belongs to this obligation
except `messages-log`, which is clause 2's registration.

## Finding, measured and not closed

`format-job-duplicate.txt`: one `#(sleep 40; echo hi)` in status-left on a
server with one attached client is TWO child processes and two `-J` rows where
the pin has one. It is there before any refresh, it survives every refresh, and
a refresh replaces only the other one.

The cause, read off a one-line instrumentation of `shell()` on this same build
and then removed: every clientless CLI invocation gets a status render of its
own, and that render has no attached session, so `facts.client` is None and the
shell cache takes the `Unattached` scope. All of them collapse into that one
entry, which holds a second live job beside the attached client's:

    JOBDBG scope=Attached(ClientId(3))  facts_client=true  cmd=sleep 40; echo hi   x16
    JOBDBG scope=Unattached client=ClientId(11..19) facts_client=false cmd=sleep 40; echo hi

ClientId 11 to 19 are nine separate `zz` CLI processes. The cache is keyed
`(scope, tag, command)` in `crates/zz-daemon/src/status.rs` and the pin keys
`all_jobs` by the expanded command and its tag alone, but the key is not really
the defect: a command client that draws nothing should not be rendering a status
line at all. Either way that is the status renderer's contract and not
show-messages', it moves every `#()` on every status surface, and no punch-list
item names it, so it is recorded here with its cause rather than changed in this
lane. `-J` reports what zz is actually running, which is the clause's ask.

A live `-J` row cannot be a fixture case: `fd` and `pid` belong to one process
and no two servers share them, and a `#(sleep ...)` in the driver's scene would
race every later case's status render. The shape is pinned by the measurement
above and by `job_summaries` writing `job_print_summary`'s format string.

## The review fix pass, 2026-09-14, on the Ubuntu box

Re-measured and re-landed on this box (Ubuntu 26.04.1, 8 cores, kernel 7.0), on
top of the reviewed tip 8d6944cf. The four findings of the APPROVE-WITH-FIXES
review are all closed here; the files of this pass are prefixed `fix-`.

`fix-environment.txt` — the box, the pin, the binary and RUN_ENV of every run
below.

THE BLOCKER, the duplicate format job. The pin keys a format job in
`format_job_get`: one `format_job_tree` per client, one global tree for an
expansion with no client, each keyed by the format tag and the command, and
`job_print_summary` walks `all_jobs` across all of them. zz's shell cache was
keyed the same way — `Attached(client)` beside `Unattached` — but zz reached the
`Unattached` key by rendering a status line for a client with no attached
session, which the pin never does: `server_client_loop` only calls
`server_client_check_redraw` for a client with a session, and that function
dereferences `c->session` on its first line. So every clientless CLI invocation
armed a second copy of every `#()` on the server. The landing is the pin's own
guard, in `StatusRenderer`'s `shell`: no attached session, no status, no job.
The `Unattached` scope then had no way to be constructed, so the cache is keyed
by `ClientId` directly and the scope enum is gone.

`fix-client-commands-run-{1,2,3}.txt`, `fix-client-commands-self-check.txt` —
three runs of compat/tui-client-commands.sh at the tip plus its --self-check.
Each run prints

    all 80 asserted comparisons identical, 29 recorded not asserted (0 for a sibling lane)

against `all 78 ... 29 recorded` at 8d6944cf. The two new asserted cases are
`messages-jobs-live` and `messages-jobs-live-restored`: one `#(sleep 40; echo
zzcc-job)` armed in status-left on both sides, waited for on the pin's own
needle (one `^Job ` row) and then on zz's, `show-messages -J` compared exactly
with the fd and the pid normalized over BOTH sides, and status-left put back
with both screens asserted equal afterwards. The self-check's new sabotage,
`live-job-sabotage`, arms that job on the zz side alone and requires stdout to
report it; it does, as `0a1 > Job 0: sleep 40; echo zzcc-job [fd=N, pid=N,
status=0]`, which is also the proof that the normalizer does not swallow a row.

THE MUST-FIX, the client-naming decision. `fix-log-client-identity.txt` is the
re-measurement, one attached client and a clientless CLI per side, both sides
driven by the same commands. The pin names ANY tty-bearing client by that tty —
its `attach-session` row reads `/dev/pts/9` — and a clientless CLI by
`client-<pid>`. zz named NO row by a tty: the server log read
`ServerState::client_names`, which holds the `device_name` the client sent in
its hello, and an interactive client sends `short_device_name()`, the hostname.
That half is closed: `server_log_client_name` now takes the client's tty when it
has one and keeps `device-<n>` otherwise, so at this tip the attached client's
own rows read `/dev/pts/8` on zz beside `/dev/pts/9` on the pin. What is left is
the clientless CLI, `client-<pid>` against `device-<n>`, still registered as the
`messages-log` case with the `args_print` half beside it.

THE TWO NITS. The unreachable `c09ccbee` this file and TUI-017's cited is
replaced by `f0d34be7`, the last code commit before b2260e32 added these runs,
which is the tree the three runs followed. And every comment line the branch had
added under `crates/` is gone: `git diff <base> -- crates/ | grep -c '^+.*//'`
answers 0 at this tip.

OTHER PROOFS AT THE TIP. `fix-package-tests.txt` — `cargo test -p zz-daemon`
(895 + 34 tests, all green) and `cargo test -p zz` (638 + 145, all green).
`fix-clippy.txt` — clippy over zz-daemon, zz-mux and zz-terminal with
`-D warnings`, clean. `fix-corpus-jobs.txt` — the eight corpus rows that drive a
`#()` status job or read the server log (smoke/status-background-jobs,
smoke/refresh-status, smoke/jobs-command-environment, smoke/jobs-shell-job-cwd,
smoke/jobs-display-popup-environment, status-options, census-hooks, move-swap),
every one at 0 divergences under --strict-geometry. `fix-corpus-plugins.txt` —
smoke/plugin-runtime-continuum, smoke/plugin-runtime-oh-my-tmux,
smoke/format-trace, smoke/positional-maximums and smoke/command-flag-errors,
also 0.
