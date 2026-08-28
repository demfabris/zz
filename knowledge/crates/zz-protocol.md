---
type: Rust Crate
title: zz-protocol crate
description: The stable, versioned wire vocabulary (IDs, framing, control messages, packed terminal lanes, and mux snapshots) shared by every zz client and the daemon.
resource: crates/zz-protocol/src/lib.rs
tags: [protocol, crate, wire, ipc]
timestamp: 2026-08-28T00:00:00-03:00
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
`PROTOCOL_VERSION`**, currently 84. See [the wire protocol](/protocol/wire-protocol.md).

# What it exports

`lib.rs` declares eight private modules and re-exports their selected public API:

| Module | Re-exported symbols (selection) | Documented in |
|--------|--------------------------------|---------------|
| `catalog` | `COMMAND_SPECS`, `CommandSpec`, `CommandOptionSpec`, `CommandValueKind`, `canonical_command`, `command_spec`, `parse_tmux_options`, `parse_tmux_command_options`, `COMMAND_ARGS_PARSE_BEHAVES` | [commands](/tmux/commands.md) |
| `framing` | `MAX_FRAME_BYTES`, `MAX_ENCODED_FRAME_BYTES`, `ProtocolError` | [wire protocol](/protocol/wire-protocol.md) |
| `id` | `ClientId`, `PaneId`, `SessionId`, `SplitId`, `WindowId` | [stable IDs](/protocol/ids.md) |
| `key` | `Binding`, `KeyTables`, `KeyEngine`, `KeyDecision`, `canonical_key`, `input_key_name`, `input_typed_text`, `is_key_name`, `KeyEngine::handle_synthetic_any_with_repeat_metadata`, `KeyEngine::handle_transient_mode_synthetic_any` | [key tables](/tmux/key-tables.md) |
| `message` | `ProtocolMessage`, `CommandInvocation` with v84 typed command-block positions, `Event`, `EventPayload` including v81 `ControlCommandOutput`, v80 `StartupConfigCauses`, v79 command-output actor IDs, v77 `ControlCommandGuard`, and v78 `ControlSourceFile`, `ControlSourceFileEvent`, `InputMessage`, v83 `ClientHello.process_id`, v82 `ClientHello.environment`, `ServerHello`, `ServerError` including v76 `CommandParse`, `ConfigOverrideEntry`, `MuxOptions`/`MuxOptionKey`/`MuxOptionValue`, `StatusLine`/`StatusPosition`, `CommandPromptType`/`CommandPromptMode`, `PROTOCOL_VERSION`, `NEW_SESSION_ATTACH_CAPABILITY`, the terminal-fact constants exposed through `ClientHello`, `SPLIT_RATIO_BASIS`, choose-tree / choose-buffer / display-panes types | [wire protocol](/protocol/wire-protocol.md) |
| `snapshot` | `MuxSnapshot`, `SessionSnapshot`, `SessionViewer`, `WindowSnapshot`, `PaneSnapshot`, `LayoutNode`, `Axis`, `BrowserDescriptor`, `AgentDescriptor`, `AgentProvider`, `EditorDescriptor`, `PaneKindSnapshot` | [snapshots](/protocol/snapshots.md) |
| `style` | `StyledSegment`, `TmuxAlign`, `TmuxAttributeState`, `TmuxAttributes`, `TmuxColour`, `TmuxDefaultType`, `TmuxList`, `TmuxRange`, `TmuxStyle`, `TmuxWidth`, and the style and colour parsers | [wire protocol](/protocol/wire-protocol.md) |
| `terminal_codec` | `encode_protocol_message`, `decode_protocol_frame`, `read_protocol_message`, `write_protocol_message`, and their `_into` buffer-reusing variants | [packed terminal lanes](/protocol/terminal-lanes.md) |

# One way to encode a message

`terminal_codec::encode_protocol_message` / `write_protocol_message` is the only encoder. It routes
`TerminalViewport`, `TerminalPatch`, and populated `CommandOutput` events to the compact **Terminal
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
`origin` pane (`$ZZ_PANE`) so untargeted CLI commands resolve against the invoking pane, an
optional 16 KiB `working_directory`, protocol v82's bounded `environment` snapshot, and protocol
v83's `process_id`. Local
clients publish that path; SSH callers omit it because
their local path has no meaning on the daemon host. A local cwd that is not UTF-8 or exceeds the
bound is omitted instead of preventing the client from connecting. Both local and SSH-forwarded
connections carry their UTF-8-representable process environment because command execution happens
on the daemon host. Unrepresentable Unix names or values are omitted without substitution and
remain tracked with non-UTF-8 client paths.
The snapshot admits at most 4,096 valid `NAME=VALUE` entries, 16,367 bytes per entry, and 4 MiB in
aggregate. Names must be nonempty and contain neither `=` nor NUL; values may be empty or contain
additional `=` bytes. The client sorts and deduplicates before sending, while encoder and decoder
enforce the same limits. Debug output reports the environment entry count and numeric process ID.
A caller that cannot report a process uses zero.
`ServerHello` answers with the assigned `ClientId`, the daemon's own capabilities, resolved
appearance plus provenance, the effective `MuxOptions`, the rendered status line, and
`key_tables` (every live table, refreshed later by `KeyTablesChanged`). Both capability
vectors deserialize through one bounded visitor: at most 64 entries of at most 256 bytes, rejected
before the strings materialize. The top-level capability-name constant is
`NEW_SESSION_ATTACH_CAPABILITY` (`new-session-attach-v1`). `ClientHello` exposes the terminal-fact
names as associated constants: `CLIENT_TERMINAL_CAPABILITY` (`client-terminal-v1`),
`CLIENT_NESTED_CAPABILITY` (`client-nested-v1`), and the v71 value-token prefixes
`CLIENT_TTY_CAPABILITY_PREFIX` (`client-tty-v1:`) and `CLIENT_SIZE_CAPABILITY_PREFIX`
(`client-size-v1:`). Every other advertised string is a literal in [the daemon](/crates/zz-daemon.md).
None of them changes an encoding. `TERMINAL_ZSTD_CAPABILITY`
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

Protocol v79 adds `output_id: u64` to the existing `EventPayload::CommandOutput` tag 11. The daemon
stamps every real command-output frame, current resync, and close with one nonzero ID allocated from
its daemon-lifetime-global monotonic counter. A populated output with ID zero fails validation. Only
the zero-ID empty form means an authoritative resync with no live output. Populated frames keep the
Terminal lane and place the ID after `sequence`; closes and empty resyncs use the reliable Control
lane. This identity lets coalescing mailboxes and client reducers reject stale actor traffic without
adding a second output protocol.

Protocol v80 appends `EventPayload::StartupConfigCauses { causes }` at tail tag 49. The exported
limits are 1,024 causes, 64 KiB per cause, and 1 MiB total. `message.rs` bounds the sequence while
deserializing it, and `terminal_codec.rs` repeats all three checks in its ordinary message validator
through `ProtocolError::InvalidStartupConfigCauses`. The daemon sends this raw vector only to
Control; renderer clients use the existing command-output surface.

Protocol v81 appends `EventPayload::ControlCommandOutput { output }` at tail tag 50. It carries raw
foreground shell output to one Control client after that command's guard. The payload has no frame
flags or status bit; `control_mode.rs` owns LF termination, literal percent-prefixed lines, and the
rule that this event does not change Control retval.

Protocol v82 appends `ClientHello.environment`. The daemon retains one immutable snapshot for the
connection and removes it on unregister. Fresh session creation, attach, native attach, Control
attach, and selected-client switch use it to apply tmux's effective `update-environment` rules.

Protocol v83 appends `ClientHello.process_id`. The daemon retains it for `#{client_pid}` and removes
it on unregister with the rest of the connection facts.

Protocol v84 appends `CommandInvocation.command_blocks`. Config and Control parsers record the
zero-based positions of standalone unquoted command blocks while quoted brace text stays an
ordinary string. Alias expansion and key-table publication retain the positions. The
command-aware option parser applies the adopted callback rules to `bind-key`, `command-prompt`,
`confirm-before`, `display-menu`, `if-shell`, `run-shell`, `set-hook`, `set-option`, and
`set-window-option`. Every
`bind-key` positional accepts a typed block or string while `-T` and `-N` values remain strings. The two set commands accept a typed
block only at value position 1. `confirm-before` accepts either type for its one command positional
while `-c`, `-p`, and `-t` remain strings. The mux constructs every lexical typed block
recursively before validating its parent command name, callback type, or arity. One user-alias
layer travels independently down each recursive path; siblings do not share it, and an
alias-produced subtree disables another user-alias expansion. Nested `if-shell`, `run-shell`,
set-option, and `confirm-before` blocks print canonical names; empty blocks read back as `{  }`,
and physical internal group newlines print as ` ;; `. Stored `bind-key` and `set-hook` lists and
typed `if-shell`, `run-shell`, and `confirm-before` callbacks execute their constructed commands
without another user-alias lookup. Typed `if-shell` and `run-shell` callbacks preserve physical
groups: a failed group stops its remaining commands while later physical lines continue; string
callbacks remain one group. Typed `command-prompt` templates retain their structured prepared
command list through submission without re-expanding aliases. Its one template positional accepts
a typed block or string while `-I`, `-p`, `-t`, and `-T` values remain strings. Structured
substitution preserves leaf-argument boundaries against quote or semicolon injection. A string
template substitutes the raw source before a fresh parse and whole-result construction pass,
retaining the originating source path and line for failures. Both paths replace the first `%%` and
every `%1`; a trailing `%` quotes double quotes, backslashes, dollar signs, semicolons, and tildes.
Typed callbacks keep their physical groups,
while string templates and free input form one group. Prompt chaining and multi-answer `%2`
substitution retain their existing owner. `set-hook` and command-valued native set-option
deliberately apply a second construction stage. Without `-B`, `set-hook` accepts a typed block only
at value position 1; its hook name and extra positionals remain strings. With `-B`, every positional
lexically accepts either type, while `-B` and `-t` values stay strings. The mux still rejects `-B`
because it does not implement format monitors. Typed hook values normalize before built-in hook,
custom `@`, or forwarded option storage. Built-in hooks flatten physical groups during their second
construction pass, while custom `@` values retain textual ` ;; ` groups. Local hook-array creation
precedes empty append and runtime parsing, so an empty or failing local append shadows an inherited
global hook with an empty local array. `display-menu` walks its positionals as repeated NAME, KEY,
and ACTION fields. A nonempty NAME advances through a string KEY to an ACTION, which accepts a
string or typed block and resets the state to NAME. An empty NAME is a separator and leaves the
parser expecting another NAME. Values for `-b`, `-c`, `-C`, `-H`, `-s`, `-S`, `-t`, `-T`, `-x`,
and `-y` remain strings. Typed children construct before this parent type check, and accepted typed
actions print canonical commands in stored bindings. Incomplete item tails reach daemon runtime
validation. Runtime resolves the current or `-c` target client before completeness, so an
unattached command or initial Control reports `no current client`; initial Control uses a flag-0
`%error` and exits 1. Once attached, Control validates an incomplete group as `not enough
arguments` before its overlay no-op and returns a flag-1 `%error`; EOF after that frame exits 1.
Interactive menu ordering remains unchanged. The daemon strips the structural
wrapper only from a typed action before its fresh selection parse; a quoted brace action stays
literal. All nine behaving commands reuse the same
protocol v84 metadata. Eager whole-file source construction and its replay-channel placement
remain a separate parser contract.

`MuxSnapshot` carries two per-recipient fields the daemon stamps for each subscriber:
`focused_window`, that client's own window focus, and `SessionSnapshot::viewers`, a
`Vec<SessionViewer>` of device name, focused window, and an `is_self` flag.
`MuxSnapshot::focused_window_for` falls back to the session's active window when the stamp is absent
or names a window that no longer exists, so removing a focused window needs no snapshot repair pass.

# Key files

| File | Role |
|------|------|
| `crates/zz-protocol/src/lib.rs` | Crate root; declares eight private modules and re-exports their selected public API |
| `crates/zz-protocol/src/catalog.rs` | Canonical command names, aliases, descriptions, accepted options, and completion value kinds |
| `crates/zz-protocol/src/framing.rs` | Length-prefixed envelope, `Lane` tag, reserved flags byte, version check, `ProtocolError`, control-lane `encode/decode/read/write` |
| `crates/zz-protocol/src/key.rs` | Shared `KeyTables`/`KeyEngine` model, default pane and overlay tables, key folding, typed-text precedence, bind/unbind, synthetic `Any` dispatch with repeat and transient-mode handling, and snapshots |
| `crates/zz-protocol/src/message.rs` | `PROTOCOL_VERSION = 84`, `CommandInvocation` with typed command-block positions, `MAX_BROWSER_KEY_REPEAT = 9,999`, the client-environment and startup-cause limits, `ProtocolMessage` (including the request-correlated command-prepare tail, `RequestFull`, `HistoryRequest`, stable client identity, and the Agent runtime messages), `CommandRequest.prepared` plus typed prepared-command results, bounded client working-directory, environment, and process context, durable chooser static-filter fallback state, `MuxOptionKey`/`MuxOptions` (seventeen keys, including the three agent adapter options and the v71 `Mouse`/`EscapeTime`/`Prefix2` tail), ordered configuration override entries, appearance provenance payloads, `Event`/`EventPayload` (including v81 `ControlCommandOutput` at tail tag 50, v80 `StartupConfigCauses` at tail tag 49, the v79 `CommandOutput.output_id` field on stable tag 11, v77 `ControlCommandGuard` at tail tag 47 with command-frame flags and independent sticky status, v78 `ControlSourceFile` at tail tag 48 with typed raw-read and invisible-completion events, `TimedClientMessage` with its v71 `message_id`, `TimedClientMessageCleared`, `PrefixCancelled`, `KeyTablesChanged`, `HistoryChunk`, `Detached`, and the Agent payloads), `AgentPaneWire` plus `AgentGitSummary` and their validation, `InputMessage` (including `CancelPrefix`, `ClientTerminalSize`, and v73 `ClientFocus`), `ServerError::CommandParse` with its v76 tail tag 12, hello/command/error/UI-state types, and their byte bounds |
| `crates/zz-protocol/src/id.rs` | The `stable_id!` macro and the five sigil-prefixed `u64` newtype IDs |
| `crates/zz-protocol/src/terminal_codec.rs` | Terminal-lane packer/unpacker for viewports and patches, plus lane-selecting encode/decode entrypoints and validation |
| `crates/zz-protocol/src/snapshot.rs` | `MuxSnapshot` and the session/window/pane/layout tree it carries, including automatic-rename and retained-dead metadata, plus per-client window focus and `SessionViewer` presence |
| `crates/zz-protocol/src/style.rs` | Shared tmux style, colour, alignment, range, list, width, parsing, and serialization vocabulary |
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
