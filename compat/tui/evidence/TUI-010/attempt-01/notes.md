# TUI-010 attempt-01

Prolonged terminal backpressure, recovery, and client lifecycle. Every file here
is the stdout and stderr of a run that happened on this box on 2026-09-09;
`environment.txt` names both binaries, the pin and the box.

## The measurement, before and after

The registered reproduction (`tui.client-output-queue-budget`) says: past the
4 MiB writer budget the raw TUI's paint loop parks, and a client whose terminal
has read nothing for 45 s stops answering keys, while pinned tmux answers in
0.050 s having spent no memory. Measured here with
`compat/tui-output-backpressure.sh`, whose relay is a pty that stops reading on
command, at 80x24, with a 45 s undrained window:

| side | drained key | key after 45 s undrained | RSS baseline | RSS peak | growth |
| --- | --- | --- | --- | --- | --- |
| pinned tmux d77c9dc6 | 0.024 s | 0.033 s | 4892 kB | 4892 kB | 0 kB |
| zz BEFORE (origin/main bfd05821) | 0.023 s | never, 12 s timeout | 105956 kB | 112100 kB | 6144 kB |
| zz AFTER | 0.024 s | 0.025 s | 105244 kB | 111816 kB | 6572 kB |

Memory was bounded on both sides before and after: the 4 MiB queue was never the
problem, the wait on it was. What changed is liveness, and it now matches the
pin's own column in the same run.

## Files

- `environment.txt` — binaries, pin, box, locale, sizes.
- `01-before-prefix-binary-undrained-45.txt` — the fixture at its final version
  against a build from BEFORE the change (`target/debug/zz-prefix-main`, the
  origin/main artifact kept aside): exit 1, `undrained key did not act within
  1.000 s (got none)`. This is the sabotage that matters most: the fixture goes
  red against the behaviour the registry recorded.
- `02-after-zz-undrained-45.txt` — the same run against the fixed binary, exit 0.
- `03-both-undrained-45.txt` — the full fixture, both sides, exit 0: nine
  assertions, three records.
- `04-prefix-binary-final-screen.txt` — the pre-change binary through the whole
  fixture, kept for one reason: the status row's spelling difference is byte for
  byte the same before the change as after it, which is what puts that record
  outside this diff.
- `05-self-check.txt` — `--self-check`, exit 0: a paint written after the
  restoration is caught in both the screen channel and the escape-tail channel,
  and the response bound fires when the deadline is halved to below the latency
  just measured.
- `06-smoke-input-backpressure.txt` — `compat/run.sh --strict-geometry
  smoke/tui-client-input-backpressure`, exit 0, still clean.
- `07-tui-screen-diff.txt` — `compat/tui-screen-diff.sh`, exit 0, all 33
  asserted checkpoints identical: the drop path did not move the screen.
- `08-tui-pane-geometry.txt` — `compat/tui-pane-geometry.sh`, exit 0.
- `09-attached-client.txt` — `compat/attached-client.sh`, exit 0, with the three
  lifecycle cases this obligation adds. `--lifecycle` runs those three alone.
- `10-attached-client-sabotage-sizes.txt` — window-size pinned to `smallest`
  before the default-rule case: the fixture goes red at
  `latest simultaneous sizing follows the second client ... last output: 70x20`,
  so the case really does tell `latest` from `smallest`.
- `10-attached-client-sabotage-readonly.txt` — the second client attached
  WITHOUT `-r`: red at `pane unexpectedly contained ATTACHED_READONLY_TYPED`,
  so the input refusal is asserted and not assumed.
- `10-attached-client-sabotage-reattach.txt` — the client comes back looking at
  a pane it was not on: red at `reattach selected target after cycle 0 ... last
  output: attached:0.1`.
- `11-transport-recovery.txt` — the server killed under both clients.
- `12-zz-tui-zz-terminal-tests.txt`, `13-clippy-zz-tui.txt`,
  `14-zz-integration-tests.txt` — the crate tests, the lint and the `zz`
  integration tests at the tip.

## What the change is

`crates/zz-tui/src/writer.rs` past `QUEUE_BUDGET` used to park the paint loop on
a condvar until the terminal drained. It now does what `tty.c tty_block_maybe`
does: drop the whole queue, keep dropping for `BLOCK_INTERVAL` (the pin's
100 ms), and clear the block on the first interval that dropped less than
`BLOCK_STOP`, which keeps the pin's 64:1 ratio between the threshold that starts
a block and the one that ends it. Clearing it asks for a repaint the way
`tty_timer_callback` sets `CLIENT_ALLREDRAWFLAGS` and calls `tty_invalidate`:
`crates/zz-tui/src/render.rs` throws away its painted state on a drop, so the
next paint is a full one drawn from the model, and `crates/zz-tui/src/app.rs`
takes a `MainEvent::Repaint` from the writer's timer so the screen is right at
the moment the terminal starts reading rather than at the next event.

The budget itself did not move. The registry's own acceptance asked for a drop
path rather than a larger budget, and the before-and-after above shows the
budget was never what cost the client its liveness.

One thing the pin gets wrong and this does not: bytes the pin drops while
blocked include mode changes, which no repaint reproduces. `flush_output` keeps
the control bytes a dropped paint carried and puts them back at the front of the
next one, so a kitty transmission or a mouse-mode change survives a block.

## Clause 3, and one premise it corrected

The brief said the pin sizes a window to the smallest of its attached clients.
It does not, by default: `options-table.c` gives `window-size` the default
`WINDOW_SIZE_LATEST`, so an untouched window follows the most recently used
client and `smallest` is one of four rules you have to ask for. The probe uses
90x20 and 70x30 on purpose, crossed, so `smallest` is 70x20 and `largest` is
90x30 and neither answer is a client that exists: a rule that just picked one of
the two clients would pass a probe with 90x30 and 70x20 and fail this one.
Measured on both binaries and identical on both, under every rule.

Read-only is asserted in all four parts the pin's own contract has (`tmux.1`:
"only keys bound to the detach-client or switch-client commands have any
effect"): typed text never reaches the pane, a bound `run-shell` never runs, a
bound `switch-client` does, and the screen keeps up with the session. Two things
that run had to teach it:

- The live-screen check has to come BEFORE any refused key. A refusal puts
  `Client is read-only` on the message line, and `status.c status_message_set`
  sets `TTY_FREEZE` on that client's terminal for the whole of `display-time`,
  which earlier probes in this file leave at 20 seconds. With the order
  reversed the check waits for a marker the pin is deliberately not painting.
- Driving the pane with `send-keys` has to name the read-write client with
  `-c`. `cmd-send-keys.c` refuses when the client it resolved is read-only, and
  with a read-only client attached the one `cmd_find` picks for a command-line
  caller can be that one.

Neither is a divergence; both are the pin being precise, and both would have
been a flaky fixture rather than a finding if the first green run had been
trusted.

Transport recovery is two things here. The one the cycle asserts is a detach the
server starts rather than the client, `detach-client` against the tty, which is
the transport closing under a client that did not ask for it: the reattach
cycles alternate that with a client-key detach and assert the same four facts
after each. The one that is measured and recorded is the server dying:
`11-transport-recovery.txt` kills it under both clients and both print
`[server exited unexpectedly]` and exit 1, the pin within 0.05 s and zz within
0.1 s. Repeating that measurement needs one thing said out loud: the zz daemon
forks, so the process that gets started is a launcher and the daemon that owns
the socket is its child. Kill the launcher and nothing happens, which is what
the first attempt at this measurement did before the pid was taken from the
command line that names the socket.

## Recorded, not asserted

- The cursor's shape, blink and colour after recovery: zz writes DECSCUSR and
  OSC 12, the pin writes neither. Standing, owned by the canvas lane this cycle.
- The status row after recovery: both binaries paint the same characters in the
  same colours, and zz re-states the default foreground after the reset that
  ends the window name where the pin states only the background. This is the
  recorded explicit-default-foreground divergence of the daemon's frame
  representation, which has no owner this cycle. Measured on both sides of the
  change (`04-...` versus `03-...`) and identical in both, so it is not
  something recovery introduced. Worth naming for whoever picks the record up:
  `compat/tui-screen-diff.sh` is green at 80x24 and does not see this, because
  its checkpoints are incremental paints and this spelling appears on a FULL
  repaint of the status row.
- The pin's client never leaves an alternate screen at all (`restores=0` in
  every pin run here), so the escape-tail assertion has no restoration point to
  be after on that side and the fixture records the tail instead. zz leaves one
  (`restores=1`) and its tail after it is the 40-byte `[detached (from session
  backpressure)]` line, with zero escape bytes.
