# TUI-013 attempt-01

Everything here was produced on 2026-09-20 on the macbook (macOS 27.0 build
26A428, Apple M4 Max, 16 cores), the platform that recorded the expiry, from
the shared checkout `/Users/demfabris/dev/zz` at
`04a39258974fed54a067df5ed27425b9faf128b4` (equal to `origin/main`), clean apart
from this directory, against pinned tmux
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. Every file is a real run.

## What each file is

| File | What it is |
| --- | --- |
| `environment.txt` | sha256 of both binaries, the zz_cli build time, revision and dirty state, pin commit, OS and hardware, TERM, both bashes, locale, the sizes each fixture drives |
| `geometry-run-1..3.stdout.txt` | `compat/tui-pane-geometry.sh target/debug/zz_cli <pin>`, three consecutive runs, exit 0 in 5.5 s, 5.5 s and 5.4 s |
| `geometry-run-*.stderr.txt` | the matching stderr, all empty: no wait fired, so no timeout dump exists to read |
| `status-row.stdout.txt` | `compat/status-row.sh`, exit 0, all 14 comparisons identical, none recorded |
| `smoke-tui-client-input-backpressure.stdout.txt` | `compat/run.sh --strict-geometry smoke/tui-client-input-backpressure`, exit 0, 2 steps, 0 divergences on every channel |
| `verify-claims.txt` | `python3 compat/tui/verify-claims.py --run TUI-013`, exit 0, the guard re-measuring the fixture once the mapping existed |
| `review.md` | the independent adversarial review of this attempt |

The binary is `target/debug/zz_cli`: since the 2026-09-16 binary split the CLI
and daemon live in `crates/zz-cli`, `compat/run.sh` builds `-p zz-cli`, and
every gate since has passed `ZZ_BIN=$PWD/target/debug/zz_cli` to the fixtures.
No `target/debug/zz` exists in this checkout today. The sha256 identifies the
artifact and does not attest it (a debug build is not bit-reproducible);
attestation is the recorded revision plus the clean tree.

## The recorded timeout, on its own platform

The ledger recorded an exploratory run on 2026-09-09 on this machine that exited
2 on `zz geometry report did not happen within 10 seconds`, from a
`target/debug/zz` that was not rebuilt before that run and was overwritten the
same evening, so its revision is unrecoverable.

It does not reproduce. Three consecutive runs of the fixture with an attested
build exit 0, each in about half the 10 second bound, with every asserted
measurement identical on both sides: 80, 100 and 120 columns handed to the pane
at 80x24, 100x24 and 120x24, and 23 rows at each size. The wait that fired in
the record is `measure()`'s, and it answers in well under a second here.

What that bounds the record to, and no further:

1. The recorded binary was unattested, not rebuilt before that run, and
   overwritten the same evening. Nothing about it can be measured now.
2. The one mechanism ever proposed for the expiry, a stale binary still carrying
   the pre-fix `flush_output` stall, is refuted by
   `tui.client-input-backpressure`'s own resolution: the fixture exited 0 at
   pre-fix `012b4dcc`, and its inner clients run inside an outer pinned tmux that
   drains them continuously, so that stall cannot build in this fixture on any
   platform. That stall is also measurably absent on this box at this revision:
   the backpressure scenario is clean on every channel.
3. An attested build at this revision passes three of three on the platform
   that recorded the expiry.
4. Since cycle 1 the fixture retains its full dump on any expiry (which wait
   fired, both outer screens, both servers' client and pane lists, the daemon's
   stdout and stderr, each client's stderr), proven by sabotage under
   `compat/tui/evidence/TUI-001/attempt-01/timeout-diagnostics/`. A recurrence
   self-documents and is a new obligation with that dump attached.

This record names no defect for the vanished binary. Naming one would be the
unevidenced claim acceptance clause 2 forbids.

## Status row on this box

`compat/status-row.sh` exits 0 here, all 14 comparisons identical. The `%b`
divergence TUI-001 recorded on alienware (pin `09-set-26` against zz
`09-Sep-26`) came from `LC_TIME=pt_BR.UTF-8`; this box runs
`LC_ALL=en_US.UTF-8` with `LC_TIME` unset, so both binaries print English month
names. That is a measurement of the locale, not evidence the divergence is
fixed; `crates/zz-mux/src/formats.rs` still expands `%b` through chrono.

## How this ran

`compat/tui/run-2.js` failed `compat/tui/lint-runner.py` at launch (13 of its
16 rules fail, including the wire rule at 99 against the tree's 105, agent
concurrency and the cargo caps), so the loop ran by hand in one session, as
`compat/tui/README.md` allows: the measurements above, an independent
adversarial review, both trackers and `compat/check.sh`, one records commit on
main. The board front `F-TUI-MACOS-TIMEOUT` was minted under TRIAGE and held
for the run.
