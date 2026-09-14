# TUI-008, the keys lane: the reviewer's verdict and what the gate did with it

The reviewer's verdict was `approve-with-fixes` with three must-fix defects and
`every_clause_asserted: "no"`. The verdict JSON, verbatim (the `notes` field is
reproduced under "The reviewer's notes" below rather than inline, so the JSON
here stays readable):

```json
{
  "lane": "keys",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "compat/tui-mouse.sh's STATUS channel flipped from `record` to `same` in this landing (disposition block: STATUS_MODE=record -> STATUS_MODE=same), which turns status-clicks/window-after-name-click, status-clicks/window-after-wheel-down and status-clicks/window-after-wheel-up into asserted checks. No --self-check sabotage drives any of them. The only self-check that runs case_status_clicks, sc_one_sided_status_left, differs on status-clicks/row alone (confirmed: in my self-check run that case's single DIFF line is `status-clicks/row`). So the three checks the WheelDownStatus/WheelUpStatus landing exists to flip carry no proof that they can fail, against the batch rule that every flipped case gets a sabotage. FAILURE SCENARIO: a later change that drops the WheelDownStatus/WheelUpStatus root rows, or makes next-window step from the context window again, leaves the fixture green and its self-check silent, and the gate reads '24 asserted, every sabotage caught' while trusting three checks whose failure path was never demonstrated. I wrote the missing sabotage and it works: unbinding WheelDownStatus on zz only reports DIFF status-clicks/window-after-wheel-down tmux:1 zz:0 and DIFF status-clicks/window-after-wheel-up tmux:0 zz:1 and nothing else, i.e. caught in its own channel.",
      "suggested_fix": "Add to compat/tui-mouse.sh beside sc_one_sided_border_drag:\n\nsc_one_sided_status_wheel() {\n  side_command zz unbind-key -T root WheelDownStatus >/dev/null 2>&1\n  case_status_clicks\n  side_command zz bind-key -T root WheelDownStatus next-window >/dev/null 2>&1\n}\n\nand register it in run_self_check as: self_check_case \"WheelDownStatus unbound on zz only\" catches sc_one_sided_status_wheel. Verified working at this tip."
    },
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "compat/tmux-gaps.json closes five items of keys.root-native-mouse, but the closure paragraph's dated measurements cover only three of the five bindings: 'border-drag/pane-width is 32 on both' (MouseDrag1Border), 'status-clicks/window-after-wheel-down is window 1 on both' (WheelDownStatus) and 'window-after-wheel-up still 0 on both' (WheelUpStatus, and that value was 0 before the landing too, so it measures nothing). key:root:MouseDown1Border and key:root:MouseDown1Control8 are closed on the unmeasured phrase 'and each runs'. No case in compat/tui-mouse.sh asserts what either binding DOES: border-user-binding asserts a USER binding fires, border-drag asserts the resize, and grep -n Control compat/tui-mouse.sh is empty, so there is no Control8 case at all. The fixture's own header block even advertises a channel 'border-click / the active pane index' that no case implements. FAILURE SCENARIO: the gate reads five items closed with dated measurements when two of the five have no behavioural measurement anywhere in the tree; if MouseDown1Border later stops clearing the mark or MouseDown1Control8 stops zooming, every fixture and both trackers stay green while the closed gap items keep asserting parity nothing measures. I drove MouseDown1Border myself and the behaviour IS correct, so this is an evidence hole rather than a divergence: with two panes, pane 1 marked and pane 0 active, a real SGR press+release on the vertical divider leaves #{pane_marked_set} at 0 and the active pane at 0 on both binaries, matching the pin's cmd-select-pane.c:145-146 server_clear_marked.",
      "suggested_fix": "Add the border-click case the fixture header already promises (I ran it at this tip and it asserts, taking the fixture to 26/10): split_both; select-pane -m -t 0.1; select-pane -t 0.0; press and release button 1 at column pane_right+2; assert_value on `display-message -p '#{pane_marked_set}'` and on active_pane_index, both 0 on both sides. Pair it with a sabotage that unbinds MouseDown1Border on zz only, then cite it in the keys.root-native-mouse closure. Otherwise move key:root:MouseDown1Border and key:root:MouseDown1Control8 back to the accepted item list until a measurement exists."
    },
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "crates/zz-daemon/src/daemon.rs: the new `mouse_format_variables` was inserted between an existing doc comment and the function that comment described, so the comment now documents the wrong function and `client_viewport_facts` has none. At the tip, line 32663 begins `/// \\`tty_window_offset1\\` reads \\`c->session->curw->window\\` and / \\`server_client_get_pane(c)\\`, so the comparison is the client's own current / window and the pane that window is showing, never the format's window.` — three lines about client_viewport_facts — followed immediately by the four new lines about `ft->m` and then by `fn mouse_format_variables(` at line 32671. `fn client_viewport_facts(` now begins at line 32695 with no doc comment at all. The 3-dot diff shows it: those three lines are context at the top of the added hunk. FAILURE SCENARIO: a reader of mouse_format_variables is told the function reads c->session->curw->window and compares the client's own current window, which it does not do at all, while client_viewport_facts — the function that paragraph actually explains — carries nothing, so the next person to touch either reasons from the wrong pin citation.",
      "suggested_fix": "Move the three `tty_window_offset1` lines from above `fn mouse_format_variables` down to immediately above `fn client_viewport_facts`, leaving only the four `ft->m` lines on the new function. No code change."
    }
  ],
  "every_clause_asserted": "no"
}
```

## The reviewer's checks_run, verbatim

1. setup: fetched origin; origin/main 289c8a7c, origin/campaign/tui-keys 7f593a59 (matches the worker's reported tip); checkout --detach 7f593a59 in /home/demfabris/dev/zz-tui-keys-8-review, git status --porcelain empty before and after every step; merge-base with origin/main is 289c8a7c so origin/main...HEAD is the whole lane
2. build: touched crates/**/*.rs and Cargo.* in the review worktree, CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-keys-8/target, systemd-run --scope MemoryMax=5G + flock slot + cargo build -p zz --jobs 3 -- exit 0, objects compiled from the review worktree's own paths
3. compat/tui-mouse.sh at tip -- exit 0, 24 asserted / 10 recorded, md5 2236b14173a19b1bab9a066c35eeec4e, byte-identical to all three recorded evidence runs
4. compat/tui-mouse.sh --self-check -- exit 0, twelve sabotages each caught in its own channel and three controls quiet; I mapped every DIFF line to the sabotage that produced it
5. REVIEWER SABOTAGE (mine): WheelDownStatus unbound on zz only -- caught, and only on status-clicks/window-after-wheel-down and /window-after-wheel-up (this is defect 1's missing sabotage)
6. REVIEWER CASE (mine): rv-border-click, pane 1 marked then a real SGR press+release on the divider -- both sides pane_marked_set 0 and active pane 0, fixture 26 asserted / 10 recorded (measures defect 2's uncovered binding; behaviour is correct)
7. compat/tui-stock-keys.sh -- exit 0 on the FIRST run, 50 cases agree / 7 recorded, byte-identical to the recorded evidence (item 5's detach-timestamp race is gone)
8. compat/tui-stock-keys.sh --self-check -- exit 0, including the new detach-time equivalence control and the detach-status sabotage (Pane is dead (status 0, DEAD-TIME) vs (status 3, DEAD-TIME)), so the normalisation blanks the second and nothing else
9. TMUX_BIN=<pin> ZZ_BIN=<tip build> compat/attached-client.sh -- exit 0, PASS (7m13s)
10. compat/tui-screen-diff.sh -- exit 0, 147 asserted / 6 recorded
11. compat/tui-copy-mode.sh -- exit 0, 147 cases, 0 recorded
12. ORACLE: list-keys -T root, -T copy-mode and -T copy-mode-vi on throwaway servers both sides (scrubbed HOME and XDG_CONFIG_HOME, pin -f /dev/null). zz root = exactly 5 rows whose command text is identical to the pin's for MouseDown1Border, MouseDown1Control8, MouseDrag1Border, WheelDownStatus and WheelUpStatus; pin root = 27. copy-mode 71 vs 77 and copy-mode-vi likewise: none of the pin's 14 copy-table mouse rows installed, as recorded
13. ORACLE: pin key-bindings.c:497-537 read directly -- MouseDown1Border { select-pane -M }, MouseDown1Control8 { resize-pane -Z }, MouseDrag1Border { resize-pane -M }, WheelDownStatus { next-window }, WheelUpStatus { previous-window }, MouseDown1Status { switch-client -t= }; cmd-select-pane.c:137-149 confirms -M is server_clear_marked. Every installed row's command matches the pin verbatim
14. ORACLE: resize-pane -M with no mouse event in the tree -- exit 0 and pane_width unchanged (40 -> 40) on BOTH binaries, matching the closure's claim
15. ORACLE: all eight mouse_* formats read through display-message with no mouse event -- empty on both binaries, so the three that moved from a constant backing to StatusHook did not start answering outside a mouse binding
16. delta corpus: 26 of the 217 rows the lane's touched_commands select, run in four sequential chunks with --strict-geometry, ALL clean on TOPO/GEO/FMT/OUT/WARN -- list-keys-padding strict-key-validation copy-mode-bindings windows resize resize-directions resize-window panes formats-values census-formats | stderr-parity smoke/command-flag-errors smoke/positional-maximums smoke/format-listing smoke/keys-prefix-stock smoke/keys-table-lifecycle pane-selection-marked smoke/focus-follows-mouse smoke/hooks-pane-focus smoke/display-menu-mouse smoke/args-parse-bind-key | daemon-command-item-format smoke/control-notify smoke/remain-on-exit-format pane-dead-time | command-item-format (8m18s, 125 steps, clean)
17. cargo test -p zz-protocol -p zz-mux -p zz-tui -p zz-client --jobs 3 -- --test-threads=3 -- exit 0
18. cargo test -p zz-daemon --jobs 3 -- --test-threads=3 -- exit 0, 893 passed, no client_focus_closes_display_panes_and_preserves_chooser_modes flake
19. cargo test -p zz --jobs 3 -- --test-threads=3 -- exit 0, cli_binary 125 passed
20. cargo clippy -p zz-protocol -p zz-mux -p zz-tui -p zz-daemon -p zz-terminal -p zz-client --all-targets --all-features -- -D warnings -- exit 0; cargo clippy -p zz ... -- exit 0; cargo fmt --all -- --check -- exit 0
21. python3 compat/tui/tracker.py check -- exit 0, knowledge/tmux/tui-parity.md current; python3 compat/tmux-tracker.py check -- exit 0, knowledge/tmux/gaps.md current
22. ledger ownership: structural diff of compat/tui/campaign.json against origin/main -- exactly one item changed, TUI-008, and only evidence_note, next_action and sources; status stays active, proof block stays null, no other id, no milestone, no baseline and no pin touched
23. gaps ownership: compat/tmux-gaps.json touches only keys.root-native-mouse, keys.copy-mode-native-mouse (unchanged), mouse.bound-context and formats.mouse-context -- the four gaps the batch names; nine items removed, each named in a dated 2026-09-13 closure paragraph, every unclosed item keeps its decision
24. wire rule: PROTOCOL_VERSION still 102; MouseKey is the LAST variant of InputMessage and `border: Option<Axis>` the LAST field of it (message.rs read by line); knowledge/protocol/wire-protocol.md names the append in both v102 places; consumer half (zz-tui input.rs writes, zz-daemon daemon.rs reads) lands in the same commit baa777d8
25. zones: crates/zz-tui/src/{render,tty,terminal_event}.rs have ZERO hunks; no chooser surface and no catalog unimplemented-command roster touched (only resize-pane's -M flag and usage string). Excursions status.rs, compat_manifest_tests.rs and tests/hunt_claims.rs are all declared in notes.md
26. GUI: crates/zz and clients/ have zero hunks; grep -rn MouseKey crates/ names only zz-daemon/daemon.rs, zz-tui/input.rs and zz-protocol/message.rs, so the GUI sends no mouse key and its presentation is unchanged
27. test honesty: no added non-doc `//` comments (grep of the + side of the crate diff is empty), no attribution trailers in any of the nine commits, git check-ignore -v on compat/tui/evidence/TUI-008/attempt-02/* exits 1 with nothing ignored, no .log files anywhere under compat/tui/evidence, notes.md names every evidence file
28. machine hygiene: no binary copied under /tmp; both probes ran under mktemp scratch HOMEs with trap cleanup; pgrep -fa 'zz-cli-|zz-user|zzprobe|zzprv' names no surviving process of mine; no scratch dir left; no server on a default socket touched; the review worktree is clean at 7f593a59

## The reviewer's notes, verbatim

HONESTY: the worker's report is accurate about what does and does not assert. All three acceptance clauses are open, status stays `active`, and my re-runs reproduce the numbers exactly - compat/tui-mouse.sh built at my own tip is byte-identical to all three recorded evidence runs at md5 2236b14173a19b1bab9a066c35eeec4e, 24 asserted / 10 recorded. Nothing is claimed asserted that is not. The obligation cannot be verified this cycle and the worker does not ask for it.

WHAT IS GENUINELY DONE. Item 5 (the detach timestamp race three gates paid for) is the cleanest piece of work here: the fixture was green on its FIRST run for me, the normalisation touches only the parenthesised second, and the new self-check pair - an equivalence control plus a status-3 sabotage - proves the rest of the dead-pane line is still compared. Item 3's choke point is real and I verified both halves against the pin: resize-pane -M with no mouse event exits 0 and changes nothing on both binaries, and all eight mouse_* answer empty on both outside a binding. Item 4's two landed cases hold. The border latch (item 2) is the right mechanism and border-drag/pane-width agrees at 32.

TWO SUSPICIONS THAT DID NOT BECOME DEFECTS. (a) select-pane -M clears the marked pane rather than selecting one, so installing MouseDown1Border changes what a raw-TUI border click does; I drove it and both sides agree (mark cleared, active pane unmoved), so the install is right and only the measurement is missing (defect 2). (b) The step_window_in_session excursion changes next-window/previous-window for every caller, not just mouse bindings; `windows`, `smoke/keys-prefix-stock`, `smoke/keys-table-lifecycle` and the whole zz-mux suite are clean, and cmd_select_window_exec calling session_next from s->curw confirms the new code is closer to the pin than the old.

NITS, none blocking.
1. `python3 compat/tui/lint-runner.py -- exit 0` is listed as a proof. With no argument it prints its usage and exits 2 (I ran it). It lints a cycle runner and there is no run-8.js in this lane, so the line is not a real result and should come out of the list rather than be chased.
2. `list-keys -T root` padding still diverges. With a user's F2 bound in root, zz prints it padded to its widest root name (MouseDown1Control8) and the pin to MouseDrag1ScrollbarSlider, so the columns differ. The landing NARROWS this - hunt_claims.rs's expected string went from unpadded to padded, which is base behaviour showing through - so it is not a regression, and no row catches it because list-keys-padding clears the root table with unbind-key -aq first. One sentence in evidence_note would stop the closure's "in the pin's own printed spelling" being read as full parity: the command text matches, the column padding cannot until all 27 rows are installed.
3. resolve_mouse_targets in crates/zz-mux/src/command.rs treats any argument after `-t` or `-s` as a target slot. `-s` is not always a target: `new-session -s <name>` and `set-option -s` are not, so a mouse binding running `new-session -s =` would have its session NAME rewritten to a pane id where the pin would create a session called `=`. Marginal enough that I did not drive it; the pin's rule (only slots a command declares as CMD_FIND_*) is the shape to grow into.
4. Product decision (3), focus delivery, genuinely reaches the GUI: a GUI pane that armed \e[?1004h now receives focus reports whatever focus-events says, because the GUI has no arming handshake. The worker recorded it with the required sentence and named the follow-up (crates/zz/src/terminal/view.rs, outside these zones). Not a presentation change, and the GUI still compiles and sends no mouse key, but the gate should carry the follow-up forward rather than let it dissolve into the note.
5. crates/zz-mux/src/lib.rs is the one changed file not named in notes.md's excursion list - a single re-export line for MouseEventTarget, whose type lives in command.rs, which is in zone. Trivial.

CORPUS SCOPE, stated plainly. I ran 26 of the 217 rows the lane's touched_commands select, not the 223 the worker ran, because the full set does not fit the review budget (command-item-format alone is 8m18s here). I chose the rows this landing can actually reach: the four key-table rows, the window-stepping and resize rows, the format inventory rows, the command-spec rows that read resize-pane's usage and its former "unsupported" error text, the marked-pane row, the two focus rows, the mouse menu row, and the dead-pane and remain-on-exit rows the new normalisation sits beside. All 26 clean, so the "a screen string you remove is asserted somewhere else" trap that cost cycle 6 two landings is not sprung here. The worker's claim of three known/ rows at their documented divergences and four environmental rows is consistent with what this box is documented to do; I did not re-derive it.

BUDGET: about 65 minutes after the build. No blockers: no false clause, no fixture that cannot fail, no zone or wire violation, no red at tip. The three must-fixes are small and mechanical, and I verified the fix for the first one works before naming it.

## What the gate did: review_actions

The verdict binds: every must-fix is applied in its own commit, with the
reviewer's own probe re-run as proof.

### must-fix 1, the missing status-wheel sabotage -- APPLIED, commit 849e9283

`sc_one_sided_status_wheel` added beside the border-drag sabotage exactly as the
reviewer wrote it, and registered in `run_self_check`. PROBE at the gate tip:
`compat/tui-mouse.sh --self-check` exits 0 and the new case reports two DIFF
lines and no others, `status-clicks/window-after-wheel-down` (tmux:1 zz:0) and
`status-clicks/window-after-wheel-up` (tmux:0 zz:1), so the two wheel checks the
WheelDownStatus/WheelUpStatus landing exists to flip now carry a demonstrated
failure path. Log: gate-tui-mouse-self-check.txt.

### must-fix 2, MouseDown1Border measured and MouseDown1Control8 told straight -- APPLIED, commit eaeb362f

`case_border_click` added, the channel the fixture's own header block already
advertised: two panes, pane 1 marked with `select-pane -m`, pane 0 active, then a
real SGR press and release on the divider at `pane_right + 2`. It asserts
`border-click/marked-set` and `border-click/active-pane`, both 0 on both
binaries, which is cmd-select-pane.c:145-146 -- `-M` clears the marked pane
unconditionally and never moves the active pane. The fixture goes from 24
asserted / 10 recorded to 26 / 10, the number the reviewer predicted. Its
sabotage, `sc_one_sided_border_click`, unbinds `MouseDown1Border` on zz only and
is caught on `border-click/marked-set` alone.

ONE THING THE REVIEWER COULD NOT HAVE KNOWN, found by running it: the sabotage
has to be registered BEFORE `sc_one_sided_border_binding`. That sabotage's case
rebinds `MouseDown1Border` and then `unbind-key -n`s it, which takes the PIN's
stock binding away for the rest of the run -- the hazard the fixture already
warns about above `case_border_user_binding`. Registered after it, the pin stops
clearing the mark too and the sabotage fails with a 10-second wait timeout
rather than a DIFF. It is registered before it, with that constraint written
down beside it.

`key:root:MouseDown1Control8` could NOT take the reviewer's "otherwise" branch.
Moving it back to the accepted item list fails
`compat_manifest_tests::option_format_hook_and_default_key_items_match_pinned_inventories`
with "implemented default key has a stale item": the `key:` items track names zz
does NOT install, and zz installs this one. So the closure paragraph is rewritten
instead to say exactly what is and is not measured. Four of the five rows are
driven from a pointer with a sabotage each and their measurements are named; the
fifth is closed on the `list-keys` oracle alone, and the paragraph now says so in
those words, names why nothing can measure it yet (the pin reaches
`MouseDown1Control8` only through the `control|8` range its default
pane-border-format publishes, options-table.c:1534-1548, which needs
`pane-border-status` on, and no fixture here draws a pane border status line or
aims a pointer at one), and names what a measurement waits on.

### must-fix 3, the doc comment on the wrong function -- APPLIED, commit 605bc692

The three `tty_window_offset1` lines moved from above `fn mouse_format_variables`
to immediately above `fn client_viewport_facts`, the function they describe. The
four `ft->m` lines stay on the new function. No code change; `cargo build -p zz`,
the workspace clippy and `cargo fmt --all -- --check` all exit 0 after it.

### The nits

Nit 2 (list-keys padding) is folded into the evidence_note: the command text
matches, the column padding cannot until all 27 rows are installed. Nit 5
(crates/zz-mux/src/lib.rs missing from the excursion list) is added to notes.md.
Nit 4 (the GUI focus follow-up) is carried into next_action and the board note
rather than left in prose. Nits 1 and 3 are recorded and not chased: the
lint-runner line is the worker's report, not the ledger, and `resolve_mouse_targets`
is a shape to grow into, not a defect measured here.

### Sibling flips

`sibling_cases` is empty for this lane, so there was nothing to flip.
compat/tui-choosers.sh reports "4 recorded not asserted (0 for a sibling lane)".

### Verified?

No. TUI-008 verifies only when compat/tui-mouse.sh has zero recorded cases
inside its three clauses; it still records ten. All three acceptance clauses stay
open, the status stays `active`, and the proof block stays null. The reviewer's
`every_clause_asserted` is "no" and the gate agrees with it.
