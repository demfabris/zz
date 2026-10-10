---
type: Configuration
title: zz icon set
description: zz's own icons, drawn from skeletons in the Mac school so their corners follow the theme radius and superellipse smoothing, how the app and the site get them, and how to draw, preview and review a new one.
resource: crates/zz-gpui-kit/src/icon/mac.json
tags: [ui, icons, zz-gpui-kit, storybook, site, theme]
timestamp: 2026-10-10T12:00:00Z
---

# Overview

zz has its own icon set: 69 glyphs, every icon zz ships except the four brand marks (`claude`, `openai`,
`brand-chrome`, `zz`), which keep their own artwork as files in `crates/zz-gpui-kit/assets/icons`. The
set lives as skeletons in `crates/zz-gpui-kit/src/icon/mac.json`, not as SVG files, and is drawn at
paint time for the current theme, so radius 0 gives flat icons and higher radii round them the way zz
rounds panes. The desktop app, the web and iOS clients and the storybook all draw it; Tabler, which zz
shipped before, is gone except for the `brand-chrome` mark.

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
`crates/zz-gpui-kit/src/icon/glyphs.rs` (Rust, what the app draws) and
`clients/storybook/icons/glyph.js` (the review page).

# How it ships

`IconName::path` stays `icons/<name>.svg`. When it renders, `Icon` rewrites a path the set draws to
`icons/<theme radius>-<corner smoothing>/<name>.svg` from `cx.theme()`, and the kit's `Assets` draws
that path on load. The radius and smoothing travel in the path because gpui's sprite atlas caches
rasters by path and size; a new radius has to be a new path or the old raster stays on screen. Changing
the radius or the interface style refreshes the windows, so every icon redraws. A plain
`icons/<name>.svg` load, outside any theme, draws at radius 6 and smoothing 4. Nothing has to be wired
per client: every app that uses the kit's `Assets` and `Icon` gets the set.

The bijection tests in `crates/zz-gpui-kit/src/icon/mod.rs` hold `IconName` to the set plus the brand
files: every variant loads, every file in `assets/icons` has a variant, and every icon in `mac.json`
names its variant and has no file shadowing it.

The site inlines static copies from `site/src/icons`, exported at the app's default look (Modern,
radius 24, smoothing 4) next to the brand marks. Re-export after changing the set:

```sh
cargo run -p zz-gpui-kit --example export_icons -- site/src/icons 24 4
```

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

1. If zz needs a new `IconName`, add the variant and its `icons/<name>.svg` path in
   `crates/zz-gpui-kit/src/icon/mod.rs`. Only brand marks get a file in `assets/icons`.
2. Add one line to `mac.json` with `name` equal to the path's stem, its `variant`, a `group` and its
   `parts`. Draw it with the rules above.
3. Preview it with `just storybook run`: Foundation › Icon set shows the set at the current Radius and
   Corner smoothing knobs, a radius ramp, chrome sizes and 48px construction. Check radius 0, 6 and 25,
   and 12 to 16px. `include_str!` reads the JSON, so touch a file under `crates/zz-gpui-kit/src` if the
   watcher does not rebuild after a JSON edit.
4. `cargo test -p zz-gpui-kit --lib icon` draws every icon at radius 0, 6 and 25 and checks the
   bijection.
5. Re-export the site copies (see [How it ships](#how-it-ships)).
6. For review, `node clients/storybook/icons/build-sheet.mjs` writes `target/icon-sheet/zz-icons.html`:
   every icon with radius and smoothing sliders, the 24 grid and how many Rust files use it. The review
   artifact from 2026-10-10 is `https://claude.ai/artifact/A4dypJQa9Nqs8puPXPiBjS`; republish to that
   URL so comments stay with it.
