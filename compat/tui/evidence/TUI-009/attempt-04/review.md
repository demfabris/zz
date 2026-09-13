# Cycle 7 input-lane gate: review, the must-fix, and what still holds

lane tip reviewed: d493fee5427f5a1b827e12a2e59426b50d1ad4ce on campaign/tui-input
gate worktree: /home/demfabris/dev/zz-gate-tui7, local branch gate-input
rebased onto: origin/main 53d206dc5167ab009f41b6bbd1a15af6ecea6c18 (no-op; the
lane tip was already a direct descendant, and `git merge-tree --write-tree`
predicted no conflict)
gate tip before this records commit: 49c0b036b326bd9c3af27fc50013f0241f0284d1
pin: d77c9dc6aa021e4bc61f0da128c591af695e6466 (tmux next-3.8)
box: alienware, CachyOS, 16 cores, 15 GB + 15 GB zram. See `gate-environment.txt`.

## Reviewer verdict, verbatim

```json
{
  "lane": "input",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-008",
      "severity": "must-fix",
      "description": "A menu that the pin marks MENU_NOMOUSE keeps the raw TUI in any-event tracking where the pin uses button tracking. crates/zz-tui/src/app.rs desired_mouse_arming returns Some(true) for any model.menu, but the pin's menu.c:592 sets MODE_MOUSE_ALL|MODE_MOUSE_BUTTON on the overlay screen only when ~md->flags & MENU_NOMOUSE, and cmd-display-menu.c:106 sets MENU_NOMOUSE whenever the menu was not invoked from a mouse event and carries no -M. I measured it twice with an outer pinned tmux as decoder, both clients attached with mouse on, the same 'bind -T prefix E display-menu -x 4 -y 8 -T PROBEMENU ...' on both sides and C-b E driven into each client, reading the outer pane's own '#{mouse_all_flag} #{mouse_button_flag}': zz all=1 btn=0, pin all=0 btn=1. The same menu raised with -M is identical on both (all=1 btn=0), so the divergence is exactly the NOMOUSE case, which is the common one (any menu from a key binding). In bytes: with that menu up the pin's tty_update_mode writes the clear then \\e[?1006h\\e[?1000h\\e[?1002h and stops, while the raw TUI writes the Any sequence ending \\e[?1003h. This is not a regression (origin/main armed \\e[?1003h for the whole attach, so it read all=1 there too), but the landing claims to have closed it: TUI-008's evidence_note decision (2) says the raw TUI now raises 1003h 'only for a menu, for focus-follows-mouse or for a pane that asked for it ... and the raw TUI now does the same', and the desired_mouse_arming doc comment states 'A menu is the overlay that carries MODE_MOUSE_ALL of its own (menu.c menu_prepare)' without the -M qualification. No fixture case covers the arming while a menu is up: tui-caps.sh's eight mouse rows are all measured with no overlay, and tui-mouse.sh's paste-under-menu compares only the screen and the picked item.",
      "suggested_fix": "In crates/zz-tui/src/app.rs desired_mouse_arming, use the menu's own policy instead of a constant: `let overlay_any = if let Some(menu) = model.menu.as_ref() { Some(menu.mouse_keys) } else { model.popup.as_ref().map(...) }`. MenuState::mouse_keys already carries the MENU_NOMOUSE distinction (crates/zz-protocol/src/message.rs:2897, documented there against the same pin rule), so no wire change is needed, and the existing branch then yields Button under a NOMOUSE menu with mouse on and Off with mouse off, which is what the pin produces. Add a case for it: a menu raised without -M on both sides, reading #{mouse_all_flag} and #{mouse_button_flag} off the outer pane (the channel tui-caps.sh already uses), with the -M menu beside it as the control. Then correct decision (2) in TUI-008's evidence_note and the desired_mouse_arming doc comment to name the -M/invoking-mouse-event condition."
    }
  ],
  "every_clause_asserted": "no"
}
```

## The reviewer's checks, as reported

- fetch origin, checkout --detach d493fee5 in /home/demfabris/dev/zz-tui-input-review; merge-base with origin/main is 53d206dc, so three-dot equals two-dot
- cargo build -p zz at tip through the slot-and-cap wrapper (CAP 5G, --jobs 4), exit 0
- read the whole diff hunk by hunk: zz-protocol/src/message.rs, zz-daemon/src/daemon.rs, zz-tui/src/{app,input,state,tty}.rs, zz/tests/cli_binary.rs, compat/tui-caps.sh, compat/tui-mouse.sh (1294 lines end to end), compat/tui/campaign.json, compat/tmux-gaps.json, knowledge/
- wire rule: PROTOCOL_VERSION is 102 unchanged; InputMessage::MouseKey is a pure end-append after DismissClientMessage; the consumer half is in the same commit 8b81c2fd; the v102 entry of knowledge/protocol/wire-protocol.md names the append
- zone audit on git diff origin/main...HEAD: three excursions, all declared; crates/zz/src is untouched
- no added code comments, no attribution trailers, no .log files, nothing ignored, review worktree clean at the tip
- python3 compat/tui/tracker.py check and python3 compat/tmux-tracker.py check, both exit 0 with current reports
- compat/tui-mouse.sh at tip, exit 0, 13 asserted / 17 recorded, byte-identical to the committed run; --self-check exit 0, 7 sabotages caught in their own channel, 3 controls quiet
- two reviewer sabotages of its own on asserted channels the fixture does not sabotage (paste-into-prompt/row, paste-under-menu/option), both caught, plus a quiet control
- compat/tui-caps.sh at tip, exit 0, 287 asserted / 15 recorded, byte-identical to the committed run
- compat/attached-client.sh at tip, three runs: FAIL once at the POPUP_C step, then PASS, PASS
- 22 delta corpus rows in two chunks, selected by grepping compat/scenarios for every string the landing removes or changes, all green first pass
- cargo test -p zz-tui -p zz-protocol, -p zz, -p zz-daemon --lib and --tests, and cargo clippy over the four packages, all exit 0
- five oracles against the pin: the mouse arming through #{mouse_all_flag}/#{mouse_button_flag}/#{mouse_standard_flag}/#{mouse_sgr_flag}, the arming bytes through pipe-pane, the bound-event context, the status locations beyond a window range, and the copy-table mouse key
- pin source read against the implementation: server-client.c, tty.c, menu.c:592, cmd-display-menu.c:106, key-string.c:347-351, tmux.h:271-285

## What this gate did about the must-fix

ONE must-fix, applied in its own commit (49c0b036, "Leave a MENU_NOMOUSE menu's
arming to the mouse option"):

1. `crates/zz-tui/src/app.rs` `desired_mouse_arming` now reads
   `MenuState::mouse_keys` instead of returning `Some(true)` for any menu. The
   pin's rule, read at the source the reviewer names: `menu.c` `menu_prepare`
   raises `MODE_MOUSE_ALL|MODE_MOUSE_BUTTON` on the menu's own overlay screen
   only behind `~md->flags & MENU_NOMOUSE`, and `cmd-display-menu.c:378` sets
   that flag when `!event->m.valid && !args_has(args, 'M')`.
2. The doc comment above it now names the `-M`/invoking-mouse-event condition
   instead of claiming every menu carries `MODE_MOUSE_ALL`.
3. A unit test, `a_nomouse_menu_leaves_the_arming_to_the_mouse_option`, pins all
   five states the branch can reach: Button and Any under `mouse on`, Off and
   Any under `mouse off`, and Any for a NOMOUSE menu under
   `focus-follows-mouse`.
4. `compat/tui-caps.sh` gains four driven cases (`menu/nomouse`,
   `menu/mouse-keys`, and each with `mouse off`) and three self-check cases. The
   menu is raised with a real `C-b E` driven onto each client's own stdin
   through the outer decoder, never through a command, and the whole 16-row mode
   tuple is compared with nothing recorded.
5. Decision (2) in TUI-008's evidence_note and the matching sentence in
   TUI-009's evidence_note now carry the `MENU_NOMOUSE` qualification.

## The reviewer's probe, re-run at the gate

`gate-menu-arming-probe.txt` is the reviewer's own measurement rebuilt as a
script: outer pinned tmux as decoder, both clients attached with `mouse on`, the
same `bind -T prefix E display-menu -x 4 -y 8 -T PROBEMENU ...` on both sides,
`C-b E` driven into each client, reading each client's own outer pane
`#{mouse_all_flag} #{mouse_button_flag}`. At the fixed tip:

```
ok    baseline (mouse on, no overlay): both 0 1
ok    menu without -M (MENU_NOMOUSE): both 0 1
ok    menu with -M: both 1 0
ok    after (mouse on, no overlay): both 0 1
ok    mouse off, no overlay: both 0 0
ok    mouse off, menu without -M: both 0 0
ok    mouse off, menu with -M: both 1 0
```

The reviewer measured zz 1 0 against the pin's 0 1 on the second line. Two rows
beyond what the reviewer measured are in there: with `mouse off` the pin's plain
menu arms nothing and `-M` still arms, which is what
`server_client_reset_state` does when the overlay's own screen mode is all the
mode there is.

`gate-menu-arming-probe-sabotage.txt` is the same probe with the `-M` menu given
to zz alone, to show the channel is not vacuous. It reproduces the reviewer's
exact numbers and nothing else moves:

```
DIFF  menu without -M (MENU_NOMOUSE): pin 0 1, zz 1 0
DIFF  mouse off, menu without -M: pin 0 0, zz 1 0
```

`gate-menu-arming-mode-tuple.txt` is the same probe reading all sixteen mode
rows instead of two. Every row agrees on both sides in every menu state,
`cursor_flag` going to 0 under a menu included, which is why the fixture case
compares the whole tuple and records nothing.

## Sibling flips

None. This lane carries no sibling_cases.

## Obligations: both stay active

VERIFIED MEANS EVERY CLAUSE, and neither obligation meets it. The reviewer's
`every_clause_asserted` is "no" and the gate confirms the dispositions rather
than the prose:

- **TUI-008** holds 17 recorded cases inside all three of its acceptance
  clauses. Eight are the pin's stock root bindings, which zz still does not
  install (`list-keys -T root` answers 27 rows on the pin and 0 on zz); two
  paste cases record `input.rs handle_paste` writing through an overlay; one
  records the focus-delivery gate; one records the mouse context formats, which
  live in `crates/zz-mux/src/formats.rs`. The dispositions are fixed in the
  fixture's own disposition block, never decided at runtime.
- **TUI-009** holds 15 recorded rows: `widths/non-utf8/line` and
  `silent/extended pane_key_mode` in clause 1, and thirteen `client_colours` and
  `client_termfeatures` rows in clause 2. Clause 3 asserts with nothing behind
  it. Each of the 15 names its cause and whose zones own it; all three blockers
  are accessors in `crates/zz-daemon/src/client.rs` and `terminal_features.rs`,
  which this batch's zones do not open.

Both feed TUI-012, and both are still active going into the superset gate.

## Nits the reviewer raised and what happened to them

1. Byte-identical runs. Still true, and now measured at the gate too: three
   `tui-mouse` runs at md5 9ae19cea and four `tui-caps` runs at md5 7551111f.
   No per-run stamp added; that is a fixture change beyond the must-fix.
2. Sabotage coverage 7 of 13 asserted `tui-mouse` checks. Unchanged. The
   reviewer sabotaged two of the uncovered channels by hand and both were
   caught, so the machinery reaches them.
3. `keys.copy-mode-native-mouse`'s remeasurement sentence. Left as written; the
   gap keeps all 14 items either way.
4. `input_mouse_key` has no unit test. Unchanged; its coverage is
   `compat/tui-mouse.sh`'s three binding cases end to end.
5. The stale-mirror risk in the client's ROOT table. Unchanged, and written into
   the `mouse_binding_names` doc comment.
6. `focus_events_option` returns false for a non-Local endpoint. Unchanged;
   copied from the existing `extended_keys_option`, so it is the crate's shape.

## Reds and flakes at the gate

- `compat/tui-stock-keys.sh` failed 3 of 4 runs, every time on the same channel:
  the `detach` case compares the whole screen after the client exits, and the
  `remain-on-exit` line "Pane is dead (status 0, <date>)" carried a wall-clock
  second that the two sides straddled (09:47:44 against 09:47:45, 09:50:08
  against 09:50:09, 09:54:01 against 09:54:02). Run 3 was green solo and
  `gate-tui-stock-keys.txt` holds the last run. This is the documented
  fragility, same channel as its sibling `root-binding-detaches`; nothing in
  this lane's diff can reach a pane's death timestamp. The rate is high enough
  to be worth normalising the timestamp in that comparison, which is a fixture
  change for whoever owns tui-stock-keys next.
- `compat/attached-client.sh` PASSED on its first run here, 6m42. The reviewer
  saw one FAIL then two PASSes; four of five greens across the two of us.
- `cargo fmt --all -- --check` exits 1 on `crates/zz-tui/src/mode_view.rs:174`.
  `git diff origin/main -- crates/zz-tui/src/mode_view.rs` is empty at this tip,
  so the drift is main's own and no lane's to fix inside its zones.
- The delta corpus is 190 rows for `bind-key,unbind-key,set-option`. 185 ran
  clean, `known/known-terminal-runtime` and `known/known-pane-scrollbar-columns`
  each carried their exact documented divergence, and `lane2-store`,
  `show-options-hooks` and `smoke/plugin-runtime-resurrect-restore` are the
  box's documented environmental rows. `smoke/status-background-jobs`, which is
  red for another lane on this box, is green here.
