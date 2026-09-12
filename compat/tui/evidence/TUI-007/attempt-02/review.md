# TUI-007, cycle 6 overlays lane: the review and what the gate did with it

The reviewer's verdict is reproduced verbatim below, then this gate's actions.

## Gate actions

VERDICT: approve-with-fixes, one must-fix, one nit.

### The must-fix: the wire rule (commit ad4444f0)

The review is right and its reading of the code checks out. Verified each append at
the gate tip rather than taking the review's word:

  crates/zz-protocol/src/message.rs:2672  DisplayPanesState { window, duration_ms,
      indicators, colour: Option<TmuxColour>, active_colour: Option<TmuxColour> }
  crates/zz-protocol/src/message.rs:2074  InputMessage::DismissClientMessage, last
      variant, after ClientFocus
  crates/zz-protocol/src/message.rs:2685  DisplayPanesAction::Dismiss, last variant,
      after Close

All three are pure end-appends and both consumer halves are in the tree. The daemon
fills the two colours at daemon.rs:28175 from a closure that reads the option through
the format engine and takes parse_tmux_colour, falling back to parse_style(...).fg,
so `None` when the option is neither a colour nor a style with a foreground: exactly
what the review's suggested sentence says.

The review asked for them in the v101 entry. Main has since moved to 102 (the caps
gate's 6bae21ff, because 101 shipped in zz 0.8.0 before either lane landed), so they
go in the v102 entry instead, as this gate's instructions direct. Both halves of
knowledge/protocol/wire-protocol.md are updated: the Overview paragraph and the
Versioning & compatibility bullet list. PROTOCOL_VERSION stays 102; no code changed.
python3 compat/tui/tracker.py check and compat/tmux-tracker.py check both exit 0.

### The nit: the fixture header overstating the *-under-message cases

Taken, and folded into commit 1a45ce91, which was already rewriting that header for
the defect below. menu-under-message passes 'OVERLAY-MESSAGE' with no trailing space;
only popup-under-message ends in two. The header now says so.

### What the gate found that the review did not: the trailing-space defect (1a45ce91)

Not the review's miss. compat/tui-copy-mode.sh went red at the rebase tip, 4 of 147
cases, because the copy gate flipped those four SIBLING:modes search cases onto the
rows channel at 33ecbd86 - after this lane was measured and after this review was
written. Neither the lane nor the reviewer could have seen it.

The lane trimmed trailing spaces off every message-area front. That matches the pin
only while a status row sits under the overlay. compat/tui-copy-mode.sh runs with
status off, where the pin paints the space. The fix trims only when
model.status_block_rows() > 0, and both halves are now pinned by unit tests. Full
measurement, both sides' bytes and the sabotage that isolates each half:
gate-trailing-space-defect.txt.

Note this also means the lane's existing pin test
trailing_prompt_spaces_are_left_to_the_fill was measuring the wrong half: its model
had no status row, so it was asserting the trim on the status-off path. It now sets
one.

### The review's caveat on clause 3, closed

The review approved clause 3 with a caveat: compat/attached-client.sh as the branch
carried it stopped in its first probe on the modes lane's copy-mode wait, and the
green came from origin/main's copy of the driver. The gate rebased onto the main that
carries the modes landing and ran the tree's own copy end to end: exit 0,
"attached-client compatibility: PASS", 435 s, no split. gate-attached-client.txt.
The caveat is closed; nothing was hiding behind it.

### Sibling flips

None to do. This lane's sibling_cases list is empty and compat/tui-overlays.sh has no
recorded case: 48 asserted, 0 recorded, confirmed at the gate tip.

### What the gate re-ran rather than trusted

Every proof the review cites, on the accumulated main: both overlays runs and the
self-check, the other six TUI fixtures with their self-checks, attached-client from
the tree's own copy, 206 corpus rows, and every cargo suite plus workspace clippy.
Numbers that moved from the review's are all explained by main: screen-diff 111
asserted / 42 recorded -> 137 / 16 (the modes and caps landings), caps 63 -> 279 rows
(the caps lane), stock-keys 19 recorded -> 12 (the box's wobble), copy-mode 147 with
0 recorded (the copy gate's flip), zz-tui 192 -> 204 (main's tests plus this gate's),
zz-daemon 864 -> 890, attached-client exit 1 -> exit 0.

### The three suspicions the review checked and cleared

Left as the review left them. The one worth repeating for the next reader: the
recorded run-1 and self-check outputs the lane committed reproduce byte-identically,
which the reviewer confirmed independently, so the recorded runs are real.

## The reviewer's verdict, verbatim

```json
{
  "lane": "overlays",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-007",
      "severity": "must-fix",
      "description": "WIRE RULE, judged on git diff origin/campaign/tui-cycle5-gated(5d9bf198)...HEAD as the invariants direct. The branch carries three protocol appends that no line of knowledge/protocol/wire-protocol.md's v101 version history names: (1) crates/zz-protocol/src/message.rs DisplayPanesState gains two trailing fields `colour: Option<crate::TmuxColour>` and `active_colour: Option<crate::TmuxColour>` after `indicators`; (2) the same file's `InputMessage` gains a trailing unit variant `DismissClientMessage` after `ClientFocus`; (3) `DisplayPanesAction` gains a trailing variant `Dismiss` after `Close`. Verified they are absent from origin/main (33ecbd86): `git show origin/main:crates/zz-protocol/src/message.rs` shows DisplayPanesState with only window/duration_ms/indicators and no DismissClientMessage, so these are new relative to what main carries. Everything else in the wire rule holds: PROTOCOL_VERSION stays 101 (message.rs:21, and its own test at message.rs:4793), all three are pure end-appends, and both consumer halves are in the tree (crates/zz-tui/src/overlay.rs:26 sends DisplayPanesAction::Dismiss, overlay.rs:53 sends InputMessage::DismissClientMessage, overlay.rs:278 reads state.active_colour; crates/zz-daemon/src/daemon.rs handles Dismiss in input_display_panes and DismissClientMessage in the input dispatch, input_dismisses_client_message, input_is_ignored_by_client_message and read_only_blocks_input). Only the history line is missing. `git diff 5d9bf198..HEAD -- knowledge/protocol/` is empty and `git diff 5d9bf198..7c692222 -- knowledge/protocol/` is empty too, so the gate's cycle-5 rebase of the overlays lane onto BASE carried the appends without the doc half, and this cycle did not notice. The v101 entry itself says 'Pure appends; the cycle's gate folds every lane's 101 appends into one entry', and the modes lane's appends ARE folded there — the overlays lane's are not. The lane's own evidence_note asserts 'WIRE: nothing changed this cycle; PROTOCOL_VERSION stays 101', which is true for 7c692222..HEAD but leaves the reader with no record of the three appends the branch delivers to main.",
      "suggested_fix": "In knowledge/protocol/wire-protocol.md, append to the v101 paragraph that ends '...and `modes` is capped at `MAX_MODE_PRESENTATIONS` (2)...' and to the matching 'Versioning & compatibility' v101 bullet, one sentence naming the overlays lane's appends: DisplayPanesState gains `colour` and `active_colour` after `indicators` (the resolved display-panes-colour and display-panes-active-colour for the client, `None` when the option does not parse as a colour or a style foreground), InputMessage gains `DismissClientMessage` (the raw TUI clearing a client message before a local overlay sees the key, the pin's server_client_handle_key order), and DisplayPanesAction gains `Dismiss` (closing the labels on a resize without typing an Escape into the pane). One commit in the gate's records push, no code change. Then re-run python3 compat/tui/tracker.py check."
    }
  ],
  "checks_run": [
    "SETUP: GIT_TERMINAL_PROMPT=0 git fetch origin main + campaign/* (exit 0); /home/demfabris/dev/zz-tui-overlays-review was clean, checkout --detach 5d335240a9fcca2a66ea9c51ac0da93d33e22ea9; verified BASE 5d9bf198 and START 7c692222 are both ancestors of the tip; worktree clean at start and at end; shared checkout /home/demfabris/dev/zz untouched and clean on main",
    "BUILD: touched every crates/**/*.rs and Cargo.* in the review worktree first, CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-overlays/target, systemd-run --scope MemoryMax=5G MemorySwapMax=2G flock slot -> cargo build -p zz --jobs 4 exit 0; sha256(target/debug/zz)=8ee410f5ba0fe8961b1b52b8986bd7f827beaebb038d64d4dcf081ac88cda7e1 (the review build; the worker's recorded b44f5479 is its own non-reproducible debug link of the same revision)",
    "PROOF: compat/tui-overlays.sh at the tip, 4 runs (3 + 1 final after restoring from my sabotages): every one exit 0, 'all 48 asserted comparisons identical, 0 recorded not asserted'. Run 1 output is byte-identical to compat/tui/evidence/TUI-007/attempt-02/tui-overlays-run-1.stdout.txt (diff empty) -> the recorded runs are real",
    "PROOF: compat/tui-overlays.sh --self-check at the tip, exit 0, 8 sabotages caught + 1 equivalence passed; output byte-identical to the recorded tui-overlays-self-check.stdout.txt",
    "PROOF: compat/tui-screen-diff.sh at the tip, exit 0, 'all 111 asserted checkpoints identical, 42 recorded not asserted' (2m21s)",
    "PROOF: compat/attached-client.sh from the TREE's own copy, TMUX_BIN=pin ZZ_BIN=tip -> exit 1, 'error: zz screen did not visibly become copy-mode within 10 seconds'. That is the known BASE red the batch pre-classifies as the modes lane's. Diff of the tree's copy against origin/main's is 3 changed lines in 2 hunks, both inside wait_for_visible_mode / wait_for_ordered_current_lines copy-mode detection; no overlay probe and no assertion touched",
    "PROOF: origin/main's compat/attached-client.sh (extracted to scratch, sha256 21ee7922f26568d391aa21035d21eb2be099a37fc5bc7e4fcf1ce5a33cc2e06a, matching what environment.txt records) against the tip's binary -> exit 0, 'attached-client compatibility: PASS', whole driver in one 7m50s run. Confirmed probe_command_prompt / probe_confirm_before / probe_display_menu / probe_display_popup / probe_display_panes_target_no_select are all invoked in that driver (lines 3868-3886)",
    "ORACLE SPOT-CHECK (lane-specific, my own harness): copied the fixture to scratch and added 8 cases the tree does not have -- a centred display-popup at the odd height 79x23 WITH a message shown ('ODD-MESSAGE  ', two trailing spaces) and after C-l clears it, for both the fg-only and the fg+bg style sets, and confirm-before at 79x23 -> 61x21 -> 79x23 -> refused. Result: 56 of 56 asserted comparisons identical, 0 recorded, exit 0",
    "ORACLE BYTES: dumped both sides' capture-pane -p -e at the centred popup at 79x23. The pin's popup blank rows read '\\e[38;5;159m\\e[48;5;17m' + 37 spaces + the border -- popup-style carried across the blanks, no ECH, no reset to the terminal ground -- which is exactly what the blit_row landing implements, and zz's dump is identical. With a message up, the pin's row 22 is '\\e[38;2;13;13;13m\\e[48;2;184;134;11mODD-MESSAGE\\e[39m' with the two trailing spaces NOT inside the message-style run, and zz's is the same byte for byte; cursor pin=21,9 flag=0 == zz=21,9 flag=0",
    "MY SABOTAGE A (punch-list item 1): neutralised only the blit_row `grounded_defaults` guard (`if false && grounded_defaults`), rebuilt, ran the fixture -> exit 1, exactly 8 DIFFs: popup-opened, popup-typed, popup-under-message, popup-resized, centre-popup-fg, centre-popup-fg-typed, centre-popup-fgbg, centre-popup-fgbg-typed -- the same 8 the punch list names. The failure bytes are the right cause: zz emits '\\e[39m\\e[49m' before the popup's blank run where the pin keeps '\\e[38;2;229;229;229m'",
    "MY SABOTAGE B: dropped only `.trim_end_matches(' ')` in status_overlay's message front -> exit 1, exactly 5 DIFFs: prompt-trailing-space, confirm-opened, confirm-resized, popup-under-message, popup-resized. A union B = the 11 the recorded tui-overlays-prefix.stdout.txt claims for the START tree, independently corroborated",
    "MY SABOTAGE C: reverted popup_client_geometry to the window extent (dropped the client_sizes read) -> the odd-height centring cases go red (centre-menu-fg, centre-menu-fg-down, centre-popup-fg, centre-popup-fg-typed, centre-menu-fgbg, centre-menu-fgbg-down, centre-popup-fgbg, centre-popup-fgbg-typed, centre-menu-mouse-opened). The odd-size cases really assert placement",
    "MY SABOTAGE D (punch-list item 2, confirm half): routed confirm through message-command-style instead of message-style -> exit 1, exactly 2 DIFFs, confirm-opened and confirm-resized, with row 23 showing the fg/bg swapped (pin '38;2;13;13;13m 48;2;184;134;11m' vs zz '38;2;184;134;11m 48;2;13;13;13m'). The confirm cases assert that confirm reads message-style, nothing else moves",
    "RESTORE: both sabotaged files restored from pristine copies, git status --short empty, rebuilt, sha256 back to 8ee410f5..., final fixture run green",
    "TESTS: cargo test -p zz-tui --jobs 4 -- --test-threads=3 exit 0, 192 passed (includes the two new pin tests popup_blank_rows_keep_the_popup_style_instead_of_an_erase and trailing_prompt_spaces_are_left_to_the_fill); cargo test -p zz-mux exit 0, 524 + 105 passed; cargo test -p zz exit 0, 634 unit + 125 cli_binary passed; cargo clippy -p zz-tui -p zz-mux --all-targets --all-features -- -D warnings exit 0; cargo build -p zz exit 0 (the GUI compiles)",
    "TESTS: cargo test -p zz-daemon -> 864 passed, 1 failed: daemon::tests::client_focus_closes_display_panes_and_preserves_chooser_modes. Reran it exact-solo 7 times: 3 red, 4 green; the panic payload carries the wall-clock '08:47 12-Sep-26'. That is the named 1-in-10 modes-lane flake, not this lane's and not caused by anything in this diff",
    "TRACKERS: python3 compat/tui/tracker.py check -> 'campaign.json is valid and knowledge/tmux/tui-parity.md is current'; python3 compat/tmux-tracker.py check -> 'tmux-gaps.json is valid and knowledge/tmux/gaps.md is current'",
    "ZONES (BASE...HEAD): compat/tmux-gaps.json, compat/tui/{campaign.json,README.md,evidence/TUI-007/...}, compat/tui-overlays.sh, crates/zz-tui/src/{app,input,lib,overlay,render}.rs, crates/zz-client/src/core.rs (test helper only), crates/zz-daemon/src/daemon.rs, crates/zz-protocol/src/{message,terminal_codec}.rs, crates/zz-mux/src/{command,compat_manifest_tests}.rs, crates/zz/src/workspace/view.rs (test helper only), knowledge/tmux/{gaps,tui-parity}.md. Every path is inside the batch's declared zones; nothing outside",
    "LEDGER OWNERSHIP: structural JSON diff of compat/tui/campaign.json across 7c692222..HEAD -- only TUI-007 changed, and only its evidence_note, next_action and sources; plus the top-level updated_on. Status was already 'review' at START, so the worker did not move it. proof is still null. No other obligation, no milestone, no baseline, no pin touched",
    "GAPS: structural diff of compat/tmux-gaps.json across 7c692222..HEAD -- one item removed from options.native-overlay-styles (option:message-style) and one new entry in `closed` (options.tui-message-style, closed_on 2026-09-11, with a dated measurement and a resolution naming status_message_redraw / status_prompt_redraw / pr->style). options.native-overlay-styles keeps its decision, its GUI presentation and option:message-command-style. The TMUX_OPTION_CONSUMERS 144->145 bump and the compat_manifest_tests.rs partition move (scope 46->47, tracked 36->35) are in the SAME commit 30134c35 as the gaps JSON and the regenerated knowledge/tmux/gaps.md",
    "FIXTURE HONESTY: `text` mode, EXCLUDE_ROWS, STATUS_TOP and message_rows() are fully removed -- grep finds no residue. verdict same -> compare_rows styled, which compares the whole capture-pane -p -e screen for all ROWS_UNDER_TEST rows plus the cursor tuple, with no exclusion path left. 43 verdict call sites, 5 of them inside the 2-label centred loop = 48 comparisons, 0 of mode `record`. Two new self-check sabotages use the new rows:<n> expectation, which requires the comparison to name exactly the message row and no cursor move, so a reintroduced exclusion would fail them",
    "TEST HONESTY: git diff --name-only 5d9bf198..HEAD has no .log file; git check-ignore -v on the whole evidence directory reports nothing ignored; no attribution trailer in any of the 11 commits (grep for co-authored / generated with / claude / codex -> none); no added code comment in crates/ either this cycle or vs BASE; evidence run-1 and self-check reproduce byte-identically against my own runs",
    "FORMAT: cargo fmt -p zz-tui -- --check names only render.rs:3055; that exact assertion line is present at BASE 5d9bf198 and at origin/main 33ecbd86, so it predates this lane, as claimed",
    "HYGIENE: pgrep -fa 'zz-cli-|zz-user|zzprobe' finds nothing of mine; no zzov.* scratch dir or zzov-*.sock left (the fixture's trap reaps its outer tmux, inner tmux, zz daemon, every pid under its scratch dir, its sockets and the dir itself -- read end to end and confirmed empirically after 8 runs); no binary copied under /tmp by me (/tmp total 728M, unchanged); the user's daemon on /run/user/1000/zz/default.sock (pid 91214) and the other agent's zz-gate-target servers on /tmp/zzc-373604.sock were left alone; the two /tmp/zzsd-diag.* dirs are from 2026-09-11 and are not mine"
  ],
  "every_clause_asserted": "yes, with one sibling caveat on clause 3 that the batch itself pre-classifies. Clause 1 (placement, clipping/wrapping, title/style, cursor, configured keys, size changes): asserted -- tui-overlays.sh 48/48 `same`, 0 recorded, four green runs by me at the tip, plus 56/56 on my extended copy. `same` is the whole decoded screen for every row plus the cursor tuple, with the exclusion machinery removed from the file entirely. Clipping is measured (prompt-wrapped, prompt-cursor-home, and my confirm at 61x21 where prompt_view clips to width); placement at an odd height is measured and my sabotage C proves those cases fail when the centring source changes. Clause 2 (nested surfaces/messages, keyboard and mouse cancellation/selection, restoration, no input to covered panes): asserted -- menu-under-message, popup-under-message and my two centre-popup-*-under-message cases, Escape and button-1 cancellation, the s shortcut, a centred -M menu chosen by a click, display-panes closed by a digit and by Z, restoration comparisons covering the pane each surface sat on, and a self-check sabotage that plants a keystroke only one side's covered pane receives. Clause 3 (reuse behavioural closures as regressions, independently measure complete output at the final candidate): asserted -- compat/tui-screen-diff.sh is green in-tree at the tip (111 asserted checkpoints identical, 42 recorded) and I re-ran it myself; compat/attached-client.sh, which carries probe_command_prompt, probe_confirm_before, probe_display_menu, probe_display_popup and probe_display_panes_target_no_select, exits 0 with PASS against the tip's binary when driven by origin/main's copy of the driver, which I extracted and ran myself end to end. CAVEAT: the copy of that driver in this tree still stops in its first probe on the copy-mode wait BASE is known to carry, which is the modes lane's -- modes is EARLIER than overlays in the gate order, the two copies differ in 3 lines of copy-mode detection and nothing else, and the batch itself says every other lane runs it and reports past that step. The gate must re-run the tree's own copy after rebasing onto the main that carries the modes landing before it sets TUI-007 verified; the worker's next_action already says exactly that.",
  "notes": "PUNCH LIST: all three items done at the tip, none re-recorded.\n\n(1) The popup red. The cause is named, not guessed, and I reproduced it: the modes landing's trailing clear in render.rs blit_row emitted an ECH after an SGR reset on a popup's blank run, so those cells came back on the terminal's default ground where the pin's popup_draw_cb gives the job's default cells popup-style. The fix stands the clear down when terminal_defaults carries an explicit fg or bg, which is set only in paint_popup (render.rs:1248) and reset immediately after (1250) -- I checked every reference to terminal_defaults, so the guard cannot leak into a normal pane blit. Neutralising only that guard gives exactly the 8 popup cases the punch list names, with the pin keeping the popup foreground across the blanks and zz resetting to default. All 8 assert at the tip and 0 cases are recorded.\n\n(2) The thirteen SIBLING:modes cases are all flipped to `same` and sibling_cases is empty, which matches the fixture: no `record` verdict remains and the `text` mode that carried them was deleted along with EXCLUDE_ROWS, STATUS_TOP and message_rows(). The four that needed work are separately provable: my sabotage D isolates the confirm-as-a-message half (wrong style option -> exactly confirm-opened and confirm-resized, fg/bg swapped), my sabotage B isolates the trailing-space half. The new prompt-trailing-space case is real, not decorative -- it is the only case that dies to B alone in the prompt channel.\n\n(3) Ledger is honest. TUI-007 sits at review (already there at START), the record still says it verifies only once TUI-004 is verified, and the evidence_note leads with what asserts and what remains rather than growing history.\n\nSUSPICIONS I CHECKED AND CLEARED, none of them defects:\n- The recorded tui-overlays run and self-check outputs were not rewritten by the last two commits (b1a56015 changed crates/zz-tui/src/input.rs and render.rs after them). Those outputs are deterministic on a full pass, so a re-run produces identical bytes and the unchanged file proves nothing either way; b1a56015 is a pure rustfmt rewrap (an import line and two test bodies) with no semantic effect, and I re-ran the fixture and the self-check at the tip and got byte-identical output. Substance is fine.\n- The worker report's clauses_proved says the three runs were 'at the final tip 5d335240'. The ledger's own evidence_note says b1747c04 and notes.md says 3d3b8871, which is the accurate pair. The report overstates by one commit; the ledger does not.\n- The worker report says main's driver 'differs in exactly three hunks'. It is 3 changed lines in 2 hunk blocks. Both are copy-mode detection, so the claim's substance holds.\n- crates/zz-daemon/src/daemon.rs popup_client_geometry now reads client_sizes. That is attempt-01's, explicitly accepted by the cycle-5 gate and carried verbatim in attempt-02/notes.md; I did not re-litigate it, and cargo test -p zz (634 + 125) plus cargo build -p zz are green, so the GUI has no presentation change.\n- compat/tui-screen-diff.sh's zz cursor inside '120x24-sidebar sidebar-shown' is informational in a case whose whole assertion is that zz differs; the summary was identical on my run and the recorded one.\n\nNIT, not worth a defect entry: the fixture's header block says 'the two *-under-message cases pass a message that ends in two spaces', but only popup-under-message does (tui-overlays.sh:781/783); menu-under-message at 704/706 passes 'OVERLAY-MESSAGE' with no trailing spaces. The behaviour is still measured -- sabotage B kills popup-under-message and popup-resized -- so this is a one-word documentation overstatement in the header, worth a line if the gate is touching the file anyway.\n\nWHY approve-with-fixes rather than approve: the single must-fix is a documentation half of the wire rule, inherited from the cycle-5 gate's rebase rather than introduced here. The appends themselves are pure, appended at the end, and both consumer halves are present and exercised; nothing about the code needs to change. It is one paragraph in the gate's records commit.\n\nWHY not reject: nothing at the tip is red that is this lane's. Every fixture and every test I ran is green, the one daemon failure is the named modes flake (3 red / 4 green exact-solo, with a wall-clock value in the panic), and the one in-tree red -- the tree's copy of attached-client.sh -- is the modes sibling the batch pre-classifies, measured green here with the driver main already carries."
}
```
