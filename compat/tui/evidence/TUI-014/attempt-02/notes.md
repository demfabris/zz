# TUI-014 attempt-02, cycle 9

The punch list was clause 1's one open divergence, the info preview `i` raises, then
clause 2 and clause 3. Clause 1 is closed. Clause 2 is untouched and clause 3 still holds
six recorded cases: the budget went to clause 1 and to TUI-006, which came first.

## Clause 1: the info preview

compat/tui-choosers.sh presses `i` on both sides now. Before this attempt the only `i` in
the file was a --self-check sabotage that typed it into the pin alone, which passed whether
or not zz's info view was right. `client_case` gains `client-info-view` and
`client-info-view-off`, compared whole-screen through `client_info_mask`, and the
--self-check gains the mirror sabotage that puts the info view on zz's side alone, which is
the one that catches a wrong zz info view.

Five divergences the case exposed, all fixed:

1. The box title read `(view: preview)` on zz under the info view. `ChooserPresentation.view`
   was the literal `"preview"`; it follows the chooser's own `info_preview` now, the way
   `window_client_init` and `window_client_key` call `mode_tree_view_name`.
2. Every info row's separator printed the literal letter `x`. `#[...,acs]` is the pin's
   alternate character set, so the raw TUI's markup path draws an acs segment through
   `tty_acs_table` (`x` is U+2502) and drops the attribute before it writes the cells.
3. `Terminal Type` read `Unknown` against the pin's `tmux next-3.8`, because
   `#{client_termtype}` was always empty. `tty_keys_extended_device_attributes` keeps the
   XTVERSION reply on the client as `c->term_type`; the raw TUI already parses that reply
   for its feature bits, and it now sends the name too, as the appended
   `ProtocolMessage::ClientTerminalType` (v102, tail append, both halves in the same push,
   128-byte cap rejected during deserialization).
4. `WINDOW_CLIENT_INFO_LINES` carried ten of the pin's twenty-three. The other thirteen are
   here verbatim: the six `WINDOW_CLIENT_FEATURE` rows, the acs rule row, mouse,
   set-clipboard, get-clipboard, focus-events, extended-keys and set-titles. At 80x24 the
   preview box shows the first ten, so the two feature rows are on screen and asserted.
5. `#{I/f:extkeys}` answered 0 where `#{client_termfeatures}` listed extkeys, so the
   feature grid painted one name grey that the pin painted green. `tty_update_features` puts
   a learned feature on the client's own `tty_term`; the daemon now hands the client's
   learned set to `client_terminal_facts` as one more `terminal-features` entry matching
   that client's `TERM`, which is the pin's own channel for it.

Two more the case needed before it could assert:

- `#{E:tree-mode-border-style}` is not a format zz answers - `tree-mode-border-style` is not
  in `TMUX_OPTION_CONSUMERS` and the mode tree's own box takes the style from a const - so
  the acs rule drew with no background. The info lines take the style from that same const.
- `#{t/r:client_created}` and `#{t/r:client_activity}` expanded empty whenever the engine's
  `format_now` was stamped before the client's last key, because `format_relative_time`
  answers nothing for a stamp that is not yet in the past. The info lines read the wall
  clock at expansion, which is what the pin does.

`client_info_mask` pins the four values two servers cannot agree on - the client name, the
PID, the two timestamps with their relative halves, and `#{client_written}` - and asserts
every other cell, including the labels, the acs rules, the session, the terminal type,
TERM, the size, the whole feature grid and the box.

## Clause 2 and clause 3: not done

clock-mode, customize-mode, switch-mode, suspend-client and server-access are still in
UNIMPLEMENTED_TMUX_COMMANDS and compat/tui-client-commands.sh still records
clock-mode-open, customize-mode-open, switch-mode, suspend-client, server-access-bare and
server-access-user, plus client-tree-open under clients.interactive-refresh. Nothing in
commands.native-client-tools closed here.

## Files

- environment.txt - the box, the pin, the revision, the binary's sha256.
- choosers-run-1/2/3.txt and choosers-self-check.txt - the same three runs and self-check
  TUI-006/attempt-03 lists: `all 78 asserted comparisons identical, 0 recorded not asserted
  (0 for a sibling lane)` three times, and twelve sabotages caught.
- client-commands-run-1/2/3.txt - compat/tui-client-commands.sh, three runs, each
  `all 61 asserted comparisons identical, 37 recorded not asserted (0 for a sibling lane)`,
  exit 0, taken after the last commit. Unchanged from the cycle 8 tip: this attempt adds
  nothing to that fixture.
- client-commands-run-2-server-exit.txt - the run that replaced: the fixture's own inner
  pin server exited part way through and every step after it read `no server running on
  /tmp/tmux-1000/zzcci-...`, so client-tree-usage compared a dead server's error against a
  live one's state. Kept rather than deleted, because it is the only red seen in six runs
  of this fixture today and it names its own cause.
- client-commands-self-check.txt - `--self-check`, exit 0.
- copy-mode.txt, screen-diff.txt, attached-client.txt, corpus.txt, cargo-crates.txt,
  cargo-zz.txt, clippy.txt - the shared proof set, listed in TUI-006/attempt-03/notes.md.

## Gate addendum, 2026-09-14 (cycle 9 choosers gate, revision a82bd0f51359809a818d64e87a2a162f3892c4ce)

TUI-014 stays **active**: clause 1 asserts whole, clauses 2 and 3 are untouched.
What changed here is the evidence under clause 1, not its scope.

The four pre-mask chooser files this directory carried (choosers-run-1/2/3/4.txt,
all one md5, captured before 65d98e0a edited compat/tui-choosers.sh) are replaced
by the gate's own runs at a82bd0f51359809a818d64e87a2a162f3892c4ce:

- **choosers-run-1.txt, -2.txt, -3.txt** - three runs, each
  `all 78 asserted comparisons identical, 0 recorded not asserted (0 for a sibling lane)`
  at exit 0, each stamped with the revision, the pin, the command, the wall clock
  and the exit code.
- **choosers-run-0-flake-zoom-tree-open.txt** - the one red of the four runs
  taken, kept rather than dropped: a blank zz capture on `zoom-tree-open` with
  both cursors agreeing.
- **choosers-self-check.txt** - exit 0, thirteen ok lines, including the
  `title cut` sabotage this gate added.

**Both masks clause 1 rides on were defective and are fixed at this gate.**
`client_info_mask` collapsed every run of two or more spaces on any row it
touched, so a client info view whose label column sat four cells off masked
identical; it now collapses only the fill that follows one of its own
substituted tokens. `client_row_mask` replaced everything between the client
name and the next box edge with one token on every row carrying the name; it now
truncates only the tree title row another box cuts, and only to a fixed 13
columns, so the sort label inside that span is still compared. The zz-side
info-view sabotage runs under `client_info_mask` now, which is the mask the
client-info-view checkpoints assert under - clause 1's evidence has a sabotage of
its own for the first time. **gate-mask-probe.txt** is the reviewer's own probe
re-run verbatim against both fixed masks.

- **gate-client-commands.txt / gate-client-commands-self-check.txt** -
  compat/tui-client-commands.sh re-run by the gate at this revision:
  `all 61 asserted comparisons identical, 37 recorded not asserted (0 for a sibling lane)`,
  exit 0, and the self-check green. Unchanged from what the lane recorded, which
  is the point: this attempt still adds nothing to clause 3.
- **gate-fixtures.txt, gate-corpus.txt, gate-cargo.txt, gate-environment.txt** -
  the gate's full proof set, shared with TUI-006/attempt-03.
- **review.md** - the reviewer's verdict verbatim, plus what this gate verified
  of clause 1 and what it did not, and the group-attribution correction clause 2's
  dispatch needs (`command:switch-mode` is in `clients.interactive-refresh` and
  `server-access` in a third group, not in `commands.native-client-tools`).

The lane's own client-commands-run-1/2/3.txt, client-commands-run-2-server-exit.txt
and client-commands-self-check.txt are left exactly as the worker took them.
