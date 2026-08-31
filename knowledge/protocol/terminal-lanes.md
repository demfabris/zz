---
type: Protocol
title: Packed terminal lanes (terminal_codec.rs)
description: The hand-packed, fixed-width Terminal envelope lane that fans immutable terminal viewports and row patches out to clients with deduplicated style and grapheme dictionaries over one ordered stream, local or ssh-forwarded.
resource: crates/zz-protocol/src/terminal_codec.rs
tags: [protocol, terminal, wire, packing, fanout]
timestamp: 2026-08-31T00:00:00-03:00
---

# Overview

The **Terminal lane** (envelope lane `1`) is a bespoke, allocation-conscious binary encoding for the
high-rate terminal fanout path. Instead of `postcard`-serializing a `TerminalViewport`, the codec in
`crates/zz-protocol/src/terminal_codec.rs` hand-packs the viewport into **fixed-width little-endian
sections** so the daemon can broadcast frames to many clients and each client can decode directly into
`Arc`-shared planes without an intermediate structure. It exists because full terminal frames dominate
traffic; the compact layout plus a shared **style dictionary** and **grapheme dictionary** keeps frames
small . a single-row patch on an 80-column grid is a few hundred bytes.

`encode_protocol_message` is the lane router: it sends `EventPayload::TerminalViewport`,
`EventPayload::TerminalPatch`, and
`EventPayload::CommandOutput { output_id, viewport: Some(..), .. }` down the Terminal lane, and
everything else through the `postcard` [Control lane](/protocol/wire-protocol.md).
All payload types (`TerminalViewport`, `TerminalViewportPatch`, `PackedCell`, `PackedStyle`,
`OverlaySpan`, `Cursor`, …) come from [zz-terminal](/crates/zz-terminal.md).

Every attached client holds its own view of a terminal (`TerminalViewId(client.0)`), and the daemon
runs one diff stream per (pane, view) pair. Two devices watching the same pane get their own base
generations and their own patch chains.

# Update kinds

The first payload byte after the envelope selects one of three record types:

| Byte | Constant | Carries | Decodes to |
|------|----------|---------|------------|
| `0` | `FULL_VIEWPORT` | complete `TerminalViewport` | `EventPayload::TerminalViewport` |
| `1` | `VIEWPORT_PATCH` | `TerminalViewportPatch` (changed rows + dictionary appends) | `EventPayload::TerminalPatch` |
| `2` | `COMMAND_OUTPUT_VIEWPORT` | nonzero `output_id` + complete `TerminalViewport` | `EventPayload::CommandOutput { output_id, viewport: Some(..), .. }` |

A full viewport is a self-contained frame; a patch references a base generation and only ships the
rows that changed plus any newly-appended dictionary entries.

# Wire layout . full viewport (`FULL_VIEWPORT` / `COMMAND_OUTPUT_VIEWPORT`)

The ordinary full viewport uses a fixed 115-byte non-variadic header, then variable metadata, packed
section counts, and the sections themselves. Offsets below are byte positions inside the payload,
after the 8-byte envelope:

```
off  size  field
  0    1   kind:u8 = 0
  1    8   pane:u64
  9    8   sequence:u64
 17    8   generation:u64
 25    8   view_generation:u64
 33    4   dictionary_generation:u32
 37    2   columns:u16
 39    2   rows:u16
 41    4   foreground:u32              (packed 24-bit color)
 45    4   background:u32
 49    8   scrollbar.total:u64
 57    8   scrollbar.offset:u64
 65    8   scrollbar.len:u64
 73    1   kitty_keyboard:u8
 74    1   mouse_tracking:u8
 75  1|17  mode
  …  1|10  search
  …    8   unseen_output:u64
  …  1|11  cursor
  …    4   title_len:u32
  …    4   working_directory_len:u32
  …    4   hovered_uri_len:u32
  …    4   cell_count:u32
  …    4   style_count:u32
  …    4   grapheme_offset_count:u32
  …    4   grapheme_byte_count:u32
  …    4   overlay_count:u32
  …   var  status
── variable-length sections, in order ──
title bytes │ working_directory bytes │ hovered_uri bytes
cells   [glyph:u32 style_id:u16 flags:u16]      × cell_count      (8 B each)
styles  [fg:u32 bg:u32 underline:u32 attrs:u16 underline_kind:u8 0:u8] × style_count (16 B each)
grapheme_offsets [u32] × grapheme_offset_count                    (4 B each)
grapheme_bytes  (UTF-8 arena)                                     × grapheme_byte_count
overlays [row:u16 start:u16 end:u16 kind_and_flags:u16] × overlay_count (8 B each)
kitty_placement_count:u32
kitty placements [72 B each] × kitty_placement_count              (see table)
```

`COMMAND_OUTPUT_VIEWPORT` uses kind 2 and inserts a nonzero `output_id:u64` at offset 17, after
`sequence`. Every field from `generation` onward shifts forward by eight bytes, so its non-variadic
header occupies 123 bytes. The codec rejects a missing or zero actor ID for this populated form.
`EventPayload::CommandOutput { output_id, viewport: None, .. }` remains on the reliable postcard
Control lane. Only the zero-ID `None` form represents an authoritative resync with no live output.

The ordinary viewport's 115 fixed bytes are the 75 above `mode` plus `unseen_output` (8) plus the
eight `u32` counts (32). `mode`, `search`, `cursor`, and `status` are sized separately because each
is a tagged niche.

# Wire layout . row patch (`VIEWPORT_PATCH`)

Fixed header is `PATCH_FIXED_BYTES = 143`. It adds `base_generation:u64` and
`base_view_generation:u64` (the frame this patch applies onto), a signed `scroll:i16` row shift with a
`u16` reserved-zero pad, and dictionary **base offsets** so appended entries extend the shared
dictionaries rather than replacing them:

```
off  size  field
  0    1   kind = 1
  1    8   pane:u64
  9    8   sequence:u64
 17    8   base_generation:u64
 25    8   base_view_generation:u64
 33    8   generation:u64
 41    8   view_generation:u64
 49    4   dictionary_generation:u32
 53    2   columns:u16
 55    2   rows:u16
 57    2   scroll:i16
 59    2   reserved:u16 (= 0)
 61    4   foreground:u32
 65    4   background:u32
 69   24   scrollbar total/offset/len:3×u64
 93    1   kitty_keyboard:u8
 94    1   mouse_tracking:u8
 95  1|17  mode
  …  1|10  search
  …    8   unseen_output:u64
  …  1|11  cursor
  …    4   title_len:u32
  …    4   working_directory_len:u32
  …    4   hovered_uri_len:u32
  …    4   changed_row_count:u32
  …    4   style_base:u32
  …    4   appended_style_count:u32
  …    4   grapheme_base:u32
  …    4   appended_grapheme_len_count:u32
  …    4   appended_grapheme_byte_count:u32
  …    4   overlay_count:u32
  …   var  status
── sections ──
title │ working_directory │ hovered_uri
changed_rows [row:u16  0:u16  (cells[columns] × 8 B)] × changed_row_count
appended_styles   × appended_style_count   (16 B each)
appended_grapheme_lengths [u32] × count    (4 B each)
appended_grapheme_bytes (UTF-8)            × appended_grapheme_byte_count
overlays × overlay_count                   (8 B each)
kitty_placement_count:u32
kitty placements [72 B each] × kitty_placement_count
```

The 143 fixed bytes are the 95 above `mode` plus `unseen_output` (8) plus the ten `u32` counts (40).

`style_base`/`grapheme_base` are the count of dictionary entries the receiver already holds; the patch
only carries entries appended since the base frame. `scroll` encodes a vertical shift of the retained
grid: positive shifts must re-supply the newly-exposed top rows, negative the bottom rows (validated).

# Schema . packed section elements

| Element | Bytes | Fields (LE) |
|---------|-------|-------------|
| Cell (`PackedCell`) | 8 | `glyph:u32`, `style_id:u16`, `flags:u16`. `glyph` is a Unicode scalar, or a grapheme-dictionary index when `GRAPHEME_TABLE_BIT` is set |
| Style (`PackedStyle`) | 16 | `foreground:u32`, `background:u32`, `underline_color:u32`, `attributes:u16`, `underline_kind:u8`, reserved `0:u8` |
| Grapheme offset | 4 | `u32` byte-offset into the grapheme arena (monotonic, first `0`, last = arena len) |
| Overlay (`OverlaySpan`) | 8 | `row:u16`, `start:u16`, `end:u16`, `kind_and_flags:u16` |
| Kitty placement | 72 | `image_id:u32`, `image_generation:u64`, `layer:u8`, `has_source_rect:u8`, reserved `0:u16`, `viewport_col:i32`, `viewport_row:i32`, `absolute_row:u64`, `cell_offset_x:u32`, `cell_offset_y:u32`, `grid_cols:u32`, `grid_rows:u32`, `pixel_width:u32`, `pixel_height:u32`, source rect `x/y/width/height:u32` (zeros when `has_source_rect` is 0). `layer` is `0` BelowBg, `1` BelowText, `2` AboveText |
| Row record (patch) | 4 + 8·cols | `row:u16`, reserved `0:u16`, then one cell per column |

**Colors** are packed to 24 bits (`decode_color` rejects values `> 0x00ff_ffff`; underline color may
also be the sentinel `NO_COLOR`). **Variable metadata** uses tag-prefixed niches:
`mode` = 1 byte (`Live`) or 17 (`Copy`/`View` with two `u64`); `search` = 1 or 10 bytes;
`cursor` = 1 or 11 bytes; `status` = `Starting`/`Running` (1 byte) or `Exited`/`Failed` (with a
length-prefixed string).

## Deduplication . the two dictionaries

Compactness comes from storing each style and grapheme once per frame and referencing it by index.
Cells store a `u16` `style_id` indexing a shared style dictionary and a `glyph` that either is a
scalar or (with `GRAPHEME_TABLE_BIT`) indexes a grapheme dictionary of UTF-8 spans described by an
offset table + byte arena. A patch appends only new dictionary entries beyond
`style_base`/`grapheme_base`, so a client's cell references stay valid across frames.

# Validation & safety limits

Both directions validate before exposing data. Section counts are checked against caps *before*
allocation, and the decoder **preflights** every declared section against the remaining bytes (exact
remaining after the kitty-placement trailer, not after overlays) so a lying length cannot force an
over-allocation:

The cloned preflight reader treats bounded status, title, working-directory, and hovered-URI payloads
as raw byte ranges; its job is only to prove that every declared section fits exactly. The
authoritative reader then performs the single UTF-8 validation while materializing those strings, so
malformed text is still rejected without scanning valid metadata twice per frame.

| Limit | Value |
|-------|-------|
| `MAX_TITLE_BYTES` | 64 KiB |
| `MAX_WORKING_DIRECTORY_BYTES` | 16 KiB |
| `MAX_HOVER_URI_BYTES` | 16 KiB |
| `MAX_STATUS_BYTES` | 1 MiB |
| `MAX_STYLE_COUNT` | 65 536 |
| `MAX_GRAPHEME_COUNT` | 1 MiB |
| `MAX_GRAPHEME_BYTES` | 16 MiB |
| `MAX_OVERLAY_COUNT` | 1 MiB |
| `MAX_KITTY_PLACEMENTS` | 512 |

`validate_viewport` / `validate_patch` further check: cell count matches `columns × rows`; every cell's
`style_id` and grapheme index resolve; grapheme offsets are monotonic and UTF-8-valid; overlays and the
cursor stay inside the grid; scrollbar ranges are consistent; hovered URIs contain no control or
whitespace characters; the working directory (a decoded path reported through OSC 7, OSC 9, or OSC
1337, empty when the shell never reported one) contains no control characters. Failures surface as
`ProtocolError::InvalidTerminal(..)`.

# Delivery

Terminal frames share one ordered stream with the Control lane and arrive in order, over a Unix
socket, a named pipe, or an SSH-carried byte stream. Frames are never compressed: the
envelope flags byte is reserved, and the zstd bit that only the QUIC writer ever set went with QUIC
at protocol v43.

Supersession therefore lives entirely in the daemon's outbound mailbox rather than the transport: one
pending frame per pane, newest replacing stale under backpressure (see
[zz-daemon](/crates/zz-daemon.md)). A stalled client still converges on the latest frame; what it no
longer does is discard bytes already in flight.

Protocol v87 separates terminal delivery scope from foreground authority. `visible_terminals` still
contains only the attached client's focused window, after zoom filtering, and remains the source for
input, history, and PTY geometry. When an attached Interactive client sends
`SetTerminalPreview { enabled: true }`, `streamed_terminals` also includes every terminal pane in
every window of that attached session as `Preview`. Each pane keeps the same client-specific
`TerminalViewId`, so enabling previews adds frame delivery without selecting a window or resizing a
PTY. Disabling previews suspends those extra streams. Panes in other sessions are not streamed until
the client attaches that session.

Preview frames are bounded and lower priority. Their pending slots and bytes reserve room for the
foreground pane set; reliable messages and foreground terminal frames can discard queued previews,
which are retried as a latest full viewport after the writer makes progress. The daemon does not
enqueue Kitty image payloads for preview panes. Foreground frames retain their existing Kitty image
delivery and full terminal behavior.

Command output uses one separate coalesced slot. The mailbox retains the actor ID beside the encoded
frame, refuses an older actor frame after a newer one is pending, and lets a reliable close discard
pending frames whose IDs are no newer than that close. An authoritative zero-ID empty resync clears
the slot. `ClientCore` keeps its own watermark, so a delayed frame cannot reopen an actor after its
close. Every newly adopted handshake resets that watermark because it may come from a restarted
daemon with a fresh ID lifetime; reconnecting to the same daemon does not restart its counter.

Recovery is per pane. When a client receives a patch it cannot apply, or a patch for a pane it has no
base frame for, it sends `ProtocolMessage::RequestFull { pane }` and the daemon replies with that
client's latest full viewport for the pane. The daemon drops the request when the pane is neither
foreground nor preview-streamed to that client, and every snapshot re-arms the client's recovery
window, so a delivery-scope race cannot wedge a pane. `HistoryRequest` remains limited to foreground
panes.

# Scrollback backfill

The lane carries the visible grid only. Scrollback above it travels on the Control lane as
`EventPayload::HistoryChunk`, which reuses `PackedCell` rows and a `TerminalDictionary` but is
`postcard`-encoded rather than packed. A client requests chunks with
`HistoryRequest { pane, start, count }`, keeps the rows in a bounded ring beside the retained
viewport, and validates each chunk before it lands: rows must be nonempty, at most 512, and exactly
`columns` wide; dictionary offsets must start at zero, stay monotonic, end at the arena length, and
slice into valid UTF-8; every cell's style and grapheme index must resolve.

Nothing on the wire tells a client its ring went stale. There is no history epoch, by design. The
client infers invalidation while applying a patch, from what the patch itself says: a column change,
a `scrollbar.total` that moved backwards or by more than the row shift, an offset delta inconsistent
with the shift, or a full-grid row replacement all drop the retained history. Rows leaving the top of
the grid on a negative scroll are pushed onto the back of the ring. Eviction from the front is the
shift minus the offset delta, because the offset delta is the truthful basis: at the live tail offset
advances with total, while a scroll toward the tail advances offset and evicts nothing.

# Examples

```rust
// Lane routing: encode_protocol_message picks the lane from the payload.
let frame = encode_protocol_message(&ProtocolMessage::Event(Event {
    sequence: 99,
    payload: EventPayload::TerminalViewport { pane: PaneId(12), viewport },
}))?;
assert_eq!(frame[4], /* Lane::Terminal */ 1);   // byte 4 of the envelope

// Buffer-reusing variants keep one allocation across frames (hot fanout path):
let mut buf = Vec::new();
encode_protocol_message_into(&message, &mut buf)?;      // reuses buf capacity
let msg = read_protocol_message_into(&mut reader, &mut buf)?;

// A cold "exited status" section on the wire (kind 2, code 7, signal "TERM"):
//   02 07 00 00 00  01  04 00 00 00  54 45 52 4D
//   ^status=Exited  ^code=7  ^has-signal ^len=4  "TERM"
```

Public entrypoints: `encode_protocol_message[_into]`, `decode_protocol_frame`,
`read_protocol_message[_into]`, `write_protocol_message`.

# Related

- The router and its sibling Control lane: [wire protocol](/protocol/wire-protocol.md).
- Part of [the zz-protocol crate](/crates/zz-protocol.md).
- Payload types (`TerminalViewport`, `PackedCell`, `PackedStyle`, dictionaries) live in
  [zz-terminal](/crates/zz-terminal.md); see [the terminal frame concept](/concepts/terminal-frame.md).
- Produced by [zz-daemon](/crates/zz-daemon.md)'s frame fanout, decoded by [the GPUI app](/crates/zz.md);
  informs [rendering parity](/terminal/rendering-parity.md).
- Identifiers in the header: [stable IDs](/protocol/ids.md).
