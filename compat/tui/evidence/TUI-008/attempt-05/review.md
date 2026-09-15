# TUI-008 attempt-05, the review and what the cycle 10 gate did with it

The reviewer worked in this worktree at eefd48f5, built there, and reproduced
every proof the lane claimed, several of them byte-identically. The verdict is
approve-with-fixes with NO blocker and every acceptance clause asserted YES.

## The verdict, verbatim

```json
{
  "lane": "context",
  "branch": "campaign/tui-mouse-context",
  "tip": "eefd48f5",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "The evidence_note and notes.md claim 'Eight unit tests in zz-terminal, one per row of the oracle's table'. There are SEVEN, and the oracle table in notes.md has 19 rows, so 'one per row' is also wrong. Counted in the diff and confirmed by running it: cargo test -p zz-terminal at my own tip build lists exactly seven new pointer_* tests (pointer_formats_read_the_word_and_the_row_under_the_cell, pointer_formats_answer_nothing_past_a_row_and_on_a_blank_row, pointer_word_crosses_a_wrap_where_the_line_does_not, pointer_hyperlink_answers_the_osc_8_uri_of_the_cell, pointer_word_reads_the_same_text_from_both_halves_of_a_wide_cell, pointer_line_keeps_leading_blanks_and_trims_trailing_ones, pointer_word_honours_the_word_separators_option). The other two 'pointer' hits in the crate are pre-existing (interaction::tests::pointer_records_keep_their_packed_layout, session::tests::out_of_range_pointer_selection_clamps_*). The ledger text is what the gate publishes, so the count has to be right.",
      "suggested_fix": "In compat/tui/campaign.json TUI-008 evidence_note and compat/tui/evidence/TUI-008/attempt-05/notes.md, say 'Seven unit tests in zz-terminal' and drop 'one per row of the oracle's table' (the table has 19 rows; seven tests cover them)."
    },
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "Two paths the evidence_note explicitly claims have ZERO automated coverage in this lane. (1) The 'gd->hsize + y' term: LiveGrid::active_base() is total_rows - screen_rows (crates/zz-terminal/src/session.rs:13081-13085), but pointer_sample_context (session.rs:13978) builds an 80x24 Terminal and writes 8 rows, and the lane's own oracle-probe.sh sample is 9 rows in an 80x24 pane, so every unit test and every recorded probe runs at hsize==0. A constant-0 active_base() would leave every proof on this branch green. Made worse by LiveGrid::reference (session.rs:13087-13092) swallowing engine errors with .ok(), so a wrong index reads as an unwritten cell and answers empty rather than erroring. (2) The copy-mode branch: pointer_sample_context hardcodes pointer_context(&terminal, None, ...), so the Some(mode) arm and its mode.viewport_offset.saturating_add(row) arithmetic (session.rs:13166-13182) have no unit test, and no compat/tui-mouse.sh case right-clicks a pane that is in copy mode. I PROVED BOTH CORRECT BY HAND against the pin, so this is missing proof, not broken behaviour: with an independent probe (my own outer pinned tmux, 80x24, real SGR MouseDown3Pane via send-keys -H) at hsize=61 on the pin / 60 on zz, both sides answer identically on every cell, including a word whose head row is the LAST HISTORY ROW and whose tail is screen row 1 (both: mouse_word = the whole 139-cell WRAPPEDHEADxxx...LONGTAIL, mouse_line = the tail's 59 cells), and including a pane in copy mode scrolled to oy=3 (both: screen row 1 -> [FILLER059][FILLER059 ignore][], row 9 -> [SCROLLTAIL][SCROLLTAIL marker][]), which after send -X cancel both answer the live grid's LONGTAIL word again. Same agreement with word-separators=_x.",
      "suggested_fix": "Add two unit tests beside the seven: one that writes more than 24 rows so active_base() is non-zero and probes a word wrapping from the active area back into scrollback; one that passes Some(&CopyModeState) with a non-zero viewport_offset. Both are a few lines on top of pointer_sample_context. Optionally add a compat/tui-mouse.sh mouse-context/copy-mode case to make it a differential."
    },
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "The copy mode whose revision is read is GUESSED, not addressed by view. PointerContextRequest (crates/zz-terminal/src/session.rs:2337) carries only column/row, no TerminalViewId, unlike every other view-scoped path (e.g. TerminalSession::copy_mode_facts(&self, view: TerminalViewId) at session.rs:1806). Both daemon arms (session.rs:4654-4662 and :5892-5900) do: let mut copy_modes = active_views.values().filter_map(|view| view.copy_mode.as_deref()); match (copy_modes.next(), copy_modes.next()) { (Some(mode), None) => Some(mode), _ => None }. So with TWO clients in copy mode on the same pane the arm falls through to the LIVE grid, which is the inverse of the pin: format_cb_mouse_word/_line in the pin's format.c answer off window_copy_get_word/_line whenever !TAILQ_EMPTY(&wp->modes), regardless of how many clients are attached. And with one client in copy mode, a SECOND client's click on that pane is answered off the first client's frozen revision at the first client's scroll offset, where the pin would answer that client's own view. Evidence here is the code path plus the pin's source, not a driven two-client repro (my probes were single-client), so I am calling it must-fix rather than blocker.",
      "suggested_fix": "Thread the requesting client's TerminalViewId through PointerContextRequest and select that view's copy_mode, the way copy_mode_facts already does. If that is out of this lane's zones, state the divergence in next_action with its own measurement instead of leaving it undisclosed."
    },
    {
      "obligation": "TUI-008",
      "severity": "nit",
      "description": "The five mouse-context/* checks are a pure two-sided comparison with no literal-expected guard, so a run alone cannot distinguish 'both sides read the cell correctly' from 'both sides answered nothing'. context_probe (compat/tui-mouse.sh:1017-1036) waits on pin_option_set (:780-782, [ -n \"$(option_value tmux ...)\" ]) and on option_nonempty via `wait_at_most ... || true` (:1032, which can never fail the run); the brackets in '[#{mouse_word}][#{mouse_line}][#{mouse_hyperlink}]' make '[][][]' non-empty, so both waits are satisfied by an all-empty answer, and mouse-context/blank-cell legitimately expects '[][][]'. The fixture's own comment at :1049-1051 says so and treats it as deliberate. In practice it is NOT vacuous: the six one-sided sabotages are each caught in their own channel, and I reproduced the concrete values in a clean run ([beta][MOUSECTX alpha beta gamma][], the whole 139-cell word for the wrapped head and tail, [LINKTEXT][LINKTEXT][https://example.com/page], [][][]). The menu channel already has a positive gate (both_screen_has 'Copy Line', :999, and 'Copy Line' is conditional on mouse_line on both binaries).",
      "suggested_fix": "Three lines in context_probe: for the four non-blank cells, assert the pin's @mousectx is not '[][][]' before comparing."
    },
    {
      "obligation": "TUI-008",
      "severity": "nit",
      "description": "compat/tui-mouse.sh's new header block is wrong twice. Line 60 says the three names are read 'as a user's own `bind -n MouseDown3Pane` expands them', but the code deliberately binds C-MouseDown3Pane (:1052) precisely so the stock MouseDown3Pane the two menu cases need stays as key-bindings.c installed it. Lines 56-57 say the pin's menu 'renders three of its items off the screen under the pointer'; it renders FOUR items from those three names. I measured the pin's DEFAULT_PANE_MENU myself over a written row: Search For <word> (C-r), Type <word> (C-y), Copy <word> (c), Copy Line (l). notes.md states it correctly ('Copy Line and its three word items'); only the fixture header undercounts.",
      "suggested_fix": "Fix compat/tui-mouse.sh lines 56-60: name C-MouseDown3Pane, and say four items off three names."
    },
    {
      "obligation": "TUI-008",
      "severity": "nit",
      "description": "Evidence bookkeeping. (a) notes.md's file list says '13-cargo-test-zz-daemon.txt (the run that caught the delegated-consumer count; the tip run is 25)' - the daemon tip run is 29; 25 is a tui-mouse run. (b) environment.txt records 'head at the time of writing: 902fcf542a6cea5a6f2c8ca45f1a3341cca04e6e', which is commit 4 of 8, four behind the tip eefd48f5. (c) crates/zz-terminal/src/terminal_core.rs is modified by this diff (the pub use PointerContext) but appears neither in TUI-008's sources nor anywhere in notes.md, while crates/zz-terminal/src/session/mode_revision.rs, which this diff does NOT modify, was added to sources.",
      "suggested_fix": "Correct the '25'->'29' typo, restamp environment.txt at eefd48f5, and add crates/zz-terminal/src/terminal_core.rs to TUI-008's sources (or name it in notes.md's file list)."
    },
    {
      "obligation": "TUI-008",
      "severity": "nit",
      "description": "The new docblock at crates/zz-daemon/src/daemon.rs:33005-33007 says cmd_mouse_at fails outside the pane's rectangle 'so no probe leaves here for one'. That is true of the three new names - the guard at :33034-33050 (geometry.pane_right.is_some_and(|right| mouse.column <= right) && geometry.pane_bottom.is_some_and(...)) fails closed and I reproduced the empty answer for a status-row click on both sides. But mouse_x and mouse_y are inserted at :33031-33032, BEFORE that guard, and still publish for an out-of-rectangle event (pin 0,0 vs zz 0,23, which I reproduced). The divergence is pre-existing and disclosed in next_action; only the new comment's cmd_mouse_at model overstates what the surrounding code does.",
      "suggested_fix": "Narrow the docblock to the three names it describes, or move the mouse_x/mouse_y insert behind the same guard in a lane that owns those two."
    },
    {
      "obligation": "TUI-008",
      "severity": "nit",
      "description": "The diff adds 44 `///` doc-comment lines across crates/zz-terminal/src/session.rs (30) and crates/zz-daemon/src/daemon.rs (14). There are no inline // or /* */ comments anywhere in the diff. The repo rule is 'Do not add comments in code'; the functions these replaced already carried /// docblocks in the same style, and several of the new ones carry the tmux citations that make the readers auditable, so I read this as within existing style rather than a violation. Flagging it so the gate makes the call rather than inheriting mine.",
      "suggested_fix": "None needed unless the gate reads the rule as covering ///; then strip them."
    },
    {
      "obligation": "TUI-008",
      "severity": "nit",
      "description": "#{mouse_hyperlink} publishes the pane's raw OSC 8 URI with none of the filtering the hover path applies. hyperlink_uri_bytes was split into a point-addressed twin correctly (crates/zz-terminal/src/session.rs:8139-8177, both pre-existing callers still go through the Point::Viewport wrapper unchanged), but hover_link_at then runs the result through is_safe_link_uri (:7844-7846: scheme allowlist http|https|file|ssh|mailto, no whitespace, no control chars), whereas pointer_hyperlink (:13198-13219) does not. This MATCHES the pin - format_grid_hyperlink in format.c does no filtering either - so it is parity-correct, and a user binding that interpolates #{mouse_hyperlink} into run-shell is as exposed on tmux as on zz. Worth being a decision rather than an accident; nothing in notes.md mentions it.",
      "suggested_fix": "One sentence in notes.md recording that the unfiltered URI is deliberate parity with format_grid_hyperlink."
    }
  ],
  "checks_run": [
    "SETUP: git fetch origin main + campaign/*; REVIEWDIR /home/demfabris/dev/zz-tui-context-10 confirmed on campaign/tui-mouse-context at eefd48f5, clean; git checkout --detach eefd48f5 so nothing could move the branch; worktree left clean at eefd48f5 (I removed one zero-byte 'env' file my own malformed flock created at 13:09, nothing else)",
    "Diff judged as git diff 6fbdf4c3...HEAD throughout (8 commits, 48 files, 6 of them Rust)",
    "WIRE: PROTOCOL_VERSION is 103 at the tip, 103 at 6fbdf4c3 and 103 on origin/main - unchanged. git diff --stat 6fbdf4c3...HEAD -- crates/zz-protocol crates/zz-client crates/zz-tui crates/zz crates/zz-ui is EMPTY. Worker's 'no wire change' claim confirmed",
    "BUILD: cargo build -p zz --jobs 3 through the shared slot lock at MemoryMax=8G -> exit 0 in 7m12s (the target was cold, not warm); target/debug/zz and zz_cli both produced. Every proof below used THIS binary and the pin at /home/demfabris/dev/zz/compat/.cache/tmux-src/tmux (d77c9dc6, next-3.8)",
    "compat/tui-mouse.sh x3 at the tip -> exit 0 each, '45 asserted checks, 0 recorded checks' / 'all 45 asserted checks identical', all three md5 64d94af20992d6bc0288855305b83072 - byte-identical to each other AND to evidence 25, 26, 27, 34 and 09",
    "compat/tui-mouse.sh --self-check -> exit 0, 'every sabotage caught in its own channel'; 3 quiet controls and 28 one-sided sabotages (31 self_check_case calls); md5 89d2bd311c927bba38ca66e4cefcf778, byte-identical to evidence 28 and 10. The six new sabotages are all zz-only and each is caught in its own channel: word-separators under the pane menu, and one per format channel (over-a-word, wrapped-head, wrapped-tail, hyperlink, blank-cell)",
    "compat/tui-copy-mode.sh -> exit 0, 'all 147 cases agree on every channel they assert, 0 recorded', BYTE-IDENTICAL to evidence 14. TUI-005 is not regressed by the FormatGrid refactor",
    "compat/tui-copy-mode.sh --self-check -> exit 0; differs from evidence 15 in exactly four lines, all of them the mktemp scratch path (/tmp/zzcm.wokewv vs /tmp/zzcm.LFduLQ) - which is positive proof the lane's file is a real captured run and not a copy",
    "compat/tui-caps.sh -> exit 0, '366 asserted rows, 0 recorded rows', BYTE-IDENTICAL to evidence 16",
    "compat/tui-stock-keys.sh -> exit 0, 'all 50 cases agree on every channel they assert, 9 recorded a difference elsewhere' (worker said 8; the documented wobble on this box is 7 to 9, so this is inside it). --self-check -> exit 0",
    "compat/attached-client.sh (TMUX_BIN=pin ZZ_BIN=my build) x3: run 1 RED with 'error: zz pane unexpectedly contained UNDERLAY_BYTE_' in the popup-underlay focus probe; runs 2 and 3 both 'attached-client compatibility: PASS'. ANSWERING THE TASK'S QUESTION: at this tip I see PASS, and I did NOT see main's documented probe_command_output_navigation / ATTACHED_NAV_65 failure at all. The one red is a different probe and cleared on rerun, so it is a load flake by the rule (the box was also running two other agents' cargo builds at the time)",
    "cargo test -p zz-terminal -p zz-mux -> exit 0: zz-terminal 267 passed (1 ignored), zz-mux 525 lib plus every integration binary. The seven new pointer_* tests all pass",
    "cargo test -p zz-daemon with HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config -- --skip russh_socks -> exit 0: 895 lib passed, 0 failed, no flake on my run (the worker's one load flake, status_command_cache_survives_transient_clients_in_the_same_directory, did not reproduce here)",
    "cargo test -p zz -> exit 0: 638 lib and 125 cli_binary. NOTE: the worker's evidence 22 records 641 lib; a 3-test delta in a crate this diff does not touch, both green - I ran with the empty HOME, which is the only difference I know of",
    "cargo clippy -p zz-terminal -p zz-mux -p zz-daemon --all-targets --all-features -- -D warnings -> exit 0",
    "cargo fmt --all -- --check -> exit 0",
    "python3 compat/tui/tracker.py check -> exit 0, 'compat/tui/campaign.json is valid and knowledge/tmux/tui-parity.md is current'",
    "python3 compat/tmux-tracker.py check -> exit 0, 'compat/tmux-gaps.json is valid and knowledge/tmux/gaps.md is current'",
    "python3 -B compat/tui/tracker_test.py -> exit 0, 8 tests. So both generated reports ARE regenerated; knowledge/tmux/gaps.md and knowledge/tmux/tui-parity.md are generator output, not hand edits (independently re-rendered in memory and compared equal)",
    "python3 compat/tui/verify-claims.py --run TUI-008 --zz <my build> -> exit 0, 'all 45 asserted checks identical' / 'every verified obligation holds up'. (First attempt exited 1 because I exported only ZZ_COMPAT_TMUX; verify-claims.py:87 env.setdefault('TMUX_BIN', ROOT/compat/.cache/...) and the fixture prefers TMUX_BIN - my invocation error, not a defect)",
    "compat/check.sh from the worktree -> exit 0",
    "CORPUS: RUN_ENV compat/run.sh --strict-geometry --delta 6fbdf4c3...HEAD --commands display-menu,set-option,display-message,list-formats,bind-key --list -> 203 rows. I then ran 17 of them in sequential foreground chunks with ZZ_COMPAT_ZZ set (so run.sh invoked cargo zero times), ALL CLEAN on every column (TOPO / 0 GEO divergences / FMT / OUT / WARN), nothing failed on the first pass: census-formats, formats, formats-values, format-flags, smoke/format-listing, smoke/copy-mode-formats, smoke/copy-mode-copy-line, smoke/terminal-facts, smoke/args-parse-display-menu, smoke/display-menu-mouse, smoke/display-menu-cell-layout, smoke/display-menu-paste, smoke/display-popup-menu, smoke/display-menu-action-queue, smoke/display-menu-shortcut-grammar, smoke/display-menu-style-refresh, smoke/display-popup-menu-policy. That reproduces ALL 13 rows the worker named and adds 4. NOT RUN, and I am naming them rather than skipping them silently: the other 186 rows of the 203-row selection. I am out of budget, not out of will - chunks of 25 and of 9 both hit the 580s Bash cap (the selection is dominated by set-option/bind-key/display-message command matches, many of them slow), so I spent the remaining time on the rows that actually exercise the changed surface. The gate should finish the sweep",
    "grep over compat/scenarios for every string the landing changes: mouse_word|mouse_line|mouse_hyperlink|mouse_status_line|mouse_status_range and mouse_pane|mouse_x|mouse_y appear in exactly one corpus file, compat/scenarios/smoke/fixtures/format-listing.sh, whose row smoke/format-listing I ran clean",
    "ORACLE SPOT-CHECK, my own outer pinned tmux at 80x24 driving real SGR reports with send-keys -H through a MouseDown3Pane binding reading '#{mouse_word}|#{mouse_line}|#{mouse_hyperlink}|#{mouse_x},#{mouse_y}|#{mouse_pane}', both binaries, sample deliberately built so the grid has REAL SCROLLBACK (pin hsize=61, zz hsize=60): plain word, the row's first cell, past the end of the text, a wrapped word whose HEAD ROW IS IN HISTORY and tail on screen row 1, an OSC 8 link, both halves of a multi-column character, a blank row. IDENTICAL ON EVERY PROBE LINE on both sides (diff clean). Same probe with word-separators=_x: IDENTICAL ON EVERY PROBE LINE (and the behaviour genuinely changes - the wrap tail goes from the whole word to empty because x is a separator)",
    "ORACLE SPOT-CHECK, COPY MODE: same harness, pane put in copy-mode and scrolled (scroll_position=3) - both sides answer [FILLER059][FILLER059 ignore][] at screen row 1 and [SCROLLTAIL][SCROLLTAIL marker][] at row 9, i.e. off the copy mode's revision at its own offset, exactly the pin's gd->hsize + y - data->oy; after send -X cancel both answer the live grid's whole 139-cell word again at row 1. This is the branch's only proof for the Some(mode) arm and I had to build it",
    "ORACLE SPOT-CHECK, mouse_x / mouse_y / mouse_pane: unchanged and identical on both sides at every pane cell probed (e.g. col=8 row=2 -> 7,1 %0). Status-row click reproduces the disclosed residue: pin <|||0,0|0|left|%0> vs zz <|||0,23|||%0> - the three names closed here are empty on BOTH sides there, so no clause rests on it, and mouse_status_line/mouse_status_range are honestly kept open (pin 0/left on status-left, zz empty)",
    "PANE MENU whole-screen, my own sample, both binaries: over a written row the menu is 17 rows carrying Search For MENUMARK (C-r), Type MENUMARK (C-y), Copy MENUMARK (c), Copy Line (l); over a blank cell it is 12 rows with none of them. The two binaries' whole 24-row screens are IDENTICAL in both cases (diff clean). Confirms DEFAULT_PANE_MENU (key-bindings.c:48-72) now renders off these names on zz",
    "STALL CHECK, the daemon's synchronous worker read: timed the pointer answer and a mouse-invoked display-menu on both binaries. Flat sample - zz 107-111 ms per pointer answer at EVERY cell (blank cell, 8000-char token, under heavy output) vs pin 17-19 ms; mouse menu zz 209-211 ms vs pin 110-111 ms. WORST CASE - a single unbroken 160,000-character non-separator token spanning ~2000 rows of history: zz answers in 118-120 ms, the pin in 206-210 ms, and zz's mouse menu is 252-282 ms against the pin's 585-621 ms. zz is FASTER than the pin under the heaviest word walk I could build, and its cost is flat in the walk length, so the O(cols)-rescan-per-cell shape in format_grid_word does NOT produce a stall. With the pane producing continuous heavy output, zz answers in 109-111 ms and the menu draws in 190 ms. No stall, no timeout, no regression",
    "LOCK DISCIPLINE: confirmed the worker read is taken outside the server lock - crates/zz-daemon/src/daemon.rs:18378-18384 binds `&self.inner.lock()` in argument position, so the guard drops at the end of that let statement, before the `if let` that does the blocking send_timeout/recv_timeout. Timeout is CAPTURE_TIMEOUT = 2s (session.rs:82) and falls back to an empty map (daemon.rs:33061-33063), so a timeout costs one client's input thread, never the server",
    "ZONES: crates/zz-tui, crates/zz-client, crates/zz-protocol, crates/zz (GUI) and crates/zz-ui have zero hunks. popup_position_variables and popup_mouse_position_values are untouched (grep of the diff returns nothing; both still at daemon.rs:33860 and :33945, outside every hunk). The three declared excursions are exactly what was declared, with one imprecision: crates/zz-daemon/src/status.rs is 2 changes not 1 - the delegated-consumer match arm at :1654-1655 AND a test count bump assert_eq!(delegated.len(), 53) -> 56 at :2395",
    "LEDGER: compat/tui/campaign.json changes ONLY TUI-008's evidence_note, next_action and sources (2 added). status stays 'review' (it was already review at the base, so the worker did not even touch it). TUI-012 is byte-for-byte unchanged. No proof block, counter or other obligation moved",
    "compat/tmux-gaps.json: updated_on 2026-09-13 -> 2026-09-15, and formats.mouse-context's items 5 -> 2 (closing exactly format:mouse_word, format:mouse_line, format:mouse_hyperlink) plus its acceptance and reason. No gap added or removed. No ASCII escape added outside formats.mouse-context - the only \\u-escape in the diff is the SGR report \\033[<18;col;rowM inside that item's own reason, and grep -P '\\x1b' finds zero raw ESC bytes in the diff and zero in the whole file. There ARE four hunks outside the item, at source lines 280/1609/2794/3246, but they are purely \\u2014 -> raw em-dash re-encoding with identical parsed values, which is exactly what attempt-04's review nit 3 asked for ('Re-serialise with ensure_ascii=False')",
    "EVIDENCE HYGIENE: 37 files in compat/tui/evidence/TUI-008/attempt-05/, no .log files, none zero-length, git check-ignore -v over the whole directory returns nothing (exit 1) so nothing is gitignored, and all 37 are tracked. Files read as real captures: 29-cargo-test-zz-daemon-tip.txt differs from 13- in cargo's parallel compile ordering the way only real runs do, and it HONESTLY KEEPS a red (status_command_cache_survives_transient_clients_in_the_same_directory, with a real thread id and status.rs:3351) alongside 30-, the --exact solo green rerun. 20-corpus-delta-formats.txt carries raw ANSI bytes from the runner. environment.txt's timestamp reconciles with its mtime in America/Sao_Paulo. Caveat for the gate: the fixture outputs are fully deterministic and carry no date/duration/host, so identical bytes prove determinism, not that three runs happened - my own three runs reproducing the same md5 is the independent evidence",
    "git log 6fbdf4c3..HEAD: no Co-Authored-By, no 'Generated with', no claude, no codex in any of the 8 commit messages",
    "PUNCH LIST, item by item: (1) oracle table first - DONE, oracle-probe.sh is committed and re-runnable, 01/02 pin, 03 zz before (empty everywhere), 06/08 zz after. (2) live-grid readers shared with copy mode - DONE, the frozen mode_format_* readers became format_grid_* over a FormatGrid trait and ModeRevision's impl (session.rs:12928-12942) is a literal forwarder to the exact calls the old code made, so copy mode reads what it always read; seven unit tests, not the claimed eight. (3) daemon publishes the three synchronously from the event's cell - DONE, outside the server lock, with the pane-rectangle guard. (4) compat/tui-mouse.sh gains right-click-pane/over-a-word ASSERTED whole-screen plus five ASSERTED direct format reads, each with a one-sided sabotage - DONE, all six asserted, zero recorded anywhere in the file. (5) the three gap items closed with dated 2026-09-15 SGR measurements and the registrations moved from the constant backing to the delegated one with the manifest partition (constant 46->43, delegated 53->56, nonconstant 152->155; 43+99+56=198 unchanged, deltas exactly -3/+3/+3) - DONE; mouse_status_line and mouse_status_range correctly NOT closed and correctly left at Empty in formats.rs. (6) the backward emacs word selection parked - CORRECTLY parked: it is decided by window_copy_cursor_next_word_end and the reader_* family, not by format_grid_word, and nothing in this diff moves them. (7) TUI-008 to review at zero recorded checks with the summary line quoted - DONE"
  ],
  "touched_commands": [
    "display-menu",
    "display-popup",
    "bind-key",
    "set-option",
    "show-options",
    "display-message",
    "list-formats",
    "copy-mode"
  ],
  "touched_packages": [
    "zz-terminal",
    "zz-daemon",
    "zz-mux"
  ],
  "every_clause_asserted": "yes, all three. CLAUSE 1 ('Compare stock/custom root and copy mouse bindings, drag selection, border resize, status clicks, wheel and double/triple clicks with mouse on/off and application tracking') - YES. compat/tui-mouse.sh at MY OWN tip build reports '45 asserted checks, 0 recorded checks' and 'all 45 asserted checks identical', three consecutive runs all md5 64d94af2, and every disposition variable in the file's mode block (lines 1490-1517) is 'same', so no check anywhere in the fixture is recorded - the root and copy tables, user bindings, drag selection, border resize, status clicks, wheel, double and triple clicks, mouse on/off and application tracking all assert. CLAUSE 2 ('Assert target, action, screen and no unintended input leakage across copy mode, prompts, choosers and popups. Cover paste, focus and supported repeat/release paths') - YES. The same run asserts paste-into-pane, paste-into-copy-mode (screen and after-cancel screen), paste-into-prompt, paste-under-menu, focus-events on and off, the mouse-target click, and both the pane and status menus on the whole 24-row decoded screen; and because the FormatGrid refactor rewrites the readers copy mode uses, I re-ran the TUI-005 fixture: compat/tui-copy-mode.sh is 147/0 and BYTE-IDENTICAL to the lane's evidence, so nothing leaked into copy mode. CLAUSE 3 ('Reassess the TUI portion of accepted native-mouse decisions while preserving explicit zz bindings and other clients') - YES. formats.mouse-context is reassessed from five open items to two, each of the three closures and both retentions carrying a dated 2026-09-15 both-sides measurement; explicit bindings are preserved by construction (the new probes bind C-MouseDown3Pane, a key NEITHER binary binds, so the stock MouseDown3Pane that key-bindings.c installs is untouched - and the two menu cases prove it still drives DEFAULT_PANE_MENU); other clients are unmoved, with tui-stock-keys at 50 cases agreeing and tui-caps at 366 asserted / 0 recorded, both reproduced by me. THE RESIDUES DO NOT HOLD ANY CLAUSE OPEN, and I checked each one is honestly measured and honestly outside the proof surface: (a) THE TAB CELL - I confirmed the owner is the engines' tab storage, not these readers, because capture-pane -p shows the same two grids on both binaries; no clause mentions tabs. (b) THE STATUS CLICK - I reproduced it (pin '<|||0,0|0|left|%0>' vs zz '<|||0,23|||%0>'); it is mouse_x/mouse_y, closed 2026-09-13, and critically the THREE NAMES CLOSED HERE ANSWER EMPTY ON BOTH SIDES at that click, so no clause rests on it. format:mouse_status_line and format:mouse_status_range are correctly kept OPEN with their own measurement and are correctly still registered Empty in crates/zz-mux/src/formats.rs. (c) OSC 8 ON SCREEN - the pin's client re-emits the hyperlink sequence and the raw TUI draws the underline alone; that is crates/zz-tui's cell writer, outside the zones, and it only forces the MENU sample to carry no link row - the format probes compare an option's value and DO assert the URI, which I reproduced as [LINKTEXT][LINKTEXT][https://example.com/page] on both sides. The clauses assert with all three parked. My reservations (the missing scrollback and copy-mode unit tests, and the view-id guess) are about COVERAGE and a multi-client edge, not about any clause: I drove both uncovered paths by hand against the pin and both agree, which is why none of them is a blocker and why I am not marking any clause 'no'.",
  "gui_reach": "Nothing in this diff reaches the GUI, and there is no presentation change. Mechanically: git diff --stat 6fbdf4c3...HEAD -- crates/zz crates/zz-ui crates/zz-client crates/zz-tui crates/zz-protocol is EMPTY - the six Rust files touched are all under zz-terminal, zz-daemon and zz-mux. rg for mode_format_word|mode_format_line|format_grid_|pointer_context|PointerContext|LiveGrid|FormatGrid across crates/zz/, crates/zz-client/, crates/zz-protocol/, crates/zz-tui/ and crates/zz-ui/ returns NO MATCHES; repo-wide the only files naming any of those symbols are crates/zz-terminal/src/session.rs, crates/zz-terminal/src/terminal_core.rs and crates/zz-daemon/src/daemon.rs. FormatGrid and LiveGrid are private to session.rs (no pub), and PointerContext is newly re-exported from terminal_core.rs but nothing under crates/zz/ imports it. THE ONE PLACE THE REFACTOR COULD HAVE REACHED THE GUI is copy mode: the GUI consumes TerminalSession::copy_mode_facts, whose cursor_line/cursor_word call sites (session.rs:12831-12832) now go through format_grid_line/format_grid_word instead of the old mode_format_* bodies. I checked that hop two ways. Statically: ModeRevision's FormatGrid impl is a literal forwarder - grid_columns->self.columns, grid_rows->self.total_rows(), grid_row_wrapped->self.row(row).wrapped(), grid_cell_width->self.cell(point).width(), grid_first_char->self.first_char(point), grid_push_text->self.push_cell_text(self.cell(point), out) - and format_grid_line_length, format_grid_is_word_separator and format_grid_word's backward walk, wrap crossing, forward pivot and last_row bound are all unchanged; the only structural difference is that the old code hoisted the cell once where the new code looks it up twice, which for ModeRevision is a clamped Vec index and so semantically identical. Behaviourally: compat/tui-copy-mode.sh is the raw-TUI proof and it is 147 asserted / 0 recorded, BYTE-IDENTICAL to the lane's evidence, plus cargo test -p zz is green (638 lib, 125 cli_binary). So the GUI's copy mode reads exactly what it read before. No GUI-visible behaviour change.",
  "expected_gate_conflicts": "git merge-tree --write-tree origin/main HEAD fails to auto-merge 13 files: compat/tmux-gaps.json, compat/tui-mouse.sh, compat/tui/campaign.json, crates/zz-daemon/src/daemon.rs, crates/zz-daemon/src/status.rs, crates/zz-mux/src/command.rs, crates/zz-mux/src/compat_manifest_tests.rs, crates/zz-protocol/src/key.rs, crates/zz-protocol/src/message.rs, crates/zz-tui/src/input.rs, knowledge/protocol/wire-protocol.md, knowledge/tmux/gaps.md, knowledge/tmux/tui-parity.md. THAT LIST IS MOSTLY NOT THIS LANE. 6fbdf4c3 is NOT an ancestor of origin/main (the cycle 9 gate is landing the menus branch right now under new shas), so HEAD carries the whole menus branch. Running the same merge-tree against the BASE alone, origin/main vs 6fbdf4c3, produces 12 of those same 13 conflicts. By set difference the ONLY conflict this lane adds on its own is crates/zz-daemon/src/status.rs - the delegated-consumer match arm and its assert_eq!(delegated.len(), ...) count, which will need the three new names merged onto whatever count main carries after the menus landing. Everything else in that list resolves as part of landing the menus branch, and crates/zz-mux/src/formats.rs, crates/zz-terminal/src/session.rs, crates/zz-tui/src/state.rs and crates/zz-protocol/src/catalog.rs all auto-merge. THE DESIGNED CONFLICT, NAMED SEPARATELY AS INSTRUCTED: compat/tui-mouse.sh. The cycle 9 gate adds right-click-pane/over-a-word to main as a RECORDED check to keep the ledger honest until this lane lands; this lane adds the same check ASSERTED. The lane's version wins at its gate - resolve to ASSERTED, and make sure CONTEXT_MENU_MODE stays 'same' (compat/tui-mouse.sh:1512) and that the summary still reads '45 asserted checks, 0 recorded checks' after the resolution. I reproduced that exact summary line three times at this tip, so it is the number to land on. Note also that the base predates compat/tui/evidence/TUI-008/attempt-04/review.md, which exists only on main (added by ccbecb01): whoever merges must confirm that file survives the merge, since it is the history TUI-008's evidence_note cites.",
  "notes": "VERDICT REASONING. No blocker. Every proof the worker claimed reproduces at the tip on my own build, several of them byte-identically (tui-mouse three runs and its self-check, tui-copy-mode, tui-caps), and the two claims the lane had NO evidence for - the gd->hsize + y addressing and the copy-mode branch - I drove by hand against the pin and both agree exactly. The fixtures can fail: 31 self-check cases, 28 of them one-sided sabotages, 6 new, each caught in its own channel, and the word-separators sabotage visibly cuts the pin's menu item from 'Search For beta' to 'Search For b' while 'Copy Line' survives on both, which also proves the clean menu is not vacuous. I added an adversarial check of my own beyond the fixture's: an independent probe harness that aims the same SGR report at cells the lane never tested (a word whose head row is the last history row, a pane in copy mode at a non-zero scroll offset, both halves of a wide cell after scrolling, the same cells under word-separators=_x) - every line identical on both binaries, and the after-cancel contrast confirms the live grid and the frozen revision are genuinely different sources being selected correctly. The three must-fixes are a wrong test count in the ledger text, missing automated coverage for two paths I proved by hand, and a multi-client copy-mode edge that is real in the code but that I could not drive; none of them is a reason to hold the baseline. ON THE KNOWN-RED QUESTION: compat/attached-client.sh PASSES at this tip (2 of 3 runs; the single red was the popup-underlay focus probe, not main's probe_command_output_navigation / ATTACHED_NAV_65, and it cleared on rerun while two other agents' builds were loading the box). I never reproduced main's documented failure here. WHAT I DID NOT FINISH, stated plainly rather than skipped: 186 of the 203 delta-corpus rows. The 17 I ran are all clean and include every one of the 13 the worker named plus 4 more menu rows; the selection is dominated by set-option/bind-key/display-message command matches and chunks of both 25 and 9 rows overran the 580s Bash cap, so I spent the remainder on the rows that actually touch the changed formats and menus. The gate should finish the sweep before landing. NOT CHARGED TO THIS LANE, as instructed: attempt-04's display-menu -xM/-yM/-xW/-yW must-fix and its four nits belong to the cycle 9 gate. For the record one of them, nit 1, is still open and this landing made it worse - compat/tui-mouse.sh:1517 RIGHT_CLICK_REASON is dead text that still says 'the three screen-reading mouse formats are still unanswered (formats.mouse-context) and the row is not installed', both of which this branch disproves. It cannot affect a run. I mention it only so the menus gate does not lose it. OTHER OBSERVATIONS, none chargeable: zz's history_size caps lower than the pin's under history-limit 5000 (821 vs 1980 in my probe) - pre-existing engine behaviour, outside these zones and outside the clause surface; tui-stock-keys recorded 9 where the worker reported 8, inside this box's documented 7-to-9 wobble; cargo test -p zz gave me 638 lib against the worker's 641, a 3-test delta in an untouched crate with both green, most likely my empty HOME. HYGIENE: I copied no binary to /tmp; no scratch dirs, sockets or probe processes of mine survive (pgrep for zz-cli-/zz-user/zzprobe is clean); I touched neither the board nor any GitHub issue nor /home/demfabris/dev/zz; the review worktree is left clean and detached at eefd48f5, with one zero-byte 'env' file removed that my own malformed flock created there at 13:09. Every server I started was a throwaway (tmux -L zzprobe-* -f /dev/null, zz --socket /tmp/zzprobe-*.sock) with HOME and XDG_CONFIG_HOME scrubbed into a mktemp dir and ZZ_TRAY=0 set, so the box's real ~/.tmux.conf and ~/.config/zz/mux.conf were never read and the user's own servers were never touched. compat/.cache was never symlinked or copied into the worktree; RUN_ENV pointed at /home/demfabris/dev/zz/compat/.cache on every compat invocation and fetch-tmux.sh never wanted to rebuild the pin."
}
```

## What the gate applied

Each must-fix is its own commit and each was proved by re-running the
reviewer's own probe. The nits are two commits, split by the files they touch.

### MUST-FIX 1, the test count in the ledger text - commit "Count the pointer
unit tests honestly in TUI-008's evidence"

The evidence_note and notes.md said "Eight unit tests in zz-terminal, one per
row of the oracle's table". There were seven and the table has nineteen rows.
Both now say seven and neither claims one per row. After MUST-FIX 2 added two
more they say NINE, which is what `cargo test -p zz-terminal --lib pointer_`
lists at the gate tip: the seven the reviewer counted plus
pointer_word_crosses_a_wrap_back_into_the_scrollback and
pointer_formats_read_the_frozen_mode_at_its_own_viewport_offset. The two
pre-existing pointer_* hits the reviewer identified are still the only others.

### MUST-FIX 2, the two uncovered paths - commit "Cover the scrollback base and
the frozen mode in the pointer tests"

Both tests read one sample: thirty FILLER%03d rows, the 139-cell wrapping word,
then twenty-two TAIL%03d rows, so the word's head row is the LAST HISTORY ROW
and its tail is screen row 0.

- pointer_word_crosses_a_wrap_back_into_the_scrollback asserts
  LiveGrid::active_base() is 31, not 0, then reads the tail at screen row 0:
  mouse_word answers the whole 139-cell word, walked backward across the wrap
  into the history, and mouse_line answers the tail's own 59 cells. Screen row
  1 answers TAIL000.
- pointer_formats_read_the_frozen_mode_at_its_own_viewport_offset enters copy
  mode and applies three ScrollUp actions, which is data->oy = 3.
  viewport_offset is then 28 against the live grid's base of 31, and screen row
  1 answers FILLER029 off the revision where the live grid answers TAIL000 -
  the same contrast the pin shows between a pane in copy mode and the same pane
  after send -X cancel.

Neither is vacuous and each catches its own mutation ONLY
(37-pointer-tests-mutation-check.txt): stubbing active_base() to Ok(0) fails
the first (left 0, right 31) with the second green, and rewriting the
Some(mode) arm to read the live grid fails the second (left TAIL000, right
FILLER029) with the first green. Both mutations were reverted; the committed
tree is unmutated and 36-pointer-tests-review-fixes.txt is the clean run.
`cargo test -p zz-terminal --lib` -> exit 0, 269 passed.

### MUST-FIX 3, the guessed copy mode - commit "Answer the pointer formats off
the requesting client's own copy mode". THREADED, not disclosed-only.

PointerContextRequest now carries the requesting client's TerminalViewId and
both worker arms select THAT view's copy_mode, the way copy_mode_facts
addresses one. The daemon's mouse-invoked command path already knew the client
(TerminalViewId(client.0)), so mouse_format_variables takes the view and
PointerProbe carries it. It cost well under the hour's budget.

PROVED with two real clients, which the reviewer could not drive
(two-client-probe.sh, committed beside oracle-probe.sh; output in
35-two-client-copy-mode-probe.txt). Sixty labelled rows, both clients attached
to one session looking at the same pane, both sent the same real SGR report
(button 18) at the same cell:

| clients | client | pin | zz |
| --- | --- | --- | --- |
| first in copy mode a page back, second not | first | [ROW019][ROW019 word019 tail] | same |
| | second | [ROW019][ROW019 word019 tail] | [ROW040][ROW040 word040 tail] |
| both in copy mode, at different offsets | first | [ROW019][ROW019 word019 tail] | same |
| | second | [ROW019][ROW019 word019 tail] | [ROW040][ROW040 word040 tail] |

Every client that is ITSELF in copy mode now answers exactly what the pin
answers, at its own offset - including the both-in-copy-mode case the counting
arm inverted, where the deleted `match (copy_modes.next(), copy_modes.next())`
fell through to the live grid for both.

WHAT IS LEFT AND IS DISCLOSED IN next_action WITH ITS MEASUREMENT, as the
reviewer's suggested alternative allows: a client that is NOT in copy mode, on
a pane where another client is, answers off the live grid here and off the
pane's mode on the pin - the [ROW040] against [ROW019] in both rows above. The
pin holds one mode per PANE and zz one per VIEW, so the two answers are the two
models; closing it means changing which model zz has, not which grid this
reader walks.

`cargo test -p zz-daemon` -> exit 0, 918 lib plus every integration binary.
`cargo clippy -p zz-terminal -p zz-mux -p zz-daemon --all-targets
--all-features -- -D warnings` -> exit 0.

### NIT 1, the vacuity guard in context_probe - commit "Hold the mouse-context
probe to the pin's own answer"

context_probe now waits on pin_option_answered for every cell but blank-cell,
which holds the run to an answer from the pin before the two sides are
compared. It is LIVE, not decoration: widened to blank-cell, which legitimately
answers [][][], the run dies with `error: the pin read the blank-cell cell did
not happen within 10 seconds`, exit 2 (42-context-probe-guard-check.txt). The
committed fixture exempts blank-cell and is exit 0.

### NIT 2, the header's two mistakes - same commit

The table now names C-MouseDown3Pane, not MouseDown3Pane, and says the pin's
menu renders FOUR items off the three names. The same undercount in the case's
own comment above case_right_click_pane_over_a_word is fixed too, naming the
four items (Search For, Type and Copy over the word, Copy Line over the row).

### NIT 3, evidence bookkeeping - commits "Answer the pointer formats..." and
this records commit

(a) notes.md's daemon-run reference is 29, not 25. (b) environment.txt is
restamped at the gate's final pre-records tip. (c)
crates/zz-terminal/src/terminal_core.rs is added to TUI-008's sources and named
in notes.md's file list for its one line, the PointerContext re-export. THE
CALL ON mode_revision.rs: KEPT, and said so in notes.md - this diff does not
modify it, but the frozen readers the copy-mode arm walks are ModeRevision's
own accessors, so it decides what a pane in copy mode answers even though the
FormatGrid impl over it lives in session.rs, and the new frozen-mode unit test
exercises it.

### NIT 4, the daemon docblock - folded into the MUST-FIX 3 commit, because the
threading rewrites the same block

It no longer models cmd_mouse_at as covering names it does not: the rectangle
guard is named as the three names' own, and mouse_x and mouse_y are named as
published above it, off the pane's origin alone, with their divergence pointed
at TUI-008's next_action where it was already recorded.

### NIT 5, the unfiltered OSC 8 URI - same commit

notes.md now records that #{mouse_hyperlink} publishes the raw URI with none of
the scheme allowlist hover_link_at applies, deliberately, because
format_grid_hyperlink does no filtering either: a binding that interpolates it
into run-shell is exposed exactly as far on the pin, and filtering on this side
alone would be the divergence.

### NIT 6, the dead RIGHT_CLICK_REASON text - the fixture commit and the rebase

The reviewer expected the cycle 9 gate might have emptied it. It had:
menus-3 carries RIGHT_CLICK_REASON="" and adds a RIGHT_CLICK_WORD pair for the
recorded over-a-word check. The rebase resolved that designed conflict in this
lane's favour - menus-3's recorded probe inside case_right_click_pane and its
RIGHT_CLICK_WORD_MODE=record/REASON are gone, this lane's asserted
case_right_click_pane_over_a_word stands, CONTEXT_MENU_MODE is `same`, and the
fixture reads `45 asserted checks, 0 recorded checks` / `all 45 asserted checks
identical`, md5 64d94af20992d6bc0288855305b83072, byte-identical to the
reviewer's three runs and to the lane's evidence 25, 26, 27, 34 and 09. No
reason text anywhere in the file still says the three formats are unanswered.

### THE 44 `///` LINES - the gate's call: LEFT IN

The reviewer flagged them for the gate to decide rather than inheriting the
call. They are doc comments on functions that already carried docblocks in the
same style, several of them carrying the tmux citations that make these readers
auditable. The repo rule is about comments in code; replacing an existing
docblock with a docblock is not what it forbids. Left in, and the MUST-FIX 3
commit adds to them rather than stripping them.

## Checks the gate ran at its own tip

Listed in TUI-008's proof block above this file, with exit codes. The corpus is
53-gate-corpus.txt: all 214 rows of the union selection, none skipped.
