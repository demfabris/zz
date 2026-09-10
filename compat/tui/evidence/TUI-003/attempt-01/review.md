# TUI-003 review — keys lane (campaign/tui-stock-keys)

Reviewer: Opus 5, independent review worktree `/home/demfabris/dev/zz-tui-keys-review`
at `76664eb4d217e306f54853b99dec44fa5eb48f21`.
Verdict: **approve-with-fixes**. Three must-fix, three nits, no blocker.

## Reviewer verdict, verbatim

```json
{
  "lane": "keys",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-003 clause 2",
      "severity": "must-fix",
      "description": "The split cases in compat/tui-stock-keys.sh cannot see the regression the obligation exists to prevent. split-horizontal and split-vertical assert the facts channel only (rows and cursor are recorded under BORDER_REASON), and FACTS_FORMAT (compat/tui-stock-keys.sh:280) carries session:window.pane, size, active, zoomed, window name and pane count -- every one of which is identical whether prefix % opened a shell or a Picker pane. PROOF: I copied the fixture, inserted `side_command zz bind-key -T prefix '%' split-picker -h` immediately before `type_prefix_both '%'` in run_stock_bindings (the exact pre-fix product state), and ran it at 80x24. The split-horizontal case printed `note  80x24 split-horizontal asserted facts, the rest recorded: the border cell has no background on the wire and layout.rs floors the split ratio` -- it PASSED with zz opening a Picker. The run did exit 1, but on split-vertical (`zz never showed format #{window_panes}=3`) and the select-* cases, because the Picker pane swallowed the following prefix chord. That downstream catch is an accident of the corpus order, not the case's assertion; the case that names the binding proves only that two panes of the pinned sizes exist.",
      "suggested_fix": "Append `cmd=#{pane_current_command}` to FACTS_FORMAT at compat/tui-stock-keys.sh:280 and re-run the fixture plus --self-check. Measured on a throwaway zz daemon at this tip: `split-picker -h` leaves `#{pane_current_command}` EMPTY on the new pane (`1 39x24 active=1 cmd=[] title=[new pane]`) while `split-window -h` leaves `cmd=[bash]`; the fixture pins default-command to `ENV= PS1='$ ' exec /bin/sh` on both sides, so both binaries report the same command for a stock split and the channel stays comparable. With that field present the sabotage above fails at split-horizontal, which is where the obligation's contract lives."
    },
    {
      "obligation": "TUI-003 ledger record",
      "severity": "must-fix",
      "description": "The evidence_note the gate turns into proof states a case count the run contradicts. compat/tui/campaign.json, TUI-003 evidence_note: \"CLAUSE 2, compat/tui-stock-keys.sh, 48 cases at 80x24 and 100x24\". My rerun at the tip and the committed compat/tui/evidence/TUI-003/attempt-01/stock-keys.run.txt both end `all 50 cases agree on every channel they assert, 22 recorded a difference elsewhere`, and the worker's own proofs.txt says 50. The corpus is 25 cases per size (17 stock-binding, 8 key-ownership) at two sizes.",
      "suggested_fix": "Change \"48 cases\" to \"50 cases\" in the TUI-003 evidence_note, then `python3 compat/tui/tracker.py check` and `write-report`."
    },
    {
      "obligation": "TUI-003 clause 3 (side effect of the chrome change)",
      "severity": "must-fix",
      "description": "The diff makes an on-screen hint false. crates/zz-tui/src/render.rs:1984 still draws the literal string \"Ctrl-\\ detach\" in the sidebar chrome, and after this branch nothing binds C-\\ by default. PROOF: on a fresh zz daemon built from this tip with a scratch HOME and XDG_CONFIG_HOME, `list-keys -T root` lists no C-\\ entry until I ran `bind-key -n 'C-\\' detach-client` myself. Before this branch chrome's TUI ui table bound C-\\ to ChromeAction::Detach, so the hint was true. Three assertions at render.rs:3278, 3292 and 3300 pin the same string. The worker recorded this in its notes and left it because render.rs is the canvas lane's file this cycle -- naming it so the gate schedules it rather than shipping a chord that does nothing.",
      "suggested_fix": "Replace the hint at crates/zz-tui/src/render.rs:1984 with the chord that actually detaches by default (prefix d, i.e. \"C-b d detach\"), and update the three assertions at render.rs:3278/3292/3300. Coordinate with the canvas lane, which owns render.rs this cycle."
    },
    {
      "obligation": "TUI-003 clause 2 fixture hygiene",
      "severity": "nit",
      "description": "compat/tui-stock-keys.sh's --self-check calls `await_observable <side> fact 'panes=2'` in the one-sided-chord and active-pane sabotages. `fact` is not one of await_observable's kinds (format|screen|sessions|gone), so the case statement matches nothing and each call spins its full 200 x 0.05s bound before returning 1, which `|| true` swallows. Both sabotages are still caught (wait_settled plus the compare do the work) and I reproduced --self-check exit 0, but roughly 30s per run is spent in a wait that can never succeed, and the value form is wrong too (`panes=2` where the format kind expects `#{window_panes}=2`).",
      "suggested_fix": "Change the three `await_observable <side> fact 'panes=2'` calls to `await_observable <side> format '#{window_panes}=2'`."
    },
    {
      "obligation": "TUI-003 proofs list",
      "severity": "nit",
      "description": "crates/zz-mux/src/compat_manifest_tests.rs is where this branch edits four count assertions (divergent 41->37, structurally matching 185->189, prefix 52->56), yet the worker's proofs list only runs clippy on zz-mux, never `cargo test -p zz-mux`. Clippy --all-targets compiles the test but does not execute it, so the edited assertions were never proved by the worker. I ran them: `cargo test -p zz-mux --jobs 5 -- --test-threads=2` is green, 520 passed, 0 failed. No defect in the change, only a missing proof.",
      "suggested_fix": "Add `cargo test -p zz-mux --jobs 5 -- --test-threads=2` to the obligation's proofs list and to proofs.txt with its exit status."
    },
    {
      "obligation": "TUI-003 recorded chooser divergence",
      "severity": "nit",
      "description": "zz labels its session chooser overlay \"Choose window\" when `prefix s` runs `choose-tree -Zs`. PROBE: prefix s typed into an attached zz client at this tip paints rows `Choose window` / `> (0) probe  4 windows` where the pin paints `(0) + probe: 4 windows (attached)`. This sits inside the recorded CHOOSER_REASON divergence (zz renders choose-tree as a client overlay), but the wrong noun is a separate, cheaper bug than the overlay-vs-mode-tree shape, and the record does not mention it.",
      "suggested_fix": "Add the mislabelled title to the recorded chooser finding in TUI-003's evidence_note so whoever picks up the chooser surface sees it as its own item."
    }
  ]
}
```

### Reviewer checks_run, verbatim

```
GIT_TERMINAL_PROMPT=0 git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'
git -C /home/demfabris/dev/zz rev-parse origin/main origin/campaign/tui-stock-keys  (bfd05821 / 76664eb4, tip matches the worker report)
git -C /home/demfabris/dev/zz worktree add /home/demfabris/dev/zz-tui-keys-review 76664eb4
git -C /home/demfabris/dev/zz-tui-keys find crates -name '*.rs' -exec touch {} +  (worker tree clean at 76664eb4 before I reused its target)
cd /home/demfabris/dev/zz-tui-keys-review && find crates -name '*.rs' -exec touch {} + && touch Cargo.toml Cargo.lock crates/*/Cargo.toml
CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-keys/target cargo build -p zz --jobs 5  -> exit 0, 2m10s
git diff --stat origin/main...HEAD  (39 files, +2913/-156)
git diff origin/main...HEAD -- compat/tui/campaign.json compat/tmux-gaps.json crates/ compat/packaged-cli.sh knowledge/  (read every hunk)
ZZ_COMPAT_TMUX=<pin> ZZ_COMPAT_CORPUS=<corpus> ZZ_BIN=<my build> compat/tui-stock-keys.sh  -> exit 0, 'all 50 cases agree on every channel they assert, 22 recorded a difference elsewhere'
compat/tui-stock-keys.sh --self-check  -> exit 0, all five expectations met
compat/tui-launch-diff.sh  -> exit 0, 'all 29 comparisons agree where they assert, 5 recorded not asserted'
compat/tui-launch-diff.sh --self-check  -> exit 0
compat/tui-screen-diff.sh  -> exit 0, 'all 33 asserted checkpoints identical' (regression)
compat/tui-pane-geometry.sh  -> exit 0, 'all 5 asserted measurements identical' (regression)
diff of my four fixture runs against the committed evidence (stock-keys.run.txt, launch-diff.run.txt, screen-diff.run.txt, pane-geometry.run.txt)  -> byte-identical, all four
MY OWN SABOTAGE 1: throwaway copy of tui-stock-keys.sh with `bind-key -T prefix '%' split-picker -h` on zz before the chord  -> exit 1, 7 of 17 cases DIFF with case name, size, first differing row index, row bytes and both facts lines; but split-horizontal itself passed (see defect 1)
MY OWN SABOTAGE 2: throwaway copy with `bind-key -T prefix s|w focus-sidebar` on zz  -> exit 1, chooser-sessions and chooser-windows both DIFF on the facts channel (pane 80x23 -> 51x23), reported with location
cargo test -p zz-protocol -p zz-tui -p zz-client -p zz-mux --jobs 5 -- --test-threads=2  -> exit 0, 0 failed (includes the zz-mux manifest test the worker never ran)
timeout 2400 cargo test -p zz --jobs 5 -- --test-threads=2  -> exit 0, cli_binary 125 passed in 126.51s (integration-test rule)
cargo clippy -p zz-protocol -p zz-client -p zz-tui -p zz-mux -p zz --all-targets --all-features --jobs 5 -- -D warnings  -> exit 0
python3 compat/tui/tracker.py check  -> exit 0;  python3 compat/tmux-tracker.py check  -> exit 0
python3 compat/tui/tracker.py write-report && python3 compat/tmux-tracker.py write-report && git status --short  -> no diff, both generated reports were regenerated not hand-edited
git check-ignore -v compat/tui/evidence/TUI-003/attempt-01/*  -> exit 1, nothing ignored; no .log file anywhere in the diff
ORACLE: grep the pinned key-bindings.c and probe the pin live on a throwaway -L zzprobe-$$ -f /dev/null server: prefix % = split-window -h, " = split-window, s = choose-tree -Zs, w = choose-tree -Zw, z = resize-pane -Z, o = select-pane -t :.+, d = detach-client -- exactly the four commands the branch put in KeyTables::default
ORACLE: same list-keys probe against a zz daemon built from this tip on /tmp/zzrev.sock with scratch HOME + XDG_CONFIG_HOME  -> identical strings for %, ", s, w, z, o, d; M-s and M-S bind as two distinct root entries; C-\ binds
MY OWN CHROME-OWNERSHIP PROBE (bindings of my choosing, not the fixture's): both binaries attached inside one outer pinned tmux, `bind -n C-\ new-window -d -n REVBACKSLASH`, `bind -n M-s ... REVALTS`, `bind -n M-S ... REVALTSHIFT` on both sides, chords typed with send-keys against the OUTER pane  -> both sides end with REVALTS,REVALTSHIFT,REVBACKSLASH,win. Identical; M-S does not fold to M-s
MY OWN STOCK-CHORD PROBE: prefix % then prefix " typed into the attached clients  -> zz `0 40x23 cmd=sh|1 39x11 cmd=bash|2 39x11 cmd=bash` and the pin identical; prefix s opens a real chooser on both (zz as an overlay)
PICKER DISTINGUISHABILITY PROBE: `split-picker -h` on a zz daemon reports pane_current_command EMPTY, `split-window -h` reports bash -- the measurement behind defect 1's fix
WIRE RULE: PROTOCOL_VERSION untouched (still 99 at crates/zz-protocol/src/message.rs:21), nothing under crates/zz-protocol/src/message.rs or snapshot.rs in the diff, crates/zz-daemon untouched entirely
PaneSnapshot field audit: crates/zz-protocol/src/snapshot.rs:458/460 carry border_colour and active_border_colour and no background -- the recorded wire finding is true
MANIFEST COUPLING: crates/zz-mux/src/compat_manifest_tests.rs:1351 asserts tracked_bindings == divergent_bindings, so the four binding: items had to leave compat/tmux-gaps.json mechanically
MERGE HAZARD: git merge-tree --write-tree origin/campaign/tui-stock-keys origin/campaign/tui-canvas-sidebar -> only conflict is the generated knowledge/tmux/tui-parity.md; state.rs and sidebar.rs auto-merge and the merged tree contains no surviving toggle_focus / toggle_sidebar_focus / global_key_route
grep -n sleep on both new fixtures: every occurrence is the poll interval inside a bounded wait_for / wait_settled / await_observable (200 x 0.05s); no fixed-duration wait
git log origin/main..HEAD --format='%H%n%B'  -> four commits, no Co-Authored-By or any attribution trailer
git diff 1c1e4ee2 76664eb4 --name-only  -> only compat/tui/evidence/TUI-003/attempt-01, which is what revision.txt claims about the amend
cleanup: killed every server I started (-L zzprobe-rev2, -L zzprobe-rev2inner, /tmp/zzr2.sock, /tmp/zzrev.sock), removed both sabotage copies, review worktree git status --short empty, no default socket touched (no /run/user/1000/zz/default.sock, no /tmp/tmux-1000/default)
```

## Gate actions (alienware/orchestrator, 2026-09-10)

The keys branch was gated SECOND, rebased onto the merged canvas tip `cdfd97be`.
The only rebase conflict was the generated `knowledge/tmux/tui-parity.md`, resolved by
running `python3 compat/tui/tracker.py write-report` rather than by hand.
`compat/tui/campaign.json` auto-merged: TUI-003 and TUI-004 are different records.

All three must-fixes applied in gate commit `b96062ca`, each proved by re-running the
reviewer's own failing probe.

1. **Must-fix, split cases blind to the substitution.** `FACTS_FORMAT` now carries
   `cmd=#{pane_current_command}`. Proof, the reviewer's exact sabotage rebuilt at the
   gate tip (a throwaway copy with `side_command zz bind-key -T prefix '%' split-picker -h`
   inserted immediately before `type_prefix_both '%'`): the run exits 1 and
   **`DIFF  80x24 split-horizontal`** now fires at the case that names the binding,
   on the facts channel, `cmd=sh` on the pin against an empty command on the zz side —
   where before the fix that case printed `note ... asserted facts` and passed. The
   sabotage copy was deleted and the worktree verified clean afterwards.
2. **Must-fix, ledger case count.** `48 cases` -> `50 cases`. The run at the gate tip
   ends `all 50 cases agree on every channel they assert, 24 recorded a difference
   elsewhere`. Both trackers check green.
3. **Must-fix, the false on-screen hint.** `crates/zz-tui/src/render.rs` drew
   `Ctrl-\ detach` for a chord nothing binds after this branch. It now reads
   `C-b d detach`, with all three assertions updated. The hint only renders under an
   uncustomised status, where the prefix is the default `C-b`, so the chord it names is
   the one that actually detaches. render.rs was the canvas lane's file this cycle, which
   is why the worker recorded it instead of touching it; the gate applied it after canvas
   had merged, and there was no textual conflict with either the canvas or the flow hunks.

Nits: the three `await_observable <side> fact 'panes=2'` calls now read
`await_observable <side> format '#{window_panes}=2'`, so they can succeed instead of
spinning their full bound. The mislabelled chooser title is recorded in the
`evidence_note` as its own item. The missing `cargo test -p zz-mux` proof is covered
more strongly at the gate: `cargo test --workspace --all-features` ran green at this
tip, which executes the manifest test the worker only compiled.

### What the gate found that neither the worker nor the reviewer did

Adding the command to the facts channel surfaced two things.

**A real regression the lane's proof set could not see.** `cargo test --workspace`
failed at the rebased tip on `zz-daemon`:
`default_percent_binding_creates_a_picker_via_split_picker` asserted
`prefix %` is bound to `split-picker -h` and that the chord builds a Picker pane —
exactly the behaviour TUI-003 set out to remove. Neither the worker's nor the
reviewer's proof set ran `zz-daemon`. Fixed in gate commit `3e7019fc`: the test now
asserts the stock chord builds a real terminal pane in a horizontal split, then binds
`split-picker -h` over it and drives the chord again, which keeps the Picker coverage
this test was the only holder of and proves the recorded decision in one test.

**A divergence this obligation neither owns nor caused.** At the instant a pane's
foreground child starts, with `default-command` pinned to `exec /bin/sh`, the pin
already answers `#{pane_current_command}=cat` while zz answers the shell or an empty
string; zz has caught up by the next chord, and every later application case asserts
the field and agrees. Measured at the gate tip. The `application-reader` case therefore
asserts its rows and cursor, which are its contract, and records the command with that
measurement; the field stays asserted in the other 49 cases, where the pane is a
settled shell. Routed in `evidence_note` for whoever owns pane runtime facts.
