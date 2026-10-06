---
type: Research
title: Where a zz frame goes (pane count, window area, focus glow)
description: Measured how zz's per-frame CPU and GPU cost grows with panes on screen and window area, compared it with Ghostty, and recorded what came out of it (a cached pane header, a closed-form inset shadow blur, and a rejected whole-pane cache).
resource: crates/zz/src/workspace/view.rs
tags:
- performance
- macos
- gpu
- layout
- gpui
timestamp: 2026-10-06T00:00:00Z
---

# Question

Asked after a research pass on whether zz needs a VT engine and whether GPUI limits it. The VT
engine is not the cost: the daemon parses VT once and clients get compact cell frames. The open
question was the renderer: GPUI draws the whole window into one surface on every frame. How much
of a frame is work for pixels or panes that did not change, and is a per-pane layer design worth
building?

# Method

MacBook Pro M4 Max, macOS 27.0, profiling bundles (`cargo xtask bundle-cef --profile
profiling`) of `2ecd80bb1` and later commits, each in its own worktree. The driver reused
`bench/gpui/frames.py` (isolated `HOME` and socket, perl writer at 240 rows/s, 5 s warmup, 20 s
window, 3 repeats) with one added scenario, `fixed`: the streaming pane resized to 100x30 beside
two idle panes. Windows sat on the secondary display (DELL P2725QE, 2688x1512 pt at scale 2)
through a `window-state.json` that names the display's UUID; without the UUID zz centers the
window on the main display. Small window 1280x900 pt, large 2672x1482 pt (3.4x the area).
Power came from `sudo powermetrics --samplers cpu_power,gpu_power,tasks -f plist`; macOS 27's
powermetrics has no per-process GPU time, so per-process GPU came from `xctrace record
--template "Metal System Trace" --attach <pid>` and the `metal-gpu-intervals` table. With
nothing running this machine idled at 614 mW package and WindowServer at 39% CPU, so deltas
matter, not absolutes.

# Findings

**Layout grew with panes on screen, not with what changed.** One pane streaming, the others
idle, small window, main-thread CPU as a share of one core:

| Panes | Main thread | Layout per frame | Layout nodes |
| --- | --- | --- | --- |
| 1 | 9.8% | 0.26 ms | 47 |
| 3 | 10.5% | 0.43 ms | 111 |
| 9 | 15.7% | 1.35 ms | 297 |

Views rendered stayed at 5 per frame: the terminal content was already a cached view. The cost
was about 31 layout nodes of pane chrome per pane (header, surface, wrappers) and about 6 per
split, laid out again on every frame because GPUI dirties every ancestor of the view that
notified, and the workspace that holds the chrome always re-renders.

**Window area cost GPU, not CPU.** The same 100x30 streaming pane in the large window instead of
the small one: GPU power 371 to 658 mW, package +395 mW, GPU busy 5 to 21%, main thread only
10.5 to 12.3%.

**Most of that GPU time was the focus glow.** Metal traces of a full-window stream in the large
window, zz fragment time per second of wall time: 245 ms default, 92 ms with
`pane-glow-strength = 0`, 244 ms with `shadow-strength = 0`, 245 ms with
`pane-background-opacity = 1`. The glow is an inset shadow with a 96 pt blur over the whole
pane, and every pixel ran GPUI's 4-sample corner-aware blur loop. `window-background-blur =
true` left zz's own GPU time alone but doubled WindowServer's (87 to 172 ms/s).

**Ghostty 1.3.1 on the same stream used more CPU, less GPU.** 35-47% of a core against zz's
20-26% (zz's number includes the daemon). In the large window its GPU power read 284 mW against
zz's 1099 mW, but its frame rate was not counted and its small window read higher than its large
one, so that comparison is a lead, not a result.

# What changed

| Change | Where | Result (median of 3) |
| --- | --- | --- |
| Terminal pane header as a cached view (`TerminalHeaderView`) | zz `760ca933e` | 9 panes: main thread 16.3 to 11.7%, layout 1.59 to 0.78 ms, nodes 297 to 117 |
| Closed-form blur for wide inset shadows, interior discard | `demfabris/gpui` `1b395e5` | zz fragment time in the large window 237 to 144 ms/s; offscreen glow 0.69 to 0.28 ms per frame |

Rejected: the whole pane frame (surface, header, content) as one cached view. GPUI re-renders
every cached view nested inside a cached view that re-renders (`window.refreshing` is set for the
subtree in `crates/gpui/src/view.rs`), and the active pane's frame re-renders on every terminal
frame because the terminal is its child. The header inside it then re-rendered too: views per
frame 5 to 7, layout nodes for one pane 27 to 49, 9-pane main thread 11.6 to 13.5% in the same
run. After the header fix a pane costs about 6 cheap nodes, so there was little left to win.

The shader change keeps the old loop for shadows whose corners are larger than a quarter of the
blur. Renders of the glow at 8x zz's strength differ from the old shader by at most 2/255 per
channel.

# Not done

- Per-pane OS layers (one `CALayer` or `wl_subsurface` per pane). The window-area result says
  they would save GPU power on large windows; after the glow fix the remaining gap is smaller and
  unmeasured.
- Splits still cost about 6 layout nodes each on every frame (about 44 of the 117 in a 9-pane
  grid). They are elements of the workspace view, which re-renders on every frame, so a cached
  view cannot skip them. Fewer nodes per split, or a GPUI change that reuses clean nested caches
  inside a dirty view, are the remaining CPU levers.
- Ghostty's frame rate during the comparison. Its launcher also prompts before running a command
  passed with `-e`; put the command in the config file instead.
