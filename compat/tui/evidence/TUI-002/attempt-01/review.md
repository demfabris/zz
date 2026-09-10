# Cycle-1 review and gate record

Lane `fixtures`, branch `campaign/tui-fixture-baseline`, reviewed tip
`5dd932abb93c2e88f3190be42afeeed00ece6368`. The lane was rejected once on a TUI-001 blocker and
fixed on the same branch by a fresh agent; the verdict below is the re-review of the fixed tip.
The first review's verdict was `reject` on one blocker (TUI-001 clause 2 claimed as explained on a
story the campaign's own registry refutes) plus three nits, two of which it marked "leave it".

## Reviewer verdict, verbatim

```json
{
  "lane": "fixtures",
  "verdict": "approve",
  "confirmed_defects": [
    {
      "obligation": "TUI-001",
      "severity": "nit",
      "description": "environment.txt's recorded zz sha256 is not the binary any tip-level proof ran on, and the gate should not carry that hash into proof. PROOF, three distinct hashes for the same source (git diff --name-only origin/main...HEAD -- crates/ Cargo.toml Cargo.lock returns 0 files, so origin/main and the tip compile identical sources): environment.txt records 3cb6e28437cd028b49c2c2eb07c4414a69226518bf2f287e7c52123d5bf70a38; the lane's target/debug/zz that the worker's proofs ran on is 4b73fa13...; my own build is da070969eea7a3a81566ee3d188370a150c18f9505c27c3e4ee13c30d8203fde. New measurement this pass that sharpens the first review's version of this nit: da070969 is EXACTLY the hash the first reviewer got building from this same worktree path, so a debug build is deterministic per worktree path and the variation tracks the path baked into debuginfo -- which leaves the recorded 3cb6e284 vs the lane's 4b73fa13 at the SAME path (/home/demfabris/dev/zz-tui-lane) unexplained by path alone, most likely incremental-codegen partitioning. Either way the hash identifies an artifact nobody can rebuild on demand. The branch now discloses this in both notes.md and TUI-001's evidence_note, so nothing on the branch needs changing.",
      "suggested_fix": "No branch change. When the gate writes proof at the merged tip, record the sha256 of the zz IT builds, not environment.txt's 3cb6e284. Provenance for this attempt is carried by reproduction rather than by a hash, and it is strong: my independently built zz reproduced all 195 committed capture files and the 29242-byte screen-diff-run-1.stdout.txt byte for byte, and reproduced geometry-run-4.stdout.txt exactly."
    }
  ],
  "checks_run": [
    "GIT_TERMINAL_PROMPT=0 git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' -> origin/main 576b6b74, tip 5dd932ab",
    "git -C /home/demfabris/dev/zz-tui-review checkout --detach 5dd932abb93c2e88f3190be42afeeed00ece6368 (worktree was clean at the reviewed tip 8981ce37)",
    "git merge-base --is-ancestor 8981ce37 5dd932ab -> exit 0 (fast-forward over the reviewed tip, no history rewrite); git merge-base --is-ancestor origin/main 5dd932ab -> exit 0",
    "BLOCKER RE-CHECK: python3 read of tui.client-input-backpressure in compat/tmux-gaps.json, resolution text compared word for word against the block quote now in compat/tui/evidence/TUI-001/attempt-01/notes.md and in campaign.json TUI-001 evidence_note -> verbatim match",
    "BLOCKER RE-CHECK: git log -1 --format='%h %cI' 314c55e0 012b4dcc 0bed7fe7 -> 2026-09-07T00:15:42, 2026-09-07T10:45:51, 2026-09-07T13:31:08; git show --stat 0bed7fe7 | grep -c tui-pane-geometry -> 0 (the fix did not touch the fixture). The empirical refutation the branch now records is correct.",
    "BLOCKER RE-CHECK: python3 structural diff of compat/tui/campaign.json vs origin/main -> TUI-001 changed [status, evidence_note, next_action] with status review->active and evidence_note headed 'CLAUSE 2 IS NOT SATISFIED BY THIS ATTEMPT'; TUI-002 changed [status, sources, evidence_note, next_action]; schema_version/title/updated_on/tmux_commit/baseline/milestones byte-identical; all ten other obligations byte-identical; both proof blocks null; no verified field; no acceptance list changed",
    "git diff origin/main...HEAD -- compat/tmux-gaps.json | wc -l -> 0 (no close, relocation, reopen or appended reason)",
    "python3 json round-trip of campaign.json -> json.dumps(indent=2)+newline is byte-identical to the committed file",
    "cargo build -p zz --jobs 8 in /home/demfabris/dev/zz-tui-review with CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-lane/target, after find crates -name '*.rs' -exec touch {} + and touch Cargo.toml Cargo.lock -> exit 0 in 37s, sha256 da070969eea7a3a81566ee3d188370a150c18f9505c27c3e4ee13c30d8203fde",
    "PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/tui-pane-geometry.sh <zz> <pin> x3 -> exit 0, 0, 0 at 4s/4s/5s, each 'all 5 asserted measurements identical', 120x24 recorded tmux 120 / zz 91",
    "same fixture at origin/main (git show origin/main:compat/tui-pane-geometry.sh, run from compat/) with the same zz -> exit 0, identical report: the branch's fixture change altered no behaviour and the baseline behaves the same",
    "diff of my geometry run 1 stdout against the committed geometry-run-4.stdout.txt -> identical",
    "compat/status-row.sh <zz> <pin> -> exit 1, '2 of 11 comparisons differ', both differences only %b ('09-set-26' vs '09-Sep-26'); LC_ALL=C LC_TIME=C same command -> exit 0, 'all 11 comparisons identical, 3 rows recorded not asserted'. git diff --stat origin/main...HEAD -- compat/status-row.sh is empty, so the red is origin/main's, not this lane's",
    "ZZ_SCREEN_CAPTURE_DIR=<scratch>/cap1 compat/tui-screen-diff.sh <zz> <pin> -> exit 0 in 57s; second run to cap2 -> exit 0 in 58s; both 'all 33 asserted checkpoints identical, 32 recorded not asserted, 65 recorded cursor differences'",
    "diff -r cap1 cap2 -> exit 0, 195 files; diff -r cap1 compat/tui/evidence/TUI-002/attempt-01/captures -> exit 0, all 195 committed capture files reproduced byte for byte by my independently built zz; diff of my stdout against screen-diff-run-1.stdout.txt -> exit 0, 29242 bytes identical",
    "compat/tui-screen-diff.sh --self-check <zz> <pin> -> exit 0 in 24s, four sabotages each caught in their own channel with row index and both sides' bytes, three equivalences reported nowhere; stdout byte-identical to the committed self-check.stdout.txt",
    "compat/tui-screen-diff.sh --bogus -> 2; /nonexistent/zz -> 2; three positional args -> 2",
    "MY OWN SABOTAGE 1 (throwaway copy compat/.rev-sabotage.sh, deleted after): pure attribute difference, plant zz '\\033[4mUNDER\\033[0m' vs plant tmux '\\033[24mUNDER\\033[0m' -- identical glyphs, underline on one side only -> caught, 'first differing row 1 of 24', tmux 'UNDER' vs zz '^[[4mUNDER^[[0m'",
    "MY OWN SABOTAGE 2: wide glyph, plant zz 'AB|' vs plant tmux '日|' -> caught, 'first differing row 1 of 24' with both sides' bytes and the shifted trailing bar",
    "MY OWN HANG TEST (throwaway copy compat/.rev-hang.sh, deleted after): 'kill -STOP $$' sent to the zz side's inner shell only, then a checkpoint -> exit 2 in 16s, 'error: hangtest settled on the zz screen did not settle within 10 seconds', 13 diagnostic files retained including outer-zz.screen.txt showing the stopped shell with the marker command typed but never run. Bounded, not hanging.",
    "ORACLE (a) decoded-screen rule, throwaway pin server -L zzprobe-$$ -f /dev/null with scrubbed HOME/XDG_CONFIG_HOME: '\\e[1m\\e[31mX' and '\\e[31m\\e[1mY' both re-emit as ^[[1m^[[31m<c>^[[0m (attribute order collapses); '\\e[01mB\\e[m' and '\\e[1mB\\e[0m' both re-emit as ^[[1mB^[[0m; '\\e[31mA' -> ^[[31mA and '\\e[38;5;1mB' -> ^[[38;5;1mB (colour class does NOT collapse). The fixture header states exactly this.",
    "ORACLE (c) colour.c colour_fromstring read on the pin: 'red' returns 1, 'colour1' returns n|COLOUR_FLAG_256 -- two different values, as the header says. Verified experimentally through the outer decoder with two inner pin servers, one bg=red one bg=colour1, attached in two outer windows: both status rows capture as ^[[41mL^[[4m0:tmux*^[[0m^[[41m, cmp -> identical. The equivalence the self-check relies on is real.",
    "ORACLE (b) the 2026-09-09 macOS timeout: the branch now claims NO explanation, which is the correct position per the registry; verified the withdrawal is complete in both notes.md and evidence_note and that status is active with next_action naming the macOS run",
    "ORACLE (d) bounded waits: grep -n sleep compat/tui-screen-diff.sh -> only lines 262 and 452, both the 0.05s poll tick inside wait_for and wait_settled, each bounded at 200 attempts with dump_diagnostics + die on expiry. compat/tui-pane-geometry.sh -> only its one poll tick.",
    "80x10 really runs at 10 rows: all 24 of the 80x10 cursor files record 'pane=80x10' on BOTH sides via display-message -p on the outer panes, and every 80x10 screen capture is exactly 10 lines on both sides. Sizes present across all cursor files: 80x24, 100x24, 80x10, 100x10, 109x24, 120x24 -- both sides attach at every SIZE the header lists.",
    "109 and 120 recordings inspected: zz's side carries a 28-column sidebar plus a U+2502 border on every row ('zz at alienware', '* # 0 win', '$ screentitle', '+ new pane') and nothing else unexplained; run stdout shows 'ok 100x24-from-120x24 resized', the deliberate threshold crossing that proves hiding the sidebar leaves the screens identical cell for cell",
    "recorded divergences verified as real captures, not prose: 80x24.colour-classes shows pin ^[[31mNAMED / ^[[38;5;196mINDEXED against zz ^[[38;2;205;0;0m / ^[[38;2;255;0;0m; 80x24.default-fg shows pin ^[[44m against zz ^[[38;2;216;222;233m^[[44m",
    "cursor-shape channel: crates/zz-tui/src/render.rs:1770-1799 read directly -- writes \\x1b[{shape} q plus \\x1b]12;#rrggbb\\x07 on every cursor placement; pin format.c has format_cb_cursor_shape, _blinking, _very_visible, _colour, _flag, _x, so the channel is reportable and is compared and printed at all 65 checkpoints rather than faked or omitted",
    "compat/attached-client.sh <zz> <pin> -> exit 0 in 355s, 'attached-client compatibility: PASS'",
    "python3 compat/tui/tracker.py check -> exit 0; python3 compat/tmux-tracker.py check -> exit 0",
    "regenerated both reports (tracker.py write-report, tmux-tracker.py write-report) -> git diff on knowledge/tmux/tui-parity.md and knowledge/tmux/gaps.md empty, so both are generated not hand-edited; the report's status table reads TUI-001 active, TUI-002 review",
    "zone discipline: git diff --name-only origin/main...HEAD outside compat/tui/evidence/ is exactly compat/tui-pane-geometry.sh, compat/tui-screen-diff.sh, compat/tui/campaign.json, knowledge/tmux/tui-parity.md; 0 files under crates/; 0 under zz-protocol|zz-daemon|zz-mux|zz-client; read every hunk of the tui-pane-geometry.sh diff (timeout diagnostics, ZZ_LOG_DIR pinned into the scratch tree, XDG_STATE_HOME scrubbed, client stderr teed)",
    "git check-ignore -v over every path in the branch diff -> exit 1, nothing ignored; 0 committed paths ending in .log",
    "attribution: git log --format='%B' origin/main..HEAD grepped for co-authored/generated with/claude/codex -> no match; all five commits authored Fabricio Dematte",
    "comment convention: origin/main's compat/attached-client.sh already carries mid-body explanatory comments (lines 661-684), so the new fixture's inline comments match the campaign's existing fixture pattern rather than breaking the no-comments-in-code rule",
    "the box's real state dir /home/demfabris/.local/state/zz/logs/ still shows only pre-existing files (newest set 3 19:27), so the fixtures' XDG_STATE_HOME/ZZ_LOG_DIR scrub kept every run out of it",
    "machine sweep: pgrep -x tmux -> 1, pgrep -x zz -> 1 (none running); removed only the five stale /tmp/tmux-1000/zzprobe-* sockets I created after confirming nothing was listening in ss -xl; left zz-oracle-131198-2fc782c2 and zz-oracle-174815-06af2780 untouched; review worktree clean at 5dd932ab; shared checkout /home/demfabris/dev/zz still 576b6b74"
  ],
  "notes": "VERDICT: approve. Both items the first review raised as actionable are fixed at 5dd932ab, and re-running the whole method on the whole branch turned up no blocker and no must-fix.\n\nTHE BLOCKER IS ACTUALLY FIXED, verified by re-running its failing probes. The first review's blocker was that TUI-001 clause 2 claimed the recorded macOS geometry timeout was EXPLAINED on a stale-pre-fix-binary story that the campaign's own registry refutes. At the new tip the claim is withdrawn, not re-argued. compat/tui/evidence/TUI-001/attempt-01/notes.md now opens the section with \"This attempt does not explain it either, and clause 2 is not satisfied here\", block-quotes tui.client-input-backpressure's resolution, and spells out both refutations separately. campaign.json's TUI-001 evidence_note is headed \"CLAUSE 2 IS NOT SATISFIED BY THIS ATTEMPT\" and carries the same quote. Status went review -> active and next_action names the macOS run clause 2 needs and says the refuted hypothesis must not be re-argued from the same citation. I checked the quote word for word against compat/tmux-gaps.json (unchanged on this branch) and it is verbatim, and I re-ran the empirical half myself: git log -1 gives 314c55e0 = 2026-09-07T00:15:42, 012b4dcc = 10:45:51, 0bed7fe7 = 13:31:08, so 012b4dcc does sit inside the pre-fix window, and git show --stat 0bed7fe7 confirms the fix never touched the fixture. compat/tmux-gaps.json got zero changes, which is right: the registry was correct and the branch was wrong, so there was nothing to append.\n\nThe header nit is fixed and the new sentence is accurate, not just shorter. compat/tui-screen-diff.sh lines 112-115 now say the rule holds below 109 for every `same` size and that the 120x24 record crosses it deliberately. I verified the arithmetic against SIZES: the three `same` alternates are 100, 80 and 100, all below 109, and 120x24's alternate is 100, which crosses. The crossing is what produces \"ok 100x24-from-120x24 resized\" in the run output -- the screens become identical cell for cell once the sidebar hides, which is the branch's strongest finding and is now correctly described rather than contradicted.\n\nTHE EVIDENCE IS REAL, and this is the strongest thing I can say about the lane. My independently built zz (da070969, built in my own review worktree, a different binary from the one the worker used) reproduced all 195 committed capture files byte for byte, reproduced screen-diff-run-1.stdout.txt exactly at 29242 bytes, reproduced self-check.stdout.txt exactly, and reproduced geometry-run-4.stdout.txt exactly. Two independent screen-diff runs of my own also produced byte-identical captures to each other. Nothing in the evidence directory describes a run that did not happen.\n\nTHE FIXTURE CAN FAIL, proved three ways beyond the branch's own self-check. (1) A pure attribute-only sabotage of mine -- identical glyphs \"UNDER\", underline on one side only -- was caught with the checkpoint name, \"first differing row 1 of 24\", and both sides' decoded bytes. (2) A wide-glyph sabotage was caught the same way. (3) kill -STOP on one side's inner shell made it exit 2 in 16 seconds naming \"hangtest settled on the zz screen\" and retaining 13 diagnostic files, including an outer capture showing the stopped shell with the marker command typed but never executed. It times out with a location; it does not hang.\n\nACCIDENTAL BONUS VALIDATION of the timeout-diagnostics work TUI-001 added. My first geometry attempt ran a zz I had copied out of target/debug, which broke its rpath. All three runs failed in 10 seconds with a full diagnostics dump, and zz-daemon.stderr.txt named the cause exactly: \"error while loading shared libraries: libcef.so\". That is the diagnostics path doing on a real unplanned failure precisely what the branch claims it does, which is a better demonstration than the worker's own sabotage.\n\nTHE ORACLE CHECKS ALL HOLD. In a throwaway pin server I confirmed attribute order collapses (\\e[1m\\e[31mX and \\e[31m\\e[1mY both re-emit as ^[[1m^[[31m...) while colour class does not (^[[31mA stays distinct from ^[[38;5;1mB), which is exactly what the fixture header states. I read colour.c (red -> 1, colour1 -> 1|COLOUR_FLAG_256) and then verified experimentally that the two still reach the outer grid as the same ^[[41m cell, so the equivalence the self-check leans on is measured, not assumed. format.c does expose cursor_shape, blinking, very_visible and colour, so shape is a reportable channel rather than a declared hole. The only sleeps in the new fixture are the 0.05s poll ticks inside the two bounded wait loops, each with dump-and-die on expiry.\n\nTHE ONE JUDGEMENT THE GATE INHERITS, unchanged from the first review and I agree with it. TUI-002 clause 1 asks that cursor shape be COMPARED; the fixture compares and prints it on both sides at all 65 checkpoints but does not gate it, because crates/zz-tui/src/render.rs:1770-1799 writes DECSCUSR and OSC 12 on every cursor placement where the pin writes neither, so a gate would be permanently red on a standing divergence and a sabotage of that channel could not be told from it. That is the campaign's existing record-mode treatment (tui-pane-geometry.sh's 120-column columns, status-row.sh's record_step), the divergence is disclosed in the ledger with its source line, and the fixture self-closes the record when the two sides converge. Not a defect, but the gate should carry it forward: a cursor-shape regression would not turn this fixture red.\n\nFOUR DIVERGENCES WITH NO REGISTRY OWNER, all reproduced by me from the committed captures and none waived by omission: (a) cursor shape/blink/colour differ at 65 of 65 checkpoints; (b) the pane body promotes named and indexed colours to RGB (pin ^[[31mNAMED and ^[[38;5;196mINDEXED against zz ^[[38;2;205;0;0m and ^[[38;2;255;0;0m) where the status row no longer does; (c) with bg=colour4 and no foreground named the pin emits none and zz emits RGB 216,222,233; (d) compat/status-row.sh is red on this box for LC_TIME=pt_BR.UTF-8 only ('09-set-26' vs '09-Sep-26'), which I reproduced along with the LC_ALL=C control that turns it green. That fixture is untouched by the branch, so the red is origin/main's on this box, not this lane's. All four need registry owners in a later cycle; nobody could open a gap this cycle.\n\nLEDGER OWNERSHIP IS CLEAN. Only TUI-001 and TUI-002 changed. The only field beyond the permitted status/evidence_note/next_action is TUI-002's sources gaining compat/tui-screen-diff.sh, which the first review inspected and told the worker to leave; it is factual and on the lane's own obligation. Baseline, milestones, tmux_commit, schema_version, title and updated_on are byte-identical, all ten other obligations are byte-identical, both proof blocks are still null, no verified field exists anywhere, and no acceptance list moved.\n\nZONES AND HYGIENE. Zero files under crates/ changed, so the declared crates/zz-tui/src excursion was never taken and the integration-test rule never applied -- no cargo test was needed or run, which is correct here. Nothing under zz-protocol, zz-daemon, zz-mux or zz-client. Both trackers exit 0 and both generated reports regenerate to byte-identical files. No committed .log, nothing ignored, no attribution trailers. The fixture's inline comments match the pattern origin/main's own compat/attached-client.sh already uses, so they are not a violation of the no-comments rule. One quiet improvement worth naming: the geometry fixture now pins ZZ_LOG_DIR into its scratch tree and scrubs XDG_STATE_HOME, and I confirmed the box's real ~/.local/state/zz/logs/ was not written by any run today.\n\nMACHINE LEFT CLEAN. No tmux and no zz running at exit. I removed only the five stale /tmp/tmux-1000/zzprobe-* sockets I created, after confirming nothing was listening on them, and left the two pre-existing zz-oracle sockets alone. My two throwaway fixture copies are deleted and the review worktree is clean at 5dd932ab. /home/demfabris/dev/zz was only read and is still at 576b6b74. /home/demfabris/dev/zz-tui-review remains at 5dd932ab if the gate wants it; it can be removed.\n\nFOR THE GATE: build your own zz at the merged tip and record ITS hash in proof, not environment.txt's 3cb6e284 -- see the single nit. The proof commands to re-run are the three geometry runs, status-row (expect exit 1 for locale, with the LC_ALL=C control at exit 0), tui-screen-diff plus --self-check, attached-client, and both tracker checks. Record proof for TUI-001 clauses 1 and 3 and TUI-002's three clauses; leave TUI-001 clause 2 open, and leave TUI-001 at active."
}
```

## Gate actions

Gate: `alienware/orchestrator`, 2026-09-09, on alienware (CachyOS Linux, 16 cores). Fresh worktree
`/home/demfabris/dev/zz-gate-fixtures` at the rebased tip; `git rebase origin/main` was a no-op
because `origin/main` 576b6b74 is already an ancestor of 5dd932ab, so the merged tip is the
reviewed tip unchanged and no history was rewritten.

The branch changes no file under `crates/`, `Cargo.toml` or `Cargo.lock`
(`git diff --stat origin/main...HEAD -- crates/ Cargo.toml Cargo.lock` is empty), so the workspace
test and clippy stage did not apply and was not run. The four non-evidence paths in the whole
branch diff are `compat/tui-pane-geometry.sh`, `compat/tui-screen-diff.sh`,
`compat/tui/campaign.json` and `knowledge/tmux/tui-parity.md`.

### Every confirmed defect, and what the gate did about it

The re-review confirmed exactly one defect, a nit on TUI-001, and its suggested fix was addressed
to the gate rather than to the branch: *"When the gate writes proof at the merged tip, record the
sha256 of the zz IT builds, not environment.txt's 3cb6e284."* Done. The proof blocks recorded in
this commit carry `ddc5ae0f7be9e52b55dac82a8285c50f74a9684236986d277f54e1d8aa3b3254`, the zz this
gate built in `/home/demfabris/dev/zz-gate-fixtures` with
`CARGO_TARGET_DIR=/home/demfabris/dev/zz-gate-target`, and every proof command listed in them ran
on that binary. No branch change was required and none was made.

That is a fourth distinct hash for source that is identical to `origin/main`, which settles the
underlying point rather than arguing it: a cargo debug build of zz is not reproducible here, and a
hash cannot attest this artifact. Provenance rests on reproduction, and this gate is the third
independent reproduction of it. My binary reproduced all 195 committed capture files byte for byte
(`diff -r` exit 0, 195 files), reproduced `screen-diff-run-1.stdout.txt` exactly at 29242 bytes,
reproduced `self-check.stdout.txt` exactly, and produced geometry output identical to all six
committed `geometry-run-*.stdout.txt` files.

The first review's other three defects were all marked "leave it" or "no action on the branch" and
the fix pass left them, correctly. Two of them are now on the record where the next cycle sees
them: TUI-002's `sources` gained `compat/tui-screen-diff.sh` (a fourth ledger field beyond
status/evidence_note/next_action, factual and on the lane's own obligation), and the
hash-provenance qualifier is written into `notes.md` and TUI-001's `evidence_note`. The first
review's blocker was fixed on the branch before this gate ran and the re-review re-ran its failing
probes; I did not re-litigate it.

### Fixture results at the merged tip

All run with `PATH=/opt/homebrew/bin:$PATH`,
`ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux`,
`ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins`, zz
`/home/demfabris/dev/zz-gate-target/debug/zz` (sha256 `ddc5ae0f...`), pin
`/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux` (sha256 `df2cafcb...`, `tmux -V next-3.8`,
commit `d77c9dc6aa021e4bc61f0da128c591af695e6466`).

| Command | Exit | Last line |
| --- | --- | --- |
| `compat/tui-pane-geometry.sh` run 1 (4.8s) | 0 | all 5 asserted measurements identical |
| `compat/tui-pane-geometry.sh` run 2 (4.5s) | 0 | all 5 asserted measurements identical |
| `compat/tui-pane-geometry.sh` run 3 (4.7s) | 0 | all 5 asserted measurements identical |
| `compat/status-row.sh` (box locale) | 1 | 2 of 11 comparisons differ |
| `compat/status-row.sh` (`LC_ALL=C LC_TIME=C`) | 0 | all 11 comparisons identical, 3 rows recorded not asserted |
| `compat/tui-screen-diff.sh` (57.3s) | 0 | all 33 asserted checkpoints identical, 32 recorded not asserted, 65 recorded cursor differences |
| `compat/tui-screen-diff.sh --self-check` (24.1s) | 0 | self-check complete: every sabotage was caught in its own channel and every equivalence passed |
| `compat/attached-client.sh` (353.6s) | 0 | attached-client compatibility: PASS |
| `compat/run.sh --strict-geometry smoke/tui-client-input-backpressure` (363s) | 0 | Nothing failed on the first pass |
| `python3 -B compat/tui/tracker_test.py` | 0 | Ran 8 tests, OK |
| `python3 compat/tui/tracker.py check` | 0 | campaign.json is valid and tui-parity.md is current |
| `python3 compat/tmux-tracker.py check` | 0 | tmux-gaps.json is valid and gaps.md is current |
| `compat/check.sh` (150.8s) | 0 | both trackers and the zz-mux suite green |

The one non-zero exit is `compat/status-row.sh`, and it is not this branch's. Both differing
comparisons are `%b` alone (pin `09-set-26`, zz `09-Sep-26`) under the box's `LC_TIME=pt_BR.UTF-8`,
and the `LC_ALL=C LC_TIME=C` control run of the same fixture on the same binaries exits 0 with all
11 identical. `git diff --stat origin/main...HEAD -- compat/status-row.sh` is empty and the branch
changes no crate source, so the binary under test compiles from sources identical to `origin/main`:
the red is `origin/main`'s on this box, reproduced here, not introduced by this lane. It has no
registry owner and the fix would land in `crates/zz-mux/src/formats.rs`, outside this cycle's
zones.

Nothing any fixture reported contradicts either obligation's claimed status.

### Statuses this gate set, and why neither obligation is verified

**TUI-001 stays `active`.** Clause 2 ("reproduce or explain the recorded macOS geometry-report
timeout") is open, and the lane withdrew its explanation rather than defending it after the first
review showed the campaign's own registry refutes it. The re-review confirmed the withdrawal is
complete and correct. Clause 2 cannot be closed on this box: the recorded timeout happened on
macOS and this is Linux. The proof block recorded here therefore stands for clauses 1 and 3 only,
and `evidence_note` and `next_action` both say so.

What the gate proved at this tip for the two clauses that are closed: clause 1's environment is
recorded in `environment.txt`, with the hash qualifier above; clause 3's repeatable baseline is
three more consecutive exit-0 geometry runs at 4.5 to 4.8 seconds against the fixture's own
10-second bound, output identical to all six committed runs, plus the retained fixture limitation
(the `status-row.sh` locale red with its control) and the waived wide-terminal columns (120x24: pin
120 columns, zz 91, the sidebar's 28 plus its 1-column border).

**TUI-002 stays `review`.** All three of its clauses have evidence, the re-review approved them,
and this gate reproduced that evidence byte for byte at the merged tip, so the obligation is
finished as work. It cannot be marked `verified` because the ledger forbids it:
`compat/tui/tracker.py` line 109 requires every dependency of a verified obligation to be verified
itself, and TUI-002 depends on TUI-001, which stays `active` on its open clause 2. Setting
`verified` here would make `tracker.py check` fail. The proof block is recorded anyway, so the
cycle that closes TUI-001 clause 2 on macOS can flip this obligation to `verified` on the
measurements taken here without re-running them.

One disclosure the gate carries forward rather than treating as a defect, following both reviews:
cursor SHAPE, blink and colour are compared and printed on both sides at all 65 checkpoints but
are not gated, because `crates/zz-tui/src/render.rs:1770-1799` writes DECSCUSR and OSC 12 on every
cursor placement where the pin writes neither. A regression in those three channels would not turn
this fixture red. Cursor position and visibility are asserted and match everywhere. The fixture
prints "recorded cursor attributes identical, the record can close" the moment the two sides
converge, so it starts asserting itself without another edit.

### Residual, handed to the next cycle

- TUI-001 clause 2: a run of `compat/tui-pane-geometry.sh` on macOS with an attested binary,
  retaining this attempt's timeout dump if the geometry-report wait fires. The refuted
  stale-pre-fix-binary hypothesis must not be re-argued from the same citation.
- TUI-004's opening measurement, recorded here and asserted nowhere: at 120x24 the pin hands its
  pane 120 columns and zz hands its pane 91, and resizing a 120-column client down to 100 makes
  the two screens identical cell for cell, so the sidebar is the whole of the difference.
- Four divergences with no registry owner: cursor shape/blink/colour (TUI-002, render.rs:1770-1799);
  named and indexed colours promoted to RGB in the pane body (TUI-002); an explicit default
  foreground under a non-default status-style (TUI-002); and the `%b` locale divergence in
  `compat/status-row.sh` (TUI-001, `crates/zz-mux/src/formats.rs`).
