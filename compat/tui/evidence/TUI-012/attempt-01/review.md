# TUI-012 attempt-01 - review and what the gate did about it

Lane `campaign/tui-superset`, cycle 6, gated third and last in the order input, roster,
superset. Gate worktree `/home/demfabris/dev/zz-gate-tui7`, gate branch `gate-superset`,
rebased onto `origin/main` 46a02abe (input at a1b865e4 and roster at 46a02abe already in).

## The reviewer's verdict, verbatim

```json
{
  "lane": "superset",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-012",
      "severity": "must-fix",
      "description": "compat/tui/evidence/TUI-012/attempt-01/environment.txt records `head: 53d206dc5167ab009f41b6bbd1a15af6ecea6c18`, which is origin/main, not any commit on campaign/tui-superset. `git log --follow` shows environment.txt was written once, in 58931ca3, and never re-stamped; the three run files and the self-check were then REPLACED in e9f56a5d ('Drive every declared verb through a user binding too', +236 lines of fixture, run counts 143 -> 189). So the environment file attests a revision at which compat/tui-superset.sh does not exist, while the runs beside it were produced by a later fixture from a dirty worktree. The file's own attestation model says it plainly ('a debug build is not bit-reproducible here, so the revision plus a clean worktree attests it'), and the revision it names cannot attest anything here. The runs themselves are real: my run of the fixture at d8de8190 is byte-identical to the recorded tui-superset.run-1.txt.",
      "suggested_fix": "Re-stamp environment.txt with the branch tip sha (and the worktree state at the moment of the recorded runs), then re-run the trackers and commit. No re-run of the fixture is needed - the recorded output reproduces exactly at the tip."
    },
    {
      "obligation": "TUI-012",
      "severity": "must-fix",
      "description": "compat/tui/evidence/TUI-012/attempt-01/notes.md's 'Files here' section names `clippy-zz-mux.txt`, which does not exist in the attempt directory, and never names `cargo-test-zz-daemon-hook-partition.txt`, which does. The ledger rule for an attempt is 'notes.md naming every file'. A reader following the notes looks for a clippy run that was never recorded (and, since no crate source changed, arguably never needed to be) and misses the daemon evidence that is there.",
      "suggested_fix": "In notes.md, drop the `clippy-zz-mux.txt` entry (or record the run it names) and add `cargo-test-zz-daemon-hook-partition.txt` to the file list."
    },
    {
      "obligation": "TUI-012",
      "severity": "nit",
      "description": "The corpus-grep claim is wrong in both notes.md and the worker report: 'the only row that names a superset verb at all, compat/scenarios/command-item-format.txt, names select-pane-kind in a comment'. Three further corpus rows name a superset verb - compat/scenarios/smoke/config-discovery.txt:17, config-discovery-explicit.txt:19 and config-discovery-launcher.txt:18 each carry `zz-only: reload-config` - and compat/scenarios/smoke/fixtures/config-discovery-import.sh runs `import-tmux-config` twice (lines 9 and 18). The conclusion survives: I ran all three rows at the tip (12, 13 and 12 steps) and every one is clean, exit 0, which is expected because the branch changes no crate source. But the sentence is the lane's own 'I grepped the corpus' claim, and the next reader will trust it as a measurement.",
      "suggested_fix": "Correct the sentence in notes.md (and the ledger's evidence_note if it repeats it) to name the four rows, and record the config-discovery run beside corpus-command-item-format.txt."
    }
  ],
  "every_clause_asserted": "yes"
}
```

The reviewer's `checks_run` and `notes` are reproduced in full at the end of this file.

## What the gate did

### Must-fix 1, environment.txt. FIXED, in its own commit e9199e38.

PROBE, RE-RUN AS PROOF rather than taken on trust. At the rebased tip:
`git log --follow origin/main..HEAD -- .../environment.txt` returns exactly one commit
(f468e4fd, the rebased f-mapping of the reviewer's 58931ca3), while the run files return two
(f468e4fd and 081c1874, the rebased e9f56a5d) - so the environment file really was written once
and the runs really were replaced under it. `git cat-file -e 53d206dc:compat/tui-superset.sh`
fails with "exists on disk, but not in 53d206dc": the revision the file attested carries no
fixture at all. Defect confirmed exactly as described.

The file now names the gate tip d002ba96, states `git status --short` empty at every run,
says plainly which files are the lane's (its three cargo captures, taken at its own tip) and
which are the gate's (every fixture run), and records that the two commits the gate adds touch
only evidence, the ledger and the generated reports, so the runs stand at the final tip.

NOT VACUOUS, and stronger than the fix needed to be. The reviewer found their run of the
fixture byte-identical to the recorded run-1. At the gate tip I ran it three times: all three
are md5 `281802bcf8f5ff222c9396738d145b83`, and so are all three of the lane's recorded run
files. Then I copied EVERY gate fixture output over the recorded one and `git status` showed
only the files I had actually edited - tui-superset run-1/2/3 and self-check, tui-screen-diff
and its self-check, tui-pane-geometry, status-row and attached-client are all byte-identical at
the gate tip with input and roster merged in. The evidence reproduces; only its attestation was
wrong.

### Must-fix 2, notes.md's file list. FIXED, same commit.

Confirmed by `ls` against the section: `clippy-zz-mux.txt` is absent and
`cargo-test-zz-daemon-hook-partition.txt` is present and unnamed. The entry now names the
daemon file, says in one parenthesis what the first revision got wrong, and points workspace
clippy at `gate-cargo.txt`, where the gate actually recorded it.

### Nit 3, the corpus-grep claim. TAKEN, not left.

This is a nit by severity but it is the lane's own measurement claim, and a wrong measurement
in an evidence file is the kind of thing the next reader inherits, so I re-measured rather than
just reworded. Grepping all 25 declared verbs across `compat/scenarios` returns exactly the four
rows the reviewer named and no others: `command-item-format.txt:103` (a `select-pane-kind`
comment), and `smoke/config-discovery.txt:17`, `-explicit.txt:19`, `-launcher.txt:18`
(`zz-only: reload-config`), with `smoke/fixtures/config-discovery-import.sh` running
`import-tmux-config` at lines 9 and 18 - and that fixture is staged by all three config rows,
which is why the count is three rows and not one. notes.md now names all four. All four ran in
the gate's sweep and all four are clean; the three config rows are recorded in
`corpus-config-discovery.txt` (12, 13 and 12 steps, every channel zero), matching the reviewer's
own numbers.

The ledger's `evidence_note` did not repeat the claim, so there was nothing to correct there.

### Sibling flips

None. `sibling_cases` is empty and the fixture records zero cases, so there was nothing to flip
and nothing to revert.

## Why TUI-012 is NOT verified

VERIFIED MEANS EVERY CLAUSE, and this lane's three clauses do all assert - the reviewer's
`every_clause_asserted` is "yes", the fixture is 189 asserted with zero recorded, and the gate
reproduced it three times. The obligation is held on its DEPENDENCIES, not on its own evidence.

`TUI-012.depends_on` is TUI-003, TUI-004, TUI-008, TUI-009, TUI-010. Read off the ledger at the
gate tip: TUI-003 verified, TUI-004 verified, TUI-010 verified, **TUI-008 active**, **TUI-009
active**. The input gate landed both of those lanes' work and set neither verified: TUI-008
still holds 17 recorded cases across all three of its clauses (eight stock root bindings zz does
not install, a paste written through an overlay, the focus-delivery gate, the mouse_* formats)
and TUI-009 holds 15 (widths, non-utf8, silent and extended pane_key_mode, and thirteen
client_colours and client_termfeatures rows blocked on daemon accessors).

So the held rule applies: the proof block is filled at the pre-records tip, the status stays
`review`, the evidence_note opens by saying the proof is complete and what it is held on, and
next_action names what must verify first. No gate may set TUI-012 verified until TUI-008 and
TUI-009 are verified on main.

## The gate's own runs

- Workspace: `gate-cargo.txt`. zz-mux, zz-daemon (lib 893 and every integration binary), zz
  (638 lib, cli_binary 125), workspace clippy with `-D warnings`, and `cargo fmt --all --check`,
  every one exit 0 through the two-slot lock at MemoryMax=7G with --jobs 6, one at a time.
- Corpus: `gate-corpus.txt`. All 238 rows of the last-gate scope (the 237-row delta plus
  copy-mode-bindings, the one keys/status scenario the delta misses; the backpressure row is
  already inside it), coverage verified by set difference. 229 clean, four known/ rows carrying
  their designed divergence, five environmental rows.
- Fixtures: `gate-fixtures.txt`. Every TUI fixture and every self-check, all exit 0 ON THE FIRST
  RUN. No rerun, no flake.

Two things the gate can tell the campaign that the lane could not, because this is the first
tree carrying both earlier lanes at once: the input landing's tui-caps counts are intact at
351/15, and the roster landing's tui-choosers records are intact at 4 with 0 for a sibling lane.

## The reviewer's suspicions

The reviewer raised five suspicions explicitly marked "not defects". The gate leaves all five as
they stand and did not turn any into a change, for these reasons:

1. `run_names` asserting negatively (a name passes unless the answer is exactly
   `unknown command: NAME`) - bounded in practice because `fresh_group` dies first, and the
   reviewer's own sabotage A shows the channel reports a real miss. Changing it is fixture design
   beyond the must-fixes.
2. `equals window-columns-under-the-sidebar 62x29` comparing against a window that was already 62
   columns - a weak case sitting beside the strong cell-for-cell comparison that carries clause 3,
   with the real column arithmetic (120 -> 91 -> 120) asserted in the sidebar group.
3. `bound_reaches_the_verb` putting the reporting command first for debug-marker - declared in the
   fixture header, and debug-marker's direct path is asserted.
4. The editor pane never drawn because the product gates it behind `experimental-editor-pane`.
   THE GATE SIGNS OFF ON THIS CONSCIOUSLY, as the reviewer asked: a product setting is not a
   parity profile. The other three pane kinds need no flag, the pin has no editor pane to compare
   against, and the picker's refusal message is asserted as the observable. Clause 1's "no
   compatibility profile and no feature activation flag" is about a zz-wide parity mode, and no
   such mode exists or was added.
5. The client-wider-than-its-window finding belongs to the client-inventory obligation, not here.
   AGREED AND ACTED ON: see problems in the gate report - it currently lives only in TUI-012's
   evidence_note, where it will be lost when that note is next rewritten, and it needs to reach
   the client-inventory obligation as a case. The gate does not own that obligation's record and
   did not write into it.

## The reviewer's checks_run, verbatim

- SETUP: fetched origin (main 53d206dc, campaign/tui-superset d8de8190); merge-base = 53d206dc, so main has not moved under the lane; REVIEWDIR /home/demfabris/dev/zz-tui-superset-review checked out --detach at d8de8190, git status clean
- BUILD: touched crates/**/*.rs and Cargo.* in the review worktree, CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-superset/target, cargo build -p zz --jobs 4 through the two-slot flock with MemoryMax=5G -> exit 0 in 51.7s
- ZONES: git diff origin/main...HEAD touches only compat/tui-superset.sh (new, 1545 lines), compat/tmux-gaps.json, compat/tui/campaign.json, compat/tui/evidence/TUI-012/attempt-01/*, knowledge/tmux/gaps.md, knowledge/tmux/tui-parity.md. `git diff origin/main...HEAD -- crates/` is EMPTY: no source file changed anywhere, so input.rs, app.rs, tty.rs and terminal_event.rs are untouched and there are no render.rs hunks to name
- WIRE: no crate change at all, so PROTOCOL_VERSION stays 102, no message/field/variant appended, nothing to add to the v102 history line. Rule satisfied vacuously
- GUI / comments / attribution: no crate file changed, so the GUI builds unchanged with no presentation change; no code comments added; `git log origin/main..HEAD --format=%B | grep -iE 'co-authored|generated with|claude|noreply@'` is empty
- LEDGER OWNERSHIP: campaign.json diff touches only TUI-012 and only its status (unmeasured -> review), sources, evidence_note and next_action. proof stays null. No other id, no baseline, no milestone, no pin touched
- GAPS OWNERSHIP: tmux-gaps.json diff touches only commands.native-superset, keys.native-defaults and pane.floating-model (reason strings) plus updated_on 2026-09-11 -> 2026-09-13. Zero items closed, each of the three carries a dated 2026-09-13 measurement saying so, so no TMUX_OPTION_CONSUMERS or compat_manifest_tests.rs partition move was owed
- TRACKERS: python3 compat/tui/tracker.py check -> valid and current; python3 compat/tmux-tracker.py check -> valid and current; re-ran both write-report at the tip -> git status --porcelain empty, so the generated reports really were regenerated
- check-ignore: git check-ignore -v on the attempt directory and every file in it -> rc 1, nothing ignored; no .log file anywhere in the diff
- FIXTURE at tip, run 1: compat/tui-superset.sh -> exit 0, 'all 189 asserted cases hold, 0 recorded', 1m44. My output is BYTE-IDENTICAL to the recorded tui-superset.run-1.txt (md5 match on all three recorded runs)
- FIXTURE at tip, runs 2 and 3: exit 0, 189 asserted cases, 0 recorded, each time
- FIXTURE self-check at tip: exit 0, all ten faults reported by the channel that owns them
- MY OWN SABOTAGE A (reachability channel, product side): built a scratch repo layout at /tmp/zzrev-sab with a copy of the fixture and a copy of catalog.rs carrying one extra name `zz-fake-verb`; ZZ_SUPERSET_ONLY=names -> 'DIFF names name-zz-fake-verb', '1 of 26 asserted cases failed', exit 1. The names group really does catch a declared name the server does not know
- MY OWN SABOTAGE B (canvas channel, product side): patched a copy of the fixture so run_terminal_around_the_sidebar never presses q, leaving the sidebar drawn when the canvas is compared; ZZ_SUPERSET_ONLY=around-sidebar -> 'first differing row 0 of 30', 'DIFF around-sidebar withdrawn', exit 1, with the surrounding ten cases still green. The withdrawal case cannot pass on a surface that stayed up
- ORACLE SPOT-CHECK: throwaway pinned server (-L zzprobe-rev$$ -f /dev/null, scrubbed HOME and XDG_CONFIG_HOME) driven directly; every one of the 25 NATIVE_COMMAND_NAMES answers exactly `unknown command: <name>` on the pin, so each clause-1 refusal is a declared case with no counterpart, exactly as the fixture frames it. Server killed, socket removed
- DELTA CORPUS: compat/run.sh --strict-geometry --delta origin/main...HEAD --commands <the lane's 43 touched_commands> --list selects 237 scenarios. I ran 48 of them at the tip in two chunks: the three rows that actually name a superset verb (smoke/config-discovery, -explicit, -launcher; 12/13/12 steps, all clean, exit 0) and 45 of the delta head (41 fully clean, plus the four known/ rows carrying their expected recorded divergences), exit 0
- CORPUS GREP: rg over compat/scenarios for every zz-only string, message and label the fixture asserts, and for tmux-gaps consumers - the only hits are the four config rows named in the defect above; no scenario asserts any string this landing touches, and the landing removes or renames nothing
- cargo test -p zz-mux --jobs 4 -- --test-threads=3 through the wrapper -> exit 0 (this is the crate whose compat_manifest_tests read compat/tmux-gaps.json)
- cargo test -p zz --jobs 4 -- --test-threads=3 through the wrapper -> every suite ok except three cli_binary control-mode tests that FAILED while I had two tui-superset runs and another lane's corpus in flight (refresh_client_b_reports_initial_change_and_exact_removal, refresh_client_c_sizes_a_control_target_for_menu_gating, control_return_and_explicit_detach_follow_the_full_retval_matrix). All three pass exact-solo at the tip: the first two in 5.54s, the third in 52.97s. Load flake, not the lane's - the worker's own evidence has cli_binary 125 passed in 89s
- compat/attached-client.sh (TMUX_BIN=pin, ZZ_BIN=my tip build) -> 'attached-client compatibility: PASS', exit 0
- compat/tui-screen-diff.sh at the tip -> exit 0, 'all 147 asserted checkpoints identical, 6 recorded'. All six records are the same cursor-style-request DECSCUSR note at six sizes; none is a sidebar case and none sits inside a TUI-012 clause
- compat/tui-pane-geometry.sh -> exit 0, all 6 asserted measurements identical
- compat/status-row.sh under LC_ALL=C LC_TIME=C -> exit 0, all 14 comparisons identical, none recorded
- FIXTURE SHAPE: read compat/tui-superset.sh end to end. Outer pinned tmux driving both sides, isolated HOME and XDG_CONFIG_HOME per side inside the scratch tree, short /tmp sockets, capture-pane -p -e for every screen comparison, wait_settled = marker on screen AND unchanged between two polls, every wait a bounded wait_for/wait_text/wait_settled (the only sleeps are the 0.05s poll intervals inside those loops), and a cleanup trap that kills the outer server, the zz daemon, the inner pin server and removes both sockets and the scratch tree
- REAPING: after each of my runs, no /tmp/zzsup.*, no zzsuo-/zzsui- entries in /tmp/tmux-1000, no /tmp/zz-cli-* and nothing from pgrep -fa 'zz-cli-|zz-user|zzprobe'. I killed the one stuck cli_binary run I started and reaped its three /tmp/zz-cli-95fbE8 daemons by pid. No binary was copied under /tmp. The zz processes still on the box belong to zz-gate-tui7/zz-gate-target and were left alone

## The reviewer's notes, verbatim

PUNCH LIST, item by item, at d8de8190.

1. CLAUSE 1 - DONE. compat/tui-superset.sh walks NATIVE_COMMAND_NAMES out of crates/zz-protocol/src/catalog.rs itself (25 names, confirmed against the array) and requires each to resolve; every verb is then driven from the CLI and through an ordinary `bind-key -n` binding. I cross-checked the binding pass name by name against the catalog array: all 25 are covered, none by inheritance from the previous case (one key rebound per case). No compatibility profile, no activation flag, no width. Sabotage A (mine) proves the reachability channel can fail.

2. CLAUSE 2 - DONE. new-window, select-window, split-window, select-pane, resize-pane, detach-client and reattach run on both sides against explicit targets around the sidebar, the picker, a browser pane, an Agent pane and the command-output overlay, with the pin's pane and window inventory compared while each surface is up and the whole decoded screen plus the whole cursor tuple compared once it is gone. Input ownership is asserted in both directions for the sidebar and the overlay. Sabotage B (mine) proves the withdrawal canvas cannot pass on a surface that stayed up.

3. CLAUSE 3 - DONE. Two clients per side; the second client's whole screen is the pin's before, during and after, while only the first draws the tree; a browser pane draws its card on both clients; capture-browser answers `browser screenshots require the zz app`. I checked crates/zz-tui/src/browser.rs for a distinct 'no Kitty support' or 'no provider' string to assert and there is none - the fallback IS the card - so the clause is asserted as far as an observable exists.

4. GAPS - DONE and honest. Nothing closed; each of the three owned gaps carries a dated 2026-09-13 measurement saying why (the pin has no counterpart, so there is no pin behaviour for the raw TUI to start honouring). commands.native-superset also had a stale count corrected, 23 -> 25, which I verified against the array. Correct call: closing any of these three would have been wrong.

5. LEDGER - DONE, and inside the lane's write permissions.

WHAT I WOULD TELL THE GATE.

The strongest fact about this branch is that it changes no code. `git diff origin/main...HEAD -- crates/` is empty. Every zone, wire and GUI invariant is satisfied because there was nothing to violate, and every corpus row is provably the origin/main row. That is also why I did not run all 237 delta scenarios: with no crate source in the diff, the 237 are selected only because the fixture's touched_commands name ordinary tmux verbs. I ran 48 of them, chosen as the rows that actually mention a superset verb plus the head of the delta, and all are clean. If the gate wants the full sweep it costs roughly 100 minutes of box time at the ~25s/row this box was giving under two-lane load.

THE THREE cli_binary REDS ARE NOT THE LANE'S. I hit them because I had two tui-superset runs going beside the suite and another lane was running corpus rows; the suite also stalled past 30 minutes and I killed it and reaped its daemons. All three pass exact-solo at the tip. Worth flagging for the runner rather than the lane: cli_binary's control-mode tests do not survive a fixture running next to them, and the batch's 'cargo test -p zz before the last commit' is easy to run in exactly that state.

SUSPICIONS, NOT DEFECTS.
- run_names asserts negatively: a name passes unless the answer is exactly `unknown command: NAME`. If the daemon died mid-group all 25 would pass in silence. fresh_group would die first, so it is bounded in practice, and my sabotage confirms the channel reports a real miss - but it is the one group in the file whose pass does not require a positive observable.
- In the clients group, `equals window-columns-under-the-sidebar 62x29` compares against a window that was already 62 columns wide before the sidebar was focused, because the second client is 62 columns. That single case cannot tell 'the sidebar took 29 columns' from 'the sidebar took none'. It does not weaken clause 3, whose content is the cell-for-cell comparison beside it, and the sidebar's real column arithmetic (120 -> 91 -> 120) is asserted properly in the sidebar group.
- bound_reaches_the_verb, used only for debug-marker, puts the reporting command FIRST, so its token proves the binding fired and not that debug-marker ran. The fixture header says exactly that and debug-marker's direct path is asserted, so it is declared rather than hidden.
- An editor pane is never drawn in a raw TUI because the product gates it behind experimental-editor-pane, so clause 3's 'retain existing Agent/editor cards' is asserted for the Agent card only. The worker's reading - a product setting is not a parity profile, since the other three pane kinds need no flag - is defensible and is recorded in the evidence_note, but the gate should sign off on it consciously rather than inherit it.
- The finding the worker recorded and did not chase (a client wider than its window draws a blank band where the pin draws a border and middle dots) is real and is outside this obligation. It should reach the client-inventory obligation as a case, not stay only in TUI-012's evidence_note where it will be lost when that note is next rewritten.

GATE ORDER. TUI-012 depends on TUI-008 and TUI-009, which land ahead of this lane from the input lane. sibling_cases is empty and the fixture records zero cases, so nothing here waits on a sibling to flip - but the dependency itself still has to be satisfied before TUI-012 can be set verified, which the record's next_action says correctly.

The three fixes above are all evidence-hygiene and cost a single commit. None of them touches the fixture, the ledger's clause content, or a measurement.
