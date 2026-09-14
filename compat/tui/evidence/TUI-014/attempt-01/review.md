# TUI-014 attempt-01, the cycle 8 commands lane, reviewed and gated

The reviewer's verdict is reproduced verbatim below, then what the gate did with
each item, then the gate's own measurements. Gate: cycle 6 commands, 2026-09-14,
worktree /home/demfabris/dev/zz-gate-tui7, branch gate-commands.

## Verdict, verbatim

```json
{
  "lane": "commands",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-014 clause 1",
      "severity": "blocker",
      "description": "Clause 1 is reported as proved, but the info preview the clause names ('the info preview `i` raises') is asserted by no case and diverges from the pin in five measured ways. compat/tui-choosers.sh never presses `i` on both sides: the only place `i` appears is the --self-check sabotage, which presses it on the PIN alone and asserts that the rows differ, so it can never catch a wrong zz info view. I drove both sides myself at 80x24, one client each, prefix D then i (probe scratchpad/probe.sh, output probe1.txt, 2026-09-13): (1) box title row: pin `┌ /dev/pts/5 (sort: name) (view: info) ───…`, zz `┌ /dev/pts/4 (sort: name) (view: preview) ───…` — zz hardcodes `view: \"preview\".to_owned()` in chooser_presentation.rs:459 while the pin calls mode_tree_view_name(data->data, \"info\") at window-client.c:455; (2) every info row's separator: pin `│ Client Name   │ /dev/pts/5 (PID …)`, zz `│ Client Name   x /dev/pts/4 (PID …)` — the pin's `#[…,acs]x` is ACS line drawing and grid.markup in crates/zz-tui/src/render/chooser.rs prints the literal letter x; (3) `│ Terminal Type │ tmux next-3.8` on the pin, `x Unknown` on zz; (4) `│ Activity Time │ … 2026 (0s)` on the pin, `… 2026 ()` on zz (#{t/r:client_activity} expands empty); (5) WINDOW_CLIENT_INFO_LINES in chooser_presentation.rs carries 10 of the pin's ~22 window_client_info_lines — the six Features rows, the acs rule, mouse, set-clipboard, get-clipboard, focus-events, extended-keys and set-titles are missing, so the pin's rows 21-22 are `Features … 256 RGB bpaste ccolour` / `clipboard cstyle extkeys focus` where zz's are `prefix x C-b` / `escape-time x 10 ms`. TUI-014's evidence_note states 'i swaps in ChooserPreview::Markup over the pin's info lines', which overstates what landed. Clause 1 also leaves d, D, x, X and the default `detach-client -t '%%'` template unasserted (the worker's notes admit this); I confirmed by probe that those five behave like the pin with two clients per side, so the behaviour is right and only the evidence is missing.",
      "suggested_fix": "Either add a both-sides info case to compat/tui-choosers.sh (open with prefix D, press i, compare whole-screen through the existing client_row_mask plus a mask for client_written/PID) and fix the four drawing divergences — set ChooserPresentation.view to \"info\" when chooser.info_preview, teach the Markup path the `acs` style attribute, and copy the pin's remaining info lines — or drop the clause-1-proved claim: mark the info preview an open recorded case in TUI-014's evidence_note with the measurement above and leave clause 1 open. A two-client scene (a second attach in a lower split, which my probe shows costs one split-window) also closes d/D/x/X and the default template."
    },
    {
      "obligation": "TUI-006 (via TUI-014's evidence_note and the worker report)",
      "severity": "blocker",
      "description": "TUI-014's evidence_note says 'client-tree-open, the one case compat/tui-choosers.sh recorded for TUI-006, is `same` now, so that fixture holds no recorded case for TUI-006 any more and the commands gate can verify it', and the worker report repeats 'the three cases compat/tui-choosers.sh still records are command-output cases belonging to other records'. Both are false. My run of compat/tui-choosers.sh at the tip ends 'all 73 asserted comparisons identical, 3 recorded not asserted (0 for a sibling lane)', and the three are output-search-prompt, output-search-typed and output-selected (compat/tui-choosers.sh:868, 870, 875). All three sit inside TUI-006's own acceptance clause 2 ('Compare output search and prompt lifecycle … including selection/copy and return to the underlying pane'), and TUI-006's record names them itself: its evidence_note opens 'CYCLE 6 PUNCH LIST, ACTIVE ON THREE RECORDED CASES PLUS choose-client' and its WHAT REMAINS paragraph measures each of the three. Grepping campaign.json, no other obligation claims them. A gate that acts on the worker's sentence sets TUI-006 verified with three recorded cases inside a clause, which is exactly what the contract forbids.",
      "suggested_fix": "Correct that sentence in TUI-014's evidence_note to say the flip clears the choose-client case only, and that TUI-006 stays active on output-search-prompt, output-search-typed and output-selected. The commands gate must NOT set TUI-006 verified at this tip."
    },
    {
      "obligation": "TUI-014 clause 3 / compat/tmux-gaps.json clients.interactive-refresh",
      "severity": "must-fix",
      "description": "client-tree-open in compat/tui-client-commands.sh moved its recorded reason from commands.native-client-tools to clients.interactive-refresh, but that gap was not widened to cover it. Its acceptance clause 2 still reads '… and clientless copy-mode, choose-tree and choose-buffer keep their attached-client errors' — choose-client is named neither there nor in the gap's items, and the gap's reason paragraph is unchanged. The case therefore cites an accepted gap whose written decision does not include it. clients.interactive-refresh is one of the three gaps this batch names, so the lane could have registered it.",
      "suggested_fix": "Add choose-client to that acceptance sentence (or an item plus reason line) in compat/tmux-gaps.json with the dated measurement `choose-client -t %N` -> `choose-client requires an interactive client`, exit 1, and regenerate knowledge/tmux/gaps.md with python3 compat/tmux-tracker.py write-report."
    },
    {
      "obligation": "TUI-014 evidence (compat/tui/evidence/TUI-014/attempt-01/environment.txt)",
      "severity": "must-fix",
      "description": "environment.txt attests 'revision: 6cc5e0b82c2a9ee39512bed2337ec409d7dbfe7b', 'worktree: 1 modified paths' and 'base: origin/main b17232ac…'. 6cc5e0b8 is not an ancestor of the tip a3fc1651 (pre-rebase commit, confirmed with git merge-base --is-ancestor), the real merge-base is 289c8a7c, and the runs are attested from a dirty worktree. On this box a debug zz build is not bit-reproducible, so the revision plus a clean worktree is the only attestation the sha256 has; as written it points at a commit that is not on the branch.",
      "suggested_fix": "Re-capture environment.txt from the clean tip (revision a3fc1651, base 289c8a7c, worktree clean) with the sha256 of the binary the re-run used."
    },
    {
      "obligation": "TUI-014 evidence notes",
      "severity": "nit",
      "description": "compat/tui/evidence/TUI-014/attempt-01/notes.md does not name cargo-crates.txt or check-sh.txt, though both files are in the directory; the rule is that notes.md names every file.",
      "suggested_fix": "Add the two lines to the 'What is in this directory' list."
    },
    {
      "obligation": "Ledger hygiene (compat/tui/campaign.json, TUI-011)",
      "severity": "nit",
      "description": "The ledger was re-dumped with json.dump defaults (ensure_ascii=True), which rewrote one line inside TUI-011's evidence_note — the literal `┌` became the escape `\\u250c` at campaign.json line 743. The decoded value is identical, so no record changed meaning, but TUI-011 is nobody's this cycle and the branch still carries a diff line in it, and it is a needless conflict site against the two lanes that merge first.",
      "suggested_fix": "Re-dump with ensure_ascii=False (the file has no other \\u escapes at origin/main) so only the lane's own records appear in the diff."
    },
    {
      "obligation": "Zone discipline",
      "severity": "nit",
      "description": "Five files outside the batch's named zones are edited but not listed among the notes' zone excursions: crates/zz-daemon/src/keys.rs (new client_mode_key_action), crates/zz-mux/src/sort.rs (WINDOW_CLIENT_ORDER_SEQ), crates/zz-mux/src/lib.rs (its re-export), crates/zz-mux/tests/hunt_claims.rs (COMMAND_SPECS 78->79 plus an info_preview field) and crates/zz/src/chooser/tree.rs (the GUI's two match arms for the new variant). Each is forced and small, and keys.rs and sort.rs are in TUI-014's sources list, but the batch asks for every excursion to be named with the measurement that forced it. render.rs (one arm in paint_chooser) is named and is the only edit to a file the batch explicitly marks NOT yours.",
      "suggested_fix": "Add the five to the 'Zone excursions' section of notes.md."
    }
  ],
  "checks_run": [
    "git fetch origin main + campaign/*; detached REVIEWDIR /home/demfabris/dev/zz-tui-commands-review at a3fc1651 (branch tip, merge-base with origin/main = 289c8a7c, 5 commits)",
    "touch crates/**/*.rs + Cargo.* then cargo build -p zz through the slot/cap wrapper (CAP 5G) into the worker target: exit 0",
    "compat/tui-choosers.sh at the tip: exit 0, 'all 73 asserted comparisons identical, 3 recorded not asserted (0 for a sibling lane)' (3m00s)",
    "compat/tui-choosers.sh --self-check: exit 0, 'self-check complete: every sabotage was caught and both equivalences passed', including the new self-check-client-info case",
    "MY OWN SABOTAGE: hline's rule glyph in crates/zz-tui/src/render/chooser.rs changed from U+2500 to '-', rebuilt, fixture re-run -> exit 1, '17 of 73 asserted comparisons differ' (every client-mode checkpoint), then reverted and rebuilt clean (worktree verified clean, binary rebuilt at the tip)",
    "compat/tui-client-commands.sh at the tip: exit 0, 'all 61 asserted comparisons identical, 37 recorded not asserted', client-tree-unknown-flag / -bad-sort / -usage green, client-tree-open recorded",
    "compat/attached-client.sh (TMUX_BIN=pin, ZZ_BIN=tip): exit 0, 'attached-client compatibility: PASS'",
    "compat/tui-screen-diff.sh: exit 0, 'all 147 asserted checkpoints identical, 6 recorded not asserted'",
    "delta corpus, chunk 1 (--strict-geometry): smoke/keys-prefix-stock, smoke/command-flag-errors, smoke/positional-maximums, list-keys-padding — 4 rows, zero divergences",
    "delta corpus, chunk 2 (--strict-geometry): smoke/args-parse-choosers, smoke/chooser-tree-vocabulary, smoke/chooser-buffer-vocabulary, strict-key-validation — 4 rows, zero divergences (the two vocabulary rows and args-parse-choosers are rows the worker did not run)",
    "grep of compat/scenarios for every string the landing changes: choose-client appears only in smoke/fixtures/command-flag-errors.tsv (updated); no scenario asserts 'requires an interactive client' or a prefix-D binding",
    "ORACLE SPOT-CHECK 1 (probe.sh): pin and zz side by side at 80x24, one client each, prefix D — client mode rows, preview box, rule and copied status row identical in shape; then `i` — five measured divergences in the info view (see blocker 1); then Enter with no template — both sides detach, so the default detach-client -t '%%' matches",
    "ORACLE SPOT-CHECK 2 (probe2.sh): two clients per side at 80x27 and 80x12 — O steps name/size/creation/activity in the pin's order on both sides, and the row order agrees with the pin for all four (lexicographic name, ascending size, oldest-first creation, newest-first activity); `d` detaches the selected row's client on both sides",
    "pin source read: cmd-choose-tree.c cmd_choose_client_entry (usage and args), window-client.c window_client_info_lines / help_lines / order_seq / do_detach, mode-tree.c title composition and view_name",
    "cargo test -p zz-protocol -p zz-mux -p zz-tui --jobs 3 -- --test-threads=3: exit 0",
    "cargo test -p zz-daemon --jobs 3 -- --test-threads=3: exit 0 (893 + suites; no client_focus_closes_display_panes flake seen)",
    "cargo test -p zz --jobs 3 -- --test-threads=3: exit 0, including the 125 cli_binary tests",
    "cargo clippy -p zz-protocol -p zz-mux -p zz-tui -p zz-daemon --all-targets --all-features -- -D warnings: exit 0; cargo clippy -p zz likewise exit 0",
    "python3 compat/tui/tracker.py check, python3 compat/tmux-tracker.py check, python3 -B compat/tui/tracker_test.py (8 tests), compat/check.sh: all exit 0",
    "both write-reports re-run in the worktree: git status stayed clean, so knowledge/tmux/tui-parity.md and gaps.md are regenerated",
    "wire audit: PROTOCOL_VERSION still 102; ChooseTreeKind::Clients, ChooseTreeTarget::Client, ChooserPreview::Client and ::Markup, ChooseTreeAction::ClientDetach/ClientDetachTagged/ClientInfo all appended at the tail of their enums, consumer half in the same push (zz-tui render/chooser.rs, zz/src/chooser/tree.rs), v102 history line added to knowledge/protocol/wire-protocol.md. MuxEffect::ChooseTree's mid-struct info_preview field is not Serialize and never crosses the wire",
    "ledger ownership: field-by-field JSON compare against origin/main — only TUI-014 (status, sources, evidence_note, next_action), TUI-016 and TUI-017 (evidence_note) changed values; TUI-006, TUI-011, TUI-012, the baseline, milestones and pin untouched in value (TUI-011 changed only in byte encoding, see nit)",
    "gap audit: commands.native-client-tools loses command:choose-client and keys.default-prefix loses key:prefix:D, each with a dated 2026-09-13 measurement in the reason; no other gap item moved; both gaps keep their decisions",
    "child-obligation audit: TUI-014/016/017 carry every field, are outside the frozen twelve-id baseline, and TUI-011 depends_on names all five children",
    "roster audit: every entry in compat/tui-client-commands.sh's header table still has a case below it (choose-client, clock-mode, customize-mode, switch-mode, suspend-client, server-access, show-messages, capture-pane, refresh-client, the lock family, the buffer family, show-hooks, display-message, source-file, split-window)",
    "hygiene: no commit carries an attribution trailer, no .log file in the diff, git check-ignore -v clean on the evidence directory, no binary copies made under /tmp, my two probe socket files removed, pgrep -fa 'zz-cli-|zz-user|zzprobe' clean, REVIEWDIR clean at a3fc1651"
  ],
  "every_clause_asserted": "no — TUI-014 clause 1 does not fully assert: the info preview it names is unasserted and measurably unlike the pin (view label, acs separator, Terminal Type, t/r time, and twelve missing info lines), and d, D, x, X and the default detach-client -t '%%' template are implemented but unasserted. Clause 2 is untouched and clause 3 holds the recorded client-tree-open case, both of which the worker states honestly. TUI-016 and TUI-017 assert nothing and are honestly left unmeasured. Separately, TUI-006 does NOT clear at this tip: compat/tui-choosers.sh still records output-search-prompt, output-search-typed and output-selected, all inside TUI-006's clause 2.",
  "notes": "THE LANDING ITSELF IS SOUND. choose-client leaves UNIMPLEMENTED_TMUX_COMMANDS and the raw TUI draws the pin's client mode; I ran the fixture, its self-check, the client-commands fixture, attached-client, tui-screen-diff, eight corpus rows, every touched crate's tests and clippy, cargo test -p zz, both trackers and check.sh, and everything is green at the tip. The fixture is not vacuous: my own one-glyph sabotage of the preview rule took it from 0 to 17 differing comparisons. The two blockers are claims, not code — one overclaimed clause and one false statement about TUI-006 — and both are text fixes the gate can apply before merge. Merge is fine once they are corrected; neither TUI-014 nor TUI-006 may be set verified at this tip.\n\nTHE MASK IS NARROW BUT NOT FREE. client_row_mask rewrites only lines that carry that side's own client name: the name becomes /dev/CLIENT, any HH:MM on that line becomes NN:NN, and any run of two or more U+2500 collapses to one. On the client screens that is the tree row and the preview box's title row, so the box title's fill length and any clock printed on it are outside the comparison. Everything else on those screens, and the cursor, is compared whole, and my sabotage confirms it. Worth knowing, not worth blocking.\n\nWHAT THE SORT CASES ACTUALLY PROVE. client-sort-size/creation/activity/name run with one client per side, so they assert the sort LABEL and that O steps the pin's window_client_order_seq — they cannot see ordering. I closed that hole myself with a two-client probe: all four orders and the d key agree with the pin. No defect, but the ledger's '17 client-mode checkpoints' reads stronger than it is.\n\nFOR THE GATE, ON MERGE ORDER. The commands lane is last and overlaps campaign/tui-keys on crates/zz-protocol/src/{catalog.rs,key.rs,message.rs}, crates/zz-mux/src/{command.rs,compat_manifest_tests.rs,lib.rs}, crates/zz-mux/tests/hunt_claims.rs, crates/zz-daemon/src/daemon.rs, compat/tmux-gaps.json, compat/tui/campaign.json and the v102 entry of knowledge/protocol/wire-protocol.md. Eleven inventory counts move with this landing (specs 83->84, positional maximums 72->73, flag shapes 503->515, supported 471->483, unimplemented specs 9->8, usage overrides 20->21, spellings 157->158, diagnostic 628->632, required 408->413, prefix cases 517->522, zz keys 317->318 with the shared/missing/structural splits, plus the command-flag-errors probe counts 516->521). After rebasing onto the main that carries keys, re-derive those numbers from a test run rather than taking either side of the conflict, and merge the wire-protocol v102 entry by union.\n\nONE MORE THING THE GATE SHOULD KNOW. The clientless error text changed from a hardcoded 'choose-tree requires an interactive client' to '{command_name} requires an interactive client'. Only choose-client is affected in practice (choose-tree keeps its wording, find-window continues past the branch), no scenario asserts the old string, and compat/tui-client-commands.sh asserts the new one — but it is a user-visible string change riding in a chooser landing.\n\nFLAKE WATCH. I did not see the filter-cleared / pane_current_command bash-vs-sh flip the worker reports, in either of my two full fixture runs; the sabotage run's non-client rows were green too. If it appears at the gate, it is the sysinfo path in terminal_current_command, untouched here.\n\nArtifacts of this review: the review session's scratchpad holds probe.sh, probe1.txt (the info-view comparison), probe2.sh, probe2.txt (the two-client sort and detach comparison), sabotage.txt (my sabotage run) and every fixture, corpus, test and clippy log I quote."
}
```

## What the gate did with each item

VERDICT WAS approve-with-fixes: two blockers, two must-fixes, three nits. Every
blocker and must-fix is applied in its own commit with the reviewer's own probe
re-run at the gate tip as proof. Neither TUI-014 nor TUI-006 is set verified.

### blocker 1, TUI-014 clause 1, the info preview - APPLIED as the text fix

The reviewer's two options were to fix the drawing and add a both-sides case, or
to drop the clause-1-proved claim and record the measurement. A gate does not
land a chooser feature on its own judgement, so it took the second.

Re-ran the reviewer's own probe against the gate build (gate-probe-info.sh, a
copy of theirs with ZZ_BIN pointed at this gate's binary), three times:
gate-probe-info-1.txt, -2.txt, -3.txt. FOUR of their five divergences reproduce
on every run and are now written into TUI-014's evidence_note with their causes:
the `view: preview` title against the pin's `view: info`, the literal `x` where
the pin's acs attribute draws a vertical rule, `Terminal Type x Unknown` against
`tmux next-3.8`, and ten of the pin's info lines against twenty-three.

THE FIFTH DID NOT REPRODUCE. The reviewer measured `#{t/r:client_activity}`
expanding empty on zz - `... 2026 ()` against the pin's `(0s)`. All three gate
runs have zz printing `(0s)`, matching the pin exactly. It is recorded as
intermittent rather than asserted as a divergence, because a record that names a
divergence nobody can reproduce costs the next lane a hunt.

Two details the gate checked rather than copied. The reviewer wrote "10 of the
pin's ~22"; counted from the pin's own array the number is 23 entries, and the
thirteen zz is missing are Features plus its five continuation rows, the acs
rule row, mouse, set-clipboard, get-clipboard, focus-events, extended-keys and
set-titles. And the pin does not only set the info view name on the `i` key:
window_client_init calls mode_tree_view_name from the `-i` flag at open too, so
`choose-client -i` is a second way in that zz's hardcoded string gets wrong.

The reviewer's claim that no case presses `i` on both sides is exact:
compat/tui-choosers.sh line 1153 is the only `i` in the file and it goes to the
pin alone.

### blocker 2, the false TUI-006 sentence - APPLIED, and TUI-006 is NOT verified

Confirmed from the other side before touching the text. compat/tui-choosers.sh
ran three times at the gate tip and each run ends `all 73 asserted comparisons
identical, 3 recorded not asserted (0 for a sibling lane)`; the three are
output-search-prompt, output-search-typed and output-selected, and TUI-006's own
record measures each of them inside its clause 2. So the lane's sentence - that
the fixture holds no recorded case for TUI-006 any more and the gate can verify
it - is false, and TUI-006 stays active. Its dependencies were never the blocker:
TUI-002, TUI-003 and TUI-004 are all verified on main.

### must-fix 3, the gap that did not cover its own case - APPLIED

clients.interactive-refresh now names choose-client in the acceptance clause and
carries a dated measurement in its reason, and knowledge/tmux/gaps.md is
regenerated. The measurement is the gate's own (gate-probe-clientless.sh,
gate-probe-clientless.txt): clientless, `choose-client -t %0` answers
`choose-client requires an interactive client` at exit 1 on zz and exits 0 with
no output on the pin.

ONE THING THE REVIEWER'S PRESCRIPTION GOT WRONG, found by running it. Their
suggested wording, and the fixture's own recorded note, say the pin "sets a mode
on the pane whether or not the caller is a client". Not for this command:
cmd_choose_tree_exec returns CMD_RETURN_NORMAL before window_pane_set_mode when
server_client_how_many() == 0, so a clientless choose-client opens no mode at all
and `#{pane_in_mode}` reads 0, where clientless choose-tree and choose-buffer
both take the mode and read 1. Measured on both binaries. So what diverges for
choose-client is the exit status and the error text, not a mode zz failed to
open - and the fixture's note said otherwise, which is its own small false
statement in evidence. Corrected in a separate commit, with the fixture and its
--self-check re-run afterwards.

### must-fix 4, the evidence attestation - APPLIED

environment.txt attested revision 6cc5e0b8, which git merge-base --is-ancestor
puts off this branch, a base that was not the merge-base, and a dirty worktree.
The lane's block is kept and labelled superseded, because it still records the
box and the pin its runs used, and the gate's own capture is appended: clean
worktree, a revision on the branch, and the sha256 of the binary every gate run
used. The four record commits after fba6dcf6 carry no Rust, so that one binary
is the code at each of them.

### nit 5, notes.md missing two filenames - APPLIED, with the gate's files too
### nit 6, the ensure_ascii re-dump - APPLIED

campaign.json is re-dumped with ensure_ascii=False. The escape inside TUI-011 is
gone and the file now round-trips byte-identically through
json.dumps(indent=2, ensure_ascii=False), so a later lane's re-dump will not
churn records it does not own.

### nit 7, five unnamed zone excursions - APPLIED to notes.md

## What the gate measured that the review did not

THE COUNT CONFLICTS WERE RE-DERIVED, NOT PICKED. The reviewer told the gate to
re-derive the eleven inventory numbers from a test run rather than take a side.
The rebase conflicted on five files; the two Rust ones are pure count conflicts.
Each was resolved as base + both lanes' deltas, then proved by the tests that
own them: (supported, unsupported) 471/32 base, 472/31 on main, 483/32 on the
lane -> 484/31; usage_overrides 21; zz_keys 323, shared 232, missing 71,
structurally matching 195, and the by-table list keeps main's ("root", 5) beside
the lane's ("prefix", 57). The arithmetic is checked by the data: 232 + 91 = 323,
232 + 71 = 303 pinned bindings, 61 + 72 + 57 + 5 = 195, 484 + 31 = 515 flag
shapes. compat_manifest_tests and the catalog tests pass at the tip, which is
what makes those numbers a measurement rather than a guess.

THE KEYS LANE'S OWN FIXTURE WAS RUN. This lane edits crates/zz-protocol/src/key.rs,
so the gate ran compat/tui-mouse.sh, which is not on its stage-3 list:
`all 26 asserted checks identical` with 10 recorded, the same numbers the keys
gate pushed. TUI-008 is still active on those ten, so TUI-012 stays at review.

THE WHOLE DELTA RAN, NOT A SAMPLE. The reviewer sampled eight corpus rows. This
gate is the last of the cycle, so its scope is the lane's delta plus every keys
and status scenario plus smoke/tui-client-input-backpressure: 164 rows selected,
164 completed, coverage proved by diffing the scenario names in every chunk log
against the selection. 162 clean; the two that diverged are this box's documented
environmental rows and are measured in gate-corpus.txt.

THE FLAKE THE WORKER WARNED ABOUT DID NOT APPEAR. filter-cleared and
find-window-tree were green in all three gate runs of compat/tui-choosers.sh, as
they were in both of the reviewer's.
