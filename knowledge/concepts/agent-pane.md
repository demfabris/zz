---
type: Concept
title: Native Agent pane
description: The daemon-addressable Agent pane, its daemon-owned ACP v1 runtime, flat transcript, approvals, session controls, and restore metadata.
resource: crates/zz/src/agent/controller.rs
tags: [agent, gpui, markdown, mermaid, acp, pane, sessions, persistence, keyboard]
timestamp: 2026-08-17T00:00:00-03:00
---

# Overview

The **Agent pane** is zz's third materialized pane surface alongside Terminal and Browser. It owns a
stable `%pane` identity in the mux, participates in split layout, focus, choose-tree, and sidebar
navigation, and hosts a live Agent Client Protocol (ACP) conversation in native GPUI controls.

The daemon owns the pane **and its ACP adapter child**, exactly as it owns a terminal's PTY. Clients
are viewports: they send prompts, answers, and settings as protocol messages, and they render a
transcript reduced from the stream the daemon fans out. A running turn therefore survives the GUI
closing, and two clients attached to the same session converge on the same conversation. Provider
traffic still never appears in the wire schema as typed ACP: every stream item crosses as an opaque
JSON blob with a byte cap, which the client deserializes into the shape its reducer already ate.

# Ownership and pane lifecycle

| Layer | Representation | Responsibility |
| --- | --- | --- |
| protocol | `PaneKindSnapshot::Agent(AgentDescriptor)` | Versioned pane identity plus `provider`, `cwd`, and opaque ACP `session_id`; introduced across v22–v32. The live wire version is `PROTOCOL_VERSION` (68) |
| mux | `PaneKind::Agent(AgentDescriptor)` | Stable layout leaf, targeting, titles, and validated restore-metadata updates |
| server | one ACP adapter child per pane, on its own thread | Spawns and owns the child (`zz-daemon/src/agent/host.rs`), auto-approves permissions, queues mid-turn prompts, captures bounded worktree summaries, journals standard session updates, adopts the session ID the adapter returns, and coalesces the stream onto a per-pane outbound lane (`fanout.rs`) |
| app | shared `Entity<AgentController>` | Reduces the daemon's stream into a flat transcript, tracks the per-pane replay cursor, restores selector preferences, normalizes pasted attachments, and turns user gestures into `AgentRequest` sends |
| view/UI | `AgentView`, `AgentTimeline`, `AgentEntry` | History picker, composer, cancellation, approval/auth actions, Markdown, reasoning, plans, tools, and Mermaid |

The daemon builds its agent runtime lazily, on the first agent pane it opens; that build also opens
and prunes the journal and prewarms the adapter package cache and the login-shell `PATH` snapshot.
The whole runtime sits behind `zz-daemon`'s `agent` cargo feature, which is on by default
(`default = ["daemon", "agent"]`); `zz-tui` and `zz-client-ffi` link `zz-daemon` with
`default-features = false` and never pull in `agent-client-protocol` at all.

`split-picker` still creates a runtime-free picker first. Choosing **Agent**, or issuing
`select-pane-kind -t %N agent`, materializes that leaf in place, preserving its `%pane` ID and split
geometry. On the local host, the picker passes `agent-working-directory` through
`select-pane-kind -c` when configured. That explicit directory wins. A remote host never receives
the desktop machine's configured path; its daemon resolves the picker donor's current terminal
working directory and records it in the new `AgentDescriptor`.

Materialization is gated on the `experimental-agent-pane` flag: while it is off (the default), the
mux engine rejects `select-pane-kind … agent` from every route . picker, palette, CLI, and
`mux.conf` bindings. The runtime flag is now the only gate a normal build has: `crates/zz/Cargo.toml`
sets `default = ["desktop", "agent-pane"]`, so a stock `cargo build` and every packaged release ship
the implementation. The `agent-pane` cargo feature survives as a build-size lever rather than a
release gate; dropping it still compiles, because `crates/zz/src/agent/mod.rs` is the facade whose
stub replaces the controller and view with inert stand-ins, and `config::agent_pane_enabled`
(`crates/zz/src/config/mod.rs:699`) folds `cfg!(feature = "agent-pane")` into the flag so the key
reads false whatever the file says. See the flag contract in
[app configuration](/configuration/app-config.md).

Only a terminal has a live working directory, so `MuxState::cwd_donor` decides which pane that
donor is: the split target when it is a terminal, otherwise the window's most recently focused
terminal (`Window::last_panes`), then layout order, then nothing. Splitting off a browser or an
agent therefore opens the new pane where the user last was rather than at the process default, and
a picker that outlived its donor re-resolves at materialization instead of losing the directory.
Terminal splits go through the same helper, so both paths follow one rule.

`AppView` reconciles visible `AgentView` entities by `PaneId`, while the shared controller retains
the complete set of Agent panes from every daemon session. Switching attached sessions therefore
drops only the inactive view entity, and the ACP session itself is untouched either way — it is not
the client's to drop. Removing the actual daemon pane is what ends a conversation: the host sends
`session/close` when supported (otherwise `session/cancel`), resolves pending permission requests as
cancelled, and stops that pane's agent process.

# Keyboard ownership

Agent keyboard input is app-local: the composer, completions, history search, and approval controls
own their keys. The one exception is the configured tmux prefix, which the window-root
[prefix claim](/crates/zz.md) intercepts before the composer and forwards to the daemon with the
Agent `PaneId` as command source, so `C-a h` leaves the composer no matter what. A key the daemon
passes back has no Agent sink and is dropped; everything else never crosses the mux protocol.

# ACP connection and session flow

The connection lives in the daemon (`crates/zz-daemon/src/agent/runtime.rs`), against
`agent-client-protocol` 2.0.0 (schema 1.5.0) and the stable ACP v1 schema — the crate speaks
protocol v1 by default and zz enables none of its unstable features. One `std::thread` per pane runs
`futures_lite::future::block_on(run_agent_connection(..))` — the ACP crate is runtime-agnostic
(`async-process` + `futures`, no tokio), so this matches the daemon's existing thread-per-pane idiom
rather than introducing a shared async runtime. Commands arrive on an `async_channel`; every item the
connection produces leaves through one sink that journals it and hands it to the fanout.

1. Read the descriptor's built-in provider and parse the `agent-command` (Codex) or
   `agent-claude-code-command` (Claude Code) mux option as either a shell-style
   executable/argument string or an `AcpAgentConfig` JSON object, then spawn that pane's child.
   `agent/environment.rs` `with_platform_environment` supplies a repaired `PATH` to the ACP child
   when the command did not configure one explicitly, so a daemon started from a launch agent, a
   login session, or `zz` itself can still resolve Homebrew, user-local, and version-manager
   executables without replacing an explicit command environment. Only stderr is sent to zz logs;
   stdout remains ACP JSON-RPC.
2. Send `initialize` with zz client information and generic session-config-option support. zz does
   not advertise provider `_meta` extensions, standard filesystem methods, or client-owned terminal
   methods. Agents must stay inside the capabilities in the handshake.
3. For each pane, prefer `session/load` when the agent advertises it and the descriptor has a
   session ID. Routing is installed before the load request so replay notifications are retained.
4. If load is unsupported or fails, create a new session in the resolved absolute working
   directory. The daemon adopts the returned opaque session ID and actual cwd into mux state
   itself (`adopt_agent_session`), then publishes the changed snapshot; the GUI's old
   `set-agent-session -t %N -c /path ID` round trip is gone.
5. Send prompts as text content blocks, plus one `ContentBlock::Image` per attachment. Images
   cross the wire as `AgentImage { format, data }` and become content blocks daemon-side, so
   `gpui::Image` never leaves the client. Prompt requests run concurrently with the runtime
   command loop, so cancellation and permission responses remain responsive during a turn.

zz does not crawl or decode Codex CLI's or Claude Code's private session files. Their ACP adapters
remain the authority over those local stores: zz discovers conversations through `session/list` and
asks the adapter to replay one through `session/load`. This preserves provider compatibility when
either CLI changes its on-disk layout.

ACP session IDs are treated as opaque, but rejected before persistence if empty, larger than 16 KiB,
or containing control characters. A stale daemon snapshot cannot overwrite a newly established
local session while the metadata command is still making its round trip.

The default provider processes are pinned to exact adapter versions . `@latest` would resolve the
dist-tag against the npm registry on every pane spawn, while an exact version spawns from the npx
cache and works offline. The first agent pane a daemon opens fires a background
`npx --package <spec> npm --version` per configured package to pre-populate that cache without
executing the adapters, and takes the PATH snapshot on that same thread
(`environment.rs` `warm_adapter_cache`) so no pane spawn pays for it; version bumps are deliberate
edits to `DEFAULT_AGENT_COMMAND` / `DEFAULT_AGENT_CLAUDE_CODE_COMMAND` in
`crates/zz-protocol/src/message.rs`:

```text
npx -y @agentclientprotocol/codex-acp@1.3.0
npx -y @agentclientprotocol/claude-agent-acp@0.68.0
```

## Where the agent keys live

Three of the four agent keys are **mux options** now, because the daemon is what spawns the adapter:

| Key | Owner | Default |
| --- | --- | --- |
| `agent-command` | daemon (`MuxOptionKey::AgentCommand`) | the pinned `codex-acp` line above |
| `agent-claude-code-command` | daemon (`MuxOptionKey::AgentClaudeCodeCommand`) | the pinned `claude-agent-acp` line above |
| `agent-auto-approve` | daemon (`MuxOptionKey::AgentAutoApprove`) | `on` |
| `agent-working-directory` | local client | unset; it feeds local pane creation only |

A user still writes all four in `zz/config`. The client's parser recognizes the three daemon keys and
partitions them into the ordered `daemon_entries` it pushes as `SetConfigOverrides`, so the file
keeps working unchanged; `set-option -g agent-command …` and `zz/mux.conf` reach the same place.
Both adapter commands are rejected empty and bounded at `MAX_AGENT_COMMAND_BYTES` (4 KiB).

```text
agent-command = my-agent --stdio
agent-claude-code-command = my-claude-agent --stdio
agent-working-directory = /absolute/project/path
agent-auto-approve = false
```

A launch-config JSON object — `{"command": …, "args": [...], "env": {"NAME": "value"}}`, the SDK's
`AcpAgentConfig` shape — is also accepted for explicit arguments and environment variables. The local
picker stores a configured working directory in the descriptor before the runtime opens, so that
value wins over a donor terminal. A persisted descriptor cwd wins on restore. Remote cwd values are
opaque to the desktop and are validated for absoluteness by the daemon that owns that filesystem.
Changing an adapter command option updates the spawn configuration for the next open or retry; it
does not replace running children by itself.

# How the conversation reaches a client

Ten `ProtocolMessage` variants go up and four `EventPayload` variants come down; the byte bounds and
exact shapes are in [the wire protocol](/protocol/wire-protocol.md). What matters here is the
delivery model.

Up: `AgentPrompt` (text plus encoded images, 6 MiB total), `AgentCancel`, `AgentUnqueue`,
`AgentRespondPermission`, `AgentSetConfigOption`, `AgentSetMode`, `AgentAuthenticate`,
`AgentSessionOp` (`List { cwd, cursor, replace }`, `New { cwd }`,
`Switch { session_id, cwd, additional_directories }`, or `Delete { session_id }`),
`AgentReplay { from_seq }`, and `AgentAcknowledgePromptRestore`.

Down: `AgentUpdates { pane, first_seq, items }` is one coalesced batch of JSON stream items;
`AgentState { pane, state: AgentPaneWire }` is the small typed pane state; `AgentLagged` says a lane
overflowed; `AgentSessions` is the JSON session-list reply.

- **Coalescing.** The fanout gathers items into 25 ms windows on one shared flush thread that parks
  whenever no pane has anything gathered, so a healthy client sees a few frames per second rather
  than one per token. A window longer than `MAX_AGENT_UPDATES_BYTES` (9 MiB) splits across frames; a
  *single* item that exceeds it is dropped with a log line rather than stamped, because a sequence
  spent on an unsendable item would read as loss to every client.
- **Sequencing.** The fanout mints the per-pane sequence, not the host: request replies leave the
  stream without spending one, and a journal replay synthesizes fresh items. A runtime replacement
  keeps that sequence moving forward and gives the new host generation a new token; the fanout drops
  output from the old child after the replacement boundary. The client keeps a cursor per pane
  (`MuxClient::agent_cursors`), drops anything at or below it, and treats a batch starting *past* it
  as a hole it cannot wait out, so it re-requests a replay instead of buffering. The ask is bounded:
  at most one outstanding replay per pane (`agent_replays_pending`), asked from the cursor itself
  since `fanout::replay` treats `from_seq` as inclusive, and cleared only when a batch lands
  contiguous with the cursor . the proof the hole closed. `AgentLagged` recovers through the same
  path. The controller keeps its own applied receipt (`PaneViewport::last_applied`) and skips items
  at or below it, so a replay-from-zero over a transcript that survived a re-attach reduces each
  item exactly once.
- **A dedicated lane.** Agent frames get their own `OutboundMailbox` slot, drained
  `reliable` → `command_output` → `agent` (one frame per pane per turn) → `terminals`. It is capped
  at `MAX_PENDING_AGENT_BYTES` (4 MiB) per pane, and overflow does **not** close the connection the
  way a reliable-lane overflow does: the pane's queued frames are dropped and a tiny `AgentLagged`
  marker is queued, so a slow client degrades to replay instead of dying.
- **Replay.** Each pane keeps a 16 MiB in-memory ring of encoded items. A replay inside the ring is
  served straight to the asking client. A replay older than the ring falls back to the journal. The
  lane emits its cached `Ready`, `SessionReset { restoring: true }`, the journalled updates, and a
  closing `SessionReady` with the current session and configuration as freshly numbered items. An
  ordered `StateSynced` item follows it, so the authoritative Running, Failed, or pending-permission
  state wins even though the mailbox drains reliable `AgentState` frames before agent frames. A
  late client restores adapter metadata and leaves the reducer in the daemon's current phase.
- **Visibility.** The heavy stream flows only to clients the pane is visible to — attached session
  plus that client's focused window, honoring zoom, the same derivation terminal frames use.
  `AgentState` goes to every client attached to the session, so badges and permission prompts work
  for a pane nobody is looking at. Attaching pushes `AgentState` for every agent pane in the session
  and a pane entering the visible set pushes its state; in both cases the *client* decides where to
  replay from.

# Driving a pane from outside: `agent-send`

`agent-send [-t %N] [--submit] [--context PATH[:START[-END]]] [TEXT]` is how a shell (or an agent
running inside another pane) puts work into an Agent pane. TEXT comes from argv, or from standard
input when it is omitted, which is the reason the verb exists: `git diff | zz agent-send`. The
CLI is the only process with a stdin, so it reads the pipe and re-sends it after a `--` so a payload
starting with `-` is not mistaken for a flag. A target that is not itself an Agent pane, including
the implicit current pane when `-t` is omitted, routes to its window's most recently focused Agent
pane via `MuxState::recent_agent_pane`, the same rule `send-last-output` uses; a pipe from a
terminal, or an nvim keymap, can address "the agent next to me" with no addressing at all.
`--context` prepends a `path:start-end` header and
fences the payload in a run of backticks longer than any run inside it. Payloads are capped at
`MAX_AGENT_SEND_BYTES` (1 MiB) and rejected before routing if they carry control characters other
than newline, carriage return, and tab, the same discipline that bounds ACP session IDs.

The two halves of the verb now sit on opposite sides of the wire:

- **`--submit` is the daemon's own business.** `deliver_to_agent` hands the text straight to the
  pane's host (`submit_agent_prompt`), which dispatches it or queues it behind the running turn. It
  works with only a TUI attached, or with nothing attached at all . the pane's runtime is what has
  to be alive, not a GUI. A pane the daemon has no runtime for answers `PaneExited`.
- **The draft is still the view's.** Without `--submit` the text is composer draft, and an Agent
  pane in another window has no `AgentView` at all, so the daemon publishes
  `EventPayload::AgentCommand { pane, request_id, command: ComposerAppend }` and parks the calling
  command thread until the GUI answers with `ProtocolMessage::GuiResponse` . the protocol's first
  client → daemon reply, added in v32 and shared with `capture-browser`. The wait is bounded (5 s)
  and a GUI disconnect fails its waiters immediately. The GUI answers from
  `AppView::drain_gui_requests`, driven by the mux observation rather than by `render`, so a
  minimized or occluded window (which schedules no frames) still applies the command and replies
  instead of letting the CLI sit out the timeout. Client-side the text lands in
  `AgentController::pending_composer`, keyed by pane independent of thread registration, and
  `AgentView::render` folds it into the `InputState` when a view exists . that fold happens at
  render because `InputState::set_value` needs a `Window`, which no observer has. Existing draft
  text is kept and separated by a newline, so a send never eats what the user was typing.

`send-last-output -t %N` reuses that delivery: it extracts a terminal pane's last completed command
and output from OSC 133 marks and appends it to `MuxState::recent_agent_pane`'s answer for that
window. Both verbs are daemon-side commands like `capture-pane`; see
[the command set](/tmux/commands.md).

# Workspace environment for the ACP child

An ACP child is additively given `ZZ_SOCKET` (the daemon endpoint, passed explicitly because a
`--socket` launch leaves it out of the daemon's own environment). Injection and the PATH repair
share one shape (`with_workspace_environment` at `environment.rs:109`, `with_executable_path` at
`:123`): take the agent's `AcpAgentConfig`, skip any name the user already configured through
`agent-command`, add the rest. A configured value always wins. So an agent inside a pane can run
`zz tools` and then `zz agent-send -t %5 …` with no MCP server; the CLI is the tool surface.

`AgentWorkspaceEnvironment` carries `pane`, `session`, and `socket`. The spawn config is built once
per daemon (`Shared::agent_spawn_config`, socket only); the per-pane identity is resolved when the
pane materializes (`agent_pane_spec` fills `AgentPaneSpec.workspace`) and merged over the config at
spawn (`adopt_pane_identity`), so every ACP child still sees its own `ZZ_PANE` and `ZZ_SESSION` and
can address itself (`zz agent-send` with no `-t`, `ZZ_PANE`-gated skills).

## The repaired PATH

A daemon started from a launch agent, a login session, or `zz` itself never runs the user's shell
init, so its `PATH` misses everything the shell shapes and the agent CLIs live exactly there. The
repair moved into the daemon with the rest of the runtime and is all-Unix: `executable_path` resolves
through the `login_shell` module on macOS and Linux (`crates/zz-daemon/src/agent/environment.rs:141`),
while Windows returns nothing because a launch there inherits the user's `PATH` already. It is
composed from three sources, in this order and deduplicated (`compose_executable_path`, `:366`):

1. The login shell's own `$PATH`, captured by running the user's `$SHELL` (falling back to
   `/bin/zsh`, `/bin/bash`, `/bin/sh`) with `-l -i -c`, then `-l -c` if that yields nothing . rc
   files that hang or `exec` a multiplexer when interactive still answer a non-interactive login
   shell. The probe carries `ZZ_RESOLVING_ENVIRONMENT=1`, writes into a temp file rather than a
   pipe, and brackets the value in `0x1e`/`0x1f` so startup noise is skipped and a torn capture is
   refused. It is killed at a 3 s deadline (`LOGIN_SHELL_TIMEOUT`, `:162`), but returns the moment
   the markers land, so init that blocks *after* printing costs nothing. The probe is spawned into
   its own session (`pre_exec` + `setsid`), and the app gives the spawned daemon its own session
   too rather than just a process group: an interactive shell that inherits a controlling tty does
   job control against it, and the stop signal lands on the whole background process group . which
   is how a daemon spawned from a terminal-launched app used to end up SIGSTOPped wholesale (pane
   frozen at "Starting", CLI timing out, the 3 s killer stopped along with everything else) the
   moment the first agent pane fired this probe. `ZZ_AGENT_LOGIN_SHELL=0`
   skips this step entirely; on macOS the fallback is then the system `path_helper` path. It is
   read in the **daemon's** environment now, not the app's, so exporting it in a shell that only
   ever launches the GUI no longer has any effect.
2. The daemon's own inherited `PATH`.
3. Node version-manager bin directories that exist on disk (`node_version_manager_bins`, `:328`):
   fnm's stable `aliases/default/bin` under `$FNM_DIR` and the three well-known roots, then
   `~/.volta/bin`, `~/.bun/bin`, `~/Library/pnpm`, `~/.local/share/pnpm`,
   `~/.local/share/mise/shims`, then every `~/.nvm/versions/node/*/bin` newest first. These land
   last on purpose: they are a fallback for what the shell never told us about, never a shadow over
   what it did. fnm's PATH entries are per-shell and nvm is a shell function, so a non-shell launch
   sees neither.

The whole result is a process-wide `OnceLock`, negative results included (`:177`), so a broken or
slow shell is probed once rather than on every pane spawn.

# Session history

When an initialized adapter advertises `sessionCapabilities.list`, the pane header exposes a
**History** picker. Starting a session is not one of the picker's controls: the header carries its
own icon-only plus beside History, gated on the connection accepting a prompt rather than on the
listing having loaded, so a new session never waits on a page of history. Every one of its verbs is an `AgentSessionOp` on the wire. `List` carries the cwd
filter, opaque cursor, and replace/append intent; `New` carries the selected cwd; `Switch` carries
the session ID, cwd, and additional directories; `Delete` carries the session ID. The daemon runs
them against the pane's adapter. A
listing comes back as `EventPayload::AgentSessions { pane, request_id: 0, result }` only to the
client that asked; the daemon carries that connection's `ClientId` beside the ACP request instead
of exposing it in the payload. Its default scope passes the pane cwd to `session/list`; **All projects** omits
that filter. Opaque pagination cursors are returned to the adapter unchanged. Listed records are
accepted only when their session ID is bounded and control-free, their cwd and additional roots are
absolute and within the wire byte limit, their additional-root count is at most 256, and their
optional title/timestamp metadata is bounded. The picker searches title, cwd,
and opaque ID locally and renders its result rows with a uniform virtual list, so a large provider
catalog does not inflate the normal transcript render tree. A provider page that cannot fit the
1 MiB reply bound returns a small `SessionListFailed` result instead of leaving History loading.

Explicit switching is allowed only from an idle pane with no unresolved permission request. zz
routes the selected ID to a staging buffer before issuing `session/load`; replay notifications do
not touch the visible thread while the request is pending. A successful response atomically resets
the reducer, applies the staged replay, adopts the selected cwd/configuration, persists the new
descriptor, and closes the former ACP session when `session/close` is supported. A failed load
discards its partial replay and route, closes the failed target when possible, and returns the pane
to ready with the former transcript and session binding intact. Starting a fresh session uses the
same post-success swap boundary.

The picker supports explicit refresh, cursor-based **Load more**, and new-session creation. Delete
is shown only when the adapter advertises `session/delete`, requires a second destructive
confirmation, and refuses to delete the active session. A session already bound to another pane
for the same provider cannot be opened twice. Its dense two-line rows reserve the first line for the
provider title and flex the cwd across the remaining metadata line instead of letting technical
fields collapse it. Updated timestamps are parsed from RFC 3339 and shown in the user's local time
with compact calendar labels such as **Today, 18:01**; opaque IDs remain searchable but are not
rendered. The narrower, headerless picker opens directly on its compact search and filter toolbar,
keeps a compact labeled **New** action at the same scale as the filter controls, and dismisses
through Escape or a backdrop click. Search refiltering listens only for content changes, so the
input's 500 ms caret repaint cannot reset selection or scroll. Async catalog updates preserve both
keyboard selection by opaque session identity and the user's viewport when that identity survives,
while only pointer movement can hand selection back to a hovered row. Its footer reuses the
tmux command palette's outlined keyboard-hint badges.

# Streaming reducer and rendering

The controller reduces the daemon's stream into provider-neutral entries with stable native IDs. Its
input is `AgentStreamPayload` (`crates/zz-daemon/src/agent/stream.rs`, re-exported from the crate
root so a client never redeclares the shapes): `Update` wrapping a raw ACP `SessionUpdate`, session
boundaries, permission requests, prompt boundaries, reclaimed prompts, and failures. The reducer
has five transcript shapes:

- `User { id, markdown, images }`
- `Assistant { id, markdown }`
- `Reasoning { id, label, markdown, default_expanded }`
- `Plan { id, markdown }`
- `Tool { id, protocol_id, kind, status, label, location, input, output, default_expanded }`

The reducer reads standard ACP v1 fields only. Provider `_meta` remains opaque and cannot change an
entry's type, title, status, nesting, animation, pane state, or journal lifecycle. A provider may
represent agent orchestration as an ordinary tool call; zz renders that call with the same flat row
as any other tool. Custom extension notifications do not enter the transcript.

Tool input and output use the provider-neutral `ToolPayload` variants `Diff { path, old, new }`,
`Text`, `Json`, and `Terminal`. ACP diff and terminal content stay typed instead of being serialized
as JSON containing full escaped files. Text content blocks remain text, while raw input, raw output,
and unsupported content become pretty JSON without Markdown fences. Structured content has priority
over `raw_output`, including when a later raw-output update arrives; raw output is displayed only
while the structured content collection is empty.

A tool payload lives as long as its pane, so the reducer caps what it retains:
`MAX_TOOL_PAYLOAD_BYTES` (512 KiB) on text and JSON, and `MAX_DIFF_SIDE_BYTES` (1 MiB) on each side
of a diff. `capped` backs up to a char boundary and appends `TRUNCATION_MARKER`
(`"… [truncated]"`), so the cut is visible in the payload rather than silent. These caps are in-memory
only; the journal writes the update as it arrived, except that adjacent text chunks of one message
coalesce into a single record before they land (see the persistence boundary below).

Only some updates may re-type a tool. `update_carries_tool_shape` accepts a title, raw input,
locations, or diff content as shape; `reclassifies_tool` then allows a
kind change only when the update carries shape, when the current kind is already `Other`, or when the
new kind is something better than `Other`. Without that gate a completion update repeating ACP's
default `other` kind would downgrade an already-typed tool into a generic one, and an empty title
would blank a good label.

Standard ACP terminal references render as static payloads. zz does not reconstruct terminal output
from metadata or advertise client-owned ACP `terminal/*` methods.

Message chunks coalesce by ACP `message_id`; chunks without one coalesce only while their role stays
active. The locally inserted user entry suppresses the matching streamed echo for that prompt.
Tool calls retain one native entry across incremental updates, map ACP kind/status fields, expose the
first affected location, and render structured output. Plan notifications replace one checklist
entry instead of appending duplicates. Session title, current mode, generic config, available
command, and usage updates feed the header/composer; a title update also becomes daemon pane-title
metadata. Prompt failure settles open tools to failed, while cancellation keeps the canceled local
state. A successful `PromptFinished` ends pane activity without fabricating a completed tool
status. Standard updates that arrive after the prompt response still update transcript data, but
they do not revive pane activity.

The daemon has already coalesced the burst, so the client's job is just to stay ordered: `MuxClient`
buffers whole batches per pane, `AppView::drain_agent_events` hands them over on a mux observation,
and `apply_stream_items` folds one batch into a single controller transaction and one pane
notification. Each thread maintains a stable ID-to-index map and a monotonic revision per entry.
The reducer remains flat.
`AgentView` folds top-level entries into presentation-only `TimelineRow` values and retains an
entry-index-to-row-index map. Tools and reasoning share one fold: an uninterrupted run of either
shares one row once it has at least two members. User, assistant, and plan entries still stand
alone and end the run. A member revision therefore remeasures its owning group row rather than its
flat entry index.
Appending another member updates the trailing group without a `ListState` splice, regardless of its
label or whether it is a tool or a thought; an append that starts a row splices exactly one row.
Tool label changes preserve the fold,
while a replacement that changes an entry's group kind, and other non-append mutations, take the
full reset-and-refold path. Same-length streaming updates continue through `Arc::make_mut` instead
of rebuilding the complete history.

For a changed existing entry, that same sync path updates any already-materialized body Markdown in
the store: a prefix extension goes through `TextViewState::push_str` for incremental background
parsing, while a non-prefix replacement uses `set_text`. Tool payload synchronization instead
compares the typed source with its materialized store entry; equal payloads retain the existing line
cache without splitting or diffing again. The render path never compares or replaces existing
Markdown state.

The transcript is a variable-height GPUI `ListState`, so only visible rows plus 1200 px of overdraw
are measured and rendered. Every row uses a centered 680 px content column with responsive side
padding; the composer extends one pixel beyond that column on each side. The composer floats over the
bottom of the full-height transcript without a section divider or a full-width underlay. Its card is
the only opaque surface, so content remains visible while scrolling beneath the floating composer.
The footer remains present because it holds the working-directory picker.
`composer_tail_clearance` aligns the settled transcript tail with the card's top edge rather than the
composer's outer padding, and the scrollbar uses the same cutoff. Controller updates for other panes
or unchanged pane metadata do not invalidate this view.

The pane-owned timeline store keys body Markdown and materialized tool content separately by entry
ID and slot, disclosure state by `(entry ID, disclosure kind)`, and expanded-tool uniform-list scroll
handles by entry ID. Reasoning, member-tool, and group disclosures therefore cannot alias even when
a group uses its first member's stable entry ID.
A never-seen virtual row still parses or splits lazily when it first becomes visible; once
materialized, its `TextViewState`, cached tool lines, disclosure choice, and tool viewport survive
frames where transcript virtualization does not render the row. Non-append timeline rebuilds and
entries-gone resets clear the entire store so reused provider IDs cannot alias state across session
switches. Markdown keeps the active light/dark syntax theme and process-stable extension registries.
Assistant-authored fenced `markdown`/`md` blocks are promoted to nested rich Markdown; this bounded
nested plugin is the sole remaining window-keyed Markdown state because it has no timeline-store
access. Other code fences remain literal.
Every literal fence reserves a compact top strip: the language from the fence, or `text` when none
was supplied, sits at the left and a copy icon at the right writes the complete raw code. Every
settled transcript exposes one response-level copy icon only after the active turn ends, attached to
the final assistant response; it writes the complete raw Markdown rather than the repaired or
rendered display text.

A second plugin recognizes fenced `mermaid` blocks and renders resvg-safe SVG off the UI thread with
`merman`. The renderer maps virtual UI font names to a conservative `system-ui, sans-serif`
measurement stack, feeds semantic colors through Mermaid's theme variables, and adds scoped SVG
overrides for variants that otherwise emit fixed light-theme styles. Gantt and XY charts use the
transcript's 640 px content width. Every diagram scales down to fit the transcript card rather than
overflowing it. Two ghost icon-only actions sit in the card's top-right corner, painting no fill until hovered so
they do not sit as two chips on the diagram: copy writes the original Mermaid source, and preview
opens the complete raster in a large scale-to-fit dialog. The resulting raster is stored in a 16-image global cache keyed by source, scale, font,
every semantic color used by the SVG theme, and appearance. Per-node state also includes the source
offset so distinct blocks do not alias, while identical diagrams can share a raster; Mermaid theme
configuration is built only on a cache miss.

User prompts use a neutral input surface. Tool
disclosures are compact, fixed single-line borderless rows with one kind icon, an ellipsized label,
and a chevron immediately after the label when details are available. There is no trailing status
glyph or ellipsis. Kind icons and chevrons use the same muted foreground regardless of ACP status;
the closed chevron points right and the open chevron points up. Reasoning, tool, and group headers
come from one `activity_row` builder, so their metrics cannot drift: a 28px row, UI font at 13px,
a 16px line box, and centred glyphs dropped half a pixel because gpui centres text on its
ascent/descent box while the ink a reader sees sits below that centre. Single-line enforcement
lives in code, since style cannot do it: `whitespace_nowrap` and `text_ellipsis` only govern
wrapping, while gpui shapes text by splitting on `\n` first and truncates by cumulative character
width without noticing the breaks, so a label with an embedded newline lays out two full-height
lines in a 28px row and the row's own overflow mask cuts both in half.
Agents send such labels routinely (one Bash call with two
commands arrives as one title with a newline between them), so every label bound for a fixed-height
row goes through `single_line`, which joins the pieces with ` · `. Clickable rows take a pointer
cursor and brighten from muted to the theme foreground on hover; the hover style hangs on the
stateful row rather than the label, since gpui only resolves a hover in time to reshape text when
the element carries an id. Pending and running rows never animate, so an
adapter can leave a status unresolved without leaving a spinner behind. Pane-level busy
chrome derives from the connection phase alone (`pane_is_busy`, `view.rs`), never from tool rows.
Pane activity, startup, and permissions share the leased pulse clock. No
mounted agent spinner starts a display-rate `gpui::Animation`. A run of two or more
adjacent tools and thoughts, regardless of label or action, renders one collapsed group header with
the first member's icon and a first-seen, count-aware action summary such as **Ran command, Reasoning,
Edit files**.
Opening a group reveals the original compact member rows, and each member independently controls its
content as a second disclosure level. Payload previews stay out of collapsed rows and remain
available only inside the expanded detail view.

Expanded tool payloads never enter Markdown or `TextView`. The store splits text and JSON into
monospace rows and computes ACP diffs once with `similar`; a single `uniform_list` for the tool
viewport shapes only visible lines. Diff rows include a path header, a `+`/`−` gutter, and subtle
success/danger line tints. Each payload retains at most 10,000 cached display lines and adds a muted
truncation row when longer. Text, JSON, and diffs retain the first 10,000 lines; a terminal retains
the newest 10,000 and moves its persistent inner scroll handle to the tail on each frame. The typed
source remains uncapped so the Input and Output section headers can offer **Copy** actions for the
complete raw payload. Machine rows are intentionally not text-selectable; copying the complete
section replaces the former partial-selection behavior of Markdown tool output.

Each expanded tool keeps its persistent uniform-list scroll handle and vertical scrollbar. Wheel
input pins the surrounding transcript to its live event-time offset while the inner viewport can
move, then chains back to the transcript at either edge; location-only and input-only tools remain
expandable.
Inline backtick spans use mixed font runs so surrounding prose stays in the UI family while code
uses the primary terminal family resolved from Ghostty. This is not agent-pane chrome: the marks
travel to `Inline` (`widget/text/inline.rs`), the one element every markdown run paints through, so
transcripts, settings, and docs views all render inline code the same way. The paragraph stays one
shaped run — split it into an element per span and the spans stop sharing a baseline, since
flexbox aligns boxes and has no inline layout — so `Inline` swaps the mono family into the runs it
already builds, then paints a rounded fill under the glyphs before painting them, reading span
positions back out of its own `TextLayout`. Each fill hugs the font's ink box rather than the
taller line box gpui's own run background fills, and a span that survives a wrap gets one fill per
visual line. The fill uses the neutral muted surface instead of the accent color, while nested
bold, italic, strikethrough, and link styling is retained. Paragraphs that mix an image with text
take the inline-flow path, which re-slices runs per fragment and still paints the older square
run background for code.

## Following the tail

gpui's own `FollowMode::Tail` is deliberately *not* how the transcript follows output. It snaps on
every layout and re-engages itself from scroll position, which makes a deliberate scroll-up
impossible to hold while the agent is still writing. `TimelineStick` (`zz-ui/src/agent.rs:734`) owns
the pin instead, leaving the list in `FollowMode::Normal` and driving a critically-shaped
`StickSpring` toward the end. Tail mode survives only as the reduced-motion fallback: `engage_now`
(`:818`) sets it when `cx.reduce_motion()` is on, and the whole spring driver is skipped.

The pin breaks on wheel input alone. `on_user_scroll` (`:859`) is the only path that can release it,
and the list calls its scroll handler from its input path only, so content growth never reaches
there . a burst of output cannot scroll the reader away from where they parked. Re-sticking is
direction-aware: `agent_should_restick` (`:628`) requires both being inside a 70 px band of the end
*and* moving toward it, because a small wheel-up notch from the bottom is still inside the band and
would otherwise make the pin impossible to break. Once unpinned and more than 320 px from the end, a
jump pill appears; clicking it re-engages the spring. The spring chases a lead point above the
target, scaled by an EMA of how fast the target is growing, and teleports whatever remains beyond
2.5 viewports, so a long replay lands rather than scrolling through the whole history.

Spinners and streamed-text fades share one clock. A mounted repeating `gpui::Animation` pins the entire window to display
refresh rate, because any notify repaints the whole window; `pulse.rs` instead runs a single ~30 fps
tick and notifies only its leaseholders. A lease is taken through `pulse_phase` or `pulse_lease` and
lapses 300 ms later . there is no release call . and the loop parks itself once the lease set
empties. Pane-level activity, approval controls, and an active text veil lease it; transcript tool
rows do not. Reduced motion returns phase `0.0` and schedules nothing.

While an entry is still streaming, its *display* copy is repaired by `zz-ui/src/mend.rs`. Markdown
arriving a token at a time is briefly ill-formed . a half-written `**bold`, an inline span with one
backtick, a link whose URL is still landing . and each reflows the paragraph when it completes. `mend`
closes hanging emphasis, strong, strikethrough, and code spans innermost-first, and only in the last
top-level block. A half-streamed link is the interesting case: rather than closing it with a literal
`)`, the URL is replaced with the `PENDING_LINK_URL` sentinel `zz:pending-link`, so the label styles
immediately and the settling URL cannot reflow the line; `resolve_workspace_link` maps that sentinel
to an inert `data:,` href, so the link is never navigable. The repair is display-only: the store
keeps the raw `source` beside the rendered `TextViewState`, exactly one entry is mended at a time,
and `settle_markdown` snaps the display copy back to the raw text when the entry stops streaming . a
completed entry always renders exactly what it holds.

Streaming does not type characters or hold a second text queue. `widget/text/veil.rs` places each
parsed suffix into `TextView` immediately, tracks the changed rendered byte range by source-span and
text-segment identity, and fades only that range's `TextRun` colors. Arrival-gap EMA sets each fade
to three times the observed cadence, clamped to 120–400 ms with overlapping chunks accelerated; the
shared leased clock advances it at about 30 fps and parks after the last range settles. Reparsed
Markdown preserves the common rendered prefix, the initial visible content of a reattached stream
is seeded at full opacity, settling clears all veil state, and reduced motion continually seeds the
current full-opacity baseline. The mixed text-and-image inline-flow path stays unanimated because
its wrap fragments do not have stable identities across reflow.

# Composer, permissions, and failure states

The native composer is a centered, bordered input card that auto-grows from two to eight lines.
Enter sends when the session is ready; Shift+Enter inserts a newline. The controller returns to ready
on ACP completion and marks in-flight tools cancelled when the stop reason is `cancelled`.
Send is disabled on an empty draft, not just on a busy connection, so the primary control never
invites a no-op. The Send, Queue, and Stop states share one 28 px icon-only button declared with
`rounded_full()`,
so it stays a true circle instead of inheriting the widget radius or squircle smoothing. The
jump-to-latest control is its mirror: a 26 px `rounded_full()` disc carrying the arrow-down twin of
Send's arrow-up, floated over the timeline at `composer_tail_clearance()` so it clears the composer
card rather than sitting on it, and labelled by tooltip alone. A separate
compact footer under the card carries the current Git branch and `N files +A -D` summary on the
left; its right side holds the 16 px context-usage ring and working-directory picker. The ring fills
clockwise, exposes exact token counts and percentage in its tooltip and accessibility value, and
clamps malformed over-capacity values to a full circle.

One button carries three actions, chosen by `composer_action(active_turn, has_content)`
(`view.rs:236`): **Send** while the pane is idle, **Queue** when a turn is running and the composer
has content, **Stop** when a turn is running and it does not. Send and Queue are the same click .
both call `submit()` . and both put the same `AgentPrompt` on the wire, because the *daemon* is what
branches: a prompt arriving while the pane is not `Ready` is pushed onto that pane's FIFO instead of
dispatched, and is never rejected. `AgentController::prompt` (`controller.rs:2760`) only bumps its
local queue count so the chip stays honest between the daemon's publications. Only Stop is different:
it sends `AgentCancel`. It is also deliberately unreachable from the keyboard . Enter over an *empty*
composer confirms the highlighted permission option rather than killing the turn it just started.

The queue's invariant is at-least-once, and it is enforced daemon-side now, so it survives the client
that typed it: a prompt is either sent or handed back, never silently dropped, images included. The
host's `dispatch_next` pops the front prompt after the active `session/prompt` request returns.
`reclaim_queue` emits `PromptsReclaimed` on cancel, on a lost runtime, on an
unexpected child death, and for the remainder after a failed dispatch. The composer refills from that
stream payload. While anything is queued the composer shows an "N queued" chip whose click sends
`AgentUnqueue` and hands it all back. `AgentPaneWire.queued_prompts` carries the depth to every
attached client, so a second device sees the same chip.

The first prompt of a session names its pane, and both sides do it. The daemon derives a title in
`fanout.rs` `derive_pane_title` and applies it through `title_agent_pane`, which writes the pane title
only while the pane still wears the default `agent` name . so a headless or TUI-driven prompt names
its pane too. The client keeps its own `derive_pane_title` (`controller.rs:3761`) and
`name_pane_after_prompt` (`:2821`) for the pane it drives, still routed as an
`AgentControllerEvent::Title` → `select-pane -t %N -T <title>`. Both take the first line, strip
quote/heading/emphasis dressing, and keep 7 words and 48 *characters* (never bytes, so CJK survives);
both refuse to rename something already named, so the two cannot fight.

Pending permissions render as a wizard, one request per page in arrival order (`PermissionWizard`,
`view.rs:261`). Keys 1-9 answer option indices 0-8 directly, Enter confirms the highlighted option,
and Escape cancels the page; the footer reads `1-9 picks · enter confirms · esc cancels`, and the
`n/m` counter appears only when more than one request is pending. Options past the ninth render
without a digit chip and are click-only. Digits are left to the composer while the user is
mid-sentence and focused, and the whole key path sits behind the History picker,
and the completion menu, so a wizard never steals a key one of those owns. Because auto-approve
answers kinded requests before they reach `pending_permissions`, the wizard is what a user sees with
`agent-auto-approve = false`, or when an agent advertises no allow option at all.

A surfaced request parks in the daemon with no timeout . a human decides . and rides
`AgentPaneWire.pending_permission { request_id, payload }` (the tool call and its options as JSON,
bounded at 64 KiB) out to every client attached to the session, so a client that attaches *after* the
agent asked still sees the question. The answer goes back as `AgentRespondPermission { request_id,
option_id }` with `None` meaning cancel; the first answer wins and a late one from a second device is
a no-op rather than an error. Closing the pane or losing the adapter resolves everything outstanding
as cancelled.

Pasting an image attaches it to the draft. A text field has nowhere to put one, so `InputState`
forwards it as `InputEvent::PasteImages` instead of dropping it, and the composer holds the
attachments (a draft is the view's, not the controller's) as a wrapping thumbnail strip above the
input, each with the control that removes it. An image alone is a whole prompt, so submit no longer
requires text. `agent::attachment::normalize` runs once on paste: SVG is refused, anything outside
png/jpeg/gif/webp is transcoded to PNG (macOS pasteboards routinely offer only TIFF), and anything
past a 1568px long edge is scaled (1568px is the ceiling Claude applies anyway, so the original only
costs upload). Because it runs once, the composer's thumbnail, the transcript's, and the base64 on
the wire are the same bytes. The prompt travels as `session/prompt` content blocks: the text first, then one
`ContentBlock::Image` per attachment, inline rather than as a path since a screenshot never had a
file and the agent may not share our filesystem. Attaching is gated on the `image` prompt capability
from the handshake. Claude Agent ACP advertises it; an agent that does not is refused at paste
time rather than mid-turn. Sent images render in the user's own message, and because an inbound
`ContentBlock::Image` decodes back into the entry, they survive a `session/load` replay instead of
degrading to an `*[Image: image/png]*` placeholder.

An attachment renders as a fixed square tile (140px in a message, 56px in the composer) with
`ObjectFit::ScaleDown` fitting the picture inside without ever enlarging it, and a click opens it in
a dialog large enough to read. The square is the sizing rule: layout never consults the image, so
it cannot depend on when the decode lands, and a tile measures the same whatever was pasted into it.
Sizing a thumbnail from the image is what broke before: one axis plus a `max_w` bounds the image and
not its parent, because taffy drops max sizes while measuring a node's content contribution
(`SizingMode::ContentSize`), so a wide screenshot painted clamped inside a bubble measured against
its *unclamped* aspect width; `items_end` anchors the bubble's right edge, so the overhang ran off
the pane's left edge where the text was unreadable. The bubble is also capped at 100% of the content
column rather than a number of its own, which keeps it inside a column narrower than
`AGENT_CONTENT_MAX_WIDTH`.

Restoring a session has to undo an agent's own lossy replay. Claude Code replays an attachment as a
`ContentBlock::Image`, which decodes straight back. Codex does not: it stores the attachment
faithfully as `{"type":"input_image","image_url":"data:image/png;base64,…"}`, but `codex-acp`'s
`userInputToContentBlocks` turns an image input back into text (`[@image](data:…)`), so a restored
transcript showed a link where the picture had been, and clicking it asked macOS to open a URL a
megabyte long. `split_inline_images` lifts `data:` image URIs out of replayed user text, taking the
Markdown link they sit in with them, and anything undecodable stays as prose rather than being
silently swallowed. Independently, the Markdown renderer refuses to hand `data:` and `javascript:`
links to the platform opener, since neither means anything to it.

One clipboard limit comes from gpui: every platform's clipboard read returns the first kind it
finds, text before images, so an image copied from a browser (which also offers its URL as text)
pastes as that text. Screenshots and image-only boards are unaffected.

The provider picker lives in the pane header. It offers Codex and Claude Code under their vendors'
marks (the OpenAI and Claude glyphs, Simple Icons artwork beside the Iconoir set that `zz-ui`
otherwise ships, since Iconoir draws no vendor logos), is disabled during an active turn, and starts
a fresh provider-bound thread on selection. The mux persists the choice via `set-agent-provider`,
clears the old opaque session ID, and replaces the daemon-owned ACP child. Retry uses the same
restart effect. Each replacement receives a new runtime generation, which prevents late output from
the old process from entering the new stream while preserving the pane's monotonic wire sequence.
The one retiring-generation payload still accepted is `PromptsReclaimed`, because it is the only
copy of queued text and images that must return to the composer.

The header's other end holds one square icon-only History control. The
working-directory picker lives at the right end of the composer footer: the pane's cwd by its last
component, its full path in the tooltip, and a native folder chooser behind a click on the local
host. It is disabled on remote hosts because a native chooser sees the desktop filesystem, not the
daemon's.
An ACP session is bound to
the directory it was created in, so choosing another one is `session/new` there rather than an
in-place move (the same boundary the History picker's **New** crosses), gated the same way on an
idle pane with no unresolved permission request. The pane's `cwd` is left alone until the agent
answers, so a failed switch keeps the pane where it was. The header carries no connection badge:
liveness reads off the composer, whose action morphs to Stop or Queue during a turn, and off the
error cards.

ACP `AvailableCommandsUpdate` notifications drive the command-completion popover. `/` opens it at
the start of a line or after whitespace for every provider. Every matching item advertised by the
agent remains available in a bounded, vertically
scrollable menu backed by a uniform virtual list, while only six rows are mounted at once. Up/Down
actions are captured before the multiline input so they select a result and keep it visible; Tab or
Enter accepts, Escape dismisses, and unstructured command input hints remain visible after
insertion. The description column truncates to the available width.
Ellipsis-only ACP description placeholders are suppressed while meaningful descriptions remain
visible. The menu closes once the user begins the command argument.

The composer renders generic ACP `Select` options categorized as `Mode`, `Model`, and `ThoughtLevel`
as the permission, model, and effort pickers (`ModelConfig` and `Other` categories are parsed and
stored but never rendered; `Boolean` config options are discarded at `config_option_model`). It sends each opaque config ID/value through
`session/set_config_option` and adopts the complete option vector returned by the agent. When an
older agent supplies no generic config options, the permission picker falls back to legacy
`SessionMode` and `session/set_mode`. The compact triggers use the same text scale as their entries,
and each popover opens directly on the available choices without a redundant category heading.
Neither provider's concrete values are hard-coded in zz.

Successful user selections for model, effort, and permission mode are sticky. They are stored in a
bounded, versioned `agent-preferences.json` under zz's platform application-data directory, with
user-only directory/file modes on Unix. Keys include provider, initialized agent identity, absolute
cwd, semantic setting kind, and the agent's opaque option ID, preventing one adapter or workspace
from applying values to another. On every new or loaded session, advertised choices are reconciled
serially in model → effort → permission order. Missing or stale choices are ignored; a rejected
restore is skipped for that session instead of looping. Legacy `SessionMode` permission values use
the same mechanism even when unrelated generic options coexist. Provider updates and individual
tool-approval answers are never persisted as user preferences.

An ACP `session/request_permission` request retains its exact responder and option IDs. The UI renders
every agent-supplied allow/reject choice plus an explicit cancel action. Closing a pane, cancelling a
turn, or shutting down responds `cancelled` to every outstanding permission before dropping the
connection. Authentication methods advertised during initialization appear beside retry when a
session fails, and send ACP `authenticate` requests.

`agent-auto-approve` (a mux option, default `on`) answers a *kinded* request in the daemon, without
waiting for the user or for a client to be attached. `preferred_allow_option`
(`zz-daemon/src/agent/runtime.rs:1288`) takes `AllowAlways` and falls back to `AllowOnce`, never a
reject kind and never an empty option ID; a request that advertises no allow option falls through to
the interactive UI rather than being silently denied. The tool call is still published as a
`ToolCallUpdate` on the stream before the response goes back (`runtime.rs:498`), so an auto-approved
action appears in the transcript exactly like one the user waved through . and if publishing it
fails, the request is answered `Cancelled` rather than approved unseen. Turning the option off, or an
agent that supplies no allow option, routes every request through the wizard.

The one shape auto-approve refuses is a *question*: `is_user_question` (`runtime.rs:1273`) treats
any option kind outside `{AllowOnce, AllowAlways, RejectOnce, RejectAlways}` as a prompt for the user
rather than a tool approval. Repeated allow kinds are deliberately not that signal . codex-acp sends
two `allow_always` options on every exec approval. This branch is forward-compat only and currently
unreachable: ACP v1 types `PermissionOptionKind` as a closed enum, so a question-shaped kind is
rejected at JSON-RPC deserialization before it can reach the check, pinned by
`acp_v1_rejects_permission_options_with_an_unknown_kind` (`controller.rs:5592`), which stayed on the
client side as the pin on why the daemon's branch is unreachable.

Launch, initialize, new/load session, prompt, and authentication failures become pane-local error
cards. Retry appears only when the connection itself is Failed; setting and history errors do not
restart a healthy adapter. Retry keeps the failure visible until the daemon publishes the new
runtime's Starting state, then replaces the failed runtime without replaying a dead host. Unexpected
process exit does not spin in an automatic restart loop. Local input errors use the same card
surface without showing runtime retry or authentication actions. The
runtime card text comes from the child's own
stderr: `StderrTail` (`zz-daemon/src/agent/runtime.rs:194`) keeps the last 6 non-empty lines, each
capped at 700 bytes and the join capped again, and `runtime_failure_message` (`:223`) folds that tail
and the parsed exit status into `"<adapter> exited unexpectedly (<status>): <tail>"`. A missing
adapter or a node version that cannot run it therefore reads as the shell's own complaint instead of
a bare exit code. The message reaches clients as the `Failed { message }` phase on `AgentPaneWire`,
so a client that was not attached when the child died still gets the real reason. **Daemon**
shutdown, not app quit, is what closes the sessions: it sends `session/close` where the adapter
supports it and cancellation otherwise, resolves approvals, and lets the ACP child guard reap.

The `session/prompt` response is the sole normal turn boundary. The daemon keeps the pane Working
until that request returns, the user cancels it, or the adapter fails. It does not infer idleness
from silence, unresolved tool calls, provider notifications, or `_meta`. An adapter that never
returns keeps the pane Working until the user stops or retries it.

Standard session updates may arrive after the prompt response because ACP v1 has no notification
flush barrier. The reducer applies those updates to transcript data without reopening the turn.

## Workspace footer

The composer footer shows a bounded current-worktree summary, not a claim about what one turn
changed. `git_summary.rs` points `GIT_INDEX_FILE` at a throwaway index, runs `git add -A`, writes a
tree, and compares that tree with `HEAD` through name-status and numstat. This includes tracked and
untracked files without touching the real `.git/index`; binary files contribute to the file count
and zero line totals. An unborn repository compares against Git's empty tree. A detached checkout
keeps the counts but has no branch label.

The daemon starts this capture after `SessionReady`, `SessionSwitched`, and `PromptFinished`. It
publishes the session or turn boundary first, starts a named background worker, and dispatches any
queued prompt without waiting for Git. A runtime generation, refresh token, and cwd check discard a
late result after a replacement, newer capture, or workspace switch. Each Git process has a five
second timeout and a 2 MiB output ceiling. Outside a worktree, or on any capture failure,
`AgentPaneWire.git` is `None` and the Git portion of the footer stays absent. Running the capture in
the daemon is what makes a remote pane summarize the remote worktree rather than the desktop's.

# Attention outside the pane

An agent pane is usually not the pane you are looking at, so its state has to be legible from the
chrome. `AgentController::pane_status` (`controller.rs:2498`) buckets one pane into `NeedsInput`,
`Failed`, `Working`, or `Idle` in that precedence . the sidebar reads it per pane rather than cloning
whole thread states, and `attention()` folds the same classification fleet-wide.

`agent/sound.rs` turns those buckets into edges. `AgentAttentionTracker::observe` is the single pass
that computes both the chime and the badge, so they cannot disagree: a chime fires on any transition
*into* `NeedsInput` (Request) and on the exact `Working → Idle` edge (Done), at most one per batch
with Request outranking Done, and never for the pane the user is currently watching. A pane's first
observation only seeds the baseline. `AgentBadge` adds a fourth state the status enum has no room
for . `Finished`, an idle pane whose completion has not been looked at yet . cleared the moment that
pane becomes the watched one.

The badge is a 5 px dot on the node's icon in the sidebar tree (bottom-right, so it coexists with the
pending-bell dot at top-right) and on the leading corner of the fixed-size strip chips. It bubbles
like the bell does: a collapsed host, session, or window carries the most urgent badge hidden beneath
it, merged by rank (`NeedsInput` < `Failed` < `Working` < `Finished`). Colors come from the theme:
warning, danger, muted foreground, success.

The chimes are synthesized rather than shipped . a two-note WAV built in code, materialized once per
process into the temp directory and handed to `afplay` on macOS or the first of `paplay`, `pw-play`,
`aplay` that spawns on Linux. Other platforms play nothing. `ZZ_AGENT_SOUND=0` disables them, and
only that exact value does. `agent/sound.rs` is compiled unconditionally, even without the
`agent-pane` feature, because the workspace chrome renders badges in every build . without the
feature the status map is simply always empty.

# Persistence boundary

The mux still persists only `AgentDescriptor { provider, cwd, session_id }`, not messages, tool
payloads, credentials, or provider state. What changed is the runtime around it: the ACP child is a
daemon-lifetime process now, exactly like a PTY. Closing every GPUI window, detaching, or quitting
the app leaves the turn running; reattaching replays the transcript and tails the live stream. The
remaining difference from a terminal is the same one browsers have . nothing survives the *daemon*.
A daemon restart takes the pane and its adapter with it, and there is no on-disk mux state to
resurrect them from.

`session/load` is preferred where it exists, but it is no longer the only durability. `AgentJournal`
(`zz-daemon/src/agent/journal.rs`) appends every inbound `session/update` verbatim to a per-session
JSONL file under `<data>/zz/daemon/agent-journal` . a directory the daemon owns, deliberately
separate from the GUI's `<data>/zz` so the two never collide on one machine . with user-only
directory and file modes. Adapters own the session-ID string, so it is jailed into a file stem before
it reaches the filesystem: everything outside `[A-Za-z0-9_-]` becomes `_`, and an id that was altered,
empty, or longer than 96 bytes is truncated and tagged with an FNV-1a digest of the original so two
hostile ids cannot land on one file. Current records use `{"seq": n, "update": …}`. Readers accept
legacy `{"seq": n, "task": …}` rows and skip them, preserving later standard updates and their
sequence order. `seq` counts from 1;
a crash mid-write leaves a torn trailing line that readers skip and the next append isolates behind
a fresh newline. Token-level streaming is collapsed before it lands: adjacent
`agent_message_chunk`/`agent_thought_chunk` updates with one coalescing identity (same kind,
`messageId`, and `_meta`, plain-text content only . anything unrecognized is written verbatim,
since a wrong merge costs a transcript where a refused one costs records) merge into a single
record holding one seq, reserved at first-chunk arrival with its byte budget charged up front, so a
flush can never overshoot the cap. The open record is bounded at 256 KiB and flushed by any
non-matching update, a replay, handle eviction, or drop; a daemon killed first loses only that
message tail, the same class as the torn line. A realistic session journals ~50× fewer records this
way. A session is capped at 18 MiB and 4,096 records, past which appends are *refused* rather than
rotated . a truncated head would replay a conversation that never happened . and at most 16
descriptors are held open. Journals untouched for 30 days are pruned once, when the daemon builds
its agent runtime (`load_persistent_journal`, `runtime.rs:268`).

The journal now serves two readers. Daemon-side it restores a transcript when the agent advertises no
`session/load` and again when a load request fails, through the staged replay path that shows
RESTORING rather than flashing STARTING at a transcript about to appear; the restored updates are
re-journalled under the new live session ID and the superseded file removed, so a chain of adapter
respawns leaves exactly one journal per conversation. Fanout-side it is the floor under the replay
ring: a client asking for a sequence older than the pane's 16 MiB in-memory ring receives cached
adapter metadata, `SessionReset { restoring: true }`, the journalled updates, and `SessionReady` as
freshly numbered items.

What is journalled is what the agent said. Prompts are not written as requests (only whatever the
agent echoes back as a `UserMessageChunk`), permission exchanges never reach the journal, and neither
does authentication material. The reducer's own `MAX_TOOL_PAYLOAD_BYTES` / `MAX_DIFF_SIDE_BYTES` caps
are client-side and in-memory only: the journal stores what arrived, chunk-coalescing aside,
subject solely to its own ceilings. A journal that cannot be opened is not fatal . the runtime simply runs without
one, and the pane falls back to whatever the provider can replay.

Sticky selector preferences remain a client-owned store because they are user intent rather than
conversation data: `agent-preferences.json` under the *app's* platform application-data directory,
using the permission-hardening helpers that now live in `zz-daemon::user_data` and are re-exported
for it. They contain only opaque advertised option IDs/values and their scope, never messages,
prompts, credentials, approval decisions, or a copy of provider session files.

# Verification

- Reducer tests cover the five flat transcript shapes, inert vendor metadata, prompt completion
  without fabricated tool completion, late updates without activity revival, message coalescing,
  replay deduplication, entry revisions, stable tool IDs, plan replacement, permissions, generic
  options, slash commands, and session-ID bounds. UI tests pin static tool semantics, nearby
  chevrons, raw Markdown and code copying, code-fence labels, the context ring, and completion-key
  routing.
- The ACP peer tests moved to the daemon with the runtime. `zz-daemon/src/agent/fixture.rs` is an
  in-process ACP agent both the host's own tests and the daemon's end-to-end wiring tests open panes
  against, in place of spawning a child. Host tests cover the whole arc . spawn through prompt to a
  finished turn, an auto-approved tool never reaching a client, a surfaced permission waiting for an
  answer, a mid-turn prompt queueing and dispatching after the prompt response, unqueueing handing
  prompts back with their images, a session the agent cannot load being replayed out of the journal,
  Git summaries refreshing after session readiness and prompt completion without blocking either,
  a pane outside a worktree reporting no summary, and pane closure cancelling what it owed.
- Fanout tests pin the lane's contract: a window of items leaving as one frame numbered from the
  first, a replay inside the ring serving only what the client missed, a replay older than the ring
  restoring Ready/reset/journal/SessionReady in order, request replies leaving the stream without
  spending a sequence, runtime replacement preserving sequence order while dropping old-generation
  output, the first prompt naming the pane and later ones leaving it alone, and pane state being
  published only when something a client renders moves.
- Daemon wiring tests cover the lane batching per pane and draining behind the reliable one, an
  overflowing lane asking for a replay instead of closing, updates reaching only the clients the pane
  is visible to, a submitted `agent-send` being dispatched by the daemon itself, and the agent mux
  options reconfiguring what the next pane spawns. They also prove that provider selection and Retry
  replace the host runtime and keep the fanout sequence monotonic.
- Client-side, preference tests cover bounded private persistence, workspace/provider scoping, and
  model → effort → permission restore order including legacy mode fallback.
- Timeline tests cover the activity fold: tools and reasoning collapse together, a lone
  member stays a plain row, an assistant or plan entry ends a run, and a member revision remeasures
  its owning group row rather than splicing.
- Mux/server tests cover descriptor round trips, session/cwd validation, stable picker
  materialization, live donor-cwd capture, and donor fallback to the last focused terminal when the
  split target has no working directory of its own.
- Config and view-adapter tests cover command/JSON parsing, absolute cwd validation, and every core
  native entry shape.
- Workspace-verb tests cover `agent-send` option parsing (detached, attached, and `--` forms),
  payload bounds and control-character rejection, context-header fencing, the stdin decision the CLI
  shares with the daemon parser, the full round trip through an attached client mailbox, and failure
  when that client disconnects. Environment tests (now in `zz-daemon`) assert workspace-identity
  injection and that a user-configured value survives, that the login PATH keeps precedence over the
  inherited one with the version-manager bins last, that a failed login shell still yields those
  bins, that the capture command follows the shell family, and that the probe is opt-out. The
  pane-identity merge is pinned by `pane_identity_is_adopted_without_touching_the_socket`.
- Permission tests cover the allow-always-then-allow-once preference and repeated allow kinds *not*
  reading as a question (both daemon-side now), plus
  `acp_v1_rejects_permission_options_with_an_unknown_kind`, which stayed with the client and pins
  the reason the question branch is unreachable rather than merely untested. Wizard tests cover
  page advance, an answered page latching until the controller drops it, page stability when another
  request resolves elsewhere, out-of-range and composer-engaged digits, and confirm/cancel.
- Journal tests cover ordered round trips with `0o600`/`0o700` modes, sequence continuity across a
  reopen, a tolerated torn trailing line, hostile session IDs staying inside the directory, refusal
  past the size cap, and retention pruning; a host test replays a session the agent cannot load out
  of the journal. A separate paths test pins that the journal lives under the daemon's own data
  directory rather than the GUI's.
- Git-summary tests cover clean, dirty, detached, unborn, and non-worktree states, the real index
  staying untouched, tracked and untracked file totals, timeout cleanup, and saturating counters.
- Protocol tests pin that the ten agent messages and four agent payloads occupy the wire tails
  they claim (`agent_runtime_variants_hold_the_wire_tails_they_were_appended_to`) and that the
  three agent mux options carry their defaults. The v54 session-op tests cover list filters and
  cursors, new-session cwd, full switch roots, path byte limits, and the 256-directory ceiling.
- `crates/zz-daemon/tests/agent_soak.rs` drives the fixture adapter through the daemon into a
  headless `InteractiveClient`: `agent_stream_soak` is `#[ignore]`d and prints throughput and daemon
  CPU time, while `agent_stream_soak_slow_client` exercises the lag-and-replay path.
- Reducer tests also pin that status-only updates never re-type a typed tool, that placeholder titles
  never become terminal commands, and that oversized payloads cap on char boundaries. Title tests
  pin the dressing strip, the 7-word/48-character limits counted in characters, and the once-per-pane
  guard. Sound tests pin the chime edges, the first-observation baseline, Request outranking Done,
  badge rollup, and the `ZZ_AGENT_SOUND` switch answering only to `0`; a sidebar test pins badges
  bubbling to every collapsed ancestor.

# Related

- [System overview](/architecture/overview.md)
- [Daemon-owned agent runtime](/designs/agent-daemon-runtime.md) . the decision record for the move
- [Wire protocol](/protocol/wire-protocol.md) . the ten agent messages, four agent payloads, and their bounds
- [Session persistence](/concepts/session-persistence.md) . what survives detach, and what a daemon restart takes
- [Mux snapshots](/protocol/snapshots.md)
- [Application configuration](/configuration/app-config.md)
- [tmux command set](/tmux/commands.md)
- [zz](/crates/zz.md) and [zz-daemon](/crates/zz-daemon.md)
- [Split-pane layout](/concepts/split-pane-layout.md)
