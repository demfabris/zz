# TUI-014 attempt-03, cycle 10, modes lane

The batch was clause 2 and clause 3: clock-mode, switch-mode, customize-mode,
suspend-client and server-access. **One of the five landed — server-access.** The
other four are measured here against the pin and not built, so TUI-014 stays
`active` and compat/tui-client-commands.sh still records four of its cases.

## What landed

`server-access` left `UNIMPLEMENTED_TMUX_COMMANDS` and is answered in
`crates/zz-mux/src/command.rs` in `cmd_server_access_exec`'s own order: `-l`, then
the missing argument, then `getpwnam`/`getgrnam`, then the owner test, then `-a`
with `-d`, then `-r` with `-w`, then `-d`. `-l` prints `<owner> (U,W)`, which is
what `server_acl_init` leaves on a fresh pin server: root is skipped by
`server_acl_display` and the owner carries no `SERVER_ACL_READONLY`.

The product decision, with the pin's measured behaviour beside the old one, is in
TUI-014's `evidence_note` and in `protocol.socket-acl`'s `reason` in
`compat/tmux-gaps.json`. The one residue is the one the gap exists for: `-a`, `-r`
or `-w` naming a second identity stores an entry on the pin and exits 0, and zz
refuses it. `command:server-access` is **closed** with a dated measurement — the
registry forces that, `crates/zz-mux/src/compat_manifest_tests.rs` asserts an
implemented command has no open item — and `semantic:multi-user-socket-acl`
**stays** as the permanent exclusion, recorded as the new fixture case
`server-access-add`.

Nothing closed in `commands.native-client-tools` or `clients.interactive-refresh`.

## Why the other four did not land

Not budget alone. clock-mode, customize-mode and switch-mode each need a
server-owned pane mode that zz does not have:

- `#{pane_in_mode}` and `#{pane_mode}` are answered from copy mode alone
  (`crates/zz-daemon/src/status.rs:1655` and `:1673`), so the clientless
  `list-panes -a -F '…#{pane_in_mode}/#{pane_mode}…'` the fixture's state channel
  reads can never report one of these modes.
- every chooser zz has is keyed by `ClientId` with a `source_pane` inside
  (`inner.choose_trees`, `inner.choose_buffers`), never by `PaneId`, so there is no
  per-pane surface to publish to every client attached to that pane's window.
- there is no key route that ends a pane mode on any key the way
  `window_clock_key` does.

suspend-client needs a client lifecycle state between attached and exited: an event
that makes the raw TUI restore its tty and stop itself, a resume that re-arms it,
and a daemon flag that drops the client from `list-clients` and `session_attached`
while its socket stays open.

The lane took server-access first, out of the punch list's order, because it is the
only one of the five whose two recorded cases are pure CLI channels and could
therefore be built and proved without any of that machinery. Starting a vertical
slice that could not be finished *and* proved inside the batch would have left
nothing asserted.

## The pin measurements, so the next lane re-derives nothing

All taken 2026-09-15 against `compat/.cache/tmux-src/tmux` (d77c9dc6, next-3.8),
through an outer pinned tmux driving an inner pinned tmux attached at 80x24 — the
same driver shape compat/tui-client-commands.sh uses. The inner pane is 80x23 under
one status row. `pin-clock-mode.txt`, `pin-customize-mode.txt`,
`pin-switch-mode.txt`, `pin-suspend-client.txt` and `pin-server-access.txt` are the
raw captures; the readings are in TUI-014's `evidence_note`.

The one trap worth repeating here: **`capture-pane -M` does not show the clock.**
`window_clock_mode` has no `.get_screen`, so `cmd-capture-pane.c` falls back to
`wp->base`. The only comparison surface is the attached client's screen through the
outer pinned tmux.

## Which tip the runs sit at

Every run below was taken at **5585cd6a**, the last commit that changes a source, a
fixture, a registry or a generated report byte. The commit that carries this
directory adds only evidence and the two generated reports, so no run here is stale
against anything it measures. `verify-claims.py` and `compat/check.sh` were re-run
once more at the final tip after that commit and both exited 0; their committed
copies are the 5585cd6a runs.

## Files

- `environment.txt` — the box, the pin, the revision, the binary's sha256, RUN_ENV.
- `client-commands-run-1/2/3.txt` — compat/tui-client-commands.sh three times at the
  final tip, each `all 71 asserted comparisons identical, 36 recorded not asserted
  (0 for a sibling lane)`, exit 0.
- `client-commands-self-check.txt` — `--self-check`, exit 0, seven ok lines
  including the new `stdout, an access entry on the pin only`.
- `choosers-run-1/2/3.txt` — compat/tui-choosers.sh three times, each `all 78
  asserted comparisons identical, 0 recorded not asserted (0 for a sibling lane)`,
  exit 0. Clause 1 is untouched by this lane and still asserts whole.
- `choosers-self-check.txt` — `--self-check`, exit 0, thirteen ok lines.
- `screen-diff.txt` — compat/tui-screen-diff.sh, `all 147 asserted checkpoints
  identical, 6 recorded not asserted`, exit 0.
- `attached-client.txt`, `attached-client-retry-1.txt`, `attached-client-retry-2.txt`
  — three reds at this tip under a load average of 17 with three other lanes on the
  box, each in a different timing-sensitive probe this diff cannot reach: the menu
  underlay, the popup underlay, and `ATTACHED_NAV_65 ATTACHED_NAV_MATCH`, which is
  the real zz bug fixed by fbeb4aa4 on origin/campaign/box-reds and landing on main
  with the cycle 9 gate. Kept rather than dropped.
- `attached-client-retry-3.txt` — the fourth run, taken after the cargo work at a
  load average of 27, red on the same `ATTACHED_NAV_65` probe. Four runs, three
  probes, no diff of this lane's in any of them.
- `corpus-catalog-rows.txt` — compat/run.sh --strict-geometry over
  command-flag-errors, cheap-flags, positional-maximums, positional-minimums,
  cli-chain-parse-abort and config-discovery, exit 0.
- `corpus-parse-rows.txt` — the same over args-parse-choosers, args-parse-set-option,
  config-grammar, config-chain-parse-abort, cli-output-bytes and lane2-store (193
  steps), exit 0.
- `cargo-zz-protocol-zz-mux.txt`, `cargo-zz-daemon.txt`, `cargo-zz.txt` — the test
  runs, exit 0 each. zz-daemon ran with an empty HOME and `--skip russh_socks`.
- `clippy-zz-protocol-zz-mux.txt` — exit 0.
- `clippy-zz-daemon.txt` — exit 101 on seven pre-existing errors, every one in
  `crates/zz-daemon/src/russh_prompt.rs`, a file this diff does not touch. They are
  main's at base 287e3815 and are repaired on origin/campaign/box-reds.
- `check.txt` — RUN_ENV compat/check.sh from this worktree.
- `verify-claims.txt` — RUN_ENV compat/tui/verify-claims.py --run TUI-014.
- `pin-clock-mode.txt`, `pin-customize-mode.txt`, `pin-switch-mode.txt`,
  `pin-suspend-client.txt`, `pin-server-access.txt` — the raw pin captures above.
- `pin-probe.sh`, `pin-suspend-probe.sh`, `pin-server-access-probe.sh` — the three
  throwaway drivers that took those captures, kept so they can be re-run verbatim.
  Each builds its own outer and inner pinned tmux on a short /tmp socket with an
  isolated HOME and XDG_CONFIG_HOME, loads `-f /dev/null`, and reaps everything it
  started from a trap.

One note on the binary: `cargo test -p zz` relinked `target/debug/zz` at 12:25,
between the fixture runs above and `verify-claims.txt`, so `environment.txt` carries
two sha256s for the same revision and the same sources. verify-claims.py re-ran both
of this obligation's fixtures against the second binary and read the same two
tallies, which is what makes the pair harmless rather than a hole.

## Count churn a reader will meet

Implementing a command moves it out of `UNIMPLEMENTED_TMUX_COMMAND_SPECS` into
`COMMAND_SPECS`, and eleven hard-coded totals follow it: `catalog.rs` (implemented
85, flag shapes 521 / none 293, supported 491, usage overrides 21, unimplemented
specs 7), `compat_manifest_tests.rs` (rows 85, implemented 74, unimplemented 6),
`hunt_claims.rs` (`COMMAND_SPECS` 80), `daemon.rs` (specs 85, positional specs 74,
spellings 159, diagnostic cases 636, prefix cases 532) and the corpus fixture
`compat/scenarios/smoke/fixtures/command-flag-errors.{tsv,sh}` (canonical 85,
failure probes 522, probe count 525, sentinel `clean:525`).

`daemon.rs`'s tmux-import test used `server-access \` + ` -a user` as its multi-line
unsupported construct and now uses `link-window`, which is still unsupported, so the
continuation-line behaviour it tests is unchanged.
