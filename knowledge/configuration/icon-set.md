---
type: Configuration
title: zz icon set
description: zz's own icons, drawn from skeletons in the Mac school so their corners follow the theme radius and superellipse smoothing, and how to draw, preview and review a new one.
resource: clients/storybook/icons/sets/mac.json
tags: [ui, icons, zz-gpui-kit, storybook, theme]
timestamp: 2026-10-10T00:00:00Z
---

# Overview

zz has its own icon set: 69 glyphs, every icon zz ships except the four brand marks (`claude`, `openai`,
`brand-chrome`, `zz`), which keep their own artwork. They live as skeletons in
`clients/storybook/icons/sets/mac.json`, not as SVG files, and are drawn at paint time for the current
theme, so radius 0 gives flat icons and higher radii round them the way zz rounds panes.

The app still ships the Tabler SVGs in `crates/zz-gpui-kit/assets/icons`. The storybook shows the new
set everywhere through its Icons knob; wiring it into the app is the step described under
[Shipping](#shipping).

The set went through two rounds on 2026-10-10. The first followed Tabler's skeletons on a 2px stroke and
was dropped as too close to Tabler. The second, the one kept, follows the Mac school. These are our own
drawings: SF Symbols may only be used on Apple platforms, so never trace or copy them.

# How an icon is drawn

Each icon is a list of parts on a 24-unit grid with one stroke width for the whole set (`stroke` in the
JSON, 1.75). Corners are the only thing the theme changes:

- **Corner radius** = theme radius × 0.5 grid units × the vertex's corner scale, capped at 0.45 of the
  shorter adjacent edge. That cap is the same rule `Window::set_adaptive_corner_fraction` applies to
  chrome, so no icon turns into a pill.
- **Corner shape** is a superellipse with the theme's `corner_smoothing` exponent (2 is a circular arc;
  zz uses 4, a squircle), sampled 16 steps per corner.
- **Radius 0 is flat**: sharp corners, square caps, mitered joins. Above 0, caps and joins are round.

Two renderers implement the same math and must stay in step when a part kind changes:
`clients/storybook/src/glyphs.rs` (Rust, the storybook's asset source) and
`clients/storybook/icons/glyph.js` (the review page). The radius and smoothing travel in the asset
path, `icon-drafts/mac/<radius>-<smoothing>/<name>.svg`, because gpui's sprite atlas caches rasters by
path and size; a new radius has to be a new path or the old raster stays on screen.

## Part kinds

Each part is an array whose first item is its kind. Points are `[x, y]` or `[x, y, cornerScale]`;
a missing scale means a sharp vertex.

| Kind | Arguments | Notes |
|---|---|---|
| `line` | `x0, y0, x1, y1` | |
| `path` | points | Open polyline; inner vertices round by their scale |
| `shape` | points | Closed polygon; every vertex rounds by its scale |
| `rect` | `x0, y0, x1, y1[, scale]` | Scale defaults to 1; boxes in the set use 1.3 |
| `circle`, `ellipse` | `cx, cy, r` / `cx, cy, rx, ry` | Never change with the radius |
| `arc` | `cx, cy, r, from, to` | Degrees, clockwise from east, drawn from `from` to `to` |
| `dot` | `cx, cy[, size]` | Filled; square at radius 0, round from radius 2 up |
| `gear` | `teeth, root, tip, tipWidth, rootWidth[, scale]` | Centered on 12, 12 |
| `curve`, `cubic` | quadratic / cubic Bézier points | Fixed curves |
| `solid` | `[part]` | Fills the inner part, no stroke |
| `fade` | `opacity, [part]` | Draws the inner part at that opacity (the loader spokes) |
| `d` | SVG path data | Escape hatch for organic outlines (palette, moon, the undo and redo loops) |

# Mac school rules

- Live area runs about 2 to 22. Objects are wider than tall, like the Mac's: windows and panes span
  2..22 × 4..20.
- Containers show their content: the window has a title bar with traffic lights, the keyboard has key
  caps, the inbox is a tray with sloped sides, the folder is the Finder folder with its front flap.
- Small filled parts carry accents: the page's dog-ear, git commits, the robot's eyes, the CPU die,
  palette wells. Dots are `dot` parts, at least 3.25 units for grip and 3.5 for ellipsis.
- Things that are round stay circles (globe, clock, search lens, user head). Boxes take corner scale 1.3.
- Arrow shafts stop about 1.25 units short of the tip. At radius 0 the shaft's square cap would
  otherwise stick out of the mitered head and the tip reads as a star.
- U-turns (undo, redo) are true half circles drawn with `d`, round at every radius.
- Meanings follow use, not names: `panel-right` and `panel-bottom` are the split right and split down
  actions (two equal panes, no plus); `layout-columns` is the "Window layout" menu (one tall pane and two
  stacked); `layers` is a session (a stack of window cards); `loader` is the Mac activity spinner.
- Each icon may carry a `note` that explains a choice; the review page shows it on the card.

# Adding or changing an icon

1. If zz needs a new `IconName`, add the variant and an SVG file in `crates/zz-gpui-kit/assets/icons`
   as before. The bijection tests in `crates/zz-gpui-kit/src/icon/mod.rs` still require both while the
   app ships files.
2. Add one line to `sets/mac.json` with `name` equal to the file stem, its `variant`, a `group` and its
   `parts`. Draw it with the rules above.
3. Preview it with `just storybook run`: Foundation › Icon drafts shows the set at the current Radius and
   Corner smoothing knobs, a radius ramp, chrome sizes and 48px construction, next to Tabler. The Icons
   knob (zz or Tabler) swaps the icons in every story. Check radius 0, 6 and 25, and 12 to 16px.
   `include_str!` reads the JSON, so touch a file under `clients/storybook/src` if the watcher does not
   rebuild after a JSON edit.
4. `cargo test --manifest-path clients/storybook/Cargo.toml` draws every icon at radius 0, 3 and 12.5.
5. For review, `node clients/storybook/icons/build-sheet.mjs` writes `target/icon-sheet/zz-icons.html`:
   every icon with radius and smoothing sliders, the 24 grid, the Tabler icon it replaces and how many
   Rust files use it. The review artifact from 2026-10-10 is
   `https://claude.ai/artifact/A4dypJQa9Nqs8puPXPiBjS`; republish to that URL so comments stay with it.

# Shipping

The hook is already in the kit: `zz_gpui_kit::IconSource(fn(&str, &App) -> Option<SharedString>)`, a
global that `Icon::render` consults to rewrite an icon's asset path. Without it nothing changes. The
storybook's `assets::icon_source` is the reference: it maps `icons/<name>.svg` to the generated path for
the theme radius and smoothing.

To ship, the app sets the global, the generator and `mac.json` move into the kit, and
`site/src/icons` gets SVGs exported at the default radius.
