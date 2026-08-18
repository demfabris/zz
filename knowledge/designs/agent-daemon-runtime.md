---
type: Design Plan
title: Daemon-owned agent runtime
description: The agent pane's ACP adapter runtime moved from the GUI into the daemon — daemon-spawned adapter children, a daemon-side journal with replay-then-tail attach, a dedicated coalescing outbound lane, and clients reduced to viewports — so agent runs survive client detach and every client kind can render them.
status: Shipped
tags:
- agent
- acp
- daemon
- protocol
- persistence
timestamp: 2026-08-17T00:00:00-03:00
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

| Concern | Before (v52) | Current (v60) |
| --- | --- | --- |
| Adapter child, stdio, ACP session | GUI (`AgentController` runtime tasks) | daemon `agent::host`, one thread per pane |
| Auto-approve, queued prompts, stderr tail, session ops | GUI runtime half | daemon `agent::host` |
| Journal | GUI `<data>/zz/agent-journal` | daemon `<data>/zz/daemon/agent-journal` |
| Worktree status | GUI-owned per-turn snapshot and diff overlay | daemon-owned bounded current summary, published through pane state |
| Transcript reducer (`AgentThread`), view, composer, wizard, badges, mend, spring | GUI | GUI, unchanged |
| Adapter command strings, auto-approve flag | GUI config keys (`agent-*`) | mux options (flow via `mux.conf` + `SetConfigOverrides`, like `experimental-agent-pane`) |
| `agent-send --submit` | daemon → GUI round-trip (`request_from_gui`) | daemon dispatches directly into the host; `ComposerAppend` keeps the GUI round-trip |
| Adopting the ACP session ID, naming a pane after its first prompt | GUI, via `set-agent-session` / `select-pane -T` round trips | daemon (`adopt_agent_session`, `title_agent_pane`); the GUI keeps its own titling for the pane it drives |

# Wire surface (v60)

v53 moved the runtime into the daemon. v54 completed its session and restart controls. v55 gives
each client process a stable identity and acknowledges restored prompts so reconnects remain
owner-specific and do not resurrect consumed drafts. v56 removes the provider task-event, parked,
and abandoned-turn stream vocabulary. v57 removes the turn-diff request/reply and adds a bounded
`AgentGitSummary` to `AgentPaneWire`. v58 appends the typed tmux target-lookup errors used by mux
commands; the Agent pane payload is unchanged. v59 adds timed client messages and retained-pane
snapshot fields. v60 appends the request-tagged prefix cancellation input and acknowledgment; the
Agent pane payload remains unchanged. Postcard
cannot carry the ACP SDK's JSON-shaped types
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
- `AgentSessionOp { pane, op }`, where `List` carries cwd/cursor/replace, `New` carries cwd,
  `Switch` carries session ID/cwd/additional directories, and `Delete` carries the session ID.
- `AgentReplay { pane, from_seq: u64 }` — request journal replay (attach, lag recovery, pane
  focus).
- `AgentAcknowledgePromptRestore { pane, reclaim_id: u64 }` — retire one daemon-cached draft after
  its owning client restores it.

Daemon → client (`EventPayload`, appended):
- `AgentUpdates { pane, first_seq: u64, items: Vec<Vec<u8>> }` — JSON `AgentStreamItem`s,
  batch ≤ 1 MiB (split into multiple frames when larger), per-pane monotonic `seq`.
- `AgentState { pane, state: AgentPaneWire }` — small typed struct: connection phase, queued
  count, active session id, title, auth methods, pending permission `(request_id, payload ≤ 64
  KiB)`, current Git summary (branch ≤ 4 KiB plus `u32` totals), and the adapter's config options
  and modes as one JSON blob each (≤ 256 KiB). Published on change to every client attached to
  the pane's session (feeds badges without the heavy stream). A payload that fails
  `AgentPaneWire::validate` is trimmed down to the fields that fit rather than dropped.
- `AgentLagged { pane, next_seq }` — the client's agent lane overflowed and was cleared; the
  client answers with `AgentReplay`.
- `AgentSessions { pane, request_id, result }` — the JSON reply to `AgentSessionOp::List`, at most
  1 MiB and returned only to the requesting connection. A session listing has no request ID on the
  ACP side, so it rides `request_id` 0 while the daemon carries `ClientId`
  out of band. An oversized listing becomes a bounded failure reply.

Every new payload gets an explicit bound in `validate_control_message`, and
`knowledge/protocol/wire-protocol.md` gets the variant list plus its byte-level example updated
(client-core contract, pitfall 13).

# Fan-out: a dedicated agent lane

The reliable lane hard-closes a client at 256 queued messages / 72 MiB; an ACP turn bursts
hundreds of updates, so agent streams get their own `OutboundState` lane, precedented by the
`terminals` slot:

- `agent: BTreeMap<PaneId, PendingAgent>` where `PendingAgent` accumulates encoded
  `AgentUpdates` batches with a per-pane cap (`MAX_PENDING_AGENT_BYTES`, 4 MiB). The fanout
  coalesces items into 25 ms windows (`BATCH_WINDOW`) before enqueueing, on one shared flush
  thread that parks whenever no pane has anything gathered, so a healthy client sees a few
  frames per second, not one per token.
- Each pane also keeps a `MAX_REPLAY_RING_BYTES` (16 MiB) in-memory ring of encoded items. A
  replay inside the ring is served straight to the asking client and nothing else moves. A
  replay older than the ring falls back to the journal. The lane emits cached adapter `Ready`,
  `SessionReset { restoring: true }`, the journalled updates, and `SessionReady` with the current
  session/configuration as freshly numbered items, followed by an ordered `StateSynced` snapshot.
  The client rebuilds the transcript, then returns to the daemon's current Running, Failed,
  permission, or Ready phase even though reliable state frames drain first.
- Drain order: `reliable` → `command_output` → `agent` (round-robin) → `terminals`.
- Overflow does NOT close the connection: the pane's lane is cleared and a tiny
  `AgentLagged { pane, next_seq }` marker queued; the client re-requests `AgentReplay` from its
  last applied seq. The journal makes the stream recoverable, so slow clients degrade to
  replay instead of dying.
- Visibility: the heavy stream flows only to clients for whom the pane is in the derived
  visible set (`visible_agent_panes`: attached session + that client's focused window, honoring
  zoom — the same shape as terminal visibility). `AgentState` flows to all clients attached to
  the session. Attach pushes `AgentState` for every agent pane in the session
  (`send_agent_resync`) and a pane entering the visible set pushes its state
  (`refresh_agent_visibility`); in both cases the client, not the daemon, decides where to
  replay from and issues `AgentReplay { from_seq }` off its own cursor. A pane leaving the
  visible set has its queued frames dropped (`cancel_agent`), which the cursor makes safe.

`Event.sequence` stays advisory (pitfall 1); the agent stream's own per-pane `seq` is the
replay cursor.

Two things about that sequence deviate from the sketch above. **The fanout mints it, not the
host.** The host stamps its own per-pane counter for its internal bookkeeping, but request
replies leave the stream entirely without spending a wire sequence, and a journal replay
synthesizes fresh items, so only the fanout can promise a client a gapless run. It is therefore
not literally the journal's seq. **An item too large for one frame is dropped, not stamped**: a
single encoded `AgentStreamItem` over `MAX_AGENT_UPDATES_BYTES` is logged and skipped, because
the wire cannot carry it and a sequence spent on it would read as loss to every client.

v54 keeps the fanout sequence across a runtime replacement. Each open receives a runtime generation;
the sink drops events whose generation no longer owns the lane. Provider changes and explicit Retry
can close one adapter and open another without resetting the client cursor or admitting a late event
from the old child. A retiring generation may deliver only `PromptsReclaimed`, which preserves the
queue without accepting stale adapter state or output.

# Daemon host (`zz-daemon/src/agent/`)

Behind an `agent` cargo feature (`default = ["daemon", "agent"]`; `zz-tui`/`zz-client-ffi`
depend on `zz-daemon` with `default-features = false` and never inherit
`agent-client-protocol`, while the desktop takes the default and consumes the stream vocabulary
`zz-daemon`'s crate root re-exports).

- One `std::thread` per agent pane running
  `futures_lite::future::block_on(run_agent_connection(..))` — the crate is runtime-agnostic
  (`async-process` + `futures`, no tokio), and this matches the daemon's thread-per-pane idiom.
  Commands in via `async_channel`; items out via a callback that appends to the journal and
  fans out.
- The ported logic is the revival's runtime half, moved nearly verbatim from
  `crates/zz/src/agent/controller.rs`: `run_agent_runtime` / `run_agent_connection`,
  `RuntimeCommand`/`RuntimeEvent` (becoming `AgentStreamItem`), `RuntimeRouting`, `StderrTail`,
  auto-approve (`is_user_question` + preferred allow), queued prompts with reclaim,
  session-id validation, journal,
  and `environment.rs`'s PATH repair (the pure-std `login_shell` module moved wholesale).
- The runtime is built lazily, on the first agent pane the daemon opens rather than at daemon
  start; building it opens and prunes the journal and calls `prewarm`, which takes the
  login-shell PATH snapshot and warms the npx cache for the configured adapter packages off the
  spawn path.
- A prompt that arrives while the pane is not `Ready` **queues**; it is never rejected. The
  host dispatches the queue head after the active `session/prompt` request returns and hands the
  whole queue back through `PromptsReclaimed` on cancel, on a lost runtime, and
  on a failed dispatch. That is the same at-least-once rule the GUI had, now enforced
  daemon-side, so a prompt survives the client that typed it.
- Prompt images arrive as bytes+format on the wire and convert to ACP `ContentBlock`s
  daemon-side; `gpui::Image` never crosses.
- Permissions: the live SDK responder parks in the host with NO timeout (a human decides).
  The pending request rides `AgentState`, so late-attaching clients see it; resolution is
  first-answer-wins; pane close or adapter death resolves it cancelled.
- A named background worker captures the current branch, changed-file count, additions, and
  deletions after session readiness, a session switch, and prompt completion. The host publishes
  the lifecycle boundary and drains queued prompts first; generation, refresh, and cwd guards reject
  stale results before a state-only publication.
- The journal moved file-for-file (`agent-journal/<session>.jsonl`, 32 MiB cap, 30-day prune,
  torn-tail tolerance) under `<data>/zz/daemon/`, with the `user_data.rs`
  permission-hardening helpers now living in `zz-daemon::user_data` and re-exported for the
  GUI's preferences file.
- `experimental-agent-pane` flipping off with live children: existing panes keep running
  (option gates materialization, not execution), matching browser-pane behavior.
- The workspace identity survives the move: the spawn config is built once per daemon
  (`agent_spawn_config`, socket only), and the per-pane `ZZ_PANE`/`ZZ_SESSION` identity rides
  `AgentPaneSpec.workspace`, merged over the config at spawn
  (`AgentWorkspaceEnvironment::adopt_pane_identity`), so an agent can still address itself.
- `set-agent-provider` emits an `AgentPaneRestart` effect after it updates the descriptor and clears
  the provider-bound session ID. The Retry action emits the same restart through
  `restart-agent-pane`. The daemon resolves the current descriptor after it drops the mux lock,
  closes the old host, and opens the replacement under a new generation.

# Client changes

- `zz-client` core: pass-through `CoreEvent`s for the stream payloads (the core stores no
  transcript, per the contract). The one exception is `AgentState`, which the core does retain
  — it is small, typed, and last-writer-wins — so a shell reads it through
  `ClientCore::agent_state` after a `CoreEvent::AgentStateChanged`, and it is cleared on
  attachment reset. `zz-tui`'s exhaustive match gained arms that keep its current "renders a
  card, no transcript" behavior; `zz-client-ffi` ignores them.
- `MuxClient` buffers agent events per pane beside `agent_commands` and keeps a per-pane
  `agent_cursors` map; `AppView::drain_agent_events` hands the drain to `AgentController`. The
  cursor is what makes the stream idempotent: a replay deliberately overlaps the live tail, so
  items at or below the cursor are dropped, and a batch that starts *past* the cursor is a hole
  the client cannot wait out, so it re-requests a replay from where it stands.
- `AgentController` loses its runtime half: `ensure_pane` becomes viewport bookkeeping that
  asks for a replay when a pane goes live; `prompt`/`cancel`/wizard answers/settings become
  protocol sends through `AgentRequest`; incoming `AgentStreamItem`s land in
  `apply_stream_items` and feed the existing reducer, so the view, wizard, badges, mend, and
  spring are untouched. The client-side journal, stdio runtime, legacy turn snapshots, and PATH repair
  are deleted, along with the `set-agent-session` round trip the daemon now does itself.
- v54 carries the client's full session intent to the daemon. History scope and cursors reach
  `session/list`; a new workspace reaches `session/new`; a restore preserves cwd and additional
  directories for `session/load`. The local picker also passes configured cwd through
  `select-pane-kind -c`, so the descriptor wins over a donor terminal before the host starts. A
  remote picker omits the desktop path and inherits cwd on the daemon host.
- Agent config keys (`agent-command`, `agent-claude-code-command`, `agent-auto-approve`)
  become mux options; the settings UI writes them through the existing option machinery.
  `agent-working-directory` stays client-side and applies only to the local host.

# Performance gates

1. Terminal throughput must not regress: back-to-back `bench/run.sh --terminals zz` on a
   release bundle built from main and from this branch, same machine, frontmost window, AC
   power. The mailbox drain-order change is the only shared-path touch and must show inside
   run-to-run noise.
2. Agent streaming soak: `crates/zz-daemon/tests/agent_soak.rs` drives a fake ACP adapter
   through the daemon into a headless `InteractiveClient`, asserting convergence and printing
   throughput and daemon CPU time; `agent_stream_soak` is `#[ignore]`d (it runs for minutes)
   and `agent_stream_soak_slow_client` covers the lag-and-replay path. Numbers land here.
3. `just profile-system mac 20s` during a soak to confirm the added threads (1 per pane + the
   shared park ticker + the shared flush thread + `async-process` reaper + `blocking` pool)
   idle correctly (the parked-clock lesson: nothing polls while no agent runs).

Measured 2026-08-14 (M-series mac, AC power, back-to-back):

Gate 1 — release bundles, same 158×106 grid, 3 runs per lane:

| test | main | this branch | delta |
| --- | ---: | ---: | ---: |
| cat 150 MiB ASCII | 198.32 MB/s | 196.51 MB/s | −0.9% |
| cat 150 MiB mixed UTF-8 | 93.55 MB/s | 91.33 MB/s | −2.4% |
| doom-fire | 243.21 fps | 239.71 fps | −1.4% |

Every delta sits inside the lanes' own run-to-run spread (main's three ASCII runs alone span
3.6%, unicode 4.0%; the run ranges overlap): no regression.

Gate 2 — 50k-item turn, three release runs, 1.2% wall-clock spread:
`SOAK items=50001 secs=5.19 items_per_sec≈9600 frames≈192 ratio≈260 bytes=20853809`,
zero `AgentLagged` for a draining client; the `--ignore`d debug run holds ratio ≈112 at
≈3860 items/sec. The slow-client test trips the 4 MiB lane cap into exactly one
`AgentLagged`, replays to a gapless transcript, and the terminal lane keeps delivering
throughout. The batcher's binding constraint at release speed is `MAX_AGENT_UPDATES_BYTES`
(1 MiB frames), not the 25 ms window.

Gate 3 was replaced by construction evidence: the flush thread and park ticker both park on
condvars when no pane has work (pinned by host tests), so an idle daemon schedules nothing.

# Risks accepted

- v54 hard-rejects older peers at three layers; the shipped mismatch UX (identity file +
  prompted daemon restart) is the rollout path.
- JSON blobs on the wire forfeit wire-level introspection beyond byte caps; the journal and
  client reducer already speak exactly this shape.
- The daemon grows its first data directory; scoped to `agent-journal`.
- `request_from_gui`'s lowest-ClientId pick still routes `ComposerAppend`; prompts no longer
  depend on it.
