---
type: Research
title: GPUI fork lab, macOS handoff
description: What the macOS session measured after the Linux GPUI lab, which fork changes it took or rejected, and what is still open, with branches and proof rules.
resource: Cargo.toml
tags:
- performance
- gpui
- fork
- macos
- handoff
timestamp: 2026-10-02T06:00:00Z
status: open
---

# Read first

1. [GPUI fork lab](/research/2026-09-30-gpui-fork-lab.md): what was taken, rejected and measured on Linux.
2. [GPUI revision pin](/references/gpui-revision.md): where the pin lives and how to move it.
3. [macOS CPU, GPU, and memory investigation](/research/2026-09-23-macos-performance.md): the existing Mac
   profiling method (`just profile-cpu mac`, `profile-metal`, `profile-terminal-diagnostics`).

# Done on the MacBook (2026-10-01 and 02)

Results and method are in [macOS (Metal)](/research/2026-09-30-gpui-fork-lab.md#macos-metal).

1. `gpui-lab`, rebased onto main `4e7c62acb`, passed clippy and every workspace test on macOS
   except `zz-daemon` load flakes, which pass alone (its daemon code equals main's). It was not
   checked by eye on screen; see Still open.
2. Metal yardstick: `bench/gpui/frames.py`. The lab pin `cc9e4d1804` against `e01edb6b1a`:
   main-thread CPU -5.5% stream, -8.5% grid, -4.9% scroll, -12.6% chrome, no repeat overlapping.
   The Linux stream and scroll regression does not appear on Metal.
3. Retained mode: accessibility is inactive on macOS and views are reused, yet the rebased port
   (local `lab/retained-mac`) costs +24% in stream, scroll and chrome and saves 1% in grid.
   Rejected again.
4. gpui-fast `9b2f43d` ported as local `lab/coretext` (fonts per size, run rendering, plus an
   `accessibility_active` field in the recorder): -10.8% stream, -3.1% grid, -5.1% scroll, -7.7%
   chrome against `cc9e4d1804`, no repeat overlapping.
5. Compact scene records, the two Metal-only commits and hot reload: see the lab document.

# Still open

1. **Push `lab/coretext` to `zz-patches`.** Its three commits sit on top of `cc9e4d1804` in the
   local fork checkout at `~/.cache/zz-forks/zed-coretext`. zz built against them (bundle C). Push
   with a dated backup of `cc9e4d1804`, then move the pin in the three manifests and both
   lockfiles. Pushing to the fork goes over ssh with the hardware key, so it needs a person.
2. **Look at the app.** Nobody has checked rendering on screen on macOS since `cc9e4d1804` or with
   `lab/coretext`: terminal backgrounds, bold and italic fallback fonts, emoji, wrapped text in
   the agent pane, sidebar, palette and settings.
3. **Hot reload on macOS.** Install dioxus-cli 0.7.10 and sort out the bundle, CEF framework
   layout and codesigning of patch libraries.
4. **Damage tracking** (zed PR #62455, open). Its present-skip part works on every platform and
   would cut GPU work for frames whose scene did not change, which zz produces when chrome
   notifies without visual change. Unmeasured.

# Proof rules

A change counts when it has an A/B on the Mac against the same base with only that change toggled
(method, command, repeats, spread), builds with zz, and has a written risk list. A fix needs the
failure reproduced first. Numbers from another project's README are unverified. Put each fork change
in its own `gpui: <imperative>` commit with its source repo and hash in the body, push `zz-patches`
only after zz builds against it, and keep a dated backup branch of the previous tip. Record results
in the lab document above and close this handoff (`status: closed`) when the list is done.
