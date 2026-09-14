# TUI-009 attempt-05, the cycle 6 caps lane: review and what the gate did with it

## Reviewer verdict, verbatim

```json
{
  "lane": "caps",
  "verdict": "approve",
  "confirmed_defects": [
    {
      "obligation": "TUI-009",
      "severity": "nit",
      "description": "Two documents describe the unknown-name rule for the new wire message differently. crates/zz-protocol/src/message.rs's doc on ClientTerminalFeatures says the daemon folds the names \"stopping at the first name it does not know\"; knowledge/protocol/wire-protocol.md's v102 entry says \"unknown names are dropped\". The code does the second on this path: Shared::add_client_terminal_features hands each Vec element to terminal_feature_mask as its own spec, so an unknown element drops and the following elements still land. (terminal_feature_mask's own `break` is faithful to the pin's tty_add_features, which does abandon the rest of a single comma-joined spec, so only the message.rs sentence is wrong.) Nothing measurable moves: the client only ever sends names terminal_features_list produced.",
      "suggested_fix": "Reword the message.rs doc to \"unknown names are dropped\", matching wire-protocol.md and the code."
    },
    {
      "obligation": "TUI-009",
      "severity": "nit",
      "description": "Exit-time Dseks is conditioned on having armed. crates/zz-tui/src/tty.rs's Drop writes EXTENDED_KEYS_DISABLE only when EXTENDED_KEYS_ARMED was set; the pin's tty_stop_tty (tty.c:494) writes tty_term_string(TTYC_DSEKS) whenever the terminal carries extkeys, whatever the option says. So on a terminal that named extkeys with extended-keys off, the pin emits \\e[>4m on exit and zz emits nothing. Invisible through capture-pane, so no fixture row moves and no clause is affected.",
      "suggested_fix": "Gate the exit write on terminal_carries_extended_keys() rather than on having armed, if the campaign later drives raw exit bytes."
    },
    {
      "obligation": "TUI-009",
      "severity": "nit",
      "description": "arm_extended_keys() is called at the end of TerminalGuard::enter, so a client whose own -T named extkeys writes \\e[>4;2m immediately. The pin never writes Eneks from tty_start_tty: its first chance is a DA reply, or the TTY_QUERY_TIMEOUT start timer firing with none (tty.c:319). Timing only; the end state agrees and the fixture's silent and decoder cases both assert.",
      "suggested_fix": "Nothing needed unless a later obligation measures the request's arrival time."
    },
    {
      "obligation": "TUI-009",
      "severity": "nit",
      "description": "The new daemon unit test a_reply_after_the_hello_joins_the_roster_the_terminfo_entry_started (crates/zz-daemon/src/daemon.rs) returns early and passes silently when `base.1.is_empty()`, i.e. wherever no terminfo entry for xterm is readable. On such a machine it proves nothing instead of failing. compat/tui-caps.sh covers the same ground and does fail, so no clause rests on it.",
      "suggested_fix": "Turn the early return into an explicit skip that is visible, or assert the entry is present so a terminfo-less box reports it."
    },
    {
      "obligation": "TUI-009",
      "severity": "nit",
      "description": "client_term_features builds a TtyTerm twice per expansion: it calls client_colour_count (which calls client_feature_mask -> client_terminal_facts -> TtyTerm::create) and then client_feature_mask again. terminfo_entries caches the raw entry list but TtyTerm::create is not cached, and client_colour_count also runs on every status compose because the stock theme colours read it.",
      "suggested_fix": "Compute the mask once in client_term_features and pass it to client_colour_count_with, or memoize TtyTerm per (TERM, COLORTERM, features, overrides)."
    }
  ],
  "checks_run": [
    "fetched origin/main (b17232ac) and campaign/tui-caps-3 (tip 1d7f6d0e, five commits, matches the worker report)",
    "detached checkout in /home/demfabris/dev/zz-tui-caps-8-review, worktree clean at start and at finish",
    "touched crates/**/*.rs and Cargo.* then rebuilt: CARGO_TARGET_DIR=/home/demfabris/dev/zz-tui-caps-8/target, cargo build -p zz through the two-slot 5G wrapper, exit 0 (this also proves the GUI crate still builds)",
    "compat/tui-caps.sh at tip, run 1: exit 0, 366 asserted rows, 0 recorded, md5 cb21624a8b09057727430af0edb69cb3 -- byte-identical to all three of the worker's evidence runs",
    "compat/tui-caps.sh at tip, run 2: exit 0, same md5 (determinism confirmed live, not just from evidence)",
    "compat/tui-caps.sh --self-check at tip: exit 0, 27 sabotages caught, 6 controls quiet, md5 b65555c16e1ea4abe8c81c5a891c2f22 -- byte-identical to the evidence file",
    "compat/tui-screen-diff.sh at tip: exit 0, 147 asserted checkpoints identical, 6 recorded, byte-identical to the evidence file",
    "compat/attached-client.sh (TMUX_BIN=pin, ZZ_BIN=my tip build): exit 0, attached-client compatibility: PASS",
    "compat/status-row.sh under LC_ALL=C LC_TIME=C: exit 0, all 14 comparisons identical, none recorded",
    "MY OWN SABOTAGE: deleted the `_ if !crate::tty::terminal_takes_utf8()` arm from render.rs write_glyph, rebuilt, re-ran compat/tui-caps.sh -> exit 1 with exactly one row differing, `DIFF widths/non-utf8/line: tmux W ____ | _ | __ |$, zz W <CJK pair> | <combining> | <emoji> |$`; restored render.rs, rebuilt, binary sha256 back to dfcefb52a803996158e8d3fda0bc50920fa44f3ac35bf35f0b430d3c989909f8 and worktree clean",
    "ORACLE SPOT-CHECK, driven by me: pinned tmux d77c9dc6 on a throwaway -L socket with -f /dev/null, scrubbed HOME and XDG_CONFIG_HOME, attached through script(1) so the terminal answers nothing, TERM=xterm -> COLOURS=8 FEAT=bpaste,ccolour,clipboard,cstyle,focus,title. That is exactly the silent/bare row the fixture asserts and exactly what the new client_feature_mask derives. Server killed, scratch removed.",
    "ORACLE SPOT-CHECK of the codeset rule: the sabotaged run printed the pin's own line, `W ____ | _ | __ |` -- width underscores for the wide pair, the combining sequence and the emoji, and no ACS. So for everything this fixture draws the pin never reaches tty_acs_reverse_get, which is what makes the matrix's NAMED ACS channel not a hidden divergence",
    "read the pin: tty-features.c tty_default_features (8 entries) matches the port name-for-name including TTY_FEATURES_BASE_MODERN_XTERM; tty-keys.c tty_keys_device_attributes2 (M/T/U) and tty_keys_extended_device_attributes (7 prefixes) match secondary_device_attributes_name and extended_device_attributes_name; tty.c tty_update_features writes Eneks only under option AND tty_term_string(TTYC_ENEKS), and tty_start_tty writes it never; tty.c tty_check_codeset matches write_glyph; tty-term.c tty_term_create mutates *feat from the terminal-features array, COLORTERM, the VT100LIKE check and an RGB entry, which is what TtyTerm::requested_features() now exposes",
    "cargo test -p zz-protocol -p zz-mux -p zz-tui --jobs 3 -- --test-threads=3: exit 0, 20 result lines all ok",
    "cargo test -p zz-daemon --jobs 3 -- --test-threads=3: exit 0, 12 result lines all ok (895 lib tests), no client_focus_closes_display_panes flake",
    "cargo test -p zz --jobs 3 -- --test-threads=3: exit 0, 11 result lines all ok, cli_binary's 125 included",
    "cargo test -p zz-tui under LC_ALL=C LANG=C: exit 0, 207 ok -- the new locale-sensitive glyph path introduces no locale-dependent test",
    "cargo clippy -p zz-protocol -p zz-mux -p zz-daemon -p zz-tui --all-targets --all-features -- -D warnings: exit 0, zero error/warning lines",
    "python3 compat/tui/tracker.py check and python3 compat/tmux-tracker.py check: both exit 0; re-running both write-report leaves git status empty, so the generated reports are current",
    "DELTA CORPUS: compat/run.sh --strict-geometry --delta origin/main...HEAD --list selects 144 rows, all smoke (touched_commands is empty, so nothing command-matched). Ran 18 of them in three chunks, chosen for client-fact, format, census, terminal and keys content: smoke/format-listing, format-modifier-interrogate, terminal-facts, pane-colours-palette, client-utf8-sanitizer, client-non-utf8-cwd, display-message-client-aliases, format-client-loop-context, format-modifier-client-loop, default-client-command, census-options, census-formats, known/known-terminal-runtime, keys-prefix-stock, keys-shift-attached, keys-table-lifecycle, client-resized-context, client-exit-actions. Every one TOPO/GEO/FMT/OUT/WARN clean except known/known-terminal-runtime at its exact documented 0 0 6 0 0; both chunk exits 0",
    "LEDGER OWNERSHIP: field-level diff of compat/tui/campaign.json shows exactly one record changed (TUI-009) and exactly four fields (status active->review, evidence_note, next_action, sources +3). proof stays null. TUI-006, TUI-008, TUI-011, TUI-012, TUI-014..TUI-018, the baseline, the milestones and the pin are untouched",
    "GAPS: compat/tmux-gaps.json removes exactly one item, option:extended-keys, from options.client-terminal-negotiation (a gap the batch names), with a dated 2026-09-13 measurement appended to the group reason and option:terminal-features explicitly kept open. The TMUX_OPTION_CONSUMERS line and the compat_manifest_tests.rs counts (145->146, scope [36,47,43,19]->[37,47,43,19], tracked 35->34) are in the SAME commit 7eb3eb85, with knowledge/tmux/gaps.md",
    "WIRE: PROTOCOL_VERSION stays 102 (message.rs:21 and its own test at 4956). ClientTerminalFeatures is the last variant of ProtocolMessage, appended after EnvironmentResponse, nothing renumbered. The consumer half (handle_connection dispatch arm + Shared::add_client_terminal_features) and the v102 version-history line in knowledge/protocol/wire-protocol.md are in the same commit 1d5a5d98. Bounded deserializer with two exported constants",
    "ZONES on git diff origin/main...HEAD (three dots): crates/zz-tui touches only render.rs and tty.rs; render.rs's two hunks are blit_row's cell write and the new write_glyph before write_cell_ground, nothing reflowed. No input.rs, app.rs, state.rs, chrome.rs, chooser or TUI command-roster file. No crates/zz/ file at all, so no GUI presentation change. Four excursions (zz-mux terminfo.rs, command.rs, compat_manifest_tests.rs, zz-daemon lib.rs) are all declared in the worker's notes",
    "HONESTY: 16 evidence files, none named .log, git check-ignore -v over the directory exits 1 (nothing ignored). No .log anywhere in the diff. No attribution trailers in any of the five commit messages. No added plain `//` comments in any .rs file (only /// docs, matching the surrounding style)",
    "HYGIENE: no binary copied under /tmp; /tmp back to 237M; pgrep -fa 'zz-cli-|zz-user|zzprobe' finds nothing of mine; the only live zz daemons belong to the zz-tui-keys-8 and zz-tui-commands worktrees and were left alone, as were the user's default sockets; every probe server was -L zzprobe-* -f /dev/null under a scratch HOME and was killed"
  ],
  "every_clause_asserted": "yes",
  "notes": "See above."
}
```

## What the gate did with it

The verdict is **approve** with five nits and no must-fix, so nothing bound this gate to a
change. One nit was cheap, provable and about a document contradicting its own code, so it was
fixed in its own commit; the other four are recorded here and carried into next_action.

### Nit 1, fixed: 63459a09 "Say the message drops a name it does not know"

The doc on `ProtocolMessage::ClientTerminalFeatures` said the daemon folds the names "stopping at
the first name it does not know". `knowledge/protocol/wire-protocol.md`'s v102 entry said unknown
names are dropped. The reviewer is right that the second is what this path does, and the gate read
the two functions before touching either sentence:

- `Shared::add_client_terminal_features` (crates/zz-daemon/src/daemon.rs) calls
  `terminal_feature_mask(features.iter().map(String::as_str))`, so every `Vec` element arrives as
  its own spec.
- `terminal_feature_mask` (crates/zz-daemon/src/terminal_features.rs) splits a spec on `:` and `,`
  and `break`s out of that inner loop at the first name it cannot map, then moves to the next spec.

So a name it does not know costs the rest of its own element and nothing after it. The `break` is
faithful to the pin's `tty_add_features`, which abandons the rest of one comma-joined spec, so the
function was left alone and only the sentence moved. Proof that nothing else moved: workspace
clippy `-D warnings`, `cargo fmt --all --check` and every package's tests are exit 0 at the tip
that carries it, and `compat/tui-caps.sh` still reports the byte-identical md5 the reviewer got
(cb21624a8b09057727430af0edb69cb3).

### Nits 2 and 3, recorded, not chased

Exit-time Dseks conditioned on having armed, and `arm_extended_keys()` running at the end of
`TerminalGuard::enter` rather than on a reply or the query timer. Both are about raw bytes on a
terminal nothing here reads: `capture-pane` cannot see either, no fixture row moves and no clause
is affected. They belong to whichever obligation first drives raw entry and exit bytes, and they
are named in TUI-009's next_action so that obligation inherits them instead of rediscovering them.

### Nit 4, recorded, deliberately not chased at a gate

`a_reply_after_the_hello_joins_the_roster_the_terminfo_entry_started` returns early where no
terminfo entry for `xterm` is readable. The reviewer's own two options are to make the skip
visible or to assert the entry is present. The second turns a machine without a terminfo database
into a red test in CI, which is a behaviour change this gate is not the right place to make on its
own judgement; the first needs a skip channel libtest does not have without a harness change. The
clause does not rest on it either way: `compat/tui-caps.sh` covers the same ground from the
outside and does fail. Carried into next_action.

### Nit 5, recorded, not chased

`client_term_features` building a `TtyTerm` twice per expansion. It is a cost, not a divergence,
and the fix the reviewer sketches (`client_colour_count_with`, or memoizing `TtyTerm` per TERM /
COLORTERM / features / overrides) is a refactor with its own measurement, not gate work. Carried
into next_action.

## Sibling flips

None. This lane's `sibling_cases` list is empty, so there was nothing to flip. The other side
agrees: `compat/tui-choosers.sh` at the gate tip reports "all 53 asserted comparisons identical, 4
recorded not asserted (0 for a sibling lane)", and `compat/tui-caps.sh` records nothing at all.

## What the gate re-measured for itself

Every fixture in the gate's stage-3 list, the whole 144-row delta corpus, and every cargo command,
on the caps lane rebased onto the accumulated main (origin/main c8086f06, which already carries the
cycle 6 keys lane). Not one number here was copied from the lane or from the reviewer:
`gate-tui-caps.txt` and `gate-tui-caps-self-check.txt` are this gate's own runs, and they came back
byte-identical to the lane's evidence at md5 cb21624a8b09057727430af0edb69cb3 and
b65555c16e1ea4abe8c81c5a891c2f22.

## One thing the gate could not re-measure mechanically

`compat/tui/verify-claims.py` landed on main in c8086f06, one commit before this gate, to re-read a
verified obligation's fixture tally instead of trusting the claim. Its `TALLY` regex matches two
spellings, "`N` recorded not asserted" and "`N` assert none and are recorded in full".
`compat/tui-caps.sh` prints neither: its summary is "`N` asserted rows, `M` recorded rows". So
`--run TUI-009` parses a recorded count of 0 out of any tui-caps run whatsoever, red or green, and
would have waved this claim through even if the fixture recorded fifteen rows. The same hole covers
`compat/tui-mouse.sh` ("`N` asserted checks, `M` recorded checks", TUI-008),
`compat/tui-stock-keys.sh` and `compat/tui-copy-mode.sh` ("`M` recorded a difference elsewhere",
TUI-003 and TUI-005) and `compat/tui-output-backpressure.sh` ("all `N` assertions passed, `M`
recorded", TUI-010). The gate left the tool alone: it belongs to whoever wrote it, a strict parser
would newly flag verified obligations whose fixtures legitimately record cases outside their
clauses (TUI-003 reports 8), and that is a decision about what "recorded" means per obligation, not
a regex. TUI-009's own tally was therefore read by hand instead: `gate-tui-caps.txt` says "366
asserted rows, 0 recorded rows" and "all 366 asserted rows identical".
