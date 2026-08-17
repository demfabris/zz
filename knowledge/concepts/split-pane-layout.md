---
type: Concept
title: Cell-authoritative split-pane layout
description: How a window's panes are arranged as an n-ary cell tree ported from tmux's layout.c — split/resize/preset algorithms in layout.rs, the derived binary wire projection with stable ^split ids, measurement-driven window extent, and the invariants model.rs enforces.
resource: crates/zz-mux/src/layout.rs
tags: [layout, splits, panes, cells, tree, tmux]
timestamp: 2026-08-16T00:00:00-03:00
---

# Overview

Every zz window arranges its panes as an **n-ary tree measured in terminal cells** — a faithful
Rust port of the pinned tmux's `layout.c` engine, living in `crates/zz-mux/src/layout.rs` and
owned per-window inside [`MuxState`](/crates/zz-mux.md). Cells are the truth: a node carries
`sx/sy/xoff/yoff` in cells, the root's extent IS the window size, and every split, resize,
preset, and kill runs tmux's exact integer arithmetic — validated against 46 golden fixtures
captured from the pinned tmux binary (`crates/zz-mux/tests/fixtures/layout-pin.txt`, regenerated
by `compat/gen-layout-fixtures.sh`) and by the differential harness running `--strict-geometry`
clean. Ratios exist only as a derived projection for clients.

# The kernel: `CellLayout`

`CellNode` is a `Leaf { pane, geometry }` or a `Node { axis, geometry, children }`; each child
except the first carries the `SplitId` of the border on its leading edge. Invariants (tmux
`layout_check`): on a node's axis, child extents plus one border cell between neighbors sum to
the node's extent; children match the node on the other axis; every node has ≥ 2 children; every
leaf is at least `PANE_MINIMUM` (1 cell); same-axis nesting is legal (kills produce it, exactly
like tmux). `PANE_MAXIMUM` is 10,000.

The tmux algorithms, with their load-bearing quirks:

- **split** (`layout_split_pane`): default gives the new pane `((extent+1)/2)−1`, the border
  eats a cell, `-l`/`-p` resolve against the target pane (the window under `-f`), sizes clamp to
  `[PANE_MINIMUM, extent−2]`, and a too-small box refuses with tmux's exact
  "no space for a new pane". Full-size splits insert at the head/tail of the root and of the
  pane order.
- **remove**: the space is gifted to the following sibling (else the preceding one); a parent
  left with one child is spliced away with a bare replace — never merged, so nested same-axis
  nodes survive exactly as in the pin.
- **resize** (window): round-robin integer spread, earliest children absorb remainders; the
  layout never shrinks below its minimum, so it can exceed the requested extent.
- **resize-pane**: tmux's victim walks — grow takes from the first sibling after (then before)
  with headroom, shrink gives to the immediate next sibling; the last child inverts; absolute
  targets convert to deltas.
- **presets**: all seven (`even-*`, `main-*` ± mirrored, `tiled`) with the pin's defaults
  (main-pane-height 24, main-pane-width 80). Spread biases remainders to the first children;
  tiled biases them to the last column/row. One deliberate divergence: with exactly two panes
  the pin never sizes the lone "other" pane (an upstream bug that violates tmux's own
  invariant) — zz sizes it. See [the divergence matrix](/tmux/divergences.md).
- **layout strings**: `dump()` emits tmux's checksummed `layout-custom.c` format; parsing is
  future work.

# Wire projection and divider identity

Clients still receive the binary ratio `LayoutNode` ([snapshots](/protocol/snapshots.md)):
`project()` folds each child list right-associatively, computing `ratio = first / (first + rest
+ inner borders)`, and stamps each fold with the divider's stable `SplitId` — the GUI's drag
handles survive unrelated mutations. A divider drag arrives as `ResizeSplit` basis points and
maps onto `set_divider_ratio`, which converts the ratio back to cells over the same denominator
and runs the sibling resize walk, so every committed drag lands on whole cells.
`swapped_layout`/`joined_layout` remain pure client-side predictors on the wire tree.

# Window extent

Windows are born at tmux's `default-size` (80x24) or inherit their session's active-window
extent; `new-session -x/-y` is honored. A drawing client's per-pane measurements update the
extent through a guarded reconstruction (`MuxEngine::set_pane_geometry`): only the zoomed pane
counts while zoomed (applied directly), only the active pane otherwise, a one-cell dead-band and
a repeat memo guarantee a fixed point (the naive per-pane write-back oscillated). Detached
windows keep their last extent, like tmux. A zoomed pane reports the full window extent to
formats; the tree underneath is untouched.

# Invariants (model.rs)

`MuxState::validate()` asserts the kernel invariant per window plus: layout leaves equal the
pane map and `pane_order`, split ids globally unique, `zoomed_pane` is the active pane, and the
chaos suite (`tests/chaos.rs`) holds it under interleaved mutation. Pane order is maintained
tmux-style: splits insert after the target (before with `-b`, head/tail with `-f`), `join-pane`
always inserts after (reproducing the pin's pass-by-value `-b` quirk), and a single-pane
`break-pane` moves the window itself, preserving its id.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/layout.rs` | The cell kernel: algorithms, projection, dump, validation |
| `crates/zz-mux/src/layout_pin_tests.rs` | Replays the 46 pin-captured fixtures through the kernel |
| `crates/zz-mux/src/model.rs` | `MuxState` write paths wrapping the kernel; invariants |
| `crates/zz-mux/src/command.rs` | Command surface: sizes, percentages, measurement feed |
| `compat/gen-layout-fixtures.sh` | Regenerates the golden fixtures from the pinned binary |

# Related

- [tmux divergence matrix](/tmux/divergences.md) . the deliberate two-pane preset divergence
- [compat harness playbook](/playbooks/compat-harness.md) . the strict-geometry differential gate
- [snapshots](/protocol/snapshots.md) . the derived wire projection
- [tmux drop-in plan](/designs/tmux-drop-in.md) . phase 3, which this page documents
