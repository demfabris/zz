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
timestamp: 2026-10-02T12:00:00Z
status: open
---

# Read first

1. [GPUI fork lab](/research/2026-09-30-gpui-fork-lab.md): what was taken, rejected and measured on Linux.
2. [GPUI revision pin](/references/zz-gpui.md): where the pin lives and how to move it.
3. [macOS CPU, GPU, and memory investigation](/research/2026-09-23-macos-performance.md): the existing Mac
   profiling method (`just profile-cpu mac`, `profile-metal`, `profile-terminal-diagnostics`).

# Done on the MacBook (2026-10-01 and 02)

Results and method are in [macOS (Metal)](/research/2026-09-30-gpui-fork-lab.md#macos-metal).

1. The `gpui-lab` branch, rebased onto main `4e7c62acb` and merged as `eb11056f7`, passed clippy and every workspace test on macOS
   except `zz-daemon` load flakes, which pass alone (its daemon code equals main's).
2. Metal yardstick: `bench/gpui/frames.py`. The lab pin `cc9e4d1804` against `e01edb6b1a`:
   main-thread CPU -5.5% stream, -8.5% grid, -4.9% scroll, -12.6% chrome, no repeat overlapping.
   The Linux stream and scroll regression does not appear on Metal.
3. Retained mode: accessibility is inactive on macOS and views are reused, yet the rebased port
   (fork commit `33656c6f51`) costs +24% in stream, scroll and chrome and saves 1% in grid.
   Rejected again.
4. gpui-fast `9b2f43d` ported as fork commits up to `8aa67c753d` (fonts per size, run rendering, plus an
   `accessibility_active` field in the recorder): -10.8% stream, -3.1% grid, -5.1% scroll, -7.7%
   chrome against `cc9e4d1804`, no repeat overlapping. The font cache alone (`ff805a96b0`)
   gives nearly all of it; run rendering is not proven.
5. Compact scene records, the two Metal-only commits and hot reload: see the lab document.
6. On 2026-10-02 the font cache went to `zz-patches` as `ff805a96b0` plus the recorder field
   `5a00ac89a4` (old tip backed up as `zz-patches-2026-10-02`), and zz moved its pin there. The
   run rendering commit `3aefd2a813` was not carried. Side-by-side screenshots of the old and
   new pin (bold, italic, colors, emoji, CJK, box drawing, ligatures, wrapping, sidebar) matched.

# Still open

- **Hot reload on macOS.** Install dioxus-cli 0.7.10 and sort out the bundle, CEF framework
  layout and codesigning of patch libraries. zz refuses to start outside a bundle that has the
  CEF framework in `../Frameworks` (`macos_cef_framework_is_available` in `crates/zz/src/lib.rs`),
  so a dx-built binary has to run from a bundle with CEF and its helpers.
- **Damage tracking** (zed PR #62455, open). Its present-skip part works on every platform and
   would cut GPU work for frames whose scene did not change, which zz produces when chrome
   notifies without visual change. Unmeasured.

# Proof rules

A change counts when it has an A/B on the Mac against the same base with only that change toggled
(method, command, repeats, spread), builds with zz, and has a written risk list. A fix needs the
failure reproduced first. Numbers from another project's README are unverified. Put each fork change
in its own `gpui: <imperative>` commit with its source repo and hash in the body, push `zz-patches`
only after zz builds against it, and keep a dated backup branch of the previous tip. Record results
in the lab document above and close this handoff (`status: closed`) when the list is done.
