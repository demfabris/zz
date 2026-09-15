# TUI-018 attempt-01, stream lane, cycle 10

The caller stream forms, built as one bounded command-stream channel and measured against pinned
tmux d77c9dc6 on the Ubuntu box. Every file in this directory is a real run; nothing is edited
after the fact.

## Files

- `environment.txt` — the box, the toolchain, the pin's build stamp, the branch and the base
  revision. Written first.
- `cargo-test-zz-daemon.txt` — `cargo test -p zz-daemon`, 911 passed and 3 failed. The three are
  `russh_socks::tests::{loopback_forwards_http_and_tcp_in_both_families_with_original_port,
  loopback_shutdown_closes_connections_and_reconnect_rebinds_same_port,
  ssh_inventory_prepares_page_and_api_ports_then_refreshes_without_dropping_streams}`, all
  `ConnectionReset` on loopback. They fail the same way when the cycle-9 gate worktree's own
  prebuilt test binary is run, which carries none of this branch, so they are this box today and
  not this lane. This branch touches no SSH or SOCKS path.
- `delta-corpus1.txt` … `delta-corpus4.txt` — `compat/run.sh --strict-geometry` over the delta
  corpus for the touched commands, in four chunks. Chunks 1 to 3 are clean (the two `known/` rows
  keep their approved GEO divergences). Chunk 4 ran twenty more rows clean and was cut off by the
  600-second call cap while the rows left were format and layout rows that name none of the touched
  commands.
- `streams-run-1.txt`, `streams-run-2.txt`, `streams-run-3.txt`, `streams-self-check.txt` —
  `compat/tui-command-streams.sh` three times and its sabotage run, at the final tip.
- `client-commands-run-1.txt` … `client-commands-run-3.txt`, `client-commands-self-check.txt` —
  the same for `compat/tui-client-commands.sh`, whose four stream cases flipped from `record` to
  `same`.
- `neighbour-fixtures.txt` — the four fixtures whose channel the cursor landing could move
  (`tui-screen-diff.sh`, `tui-copy-mode.sh`, `tui-overlays.sh`, `tui-indicators.sh`), each green.
- `attached-client.txt` — `compat/attached-client.sh`, red, and the measurement that shows it is
  not this lane's.

## The zone excursion

`crates/zz-tui/src/render.rs` is named as another lane's file, and this landing takes six lines of
`place_viewport_cursor` anyway. The measurement that forced it: with the empty pane carrying the
pin's screen mode, `split-window -I` puts the pin's client cursor at `19,12 flag=0` — the pane's
own cell, hidden — and put zz's at `0,22 flag=0`, because the raw TUI emitted DECTCEM-off and never
moved the cursor, leaving it where the frame's last paint ended. `compat/tui-client-commands.sh`
asserts that cursor for `stream-split-window`, so the case could not pass without it, and the fix
is not reachable anywhere else: `place_viewport_cursor` is the only place the raw TUI decides where
a pane's cursor goes. The pin's own rule is `tty_update_mode` turning the mode off while
`tty_cursor` still moves, so the change makes zz do what was measured, not what was guessed. The
four neighbouring fixtures were run afterwards to show the change moves nothing else.

## The one decided difference

The cap. `MAX_AGENT_SEND_BYTES` is 1 MiB and the reader refuses a longer stream before the daemon
sees a byte; pinned tmux has no total limit. Measured both ways: a payload exactly at the cap is
identical on both sides for `load-buffer -` and `source-file -`, and a payload one byte over is
refused by zz and accepted by the pin. Bulk file transfer through a command client is a workload zz
does not serve, because an unbounded stream lets one caller grow daemon memory without limit;
decided 2026-09-14 by the orchestrator under fabrico's TUI parity contract of 2026-09-09;
reversible.

## History

TUI-018 split from TUI-011 on 2026-09-13 with the three forms refused: `source-file -` answered
`source-file from standard input is not supported`, `display-message -I` and `split-window -I`
answered `unsupported command: … -I`. fabrico approved the full scope on 2026-09-14 — build the
channel, do not carry only the cheap text forms — and that is what this attempt does.
