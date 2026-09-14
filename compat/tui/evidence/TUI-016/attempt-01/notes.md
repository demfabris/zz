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
  times at c09ccbee.
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
