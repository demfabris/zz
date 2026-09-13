# TUI-011, cycle 7 roster lane: the review and what the gate did with it

## The reviewer's verdict, verbatim

```json
{
  "lane": "roster",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-011",
      "severity": "must-fix",
      "description": "compat/tui/evidence/TUI-011/attempt-01/before-the-landing.txt is not what notes.md says it is. notes.md calls it \"the same fixture run at BASE\", but (a) it was produced by an EARLIER revision of the fixture, not the committed one: it prints `DIFF  capture-escape` and `DIFF  stream-source-file-effect`, and both of those cases are `record` mode at the tip (compat/tui-client-commands.sh lines 715 and 743), so the committed fixture can never print DIFF for them; and (b) the run ABORTED - its last line is `error: the pin mode ended for switch-mode-closed did not happen within 10 seconds`, not a summary line, so it never reached the lock, client-tool or suspend cases. FAILURE: the gate reads it as a BASE run of the committed fixture, sees two DIFF lines the committed fixture cannot produce, and either concludes the fixture was edited to hide two reds or trusts a truncated run as a complete BASE measurement. The underlying claim is nonetheless true and I confirmed it independently: `git checkout origin/main -- crates/zz-daemon/src/daemon.rs`, rebuild, re-run the COMMITTED fixture at the tip gives exit 1 and `9 of 57 asserted comparisons differ`. But it is nine, not eight: alongside the eight refresh-client cases the worker names, `DIFF  lock-client-current: the CLI channels differ` also flips, and the \"eight cases\" count is repeated in TUI-011's evidence_note, so a later lane fixing lock-client will not know lock-client-current was one of the nine.",
      "suggested_fix": "Regenerate before-the-landing.txt with the committed fixture against a BASE build (it exits 1 with `9 of 57 asserted comparisons differ`), or rename it to say which fixture revision it is and that it aborted at switch-mode-closed. Correct \"eight cases moved from differing to identical\" to nine in TUI-011's evidence_note and in notes.md, naming lock-client-current."
    },
    {
      "obligation": "TUI-011",
      "severity": "must-fix",
      "description": "compat/tui/evidence/TUI-011/attempt-01/environment.txt attests the wrong revision. It records `revision 53d206dc5167ab009f41b6bbd1a15af6ecea6c18` (that is origin/main, the BASE) and `worktree 3 modified paths at the time of writing`, while every run it heads was taken at 67d9e3e7 or 15ef0463. On this box a cargo debug build of zz is not bit-reproducible, so the revision plus a clean worktree is the only thing that attests which binary produced the .txt files; a BASE sha and a dirty tree attest nothing. FAILURE: a later reader wanting to reproduce run-1.txt checks out 53d206dc as environment.txt tells them to, builds, runs the fixture, and gets nine DIFFs and exit 1 instead of the recorded `all 57 asserted comparisons identical`, because 53d206dc is the revision without the landing.",
      "suggested_fix": "Rewrite environment.txt with the tip sha (15ef04639a8d9bc486918efa63184344f36b233f) and a `git status --short` line showing a clean worktree, or add a second block recording the revision each run was taken at."
    },
    {
      "obligation": "TUI-011",
      "severity": "must-fix",
      "description": "compat/tui/evidence/TUI-011/attempt-01/cargo.txt is a filtered capture that reads as an empty test run. Under the heading `cargo test -p zz-daemon --jobs 4 -- --test-threads=3` it records exactly one line, `test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s`, so the file as committed shows zz-daemon's suite executing nothing. FAILURE: the gate opens cargo.txt to check the touched crate's tests and cannot tell a green 893-test run from a run whose test binary never executed. The real result is fine - I ran it at the tip and got `ok. 893 passed; 0 failed` for the lib target plus ten more binaries, exit 0.",
      "suggested_fix": "Recapture cargo.txt keeping every `test result:` line per target (or the full output), so the 893-test lib result is visible; the batch also forbids piping cargo test through tail or grep, which is how the single line got there."
    },
    {
      "obligation": "TUI-011",
      "severity": "nit",
      "description": "One roster entry is in the header table with no case behind it in the same file. The header row `show-hooks [-Bgpw] [-t]   the hook table   same   PROVED` claims -t, but compat/tui-client-commands.sh's message_hook_cases has hooks-show, -g, -w, -p, -B, a named hook and a missing hook, and no -t case. I drove both binaries myself against the pin at d77c9dc6: `show-hooks -t p`, `show-hooks -w -t p` and `show-hooks -p -t p` all exit 0 with empty stdout and stderr on both sides, so the PROVED marking is TRUE - it is simply not backed by a case, which is what clause 2 asks of every entry. FAILURE: a later lane changes show-hooks target resolution, the roster still says show-hooks [-t] is PROVED, and no case in the file notices - the divergence reaches the gate the way the chooser chrome did in cycle 6.",
      "suggested_fix": "Add `case_run hooks-show-target same '' -- show-hooks -t PANE` to message_hook_cases; it passes today (verified) and takes the asserted count to 58."
    },
    {
      "obligation": "TUI-011",
      "severity": "nit",
      "description": "The landing adds a three-line `///` doc comment above the new `current_client` helper in crates/zz-daemon/src/daemon.rs, and the batch's delivery rule is \"NO comments in code (a fixture's header block is documentation)\". FAILURE: a gate applying that rule literally rejects an otherwise clean 14-line landing over three doc-comment lines. Mitigating: daemon.rs already carries 698 `///` doc comments, so the addition matches the file's own convention rather than breaking it, and the comment is the citation `cmd_find_current_client, cmd-find.c:1274` that makes the rule checkable against the pin's source.",
      "suggested_fix": "Leave it, or move the cmd-find.c:1274 citation into the commit message and TUI-011's evidence_note (both already carry it) and drop the three lines."
    },
    {
      "obligation": "TUI-011",
      "severity": "nit",
      "description": "The headline \"57 asserted comparisons\" mixes two different things. Counting case_run calls in compat/tui-client-commands.sh: 41 `same` cases and 4 `cli` cases are command comparisons, and the other 12-13 CHECKS come from restore_case / restore_pin_pane checkpoints that assert the pin gave the pane back and zz's screen still matches. Those checkpoints are genuine assertions with real content - I read `P cli:0.0 mode=1/client-mode` in the state diff of client-tree-open, so the pin really is in a mode beforehand - but FAILURE: a reader of the summary line counts 57 commands compared and credits clause 2 with better coverage than the 41 same-mode plus 4 cli-mode invocations that are the actual command surface.",
      "suggested_fix": "Have run_cases print the split, e.g. `41 command comparisons and 12 restore checkpoints identical, 37 recorded`, and use the same wording in evidence_note."
    },
    {
      "obligation": "TUI-011",
      "severity": "nit",
      "description": "status is set to `review` while clause 2 holds 33 recorded cases, none of them SIBLING. The general ledger rule in the batch is \"review when every clause asserts on the branch, SIBLING cases aside\"; the batch's own punch-list item 5 gives this obligation a narrower definition (roster complete + every entry proved, declared or handed to a child + depends_on names the children) that the worker does satisfy, and next_action says in as many words that the parent cannot verify before TUI-014 to TUI-018 do. FAILURE: a gate scanning for status==review picks TUI-011 up as verifiable, sets verified, and the campaign records a parent as proved while 33 of its roster entries have never asserted.",
      "suggested_fix": "No change needed if the gate reads next_action; otherwise set status back to active, or add one clause-level sentence at the head of evidence_note saying clause 2 is open until the five children verify."
    }
  ],
  "checks_run": [
    "fetched origin, checked out 15ef04639a8d9bc486918efa63184344f36b233f --detach in /home/demfabris/dev/zz-tui-roster-review; merge-base with origin/main is exactly 53d206dc, so three-dot and two-dot diffs coincide",
    "touched crates/**/*.rs and Cargo.* then built: cargo build -p zz through the two-slot lock at MemoryMax=5G, exit 0 (the GUI crate compiles at the tip)",
    "compat/tui-client-commands.sh at the tip, run 1: exit 0, 'all 57 asserted comparisons identical, 37 recorded not asserted (0 for a sibling lane)', 2m04s",
    "compat/tui-client-commands.sh at the tip, run 2 (after rebuilding the tip binary): exit 0, same summary line",
    "compat/tui-client-commands.sh --self-check: exit 0, all four one-sided sabotages caught in their own channel plus both equivalences silent",
    "MY OWN SABOTAGE: reverted crates/zz-daemon/src/daemon.rs to origin/main, rebuilt, re-ran the committed fixture -> exit 1, '9 of 57 asserted comparisons differ': refresh-bare, refresh-status, refresh-flag-set, refresh-flag-clear, refresh-flag-restore, refresh-control-pane, refresh-control-subscribe, refresh-control-size and lock-client-current. The fixture can fail, and it fails on exactly the behaviour the landing changes",
    "ORACLE PROBE, driven by hand against pinned tmux d77c9dc6 with throwaway -L zzprobe sockets and scrubbed HOME/XDG_CONFIG_HOME: with NO client attached, pin and zz-at-tip both exit 1 with 'no current client' for refresh-client, refresh-client -S and lock-client, and both exit 0 for lock-server; with one client attached, both exit 0 for refresh-client, refresh-client -S, lock-client, lock-server and lock-session. The landing matches cmd_find_current_client and does not over-widen the no-client case",
    "ORACLE PROBE for the unbacked roster flag: show-hooks -t p, show-hooks -w -t p, show-hooks -p -t p all exit 0 with empty stdout and stderr on both binaries",
    "DELTA CORPUS at the tip (--strict-geometry): smoke/args-parse-display-panes, smoke/args-parse-display-menu, smoke/command-prompt-target, smoke/positional-maximums, smoke/daemon-invalid-flags, smoke/control-notify - all 0 TOPO/GEO/FMT/OUT/WARN",
    "DELTA CORPUS at the tip (--strict-geometry): smoke/refresh-status, smoke/client-resized-context, smoke/display-message-client-aliases, capture-pane (23 steps), smoke/buffer-standard-streams, smoke/command-flag-errors, census-hooks (152 steps), smoke/control-eof-drain - all 0 divergences",
    "smoke/status-background-jobs is red at the tip (1 OUT, 1 WARN, twice including the solo retry) - and equally red at origin/main on this box: I built BASE from the same worktree and re-ran it alone with the same result. Pre-existing/environmental, not this lane's, and worth adding to the box's four known environmental rows",
    "corpus selection audited: compat/run.sh --delta origin/main...HEAD --commands refresh-client,lock-client --list selects 145 of 250 scenarios (daemon.rs pulls in nearly everything); I ran the subset that can actually see the change - every scenario mentioning refresh-client, lock-client, lock-server, lock-session or 'no current client', plus the capture and buffer rows",
    "cargo test -p zz-daemon --jobs 4 -- --test-threads=3: exit 0 (lib 893 passed, plus ten integration binaries; no client_focus_closes_display_panes_and_preserves_chooser_modes flake)",
    "cargo clippy -p zz-daemon --all-targets --all-features --jobs 4 -- -D warnings: exit 0",
    "cargo test -p zz --jobs 4 -- --test-threads=3: exit 0 (638 lib, cli_binary 125)",
    "compat/attached-client.sh with TMUX_BIN=the pin and ZZ_BIN=my tip build: exit 0, 'attached-client compatibility: PASS', and byte-identical to the committed compat/tui/evidence/TUI-011/attempt-01/attached-client.txt",
    "python3 compat/tui/tracker.py check -> valid and report current; write-report leaves the tree clean; python3 -B compat/tui/tracker_test.py -> 8 tests OK; python3 compat/tmux-tracker.py check -> valid, write-report leaves the tree clean",
    "WIRE: PROTOCOL_VERSION untouched, crates/zz-protocol not in the diff at all, no message/payload/variant/field added, so no v102 history line was owed",
    "ZONES on git diff origin/main...HEAD: the only crate file touched is crates/zz-daemon/src/daemon.rs; crates/zz-tui/src is untouched; compat/tmux-gaps.json is untouched and no gap item was closed; TUI-006's record and compat/tui-choosers.sh are untouched; the rest is the new fixture, TUI-011's own record, its evidence, and the generated knowledge/tmux/tui-parity.md",
    "verified the stated choose-client blocker: paint_chooser is at crates/zz-tui/src/render.rs:1719 and ChooseTreeKind at crates/zz-protocol/src/message.rs:2367, so an appended kind does need an arm in a file the batch's zones exclude",
    "verified two capture records really are out of zone: 'alternate screen is not active' and 'pane is not in a native mode' are both in crates/zz-terminal/src/session.rs:1064-1066, not daemon.rs",
    "read the daemon diff hunk by hunk: resolve_client_target's three branches are preserved exactly in the extracted current_client helper (behaviour-preserving), and resolve_attached_client changes only the not-attached branch from an error to the same fallback",
    "roster completeness against catalog.rs: UNIMPLEMENTED_TMUX_COMMANDS holds new-pane, server-access, customize-mode, choose-client, clock-mode, suspend-client, link-window, unlink-window, switch-mode; the roster covers all six client-side ones and leaves the three window commands, which are out of a client-command roster's scope",
    "child obligations TUI-014..TUI-018: each has title, milestone breadth, priority, depends_on, tmux_gaps, sources, acceptance clauses, a dated evidence_note and next_action; none is in campaign.json's frozen twelve-item baseline (unchanged in the diff); TUI-011's depends_on names all five; no child depends on TUI-011, so no cycle; tracker check agrees",
    "no attribution trailers in any of the three commits; one added /// doc comment in crates/; git check-ignore -v over the evidence directory and the fixture is clean (exit 1, nothing ignored); no .log files committed",
    "counted the fixture's cases in source: 41 same, 4 cli, 33 record, 12-13 restore checkpoints, matching the run's CHECKS=57 / RECORDS=37",
    "HYGIENE: no zz or tmux server of mine left running (pgrep -fa 'zz-cli-|zz-user|zzprobe|zzcc-' finds nothing but my own shell); the fixture reaps its own scratch dir and sockets (no /tmp/zzcc.* or /tmp/zzcc-*.sock survive a run); I removed the three /tmp/tmux-1000 socket files my hand probes left; I copied no binary under /tmp; /tmp is at 12%"
  ],
  "every_clause_asserted": "no",
  "notes": "NOTHING IS RED AT THE TIP THAT IS GREEN AT BASE, the fixture can fail (I proved it myself by reverting the landing), and there is no zone or wire violation. Hence approve-with-fixes rather than reject, even though the batch's headline item did not land.\n\nCLAUSE AUDIT. Clause 1 asserts: the header table of compat/tui-client-commands.sh is the finite command-and-flag roster, derived from the oracle list and the five gaps, with the pin behaviour, zz's disposition and the disposition class per entry. Clause 3 asserts: five stable ids TUI-014..TUI-018, every required field, none in the frozen baseline, parent depends on all five. Clause 2 does NOT assert: 33 recorded cases, none SIBLING, so every_clause_asserted is no and the gate must not verify TUI-011.\n\nPUNCH LIST, ITEM BY ITEM.\n1. ROSTER - DONE, with one flag marked PROVED and not backed by a case (nit 4).\n2. CHOOSE-CLIENT - NOT DONE, and honestly recorded. The case stays recorded in the new fixture with a real pin measurement (I read it in my own run: row 0 in the message style, the preview box header on row 12, and `P cli:0.0 mode=1/client-mode` in the state diff, exactly what evidence_note quotes), compat/tui-choosers.sh and TUI-006's record are untouched, and TUI-014 carries it with the measurement and a design. I checked the stated blocker rather than taking it: paint_chooser really is in crates/zz-tui/src/render.rs, which this batch's zones exclude. So it is not a blocker under the punch-list rule - but the practical consequence stands: TUI-006 cannot verify at this gate, which is what item 2 existed to fix.\n3. COMPARISONS - the fixture half is done and good; the \"fix inside your zones\" half is partly unmet. Landed: refresh-client bare/-S/the flag paths (the one landing, 14 insertions). Already at parity and now asserted: show-buffer, show-hooks, load-buffer -, save-buffer - and -a -. Handed to children instead of fixed: source-file - stdin (TUI-018), show-messages (TUI-016), the lock family's observable result (TUI-015). Of those three, source-file - is the weakest hand-off: the batch names it explicitly among the in-zone fixes and crates/zz/src/lib.rs (the CLI's stdin paths) is in the zones, though the refusal itself sits in daemon.rs outside the named sub-zones. The capture hand-offs, by contrast, I checked and they are honest - -a's wording and -M's error both live in crates/zz-terminal/src/session.rs, genuinely outside this lane.\n4. CHILDREN - DONE. Five ids, one per accepted gap, every required field present, none in the frozen baseline, parent depends on all five, no cycle, tracker green.\n5. LEDGER - done modulo nit 7 (review vs the general clause rule).\n\nTHE ONE LANDING IS RIGHT. I did not just re-run the worker's proof; I drove the pin by hand. With no client attached, pin and zz both refuse with `no current client`; with one attached, both succeed. That is the part a careless widening would have broken, and it holds. The extracted helper preserves resolve_client_target's three branches in order, and 893 zz-daemon tests plus cli_binary's 125 pass over it.\n\nWHAT THE GATE SHOULD KNOW ABOUT `record` MODE. A record case asserts nothing at all - not even that the divergence is still the one described. That is deliberate and it is what keeps clause 2 open, but it means the 33 recorded measurements are only as fresh as the last run's printed diff. The five children each inherit a slice of them; when a child closes, its cases should flip to `same`, not stay recorded against a closed gap.\n\nSUSPICION, NOT A DEFECT. compat/scenarios has no row for the new comparisons - the worker's reasoning (a corpus row cannot read an attached client's screen or the session state, so the fixture is the right home) is sound, and I confirmed the landing removes no zz-only string, prompt, label, option name or wire field. I also ran the display-panes, display-menu, command-prompt-target and positional-maximums rows the worker did not, precisely because they are the four scenarios that assert `no current client`, and all four are clean at the tip.\n\nONE PROCESS NOTE FOR THE RUNNER, not against this lane. smoke/status-background-jobs fails identically at the tip and at origin/main on this box (the drawn-DATE interval assertion, `DATE[]` empty). The batch's box note lists four environmental corpus rows; this is a fifth, and establishing it cost a BASE build and two extra runs. Worth adding to the note before the next cycle.\n\nTIME. Roughly 55 minutes after the build. Two full fixture runs, one sabotage run, two hand-driven pin probes, fourteen corpus rows at the tip plus one at BASE, three cargo commands, all four tracker commands, and attached-client.sh."
}```

## What the gate did

Verdict was approve-with-fixes with three must-fixes, all three applied.

### Must-fix 1 and 2 and 3, commit 4e1ef24f "Retake TUI-011's evidence at the gate tip"

before-the-landing.txt was the wrong file under the right name: an earlier
revision of the fixture (it printed DIFF for two cases that are `record` at the
tip) and an aborted run (its last line was a mode-timeout error, not a summary),
so it never reached the lock, client-tool or suspend cases.

PROBE, RE-RUN AS PROOF. I rebuilt the reviewer's measurement rather than taking
it. At the gate tip, in the gate worktree, `git checkout origin/main --
crates/zz-daemon/src/daemon.rs`, `cargo build -p zz` through the slot-and-cap
wrapper, then the committed fixture: exit 1,

    9 of 58 asserted comparisons differ, 37 recorded (0 for a sibling lane)
    DIFF  refresh-bare          DIFF  refresh-control-pane
    DIFF  refresh-status        DIFF  refresh-control-subscribe
    DIFF  refresh-flag-set      DIFF  refresh-control-size
    DIFF  refresh-flag-clear    DIFF  lock-client-current: the CLI channels differ
    DIFF  refresh-flag-restore

Nine, exactly the nine the reviewer named, and lock-client-current among them.
Restoring daemon.rs and rebuilding takes the same fixture back to `all 58
asserted comparisons identical`. NOT VACUOUS: the fixture fails on exactly the
behaviour the landing changes and on nothing else. That run is the new
before-the-landing.txt.

The count is corrected from eight to nine in TUI-011's evidence_note and in
notes.md, both naming lock-client-current, so a later lane on the lock family
knows its current-client resolution was in the set the landing moved.

environment.txt attested 53d206dc (BASE) with a dirty tree while heading runs
taken with the landing in. Rewritten with the revision the runs were taken at,
a clean worktree, and an explicit note of the single file that differs for
before-the-landing.

cargo.txt showed zz-daemon's suite as one `0 passed; 0 failed` line. Recaptured
keeping every target line and every result line: the lib target's 893 tests are
now readable, alongside ten integration binaries, zz's 638 plus cli_binary's
125, workspace clippy and the build.

### Nit 4, taken not left, commit 96f777aa

Of the four nits the reviewer left, one is a hole in the deliverable rather than
a matter of taste: the roster header marks `show-hooks [-Bgpw] [-t]` PROVED and
message_hook_cases had no -t case, so half that row rested on prose in a file
whose whole claim is that every row is backed by a case beside it. That is the
shape of the cycle 6 chooser-chrome miss. Added
`case_run hooks-show-target same '' -- show-hooks -t PANE`. It is green at the
tip and green at BASE - show-hooks cannot see the current-client landing - so it
backs an existing true claim rather than making a new one. The fixture now
asserts 58 and records 37, and every count in the evidence is restated at 58.

### Nits 5, 6 and 7

Nit 5, the three `///` lines on current_client: LEFT. daemon.rs carries 698 of
them and the added one is the cmd-find.c:1274 citation that makes the rule
checkable against the pin. Removing it would delete the only pointer to the
source the behaviour is copied from.

Nit 6, the headline mixing command comparisons with restore checkpoints:
ADDRESSED IN THE LEDGER, not in the fixture. TUI-011's evidence_note now splits
the 58 into 42 `same` plus 4 `cli` command comparisons and 12 restore
checkpoints, counted from source, and says a reader counting invocations should
count the 46. Changing run_cases' summary line is a fixture change that would
invalidate the three runs it prints in; the split belongs where a reader of the
ledger will meet it.

Nit 7, status `review` reading as verifiable: ADDRESSED. evidence_note now opens
with one clause-level sentence saying clause 2 is open until the five children
verify and that no gate may set this obligation verified on the strength of its
status. That is cheaper than moving the status back to active, which would lose
the true fact that the roster and the children are done.

## Sibling flips

None. This lane carries no sibling_cases, so nothing was flipped and nothing was
reverted.

## What the gate did NOT change

TUI-006 stays active. choose-client did not land, so compat/tui-choosers.sh
still records client-tree-open at this tip, alongside output-search-prompt,
output-search-typed and output-selected. TUI-006's dependencies (TUI-002,
TUI-003, TUI-004) are all verified on main and were never the blocker; the four
recorded cases are. Its next_action said cycle 7's roster lane would implement
choose-client, which did not happen, and has been corrected to point at TUI-014.

TUI-011 stays at review, unverified. Clause 2 holds 37 recorded cases, none
SIBLING, and an accepted gap is never evidence.
