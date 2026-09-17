# TUI-018 sixth pass

Base: campaign/tui-stream-alias-5 at 3c4a5255, which merges cleanly into origin/main 38c50df4. The
four findings of the sixth review are built, none is recorded. The one wire change is a tail append; see the tip section for the 105 bump. No environment dump was taken.

## What changed

1. Spent PaneInput readers continue. A later `display-message -I` or `split-window -I` in a direct
   command sequence now treats the spent stream (the client's EBADF answer) as empty input and runs
   the rest of the sequence, as the pin's `window_pane_input_callback` does on any error.
2. PaneInput streams. `ClientFileOperation::ReadStdinChunk` is appended after `ReadStdin`. The
   daemon asks for one chunk of at most 16 KiB, feeds it to the pane's terminal actor through its
   one-slot command queue, and asks for the next only after the actor took the previous one; the
   client reads fd 0 only when asked, so a fast writer blocks in its pipe. Delivered chunks stay after
   SIGTERM or a read error. Two neighbouring defects had to go for screen and cursor to match: a
   PTY-free pane's terminal now takes the pane's laid-out size before a chunk is fed (it stayed
   80x24 in a detached session), and the output-view actor refreshes `#{cursor_x}`, `#{cursor_y}` and
   `#{history_size}` after each fed chunk (they stayed 0). Both reproduce on the 3c4a5255 binary
   (probes/cursor-base.txt, probes/geom-base.txt).
3. Unreadable stdin. Both read shapes duplicate fd 0 and read the duplicate, so Rust's `Stdin` no
   longer turns EBADF into end of file, and every read error maps to the pin's `Input/output error`.
   `source-file -` and `load-buffer -` report `Input/output error: -` once, raise the exit status to
   1 and resume the queue; later readers report `Bad file descriptor: -`. PaneInput readers continue
   silently. Write-only and directory stdin are measured for all four readers.
4. Alias shell guards. `execute_control_command_with_guard` publishes a `run-shell` (without `-C`)
   guard before its event capture begins and publishes only the captured events afterwards, the
   same shape file replay already had. SIGTERM during the wait in `-C` and attached `-CC` now leaves
   the pair in the transcript, before and after the reader.

The 1 MiB decision: the pin accumulates `source-file -` and `load-buffer -` without a total bound
and parses pane input chunk by chunk, draining each. The cap stays on the two sinks that hold their
payload, so the five decided cells keep their meaning; a pane stream has a per-chunk bound only, and
`stream-display-fast` asserts a 1.26 MiB pane stream. Recorded in
knowledge/designs/command-stream-channel.md, the roadmap and `protocol.binary-streams`.

The pre-main constructor: the desktop `zz` binary was built at the candidate source
(build-gui.txt). Its CLI surface answers exactly like `zz_cli` for closed, piped and write-only stdin
and for a streamed pane (probes/gui-binary.txt). The window itself was not launched on this box. The
constructor only stores a flag, `caller_stdin` is its only reader, and `CommandClient::enable_stdin`
has one call site, in the CLI command path.

## Proof

All counted runs use `target/alias6-bins/zz_cli-final` (sha256 in binary-hashes.txt, built from
613ef711 and unchanged through e3e6b003) and the scrubbed environment.

- `compat/tui-command-streams.sh`, three runs (streams-1/2/3.txt): 201 asserted comparisons
  identical, 0 recorded, 5 decided, exit 0 each. The 45 new cells: 9 delivery cases
  (`stream-display-{partial,eof,term,slow,fast,utf8}`, `stream-split-{partial,term,utf8}`) and 36
  matrix rows (6 spent pane readers, 24 write-only or directory stdin across source, buffer, display
  and split, 6 alias or file shell guards under SIGTERM in `-C` and `-CC`).
- `--self-check` (self-check.txt): 175 expectations met, both equivalences, exit 0. Every new case
  has a sabotage in the channel its fix moves: delivery cases withhold, drop or corrupt the bytes the
  zz side writes (state), spent and unreadable pane rows change the sampled state, source and buffer
  rows swap the reader for a missing file (stderr), guard rows drop one guarded command (stdout).
- Fix reverted: the same 51 filtered cells against the 3c4a5255 binary
  (new-cases-baseline-3c4a5255.txt) fail 33, which is every new cell except 12 that already matched
  the pin: alias and file spent pane readers, write-only pane readers, and file-replay shell guards.
  Those 12 stay as regression guards.
- `compat/tui-client-commands.sh` (client-commands.txt): 198 asserted, 32 recorded, owners
  `TUI-014=6 TUI-015=3 TUI-017=4 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6
  gap:clients.interactive-refresh=8 unattributed=0`, no TUI-018 record, exit 0.
- `compat/attached-client.sh` (attached-client.txt): PASS.
- Probes (probes/): each finding on the 3c4a5255 binary and the final binary, plus run-shell guard
  edges (`-t` missing pane, nonzero exit, invalid delay, output) that stay identical to the pin.
- Delta corpus (`--commands source-file,load-buffer,display-message,split-window,run-shell,send-text,agent-send`,
  220 rows, 2531 steps, three shards): 210 zero-divergence rows, 3 registered `known/` rows, 7 rows
  red twice. lane2-store, show-options-hooks, smoke/cli-chain-parse-abort,
  smoke/default-client-command and smoke/plugin-runtime-resurrect-restore fail with identical diff
  lines on the 3c4a5255 binary (candidate-comparisons/ against base-comparisons/). micro-flags fails
  on both with the same locale difference, `17-Sep-26` against the pin's `17-set-26`; only the clock
  minute differs. smoke/status-background-jobs fails at the `drawn DATE changes at seconds 1, 3, 5
  and 7` checkpoint on both binaries: over eight alternating trials the candidate is red twice 3
  times and the 3c4a5255 binary once, first attempts fail 5 and 4 times
  (status-background-jobs-trials.txt). It is timing-sensitive on both and attempt-10 recorded the
  same checkpoint on fresh main; the rate difference is within this sample's noise but is not
  claimed to be zero. No row is classified as caused here.
- Tests (scrubbed home, `--test-threads=4`): zz-protocol 231 lib plus 23 integration pass; zz-daemon
  lib 984 pass, `agent_stream_soak_slow_client` fails 1501 against 1500 exactly as attempt-10's
  fresh-main run and fails the same way alone twice; zz-terminal lib 285 pass, 1 ignored; zz-cli lib
  107 pass, cli_binary 134 pass and 2 fail. `wait_exit_holds_the_control_process_until_a_second_blank_line`
  stalled and passes 3 of 3 alone. `control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`
  races a `%window-renamed @0 cat` notification between the sourced guard and the child output: the
  3c4a5255 binary passes it 2 of 8 times under the same test binary, the final binary 0 of 5, and
  ten alternating trials each pass the 3c4a5255 binary once and the final binary twice
  (test-sourced-run-shell-alternating.txt). A direct reproduction shows the same transcript order on
  both binaries.
- `clippy -p zz-protocol -p zz-terminal -p zz-daemon -p zz-cli --all-targets --all-features -D warnings`,
  `cargo fmt --all -- --check`, `compat/check.sh`, `python3 compat/tui/tracker.py check`,
  `python3 compat/wire-version.py` (104 unreleased) and the OKF validator pass.

Residual found while measuring, not in this batch: `zz -C cmd ; run-shell 'sleep 1' ; tail`
with stdin at EOF exits after the run-shell guard, where the pin runs the queue to the end
(probes/f4d-*.txt, identical on the 3c4a5255 binary).

## The tip after zz 0.11.0

origin/main moved while this pass ran: 066b862a released zz 0.11.0, and that release shipped
`PROTOCOL_VERSION` 104, the number this branch had been appending to. `python3 compat/wire-version.py`
now says a release shipped 104, so the branch merges origin/main (no file is touched on both sides)
and moves this cycle's three appends (`CommandInvocation.stdin_available`,
`ClientFileOperation::ReadStdin` and `ReadStdinChunk`) into a v105 entry, with both pins and the
`0x69` hello bytes updated. The proof above is at e3e6b003; at the merged tip 4bb7a2ca, with
`target/alias6-bins/zz_cli-tip` (tip-binary-hash.txt), the fixtures were re-run:
`compat/tui-command-streams.sh` 201 asserted, 0 recorded, 5 decided (tip-streams.txt);
`compat/tui-client-commands.sh` 198 asserted with `unattributed=0` (tip-client-commands.txt);
`compat/attached-client.sh` PASS (tip-attached-client.txt); zz-protocol tests 231 plus 23
(tip-test-zz-protocol.txt); `compat/check.sh` including `evidence-secrets` and
`wire-version: 105 is unreleased` (tip-compat-check.txt). The delta corpus, the self-check and the
other packages' tests were not re-run at the tip: between e3e6b003 and the tip the only source change
is the protocol number and main's five commits, none of which touches a file this branch touches.

`cargo fmt --all -- --check` fails at the tip on `crates/zz/src/terminal/view.rs`
(tip-fmt-check.txt), which is byte-identical to origin/main and is not this branch's file; the
pre-merge check on this branch's own files passed (fmt-check.txt).

## Failed and superseded runs

- new-cases-first-run.txt and fast-case-history-diff.txt ran an intermediate candidate: the fast
  case failed on pane geometry, then on `#{history_size}` past history-limit, which is
  `semantic:history-limit-product-default` (zz 685 retained rows, pin 1939). The fast sample now
  reads the screen and cursor only; the other shapes still read history.
- delta-shard-*-aborted-no-sibling-cli.txt were stopped because the prebuilt binary had no `zz_cli`
  beside it.
- streams-unscrubbed-env-*.txt and delta-shard-*-unscrubbed-env-stopped.txt were started before the
  orchestrator's credential hygiene rule and stopped when it arrived; every counted run afterwards
  used the scrubbed environment. The first unscrubbed stream run had passed (201 asserted).
- test-zz-cli.txt: `wait_exit_holds_the_control_process_until_a_second_blank_line` stalled for 54
  minutes and its control child was killed (wait-exit-intervention.txt), the intermittent residual
  attempt-10 recorded.
- new-cases-*.txt, test-focused.txt, clippy.txt, build-final.txt and the probe outputs other than
  gui-binary.txt were produced before the hygiene rule. They hold fixture verdicts, test
  names and control transcripts only; `python3 compat/evidence-secrets.py` scans them with the rest.
