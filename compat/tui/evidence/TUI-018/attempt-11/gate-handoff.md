# TUI-018 sixth pass: gate handoff

Branch `campaign/tui-stream-alias-6`. Production commits 8be81508 (control run-shell guard) and
613ef711 (streamed pane input, unreadable stdin, spent pane readers, pane geometry and facts);
fixture 13c78b22; documents e3e6b003; evidence 9ac5831e; then the origin/main merge 47f6ab30, the
attempt-09 redaction 9a1211e1 and the protocol 105 move 4bb7a2ca. Read notes.md first, including its
tip section: the counted proof is at e3e6b003 and the fixtures were re-run at the tip.

Re-measure, in this order:

1. `ZZ_BIN=<zz_cli> TMUX_BIN=compat/.cache/tmux-src/tmux compat/tui-command-streams.sh` expects
   `all 201 asserted comparisons identical, 0 recorded not asserted, 5 decided`.
2. The same with `--self-check` expects 175 met expectations and exit 0.
3. `ZZ_STREAM_MATRIX_FILTER='^stream-|spent|writeonly|directory|alias-attached-term|control-term' compat/tui-command-streams.sh --matrix <3c4a5255 zz_cli>`
   should fail 33 cells: that is the fix-reverted proof.
4. `compat/tui-client-commands.sh` ends `unattributed=0` with no TUI-018 owner.
5. `compat/attached-client.sh` passes.

Things to look at hard:

- The early guard now applies to every control-guarded `run-shell` without `-C` in
  `execute_control_command_with_guard`: alias members, hooks and inserted commands. The pin emits
  `%end` when `cmd_run_shell_exec` returns `CMD_RETURN_WAIT`, so this matches it everywhere; the
  corpus rows with control run-shell passed, and probes/f4e covers target, exit, delay and output.
- `stream-display-fast` leaves `#{history_size}` out on purpose (history-limit product default).
- Seven corpus rows are red twice; all seven fail on the 3c4a5255 binary too (notes.md lists how).
- The desktop window was not launched; its CLI surface was.
