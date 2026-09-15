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
