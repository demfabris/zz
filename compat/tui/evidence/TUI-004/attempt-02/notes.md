# TUI-004 attempt-02: acceptance clause 3

Cycle 4, the canvas-close lane. Clauses 1 and 2 keep cycle 3's evidence in
`../attempt-01/`; nothing here re-runs their matrix. This attempt is clause 3:

> Compare copy/view/prefix indicators and message/prompt restoration without
> waiving cells occupied by zz hints. Turn the three recorded theme differences
> into asserted comparisons.

## The files

| file | what ran |
|---|---|
| `environment.txt` | both binaries' sha256, the revision, the pin, the OS line, TERM, shell, locale, outer size. Read this first. |
| `indicators-before-the-render-fix.txt` | `compat/tui-indicators.sh` at the tip the fixture was written against, BEFORE the two render.rs changes: 4 of 11 asserted comparisons differ. This is the measurement the clause exists to produce. |
| `indicators-run-1.txt` | the same fixture after the two render.rs changes: exit 0, all 11 asserted comparisons identical, 5 recorded. |
| `indicators-self-check.txt` | `compat/tui-indicators.sh --self-check`: exit 0, three sabotages each caught, one equivalence not reported. |
| `screen-diff-regression.txt` | `compat/tui-screen-diff.sh`: exit 0, all 111 asserted checkpoints identical, 42 recorded. The render.rs cursor change touches every checkpoint of that fixture, so it ran before the commit and not only after it. |
| `pane-geometry-regression.txt` | `compat/tui-pane-geometry.sh`: exit 0, all 6 asserted measurements identical. |
| `status-row-regression.txt` | `compat/status-row.sh` under `LC_ALL=C LC_TIME=C`: exit 0, all 11 comparisons identical, 3 rows recorded. |
| `zz-tui-unit-tests.txt` | `cargo test -p zz-tui`: 173 passed. |
| `proofs-at-tip.txt` | every proof command re-run at the final tip, with exit codes. |

## What the fixture found, and what was done with each finding

Five channels, driven identically on both sides inside one outer pinned tmux,
compared whole-screen plus the cursor tuple at named settled checkpoints.

**1. The armed prefix. FIXED.** The pin paints nothing on the status row while
the prefix is armed: status.c redraws the row from the same formats it always
uses and no default format reads `#{client_prefix}`. The raw TUI painted
` PREFIX ` over the right of the row, so the two screens differed in eight cells
for as long as a user held C-b. `crates/zz-tui/src/render.rs` now splits
`status_indicators` in two: `mode_indicators` (the copy/view/search badges) is
what the status ROW overlay carries, and `status_indicators` (those plus the
hint) is what the SIDEBAR's indicator row carries. The sidebar is client-local
chrome with no counterpart in the pin, which only `focus-sidebar` or a user
binding shows, so the hint keeps a home. `prefix-armed` and `prefix-released`
are asserted whole and identical.

**2. The cursor under a message. FIXED.** With `display-message` up, the pin
reports `cursor_flag=0` at the PANE cursor's own position (`2,22`) and the raw
TUI reported `1` at `79,23`, where its last status-row write had left it. Two
differences in one channel: visibility and position.
`server_client_reset_state` (server-client.c:2073) drops `MODE_CURSOR` for a
client whose status screen is the one being drawn and which has no prompt, and
still calls `tty_cursor`, so the hidden cursor stays where the pane's cursor is.
`place_active_cursor` now places the pane cursor first and hides it after, under
a `client_message_hides_the_cursor` guard that mirrors `status_overlay`'s: with
the sidebar up and no command output the message is a sidebar row and the status
row is untouched, so the cursor is not that message's to take. A prompt keeps
its cursor, which is why the check sits after the prompt branch.

**3. The message and prompt STYLE. RECORDED, with the field it would need.** The
message row's and the prompt row's glyphs, columns and cursor are the pin's on
both sides. The style is not: the pin resolves `message-style`
(`bg=themeyellow,fg=themeblack`, options-table.c:941) and `message-command-style`
(options-table.c:910) and paints `\e[38;2;13;13;13m\e[48;2;184;134;11m`; the raw
TUI paints its own overlay appearance, `\e[38;2;16;19;24m\e[48;2;216;222;233m`,
and never reads either option. This is not a render bug that render.rs can fix:
the daemon publishes no resolved message style on `StatusLine`, so the client has
nothing to dispatch on. THE FIELD IT WOULD NEED: `StatusLine` gains the resolved
`message-style` and `message-command-style` beside the theme table, expanded by
`crates/zz-daemon/src/status.rs` the way the status row's own base style already
is, and `status_overlay` dispatches on them instead of `overlay_style`. That is a
protocol append and this lane's declared arm was the theme one, so it is left
measured. Three cases run in `text` mode: every glyph, every column and the
cursor asserted, the styles recorded with that reason.

**4. Copy mode. RECORDED against an accepted gap.** window-copy.c:5228 draws
`copy-mode-position-format` into the PANE's grid at the top right whenever the
mode screen's first line is written and `hide_position` is off, and leaves the
status row alone. Measured: the pin paints `[0/45]` at columns 74..79 of row 0 in
`copy-mode-position-style`, and its cursor sits in the pane at the copy cursor;
the raw TUI paints nothing in the pane, a ` COPY 62/62 ` badge on the status row,
and hides its cursor. `#{copy_position}` and `#{copy_position_limit}` answer
empty on zz where the pin answers `0` and `45`. All of that is
`options.native-mode-styles`, the ACCEPTED gap that holds
`option:copy-mode-position-format`: zz renders native mode surfaces instead of
tmux cell grids. Neither the indicator's cells nor its numbers are this fixture's
to assert, and the badge is not waived either - it is printed, both sides, every
run. What IS asserted is `copy-mode-restored`: once the mode is cancelled the two
screens are the same screen again, style and cursor included.

**5. The view surface. MEASURED FIRST, then recorded against the same gap.** The
pin has no `view-mode` command. `window_view_mode` (window-copy.c:184) is reached
from cmd-run-shell.c:103 for the output of `run-shell`, from cfg.c:271 for a
config error and from server-client.c:3061; `copy-mode -e` is not a second
surface at all, it is copy mode with `scroll_exit` set (window-copy.c:616) and
both binaries answer `#{pane_mode}` `copy-mode` for it. So the view surface the
pin actually reaches is run-shell's, and that is what the case drives.
`run-shell -t pane 'printf VIEWLINE-1'` puts the pin's pane in view-mode with the
output as the whole screen and `[0/0]` at the top right; zz leaves
`#{pane_in_mode}` at 0 and draws a client-side command-output overlay with a
` VIEW 1/21 ` badge. Recorded; `view-surface-restored` is asserted.

**6. The three theme rows of `compat/status-row.sh`.** Still recorded. See the
obligation's `evidence_note` for what did and did not land there this cycle.

## The fixture

`compat/tui-indicators.sh`, 80x24, the outer-pinned-tmux driver copied from
`compat/status-row.sh` and widened from one row to the whole screen.

Two things in it are worth knowing before reading it:

- **It marks before it acts.** `tui-screen-diff.sh` ends a checkpoint by typing
  `printf 'MARK-%s\n' NAME` into the pane. Three of these five surfaces SWALLOW
  that keystroke: a pane in copy mode feeds it to the mode's key table, an armed
  prefix eats it, an open command prompt puts it in the prompt. So each case
  marks first, settles on the marker, and only then applies the state. The view
  surface is the exception the header names: it replaces the screen on both
  sides, so its checkpoint settles on the run-shell output instead.
- **The prefix is a real keystroke.** `send-keys` writes into the PANE, beneath
  the client's key table, so it can never arm a prefix. The case sends `C-b` to
  the OUTER pane, where the attached inner client reads it as a keystroke, and
  waits on `#{client_prefix}` against each client before comparing. The same path
  opens the command prompt: `command-prompt -t <client>` from a one-shot client
  does not return until the prompt is answered, which was measured by deadlocking
  a probe on it.

Declared dynamic values: `status-right ''`, `status-left L`,
`automatic-rename off`, a pinned pane title, the `ENV= PS1='$ '` inner shell,
`display-time` (held at 20000 ms for the during-comparison and dropped to 100 ms
for the restoration one, both sides), and `copy-mode-position-format` pinned on
both sides to the pin's default minus its leading `#{t/p:top_line_time}`, which
is a clock. Nothing else is masked.

`--self-check` plants one deliberate one-sided difference per channel and
requires each to be reported: a status row carrying a ` PREFIX ` label on one
side while the prefix is armed on both, a message whose text differs by one
character, and a prompt one side leaves open as residue. The fourth case is the
equivalence: the same prefix armed on both sides with nothing planted has to
report nothing at all, without which the three sabotages would be satisfied by a
comparison that always reports a difference.
