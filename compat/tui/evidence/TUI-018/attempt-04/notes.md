# Protocol correction for the alias lane

This attempt supersedes attempt-03's wire blocker. The orchestrator corrected the stale
103-unreleased instruction and explicitly requested 104 for this branch. Fetch and
`git ls-remote origin refs/heads/main` returned
`be5709dfcd9936d44519683e5da8dc6ab7748d96`, the v0.10.0 release. The branch already
contained that base; `git rebase origin/main` reported it up to date. The remote guard
still watches only message.rs at this fetch. No lane edit changes the guard.

Commit `753c8497781e54dd6b4717e0e0e1243d2857737f` sets the protocol constant and both
decimal assertions to 104, and the two hello-frame bytes to 0x68. It updates the
current-version documentation and adds a v104 history paragraph. The v103 history
matches the release: CommandInvocation.stdin had already shipped. No alias payload
append needs moving. The new spent marker has serde(skip), so absent and spent
invocations encode identically; its regression test remains in the protocol suite.
The 104 envelope and hello versions now differ from v0.10.0, as directed.

All Cargo commands run through /tmp/zz-cargo.sh, including those inside compat/check.sh.
The PATH shim from attempt-03 preserves the cap and shared slots and moves the wrapper's
jobs option ahead of Cargo's argument separator. The home and config directories are
scrubbed. run.sh records each command, revision, timestamp and exit status.

The binary built from the 104 source has SHA256
`b02e5b50562b9755a2092868bd5844ef1d484f1bbaa9ea1fdfb469b3e31d4e4a`.
It and its CEF siblings have independent hard links under target/c11-alias-v104-proof.
The build began before the source commit, after the version edit; the only Rust edit
while it ran renamed the hunt_claims test, which the binary does not compile.
Runtime fixtures start at the committed source revision. Their results follow below.

Attempt-03 retains the complete 222-row, 2,639-step delta run on the same be5709df base
and the full four-package tests. This correction changes the wire version and pins,
not the alias implementation or fixture cases. Do not read the earlier corpus and GUI
test measurements as reruns against the 104 executable.

## Corrected-version checks

- wire-version.py exits 0: 104 is unreleased; v0.10.0 shipped 103.
- The complete protocol package exits 0: 228 library tests, seven default-theme
  tests and 16 hunt_claims tests pass. The serde-skipped marker test passes.
- compat/check.sh exits 0, including its 533-test mux suite and three required
  daemon manifest checks. Its internal Cargo calls all pass through the wrapper.
- Four-crate clippy with all targets, all features and -D warnings exits 0.
- cargo fmt --all exits 0. No subsequent Rust changes were made.
- The stream fixture exits 0 with 57 asserted comparisons identical, zero recorded
  and four decided. Its self-check exits 0: 29 sabotages caught, two equivalences
  pass. This retains all 14 assertions and 16 sabotages added in attempt-03.

The focused caller_stream_ CLI integration filter also exits 0 (three tests).
The shared client fixture exits 1 with the same three failures as attempt-03:
refresh-missing-argument, client-tree-unknown-flag and client-tree-usage return
2 on zz and 1 on the pin. Its 25 records remain attributed to TUI-014 (6),
TUI-015 (4), TUI-017 (6), decided:TUI-016 (1) and gap:clients.interactive-refresh
(8); unattributed=0, with no TUI-018 records. Its self-check exits 0 and catches
all sabotages while both equivalences pass.

The full attached-client fixture exits 0: attached-client compatibility: PASS.

The final compat/check.sh rerun also exits 0. verify-claims.py --run TUI-018
exits 1: its stream rerun confirms 57/0/4, then its shared fixture repeats the
three-of-84 CLI-status failure summary. This correction resolves the wire guard,
not those status mismatches. Status remains partial in the worker report and
review in campaign.json. No independent approval or verified claim is made.

The gate-handoff is the worker assessment. The complete three-dot footprint is
footprint.txt, including the carried attempt-02 review evidence, attempt-03 fixes
and measurements, and this correction's evidence. The generated reports came
from the tracker write-report commands. No Rust or fixture logic changed after
source revision 753c8497. The daemon and client runtime hash remains the one in
environment.txt. Failed earlier runs and the ledger-key scripting error remain
recorded; no test failure was discarded.

The full evidence diff check flags trailing spaces in raw terminal/diff transcripts.
Those bytes are retained. The source, fixture, ledger and knowledge diff check passes;
see diff-check.txt. The staging script raised on the raw-evidence check, and the
following shell command still created the evidence commit. This amendment records
that result rather than trimming measured output.
