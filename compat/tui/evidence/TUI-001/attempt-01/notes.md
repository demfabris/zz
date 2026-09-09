# TUI-001 attempt-01

Everything here was produced on 2026-09-09 on the alienware box (CachyOS, 16
cores) from the worktree `/home/demfabris/dev/zz-tui-lane` at
`576b6b745821856d9c00b3a62d6a1982fd8571e1`, against pinned tmux
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. Every file is a real run.

## What each file is

| File | What it is |
| --- | --- |
| `environment.txt` | sha256 of both binaries, zz revision and dirty state, pin commit, OS, TERM, shell, bash, locale, the sizes each fixture drives |
| `geometry-run-1..3.stdout.txt` | `compat/tui-pane-geometry.sh` as it stands at `origin/main`, three consecutive runs, all exit 0 |
| `geometry-run-4..6.stdout.txt` | the same fixture after this attempt's timeout-diagnostics change, three consecutive runs, all exit 0 |
| `geometry-run-*.stderr.txt` | the matching stderr, all empty |
| `status-row.stdout.txt` | `compat/status-row.sh` under the box's own locale, exit 1 |
| `status-row-LC_ALL-C.stdout.txt` | the same fixture with `LC_ALL=C LC_TIME=C`, exit 0 — the control that isolates the exit 1 above |
| `smoke-tui-client-input-backpressure.stdout.txt` | `compat/run.sh --strict-geometry smoke/tui-client-input-backpressure`, exit 0, 0 divergences |
| `attached-client.stdout.txt` | `compat/attached-client.sh`, exit 0, `attached-client compatibility: PASS` |
| `timeout-diagnostics/` | a deliberately sabotaged run, kept as proof that the new dump fires and as the shape a real timeout leaves behind |

`environment.txt`'s zz sha256 identifies the artifact that produced these runs;
it does not attest it. A cargo debug build of zz is not bit-reproducible on this
box: the cycle-18 reviewer rebuilt `-p zz` at this same worktree path from the
same source and got a different hash, and a third from a different worktree. The
pin's hash does verify exactly. Provenance here is carried by reproduction
instead, which is the stronger check: the reviewer's independently built zz
reproduced this attempt's fixture results and TUI-002's 195 capture files byte
for byte. `environment.txt` was also captured before this attempt's
timeout-diagnostics change, so its `git status --short` line describes the tree
that produced `geometry-run-1..3`, not `geometry-run-4..6`; both shapes are
named in the table above. A later cycle that wants an attestable hash should
take it from a release build, or write a second `environment.txt` after the last
fixture edit.

## The recorded timeout

The ledger recorded an exploratory run on 2026-09-09 that exited 2 on `zz
geometry report did not happen within 10 seconds`, on macOS, with an existing
`target/debug/zz` that was neither rebuilt nor revision-attested.

That macOS run cannot be re-run on this box. **This attempt does not explain it
either, and clause 2 is not satisfied here.** An earlier draft of this file
claimed it was, on a stale-pre-fix-binary story that the campaign's own registry
refutes. What follows is what was measured, including that refutation.

1. **The fixture and the runtime at `576b6b74` are sound here.** A zz built in
   this worktree passes `compat/tui-pane-geometry.sh` six times out of six
   across two shapes of the fixture, in 4 to 5 seconds a run against a 10 second
   bound. The wait that fired in the record is the one at `measure()`, and it is
   nowhere near its bound at this revision.

2. **The named backpressure cause is measurably absent at this tip.**
   `compat/run.sh --strict-geometry smoke/tui-client-input-backpressure` exits 0
   with 0 divergences here, so the mechanism `tui.client-input-backpressure`
   named is not present in the binary this attempt attests.

3. **The stale-pre-fix-binary hypothesis is refuted, by the very gap that was
   cited for it.** `tui.client-input-backpressure` (closed 2026-09-07 at
   `0bed7fe7`) does list `file:compat/tui-pane-geometry.sh` among its evidence,
   and its recorded cause is exact — `crates/zz-tui/src/render.rs`
   `flush_output` did a blocking `write_all` plus `flush` on the main event
   loop. But that same resolution rules the stall out as the cause of a macOS
   geometry expiry, verbatim:

   > It also exited 0 at origin/main 012b4dcc before the fix, so the macbook
   > expiry the cycle-17 gate recorded is not this stall reproducing there: that
   > script drives the pane through the CLI's send-keys, not through the
   > client's stdin, and its inner clients run inside an outer pinned tmux that
   > reads their output continuously, so no backpressure ever builds. What
   > remains on the macbook is unexplained and belongs to whoever next runs the
   > tool there.

   Two refutations, either one sufficient.

   **Empirical.** `012b4dcc` is 2026-09-07T10:45:51-03:00, after the fixture
   reached its current shape at `314c55e0` (00:15:42) and before the fix at
   `0bed7fe7` (13:31:08) — exactly the window a stale binary would have to come
   from — and the registry records this fixture exiting 0 there. A pre-fix
   binary running this fixture is measured as passing, so a pre-fix stale binary
   does not produce this timeout.

   **Mechanism, and it is platform-independent.** The fixture's inner clients
   run inside an outer pinned tmux that drains their output continuously, so the
   backpressure `flush_output` stalled on never builds in this fixture at all.
   If no backpressure builds, the size of the macOS pty write buffer is
   irrelevant and the pre-fix stall cannot fire on any platform. The pty-buffer
   hedge the earlier draft offered does not reach this half.

**So what clause 2 still needs.** The registry names this exact clause as open
work: "What remains on the macbook is unexplained and belongs to whoever next
runs the tool there." Neither measurement (1) nor (2) above explains a macOS
expiry — they establish that the fixture and the runtime are sound on Linux at
this revision and that one candidate cause is gone. The recorded timeout stays
unexplained, and closing clause 2 needs a run of `compat/tui-pane-geometry.sh`
on macOS with an attested binary, with the timeout dump this attempt added
retained if it fires. The timed-out binary's own revision is unrecoverable:
`/home/demfabris/dev/zz/target/debug/zz` was rebuilt on this box at 18:25 on
2026-09-09, before this attempt started, so its hash says nothing about the
record.

## What the timeout dump now retains

A bounded wait that runs out used to print one line. `compat/tui-pane-geometry.sh`
now copies out, before the scratch tree is removed:

- `what-fired.txt` — which wait ran out, the size under test, both binary paths,
  all three socket names, and a listing of `ZZ_LOG_DIR`
- `outer-zz.screen.txt`, `outer-tmux.screen.txt` — the outer pinned tmux's
  `capture-pane -p -e -S -` of each side, escapes and scrollback included
- `zz.list-clients.txt`, `tmux.list-clients.txt`, `zz.list-panes.txt`,
  `tmux.list-panes.txt`, `outer.list-panes.txt` — each server's view
- `zz-daemon.stdout.txt`, `zz-daemon.stderr.txt` — the daemon it started
- `zz-client.stderr.txt`, `tmux-client.stderr.txt` — each attached client's own
  stderr, teed out of the pane so a repaint cannot overwrite it

The fixture also pins `ZZ_LOG_DIR` into its scratch tree and scrubs
`XDG_STATE_HOME`, so the daemon it starts can no longer write into the box's
real state directory (`crates/zz/src/diagnostics/mod.rs` `platform_log_dir`
reads `ZZ_LOG_DIR`, then `XDG_STATE_HOME`, then `HOME`). A foreground
`zz daemon` logs to its own stderr rather than to the ring file, so the ring
listing in `what-fired.txt` is empty and `zz-daemon.stderr.txt` carries the
daemon log; the dump says so rather than leaving the absence unexplained.

### The sabotage in `timeout-diagnostics/`

`file_has_two_fields` was edited in place to demand three fields, which no
`tput cols; tput lines` report can ever produce, and the fixture was run once.
It exited 2 with **the same text as the record**, `zz geometry report did not
happen within 10 seconds`, and left the fourteen files in that directory. The
edit was reverted immediately afterwards; the fixture on the branch is the
unsabotaged one, and `geometry-run-4..6` are its runs. The retained screen shows
the send-keys'd command delivered and executed and the pane at 80x23, which is
what a dump should look like when the runtime is healthy and the assertion is
the thing that is wrong — the opposite reading from a dump where the screen is
blank or the client list is empty.

## Measurements this attempt hands to other lanes

**120 columns, for TUI-004.** At 120x24 the pin hands its pane **120** columns
and zz hands its pane **91**, both at 23 rows. The 29 column difference is the
sidebar's 28 plus its 1 column border, which is the same arithmetic as
`AUTO_HIDE_COLUMNS = 80 + 28 + 1 = 109` in `crates/zz-tui/src/sidebar.rs`. Rows
match at every size. This stays recorded, not asserted: the sidebar at 120 is
the standing `tui.sidebar-auto-hide` decision and TUI-004 owns changing it.

**`compat/status-row.sh` exits 1 on this box, and the cause is the locale, not
the status row.** Two of eleven comparisons differ, `defaults` and
`status-left = [#{session_name}]` — the only two steps that still carry the
default `status-right`, which ends in `%d-%b-%y`. The rows are identical byte
for byte except the month token:

```
tmux: $'...\E[48;2;154;205;50m ... "rowtitle" 18:35 09-set-26'
zz:   $'...\E[48;2;154;205;50m ... "rowtitle" 18:35 09-Sep-26'
```

This box runs `LC_TIME=pt_BR.UTF-8` (`LANG=en_US.UTF-8`). The pin expands `%b`
through libc `strftime(3)`, which honours `LC_TIME`, and prints `set`. zz
expands it in `crates/zz-mux/src/formats.rs:4707` `format_datetime`, through
`chrono::format::StrftimeItems` and `format_with_items`, which is
locale-independent and always prints the English abbreviation. The control run
settles it: with `LC_ALL=C LC_TIME=C` the same fixture on the same binaries
exits 0 with all 11 comparisons identical.

Three consequences, none of them fixed here:

- It is a real divergence in zz, not a fixture artifact, and no gap in
  `compat/tmux-gaps.json` records it (`LC_TIME`, `setlocale` and `strftime`
  appear there only in unrelated prose). It needs a registry owner.
- The fix would land in `crates/zz-mux/src/formats.rs`, which this cycle's wire
  rule puts out of reach, so it was measured and left.
- It is a limitation of `compat/status-row.sh` as a baseline: the fixture pins
  the pane title but not `LC_TIME`, so its default-row comparison carries the
  caller's locale into the result. Pinning the locale would hide the divergence
  rather than record it, so this attempt names it instead of masking it.
