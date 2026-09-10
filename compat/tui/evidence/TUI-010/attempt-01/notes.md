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
- `09-attached-client.txt` — `compat/attached-client.sh`, exit 0, with the
  lifecycle cases this obligation adds.
- `10-attached-client-sabotage-*.txt` — one deliberately broken expectation per
  added lifecycle case, each caught by the case that owns it.
- `11-zz-tui-tests.txt`, `12-clippy.txt`, `13-zz-integration.txt` — the crate
  tests, the lint and the `zz` integration tests at the tip.

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
