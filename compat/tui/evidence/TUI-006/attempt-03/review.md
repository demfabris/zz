# Gate review record, cycle 9 choosers lane

The independent reviewer's verdict on `campaign/tui-choosers-3` at tip `5f06595b`,
verbatim, followed by what this gate ran and what it did about each finding.

Gate revision: `a82bd0f51359809a818d64e87a2a162f3892c4ce` (campaign/tui-choosers-3 rebased onto origin/main 45481a72,
then six gate commits). Pin: tmux next-3.8 d77c9dc6aa021e4bc61f0da128c591af695e6466.

## Reviewer verdict, verbatim

```json
{"lane":"choosers","branch":"campaign/tui-choosers-3","tip":"5f06595b","verdict":"approve-with-fixes","confirmed_defects":[{"obligation":"TUI-006","severity":"must-fix","description":"The known 102 bump, proven end-to-end. PROTOCOL_VERSION is still 102 at crates/zz-protocol/src/message.rs:21, and v0.9.1 and v0.9.0 both shipped 102. origin/main's compat/check.sh:13 now runs compat/wire-version.py, which does not exist at this tip; applied to the tip tree that guard fails: git diff v0.9.1 -- crates/zz-protocol/src/message.rs has 34 meaningful changed lines against a released version, so post-merge compat/check.sh exits 1. The appends themselves are clean: CommandPromptState.pane is appended after no_freeze (last field on main), ProtocolMessage::ClientTerminalType is variant 43 appended after ClientTerminalFeatures (42 on main), no reorder or renumber, and both have consumer halves in the same push (producer crates/zz-tui/src/tty.rs:130 -> crates/zz-daemon/src/client.rs:1299; consumer crates/zz-daemon/src/daemon.rs -> set_client_terminal_type -> client_format_facts -> #{client_termtype}). The branch even proves its own prefix stability in message.rs.","suggested_fix":"Set PROTOCOL_VERSION to 103 in crates/zz-protocol/src/message.rs:21, move the assertion in crates/zz-protocol/tests/hunt_claims.rs:18 (test protocol_version_on_this_commit_is_one_hundred_and_two) and the in-crate assertion, and split the two appends out of the v102 paragraph in knowledge/protocol/wire-protocol.md:653 into a new v103 entry. Re-run compat/check.sh with main's wire-version.py present."},{"obligation":"TUI-014","severity":"must-fix","description":"client_info_mask, the mask that carries TUI-014 clause 1's only evidence, is blind to a column-alignment divergence, and no sabotage exercises it. compat/tui-choosers.sh:562-586 collapses every run of two-or-more spaces on any line the mask touched (masked = re.sub(r\"  +\", \" \", masked)), not just the box fill after a substituted value as its header claims. Proved by running the mask function verbatim: pin row 'Client Name      | /dev/pts/9 (PID 12345)' and a zz row with the label column shifted four columns left, 'Client Name  | /dev/pts/9 (PID 999)', both mask to 'Client Name | /dev/CLIENT (PID NNNN)' - identical. The mask still catches label text ('Terminal Typo') and neighbouring values ('size 81x24'), so the fixture is not un-failable, but every row carrying the client name, a PID, a timestamp, 'Bytes Written', '(N discarded)' or an HH:MM loses its column geometry. Worse, neither new sabotage runs under this mask: compat/tui-choosers.sh:1230 sets ROW_MASK=client_row_mask before both the pin-side and the new zz-side info-view sabotages, so 'client info, the zz client tree on its info view alone' proves nothing about client_info_mask.","suggested_fix":"Scope the space collapse to the substituted token's trailing fill (anchor it to the token, as the header describes) instead of the whole line, and set ROW_MASK=client_info_mask around the zz-side info-view sabotage at compat/tui-choosers.sh:1236-1241 so the sabotage runs through the mask the client-info-view case actually asserts under."},{"obligation":"TUI-006","severity":"must-fix","description":"The client_row_mask widening added by commit 65d98e0a erases arbitrary row content, unconditionally and with no sabotage. compat/tui-choosers.sh:552 adds line = re.sub(r\"(/dev/CLIENT).*?(?=\\u2502)\", r\"\\1 (CUT) \", line, count=1) to every line matching the client name. The header justifies it for one case (a help box cutting the tree's title row at 100x40) but the regex is not scoped to that row or that geometry. Proved by running the function verbatim: '(0) /dev/pts/9: 3 windows (attached) | preview' and '(0) /dev/pts/9: 9 windows (detached) | preview' both mask to '(0) /dev/CLIENT (CUT) | preview' - identical. Any client row where a box edge follows the name loses everything between the two.","suggested_fix":"Gate the CUT on the geometry and the row it was measured for (the tall 100x40 help-box title row), or replace it with a fixed-width truncation of the title field only, and add a sabotage that plants a divergence inside the cut span."},{"obligation":"TUI-006","severity":"must-fix","description":"The code reverses a closed gap decision and leaves five places asserting the opposite. compat/tmux-gaps.json closed[161] prompt.pane-rendered, 'Keep the prompt on the client surface, -P included', closed_on 2026-09-02, says the new stance is that 'the prompt contract is the client surface, and the pane-cell placement is presentation zz does not copy' and that -P is 'accepted and ignored by the daemon'. The raw TUI now paints exactly that pane-cell placement. That gap's text is what closed fourteen items under copy-mode.command-fidelity, which is a gap of the VERIFIED TUI-005. tmux-gaps.json was not touched at all by this branch (empty three-dot diff). Four more stale claims survive: crates/zz-protocol/src/key.rs:555-558 (the doc on bind_copy_mode_search_defaults, the function that installs these very bindings), crates/zz-protocol/src/catalog.rs:2124 ('presentation hint: the prompt is the pane's'), knowledge/tmux/copy-mode.md:70 and knowledge/tmux/key-tables.md:290. The worker did correct the three compat/scenarios/ headers carrying the same prose, so the omission is partial, not systematic. Note for the orchestrator: prompt.pane-rendered is NOT one of the four tmux-gaps items the lane's zone allowed, so the worker could not have fixed it in zone.","suggested_fix":"Reopen or re-scope prompt.pane-rendered with a dated 2026-09-14 measurement saying the raw TUI now draws the pane-cell prompt while zz-client-based clients still raise the client prompt, regenerate knowledge/tmux/gaps.md, and correct key.rs:555, catalog.rs:2124, knowledge/tmux/copy-mode.md:70 and knowledge/tmux/key-tables.md:290."},{"obligation":"TUI-006","severity":"must-fix","description":"The committed chooser evidence predates the fixture it certifies. Both ledger records claim 'three runs at this tip'. compat/tui/evidence/TUI-006/attempt-03/choosers-run-1/2/3.txt (and the identical copies under TUI-014/attempt-02) were captured at 0ecfd454 (08:10), but 65d98e0a (09:34) edits compat/tui-choosers.sh itself, adding the client_row_mask CUT above - a reduction of the compared surface - before the runs were committed at 197b7d8e (10:01). Only choosers-run-4.txt is post-mask, and all eight choosers-run files share one md5 (46fa91d88dd72674cee37c4ba25a36bc), so run-4 adds no independent signal; the format carries no timestamp, PID, duration or exit code, so nothing in those artifacts distinguishes one run from another or names the revision. The CLAIM is true - I re-ran the fixture three times plus --self-check at the tip on this box and got the identical summary line every time - but the committed evidence does not establish it.","suggested_fix":"Re-capture three runs plus --self-check at the merged tip (which TUI-006's own next_action already asks the gate to do) and stamp each run file with the revision, the wall clock and the exit code, the way client-commands-run-*.txt already is."},{"obligation":"TUI-006","severity":"nit","description":"Run selection in the evidence directory is 4-of-6 green. TUI-006's own evidence_note discloses 'filter-cleared failed in two of six runs at this tip' on #{pane_current_command} answering bash where the pin answers sh, and commit 78337711 writes an 18-line measurement of the cause into the fixture header. Neither red run is in the evidence directory, while TUI-014 did keep its own red (client-commands-run-2-server-exit.txt). I saw zero reds in three runs here. Separately: the file committed as client-commands-run-2.txt is chronologically the LAST of four attempts (10:38), dropped into the slot whose real second run died at 10:33; the notes disclose this, the ledger record does not.","suggested_fix":"Commit the red runs beside the green ones, or say in the evidence_note how many runs were taken and how many were kept."},{"obligation":"TUI-006","severity":"nit","description":"Two edits outside the drawn zones. crates/zz-daemon/src/client.rs gains a new public API (+15, pub fn report_terminal_type) and crates/zz-tui/src/tty.rs gains the call that fires it (+1, inside note_extended_device_attributes). Neither is in a forbidden path, both are small and mirror the report_learned_terminal_features pair directly above them, so nothing collides with the mouse or introspection lanes. Re-exports in zz-daemon/src/lib.rs and zz-protocol/src/lib.rs follow from them. Everything else outside the zones is a pane: None test fill.","suggested_fix":"Nothing to change in the code; record the widened zone in the cycle log so the next lane inherits it."},{"obligation":"TUI-006","severity":"nit","description":"A doc comment was stolen. client_terminal_features_option was inserted in crates/zz-daemon/src/daemon.rs between client_feature_mask and that function's existing doc block, so client_feature_mask is now undocumented and the c->term_features / tty_update_features paragraph describes the wrong function. 40 added comment lines across 7 crates files, every one a /// doc comment (no non-doc // addition); doc comments are the overwhelming existing convention here (741 in daemon.rs at main), so not charged.","suggested_fix":"Move the c->term_features doc block back above client_feature_mask and give client_terminal_features_option its own."},{"obligation":"TUI-014","severity":"nit","description":"A record outside the worker's two was rewritten. TUI-011's evidence_note changed bytes in compat/tui/campaign.json: main holds a literal U+250C and HEAD holds the escape \\u250c. It JSON-decodes to the identical string; the signature of re-serializing the ledger with ensure_ascii=True. No proof block changed, no verified was set.","suggested_fix":"Serialize the ledger with ensure_ascii=False so an unrelated record's bytes stop moving."},{"obligation":"TUI-014","severity":"nit","description":"Punch-list item 3 was not attempted: zero of the five commands closed an item in commands.native-client-tools, and compat/tmux-gaps.json has an empty three-dot diff. Honestly recorded: TUI-014's evidence_note says 'Nothing in commands.native-client-tools closed here', clause 2 and clause 3 are named as open, and the status stays active. Verified on both binaries: clock-mode, customize-mode, switch-mode, suspend-client and server-access all answer 'unsupported command: <name>' at exit 1 on zz. Structural note: command:switch-mode lives in clients.interactive-refresh and server-access in a third group, not in commands.native-client-tools.","suggested_fix":"Carry clause 2 and clause 3 into the next cycle, and fix the punch list's group attribution for switch-mode and server-access before dispatching it."}],"checks_run":["git status clean, detached at 5f06595bcb9006d0b7b9b7ad5dff8e6002a23a88","git merge-base origin/main HEAD -> 879b68fc","git merge-tree --write-tree origin/main HEAD -> clean","cargo build -p zz -> exit 0 in 6m22s","RUN_ENV compat/tui-choosers.sh x3 -> exit 0 each: 'all 78 asserted comparisons identical, 0 recorded not asserted (0 for a sibling lane)'","RUN_ENV compat/tui-choosers.sh --self-check -> exit 0, 12 ok lines","RUN_ENV compat/tui-copy-mode.sh -> exit 0: 'all 147 cases agree on every channel they assert, 0 recorded a difference elsewhere'","RUN_ENV compat/tui-copy-mode.sh --self-check -> exit 0","RUN_ENV compat/tui-client-commands.sh -> exit 0: 'all 61 asserted comparisons identical, 37 recorded not asserted (0 for a sibling lane)'","RUN_ENV compat/tui-client-commands.sh --self-check -> exit 0","cargo clippy on the six touched crates -D warnings -> exit 0","cargo test -p zz-protocol -p zz-mux -p zz-terminal -p zz-client -p zz-tui -> exit 0","HOME=/tmp/zz-emptyhome cargo test -p zz-daemon -> exit 0, 930 passed","cargo test -p zz -> exit 0, 783 passed","RUN_ENV compat/run.sh --strict-geometry copy-mode-bindings copy-mode-goto-line list-keys-padding smoke/copy-mode-prompt-bindings renderer-styles copy-mode-previous-bracket copy-mode-recentre -> exit 0, every row clean","python3 compat/tui/tracker.py check -> exit 0","python3 compat/tmux-tracker.py check -> exit 0","compat/check.sh at HEAD -> exit 0","origin/main's wire-version.py logic applied to the tip -> FAILS (102 released, 34 changed lines)","ORACLE PROBE: real pane copy mode at 80x24 with status ON: C-s and C-r on both sides put the prompt on row 23 and keep the status row; styled captures byte-identical","ORACLE PROBE formats: search_present/search_count/selection_present agree after C-s 77 Enter and after C-Space Right Right; rows 1-23 byte-identical","CLAUSE 2 PROBE: clock-mode, customize-mode, switch-mode, suspend-client, server-access all 'unsupported command' exit 1 on zz","MASK BLINDNESS PROBE: client_info_mask and client_row_mask blind as described; controls still catch label and value divergence","NOT RUN: compat/tui-screen-diff.sh and compat/attached-client.sh"],"every_clause_asserted":"TUI-006: yes. All three clauses assert at the tip and compat/tui-choosers.sh records nothing; the three cases the cycle 6 and cycle 8 gates left open (output-search-prompt, output-search-typed, output-selected) are flipped to same with a matching sabotage each. Caveat: clause 1's client-tree rows and clause 2's nothing are asserted through two masks this branch widened (see the two must-fix mask defects); the assertion is real but narrower than the header claims. TUI-014: no. Clause 1 asserts whole (client-info-view and client-info-view-off press i on BOTH sides). Clause 2 is SHORT: untouched. Clause 3 is SHORT: 61 asserted / 37 recorded, six of those this obligation's, no item closed. TUI-014 correctly left at active; TUI-006 at review with a null proof block.","notes":"No blocker is provable; no red at the tip on this box. The wire change is a pure tail append with both halves present. THE GUI HUNKS ARE TEST-ONLY (crates/zz/src/workspace/view.rs:3829 and crates/zz/src/command/palette.rs inside #[cfg(test)]); nothing in crates/zz/src reads the new field, so command-prompt -P now renders differently per client: over the pane's last row in the raw TUI, on the client surface in the desktop app, iOS and web. A PRE-EXISTING DIVERGENCE, NOT CHARGED: in real pane copy mode with the default copy-mode-position-format, after begin-selection + cursor-right the pin paints [120/179] while zz still paints [120/179] (2 results); both agree search_present=0, so it is a stale push: the daemon re-expands the indicator only when request_mode_refresh fires and its memo key (daemon.rs mode_kind, viewport.scrollbar, viewport.search) does not move when the marks clear, because viewport.search in session.rs copy_mode_facts is built from view.search unconditionally; every file on that path is byte-identical to origin/main; no fixture can see it (tui-copy-mode.sh pins copy-mode-position-format to '' and tui-choosers.sh to a form with no results clause). One-line follow-up: .filter(|_| mode.search_marks) on the viewport.search field. THE REGRESSION GATE IS NARROWER THAN IT LOOKS: tui-copy-mode.sh sets status off, so the prompt's row and the last row coincide; the oracle probe with status ON closes that hole. SUSPICIONS WITHOUT PROOF: environment.txt binary mtime 07:47 predates the revision it names; TUI-006's note says twelve sabotages (ten plus two equivalences); -P resolves the current pane ignoring -t (probably right: -t is a target-client); client_format_facts resolves against an augmented terminal-features array while client_feature_mask reads the bare option."}
```

## review_actions

Every must-fix applied, each in its own commit on the rebased branch, with the
reviewer's own probe re-run as proof where the reviewer named one.

1. **The wire.** `Open protocol version 103 for this cycle's wire appends`.
   `PROTOCOL_VERSION` is 103 at crates/zz-protocol/src/message.rs:21. Three
   assertions moved, not the two the reviewer counted: the in-crate
   `assert_eq!(super::PROTOCOL_VERSION, 102)`, the named test in
   crates/zz-protocol/tests/hunt_claims.rs (renamed to
   `protocol_version_on_this_commit_is_one_hundred_and_three`), and a third the
   reviewer did not have - the `ClientHello` frame in the same file pins the
   number twice in hex (`0x66` -> `0x67`), and `cargo test -p zz-protocol`
   failed on it until it moved. The two appends are split out of the v102
   paragraph of knowledge/protocol/wire-protocol.md into a v103 paragraph and a
   new v103 bullet in the version history, the document's own title and the
   knowledge/protocol/index.md listing say v103, and the `Appended in v102` doc
   on `ClientTerminalType` says v103. PROOF: `python3 compat/wire-version.py`
   exits 0 at this tip - "wire-version: 103 is unreleased (v0.9.1 shipped 102);
   appends are free" - and `compat/check.sh`, which runs it, exits 0.

2. **client_info_mask's blanket space collapse.** `Collapse only the fill a
   masked token pushes, not every column`. The collapse is now anchored to the
   substituted token: `re.sub(TOKEN_FILL, r"\g<1> ", masked)` where TOKEN_FILL
   matches one of the seven fixed tokens followed by two or more spaces, instead
   of `re.sub(r"  +", " ", masked)` over the whole line. The header sentence that
   already claimed this now describes what the code does, and says plainly that
   the label column is compared as it stands. The zz-side info-view sabotage runs
   under `ROW_MASK=client_info_mask` (the mask the client-info-view checkpoints
   assert under) and the pin-side one stays under `client_row_mask`, so the two
   sabotages now cover one mask each. PROOF, the reviewer's probe re-run
   verbatim against the mask body (gate-mask-probe.txt): the pin row
   `Client Name      │ /dev/pts/9 (PID 12345)` and the four-cell-shifted zz row
   `Client Name  │ /dev/pts/9 (PID 999)` no longer mask identical, while a row
   pair that differs only in the box fill after the token still does.

3. **client_row_mask's CUT.** `Truncate the cut title only on the row a box cuts
   it`. The unconditional `re.sub(r"(/dev/CLIENT).*?(?=│)", r"\1 (CUT) ")` is
   replaced by a truncation anchored to the one row it was measured for: the
   preview box's title row, recognised by that box's own top-left corner opening
   the row with the client name right after it, with the span between the name
   and the covering box's edge truncated to a fixed 13 columns rather than
   dropped. Measured at 100x40 (the probe re-run at this gate): the help box is
   42 columns wide at column 29, so the title span runs 17 characters for a
   one-digit pty number and shortens by one per extra digit; 13 is what a
   five-digit number leaves, and it keeps ` (sort: name)` whole, so the sort
   label is still compared. PROOF: the reviewer's probe re-run verbatim - the
   two client rows `(0) /dev/pts/9: 3 windows (attached) │ preview` and
   `(0) /dev/pts/9: 9 windows (detached) │ preview` no longer mask identical -
   plus a new self-check sabotage, `title cut, the sort label inside the span a
   box cuts`, which opens the client tree at 100x40, changes the sort order on
   the zz side alone, raises the help box on both, and requires the comparison
   to report it. It does, in exactly one row out of forty, and the old mask
   would have reported nothing. compat/tui-choosers.sh --self-check ends
   "every sabotage was caught and both equivalences passed" with 13 ok lines
   (12 before, plus this one).

4. **The reversed gap decision.** `Re-scope the pane prompt decision to the
   surface that draws it`. compat/tmux-gaps.json `prompt.pane-rendered` keeps
   its closed status and gains a dated 2026-09-14 measurement: under the
   2026-09-10 triage in knowledge/designs/tui-parity.md, an accepted gap that
   keeps a native presentation does not waive the raw TUI's cells, so the raw
   TUI now draws the pane-cell prompt for `command-prompt -P` and for copy
   mode's search prompts while the GPUI, iOS and web clients read nothing from
   `CommandPromptState.pane` and keep the client prompt. The item's title says
   that split now instead of asserting the whole of the old stance, and the
   measurement names the files on both halves. The four stale claims are
   corrected: crates/zz-protocol/src/key.rs (the doc on
   `bind_copy_mode_search_defaults`), crates/zz-protocol/src/catalog.rs (the -P
   flag help, now "the prompt belongs to the target pane, not the client"),
   knowledge/tmux/copy-mode.md and knowledge/tmux/key-tables.md.
   knowledge/tmux/gaps.md is regenerated, never hand-merged. PROOF:
   `python3 compat/tmux-tracker.py check` exits 0.

5. **Stale evidence.** `choosers-run-1.txt`, `-2.txt` and `-3.txt` and
   `choosers-self-check.txt` in both attempts are this gate's own runs at this
   revision, each stamped with the revision, the pin, the exact command, the
   start and end wall clock, the elapsed seconds and the exit code. The four
   identical pre-mask files they replace are gone, `choosers-run-4.txt`
   included. The red run this gate took is kept beside them as
   `choosers-run-0-flake-zoom-tree-open.txt` rather than dropped.

Nits: (a) applied, `Give each client feature helper back its own doc` - the
`c->term_features` paragraph is back above `client_feature_mask` and
`client_terminal_features_option` keeps the `tty_update_features` half as its
own. (b) applied, `Keep the ledger's non-ascii bytes where they are` - the
ledger is serialized with `ensure_ascii=False` and TUI-011's literal U+250C is
restored, so the only records that move against origin/main are the two this
lane owns. (c) applied in TUI-006's evidence_note, which now says how many runs
were taken and how many kept, for the lane's `filter-cleared` flake and for this
gate's own. The zone nit needs no code change and is carried in the board note.

## checks_run (this gate)

Every fixture in the tree, the full corpus delta, every touched package, the
workspace clippy, both trackers and compat/check.sh. Exit codes and last lines:
gate-fixtures.txt, gate-corpus.txt, gate-cargo.txt. Environment: gate-environment.txt.

Two reds, both identical at origin/main 45481a72 built in this same worktree,
so both are this box's baseline and neither is this lane's:

- `compat/attached-client.sh` exits 1 in `probe_command_output_navigation`,
  "zz current screen did not show ATTACHED_NAV_65 ATTACHED_NAV_MATCH within 10
  seconds" - `n` (search-again) does not advance to the next match in retained
  command output. Reproduced twice at this tip and once at origin/main.
  That probe rebinds `/` in copy-mode-vi to zz's native
  `copy-mode-search-prompt` for the zz side and says so on its own stdout
  ("native copy-mode-search-prompt editor; stock -P lifecycle measured
  separately"), so it exercises the replacement zz-only binding that TUI-006
  clause 2 explicitly excludes; the stock-binding half of clause 2 is asserted
  in compat/tui-choosers.sh (output-search-prompt, output-search-typed,
  output-selected, output-copied) at zero recorded across three runs. The probe
  is the closed gap `clients.tui-command-output-navigation`, and
  compat/attached-client.sh is a declared source of six obligations already
  verified on main (TUI-001, TUI-002, TUI-004, TUI-005, TUI-007, TUI-010), every
  one of which recorded "attached-client compatibility: PASS" at its own gate on
  alienware. This box has never run it before today. It needs a cycle 10 lane.
- `compat/tui-overlays.sh --self-check` exits 2 on its final equivalence,
  "equal settled on the zz screen did not settle within 10 seconds", five tries
  at this tip and twice at origin/main. Every one of its seven sabotages is
  caught; only the closing equivalence fails to settle. `compat/tui-overlays.sh`
  itself is green, 48 asserted comparisons identical, 0 recorded.

Three zz-daemon tests, `russh_socks::tests::loopback_forwards_*`,
`loopback_shutdown_*` and `ssh_inventory_prepares_*`, fail with ConnectionReset
at this tip and identically at origin/main; that whole module and Cargo.lock are
byte-identical to main on this branch.

## flips

None. No case in any fixture carries a `SIBLING:` reason that this gate flipped,
and compat/tui-choosers.sh and compat/tui-client-commands.sh both end
"(0 for a sibling lane)" on every run. This is the first gate of cycle 9, so
there is no sibling landing to flip against.
