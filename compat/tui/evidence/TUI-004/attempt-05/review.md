# TUI-004, cycle 6 mux lane: the reviewer's verdict and what the gate did with it

The reviewer's verdict JSON, verbatim:

```json
{
  "lane": "mux",
  "verdict": "approve",
  "confirmed_defects": [
    {
      "obligation": "TUI-004",
      "severity": "nit",
      "description": "PRE-EXISTING, NOT THIS BRANCH. The #{T:...} / #{E:...} re-expansion of an UNTERMINATED style section diverges from the pin. Probed by me at the tip on throwaway servers (scrubbed HOME, -L zzprobe-*, --socket /tmp/zzu*.sock) with status-left set to the eight bytes '#[fg=red' on both sides: display-message -p '#{T:status-left}' answers '#[fg=red' on the pin and the empty string on zz; the same for '#{E:status-left}', '#{T;=/0:status-left}' and '#{T;=/-N:status-left}' for every N I drove (37 of my 720 modifier answers, all of them in this one value). It is NOT caused by this lane: the plain trim of the same value, '#{=/-2:status-left}', answers '#[fg=red' on BOTH sides, and truncate_value plus the two new trim helpers are the only formats.rs code this branch changed, so an expansion that never reaches a trim cannot have been broken by it. The other 683 answers agree, over ten styled values covering escaped hashes, a style after an odd run of hashes, wide characters, a trailing style and a style-only value.",
      "suggested_fix": "Leave it out of this merge. Record it as a fixture case (or a next_action line) under whichever obligation owns the format engine's E/T expansion, with the measurement above; format-draw.c's format_width returns 0 for an unterminated section but format.c's expansion still hands the raw bytes back, and zz's expansion drops them."
    },
    {
      "obligation": "TUI-004",
      "severity": "nit",
      "description": "The evidence_note's account of the probe is wrong in one clause. It says MuxEngine::resize_window_to_extent goes 'through state.resize_window, which clears last_extent_probe so the next client report cannot back-solve the old extent'. Clearing last_extent_probe (crates/zz-mux/src/model.rs:1508) does the opposite of what that sentence claims: set_pane_geometry (crates/zz-mux/src/command.rs, the 'probe: block) SKIPS a report whose (pane, columns, rows) equals last_extent_probe, so clearing it makes the next identical report processable rather than blocked. What actually holds the resize is that the daemon pushes the new pane geometries as DeferredTerminalCommand::Resize in the same effect drain, so the client's next TerminalGeometry report carries the new size and back-solves to the extent the window already has. That leaves a theoretical race on a stale in-flight report at crates/zz-daemon/src/daemon.rs:16571, where a client report reaches set_pane_geometry unconditionally. I saw no instance: five settled full runs of tui-screen-diff.sh and my own 12-step probe at 80x10 and 80x6 all agree with the pin.",
      "suggested_fix": "Reword that clause of TUI-004's evidence_note at the gate to say the resize survives because the daemon pushes the new pane geometries in the same drain, not because the probe suppresses anything. No code change."
    },
    {
      "obligation": "TUI-004",
      "severity": "nit",
      "description": "tui-screen-diff.sh compares the decoded screen and the cursor, never a pane height, so the window-sizing landing is asserted only where content is taller than the box it is painted into. The worker measured this themselves: their first status-rows sabotage, without the eight-line fill, produced no row difference at all and is kept as evidence (attempt-05/screen-diff-self-check-item1-first-sabotage.txt). tui-pane-geometry.sh does compare the numbers, 6 of 6 green at the tip, but drives no status-row change, so no proof surface pins the pane heights under status 2 directly.",
      "suggested_fix": "Optional hardening for a later cycle: add a tui-pane-geometry.sh measurement at 80x10 with a two-pane split under status 2, asserting %0 h=3 beside %1 h=4 against the pin. Not a merge condition; my own probe already measured those numbers on both binaries."
    }
  ],
  "every_clause_asserted": "yes. Clause 1: tui-screen-diff.sh asserts every canvas checkpoint at all six sizes on two full runs of my own with dispositions identical to the worker's three, tui-pane-geometry.sh is 6 of 6, and the sidebar case asserts both the difference and the return. Clause 2: tui-screen-diff.sh now holds no recorded checkpoint that TUI-004 owns, and I verified the flip is real rather than declared by reverting each landing and watching exactly the right checkpoints go red. The 18 records that remain are default-fg (6), colour-classes (6, one of them a text-mode record whose styles happened to match) and cursor-style-request (6). I checked the ownership claim rather than taking it: TUI-009's acceptance clause 3 is literally \"Preserve named/indexed/RGB color behavior under palette changes\" and its next_action names \"the default-foreground divergence\", so both belong to the caps gate, which is EARLIER than mux in the gate order; cursor-style-request is the cursor channel and is carried in TUI-002's record too. Their reasons are untouched by this branch. status-row.sh is 14 of 14 with none recorded under LC_ALL=C LC_TIME=C. Clause 3: tui-indicators.sh 23 of 23 with 0 recorded and attached-client.sh PASS, both run by me at the tip. No case is held on a sibling and sibling_cases is correctly empty."
}
```

## The reviewer's checks_run, verbatim

1. setup: fetched origin main + campaign refs; origin/main has moved to e63c5333 (caps gate) but the merge-base with the branch is 33ecbd86, so every zone/wire judgement below is git diff 33ecbd86...55487e10 (three dots)
2. checkout --detach 55487e10 in /home/demfabris/dev/zz-tui-mux-review; touched every crates/**/*.rs plus Cargo.*; CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-mux/target; cargo build -p zz -> exit 0
3. read the full branch diff by hand: crates/zz-mux/src/{command,formats,status}.rs, crates/zz-daemon/src/daemon.rs, compat/tui-screen-diff.sh, compat/tui/, knowledge/tmux/tui-parity.md
4. read the pin's format-draw.c format_trim_left, format_trim_right, format_width and format_leading_hashes, and status.c status_line_size and resize.c:452 (CLIENT_STATUSOFF when c->tty.sy <= s->statuslines); compared them line by line with the new Rust format_trim_left/format_trim_right and interactive_client_window_extent
5. compat/tui-screen-diff.sh at the tip, TWICE -> exit 0 both, 'all 135 asserted checkpoints identical, 18 recorded not asserted'; my two runs' dispositions are identical to each other AND, checkpoint by checkpoint, to all three of the worker's evidence runs
6. compat/tui-screen-diff.sh --self-check at the tip -> exit 0, 16 expectations, both new sabotages caught in the rows channel (status-rows: pin FILL-7 vs zz FILL-6; styled-trim: pin draws LEFT, zz draws nothing)
7. compat/tui-pane-geometry.sh -> exit 0, all 6 asserted measurements identical
8. LC_ALL=C LC_TIME=C compat/status-row.sh -> exit 0, all 14 comparisons identical, none recorded
9. compat/tui-indicators.sh -> exit 0, all 23 asserted comparisons identical, 0 recorded
10. compat/attached-client.sh -> exit 0, PASS (first attempt returned 124 on MY 400 s timeout with the other agent's corpus run loaded; it needs more than that on this box, it did not fail)
11. ORACLE PROBE, item 1, written by the reviewer: outer pinned tmux, isolated HOME and XDG_CONFIG_HOME per side, -L zzprobe-* -f /dev/null and --socket /tmp/zzp*.sock, bounded wait_for on the marker plus a two-poll settle, trap reaps everything. At 80x10 and 80x6 with a real two-pane split they drove status 1 (after split), status 2, status 3, status off, status 2 + status-position top, and back to status on, reading #{window_width}x#{window_height} and every pane's #{pane_id} #{pane_width}x#{pane_height} through display-message -p AND capturing the whole decoded screen. 12 steps, 12 GEOM AGREE, 12 SCREEN AGREE. The pin's numbers they measured themselves: 80x10 status 2 -> window 8, %0 h=3, %1 h=4; status 3 -> window 7, 2 beside 4; off -> 10, 4 beside 5; top -> 8, 3 beside 4; back to on -> 9, 4 beside 4. 80x6 status 2 -> window 4, 1 beside 2; status 3 -> 3, 1 beside 1; off -> 6, 3 beside 2; top -> 4, 2 beside 1; back to on -> 5, 3 beside 1.
12. ORACLE PROBE, item 2: a styled status-left and a styled status-right longer than their length limits, driven on both sides with a client attached, comparing the display-message answer AND the drawn status row. status-left '#[fg=red,bold]LEFT' at status-left-length 10 -> both answer '#[fg=red,bold]LEFT' and the drawn rows are byte for byte the same. status-right '#[fg=green,bold]RIGHTSIDE' -> both agree on answer and row.
13. ORACLE PROBE, bulk trim: 720 modifier answers (=/N, =/-N, =/N/.., =/-N/.., p N, p -N for twelve N) over ten styled values on both binaries -> 683 agree, 37 differ, and all 37 are the pre-existing #{T:}/#{E:} unterminated-style divergence reported as nit 1
14. TEST HONESTY, revert 1: made the daemon's MuxEffect::StatusRowsChanged arm a no-op, rebuilt -> status_rows_resize_every_window_of_an_interactive_clients_session FAILS with left Some(9) right Some(8), and a_client_no_taller_than_its_status_block_keeps_every_row still passes (the two landings are independently pinned). tui-screen-diff.sh over 80x10 and 80x6 -> exactly 4 DIFFs and nothing else: status-two-rows and status-top at both sizes, zz's screen a row out of step with the pin's.
15. TEST HONESTY, revert 2: restored the daemon, put truncate_value back on the byte count, rebuilt -> a_style_section_costs_a_trim_no_column_the_way_format_trim_left_does FAILS with left "#[fg=red,b" right "#[fg=red,bold]LEFT". tui-screen-diff.sh over 80x10 and 80x6 -> exactly 2 DIFFs and nothing else: styled-left-trim at both sizes, pin drawing LEFT where zz draws nothing.
16. THE REVIEWER'S OWN SABOTAGE: at the flipped styled-left-trim checkpoint, zz alone gets status-left-length 2 -> caught at both sizes (pin LEFT, zz LE). A first attempt at length 4 was correctly NOT caught, because the style costs no column and LEFT is exactly 4 wide, which is the pin's own behaviour rather than a hole in the checkpoint.
17. cargo test -p zz-mux --jobs 4 -- --test-threads=3 -> exit 0, 525 lib tests plus every suite green
18. cargo test -p zz-daemon --jobs 4 -- --test-threads=3 -> exit 0, 888 lib tests green; client_focus_closes_display_panes_and_preserves_chooser_modes passed, the known flake did not fire
19. cargo test -p zz --jobs 4 -- --test-threads=3 -> exit 0, 638 lib and 125 cli_binary green
20. cargo clippy -p zz-mux -p zz-daemon --all-targets --all-features --jobs 4 -- -D warnings -> exit 0
21. every cargo command through the two-slot flock with MemoryMax=5G; nothing hit the cap, nothing timed out on a slot
22. ZONES: git diff 33ecbd86...HEAD --name-only holds nothing outside the batch's list. No crates/zz-tui change of any kind, so no render.rs hunk to name.
23. WIRE: crates/zz-protocol is untouched, no message gained a field, no protocol doc edited. The branch still carries the base's PROTOCOL_VERSION 101 while origin/main is now 102 from the caps gate; the mux lane appends nothing, so it owes no v102 line and the gate picks up 102 on its rebase.
24. GUI: cargo build -p zz clean, and no presentation path moves. client_sizes is written from exactly two non-test sites, InputMessage::ClientTerminalSize (sent only from crates/zz-tui/src/app.rs:698 and :927) and the client-size-v1 hello fact (crates/zz-daemon/src/client.rs:1192, attached only when EndpointFactsScope includes terminal size, which connect_endpoint_with_prompts_and_terminal picks only for terminal_surface). A GUI client falls through to the terminal_geometries back-solve, which returns the extent it already holds, so resize_window_to_extent no-ops.
25. COMMENTS AND ATTRIBUTION: git diff over crates/ adds no // or /* line (one stale doc block was removed); no Co-Authored-By, no 'Generated with', no mention of claude in any of the six commit messages
26. LEDGER OWNERSHIP: a structural key-by-key diff of compat/tui/campaign.json between 33ecbd86 and the tip changes exactly five leaves, all under item 3 (TUI-004): status, evidence_note, next_action and two appended sources. No other obligation, no proof block, nothing set to verified. compat/tmux-gaps.json untouched.
27. TRACKERS: python3 compat/tui/tracker.py check -> 0; python3 compat/tmux-tracker.py check -> 0; python3 -B compat/tui/tracker_test.py -> OK, 8 tests. Re-running both write-report at the tip leaves git status empty.
28. EVIDENCE: compat/tui/evidence/TUI-004/attempt-05/ has environment.txt first, 20 files, notes.md names every one of them (checked by script), no .log anywhere in the diff, git check-ignore over every added path finds nothing ignored. The test evidence captured at 451cf194 is attested: git diff 451cf194 55487e10 -- crates/ is empty.
29. COMMITS: one per punch-list item, scope confirmed per commit (efd9beb2 sizing, df1dedcb trim, 881e0b35 records, 451cf194 the short-client rule in daemon.rs alone, d7a7982f its records, 55487e10 a fixture comment fix)
30. HYGIENE: no binary copied under /tmp; every probe used a scratch HOME under /tmp with -L zzprobe-* -f /dev/null and --socket /tmp/zz*.sock and a trap; at the end no zzrev/zzprobe process or scratch directory of the reviewer's survives. The review worktree is clean at 55487e10.

## What the gate did

The verdict is **approve**: no must-fix list, so nothing was owed. The gate did not take the review on
trust either. Every proof it cites was re-run here on the accumulated main, and the numbers that moved
from the review's are all explained by what landed between the review and this rebase:

| Fixture | Review, at the lane tip | Gate, at the rebased tip | Why |
|---|---|---|---|
| tui-screen-diff.sh | 135 asserted / 18 recorded | 147 asserted / 6 recorded | the caps gate flipped default-fg and colour-classes (12 checkpoints) between the review and this rebase |
| tui-stock-keys.sh | not run | 50 agree / 11 recorded | the box's root-binding-detaches wobble moves the recorded count; the copy and caps gates saw 12, the modes gate 13 |
| zz-daemon --lib | 888 | 892 | tests main added |
| tui-caps.sh | not run | 279 asserted | the caps landing |
| tui-overlays.sh | not run | 48 asserted / 0 recorded | the overlays landing |

The six checkpoints still recorded at this tip are cursor-style-request, one per size: the pin forwards a
DECSCUSR a pane asked for and zz has nowhere to carry the request. That is the cursor channel, and
TUI-002 — verified — is the record that carries it. No checkpoint inside a TUI-004 clause is recorded any
more, which is the batch's stated bar, so TUI-004 is set verified here.

### The three nits

1. **The pre-existing `#{T:}`/`#{E:}` divergence on an unterminated style section.** Left out of this merge,
   as the reviewer asked. It is written into TUI-004's next_action with the measurement, for whichever lane
   owns the format engine's E/T expansion. The gate checked the reviewer's reasoning rather than the
   conclusion: `truncate_value` and the two new helpers are the only formats.rs code this branch changed,
   and the plain trim `#{=/-2:status-left}` of the same value agrees on both sides, so an expansion that
   never reaches a trim cannot be this lane's.

2. **The wrong clause in the evidence_note about `last_extent_probe`.** Verified at the source and fixed
   here. `set_pane_geometry` (crates/zz-mux/src/command.rs, the `'probe:` block) breaks out `false` when
   `window.last_extent_probe == Some(probe)`, so clearing the field in `Model::resize_window`
   (crates/zz-mux/src/model.rs:1508) makes the next identical report *processable*, not blocked — the
   opposite of what the sentence claimed. The evidence_note now says what actually holds the resize: the
   daemon pushes the new pane geometries as `DeferredTerminalCommand::Resize` in the same effect drain, so
   the client's next TerminalGeometry report already carries the new size and back-solves to the extent the
   window has. No code changed.

3. **No proof surface pins a pane height under status 2.** Agreed and carried into TUI-004's next_action as
   optional hardening for a later cycle, with the numbers the reviewer measured on both binaries
   (80x10 status 2: window 8, %0 h=3 beside %1 h=4). Not a merge condition, and not something to spend an
   oracle edit on at a gate.

### Sibling flips

None to do: this lane's `sibling_cases` list is empty and no fixture on main at this tip carries a
SIBLING case this lane owns. compat/tui-copy-mode.sh — where the SIBLING:modes cases lived — reports 147
cases with 0 recorded here, so the copy gate's flips held.

### The short-client finding

The lane found, and landed half of, a divergence no punch-list item names: a client of 5, 4, 3 or 2 rows
whose status block would leave the window one row tall keeps its full height on zz where the pin gives it
one. The daemon half landed (`interactive_client_window_extent` now carries resize.c's CLIENT_STATUSOFF
rule) and is pinned by `a_client_no_taller_than_its_status_block_keeps_every_row`. The other half needs a
rule in crates/zz-tui, outside this lane's zones, and a fixture size below 80x6, outside its fixture zone.

The gate weighed whether this keeps TUI-004 open and decided it does not. No fixture drives a client
shorter than six rows, so it is not a recorded case anywhere; the clause-1 comparison covers small heights
at 80x6 and 80x10 and every asserted checkpoint agrees there; and the branch strictly shrank the divergence
set (three of the four measurements converged). It stays named in next_action so it is not lost.
