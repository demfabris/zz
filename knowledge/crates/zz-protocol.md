---
type: Rust Crate
title: zz-protocol crate
description: The stable, versioned wire vocabulary (IDs, framing, control messages, packed terminal lanes, and mux snapshots) shared by every zz client and the daemon.
resource: crates/zz-protocol/src/lib.rs
tags: [protocol, crate, wire, ipc]
timestamp: 2026-08-23T12:00:00-03:00
---

# Overview

`zz-protocol` is the
**renderer-neutral, versioned contract** that every zz process speaks. It defines the stable IDs
(`$session`, `@window`, `%pane`, `^split`, client `c`), the length-prefixed enveloped framing, the
`ProtocolMessage` control enum encoded with `postcard`, a hand-packed terminal fanout lane, and the
`MuxSnapshot` state tree. It deliberately owns **no** renderer, transport, or business logic; it is
the shared vocabulary that keeps [the mux state machine](/crates/zz-mux.md), [the daemon](/crates/zz-daemon.md),
and [the GPUI client](/crates/zz.md) interoperable across a socket or named pipe.

The crate is small and dependency-light: five dependencies, `postcard`, `serde`, `smallvec`,
`thiserror`, and `zz-terminal` (for `TerminalViewport`, `TerminalAppearance`, `PackedCell`, and
friends that ride the terminal lane). It has no cargo features at all since v43 retired `compress` and its optional
`zstd`. Because it encodes the wire format, **any encoding-affecting change requires bumping
`PROTOCOL_VERSION`**, currently 72. See [the wire protocol](/protocol/wire-protocol.md).

# What it exports

`lib.rs` re-exports seven modules' public surface:

| Module | Re-exported symbols (selection) | Documented in |
|--------|--------------------------------|---------------|
| `catalog` | `COMMAND_SPECS`, `CommandSpec`, `CommandOptionSpec`, `CommandValueKind`, `canonical_command`, `command_spec` | [commands](/tmux/commands.md) |
| `framing` | `MAX_FRAME_BYTES`, `MAX_ENCODED_FRAME_BYTES`, `ProtocolError` | [wire protocol](/protocol/wire-protocol.md) |
| `id` | `ClientId`, `PaneId`, `SessionId`, `SplitId`, `WindowId` | [stable IDs](/protocol/ids.md) |
| `key` | `Binding`, `KeyTables`, `KeyEngine`, `KeyDecision`, `canonical_key`, `input_key_name`, `input_typed_text` | [key tables](/tmux/key-tables.md) |
| `message` | `ProtocolMessage`, `Event`, `EventPayload`, `InputMessage`, `ClientHello`, `ServerHello`, `ServerError`, `ConfigOverrideEntry`, `MuxOptions`/`MuxOptionKey`/`MuxOptionValue`, `StatusLine`/`StatusPosition`, `CommandPromptType`/`CommandPromptMode`, `PROTOCOL_VERSION`, `NEW_SESSION_ATTACH_CAPABILITY`, `CLIENT_TTY_CAPABILITY_PREFIX`/`CLIENT_SIZE_CAPABILITY_PREFIX`, `SPLIT_RATIO_BASIS`, choose-tree / choose-buffer / display-panes types | [wire protocol](/protocol/wire-protocol.md) |
| `snapshot` | `MuxSnapshot`, `SessionSnapshot`, `SessionViewer`, `WindowSnapshot`, `PaneSnapshot`, `LayoutNode`, `Axis`, `BrowserDescriptor`, `AgentDescriptor`, `AgentProvider`, `EditorDescriptor`, `PaneKindSnapshot` | [snapshots](/protocol/snapshots.md) |
| `terminal_codec` | `encode_protocol_message`, `decode_protocol_frame`, `read_protocol_message`, `write_protocol_message`, and their `_into` buffer-reusing variants | [packed terminal lanes](/protocol/terminal-lanes.md) |

# One way to encode a message

`terminal_codec::encode_protocol_message` / `write_protocol_message` is the only encoder. It routes
`TerminalViewport`, `TerminalPatch`, and `CommandOutput{Some}` events to the compact **Terminal
lane** (hand-packed fixed-width sections) and everything else to the **Control lane** (`postcard`).
The [server](/crates/zz-daemon.md) uses this path for high-rate terminal fanout.

`framing` is private machinery: it owns the envelope (`Lane`, header write/parse, length prefix) and
`ProtocolError`, and exposes only the size constants publicly. A generic `encode_frame`/`decode_frame`
/`read_message`/`write_message` pair used to sit alongside it; it was deleted once `terminal_codec`
covered both lanes.

# The envelope and its flags byte

A frame is a little-endian `u32` length prefix followed by four header bytes: the lane tag, a flags
byte, and `PROTOCOL_VERSION` as a `u16`. Decoding rejects any version that is not an exact match, so
skew fails at the handshake rather than negotiating down.

The flags byte is **entirely reserved**. Encoders write `0x00` and the decoder answers any other
value with `ProtocolError::UnsupportedFlags`, so an extension cannot slip through unnoticed. Its one
former meaning, `0x01` for a zstd payload, was removed at v43 along with the QUIC writer that was the
only thing that ever set it; `compress_terminal_frame` and the `compress` feature are gone with it,
and a frame's payload is now always exactly what the lane's encoder produced.

# Handshake vocabulary

`ClientHello` carries the protocol version, a `ClientKind`, an optional `device_name` (the client's
short hostname, bounded at 256 bytes), a capability list, the client's color scheme, an optional
`origin` pane (`$ZZ_PANE`) so untargeted CLI commands resolve against the invoking pane, and an
optional 16 KiB `working_directory`. Local clients publish that path; SSH callers omit it because
their local path has no meaning on the daemon host. A local cwd that is not UTF-8 or exceeds the
bound is omitted instead of preventing the client from connecting.
`ServerHello` answers with the assigned `ClientId`, the daemon's own capabilities, resolved
appearance plus provenance, the effective `MuxOptions`, the rendered status line, and
`key_tables` (every live table, refreshed later by `KeyTablesChanged`). Both capability
vectors deserialize through one bounded visitor: at most 64 entries of at most 256 bytes, rejected
before the strings materialize. The capability-name constants here are
`NEW_SESSION_ATTACH_CAPABILITY` (`new-session-attach-v1`), `CLIENT_TERMINAL_CAPABILITY`
(`client-terminal-v1`), and the v71 value-token prefixes `CLIENT_TTY_CAPABILITY_PREFIX`
(`client-tty-v1:`) and `CLIENT_SIZE_CAPABILITY_PREFIX` (`client-size-v1:`); every other advertised
string is a literal in [the daemon](/crates/zz-daemon.md). None of them changes an encoding . `TERMINAL_ZSTD_CAPABILITY`
and `egress-v1`, the two that did, were removed at v43.

`MuxOptions` is the daemon-owned option surface: seventeen `MuxOptionKey`s, each with a value string
and a `MuxOptionSource` provenance. Postcard encodes enum variants by index, so keys are only ever
appended; the v71 tail is `Mouse` (published per recipient with the attached session's effective
value), `EscapeTime`, and `Prefix2` at tags 14-16, with `Prefix2` last defaulting to `None`.
`Predict` and `BrowserEgress` used to follow `HistoryTrickle` and are gone.

# Streaming repair and presence

The daemon coalesces per-pane frames under backpressure, so a client can ask for repair:

| Message | Direction | Role |
|---------|-----------|------|
| `ProtocolMessage::RequestFull { pane }` | client → daemon | Request a fresh full viewport after an unusable or superseded per-pane frame |
| `ProtocolMessage::HistoryRequest { pane, start, count }` | client → daemon | Pull a scrollback range into the client's local history ring |
| `EventPayload::HistoryChunk { pane, start, total, offset, columns, rows, dictionary }` | daemon → client | One self-contained chunk: packed rows plus the dictionary they index, stamped with the scrollbar totals it was cut against |

`EventPayload::Detached { session, by }` tells a client the daemon dropped its attachment. `by` names
the device that stole the session through `attach-session -d`; `None` means the session ended.
Nothing refuses a second attachment: one session accepts as many interactive clients as the user has
devices, and `ServerError` carries no `SessionAlreadyAttached` variant.

`MuxSnapshot` carries two per-recipient fields the daemon stamps for each subscriber:
`focused_window`, that client's own window focus, and `SessionSnapshot::viewers`, a
`Vec<SessionViewer>` of device name, focused window, and an `is_self` flag.
`MuxSnapshot::focused_window_for` falls back to the session's active window when the stamp is absent
or names a window that no longer exists, so removing a focused window needs no snapshot repair pass.

# Key files

| File | Role |
|------|------|
| `crates/zz-protocol/src/lib.rs` | Crate root; declares the seven private modules and re-exports the public API |
| `crates/zz-protocol/src/catalog.rs` | Canonical command names, aliases, descriptions, accepted options, and completion value kinds |
| `crates/zz-protocol/src/framing.rs` | Length-prefixed envelope, `Lane` tag, reserved flags byte, version check, `ProtocolError`, control-lane `encode/decode/read/write` |
| `crates/zz-protocol/src/key.rs` | Shared `KeyTables`/`KeyEngine` model, default pane and overlay tables, key folding, typed-text precedence, bind/unbind, and snapshots |
| `crates/zz-protocol/src/message.rs` | `PROTOCOL_VERSION = 72`, `ProtocolMessage` (including `RequestFull`, `HistoryRequest`, stable client identity, and the Agent runtime messages), bounded client working-directory context, durable chooser static-filter fallback state, `MuxOptionKey`/`MuxOptions` (seventeen keys, including the three agent adapter options and the v71 `Mouse`/`EscapeTime`/`Prefix2` tail), ordered configuration override entries, appearance provenance payloads, `Event`/`EventPayload` (including `TimedClientMessage` with its v71 `message_id`, `TimedClientMessageCleared`, `PrefixCancelled`, `KeyTablesChanged`, `HistoryChunk`, `Detached`, and the Agent payloads), `AgentPaneWire` plus `AgentGitSummary` and their validation, `InputMessage` (including `CancelPrefix` and `ClientTerminalSize`), hello/command/error/UI-state types and their byte bounds |
| `crates/zz-protocol/src/id.rs` | The `stable_id!` macro and the five sigil-prefixed `u64` newtype IDs |
| `crates/zz-protocol/src/terminal_codec.rs` | Terminal-lane packer/unpacker for viewports and patches, plus lane-selecting encode/decode entrypoints and validation |
| `crates/zz-protocol/src/snapshot.rs` | `MuxSnapshot` and the session/window/pane/layout tree it carries, including automatic-rename and retained-dead metadata, plus per-client window focus and `SessionViewer` presence |
| `crates/zz-protocol/Cargo.toml` | Deps: `postcard`, `serde`, `smallvec`, `thiserror`, `zz-terminal`. No features |

# Related

- [Wire protocol](/protocol/wire-protocol.md) . framing, versioning, and the `ProtocolMessage` schema.
- [Stable IDs](/protocol/ids.md) . the `$`/`@`/`%`/`^`/`c` identifier types.
- [Packed terminal lanes](/protocol/terminal-lanes.md) . the compact terminal fanout encoding.
- [Snapshots](/protocol/snapshots.md) . the `MuxSnapshot` state tree.
- Consumed by [the mux state machine](/crates/zz-mux.md), served by [zz-daemon](/crates/zz-daemon.md),
  and reconciled by [the GPUI app](/crates/zz.md).
- Terminal payload types (`TerminalViewport`, `PackedCell`, `TerminalAppearance`) come from
  [zz-terminal](/crates/zz-terminal.md).
- System context: [architecture overview](/architecture/overview.md), [process model](/architecture/process-model.md),
  [data flow](/architecture/data-flow.md).
