# TUI-002 attempt-01

`compat/tui-screen-diff.sh` compares every decoded cell of both screens plus the
cursor, at named settled checkpoints, on the alienware box on 2026-09-09 from
`/home/demfabris/dev/zz-tui-lane` against pinned tmux `d77c9dc6`. Every file
here is a real run of the fixture as it stands on this branch.

## What each file is

| File | What it is |
| --- | --- |
| `environment.txt` | both binary hashes, revision, host, locale, the sizes and checkpoints driven, and the exact comparison channels |
| `screen-diff-run-1.stdout.txt` | the fixture, exit 0, with `ZZ_SCREEN_CAPTURE_DIR` pointed at `captures/` |
| `screen-diff-run-2.stdout.txt` | the same fixture again, exit 0 |
| `captures/` | 195 files: for each of the 65 checkpoints, both sides' full screen with escapes and both cursor tuples |
| `captures-run1-vs-run2.diff.txt` | empty: run 2's captures are byte-identical to run 1's, across all 65 checkpoints |
| `self-check.stdout.txt` | `--self-check`, exit 0: four sabotages each caught in its own channel, three equivalences reported nowhere |
| `timeout-diagnostics/` | a deliberately sabotaged settle, exit 2, and the fifteen files the fixture leaves behind |

## The result

**33 asserted checkpoints identical, 32 recorded, 65 recorded cursor
differences.** Asserted means every one of the outer pane's rows matched byte
for byte with its escapes, and the asserted cursor tuple matched, at 80x24,
100x24 and 80x10, across: fresh attach, status off, status on, split, zoom,
unzoom, an outer resize away and back, two status rows, status at the top, and a
row of wide and combining glyphs. Not one cell differed.

Two results worth naming beyond the count:

- **80x10.** The small height behaves like the wide ones. Nothing about the
  fixture or either binary needs 24 rows.
- **`ok 100x24-from-120x24 resized`.** At 120 columns zz paints a sidebar and
  the two screens are nothing alike; resize that same client down to 100 and the
  screens become identical again, cell for cell. The sidebar is the whole of the
  109-and-above difference, and hiding it leaves nothing behind.

## What is recorded rather than asserted, and why

Recorded means the fixture prints both sides' bytes on every run and keeps
going. Nothing is waived by omission; every recorded checkpoint's captures are
in `captures/`.

**1. The sidebar, at 109 and 120 columns.** The standing `tui.sidebar-auto-hide`
decision, which TUI-004 owns. 24 of the 32 records. The first differing row at
109x24 shows what it is:

```
tmux: $ printf 'MARK-%s\n' fresh
zz:   ^[[38;2;216;222;233m^[[48;2;16;19;24mzz at alienware             │^[[39m^[[49m$ printf 'MARK-%s\n' fresh
```

**2. The cursor's attributes, at every checkpoint, 65 of 65.** This is a NEW
divergence measured by this fixture and it has no registry owner:

```
tmux: shape=default blinking=0 very_visible=0 colour=none
zz:   shape=block   blinking=1 very_visible=0 colour=#e5c07b
```

`crates/zz-tui/src/render.rs:1770-1799` turns the pane cursor's style and blink
into `\x1b[<n> q` and writes an `\x1b]12;#rrggbb\x07` cursor colour on every
cursor placement. Pinned tmux writes neither and leaves the outer terminal's own
cursor alone, so the outer tmux reports `default`/`0`/`none` for the pin and
`block`/`1`/`#e5c07b` for zz at every size. Cursor **position** and
**visibility** are asserted and match everywhere; only shape, blink and colour
are recorded.

**3. Named and indexed colours arrive as RGB.** The `colour-classes` checkpoint
writes the same three cells on both sides, as a named colour, an indexed one and
an RGB one:

```
tmux: ^[[31mNAMED^[[39m ^[[38;5;196mINDEXED^[[39m ^[[38;2;1;2;3mRGB^[[39m
zz:   ^[[38;2;205;0;0mNAMED^[[39m ^[[38;2;255;0;0mINDEXED^[[39m ^[[38;2;1;2;3mRGB^[[39m
```

The pin keeps the class it was given. zz resolves the named cell and the indexed
cell through its palette and hands the outer terminal RGB; the RGB cell passes
through unchanged. This is the colour-class channel the decoded-screen rule says
must not collapse, and in the pane's own content it does not survive zz.
`tui.status-row`'s closed class-1 finding made zz emit the pin's named-colour
bytes in the STATUS ROW; this is the pane body, and it is still promoting.

**4. A status-style with no foreground gets an explicit one.** The `default-fg`
checkpoint sets `status-style bg=colour4` on both sides and leaves the
foreground alone:

```
tmux: ^[[44mL^[[4m0:win*^[[0m^[[44m
zz:   ^[[38;2;216;222;233m^[[44mL^[[4m0:win*^[[0m^[[38;2;216;222;233m^[[44m
```

The pin emits no foreground at all. zz emits an explicit RGB foreground, the
216,222,233 its resolved appearance carries. Under the DEFAULT status-style the
two agree byte for byte, which is why `compat/status-row.sh` never saw this; it
only appears once a style names a background and not a foreground.

Findings 2, 3 and 4 are all new, all measured here, and none of them is fixable
inside this cycle's zones: 2 lives in `crates/zz-tui/src` outside TUI-001's
excursion, and 3 and 4 reach the daemon's frame representation. They need
registry owners.

## The self-check

Four sabotages, each of which must be caught in its own channel, and three
equivalences the pin collapses, which must be reported nowhere. `--self-check`
exits 0 only when all seven behave.

| Case | What is done to one side | Required |
| --- | --- | --- |
| glyph | one character of the OUTPUT differs, from a file each side reads under its own `$HOME`, so the typed command is identical bytes on both sides | rows differ |
| colour | `status-style bg=red` on the zz side only | rows differ |
| cursor | an unterminated line of a different length, again from each side's own file | the CURSOR tuple differs |
| geometry | `split-window -v` on the zz side only | rows differ |
| attribute order | `bg=colour1,fg=colour7,bold` against `bold,fg=colour7,bg=colour1` | nothing differs |
| red vs colour1 | `bg=red,fg=colour7` against `bg=colour1,fg=colour7` | nothing differs |
| escape spelling | `\033[1mBOLD\033[0m` against `\033[01mBOLD\033[m` | nothing differs |

Two of these were rebuilt after the first attempt measured something other than
what it claimed, and both corrections are the interesting part:

- **The red / colour1 equivalence is real, but not for the reason `colour.c`
  suggests.** `colour_fromstring` gives `red` the value 1 and
  `colour1` the value `1|COLOUR_FLAG_256` — two different values. Measured
  end to end, both reach the outer grid as the same cell and `capture-pane -e`
  re-emits both as `\e[41m`, so the pair really does collapse. The fixture's
  header states the measurement, not the header-file reading.
- **The first cursor sabotage proved nothing.** It sent a marker after the
  unterminated line, which ran a whole command and put the cursor back at a
  fresh prompt on both sides: rows differed, the cursor tuple did not. It now
  settles on a substring the two sides' own sabotage output share, so the
  comparison happens where the sabotage left the cursor. Without that fix the
  cursor channel had no demonstration at all.
- A bare `bg=red` and a bare `\e[31m` cannot carry an equivalence at all here,
  because findings 3 and 4 above mean the two sides already differ on a colour
  with no foreground named. An equivalence built on top of a divergence proves nothing, so
  the foreground is named explicitly in the style cases and the escape-spelling
  case uses bold, which carries no colour.

## The settle, and the run that fixed it

A checkpoint is settled when its marker is on the screen AND the screen has not
changed between two polls. The second half is not decoration. With the marker
alone, two runs of the same fixture disagreed at the 80x24 `zoom` checkpoint:

```
run 1  zz row 11: $        cursor 2,10
run 2  zz row 11: <empty>  cursor 0,10
```

The shell writes the marker line and the prompt that follows it as two separate
writes, and a capture can land between them. The stability window closed it:
three consecutive runs after the fix were exit 0, and their captures were
byte-identical to each other across all checkpoints, as were the two runs
retained here. The poll is 50 ms, the same interval every bounded wait in this
harness uses; no wait in the fixture is a sleep.

## The timeout dump

`timeout-diagnostics/` is the fixture run with the checkpoint settle changed to
wait for `NEVER-<name>`, a marker nothing prints. It exited 2 with `fresh
settled on the zz screen did not settle within 10 seconds` and left fifteen
files: which wait ran out, both outer screens with escapes and scrollback, both
cursor tuples, both servers' client and pane lists, the daemon's output and each
client's stderr. The retained screen reads

```
$ printf 'MARK-%s\n' fresh
MARK-fresh
$
```

which is the point of the dump: the runtime was healthy and the assertion was
wrong, and the files say so without a re-run. The sabotage was reverted and the
fixture on this branch is byte-identical to the one that produced the two exit-0
runs.

That dump also earned its place before it was ever sabotaged. The first full run
of this fixture timed out at 109x24, and the retained screen showed why in one
line: zz draws its sidebar to the LEFT of the pane at that width, so the marker
shared its row with sidebar cells and a border, and a whole-line match could
never succeed. That was a fixture fault, fixed by matching the marker as a
substring — which is still safe, because the typed command carries `MARK-%s` and
never `MARK-<name>`.

## Controlled dynamic values

Declared in the fixture header with its rule, and set on both sides before the
first checkpoint: `status-right ''` (the default carries a clock, and the
`%d-%b-%y` in it is the locale divergence TUI-001 recorded), `status-left L`,
`automatic-rename off` plus `rename-window win`, `select-pane -T screentitle`
(the recorded `pane.runtime-facts` decision), and an inner shell of
`ENV= PS1='$ ' exec /bin/sh` handed to both `new-session` and `split-window`, so
no rc file runs and the prompt carries no host, user, path or clock. Global
options are reset to each server's own defaults before every attach, so a case
that sets one cannot leak into the next case or the next size. Nothing else is
masked.

## Coverage against the acceptance, and what remains

Clause 3's list is fully driven: 80, 100, 109 and 120 columns, a small height at
80x10, split, zoom and resize, and status off, on, two rows, top and bottom.

Clause 1 asks for these channels to be COMPARED at named settled checkpoints,
and every one of them is: cells, glyph widths, styles, cursor position, cursor
visibility, cursor shape, geometry and the interaction state a split, a zoom and
a resize leave behind. Cells, glyph widths, styles, cursor position, cursor
visibility and geometry are also GATED — a difference in any of them exits 1.
Cursor shape, blink and colour are compared and printed on every run but not
gated, because zz and the pin genuinely differ there today (finding 2) and a
sabotage of that channel could not be told apart from the standing divergence.
A shape regression would not turn this fixture red, and that is the one thing a
reviewer should weigh: if clause 1 is read as requiring a gate rather than a
comparison, this obligation is short on that point and on nothing else. The
channel starts asserting the moment the divergence closes — the fixture already
prints `recorded cursor attributes identical, the record can close` when the two
sides agree.

What this attempt did not cover, for the lane that takes it further:

- Mouse, focus events and bracketed paste, which TUI-008 owns.
- Copy mode and search overlays, which TUI-005 owns.
- `pane-border-status`, which changes the row budget and belongs with TUI-004's
  canvas clause.
- More than one window, and window switching.
- A second attached client on the same session.
