---
type: Research Report
title: Native agent drivers survey (retiring the ACP pane)
description: How t3code, oh-my-pi, and other harnesses keep up with Claude Code and Codex without ACP, the current Claude stream-json and Codex app-server surfaces, how fork/branch/side/btw, questions, plans, and background agents map onto each, and a proposed driver shape for a zz agent pane and orchestrator.
tags:
- agent
- acp
- claude-code
- codex
- t3code
- oh-my-pi
- orchestration
- survey
timestamp: 2026-10-06T00:00:00Z
---

# Question

The ACP agent pane lags Claude Code and Codex (fork broken, steering hangs, subagents and
background work flattened). How do other orchestration harnesses stay compatible with Claude Code,
Codex, and the rest, how do they parse each vendor's output, and what would a unified zz agent pane
look like that supports background agents, markdown and mermaid, question tools, and vendor
commands (fork, branch, side, btw) with real UI, while making zz the backbone for an orchestrator
(agents spawning PiP panes, inter-session chat over the Claude and Codex bridges)?

# Method

Five research lanes on 2026-10-06, each reading source in shallow clones (session scratchpad, not
kept): `can1357/oh-my-pi`, `pingdotgg/t3code` (Claude, Codex, other-provider, web UI, and churn
sub-lanes), `openai/codex` at 5679e675, the Claude Agent SDK 0.3.292 `.d.ts` plus the MIT Python
SDK, and Happy, claude-agent-acp, vibe-kanban, hapi. Local CLIs: Claude Code 2.1.288, Codex 0.160.1.
No model turn was run. One Claude probe started the binary with `--bare`, an isolated
`HOME`/`CLAUDE_CONFIG_DIR`, no credentials, and sent only `initialize`. Codex schema generation ran
with `CODEX_HOME` in the scratchpad. Nothing touched `~/.claude`, `~/.codex`, peer sockets, or the
shared Codex daemon. A code map of the current pane came from an Explore pass over main at 3825bfee6.

# Answer

Nobody serious keeps up through ACP for Claude Code and Codex. Everyone with a rich UI speaks the
vendor's own wire, the same one the vendor's own GUIs use:

- **Claude Code**: `claude -p --input-format stream-json --output-format stream-json` plus
  `control_request`/`control_response`. Anthropic's Desktop Code tab and VS Code extension drive the
  CLI exactly this way. t3code, Happy, and claude-agent-acp go through the TS Agent SDK (which
  speaks that wire); vibe-kanban speaks it raw from Rust.
- **Codex**: `codex app-server` JSON-RPC. The Codex TUI, Desktop, and VS Code extension are all
  app-server clients. Every open harness checked (t3code, vibe-kanban, hapi, CodexMonitor, Happy,
  superset) uses it with `experimentalApi: true`.
- **ACP stays for the long tail**: Gemini CLI, opencode, Cursor `agent acp`, Grok, Devin,
  Antigravity, and oh-my-pi speak ACP natively and do not lag. t3code runs a generic ACP driver for
  them next to its native Claude, Codex, OpenCode, Pi, and Cursor drivers.

ACP is lagging, not dying (schema v1.24.1 on 09-30, 622 spec commits in 90 days, 41 agents, 131
clients), but the two agents zz cares about most are the two whose vendors ship their own protocol.
Adapter lag is days; the feature gaps are the real problem (claude-agent-acp #1110 fork broken,
#1039 and #1267 steering hangs, codex-acp #315 fork open, steering is out of the spec entirely).

How they keep up, in one list:

1. **Run the user's installed binary**, not a bundled one (t3code strips the SDK's eight platform
   binaries; Happy local mode; Desktop and VS Code ship their own). The API rejects new models from
   old CLIs ("Claude Code 2.1.185 does not support this model; version 2.1.280 or newer is
   required"), so a pinned binary goes stale in weeks. vibe-kanban pinned 2.1.119 and is 170
   versions behind.
2. **A small adapter trait plus capability flags** the core checks instead of branching on vendor.
3. **Tolerant decoding with a raw fallback.** t3code reads undeclared Claude fields with
   `Reflect.get`; oh-my-pi's Rust client decodes unknown frames to `Unknown(Value)`. The anti-pattern
   is t3code's strict generated Codex schema, which silently drops a notification that fails to
   decode (`effect-codex-app-server/src/client.ts:164-170`).
4. **Capability probing at runtime**: Claude `system/init.capabilities` and the `initialize`
   response; Codex `-32601` method-not-found probing (hapi sends each method with a bogus thread id).
5. **Version manifest**: t3code fetches per-driver version ranges from its own repo (codex >= 0.156
   supported, < 0.149 broken; claude >= 2.1.280) with a bundled fallback.
6. **Record/replay fixtures**: t3code has 87 scenarios, each with one recorded NDJSON transcript per
   provider and a recorder script per vendor.
7. **Types**: t3code regenerates Codex types from the schema OpenAI checks in at
   `codex-rs/app-server-protocol/schema/json` (three full regenerations in 90 days, hand patches in
   between). hapi, CodexMonitor, Happy, and superset hand-write tolerant types for the subset they
   use. `git diff rust-vA..rust-vB -- codex-rs/app-server-protocol/schema/json` gives the exact
   protocol delta between two Codex releases; for Claude, the `.d.ts` in the npm SDK is readable even
   though the JS is minified.

# t3code

TypeScript on Effect 4, MIT, ~25.8k stars, 2,958 commits in 90 days from mostly two maintainers.
Server owns provider processes, PTYs, and git; web, desktop (Electron), and mobile are clients over
Effect RPC on WebSocket. Event-sourced SQLite. The whole v2 orchestrator landed as one squash on
2026-10-02 (`de3439142`, +380k/-203k), so its shape is days old.

- **Adapter trait** (`apps/server/src/orchestration-v2/ProviderAdapter.ts`): `getCapabilities`,
  `openSession` returning a `SessionRuntime` with `events`, `startTurn`, `steerTurn`,
  `interruptTurn`, `compactThread?`, `respondToRuntimeRequest`, `rollbackThread`, `forkThread`,
  `hasPendingBackgroundWork?`. Adapters emit whole-entity snapshots under ids derived from native
  ids (upserts, never deltas); text is coalesced every 50 ms.
- **Unified model** (`packages/contracts/src/orchestrationV2.ts`): thread > run > attempt
  (`initial | steering_restart | retry | provider_recovery`), each run owns a node tree. `TurnItem`
  kinds: user/assistant message, reasoning, proposed_plan, todo_list, user_input_request,
  file_change, command_execution, file/web search, approval_request, checkpoint, compaction,
  handoff, fork, subagent `{childThreadId, prompt, progress, result}`, dynamic_tool (raw in/out),
  error, notification. Runtime requests carry `responseCapability: live | message | not_resumable`.
  Capabilities are 12 flag groups (`supportsActiveSteering`, `supportsSteeringByInterruptRestart`,
  `terminalStatusQuality: strong|weak|none`, enforcement `native|client-boundary`).
- **Claude adapter** (8.0k lines): one long-lived SDK `query()` per session fed by a queue;
  `canUseTool` turns `AskUserQuestion` into a question card and answers with
  `updatedInput.answers`; `ExitPlanMode` is captured as a proposed plan and then denied so the model
  stops; steering pushes a user message with `priority: "now"`; fork is the SDK's
  `forkSession(upToMessageId)`; rollback is `resumeSessionAt`; model or policy change closes the
  process and resumes. Subagents come from `system:task_*` plus `parent_tool_use_id` routing. Text
  deltas are dropped (whole blocks only).
- **Codex adapter** (7.0k lines): `codex app-server` over stdio, `thread/{start,resume,fork,read,
  turns/list,revert,compact/start,inject_items}`, `turn/{start,steer,interrupt}`, approvals,
  `item/tool/requestUserInput`, plan mode as `collaborationMode` on `turn/start`. Drops
  `review/start`, output deltas, and keeps only `changes[0]` of a file change.
- **Churn, 90 days** (old v1 file / new v2 file): Claude 50/17 commits (~60% fixes), Codex 28/13
  plus 26 on its session runtime, OpenCode 26/4, Grok 15/3, Cursor 13/4. Many commits touch every
  adapter at once.
- **UI**: steer/queue/restart/start/defer dispatch modes resolved under the thread lock (button
  flips with Cmd); server-side reorderable queue; subagents as read-only child threads with one row
  in the parent; question panel in the composer (single/multi select plus free text); checkpoints
  as hidden git refs (`refs/t3/orchestration-v2/checkpoints`) and "edit from here" rewinds both
  files and the vendor conversation; react-markdown, mermaid 11 lazy-loaded, shiki diffs; streamed
  text released at markdown block boundaries. Its terminal is libghostty-vt in WASM. No `/btw`,
  `/side`, `/fork`, or `/review` commands; fork is a timeline action.
- **Orchestration**: an authenticated HTTP MCP server `t3-code` injected into every provider
  (Claude via `mcpServers`, Codex via thread config, ACP via a stdio bridge, Pi via a generated
  extension). 16 tools: `delegate_task`, `task_status`, `task_cancel`, `create_threads`,
  `t3_thread_{list,read,send,wait,interrupt}`, `schedule_task` and friends. A child gets only the
  task prompt and never wider permissions than its parent. Async completion wakes the parent with a
  server-written message queued behind its active run (never `priority: "now"`, so tools are not
  cancelled), batched, at-least-once with a stable message id. Cross-provider handoff is a
  transcript cut to 16k tokens, no LLM summary.

# oh-my-pi

A hard fork of pi (MIT, TypeScript/Bun + ~360k lines of Rust natives, ~170 commits a day including
a bot, 30 releases in three weeks). It is a **harness**: its own loop against 60+ provider HTTP APIs.
It never spawns `claude` or `codex`. It reaches Claude subscriptions by posing as Claude Code
(`claude-code-fingerprint.ts`: pinned version string, "You are Claude Code" system line, Claude Code
beta headers, a proxy script that records the real CLI's traffic) and pools accounts. That is
exactly what Anthropic's legal page forbids third parties to do, and Anthropic blocked OpenCode for
the same thing in January. **Not a backend for zz.** It already runs in today's ACP pane via
`omp acp`.

Worth borrowing:

- `/btw` as an ephemeral turn: snapshot history including the half-streamed reply, send the same
  tool list so the prompt cache hits, discard tool calls, never touch history, store answers apart
  (`session/agent-session.ts:10716`).
- `ask` tool schema: `{questions: [{id, question, header?, options: [{label, description?,
  preview?}], multi?, recommended?}]}`, free text always offered.
- Separate steer and follow-up queues with `queue_update` snapshots; steering checked at loop start
  and after each tool batch.
- Three completion signals in its RPC: ack, `prompt_result`, `session_settled`.
- Host tools: the RPC host registers tools the agent can call (`host_tool_call/update/result/
  cancel`). Same idea as t3code's MCP injection.
- JSONL session tree with `id`/`parentId`, a leaf pointer, and branch summaries.
- Claude and Codex transcript importers (`session/claude-session-store.ts`,
  `session/codex-session-store.ts`) and an MIT Rust mermaid-to-ASCII renderer (for the TUI client).

Its Tern Surface Protocol (in-band APC UI nodes) is a new vendor format; zz already passed on it.

# Claude Code surface (CLI 2.1.288, SDK 0.3.292)

SDK 0.3.N bundles CLI 2.1.N and both release minutes apart: 77 CLI and 79 SDK releases in 90 days.

- **Flags Anthropic's Desktop Code tab uses**: `--output-format stream-json --input-format
  stream-json --verbose --include-partial-messages --replay-user-messages --permission-prompt-tool
  stdio --await-initialize --setting-sources=user`. Plus `--resume`, `--session-id`,
  `--fork-session`, hidden `--resume-session-at <uuid>`.
- **Messages** (`sdk.d.ts:5353`, 39 members, unchanged in 90 days): assistant, user (with
  `parent_tool_use_id`, `priority now|next|later`, `origin`), result, stream_event, and system
  subtypes including `init` (slash_commands, capabilities), `compact_boundary`, `api_retry`,
  `task_started/progress/updated/notification`, `background_tasks_changed` (replace semantics),
  `session_state_changed` (the reliable turn-over signal), `hook_*`, `permission_denied`; top-level
  `tool_progress`, `rate_limit_event`, `prompt_suggestion`.
- **Control requests** (`sdk.d.ts:5011`, 36 to 40 in 90 days). CLI to host: `can_use_tool`,
  `hook_callback`, `elicitation`, `request_user_dialog`, `mcp_message`. Host to CLI: `initialize`
  (hooks, agents, sdkMcpServers, `planModeInstructions`, `supportedDialogKinds`,
  `perTaskStopAffordance`), `interrupt`, `set_permission_mode`, `set_model`,
  `set_max_thinking_tokens`, `rename_session`, `rewind_files`, `stop_task`, `background_tasks`,
  `get_task_output`, `get_context_usage`, `mcp_*`, `reload_*`. Untyped but sent by Anthropic's own
  clients: `side_question` (the headless `/btw`), `generate_session_title`, `export_conversation`,
  `get_plan`. The binary also answers `rewind_conversation`; `fork_conversation` needs a remote
  control server.
- **Sessions**: `listSessions`, `getSessionMessages`, `forkSession(upToMessageId)` and friends are
  plain JSONL operations inside the SDK; the MIT Python versions port directly
  (`_internal/sessions.py`, `session_mutations.py`).
- **Commands headless** (probe `initialize` returned 44): compact, code-review (= `/review`), goal,
  model, effort, context, usage, agents, init, security-review, simplify, loop, recap and others.
  TUI-only: `/btw`, `/fork`, `/branch`, `/rewind`, `/plan`, `/background`. Each has a headless
  equivalent (table below).
- **Peer bus is documented** (code.claude.com/docs/en/cross-session-messaging, 2.1.224+). A `-p`
  session binds its own inbox; inbound peer messages arrive as `user` messages with
  `origin.kind: "peer"`. A host must tag keyboard input `origin.kind: "human"`. The record file
  format under `~/.claude/sessions/` is still undocumented.
- **Traps**: answer every unknown `control_request` with an error reply (vibe-kanban parks them in
  `Other`, the CLI waits forever); hold stdin open yourself (SDK issues #376, #384, #385, #438 are
  the SDK closing stdin early); the SDK sets `CLAUDE_CODE_ENTRYPOINT=sdk-ts`, which hides sessions
  from `claude --resume` (Happy overrides it); todo tools are off by default on current models
  (`CLAUDE_CODE_ENABLE_TODO_TOOLS=1`).
- **Terms** (legal-and-compliance page, fetched 2026-10-06): a user signing in to the unmodified
  binary with their own subscription is allowed. The host must not remove login methods, pay for,
  resell, or intermediate usage, collect tokens, or ship a "log in with Claude" button, and may say
  it "runs Claude Code" but not use the name in its own feature name. Shipping our own copy of the
  binary needs the Commercial Terms. Spawn the user's `claude`, never read credentials.

# Codex surface (0.160.1)

- **Transport**: `codex app-server` over stdio (NDJSON) or `--listen unix://PATH` (WebSocket over a
  Unix socket). A bare `unix://` means the shared daemon socket; always pass a private path. JSON-RPC
  without the `jsonrpc` field. No protocolVersion; the version is in `initialize.userAgent`.
- **Size**: 104 stable plus 63 experimental client requests, 11 server requests, 83 notifications.
  Schema checked in at `codex-rs/app-server-protocol/schema/{json,typescript}`.
- **Threads**: `thread/{start,resume,fork,read,list,turns/list,items/list,revert,inject_items,
  compact/start,archive,name/set,goal/*}`; experimental `thread/queue/*`,
  `thread/backgroundTerminals/*`, `thread/fork.beforeTurnId`, `collaborationMode`,
  `multiAgentMode`. `thread/rollback` was removed on 09-11 for `thread/revert`.
- **Turns**: `turn/start`, `turn/steer {expectedTurnId}`, `turn/interrupt`. `review/start`.
- **Items**: agentMessage, reasoning, plan, commandExecution, fileChange, mcpToolCall,
  dynamicToolCall, collabAgentToolCall, subAgentActivity, webSearch, imageView, imageGeneration,
  contextCompaction, enteredReviewMode/exitedReviewMode, hookPrompt, sleep. Deltas for messages,
  reasoning, command output, patches; `turn/plan/updated`; `thread/tokenUsage/updated`.
- **Server requests**: command/fileChange/permissions approvals, `item/tool/requestUserInput`
  (questions, `isBlocking`, `autoResolutionMs`), MCP elicitation, `item/tool/call`.
  `serverRequest/resolved` tells every other client a request was answered.
- **TUI commands** (`tui/src/slash_command.rs`, 63 variants): `/side` and `/btw` both exist and are
  the same thing, `thread/fork` with `ephemeral=true` then `thread/inject_items` with a hidden
  boundary prompt; the side thread answers only, no edits or subagents (`app/side.rs:637-790`).
  `/fork` = `thread/fork`, `/review` = `review/start`, `/compact`, `/plan` (collaboration mode),
  `/agents`, `/subagents`, `/goal`, `/ps` and `/stop` (background terminals).
- **Subagents**: `multi_agent` stable and on. Each subagent is its own thread with
  `parentThreadId`, `agentNickname`, `agentRole`; the parent sees `collabAgentToolCall` and
  `subAgentActivity`.
- **Sharing a thread with the real TUI**: a private `--listen unix://` server, zz connects, then
  `codex --remote unix://PATH resume <id>` in a pane (hapi does this). Approvals go to every
  subscriber. A bare `codex` TUI auto-attaches to a discovered shared daemon, so a zz-launched TUI
  must always get `--remote`.
- **Churn, 90 days**: 315 commits to `app-server-protocol`, 40 stable releases, about seven breaking
  removals, the rest additive. Development happens in a private repo mirrored to GitHub.

# The requested features, per vendor

| Feature | Claude Code | Codex | Notes |
|---|---|---|---|
| Background agents | Agent tool `run_in_background` (default true), `task_*` events, `background_tasks_changed`, `stop_task`, `get_task_output`, Bash `run_in_background`, Monitor; set `perTaskStopAffordance` or interrupt kills them | `collabAgentToolCall`, `subAgentActivity`, subagent threads; `thread/backgroundTerminals/*` (exp) | Both give real ids; render as child threads |
| Question tool | `can_use_tool` for `AskUserQuestion`, answer `allow` + `updatedInput {questions, answers}`; also `elicitation` | `item/tool/requestUserInput` (exp), MCP elicitation | t3code panel, oh-my-pi schema |
| Plan mode | `set_permission_mode: plan`, `planModeInstructions`; `ExitPlanMode` arrives via `can_use_tool` with `plan` | `collaborationMode: plan` on `turn/start`, `item/plan/delta` | Proposed-plan card, "implement" |
| btw / side | `side_question` control request (untyped, sent by Desktop and VS Code) | `thread/fork ephemeral` + `inject_items` boundary prompt | Answer in a sheet, never in history |
| fork | `forkSession(upToMessageId)` (port the MIT Python) or `--resume X --fork-session` | `thread/fork` | New pane or tab with lineage |
| branch from a message | fork + `--resume-session-at <uuid>` | `thread/fork beforeTurnId` (exp) | "Branch here" on any row |
| rewind | `rewind_files` (needs file checkpointing) + truncating resume | `thread/revert` | Files: Write/Edit only on Claude |
| compact | send `/compact` as a prompt, `compact_boundary` back | `thread/compact/start` | |
| review | `/code-review` skill headless | `review/start` | |
| steer vs queue | user message `priority now` vs `next`/`later` | `turn/steer` vs `thread/queue/*` (exp) or local queue | t3code: steer, queue, restart |
| Slash list | `init.slash_commands` + `commands_changed` | fixed list + `skills/list` | |
| Inter-session | documented peer bus; hosted session joins natively | daemon owns the connection: `turn/start`/`turn/steer` direct | Replaces `codex queue` 10 s path |

# What zz has today

From the code map (main, 3825bfee6):

- **The UI model is already zz-owned; only the reducer input is ACP.** The daemon ships ACP
  `session/update` as `serde_json::Value`; desktop (`zz/src/agent/controller.rs`), gpui-shared
  (`clients/gpui-shared/src/agent_pane.rs`), and FFI (`zz-client-ffi/src/ffi/agent.rs`) each decode
  it and reduce into `AgentThreadEntry` (User, Assistant, Reasoning, Tool, Plan).
- **ACP-only code**: about 6.5-7.5k non-test lines (`agent/runtime.rs` 1,852, ACP decode and
  session ops in `controller.rs` ~1.5k, gpui-shared ~800, FFI ~600, `agent_transcript.rs`
  translation ~520, plus catalog, config, environment) and about 5k lines of tests on the fake agent.
- **Vendor-neutral and kept**: ~11k lines (host pane threads and prompt queue, fanout with 25 ms
  coalescing, wire seq and replay ring, journal, mailbox lane, attention, peer bus,
  `codex_queue.rs`, most of `view.rs`) plus ~12k of widgets: custom markdown on markdown-rs with
  tables and streaming repair (`mend.rs`), mermaid via merman to themed images, tree-sitter
  highlighting (only six grammars), line diffs via `similar`.
- **Missing**: question UI (everything goes through the permission wizard), subagent and background
  display (removed at protocol v56 on 08-16), images in assistant output (placeholders), word-level
  diffs, local slash commands, PiP. `display-popup` exists (`FloatingSurface`) but hosts only a
  terminal; floating panes are a format flag only.
- **Reusable from history**: tag `archive/codex-host` holds `codex_host.rs`, the 09-10 tokio-
  tungstenite app-server client with thread binding, steer, and approval answering.

# Proposed shape

**Drivers in the daemon, normalized there, typed on the wire.**

```
daemon
  AgentDriver (one per pane process)
    ClaudeDriver   user's `claude`, Desktop-tab flags, stream-json + control protocol
    CodexDriver    `codex app-server` (stdio, or a private unix socket for --remote)
    AcpDriver      today's runtime.rs, for Gemini/opencode/Cursor/Grok/omp (optional)
  -> AgentEvent (zz-owned, closed core + Raw{vendor, json})
  -> existing fanout / journal / replay ring / mailbox lane
clients (desktop, gpui-shared, FFI, TUI)
  reduce AgentEvent -> AgentThreadEntry, no vendor JSON anywhere
```

- **Event core**, borrowed from t3code and oh-my-pi: item upserts keyed by native id
  (started/updated/completed), turn state separate from prompt ack (Claude
  `session_state_changed`, Codex `turn/completed`), runtime requests `{approval | question | plan}`
  with a response capability, subagents as child threads with a parent link, tasks for background
  work, notices (compaction, retry, rate limit), and `Raw` for anything unknown, rendered as a
  collapsible row and never dropped. Per-driver capability flags gate UI affordances.
- **Typed wire wins on its own**: the JSON blobs exist because postcard could not carry the acp
  crate's serde_json types. Once the daemon normalizes, three clients stop parsing vendor JSON.
- **Commands**: `/` lists the vendor's own commands (Claude `init.slash_commands`, Codex list plus
  skills) and passes them through. A small set of zz-level verbs (fork, branch, side/btw, rewind,
  compact, review, plan) maps per driver through the table above and gets dedicated UI: side answers
  in a sheet, fork and branch open a new pane with lineage, rewind on any message.
- **Orchestrator backbone**: agents already drive zz through the `zz` CLI and the zz-workspace
  skill. The t3code pattern on top of that is to inject a `zz` MCP server into every driven session
  (Claude `mcpServers` in `initialize`, Codex thread config) generated from the command catalog,
  so tools stay in step with verbs: spawn a pane (PiP), send, wait, read output, list. Rules from
  t3code: a child gets only its task prompt, never wider permissions; completions wake the parent
  queued behind its active turn, batched, at-least-once with a stable id. Subagent child threads
  and agent-spawned panes render as PiP, which needs a real floating-pane layer in the mux.
- **Inter-session chat**: hosted Claude sessions join the documented peer bus themselves (this also
  removes the 09-10 trap where the ACP adapter's inner Claude registered a shadow peer). Codex
  panes get direct `turn/start`/`turn/steer` from the daemon's own connection, replacing the
  10-second `codex queue` route. `agent-send`, `agent-respond`, and `wait-for` stay
  vendor-neutral verbs.

**Keep-up rules**: user's binary plus a min-version table; tolerant serde with `#[serde(other)]` and
raw fallback; reply to every unknown control request; capability probing; NDJSON replay fixtures
per scenario per vendor with a recorder; a scheduled job that diffs `sdk.d.ts` between SDK releases
and `schema/json` between Codex releases and opens an issue. Budget about one driver fix a week
across both (t3code, with a team, made 67 Claude and ~67 Codex adapter commits in 90 days).

**Rough size**: Claude driver ~2k lines, Codex driver ~2k (part of it from `archive/codex-host`),
event core and reducer ~1k, new UI (question card, child threads, tasks tray, side sheet, fork and
branch actions) ~2.5k, against ~7k ACP lines and ~5k ACP tests removed. Estimates, not counts.

# Why this differs from 08-16 and 09-21

- **08-16** (protocol v56) dropped the high-fidelity pane because zz parsed Claude SDK messages
  passed through claude-agent-acp's `_claude/sdkMessage` channel: two moving layers, and the
  adapter dropped `task_*` events by default. Driving the vendor wire directly removes one layer;
  it is the wire Anthropic's and OpenAI's own GUIs use.
- **09-17 to 09-21** (lens) was a read-only view over TUI transcripts with no control path
  (questions, approvals, steering). Different problem.
- **09-10** withdrew `split-codex` because it put vendor behavior into zz mux verbs. Here vendor
  behavior stays inside drivers and the pane's command menu; mux verbs stay vendor-neutral.

# Open decisions

1. Keep an ACP driver for the long tail, or Claude and Codex only? Keeping it costs a few hundred
   lines of mapping into the new events.
2. Is `//` meant as the zz-level verb prefix (fork, branch, side, btw) next to vendor `/`?
3. Codex transport: per-pane stdio (simplest), or a private unix socket so the same pane can flip to
   the real TUI with `codex --remote`.
4. Order: Claude driver first, then Codex, then MCP injection and PiP.
