---
type: Protocol
title: zz wire protocol (v52)
description: The versioned, little-endian length-prefixed, postcard-encoded control protocol whose ProtocolMessage enum carries the entire client/daemon conversation over a local socket or an ssh-forwarded one.
resource: crates/zz-protocol/src/framing.rs
tags: [protocol, wire, framing, postcard, versioning]
timestamp: 2026-08-13T00:00:00Z
---

# Overview

The zz wire protocol is a **versioned, framed, `serde`/`postcard`-encoded** control protocol spoken
over a Unix-domain socket (Linux/macOS) or a named pipe (Windows). A remote daemon is reached over
the same Unix socket, forwarded by `ssh -L`, so there is exactly one transport shape.
Every message is wrapped in a fixed envelope carrying a `u32` little-endian length prefix, a
one-byte **lane** tag, a **flags** byte, and a `u16` **protocol version**. The current wire version is
**`PROTOCOL_VERSION = 52`** (`crates/zz-protocol/src/message.rs`).

The version is a gate, not a negotiation: a frame whose envelope version differs from the running
build's is rejected outright. Before disconnecting, a daemon makes a best-effort
`CommandResponse::Error(ServerError::ProtocolMismatch)` reply encoded with its own version, so the
client can identify the stale side and offer a guarded restart. Anything that changes an
already-shipped encoding bumps it. The current bump is recorded above `PROTOCOL_VERSION` in
`message.rs`; the historical changes live in `knowledge/log.md`. This doc describes only the shape
speaking today. Framing lives in `crates/zz-protocol/src/framing.rs`, the message vocabulary in
`crates/zz-protocol/src/message.rs`.

There are two lanes sharing one envelope: **Control** (lane `0`, `postcard`-encoded
`ProtocolMessage`) and **Terminal** (lane `1`, hand-packed; see
[packed terminal lanes](/protocol/terminal-lanes.md)). Both lanes ride one ordered stream, local or
forwarded, and every frame is delivered reliably and in order. This concept documents the Control
lane and the shared framing.

# Framing

Every frame on the wire is:

```
┌───────────────┬──────┬───────┬─────────────┬───────────────────────────┐
│ length  u32LE │ lane │ flags │ version u16 │ payload                   │
│ 4 bytes       │ 1 B  │ 1 B   │ 2 bytes LE  │ length-4 bytes            │
└───────────────┴──────┴───────┴─────────────┴───────────────────────────┘
 offset 0..4      4      5       6..8          8..
```

- **length** = number of bytes that follow the prefix = `ENVELOPE_BYTES (4) + payload.len()`. It is
  validated against `MAX_FRAME_BYTES` (**64 MiB**) *before* any allocation.
- **lane** = `0` Control or `1` Terminal (`Lane` enum). Any other value → `UnsupportedLane`.
- **flags** = `0x00`, wholly reserved for future envelope extensions. Encoders always write zero and
  the decoder rejects every other value with `UnsupportedFlags`. v43 removed the one bit that ever
  meant anything (`0x01`, zstd), because only the deleted QUIC writer set it.
- **version** = `PROTOCOL_VERSION` as `u16` LE. A mismatch → `VersionMismatch { expected, received }`.
- **payload** = Control lane: `postcard::to_stdvec(&ProtocolMessage)`. Terminal lane: packed sections.
  Frames are never compressed.

Relevant constants (`framing.rs`): `MAX_FRAME_BYTES = 64 * 1024 * 1024`, `ENVELOPE_BYTES = 4`,
`LENGTH_PREFIX_BYTES = 4`, `FRAME_HEADER_BYTES = 8`, `MAX_ENCODED_FRAME_BYTES = 4 + MAX_FRAME_BYTES`.

# Schema . envelope fields

| Field | Offset | Type | Meaning |
|-------|--------|------|---------|
| length | 0..4 | `u32` LE | Bytes following the prefix (`4 + payload`) |
| lane | 4 | `u8` | `0` = Control, `1` = Terminal |
| flags | 5 | `u8` | `0x00` only; every other value is rejected |
| version | 6..8 | `u16` LE | `PROTOCOL_VERSION` (52) |
| payload | 8.. | bytes | `postcard(ProtocolMessage)` (Control) or packed terminal sections |

# Schema . `ProtocolMessage` (Control lane)

The top-level enum (`message.rs`). Postcard encodes the variant index as a varint tag, then the
fields in declaration order.

| Variant | Fields | Purpose |
|---------|--------|---------|
| `ClientHello(ClientHello)` | `protocol_version: u16`, `kind: ClientKind`, `device_name: Option<String>` (≤256 B), `capabilities: Vec<String>`, `color_scheme: Option<TerminalColorScheme>`, `origin: Option<PaneId>` | Client → daemon handshake. `device_name` labels this device in presence and eviction notices; `$ZZ_PANE` supplies `origin`, so an untargeted CLI command resolves against its invoking pane |
| `ServerHello(ServerHello)` | `protocol_version: u16`, `server_id: u64`, `client_id: ClientId`, `capabilities: Vec<String>` (≤64 entries, ≤256 B each), `appearance: TerminalAppearance`, `appearance_provenance: AppearanceProvenance`, `mux_options: MuxOptions`, `status: StatusLine`, `key_tables: Vec<KeyTableSnapshot>` | Daemon → client handshake reply; every key table (root, prefix, copy-mode, copy-mode-vi, custom) lets clients label key hints and render binding help, while capabilities describe optional behavior |
| `CommandRequest(CommandRequest)` | `request_id: u64`, `command: CommandInvocation` | tmux-style command from any client |
| `CommandResponse(CommandResponse)` | `Success { request_id, output }` / `Error { request_id, error: ServerError }` | Command result |
| `Attach { session: String }` | target string | Interactive attach request. A session holds a set of attached clients, so a second device never collides with the first |
| `Attached { session: SessionId, snapshot: MuxSnapshot }` | resolved id + full state | Attach acknowledgement |
| `Detach` | . | Interactive detach; drops the sending client's attachment and leaves every other viewer attached. The connection stays open, and the client remains a subscriber that can send another `Attach` |
| `SetColorScheme(TerminalColorScheme)` | light/dark | Client-driven appearance change |
| `SetConfigOverrides { entries }` | ordered `Vec<(String, String)>` | Replace the daemon's complete appearance and mux override sets; an empty vector clears both |
| `Input(InputMessage)` | see below | Keyboard / resize / view / split input |
| `Event(Event)` | `sequence: u64`, `payload: EventPayload` | Ordered daemon → client push |
| `GuiResponse(GuiResponse)` | `Success { request_id, output }` / `Error { request_id, message }` | Interactive client → daemon answer to an `AgentCommand` or `BrowserCommand::Screenshot` request; both strings bounded to `MAX_GUI_TEXT_BYTES` (64 KiB) |
| `Resync` | . | Client requests a fresh snapshot after a sequence gap |
| `RequestFull { pane: PaneId }` | `pane` | Client asks for one full replacement viewport for a single pane after a dropped or unusable terminal frame |
| `HistoryRequest { pane: PaneId, start: u32, count: u32 }` | scrollback window | Client asks for a chunk of a pane's scrollback; the daemon answers with `EventPayload::HistoryChunk` |
| `PasteUploadBegin { upload_id, pane, purpose, extension, total_bytes }` | one image announced | Interactive client → daemon: start streaming a pasted image. `purpose` is `PastePath` (write the file on the daemon host and paste that path) or `RecordPastedImage` (keep encoded bytes until the terminal prints a numbered placeholder). `extension` is 1..=8 lowercase ASCII alphanumerics, `total_bytes` is 1..=`MAX_PASTE_UPLOAD_BYTES` (6 MiB) |
| `PasteUploadChunk { upload_id: u64, bytes: Vec<u8> }` | one ordered slice | Body of the announced upload, at most `MAX_PASTE_UPLOAD_CHUNK_BYTES` (1 MiB) per message |
| `FetchPastedImage { pane, number }` | one placeholder | Interactive client → daemon: fetch the encoded image the terminal numbered `number` in `pane` |
| `PastedImageBegin { pane, number, format, total_bytes }` | one image announced | Daemon → client: start streaming that numbered image. `format` is `Png` / `Jpeg` / `Gif` / `Webp` |
| `PastedImageChunk { pane, number, bytes }` | one ordered slice | Body of the announced fetch, at most `MAX_PASTE_UPLOAD_CHUNK_BYTES` (1 MiB) per message |
| `PastedImageUnavailable { pane, number }` | one placeholder | Daemon → client: that numbered image is gone or could not be encoded |

`ClientKind`: `Interactive | Command`. `ClientMessageKind` (typed notifications introduced in v17):
`Info | Success | Warning | Error`.

## `InputMessage` variants

| Variant | Fields |
|---------|--------|
| `Text` | `pane: PaneId`, `text: String` |
| `Key` | `pane: PaneId`, `input: KeyInput`, `text_follows: bool` |
| `BrowserSurfaceText` | `pane: PaneId`, `text: String` |
| `BrowserSurfaceKey` | `pane: PaneId`, `input: KeyInput`, `text_follows: bool` |
| `ResizeTerminal` | `pane`, `columns: u16`, `rows: u16`, `cell_width_px: u32`, `cell_height_px: u32` |
| `TerminalView` | `pane`, `action: TerminalViewAction` |
| `ResizeCommandOutput` | `columns`, `rows`, `cell_width_px`, `cell_height_px` |
| `CommandOutputView` | `action: TerminalViewAction` |
| `ChooseTree` / `ChooseBuffer` / `DisplayPanes` | `action: …Action` |
| `CommandPrompt` | `action: CommandPromptAction::{Update, Submit, Close}` |
| `ResizeSplit` | `window: WindowId`, `split: SplitId`, `ratio_basis_points: u16` (fixed-point over `SPLIT_RATIO_BASIS = 10_000`) |

Every `Key`/`Text` resolves the per-client `KeyEngine` against the live key tables first; `Pass`
reaches the synchronized Terminal/Browser sinks (Picker and Agent source panes have no sink and
drop passed keys). `BrowserSurfaceKey`/`BrowserSurfaceText` validate a Browser source and go
straight to those sinks, so root bindings cannot consume page input. The desktop client's
window-root prefix claim sends the configured prefix and armed sequence as `Key`, preserving tmux
ownership only for that sequence. The daemon publishes `EventPayload::PrefixArmed` when it arms or
clears.

## `EventPayload` variants

`Snapshot(MuxSnapshot)`, `AgentCommand { pane, request_id, command }`,
`AppearanceChanged { appearance: Box<TerminalAppearance>, provenance: AppearanceProvenance }`,
`MuxOptionsChanged { options: MuxOptions }`, `StatusChanged { status: StatusLine }`,
`TerminalViewport { pane, viewport }`, `TerminalPatch { pane, patch }`,
`Clipboard { pane, request_id, target, text }`, `BrowserCommand { pane, command }`,
`TerminalUiCommand { pane, command }`, `CommandPrompt { state }`, `CommandOutput { pane, viewport }`,
`ChooseTree { state }`, `ChooseTreeUpdate { search, selected }`, `ChooseBuffer { state }`,
`ChooseBufferUpdate { search, selected }`, `DisplayPanes { state }`,
`ClientMessage { pane, kind: ClientMessageKind, text }`, `PaneRemoved(PaneId)`, `ServerStopping`,
`OpenUri { pane, uri }`, `FocusSidebar`, `PrefixArmed { armed }`, `Bell { pane }`,
`KeyTablesChanged { tables }`,
`Detached { session: SessionId, by: Option<String> }`, `HistoryChunk { pane, start: u32, total: u32,
offset: u32, columns: u16, rows: Vec<Vec<PackedCell>>, dictionary: TerminalDictionary }`,
`KittyImageBegin { pane, image_id, generation, width, height, total_bytes }`
(`total_bytes` ≤ `MAX_KITTY_IMAGE_BYTES`, 16 MiB),
`KittyImageChunk { pane, image_id, generation, bytes }`
(each chunk ≤ `MAX_KITTY_IMAGE_CHUNK_BYTES`, 1 MiB), and
`KittyImagesRemoved { pane, image_ids }`.

The three payloads `TerminalViewport`, `TerminalPatch`, and `CommandOutput { viewport: Some(..) }` are
diverted to the [Terminal lane](/protocol/terminal-lanes.md) by `encode_protocol_message`; all other
payloads ride the Control lane. `OpenUri`, `TerminalUiCommand`, `ClientMessage`,
`CommandPrompt`, `FocusSidebar`, and the choose-tree/buffer state updates deliberately use the
reliable Control lane. `HistoryChunk` does too: scrollback rows are `postcard`-encoded `PackedCell`
rows rather than a packed terminal frame, and a lost chunk would leave a hole in the client's ring.

## `ServerError`

`ProtocolMismatch { client, server }`, `MissingTarget(String)`, `AmbiguousTarget(String)`,
`InvalidTarget(String)`, `UnsupportedCommand(String)`, `InvalidCommand(String)`,
`PaneNotAttached(PaneId)`, `PaneExited(PaneId)`, `Internal(String)`.

# Attachment, presence, and per-client views

The daemon keys attachment as `attached: BTreeMap<SessionId, BTreeSet<ClientId>>`, so any number of
devices can hold one session at once and no error variant exists for a session that is already
attached. Each attached client owns its own terminal view (`TerminalViewId(client.0)`), key engine,
focused window, and overlay state. A pane's PTY takes its geometry from **one** owning viewer,
**latest active wins**: `terminal_geometry_owner` picks the visible viewer with the highest
daemon-global terminal-input sequence (ties broken by lowest `ClientId`), and columns, rows, and both
cell-pixel dimensions all come from that client. Typing in a pane reclaims its geometry. The
min-of-viewers rule this doc used to describe was reversed on 2026-07-31 because it produced
geometries no client actually had; see
[multi-device attach](/designs/multi-device-attach.md).

`attach-session -d` evicts the other viewers of the target session: each victim receives
`EventPayload::Detached { session, by: Some(device) }` naming the stealer's `device_name` (or
`device-<client id>` when the hello carried none). Session teardown sends the same payload with
`by: None`, which is how a killed session clears stale attachments. A command client cannot attach,
but `attach-session -d` from one still evicts the session's viewers.

Snapshots are stamped per subscriber before they go out: `MuxSnapshot.focused_window` carries the
receiving client's own focus and every `SessionSnapshot.viewers` entry sets `is_self` for that
client. See [snapshots](/protocol/snapshots.md) for the shape.

# Scrollback history

A client keeps a bounded ring of scrollback rows per pane and fills it with
`HistoryRequest { pane, start, count }`. The daemon answers with `EventPayload::HistoryChunk`
carrying the requested rows plus the pane's current `total`/`offset` scrollbar position and the style
and grapheme dictionary those rows reference. It serves at most `MAX_HISTORY_CHUNK_ROWS` (512) rows
per request, and only for panes that are visible to the requesting client; anything else is dropped
without a reply, and the client re-arms on the next snapshot.

The `history-trickle` mux option bounds how many rows a client backfills in the background: default
`2000`, maximum `10000`, and `0` disables background backfill while leaving scroll-driven prefetch
intact.

Nothing on the wire versions the scrollback, and there is no history epoch by design. The client
detects invalidation from what it observes in terminal frames, a shrinking `scrollbar.total` or a
column change, and drops its ring. See [packed terminal lanes](/protocol/terminal-lanes.md) for the
full rule.

# Paste upload

Pasting an image into a pane on a **remote** host cannot work through the pasteboard: the CLI in that
pane reads its own host's board, which on a headless machine is empty. So the client uploads the
bytes instead. It normalizes the clipboard image exactly as an Agent pane does (SVG refused, TIFF
transcoded, long edge capped at 1568 px, 5 MiB ceiling), then sends one `PasteUploadBegin` followed by
`PasteUploadChunk`s of at most 1 MiB, all under one writer lock so nothing interleaves.

`PasteUploadBegin.purpose` chooses what happens after the bytes land:

- `PastePath` writes `<runtime-dir>/paste/paste-<client>-<upload id>.<extension>` (directory `0700`,
  file `0600` on unix) next to the daemon socket, keeps only the newest 8 files there, and pastes
  that absolute path into the pane through the ordinary bracketed-paste path. Path-aware CLIs read
  the file from there.
- `RecordPastedImage` keeps the encoded bytes until the terminal prints a numbered placeholder
  (`[Image #N]`). A later `FetchPastedImage { pane, number }` asks the daemon to stream that image
  back as `PastedImageBegin` + `PastedImageChunk`s, or `PastedImageUnavailable` if it is gone.

There is no End message and no offsets: the stream is reliable and ordered, so the daemon appends
chunks until the accumulated length **equals** `total_bytes`, and that is what completes the upload.

Bounds hold on encode and on decode, and again in the daemon: the extension alphabet is what keeps
the name inside one path segment, uploads are capped at **2 concurrent per client** (a third is
refused), a `Begin` reusing a live `upload_id` replaces it, a chunk for an unknown id is ignored as
debris behind a dropped upload, and overflowing the declared total drops the upload. Every refusal
and every IO failure comes back as an `EventPayload::ClientMessage` error toast to the uploading
client; a disconnect drops that client's in-flight uploads. A local pane that only needs a file path
still reads the local pasteboard; the upload path is the remote `PastePath` case and the
record/fetch round-trip for numbered placeholders.

# Remote transport

A remote daemon speaks the identical envelope over the identical Unix socket; the only difference is
that `ssh -N -L` forwards it. `Endpoint::parse` accepts `unix://`, a bare path, and
`ssh://[user@]host[:port][/remote/socket]`, and nothing else . a `quic://` string is rejected with a
pointer at `ssh://`. There is no second stream shape, no compression, no unidirectional supersession,
and no client-opened tunnel: every frame is reliable and ordered, exactly as it is locally, which is
what makes the protocol location-transparent.

The old QUIC transport was deleted on 2026-08-01 along with the per-frame unidirectional streams,
the negotiated zstd envelope flag, and the mux `browser-egress` option / QUIC splice. Remote browser
egress itself is current: a client-local `browser-egress` key in `zz/config` points CEF at
`ssh -D` (`socks5-direct`). See [remote browser egress](/designs/remote-browser-egress.md).
`RequestFull { pane }` outlives QUIC: a client that cannot apply a patch still repairs one pane at a
time, and the daemon's frame supersession now happens entirely in the outbound mailbox (see
[zz-daemon](/crates/zz-daemon.md)).

When a forward dies, the client reconnects with a backoff of 1, 2, 4, 8, 16, then 30 seconds, and
re-attaches the same session. Every reconnect timer is guarded by a connection generation counter, so
a fast reconnect cannot be undone by a stale timer firing later. The daemon sees an ordinary
disconnect followed by an ordinary attach.

# Appearance and mux overrides with provenance

An interactive client sends `SetConfigOverrides` after a hello advertising
`config-overrides-v1`, then sends the complete replacement set after every `zz/config` poll
change. Entries retain file order and repeated keys. The client does not parse appearance values;
the daemon feeds them through its Ghostty-compatible loader. Sending zero entries removes every client
override and restores the pure Ghostty/tmux/default derivation. The mux keys are likewise unparsed by
the client; the daemon dispatches them through its existing global
`set-option` arms and reapplies the stored subset after each tmux config replay.

`AppearanceProvenance` is a complete map over the 30 values in `AppearanceConfigKey::ALL` (from
`theme` and `background` through the `zz-` prefixed extensions). Each value carries one
`AppearanceSource` of four: `Default`, `ThemeFile`, `Ghostty`, or `Override`. `theme` is a live key
resolved *before* the rest of a set . the selected theme file becomes the base the remaining entries
paint over . and any key that file supplied reports `ThemeFile`. `Palette` is one provenance key
rather than 256 entries. Both `ServerHello.appearance_provenance` and `AppearanceChanged.provenance`
carry the map, so the settings UI can explain the current value without inferring it from colors.
Missing keys are rejected during control-message validation.

`MuxOptions` is a `BTreeMap<MuxOptionKey, MuxOptionValue>` over the 11 keys of `MuxOptionKey::ALL`, in
declaration order: `prefix`, `mode-keys`, `history-limit`, `word-separators`, `copy-command`,
`set-clipboard`, `buffer-limit`, `synchronize-panes`, `experimental-agent-pane`,
`experimental-editor-pane`, and `history-trickle` (default `2000`). `postcard` encodes the key as its
variant index, so a new key is only ever appended: `history-trickle` is `10`, and it is now the last
one . v43 removed `predict` (11) and `browser-egress` (12) with the client predictor and the egress
tunnel they configured. Every entry contains an effective display string plus `MuxOptionSource`:
`Default`, `TmuxConfig`, `Override`, or `RuntimeCommand`. `ServerHello.mux_options` supplies initial
state and `MuxOptionsChanged` replaces it whenever a successful writer changes a value or source.
Validation requires exactly those 11 keys and bounds every string to 64 KiB on encode and during
deserialization.

`StatusLine { left, right }` is the daemon-rendered [tmux status line](/tmux/status-line.md): finished
text, never formats, because `#()` commands run once per `status-interval` on the daemon's host. It is
**per client** (a format names the receiving client's own view), so it rides `ServerHello` on connect
and `publish_to_client` afterwards, and only when the text changed. Both halves are bounded
to `MAX_STATUS_TEXT_BYTES` (1 KiB) on encode and during deserialization, which keeps an
unbounded `#()` script off the wire.

# Versioning & compatibility

- **`PROTOCOL_VERSION: u16 = 52`** is stamped into every frame's envelope and re-checked inside
  `ServerHello` (`validate_control_message` rejects an inner-version mismatch even if the envelope
  version passed).
- **Any change that affects an already shipped encoding** (new enum variants, reordered fields,
  changed integer widths) **requires bumping the version**. The envelope's `VersionMismatch` guard
  then forces the peers to agree. A rejected first frame gets a best-effort mismatch response before
  disconnect; clients also classify the received envelope version or the daemon's versioned identity
  file, so an older local daemon produces a legible restart path rather than a bare EOF.
- **Capability strings are now descriptive, all of them.** The daemon advertises a fixed list in
  `ServerHello.capabilities` (`mux-v1`, `terminal-viewport-v3`, `terminal-row-patches`,
  `terminal-appearance-v2`, `config-overrides-v1`, `browser-panes`, `tmux-config-subset`,
  `new-session-attach-v1`, and the `native-*` command surfaces) that names features, not encodings;
  the numeric version alone carries a break. The two strings that were real switches,
  `terminal-zstd-v1` and `egress-v1`, went with QUIC at v43, so nothing in the handshake changes how
  a frame is encoded any more.
- The **flags byte is entirely reserved** and every nonzero value is rejected, so no silent
  capability drift is possible.
- Bounded decoders reject oversized input: `ClientHello.capabilities` uses a visitor that refuses
  each entry in `visit_str` before `to_owned`. `device_name`, status-line halves, and several other
  strings deserialize first and then reject on `len() > limit`. Frame lengths are validated against
  `MAX_FRAME_BYTES` before the payload buffer grows.
  `SetConfigOverrides` is additionally validated at no more than 1,024 single-line pairs, with
  nonempty keys of at most 128 bytes and values of at most 64 KiB. Each mux-option payload must be a
  complete 11-key map with values of at most 64 KiB; bounded value deserialization rejects an
  oversized string before it can become protocol state.
- The GUI-request path is bounded twice, on encode and on decode: `AgentCommand` text to
  `MAX_AGENT_SEND_BYTES` (1 MiB), the screenshot path and every `GuiResponse` string to
  `MAX_GUI_TEXT_BYTES` (64 KiB), each with a bounded deserializer *and* a `validate_control_message`
  check. Rejections surface as `ProtocolError::InvalidGuiRequest`.
- Paste uploads are bounded the same way, on encode and on decode: `total_bytes` to
  `MAX_PASTE_UPLOAD_BYTES` (6 MiB) and nonzero, each chunk to `MAX_PASTE_UPLOAD_CHUNK_BYTES` (1 MiB),
  and the extension to 1..=8 lowercase ASCII alphanumerics . an alphabet, not a length, because the
  daemon interpolates it into a file name. Rejections surface as
  `ProtocolError::InvalidPasteUpload`.

# Examples

## Control-frame layout (a `ClientHello`)

`ClientHello { protocol_version: 52, kind: Interactive, device_name: None, capabilities: [],
color_scheme: Some(Dark), origin: None }` is 16 bytes on the wire: an 8-byte envelope over an
8-byte postcard payload.

```text
byte  0  1  2  3 | 4    | 5     | 6  7        | 8  9 10 11 12 13 14 15
      0c 00 00 00  00     00      34 00         00 34 00 00 00 01 01 00
      └ u32 LE ─┘  lane   flags   version LE    postcard payload
      length = 12  Control        (= 52)
```

- **length `12`** = `ENVELOPE_BYTES` (4) + payload (8); it counts the four envelope bytes, not itself.
- **payload** `00 34 00 00 00 01 01 00`: variant `0` (`ProtocolMessage::ClientHello`),
  `protocol_version` as the varint `0x34` (= 52), `kind` variant `0` (`Interactive`), `device_name`
  as the `Option::None` tag `00`, `capabilities` as the sequence length `00`, `Option::Some` tag
  `01`, `TerminalColorScheme` variant `1` (`Dark`), then `origin` as `Option::None` (`00`). Postcard
  writes multi-byte integers as LEB128 varints, so the version is one byte here and two in the
  envelope, which is fixed-width LE. A real interactive hello carries the device's short hostname
  and still no capabilities.

The round-trip test asserts `&frame[6..8] == PROTOCOL_VERSION.to_le_bytes()` and that lane byte 4 is
`Lane::Control`.

## Public API (`terminal_codec.rs`)

```rust
pub fn encode_protocol_message(message: &ProtocolMessage) -> Result<Vec<u8>, ProtocolError>;
pub fn decode_protocol_frame(frame: &[u8]) -> Result<ProtocolMessage, ProtocolError>;
pub fn write_protocol_message(w: &mut impl Write, m: &ProtocolMessage) -> Result<(), ProtocolError>;
pub fn read_protocol_message(r: &mut impl Read) -> Result<ProtocolMessage, ProtocolError>;
```

Plus the buffer-reusing `_into` variants. `framing.rs` keeps the envelope itself private and exports
only `MAX_FRAME_BYTES`, `MAX_ENCODED_FRAME_BYTES`, and `ProtocolError`.

`ProtocolError` variants: `Truncated`, `FrameTooLarge(usize)`, `LengthMismatch`,
`UnsupportedLane(u8)`, `UnsupportedFlags(u8)`,
`VersionMismatch { expected, received }`, `Encode/Decode(postcard::Error)`, `InvalidTerminal`,
`InvalidAppearance`, `InvalidServerHello`, `InvalidClientHello`, `InvalidConfigOverrides`,
`InvalidGuiRequest`, `InvalidPasteUpload`, `Io`.

## Handshake sketch

```text
client → ClientHello { protocol_version: 52, kind: Interactive, device_name: Some("laptop"),
                       capabilities: [], color_scheme: Some(Dark), origin: None }
server → ServerHello { protocol_version: 52, server_id, client_id: c11,
                       capabilities: ["mux-v1", "terminal-viewport-v3", "terminal-row-patches",
                                      "terminal-appearance-v2", "config-overrides-v1", ...,
                                      "new-session-attach-v1"],
                       appearance, appearance_provenance, mux_options, status, prefix_bindings }
client → SetConfigOverrides { entries: [("theme", "Catppuccin Mocha"),
                                        ("font-size", "13"),
                                        ("prefix", "C-a"), ("mode-keys", "vi"), ...] }
client → Attach { session: "$0" }
server → Attached { session: $0, snapshot: MuxSnapshot { generation, sessions, focused_window } }
server → Event { sequence: 1, payload: TerminalViewport { pane: %3, viewport } }   // Terminal lane
server → Event { sequence: 2, payload: MuxOptionsChanged { options } }             // Control lane
client → HistoryRequest { pane: %3, start: 1_488, count: 512 }
server → Event { sequence: 3, payload: HistoryChunk { pane: %3, start, total, offset, ... } }
...
(unusable patch)      client → RequestFull { pane: %3 }
(sequence gap)        client → Resync
(another device runs `attach-session -d`)
server → Event { sequence: n, payload: Detached { session: $0, by: Some("desktop") } }
```

`theme` resolves first and seeds the appearance, then `font-size` and the rest of the appearance
entries apply in file order; `prefix` and `mode-keys` are dispatched through the daemon's global
`set-option` arms.

# Related

- Part of [the zz-protocol crate](/crates/zz-protocol.md).
- The compact fanout path: [packed terminal lanes](/protocol/terminal-lanes.md).
- Identifiers carried in messages: [stable IDs](/protocol/ids.md).
- State tree carried by `Attached`/`Snapshot`: [snapshots](/protocol/snapshots.md).
- Produced/consumed by [zz-daemon](/crates/zz-daemon.md) and [the GPUI app](/crates/zz.md); commands
  resolve through [the mux state machine](/crates/zz-mux.md).
