# TUI-004 attempt-01

Every file here is the stdout and stderr of a run that actually happened, on alienware, against the
binaries `environment.txt` names. Nothing here describes a check that did not run.

Start at `proofs-at-tip.txt`: it lists every proof command re-run at the final commit with its exit
code and the file that holds the run. `environment.txt` holds the revision, both binaries' hashes,
the pin, the OS, TERM, shell, locale and the sizes each fixture drives.

## The at-tip proofs

- `screen-diff-tip-1.txt`, `screen-diff-tip-2.txt` — `compat/tui-screen-diff.sh` twice, 111 asserted
  checkpoints identical, 42 recorded.
- `screen-diff-tip-self-check.txt` — `--self-check`: every sabotage caught in its own channel, every
  equivalence reported nowhere.
- `pane-geometry-tip-1.txt` … `-3.txt` — `compat/tui-pane-geometry.sh` three times, columns and rows
  asserted at 80, 100 and 120.
- `status-row-tip.txt` — `compat/status-row.sh` under `LC_ALL=C LC_TIME=C`, the control the box note
  prescribes; 11 comparisons identical, the 3 theme rows still recorded.
- `attached-client-tip.txt` — `compat/attached-client.sh`, PASS.
- `unit-tests-tip.txt`, `clippy-tip.txt`, `zz-integration-tests.txt` — the crate checks and the
  integration-test rule.

## The measurements behind the changes

- `screen-diff-before-the-flip.txt` — the fixture with the sidebar change in the binary but 109 and
  120 still in `record` mode. This is what justified promoting them: every checkpoint at both widths
  reports `ok`, leaving only the two standing records.
- `screen-diff-asserted.txt` — the first run after the flip, with the sidebar case.
- `screen-diff-cursor-asserted.txt` — the first run with shape, blink, very-visible and colour folded
  into the asserted cursor tuple. The same corpus previously printed 73 recorded cursor differences.
- `screen-diff-canvas-matrix.txt` — the first run with the pane-border, colour-class, theme and 80x6
  cases.
- `screen-diff-self-check.txt`, `-cursor.txt`, `-canvas.txt` — the self-check after each of those.
- `status-left-trim-probe.txt` — zz counts a `#[...]` section against `status-left-length` where
  `format_trim_left` does not, so a styled status-left leaves the row entirely.
- `theme-arm-probe.txt` — why the theme arm is one landing and not two, with the corrected roster.
- `pane-geometry.txt`, `cli-status-tests.txt` — the intermediate runs kept for the record.

## What the sidebar case proves, and what it cannot

`focus-sidebar` has no counterpart in the pin, so the case is the one place in the fixture where the
two sides are driven differently. It asserts BOTH directions: the zz screen must DIFFER from the
pin's while the sidebar is up, and must be identical to it again once the sidebar is withdrawn. The
first half is what makes the second worth anything — a sidebar that never drew would pass a case
that only asserted equality — and the same first half runs as a `--self-check` sabotage.

The command is reached through a user binding (`bind-key -n F8 focus-sidebar`), not a one-shot CLI
invocation: `crates/zz-daemon/src/daemon.rs` answers a non-interactive client `focus-sidebar requires
an interactive client`. F8 rather than the default `prefix s` so that a lane changing what `prefix s`
means cannot change what this case measures.

## What stays recorded, and whose it is

- `default-fg` and `colour-classes` — the standing daemon-frame divergences, no owner this cycle.
- `pane-border-top` and `pane-border-bottom` — asserted for every glyph, column and cursor; only the
  border STYLE is recorded, because the theme colour is inside the recorded
  `presentation:tui-status-row-theme-defaults` decision and the explicit ground is the `default-fg`
  class.
- `styled-left-trim` — the `#[...]` trim above; the fix is in the format engine.
- `cursor-style-request` — a pane that really does ask for a cursor style. The raw TUI now writes no
  DECSCUSR at all, which is the pin's behaviour for a pane that never asked and not for one that
  does; the wire has nowhere to carry the request.
- `theme-light` — the forced server theme option, the theme arm's own record.
