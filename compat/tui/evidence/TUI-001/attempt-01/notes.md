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

## The recorded timeout

The ledger recorded an exploratory run on 2026-09-09 that exited 2 on `zz
geometry report did not happen within 10 seconds`, on macOS, with an existing
`target/debug/zz` that was neither rebuilt nor revision-attested.

That macOS run cannot be re-run on this box, so this attempt explains it rather
than reproducing it, which is what the clause allows. The explanation rests on
four measurements, not on a narrowing:

1. **The fixture and the runtime at `576b6b74` are sound here.** A zz built in
   this worktree, sha256
   `3cb6e28437cd028b49c2c2eb07c4414a69226518bf2f287e7c52123d5bf70a38`, passes
   `compat/tui-pane-geometry.sh` six times out of six across two shapes of the
   fixture, in 4 to 5 seconds a run against a 10 second bound. The wait that
   fired in the record is the one at `measure()`, and it is nowhere near its
   bound at this revision.

2. **The wait that fired names the input path, and the fixture the closed gap
   cites is the same one.** `tui.client-input-backpressure` (closed 2026-09-07,
   commit `0bed7fe7`) lists `file:compat/tui-pane-geometry.sh` among its
   evidence. Its recorded cause is exact: `crates/zz-tui/src/render.rs`
   `flush_output` took `io::stdout().lock()` and did a blocking `write_all` plus
   `flush` on the main event loop, the same loop that drains the `MainEvent`
   channel the stdin reader thread feeds, so a client whose own terminal output
   backed up stopped acting on input entirely — measured at `012b4dcc` as "did
   not act at all within the fixture's 10 second bound", against 0.020 s on the
   pin.

3. **The timeline puts a stale binary on the wrong side of that fix.** The
   fixture reached its current shape at `314c55e0` (2026-09-07T00:15:42-03:00);
   the backpressure fix landed at `0bed7fe7` (2026-09-07T13:31:08-03:00),
   thirteen hours later, and did not touch the fixture. A `target/debug/zz` left
   over from anywhere in that window, or before it, runs the current fixture
   with the stalling client. That is what "not rebuilt or revision-attested"
   admits.

4. **The named cause is measurably absent at this tip.**
   `compat/run.sh --strict-geometry smoke/tui-client-input-backpressure` exits 0
   with 0 divergences here, so the mechanism that would produce that timeout is
   not present in the binary this attempt attests.

What this explanation does **not** establish: it does not measure the macOS pty
write buffer that would decide how quickly a pre-fix client's `write_all` parks,
and it does not identify the exact revision of the binary that timed out,
because that binary was overwritten. `/home/demfabris/dev/zz/target/debug/zz`
was rebuilt on this box at 18:25 on 2026-09-09, before this attempt started, so
it is not the artifact from the record and its hash proves nothing about it. The
falsifier is stated plainly: build a zz at `0bed7fe7^` on macOS and run this
fixture. If it passes there, the explanation is wrong.

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
