# TUI-017 attempt-01, cycle 9 introspection lane

Base `origin/main` b6a57af1. Branch `campaign/tui-introspection`. Box: alienware,
CachyOS Linux, pin d77c9dc6. Clause 2 only; clause 1's six rich transports are
untouched and stay refused.

## Files

- `environment.txt` — the box, the pin, the zz binary's sha256, the toolchain,
  the cargo memory wrapper. Written first.
- `capture-pane-probe.txt` — 26 capture forms run against both a throwaway zz
  daemon and a throwaway pinned tmux server on an 80x24 pane holding three
  written rows: every explicit `-E` from 0 to 30, the default range, `-N`, `-J`,
  `-T`, `-e`, `-M`, `-a`, `-a -q` and a history start. Each prints both sides'
  exit status, line count, byte count, stderr and a `cat -A` diff.
- `attached-client.txt` — `compat/attached-client.sh` against this build.
- `corpus-capture.txt` — `compat/run.sh --strict-geometry` over capture-pane,
  display-message and every other corpus row that captures a pane.
- `package-tests.txt`, `clippy.txt` — every touched crate.
- `tui-copy-mode.txt` — `compat/tui-copy-mode.sh`, because `-M` now routes to
  the pane when no mode is open and copy mode is the other half of that
  routing: 147 cases agree on every channel they assert, 0 recorded.
- The fixture runs and the self-check are in `../../TUI-016/attempt-01/`; one
  fixture carries both obligations' cases.

## What the runs say

All 26 forms are byte-identical on both sides, `identical` on every one. Before
the landing: `-p` was 3 lines against the pin's 24, `-p -N -S 0 -E 0` was 25
bytes against 41, `-p -M` was `pane is not in a native mode` at exit 1 against
the pin's pane text, `-p -a` said `alternate screen is not active` against `no
alternate screen`, `-p -e -S 0 -E 2` kept one trailing cell the pin trims, and
`-p -a -q` printed nothing against the pin's one empty line.

Two of the five residues were not what the roster wrote down.

`-N` does not pad to the pane edge. `grid_string_cells` walks to `gl->cellsize`
under `GRID_STRING_EMPTY_CELLS`, and `grid_expand_line` rounds a row's cell array
up to a quarter, a half or the whole screen width, so on an 80-column pane a row
holding 24 cells prints 40 and a row holding 8 prints 20, while a row nothing
ever wrote prints empty. The landing reads that bucket off the cells the row
currently uses. The one fact it cannot reproduce: a row the pin once wrote wider
and later erased keeps the wider allocation there and not here, because the
allocation is a high-water mark and the snapshot is not.

The default range was not the only range losing rows. Every range past the last
written row lost them, which `-S 0 -E 3` through `-S 0 -E 30` show. And the last
row of any range was lost again in the printing: `cmd_capture_pane_exec` drops
one trailing newline off the buffer and then prints one, while zz's writer adds a
newline only when the text lacks one, so an empty last row was swallowed on the
way out. That is why `-p -a -q` also differed by exactly one byte.

The fixture's own summary line, three runs at c09ccbee:

    all 78 asserted comparisons identical, 29 recorded not asserted (0 for a sibling lane)

The 29 recorded include this obligation's six clause-1 cases: capture-control,
capture-flags, capture-hyperlinks, capture-line-numbers, capture-pending and
capture-grid, each under capture.rich-transports.
