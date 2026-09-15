# TUI-018 gate review, stream lane, cycle 10

Reviewed at `aa65960f` on the ubuntu box, 2026-09-15. Verdict **approve-with-fixes**, no blockers.
The gate applied every must-fix and every nit in its own commit, measured the one unmeasured
divergence the notes named, and left TUI-018 at `review` rather than `verified` because that
measurement put a recorded case inside clause 2.

## The verdict, verbatim

```json
{
  "lane": "stream",
  "branch": "campaign/tui-stream",
  "tip": "aa65960fe1a40722d52c8b922e79962daef6666e",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "obligation": "TUI-018",
      "severity": "must-fix",
      "description": "compat/tui-command-streams.sh carries no `export ZZ_TRAY=0`. Main added that header to compat/run.sh and every compat/tui-*.sh in the ten desktop commits past the base (compat/tui-client-commands.sh:122 at origin/main); the branch is based on 3ecd4702, which predates it, so `git grep -l ZZ_TRAY HEAD -- compat/` returns nothing. The rebase restores the header in the files main already had, but tui-command-streams.sh is a new file and no conflict will surface it, so it will land as the only compat fixture that starts a zz daemon by hand (`zz_command -f /dev/null daemon`, line 690) with no tray suppression.",
      "suggested_fix": "Add `export ZZ_TRAY=0` beside `set -eEuo pipefail` in compat/tui-command-streams.sh, matching origin/main:compat/tui-client-commands.sh line 122."
    },
    {
      "obligation": "TUI-018",
      "severity": "must-fix",
      "description": "knowledge/designs/tmux-superset-roadmap.md milestone 5 is untouched and still reads 'Only after the practical alias gate is green, measure demand for binary stdin/stdout and lock processes. If required, design one bounded command-stream channel for display-message -I, split-window -I, load-buffer -, save-buffer -, and source-file -.' (lines 1233-1238). The channel is designed and built at this tip, and knowledge/designs/command-stream-channel.md's own frontmatter says 'milestone 5 of the tmux superset roadmap'. The batch listed this file (milestone 5 only) as in-zone, and knowledge/designs/index.md's own preamble says this section must never describe unbuilt work as current behaviour - the inverse is just as stale.",
      "suggested_fix": "One paragraph in milestone 5 recording that the channel was designed and built on 2026-09-14 under TUI-018, pointing at knowledge/designs/command-stream-channel.md and naming the 1 MiB bound as the decided difference."
    },
    {
      "obligation": "TUI-018",
      "severity": "nit",
      "description": "crates/zz-daemon/src/daemon.rs: the new `feed_pane_stream_input` was inserted directly under `record_command_stderr`'s existing doc comment, so '/// Append one line to the running Command request's stderr.' now documents feed_pane_stream_input (whose own `/// window_pane_input_callback: ...` lines follow it) and record_command_stderr is left with no doc at all. Visible at daemon.rs:23661-23679.",
      "suggested_fix": "Move '/// Append one line to the running Command request's stderr.' back above `fn record_command_stderr`."
    },
    {
      "obligation": "TUI-018",
      "severity": "nit",
      "description": "Three plain `//` comment blocks are added against the repo's 'do not add comments in code' rule: crates/zz-mux/src/command.rs (the alias-group stream comment at ~4468 and the cmd_display_message_exec comment at ~8418) and a four-line block in crates/zz-tui/src/render.rs place_viewport_cursor. render.rs carries zero `//` comments at origin/main, so the branch's block is the only one in that file, and it sits inside the declared zone excursion.",
      "suggested_fix": "Drop the three blocks, or fold what they say into the design document, which already carries the tty_update_mode/tty_cursor rule."
    },
    {
      "obligation": "TUI-018",
      "severity": "nit",
      "description": "compat/tui/evidence/TUI-018/attempt-01/environment.txt contradicts itself twice. It records 'based on origin/main: 45481a72b9ef300365eec484a0c293552dd8acfe', but the branch's merge-base with origin/main is 3ecd4702f06afe6586d845acae34be7c02f10ec5 (45481a72 is an older ancestor). It also lists a RUN_ENV block naming ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux and then says the fixtures actually used the lane worktree's own copy of the pin (binary-cksum 4253032590, 7455240 bytes) - a different build from the shared pin, which is 7455232 bytes. Same tmux commit, so the results stand (I reran everything against the shared pin and got the identical summary lines), but the record says two different things.",
      "suggested_fix": "Correct the base revision to 3ecd4702 and state plainly which pin binary the recorded runs used."
    },
    {
      "obligation": "TUI-018",
      "severity": "nit",
      "description": "compat/tmux-gaps.json protocol.binary-streams' third acceptance clause now reads 'A caller's non-UTF-8 argv word reaches the daemon as bytes over the typed protocol. The remaining refusal is show-buffer's binary policy.', while the group's item list still carries semantic:non-utf8-command-arguments for its remaining consumers. The first clause names both; the third drops one.",
      "suggested_fix": "Reword the third clause to match the first: the remaining refusals are the non-UTF-8 argv word's remaining consumers and show-buffer's binary policy."
    }
  ],
  "checks_run": [
    "git fetch origin main + campaign/*; worktree confirmed clean and detached at aa65960f, merge-base 3ecd4702",
    "cargo build -p zz --bin zz at the tip: green in 4m39s (GUI builds; gpui fork resolves)",
    "RUN_ENV compat/tui-command-streams.sh x3: green, every run 'all 33 asserted comparisons identical, 0 recorded not asserted, 4 decided (0 for a sibling lane)' (128-131s each)",
    "RUN_ENV compat/tui-command-streams.sh --self-check: green, four sabotages each caught in its own channel plus both equivalences",
    "RUN_ENV compat/tui-client-commands.sh x3: green, every run 'all 65 asserted comparisons identical, 33 recorded not asserted (0 for a sibling lane)'",
    "RUN_ENV compat/tui-client-commands.sh --self-check: green",
    "MY OWN SABOTAGE: copied the fixture to the scratchpad and made zz read /dev/null for source-file only, i.e. a channel that reads stdin and drops it. Result: exit 1, '33 of 33 asserted comparisons differ' - the option read-back catches it, and the shared state channel cascades it to every later case",
    "cargo test -p zz-protocol -p zz-mux -p zz-tui --all-features: green",
    "cargo test -p zz-daemon --all-features with HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=.../config --skip russh_socks: green, 902 passed 0 failed",
    "cargo test -p zz --all-features: green (638 + 125 cli_binary, 0 failed)",
    "cargo clippy -p zz-protocol -p zz-mux -p zz-daemon -p zz-tui -p zz --all-targets --all-features -- -D warnings: green",
    "cargo fmt --all -- --check: RED, one hunk in crates/zz-protocol/src/catalog.rs (command-prompt -P). Reproduced identically on origin/main's own copy of that file with rustfmt 1.9.0 - pre-existing on main, not this lane's",
    "python3 compat/tui/tracker.py check: green ('campaign.json is valid and knowledge/tmux/tui-parity.md is current')",
    "RUN_ENV python3 compat/tui/verify-claims.py --run TUI-018 --zz target/debug/zz: green, 'every verified obligation holds up'; the FIXTURES map entry for TUI-018 is present and maps to both fixtures (main's map has none)",
    "RUN_ENV compat/check.sh from the worktree: green (exit 0, 194s)",
    "RUN_ENV compat/run.sh --strict-geometry --delta origin/main...HEAD --commands source-file,save-buffer,display-message,split-window,load-buffer,send-keys --list: 219 rows",
    "Delta corpus chunk 1 (12 rows, source-file/config family): clean",
    "Delta corpus chunk 2 (12 rows: pane-spawn-options, pane-spawn-retain, nested-splits, splits-sized, smoke/split-window-wait, smoke/remain-on-exit-format, display-message, smoke/display-message-client-aliases, smoke/command-flag-errors, smoke/cheap-flags, smoke/buffer-standard-streams, smoke/cli-output-bytes): clean, 0 TOPO/GEO/FMT/OUT/WARN divergences on every row",
    "compat/attached-client.sh with the pin and my build, twice solo: RED both times at probe_command_output_navigation zz, wait_for_current_marker 'ATTACHED_NAV_65 ATTACHED_NAV_MATCH' after n (search-again) - see notes",
    "ORACLE: source-file - applies its stdin on both sides (show-options reads the value back); bad line answers '-:1: unknown command: ...' on STDOUT with exit 1 on both",
    "ORACLE: save-buffer - of a \\303\\251 b \\377 c \\0 d \\n is byte-identical on both sides under od -c",
    "ORACLE: display-message -I against a live shell pane answers 'pane is not empty' exit 1 on both; split-window -I exits 0 on both and builds a pane with pane_dead=0 and no pid carrying the piped bytes",
    "ORACLE: caller closes the descriptor (<&-, not /dev/null) for load-buffer - and source-file -: both sides exit 0, create no buffer and apply nothing",
    "ORACLE: over-cap producer - zz refuses at the reader with 'zz: standard input exceeds 1048576 bytes' exit 1, the pin accepts 1048577 bytes; at-cap payloads identical on both sides for load-buffer and source-file",
    "PROCESS LEAK: no /tmp/zzcs.* scratch dirs and no zzcs-/zzcst- servers survive a green run or the exit-1 sabotage run",
    "git check-ignore -v over compat/tui/evidence/TUI-018: nothing ignored; no .log files in the evidence directory",
    "git log origin/main..HEAD for attribution trailers: none",
    "Added \\uXXXX escapes in compat/tmux-gaps.json and compat/tui/campaign.json: none",
    "Evidence honesty: my streams-run-1 case lines are identical to compat/tui/evidence/.../streams-run-1.txt, and my attached-client failure reproduces the worker's attached-client.txt byte for byte at the failing step",
    "git merge-tree --write-tree origin/main HEAD"
  ],
  "touched_commands": [
    "source-file",
    "display-message",
    "split-window",
    "load-buffer",
    "save-buffer",
    "send-text",
    "agent-send",
    "new-window"
  ],
  "touched_packages": [
    "zz-protocol",
    "zz-mux",
    "zz-daemon",
    "zz-tui",
    "zz"
  ],
  "every_clause_asserted": "Clause 1 ('Design one bounded command-stream channel covering stdin, stdout, binary bytes, backpressure, cancellation and process lifetime, and carry source-file -, display-message -I and split-window -I over it, or keep each refusal with the workload it would serve') - YES. knowledge/designs/command-stream-channel.md exists with OKF frontmatter and a status field ('Agreed and built 2026-09-14 for TUI-018, milestone 5 of the tmux superset roadmap; carried on protocol 103'), is listed in knowledge/designs/index.md's managed okf fence, and gives each of the six its own named paragraph. The code answers the document rather than the reverse: one reader (read_stdin_payload, crates/zz/src/lib.rs:1657, take(MAX_AGENT_SEND_BYTES + 1) then refuse if the extra byte arrived), one carrier (CommandInvocation.stdin, a serde tail append after expanded_alias_group, PROTOCOL_VERSION still 103), one resolver (zz_daemon::command_stdin_sink, crates/zz-daemon/src/daemon.rs) answering the exact three sinks the table names, asked by both the CLI and the daemon. All three previously refused forms carry, none is left refused, and I drove all three against the pin myself. Clause 2 ('Compare each form's exact stdout, stderr, exit status and the state it leaves behind, including that the pin's source-file - really applies its stdin') - YES. compat/tui-command-streams.sh compares all four channels per case (rc, stdout bytes via cmp, stderr bytes via cmp, and a state made of list-sessions, list-panes with dead/has-pid/size, list-buffers, five probe option values and the first pane rows); 33 asserted, 0 recorded, 4 decided, green three times plus a self-check. Nothing inside either clause is 'record', so no case holds a clause open. The pin-applies-its-stdin half is asserted twice (source-file-applies then source-file-applied-value reading the option back) and I reproduced it independently: 'set -g @probeA applied-on-pin' on stdin exits 0 on both and show-options -gqv answers applied-on-pin on both. The four decided cases are one fact, the 1 MiB cap; the fixture die()s if a decided case's reason does not contain the sentence, and the reason carries 'decided 2026-09-14 by the orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible' verbatim and names the workload it would serve ('bulk file transfer through a command client is a workload zz does not serve, because an unbounded stream lets one caller grow daemon memory without limit'). Payloads exactly at the cap are asserted identical on both sides for both load-buffer and source-file, so what is decided is the bound and nothing else. Punch list: (1) design first YES; (2) source-file - proving the pin applies YES; (3) save-buffer - stdout and binary bytes YES (byte-identical under od -c in my own probe); (4) display-message -I and split-window -I YES; (5) the fixture's shape YES with one deliberate departure it states and justifies - no outer pinned tmux, because window_pane_start_input returns before reading when the invoking client has a session, so the pin reads caller stdin only for a clientless client, and the attached half is asserted in compat/tui-client-commands.sh, which does run both binaries under one outer tmux; isolated HOME and XDG_CONFIG_HOME per side, short /tmp sockets, bounded wait_for (200 x 0.05s) plus a two-equal-reads settle, and a trap that kills both servers, the daemon pid, the sockets and the scratch dir - I confirmed nothing survives a green run or an exit-1 run; (6) TUI-018 set to review with both summary lines quoted verbatim and both reproduced by me. The one place a case name is stronger than what it does: source-file-closed-stdin, display-message-closed-stdin and split-window-closed-stdin feed /dev/null, an empty stream, not a closed descriptor. I drove the real closed descriptor (<&-) on both sides and both exit 0 and leave nothing, so nothing is hidden by the substitution.",
  "expected_gate_conflicts": "git merge-tree --write-tree origin/main HEAD exits 1 (tree a0fc8db2bf4300ee423ff924237d1aa082ee13b0) with three content conflicts: crates/zz-protocol/src/catalog.rs, knowledge/protocol/wire-protocol.md and knowledge/tmux/gaps.md. Everything else auto-merges, including compat/tmux-gaps.json, compat/tui-client-commands.sh, compat/tui/campaign.json, crates/zz-daemon/src/{daemon,lib}.rs, crates/zz-mux/src/command.rs, crates/zz-mux/tests/hunt_claims.rs, crates/zz-protocol/src/message.rs, crates/zz/src/lib.rs, knowledge/designs/index.md and knowledge/tmux/tui-parity.md. The catalog conflict is main's mouse work against this branch's display-message/split-window -I entries and the (supported, unsupported) = (486, 29) / usage_overrides.len() = 20 counters, which the gate must recount after the merge rather than take from either side. The wire-protocol conflict is the one v103 entry: main added view_action and press_action on InputMessage::MouseKey, this branch added the CommandInvocation.stdin sentence, and both belong in the merged entry. The gaps.md conflict is the regenerated protocol.binary-streams paragraph plus the surface counters, which come from compat/tmux-gaps.json and must be regenerated after the merge, not hand-resolved. One thing no conflict will surface: compat/tui-command-streams.sh is a new file, so main's `export ZZ_TRAY=0` header will not reach it - see the first must-fix.",
  "notes": "No blockers. Every clause the record claims is asserted at the tip, both summary lines reproduce exactly, the fixture bites when I sabotage it, and nothing is red that is this lane's. Two reds to hand the gate, both argued as not this branch's: (1) cargo fmt --all --check fails at the tip on crates/zz-protocol/src/catalog.rs (the command-prompt -P flag line), and it fails identically on origin/main's own copy of that file under rustfmt 1.9.0, so main is unformatted there and the branch only moved the line four rows up. (2) compat/attached-client.sh is red at the tip, twice solo, at probe_command_output_navigation on the zz side: after `n` (search-again) the view stays on ATTACHED_NAV_35 and never reaches ATTACHED_NAV_65. wait_for_current_marker only greps the captured screen text, so the branch's place_viewport_cursor change - which moves a hidden cursor and emits DECTCEM-off, and changes no cell - cannot produce it, and the branch touches no copy-mode or search path. It matches this box's documented intermittent failure in that exact probe at origin/main, and my run reproduces the worker's own attached-client.txt byte for byte. It is failing deterministically today rather than intermittently, so the gate should confirm it once against a main build before merging. Two things the summary does not name. First, an in-zone presentation change beyond the declared render.rs excursion: crates/zz-daemon/src/daemon.rs now feeds EMPTY_PANE_SCREEN_MODE (\\x1b[20h\\x1b[?25l, MODE_CRLF on and cursor off) into EVERY empty pane, both the split-window and the new-window spawn branches, not only -I panes. That is right - it is spawn.c's SPAWN_EMPTY - and pane-spawn-options, pane-spawn-retain and smoke/remain-on-exit-format are clean, but it changes what a split-window -E pane looks like in the GUI too, and it deserves a line beside the six-line render.rs excursion rather than being folded into it. The declared excursion itself is exactly what was declared: place_viewport_cursor now positions the cursor before hiding it instead of returning early, twelve diff lines of which four are the comment. Second, one unmeasured divergence I could not close for lack of budget, so it is suspicion and not a defect: crates/zz-mux/src/command.rs's alias-group path hands the caller's stream to EVERY member of an expanded command-alias group (`for mut command in commands { command.set_stdin(stream.clone()) }`), where the pin's file_read gives it to the first `-` and answers EBADF for a later one. Within a single invocation the branch models the pin correctly - source-file - - takes SourceStream::Spent and answers 'Bad file descriptor: -', asserted green by source-file-twice-one-stream - but a command-alias whose body holds two stream-sink members would apply the payload twice on zz and once on the pin. No fixture case covers it and it is not on the punch list. Worth one case, not worth holding the branch. Two smaller observations for the gate: the fixture's state channel is global, so one divergence cascades - my sabotage of only source-file turned all 33 asserted cases red rather than the twelve it touched. That over-reports rather than under-reports, so it is safe, but a DIFF list from this fixture will not localize the fault. And the worker's own delta-corpus4.txt says chunk 4 was cut off by the 600-second call cap with rows left unrun; I ran 24 rows of the 219-row delta selection myself, chosen as every row that names a touched command's changed behaviour plus the empty-pane and catalog-flag rows, and all 24 are clean, but the delta selection as a whole has not been run end to end by anyone."
}
```

## What the gate did with it

**MUST-FIX 1, ZZ_TRAY.** Confirmed at the tip: `git grep ZZ_TRAY -- compat/` named every other
compat fixture and not this one, and the fixture starts a daemon by hand at
`zz_command -f /dev/null daemon`. Added `export ZZ_TRAY=0` directly under `set -eEuo pipefail`,
matching `origin/main:compat/tui-client-commands.sh`. Proof: the fixture green and its `--self-check`
green after the change, both against the shared pin.

**MUST-FIX 2, milestone 5.** Confirmed the roadmap still described the channel as a future
"if required" design. Added one `**Complete:**` paragraph in the house style recording that the
channel was designed and built on 2026-09-14 under TUI-018, pointing at
`knowledge/designs/command-stream-channel.md`, and naming the 1 MiB reader bound as the one decided
difference from the pin. OKF frontmatter untouched; `compat/check.sh`'s knowledge checks pass.

**NIT 1, the doc line.** Confirmed at `daemon.rs`: the stderr recorder's doc comment sat above
`feed_pane_stream_input`. Moved it back above `fn record_command_stderr`; the new function keeps its
own `window_pane_input_callback` lines.

**NIT 2, the comment blocks.** Confirmed exactly nine added `//` lines in three blocks
(`crates/zz-mux/src/command.rs` twice, `crates/zz-tui/src/render.rs` once). Removed all three; a
diff of `crates/` against the merge-base now adds zero plain `//` lines. The design document already
carries the `tty_update_mode`/`tty_cursor` rule.

**NIT 3, environment.txt.** Corrected the base revision from `45481a72` to the real merge-base
`3ecd4702`, and replaced the self-contradicting pin note with a plain statement that the recorded
runs used the lane worktree's own build of the pinned commit (cksum 4253032590, 7455240 bytes) and
not the shared copy named in the RUN_ENV block (cksum 2524377659, 7455232 bytes). Same tmux commit
`d77c9dc6`. The gate reran both fixtures against the shared pin and got identical summary lines.

**NIT 4, the acceptance clause.** Reworded `protocol.binary-streams`' third clause so it names the
non-UTF-8 argv word's remaining consumers alongside show-buffer's binary policy, matching the first
clause and the item list. `tmux-tracker.py check` and `write-report` are green.

**(i) The empty-pane screen mode.** Confirmed on the code: `EMPTY_PANE_SCREEN_MODE` is fed on both
spawn branches, gated on the mux's single `empty` flag, which `pane_spawn_empty` raises for `-E`,
`-I` and an empty command word alike, so it reaches new-window and split-window panes in the GUI as
well as the TUI. Declared in TUI-018's `evidence_note` beside the render.rs excursion, with the
three clean corpus rows and the fact that it is reversible.

**(ii) The alias-group stream, measured.** See `evidence_note` and `next_action`. The pin gives a
multi-member `command-alias` body the caller's descriptor; the first `source-file -` applies and a
second answers `Bad file descriptor: -`. zz hands it to no member: `resolve_command_alias` rebuilds
a multi-member body as a fresh expanded-alias-group invocation and copies `source` but not `stdin`.
A single-member alias and a direct invocation match the pin and are now asserted. Not fixed here:
spending one stream across a group needs a spent marker `CommandInvocation` has no slot for, plus a
separate CLI fix for Argument sinks. Recorded by `source-file-alias-group-two-members` and its value
case, whose reason names `protocol.binary-streams` and TUI-018.

**(iii) cargo fmt.** Reproduced red at the tip and on `origin/main`'s own copy of
`crates/zz-protocol/src/catalog.rs`, so it was never this lane's. The gate committed the formatting;
main's own `51c858e2` landed the identical fix first, so the gate's commit dropped as a duplicate in
the rebase and `cargo fmt --all -- --check` is green at the pushed tip.

**(iv) The delta selection.** The reviewer ran 24 rows; the gate ran the whole 226-row selection end
to end in sequential chunks, skipping none. See `gate-delta-corpus.txt`.

## The two reds the reviewer handed over

`cargo fmt` is green, as above. `compat/attached-client.sh` is GREEN at the pushed tip: the box-reds
lane's one-line daemon fix (`ea6db587`, the engine knobs a command output view was missing) reached
this branch through the rebase, so the search-again probe now finds `ATTACHED_NAV_65`. The reviewer's
reading that nothing in this branch touched the copy-mode or search path is confirmed by the fix
landing elsewhere.
