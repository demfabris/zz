# TUI-014 eighth pass, 2026-09-18 (lane modes-opus8)

Branch `campaign/tui-customize-4` rebased onto `origin/main` `394ef850` (release zz 0.11.1) and
pushed as `campaign/tui-customize-5`. The runtime under test is `target/debug/zz_cli`, copied to
`target/lane/zz_cli-final` and hashed before any run; `environment.txt` carries both binaries'
hashes and the fixture environment. `PROTOCOL_VERSION` stays 105 and this pass appends nothing to
the wire.

## BLOCKER 1, the pane modes' pointer: built, for both modes

The review asked for customize-mode's mouse to be built rather than parked, and pointed at the
`keys.copy-mode-native-mouse` precedent where the daemon looks a mouse key up in the mode's key
table. That precedent does not reach these two modes, and the pin says why:
`server_client_key_callback` swaps a mode's table in only when the mode declares one
(`wme->mode->key_table != NULL`). `window_copy_mode` declares `copy-mode`/`copy-mode-vi`;
`window_customize_mode` and `window_switch_mode` declare none. Their pointer rows are reached the
other way, and it is the path the root bindings already take: `MouseDown1Pane` runs
`select-pane -t = ; send-keys -M`, `send -M` is `window_pane_key`, and `window_pane_key` hands a
mouse key to `wme->mode->key` before any of the pane's own input. Building a synthetic key table
for these modes would have put rows in `list-keys` that the pin does not have, so the port follows
`window_pane_key` instead.

What now runs, against `mode-tree.c` and `window-switch.c`:

- `MuxEngine::customize_mouse` (`crates/zz-mux/src/command/customize.rs`) is `mode_tree_key`'s
  pointer half in its own order. An open prompt gets the key first, and only a button-1 press on
  the prompt's own row (`status-position`-aware) reaches `prompt_mouse`; anything else is
  NOT_HANDLED and falls THROUGH to the tree, which is why a click still moves the selection with
  the filter prompt open. An open help page swallows a pointer and stays open. Then the cell: a
  press with `x > width` or `y > height`, or on a row past the last line, does nothing;
  `MouseDown1Pane`, `MouseDown3Pane` and `DoubleClick1Pane` put `current` at `offset + y`; and a
  double click becomes `\r`, which is `window_customize_set` on an option row. The wheel and a
  drag are swallowed, because with a real mouse event in hand `mode_tree_key` turns every key that
  is not one of those three into `KEYC_NONE` - the `KEYC_WHEELUP_PANE` arm of its own switch is
  reachable only from `send-keys WheelUpPane`, which carries no mouse event.
- `SwitchMode::mouse` (`crates/zz-mux/src/command/switch_mode.rs`) is `window_switch_key`'s mouse
  branch: a button-1 press on the last row moves the prompt cursor, the wheel steps the selection
  one row and does NOT wrap, a press picks `offset + y` when it is inside the visible rows and
  inside the match list, and a double click picks and then runs the template.
- `ModePrompt::mouse` (`crates/zz-mux/src/command/mode_prompt.rs`) is `prompt_mouse` over the same
  scroll arithmetic `ModePrompt::draw` already used, which is now one set of width helpers.
- `Daemon::pane_mode_input` (`crates/zz-daemon/src/daemon.rs`) is the one dispatch for a key and a
  pointer, so a gesture that becomes `\r` runs exactly the tree key the keyboard runs.
  `mouse_pane_cell` is `cmd_mouse_at`, reusing the `pane_left`/`pane_top` resolution
  `#{mouse_x}`/`#{mouse_y}` already publish.
- The raw TUI forwards `MouseDown1Pane`, `MouseDown3Pane`, `DoubleClick1Pane`, `WheelUpPane` and
  `WheelDownPane` whenever the pane the pointer resolved to holds a mode
  (`pane_mode_mouse_key_is_reachable`, `crates/zz-tui/src/input.rs`), on both the live press and
  the `server_client_click_timer` replay. This is the same rule `copy_mouse_key_is_reachable`
  already applies for the copy tables, and it is load-bearing: the daemon is the side that runs
  `window_pane_key`, so a name with nothing bound to it still has to arrive. `compat/tui-mouse.sh`
  unbinds `MouseDown1Pane` and `WheelUpPane` in its own binding cases and never puts the stock
  bindings back, so every mode-pointer case in that fixture runs with those names unbound - which
  is exactly the state the pin still answers in.
- The replayed `DoubleClick`'s `m->ignore` moved out of `send-keys -M` and into the daemon's
  pane input. `input_key_mouse` is where the pin drops an ignored event, and `window_pane_key`
  reaches it only after a pane holding a mode has already answered the key, so dropping it in the
  engine hid the double click from both modes.

### The scope correction the review asked for

The review's own reading was that the pin shows NO mouse response inside switch-mode and that the
item therefore overstates itself by naming switch-mode. Measured here on a fresh 80x24 scene with
`mouse on`, two sessions and switch-mode open on pane 0, the pin moved its selection on every
press and every wheel press driven at it, and `window_switch_key`'s mouse branch in the pinned
source is the reason. So the item could not be dropped on that reading, and switch-mode's mouse is
built instead.

What is left, and what `semantic:mode-tree-mouse-menu` now names, is one row: `MouseDown3Pane`
selects the line under the pointer on BOTH binaries, and only the pin then opens
`mode_tree_display_menu` over it. Measured 2026-09-18 with a press-only gesture that left the
pin's six-item menu on eleven rows of its screen (`Select (Enter)`, `Expand (Right)`, `Tag (t)`,
`Tag All ([DC4])`, `Tag None (T)`, `Cancel (q)`) and nothing on zz's; the button release closes it
again on the pin, which is why a press-and-release gesture shows only the selection move. The item
left `semantic:pane-mode-mouse` behind with a dated resolution in `compat/tmux-gaps.json`, and
this is the one thing of TUI-014's own still parked in a gap. No fixture case here asserts or
records that menu: adding a recorded case for it would have put an ordinary record back on the
obligation for a divergence the gap already carries with its measurement.

### What asserts it

`compat/tui-mouse.sh` gained ten asserted checks in five cases, every one on the whole decoded
screen except the double click's own, which reads the client's session:

    customize-mouse-click/screen          a press at pane row 5 moves the selection and the
                                          preview title it names
    customize-mouse-double-click/screen   Right, then a double click on an option row, which is
                                          the pin's own `(backspace) C-?` edit prompt
    customize-mouse-wheel/screen          wheel up and down inside the tree change nothing
    customize-mouse-wheel/pane-mode       and leave `#{pane_in_mode}/#{pane_mode}` alone
    customize-mouse-preview/screen        a press below the tree's height changes nothing
    customize-mouse-prompt/screen         a press on the filter prompt's row moves the prompt
                                          cursor, read by typing `Z` and comparing where it landed
    switch-mouse-click/screen             a press picks the row under it
    switch-mouse-wheel-up/screen          the wheel steps one row up
    switch-mouse-wheel-down/screen        and one row down
    switch-mouse-double-click/client-session  a double click runs the template

The settle for a pointer in a mode is documented in the fixture: the mode's screen is the only
observable either binary offers for a selection, so each case waits a bounded two seconds for the
two screens to agree - which only gives the slower client time to repaint and cannot hide a
divergence, because the comparison is taken afterwards on a settled screen and a screen that never
agrees still fails - and then settles both sides on four consecutive identical captures. The
self-check gained two one-sided sabotages: zz's customize click aimed three rows higher, caught by
`customize-mouse-click/screen`, and switch-mode's wheel held back on zz only, caught by
`switch-mouse-wheel-up/screen`.

## MUST-FIX 2, `status-keys vi` in the mode prompts

`mode_tree_set_prompt` and `window_switch_init` both build their prompt through
`prompt_set_options`, so every prompt either mode raises - filter, search, option edit, key
command, key note, and the `u`/`d`/`D`/`U` single-key confirmations - keeps the raising session's
`status-keys` and `word-separators` for its whole life. zz read `word-separators` and not
`status-keys`.

`MuxEngine::prompt_key_options` is the existing `prompt_set_options` port the command prompt
already uses; `customize_prompt_options` and `pane_prompt_keys_are_vi` now resolve the pane's
session and read the same pair, and `ModePromptOptions` is the one place a mode prompt is built,
so no construction site can forget. `ModePrompt::translate_vi` carries `prompt_translate_key`
whole against `ModeKey`: insert mode passes the fixed control list through, takes Escape or `C-[`
into command mode with the cursor stepped back and appends everything else; command mode maps the
vi keys onto emacs keys and onto the three `KEYC_VI` word motions, with the big-letter variants
passing no separators. The motions are `prompt_forward_word`, `prompt_end_word` and
`prompt_backward_word` over `ModePrompt`'s own buffer, which already held the emacs halves.

The row's STYLE is part of it: `prompt_draw` picks `message-command-style` over `message-style`
while `PROMPT_COMMANDMODE` is set, and the measurement showed the pin's row in yellow-on-black.
The daemon sends that in `ChooserPresentation.prompt_style`, a field that already crosses the
wire, so no append was needed. The cursor style `prompt_draw` also swaps is not in a
`capture-pane` and is not claimed.

Literal reuse of the daemon's own `prompt_translate_key` was not available: it is written over
`zz_terminal::KeyInput` in and out, and bridging that to `ModeKey` both ways is lossy (`text`,
`unshifted_codepoint`) and would put the closed command-prompt work at risk for no observable
gain. The table is shared as a port, not as a call.

Twenty-two asserted `customize-prompt-vi-*` cases in `compat/tui-client-commands.sh` - twenty-one
drives and the restore that closes the mode - take the fixture's own tally from 495 to 517. They drive the
filter prompt (`f`, `abc`, Escape, `h`, `x`, `i`, `Z`, Enter, `c`), the search prompt (`/`,
`status`, Escape, `0`, `$`, `D`, `q`) and an option edit prompt (Enter, Escape, `0`, `w`, `b`,
`A`, `Z`, `C-c`), each on the decoded screen, the cursor and the state. The self-check sabotage is
one-sided: zz put back on `status-keys emacs` before the same Escape, which cancels the prompt and
changes the row.

`notes.md:97` of `attempt-18-modes-opus` claimed the search and filter prompts are unaffected by
`status-keys`. That line is corrected in place with a dated note: the filter prompt is exactly
where it diverged.

## MUST-FIX 3, the ledger proof block

`compat/tui/campaign.json`'s TUI-014 `proof` block is repointed at this pass: `revision`,
`environment`, `commands`, `artifacts` and `review` all name this runtime and this attempt, with
this pass's tallies and wire version.

## The nits

- `crates/zz-protocol/tests/hunt_claims.rs:17` is now `..._one_hundred_and_five`.
- `knowledge/protocol/wire-protocol.md` carries ONE v105 entry above v104 on this branch. The
  sibling `campaign/tui-stream-alias-6` adds its own v105 bullet and the two text-merge cleanly
  into two bullets straddling v104, which git will not flag. Whoever lands second folds them into
  one entry above v104; this is named in the lane report so the gate knows.
- `ProtocolReceiver::recv_decodable` now reports that it skipped, and `DaemonClient::recv` asks
  the daemon for a `Resync` after a skip, so an untyped skip cannot leave a client silently stale.
  Limiting the skip to stateless message kinds was not available: an undecodable frame has no
  readable kind.
- The 13 lines across 5 committed `attempt-18` evidence files that baked a Claude Code scratchpad
  path with a session UUID into this public repo are scrubbed to `<scratch>/...`.
  `compat/tui/evidence/TUI-008/attempt-05/53-gate-corpus.txt` carries two more of the same shape
  from a different session; it belongs to another obligation's zones and was left alone, named in
  the lane report for its owner.
- The three `///` lines added at `crates/zz-daemon/src/daemon/chooser_presentation.rs:565-567` are
  gone; they were the only doc comment this branch added to that file.

## The one asserted check that differs, and why it is not this pass's

`compat/tui-mouse.sh` exits 1 on `paste-under-menu/screen`, a TUI-008 case, measured three times
on this box with three binaries:

    origin/main 394ef850              1 of 45 asserted checks differ, paste-under-menu/screen
    this branch before this pass      1 of 45 asserted checks differ, the same check, same row
    this tip                          1 of 55 asserted checks differ, the same check, same row

So it is inherited, not caused here: it is already there on `origin/main` with none of this
branch's commits. The row is the pane's first line after a bracketed paste under an open menu:
the pin's shell echoes `sted-text~` plain, zz's echoes `sted-text` inside readline's highlighted
bracketed-paste region. `/bin/sh` is bash 5.3 on this box and was dash where TUI-008 was verified,
and bash's readline highlights an active paste region while dash echoes nothing of the kind, so
what the two binaries forward under a menu differs and only bash makes it visible. It is TUI-008's
case, in TUI-008's zones, and is named for its owner rather than touched here.

## What the runs say

    compat/tui-client-commands.sh, runs 1-3 and run 4   exit 0, all 517 asserted comparisons
                                                        identical, 30 recorded, ordinary
                                                        TUI-014=0, unattributed=0
    compat/tui-client-commands.sh --self-check          exit 0, every sabotage caught in its own
                                                        channel and both equivalences passed
    compat/tui-mouse.sh                                 exit 1, 55 asserted, 0 recorded, 1 differs
                                                        (paste-under-menu, inherited, above)
    compat/tui-mouse.sh --self-check                    exit 0, 33 sabotages caught
    compat/tui-choosers.sh and --self-check              exit 0, 78 asserted, 0 recorded
    compat/tui-overlays.sh and --self-check              exit 0, 48 asserted, 0 recorded
    compat/tui-screen-diff.sh and --self-check           exit 0, 147 checkpoints, 6 recorded
    compat/tui-copy-mode.sh and --self-check             exit 0, 147 cases assert, 0 record in full
    compat/attached-client.sh                            exit 0, PASS
    verify-claims.py --run TUI-014                       exit 0, every verified obligation holds up
    compat/run.sh over 37 delta rows                     exit 0, no divergence in any channel
    cargo test zz-mux/zz-protocol/zz-tui                 exit 0
    cargo test zz-daemon                                 exit 101, one load flake per run, each
                                                        passing solo
    cargo clippy on the five touched crates              exit 0
    cargo fmt --all -- --check                           exit 0
    compat/check.sh                                      exit 0
    python3 compat/tui/tracker.py check                  valid and current

Two runs are retained failed beside their passes because a retry is not a re-measurement:
`attached-client.txt` timed out on a popup probe's ten-second wait while three fixture chains and
a cargo suite shared the box, and `attached-client-retry.txt` is the same driver alone, PASS.
`tui-client-commands-self-check.txt` was first written by a run that exited 1 because the new
customize-prompt-vi sabotage changed `status-keys` AFTER the prompt was raised, where
`prompt_set_options` copies the option at creation; the sabotage now changes it before the prompt
is raised, and the file holds the passing run. Runs 1 to 3 were taken with the fixture before that
one self-check function changed and run 4 with the final one; `fixture-drift.txt` is the diff, and
no non-self-check run calls that function.

`delta-selection.txt` is the whole 196-row `origin/main..HEAD --commands
send-keys,customize-mode,switch-mode,set-option` selection. 37 of those rows ran - every row whose
name carries send-keys, the mouse, a mode, customize, switch, status-keys, a prompt or a key - and
the other 159 are UNRUN, named there: two other campaign lanes shared the box and the full
selection was taking about two minutes a row.

## Limits, retained

- `mode_tree_display_menu` on `MouseDown3Pane` is unbuilt (`semantic:mode-tree-mouse-menu`); the
  selection move that press also makes is built and asserted through the click cases.
- A mode prompt still has no history (`Up`/`Down`), no `Tab` completion and no `prompt_paste`, so
  `C-y` and the vi `p` that maps onto it are no-ops.
- Rows are rebuilt from live state on every snapshot, so an option changed from outside the mode
  shows at once where the pin shows it after its next build.
- The pin crashes when its customize preview draws a `pane-colours` array child, so the
  pane-scope array cases open the mode with `-N`.
- Default key tables differ by accepted `keys.*` gaps, so the key-binding cases use a table the
  fixture binds itself rather than `root` or `prefix`.
