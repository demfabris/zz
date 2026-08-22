---
type: Protocol
title: zz wire protocol (v71)
description: The versioned, little-endian length-prefixed, postcard-encoded control protocol whose ProtocolMessage enum carries the entire client/daemon conversation over a local socket or an ssh-forwarded one.
resource: crates/zz-protocol/src/framing.rs
tags: [protocol, wire, framing, postcard, versioning]
timestamp: 2026-08-21T12:00:00-03:00
---

# Overview

The zz wire protocol is a **versioned, framed, `serde`/`postcard`-encoded** control protocol spoken
over a Unix-domain socket (Linux/macOS) or a named pipe (Windows). A remote daemon is reached over
the same Unix socket, forwarded by `ssh -L`, so there is exactly one transport shape.
Every message is wrapped in a fixed envelope carrying a `u32` little-endian length prefix, a
one-byte **lane** tag, a **flags** byte, and a `u16` **protocol version**. The current wire version is
**`PROTOCOL_VERSION = 71`** (`crates/zz-protocol/src/message.rs`).

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
| version | 6..8 | `u16` LE | `PROTOCOL_VERSION` (71) |
| payload | 8.. | bytes | `postcard(ProtocolMessage)` (Control) or packed terminal sections |

# Schema . `ProtocolMessage` (Control lane)

The top-level enum (`message.rs`). Postcard encodes the variant index as a varint tag, then the
fields in declaration order.

| Variant | Fields | Purpose |
|---------|--------|---------|
| `ClientHello(ClientHello)` | `protocol_version: u16`, `client_instance_id: ClientInstanceId`, `kind: ClientKind`, `device_name: Option<String>` (≤256 B), `capabilities: Vec<String>`, `color_scheme: Option<TerminalColorScheme>`, `origin: Option<PaneId>` | Client → daemon handshake. The process-stable instance ID owns recoverable Agent drafts across reconnects; `device_name` labels this device in presence and eviction notices; `$ZZ_PANE` supplies `origin`, so an untargeted CLI command resolves against its invoking pane |
| `ServerHello(ServerHello)` | `protocol_version: u16`, `server_id: u64`, `client_id: ClientId`, `client_instance_id: ClientInstanceId`, `capabilities: Vec<String>` (≤64 entries, ≤256 B each), `appearance: TerminalAppearance`, `appearance_provenance: AppearanceProvenance`, `mux_options: MuxOptions`, `status: StatusLine`, `key_tables: Vec<KeyTableSnapshot>` | Daemon → client handshake reply; echoes the accepted process identity, while every key table (root, prefix, copy-mode, copy-mode-vi, custom) lets clients label key hints and render binding help and capabilities describe optional behavior |
| `CommandRequest(CommandRequest)` | `request_id: u64`, `command: CommandInvocation` | tmux-style command from any client |
| `CommandResponse(CommandResponse)` | `Success { request_id, output, exit_code, stderr }` / `Error { request_id, error: ServerError, output }` | Command result. A client prints either output field before it reports an error or returns the exit code. `stderr` (appended at v71) is populated for `source-file` diagnostics issued by a Command client since Wave E (2026-08-22) and stays empty for every other command; `Success` with a nonzero `exit_code` is a COMPLETED command, so `Error` stays reserved for dispatch, transport, and server failures |
| `Attach { session: String }` | target string | Interactive attach request. An empty target lazily creates the next numeric session when the daemon has none; explicit missing targets and Command-kind attaches do not create. A session holds a set of attached clients, so a second device never collides with the first |
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
| `AgentPrompt { pane, text, images: Vec<AgentImage> }` | one prompt | Client → daemon: submit an ACP prompt to the pane's daemon-owned adapter. `AgentImage { format, data }` carries encoded bytes the daemon turns into content blocks; text plus every image totals at most `MAX_AGENT_PROMPT_BYTES` (6 MiB), each `format` at most 64 bytes |
| `AgentCancel { pane }` | . | Cancel the pane's running turn |
| `AgentUnqueue { pane }` | . | Reclaim the pane's queued prompts; the daemon returns them inside the stream so the composer refills |
| `AgentRespondPermission { pane, request_id, option_id: Option<String> }` | one answer | Answer a parked permission request; `None` cancels. First answer wins and a late one is a no-op |
| `AgentSetConfigOption { pane, option_id, value }` | one setting | Write one adapter config option |
| `AgentSetMode { pane, mode_id }` | one mode | Switch the session mode |
| `AgentAuthenticate { pane, method_id }` | one method | Run one advertised authentication method |
| `AgentSessionOp { pane, op }` | `List { cwd, cursor, replace }` / `New { cwd }` / `Switch { session_id, cwd, additional_directories }` / `Delete { session_id }` | Session management against the pane's adapter; `List` answers with `EventPayload::AgentSessions` |
| `AgentReplay { pane, from_seq: u64 }` | journal cursor | Replay the pane's journal from `from_seq`, then tail it. Sent on attach, after `AgentLagged`, and when a pane enters the visible set |
| `AgentAcknowledgePromptRestore { pane, reclaim_id }` | one restored draft | Retire one daemon-cached recovered prompt after its owning client has put it back in the composer, preventing a later replay from restoring it again |

The agent identifiers (`option_id`, `value`, `mode_id`, `method_id`) are bounded to
`MAX_AGENT_OPTION_BYTES` (4 KiB), and session IDs and list cursors to
`MAX_AGENT_SESSION_ID_BYTES` (16 KiB), on encode and during deserialization. Session directories
must be nonempty and fit `MAX_GUI_TEXT_BYTES`; a switch carries at most
`MAX_AGENT_SESSION_DIRECTORIES` (256) additional roots. The wire treats path syntax as opaque so a
Windows client can carry a Unix daemon path and vice versa; the receiving daemon enforces local
absoluteness before the adapter sees it. Rejections surface as `ProtocolError::InvalidAgentPayload`.

`ClientKind`: `Interactive | Command | Control`. `Control` (appended at v65) is a control-mode
(`-C`/`-CC`) client: it subscribes to the session like an Interactive client but renders the
tmux CC text protocol on its own stdio instead of a UI. `ClientMessageKind` (typed notifications
introduced in v17): `Info | Success | Warning | Error`.

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
| `CancelPrefix` | `request_id: u64`; retire an armed one-shot prefix table without forwarding a key to the pane |
| `Popup` | `action: PopupAction::{Text(String), Key { input, text_follows }, TerminalView(TerminalViewAction), Close}`; input and view control for the client's open `display-popup` |
| `Menu` | `action: MenuAction::{Choose(u32), Cancel}`; drives the client's open `display-menu` |
| `Confirm` | `action: ConfirmAction::Reply(bool)`; answers the client's open `confirm-before` prompt |

Every `Key`/`Text` resolves the per-client `KeyEngine` against the live key tables first; `Pass`
reaches the synchronized Terminal/Browser sinks (Picker and Agent source panes have no sink and
drop passed keys). `BrowserSurfaceKey`/`BrowserSurfaceText` validate a Browser source and go
straight to those sinks, so root bindings cannot consume page input. The desktop client's
window-root prefix claim sends the configured prefix and armed sequence as `Key`, preserving tmux
ownership only for that sequence. A desktop dialog sends `CancelPrefix { request_id }` before it
takes input. The daemon clears only the one-shot prefix table and answers with the matching
`PrefixCancelled { request_id }`. The desktop keeps ordinary workspace keys behind that barrier;
platform and function shortcuts remain available.

## `EventPayload` variants

`Snapshot(MuxSnapshot)`, `AgentCommand { pane, request_id, command }`,
`AppearanceChanged { appearance: Box<TerminalAppearance>, provenance: AppearanceProvenance }`,
`MuxOptionsChanged { options: MuxOptions }`, `StatusChanged { status: StatusLine }`,
`TerminalViewport { pane, viewport }`, `TerminalPatch { pane, patch }`,
`Clipboard { pane, request_id, target, text }`, `BrowserCommand { pane, command }`,
`TerminalUiCommand { pane, command }`, `CommandPrompt { state }`, `CommandOutput { pane, viewport }`,
`ChooseTree { state }`, `ChooseTreeUpdate { search, selected }`, `ChooseBuffer { state }`,
`ChooseBufferUpdate { search, selected }`, `DisplayPanes { state }`,
`ClientMessage { pane, kind: ClientMessageKind, text }`,
`PaneRemoved(PaneId)`, `ServerStopping`,
`OpenUri { pane, uri }`, `FocusSidebar`, `PrefixArmed { armed }`,
`PrefixCancelled { request_id }`, `Bell { pane }`,
`KeyTablesChanged { tables }`,
`Detached { session: SessionId, by: Option<String>, reason: DetachReason }`, `HistoryChunk { pane, start: u32, total: u32,
offset: u32, columns: u16, rows: Vec<Vec<PackedCell>>, dictionary: TerminalDictionary }`,
`KittyImageBegin { pane, image_id, generation, width, height, total_bytes }`
(`total_bytes` ≤ `MAX_KITTY_IMAGE_BYTES`, 16 MiB),
`KittyImageChunk { pane, image_id, generation, bytes }`
(each chunk ≤ `MAX_KITTY_IMAGE_CHUNK_BYTES`, 1 MiB), and
`KittyImagesRemoved { pane, image_ids }`.

The agent lane added at v53 carries four more: `AgentUpdates { pane, first_seq: u64, items: Vec<Vec<u8>> }`
carries one coalesced batch of JSON agent stream items numbered by the pane's fanout lane
(`first_seq` names the first item, the rest follow one by one; a batch is nonempty and totals at most
`MAX_AGENT_UPDATES_BYTES`, 9 MiB, so a longer window splits across frames).
`AgentState { pane, state: AgentPaneWire }` is the small typed pane state published to every client
attached to the session: `phase` (`Starting | Ready | Running | AwaitingPermission | Failed
{ message }`), `queued_prompts: u32`, `session_id`, `title`, the adapter's auth methods, config
options, and modes as one JSON `String` blob each (≤ `MAX_AGENT_STATE_BLOB_BYTES`, 256 KiB, because
postcard cannot carry the ACP SDK's JSON-shaped types),
`pending_permission: Option<AgentPermissionWire { request_id, payload }>` whose payload is bounded to
`MAX_AGENT_PERMISSION_BYTES` (64 KiB), and
`git: Option<AgentGitSummary { branch, changed_files, additions, deletions }>`; a branch is bounded
to `MAX_AGENT_OPTION_BYTES` (4 KiB). `AgentLagged { pane, next_seq }` says the client's agent lane
overflowed and was cleared, which the client answers with `AgentReplay` rather than dying.
`AgentSessions { pane, request_id, result }` is the JSON reply to `AgentSessionOp::List`, bounded by
`MAX_AGENT_RESULT_BYTES` (1 MiB) and sent only to the client that made the request. A session
listing uses request ID zero; the daemon carries its requester out of band.

v59 appends `TimedClientMessage { pane, kind: ClientMessageKind, text, duration_ms: u32 }` after
the agent payloads. v60 appends `PrefixCancelled { request_id }`; older enum tags keep their wire
values.

The overlay and control-mode waves append ten more, in tag order: `Popup { state: Option<PopupState> }`,
`Menu { state: Option<MenuState> }`, and `Confirm { state: Option<ConfirmState> }` (v63/v64) carry the
full overlay state on every change, `None` meaning closed; `ControlExit { reason }` (v66) tells a
control-mode client why the daemon is ending the conversation (its front-end renders `%exit <reason>`);
`HookEvent { name, variables }` (v66) is the hook-bus notification feed — the daemon sends the event
name plus its format variables and the control front-end alone knows the `%`-line spellings;
`PaneOutput { pane, bytes }` (v66) is the raw pane-output tap (the same tap `pipe-pane` uses) that
becomes `%output`; `PaneOutputState { pane, paused }` and `PaneOutputAged { pane, age_ms, bytes }`
(v67) carry flow-control pause/resume and age-stamped output for `%extended-output`;
`ControlFlags { wait_exit, pause_after_ms, no_output }` (v67) echoes the client's
`refresh-client -f` flags; and `SubscriptionChanged { name, session, window, window_index, pane, value }`
(v68) reports a `refresh-client -B` format subscription's value change. v71 appends
`TimedClientMessageCleared { message_id }` at tag 46 — the daemon's explicit clear for one
timed message, produced since Wave D3 by the `zz-client-message` deadline dispatcher when a
`display-message` timer expires and by the input path when a key dismisses the message.
Surfaces must match the identity before dropping anything, so a retired message's clear can
never take down the message that replaced it.

The three payloads `TerminalViewport`, `TerminalPatch`, and `CommandOutput { viewport: Some(..) }` are
diverted to the [Terminal lane](/protocol/terminal-lanes.md) by `encode_protocol_message`; all other
payloads ride the Control lane. `OpenUri`, `TerminalUiCommand`, `ClientMessage`, `TimedClientMessage`,
`CommandPrompt`, `FocusSidebar`, `PrefixCancelled`, and the choose-tree/buffer state updates use the
reliable Control lane. `HistoryChunk` does too: scrollback rows are `postcard`-encoded `PackedCell`
rows rather than a packed terminal frame, and a lost chunk would leave a hole in the client's ring.

## `ServerError`

`ProtocolMismatch { client, server }`, `MissingTarget(String)`, `AmbiguousTarget(String)`,
`InvalidTarget(String)`, `UnsupportedCommand(String)`, `InvalidCommand(String)`,
`PaneNotAttached(PaneId)`, `PaneExited(PaneId)`, `Internal(String)`,
`SessionNotFound(String)`, `WindowNotFound(String)`, `PaneNotFound(String)`. The last three carry
tmux target-lookup wording and only the normalized component that failed.

# Attachment, presence, and per-client views

The daemon keys attachment as `attached: BTreeMap<SessionId, BTreeSet<ClientId>>`, so any number of
devices can hold one session at once and no error variant exists for a session that is already
attached. Registration itself does not attach or create a session. An Interactive empty-target
attach to an empty daemon creates and attaches the next numeric session atomically from the caller's
perspective; the first one is `0` with ids `$0`, `@0`, and `%0`. Each attached client owns its own
terminal view (`TerminalViewId(client.0)`), key engine, focused window, and overlay state.

With `aggressive-resize` off, a pane's PTY takes its geometry from **one** owning viewer, **latest
active wins**: `terminal_geometry_owner` picks the visible viewer with the highest daemon-global
terminal-input sequence (ties broken by lowest `ClientId`), and columns, rows, and both cell-pixel
dimensions all come from that client. Typing in a pane reclaims its geometry. With the option on,
columns and rows become the componentwise minima across viewers focused on that window; cell-pixel
dimensions still come from the latest-input eligible viewer. Both policies feed the existing
guarded window-extent write-back. See [multi-device attach](/designs/multi-device-attach.md).

`attach-session -d` evicts the other viewers of the target session: each victim receives
`EventPayload::Detached { session, by: Some(device), reason: Evicted }` naming the stealer's
`device_name` (or `device-<client id>` when the hello carried none). Session teardown sends
`reason: SessionDestroyed` with `by: None`, while a client's own detach uses `Requested` and server
shutdown uses `ServerStopping`. A command client cannot attach,
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

`MuxOptions` is a `BTreeMap<MuxOptionKey, MuxOptionValue>` over the 17 keys of `MuxOptionKey::ALL`, in
declaration order: `prefix`, `mode-keys`, `history-limit`, `word-separators`, `copy-command`,
`set-clipboard`, `buffer-limit`, `synchronize-panes`, `experimental-agent-pane`,
`experimental-editor-pane`, `history-trickle` (default `2000`), `agent-command`,
`agent-claude-code-command`, `agent-auto-approve`, and the v71 tail `mouse` (default `on`),
`escape-time` (default `10`), and `prefix2` (default `None`). `postcard` encodes the key as its variant
index, so a new key is appended. Every entry contains an effective display string plus `MuxOptionSource`:
`Default`, `TmuxConfig`, `Override`, or `RuntimeCommand`. `ServerHello.mux_options` supplies initial
state and `MuxOptionsChanged` replaces it whenever a successful writer changes a value or source.
Since v71 the replacement map is **per recipient** for session-effective values: `mouse` carries the
receiving client's attached session's effective value, so the daemon publishes the effective map after
attach and client switch, recomputes every attached client on a global mouse write, and refreshes only
the clients attached to a target session on a session-scoped write. Each client's map is
equality-deduplicated like the status line: a recomputation whose result matches what that client
already holds sends nothing. `escape-time` and `prefix2` publish the global values. Since the
2026-08-21 B2/B3 slice zz-tui consumes `mouse` (outer-terminal mouse-mode gating) and `escape-time`
(the escape fold timeout); since Wave C run 2 the same day, `prefix2` feeds the shared key tables
(either prefix arms) and the GPUI client's local prefix claim. `from_config_key` maps all three —
`zz/config` can write them with the standard reload-reapply semantics.
Validation requires exactly those 17 keys and bounds every string to 64 KiB on encode and during
deserialization.

`StatusLine` is the daemon-rendered [tmux status line](/tmux/status-line.md): finished
text, never formats, because `#()` commands run once per `status-interval` on the daemon's host. It is
**per client** (a format names the receiving client's own view), so it rides `ServerHello` on connect
and `publish_to_client` afterwards, and only when the text changed. v70 carried `{ left, right }`;
v71 appends `title`, `base_style`, `rows: Vec<String>`, `position: StatusPosition` (`Top`/`Bottom`,
default `Bottom` on wire tag 1), `message_line: u8`, and `customized: bool`. Since Wave B1 `rows`
is the authoritative personalized status block, and since the 2026-08-21 title slice `title` carries
the per-client `set-titles-string` expansion whenever `set-titles` is on for the recipient's session
— published even while `status off` empties the rows; it stays empty at defaults because
`set-titles` defaults off. zz-tui writes OSC 2 when a non-empty title changes and the GUI adopts the
window title only for a non-empty (hence explicitly enabled) value. Every string field and each row is bounded to
`MAX_STATUS_TEXT_BYTES` (4 KiB) on encode and during deserialization, rows are capped at
`MAX_STATUS_ROWS` (5) with a sixth rejected before allocation, `base_style` must parse as a style,
and `message_line` must be `0` with no rows or under `rows.len()` otherwise; `StatusChanged` payloads
now validate on both encode and decode.

# Versioning & compatibility

- **`PROTOCOL_VERSION: u16 = 71`** is stamped into every frame's envelope and re-checked inside
  `ServerHello` (`validate_control_message` rejects an inner-version mismatch even if the envelope
  version passed).
- v71 is the tmux-campaign append bundle: `MuxOptionKey::{Mouse, EscapeTime, Prefix2}` (tags 14-16)
  with per-recipient session-effective `mouse` publication; the personalized `StatusLine` tail
  (`title`, `base_style`, bounded `rows`, `position`, `message_line`, `customized`);
  `PaneIndicator.label` (≤1 KiB, `Copy` dropped); `PaneSnapshot.{border_colour,
  active_border_colour}: Option<TmuxColour>` with validated colour serialization (`Rgb` over
  `0xFFFFFF` rejected on decode); `CommandPromptState.{prompt_type, mode, no_freeze}`;
  `TerminalMode::Copy.hide_position` (one canonical bool byte on the terminal lane; `View` stays
  17 bytes) and `TerminalViewAction::EnterCopyModeWith` (tag 27); `key` on `ChooseTreeItem` and
  `ChooseBufferItem` (≤64 B); `CommandResponse::Success.stderr`;
  `TimedClientMessage.message_id` plus `EventPayload::TimedClientMessageCleared` (tag 46); and
  `InputMessage::ClientTerminalSize` (tag 17) beside the new `client-tty-v1:`/`client-size-v1:`
  hello value tokens. Everything ships with inert daemon defaults; consumption lands in Waves B-E.
  Since 2026-08-21 (B5) terminal-surface clients emit the value tokens — `client-size-v1:` whenever
  a caller terminal size is discoverable, `client-tty-v1:` only when `$TMUX` marks a nested run and
  the endpoint is local — and republish `SIGWINCH` changes through `ClientTerminalSize`. The daemon
  uses the tty for the pinned nested-attach refusal, and the size for `-x -`/`-y -` creation
  dimensions, `#{client_width}`/`#{client_height}`, and scoping the mouse-off input rejection to
  terminal surfaces.
- v70 appends `reason: DetachReason` to `EventPayload::Detached`. The TUI distinguishes requested
  or evicted detaches, destroyed sessions, and server shutdown without adding a second retarget
  message; live client switches still converge through `ProtocolMessage::Attached`.
- v69 appends `status_label: String` to `WindowSnapshot` — the daemon-expanded
  `window-status-format` / `window-status-current-format` product for that window
  (`#[…]` style markers included, ≤1024 B) — and raises `MAX_STATUS_TEXT_BYTES` from
  1 KiB to 4 KiB so status halves can carry style-marker syntax.
- v68 appends `EventPayload::SubscriptionChanged`, closing the control-mode surface:
  `refresh-client -B` subscriptions report format-value changes, and sized (`-C`) control clients
  participate in window sizing like any attached client.
- v67 appends `EventPayload::PaneOutputState`, `PaneOutputAged`, and `ControlFlags` — `%output`
  flow control: pause/continue per pane, age-stamped `%extended-output`, and the
  `refresh-client -f` flag echo.
- v66 appends `EventPayload::ControlExit`, `HookEvent`, and `PaneOutput`. Hook-bus notifications
  and the raw pane-output tap reach the wire; the control-mode front-end renders every `%`-line
  from these, so the daemon never learns CC text shapes.
- v65 appends `ClientKind::Control` for `-C`/`-CC` clients.
- v64 appends `InputMessage::Menu`/`Confirm` and `EventPayload::Menu`/`Confirm` for
  `display-menu` and `confirm-before`.
- v63 appends `InputMessage::Popup` and `EventPayload::Popup` for `display-popup`: a popup is a
  real pane floated above the client, with its own input routing.
- v62 appends `output` to `CommandResponse::Error`. Command-error hook output now reaches the
  requester before the client prints the original command error.
- v61 appends `exit_code` to `CommandResponse::Success`, so foreground shell commands can return
  output and a nonzero status in one response.
- v60 appends `InputMessage::CancelPrefix { request_id }` and
  `EventPayload::PrefixCancelled { request_id }`. The pair gives dialogs an ordered cancellation
  barrier before they return keyboard ownership to the workspace.
- v59 appends `TimedClientMessage` and adds automatic-rename plus retained-dead metadata to mux
  snapshots. Older clients cannot decode either shape, so the normal exact-version restart path is
  required.
- **Any change that affects an already shipped encoding** (new enum variants, reordered fields,
  changed integer widths) **requires bumping the version**. The envelope's `VersionMismatch` guard
  then forces the peers to agree. A rejected first frame gets a best-effort mismatch response before
  disconnect; clients also classify the received envelope version or the daemon's versioned identity
  file, so an older local daemon produces a legible restart path rather than a bare EOF.
- **`ClientHello.capabilities` carries the caller's terminal.** Since 2026-08-20 an Interactive
  client sends `client-terminal-v1` when its stdin and stdout are both a TTY. That is tmux's
  `CLIENT_TERMINAL` flag: the daemon uses it to decide whether `new-session`/`attach-session` may
  attach the caller, and the engine turns a missing token into the pin's
  `open terminal failed: not a terminal`. Command clients never send it; Control clients are
  exempt (the pin's `server_client_open` returns early for `CLIENT_CONTROL`), and the GUI defaults
  to sending it. Because the field is an already-shipped `Vec<String>`, adding a token changes no
  encoding and needed **no version bump** — the phase-8 attach wave shipped entirely on v69.
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
  complete 17-key map with values of at most 64 KiB; bounded value deserialization rejects an
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
- The agent lane is bounded the same way, on encode and on decode: a prompt's text plus its image
  bytes to `MAX_AGENT_PROMPT_BYTES` (6 MiB) with each image `format` at most 64 bytes; option, mode,
  and method identifiers to `MAX_AGENT_OPTION_BYTES` (4 KiB), and session IDs and list cursors to
  `MAX_AGENT_SESSION_ID_BYTES` (16 KiB); session paths must be nonempty, fit
  `MAX_GUI_TEXT_BYTES`, and carry at most 256 additional roots, while the destination daemon checks
  its own absolute-path syntax; an `AgentUpdates` batch nonempty, inside the `u64` sequence
  space, and at most `MAX_AGENT_UPDATES_BYTES` (9 MiB); each `AgentPaneWire` JSON blob to 256 KiB and
  a pending permission payload to 64 KiB; and every JSON result to 1 MiB. Rejections surface as
  `ProtocolError::InvalidAgentPayload`.

# Examples

## Control-frame layout (a `ClientHello`)

`ClientHello { protocol_version: 71, client_instance_id: ClientInstanceId(0), kind: Interactive,
device_name: None, capabilities: [], color_scheme: Some(Dark), origin: None }` is 17 bytes on the
wire: an 8-byte envelope over a 9-byte postcard payload.

```text
byte  0  1  2  3 | 4    | 5     | 6  7        | 8  9 10 11 12 13 14 15 16
      0d 00 00 00  00     00      47 00         00 47 00 00 00 00 01 01 00
      └ u32 LE ─┘  lane   flags   version LE    postcard payload
      length = 13  Control        (= 71)
```

- **length `13`** = `ENVELOPE_BYTES` (4) + payload (9); it counts the four envelope bytes, not itself.
- **payload** `00 47 00 00 00 00 01 01 00`: variant `0` (`ProtocolMessage::ClientHello`),
  `protocol_version` as the varint `0x47` (= 71), `client_instance_id` as varint `00`, `kind`
  variant `0` (`Interactive`), `device_name` as the `Option::None` tag `00`, `capabilities` as the
  sequence length `00`, `Option::Some` tag `01`, `TerminalColorScheme` variant `1` (`Dark`), then
  `origin` as `Option::None` (`00`). Postcard
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
`InvalidStatusLine`, `InvalidGuiRequest`, `InvalidPasteUpload`, `InvalidAgentPayload`, `Io`.

## Handshake sketch

```text
client → ClientHello { protocol_version: 71, client_instance_id: i1, kind: Interactive, device_name: Some("laptop"),
                       capabilities: [], color_scheme: Some(Dark), origin: None }
server → ServerHello { protocol_version: 71, server_id, client_id: c11, client_instance_id: i1,
                       capabilities: ["mux-v1", "terminal-viewport-v3", "terminal-row-patches",
                                      "terminal-appearance-v2", "config-overrides-v1", ...,
                                      "new-session-attach-v1"],
                       appearance, appearance_provenance, mux_options, status, key_tables }
client → SetConfigOverrides { entries: [("theme", "Catppuccin Mocha"),
                                        ("font-size", "13"),
                                        ("prefix", "C-a"), ("mode-keys", "vi"), ...] }
client → Attach { session: "" }      // attach current, or lazily create numeric 0 if empty
server → Attached { session: $0, snapshot: MuxSnapshot { generation, sessions, focused_window } }
server → Event { sequence: 1, payload: TerminalViewport { pane: %3, viewport } }   // Terminal lane
server → Event { sequence: 2, payload: MuxOptionsChanged { options } }             // Control lane
client → HistoryRequest { pane: %3, start: 1_488, count: 512 }
server → Event { sequence: 3, payload: HistoryChunk { pane: %3, start, total, offset, ... } }
...
(unusable patch)      client → RequestFull { pane: %3 }
(sequence gap)        client → Resync
(another device runs `attach-session -d`)
server → Event { sequence: n, payload: Detached { session: $0, by: Some("desktop"), reason: Evicted } }
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
