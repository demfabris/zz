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
- screen-diff.txt - compat/tui-screen-diff.sh, 147 identical / 6 recorded, exit 0.
- attached-client.txt - compat/attached-client.sh, PASS.
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
`bash` where the pin answered `sh` for the scene's `exec /bin/sh`. It is intermittent, it
is the same row and the same reading the cycle 6 commands gate recorded at its own tip
before any of this landing existed, and it comes from `terminal_current_command`'s sysinfo
read of the pane's foreground pgid, which nothing here touches. The pin reads
`/proc/<pgid>/cmdline` and takes the first argument through `parse_window_name`; zz reads
the process name. Left alone rather than changed under this obligation: it would move a
format every corpus row reads.
