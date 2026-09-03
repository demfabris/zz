---
type: Protocol
title: zz wire protocol (v96)
description: The versioned, little-endian length-prefixed, postcard-encoded control protocol whose ProtocolMessage enum carries the entire client/daemon conversation over local IPC or an SSH tunnel.
resource: crates/zz-protocol/src/framing.rs
tags: [protocol, wire, framing, postcard, versioning]
timestamp: 2026-08-31T00:00:00-03:00
---

# Overview

The zz wire protocol is a **versioned, framed, `serde`/`postcard`-encoded** control protocol spoken
over a Unix-domain socket (Linux/macOS) or a named pipe (Windows). macOS and Linux reach a remote
daemon through an OpenSSH `ssh -L` Unix-socket forward. iOS instead carries the same framed stream
through `zz proxy` over an in-process `russh` SSH channel.
Every message is wrapped in a fixed envelope carrying a `u32` little-endian length prefix, a
one-byte **lane** tag, a **flags** byte, and a `u16` **protocol version**. The current wire version is
**`PROTOCOL_VERSION = 96`** (`crates/zz-protocol/src/message.rs`).

The version is a gate, not a negotiation: a frame whose envelope version differs from the running
build's is rejected outright. Before disconnecting, a daemon makes a best-effort
`CommandResponse::Error(ServerError::ProtocolMismatch)` reply encoded with its own version, so the
client can identify the stale side and offer a guarded restart. Anything that changes an
already-shipped encoding bumps it. The version history below records each wire change, and
`knowledge/log.md` records its delivery. This doc describes only the shape speaking today. Framing
lives in `crates/zz-protocol/src/framing.rs`, the message vocabulary in
`crates/zz-protocol/src/message.rs`.

There are two lanes sharing one envelope: **Control** (lane `0`, `postcard`-encoded
`ProtocolMessage`) and **Terminal** (lane `1`, hand-packed; see
[packed terminal lanes](/protocol/terminal-lanes.md)). Both lanes ride one ordered stream, local or
SSH-tunneled, and every frame is delivered reliably and in order. This concept documents the Control
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
| version | 6..8 | `u16` LE | `PROTOCOL_VERSION` (96) |
| payload | 8.. | bytes | `postcard(ProtocolMessage)` (Control) or packed terminal sections |

# Schema . `ProtocolMessage` (Control lane)

The top-level enum (`message.rs`). Postcard encodes the variant index as a varint tag, then the
fields in declaration order.

| Variant | Fields | Purpose |
|---------|--------|---------|
| `ClientHello(ClientHello)` | `protocol_version: u16`, `client_instance_id: ClientInstanceId`, `kind: ClientKind`, `device_name: Option<String>` (≤256 B), `capabilities: Vec<String>`, `color_scheme: Option<TerminalColorScheme>`, `origin: Option<PaneId>`, `working_directory: Option<PathBuf>` (≤16 KiB), `environment: Vec<String>` (≤4,096 entries, ≤16,367 B each, ≤4 MiB total), `process_id: u32` | Client → daemon handshake. The process-stable instance ID owns recoverable Agent drafts across reconnects; `device_name` labels this device in presence and eviction notices; `$ZZ_PANE` supplies `origin`, so an untargeted CLI command resolves against its invoking pane; eligible local endpoints publish an absolute UTF-8 cwd while SSH, unrepresentable, and oversized paths omit it. Local and SSH-forwarded connections publish a sorted and deduplicated UTF-8 process-environment snapshot; unrepresentable Unix names or values are omitted without substitution. Entries require a nonempty name and `=` separator, forbid NUL, and allow empty values or additional `=` bytes. `process_id` supplies `#{client_pid}` and uses zero when a caller cannot report one. Additive capability strings carry terminal identity and nested intent without changing this struct |
| `ServerHello(ServerHello)` | `protocol_version: u16`, `server_id: u64`, `client_id: ClientId`, `client_instance_id: ClientInstanceId`, `capabilities: Vec<String>` (≤64 entries, ≤256 B each), `appearance: TerminalAppearance`, `appearance_provenance: AppearanceProvenance`, `mux_options: MuxOptions`, `status: StatusLine`, `key_tables: Vec<KeyTableSnapshot>` | Daemon → client handshake reply; echoes the accepted process identity, while every key table (root, prefix, copy-mode, copy-mode-vi, custom) lets clients label key hints and render binding help and capabilities describe optional behavior |
| `CommandRequest(CommandRequest)` | `request_id: u64`, `command: CommandInvocation`, `prepared: bool` | tmux-style command from any client. Control and the local CLI set `prepared` after the daemon freezes one alias layer. A prepared multi-command alias travels as one opaque `CommandInvocation`; the daemon still runs authorization and ordinary dispatch validation |
| `CommandResponse(CommandResponse)` | `Success { request_id, output, exit_code, stderr }` / `Error { request_id, error: ServerError, output }` | Command result. A client prints either output field before it reports an error or returns the exit code. `stderr` (appended at v71) is populated for `source-file` diagnostics issued by a Command client since Wave E (2026-08-22) and stays empty for every other command; `Success` with a nonzero `exit_code` is a COMPLETED command, so `Error` stays reserved for dispatch, transport, and server failures |
| `Attach { session: String }` | target string | Interactive attach request. An empty target lazily creates the next numeric session when the daemon has none; explicit missing targets and Command-kind attaches do not create. A session holds a set of attached clients, so a second device never collides with the first |
| `Attached { session: SessionId, snapshot: MuxSnapshot, read_only: bool, client_flags: String }` | resolved id + full state + effective reconnect options | Attach acknowledgement. The option fields describe the state actually applied by this attachment, so a TUI changing connections does not have to infer which child of an opaque alias attached |
| `Detach` | . | Interactive detach; drops the sending client's attachment and leaves every other viewer attached. The connection stays open, and the client remains a subscriber that can send another `Attach` |
| `SetColorScheme(TerminalColorScheme)` | light/dark | Client-driven appearance change |
| `SetConfigOverrides { entries }` | ordered `Vec<(String, String)>` | Replace the daemon's complete appearance and mux override sets; an empty vector clears both |
| `Input(InputMessage)` | see below | Keyboard / resize / view / split input |
| `Event(Event)` | `sequence: u64`, `payload: EventPayload` | Ordered daemon → client push |
| `GuiResponse(GuiResponse)` | `Success { request_id, output }` / `Error { request_id, message }` | Interactive client → daemon answer to an `AgentCommand` or `BrowserCommand::Screenshot` request; both strings bounded to `MAX_GUI_TEXT_BYTES` (64 KiB) |
| `Resync` | . | Client requests a fresh snapshot after a shell-detected reconciliation error; event sequence gaps alone are valid because the daemon may supersede stale terminal frames |
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
| `PrepareCommandList { request_id, commands }` | request identity plus `Vec<CommandInvocation>` | Client → daemon: freeze one live alias layer for a complete command unit under one mux lock. Preparation performs no command effects, target or format resolution, hook emission, message publication, or authorization |
| `PreparedCommandList { request_id, commands }` | request identity plus one `PreparedCommand` per input | Daemon → client: return the immutable invocation, optional canonical identity, `alias_matched`, and `Ready` or a typed `ServerError`. Multi-command and empty aliases return their opaque invocation with `canonical_name: None`, `alias_matched: true`, and `Ready`. The echoed request ID lets a client ignore stale replies while notifications share the stream |
| `SetTerminalPreview { enabled }` | `enabled: bool` | An attached Interactive client enables or disables passive terminal delivery for every window in its attached session. Foreground visibility, input, history, and PTY geometry remain unchanged |

`CommandInvocation` is `{ name: String, args: Vec<String>, source: Option<SourceSpan>,
command_blocks: Vec<u32>, expanded_alias_group: bool }`. Protocol v84 appends `command_blocks`: sorted, unique, zero-based
positions into `args` for standalone unquoted `{ ... }` arguments. The corresponding argument keeps
its brace-bearing text. Quoted brace text has no entry and remains a string. Config and Control
parsers populate the positions; argv-created commands start with an empty vector. Aliases and key
table snapshots preserve the positions across request, preparation, `ServerHello`, and
`KeyTablesChanged` frames.

Multi-command and empty aliases reuse this encoding. The mux stores the frozen body under the
reserved name `__zz-command-alias-group` with one `{ ... }` argument, `command_blocks = [0]`, and
the v93 `expanded_alias_group` flag; it does not add a child-vector field. The flag, not the name,
is what every reader tests: it is set only by alias expansion, so a config, command line, or Control
line spelling that name by hand stays an ordinary unknown command. Alias expansion appends caller arguments to the final parsed child before
it renders that body. Clients scan every child for attach and attaching `new-session` TUI routing,
but only the final leaf for stdin routing. A stdin-reading leaf receives the captured payload inside
that body; the client then sends one opaque invocation with `CommandRequest.prepared = true`. The
daemon reparses and validates the children, executes each through its ordinary path, and treats an
empty body as a successful zero-item queue. This closure changes no postcard shape or protocol tag.

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
| `Popup` | `action: PopupAction::{Text(String), Key { input, text_follows }, TerminalView(TerminalViewAction), Close, Pointer { pointer: PopupPointer, view: Option<TerminalViewAction> }}`; input, pointer and view control for the client's open `display-popup` |
| `Menu` | `action: MenuAction::{Choose(u32), Cancel}`; drives the client's open `display-menu` |
| `Confirm` | `action: ConfirmAction::Reply(bool)`; answers the client's open `confirm-before` prompt |
| `ClientTerminalSize` | `columns: u16`, `rows: u16`; current producer is the TUI terminal surface, which reports later outer-terminal resizes |
| `ClientFocus` | `focused: bool`; reports client-window focus independently from pane/application focus |

Every `Key`/`Text` resolves the per-client `KeyEngine` against the live key tables first; `Pass`
reaches the synchronized Terminal/Browser sinks (Picker and Agent source panes have no sink and
drop passed keys). `BrowserSurfaceKey`/`BrowserSurfaceText` validate a Browser source and go
straight to those sinks, so root bindings cannot consume page input. The desktop client's
window-root prefix claim sends the configured prefix and armed sequence as `Key`, preserving tmux
ownership only for that sequence. A desktop dialog sends `CancelPrefix { request_id }` before it
takes input. The daemon clears only the one-shot prefix table and answers with the matching
`PrefixCancelled { request_id }`. The desktop keeps ordinary workspace keys behind that barrier;
platform and function shortcuts remain available.

`text_follows` is a correlation promise, not an activity request. After validating a press or
repeat `Key` or `BrowserSurfaceKey`, the daemon appends an entry carrying its pane and Terminal or
BrowserSurface lane to one ordered queue per client, capped at 32 entries. `Text` scans forward to
the first entry on the same pane and lane, retires only the skipped prefix, consumes that match, and
preserves later entries. It
inherits the key's dispatch, modal-consumption, read-only, and activity result. The pair therefore
contributes at most one activity/latest update, not necessarily one: a writable prompt or
`display-panes` surface may consume both at zero. Empty matching text is inert but retires the
matching dispatch suppression. A no-match Text leaves the queue intact and treats nonempty text as
standalone. Bounded eviction retires any suppression debt linked to the evicted entry. Detach,
unregister or reconnect, and a successful wire `Attach` clear
the ledger. A synchronous `switch-client` executed by the key keeps it, because the trailing text
still belongs to that key. GPUI terminal committed text uses standalone `Text`; the GPUI browser
emits a correlatable `BrowserSurfaceKey` plus `BrowserSurfaceText` pair. TUI keys set
`text_follows: false`; FFI callers choose the bit explicitly, and iOS sends standalone text plus
unpaired keys.

## `EventPayload` variants

`Snapshot(MuxSnapshot)`, `AgentCommand { pane, request_id, command }`,
`AppearanceChanged { appearance: Box<TerminalAppearance>, provenance: AppearanceProvenance }`,
`MuxOptionsChanged { options: MuxOptions }`, `StatusChanged { status: StatusLine }`,
`TerminalViewport { pane, viewport }`, `TerminalPatch { pane, patch }`,
`Clipboard { pane, request_id, target, text }`, `BrowserCommand { pane, command }`,
`TerminalUiCommand { pane, command }`, `CommandPrompt { state }`,
`CommandOutput { pane, output_id, viewport }`,
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
`KittyImagesRemoved { pane, image_ids }`,
`StartupConfigCauses { causes: Vec<String> }`, and
`ControlCommandOutput { output: String }`.

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
control-mode client why the daemon is ending the conversation. A nonempty reason renders
`%exit <reason>`; an empty reason is the targeted clean-shutdown signal and renders bare `%exit`.
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

The wire already carries enough identity for the slice 10ai Control-exit fix. On EOF or a blank
Return, the front-end must stop rendering pending and later `PaneOutput` and `PaneOutputAged` bytes
while it drains guards, diagnostics, command output, flow notifications, retained status, and one
final exit. The active `control-mode.exit-pane-output` item requires no protocol field or version.

Slice 10ah closed `control-mode.kill-server-response-order` with existing messages. Shutdown now
freezes new response admissions, waits for admitted Command and Control responses, publishes
`ServerStopping`, and drains every registered writer before endpoint removal. Control closes the
flags-1 command guard with `%end` before exactly one `%exit`; ordinary Command clients receive their
success response before socket teardown. The listener remains held through the bounded drain, so
the change needs no protocol field or version. A Control client that itself executes `kill-server`
also receives an empty-reason `ControlExit` immediately after that child's guard. This supplies the
pinned early clean exit without moving the generic `ServerStopping` ahead of admitted responses.

Multi-body alias shutdown also uses existing messages. The daemon records force or a quiet policy
recheck in queue-local state, holds shutdown behind admitted foreground jobs and zero-delay
detached command-mode callbacks, and buffers structural hook events until the outer queue settles.
Force discards structural events queued before `kill-server`, derives shutdown events from the
sessions that remain, and cancels delayed non-detached hook jobs. Source and hook queues keep their
own yield boundaries. A Control alias has no synthetic parent frame: its existing
`ControlCommandGuard` events describe the children with inherited flags, and the existing
`CommandResponse` settles the request. A clean self-shutdown signal is captured behind the
`kill-server` child guard, while a generic shutdown observed from another client stays behind the
in-flight alias. The
exact history-dependent multi-window `window-unlinked` sequence remains tracked under
`hooks.shutdown-window-unlinked-order`; no shutdown field or version bump can reconstruct tmux's
winlink-tree history.

Direct Control config construction still runs in the client process before daemon execution, but
since v92 its `~` lookups do not. A line containing a tilde is walked once with a recording context,
the names it needs cross as one `HomeDirectoryRequest`, and the line is re-parsed with the answers,
so source, startup, alias, callback, and direct Control parsing all read the same daemon-global
`HOME` and the same daemon-host passwd database. The round trip is invisible: no guard, no frame, no
command number. Variable expansion is unaffected, so `$NAME` stays literal in direct Control input
where the pin's lexer expands it.

v76 introduced `SourcedCommandGuard { output, error, client_failure }` at `EventPayload` tail tag 47.
It gave parser-owned source replay and synchronous foreground inserted lists one flags-1 command
guard per command that survived name resolution. v77 renames the same tag in place to
`ControlCommandGuard { output, error, sticky_failure, flags }`. The tag stays 47, but the field shape
changes, so exact-version peers reject v76/v77 skew. `error` selects `%error` rather than `%end`,
`sticky_failure` independently retains Control retval 1, and `flags` carries tmux's command-frame
state. The pinned states observed here are 1 for parser-owned replay and 0 for fresh immediate-hook
and background-callback queues.

An opaque alias group emits no wrapper guard. Each child emits its existing guard with the input
unit's inherited flags, including children reached through a nested source or callback. The final
group response does not add another frame. An empty group emits no guard, and a dispatch failure
before the first child guard falls back to one ordinary error frame in the Control front end.

Parser-owned replay, foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`,
and foreground `run-shell -C` retain flags 1. Per-client and per-thread capture publishes the
containing replay command before each inserted command, then an inserted source before its children.
The writer defers that tree until the direct outer frame closes. An unsupported zz-only inserted
command receives an empty success guard and later siblings continue, but it does not enter
`ConfigLoadReport`'s skipped summary.

Immediate `after-*` and `command-error` hooks retain the originating Control recipient but clear the
parser replay client and enter a fresh no-hooks state. Every hook command, hook `source-file`, and
sourced descendant gets its own flags-0 frame. Hook array entries run in order. A failure stops only
the current command list, so later array entries and later parser-owned flags-1 commands continue.
Hook output and diagnostics never fold into the triggering frame, and a hook cannot automatically
retrigger itself. Unknown or ambiguous sourced command names are rejected before execution, publish
only `%config-error`, and do not fire `command-error`. Alias resolution is frozen once before source
classification and execution. `set-hook -R` copies only the hidden Control target into its retargeted
hook context. A mixed missing-and-matched hook source ends `%end` but sets `sticky_failure`, retaining
process status 1 while later hook commands continue.

Shell-evaluated `if-shell -b` and `run-shell -bC` retain the same exact Control recipient after the
triggering flags-1 frame closes, clear `replay_client`, and map every callback command to flags 0.
Later flags-1 input may finish before the callback. Inserted sources keep parent-before-child order,
and missing sources or runtime failures set `sticky_failure` without folding output into the trigger.
Malformed delayed lists stay silent and status-neutral. Before callback execution begins, the
callback verifies that its originating Control client remains registered. A client disconnected
before that point gets no callback frame or inserted effect, and a replacement client receives
nothing from it. `control-mode.disconnect-cancels-command-queue` separately owns hard disconnect
after an immediate hook or source queue has already started. Ordinary `run-shell -b` shell jobs
remain outside this command-guard path.

v78 appends `ControlSourceFile { event: ControlSourceFileEvent }` at `EventPayload` tail tag 48.
`ControlSourceFileEvent::ReadError(String)` keeps matched OS or path read failures typed on the wire,
while the Control writer renders the text as one raw unframed line immediately after the source
guard and retains status 1. `Complete` emits no text or frame. It advances the writer's command
number once after the invocation's descendants, matching the pin's invisible source-completion
callback item. One invocation emits one `Complete` even when it has multiple matched read failures.

Every source invocation that passes depth checking publishes `Complete`, including an empty file, a
loud or quiet miss, a matched parser error, and `source-file -`. A depth-refused invocation and a
syntax, arity, or unknown-flag rejection publish none. Parser-owned flags-1 and immediate-hook
flags-0 sources share this event path. The daemon reads every matched file before replay, so multiple
raw read diagnostics precede the first replayed child while the single completion follows all
descendants. Non-UTF-8 content remains under `config.non-utf8-file-bytes`: the pin's measured
lone-`0xff` case also consumes an extra invisible empty-command item that zz does not model. Source
stdin transport, Control sourced-hook cwd, and deferred event-hook client selection retain their
separate gaps. The `aliases.command-bodies` closure covers the alias queue's source and hook yield
boundaries.
Slice 10z closes config file-unit construction in daemon-local state without a protocol field.
Config command-name and lexer diagnostics remain generic Warning events on the
`%config-error` classification path.

`zz-client::ClientCore` accepts and ignores `ControlCommandGuard` and `ControlSourceFile`;
`crates/zz/src/control_mode.rs` renders both Control-only events. The daemon preflights every declared
path for one source command before recursion, so a
three-level parser replay publishes the root missing-path guard, middle missing-path guard, and leaf
output guard once each. The Control front end combines guards with `CommandResponse` and `Detached`
messages. Direct runtime errors, parser-owned sourced runtime errors, nonruntime source failures,
synchronous inserted runtime errors, hook sticky failures, and typed parser-owned OS or path read
errors set retval 1. A `PostAdmissionCallback` error still updates that long-lived value while
letting a Return captured before the selected callback failure preserve its prior code. Generic
nonzero successes and flags-1 parse or preparation failures do not. Return and detach precedence
remain unchanged.

Deferred event hooks clear the Control target and remain separate. Command replay retains the
caller cwd for sourced hooks; Control hook framing clears the replay client, so sourced-hook cwd is
a Control-only gap. Event-hook cwd and the three missing pane-event producers stay under their
named gaps. Pinned `after-queue` is explicit-only and needs no automatic producer.

Command and Interactive replay transcripts closed without another wire field. Each source invocation
appends its complete verbose batch, replay output, and buffered command-name or parser diagnostics in
that order. Source no-match, glob, and actual OS or path read failures retain their existing error channels. Nested
invocations insert their complete frame at the parent replay position. Command returns that
transcript once in its existing response output. Interactive renders one existing command-output
viewport from the same transcript, subject to the existing command-output size bound. This is
per-invocation batching, not a claim of physical interleaving.
At the v78 checkpoint, generic config Warning typing, startup diagnostic delivery, hard-disconnect
queue cancellation, config byte input, source stdin transport, parser abort semantics, hook cwd
selection, and deferred event-hook client selection remained open. v80 later closes startup
diagnostic delivery; the multi-body alias slice later closes queue-local event buffering and yield
ordering except for `hooks.shutdown-window-unlinked-order`.

v79 changes the existing `EventPayload::CommandOutput` tag 11 in place to
`CommandOutput { pane, output_id, viewport }`. The daemon allocates each real command-output actor a
nonzero ID from one daemon-lifetime-global monotonic counter. Initial frames, later view updates,
current-state resync frames, and the actor's close keep that ID. Only
`CommandOutput { output_id: 0, viewport: None, .. }` means an authoritative resync with no live
output. A populated viewport with ID zero fails validation on both encode and decode.

The populated form stays on the Terminal lane and inserts `output_id` after `sequence`; closes and
the zero-ID empty resync stay on the reliable Control lane. `zz-client::ClientCore` advances an ID
watermark on newer frames and closes, ignores older traffic, and refuses to resurrect a closed actor
from a delayed same-ID frame. `adopt_hello` resets the watermark for every newly adopted handshake
because that handshake may come from a restarted daemon with a fresh ID lifetime. Reconnecting to
the same daemon does not restart its counter. The TUI keeps search, swallowed-key, and
output-geometry state across same-actor frames and resets them when the actor ID changes or the
output closes.

v80 appends `EventPayload::StartupConfigCauses { causes }` at tail tag 49. A bounded deserializer
and `validate_control_message` both enforce at most 1,024 causes, 64 KiB per cause, and 1 MiB across
the vector; validation failures use `ProtocolError::InvalidStartupConfigCauses`. The daemon sends
the raw vector only to a Control client that wins the global startup-cause delivery. The Control
front end writes one `%config-error ` prefix per vector element, so embedded newlines remain raw
continuation lines, and the event alone does not change its return value.
`startup-config-owner-v1` appears only on the Control connection made after that process spawned the
daemon. That owner receives the event after `ServerHello` and before its first `%begin`. A Control
client that connects to a running daemon receives it only after `Attached`, inside the attach frame.
Interactive clients receive no v80 payload. The daemon opens the existing `CommandOutput` surface
with an ordered, UTF-8-safe 64 KiB preview, replacing every Unicode control except LF and TAB. A
truncated preview ends with `... startup diagnostics truncated; restart in Control mode for full
output`. Exact Interactive recovery of the full retained 1 MiB vector is intentionally not part of
the protocol. `zz-client::ClientCore` accepts and ignores the Control-only event.

v81 appends `EventPayload::ControlCommandOutput { output }` at tail tag 50. The daemon sends this
Control-only event to the exact originating client for targetless or invalid-target foreground
`run-shell` output. The Control writer waits for the open direct or sourced flags-1 guard to close,
then writes the payload raw. Embedded LF and percent-prefixed lines remain literal; a payload without
a trailing LF gains one. The event does not update Control retval, so a fresh client still exits 0
after a shell returns nonzero. Foreground `run-shell -C` keeps its existing synchronous framed output.
A resolved `-t` and ordinary `run-shell -b` use the native Interactive command-output actor instead
of this event.

The three payloads `TerminalViewport`, `TerminalPatch`, and
`CommandOutput { output_id, viewport: Some(..), .. }` are
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
tmux target-lookup wording and only the normalized component that failed. Protocol v76 appends
`CommandParse(String)` at tail tag 12. It identifies command-name, flag, arity, and other parse or
preparation failures. Target lookup and semantic or runtime failures retain their existing variants,
so callers can abort parse failures before effects without changing runtime queue ordering. Protocol
v85 appends `PostAdmissionCallback(Box<ServerError>)` at tail tag 13. It preserves the underlying
error and display text while identifying a selected synchronous callback that may fail after Control
has already captured a Return, so Control can preserve that Return without matching error prose.

# Attachment, presence, and per-client views

The daemon keys attachment as `attached: BTreeMap<SessionId, BTreeSet<ClientId>>`, so any number of
devices can hold one session at once and no error variant exists for a session that is already
attached. Registration itself does not attach or create a session. An Interactive empty-target
attach to an empty daemon creates and attaches the next numeric session atomically from the caller's
perspective; the first one is `0` with ids `$0`, `@0`, and `%0`. Each attached client owns its own
terminal view (`TerminalViewId(client.0)`), key engine, focused window, and overlay state.

`aggressive-resize` selects which viewers are eligible, while `window-size` selects how their
measurements combine. `terminal_geometry_owner` picks the eligible viewer with the highest
daemon-global terminal-input sequence, breaking ties by lowest `ClientId`. `latest` takes columns,
rows, and cell-pixel dimensions from that owner. `largest` and `smallest` aggregate columns and rows
componentwise, and `manual` retains the stored layout extent; all three still take cell-pixel
dimensions from the owner. Typing in a pane reclaims ownership. Client FocusIn does too when the
server `focus-events` option is on, so every mode refreshes the owner's cell
metrics while only `latest` also uses that ownership for rows and columns. These policies feed the
existing guarded window-extent write-back. See [multi-device attach](/designs/multi-device-attach.md).

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

Remote carriers preserve the identical envelope but are platform-specific. macOS and Linux desktop
clients use OpenSSH `ssh -N -L` to forward the daemon's Unix socket (`crates/zz-daemon/src/endpoint.rs`
`ssh_forward_command`). iOS selects `RusshForward` in `crates/zz-daemon/src/client.rs`
`connect_endpoint_with_prompts_and_terminal`; it opens an in-process `russh` session, runs
`zz proxy --socket ...`, and pumps frames over the SSH channel's stdio
(`crates/zz-daemon/src/russh_client.rs` `establish`). `Endpoint::parse` accepts `unix://`, a bare
path, and `ssh://[user@]host[:port][/remote/socket]`, and nothing else . a `quic://` string is
rejected with a pointer at `ssh://`. Both carriers expose one reliable ordered byte stream to the
protocol, with no alternate frame shape, compression, or unidirectional supersession. That keeps
the protocol location-transparent.

The old QUIC transport was deleted on 2026-08-01 along with the per-frame unidirectional streams,
the negotiated zstd envelope flag, and the mux `browser-egress` option / QUIC splice. Remote browser
egress itself is current: a client-local `browser-egress` key in `zz/config` points CEF at
`ssh -D` (`socks5-direct`). See [remote browser egress](/designs/remote-browser-egress.md).
`RequestFull { pane }` outlives QUIC: a client that cannot apply a patch still repairs one pane at a
time, and the daemon's frame supersession now happens entirely in the outbound mailbox (see
[zz-daemon](/crates/zz-daemon.md)).

When a remote connection dies, the GPUI client backs off for 1, 2, 4, 8, 16, then 30 seconds. The
native Apple client uses 1, 2, 4, 8, then a 16-second cap and starts the next attempt when network
service returns. Both retain the intended session and guard connection attempts so a stale timer
cannot replace a newer connection. The daemon sees an ordinary disconnect followed by an ordinary
attach.

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
`set-titles` defaults off. zz-tui writes OSC 2 when a non-empty title changes. GUI clients ignore
every `StatusLine` field, including `title`. Every string field and each row is bounded to
`MAX_STATUS_TEXT_BYTES` (4 KiB) on encode and during deserialization, rows are capped at
`MAX_STATUS_ROWS` (5) with a sixth rejected before allocation, `base_style` must parse as a style,
and `message_line` must be `0` with no rows or under `rows.len()` otherwise; `StatusChanged` payloads
now validate on both encode and decode.

# Versioning & compatibility

- **`PROTOCOL_VERSION: u16 = 96`** is stamped into every frame's envelope and re-checked inside
  `ServerHello` (`validate_control_message` rejects an inner-version mismatch even if the envelope
  version passed).
- v96 appends `ProtocolMessage::CommandQueueParked { request_id: u64 }` after `HomeDirectoryResponse`.
  The daemon sends it from a Control client's command worker at the moment that client's command
  queue parks on a request that has not answered yet: a blocking `run-shell` or `if-shell` waiting
  on its job, a `wait-for` lock, and the overlays that hold their invoker (`command-prompt`,
  `display-panes`, `display-menu`, `display-popup`, `confirm-before`). It is the wire half of the
  pin's `CMD_RETURN_WAIT`: `cmdq_next` stops on a waiting item, so a client whose input already
  reached end of file exits there and its queue is freed with every later item unrun. The variant
  is appended, so a v95 peer cannot decode it.
- v96 appends `CopyModeAction::Search(Box<CopyModeSearch>)` after `CopyLine`. `CopyModeSearch` is a
  new type carrying `text` (`String`), `direction` (the existing `SearchDirection`), `regex` and
  `incremental` (both `bool`), and it is the payload of the six pinned copy-mode search entry points
  that take a string: `search-forward`, `search-backward`, their `-text` spellings, which clear
  `regex`, and their `-incremental` spellings, which arrive with the `command-prompt -i` prefix
  character already read off the argument. `CopyModeAction` rides `TerminalViewAction`, which
  `InputMessage` and `EventPayload::TerminalUiCommand`'s neighbours on `ProtocolMessage` carry, so
  the appended variant is wire-reachable and a v95 peer cannot decode it. Nothing else moves: no
  existing variant, field or type changes.
- v96 appends `PopupAction::Pointer { pointer: PopupPointer, view: Option<TerminalViewAction> }`
  after `Close`, and adds the two types it carries: `PopupPointer` (`column`, `row`, `button`,
  `drag`, `release`, `meta`) and `PopupPointerButton` (`None`, `Left`, `Middle`, `Right`). tmux
  keeps the popup's border, drag, and menu policy in `popup_key_cb`, so a client reports the
  pointer event in its own cell grid and hands over the `TerminalViewAction` it would have given
  the popup's job; the daemon runs the policy and executes exactly one of the two. Appended, so a
  v95 peer decodes none of it. Nothing else on the wire changed.
- v95 appends the pane border chrome to `MuxSnapshot`. `WindowSnapshot` gains four fields after
  `activity`: `pane_border_status` (`PaneBorderStatus`, the window's `pane-border-status` with the
  pin's `window_get_pane_status` folding of `top-floating`/`bottom-floating` back to off already
  applied), `pane_border_lines` (`PaneBorderLines`, the `pane-border-lines` glyph family),
  `pane_border_indicators` (`PaneBorderIndicators`, how the active pane is marked), `pane_order`
  (`Vec<PaneId>`, the pin's `w->panes`, which is what `window_pane_index` counts and so what the
  `number` glyph family draws) and `pane_z_order` (`Vec<PaneId>`, the pin's `w->z_index` for tiled
  panes, which decides which of two panes owns a border cell they both touch). `PaneSnapshot` gains `border_status_text` after
  `active_border_colour`: the `pane-border-format` the daemon expanded for that pane, empty while
  the status row is off, because the row is a status line the daemon owns and not something a client
  can compose from the snapshot. All six are appended and a v94 peer decodes none of them.
- v94 appends `CopyModeAction::RefreshOn`, `RefreshOff`, `RefreshToggle`, `RefreshRevision` and
  `CopyLine(Box<CopyModeCopy>)` after `PreviousMatchingBracket`. The first three are the pinned
  `refresh-on`, `refresh-off` and `refresh-toggle` names; `RefreshRevision` is one tick of the
  refresh timer, which tmux runs inside `window-copy.c` as a libevent timer and zz runs in the
  daemon because the terminal worker owns none; `CopyLine` carries the existing `CopyModeCopy`
  payload and is the `copy-line` family (`window_copy_do_copy_line` selects the whole logical line
  the cursor sits on without a selection, copies it, then restores the cursor and view).
  `CopyModeAction` rides `TerminalViewAction`, which is carried by `InputMessage` and by
  `EventPayload::TerminalUiCommand`'s neighbours on `ProtocolMessage`, so the appended variants are
  wire-reachable and a v93 peer cannot decode them.
- v94 appends `MenuState.mouse_keys` after `stay_open`. It is the pin's
  `MENU_NOMOUSE` inverted: `cmd_display_menu_exec` sets that flag whenever the
  command has no invoking mouse event and no `-M`, and such a menu answers only
  button 1, which it ignores, while any other button leaves it. The flag has to
  reach the client because the client owns the live pointer route, and
  `menu_prepare` also resolves a starting choice only for a `MENU_NOMOUSE` menu,
  so a `-M` menu opens with no highlight and the daemon now publishes `selected`
  as `None` for one. A v93 peer cannot decode the appended field.
- v94 appends `MuxOptionKey::FocusFollowsMouse` after `Prefix2`, taking `MuxOptionKey::ALL` from
  seventeen keys to eighteen. `MuxOptions` must carry every key exactly once, so the appended key
  changes the map on `EventPayload::MuxOptionsChanged` and a v93 peer decodes neither the variant
  nor a map that contains it. The daemon stamps the value the way it already stamps `mouse`: it
  resolves `focus-follows-mouse` against the session the recipient is attached to, the way the pin
  reads `s->options` in `server_client_check_mouse`, and against the global session table for a
  client attached to none. Both clients gate their pointer route on it.
- v93 appends `KeyToken::Raw(u8)` after `Named`. `send-keys -H` is strtol base 16 clamped to one
  byte and tmux writes that byte to the pane raw as `KEYC_LITERAL`, so codes 0x80..0xff cannot ride
  `KeyToken::Literal`, whose UTF-8 text would re-encode them as two bytes. `KeyToken` travels on
  `EventPayload::BrowserCommand` through `BrowserCommand::SendKeys` and `SendKeysRepeated`, so the
  appended variant is wire-reachable; a v92 peer cannot decode it. Browser panes have no raw byte
  sink and drop the token.
- v93 appends `expanded_alias_group: bool` to `CommandInvocation`, after `command_blocks`. It is
  the provenance that marks a multi-command `command-alias` expansion, and it replaces the
  name-only test that any parser could satisfy. `CommandInvocation::new` clears it, only
  `into_expanded_alias_group` sets it, and alias expansion is the single caller, so a user-authored
  `__zz-command-alias-group { ... }` block is rejected as `unknown command:
  __zz-command-alias-group` at the CLI, in a configuration file, and in Control input the way the
  pin rejects any unknown name. A v92 peer cannot decode the appended field.
- v93 appends `text: String` to `ChooseTreeItem` and to `ChooseBufferItem`. It carries the
  `choose-tree -F` / `choose-buffer -F` product the daemon expanded in that row's own session,
  window, pane, or paste-buffer context, and an attached client draws it in place of the row it
  would otherwise compose from `label`/`detail` or `name`/size/`preview`. The field is empty when
  the command carried no `-F`, so a chooser without the flag draws exactly what it drew on v92.
  Both fields are bounded by `MAX_CHOOSE_ITEM_TEXT_BYTES` (512) on decode. A v92 peer cannot
  decode the appended field.
- v92 appends `ProtocolMessage::HomeDirectoryRequest { request_id, users }` at tail tag 36 and
  `ProtocolMessage::HomeDirectoryResponse { request_id, homes }` at tail tag 37. A Control client
  parsing its own direct input asks the daemon to resolve the `~` names one line needs before it
  re-parses that line, so a bare tilde reads the server's global `HOME` and a named tilde reads the
  daemon host's passwd database, the way the pin's own lexer does. The empty name is the bare
  tilde; `homes` answers one entry per requested name, in order, with `None` for a failed lookup
  that becomes the pin's located syntax error. The request only leaves a client when the line
  actually needs a home, and it consumes no command number, so Control framing and numbering are
  unchanged. `users` is bounded by `MAX_HOME_DIRECTORY_USERS` (1024) entries of
  `MAX_HOME_DIRECTORY_USER_BYTES` (1 KiB), answers by `MAX_HOME_DIRECTORY_BYTES` (16 KiB). A v91
  peer cannot decode the appended variants.
- v92 also replaces `ClientHello.working_directory`'s UTF-8 string encoding with `ClientPath`, a
  bounded byte string. A Unix path is a byte string, not text, so the wire now carries the client's
  own `OsStr` bytes and registration rebuilds the exact absolute `PathBuf`; other platforms carry
  UTF-8 bytes. A client launched in a directory holding byte `0xff` therefore resolves a relative
  `source-file` path the way the pin does, instead of having its cwd dropped for failing
  `to_str()`. Omission is unchanged: a relative cwd is discarded at registration, and a cwd past
  `MAX_CLIENT_WORKING_DIRECTORY_BYTES` (16 KiB) is omitted by the client rather than failing the
  connection, with the encode and decode guards still rejecting an oversized one. The client
  environment keeps its v82 UTF-8 `NAME=VALUE` shape.
- v91 appends `action: ClientExitAction` after `reason` on `EventPayload::Detached` and adds that
  enum (`Exit`, `ParentHangup`, `Exec { command, shell }`). It is what `detach-client -E` and `-P`
  ask the presentation process to do, matching the pin's `MSG_EXEC` and `MSG_DETACHKILL`: `Exec`
  replaces the client process with `shell -c command` and prints no notice, `ParentHangup` prints
  the SIGHUP notice and signals the client's parent, and `Exit` is the ordinary detach every other
  producer sends. `attach-session -x` and an attaching `new-session -X` put `ParentHangup` on the
  eviction path the same way. A v90 peer cannot decode the appended field.
- v91 appends `EventPayload::ControlConfigError { text }` at tail tag 51. A configuration
  diagnostic now carries typed identity from where it was raised instead of arriving as a generic
  `ClientMessage` warning that Control had to recognize by English prose, so `%config-error`
  routing matches the pin's structural `cfg_add_cause` decision and survives reworded or localized
  messages. Only the two config-replay producers emit it and only to Control clients, so no other
  surface changes.
- v90 inserts `MenuItem.annotation` between `key` and `enabled`, the action key a menu row has
  room to draw beside its name; the mid-struct insert shifts `enabled`'s postcard position, which
  the version gate makes safe. `key` stays the key the row answers to, so a row whose bracketed
  key does not fit keeps working while it draws no annotation, the way `menu_add_item` drops
  `key` from the drawn name but leaves `new_item->key` set. `MenuItem` rides `MenuState` inside
  `EventPayload::MenuOpened`, so a v89 peer cannot decode the inserted field.
- v89 appends `ProtocolMessage::ClientFileRequest(ClientFileRequest)` at tail tag 34 and
  `ProtocolMessage::ClientFileResponse(ClientFileResponse)` at tail tag 35, with the
  `ClientFileOperation` enum they carry. The daemon asks an unattached command client to read or
  write one file on the client's own host, the way the pin hands `MSG_READ_OPEN` and
  `MSG_WRITE_OPEN` to a client instead of touching the server's filesystem; `load-buffer` and
  `save-buffer` are the only producers. `path` is already absolute and bounded by
  `MAX_CLIENT_FILE_PATH_BYTES` (16 KiB), payloads by `MAX_CLIENT_FILE_BYTES` (32 MiB), and one
  response answers exactly one request by `request_id`.
- v88 appends fifteen `CopyModeAction` variants at tail tags 50..64 (`SelectionMode` carrying the new
  `CopySelectionMode` unit enum, `StopSelection`, the scroll-exit family, view-relative cursor and
  scroll placements, `TogglePosition`, `RecentreTopBottom`, and `PreviousMatchingBracket`).
  `CopyModeAction` rides the wire inside `TerminalViewAction`, so a v87 peer cannot decode the new
  tail tags; every existing tag is unchanged.
- v87 appends `ProtocolMessage::SetTerminalPreview { enabled }` at tail tag 33. An attached
  Interactive client can request low-priority viewport streams for every terminal pane across all
  windows of its attached session without changing `visible_terminals`, input authority, history
  access, or PTY geometry. Preview frames use the bounded terminal mailbox, yield to foreground and
  reliable traffic, and omit Kitty image payload delivery.
- v86 appends `activity: bool` after `status_label` at the end of `WindowSnapshot`. The daemon copies
  the window's latched activity flag into every snapshot, and native clients can render it without
  parsing a status format. The frozen postcard fixture for a true flag is
  `[1, 2, 1, b'w', 1, 3, 0, 0, 3, 0, 1, b'L', 1, b'V', 1, b'S', 1]`; the final byte is the appended
  boolean.
- v85 appends `ServerError::PostAdmissionCallback` and the `read_only` / `client_flags` fields on
  `ProtocolMessage::Attached`. Control can distinguish selected synchronous callback failures from
  direct wrapper errors without inspecting names or prose, and TUI reconnect uses the attachment
  state the daemon actually applied.
- The opaque multi-body alias value itself still reuses `CommandInvocation`, `PreparedCommand`,
  `CommandRequest.prepared`, `ControlCommandGuard`, and `ServerStopping`; it added no child-vector
  field at v84.
- v84 appends `CommandInvocation.command_blocks`. Zero-based positions preserve lexical command
  blocks through Control requests, prepared commands, and key-table snapshots while quoted braces
  remain ordinary strings.
- v83 appends `ClientHello.process_id`. Each local or SSH-forwarded client reports its originating
  process ID; the daemon retains the value for `#{client_pid}` and clears it with the connection.
  A zero value remains the defined unavailable form.
- v82 appends `ClientHello.environment`. Encoder and decoder enforce its count, per-entry, syntax,
  and aggregate bounds. The daemon keeps one immutable map per connection, removes it on
  unregister, and uses it for fresh-session, attach, native-attach, Control-attach, and selected
  `switch-client` environment refresh.
- v81 appends `EventPayload::ControlCommandOutput` at tail tag 50. The exact originating Control
  client receives raw foreground shell output after its command guard, without a retval change.
- v80 appends `EventPayload::StartupConfigCauses` at tail tag 49. Both decode-time and ordinary
  control-message validation enforce 1,024 entries, 64 KiB per entry, and 1 MiB total.
- v79 adds `output_id: u64` to existing `EventPayload::CommandOutput` tag 11. Real output actors use
  nonzero daemon-lifetime-global monotonic IDs on populated frames, current resyncs, and closes. The
  zero-ID empty form is the authoritative no-output resync sentinel. The packed populated form puts
  the ID after `sequence`, while close and empty-resync forms use postcard on the Control lane.
- v78 appends `EventPayload::ControlSourceFile` at tail tag 48. Its `ReadError(String)` event renders
  one raw unframed Control line and retains retval 1. Its `Complete` event renders nothing and
  consumes one command number after the source invocation's descendants.
- v77 renames tail-tag-47 `EventPayload::SourcedCommandGuard` in place to
  `ControlCommandGuard { output, error, sticky_failure, flags }`. The new flag carries tmux command
  frame state 0 or 1, and `sticky_failure` separates retained process status from the `%end` or
  `%error` terminator. Immediate command hooks can therefore use flags 0 without changing the tag.
- v76 appends `ServerError::CommandParse(String)` at tail tag 12 and
  `EventPayload::SourcedCommandGuard` at tail tag 47. `CommandParse` gives command parsing and
  preparation a wire-stable phase separate from target lookup and runtime command failures. The
  sourced guard carries one parser-owned Control replay command's output, `%end` or `%error` choice,
  and sticky client-failure bit without changing the framing of direct commands.
- v75 appends the counted copy-mode action and repeated browser-key command used by `send-keys -N`.
  `TerminalViewAction::CopyModeCounted { action, count }` uses tail tag 28 and embeds one flat
  `CopyModeAction`; the removed recursive action tag cannot decode or nest. Browser command tail tag
  7 carries `SendKeysRepeated { keys, count }`. The mux never materializes N cloned actions or key
  vectors. Terminal delivery stops after the first full input queue. The daemon and both browser
  consumers clamp browser repetition to `MAX_BROWSER_KEY_REPEAT` (9,999), since browser panes have
  no tmux counterpart and must not loop UINT_MAX on a client UI thread.
- v74 appends `PrepareCommandList` and `PreparedCommandList` at `ProtocolMessage` tags 31 and 32,
  plus `CommandRequest.prepared`. The daemon prepares a complete vector under one mux lock and
  expands one live alias layer without executing commands or resolving targets and formats. Control
  prepares its initial argv unit and each complete input line before it allocates a command frame,
  then executes the returned invocation with `prepared: true`. A prepared request skips alias lookup
  so an earlier command cannot change what a later command in the same prepared line means. The bit
  carries no authority: the daemon still applies read-only authorization and dispatch validation,
  and rejects a forged destructive request from a read-only client. Prepare requests use nonzero
  request IDs because notifications and command responses can interleave on the same stream.
- v73 appends `InputMessage::ClientFocus { focused }` at wire tag 18. GPUI derives the desired state
  only from the window activation lifecycle. `AppView` seeds `true` when construction finds an active
  window; inactive construction leaves the state unset until the first activation callback.
  `MuxClient` sends the value after attachment is ready. A written attach opens a pending epoch;
  `Attached` confirms it and replays the latest desired state once. Reconnect, host switch, and
  session attach therefore do not depend on another OS activation transition. A rejected
  same-connection session attach restores the retained session's ready epoch and sends the latest
  desired state if it changed while the request was pending. Other request-zero failures leave a
  pending or ready focus epoch unchanged. Pane selection and sidebar focus do not update this client
  state. The TUI assumes the outer terminal starts in the foreground, caches outer focus reports
  while attachment is pending, and sends the latest `ClientFocus` value once after each `Attached`
  event. A protocol-owned attach-attempt marker, separate from the focus cache, selects missing-target
  retry and fallback. It returns to idle on `Attached` or terminal attach failure. The TUI restores
  the retained session's ready focus epoch after a rejected sidebar attach and suppresses repeated
  reports with the same value. Other request-zero failures change neither state machine. Real
  `FocusGained` and `FocusLost` events also emit pane focus when the active pane is a terminal;
  attachment does not synthesize pane focus.
  `zz_client_attach` returning true means the client wrote the request. The additive FFI focus call
  does not confirm attachment. iOS waits for `ZZ_EVENT_ATTACHED`, then reports the current scene state
  once for initial, selected-session, recovery, and recreated-session attachments. Foreground and
  background still send the separate pane transition when a terminal input owner exists.
  `TerminalViewAction::Focus` remains the pane/application signal. Exact-version handshakes reject
  mixed-version clients and daemons; there is no negotiation path.
- v72 appends `ClientHello.working_directory`, an optional daemon-host path bounded to 16 KiB.
  Local command, control, GUI, TUI, and FFI connections publish their absolute UTF-8 process cwd
  when it fits; an unrepresentable or oversized cwd is omitted rather than failing the connection.
  An SSH endpoint never publishes its caller-host path. The daemon retains the value per client and
  uses it to prefix relative top-level `source-file` paths after `-F` expansion and before globbing.
  For a registered client, the daemon snapshots that selected base and carries it through nested
  replay, including when runtime `source-file` loads the active default `zz/mux.conf` as an ordinary
  matched path. A direct `reload-config` carries the same base through its separate native reset
  path. Other sentinel-client reloads keep their clientless base. Slice 10ag closes cold startup
  selection without a wire field: only the launcher that auto-spawns a daemon passes a bounded
  UTF-8 cwd through private `--bootstrap-client-cwd`; startup temporarily prefers it through nested
  replay, then clears it before registered runtime source commands. The later
  `clients.attach-session-cwd` slice added an internal per-session cwd and made attached source
  selection prefer it without changing the wire. Deferred event hooks remain under
  `source-file.event-hook-client-cwd`. Command replay carries the caller cwd for sourced hooks.
  `source-file.sourced-hook-client-cwd` now covers Control hook framing, which clears the replay
  client before the hook runs.
  This replay change uses daemon-local state and does not add a protocol field.
  The same unshipped version appends `ChooseTreeState.filter_no_matches` and
  `ChooseBufferState.filter_no_matches` as canonical bool tails. Full chooser events carry the
  static-filter fallback state; the existing search and selection delta events leave it unchanged.
- v71 is the tmux-campaign append bundle: `MuxOptionKey::{Mouse, EscapeTime, Prefix2}` (tags 14-16)
  with per-recipient session-effective `mouse` publication; the personalized `StatusLine` tail
  (`title`, `base_style`, bounded `rows`, `position`, `message_line`, `customized`);
  `PaneIndicator.label` (≤1 KiB, `Copy` dropped); `PaneSnapshot.{border_colour,
  active_border_colour}: Option<TmuxColour>` with validated colour serialization (`Rgb` over
  `0xFFFFFF` rejected on decode); `CommandPromptState.{prompt_type, mode, no_freeze}`, consumed
  since Wave D's final run (D1) — `mode` is what tells a client to stop editing and relay raw
  presses on the pane-targeted `InputMessage::Key` for `-1`/`-N`/`-k`, and there is deliberately
  no prompt-key action;
  `TerminalMode::Copy.hide_position` (one canonical bool byte on the terminal lane; `View` stays
  17 bytes) and `TerminalViewAction::EnterCopyModeWith` (tag 27); `key` on `ChooseTreeItem` and
  `ChooseBufferItem` (≤64 B); `CommandResponse::Success.stderr`;
  `TimedClientMessage.message_id` plus `EventPayload::TimedClientMessageCleared` (tag 46); and
  `InputMessage::ClientTerminalSize` (tag 17) beside the new `client-tty-v1:`/`client-size-v1:`
  hello value tokens. Everything ships with inert daemon defaults; consumption lands in Waves B-E.
  Since 2026-08-21 (B5), terminal-surface and Command connections emit `client-size-v1:` whenever a
  caller terminal size is discoverable. Only the TUI terminal surface republishes later `SIGWINCH`
  changes through `ClientTerminalSize`. A local terminal surface and a local Command client emit a
  discoverable tty through `client-tty-v1:` regardless of nesting, while remote endpoints omit the
  caller-host tty. Since 2026-08-25 the
  additive `client-nested-v1` capability records a nonempty inherited `$TMUX` independently. The daemon requires both that
  marker and an exact pane-tty match for the pinned nested-attach refusal, so `env -u TMUX` forces
  attach without discarding the tty used by client targeting. The additive capability changes no
  field, tag, or protocol version. A local Control connection uses a narrower identity-only scope:
  it publishes `client-tty-v1:` only when stdin has a discoverable tty and publishes
  `client-nested-v1` only when `$TMUX` is nonempty. It does not inspect terminal size or emit
  `client-size-v1:`. Piped stdin therefore contributes no tty identity. Control geometry remains
  explicit `refresh-client -C` state and does not use `ClientTerminalSize`.
  The retained tty participates in the supported attached-client matcher as its full path or after
  exactly one leading `/dev/` removal, with exactly one optional trailing colon and no final-basename
  alias. It remains internal daemon state; `ClientFormatFacts` does not expose `#{client_tty}`.
  The local Control identity scope also does not publish TERM or terminal-name format facts.
  Client size also supplies `-x -`/`-y -` creation dimensions,
  `#{client_width}`/`#{client_height}`, and scoping the mouse-off input rejection to terminal
  surfaces.
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
  to sending it. A local Control client's tty and nested-intent tokens do not set
  `client-terminal-v1` or turn it into a terminal surface. Because the field is an already-shipped
  `Vec<String>`, adding a token changes no
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
  each entry in `visit_str` before `to_owned`. `ClientHello.environment` similarly caps count,
  individual entry length, syntax, and aggregate bytes before accepting the snapshot. `device_name`, status-line halves, and several other
  strings deserialize first and then reject on `len() > limit`. Frame lengths are validated against
  `MAX_FRAME_BYTES` before the payload buffer grows.
  `SetConfigOverrides` is additionally validated at no more than 1,024 single-line pairs, with
  nonempty keys of at most 128 bytes and values of at most 64 KiB. Each mux-option payload must be a
  complete 17-key map with values of at most 64 KiB; bounded value deserialization rejects an
  oversized string before it can become protocol state.
  `StartupConfigCauses` has a dedicated `InvalidStartupConfigCauses` validation path with a bounded
  sequence visitor: at most 1,024 causes, 64 KiB each, and 1 MiB total.
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

`ClientHello { protocol_version: 90, client_instance_id: ClientInstanceId(0), kind: Interactive,
device_name: None, capabilities: [], color_scheme: Some(Dark), origin: None,
working_directory: None, environment: [], process_id: 0 }` is 20 bytes on the wire: an 8-byte
envelope over a 12-byte postcard payload.

```text
byte  0  1  2  3 | 4    | 5     | 6  7        | 8  9 10 11 12 13 14 15 16 17 18 19
      10 00 00 00  00     00      5a 00         00 5a 00 00 00 00 01 01 00 00 00 00
      └ u32 LE ─┘  lane   flags   version LE    postcard payload
      length = 16  Control        (= 90)
```

- **length `16`** = `ENVELOPE_BYTES` (4) + payload (12); it counts the four envelope bytes, not itself.
- **payload** `00 5a 00 00 00 00 01 01 00 00 00 00`: variant `0` (`ProtocolMessage::ClientHello`),
  `protocol_version` as the varint `0x5a` (= 90), `client_instance_id` as varint `00`, `kind`
  variant `0` (`Interactive`), `device_name` as the `Option::None` tag `00`, `capabilities` as the
  sequence length `00`, `Option::Some` tag `01`, `TerminalColorScheme` variant `1` (`Dark`), then
  `origin` and `working_directory` as two `Option::None` tags (`00 00`), followed by the empty
  environment sequence length (`00`), then `process_id` as the varint `00`. Postcard
  writes multi-byte integers as LEB128 varints, so the version is one byte here and two in the
  envelope, which is fixed-width LE. A real local interactive hello may carry the device's short
  hostname, capabilities, origin pane, working directory, and environment.

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
`InvalidStartupConfigCauses`, `InvalidStatusLine`, `InvalidGuiRequest`, `InvalidPasteUpload`,
`InvalidAgentPayload`, `Io`.

## Handshake sketch

```text
client → ClientHello { protocol_version: 90, client_instance_id: i1, kind: Interactive, device_name: Some("laptop"),
                       capabilities: [], color_scheme: Some(Dark), origin: None,
                       working_directory: Some("/home/demo"), environment: ["TERM=xterm-256color"],
                       process_id: 1234 }
server → ServerHello { protocol_version: 90, server_id, client_id: c11, client_instance_id: i1,
                       capabilities: ["mux-v1", "terminal-viewport-v3", "terminal-row-patches",
                                      "terminal-appearance-v2", "config-overrides-v1", ...,
                                      "new-session-attach-v1"],
                       appearance, appearance_provenance, mux_options, status, key_tables }
client → SetConfigOverrides { entries: [("theme", "Catppuccin Mocha"),
                                        ("font-size", "13"),
                                        ("prefix", "C-a"), ("mode-keys", "vi"), ...] }
client → Attach { session: "" }      // attach current, or lazily create numeric 0 if empty
server → Attached { session: $0, snapshot: MuxSnapshot { generation, sessions, focused_window },
                    read_only: false, client_flags: "" }
server → Event { sequence: 1, payload: TerminalViewport { pane: %3, viewport } }   // Terminal lane
server → Event { sequence: 2, payload: MuxOptionsChanged { options } }             // Control lane
client → HistoryRequest { pane: %3, start: 1_488, count: 512 }
server → Event { sequence: 3, payload: HistoryChunk { pane: %3, start, total, offset, ... } }
...
(unusable patch)      client → RequestFull { pane: %3 }
(reconciliation error) client → Resync
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
