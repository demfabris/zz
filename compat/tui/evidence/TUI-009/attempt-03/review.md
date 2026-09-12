# TUI-009 review and cycle-6 caps gate

lane tip reviewed: 7d61628e6fd228a46f7e548d4c6c384fb999e2d4 on campaign/tui-caps-2
gate worktree: /home/demfabris/dev/zz-gate-tui6, local branch gate-caps
rebased onto: origin/main 33ecbd8694ac90c0d6c35c0e1fb700cde1f09f71 (cycle-6 copy gate)
gate tip before this records commit: see gate-01..gate-16 headers
pin: d77c9dc6aa021e4bc61f0da128c591af695e6466 (tmux next-3.8)
box: alienware, CachyOS, 16 cores, 15 GB + 15 GB zram

## Reviewer verdict, verbatim

```json
{
  "lane": "caps",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-009 clause 3 (\"Preserve named/indexed/RGB color behavior under palette changes...\")",
      "severity": "must-fix",
      "description": "The new downgrade in crates/zz-tui/src/render.rs skips indices 0..15, so on a terminal with fewer than 16 colours the raw TUI writes an aixterm colour the pin never writes. downgrade_palette early-returns `if index < 16 || colours >= 256`, but the pin's tty_check_fg (tty.c) runs colour_256to16 for EVERY COLOUR_FLAG_256 colour and then, when colours < 16, does `fg &= 7`. MEASURED BY ME at the tip (7d61628e), on the fixture's own silent-terminal case with the sample extended by two cells (\\e[38;5;12mA and \\e[38;5;9mQ, added to COLOUR_SAMPLE/COLOUR_SAMPLE_RGB and to the `for glyph in R I X B` loop in compat/tui-caps.sh): DIFF colours/silent cell A: tmux ^[[34mA, zz ^[[94mA; DIFF colours/silent cell Q: tmux ^[[31mQ, zz ^[[91mQ; DIFF colours/silent line. The other 276 rows agreed, and on the 256-colour outer tmux and under -2 both sides write ^[[94m/^[[91m, so the divergence is exactly the sub-16-colour path the landing added. This sits inside clause 3, which the worker reports as proved, and is neither asserted nor recorded anywhere.",
      "suggested_fix": "In crates/zz-tui/src/render.rs change downgrade_palette's guard to `if colours >= 256 || (index < 16 && colours >= 16) { return index; }`. I applied exactly that in the review worktree, rebuilt, and re-ran the extended fixture: all 279 asserted rows identical (colours/silent cell A and Q both ^[[34m and ^[[31m), 23 recorded, nothing else moved. Then add the two cells to the fixture's colour sample permanently so the case asserts them, or name the sub-16-colour aixterm path in the header's declared-uncovered list with the measurement."
    },
    {
      "obligation": "Invariant: the wire rule (PROTOCOL_VERSION 101, pure appends, consumer half present, the v101 history line added)",
      "severity": "must-fix",
      "description": "The branch's range (git diff origin/campaign/tui-cycle5-gated...HEAD) carries a real terminal-lane append with no line in knowledge/protocol/wire-protocol.md's v101 history. crates/zz-protocol/src/terminal_codec.rs appends one trailing u32 class word per style to BOTH payloads (encode_viewport_into `for style in viewport.styles() { push_u32(output, style.class_word()) }`, encode_patch_into over `patch.dictionary.appended_styles()`, with the matching decode_patch/decode_viewport_kind/decode_style_classes halves and both *_payload_capacity entries), and crates/zz-protocol/src/message.rs adds CLIENT_FEATURES_CAPABILITY_PREFIX = \"client-features-v1:\". PROTOCOL_VERSION stays 101, both appends are pure trailing appends, and both consumer halves are in the same commit (0faac6c0), so the substance is clean; the required history line is what is missing. knowledge/protocol/terminal-lanes.md is also now stale: its viewport payload table (the `styles [fg:u32 bg:u32 underline:u32 attrs:u16 underline_kind:u8 0:u8] x style_count (16 B each)` line, ~line 83) and its patch table (~line 145) describe the payload without the trailing class-word array, and its PackedStyle row still calls the last byte `reserved 0:u8` where it is now `background_index`. The worker's report states \"WIRE APPENDS: none\", which is true of this cycle's five commits but not of the range the invariant is judged over.",
      "suggested_fix": "Add one line to the v101 entry in knowledge/protocol/wire-protocol.md naming both appends (a u32 colour-class word per style on the Terminal lane's viewport and patch payloads; the client-features-v1: ClientHello capability token), and update the two payload tables plus the PackedStyle row in knowledge/protocol/terminal-lanes.md. No code change."
    },
    {
      "obligation": "Invariant: no added code comments",
      "severity": "nit",
      "description": "crates/zz-tui/src/terminal_event.rs gains 8 lines of plain `//` comments in three blocks (the tty_keys_device_attributes2 letter note, the tty_default_raw_keys theme-answer note, and the numeric-keypad note). The house rule in CLAUDE.md is \"Do not add comments in code\" and the review invariant repeats it. The file already carried 3 such comments at BASE, so it is not a clean-file violation; the `//!` module header on the new crates/zz-daemon/src/terminal_features.rs and every `///` doc comment are fine.",
      "suggested_fix": "Delete the three `//` blocks in crates/zz-tui/src/terminal_event.rs, or promote the pin citations they carry into the enclosing function's `///` doc comment."
    },
    {
      "obligation": "TUI-009 sources: crates/zz-terminal/src/model.rs (PackedStyle)",
      "severity": "nit",
      "description": "PackedStyle now spends attribute bits 12..15 on the two colour-class codes, and PackedStyle::new and from_raw silently `& ATTRIBUTE_MASK` (0x0FFF) whatever they are handed. Today's ATTR_* constants stop at ATTR_HYPERLINK = 1 << 8, so nothing is lost and the new test even asserts the mask, but the attribute space is down to three spare bits and a future `pub const ATTR_X: u16 = 1 << 12` would be dropped on the floor with no compile error and no debug assertion.",
      "suggested_fix": "Add a const assertion beside the ATTR_* block that the highest defined attribute bit is below CLASS_SHIFT, so a future attribute that collides fails the build instead of vanishing."
    },
    {
      "obligation": "Evidence honesty, compat/tui/evidence/TUI-009/attempt-03/",
      "severity": "nit",
      "description": "Two small record inaccuracies. (a) notes.md's \"Files\" entry for `19` says the tip's tree is \"identical to 3f6dc460\", but 0ff4a261 changed crates/zz-tui/src/terminal_event.rs after 3f6dc460 (git diff --stat 3f6dc460 HEAD -- crates/ is 1 file, 24 insertions); file 19 itself correctly names 0ff4a261. (b) 01/02/03-tui-caps-run-*.txt are byte-identical (md5 197e52072b8637b8428b0ac7cbf25bee x3). My own independent run at the tip reproduces that file byte for byte, so the fixture output is genuinely deterministic and three identical files are consistent with three real runs, but nothing in the artifacts distinguishes one run from another, so the record's \"three runs with identical dispositions\" cannot be checked from the evidence alone.",
      "suggested_fix": "Fix the 3f6dc460 reference in notes.md to 0ff4a261. For the runs, have the fixture (or the capture) print a per-run stamp - the scratch token, or a start/end time line - so three runs leave three distinguishable files."
    }
  ],
  "every_clause_asserted": "no"
}
```

### checks_run, verbatim

- fetched origin, detach-checked out 7d61628e in /home/demfabris/dev/zz-tui-caps-review (clean before and after; restored to the tip after my probes)
- confirmed BASE origin/campaign/tui-cycle5-gated (5d9bf198) is an ancestor of the tip; walked all 20 commits in BASE..HEAD with per-commit --stat
- built zz at the tip into the worker's target dir after touching crates/**/*.rs and Cargo.* (exit 0, 37s), through systemd-run MemoryMax=5G + flock slot
- compat/run.sh smoke/pane-colours-palette at the tip: exit 0, 3 steps, 0 TOPO/GEO/FMT/OUT/WARN (the punch list's item 1, the row that skipped this branch)
- compat/tui-caps.sh at the tip: exit 0, 243 asserted rows, 23 recorded, all 243 identical; byte-identical to evidence 01
- compat/tui-caps.sh --self-check at the tip: exit 0, 21 sabotages caught, 3 controls quiet
- compat/tui-screen-diff.sh at the tip: exit 0, 123 asserted checkpoints identical, 30 recorded
- compat/tui-screen-diff.sh --self-check at the tip: exit 0, including the two new sabotages for the flipped colour-classes and default-fg cases
- LC_ALL=C LC_TIME=C compat/status-row.sh at the tip: exit 0, 14/14, none recorded
- compat/run.sh smoke/format-listing smoke/terminal-facts renderer-styles at the tip: 0 divergences each (the format-listing.sh zone excursion holds)
- compat/attached-client.sh at the tip: exit 1 at 'zz screen did not visibly become copy-mode within 10 seconds', the known BASE red the modes lane owns; nothing past that step
- MY OWN ORACLE PROBE (the lane-specific one): drove the pin and zz side by side inside the outer pinned tmux with the colour sample extended to \e[31m, \e[38;5;42m, \e[38;2;10;20;30m, \e[41m, \e[38;5;12m and \e[38;5;9m, read every class back through capture-pane -p -e, across all 18 colour stages including pane-colours 1=#123456 42=colour200, OSC 4 over the option, OSC 104 back to it, the untouched entry, the array unset, OSC 10/110/11/111 and three window-style grounds - this is what found defect 1
- MY OWN SABOTAGE-OF-THE-FIX: patched downgrade_palette in the review worktree, rebuilt, re-ran the extended fixture (279 asserted rows all identical), then reverted and rebuilt back to the tip
- verified the ported colour_256to16 table byte for byte against the pin's colour.c (256/256 identical) and read tty.c tty_check_fg to check the downgrade order
- read the whole wire diff hunk by hunk: PROTOCOL_VERSION 101 unchanged, both appends trailing, consumer halves present, capacity accounting updated, class-word validation rejects garbage (with_class_word round-trip test)
- read the ClassHints/Classifier classing logic in crates/zz-terminal/src/session.rs against the pin's colour_palette_get semantics; confirmed the pane-colours class rides beside the pane palette instead of inside it
- confirmed Color::from_packed masks the top byte, so stuffing a palette index into foreground bits 24..31 does not move the GUI's resolved RGB; confirmed the GUI still connects with TerminalColorScheme::Dark (crates/zz/src/lib.rs:1135 and :1203) so the theme landing changes no desktop presentation
- cargo test -p zz-terminal (259 passed, 1 ignored), -p zz-protocol (222+7+16), -p zz-tui (194), -p zz-daemon (867 + every integration binary), -p zz (634 lib + 125 cli_binary) - all exit 0, all through the slot-and-cap wrapper at --jobs 4 --test-threads=3
- cargo clippy -p zz-terminal, zz-protocol, zz-tui, zz-daemon, zz --all-targets --all-features -- -D warnings: all exit 0
- python3 compat/tui/tracker.py check and python3 compat/tmux-tracker.py check: both green, both generated reports current
- git check-ignore -v on compat/tui/evidence/TUI-009/attempt-03/*: exit 1, nothing ignored; no .log files anywhere under compat/tui/evidence/TUI-009
- attribution scan over BASE..HEAD commit bodies: no Co-Authored-By, no 'Generated with', single author
- added-comment scan over the crates/ diff: 12 added comment lines, 4 of them the //! module header on terminal_features.rs, 8 plain // in terminal_event.rs
- zone audit against the batch's list, including the three excursions the worker declared (format-listing.sh, app.rs/lib.rs call sites, the rewritten gap acceptance clause) and the tmux-gaps.json closure (semantic:harness-theme-steering has zero hits in crates/**/*.rs and zero semantic: entries in honest_knobs.rs, so no TMUX_OPTION_CONSUMERS or compat_manifest_tests.rs partition move was due)
- hygiene: no binary copied under /tmp, /tmp at 719 MB (started 717), no zzcaps.* scratch dirs left, pgrep -fa 'zz-cli-|zz-user|zzprobe' finds nothing of mine, no server I did not start was touched

### Reviewer notes, verbatim

TUI-009 is honestly left at active, and that is the right status. Clause 1 and clause 2 openly hold 23 recorded rows, and I reproduced exactly those 23 at the tip: 8 mouse rows (mouse_all_flag and mouse_button_flag across legacy, extended, extended-always and silent/extended), 5 client_colours, 8 client_termfeatures, widths/non-utf8/line, and silent/extended pane_key_mode. Each is named in the record with its owner and its next step, and the three the worker says are outside its zones really are (app.rs for the mouse arming, render.rs's glyph path for widths).

Clause 3 is the one to watch. Everything the fixture drives asserts, and I confirmed the punch list's first item end to end: pane-colours[1]=#123456 leaves both sides as ^[[38;2;18;52;86m and pane-colours[42]=colour200 as ^[[38;5;200m, an OSC 4 over the option goes RGB on both, OSC 104 returns to the option's own class on both, an untouched entry keeps ^[[31m, and smoke/pane-colours-palette is 0 divergences. But defect 1 sits inside that clause and neither asserts nor is recorded, so the gate should not treat clause 3 as closed until the guard is fixed or the path is named in the declared-uncovered list. The fix is one line and I proved it green.

What I could not fault. The three fixtures reproduce their claimed numbers exactly (243/23, 21+3, 123/30), the self-checks catch every sabotage in its own channel, my own added sabotage found a real difference rather than passing quietly, both trackers are green with their reports regenerated, tests and clippy are green on all five touched crates with no flake (client_focus_closes_display_panes_and_preserves_chooser_modes passed on my single zz-daemon run), and the tree is clean at the pushed tip.

Suspicion I could not turn into a defect, for the gate to weigh:
- crates/zz/src/lib.rs does more than compile: -2, -u and -T now feed zz_daemon::set_client_terminal_flags, which is real behaviour in a file the zones open "only for GUI compile consequences". It is squarely on clause 2's subject, it changes no GUI presentation, and I would not hold the branch for it - but it is a wider excursion than the three the worker named.
- compat/tmux-gaps.json's third acceptance clause on options.client-terminal-negotiation was rewritten, not trimmed. The worker flagged it. The rewrite keeps the stance for the desktop client, which is what the rule asks; leaving the old clause would have made the gap assert something the raw TUI no longer does. I think it is right, but it is the kind of edit an orchestrator should bless rather than a reviewer.
- TerminalGuard::enter no longer writes \e[?7l, so the raw TUI now runs with the outer terminal's autowrap on, matching tty_start_tty. All four wrap_flag rows assert and every screen-diff checkpoint is identical, so nothing is broken today; it is just a wider blast radius than a colour change.
- crate::tty::TERMINAL_COLOURS is a process-global AtomicU32 raised with fetch_max and never cleared when a TerminalGuard drops. One attach per process makes that fine, and the fixture proves the seeded and the raised values both reach the writer, but a second attach in the same process would inherit the first terminal's count.
- The class word costs 4 bytes per style on every terminal frame, GUI clients included, and PackedStyle's Eq/Hash now separate two styles that differ only in class, so the per-frame style dictionary can grow. Not measured; worth a glance if frame size ever matters.

Verdict is approve-with-fixes: nothing here is a blocker, both must-fixes are small and mechanical, and the gate can apply them before merge.

## What the gate did with each finding

An earlier run of this gate was killed by a reboot after the rebase and the fix
pass. It left the three fix commits in place and this run started from them,
checked each against the review, and re-ran every probe the review cites.

**Defect 1, must-fix, the sub-16-colour aixterm path.** Applied in dfeff8cf,
exactly the guard the review names:

    if colours >= 256 || (index < 16 && colours >= 16) { return index; }

and the two cells are permanent, not a probe: `\e[38;5;12mA` and `\e[38;5;9mQ`
join COLOUR_SAMPLE and COLOUR_SAMPLE_RGB and the per-glyph loop in
compat/tui-caps.sh, so every colour stage asserts them. Re-ran the review's
probe at the gate tip: **279 asserted rows, 23 recorded, all 279 identical**
(gate-01), which is the review's own post-fix number, and --self-check still
catches 21 sabotages with 3 controls quiet (gate-02).

**Defect 2, must-fix, the unrecorded wire appends.** Applied in 0394293c: the
two appends are named in wire-protocol.md, and terminal-lanes.md's two payload
diagrams gain the `style_classes` array while the PackedStyle row explains that
the 16-byte record is unchanged because `foreground_raw` masks the palette index
away and `attributes` masks the class codes away.

The gate went further than the review asked, and the reason is in gate-16: the
review judged the appends against PROTOCOL_VERSION 101 and found them clean as
pure appends, which they are, but 101 **shipped** in zz 0.8.0 at fd3c64e4
before this lane landed. A released 0.8.0 client passes the envelope check and
then misdecodes every frame's style dictionary. So this gate moved the wire to
**102** in its own commit, closed the v101 entry with the tag it shipped in and
opened a v102 entry for the two appends. `git log fd3c64e4..origin/main --
crates/zz-protocol` is empty, so v102 is this lane's alone.

**Nit 3, the eight plain comments in terminal_event.rs.** Applied in 82d83713:
the three blocks became `///` documentation on the functions that own them,
which is how the file already carries its XTVERSION citation. The pin citations
survive; only the comment style moved.

**Nit 4, the attribute bits.** Applied in 82d83713: a const assertion beside the
ATTR_ block, so a future `ATTR_X: u16 = 1 << 12` fails the build instead of
being masked away in silence.

**Nit 5, the evidence inaccuracies.** The 3f6dc460 reference in notes.md is
corrected to 0ff4a261. The per-run stamp is done for this gate's own captures:
every gate-*.txt carries its revision, its command, its exit code and the wall
clock it was taken at, so no two are byte-identical. The fixture itself was not
changed to stamp its runs; that would touch a fixture for a record-keeping
reason and it is written into TUI-009's next_action instead.

## The suspicions, weighed

- **crates/zz/src/lib.rs carrying behaviour, not just compile consequence.**
  Agreed with the review that it is not a hold. It is 22 lines of argument
  parsing that stop discarding `-2`, `-u` and `-T`, and clause 2 is exactly
  "compare effects and client facts for global flags including -2, -u and -T".
  A lane cannot prove that clause without the flags reaching the daemon. Left
  as it is and named here so the orchestrator can see it.
- **The rewritten gap acceptance clause.** Left as it is. Named on the board
  note for the orchestrator to bless.
- **Autowrap, the process-global TERMINAL_COLOURS, and the 4 bytes a style.**
  All three are recorded in TUI-009's next_action so cycle 7 can pick them up.
  None is red today: every wrap_flag row asserts, all 137 screen-diff
  checkpoints are identical, and no frame-size budget exists to breach.

## What the gate found that the review did not

One corpus row, `smoke/jobs-command-environment`, is red at the lane tip and
green at origin/main on this box, five runs to two. It is not a behaviour
change - the job path is not in the diff at all - it is the fixture's own
one-second margin, which this lane's ~11 ms a call of extra load time pushes
over. Diagnosed and fixed in 5c28f025, whole story in gate-15.
