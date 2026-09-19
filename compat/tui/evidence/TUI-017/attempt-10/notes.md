# Tab capture decision, the wide erased captures and the patched build

This attempt applies fabrico's 2026-09-18 ruling to TUI-017, closes the eight
100x30 erased-background probe differences attempt-07 left owned, and keys the
patched Ghostty tree by the patch that made it. TUI-015 is untouched: its
resolver and its three configured `pane-base-index` cases are the ones the
previous review accepted, and they still pass in every run below.

The lane rebased onto `origin/main` `7f8d2cc4` (`v0.11.0`). `compat/wire-version.py`
reports 104 matching the release with the wire unchanged, so nothing here
touches a payload.

## 1. Capture returns the spaces a tab left

Since tmux 3.4 the pin marks every cell a tab produced (`GRID_FLAG_TAB`) and
`grid_string_cells` prints a literal tab for the span whatever the capture flags
say (`grid.c:1202`); an edit that removes the head of such a tab also drops the
padding cells behind it. Cycle 11 imitated that with tab spans carried in spare
Ghostty cell and style bits. fabrico decided on 2026-09-18 that zz does not
imitate the marker, so the span tracking left the patch.

`provenance.patch` now carries only the indexed-colour class (`fg_indexed`,
`bg_indexed`, the `GHOSTTY_CELL_DATA_BG_INDEXED` accessor) and the ICH hunk in
`Terminal.zig`. The tab style flag, the `printSliceFill` mask change, the
per-HT marking and the ICH/DCH/ECH/IL/DL/scroll handling that existed only for
tabs are gone, and `crates/zz-terminal/src/session.rs` lost the span scan its
capture path used. `capture-low-indexed-colour` stays asserted.

The ICH hunk stays because it is not a tab rule: an insert wider than the cells
it moves keeps the pin's stale cells on screen and in capture alike, which
`capture-edited-tab-ich-off-line` asserts against the pin.

Seventeen cases became records whose reason opens `DECIDED 2026-09-18 (fabrico)`
and is filed under `decided:TUI-017`: `capture-tab-trailing`,
`capture-tab-internal`, `capture-tab-wide` and the fourteen edited-tab cases
whose rows still hold a tab on the pin. Nine edited-tab cases stay asserted,
because the pin's row ends up holding no tab cell there and both sides capture
the same spaces; each keeps its one-sided scene sabotage.
`capture-edited-tab-ech-entire` needed a new sabotage: erasing the cells a tab
produced is invisible once the literal tab is gone, so the sabotage now changes
the text behind the erase instead of withdrawing it. `case-changes.json` lists
every case on both sides of that split.

The decision is recorded in `knowledge/designs/tui-parity.md` as a dated
amendment beside the 2026-09-14 lock and 2026-09-17 copy-mode rulings,
`knowledge/tmux/divergences.md` replaces the old tab rule it misdescribed, and
`compat/tmux-gaps.json` cites both.

## 2. The benchmark after the removal

`native-benchmark.sh` runs the reviewer's own workloads through one native write
each: `overwrite` (output that overwrites rows holding tabs), `widestops` (a tab
stop on every column), `alltabs` (a line of all tabs), `mixed`, `edits`
(ICH/DCH/ECH/IL/DL), `htsevery` and `reflow`, each in a tab and a space-only
variant. Three blocks of five ABBA candidate/main pairs, one pinned CPU, ASLR
off per child, twenty warm-up pairs; medians in `native-summary.txt`, raw
samples in `native-samples.jsonl`.

No workload is outside this host's noise any more. Pooled CPU medians run from
-19.6% to +15.7% with paired medians between -3.3% and +7.2%, in both
directions, on a shared box. The instruction counts settle it:
`native-attribution-summary.txt` compares main, an indexed-colour-only build and
the candidate. Every workload matches main to five decimal places except `edits`
at +1.08% (tabs) and +1.29% (spaces), and the indexed-only control reproduces
main there, so the retained ICH hunk is the only thing that still costs
anything. The reviewer's +68.33% on `overwrite` and +33% on `widestops` are
gone: both now execute main's instruction stream.

## 3. The eight 100x30 erased-background differences

`compat/tui/evidence/TUI-017/attempt-07/probe-residuals.json` kept eight
TUI-017-owned differences under `-e`/`-N` capture of erased backgrounds at
100x30. They were not a capture-serialization fault: a detached
`new-session -x 100 -y 30` started its pty at 80x24, because the daemon passed
no initial size and the terminal worker fell back to `INITIAL_COLUMNS` and
`INITIAL_ROWS`. The pane was resized to 100x30 afterwards, and the rows the
erase had already touched kept the 80-column allocation the pin never had. The
daemon now gives both detached spawn paths the size the layout assigns the pane
(`Engine::pane_geometry`, which already carves out a `pane-border-status` row),
which is what the pin does when it forks.

The eight cases are asserted in `compat/tui-client-commands.sh` as
`capture-erased-100x30-{line,display,clear,region}-{escape,padding}`, each with
its scene sabotage. `wide-erase-attached-probe.sh` reproduces the original
attached comparison: three runs of the fixed binary report 0 differences, the
binary with the size restored to `None` reports 8 and 7.

## 4. The patched build cannot be reused by an unpatched one

`build.rs` applied `provenance.patch` in place to the Ghostty tree it fetched
into `OUT_DIR`, and the sys unit hash depends only on the relative path, so a
build with a different patch, or none, could link the patched library. A
reviewer's origin/main build was contaminated that way.

The fetched tree stays pristine now. `provenance_tree` mirrors it into
`ghostty-provenance-<hash of the patch and the pinned commit>` with hard links,
copies only the files the patch touches, applies the patch there and installs
into `<that name>-install`. Stale trees and installs are removed. A tree an
older build patched in place is detected and restored, and
`GIT_CEILING_DIRECTORIES` keeps `git apply` inside `OUT_DIR`.

## 5. Runs

Every fixture, probe and corpus command ran with the credential variables unset.

- `client-commands-final-1.txt`, `client-commands-final-2.txt`,
  `client-commands-final-3.txt`: three shared runs, each 219 asserted
  comparisons identical and 42 recorded, `owners TUI-014=6 decided:TUI-015=4
  decided:TUI-016=1 decided:TUI-017=23 gap:clients.interactive-refresh=8
  unattributed=0`. Ordinary TUI-015 and TUI-017 records are 0.
- `client-self-check-final.txt`: 152 expectations, every sabotage caught in its
  own channel and both equivalences passed.
- `copy-mode.txt` 147 asserted with no recorded difference,
  `copy-mode-self-check.txt` 27 expectations.
- `screen-diff.txt` 147 asserted with 6 records, all of them the
  `cursor-style-request` DECSCUSR case this fixture already owned;
  `screen-diff-self-check.txt` 18 expectations.
- `attached-client.txt`: PASS.
- `verify-claims-TUI-015.txt` and `verify-claims-TUI-017.txt`: both re-measure
  the fixture and report `42 recorded, none of them TUI-015's` and `none of them
  TUI-017's`.
- `clippy-final.txt`: `zz-terminal`, `zz-mux`, `zz-daemon` and `zz-tui`, all
  targets and features, warnings denied, exit 0.
- `tests-terminal-mux-tui-final.txt`: 1136 passed, 1 ignored, none failed.
- `tests-daemon-final.txt`: 986 passed, 1 ignored, and the one failure is
  `agent_stream_soak_slow_client`, which the ACP agent lane owns. It fails the
  same way alone (`tests-daemon-soak-alone-1.txt`, `-2.txt`: 1501 update items
  against the expected 1500), and attempt-07 already reproduced that exact count
  on a clean unmodified baseline archive. Nothing on this branch touches the
  agent stream. The new `detached_panes_spawn_at_their_laid_out_size` passes.

`client-commands-1.txt`, `client-commands-2.txt` and `client-self-check.txt` are
the earlier runs at fixture `75d5f5f3`, kept: the self-check there caught the
`capture-edited-tab-ech-entire` sabotage that no longer changed the screen, which
is the fixture fix above. `clippy.txt`, `tests-terminal-mux-tui.txt` and
`tests-daemon.txt` are empty because an interruption killed those jobs before
they wrote anything; the runs that count are the `-final` ones.

## 6. Delta corpus

254 rows ran, all 221 rows `compat/run.sh --delta origin/main..HEAD` selects plus
33 copy-mode, resize and buffer rows named for the touched commands. There are no
unrun rows. Ten rows failed, none of them caused here; `delta-summary.json` has
the partition.

Seven fail on the immutable main control as well
(`target/capture-residuals-main-control/zz_cli`, main `38c50df4`), with the same
step count and the same divergence counts on both sides: `lane2-store`,
`micro-flags`, `show-options-hooks`, `smoke/cli-chain-parse-abort`,
`smoke/default-client-command`, `smoke/plugin-runtime-resurrect-restore` and
`smoke/status-background-jobs`. The options rows differ in this pin build's
`vlock` default against zz's `lock -np`; `micro-flags` differs in the month the
pin spells with the host locale; the restore fixture reports its own pin-side
assertion failure. The control is a day older than the `7f8d2cc4` base, and the
commits between them touch the desktop client, the release version and evidence
redaction, not CLI options, formats or pane spawning.

Three more failed for the harness, not for zz: `compat/diff-scenario.sh`
symlinks the installed launcher to a `zz_cli` beside `ZZ_BIN`, and the shard runs
had none. With the sibling in place `smoke/launcher-installed-layout`,
`smoke/config-discovery-launcher` and `smoke/pane-tmux-path` run clean
(`corpus-launcher-rows.txt`).

## 7. Limits

- The host is shared with other lanes. No exclusive-host claim is made for the
  CPU medians; the instruction counts are what the performance argument rests on.
- The `edits` workload still executes about 1% more instructions than main. That
  is the ICH rule, which an asserted pin case requires.
- Styled frozen-mode capture, narrow-screen reflow and compressed-history round
  trips remain unproven, as attempt-07 recorded. `-F`, `-H`, `-P` and `-R` keep
  their measured refusals.
- The binary under test was built at `75d5f5f3`; the commits after it change a
  daemon test, the fixture's sabotage branch and knowledge documents only.
- The environment listings attempt-07 retained are redacted at this tip, but the
  unredacted text is still in this branch's history at the commit that recorded
  them. `python3 compat/evidence-secrets.py` passes on the tip.
- `bench/run.sh` still cannot run here: its fixtures and the release desktop
  bundle are absent.
- `cargo test -p zz-daemon` is not green: the slow-client agent soak fails here
  as it failed on attempt-07's clean baseline. No all-tests-green claim is made.
- Both obligations stay at `review`. The gate decides verification.
