# TUI-009 attempt-05, cycle 8, the caps lane

Alienware, CachyOS, against pinned tmux d77c9dc6. `environment.txt` carries the
box, both binaries and the tip. attempt-01 to attempt-04 are read-only history;
attempt-04/notes.md holds cycle 7's mouse work and the gate's menu-arming fix,
and its "15 rows that still record" section is the punch list this attempt
closes.

## The files

- `environment.txt`
- `tui-caps-run-1.txt`, `-2`, `-3` — three runs at the tip, byte-identical
  (md5 `cb21624a`): 366 asserted rows, 0 recorded.
- `tui-caps-self-check.txt` — 27 one-sided sabotages caught, 6 controls quiet.
- `tui-screen-diff.txt`, `tui-screen-diff-self-check.txt`
- `attached-client.txt`, `status-row-c-locale.txt`
- `tui-indicators.txt`, `tui-stock-keys.txt`
- `corpus-formats-and-palette.txt` — smoke/pane-colours-palette,
  smoke/format-listing, smoke/format-modifier-interrogate.
- `corpus-census-and-terminal-runtime.txt` — census-options, census-formats,
  smoke/terminal-facts, known/known-terminal-runtime.
- `cargo.txt`

## What moved

`compat/tui-caps.sh` asserts **366** rows and records **none**, from 351 and 15
at origin/main. All fifteen recorded rows closed, in the three causes
attempt-04 named.

### 1. The capability wire, thirteen rows

`client_colours` on `facts/bare`, `facts/-2`, `facts/-u`, `facts/-T` and
`facts/utf8-locale`, and `client_termfeatures` on those five plus
`silent/bare`, `silent/-T` and `silent/-2`.

The pin's `c->term_features` is two halves. `tty_term_create` derives one from
the terminfo entry, the `terminal-features` array (whose stock value gives
`xterm*` clipboard, ccolour, cstyle, focus and title), `COLORTERM`, the
VT100-like check (bpaste, focus, title) and an RGB-capable entry. Replies give
the other: `tty_keys_device_attributes2` reads a secondary DA's first parameter
as a letter and `tty_keys_extended_device_attributes` reads an XTVERSION reply's
text, and both hand `tty_default_features` a terminal name. zz had neither: a
fixed fourteen-name list stood in for the first and nothing carried the second,
because `ClientHello` goes out before `TerminalGuard::enter` has even asked.

Both halves land here.

- `crates/zz-protocol/src/message.rs` appends
  `ProtocolMessage::ClientTerminalFeatures { features: Vec<String> }` at the
  tail of the enum, bounded at 64 names of 64 bytes on decode. Named in the
  v102 entry of `knowledge/protocol/wire-protocol.md`.
- `crates/zz-daemon/src/terminal_features.rs` carries `tty_default_features`'s
  own table, the eight names the pin can learn from a reply.
- `crates/zz-tui/src/tty.rs` turns each reply into that table's list,
  `crates/zz-daemon/src/client.rs` keeps the union, reports it over the
  interactive connection the client already holds (the writer is now an `Arc`
  the connect path registers, so the report goes out under the same mutex every
  other message uses — no second connection and no second writer), and folds it
  into the next hello as one more `client-features-v1:` token so a reconnect
  does not wait for the terminal to answer twice.
- `crates/zz-daemon/src/daemon.rs` folds a client's mask from three sources in
  `client_feature_mask`: the hello's flag specs, this message, and
  `client_terminal_facts`'s own `feat` for the client's TERM, which
  `crates/zz-mux/src/terminfo.rs` now exposes as `requested_features` (it kept
  `tty_term_create`'s `requested` set and threw it away). `client_termfeatures`
  is `tty_get_features` of that mask and nothing else: the old code also raised
  `256` and `RGB` from the colour count, which the pin never does — a
  `xterm-256color` client takes 256 colours out of terminfo's `colors` number
  and carries no `256` feature.

Measured: on the outer decoder both sides now answer
`256,bpaste,ccolour,clipboard,hyperlinks,cstyle,extkeys,focus,mouse,overline,progressbar,RGB,strikethrough,title,usstyle`
and 16777216 colours, `facts/-T sixel` adds `sixel` on both, and on a terminal
that answers nothing both answer `bpaste,ccolour,clipboard,cstyle,focus,title`,
`-T RGB` adding `RGB` and `-2` adding `256`.

### 2. The codeset, one row

`widths/non-utf8/line`. `tty_check_codeset` passes a cell whose single byte is
below `0x7f`, passes everything for a client with `CLIENT_UTF8`, and otherwise
sends `data.width` underscores. `crates/zz-daemon/src/client.rs` exports the
client's own UTF-8 answer as `client_takes_utf8_terminal` (it already computed
it for the hello capability, one line above the colour count that was exported),
`crates/zz-tui/src/tty.rs` reads it once the way `tmux.c` does, and
`render.rs`'s `write_glyph` — the one place `blit_row` writes a cell — ports the
rule. Both sides now draw `W ____ | _ | __ |` for the wide pair, the combining
sequence and the emoji.

The ACS half of `tty_check_codeset` is NOT ported and is named in the fixture's
matrix instead: the pin maps a cell to an ACS key before it falls back to
underscores and writes it under `GRID_ATTR_CHARSET`, and the raw TUI's cell
writer has no charset attribute to write it under. Nothing this fixture draws
is in `tty_acs_reverse2`/`tty_acs_reverse3`, and the chrome that is is compared
by `compat/tui-screen-diff.sh` under UTF-8 clients only.

### 3. The extended-key condition, one row

`silent/extended pane_key_mode`. `tty_start_tty` writes `Eneks` never;
`tty_update_features` writes it while `extended-keys` is on, and
`tty_term_string(TTYC_ENEKS)` is empty unless the terminal carries `extkeys`.
`TerminalGuard::enter` armed `\e[>4;2m` from the option alone. It now sets the
option aside and `arm_extended_keys` fires once — on entry for a client whose
own `-T` named `extkeys`, and again from each reply — so a silent
`TERM=xterm-256color`, which names none, is left at `VT10x` on both sides while
the outer decoder's case still reads `Ext 2` on both.

`option:extended-keys` leaves `compat/tmux-gaps.json`'s
`options.client-terminal-negotiation` with that measurement, and moves into
`crates/zz-mux/src/command.rs` `TMUX_OPTION_CONSUMERS` with the
`compat_manifest_tests.rs` partition counts in the same commit (145 to 146
consumers, server scope 36 to 37, 35 to 34 tracked option items).
`option:terminal-features` does NOT close: the daemon reads the array now, the
raw TUI's own arming still does not, so an array entry granting `extkeys` arms
the pin and not zz. That shortfall is named in the fixture's matrix.

## The fixture

Three sabotages come with the three closures, and one moves.

- `sc/one-sided-replies`: `SILENT_SIDE` puts one side alone behind the relay, so
  zz learns nothing while the pin takes tmux's `tty_default_features`.
  `client_colours` and `client_termfeatures` are the rows that carry it.
- `sc/silent-extkeys`: `-T extkeys` for zz alone on a silent terminal, with
  every mode row but `pane_key_mode` recorded, so it can only report there.
  `sc/silent-extended-control` is the same case with neither side named, quiet.
- `sc/one-sided-codeset`: `-u` for zz alone under a C locale, which puts the
  codepoints back on zz's line against the pin's underscores. The cursor column
  is the same either way, because the underscores fill the cells the glyph
  would.
- `sc/one-sided-2` moves to the silent terminal as `sc/silent-2`. On the outer
  decoder both rosters already carry 256 and RGB out of the tmux reply, so `-2`
  adds nothing on either side and the sabotage stopped catching — which is the
  pin's own behaviour, not a hole.

27 sabotages caught with 6 controls quiet, from 24 and 5.

## What else was measured, and stayed green

`compat/tui-screen-diff.sh` 147 asserted checkpoints identical with 6 recorded,
and its self-check clean. `compat/attached-client.sh` PASS.
`compat/status-row.sh` under `LC_ALL=C LC_TIME=C`, 14 comparisons identical.
`compat/tui-indicators.sh` 23 identical. `compat/tui-stock-keys.sh` 50 cases,
8 recorded elsewhere, no flake this run. Corpus: smoke/pane-colours-palette,
smoke/format-listing, smoke/format-modifier-interrogate, census-options,
census-formats and smoke/terminal-facts all clean;
known/known-terminal-runtime carries its exact documented 6 FMT divergences.
`cargo test` and `cargo clippy -- -D warnings` clean on zz-protocol, zz-mux,
zz-daemon and zz-tui, and `cargo test -p zz` clean.

The GUI is unchanged: it never sends the new message, it reads
`client_termfeatures` through the same daemon path, and nothing in
`crates/zz/src` moved.

## GATE ADDENDUM, 2026-09-14, cycle 6 caps

The lane tip `1d7f6d0e` was rebased onto `origin/main` `c8086f06` (which already
carries the cycle 6 keys lane and the orchestrator's `verify-claims.py` landing)
and pushed. Two conflicts, both resolved by union and neither inside a file this
lane owns alone: `knowledge/protocol/wire-protocol.md`, where the keys lane's
`border` field on `InputMessage::MouseKey` and this lane's
`ProtocolMessage::ClientTerminalFeatures` both append to the v102 entry and both
sentences are kept; and `knowledge/tmux/gaps.md`, which is generated and was
regenerated from the merged `compat/tmux-gaps.json` with
`python3 compat/tmux-tracker.py write-report`. `PROTOCOL_VERSION` is still 102.

One commit was added on top of the lane's five before the records commit:
`63459a09`, the reviewer's nit 1, a doc sentence in
`crates/zz-protocol/src/message.rs` that contradicted both `wire-protocol.md` and
the code. Everything the gate ran is in `review.md` and in these files:

- `gate-tui-caps.txt`, `gate-tui-caps-self-check.txt` -- the gate's own runs, md5
  `cb21624a8b09057727430af0edb69cb3` and `b65555c16e1ea4abe8c81c5a891c2f22`,
  byte-identical to the lane's evidence
- `gate-tui-screen-diff.txt`, `gate-tui-screen-diff-self-check.txt`
- `gate-tui-pane-geometry.txt`, `gate-status-row-c-locale.txt`
- `gate-tui-indicators.txt`, `gate-tui-indicators-self-check.txt`
- `gate-tui-overlays.txt`, `gate-tui-overlays-self-check.txt`
- `gate-tui-choosers.txt`, `gate-tui-choosers-self-check.txt`
- `gate-tui-stock-keys.txt`, `gate-tui-stock-keys-self-check.txt`
- `gate-tui-copy-mode.txt`, `gate-tui-copy-mode-self-check.txt`
- `gate-attached-client.txt`
- `gate-corpus.txt` -- all 144 delta rows, not a sample
- `gate-cargo.txt`, `gate-environment.txt`, `review.md`

The gate read `git diff --stat origin/main...HEAD` at the rebased tip: the crates
half is exactly `zz-daemon` (client.rs, daemon.rs, lib.rs, terminal_features.rs),
`zz-mux` (command.rs, compat_manifest_tests.rs, terminfo.rs), `zz-protocol`
(lib.rs, message.rs) and `zz-tui` (render.rs, tty.rs). That is the lane's four
declared excursions and nothing else; no `crates/zz/` file is touched, so no GUI
presentation moved.
