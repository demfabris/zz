---
type: Concept
title: Terminal frame (TerminalViewport)
description: The immutable, renderer-neutral terminal snapshot (packed cells, interned styles, overlays, cursor, and modes) published from the worker thread and diffed into retained-grid patches.
resource: crates/zz-terminal/src/model.rs
tags: [frame, viewport, packed-cell, patch, immutable, snapshot]
timestamp: 2026-07-31T00:00:00Z
---

# Overview

A terminal frame is one immutable, renderer-neutral snapshot of a pane, modeled as `TerminalViewport` in
`crates/zz-terminal/src/model.rs`. It is what the [`zz-terminal`](/crates/zz-terminal.md) worker
thread produces from [`libghostty-vt`](/terminal/libghostty-vt.md) and publishes; it carries no GPUI types
and no heap allocation per visible cell. Cells are row-major and reference an interned style/grapheme
dictionary, so cloning a frame is a handful of `Arc` bumps. Frames are consumed by
[`/crates/zz-daemon.md`](/crates/zz-daemon.md) fanout and then by GPUI painting per
[rendering-parity](/terminal/rendering-parity.md).

# Schema

`TerminalViewport` fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `generation` | `u64` | Content generation (bumps on `SnapshotChange::Content`). |
| `view_generation` | `u64` | View generation (bumps on every published snapshot). |
| `dictionary_generation` | `u32` | Style/grapheme dictionary epoch; a mismatch forces a full reset. |
| `columns`, `rows` | `u16` | Grid dimensions. |
| `foreground`, `background` | `Color` | Default RGB colors for this frame. |
| `presentation` | `Arc<TerminalPresentation>` | Cold metadata: `title` + optional `hovered_uri`. |
| `cells` | `Arc<[PackedCell]>` | Row-major cell plane (8 bytes/cell). |
| `dictionary` | `Arc<TerminalDictionary>` | Interned `styles: Arc<[PackedStyle]>` + grapheme arena (`grapheme_offsets`, `grapheme_bytes`). |
| `overlays` | `Arc<[OverlaySpan]>` | Selection / search / link-hover / copy-cursor spans, off the hot cell plane. |
| `cursor` | `Option<Cursor>` | Packed cursor (position, style, visible/blinking/wide-tail, color) in one `NonZeroU64`. |
| `scrollbar` | `ScrollbarState` | `total` / `offset` / `len`. |
| `mode` | `TerminalMode` | `Live`, `Copy { position, total }`, or `View { position, total }`. |
| `search` | `Option<SearchStatus>` | Packed current/total + pending/invalid-pattern flags. |
| `unseen_output` | `u32` | Rows of output since the viewport was pinned. |
| `kitty_keyboard`, `mouse_tracking` | `bool` | Active terminal input modes. |
| `status` | `SessionStatus` | `Starting` / `Running` / `Exited` / `Failed` (cold arc-shared payloads). |

Hot records are layout-checked by `const` assertions: `PackedCell` = 8 bytes (`glyph: u32`, `style: u16`,
`flags: u16` with width in the low bits), `PackedStyle` = 16 bytes (fg/bg/underline packed RGB + attribute
bits incl. `ATTR_EXPLICIT_RGB` and `ATTR_HYPERLINK` + underline kind), `OverlaySpan` = 8 bytes,
`Cursor` = 8 bytes, `TerminalViewport` = 144 bytes on 64-bit. Glyphs are either a scalar `char`, `Empty`,
or an index into the grapheme arena flagged by `GRAPHEME_TABLE_BIT`.

# Publication and consumption

The worker builds frames in `build_snapshot`: it walks libghostty dirty rows through `RenderState` +
`RowIterator`/`CellIterator`, interns styles/graphemes into the actor's `ViewportDictionary` (with cell/
overlay plane pooling), and assembles overlays for selection, search matches, hover links, and the copy
cursor. Live dictionaries use viewport-scaled high-water marks: crossing one starts a new dictionary
generation and rebuilds the visible working set, which forces one full viewport instead of retaining
historical styles/graphemes indefinitely. The hard wire budgets are 65,536 styles, 1,048,576 graphemes,
and 16 MiB of grapheme bytes; an unrepresentable visible cell falls back to default style or its first
Unicode scalar rather than terminating the terminal actor.

One publish carries one frame per **active view**, and a view is an attached client:
`publish_active_views` restores each view's scroll anchor, selection, copy-mode, and search state
before snapshotting it, so two devices reading one pane get two frames from the same grid.
`Publisher::publish_viewports` swaps them into `Arc<RwLock<PublishedViewports>>`, a `by_view` map
plus a `fallback` for readers that name no view, and coalesces a single `ViewportReady` event. A
pane with no active view publishes only that fallback, through `Publisher::publish`.
Content publishes are themselves time-coalesced in the actor
(`CONTENT_PUBLISH_STALENESS`, 16 ms): a PTY burst arriving later than that after the previous publish
snapshots immediately, so interactive echo is never delayed. Faster bursts (sustained floods)
defer to a select-deadline arm, capping full-grid snapshot builds on the drain path at ~60 Hz
regardless of throughput. The reader thread likewise folds everything FIONREAD reports as queued into one
pool buffer (up to 64 KiB) before waking the actor, bounded and never blocking past the first read. On
macOS this rarely exceeds a kilobyte (the pty's tiny output queue blocks the producer per ~1 KiB, making
throughput scheduler-roundtrip-bound); Linux's deeper ldisc queue is where the folding batches. The reliable-event queue for `CopyReady`/`OpenUri`/`ViewClosed` is separate
and byte-bounded. If a copy or URI action exceeds that backlog it is discarded and logged without
stopping the actor; loss of the event consumer remains fatal. A client reads its own newest frame via
`TerminalSession::latest_viewport_for(view)`, or `latest_viewport()` for the fallback. The
[server](/crates/zz-daemon.md) reads `latest_viewports()`, diffs each view stream against that view's
previous frame, and fans the result to that view's client over the
[terminal lanes](/protocol/terminal-lanes.md); the app applies it to one retained mutable grid and
repaints changed rows.

# Diffing and patches

To avoid resending whole grids, `TerminalViewport::diff` (or `diff_with_scratch`) produces a
`TerminalViewportPatch`: a `scroll` shift plus strictly-ascending replacement rows (`TerminalPatchRows`,
one contiguous cell plane) and an append-only dictionary delta (`TerminalDictionaryPatch`). Row-shift
detection uses per-row fingerprints (`best_row_shift`). Diff returns `None`, forcing a full reset, when
dimensions or the dictionary generation change, or when the new dictionary does not extend the old one.
`apply_patch` validates the entire patch **atomically** against `base_generation` / dictionary /
dimensions / row bounds / cell references / metadata before mutating retained state, returning a typed
`PatchError` (`Generation`, `Dictionary`, `Dimensions`, `Row`, `Cell`, `Metadata`) that leaves the last
renderable frame intact so the client can resynchronize. `TerminalDiffScratch` reuses fingerprint and
source-identity buffers across successive diffs.

Applying a patch is also where the client keeps its scrollback: a negative `scroll` shift pushes the
departing top rows into that pane's `HistoryRing` before they are overwritten, which is what lets a
scroll back into history render without a round trip. See [zz app](/crates/zz.md).

# Examples

```rust
// Worker side: build a frame and hand it to the publisher.
let frame: TerminalViewport = snapshot(&terminal, /* … */, SnapshotChange::Content, /* … */)?;
publisher.publish(frame);

// Client side: retain and patch instead of replacing the grid.
let patch = TerminalViewport::diff(&previous, &current); // None ⇒ full reset
if let Some(patch) = patch {
    retained.apply_patch(patch)?; // atomic validation; PatchError on mismatch
}
```

# Related

- Produced by [`zz-terminal`](/crates/zz-terminal.md) from [`libghostty-vt`](/terminal/libghostty-vt.md); copy/view frames come from a [mode revision](/terminal/libghostty-vt.md).
- Serialized as [snapshots](/protocol/snapshots.md) and streamed over the [terminal lanes](/protocol/terminal-lanes.md).
- Painted onto GPUI per [rendering-parity](/terminal/rendering-parity.md); default colors come from [appearance](/terminal/appearance.md).
- Overlays and cursor reflect [interaction](/terminal/interaction.md) state; actor publication in [pty-worker](/concepts/pty-worker.md).
