---
type: Design Plan
title: TUI client - zz in a terminal, browsers included
description: A second presentation backend that renders zz sessions in a raw TTY - cell blit for terminals, text views for agents, kitty-graphics CEF frames for browsers - turning the GUI into one of three faces of the daemon.
status: Rungs 1 + 3 landed 2026-08-09 (chrome, sidebar v2, kitty bridge, CEF browser panes via provider seam — headless-smoked incl. a real Chromium frame over PTY) + frame transport v2 same day (t=f file medium probed at startup, zlib inline fallback, CEF damage seam, byte-debt pacing); rung 2 (agent panes) proposed
tags:
- tui
- client
- browser
- kitty-graphics
- remote
timestamp: 2026-08-09T00:00:00Z
---

# Overview

The TUI is not a port of zz — it is a **second presentation backend for the client
stack that already exists**. The daemon owns all mux state (layout, key tables,
copy mode, status) and ships terminal content as rendered cell grids
(`TerminalViewport`/`PackedCell` in `zz-protocol`), so a TTY client is a pure view:
blit cells, forward keys, resolve `LayoutNode` ratios into cell rects. Combined with
`--host` (see the [tmux superset roadmap](/designs/tmux-superset-roadmap.md)),
`zz --host box attach` from any terminal closes the last structural gap between zz
and "drop-in tmux replacement."

The product inversion this completes: zz stops being "a GUI terminal that contains
browsers" and becomes "a mux whose sessions contain browsers, viewable from any
client — GUI, TTY, iOS." The daemon was always the product; the GUI becomes one face.

# Prior art validating the weird part

[zenbu-labs/terminal-browser][1] ships a real browser inside a PTY: Chromium via
Electron's offscreen API reading pixels off the GPU, encoded to the **Kitty graphics
protocol** (supported by ghostty, kitty, cmux, vscode, and others), with terminal
mouse/keyboard converted to synthetic Chromium events. That is architecturally
identical to zz's existing browser pane (CEF-OSR, GPU frame readback, synthetic
input, daemon-held tab descriptors) with one difference: their blit target is kitty
escape sequences, zz's is a GPUI surface. They built the whole stack to get there;
zz only needs the last encode step.

# The rung ladder

Ship each rung independently; never let a higher rung block a lower one.

1. **Attach client (terminal panes) — LANDED 2026-08-09.** Cell blit + key
   forwarding + resize. Prefix engine, copy mode, choosers, status are daemon-side
   already. Browser/agent/editor panes render as placeholder cards (kind, title,
   URL, "open in GUI"). Delivered as `crates/zz-tui` (lib + standalone binary;
   `zz attach` in a TTY dispatches to it) and live-smoked end to end: attach,
   typing, prefix engine + PREFIX indicator, command prompt, `split-window <cmd>`,
   pane navigation, detach-client push, Ctrl-\ detach, reattach with layout
   intact, fresh empty-daemon lazy-create on Interactive attach. Known gap: a bare `split-window` opens a
   Picker pane the TUI can only placeholder — use the command prompt's
   `split-window <cmd>` form, or teach the daemon a TUI-kind default later.
2. **Agent panes as text.** ACP transcripts are structured data; a terminal is
   their native habitat. Composer input, permission prompts, and the attention
   rollup map to plain TUI affordances.
3. **Browser panes via kitty graphics**, capability-gated twice: the outer terminal
   must answer the kitty graphics query escape, and the host must have the CEF
   bundle. The kitty-encode half landed 2026-08-09 as the inline-image bridge
   (probe + transmit + placement reconciliation in crates/zz-tui/src/kitty.rs),
   so this rung reduces to CEF frames as the image source, injected through a
   provider seam so zz-tui's dependency tree stays CEF-free (the fat `zz attach`
   binary wires it in; standalone zz-tui keeps placeholder cards). Anything else keeps the rung-1 placeholder. CEF runs **in the TUI client
   process** — symmetric with the GUI, so browser descriptors (v46 tab persistence)
   materialize wherever you attach; frames take the CPU-readback path → kitty
   encode into the pane's cell rect.

# Packaging (settled 2026-08-09)

Separate package `crates/zz-tui`, independent from presentation shells. It depends on `zz-client` for reduction and
chrome tables, the client-only `zz-daemon` transport, `zz-protocol`, model-only
`zz-terminal`, and small runtime/encoding crates. Lib + thin `[[bin]]` in one crate: `crates/zz` links the lib for
`zz attach`, the standalone binary serves headless boxes. One forced extraction:
`configured_fleet_hosts`/`HostEntry` move from `crates/zz/src/config/mod.rs`
(gpui-entangled module) down to `zz-daemon` beside `Endpoint`. The gpui-free dep
tree doubles as compile-time enforcement of what the TUI must never grow: VT
parsing, daemon option semantics, or pane key-table execution. Layout projection
remains presentation work, and client chrome resolves through `ChromeKeymap`.

# Chrome direction (settled 2026-08-09, after rung 1)

The TUI mirrors the zz app's UI structure, not tmux's: a toggleable left
sidebar tree (host → sessions → windows → panes, `+ new pane` row, tmux status
line at the sidebar bottom), the pane canvas in the center, and the pane-kind
picker as a floating card with key hints — "the gpui app rendered in TUI
symbols." The tree is a pure view over the `MuxSnapshot` the client already
holds (same source the GUI sidebar reads), which also gives session switching
for free. **Landed 2026-08-09** (second codex batch, crates/zz-tui only,
live-smoked incl. yank→OSC 52 verified in the outer terminal's clipboard);
one review fix worth keeping in mind: copy/view indicators ride viewport
frames, so the frame paint path must refresh the status area or the
copy-mode indicator freezes. Picker selections execute the existing materialize-pane command —
browser panes created from a TUI materialize as daemon descriptors and are
live when a GUI next attaches; the agent entry is hinted "(runs in the zz
app)". Focus model: sidebar-focus toggle (`EventPayload::FocusSidebar`
exists), arrows + Enter, mouse on tree rows; sidebar auto-hides under a
minimum width like the GUI slideover. Sidebar Up/k, Down/j, Enter, r, Escape, and
q now come from the TUI `sidebar` chrome table instead of inline chord matches.
tmux muscle memory is untouched because the prefix engine and pane key tables stay daemon-side. Deferred: multi-host tree
(one `InteractiveClient` per host side by side, a later rung, not a v1
compromise).

# Input

- SGR pixel mouse (mode 1016) gives pixel-precision coordinates in supporting
  terminals — no OS-level event helper needed (zenbu's Swift listener is their
  workaround for cell-granularity mice).
- zz's own VT already speaks the kitty keyboard protocol, so zz-in-zz gets full key
  fidelity; other terminals degrade to legacy encoding.

# Known hard parts

- **CEF headless is heavy**: software rendering (no GPU on servers), a large bundle
  on machines that are otherwise "a box I ssh into." Rung 3 stays optional forever.
- **The recursive gap — CLOSED 2026-08-09**: kitty graphics landed in the zz VT
  and full pipeline (placements in frames, image payloads on the reliable queue,
  three-layer GUI paint; protocol 48), so zz-TUI inside zz's own GUI can show real
  browser panes, and the TUI can re-encode inline images arriving from remote
  panes to any kitty-capable outer terminal. The long pole for the zz-in-zz demo
  is done.
- **Small-viewport geometry**: a TUI attaching to a session a big GUI also views
  exercises `terminal_geometry_owner` (most-recent-input client wins, verbatim) far
  more often than today's multi-GUI setups do.
- **Frame bandwidth — CLOSED 2026-08-09 (transport v2)**: pixel bytes no longer
  cross the PTY on capable terminals. Startup probes `a=q,t=f` (a 1×1 probe file);
  on OK, every image transmit writes RGBA into one of 8 rotating temp slot files
  and sends a ~100-byte escape naming the path (kitty file medium). Fallback is
  the old inline base64, now zlib (`o=z`) and paced by a bytes/sec debt
  (`ZZ_TUI_FRAME_BUDGET_MBPS`, default 3 MiB/s) instead of a fixed frame
  interval. CEF dirty rects ride the provider seam so unchanged frames skip
  hashing entirely. Design lifted from zenbu-labs/terminal-browser (see the
  dissection memory). Validation caps carry 1 MiB headroom above the clamp
  budget — providers render at rounded logical sizes, so frames legitimately
  come back a pixel wider than the clamp target.

# Sequencing

Rung 1 after the Tier 2 review lands — it hardens the same "non-GUI client of the
daemon" seams the [native iPhone client](/designs/ios-client.md) now consumes.
Kitty graphics in the VT landed 2026-08-09 (protocol 48), so rung 3 now waits only
on rung 1 plus the CEF-headless decision.

# Citations

1. https://github.com/zenbu-labs/terminal-browser — browser-in-PTY prior art:
   Chromium OSR → kitty graphics, Rust + TypeScript, macOS-first.

[1]: https://github.com/zenbu-labs/terminal-browser
