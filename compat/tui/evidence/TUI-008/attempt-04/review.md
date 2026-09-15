# Review of campaign/tui-mouse-menus-2 at 6fbdf4c3 (the menus second half of TUI-008)

Adversarial review on the ubuntu box, 2026-09-14, by an Opus 5 reviewer that did not write the
branch. Verdict: **approve-with-fixes, with one blocker that keeps TUI-008 at review.** The branch
was NOT gated before the campaign paused to move machines; the gate that takes it applies what is
below and must not verify TUI-008 or flip TUI-012 until the blocker's recorded check is closed.

## Blocker

right-click-pane/screen was flipped from record to assert at a pane cell whose row is blank, and
that is the only reason it agrees. The pin's DEFAULT_PANE_MENU renders three items off mouse_word,
mouse_line and mouse_hyperlink; zz answers empty for all three, so the menus differ over any cell
whose row carries text. Same stock gesture, own harness (outer pinned tmux, send-keys -H, real SGR
`\e[<2;col;rowM`, capture-pane -p -e, 80x24): at pane offset (6,4), the aim compat/tui-mouse.sh:908
uses, all 24 rows identical; at (2,1), one row up onto the shell prompt row, 12 of 24 rows differ:
the pin's menu carries `│ Copy Line        (l) │` and is 14 rows tall, zz's is 12 rows with no
Copy Line. Read directly with `bind-key -n MouseDown3Pane set-option -gF @mc`: over the word beta
the pin answers w=[beta] l=[alpha beta gamma] h=[] and zz w=[] l=[] h=[]; over a wrapped line head
w=[WRAPPED] l=[WRAPPED-xxx...] vs empty; over the wrap tail w=[xxx...LONGTAIL] vs empty; over an
OSC 8 link w=[LINKTEXT] l=[LINKTEXT] h=[https://example.com/page] vs empty; over a blank cell both
empty. mouse_x and mouse_y agree everywhere. This is formats.mouse-context, one of TUI-008's own
four gaps, and at the base it WAS carried by the fixture as a recorded check; after this lane
nothing in compat/tui-mouse.sh drives those three names, so the summary reads `0 recorded checks`
while the divergence is live two cells away.

Fix: keep right-click-pane/screen asserted at the blank cell and add a second check beside it,
right-click-pane/over-a-word, driving the identical gesture at a cell whose row carries text, with
MODE=record and a reason naming formats.mouse-context format:mouse_word, format:mouse_line and
format:mouse_hyperlink. The summary then reports 1 recorded, TUI-008 stays at review, TUI-012 stays
held. Closing it for real is a worker item: the daemon needs a synchronous read of the live grid
under a cell (zz-terminal already answers it for a frozen copy-mode revision in mode_format_word
and mode_format_line), as TUI-008's next_action describes.

## Must-fix

display-menu -x M, -y M, -x W and -y W still answer the screen centre on zz when there is NO
invoking mouse event, where the pin answers 0 (popup_mouse_* expand empty without event->m.valid,
strtol gives 0) and the target window's own status range for popup_window_status_line_*.
daemon.rs popup_position_variables seeds every popup_* name with centre_x/centre_y and
popup_mouse_position_values overrides them only when context.invoking_mouse() is Some. Measured on
both sides, 80x24, status at bottom, two windows, menu 12x4, from a command client: `-x M -y M`
puts the pin's menu at row 0 column 0 and zz's at row 9 column 33 (8 of 24 rows differ); `-x W -y W`
puts the pin's at row 19 column 1 over window 0's status range and zz's at row 9 column 33; `-x C
-y C` identical. Pre-existing, not a regression, charged because the lane owns the function now and
the evidence_note reads as a general fix. Related and unmeasured: for a mouse-driven menu whose
event carries no status range (a pane click), zz leaves popup_window_status_line_x at centre_x while
the pin fills it from the target window's range; status_range_start is computed from whatever range
the pointer landed in, where cmd_display_menu_get_pos filters on sr->type == STYLE_RANGE_WINDOW and
sr->argument == wl->idx.

Fix: either narrow the evidence_note to the event-driven positions and record the command-line case,
or fill popup_window_status_line_x/y from the target window's status range independently of the
event and let an absent event resolve popup_mouse_* to 0.

## Nits

- compat/tui-mouse.sh:1365 still carries the old RIGHT_CLICK_REASON after RIGHT_CLICK_MODE was
  flipped to same: dead, but it now states two false things. Empty it or reuse it for the recorded
  over-a-word check.
- The evidence_note and attempt-04/notes.md say twenty sabotages; the run drives three controls and
  twenty-two sabotages (26 self_check_case calls).
- compat/tmux-gaps.json was re-serialised with ASCII escapes in entries the lane does not own (the
  only semantic change is inside keys.root-native-mouse). Re-serialise with ensure_ascii=False.
- attempt-04/17-verify-claims.txt is a plain verify-claims run that never re-measured TUI-008 (it is
  not verified); `verify-claims.py --run TUI-008 --zz <build>` passes (`all 39 asserted checks
  identical`) but the file does not show it. Keep the --run output.

## Every clause asserted

Clause 1: NO (the formats.mouse-context hole above; everything else asserts, each with a one-sided
sabotage). Clause 2: YES (paste-under-menu asserts and reproduced independently, including a tail
carrying a second menu key; paste into pane, copy mode and prompt; focus on and off; tui-copy-mode
147/0; tui-caps 366/0). Clause 3: YES for what it reaches (four more keys.root-native-mouse items
closed with dated SGR measurements, twelve honestly kept, mouse.bound-context keeps two, a user
rebind of either menu changes the gesture on both, the GUI untouched).

## paste-under-menu

The lane is right and the first half's record was wrong: both sides end with the menu gone, the
whole 24-row decoded screen identical, the pane's own row exactly `$ sted-text` on both, no trailing
tilde on either (od -c byte-equal), @menupick alpha on both; with a tail carrying a second menu key
(`\e[200~pXaYbZ-tail\e[201~`) both show `$ YbZ-tail` and alpha. Nothing in the three-dot diff
touches the paste path, so whatever fixed it was already in bad0261f.

## Checks run (all at 6fbdf4c3 against pin d77c9dc6)

- compat/tui-mouse.sh x3: exit 0, `39 asserted checks, 0 recorded checks`, md5 ff98b647 each,
  byte-identical to the evidence runs; --self-check exit 0, 3 controls quiet, 22 sabotages caught.
- compat/tui-stock-keys.sh exit 0 (50 agree, 7 recorded; the application-reader timing record is
  load dependent) plus --self-check exit 0; compat/tui-copy-mode.sh 147/0; compat/tui-caps.sh 366/0
  twice; compat/attached-client.sh PASS first run in 5m53s.
- cargo test -p zz-protocol -p zz-mux -p zz-tui -p zz-client, -p zz-daemon (empty HOME, 895 lib),
  -p zz (638 lib, cli_binary 125): all exit 0; clippy -D warnings on all six crates exit 0; fmt
  --check exit 0.
- Delta corpus (163-row selection): all ten display-menu/display-popup-menu rows and the
  parse-sensitive batch (list-keys-padding, strict-key-validation, copy-mode-bindings, prefix2,
  smoke/command-flag-errors, smoke/args-parse-if-shell, smoke/positional-minimums,
  smoke/positional-maximums, smoke/args-parse-bind-key, smoke/alias-group-forgery, command-alias):
  every row clean. The rest of the selection was not run.
- Both trackers, tracker_test.py, wire-version.py (103; status_range_start is the last field of the
  last InputMessage variant with #[serde(default)], producer, consumer and v103 line in one commit
  35370550), verify-claims --run TUI-008, compat/check.sh: all exit 0.
- Oracle probes: the three pin rules hold (MENU_NOMOUSE starting choice; modifier bits in
  advance_click_sequence; menu item command names resolved with flags left to selection, seven
  variants message-for-message identical; the doc comment above validate_menu_item_command_names
  overstates the rule, since a single-command block with an unknown name is accepted rc=0 on the
  pin too); list-keys rows for MouseDown3Status, M-MouseDown3Status, MouseDown3Pane and
  M-MouseDown3Pane byte-identical; root table 28 rows on the pin, 16 on zz, the twelve kept items.
- Own sabotages: zz's MouseDown3Status rebound with -y S instead of -y W: 19 of 24 rows differ, so
  the status-clicks channel carries the position; a paste tail with a second menu key: identical.
- Zones: only the declared sources plus compat/tmux-gaps.json, compat/tui-mouse.sh, the ledger and
  the generated reports; crates/zz-mux/src/lib.rs gains one pub-use line; crates/zz is untouched
  (no GUI hunk at all); no attribution trailers; no non-doc comment under crates/.

For TUI-012's flip: its proof block is untouched and its revision e9199e38 is far behind; whichever
gate flips it owes a fresh three-run compat/tui-superset.sh plus --self-check at its own tip.
Carried forward: the backward emacs word selection (pin copies beta, zz copies bet) is parked in
TUI-008's next_action with the pin's measurement.
