---
type: Research
title: GPUI fork lab, macOS handoff
description: Where the Linux GPUI lab left off and what the macOS session should measure next, in priority order, with branches, proof rules and the open questions only a Mac can answer.
resource: Cargo.toml
tags:
- performance
- gpui
- fork
- macos
- handoff
timestamp: 2026-10-01T12:00:00Z
status: open
---

# Read first

1. [GPUI fork lab](/research/2026-09-30-gpui-fork-lab.md): what was taken, rejected and measured on Linux.
2. [GPUI revision pin](/references/gpui-revision.md): where the pin lives and how to move it.
3. [macOS CPU, GPU, and memory investigation](/research/2026-09-23-macos-performance.md): the existing Mac
   profiling method (`just profile-cpu mac`, `profile-metal`, `profile-terminal-diagnostics`).

Everything in the lab ran on the Ubuntu box. Nothing in this document has a macOS number yet.

# State you start from

| Repo | Ref | What it is |
| --- | --- | --- |
| `demfabris/zed` | `zz-patches` at `cc9e4d1804` | previous tip `e01edb6b1a` plus the 27 lab commits |
| `demfabris/zed` | `zz-patches-2026-10-01` | backup of the previous tip `e01edb6b1a` |
| `demfabris/zed` | `lab/retained` | gpui-fast retained mode ported onto `beca360ad3` (not carried) |
| `demfabris/zed` | `lab/micro` | all 18 micro ports on `beca360ad3`, including compact scene records `c7b073228d` (dropped) |
| `demfabris/zed` | `lab/upstream-rebase` | trial rebase of the carried patches onto upstream `decbf641b1` (before the 27) |
| zz | `gpui-lab` | zz main plus: pin bump to `cc9e4d1804`, zz-ui accessibility commits, `just hot linux`, these two documents |
| zz | `lab/gpui-retained` | zz-side fixes retained mode needs (Markdown paint scope, terminal no-op guards), on an older main |

zz `main` still pins `e01edb6b1a`. The `gpui-lab` branch has not been built or run on macOS.

# Tasks, in order

## 1. Qualify `gpui-lab` on macOS and merge it

Build and run it on the Mac (`just run mac`), then the CI gates (`cargo test --workspace
--all-features`, clippy). Things only a Mac exercises: the native Metal renderer with the scene
bounds grid and glyph tile cache, CoreText with the fallback attribute fix and the recent shape
cache, the macOS display link pause, and the frame instrument's thread clock. Look at terminal
backgrounds, bold and italic fallback fonts, wrapped text in the agent pane, sidebar, palette and
settings. If it holds, merge `gpui-lab` into main (check first that the daemon perf campaign is not
mid-merge on the same files) and update `gpui-revision.md`.

## 2. Measure on Metal

Port the yardstick: build with the frame instrument and run zz Dev with `GPUI_FRAME_STATS=<file>`,
drive the same five scenarios through the zz CLI (idle, 240 rows/s stream, nine-pane 87x30 Manual
grid with one pane streaming, scroll at 20 commands/s, 25-session sidebar plus
`command-prompt -b -C` palette while streaming), 3 x 20 s after 5 s warmup, and pair it with
`just profile-cpu mac` for attribution. Record main-thread CPU, per-phase medians, frames and
footprint. Compare `e01edb6b1a` against `cc9e4d1804`. Answer: do the Linux gains (grid -14.6%,
chrome -6.2%) hold on Metal, and is the small stream/scroll regression (+0.2 to 0.35 points with
the recorder off) there too? If it is, bisect it; the scene bounds grid and the recent shape cache
are the first suspects.

## 3. Retained mode, again, on macOS

The Linux rejection hinged on accessibility: GNOME keeps AT-SPI enabled, AccessKit activates on
that alone, and gpui-fast disables view reuse while accessibility is active, so zz reused zero
views. On macOS AccessKit activates only when an assistive client queries the window. First
confirm that during a normal run (no VoiceOver) GPUI reports accessibility inactive. If so, rebase
`lab/retained` onto `zz-patches` (expect real conflicts: 33 carried commits share its hook files,
and the 27 new ones touch `window.rs`, `div.rs`, `text_system`), bring the zz fixes from
`lab/gpui-retained`, and measure all five scenarios with retention on and off
(`GPUI_VIEW_RETENTION=0`). It costs about +12.5k lines to carry, so it needs a large, repeatable
win in grid and chrome, and no stale rendering in terminal, browser, agent and Markdown panes.

## 4. gpui-fast's native CoreText path

longbridge/gpui-fast `9b2f43d` ("bring back the macOS native font per size and glyph-run
painting") was never evaluated because it is macOS-only. Text shaping and glyph painting are zz's
main terminal costs. Port it onto `zz-patches` alone and A/B it with the yardstick, watching our
carried glyph raster cache and the new per-frame glyph tile cache for overlap.

## 5. Smaller Mac questions

- Compact scene records (`c7b073228d` on `lab/micro`): sorting regressed on Linux; Metal's scene
  finish may differ. One A/B decides it.
- Hot reload on macOS: `scripts/run-hot.sh` refuses non-Linux today. Subsecond supports macOS; the
  open parts are the app bundle, the CEF framework layout and codesigning of patch libraries.
- The two commits zz main pinned without lab coverage: headless Metal renderer (`88d396491d`) and
  WGSL shader layers (`e01edb6b1a`). Nobody measured them.
- Damage tracking (zed PR #62455) and separate image atlas pages (zed PR #60791): research only so
  far, both apply to every platform.

# Proof rules

A change counts when it has an A/B on the Mac against the same base with only that change toggled
(method, command, repeats, spread), builds with zz, and has a written risk list. A fix needs the
failure reproduced first. Numbers from another project's README are unverified. Put each fork change
in its own `gpui: <imperative>` commit with its source repo and hash in the body, push `zz-patches`
only after zz builds against it, and keep a dated backup branch of the previous tip. Record results
in the lab document above and close this handoff (`status: closed`) when the list is done.
