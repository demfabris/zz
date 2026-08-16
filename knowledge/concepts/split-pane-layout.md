---
type: Concept
title: Binary split-pane layout tree
description: How a window's panes are arranged as a recursive binary LayoutNode tree of ^split nodes with axes and ratios, plus the reconciliation, resize, directional-navigation, and preset logic in model.rs.
resource: crates/zz-mux/src/model.rs
tags: [layout, splits, panes, tree, tmux]
timestamp: 2026-07-20T00:00:00Z
---

# Overview

Every zz window arranges its panes as a **recursive binary tree**, the `LayoutNode` defined in
[`crates/zz-protocol`](/crates/zz-protocol.md) and owned per-window inside
[`MuxState`](/crates/zz-mux.md). A node is either a leaf `Pane(%id)` or a
`Split { id: ^SplitId, axis, ratio, first, second }`. This one structure expresses arbitrarily nested
tmux layouts while keeping surfaces (PTYs / browser sessions) stable: operations rebuild the *tree*,
never the panes. `^split` IDs are monotonically allocated and stable, so a divider drag or nested
resize adjusts exactly the split that was grabbed and a rebuilt layout can never reuse a retired ID.
The tree stores only proportions; concrete geometry is computed on demand and never persisted.

# Schema: `LayoutNode`

| Variant | Fields | Meaning |
| --- | --- | --- |
| `Pane` | `PaneId` | A leaf surface (pending picker, terminal, browser, or Agent). |
| `Split` | `id: SplitId`, `axis: Axis`, `ratio: f32`, `first: Box<LayoutNode>`, `second: Box<LayoutNode>` | An interior divider; `ratio` is `first`'s share. |

`Axis::Horizontal` places `first`/`second` side-by-side (a vertical divider, tmux `-h`);
`Axis::Vertical` (the default) stacks them (a horizontal divider, tmux `-v`). Ratios are clamped to
`[MIN_SPLIT_RATIO, MAX_SPLIT_RATIO]` = `[0.1, 0.9]`. `LayoutNode` provides `contains`, `panes`,
`splits`, and `contains_split` helpers used throughout the model.

# Core operations (model.rs)

| Operation | Helper | Behavior |
| --- | --- | --- |
| Split a pane | `insert_existing_pane` | `split_pane_with` inserts the new leaf beside the target under a `SplitPlacement` (`ratio` = the new pane's share, `before`, `full_size`, `detached`); plain `split_pane` is that with the `0.5`-after-and-focus default. |
| Remove a pane | `remove_leaf` | `kill_pane`/`break_pane`/`join_pane` collapse the split by moving the owned sibling into its parent, preserving descendant allocations. |
| Reparent a pane | `insert_existing_pane` | `join-pane` moves an existing leaf beside a target through the same placement (`-b` before, `-f` full-size, `-p` ratio). |
| Swap two leaves | `swap_layout_panes` | `swap-pane` exchanges leaves, keeping split IDs/ratios. |
| Rotate | `remap_layout_panes` | `rotate-window` walks surfaces through slots via a pane→pane remap. |
| Exact resize | `set_split_ratio` | `resize-split` (divider drag commit) sets one `^split`'s ratio. |
| Directional resize | `resize_boundary` | `resize-pane -L/-R/-U/-D` (`resize_pane`) and `-x`/`-y` (`resize_pane_to`) move **one** boundary: the deepest on-axis ancestor holding the pane in its `first` subtree (the divider touching the pane's right/bottom edge), falling back to the deepest ancestor holding it in `second` when the pane already ends at the window edge. A positive delta always moves that divider right/down, so `-R` grows a pane that has a neighbor to its right and shrinks one pinned to the window edge, which is what `layout_resize_pane` in tmux does with its last-cell/previous-cell rule. The tree stores proportions, so the space that move frees is shared proportionally within each side instead of being taken cell-for-cell from the adjacent pane as in tmux. |

# Layout reconciliation (presets)

`select-layout` and `next/previous-layout` rebuild **only the tree** from the window's `pane_order`,
preserving PTYs, browser sessions, pane IDs, focus history, and pane options. `build_preset_layout`
allocates fresh `^split` IDs per edge and dispatches to:

- `combine_equal_nodes` . a balanced binary split giving every pane equal extent (`even-horizontal`,
  `even-vertical`, and the rows/columns of `tiled`);
- `build_main_layout` . a `0.5` root split with one main pane and a balanced secondary stack
  (`main-horizontal`, `main-vertical`, and their `-mirrored` variants that swap `first`/`second`);
- `build_tiled_layout` . a near-square grid of rows of equal columns.

The prior tree is saved to `previous_layout` so `select-layout -o` (`restore_previous_layout`) can
toggle back, but only if the saved layout still matches the current pane count and has valid ratios;
otherwise it errors without mutating. `select-layout -E` (`spread_layout` /
`spread_first_uneven_ancestor`) resets the nearest uneven ancestor split to `0.5`. Any layout mutation
clears `zoomed_pane`, so lossless zoom (`resize-pane -Z`, which never alters the tree) is dropped when
the geometry changes.

# Target resolution and directional navigation

Directional `select-pane -L/-R/-U/-D` and `last-pane` derive spatial neighbors from this same tree.
`collect_pane_rects` walks the layout into a bounded `LAYOUT_COORDINATE_MAX` (`1_000_000`)-unit
logical rectangle space (via `split_coordinate`), then `directional_candidates` finds panes sharing
the relevant edge (with tmux-style **wrapping** across a window edge) whose orthogonal ranges overlap.
Ties are broken by the window's most-recently-used `last_panes` history. Relative targets `-t:.+` /
`-t:.-` step through `pane_order` instead (`next_pane`/`previous_pane`).

# Invariants

`MuxState::validate()` guarantees the tree stays consistent: the set of layout leaves exactly equals
the window's `panes` map and its `pane_order`; every `^split` ID is globally unique across all
windows; all ratios are finite and within `[0.1, 0.9]`; and the `active_pane`, `zoomed_pane`, and
`last_panes` all reference live panes.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/model.rs` | All layout mutation, preset building, rect collection, directional nav, and `validate`. |
| `crates/zz-protocol/src/snapshot.rs` | `LayoutNode`, `Axis`, and the `contains`/`panes`/`splits` helpers. |

# Related

- Mutated through [commands](/tmux/commands.md) (`split-window`, `select-layout`, `resize-pane`,
  `swap-pane`, `rotate-window`, `join-pane`, `break-pane`); the tree ships in
  [snapshots](/protocol/snapshots.md).
- Owned by [MuxState in crates/zz-mux](/crates/zz-mux.md); `^split` IDs defined in the
  [protocol IDs](/protocol/ids.md). Layout behavior checked against `layout.c`/`layout-set.c` in the
  [tmux upstream reference](/references/tmux-upstream.md).
