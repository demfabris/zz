# TUI-018 attempt 02

Started 2026-09-15 20:18:37 UTC; hard deadline 22:18:37 UTC.
Base: 7e7cb1ee25122a10e7f1d8815241422eb00c4199.

## Measurement

The pinned tmux d77c9dc6 applies a single source alias and exits 0. With two
`source-file -` members it applies the input once, reports `Bad file descriptor: -`,
and exits 1. A first `load-buffer -` retains invalid UTF-8 and NUL bytes. A later
source or buffer reader reports EBADF. After a later buffer reader fails, tmux
still prints a following `display-message -p after-error`.

The baseline zz binary reproduces the missing stream in a source alias group.
A buffer followed by a source reader also rejects binary input at the CLI's UTF-8
reader: it selects the tail sink instead of the first sink. A buffer at the tail
of an alias formats the bytes into its text body and fails with `invalid octal
escape`. These are additional observations of the same two routing defects.

## Implementation

`CommandInvocation.stdin_spent` is an in-process boolean with `#[serde(skip)]`.
The existing stdin field remains the only wire carrier; protocol 103 does not
change. Alias expansion preserves stdin on both single and grouped bodies. The
mux consumes it on the first emitted stream effect. The daemon also needs the
carrier in its prepared alias queue, because CLI execution bypasses the mux's
group loop. This daemon change is required for the recorded source case.

## Files

- `environment.txt`: initial revision, OS, pin, baseline binary hashes and tools.
- `probe.py`: isolated, bounded probe driver; invokes explicit throwaway sockets,
  scrubs HOME/config and reaps each server. Run with `python3 .../probe.py tmux zz`.
- `pin-probes.txt`: initial pin measurement, including binary buffer payloads.
- `pin-sequencing.txt`: pin measurement with a print after a spent buffer reader.
- `zz-baseline-probes.txt`: baseline zz measurement with that same driver.
- `build-baseline.txt`: initial wrapper build, exit 0.
- `fmt-fix1.txt`: wrapper formatting run, exit 0.
- `test-fix1-unit.txt`: filtered protocol and mux stream tests.
- `test-fix1-cli.txt`: live CLI source alias test, exit 0.
- `okf.txt`: knowledge validation, exit 0; one existing aged-research warning.
- `streams-fix1.txt`: stream fixture after the first fix.
- `streams-fix1-self-check.txt`: source error/value sabotages and existing channels, exit 0.

First-fix checks (all exit 0): `cargo test -p zz-protocol -p zz-mux --lib
caller_stream --jobs 3 -- --test-threads=3`; `cargo test -p zz --test cli_binary
caller_stream_source_alias_group --jobs 3 -- --test-threads=3`; the stream fixture
and its `--self-check`. Cargo used the requested systemd memory scope and random
flock slot. The CLI test used `/tmp/zz-emptyhome` for HOME/config. The fixture
reported `all 37 asserted comparisons identical, 0 recorded not asserted, 4 decided
(0 for a sibling lane)`. `git check-ignore -v` returned 1: the evidence directory
is not ignored. The first commit leaves the Argument fix explicitly open.

## Second fix

The CLI chooses the first sink in the prepared alias members. The group retains
its raw stdin until the daemon has prepared the members; the daemon then appends
Argument bytes to the first consuming member. Direct CLI Argument calls and the
daemon use one `append_stdin_payload` helper. A spent Argument reader records
EBADF and returns `CommandExit`, which preserves the pin's continuation behavior.
The helper move also reaches `send-text` and `agent-send`; their boundary tests
and existing live agent alias test remain part of the zz package check.

The updated fixture reports `all 43 asserted comparisons identical, 0 recorded
not asserted, 4 decided (0 for a sibling lane)`. It adds six buffer comparisons:
a source reader after a buffer reader, two buffer readers with a subsequent
print, and a reader after a print, each paired with raw buffer readback. The
self-check removes readers or changes one byte on only the zz side. It catches
each added/flipped case in its own channel and restores both equivalences.

The changes span mux, CLI, the daemon's actual alias queue and argument
dispatcher, and the protocol crate's in-process invocation marker. No GUI or terminal renderer
change, dependency bump, wire append, or sibling obligation edit accompanies
these fixes. No board operation or GitHub comment was made.

## Additional files

- `cargo-wrapper.sh`: requested random-slot systemd cargo wrapper.
- `check-cargo-wrapper.sh`: PATH shim for compat/check.sh's nested cargo tests;
  adds three jobs and three test threads, then uses the same scope and flock.
- `fmt-fix2.txt`: formatting, exit 0.
- `test-fix2-routing.txt`: both prepared CLI routing unit tests, exit 0.
- `test-fix2-cli.txt`: live source and binary buffer alias tests, exit 0.
- `streams-fix2.txt`: expanded stream fixture, exit 0, 43 asserted/0 recorded/4 decided.
- `streams-fix2-self-check.txt`: per-case sabotage checks, exit 0.
- `delta-list-precommit.txt`: delta selection, exit 0; 22 alias/source/buffer/stream rows.
- `scenario-strings-precommit.txt`: exact diagnostic and command searches;
  missing-string searches return 1. Also identifies smoke/cli-output-bytes.txt
  and smoke/config-grammar.txt for the final corpus checks.
- `test-mux-precommit.txt`: whole mux library test output.

The first whole mux-library wrapper waited 540 seconds on slot 1 and exited 1
without starting cargo (`test-mux-precommit.txt` is therefore empty). The retry
combines the mux, daemon and protocol library suites in one wrapped cargo call.
Its output is `test-libraries-precommit.txt`.
The combined retry also selected occupied slot 1. I stopped its flock only after
SIGSTOP and confirming it had no cargo child; wrapper exit 143, empty output in
`test-libraries-precommit.txt`. The next retry selected slot 0, which another
waiter had meanwhile reserved. Its output is `test-libraries-precommit-retry.txt`.
The final library retry passed: 919 daemon, 527 mux and 224 protocol tests,
exit 0, including the serde-skipped marker test. `test-zz-precommit.txt` contains
the full zz package run that follows it. No test failure occurred in the lock
waits described above.
The full zz package passed with exit 0: 642 library tests (one existing ignored),
127 CLI tests, and all additional integration targets. The final-tip proof outputs
will be written after the closing commit as `final-*.txt` in this directory;
those files are local artifacts so recording them cannot invalidate their tip.

## Final tip

Both commits are pushed on `campaign/tui-stream-alias`. The final tip is
`ff58de8a6f8fc194db0480deb6893ba85e6121e0`, based on
`7e7cb1ee25122a10e7f1d8815241422eb00c4199`. Every required closing proof was run
after the final commit. No subsequent code or ledger change was made.

All final commands used ZZ_COMPAT_TMUX and TMUX_BIN pointing to the pinned
`/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux`, ZZ_COMPAT_CORPUS pointing to
`/home/demfabris/dev/zz/compat/.cache/plugins`, and ZZ_TRAY=0. The exact command,
start tip, timestamps and exit codes are in `final-commands.txt`; the environment
and capture wrapper are in `final-runner.sh`. Cargo used `cargo-wrapper.sh`
through its identical `/tmp/zz018-cargo.sh` copy. compat/check.sh used
`check-cargo-wrapper.sh` through the `/tmp/zz018-tools/cargo` PATH shim. HOME was
`/tmp/zz-emptyhome`, XDG_CONFIG_HOME was its config subdirectory, and CARGO_HOME
and RUSTUP_HOME were explicitly retained for the toolchain. Fixtures installed
their own isolated per-side HOME and configuration underneath this environment.

All required proofs returned 0. The three stream runs each report
`all 43 asserted comparisons identical, 0 recorded not asserted, 4 decided (0 for a sibling lane)`.
Both fixture self-checks passed. The shared client fixture reports 84 asserted
comparisons and 25 recorded cases, all owned by other obligations or accepted
gaps, with none unattributed. verify-claims independently reran both and found
no recorded case owned by TUI-018. All 24 selected corpus scenarios passed with
zero divergences and no retries; every six-row chunk finished in under three
minutes. The full zz suite and mux, daemon and protocol libraries passed without
reruns; clippy, formatting, tracker and compat/check.sh also passed.

The optional unrestricted `git diff --check origin/main...HEAD` returned 2 solely
for whitespace retained in real test output files. The same check excluding
`compat/tui/evidence` returned 0. Raw output was not rewritten. No final test
flake occurred. The stream fixture retained its existing null-byte command
substitution warning; the new binary alias cases use file stdin.

The required process sweep found only the sweep's own shell command, with no
leftover zz-cli, zz-user, zzprobe or zzcs servers to kill. No binary was copied
to /tmp. No other checkout, user server, GitHub issue or board was changed.

Final-tip outputs and this final appendix are intentionally local and uncommitted:
committing them would move the tip after the proofs. The pushed commits contain
the complete implementation, fixture assertions, ledger/report and real precommit
evidence. The following files hold the final-tip run:

- `final-build-identity.txt`
- `final-client-commands.txt`
- `final-client-self-check.txt`
- `final-clippy.txt`
- `final-commands.txt`
- `final-compat-check.txt`
- `final-delta-1.txt`
- `final-delta-2.txt`
- `final-delta-3.txt`
- `final-delta-4.txt`
- `final-delta-list.txt`
- `final-diff-check.txt`
- `final-diff.txt`
- `final-fmt.txt`
- `final-processes-after.txt`
- `final-processes-before.txt`
- `final-push.txt`
- `final-remote-tip.txt`
- `final-runner.sh`
- `final-scenario-search.py`
- `final-scenario-strings.txt`
- `final-source-diff-check.txt`
- `final-streams-1.txt`
- `final-streams-2.txt`
- `final-streams-3.txt`
- `final-streams-self-check.txt`
- `final-test-libraries.txt`
- `final-test-zz.txt`
- `final-tracker.txt`
- `final-verify-claims.txt`
- `final-wire-version.txt`
- `final-audit.txt`
- `final-audit.py`

The first final audit compared the complete ledger directly to the moving
origin/main ref and failed its equality assertion: another worktree had advanced
that shared ref to `1077951108aabe956104ac8de62da319aa06db4b`, landing other
obligations. No local ledger edit caused that failure. The corrected audit uses
the fixed starting base and verifies that the three-dot merge base is still
`7e7cb1ee25122a10e7f1d8815241422eb00c4199`; only the four permitted TUI-018 fields
differ. This does not change the completed delta selection or final-tip proofs.
