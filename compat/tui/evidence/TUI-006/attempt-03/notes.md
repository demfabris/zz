# TUI-006 attempt-03, cycle 9

The punch list was TUI-006's three recorded cases, all in acceptance clause 2 and all in
compat/tui-choosers.sh: output-search-prompt, output-search-typed and output-selected. All
three are `same` now and the fixture records nothing.

## What the landing is

- `command-prompt -P` is `window_pane_set_prompt`, so the prompt belongs to the pane the
  command targeted rather than to the client. `CommandPromptState` gains a trailing
  `pane: Option<PaneId>` (v102, pure append, consumer half in the same push) that the mux
  fills from `-P` with the invocation's own pane, and the raw TUI paints that prompt over
  the pane's last row - its first under `status-position top` - in `message-style` over a
  row of the pane's own default cells, leaving the status row to `status_redraw`. `-P` with
  no pane to resolve raises nothing at all, which is `cmd_command_prompt_exec`'s early
  `CMD_RETURN_NORMAL`.
- `window_copy_command` clears `data->searchmark` for every command that is not `search-*`.
  zz already carried the rule on the mode slot, but `copy_mode_snapshot` pushed its match
  overlays from the view's own search rather than from the marks, so a `begin-selection`
  followed by `cursor-right` left the current match painted under the selection in the
  retained command output. The overlays now follow the marks.

## Files

- environment.txt - the box, the pin, the revision, the binary's sha256. The proof outputs
  were captured at revision 0ecfd454; the commits after it are the GUI's prompt test states
  (test-only, no binary change), the help-box mask below and this record.
- choosers-run-4.txt - a fourth run and a second --self-check, both taken after the last
  commit, which adds the measurement below to the fixture's header and to the ledger and
  changes no code: `all 78 asserted comparisons identical, 0 recorded not asserted (0 for a
  sibling lane)` again, and the twelve sabotages again. copy-mode.txt was re-run there too.
- choosers-run-1/2/3.txt - compat/tui-choosers.sh, three runs, each
  `all 78 asserted comparisons identical, 0 recorded not asserted (0 for a sibling lane)`,
  exit 0.
- choosers-self-check.txt - `--self-check`, exit 0, twelve sabotages caught and both
  equivalences passing. Two of the twelve are new and belong to this record: the stock
  search prompt over one pane only, and the current match painted on one side only.
- copy-mode.txt, copy-mode-self-check.txt - compat/tui-copy-mode.sh, `all 147 cases agree
  on every channel they assert, 0 recorded a difference elsewhere`, and its --self-check.
  TUI-005 is verified and the pane prompt moves its search prompt too, so this is the
  regression gate on that landing. The copy-mode corpus runs with `status off`, where the
  pane's last row IS the last screen row, which is why the fixture was green before and
  after.
- screen-diff.txt - compat/tui-screen-diff.sh, 147 identical / 6 recorded, exit 0, taken
  after the last commit.
- attached-client.txt - compat/attached-client.sh, PASS, taken after the last commit.
  compat/attached-client.sh is green at origin/main and this lane changes input and
  presentation, so it is run here and reports nothing red.
- corpus.txt - seventeen corpus rows for the touched commands: copy-mode-bindings,
  smoke/copy-mode-prompt-bindings, smoke/copy-mode-search, smoke/copy-mode-formats,
  smoke/command-prompt-chain, smoke/command-prompt-target,
  smoke/args-parse-command-prompt, smoke/chooser-tree-vocabulary,
  smoke/args-parse-choosers, smoke/chooser-row-flags, smoke/chooser-kill-keys,
  smoke/format-modifier-interrogate, smoke/format-listing, smoke/keys-prefix-remainder,
  formats, formats-values and census-formats. Every row TOPO, GEO, FMT, OUT and WARN clean.
  The two scenario headers that said the prompt's surface was the one thing that differed
  were corrected in the same commit.
- cargo-crates.txt, cargo-zz.txt, clippy.txt - zz-protocol, zz-mux, zz-tui, zz-client,
  zz-terminal and zz-daemon tests (895 in the daemon lib), `cargo test -p zz` (638 lib and
  125 cli_binary), and clippy --all-targets --all-features -D warnings over every touched
  crate, all exit 0.

## The one red seen and not owned

`filter-cleared` failed in two of the six choosers runs taken at this tip, always on the
same cell: the window tree's pane row reads `#{pane_current_command}` and zz answered
`bash` where the pin answered `sh` for the scene's `ENV= PS1='$ ' exec /bin/sh`. It is
intermittent, and it is the same row and the same reading the cycle 6 commands gate
recorded at its own tip before any of this landing existed.

The cause, measured here: `format_cb_current_command` runs `osdep_get_name` on every
expansion - `/proc/<pgid>/cmdline`'s first argument through `parse_window_name` - and falls
back to `cmd_stringify_argv(wp->argc, wp->argv)` and then to `wp->shell` when that read
comes back empty (format.c:941-949). zz's `terminal_current_command` reads the process name
once per change of the pane's foreground pid, and `execve` does not change a pid, so a read
that landed before the pane's own `exec /bin/sh` keeps answering with the login shell's
name for that pane's whole life.

The naive fix was written, measured and reverted the same day. Reading
`/proc/<pgid>/cmdline` per publish and dropping the per-pid cache makes this fixture green
twice in a row, and it makes three zz-daemon tests fail under parallel load
(`daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts`,
`scoped_status_format_writes_refresh_only_that_sessions_clients` and one rotating third)
on `pane runtime facts did not settle`, reproducibly, while each passes exact-solo: a
foreground pgid that is a child already gone reads back empty and publishes an empty
command where the cached value used to stand. The pin's fallback chain is the missing half
and the pane's own argv is not on the daemon's runtime facts, so it is more than this
obligation's remaining budget. The revert is complete: the binary this evidence attests
(sha256 fb19f6b0...) is the one without it, rebuilt and re-hashed after the revert.

The cell is left asserted rather than recorded, because it agrees on most runs and a
recorded case would understate the parity that is there. compat/tui-choosers.sh's own
header carries the same measurement so a red run explains itself.

## Gate addendum, 2026-09-14 (cycle 9 choosers gate, revision a82bd0f51359809a818d64e87a2a162f3892c4ce)

The chooser evidence in this directory is now the gate's own, taken on the
rebased branch after the five must-fixes the reviewer returned. The four
pre-mask files that were here before (choosers-run-1/2/3/4.txt, all one md5,
all captured at 0ecfd454 before 65d98e0a edited the fixture) are gone; nothing
in them distinguished one run from another or named a revision.

New and replaced files:

- **choosers-run-1.txt, -2.txt, -3.txt** - three runs of compat/tui-choosers.sh
  at a82bd0f51359809a818d64e87a2a162f3892c4ce, each ending
  `all 78 asserted comparisons identical, 0 recorded not asserted (0 for a sibling lane)`
  at exit 0. Every one is stamped in its own header with the revision, the pin's
  commit, the exact command, the start and end wall clock, the elapsed seconds
  and the exit code.
- **choosers-run-0-flake-zoom-tree-open.txt** - the fourth run this gate took,
  and the only red: `1 of 78 asserted comparisons differ` on `zoom-tree-open`,
  where the zz capture came back blank for all 24 rows while both cursors agreed
  at 0,1. It is kept rather than dropped. Four runs were taken at this revision
  and three were kept green; this is the discarded one, and the three that
  follow it and the --self-check all pass with the box under three sibling
  lanes' load.
- **choosers-self-check.txt** - compat/tui-choosers.sh --self-check at a82bd0f51359809a818d64e87a2a162f3892c4ce,
  exit 0, "every sabotage was caught and both equivalences passed", thirteen ok
  lines: the twelve that were here plus `title cut, the sort label inside the
  span a box cuts`, which this gate added for the client_row_mask must-fix.
- **gate-mask-probe.txt** - the reviewer's own mask probes re-run verbatim
  against the two fixed masks.
- **gate-fixtures.txt** - every fixture in the tree at this revision, with its
  exit code and its own last line.
- **gate-corpus.txt** - the 145-scenario corpus delta, none skipped.
- **gate-cargo.txt** - every touched package, the workspace clippy, verify-claims.
- **gate-environment.txt** - this box, this worktree, both binaries' sha256.
- **gate-copy-mode.txt / gate-copy-mode-self-check.txt**,
  **gate-screen-diff.txt / gate-screen-diff-self-check.txt**,
  **gate-client-commands.txt / gate-client-commands-self-check.txt** - the
  neighbouring fixtures at this revision. Copy mode matters most: TUI-005 is
  verified and this lane's prompt change reaches it, and it is still at
  `all 147 cases agree on every channel they assert, 0 recorded a difference
  elsewhere`.
- **gate-attached-client.txt** and **gate-attached-client-origin-main.txt**,
  **gate-overlays-self-check.txt** and
  **gate-overlays-self-check-origin-main.txt** - the two reds this gate found
  and their controls, each captured from a build of origin/main 45481a72 in this
  same worktree. Both fail identically there, so both are this box's baseline.
  The attached-client one is the more interesting: it is
  `probe_command_output_navigation`, which rebinds `/` to zz's native
  `copy-mode-search-prompt` for the zz side and says so on its own stdout, so it
  drives the replacement zz-only binding this obligation's clause 2 excludes -
  but compat/attached-client.sh is a declared source of six obligations already
  verified on main, every one of which recorded PASS at its own gate on
  alienware. This box has never run it before today. It wants a cycle 10 lane.

- **review.md** - the reviewer's verdict verbatim, what this gate did about each
  finding, and the probe that proves each mask fix.

The older files (environment.txt, copy-mode.txt, screen-diff.txt,
attached-client.txt, corpus.txt, cargo-crates.txt, cargo-zz.txt, clippy.txt) are
the lane's own and are left as the worker took them.
