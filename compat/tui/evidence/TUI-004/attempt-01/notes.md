# TUI-004 attempt-01

Every file here is the stdout and stderr of a run that actually happened, on alienware, against the
binaries `environment.txt` names. Nothing here describes a check that did not run.

## Files

- `environment.txt` — both binaries' sha256, the worktree revision and its dirty files at the time of
  recording, the pin and its version, the OS line, TERM, shell, locale, and the sizes each fixture
  drives. The zz hash identifies the artifact; a cargo debug build of zz is not bit-reproducible on
  this box, so attestation is the revision plus a clean worktree and provenance is reproduction.
- `screen-diff-before-the-flip.txt` — `compat/tui-screen-diff.sh` run with the sidebar change in the
  binary but the fixture's 109 and 120 sizes still in `record` mode. This is the measurement that
  justified promoting them: every checkpoint at both widths reports `ok`, and the only notes left at
  those widths are the two standing records (`default-fg`, `colour-classes`).
- `screen-diff-asserted.txt` — the same fixture after the flip, with all five sizes asserted, the
  resize cases crossing the retired 109-column threshold in both directions, the new
  `120x24-sidebar` case and the new `styled-left-trim` record. 58 asserted checkpoints identical.
- `screen-diff-self-check.txt` — `--self-check`. Each sabotage caught in its own channel and each of
  the three equivalences reported nowhere, including the new `sidebar, focus-sidebar shown on one
  side` case.
- `pane-geometry.txt` — `compat/tui-pane-geometry.sh`, now asserting columns at 80, 100 and 120.
- `status-left-trim-probe.txt` — the divergence found while proving the decision, probed directly on
  both binaries with `display-message -p`: zz counts a `#[...]` section against `status-left-length`
  where the pin does not.
- `cli-status-tests.txt` — the four `crates/zz/tests/cli_binary.rs` status tests that read the raw
  TUI's own escape stream, after the threshold assertions were rewritten.
- `cursor-attributes-*.txt`, `screen-diff-tip-*.txt`, `pane-geometry-tip-*.txt`,
  `status-row-tip.txt`, `attached-client-tip.txt`, `unit-tests-tip.txt`, `clippy-tip.txt` — the
  at-tip re-runs; see `proofs-at-tip.txt` for the command list and exit codes.

## What the sidebar case proves, and what it cannot

`focus-sidebar` has no counterpart in the pin, so the case is the one place in the fixture where the
two sides are driven differently. It asserts BOTH directions: the zz screen must DIFFER from the
pin's while the sidebar is up, and must be identical to it again once the sidebar is withdrawn. The
first half is what makes the second worth anything — a sidebar that never drew would pass a case
that only asserted equality — and the same first half runs as a `--self-check` sabotage.

The command is reached through a user binding (`bind-key -n F8 focus-sidebar`), not through a
one-shot CLI invocation: `crates/zz-daemon/src/daemon.rs` answers a non-interactive client
`focus-sidebar requires an interactive client`. F8 rather than the default `prefix s` so that a lane
changing what `prefix s` means cannot change what this case measures.
