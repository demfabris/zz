---
type: Concept
title: Zed GPUI terminal rendering parity
description: The effort to bring zz's terminal painting up to Zed's GPUI standard by mapping immutable renderer-neutral frames and dirty-row patches onto GPUI text, cursor, and overlay painting.
resource: crates/zz/src/terminal/view.rs
tags: [rendering, gpui, zed, parity, cursor, ime, contrast, box-drawing, block-elements, local-scroll]
timestamp: 2026-08-01T00:00:00Z
---

# Overview

Rendering parity is the goal of matching Zed's current terminal rendering fidelity while keeping zz's
existing incremental frame pipeline. Both projects share the same broad structure: a custom GPUI element
paints terminal backgrounds, shaped text, overlays, and a cursor over a fixed cell grid;
[`libghostty-vt`](/terminal/libghostty-vt.md) owns VT parsing and state; GPUI owns font selection,
shaping, rasterization, layout, and painting. zz keeps an incremental path Zed's terminal lacks: Ghostty
dirty rows become compact [viewport patches](/concepts/terminal-frame.md), the client retains row
revisions, and GPUI caches shaped rows by revision. The work raises typography, color, cursor, IME,
selection/links, and fractional-scale geometry to Zed's standard without discarding that path. One
client-only layer paints on top of it so a remote pane feels local: local scroll sources rows from the
pane's history ring. It patches
`gpui` + `gpui_platform` through the `demfabris/zed` `zz-patches` branch; the exact revision
and the list of carried patches live in [gpui-revision](/references/gpui-revision.md), which tracks
`Cargo.lock`.

# How frames map to GPUI painting

The [terminal frame](/concepts/terminal-frame.md) is renderer-neutral; the app translates it:

| Frame data | GPUI rendering |
| --- | --- |
| Dirty rows → `TerminalViewportPatch` | Applied in place to one retained mutable grid; each changed row bumps a row revision that keys the shaped-row cache. |
| `PackedCell` glyph + `PackedStyle` | Resolved to a GPUI text run; multi-codepoint graphemes and wide glyphs keep their grid reservation. |
| Connected light box / solid block glyph | Light segments, junctions, rounded corners, and diagonals become device-pixel-snapped geometry; solid block elements fill exact snapped cell fractions. |
| `foreground`/`background` + palette | Default and palette colors corrected against the resolved cell background when below `minimum-contrast`. |
| `ATTR_EXPLICIT_RGB` bit | Skips contrast correction for application-chosen truecolor; box/block/Powerline glyphs also skip it so adjacent graphical cells keep identical colors. |
| `OverlaySpan` plane | Selection, search match/current, link hover, and copy cursor painted from [appearance](/terminal/appearance.md) colors on a separate plane. |
| `Cursor` record | Derived cursor paint record (see below). |

`PackedStyle` stays 16 bytes; two previously unused attribute bits now record explicit-RGB provenance and
OSC 8 hyperlink presence. Appearance and contrast settings participate in the row-cache signature, so a
config change cannot reuse stale shaped colors. The app sends a resize only when rows, columns, physical
cell width, or physical cell height change; the daemon coalesces resizes and updates libghostty + the PTY
as one operation.

# Row sources under local scroll

A live pane paints every row from the server frame. While [local scroll](/terminal/interaction.md) is
active, `TerminalView::local_scroll_target` names an absolute offset the server has not reached yet,
and `local_row_source` resolves each grid row to one of three sources:

| `LocalRowSource` | Painted from |
| --- | --- |
| `History(index)` | A row of the pane's client-side `HistoryRing`, shaped from its own interned dictionary and cached under its own row revision. |
| `Live(row)` | The slice of the server frame the target offset still overlaps, placed by `local_live_projection` on the display rows that intersection covers. |
| `Shimmer` | A full-width fill of the foreground color at `0.04` alpha, standing in for rows the ring has not backfilled. |

Ring rows enter `RowRenderCache` only while the overlay renders them, so the ordinary live path keeps
its pre-overlay shaping cost and cache footprint. Cursor, selection, search matches, and link hover
all project through the same `local_live_projection`, which means they ride the visible live slice and
vanish once the target scrolls clear of it. The scrollbar thumb is drawn from the local target rather
than the frame's own offset. Rows the ring cannot cover paint the shimmer fill and nothing else, so a
scroll ahead of the backfill reads as empty rather than as stale content.

# Typography and grid geometry

- Each regular, bold, italic, and bold-italic Ghostty family stack maps to one GPUI font stack; an
  empty style-specific stack inherits the regular stack. OpenType tags become `FontFeatures`;
  Ghostty's explicit default `liga=1` entry is prepended before configured entries, so a later
  `-liga` override still wins. Bold/italic runs retain the configured features and fallbacks.
- On macOS, zz compares the requested style's resolved CoreText face with the corresponding
  unstyled face. If the family has no real bold or italic face and `font-synthetic-style` permits it,
  GPUI paints a Ghostty-compatible fill-plus-stroke bold and 15-degree italic shear. The decision is
  cached with each shaped row. Synthetic-bold line width is converted back to user space before the
  Retina context transform so the stroke remains device-pixel sized. `font-thicken` and its 0–255
  strength map to explicit CoreGraphics smoothing, independently of synthetic bold. Both keys are
  macOS-only: GPUI's swash rasterizer on Linux never reads the smoothing parameters, as in Ghostty.
- Font size remains in Ghostty points through the renderer-neutral model. On macOS those points map
  one-to-one to GPUI/CoreText units and display scaling happens later; other backends convert points to
  GPUI logical pixels at 96 logical DPI. The resolved `m` advance, snapped to whole device pixels like the line height so every
  column origin lands on a device pixel, defines cell width; ascent +
  descent + line gap defines the natural cell height. The optional configured adjustment modifies
  that natural value, which rounds to a whole device pixel and converts back to logical pixels.
- Padding is independent per edge and excluded before grid dimensions are computed; a float tolerance is
  applied before flooring row/column counts.
- Standard light box glyphs (`─│┌┐└┘├┤┬┴┼╭╮╯╰` and light half-lines) do not rely on the
  font em box reaching the adjusted cell edges. Straight runs and junctions are synthesized across the
  whole cell; rounded corners use a single stroked cubic path with the same width and centerlines as their
  neighboring runs. Solid block elements (`▀▁▂▃▄▅▆▇█▉▊▋▌▍▎▏▐▔▕` and quadrant forms) are filled to exact
  snapped cell fractions so pixel-art cells meet without gaps. The geometry keeps faint/selection colors
  and prevents row or column seams when configured line height exceeds the font glyph bounds. Light
  diagonals (`╱╲╳`) are stroked corner to corner across the snapped cell as one path, overshooting each
  corner by half a step of the cell's own slope so neighbors overlap instead of leaving a notch; a font's
  diagonal is sloped for its em box rather than for the configured cell, so consecutive rows disagree
  about where the line sits and a run of them visibly steps. Heavy, double, dashed, and shaded forms
  remain font-shaped so their distinct stroke or texture semantics are preserved.
- Box stroke width follows Ghostty's `box_thickness = max(1, ceil(underline_thickness))` in device
  pixels, resolved from the face rather than from the cell advance so a synthesized stroke carries the
  weight of the text beside it. It is computed once per `CellMetricsSignature` alongside cell width and
  line height, and reaches the painters as an explicit argument. The earlier `cell_width * 0.125` rule
  disagreed with Ghostty for most faces (at BerkeleyMono 13pt @2x it strokes 2 device pixels where
  Ghostty strokes 3), which made box-drawn art read lighter. Reading the metric needed a
  `TextSystem::underline_thickness` accessor, carried patch 13 in
  [gpui-revision](/references/gpui-revision.md). Underline, strikethrough, and overline still paint one
  device pixel rather than this thickness.
- When the viewport follows live output, spare vertical pixels sit above the grid so the bottom row stays
  anchored; scrollback, copy mode, search, command-output views, and an active local scroll stay
  top-anchored. During a live resize, retained rows are projected from the bottom so the prompt does not
  flash stale content.

# Cursor, IME, and paint order

Cursor: derived from the visible cell under the cursor (a wide-tail marker resolves to the wide glyph's
leading cell). Focused: block paints a filled rectangle then repaints the glyph in the background color;
bar is a 1px vertical stroke; underline a 2px bottom stroke; hollow a 1px outline. Unfocused block/bar/
underline become hollow. Blinking is GPUI-local (off / on / terminal-controlled) and sends no protocol
traffic; input activity resets it to visible.

There is no prediction layer any more. The provisional predicted-cell pass (dimmed glyph, underline,
cursor pulled one cell past the newest prediction) and its `prediction_overlay` went with
`terminal/predict.rs` on 2026-08-01; the grid only ever paints cells the daemon confirmed.

IME: while marked text is non-empty the normal cursor does not paint, marked text is shaped with the
active terminal font + fallbacks under the base/selection glyphs (backgrounds preserved), and painted with
a 1px underline; the candidate window follows the composition bounds.

Paint order: (1) terminal surface, (2) non-default cell backgrounds, (3) selection / search / link-hover /
copy-mode highlights, (4) box/block cell geometry, (5) cached text runs + decorations, (6) normal cursor,
(7) IME-masked base/selection glyphs then marked text, (8) hovered-link and terminal-mode presentation.

# Non-goals

No Ghostty custom shaders or GPU renderer; no Kitty graphics / Sixel / iTerm images; no settings UI; text
blink is preserved in the model but not painted this milestone.

# Related

- Consumes the immutable [terminal frame](/concepts/terminal-frame.md) and its retained-grid patches.
- Colors, fonts, padding, and contrast come from the [appearance](/terminal/appearance.md) subsystem.
- Overlays and pointer feedback are produced by the [interaction](/terminal/interaction.md) subsystem,
  which also drives local scroll; its client state (`HistoryRing`, `LocalScroll`) lives in the
  [zz app](/crates/zz.md).
- Frames arrive over the [terminal lanes](/protocol/terminal-lanes.md); painting lives in [`/crates/zz.md`](/crates/zz.md).
- GPUI pin and carried patches: [gpui-revision](/references/gpui-revision.md).
