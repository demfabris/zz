---
type: Design Plan
title: Daemon-owned agent runtime
description: The plan that moves the agent pane's ACP adapter runtime from the GUI into the daemon — daemon-spawned adapter children, a daemon-side journal with replay-then-tail attach, a dedicated coalescing outbound lane, and clients reduced to viewports — so agent runs survive client detach and every client kind can render them.
status: In progress
tags:
- agent
- acp
- daemon
- protocol
- persistence
---

# Goal

An agent pane's ACP adapter is spawned and owned by the daemon, exactly as PTYs are. A running
turn survives the GUI closing; a reattaching client replays the transcript and tails the live
stream; two clients attached to the same session converge on the same conversation. The desktop
keeps today's feature set (borrows #1–#14) with the runtime half executing daemon-side.

Non-goals for this iteration: daemon-restart resurrection (no daemon state survives a restart
today — agents match terminals, not exceed them), TUI transcript rendering (unblocked, not
built), remote-host anything beyond what plain ssh attach already provides (the adapter simply
runs on the daemon's machine, which is the correct machine).

# Ownership shift

| Concern | Today (v52) | After (v53) |
| --- | --- | --- |
| Adapter child, stdio, ACP session | GUI (`AgentController` runtime tasks) | daemon `agent::host`, one thread per pane |
| Auto-approve, quiesce park, queued prompts, stderr tail, session ops | GUI runtime half | daemon `agent::host` |
| Journal | GUI `<data>/zz/agent-journal` | daemon `<data>/zz/daemon/agent-journal` |
| Turn snapshots (git write-tree at dispatch) | GUI background executor | daemon (correct for remote daemons) |
| Transcript reducer (`AgentThread`), view, composer, wizard, badges, mend, spring | GUI | GUI, unchanged |
| Adapter command strings, auto-approve flag | GUI config keys (`agent-*`) | mux options (flow via `mux.conf` + `SetConfigOverrides`, like `experimental-agent-pane`) |
| `agent-send --submit` | daemon → GUI round-trip (`request_from_gui`) | daemon dispatches directly into the host; `ComposerAppend` keeps the GUI round-trip |

# Wire surface (v53)

`PROTOCOL_VERSION` 52 → 53. Postcard cannot carry the ACP SDK's JSON-shaped types
(`serde_json::Map` metas, `RawValue` params), so every stream item crosses as an opaque JSON
blob with a byte cap, and the client deserializes into the same `RuntimeEvent`-shaped enum the
reducer already consumes. The daemon defines `AgentStreamItem` (a serde/JSON mirror of today's
`RuntimeEvent` minus channel-only variants); `zz-protocol` carries only bytes plus small typed
envelopes.

Client → daemon (`ProtocolMessage`, appended, never reordered):
- `AgentPrompt { pane, text, images: Vec<AgentImage { format: String, data: Vec<u8> }> }` —
  total payload ≤ 6 MiB (mirrors paste bounds); replaces the GUI-side `RuntimeCommand::Prompt`.
- `AgentCancel { pane }`
- `AgentUnqueue { pane }` — reclaim queued prompts; the daemon returns them inside the stream so
  the composer refills.
- `AgentRespondPermission { pane, request_id: u64, option_id: Option<String> }` — `None`
  cancels. First answer wins; late answers are acknowledged as no-ops.
- `AgentSetConfigOption { pane, option_id, value }` / `AgentSetMode { pane, mode_id }` /
  `AgentAuthenticate { pane, method_id }` — strings, 4 KiB bounds.
- `AgentSessionOp { pane, op: List | New | Switch { session_id } | Delete { session_id } }`
- `AgentReplay { pane, from_seq: u64 }` — request journal replay (attach, lag recovery, pane
  focus).
- `AgentTurnDiff { pane, request_id: u64 }`

Daemon → client (`EventPayload`, appended):
- `AgentUpdates { pane, first_seq: u64, items: Vec<Vec<u8>> }` — JSON `AgentStreamItem`s,
  batch ≤ 1 MiB (split into multiple frames when larger), per-pane monotonic `seq`.
- `AgentState { pane, state: AgentPaneWire }` — small typed struct: connection phase, queued
  count, active session id, title, auth methods, pending permission
  `(request_id, payload_json ≤ 64 KiB)`, config options as one JSON blob. Published on change to
  every client attached to the pane's session (feeds badges without the heavy stream).
- `AgentLagged { pane, next_seq }` — the client's agent lane overflowed and was cleared; the
  client answers with `AgentReplay`.
- `AgentSessions { pane, request_id, result_json }` and
  `AgentTurnDiffResult { pane, request_id, result_json }` — replies to `AgentSessionOp::List`
  and `AgentTurnDiff` (the `HistoryRequest → HistoryChunk` precedent).

Every new payload gets an explicit bound in `validate_control_message`, and
`knowledge/protocol/wire-protocol.md` gets the variant list plus its byte-level example updated
(client-core contract, pitfall 13).

# Fan-out: a dedicated agent lane

The reliable lane hard-closes a client at 256 queued messages / 72 MiB; an ACP turn bursts
hundreds of updates, so agent streams get their own `OutboundState` lane, precedented by the
`terminals` slot:

- `agent: BTreeMap<PaneId, PendingAgent>` where `PendingAgent` accumulates encoded
  `AgentUpdates` batches with a per-pane cap (4 MiB). The daemon-side host already coalesces
  items into ≤ 25 ms windows before enqueueing, so a healthy client sees a few frames per
  second, not one per token.
- Drain order: `reliable` → `command_output` → `agent` (round-robin) → `terminals`.
- Overflow does NOT close the connection: the pane's lane is cleared and a tiny
  `AgentLagged { pane, next_seq }` marker queued; the client re-requests `AgentReplay` from its
  last applied seq. The journal makes the stream recoverable, so slow clients degrade to
  replay instead of dying.
- Visibility: the heavy stream flows only to clients for whom the pane is in the derived
  visible set (same derivation as terminal frames: attached session + focused window, zoom
  rules). `AgentState` flows to all clients attached to the session. A pane entering the
  visible set triggers an implicit replay-then-tail (`send_resync` sends `AgentState`; the
  client issues `AgentReplay { from_seq: 0 }` or from its cached seq).

`Event.sequence` stays advisory (pitfall 1); the agent stream's own per-pane `seq` — which is
the journal seq — is the replay cursor.

# Daemon host (`zz-daemon/src/agent/`)

Behind a new `agent` cargo feature (`default = ["daemon", "agent"]`; `zz-tui`/`zz-client-ffi`
build with `default-features = false` and never inherit `agent-client-protocol`).

- One `std::thread` per agent pane running
  `futures_lite::future::block_on(run_agent_connection(..))` — the crate is runtime-agnostic
  (`async-process` + `futures`, no tokio), and this matches the daemon's thread-per-pane idiom.
  Commands in via `async_channel`; items out via a callback that appends to the journal and
  fans out.
- The ported logic is the revival's runtime half, moved nearly verbatim from
  `crates/zz/src/agent/controller.rs`: `run_agent_runtime` / `run_agent_connection`,
  `RuntimeCommand`/`RuntimeEvent` (becoming `AgentStreamItem`), `RuntimeRouting`, `StderrTail`,
  auto-approve (`is_user_question` + preferred allow), queued prompts with reclaim, quiesce
  park (`ZZ_AGENT_QUIESCE_MS`; ticked from a small shared deadline thread, not a runtime),
  session-id validation, journal, and `environment.rs`'s PATH repair (pure-std `login_shell`
  module moves wholesale; `warm_agent_adapter_cache` prewarms at daemon start).
- Prompt images arrive as bytes+format on the wire and convert to ACP `ContentBlock`s
  daemon-side; `gpui::Image` never crosses.
- Permissions: the live SDK responder parks in the host with NO timeout (a human decides).
  The pending request rides `AgentState`, so late-attaching clients see it; resolution is
  first-answer-wins; pane close or adapter death resolves it cancelled.
- Turn snapshots run at dispatch on the pane thread (blocking git is fine there);
  `AgentTurnDiff` captures against the stored base.
- The journal moves file-for-file (`agent-journal/<session>.jsonl`, 32 MiB cap, 30-day prune,
  torn-tail tolerance) under a new daemon data dir, with the `user_data.rs`
  permission-hardening helpers moving into `zz-daemon` and re-exported for the GUI's
  preferences file.
- `experimental-agent-pane` flipping off with live children: existing panes keep running
  (option gates materialization, not execution), matching browser-pane behavior.

# Client changes

- `zz-client` core: pure pass-through `CoreEvent`s for the new payloads (the core stores
  nothing, per the contract); `zz-tui`'s exhaustive matches gain arms that keep its current
  "renders a card, no transcript" behavior; `zz-client-ffi` ignores them.
- `MuxClient` buffers agent events per pane beside `agent_commands`; `WorkspaceView` drains
  them into `AgentController`.
- `AgentController` loses its runtime half: `ensure_runtime` becomes wire subscription
  bookkeeping; `prompt`/`cancel`/wizard answers/settings become protocol sends; incoming
  `AgentStreamItem`s deserialize into the existing `handle_runtime_event` input, so the
  reducer, view, wizard, badges, mend, and spring are untouched. The client-side journal,
  stdio runtime, and PATH-repair usage are deleted.
- Agent config keys (`agent-command`, `agent-claude-code-command`, `agent-auto-approve`)
  become mux options; the settings UI writes them through the existing option machinery.
  `agent-working-directory` stays client-side (it feeds pane creation, which is already a
  client concern).

# Performance gates

1. Terminal throughput must not regress: back-to-back `bench/run.sh --terminals zz` on a
   release bundle built from main and from this branch, same machine, frontmost window, AC
   power. The mailbox drain-order change is the only shared-path touch and must show inside
   run-to-run noise.
2. Agent streaming soak: an ignored integration test drives a fake ACP adapter emitting
   50k-token turns through the daemon into a headless `InteractiveClient`, asserting
   convergence and printing throughput, daemon CPU time, and max lane depth. Run before merge;
   numbers land in this document.
3. `just profile-system mac 20s` during a soak to confirm the added threads (1 per pane +
   `async-process` reaper + `blocking` pool) idle correctly (the parked-clock lesson: nothing
   polls while no agent runs).

# Risks accepted

- v53 hard-rejects v52 peers at three layers; the shipped mismatch UX (identity file +
  prompted daemon restart) is the rollout path.
- JSON blobs on the wire forfeit wire-level introspection beyond byte caps; the journal and
  client reducer already speak exactly this shape.
- The daemon grows its first data directory; scoped to `agent-journal`.
- `request_from_gui`'s lowest-ClientId pick still routes `ComposerAppend`; prompts no longer
  depend on it.
