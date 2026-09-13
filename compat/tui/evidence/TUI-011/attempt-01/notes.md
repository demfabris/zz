# TUI-011 attempt-01, cycle 7

The remaining stock client command inventory: the roster, the comparison of
every entry on five channels, and the split into children.

## What is in this directory

- `environment.txt` — the box, the revision, both binaries and the pin. Retaken
  at the gate: it now records the revision the runs were actually taken at and a
  clean worktree, which is the only thing that attests which binary produced
  these files on a box where a cargo debug build is not bit-reproducible.
- `run-1.txt`, `run-2.txt`, `run-3.txt` — three consecutive runs of
  `compat/tui-client-commands.sh` at 80x24 against the pin, retaken at the gate
  on the rebased tip with the `hooks-show-target` case added. Each ends
  `all 58 asserted comparisons identical, 37 recorded not asserted (0 for a
  sibling lane)`, exit 0. The three files are not byte-identical and are not
  meant to be: the only lines that move are inside `case messages-log`, whose
  server-log rows carry a per-run client pid and a wall-clock minute. That case
  is recorded, not asserted, and it is TUI-016's. The recorded cases print their divergence in full, so
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
- `before-the-landing.txt` — the committed fixture, at this same revision, with
  `crates/zz-daemon/src/daemon.rs` alone reverted to origin/main and zz rebuilt:
  the tip minus the current-client landing and nothing else. It runs to
  completion and exits 1 with `9 of 58 asserted comparisons differ`. NINE, not
  eight. Eight are refresh-client — `refresh-bare`, `refresh-status`,
  `refresh-flag-set`, `refresh-flag-clear`, `refresh-flag-restore`,
  `refresh-control-pane`, `refresh-control-subscribe`, `refresh-control-size` —
  and the ninth is `lock-client-current`, where a clientless `lock-client` with
  no `-t` answered `no current client` too. A later lane touching lock-client
  should know it was in this set.
  (The file the worker committed under this name was a different, earlier
  revision of the fixture — it printed DIFF for `capture-escape` and
  `stream-source-file-effect`, which are `record` cases at the tip and can never
  print DIFF — and it aborted at `switch-mode-closed` rather than reaching a
  summary. It has been replaced by the run described above.)
- `corpus.txt` — the worker's own corpus run, taken before the rebase:
  `compat/run.sh --strict-geometry smoke/refresh-status
  smoke/client-resized-context smoke/display-message-client-aliases
  capture-pane smoke/buffer-standard-streams smoke/command-flag-errors`, every
  row clean.
- `gate-corpus.txt` — the gate's own delta corpus, all 145 scenarios
  `--delta origin/main..HEAD --commands refresh-client,lock-client` selects,
  each run once under `--strict-geometry`.
- `gate-fixtures.txt` — every TUI fixture at the gate tip, with its exit code
  and summary line.
- `gate-status-background-jobs.txt` — the one corpus row that is red at this
  tip, measured on both sides of the landing.
- `cargo.txt` — the cargo record, retaken at the gate. The file the worker
  committed showed zz-daemon's suite as a single
  `test result: ok. 0 passed; 0 failed` line, which reads as a test binary that
  never ran; it was a filtered capture, not an empty run. It now keeps every
  target line and every `test result:` line, so the lib target's 893 passing
  tests are visible, alongside the workspace clippy and the build.

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

Measured at the gate by reverting daemon.rs alone: NINE cases move from
differing to identical, not eight. The eight refresh-client cases, and
`lock-client-current`.

## What did not land

choose-client. The measurement is taken and is in `run-*.txt` under
`case client-tree-open`; the blocker and the plan are in TUI-014. The one case
compat/tui-choosers.sh records for TUI-006 therefore stays recorded, and this
lane did not touch TUI-006's record.
