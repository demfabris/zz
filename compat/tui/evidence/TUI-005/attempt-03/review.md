# TUI-005 review and cycle-6 copy gate

revision: bcf9d12328ed2ef15b56d9d4aa12c229273901c6 (gate tip before this records commit)
lane tip reviewed: 8d766d0dd89e30dd0abcc696f4110a6a3781ee6a on campaign/tui-copy-2
gate worktree: /home/demfabris/dev/zz-gate-tui6, rebased onto origin/main fd3c64e4 (release: zz 0.8.0)
pin: d77c9dc6aa021e4bc61f0da128c591af695e6466 (tmux next-3.8)

## Reviewer verdict, verbatim

```json
{
  "lane": "copy",
  "verdict": "approve",
  "confirmed_defects": [],
  "checks_run": [
    "Fetched origin; detached /home/demfabris/dev/zz-tui-copy-review at 8d766d0dd89e30dd0abcc696f4110a6a3781ee6a (= pushed campaign/tui-copy-2 tip, ls-remote matches); worktree clean the whole review",
    "touch crates/**/*.rs + Cargo.* in the review worktree, CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-copy/target, cargo build -p zz --jobs 4 through the 5G slot wrapper -> exit 0 (GUI crate builds)",
    "LANE-SPECIFIC ORACLE PROBE (my own, built from the fixture driver: outer pinned tmux, isolated HOME/XDG per side, short /tmp sockets, bounded wait_for, trap that reaps): 44 cases, only the 4 current-match rows differ. A: 5k + C-u C-u + C-d C-d C-d and 5k + NPage land BOTH sides on cursor 1,23 column=1 line=[$] in vi AND emacs. B: C-b C-b pages up in copy-mode-vi (scroll 22, then 41) and moves left in copy-mode (col 12->11->10), identical both sides, nothing reached the shell. C: send-keys -t <up>, -N 3 Z <up>, -l QQ into a copy-mode pane, identical both sides, Z and QQ dropped, nothing on the shell after cancel. D: #{pane_search_string} empty before, needle inside, needle after q, needle after re-entering, and n relands line-48 needle-48 on the re-entry, identical both sides, both tables",
    "EXTRA ORACLE PROBE of my own beyond the fixture: send-keys -l k, -N 2 -l kk, -H 6b, named Up Up and -l q into a copy-mode-vi pane -> zz and the pin identical on all six checkpoints (the literal/hex flag is masked before the table lookup on both, -l q cancels on both, nothing reached the shell)",
    "compat/tui-copy-mode.sh at the tip, run 1 -> exit 0: 147 cases, all asserting at least one channel, 13 recorded; every recorded reason starts SIBLING:modes and each still asserts text,cursor,facts,view,buffer (only rows recorded). Case list byte-identical to the recorded evidence copy-mode-tip-1.txt",
    "compat/tui-copy-mode.sh at the tip, run 2 -> exit 0: same 147 / 13",
    "compat/tui-copy-mode.sh --self-check at the tip -> exit 0, 27 of 27 expectations met; expectation list identical to the recorded copy-mode-self-check-tip.txt; the 5 new expectations (page landing on column 0 caught in cursor and facts, C-b C-b one-sided caught in facts and view, keys reaching the shell caught in text) all fire",
    "MY OWN CODE SABOTAGE (not the fixture's): reverted page_copy_cursor to the old always-clamp form AND disabled the SendKeys copy-table dispatch, rebuilt, re-ran the fixture -> exit 1 with exactly 8 red cases and the right bytes: vi-clamp-halfpage-down-onto-the-prompt and vi-clamp-page-down-onto-the-prompt (cursor 0,23 vs 1,23, column 0 vs 1), vi/emacs-send-prefix-runs-the-copy-binding (zz scroll 0 vs pin 22), vi/emacs-send-keys-runs-the-copy-table, vi/emacs-send-keys-cancel (zz shell shows `$ ZkZk`). Restored with git checkout -- crates/ and rebuilt before every later proof",
    "compat/tui-screen-diff.sh at the tip -> exit 0, 111 asserted checkpoints identical, 42 recorded",
    "compat/tui-stock-keys.sh at the tip -> exit 0, 50 cases agree, 19 recorded",
    "compat/attached-client.sh at the tip -> exit 1 after 19s at the step known at BASE ('zz screen did not visibly become copy-mode within 10 seconds'), nothing past it; same as the recorded attached-client-tip.txt",
    "cargo test -p zz-terminal --jobs 4 -- --test-threads=3 (5G wrapper) -> exit 0, 255 passed 1 ignored",
    "cargo test -p zz-daemon --jobs 4 -- --test-threads=3 (5G wrapper) -> exit 0, lib 865/865, every integration target green. The named flake client_focus_closes_display_panes_and_preserves_chooser_modes did not fire",
    "cargo clippy -p zz-terminal -p zz-daemon --jobs 4 --all-targets --all-features -- -D warnings -> exit 0",
    "cargo test -p zz --jobs 4 --lib --test buffer_client_file_load --test buffer_client_file_save --test cli_binary -- --test-threads=3 -> exit 0 (lib 634, load 6, save 5, cli_binary 125)",
    "python3 compat/tui/tracker.py check -> exit 0; python3 compat/tmux-tracker.py check -> exit 0; both write-report re-run in the worktree -> zero diff, so the generated knowledge/tmux/tui-parity.md and gaps.md are current",
    "PIN SOURCE READ: cmd-send-keys.c:60-108 (the -K branch returns early; otherwise the pane's first mode entry's key_table is used, a bound key dispatches with the pane as target and an unbound one is dropped) and window-copy.c window_copy_pageup1/pagedown1 (data->cx = data->lastcx unconditional, window_copy_cursor_end_of_line only when (cx >= lastsx && cx != px) || cx > px). The zz code at the tip mirrors both",
    "SCOPE OF THE DAEMON CHANGE: grepped every MuxEffect::SendKeys producer in crates/ -- exactly two, zz-mux command.rs:7823 (send-keys without -X/-K; -K goes to SendClientKeys) and :7847 (send-prefix). paste-buffer and send-keys -X are untouched, matching the pin's scope",
    "TEST-HONESTY on the one modified test: daemon_keeps_pty_and_mux_state_across_interactive_detach still asserts the live PTY advancing under copy mode, the -M capture staying frozen, unseen_output > 0 and the cancel revealing the live text; only the delivery changed from send-keys (which is now correctly consumed by the copy table) to set-buffer + paste-buffer -d",
    "ZONES on git diff origin/campaign/tui-cycle5-gated...HEAD: compat/tui/, compat/tui-copy-mode.sh, compat/tmux-gaps.json, crates/zz-terminal/src/session.rs (copy-mode code), crates/zz-daemon/src/daemon.rs (SendKeys effect dispatch), generated knowledge reports -- all inside the batch's zones; the single out-of-zone file is knowledge/tmux/key-tables.md (one paragraph, declared in notes)",
    "WIRE: no diff under crates/zz-protocol, crates/zz-mux, crates/zz-client, crates/zz-tui, crates/zz or knowledge/protocol; PROTOCOL_VERSION untouched at 101; no append, so no v101 history line is owed",
    "GUI: crates/zz-tui and crates/zz untouched, cargo build -p zz green, no presentation change",
    "HOUSE RULES: no added // or /* lines in any .rs hunk; no Co-Authored-By / Generated-with / Claude / noreply in any of the five commit messages; one commit per punch-list item with the ledger and evidence last and no code in it",
    "EVIDENCE: every file under compat/tui/evidence/TUI-005/attempt-03 carries a revision line, the exact command and a start timestamp; no .log file anywhere in the diff; git check-ignore -v over the whole attempt-03 tree reports nothing ignored",
    "GAPS: compat/tmux-gaps.json diff is confined to formats.pane-runtime -- acceptance corrected and a dated 2026-09-11 measurement appended. No item closed, no status change, so no TMUX_OPTION_CONSUMERS or compat_manifest_tests.rs partition move is owed",
    "LEDGER OWNERSHIP: the campaign.json diff changes only TUI-005's status, evidence_note and next_action. proof stays null. No other record touched",
    "SIBLING DIRECTION: the 13 recorded cases point at modes, which precedes copy in the gate order. origin/campaign/tui-modes-2 carries copy-mode-match-flip evidence and current-match hunks in crates/zz-tui render.rs and mode_view.rs, so the claim that the modes landing flips them is well founded",
    "HYGIENE: copied no binary under /tmp; pgrep -fa for zz-cli-/zz-user/zzprobe/zzcm and ls of /tmp/zzcm* both empty at the end; the review worktree is clean and rebuilt at the tip, not at the sabotage"
  ],
  "every_clause_asserted": "yes",
  "notes": "VERDICT: approve. All five punch-list items are done at the tip, each with a fixture case that asserts the pin's measured behaviour and a sabotage that fails for the right reason. I found no defect I can prove.\n\nCONTRACT AUDIT, clause by clause, at 8d766d0d:\n- Clause 1 (stock emacs and vi tables through actual input: search editing, repeat search, counts, line/page movement, selection, rectangles, copy, cancel): ASSERTED. compat/tui-copy-mode.sh types all of it into both attached clients inside one outer pinned tmux; 147 cases, 134 asserting all six channels, 13 asserting five. Every one of those 13 is a SIBLING:modes case, and modes precedes copy in the gate order, so by the review rule they do not keep the clause open. Two green runs plus the self-check at the tip, mine, not the worker's.\n- Clause 2 (cells, prompts, cursor, selection styles, paste-buffer bytes, live mode/binding changes, return to terminal input): ASSERTED. The rows channel is recorded on the same 13 and asserted on the other 134, the selection-style cases included (the cycle-5 gate's flip holds). live-mode-keys, live-binding and $table-terminal-input-returns are green on all six channels.\n- Clause 3 (ordinary-pane search kept separate from command-output search; inventory before claiming coverage): ASSERTED. The native-search-command group prints the separation on every run (zz's copy-mode-search-prompt still answers `terminal search is unsupported here` on an ordinary pane, the pin refuses the command), and it asserts, it is not recorded. The covered-subset inventory stands in evidence_note and names both the typed set and the set resting on copy-mode.action-fidelity's send-keys -X closure.\nThe worker listed all three clauses as open. It was being conservative about its own SIBLING:modes cases; under the rule the gate applies, they assert. I am marking every_clause_asserted yes, so the obligation can be set verified at the copy gate once the 13 cases flip on the main that carries modes.\n\nWHAT I MEASURED MYSELF, not read:\nThe lane-specific probe2 re-run reproduces the pin exactly at the tip. The vi page clamp lands both sides on cursor 1,23 column=1 line=[$] for both sequences in both tables, where the cycle-5 review had zz at 0,23 column=0. C-b C-b pages up once per pair in copy-mode-vi (scroll 0 -> 22 -> 41) and moves the cursor left once per pair in copy-mode, identical on both binaries, with no ^B reaching either shell. send-keys -t into a copy-mode pane behaves as the pin does for named keys, -N repeats, -l literals and -H hex: bound keys run the binding, unbound ones are dropped, and after cancel both shells are untouched. I went past the batch's list and drove -l k, -l kk, -H 6b and -l q as well, because the pin masks the literal flag before the table lookup and a naive port would not: zz matches on all four, -l q cancelling the mode on both sides. #{pane_search_string} answers empty before any search, `needle` inside the mode, `needle` after q and `needle` again after re-entering, on both sides in both tables, and n on the re-entry relands line-48 needle-48 on both.\nMy own code sabotage is the real test of the fixture. Reverting page_copy_cursor to the old always-clamp form and disabling the SendKeys copy-table dispatch turns exactly eight cases red and nothing else, with the bytes the punch list measured: cursor 0,23 against 1,23 on the two vi clamp cases, zz sitting at scroll 0 against the pin's 22 on the send-prefix cases, and `$ ZkZk` on zz's shell on the send-keys cancel cases. The fixture can fail, and it fails for the stated reason.\n\nNITS, none blocking:\n1. knowledge/tmux/key-tables.md is outside this lane's zones. It is one paragraph, declared as an excursion in the notes, and its content is correct against the pin and against my probe. The only cost is a possible collision if a keys or modes lane touches the same sentence.\n2. The evidence files and proofs-at-tip.txt all carry revision 4f4f0b0b, one commit below the tip. 8d766d0d touches only campaign.json, the evidence and the generated report, so nothing is stale, but the record does not literally carry a run at the pushed tip. I re-ran every proof at 8d766d0d myself and they all land on the same numbers, so I did not raise it further.\n3. The emacs half of the clamp group cannot catch the measured regression, because emacs never clamped: only the vi cases go red under sabotage. That is the right shape anyway, since the punch list asked for a case per sequence per table and both tables now pin the pin's answer.\n4. copy_mode_key_owners dispatches to the caller's own copy session when it has one, and otherwise to every live copy session on the pane, so with two clients in copy mode on one pane a key can run once per client. The pin has one mode per pane, so this is not observable against the oracle, and it is the same per-client aggregation choice formats.pane-runtime already writes down for pane_in_mode. It is not written down anywhere, which is worth a sentence in the record next time this code moves.\n5. The zz-daemon run passed 865 of 865 here, so I saw nothing of the named client_focus flake this session.\n\nSUSPICION I COULD NOT TURN INTO A DEFECT: inject_mode_table_keys reuses CLIENT_KEY_INJECTION_DEPTH and returns early without decrementing when it is already at the cap. That is correct, because it does not increment in that branch, and I read it twice to be sure.\n\nGATE HANDOFF: nothing to fix before merge. The gate's job is the one the record already names, flip the 12 MATCH_REASON cases and emacs-ordinary-pane-search-backspace after rebasing onto the main that carries modes, keep each flip only if the fixture stays green three times and --self-check stays green, then set TUI-005 verified. compat/attached-client.sh still stops at the BASE step that belongs to modes; nothing ran past it here either."
}
```

## What the gate did

VERDICT WAS `approve`, so there was no must-fix list. The gate reproduced the
lane on the accumulated main, flipped the sibling cases the modes landing
unblocked, and set the obligation verified.

### Rebase

`git merge-tree --write-tree origin/main 8d766d0d` exits 1 and reports content
conflicts in fifteen paths, which is what a straight MERGE has to say here and
not a real prediction: the merge base is below the cycle-5 chain, and the modes
gate put that chain on main as NEW commits by rebasing it, so BASE's own hunks
appear on both sides with no ancestor to reconcile them (daemon.rs, status.rs,
session.rs, render.rs, mode_view.rs, the two protocol files, campaign.json,
tmux-gaps.json, the two fixtures and the generated reports). The rebase is the
operation that matches how the lanes land, and it was clean: the five lane
commits replayed onto origin/main fd3c64e4 with ONE conflict, the generated
`knowledge/tmux/tui-parity.md` status-count line, resolved by regenerating the
report. The rebased diff against origin/main
is byte for byte the lane's own diff against its BASE (30 files, +4086/-25), so
the rebase carried the lane and nothing else. BASE (origin/campaign/tui-cycle5-gated
5d9bf198) was already on main by content: the modes gate carried it in at
a2a1a288.

### One gate commit the lane did not owe

`knowledge/tmux/gaps.md` was STALE AT origin/main, not because of this lane.
Main's own 268ccd9d ("Own the configuration files and import tmux on request")
edited `compat/tmux-gaps.json` (config.discovery and presentation.native-status)
without regenerating the report, and both `compat/run.sh` and `compat/check.sh`
refuse to run while the two disagree. Proved by checking out origin/main's two
files alone into the gate worktree: `python3 compat/tmux-tracker.py check` said
`knowledge/tmux/gaps.md is stale`. Commit 1166303b regenerates it; the diff is
the two sentences main left behind and nothing of this lane's.

### Sibling flip

Thirteen cases in `compat/tui-copy-mode.sh` recorded the rows channel against
`SIBLING:modes`: the twelve MATCH_REASON search cases (submit, again, reverse,
backward, re-entry search-again and forward-search-re-entry-searches-up, per
table) and `emacs-ordinary-pane-search-backspace`, whose incremental prompt
already paints the match. The modes lane landed
`copy-mode-current-match-style` in `crates/zz-tui` render.rs and merged at
a2a1a288, so commit bcf9d123 drops MATCH_REASON, sets the emacs
`SEARCH_OPEN_MODE` back to all six channels, and takes the recorded-channel
argument off all twelve call sites. The fixture now has NO recorded case at all.

The flip was kept because the fixture stayed green three consecutive times and
`--self-check` stayed green, exactly as the record's next_action asked:

- run 1 -> exit 0, `all 147 cases agree on every channel they assert, 0 recorded a difference elsewhere`
- run 2 -> exit 0, same
- run 3 -> exit 0, same
- `--self-check` -> exit 0, `every sabotage was caught in its own channel and every equivalence passed`

Before the flip the same fixture at the same tip reported 13 recorded; the
difference is the modes landing on main, not this lane's code, which the gate
did not touch.

### Verified

Every clause of TUI-005 now asserts with no recorded case behind it, both
dependencies (TUI-002, TUI-003) are verified on main, and stage 3 is green
including `compat/attached-client.sh`, which the lane could not get past at its
own tip: the step it stopped on (`zz screen did not visibly become copy-mode
within 10 seconds`) was the modes lane's, and with modes on main the whole
fixture reaches `attached-client compatibility: PASS`.

### Nits from the review, and what became of them

1. `knowledge/tmux/key-tables.md` zone excursion: left as it is. No keys or
   modes lane touched that sentence, so there was no collision to settle.
2. Evidence carrying revision 4f4f0b0b rather than the pushed tip: answered.
   Every proof was re-run at the gate tip and is recorded here as `gate-*.txt`.
3. The emacs half of the clamp group not catching the sabotage: agreed, left.
4. `copy_mode_key_owners` aggregating per client: written down here, since the
   review asked for a sentence somewhere. With two clients in copy mode on one
   pane a key sent to the pane runs once per live copy session; the pin has one
   mode per pane, so no oracle case can see it, and it is the same per-client
   aggregation `formats.pane-runtime` already records for `pane_in_mode`.
5. The named zz-daemon flake did not fire at the gate either: 886 of 886.
