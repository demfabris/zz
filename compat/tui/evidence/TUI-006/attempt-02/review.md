# TUI-006 review — choosers lane (campaign/tui-choosers-2)

Reviewer: Opus 5, independent review worktree `/home/demfabris/dev/zz-tui-choosers-review`
at `0382bca4cb7ca5d15b6a26025953cbebbc5a6fe6`.
Verdict: **approve-with-fixes**. One must-fix, five nits, no blocker.

## Reviewer verdict, verbatim

```json
{
  "lane": "choosers",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-006",
      "severity": "must-fix",
      "description": "The v102 history entry of knowledge/protocol/wire-protocol.md names only this cycle's three appends (ChooseTreeAction::ClearFilter, ChooseBufferAction::FilterPrompt/ClearFilter, ChooseBufferState.prompt). The branch also carries cycle 5's chooser appends, which have never been on main and therefore land at v102 too: EventPayload::ChooserPresentation { presentation: Option<Box<ChooserPresentation>> } (the 51st EventPayload variant, tail tag 52) with its new payload types ChooserPresentation, ChooserRow, ChooserPreview, ChooserPreviewSize and ChooserPreviewTile, plus ChooseTreeAction::PreviewCycle and ChooseBufferAction::PreviewCycle. git log -S confirms commit 86afc2c2 added every one of them and touched no doc; the only wire-doc commit on the branch is bbb46ddb. Every other EventPayload append in this document is recorded with its tail tag (v80 tag 49, v81 tag 50, v91 tag 51), and the v101 entry ends 'Shipped in zz 0.8.0, so 101 is closed', so after this merge a reader of the history sees no record that the chooser event exists on the wire at all. FAILURE: a client author implements v102 from the version history, never handles tail tag 52, and fails to decode every frame the daemon publishes on a chooser open. The appends themselves are correct — all at the end of their enums, consumer halves present in the same push, PROTOCOL_VERSION 102 with the message.rs assert and both hunt_claims pins moved, never above 102 — which is why this is must-fix and not a reject.",
      "suggested_fix": "Extend the v102 entry in knowledge/protocol/wire-protocol.md with a sentence naming EventPayload::ChooserPresentation at tail tag 52, the ChooserPresentation/ChooserRow/ChooserPreview/ChooserPreviewSize/ChooserPreviewTile payload types the daemon fills from the mode tree, and ChooseTreeAction::PreviewCycle plus ChooseBufferAction::PreviewCycle appended at the end of their enums for the v key."
    },
    {
      "obligation": "TUI-006",
      "severity": "nit",
      "description": "compat/tmux-gaps.json, closed item choosers.native-presentation: the resolution still reads 'The daemon publishes a ChooserPresentation event after every chooser state or delta (PROTOCOL_VERSION 101: the rows' mti->name and expanded row format cached at rebuild, ...)'. That event lands at 102 on this branch, not 101. FAILURE: the generated knowledge/tmux/gaps.md tells a reader the chooser presentation event is a v101 field, i.e. that a shipped 0.8.0 client already speaks it.",
      "suggested_fix": "Change '(PROTOCOL_VERSION 101:' to '(PROTOCOL_VERSION 102:' in that resolution and regenerate with python3 compat/tmux-tracker.py write-report."
    },
    {
      "obligation": "TUI-006",
      "severity": "nit",
      "description": "compat/tui/evidence/TUI-006/attempt-02/notes.md: the run-table row for screen-diff-regression.txt says 'exit 0, 125 checkpoints identical', but the banked file's last line reads 'all 137 asserted checkpoints identical, 16 recorded not asserted' after the rebase onto the caps landing. The prose two paragraphs below the table gives 137/16 correctly, so only the table cell is stale. FAILURE: a gate reading the evidence index sees 125 against a file that says 137 and has to open the file to learn which is the run.",
      "suggested_fix": "Update that one table cell to 'exit 0, 137 checkpoints identical, 16 recorded'."
    },
    {
      "obligation": "TUI-006",
      "severity": "nit",
      "description": "compat/tmux-gaps.json edits reach keys.native-defaults, outside the three gaps the batch names (choosers.native-presentation, clients.tui-command-output-navigation, clients.command-output-pane-prompt); six native-key items were added. I verified the justification rather than taking it: at cycle 5's carried commit 86afc2c2 the key table gained choose-tree v, choose-tree f and choose-buffer v while compat_manifest_tests.rs still asserted zz_keys 311 / native_keys 85, so the partition assert was already red at the branch's start; commit dd25c71b added the other three keys and moved both the gap items and the counts (317 / 91) in one commit, which is what the ledger rule's parenthetical asks for. Nothing was closed there and the reason carries a dated measurement. FAILURE: none technical; a gate applying the zone list literally has to reconstruct why the edit was forced.",
      "suggested_fix": "No change needed. Worth the gate confirming no other lane claims the same six native-key items before merge."
    },
    {
      "obligation": "TUI-006",
      "severity": "nit",
      "description": "Punch-list item 1 asked for a sabotage per divergence. --self-check gained one for -Z zoom and one for the buffer tree's filter prompt, but nothing dedicated to c / clear-filter. I closed the gap myself: I made ChooseTreeAction::ClearFilter and ChooseBufferAction::ClearFilter in crates/zz-daemon/src/daemon.rs clear the prompt but not self.filter, rebuilt, and the fixture reported exit 1, '2 of 53 asserted comparisons differ', exactly DIFF filter-cleared and DIFF buffer-filter-cleared, with the row dump showing the zz tree still filtered and every other case green. So the cases do fail for the right reason; only the packaged sabotage is missing. FAILURE: a later lane breaks c, runs only --self-check rather than the full fixture, sees every sabotage caught and believes the clear-filter channel is covered.",
      "suggested_fix": "Add a self-check case that types c into the pin's window tree alone after a filter, expecting rows; or say in the fixture header that clear-filter rides the row channel."
    },
    {
      "obligation": "TUI-006",
      "severity": "nit",
      "description": "crates/zz-mux/tests/hunt_claims.rs and crates/zz-protocol/tests/hunt_claims.rs are touched and neither is in the lane's zone spelling (which names crates/zz-mux/src/ consumer lists and compat_manifest_tests.rs, and crates/zz-protocol/src/ for appends). Both edits are the wire rule's own pins: zoom: false added to the MuxEffect literals, prompt: String::new() added to the ChooseBufferState literal, the pinned bytes [0,0,3,1,0] -> [0,0,3,1,0,0], and the version test renamed to protocol_version_on_this_commit_is_one_hundred_and_two. FAILURE: none at runtime; a zone audit reads two files outside the list.",
      "suggested_fix": "No change; the wire rule requires the version and byte pins to move with the append."
    }
  ]
}
```

### Reviewer checks_run, verbatim

```
setup: fetched origin main + campaign refs; origin/main e63c5333 is an ancestor of the tip, so three-dot and two-dot diffs agree; checked out --detach 0382bca4 in /home/demfabris/dev/zz-tui-choosers-review (clean before and after)
build at tip: touched crates/**/*.rs and Cargo.*, CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-choosers/target, cargo build -p zz through the two-slot lock at MemoryMax=5G -> exit 0
compat/tui-choosers.sh at tip -> exit 0, 'all 53 asserted comparisons identical, 4 recorded not asserted (0 for a sibling lane)', 2m57s
compat/tui-choosers.sh --self-check at tip -> exit 0, 6 sabotages caught (tag, preview, row, cursor, zoom, buffer filter prompt) and both equivalences quiet; byte-identical to all four banked self-check files (md5 fbedeb4b2688f73d29683d5a89c8740e), which is what makes four identical banked runs credible rather than suspicious
compat/tui-copy-mode.sh at tip -> exit 0, 'all 147 cases agree on every channel they assert, 0 recorded a difference elsewhere' — the fixture that governs the mode_view.rs over_underlay change
compat/tui-screen-diff.sh at tip -> exit 0, 'all 137 asserted checkpoints identical, 16 recorded not asserted'
compat/tui-stock-keys.sh at tip -> exit 0, 'all 50 cases agree on every channel they assert, 8 recorded a difference elsewhere' (8 is the wobble the box note and the lane both name)
compat/attached-client.sh at tip -> exit 0, 'attached-client compatibility: PASS'. My first attempt returned exit 124 purely because I capped it at 400s; the fixture takes 7m30s on this box. Not a lane defect
own oracle spot-check, pin side: a pinned tmux inside an outer pinned tmux on throwaway sockets under a scrubbed HOME/XDG_CONFIG_HOME, all servers reaped. prefix w (choose-tree -Zw) -> status 'L0:win*Z' and tree row '+ 0: win*Z'; f -> '(filter) ' on the mode screen's own last row with the status row kept; '#{==:#{window_name},two}' + Enter -> one row; c -> filter cleared AND the rebuild rule visible: rows 0 and 2 return EXPANDED with their pane rows while the survivor '+ 1: two' stays collapsed; q -> zoom released. prefix = then f -> the same (filter) prompt on the buffer tree; Enter filters to alpha; c restores both buffers
own oracle spot-check, zz side at the review tip: byte-for-byte the same screens at every one of those steps, including the expanded-on-return rebuild rule and the Z flag reaching both the tree row and the status row. Independent of the lane's fixture
my own sabotage: ChooseTreeAction::ClearFilter and ChooseBufferAction::ClearFilter in crates/zz-daemon/src/daemon.rs made to clear the prompt but not self.filter; rebuilt; compat/tui-choosers.sh -> exit 1, '2 of 53 asserted comparisons differ', exactly DIFF filter-cleared and DIFF buffer-filter-cleared, row dump showing the zz tree still filtered. Reverted from a backup and rebuilt; worktree clean
cargo test -p zz-protocol / zz-mux / zz-tui / zz-client, each --jobs 4 -- --test-threads=3 through the lock -> exit 0 (222/7/16, 60+, 194, 87 and friends); confirmed option_format_hook_and_default_key_items_match_pinned_inventories actually ran
cargo test -p zz-daemon -> exit 0, 889 lib tests plus every integration binary, no flake (client_focus_closes_display_panes_and_preserves_chooser_modes passed)
cargo test -p zz -> exit 0, 638 lib plus cli_binary 125
cargo clippy -p zz-tui and -p zz-daemon --all-targets --all-features -- -D warnings -> exit 0 each
python3 compat/tui/tracker.py check -> exit 0, report current; python3 compat/tmux-tracker.py check -> exit 0, report current
evidence hygiene: git check-ignore -v over compat/tui/evidence/TUI-006/attempt-02 -> nothing ignored; no .log file anywhere under the evidence tree; run-05/06/07 are three genuinely distinct runs (different ptys and clocks: 15:30, 16:02, 16:09); notes.md names every file in the directory; environment.txt carries host, revision, pin sha, toolchain and the binary hash
wire audit: PROTOCOL_VERSION 102 in message.rs plus the assert at message.rs:4902; ChooseTreeAction::{PreviewCycle,FilterPrompt,ClearFilter}, ChooseBufferAction::{PreviewCycle,FilterPrompt,ClearFilter} and EventPayload::ChooserPresentation each verified to be the last variants of their enums by parsing the enum bodies; ChooseBufferState.prompt appended after help; hunt_claims byte pin [0,0,3,1,0] -> [0,0,3,1,0,0]; version test renamed to ..._one_hundred_and_two per the conflict map
rebase audit: git diff origin/main...HEAD on crates/zz-tui/src/render.rs mentions command_output nowhere, so modes' command-output removal survived; the paint() hunk is the choosers' paint_mode_tree match with no command-output branch re-added; the second hunk replaces hide_cursor with restore_mode_tree_cursor(model), and that helper yields None (hence hide_cursor) whenever no chooser is open, so the menu/confirm/popup cursor rule is intact. Seven render.rs hunks total, three of them in mod tests or the module declaration
zone audit: only crates/zz-mux/tests/hunt_claims.rs and crates/zz-protocol/tests/hunt_claims.rs fall outside the literal zone list, both wire pins; zero added // or /// lines anywhere under crates/ (the two new files, render/chooser.rs and daemon/chooser_presentation.rs, carry no comments at all); no attribution trailer in any commit body
gap-close audit: choosers.native-presentation moved to the closed section with closed_on 2026-09-10 and a resolution carrying the zoom, the two keys, the rebuild rule, the six declared zz-only key rows, the 53-comparison count, the 'or its first under status-position top' phrase punch item 3 asked for, and an explicit GUI key-change declaration; clients.tui-command-output-navigation and clients.command-output-pane-prompt untouched; keys.native-defaults gained six items with a dated reason and the compat_manifest_tests.rs counts moved with them in the same commit dd25c71b
GUI audit: cargo build -p zz and cargo test -p zz green; the only non-test GUI change is crates/zz/src/chooser/buffer.rs implementing the pre-existing ChooserSpec::prompt (whose default returns None and which tree.rs already overrode), so the GUI buffer chooser draws a prompt only when the newly bound f is pressed. Declared in the gap resolution
hygiene: no binary copied under /tmp (nothing over 50 MB created there during the review); my four probe sockets under /tmp/tmux-1000 checked dead and removed; scratch backups deleted; pgrep shows no zz or tmux process of mine — the live zz processes belong to the gate (zz-gate-tui6 / zz-gate-target) and were left alone
```

### Reviewer notes, verbatim

```
TUI-006 is at active on the branch, and that is the honest reading. Clause 3 asserts in full: 53 whole-screen comparisons at 80x24 and 100x40, all identical, 14 more than cycle 5, no marker-only expectation left, --self-check catching six channels. Clauses 1 and 2 stay open on four recorded cases (client-tree-open for the missing choose-client command, output-search-prompt and output-search-typed for the -P pane prompt, output-selected for the search marks under a selection). Zero SIBLING cases remain, which is what punch item 2 wanted, and every remaining record carries a dated pin citation rather than a shrug. Nobody should read this report as "TUI-006 can be verified"; it cannot, and the lane does not claim it can.

The punch list itself is done, and I checked each item rather than taking the lane's word.

Item 1, the three unrecorded divergences: all three are real fixes, not fixture edits. I drove the pin myself on throwaway servers and then drove zz at the tip through the identical script, and the two screens agree step for step on -Z zoom (status row and tree row both carry Z, released on q, a user's pre-existing zoom left alone), on choose-buffer f, and on choose-tree c. The daemon's hold_chooser_zoom reads the window's zoomed_pane before it zooms and only records what it took, which is mode_tree_zoom's rule at mode-tree.c:613 and 708. The lane also found and fixed the pin's rebuild rule while doing item 1 — a row a filter hid returns EXPANDED, the survivor keeps its state — and I reproduced that on both sides independently. That was found, not assigned, and it was fixed and asserted rather than recorded.

Item 2: output-shown, output-searched, find-window-prompt and find-window-typed all flipped to asserted and all pass. The find-window fix is the interesting one and it touches shared ground: over_underlay in crates/zz-tui/src/mode_view.rs now lets a filled row's fill take the front's trailing spaces. That is the copy lane's surface, so I ran compat/tui-copy-mode.sh at the tip rather than trusting the banked run — 147 cases, exit 0 — and tui-screen-diff.sh, 137/16. Narrowing the rule to over_underlay-with-a-fill is the right shape; the lane's first, unconditional version turned four copy-mode search-prompt cases red, and that failed run is banked honestly instead of deleted.

Items 3 and 4 are done and correctly refused respectively: the resolution phrase gained "or its first under status-position top", the GUI key change is declared in its own sentence, and the ledger stayed at active because the fixture does not have zero recorded cases.

Test honesty held up under pressure. The four byte-identical banked self-check files looked like a copy at first; my own self-check run produced the same md5, so that output is genuinely deterministic and the four files are four runs. run-05/06/07 differ in pty and clock, so they are three runs, not one banked three times as the lane's own prose clumsily suggests. My own sabotage landed on exactly the two cases it should and nothing else.

The one thing the gate has to apply before merging is the wire doc. This branch is the vehicle for cycle 5's chooser landing as well as cycle 6's, and cycle 5's landing put a whole new EventPayload variant and its payload family on the wire without a single line in the version history. Everything else about the wire is right, which is why I graded it must-fix rather than reject: the appends are at the end of their enums, the consumer halves are in the push, the version is 102 and never higher, and both byte pins moved. If your reading of "a zone or wire violation is a blocker" is stricter than mine, escalate it — the fix is four sentences in one paragraph and costs the lane nothing.

Suspicion I could not turn into a defect, for the record: the over_underlay trailing-space rule drops every trailing space of a front on a filled row, not just the last one. I could not construct a case where that is wrong — copy-mode, screen-diff, choosers and stock-keys are all green — but it is a shared-surface rule owned by another lane, so the copy lane's next fixture run is worth watching. Separately, tui-stock-keys.sh read 8 recorded here as it did for the lane, so the 7 -> 8 wobble has settled at 8 rather than moving again under me.
```

## Gate actions (alienware/orchestrator, 2026-09-13)

This gate ran twice. A reboot at about 22:35 on 2026-09-12 killed the first run after it
had rebased the lane, applied the must-fix and both actionable nits, and taken nine
corpus chunks. That work survived on `gate-choosers` at `f45fd10c` with a clean tree,
and this run continued it rather than starting over: `git log --oneline origin/main..HEAD`,
`git status --short` and `git diff origin/main...HEAD --stat` all matched what the
killed run left, and every claim below was re-measured at the tip that was pushed.

While the gate was down, fabrico pushed `bf44823d`, the GPUI and CEF dependency refresh
(2259 lines of Cargo.lock, 48 of Cargo.toml). The lane touches neither file, so
`git merge-tree --write-tree origin/main f45fd10c` predicted no conflict and the rebase
of all fourteen commits onto `bf44823d` took none. The dependency refresh means nothing
was inherited on trust: the whole corpus was re-run rather than the ten chunks the
killed run had not reached.

### The must-fix

Applied as its own commit, `Record the cycle-5 chooser wire appends in the v102 entry`.
The v102 paragraph of `knowledge/protocol/wire-protocol.md` now names
`EventPayload::ChooserPresentation { presentation: Option<Box<ChooserPresentation>> }`
at tail tag 52, the `ChooserPresentation`, `ChooserRow`, `ChooserPreview`,
`ChooserPreviewSize` and `ChooserPreviewTile` payload types the daemon fills from the
mode tree, and `ChooseTreeAction::PreviewCycle` plus `ChooseBufferAction::PreviewCycle`
at the end of their enums for the `v` key, with the sentence that these landed with
cycle 5 against a base carrying 101.

I re-ran the reviewer's wire probe rather than trusting the fix. Parsing the enum
bodies of `crates/zz-protocol/src/message.rs` at the pushed tip:
`ChooseTreeAction` ends `CommandPrompt, PreviewCycle, FilterPrompt, ClearFilter`;
`ChooseBufferAction` ends `PasteTagged, PreviewCycle, FilterPrompt, ClearFilter`;
`EventPayload` has 53 variants with `ChooserPresentation` at index 52, which is the
tail tag the entry names; `ChooseBufferState` ends `help` then `prompt: String`.
`PROTOCOL_VERSION` is 102 at message.rs:21 with the assert at message.rs:4906, and the
`hunt_claims` byte pins in `crates/zz-mux` and `crates/zz-protocol` pass in their own
package runs. Every append is at a tail, every consumer half is in this push, and the
version never goes above 102.

### The nits

Two were actionable and were applied in one commit, `Correct the chooser event's wire
version and one evidence cell`: `compat/tmux-gaps.json`'s closed
`choosers.native-presentation` resolution now dates the ChooserPresentation event to
PROTOCOL_VERSION 102, and `notes.md`'s run table reads 137 checkpoints and 16 recorded
for `screen-diff-regression.txt` instead of 125. `knowledge/tmux/gaps.md` was
regenerated, not hand-edited.

Nit 4 (the six `keys.native-defaults` items outside the batch's three gaps) asked the
gate to confirm no other lane claims those items. Confirmed: the five earlier cycle-6
gates are all on main and none of them touches `keys.native-defaults`. The addition was
forced rather than chosen, exactly as the reviewer reconstructed, and the
`compat_manifest_tests.rs` counts moved with it in the same commit.

Nits 5 and 6 asked for no change and got none. Nit 5's missing `c` sabotage is real but
the reviewer proved the two clear-filter cases fail for the right reason by breaking the
daemon himself; packaging that as a `--self-check` case is a lane's work, not a gate's,
and TUI-006's `next_action` already carries cycle 7's chooser work. Nit 6 is the wire
rule doing what it is supposed to do.

### One red the gate found and fixed

`compat/run.sh --strict-geometry smoke/keys-prefix-remainder` failed at the tip, twice,
including the run.sh retry alone: 1 OUT divergence and 2 WARN divergences, from
`keys-contract.py` timing out on `wait_for(lambda: b"Choose pane" in text())` after
`prefix f` and a `KEYS_FIND_SENTINEL` filter. That is this lane's, and it is a fixture
assertion the lane made stale rather than a behaviour it broke. The zz branch of that
step was waiting on the pre-mode-tree chooser's own heading; the raw TUI now draws the
pin's mode tree for find-window, so no such heading is written.

I reproduced it outside the harness before touching anything: driving
`keys-contract.py` by hand against a throwaway zz socket under a scrubbed HOME, the
chooser is up and filtered — the dump carries
`keyscontract (sort: index) (view: preview) (filter: active)` and both window rows —
and `#{pane_in_mode}` reads `0`, because a zz chooser is a client overlay and not a pane
mode. That is why the step has two branches at all, so the branch stayed. The same
script against the pin reads `#{pane_in_mode}` `1` and prints the identical
`filter: active` marker. The zz branch now waits for `filter: active`, which is a
stronger assertion than the old one: it proves the tree is up AND filtered, where
`Choose pane` proved only that a chooser opened and named the wrong kind for
find-window. `smoke/keys-prefix-remainder` is clean on every channel afterwards.
Committed as `Wait for the pin's filter line instead of zz's old chooser title`.

### Sibling flips

None. This lane reported no `sibling_cases` and `compat/tui-choosers.sh` reports
`0 for a sibling lane`, which is what the cycle's punch list asked for. The four cases
the fixture still records are blocked on a missing command (`choose-client`) and on two
zones this lane did not hold (`crates/zz-terminal`, and `-P` plus the pane it targets on
the wire), not on a sibling's landing, so no earlier lane's merge unblocks them.

### TUI-006 stays active

The reviewer answered `every_clause_asserted: no` and the gate agrees. Clause 3 asserts
whole. Clauses 1 and 2 each hold recorded cases with dated pin citations. An accepted
gap is never evidence, so `commands.native-client-tools` does not turn `client-tree-open`
into a pass. TUI-006's dependencies are all verified on main (TUI-002, TUI-003, TUI-004),
so the only thing between this obligation and `verified` is its own recorded cases. The
`proof` block stays null and `next_action` now names cycle 7's roster lane.

No earlier-held obligation reaches the bar on this merge either: TUI-011 waits on
TUI-006, and TUI-009's 23 recorded rows are its own lane's.

### Everything green at the pushed tip

Tests, per package, each through the box's two-slot lock at MemoryMax=7G:
`zz-protocol` 222+7+16, `zz-mux` 525 plus its integration binaries, `zz-tui` 204,
`zz-client` 88+2+2, `zz-daemon` 893 lib plus every integration binary,
`zz` 638 lib plus `cli_binary` 125 — all exit 0, first try, no flake, including
`client_focus_closes_display_panes_and_preserves_chooser_modes`.
`cargo clippy --workspace --all-targets --all-features -- -D warnings` exit 0.

Corpus: 150 scenarios — the `origin/main..HEAD` delta selection for choose-tree,
choose-buffer and find-window, every keys and status scenario, and
`smoke/tui-client-input-backpressure` — run in sequential chunks. Every row clean except
`smoke/keys-prefix-remainder` (fixed above, then clean) and
`smoke/plugin-runtime-resurrect-restore`, which is one of the four rows this box records
as environmental.

Fixtures, all against the pinned tmux and the tip's own debug build:
`tui-choosers.sh` 53 identical / 4 recorded and its `--self-check` catching all six
sabotages; `tui-screen-diff.sh` 147 identical / 6 recorded and `--self-check`;
`tui-pane-geometry.sh` 6 identical; `tui-stock-keys.sh` 50 cases and `--self-check`;
`status-row.sh` under `LC_ALL=C LC_TIME=C` 14 identical; `tui-indicators.sh` 23 and
`--self-check`; `tui-copy-mode.sh` 147 and `--self-check`; `tui-caps.sh` 279 rows and
`--self-check`; `tui-overlays.sh` 48 and `--self-check`; `attached-client.sh` PASS.

Two counts moved under the gate and neither is a defect. `tui-screen-diff.sh` reads
147 asserted / 6 recorded against the lane's 137 / 16, which is the overlays and mux
landings flipping ten of their own cases on main. `tui-stock-keys.sh` records 9 where
the lane and the reviewer both read 8 and cycle 5 read 7; the ninth is
`100x24 application-reader` with the same `#{pane_current_command}` beat as the 80x24
one already on the list, which is the wobble the box note names. Exit 0 and no case
moved from asserted to failing in either fixture.
