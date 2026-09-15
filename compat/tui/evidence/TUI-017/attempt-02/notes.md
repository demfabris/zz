# TUI-017 attempt-02, cycle 10 capture lane, 2026-09-15

Clause 1. Attempt-01 belongs to cycle 9's introspection lane, which closed clause 2's residues on
`campaign/tui-introspection`; that branch reached `origin/main` as 627e717a while this lane was
proving clause 1 on 287e3815, and this branch was rebased onto it. Two cases this lane had recorded
against clause 2 then flipped to asserted with no code change of its own: `capture-control-escape`
(`-C` with `-e`) and `capture-line-numbers-trailing` (`-L` with `-N`). Both clauses assert at the
rebased tip.

## Files

- `environment.txt` — the box, the branch, the base, the zz build under test and the pin.
- `01-tui-client-commands-run.txt` — `compat/tui-client-commands.sh` at the landing. Summary line:
  `all 109 asserted comparisons identical, 28 recorded not asserted (0 for a sibling lane)` at the
  rebased tip. The capture family is lines 57 to 167.
- `02-tui-client-commands-self-check.txt` — `--self-check`, including the numbered-capture sabotage
  this attempt added.
- `03-rich-transports-probe.txt` — what the pin emits for each of the six flags on a 40x8 pane, what
  zz emits at this landing, and the two measurements behind the refusals.
- `03-rich-transports-probe.sh` — the script that produced it.
- `04-corpus-capture-pane.txt` — `compat/run.sh --strict-geometry capture-pane`:
  `34 step(s), 0 TOPO divergence(s), 0 GEO divergence(s), 0 FMT divergence(s), 0 OUT divergence(s),
  0 WARN divergence(s)`, with the eight steps this attempt added, including a soft-wrapped row.
- `05-delta-corpus.txt` — every scenario the delta selected for the touched commands, one line each.

## What landed: -C and -L

Both are transforms of the text the terminal worker already produces, so neither needs a grid zz
does not have, and both were read out of the pin rather than out of its manual page.

`-C` sets `GRID_STRING_ESCAPE_SEQUENCES`, and in tmux's history path that flag does exactly two
things (grid.c): a cell whose single byte is `\` is written as `\\`, and, when `-e` is also given,
`grid_string_cells_code` and `grid_string_cells_add_hyperlink` write the literal five bytes `\033`
where they would otherwise write ESC. The three-digit octal escaping the manual page describes lives
in `cmd_capture_pane_pending`, so it belongs to `-P` and not to this flag. zz doubles the backslash
and then writes `\033` for each ESC, in that order, so the backslash of `\033` is not doubled —
which is what the pin's two writers produce between them.

`-L` numbers each line with `i - hsize`, signed, so history rows number negative, and the number is
emitted per grid line inside the loop. With `-J` the newline is suppressed for a wrapped row but the
number is not, so the pin puts the number of every joined row inside the joined line:
`0 $ printf 'AAAA1 AAAA2 BBB\n'`. zz reproduces that by numbering against a second unjoined pass and
walking the two together, rather than by asking the engine a wrap question it does not answer.

Asserted against the pin: `-C`, `-C -e`, `-L`, `-C -L`, `-L` over a history start where the numbers
go negative, `-L -J` over a soft-wrapped row, `-L -N`, `-L` with reversed bounds, `-L` on a missing
pane, and `-C` into a named buffer read back with `show-buffer`. Twelve new assertions in the fixture and
eight corpus steps.

`-C -e` and `-L -N` assert too, on the rebased base: both ride on clause 2's residues, which cycle
9 closed. The only recorded capture cases left are the four refusals below.

## What stays refused, and why

The clause allows a refusal that names the workload it would serve. Each of these four now carries
its measurement in the fixture's own reason string.

- `-F` prints six `grid_line` flags. `D` is a dead-pane line, `X` an extended-cell line and `H` a
  hyperlink line — all tmux grid bookkeeping. `O` and `P` are the OSC 133 marks libghostty records
  on cells but does not publish per line. `W` is the wrap the formatter consumes and never reports.
  The workload: a script reading which rows are output, prompt or continuation. That wants a
  line-fact channel out of the terminal worker, not a sixth text transform.
- `-H` prints each line's OSC 8 URIs. Measured in `03`: on the row the pin marks `HX`, a zz `-e`
  capture emits `LINK` with no OSC 8 at all, while the pin emits
  `^[]8;id=zzid;https://example.com/a^[\LINK^[]8;;^[\`. There is no hyperlink in the retained
  snapshot to print. The workload: harvesting the URLs on a screen. That wants hyperlinks retained
  and published first.
- `-P` prints `input_pending(wp->ictx)`, the bytes the pin's parser has read and not completed.
  libghostty-vt publishes no parser-pending buffer. An empty answer would be a fake channel that
  matched only because that buffer is almost always empty. The workload: debugging a half-written
  escape sequence.
- `-R` is the one that was weighed. It dumps the pin's internal grid: a header
  `G <sx>x<sy> (<hsize>/<hlimit>)`, then per line `L <yy> (<n>) flags=<string>[<hex>]
  <cellused>/<cellsize>`, then one `C` line per column carrying that cell's colour, attribute and
  link ids. Measured at 40x8 that is 329 lines for eight rows. zz has no hsize/hlimit pair, no
  per-line cellused and cellsize, and no grid flag word; building them inside zz would be inventing
  tmux internals so that bytes match, which is the opposite of what this project is. The workload it
  would serve is a tmux regression test reading another tmux's grid. Decided 2026-09-15 by the
  orchestrator under fabrico's TUI parity contract of 2026-09-09; reversible.

## A second measurement for clause 2

Clause 2 names one trailing cell for `-e`. On a coloured row there is more: the pin writes
`^[[31mRED^[[39m` and zz writes `^[[0m^[[38;5;1mRED^[[0m`, so the colour spelling and the reset
differ as well. The fixture's own scene has no colour, which is why the roster only ever saw the
trailing cell. Whoever closes clause 2 should know the `-e` residue is wider than one cell. It no longer holds `-C -e` open — that case
asserts on the rebased base — but a scene with colour in it would still diverge, and no case covers
one. It is a formatter question rather than a transport one and wants its own owner.

## The sabotage

`--self-check` plants three glyphs at the zz client's prompt and takes a `-L` capture: a numbered
capture of a capture that already matched has to move with it, caught in stdout alone, so the `-L`
bytes are compared and not merely produced.


## Fix pass after the rejected review

The review rejected the earlier claim that both clauses assert. The historical narrative above
is superseded by this fix pass. The old history and wrap cases did not exercise negative history
or the coloured wrap that lost NEXT. The old assertion count was twelve new capture assertions,
not thirteen.

`compat/tui-client-commands.sh` now opens all four refusal reasons with DECIDED and includes the
required decision sentence verbatim at runtime. The flag reason now acknowledges the exposed wrap
bit. The explanatory shell comments moved here: a numbered capture must detect a one-sided glyph
change, and a real multi-row scene must keep the following row. ANSI byte lengths cannot identify
physical rows.

This pass also removes the added explanatory comments from `compat/scenarios/capture-pane.txt`.
The source fix in `crates/zz-terminal/src/session.rs` uses the physical row's wrap flag and keeps
its line number regardless of whether the newline is joined.
