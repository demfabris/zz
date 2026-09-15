# Gate review record, cycle 9 mouse lane

The independent re-reviewer's verdict on `campaign/tui-mouse` at tip `bad0261f`,
verbatim, followed by what this gate ran and what it did about each finding.
The first review of this branch, at `142139fd`, was a REJECT with three
blockers; a fresh agent fixed them in place and the re-review below rechecked
every one of them independently.

Gate base: origin/main `3ecd4702` (the cycle 9 choosers gate). Pin: tmux
next-3.8 `d77c9dc6aa021e4bc61f0da128c591af695e6466`. Box: Ubuntu 26.04.1,
8 cores, 30 GB.

## Reviewer verdict, verbatim

```json
{"lane":"mouse","branch":"campaign/tui-mouse","tip":"bad0261f","verdict":"approve-with-fixes","blockers_recheck":"BLOCKER 1 (the wire) FIXED. crates/zz-protocol/src/message.rs now reads pub const PROTOCOL_VERSION: u16 = 103; and the two appends view_action/press_action sit after border at the tail of the InputMessage::MouseKey struct variant (pure end-appends, verified on the three-dot diff). A v103 entry exists in knowledge/protocol/wire-protocol.md's version history with the two lines moved OUT of the v102 entry, and the header/envelope table/constant lines all say 103. THREE assertions moved: message.rs detached_reason_holds_its_appended_wire_field (102->103), crates/zz-protocol/tests/hunt_claims.rs renamed protocol_version_on_this_commit_is_one_hundred_and_three with its value, and the byte-for-byte hello frame in the same file where the version appears TWICE (0x66,0x66 -> 0x67,0x67). origin/main's compat/wire-version.py run with the worktree as root: exit 0, 'wire-version: 103 is unreleased (v0.9.1 shipped 102); appends are free'. Consumer halves in the same push: crates/zz-daemon/src/daemon.rs forward_mouse_key_to_pane reads view_action, and crates/zz-mux/src/command.rs reads view_action in send-keys -M and mouse_cursor_move and both fields in copy-mode -M. BLOCKER 2 (drag lost its release) FIXED: own probe (outer pinned tmux as injector/decoder, send-keys -H raw SGR, capture-pane -p, both sides scrubbed, pane running stty -echo -icanon min 1 time 0; printf MARK; exec cat -v armed with \\033[?1002h\\033[?1006h); press \\e[<0;2;3M, motion \\e[<32;8;3M, release \\e[<0;8;3m: THREE OF THREE RUNS pin ^[[<0;2;3M^[[<32;8;3M^[[<0;8;3m and tip identical. BLOCKER 3 (double click out of order) FIXED: THREE OF THREE RUNS pin ^[[<0;5;3M^[[<0;5;3m^[[<0;5;3M^[[<0;5;3m and tip identical. The pin's source backs the fix's model: server-client.c:1399-1406 sets first = table, :1536-1542 retries root, :1552 if (first != table ...) goto out; the daemon's root_was_first is that predicate, and its read-only gate matches the pin's CLIENT_READONLY check before window_pane_key. MUST-FIX 4 (honesty) FIXED: every checkable sentence of the rewritten evidence_note holds against the fixture ('35 asserted checks, 3 recorded checks', md5 76e3cc87 on three runs, nineteen sabotages and three controls, no triple click or wheel under tracking driven, tui-copy-mode 147/0, tui-caps 366/0, attached-client PASS, 22 gap items closed, wire-version exit 0).","confirmed_defects":[{"obligation":"TUI-008","severity":"must-fix","description":"The record states a paste-under-menu divergence as present fact that does not reproduce at this tip. PASTE_MENU_REASON (compat/tui-mouse.sh:1349) and the evidence_note's WHAT REMAINS say the pin's overlay eats the paste-start key and the characters behind it, the pane sees the tail as ordinary keys and the unmatched paste-end leaves a trailing ~ on its row, where the raw TUI hands the tail over as a fresh bracketed paste. Measured: paste-under-menu/screen: all 24 rows identical on SEVEN of seven runs (three reviewer runs at md5 76e3cc87 and the fix agent's four evidence files). paste-under-menu/option is both alpha too. The case genuinely raises the menu on both sides (case_paste_under_menu binds prefix-E to display-menu and both_screen_has PASTEMENU waits for it on each side before pasting). Recording a check that agrees is conservative rather than a false proof claim, so not a blocker, but TUI-008's next_action hands the menus lane a fix for a screen difference that currently measures identical.","suggested_fix":"Either flip PASTE_MENU_MODE to same with a --self-check sabotage in its own channel, or restate PASTE_MENU_REASON and the evidence_note to say the two screens agree on this box at this tip and name what the record is actually still holding open. Correct next_action item (2) to match."},{"obligation":"TUI-008","severity":"nit","description":"The evidence_note says tui-stock-keys 50 cases with 8 recorded elsewhere; measured NINE: the application-reader record appears at BOTH 80x24 and 100x24. Exit 0 both ways and no asserted channel moved: a load-dependent count of a channel the note documents as timing-sensitive. Verified no hunk touches pane_current_command.","suggested_fix":"Say eight or nine depending on load, or drop the count and name the channel."}],"checks_run":["git status clean at bad0261f","fetch: origin/main 45481a72, merge-base 879b68fc","cargo build -p zz --bin zz -> exit 0, 5m10s (first attempt died with ENOSPC, not a result)","origin/main's wire-version.py with the worktree as root -> exit 0: 103 is unreleased","own blocker probe x3 (drag under tracking) -> AGREE 3/3","own blocker probe x3 (double click under tracking) -> AGREE 3/3","compat/tui-mouse.sh x3 -> exit 0: 35 asserted checks, 3 recorded checks; md5 76e3cc87 each","compat/tui-mouse.sh --self-check -> exit 0; 19 sabotages caught, 3 controls","the two new sabotages read in full: sc_one_sided_drag_release and sc_one_sided_second_click fail in the only asserted channel of their case","own sabotage (bind-key -T root MouseDown1Pane set-option -g @claimed down on zz only) -> CAUGHT: tip prints 2 reports to the pin's 3","compat/tui-stock-keys.sh -> exit 0: all 50 cases agree, 9 recorded elsewhere","compat/tui-stock-keys.sh --self-check -> exit 0","compat/tui-copy-mode.sh -> exit 0: 147 agree, 0 recorded","compat/tui-caps.sh -> exit 0: 366 asserted rows, 0 recorded","compat/attached-client.sh -> exit 0 PASS","cargo test -p zz-protocol -> exit 0","cargo test -p zz-mux -> exit 0","cargo test -p zz-tui -> exit 0, 207 passed","cargo test -p zz-daemon (empty HOME) -> exit 0, 895 passed, 1 ignored","cargo test -p zz (empty HOME) -> exit 0, 11 ok lines","cargo clippy on zz-protocol zz-mux zz-tui zz-daemon -D warnings -> exit 0","cargo fmt --all -- --check -> exit 0","python3 compat/tui/tracker.py check -> exit 0, report current","verify-claims.py --run TUI-008 -> exit 0: all 35 asserted checks identical","compat/check.sh -> exit 0","corpus delta chunks 1-4 (all 14 non-smoke rows incl. copy-mode-stock-action-keys alone) -> exit 0, all clean (chunk 3 re-run after the pin relink race at 19:42)","corpus delta smoke rows 1-60 of 144 -> exit 0, all clean","ORACLE: #{mouse_any_flag} both 0 / both 1 / both 1 across no tracking, ?1000h, ?1002h+1006h","ORACLE: wheel-up under tracking both ^[[<64;4;3M, pane_in_mode both 0","ORACLE: triple click under tracking, six reports identical on both","ORACLE: double click no tracking buffer both beta; triple both alpha beta gamma; outside 300 ms both beta","ORACLE: drag-select in copy mode buffer both alpha be","ORACLE first != table guard: MouseUp1Pane unbound; out of copy mode both panes print ^[[<0;6;3m; in copy mode row unchanged on BOTH","ORACLE: list-keys mouse names per table: zz copy tables carry the pin's 7 names each; root carries 11 of 20 incl. all 6 closed root items","GAP SPOT-CHECK against the merge-base: exactly 22 items closed; three re-measured with a real SGR gesture","ZONES: crates/zz/src untouched; no non-doc comment; no trailer; tmux-gaps.json untouched by the fix pass"],"every_clause_asserted":"Clause 1 NO: everything but the application-tracking half asserts; under tracking only a drag and a double click are driven; a triple click and a wheel under tracking are driven by no case (both agree in the reviewer's own probe). Clause 2 NO: three checks record (right-click-pane/screen and status-clicks/right-click-screen genuinely divergent; paste-under-menu/screen recorded but measures identical). Clause 3 YES: 22 items closed with dated measurements inside the owned gaps plus one declared excursion; the GUI provably untouched; the daemon's forward gated on the pin's own read-only and first-table conditions. TUI-008 correctly stays active with proof null.","notes":"DISK: the box filled at 18:35 before the review ran anything; the reviewer freed 16 GB inside the lane's own worktree only (target/release, ui-showcase, window-corner-test). THE PIN WAS RELINKED at 19:42 by a sibling (same commit, stamp matches again); chunk 3 re-run clean. CORPUS: 74 of 158 delta rows run (all non-smoke plus 60 smoke); 84 smoke rows skipped for time; the lane's argument that no corpus row can reach these hunks holds on the code. ZONE EXCURSIONS all declared: status.rs (one match arm), zz-mux tests/hunt_claims.rs and compat_manifest_tests.rs, zz-protocol tests/hunt_claims.rs, knowledge/index.md and knowledge/protocol/index.md one-word titles. PRODUCT DECISION LEAVING THE ZONES: the select-word change in crates/zz-terminal/src/session.rs reaches every client including the GUI; declared and reversible; its sibling backward emacs word selection also changed and no fixture drives it. FOR THE MENUS LANE: paste-under-menu/screen measures identical 7/7; item (1) is real (MouseDown3Pane and MouseDown3Status absent from zz's root table, -x M/-y M answer the screen centre); the client now offers copy-table mouse names while the pane holds a mode, and the daemon forwards to the pane only when root was the first table tried. SUSPICION: reports_held's 12x50 ms settle makes app-mouse-double-click deterministic; under heavier load it would flake as a DIFF, so re-run solo before charging. HYGIENE clean; the menus lane's zzprobe processes were left alone."}
```

## review_actions

Both findings applied, each in its own commit, with the probe the reviewer named
re-run as proof.

1. **MUST-FIX, the paste-under-menu claim.** `Say that the paste under a menu
   measures identical on both binaries`. The record no longer states a
   divergence that does not reproduce. `PASTE_MENU_REASON` in
   compat/tui-mouse.sh and TUI-008's WHAT REMAINS now say plainly that the two
   screens agree on this box at this tip - all 24 rows identical, seven of seven
   runs, `paste-under-menu/option` alpha on both, with the menu genuinely raised
   on both sides first - that the earlier sentence about the pin's overlay
   eating the paste-start key and leaving a trailing `~` is withdrawn, and that
   the channel is held at `record` only until a lane drives it one-sided with a
   `--self-check` sabotage and flips it to `same`. `next_action` item (2) no
   longer hands the menus lane a fix for a screen difference: it asks for the
   driver, names the sabotage as fixture work that lane already carries, and
   keeps the code pointers behind an `only if` clause. The case mode itself is
   NOT flipped here: the menus branch flips it with its own sabotage and its
   gate takes that branch's compat/tui-mouse.sh and TUI-008 record wholesale
   where the two conflict. PROOF, the fixture at this gate: three runs, exit 0
   each, `ok    paste-under-menu/screen: all 24 rows identical` on every one,
   whole output md5 `76e3cc87` all three - the reviewer's own md5. The reason
   string cannot reach that output at all: `record_screen` (compat/tui-mouse.sh)
   prints the reason only on the branch where the row count differs, which is
   why the md5 is unchanged by this commit and why the claim was never under
   test.

2. **NIT, the stock-keys record count.** `Let the stock-keys record count wobble
   with load in TUI-008's note`. The note said 8 recorded elsewhere; it now says
   eight or nine depending on load and names the channel that wobbles - the
   application-reader `pane_current_command` timing record, raised at 80x24 and,
   under load, again at 100x24. PROOF: `compat/tui-stock-keys.sh` at this gate,
   exit 0, `all 50 cases agree on every channel they assert, 9 recorded a
   difference elsewhere`; `--self-check` exit 0 beside it.

Neither commit touches code. TUI-008 stays `active` with a null proof block:
three checks still record, so `verified` would be a lie, and the menus lane
gated next closes them.

## Gate checks

Everything below ran in the lane's own worktree on the ubuntu box, one at a
time, cargo through the shared two-slot lock with `--jobs 4` and
`--test-threads=4`.

- `git rebase origin/main` (45481a72) -> only conflict knowledge/tmux/gaps.md,
  generated, regenerated with `compat/tmux-tracker.py write-report`.
- `git rebase origin/main` (3ecd4702, the choosers gate) -> no conflict. The two
  v103 version-history bullets it left were folded into one entry by hand.
- `cargo test -p zz-protocol -p zz-mux` -> exit 0.
- `cargo test -p zz-tui` -> exit 0, 207 passed.
- `cargo test -p zz-daemon` (empty HOME) -> 910 passed, 3 failed, all three
  `russh_socks::tests` with `ConnectionReset`. Red at origin/main too, with
  main's own binary and main's own worktree: not this lane's.
- `cargo test -p zz` (empty HOME) -> exit 0, 125 cli_binary tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` -> exit 0.
- `cargo fmt --all -- --check` -> exit 0.
- corpus `--delta origin/main..HEAD --commands send-keys`: 158 rows selected;
  all 14 non-smoke rows and smoke rows 61-144 run here (the review ran the first
  60), every channel clean. `smoke/format-modifier-client-loop` failed once and
  passed on run.sh's own solo retry.
- every `compat/tui-*.sh` in the tree, plus each `--self-check`, plus
  `compat/status-row.sh` and `compat/attached-client.sh`. Exit codes in the
  gate report; `compat/tui-mouse.sh` three times at md5 `76e3cc87`.
- `compat/tui/verify-claims.py --run TUI-008` -> exit 0, `all 35 asserted checks
  identical`, `every verified obligation holds up`.
- `compat/tui/tracker.py check`, `compat/tmux-tracker.py check`,
  `python3 -B compat/tui/tracker_test.py`, `compat/check.sh` (wire-version at
  103) -> exit 0.

Two fixtures are red at this tip and both are red at origin/main on this box,
proven with a throwaway worktree at origin/main and a zz built from it:
`compat/attached-client.sh` (`ATTACHED_NAV_65 ATTACHED_NAV_MATCH` never
arrives after `n` in copy-mode-vi) and `compat/tui-overlays.sh --self-check`
(its `equivalence` control: `C-l` reaches the pane instead of dismissing the
message, on BOTH binaries, and the literal `^L` then eats the next mark). Not
this lane's, and named in the gate report for whoever owns them.
