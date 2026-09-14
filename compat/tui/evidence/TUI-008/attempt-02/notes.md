# TUI-008 attempt-02, cycle 8, the keys lane

Alienware, CachyOS, against pinned tmux d77c9dc6. `environment.txt` carries the
box, both binaries, the tip and the base every file below was produced at.

## What this attempt was for

Cycle 7 built `compat/tui-mouse.sh` and left seventeen recorded checks behind
it. Cycle 8's punch list named them and their causes. This attempt takes the
ones the raw TUI can reach without the pin's pane-body commands, and says
plainly which it cannot.

`tui-mouse.sh` now asserts 24 checks and records 10, three runs byte-identical
at md5 `2236b141`, and its `--self-check` drives twelve one-sided sabotages and
three controls, each caught in its own channel.

## The files

- `environment.txt` - the box, both binaries, the tip, the base.
- `tui-mouse-run-1.txt`, `-2`, `-3` - three runs at the tip, identical.
- `tui-mouse-self-check.txt` - twelve sabotages, three controls.
- `tui-stock-keys.txt`, `tui-stock-keys-self-check.txt`
- `attached-client.txt` - PASS.
- `tui-copy-mode.txt` - 147 cases, none recorded.
- `tui-screen-diff.txt`, `tui-screen-diff-self-check.txt`
- `corpus-delta.txt` - all 223 delta corpus rows, run in fifteen calls.
- `tui-caps.txt`, `tui-indicators.txt`, `tui-pane-geometry.txt`,
  `tui-overlays.txt`, `tui-choosers.txt`, `status-row.txt` - the TUI fixtures
  this batch does not name, run because the landing changes focus delivery,
  paste routing and the root key table.
- `cargo-test-protocol-mux.txt`, `cargo-test-daemon.txt`,
  `cargo-test-tui-client.txt`, `cargo-test-zz.txt`
- `cargo-clippy-and-fmt.txt` - clippy over the six touched crates with
  `-D warnings`, and `cargo fmt --all -- --check`.

## What landed, item by item

### 1. The pin's stock mouse bindings, five of the twenty-seven

`KeyTables::default()` installs `MouseDown1Border { select-pane -M }`,
`MouseDown1Control8 { resize-pane -Z }`, `MouseDrag1Border { resize-pane -M }`,
`WheelDownStatus { next-window }` and `WheelUpStatus { previous-window }` in the
shared root table. `list-keys -T root` prints those five rows in the pin's own
printed spelling on both binaries, and each one runs.

`MouseDown1Status { switch-client -t= }` was installed and taken back out: the
pin's `cmd_switch_client_exec` special-cases `strcmp(tflag, "=") == 0` into
`CMD_FIND_PANE` and then sets the session's current window and active pane from
it, and zz's `switch-client` does not, so the status click never settled on the
zz side and the run stalled. Recorded with that cause; `status-clicks/window-after-name-click`
still asserts through the raw TUI's own `select-window` on a window range.

`next-window` and `previous-window` had to move: `cmd_select_window_exec` steps
from `s->curw` and zz's `step_window_in_session` preferred the execution
context's window, which is the same window on every path EXCEPT a mouse binding,
where the daemon retargets the context to the window the pointer landed on. A
wheel down over the status range of window 1 therefore stepped 1 to 0 on zz and
0 to 1 on the pin. It now reads the session's active window, as the pin does.

### 2. The gestures the client could not form

The border hit test was already there from cycle 7; what was missing was the
LATCH. `server_client_check_mouse` resolves a location on the press and
`c->tty.mouse_drag_flag` keeps every drag and the release on the same pane and
the same location until the button comes up. Without it a drag that starts on a
divider and moves eight cells into the pane resolves as `MouseDrag1Pane` and the
border gesture is lost. `input.rs` now stores the press's location, pane, window
and divider axis and reads them back for the drag and the release.

The click counter did NOT land. It is not a counter on its own: the pin's
`SecondClick` fires on the second press, `TripleClick` on the third, and
`DoubleClick` only when a 300 ms `KEYC_CLICK_TIMEOUT` expires with no third
press, and the gestures it produces are useless until `copy-mode -H`,
`send -X select-word` and `copy-pipe-and-cancel` run from a root binding.
`multi-click` stays recorded with that cause.

### 3. Bound-event context

`ExecutionContext` carries a `MouseEventTarget` - the pane, the window, the cell
and the divider axis the event landed on - and `execute_without_alias_expansion`
resolves a bare `=` or `{mouse}` in a `-t` or `-s` slot to that event's id
before the command runs, which is `cmd_find_target`'s answer from
`cmdq_get_event(item)->m`. Nested command blocks re-enter the same choke point,
so a `-t=` inside an `if-shell` body resolves too. With no mouse event in the
tree the spelling stays the quiet miss both binaries already answered with.

`resize-pane -M` is a supported flag and resizes the pane whose border the drag
grabbed to the cell the event landed on, along the axis the client latched.

Three of the eight mouse formats answer: `mouse_pane`, and `mouse_x` and
`mouse_y` as the event's cell inside the pane, which is what `cmd_mouse_at`
measures. The registrations moved from a constant backing to the delegated one
so the manifest partition names them as behavior.

### 4. Paste and focus

A bracketed paste to a pane the client holds a mode on is dropped, which is
`window_pane_paste`'s first line: `if (!TAILQ_EMPTY(&wp->modes)) return;`. The
text no longer appears behind the mode when the mode is left.

The daemon no longer gates the DELIVERY of a focus report on `focus-events`. The
pin gates only the arming, in `tty_start_tty`, and hands an arriving report to a
pane that asked for it whatever the option says.

`paste-under-menu` stays recorded: the pin's overlay takes the paste key and
each character behind it and the pane sees the tail as ordinary keys, so its
screen reads `$ sted-text~` where the raw TUI hands the tail to the pane as a
fresh bracketed paste.

### 5. The detach timestamp

`compat/tui-stock-keys.sh`'s two detach cases compare the outer pane's screen
after the inner client exits, and `remain-on-exit-format` paints
`Pane is dead (status 0, #{t:pane_dead_time})` over it. The two clients exit one
after the other, so the second straddles whenever the run crosses one, and cycle
7's gate got one green in four runs on that alone. `capture_screen` now rewrites
the time inside that message and nothing else; the words, the exit status or
signal, the styling and every other cell still compare. Its self-check gained a
control - both sides detached, no difference reported - and a sabotage: zz's
attach wrapper exits 3 instead of 0, and the fixture reports
`status 0` against `status 3` with the time normalised on both.

## The ten checks that still record

- `wheel-up-pane/pane-in-mode` and `/pane-mode`. `WheelUpPane` is
  `if -F '#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}' { send -M }
  { copy-mode -e }`. Two of its three pieces are missing: `send-keys -M` is a
  catalogued unsupported flag, and `#{mouse_any_flag}` is a constant zero in
  `crates/zz-mux/src/formats.rs`, so installing the binding today would enter
  copy mode on a wheel over a program that asked for mouse tracking, which is
  worse than the raw TUI's own scroll. Owner: TUI-008, in
  `crates/zz-mux/src/command.rs` (`send_keys`) and `formats.rs`.
- `drag-selects/buffer`. `MouseDrag1Pane` falls through to `copy-mode -M`, and
  the selection it starts belongs to the copy table's own `MouseDrag1Pane` and
  `MouseDragEnd1Pane`. A copy-table mouse name is still unreachable from a
  pointer: `crates/zz-tui/src/app.rs mouse_binding_names` reads the ROOT table
  only, because the client has to know a name is bound before it hands the
  gesture over. Owner: TUI-008, in `app.rs` and `input.rs`.
- `multi-click/double-buffer` and `/triple-buffer`. The click sequence above.
  Owner: TUI-008, in `input.rs` for the sequence and `command.rs` for
  `copy-mode -H` plus `send -X select-word`/`select-line`.
- `right-click-pane/screen` and `status-clicks/right-click-screen`. Both are
  `display-menu -t= -x M -y M` over the pin's DEFAULT_PANE_MENU and
  DEFAULT_WINDOW_MENU: twenty-eight and eleven items over `#{m/r:}`,
  `#{=/9/...:}`, `buffer_sample`, `mouse_word`, `mouse_line`,
  `mouse_hyperlink`, `pane_floating_flag` and a nested `display-menu`. Owner:
  TUI-008, in `command.rs` (`display_menu`'s `-x M`/`-y M`) and `formats.rs`.
- `paste-under-menu/screen`. Above. Owner: TUI-008, in
  `crates/zz-client/src/menu.rs` and `crates/zz-tui/src/input.rs`.
- `drag-selects/pane-in-mode` and `drag-selects/selection` already AGREE on both
  sides and are counted as recorded only because they share the drag case's
  disposition with `drag-selects/buffer`, which does not. They flip with it.

## The corpus

All 223 rows of `--delta origin/main..HEAD` over `list-keys`, `next-window`,
`previous-window`, `resize-pane`, `list-commands`, `display-message`,
`set-option`, `select-window`, `send-keys` and `copy-mode` ran; 217 clean.
Three `known/` rows carry their exact documented divergences. Three of this
box's four documented environmental rows came up red and the fourth ran clean.
`smoke/status-background-jobs` went red once under load and green on each of
three solo re-runs; its assertion samples a `#(date +%s%N)` status job at one,
three, five and seven wall-clock seconds and the three-second sample read
`DATE[]` with the job's output empty, which nothing in this diff can reach.

Every zz-only string this landing could have taken away was grepped for first:
`list-keys -T root` is asserted by `list-keys-padding`, which clears the root
table before it reads it and is clean; the `unsupported command: resize-pane -M`
text nothing asserts; and the `list-commands` usage line the flag change moves
is asserted by `command-item-format` and `daemon-command-item-format`, both
clean.

## Gap items closed, each with its measurement

Nine, all inside this batch's four gaps, each with a dated 2026-09-13
measurement in `compat/tmux-gaps.json`:

- `keys.root-native-mouse`: the five root rows above.
- `mouse.bound-context`: `flag:resize-pane:-M` and
  `semantic:display-message-bare-mouse-target`.
- `formats.mouse-context`: `format:mouse_pane`, `format:mouse_x`,
  `format:mouse_y`.

`keys.copy-mode-native-mouse` keeps all fourteen: no copy-table mouse name is
reachable from a pointer yet.

## Zone excursions

- `crates/zz-mux/src/command.rs step_window_in_session`, outside the batch's
  "mouse key tables, mouse key dispatch and -M flags" scope in that file. Forced
  by the measurement in item 1 above: with `WheelDownStatus { next-window }`
  installed, a wheel over window 1's status range answered window 0 on zz and
  window 1 on the pin, because zz stepped from the context's window and
  `cmd_select_window_exec` steps from `s->curw`.
- `crates/zz-mux/src/formats.rs` is in the zones for the eight `mouse_*`
  formats; the three registrations that moved are exactly those.
- `crates/zz-mux/src/compat_manifest_tests.rs` and
  `crates/zz-mux/tests/hunt_claims.rs` carry the partition counts and the two
  claims the landing moves, in the same commits.
- `crates/zz-daemon/src/status.rs`, one arm, so the three delegated mouse
  formats answer empty from the facts path when no mouse event is in the tree,
  which is what the pin's NULL expands to. Forced by
  `daemon_delegated_format_consumers_match_mux_inventory`.

## The GUI

The GUI sends no mouse key and its pointer handling is untouched. One product
decision reaches it: the daemon no longer swallows a focus report when
`focus-events` is off, so a GUI pane that armed `\e[?1004h` now receives one on
window focus. That is the pin's rule for a report that arrives - `tty_start_tty`
gates the ARMING and `window_pane_key` hands an arriving report to the pane -
and the GUI has no arming handshake to gate. A GUI-side arming gate reading
`focus-events`, in `crates/zz/src/terminal/view.rs`, is the follow-up; the raw
TUI already reads the option in `tty.rs` and arms only when it is on.

## Gate addendum, 2026-09-14 (cycle 6 keys gate)

Files the gate added to this directory:

- `review.md` -- the reviewer's verdict JSON verbatim, their checks_run and
  notes, and what the gate did with each must-fix.
- `gate-tui-mouse.txt` -- compat/tui-mouse.sh at the merged tip, exit 0,
  26 asserted / 10 recorded (the lane's 24 plus the border-click case's two).
- `gate-tui-mouse-self-check.txt` -- --self-check at the merged tip, exit 0,
  fourteen sabotages each caught in its own channel and three controls quiet.
- `gate-tui-stock-keys.txt` -- exit 0 on its first run, 50 cases agree.
- `gate-attached-client.txt` -- PASS.
- `gate-status-background-jobs-baseline.txt` -- the one red the gate chased:
  three solo runs red at the tip, nine runs at origin/main 289c8a7c in the same
  worktree and target dir, 2 green and 7 red in both failure spellings.

Excursion the lane's list did not name, added here on the reviewer's nit 5:
`crates/zz-mux/src/lib.rs` carries a single re-export line for
`MouseEventTarget`, whose type lives in `command.rs`, which is in zone.
