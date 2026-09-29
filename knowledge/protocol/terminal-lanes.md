---
type: Protocol
title: PaneFrame terminal lane (pane_frame.rs)
description: The Terminal envelope lane that carries full viewports, span patches, command-output viewports and history chunks as varint headers, changed-metadata fields and rows of style runs with UTF-8 text, decoded by every client straight into PackedCell planes.
resource: crates/zz-protocol/src/pane_frame.rs
tags: [protocol, terminal, wire, packing, fanout]
timestamp: 2026-09-29T18:00:00Z
---

# Overview

The **Terminal lane** (envelope lane `1`) carries every terminal grid the daemon sends: full
viewports, patches, populated command-output viewports and scrollback history chunks. The codec
lives in `crates/zz-protocol/src/pane_frame.rs`; `terminal_codec.rs` routes messages to it.

Since W2-TERM (protocol 107, unreleased) a frame is a **PaneFrame**: varint header fields, a
bitset of the metadata the frame carries, and rows written as runs of cells that share a style,
with the glyphs as UTF-8. Trailing default blank cells are never sent, and a patch sends only
the changed column span of each changed row and only the metadata that changed. The old layout
(fixed 115/143-byte headers and 8 bytes per cell) is gone.

Clients decode into the same in-memory types as before (`TerminalViewport`,
`TerminalViewportPatch`, `PackedCell` planes, `TerminalDictionary`), so renderers, the retained
grids and `include/zz-client.h` did not change.

`encode_protocol_message` sends `EventPayload::TerminalViewport`, `TerminalPatch`,
`CommandOutput { viewport: Some(..) }` and `HistoryChunk` down this lane and everything else
through the `postcard` [Control lane](/protocol/wire-protocol.md). The frame encoders are also
callable on their own: `encode_terminal_viewport_event_into` and
`encode_terminal_patch_event_into` write one enveloped frame into a caller-owned buffer.

# Measured sizes

| Frame | Old layout | PaneFrame |
|---|---|---|
| blank 180x50 full viewport | 72,203 B | under 64 B |
| echo of one key (`§001` plus the cursor move), 120x40 | 1,133 B on the wire per key | under 40 B |
| attach of four panes (`attach.wire_s2c.p4`), terminal part | about 72 KB | see the W2-TERM as-built notes |

# Frame kinds

The first payload byte after the 8-byte envelope:

| Byte | Kind | Decodes to |
|---|---|---|
| `0` | full viewport | `EventPayload::TerminalViewport` |
| `1` | patch | `EventPayload::TerminalPatch` |
| `2` | command-output viewport | `EventPayload::CommandOutput { viewport: Some(..) }` |
| `3` | history chunk | `EventPayload::HistoryChunk` |

Then every kind has `pane` (varint) and `sequence` (varint). The sequence is the pane's
**stream sequence**: `TerminalSession::next_stream_sequence`, counted per terminal session from 1
(a respawned pane starts again), shared by the pane's frames to every client. Clients do not
read it today; W4-DELIVER uses it to order pane notifications after the pane's frames. A
command-output frame then has a nonzero `output_id` varint.

All integers are LEB128 varints unless the table says otherwise. A signed or wrapping value is a
zigzag varint of the wrapping difference, so any `u64` pair round-trips.

# Full viewport body

```
view_generation                         varint
view_generation - generation            zigzag
dictionary_generation                   varint
columns, rows                           varint, varint
fields                                  varint bitset (metadata present)
metadata sections, in field-bit order
styles:     count varint, then each style
graphemes:  count varint, each length varint, then all bytes
rows:       row records, then 0
```

A metadata field whose bit is clear takes its default (black colours, the default presentation,
no overlays, no placements, no cursor, a zero scrollbar, `Live`, no search, no unseen output,
input modes off, `Starting`). Cells not covered by a row record are `PackedCell::EMPTY`
(glyph 0, style 0, flags 0), and a row with no non-empty cell is not sent.

# Patch body

```
base_view_generation                    varint
base_view_generation - base_generation  zigzag
view_generation - base_view_generation  zigzag
generation - base_generation            zigzag
dictionary_generation                   varint
columns, rows                           varint, varint
fields                                  varint bitset
[SCROLL]      scroll                    zigzag, 0 < |scroll| < rows
metadata sections, in field-bit order
[DICTIONARY]  style_base varint, styles; grapheme_base varint, appended graphemes
[ROWS]        span records, then 0
```

A metadata field whose bit is clear keeps the base frame's value. `style_base` and
`grapheme_base` are only on the wire with a dictionary append; `apply_patch` checks them against
the retained dictionary then, and otherwise uses its own counts.

# Field bits

The same bitset (`zz_terminal::TerminalPatchFields`) names what a frame carries:

| Bit | Field | Wire |
|---|---|---|
| 0 | `ROWS` (patch) | span records |
| 1 | `CURSOR_AT` (patch) | `column << 1 | at_wide_tail`, `row`: a move of a present cursor, appearance kept |
| 2 | `SCROLL` (patch) | row shift |
| 3 | `SCROLLBAR` | `total`, `offset`, `len` |
| 4 | `DICTIONARY` (patch) | appended styles and graphemes |
| 5 | `OVERLAYS` | count, then `row`, `start`, `end`, `kind_and_flags` per span |
| 6 | `CURSOR` | flags byte (present, visible, blinking, wide tail, style in bits 4-5), then `column`, `row`, 3-byte colour when present |
| 7 | `PRESENTATION` | title (length + UTF-8), working directory and hovered URI (length + 1, 0 for none) |
| 8 | `COLORS` | foreground and background, 3 bytes each |
| 9 | `MODE` | tag (`0` live, `1` copy, `2` view), position and total, copy's hide byte |
| 10 | `SEARCH` | flags byte (present, pending, invalid pattern), then current and total when present |
| 11 | `UNSEEN` | unseen output count |
| 12 | `INPUT_MODES` | byte: bit 0 kitty keyboard, bit 1 mouse tracking |
| 13 | `STATUS` | tag (`0` starting, `1` running, `2` exited, `3` failed); exit code and optional signal, or the failure text |
| 14 | `KITTY` | count, then fixed 72-byte placement records |

The fields byte of a typical echo is one byte (`ROWS | CURSOR_AT`). A patch with `KITTY` always
carries `SCROLLBAR`, so the placements' absolute rows can be checked when decoding; `CURSOR` and
`CURSOR_AT` never appear together. A full frame never carries the patch-only bits.

# Styles and graphemes

A style is a flags byte (bit 0 underline colour present, bits 1-3 underline kind, bit 4
attributes present, bit 5 colour classes present), the foreground and background as 3-byte
colours, then the optional underline colour, attribute bits and class word
(`PackedStyle::class_word`) as varints. About 7 bytes a style, against 20 before. Graphemes are
lengths and one UTF-8 arena; the decoder rebuilds the offset table.

# Rows and runs

A row record (full frame and history chunk) or span record (patch) is:

```
row step       varint: row - (previous row + 1) + 1, so 0 ends the section
head           varint: start column << 1 | clear
runs           run records, then 0
```

A patch span replaces columns `start..start + n` of its row, where `n` is the number of cells
its runs decode to; with `clear` the rest of the row becomes `PackedCell::EMPTY`. A full row never
sets `clear`. After a scroll, each newly exposed row must have a span that starts at column 0 and
clears or covers the whole row.

A run header is `count << 3 | style << 2 | kind`. With the style bit a style id varint follows;
otherwise the run keeps the previous run's style, and each row starts at style 0.

| Kind | Cells | Data |
|---|---|---|
| `0` text | `count` narrow cells | one UTF-8 scalar per cell, byte `0x00` for glyph 0 |
| `1` wide | `count` wide pairs: a `Wide` head and a `SpacerTail` of the same style | one UTF-8 scalar per pair |
| `2` repeat | `count` equal cells | one glyph code and flags |
| `3` raw | `count` cells | glyph code and flags per cell |

A glyph code is `index << 1 | 1` for a grapheme dictionary entry and `scalar << 1` otherwise.
The encoder starts a repeat run for 6 or more equal cells, so blank gaps and rules cost a few
bytes. Diffs are column spans (`TerminalViewport::diff_with_scratch`): the first to the last
changed column of each changed row, or up to the last non-empty cell with `clear` when the change
empties the row's tail.

# History chunks

Kind 3 carries `start`, `total`, `offset`, `columns`, the row count (at most
`MAX_HISTORY_CHUNK_ROWS`, 512), the styles and graphemes as in a full frame, then row records
with the row's index inside the chunk. It travels in the reliable queue like any event; only its
encoding moved from `postcard` to this lane. The decoder rebuilds `rows: Vec<Vec<PackedCell>>`
with every row exactly `columns` wide.

# Previews on the Control lane

Viewports inside Control-lane messages (`ChooserPreview::Screen`, `ChooserPreview::Client`,
`ChooserPreviewTile`, a `CommandOutput` event encoded with `postcard` directly) serialize through
`pane_frame::viewport_bytes` and `optional_viewport_bytes`: the full viewport body as one
`postcard` byte string. A chooser preview no longer costs about 3 bytes a cell.

# Validation and limits

Encoders and decoders validate the same things, so a frame the daemon writes always decodes:
style ids and grapheme indexes resolve, scalars are valid, runs stay inside their row, rows are
ascending and inside the grid, overlays and the cursor are inside the grid, scrollbar, mode and
search are consistent, the working directory has no control characters, the hovered URI no
whitespace or control characters, kitty placements are well formed. Declared counts are checked
against the bytes left before anything is allocated. Unknown field bits, a varint past 64 bits and
trailing bytes are errors.

| Limit | Value |
|---|---|
| title | 64 KiB |
| working directory, hovered URI | 16 KiB |
| status text | 1 MiB |
| styles | 65,536 |
| graphemes | 1 Mi entries, 16 MiB |
| overlays | 1 Mi |
| kitty placements | 65,536 (`MAX_KITTY_PLACEMENTS`, zz-terminal) |
| history rows per chunk | 512 |

# Delivery

Terminal frames share one ordered stream with the Control lane over a Unix socket, a named pipe
or an ssh-carried byte stream. Frames are never compressed.

Supersession lives in the daemon's outbound mailbox: one pending frame per pane, newest replacing
stale under backpressure (see [zz-daemon](/crates/zz-daemon.md)). A preview frame is encoded
before the mailbox checks it against the preview byte budget; the old layout could size a frame
without encoding it, a compact frame cannot, and it is cheap to encode.

Every attached client holds its own view of a terminal (`TerminalViewId(client.0)`), and the daemon
runs one diff stream per (pane, view) pair, so each client's frames are encoded for that client.
Sharing one encoded frame between clients needs the views to share frames first (W3-SHARDS, then
W4-DELIVER's per-(pane, base) encode).

Protocol v87 separates terminal delivery scope from foreground authority. `visible_terminals`
contains the attached client's focused window after zoom filtering and is the source for input,
history and PTY geometry. With `SetTerminalPreview { enabled: true }`, `streamed_terminals` also
holds every terminal pane of the attached session as `Preview`. Preview frames are bounded and
lower priority: their pending slots and bytes reserve room for the foreground panes, and queued
previews are retried as a latest full viewport after the writer makes progress. The daemon does not
send Kitty image payloads for preview panes.

Command output uses one coalesced slot. The mailbox keeps the actor ID beside the encoded frame,
refuses an older actor frame after a newer one is pending, and lets a reliable close discard
pending frames whose IDs are no newer than that close. An authoritative zero-ID empty resync
clears the slot.

Recovery is per pane. A client that cannot apply a patch, or gets one for a pane it has no base
for, sends `ProtocolMessage::RequestFull { pane }` and the daemon replies with that client's latest
full viewport. A patch's metadata applies on top of the retained frame, so a client that lost a
frame always resynchronizes through a full one rather than guessing.

# Scrollback backfill

The lane's viewport frames carry the visible grid. Scrollback above it comes in history chunks
(`HistoryRequest { pane, start, count }`), which the GUI keeps in a bounded ring beside the retained
viewport. Nothing on the wire tells a client its ring went stale; it infers invalidation while
applying a patch from what the patch says: a column change, a `scrollbar.total` that moved backwards
or by more than the row shift, an offset delta inconsistent with the shift, or a patch with a span in
every row drop the retained history. Rows leaving the top of the grid on a negative scroll are
pushed onto the back of the ring. A patch without `SCROLLBAR` keeps the retained scrollbar
(`TerminalViewportPatch::scrollbar_after`).

# Examples

```rust
let frame = encode_protocol_message(&ProtocolMessage::Event(Event {
    sequence: terminal.next_stream_sequence(),
    payload: EventPayload::TerminalViewport { pane, viewport },
}))?;
assert_eq!(frame[4], 1);

let mut buffer = Vec::new();
encode_terminal_patch_event_into(pane, sequence, &patch, &mut buffer)?;
```

# Related

- The router and its sibling Control lane: [wire protocol](/protocol/wire-protocol.md).
- Part of [the zz-protocol crate](/crates/zz-protocol.md).
- Payload types (`TerminalViewport`, `TerminalViewportPatch`, `TerminalPatchFields`,
  `PackedCell`, `PackedStyle`, dictionaries) live in [zz-terminal](/crates/zz-terminal.md); see
  [the terminal frame concept](/concepts/terminal-frame.md).
- Produced by [zz-daemon](/crates/zz-daemon.md)'s frame fanout, decoded by [the GPUI app](/crates/zz.md),
  the TUI and the zz-client core.
- Identifiers in the header: [stable IDs](/protocol/ids.md).
