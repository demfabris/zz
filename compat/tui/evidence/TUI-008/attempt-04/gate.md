# Cycle 9 gate of the menus lane: HELD, not merged

Ubuntu box, 2026-09-15, the cycle-9 gate that took campaign/tui-mouse-menus-2 (6fbdf4c3) and
campaign/tui-introspection together. The introspection half went to main. THIS half did not: it
carries a deterministic corpus regression, found below, that no fixture on the branch drives. The
branch is pushed as `campaign/tui-mouse-menus-2-gated` with the review applied, so the next gate
starts from the fixes rather than from the review.

Base: origin/main 287e3815. attempt-04/review.md is the verdict this gate applied, copied in beside
the runs it names.

## Why it is held

`smoke/plugin-runtime-oh-my-tmux` fails at this tip and passes at origin/main. Six runs of the row
alone, alternating binaries on the same box, same pin d77c9dc6, same corpus:

    this tip   exit 1, exit 1, exit 1   WARN clean? no
    origin/main exit 0, exit 0, exit 0  WARN clean? yes

The divergence is one extra line on zz's control-mode stdout for the config step, which the pin
never prints:

    cfg.in:25: unsupported command: break-pane -W

The cause is this lane's own landing. `list-keys -T root` at this tip carries two bindings the pin
carries and main did not have - `MouseDown3Pane` and `M-MouseDown3Pane` over the pin's
`DEFAULT_PANE_MENU` - and that menu's Float item is `{ break-pane -W }`. `break-pane -W` is
`unsupported_flag` in zz's catalog on purpose: the five `break-pane` placement flags are an accepted
gap under the floating-pane group in compat/tmux-gaps.json, because a floating pane is a mux object
in the pin and a presentation object in zz. The pin implements `-W`, so the pin is silent. Something
on zz's config-load path parses the bound menu item's flags, which is exactly what commit 1ab5fdcf
("Resolve a menu item's command names without parsing its flags") says must not happen until
`menu_key_cb` runs: "the string is kept as written and menu_key_cb is the first thing that parses
its flags, which is how the pin's own DEFAULT_PANE_MENU carries move-pane -P". The names are
resolved correctly; the flags are not being left alone on every path.

Measured facts the next lane can start from, all on this box at this tip:

- `break-pane -W` invoked directly is refused identically by the main binary and this tip's binary,
  so the catalog entry is not what moved; the binding is.
- `list-keys -T root | grep -c 'break-pane -W'` is 2 at this tip and 0 at origin/main.
- `set -g mouse on`, `set -g prefix C-a`, `set -g prefix2 C-a`, `set -g @x 1` and
  `bind -n MouseDown2Pane paste-buffer` sourced on their own do NOT reproduce it, so the trigger is
  not a plain option write or a plain root rebind.
- The row's own reproduction outside the harness needs the staged smoke HOME; a bare source of the
  corpus `.tmux.conf` stops earlier, on both binaries alike, at the `run-shell` that sources
  `.tmux.conf.local`.

The gate did not fix this: it is code in the lane's own zone that the review did not mandate, and
proving a fix needs the whole compat/tui-mouse.sh suite plus the display-menu corpus rows re-run,
which is the lane's next attempt, not a gate edit.

## What this gate applied from the review

### The blocker, as a recorded check

`compat/tui-mouse.sh` `case_right_click_pane` now drives the identical stock gesture a second time
at pane offset (2,1), one row up onto the shell prompt row, where `right-click-pane/screen` aims at
the blank cell (6,4). The new check is `right-click-pane/over-a-word`, `RIGHT_CLICK_WORD_MODE=record`,
and its reason names formats.mouse-context with format:mouse_word, format:mouse_line and
format:mouse_hyperlink. Same driver as its neighbour: `send_mouse_both 2 <col> <row> M`, a bounded
wait for the menu on both screens, a settle, `capture-pane -p -e` through the outer pinned tmux at
80x24. The summary line therefore reports 1 recorded check and TUI-008 stays at review with proof
null. TUI-012 is untouched and stays held.

### The must-fix, in code

`popup_position_variables` in crates/zz-daemon/src/daemon.rs no longer seeds `popup_mouse_x`,
`popup_mouse_y`, `popup_mouse_centre_x`, `popup_mouse_centre_y`, `popup_mouse_top`,
`popup_mouse_bottom`, `popup_window_status_line_x` or `popup_window_status_line_y` with the screen
centre. `cmd_display_menu_get_pos` adds the mouse names only under `event->m.valid` and the window
status names only when the target window owns a `STYLE_RANGE_WINDOW` range on the target client's
status, so an absent one expands empty and `strtol` reads it as 0. `popup_status_line_y` is now the
pin's own rule instead of the centre: `lines + h` under `status-position top` and `tty->sy - lines`
otherwise, and it is absent when the session has no status lines.

The reviewer's probe, 80x24, status at bottom, two windows, a small menu, driven from a command
client with no key event, captured through an outer pinned tmux. Full output in
24-menu-position-probe-before.txt (origin/main's binary) and 25-menu-position-probe-after.txt (this
tip). The box row/column is where the menu's first item lands:

    -x M -y M   before: zz 10/33 vs pin 1/0, 8 of 24 rows differ
                after:  zz  1/0  vs pin 1/0, 0 rows differ
    -x C -y S   before: zz 10/33 vs pin 20/33, 8 of 24 rows differ
                after:  zz 20/33 vs pin 20/33, 0 rows differ
    -x C -y C   before and after: identical, 0 rows differ
    -x W -y W   before: zz 10/33 vs pin 20/1, 8 of 24 rows differ
                after:  zz  1/0  vs pin 20/1, 8 of 24 rows differ

Two of the three are closed. `-x W -y W` is NOT, and this gate did not close it, because it cannot
be closed where the review put it. `cmd_display_menu_get_pos` reads `sr->start` off
`tc->status.entries[line].ranges`, the TARGET CLIENT's own laid-out status row. zz's daemon has no
such thing: it publishes the status row as `#[...]`-marked-up strings and the client composes the
hit ranges itself, in `zz_client::compose_status_row`, which is why `status_range_start` had to be
added to `InputMessage::MouseKey` in the first place. Filling `popup_window_status_line_x` daemon-side
needs either the client to publish its ranges (a wire change this cycle's contract does not have) or
zz-daemon to depend on zz-client and re-lay-out the row (backwards layering and a refactor no review
asked for). What the fix does reach is the pin's own `sr == NULL` branch: zz now answers 0 rather
than inventing the screen centre.

So TUI-008's evidence_note is narrowed to the event-driven positions and the command-line `-x W -y W`
case is recorded there with the measurement above, which is the first of the two resolutions the
review offered. The asserted fixture case the second resolution would have needed is not written:
the fixture has no command-client driver shape (a `display-menu` from a command client blocks until
the menu is dismissed, so it has to be backgrounded and dismissed through the outer client), and
building one was past the 40 minutes the gate had for it.

### The nits

- `RIGHT_CLICK_REASON` is emptied; the over-a-word check carries its own reason.
- "twenty one-sided sabotages" is now "twenty-two" in TUI-008's evidence_note and in
  attempt-04/notes.md. Measured at this tip: `compat/tui-mouse.sh` has 25 `self_check_case` calls,
  three of them controls and twenty-two sabotages. The review's parenthetical "(26 self_check_case
  calls)" is one too many; three controls plus twenty-two sabotages is 25 and that is what the file
  holds.
- compat/tmux-gaps.json is re-serialised with `ensure_ascii=False`. Six `—` escapes in four
  entries no lane owns are gone, and `git diff origin/main...HEAD -- compat/tmux-gaps.json` now
  changes exactly one gap id, `keys.root-native-mouse`.
- attempt-04/17-verify-claims.txt is NOT replaced with a `--run TUI-008` output. The gate stopped
  before its fixture pass when the corpus regression above came back deterministic, so there is no
  honest `--run` output to put there. It stays as the lane left it.

## What this gate proved, and what it did not

Proved at this tip (which is menus + the three commits above, on origin/main 287e3815):

- `cargo test -p zz-protocol -p zz-mux -p zz-terminal -p zz-tui -p zz-client`: exit 0.
- `cargo test -p zz-daemon` (empty HOME, `--skip russh_socks`): exit 0, 906 lib tests.
- `cargo test -p zz` (empty HOME): exit 0, 642 lib and 125 cli_binary.
- The menu position probe above.
- Corpus, 42 of the 211-row selection this gate drew: chunks 0 and 1 clean, chunk 2 clean but for
  the row above.

NOT proved, because the gate stopped: the remaining corpus chunks, the fixture pass
(compat/tui-mouse.sh three times plus --self-check and the other twelve fixtures),
compat/attached-client.sh, compat/status-row.sh, verify-claims --run TUI-008, compat/check.sh.

Note on the two clippy and rustfmt reds this gate met: both are origin/main's, not this branch's.
`crates/zz-daemon/src/russh_prompt.rs` is byte-identical to origin/main's and carries all seven
`-D warnings` errors (duration_suboptimal_units, default_trait_access) from main's 5377ae99;
`rustfmt --check` on origin/main's own crates/zz-protocol/src/catalog.rs flags the same `-P` line it
flags here. Neither is charged to a lane.
