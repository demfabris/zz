# TUI-015 attempt-01, cycle 10 capture lane, 2026-09-15

Clause 1 was already recorded before this attempt: fabrico's amendment of 2026-09-14 sits in
`knowledge/designs/tui-parity.md` and in the `options.lock-program` gap, and zz builds no lock
surface. This attempt is clause 2 only: the three lock commands answering what the pin answers,
and the two knobs named as accepted and arming nothing.

## Files

- `environment.txt` — the box, the branch, the base commit, the zz build under test and the pin.
- `01-tui-client-commands-run.txt` — `compat/tui-client-commands.sh` at the landing.
  Summary line: `all 109 asserted comparisons identical, 28 recorded not asserted (0 for a sibling
  lane)`. The lock family is lines 277 to 311: 30 asserted
  comparisons and 5 records. 25 of the 30 assert all five channels; 5 are `cli` cases that
  assert the three CLI channels and record the two attached ones against
  `options.lock-program`, which is where the pin's drawn lock lives.
- `02-tui-client-commands-self-check.txt` — `--self-check`, including the two lock sabotages this
  attempt added. Summary line: `self-check complete: every sabotage was caught in its own channel
  and both equivalences passed`.
- `03-lock-surface-probe.txt` — the measurement behind every case added here, taken clientless
  against two throwaway servers, section A arity and target validation, B the hook names, C both
  knobs at both scopes, D the one divergence and its control.
- `04-lock-surface-probe.sh` — the script that produced `03`, kept so the measurement can be
  retaken.

## What asserts

30 asserted comparisons over the lock family, identical on both sides:

- arity and flags: `lock-server zzcc-extra`, `lock-session zzcc-extra`, `lock-client zzcc-extra`
  are `too many arguments (need at most 0)`; `lock-server -t` is `unknown flag -t`, which is its
  whole target validation, since cmd-lock-server.c gives lock-server `.args = { "", 0, 0, NULL }`;
  `lock-client -t` with nothing after it is `-t expects an argument`.
- target validation: `lock-session -t` on a missing session, `lock-client -t` on a missing client,
  `lock-session` and `lock-client` with no `-t` at all. The clientless `lock-client` answers
  `no current client` on both, which is the cycle-7 landing TUI-011 recorded; `lock-session` with
  no `-t` resolves the one session and exits 0 on both.
- the hook: `after-lock-server` fires on `lock-server` and on nothing else. All three commands
  carry CMD_AFTERHOOK, so the pin fires `after-lock-<command name>`, and `after-lock-session` and
  `after-lock-client` are not hook names — both sides answer `invalid option`. Section B of the
  probe runs lock-session and lock-client with the marker hook armed and reads the marker back
  empty on both, then lock-server and reads `yes` on both.
- the knobs: `lock-after-time` and `lock-command` read back identically at global scope, take a
  session-scope value, read it back, unset it, and refuse an invalid value with the pin's
  `value is invalid: zzcc-nope`. The pin's own global default is whatever configure found on the
  oracle box, so the fixture sets `lock-command true` on both sides and the default value itself is
  not a parity target.

Nothing in zz was changed for this: every one of these already held. What this attempt adds is the
assertion, which did not exist for any of them.

## One finding, measured here and carrying no case

The pin resolves a session target through the whole
`session:window.pane` grammar and names the part it cannot find; zz matches the whole string as a
session name:

| target | pin | zz |
| --- | --- | --- |
| `=cli:win` | exit 0 | `can't find session: cli:win` |
| `=nosuch:win` | `can't find session: nosuch` | `can't find session: nosuch:win` |
| `=cli:nosuchwin` | `can't find window: nosuchwin` | `can't find session: cli:nosuchwin` |
| `%0` | exit 0 | `can't find session: %0` |
| `%99` | `can't find pane: %99` | `can't find session: %99` |
| `=cli:win.9` | `can't find pane: 9` | `can't find session: cli:win.9` |

Section D's control settles where it lives: `has-session -t =cli:win` and
`list-windows -t =cli:win` return those same bytes, with no lock anywhere in them. It is
`resolve_named_session` in `crates/zz-mux/src/model.rs`, one function every session-targeted
command in the product shares, and it is outside this lane's zones — a change there moves the
error text of every such command and of the corpus rows that assert them. No gap in
`compat/tmux-gaps.json` names it today. A case for it was written and then removed: the gate's
verify-claims charges an unattributed record in this shared fixture to every obligation the
fixture is mapped to, and this one names no obligation and no gap, so a case would have held five
obligations open for a divergence none of them own. The measurement stays here and in the ledger,
so nothing is waived by omission; it wants a gap or an obligation of its own first.

## The two sabotages

The three lock commands print nothing on either side, so a comparison that only read their stdout
would pass while zz did nothing at all. `--self-check` now plants, and requires the driver to
catch, a lock target that exists on the zz side only (caught in exit and stderr, and in neither of
the other three channels) and the `after-lock-server` hook armed on the zz side only, read back
through the marker it writes (caught in stdout alone).


## Fix pass after the rejected review

The review rejected the earlier claim that all clauses assert. The historical measurements above
are not final-tip proof. The rebased baseline at `0f7ea318` reports:
`all 113 asserted comparisons identical, 24 recorded not asserted (0 for a sibling lane, owners TUI-014=6 TUI-015=4 TUI-017=4 decided:TUI-016=1 gap:clients.interactive-refresh=8 unattributed=1)`.

Declared zone excursions: `compat/tui/verify-claims.py` retains the TUI-015 fixture mapping;
`crates/zz-mux/src/model.rs` is authorized for `resolve_named_session` grammar and its unit tests.
The earlier excursions remain `knowledge/tmux/divergences.md` and
`compat/scenarios/smoke/fixtures/command-flag-errors.sh`.

`compat/tui-client-commands.sh` now assigns the lock cases to TUI-015 and marks the five historical
screen records with fabrico's OS-locking decision. CLI channels remain asserted. The old shell
comments described the intended hook checks but did not implement negative assertions. This pass
adds those assertions separately. A target sabotage creates a session only on zz, and a hook
sabotage compares the marker after zz alone fires the hook; silent lock stdout is insufficient.
