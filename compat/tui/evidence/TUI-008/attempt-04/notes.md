# TUI-008 attempt-04, the cycle-9 menus lane

The second half of the cycle-9 mouse work: the two pointer menus and the paste
under one. `environment.txt` first; every other file here is a real run at this
branch's tip on the ubuntu box against pinned tmux d77c9dc6. Attempts 01 to 03
carry the history and nothing here restates it.

## Files

- `environment.txt` — box, tip, base, binary hash, pin, toolchain, locale.
- `01-tui-mouse-run-1.txt`, `02-…-2.txt`, `03-…-3.txt` — compat/tui-mouse.sh
  three times at the tip, byte-identical at md5 `ff98b647`, each
  `39 asserted checks, 0 recorded checks` and
  `all 39 asserted checks identical`.
- `04-tui-mouse-self-check.txt` — the fixture's `--self-check`: three controls
  that stay quiet and twenty-two one-sided sabotages, each caught in its own
  channel. Three of the sabotages are new here, one per channel that flipped.
- `05-attached-client.txt` — `attached-client compatibility: PASS`.
- `06-tui-stock-keys.txt`, `07-…-self-check.txt` — 50 cases agree on every
  channel they assert, 8 record a difference elsewhere (none owned here). The
  self-check's first run reported `the prefix and the key in one write or two`
  as a difference where both sides agree; it was rerun and passed, which is the
  load-flake shape this box documents.
- `08-corpus-delta-menu-rows.txt` — the twelve delta rows this landing can
  reach: list-keys-padding, strict-key-validation, copy-mode-bindings, prefix2
  and the eight display-menu and flag-error smoke rows. All clean.
- `09-corpus-delta-menu-rows-first-cut.txt` — the same set against the FIRST
  cut of the menu-item change, where smoke/args-parse-display-menu went red on
  `unknown command: no-such-display-menu-child`. Kept because it is the run
  that found the over-broad skip and forced the narrower rule.
- `10-…` to `15-…` — cargo test over zz-protocol, zz-mux, zz-tui, zz-client,
  zz-daemon and zz's cli_binary, and clippy with `-D warnings` over the five
  touched crates. `12` shows `daemon_autostart::nested_attach_inside_a_pane_
  prints_the_pinned_refusal` failing under load; `13` is the same test alone,
  passing, which is this box's load-flake rule.
- `16-compat-check.txt` — compat/check.sh, exit 0, with the pinned tmux build
  lines dropped. `17-verify-claims.txt` — compat/tui/verify-claims.py:
  "every verified obligation holds up".

## The rebase onto the fix pass's newest tip

Files `18` to `23` are the proofs re-taken AFTER the last rebase, at the sha
this lane pushed: compat/tui-mouse.sh three times, byte-identical at the same
md5 `ff98b647` and each `39 asserted checks, 0 recorded checks`, its
`--self-check`, and compat/attached-client.sh. `23` is that fixture's first run
at this tip, which timed out waiting for its reattach marker under load; `22`
is the rerun, `attached-client compatibility: PASS`, which is this box's
documented load-flake shape.

## One thing this lane touched outside the repo

compat/check.sh's first run rebuilt the pinned tmux in the SHARED checkout's
cache, because this worktree had no compat/.cache of its own and the run was
given a symlink to the shared one. The source tree is untouched and still
clean at d77c9dc6; only the binary was relinked, which a cargo-style build is
not reproducible across, so compat/.cache/tmux-build.stamp was rewritten from
the same fetch-tmux.sh formula and compat/fetch-tmux.sh now accepts the cache
without rebuilding. Every fixture run listed here was taken before that
relink, against the same pinned source.

## What moved

Three recorded checks at the base, none now; two checks were added.

1. `status-clicks/right-click-screen` and the new
   `status-clicks/alt-right-click-screen`. `MouseDown3Status` and
   `M-MouseDown3Status` are installed over `DEFAULT_WINDOW_MENU`, and `-x W`
   and `-y W` answer `popup_window_status_line_x`/`_y` from the event instead
   of the screen centre. Both are all 24 rows identical on both binaries.
2. `right-click-pane/screen`. `MouseDown3Pane` and `M-MouseDown3Pane` are
   installed over `DEFAULT_PANE_MENU` and its two submenus, and `-x M`/`-y M`
   answer from the event. All 24 rows identical, the same ten-row menu at the
   same left 0, top 3, width 24 and height 12.
3. `paste-under-menu/screen`. Identical on both binaries at this tip and at
   the base, every run; the case asserts rather than records.

## Three things the menus needed, each measured

- A menu a POINTER raises starts with nothing selected. `menu_prepare` walks
  the starting choice only under `MENU_NOMOUSE`, and `cmd_display_menu_exec`
  raises that flag only for a menu with neither `-M` nor an invoking event.
  zz read the `-M` half alone and highlighted `Swap Left`.
- `server_client_check_mouse` resets the click sequence when `m->b` differs,
  and `m->b` carries the modifier bits. A plain right press followed inside
  the 300 ms timeout by an alt-modified one is two `MouseDown`s on the pin;
  zz compared the button alone, made the second a `SecondClick` nobody binds
  and swallowed it. Measured on this fixture: whichever of the two gestures
  ran second, its menu was absent on zz and present on the pin.
- A menu item's command is a string. `cmd_display_menu_get_type` types the
  slot `ARGS_PARSE_COMMANDS_OR_STRING`, so the parser resolves every command
  NAME inside it and stops on one it does not know, and `menu_key_cb` is the
  first thing that parses its flags. zz parsed and prepared every item block
  when the menu was raised and refused the whole menu over `move-pane -P`, a
  flag `pane.floating-model` keeps native. Resolving names only is what
  `smoke/args-parse-display-menu` and the pane menu both need.

## The race the two menu cases were losing

Both menu cases waited for the PIN's menu and then settled on a marker the raw
TUI had already drawn, so the capture could run inside the hundred and forty
milliseconds the daemon spends expanding twenty-eight item formats. Measured
from the client's own log: the daemon built the pane menu at +140 ms and the
client had it at +150 ms, while the capture had already run. Both cases wait
for the menu text on BOTH screens now and settle on that text, which is the
shape `case_paste_under_menu` already used.

## What is not here

`mouse_word`, `mouse_line` and `mouse_hyperlink` are still unanswered and
`formats.mouse-context` keeps all five of its items. The pane menu case aims
at a blank cell, so the pin answers empty for all three and the two sides
agree without them; measuring them over a word, a wrapped line and an OSC 8
hyperlink needs a synchronous read of the live grid under a cell, which the
daemon has no path to today.
