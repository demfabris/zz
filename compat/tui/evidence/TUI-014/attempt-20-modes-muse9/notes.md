# TUI-014 attempt-20-modes-muse9: the mode-tree mouse menu

Lane `modes-muse9`, branch `campaign/tui-customize-6`, measured 2026-09-19 on the
Alienware box (Linux x86_64, /bin/sh is bash 5.3). Builds and tests go through
/tmp/zz-cargo.sh (4G cap, 2 jobs, shared flock slots).

## What closed

`semantic:mode-tree-mouse-menu` left `clients.interactive-refresh` with a dated
resolution. TUI-014 owns zero ordinary records and nothing of its own is parked
in a gap anymore.

## The build

`MouseDown3Pane` in customize-mode opens `mode_tree_display_menu` over the
selected line, through the `display-menu` MenuState surface (no wire change,
stays at 105):

- mux (`crates/zz-mux/src/command/customize.rs`): `customize_mouse` returns a
  `CustomizeMenu { line, outside, name }` request on Down3, following
  `mode_tree_key`'s branch order (prompt row, help, outside the tree box,
  below the lines, on a line). `CUSTOMIZE_MENU_ITEMS` /
  `CUSTOMIZE_OUTSIDE_MENU_ITEMS` carry the pin's tables;
  `customize_menu_feed` maps a chosen row to the key fed back;
  `customize_menu_choice` mirrors `mode_tree_menu_callback` (line bounds,
  current = line, feed the key through `customize_key`).
- daemon (`crates/zz-daemon/src/daemon.rs`): `raise_mode_tree_menu` lays the
  rows out with `layout_menu_row`, places the box with the pin's
  pointer-centred rule clamped to the client geometry, and stores a
  `ModeTreeMenu { pane, line, outside }` owner on the `MenuSession`.
  `input_menu` routes a choice on such a session to `mode_tree_menu_choice`,
  which reuses the extracted `apply_customize_result` tail. Cancel is a
  no-op; a choice with the mode gone or the line out of range is dropped.
- No client change: the raw TUI already renders MenuState and routes
  press/drag/release and keys through it, including release-closes.

Switch-mode gets no menu: `window_switch_key` has no Down3 branch.

Two measured pin quirks are reproduced. Tag All feeds a raw `\x14`, which
matches no arm of the mode's key handling (`case 't'|KEYC_CTRL` wants the
flag form), so choosing it by pointer closes the menu and changes nothing;
its row key is `[DC4]`, which no keypress spells, so it is pointer-only. A
first version stripped modifiers in `resolve_menu_key` so C-t would choose
Tag; a server-logged probe (`complete key \024 0x200000000074`, menu stays
open) showed `KEYC_MASK_FLAGS` keeps the CTRL/META/SHIFT bits, so the pin's
scan is exact and C-t is swallowed with the menu left open. The strip was
reverted; `crates/zz-client` is untouched.

## The fixture

`compat/tui-mouse.sh` gains `customize-mouse-menu-*` (open, release, expand,
tag, tag-none, tag-ctrl, tag-all, select, cancel) and
`customize-mouse-menu-outside-*` (open, scroll, cancel), 12 whole-screen
checks, plus 6 one-sided sabotages (press aimed one row lower, press aimed
seventeen cells right, release held back, drag released on the press cell,
Tag All drag released on the Tag row, outside press aimed twenty cells
right). Keys reach the menu as real client bytes; `send-keys` would bypass
the client overlay. Tag and Tag All are chosen by pointer (press, SGR-34
drag, release); the drag math needs no title width because the menu always
covers the press column at these cells.

Two sabotage-tolerance notes. The switch-wheel sabotage leaves zz's
switch-mode open (pre-existing: it used to run last, so nothing observed
it); each menu sabotage fn starts with `mode_leave_both`. The release-hold
sabotage would cascade through the expand step, so the case clears the aim
and release sabotage vars after the release check and sends one resync
release; the resync is swallowed when no menu is open.

## The shell fix (second item)

`paste-under-menu/screen` failed here because /bin/sh is bash 5.3: the pin
forwards the post-menu paste tail as ordinary keys plus a stray `\e[201~`
(the shell shows `sted-text~`), while zz re-wraps it as a bracketed paste
(readline highlights `sted-text`). The case now runs its pane on a pinned
`stty -echo -icanon min 1 time 0; exec cat` program with a MENUPROG marker
(the way the focus and paste-into-pane cases pin theirs) instead of the
interactive shell, and respawns the shell afterwards. The asserted channels
are unchanged: the whole decoded screen (menu opens, the paste picks Alpha,
the menu closes, the tail reaches the pane program) and the menu's option.
Under dash both forms echo the tail plain, which is why TUI-008 verified it
there. Not weakened: the tail bytes still reach a live pane program and the
screen still carries them; the longer-paste sabotage still bites.

## Incidental observations

- One full run showed `customize-mouse-double-click/screen` differing on the
  prompt row; the eighth-pass case passed on every rerun (flake, 1 of 4).
- Two self-check runs showed a stale bash `Display all 3634 possibilities?`
  query on the pin's shell grid inside the drag sabotage's cancel check, and
  once on zz's grid right after. It never appears in normal runs or in the
  sabotage run in isolation, no fixture input sends a Tab, and both binaries
  (including the unmodified pin) print it, so it is shell-side dynamics of
  the sabotage chain, not an implementation bug. It only adds an incidental
  DIFF line; every sabotage is still caught in its own channel.

## Cycle-11 measurements (2026-09-19, this pass)

Base `origin/main d03e20a0` (`Verify TUI-015 and TUI-017 at the residuals
gate`); `origin/main` has since moved to `b1596c72`, which this pass does
not chase: every run below is measured on the `d03e20a0` tree and the
run binary `target/debug/zz_cli` (SHA-256
`db2811c43eced9b7...0093c`, built after the last source edit; only the
ledger JSON and its generated report are newer, and they are not code).
Pinned tmux `next-3.8 d77c9dc6` (SHA-256 `df2cafcb...63ee5`).
Alienware box, Linux 7.2.3-1-cachyos-deckify x86_64, rustc 1.97.0.
Every cargo invocation went through `/tmp/zz-cargo.sh`.

- `compat/tui-mouse.sh` three times: each exit 0, `67 asserted checks,
  0 recorded checks`, all identical; `--self-check` exit 0, `every
  sabotage caught in its own channel` (37 caught lines).
- `compat/tui-client-commands.sh`: exit 0, `all 538 asserted
  comparisons identical, 40 recorded not asserted (0 for a sibling
  lane, owners decided:TUI-014=3 decided:TUI-015=4 decided:TUI-016=1
  decided:TUI-017=23 gap:clients.interactive-refresh=8
  gap:protocol.socket-acl=1 unattributed=0)`; ordinary TUI-014=0.
  `--self-check` exit 0, `every sabotage was caught in its own channel
  and both equivalences passed`. One earlier self-check run of the same
  fixture exited 1 at `capture-named-background` under parallel-lane
  load and passed on the immediate rerun; the failed run is retained as
  `tui-client-commands-self-check-flake.txt`.
- `compat/tui-choosers.sh` and `--self-check`: both exit 0, `all 78
  asserted comparisons identical, 0 recorded`.
- `compat/tui-overlays.sh` and `--self-check`: both exit 0, `all 48
  asserted comparisons identical, 0 recorded`.
- `compat/attached-client.sh`: exit 0, `attached-client
  compatibility: PASS`.
- Delta corpus: the 15 rows of the eighth-pass selection this change
  can reach (send-keys x2, display-menu x7, display-popup x3, send-keys
  smoke x3) all exit 0 with 0 TOPO/GEO/FMT/OUT/WARN divergences,
  `Nothing failed on the first pass`; the rest of the 196-row
  selection is SKIPped as outside reach (delta-selection.txt).
- `cargo test -p zz-mux --lib`: 560 passed, 0 failed. `cargo test -p
  zz-daemon --lib`: 985 passed, 2 failed under full-box parallel load
  (`positive_delay_shell_job_retains_destroyed_target...`,
  `wait_pane_idle_returns_after_the_dwell`, both timing-sensitive and
  neither touching the menu code); each passes solo
  (tests-daemon-solo1/2.txt), which is the load-induced flake the
  repo's AGENTS.md names. `positive_delay_shell_job...` flaked the
  same way in attempt-19.
- `cargo clippy -p zz-mux -p zz-daemon --all-targets --all-features --
  -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0.
- `compat/check.sh` (nested cargo routed through the shim): exit 0,
  including wire-version `105 is unreleased` and evidence-secrets.
- `python3 compat/tui/verify-claims.py --run TUI-014`: exit 0, `every
  verified obligation holds up`.
- `python3 compat/tui/tracker.py check` and `tmux-tracker.py check`:
  both ledgers valid with current generated reports.

Cross-lane note: a first `compat/check.sh` run deadlocked because the
lane's `cargo` shim re-entered `/tmp/zz-cargo.sh` (which execs bare
`cargo`) and the nested flocks wedged across both slots; killing the
chain with a broad pkill also orphaned a sibling lane's
`zz-c11-alias-review6` zz-protocol test run, whose detached cargo was
then killed by PID to restore the two-slot discipline. The sibling's
wrapper recorded the kill as a failure and retries it; no sibling
checkout file was touched. The shim now strips itself from PATH, and
the retained `compat-check.txt` is the clean rerun.
