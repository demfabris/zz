# TUI-011 attempt-01, cycle 7

The remaining stock client command inventory: the roster, the comparison of
every entry on five channels, and the split into children.

## What is in this directory

- `environment.txt` — the box, the revision, both binaries and the pin, written
  before the runs.
- `run-1.txt`, `run-2.txt`, `run-3.txt` — three consecutive runs of
  `compat/tui-client-commands.sh` at 80x24 against the pin. Each ends
  `all 57 asserted comparisons identical, 37 recorded not asserted (0 for a
  sibling lane)`, exit 0. The recorded cases print their divergence in full, so
  every measurement quoted in the ledger is readable in these files: the pin's
  client tree is under `case client-tree-open`, the capture residues under
  `case capture-default-range`, `capture-preserve-trailing`, `capture-mode-screen`
  and `capture-alternate`, the stream refusals under `case stream-*`, and the
  server log under `case messages-log`.
- `self-check.txt` — `compat/tui-client-commands.sh --self-check`, exit 0. Four
  one-sided sabotages, each caught in its own channel: a buffer on the zz side
  only reported in stdout and not in stderr; showing that missing buffer
  reported in the exit status and stderr; a second session on the zz side only
  reported in the state with the screen silent; one space typed at the zz
  client's prompt reported in the screen and nothing else. Both equivalences,
  before the first sabotage and after the last is withdrawn, report nothing.
- `attached-client.txt` — `compat/attached-client.sh` at this tip, PASS.
- `before-the-landing.txt` — the same fixture run at BASE, before the
  current-client fallback. Eight refresh-client cases differ there with
  `no current client` on zz where the pin refreshed its one attached client;
  that is the run the landing answers.
- `corpus.txt` — `compat/run.sh --strict-geometry smoke/refresh-status
  smoke/client-resized-context smoke/display-message-client-aliases
  capture-pane smoke/buffer-standard-streams smoke/command-flag-errors`, every
  row clean. The same command over `census-hooks`, `smoke/positional-maximums`
  and `smoke/command-prompt-target` ran clean earlier in the attempt, before a
  header-only edit to the fixture.
- `cargo.txt` — `cargo test -p zz-daemon`, `cargo clippy -p zz-daemon
  --all-targets --all-features -- -D warnings` and `cargo test -p zz` at this
  tip, each exit 0.

## The roster

The roster itself is the header table of `compat/tui-client-commands.sh`: one
row per command and flag, with the pin behaviour, zz's disposition and which of
the three dispositions the entry takes. It is not copied here, so there is one
copy to keep true.

## The one landing

`cmd_find_client` with no `-t` is `cmd_find_current_client` (cmd-find.c:1274):
the invoking client when it has a session, then the best client of the session
the invocation came from, then the best client of the best session. zz carried
that rule in `resolve_client_target` and a stricter one in
`resolve_attached_client`, which `refresh-client` and `lock-client` use, so a
clientless CLI answered `no current client` against a server with one attached
client. Both resolvers now call one `current_client` helper. Nothing else in
this attempt changes behaviour.

## What did not land

choose-client. The measurement is taken and is in `run-*.txt` under
`case client-tree-open`; the blocker and the plan are in TUI-014. The one case
compat/tui-choosers.sh records for TUI-006 therefore stays recorded, and this
lane did not touch TUI-006's record.
