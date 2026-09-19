# TUI-017 attempt-11: drop the vendored Ghostty patch entirely (lane `unpatch`)

fabrico's 2026-09-18 ruling: zz carries no patch on the vendored terminal
engine. This attempt removes the remaining 171-line `provenance.patch`
(indexed-colour class plumbing plus the one ICH hunk), the `build.rs`
machinery that applied it, the accessors/bindings/capture code that consumed
it, and the safe wrapper an earlier cycle-11 lane vendored to reach those
fields. The wrapper existed ONLY for the patch fields (its UPSTREAM.md: "The
local API exposes `Style.fg_indexed`, `Style.bg_indexed` and
`Cell.bg_indexed()`"), so it is un-vendored and `libghostty-vt` resolves to
upstream `uzaaft/libghostty-rs` at `46a9d2a` again.

## What changed, by commit

- `Drop the vendored Ghostty patch and un-vendor the safe wrapper`: the patch
  file, the `provenance_tree`/mirror/stale-cleanup/`zz_capture_provenance`
  machinery in `build.rs` (it now builds the fetched tree pristine in place),
  the `BG_INDEXED` query and `fg_indexed`/`bg_indexed` fields in
  `bindings.rs`, the `[patch]` entry plus workspace exclusion and lockfile
  source line, the colour-class branch in `push_capture_colour`, and the
  26-file vendored wrapper.
- `File the two unpatched-engine cases under decided:TUI-017`: both reasons
  open `DECIDED 2026-09-18 (fabrico)` and carry the decision sentence plus
  measured bytes (below). Both cases move out of `--self-check`, the way the
  tab and charset decisions are excluded: a decided case differs on both
  sides by definition, so equivalence-then-sabotage cannot run on it. The
  dead `capture-low-indexed-colour` sabotage arm and the unreachable
  `capture-edited-tab-ich-*` arm are removed; every remaining arm fires.
- `Record the no-patch ruling in the contract and the gap registry`: dated
  amendment in `knowledge/designs/tui-parity.md` (superseding the "bits stay"
  half of the tab ruling), the capture section and park line of
  `knowledge/tmux/divergences.md`, the `capture.rich-transports` reason,
  acceptance and evidence, the sys `UPSTREAM.md`, and the dangling wrapper
  citation in `knowledge/designs/terminal-bell.md`.
- `Retarget capture unit tests at the unpatched engine`: the two tests that
  asserted patch behavior now assert the decided bytes (wide insert clears
  to 76 spaces + ABCD; low palette indices fold to named/bright SGR).
- `Regenerate the gap report for the no-patch record`: generated.

## Measured bytes (probe-bytes.sh, exact)

- `capture-low-indexed-colour` (`-C -e -S 0 -E 0`):
  pin `\033[38;5;1mRED\033[39m\n` (24 bytes),
  zz `\033[31mRED\033[39m\n` (20 bytes).
- `capture-edited-tab-ich-off-line` (`-C -S 0 -E 4`), row 0 of 80 cells:
  pin 74 spaces + `C` + 3 spaces + `AB`,
  zz 78 spaces + `AB`.
- The pre-conversion baseline (`baseline-unpatched-fixture.txt`, kept from
  the previous agent in this worktree) turned exactly these two red out of
  219 asserted, confirming the review's claim that no other asserted case
  depends on the patch. Re-measured, not trusted: this attempt's own probes
  reproduce both diffs byte for byte.

## Proof at this tip

- `compat/tui-client-commands.sh` three times (run1/2/3.txt), exit 0 each:
  `all 217 asserted comparisons identical, 44 recorded not asserted
  (0 for a sibling lane, owners TUI-014=6 decided:TUI-015=4
  decided:TUI-016=1 decided:TUI-017=25 gap:clients.interactive-refresh=8
  unattributed=0)`. Ordinary TUI-017 records are zero.
- `--self-check` (self-check.txt), exit 0: 148 oks, no failures, every
  sabotage caught in its own channel; re-run on the final tree after the
  ICH sabotage-arm removal (self-check-final.txt), exit 0, identical
  148 oks.
- `compat/tui-copy-mode.sh` (copy-mode.txt), exit 0: all 147 agree, 0
  recorded elsewhere; `--self-check` exit 0, every sabotage caught.
- `compat/tui-screen-diff.sh` (screen-diff.txt), exit 0: 147 identical, 6
  recorded; `--self-check` exit 0, every sabotage caught.
- `compat/attached-client.sh` (attached-client.txt), exit 0: PASS.
- Unit suites: zz-terminal 287 pass / 1 ignored; zz-mux 536; zz-tui 208;
  zz-daemon 979 pass with `switch_client_key_table_and_formats_are_client_local`
  failing once under fixture+clippy load and passing solo in 0.71s
  (load flake by the campaign rule).
- `clippy -p zz-terminal -p zz-mux -p zz-daemon -p zz-tui --all-targets --
  -D warnings`: exit 0. `cargo fmt --check`: clean. Wire 104 unchanged
  (`wire-version.py`: "104 matches v0.11.1 and the wire is unchanged").
- Delta corpus (delta.txt, delta-selection.rows): 172 rows, none unrun
  (smoke 144, changed 0, command-matched 44 over
  `--commands capture-pane,copy-mode,resize-pane`), exit 1 with 7 rows
  divergent after the retry pass — and all 7 diverge identically on the
  d03e20a0 control build (delta-main-control.txt), so inherited 7, caused
  here 0. The 7: known/known-spread-mixed 1 GEO (matches its registered
  `0 1 0 0 0` tuple); micro-flags 1 OUT (the box-locale `%b`: `Sep` vs
  `set`); show-options-hooks 7 OUT (`lock-command` default `"lock -np"`
  vs `vlock`); smoke/cli-chain-parse-abort 1 OUT (`CLI_PARSE_ABORT`
  parser behavior); smoke/default-client-command 1 OUT + 2 WARN (missing
  `DEFAULT_CLIENT_COMMAND` env); smoke/plugin-runtime-resurrect-restore 1
  OUT + 1 WARN (tmux-side restore assertion); smoke/status-background-jobs
  1 OUT + 1 WARN (documented environmental status-timing row). None
  touches capture SGR bytes or ICH-cleared grid content, the only two
  behaviors this lane changes. First-pass flakes that passed alone on
  retry: smoke/copy-mode-refresh, smoke/jobs-command-environment,
  smoke/plugin-runtime-continuum, smoke/source-replay-diagnostics.
- Clean sys-crate build (clean-sys-build.txt), exit 0 in 60s from an empty
  `third_party/rust/libghostty-vt-sys/target/`: the only fetch is the
  pristine upstream clone (`git clone --filter=blob:none` + checkout
  `20c3eae`), and nothing is patched. OUT_DIR holds exactly
  `ghostty-install`, `ghostty-src`, `zig-cache`: no `ghostty-provenance-*`
  mirror, no in-place stamp, no `fg_indexed` in the installed header, no
  `zz_capture_provenance` marker in the pkg-config files, no patch or
  provenance mention in the build log.
- `python3 compat/tui/verify-claims.py --run TUI-017 --zz
  target/debug/zz_cli` (verify-claims-TUI-017.txt), exit 0: `all 217
  asserted comparisons identical, 44 recorded not asserted (...,
  decided:TUI-017=25, ...)`, `(44 recorded, none of them TUI-017's)`,
  `every verified obligation holds up`.
- `compat/check.sh` (compat-check.txt): first attempt self-deadlocked and was
  killed (exit 143, retained as compat-check-deadlocked.txt): `check.sh`
  invokes bare `cargo`, which the box rules forbid, so it ran with a PATH
  shadow routing `cargo` through `/tmp/zz-cargo.sh` — but the naive shadow
  kept itself first on PATH, the wrapper re-entered it, and the outer
  flock's lock met the inner flock's wait on the same slot. The fixed
  shadow exports a PATH without its own directory (verified to resolve
  real cargo); the rerun uses it and exits 0, with the zz-mux lib suite
  (536 pass) and the three daemon manifest tests green inside it.

## Perf: what removing the patch buys (unpatch-perf.*)

Pristine vs patched ReleaseSafe `libghostty-vt` built from the same
`20c3eae` tree (patched tree = pristine + HEAD's 171-line patch), driven by
the attempt-10 reviewer's `native-perf-review.c` on `edits` (DCH+ICH+ECH per
row) and `overwrite`, tabs/spaces variants, 15 alternating ABBA pairs each,
pinned CPU, ASLR off, CPU seconds:

- edits/tabs: patched +1.56% over pristine (medians 0.161548 vs 0.159074)
- edits/spaces: patched +0.86% (0.132417 vs 0.131290)
- overwrite/tabs: -1.98%; overwrite/spaces: -1.60% (patched nominally
  faster; distributions overlap, inside host noise)

Removing the patch buys ~1% CPU on the ICH-exercising workload, matching
the reviewer's +1.08%/+1.29% instruction attribution to the ICH hunk; the
overwrite control confirms the indexed-colour plumbing cost nothing
measurable either way. Headers verified distinct (patched installs
`fg_indexed`, pristine does not).

## TUI-015 note

TUI-015 shares the gate-4 proof and its `sources` listed the three deleted
paths, which `tracker.py check` requires to exist, so those three lines are
dropped as mechanical fallout of the deletion. Its status, proof and note
are untouched: this lane owns TUI-017 only, and the gate re-measures both.
