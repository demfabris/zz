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
