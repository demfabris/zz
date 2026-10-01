---
type: Research
title: GPUI fork lab (gpui-fast, gpui-ce, upstream, hot reload)
description: What zz took from longbridge/gpui-fast, gpui-ce and upstream zed after measuring each candidate on Linux, what it rejected and why, the combined frame-cost result, and the Subsecond hot reload prototype.
resource: Cargo.toml
tags:
- performance
- gpui
- fork
- linux
- hot-reload
timestamp: 2026-10-01T12:00:00Z
---

# Scope

Between 2026-09-30 and 2026-10-01, on the Ubuntu 26.04 dev box (Ryzen 7800X3D, RX 7900 XTX,
GNOME Wayland at scale 1.25), six Codex lanes evaluated GPUI changes for the `demfabris/zed`
`zz-patches` fork, followed by one integration lane:

| Lane | Question |
| --- | --- |
| bench | What does GPUI cost inside zz, and where does it go? Builds the frame-cost yardstick for the others. |
| retained | Does gpui-fast's retained mode (view, layout and text retention) help zz? |
| micro | Which gpui-fast per-frame changes stand alone without retained mode, and which help? |
| ce | What in gpui-ce is worth carrying: fixes, speed, features zz uses? |
| upstream | What did zed land since our base, what open PRs and other forks matter? |
| hotreload | Can zz Dev patch UI code into the running app? |

A candidate counted only with an A/B on this box against the same base, a zz build, and a risk
list. Numbers copied from a README did not count. Everything ran on Linux; macOS is untested
and is the subject of the [macOS handoff](/research/2026-10-01-gpui-mac-handoff.md).

# Yardstick

The fork now carries `gpui: Record env-gated frame phase costs and counts`. With
`GPUI_FRAME_STATS=<path>` set, every drawn frame appends one JSONL record with wall and thread CPU
time for request_layout, prepaint, paint, scene finish and platform submit, plus counts (views,
layout nodes, sprites, glyphs shaped). Unset, it costs one cached branch. The thread clock is
`clock_gettime(CLOCK_THREAD_CPUTIME_ID)` on every unix target.

The lab harness drove zz Dev through the CLI with an isolated daemon, a 1280x900 window and five
fixed scenarios, three 20 s repeats after 5 s warmup each:

| Scenario | Work |
| --- | --- |
| idle | one terminal, nothing happening |
| stream | one terminal printing 240 rows/s |
| grid-one-active | nine panes, 87x30 Manual root, one streaming |
| scroll | scrolling a large scrollback at 20 commands/s |
| chrome | 25-session sidebar plus the palette open (`command-prompt -b -C`) while a pane streams |

Baseline at fork `beca360ad3`, main-thread CPU as a share of one core: idle 0.05% (zero frames
drawn), stream 7.40%, grid 14.70%, scroll 3.20%, chrome 13.50%. In `perf` captures of grid and
chrome, Taffy/layout was the largest resolved domain: 32.5% of grid and 16.7% of chrome main-thread
cycles. About a quarter of samples could not be assigned to a caller. Terminal panes and the
sidebar already render through `AnyView::cached`, so most of the remaining cost is in the active
pane and in chrome that reads `MuxClient`, which every terminal frame notifies.

The recorder itself adds about 0.15 to 0.5 points of CPU, so small claims were checked again with
it off.

# Rejected: retained mode

gpui-fast's retained stack (view retention, layout-node retention, text measurement carry, element
identity) was ported onto the fork. The synthetic `retained_bench` reproduced: 82 to 87% less CPU
when few panels change. In zz it reused **zero** views.

zz frames run with accessibility active. GNOME reports AT-SPI `IsEnabled=true` even with the screen
reader off, AccessKit 0.22 activates on that flag alone, and gpui-fast disables view reuse whenever
accessibility is active. GUI CPU base versus retention on: stream 7.80 vs 8.80%, grid 15.65 vs
13.90%, scroll 3.60 vs 4.10%, chrome 14.10 vs 17.40%. Only the grid improved, from layout
retention. In gpui-fast's own native showcase on this box, retention also replayed about 0 views
per frame; its real-window wins come from its smaller changes, not retention.

The port is 43 files, +12,514/-700 lines, and 33 of our then-58 carried commits touch the same
files. It was not carried. On macOS AccessKit only activates when an assistive client asks, so this
verdict may not hold there; see the handoff.

Measured cost of the always-on GNOME tree on the integrated build: 0.2 to 0.65 points of CPU per
scenario. A lazy-activation patch would need AT-SPI subscription and query-only client handling
and is not worth it at that cost.

# Carried: 27 fork commits

All on `zz-patches`, on top of `e01edb6b1a`, each with its source repo and commit in the body.

**Fixes from gpui-fast (via the micro lane):** wrapped-layout cache keys now include the line clamp
(base reused 19 wrap boundaries for a clamp of one); fallback font resolution keeps bold, italic and
tabular attributes; wrapped text is remeasured for intrinsic size probes (base answered 96x10 with a
cached 36x30).

**Fixes from gpui-ce:** validate atlas upload sizes before allocating; ignore non-finite font
rendering overrides (inf rendered glyphs black); reconfigure the WGPU surface after an aborted frame
(on the 7900 XTX the base starved after an encoding error; patched, all eight error/healthy cycles
presented).

**Fixes from upstream zed:** Linux dispatcher threads stop when their owner drops (from
`3135cdf8dc`); aligned text reserves its full paint bounds for ordering (from `bda9c0bd43`).

**Speed from gpui-fast (14 commits), each A/B'd alone in release builds:**

| Change | Isolated result |
| --- | --- |
| Uniform spatial grid for scene bounds | 12,000-sprite insertion 5.86 to 1.55 ms |
| Cached element path hashes | 3,000-ID tree 9.64 to 6.55 ms |
| Reuse element ID allocations across frames | 9.57 to 8.16 ms, 3,069 fewer allocations |
| Lazy listener and accessibility storage | 3,000-div construction 29% faster |
| Skip text background paint when no run has one | 60-row helper 18.1 to 1.7 us |
| Per-frame glyph raster bound and tile cache | 200x60 glyph painting 11 to 14% faster |
| Keep externally held line layouts | 500 held lines 11.0 to 0.2 ms |
| Reuse unwrapped measurements that fit | 3,000 labels 26.7 to 24.4 ms |
| Read alignment without resolving the full style | 35.7 to 33.9 ms |
| Skip unchanged WGPU uniform writes | submit CPU 95.6 to 88.7 us |
| Cache WGPU instance bind groups | steady 12,000-sprite submit 142.8 to 129.7 us |
| Adaptive decoration reservation | short paragraph shaping 11.6% faster |
| One-pass cached dispatch subtree copy | 0.501 to 0.466 ms |
| Recent native line shape cache | recurring 200x60 shaping 325.6 to 14.4 us/frame |

gpui-fast's compact scene records were dropped: replay improved but sprite sorting regressed 13%
and native scene finish got slower. The two WGPU items do not affect the native Metal renderer.

**Accessibility API from gpui-ce:** opt-in forced tree construction for automation, retained frame
snapshots, fallback clicks clipped to visible bounds, an `aria_disabled` setter. zz-ui reports
disabled Button, Input and NumberInput, and `crates/zz-ui/tests/ce_accessibility.rs` drives the
reasoning-effort slider through the tree.

Rejected from gpui-ce: its WGSL/ScenePlan renderer migration, corner smoothing, custom GPU API,
gradients, transitions and Motion (ours already cover zz's callers, theirs change public structs),
the Parley text stack (open PRs, unmeasured), and the MSAA transient fix (our fork never had the
bug).

# Combined result

Base `beca360ad3` plus the instrument versus all 27 commits, same CLI, same geometry, three
repeats:

| Scenario | Base | Integrated | Change |
| --- | --- | --- | --- |
| idle | 0.05% | 0.00% | 0 frames either way |
| stream | 7.10% | 6.85% | -3.5% |
| grid-one-active | 14.40% | 12.30% | -14.6% |
| scroll | 2.60% | 2.70% | +3.8% |
| chrome | 12.85% | 12.05% | -6.2% |

With the recorder off, grid improved 1.25 and chrome 1.35 points, while stream got 0.35 and scroll
0.20 points worse. Treat stream and scroll as a small regression nobody has explained yet. GUI RSS
moved by under 2 MiB.

The zz workspace suite against the integrated fork: 4,266 pass, 3 fail, 5 ignored. The three
failures do not come from the fork: `codex_counts_only_inside_the_foreground_group` copies
`/usr/bin/sleep` as `codex`, and on Ubuntu 26.04 that is the uutils multicall binary, which refuses
to run under another name; a which-key test fails the same way on base; a loopback test passes
alone. Screenshots of terminal backgrounds, splits, sidebar, palette, which-key, settings hover and
click, and an ACP agent reply all rendered correctly.

# Upstream

Eleven upstream commits since `933d8d93819c` touch GPUI; none is a frame-pacing or renderer
change. All carried patches replayed onto upstream `decbf641b1` with five conflict steps, and
`cargo check -p gpui -p gpui_platform` passed with the Linux backends. That trial branch predates
the 27 commits above.

zed PR #62379 composes native views inside a GPUI window with GPUI overlays above them. For zz's
CEF panes it would not remove the copy CEF's off-screen callback forces, its Linux example has no
CEF, and it conflicts with our masks, blur, zoom and external textures. Not worth adopting before
it lands. Damage tracking (zed PR #62455) and separate image atlas pages (zed PR #60791) are
promising for idle and agent panes but unmeasured.

# Hot reload

`just hot linux` (zz `hotreload` feature, off by default) runs zz Dev through Dioxus Subsecond
0.7.10 with a separate `zz-hotreload` launcher crate. Dioxus's own same-package bin/library layout
reported successful patches while the app kept running the old code. Patches land on the
foreground executor and refresh every window. The sidebar render path is the first hot boundary.

| Loop | Save to pixels |
| --- | --- |
| `just watch linux` (rebuild and relaunch) | median 17.8 s |
| `just hot linux` | median 3.6 s |

Five consecutive patches kept the GUI PID, the daemon connection, terminal sessions and sidebar
state. Each patch keeps its library loaded, about 22 MiB of RSS per patch. Changing the layout of
a struct that lives across patches still needs a restart. gpui-ce PR #292 adds the same mechanism
inside gpui itself; zz did not need a fork hook.

Linker and codegen tweaks (LLD, mold, split debuginfo, codegen units, shared generics) gave no
reliable gain on the incremental zz rebuild under load.
