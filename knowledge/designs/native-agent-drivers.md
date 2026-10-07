---
type: Design Plan
title: Native agent drivers
description: Agent panes drive each vendor's own protocol from the daemon. Claude Code runs over stream-json and its control protocol, Codex over a private `codex app-server` per pane, and ACP stays for the agents that speak it natively, all emitting the stream the shared reducer already renders.
status: In progress (Claude Code and Codex drivers, zz commands, and question cards, the task tray, and nested subagent steps on every client landed 2026-10-07; orchestration remains)
resource: crates/zz-daemon/src/agent/claude/mod.rs
tags:
- agent
- claude-code
- codex
- acp
- daemon
timestamp: 2026-10-07T00:00:00-03:00
---

# Why

The ACP pane lagged Claude Code and Codex: fork broken, steering hangs, subagents and background
work flattened. Every harness with a rich UI speaks each vendor's own protocol, the same one the
vendor's own apps use ([survey](/research/2026-10-06-native-agent-drivers-survey.md)).

# Decisions (fabrico, 2026-10-07)

1. ACP stays as a fallback driver for agents that speak it natively (Gemini, opencode, Cursor,
   Grok, oh-my-pi). A configured command whose program is not `claude` runs as ACP.
2. `/` passes the vendor's own commands through. `//` is zz's own verbs (fork, branch, side, btw),
   mapped per vendor inside the driver.
3. Codex does what the other apps do. t3code, superset, happy, vibe-kanban, CodexMonitor, and
   Codex Desktop each spawn one private `codex app-server` child over stdio and send
   `experimentalApi: true`. Only hapi puts a socket in the path, to attach the real TUI to the same
   thread. zz runs one stdio child per pane.
4. Order: the Claude driver, then Codex, then orchestration (a zz MCP server injected into driven
   sessions, PiP panes).

# Shape

The driver boundary is the existing `PaneRunner` in `crates/zz-daemon/src/agent/host.rs`. A runner
takes `RuntimeCommand` and `RuntimeControl` and emits `AgentStreamPayload`. The Claude runner keeps
the ACP runtime's contract, so the host queue, fanout, journal, text projection, hooks, and all
four clients work unchanged.

Transcript atoms stay ACP `SessionUpdate` JSON. It is a published schema with tolerant decoding,
and `zz-client/src/agent_transcript.rs` already renders it for every client. The lag was in the
adapters, not in that schema. The survey proposed a closed zz event core typed on the wire; that is
still the plan for what ACP cannot carry (question cards, subagent threads, the task tray, side
answers), as zz-owned stream variants in protocol v108. Rewriting the transcript atoms first would
churn about 12k lines of reducers and views with no visible change.

# Claude Code driver

`crates/zz-daemon/src/agent/claude/`: `mod.rs` is the process loop, `translate.rs` maps frames to
updates, `sessions.rs` reads Claude's own session files.

- **Launch.** The user's `claude`, found on the repaired login-shell PATH, with the flags
  Anthropic's Desktop Code tab uses: `-p --output-format stream-json --input-format stream-json
  --verbose --include-partial-messages --replay-user-messages --permission-prompt-tool stdio
  --await-initialize`. zz leaves out `--setting-sources=user`, so project and local settings apply
  as they do in the TUI. Extra arguments in `agent-claude-code-command` are appended, and `--model`
  or `--permission-mode` there seed the pickers.
- **Environment.** `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS=1`, the workspace identity
  (`ZZ_PANE` and friends), and the daemon's parent-Claude scrub list removed.
  `CLAUDE_CODE_ENTRYPOINT` stays unset, so sessions record `sdk-cli` and the `claude --resume`
  picker hides them; resuming by ID still works, and zz lists them itself. Happy sets
  `remote_mobile` to get past that filter; zz does not claim another product's entrypoint.
- **Process.** Each child gets its own process group. Shutdown closes stdin, then sends SIGTERM and
  finally SIGKILL to the group, two seconds apart.
- **Sessions.** A new pane passes `--session-id <uuid>`. Restart and resume pass `--resume <id>`
  and replay history from Claude's JSONL along the live branch (the SDK's `getSessionMessages`
  rules), then rewrite the zz journal from it; the journal is the fallback when the file is gone.
  The session list reads `~/.claude/projects/<sanitized cwd>/*.jsonl` with the SDK's
  `listSessions` rules. zz does not offer delete.
- **Turns.** A prompt is a `user` frame with a fresh UUID and `origin.kind: human`. The turn ends on
  `command_lifecycle` `completed` or `cancelled` for that UUID, with `session_state_changed: idle`
  as the fallback. Cancel sends `interrupt` and settles the turn at once.
- **Permissions.** `can_use_tool` becomes a permission request: Allow, Always allow when Claude
  offers rules, Reject. `AskUserQuestion` becomes one request per question with its choices as
  options, answered through `updatedInput.answers`. `ExitPlanMode` offers accept edits, approve, or
  keep planning, through `updatedPermissions` `setMode`. The pane's auto-approve tier still applies,
  judged on the mapped tool kind. Every other control request gets an error reply, because the CLI
  otherwise waits forever.
- **Settings.** Model from `initialize.models`; mode from default, acceptEdits, plan, and auto when
  the model supports it; effort from the model's `supportedEffortLevels`. They apply through
  `set_model`, `set_permission_mode`, and `apply_flag_settings {effortLevel}`.
- **Rows.** Titles and kinds follow claude-agent-acp. Edit, Write, and MultiEdit render diffs from
  their input. TodoWrite and the Task tools become the plan. Subagent tool calls show inline with
  `_meta.claudeCode.parentToolUseId`; subagent text stays out of the main thread.
- **Peer bus.** The CLI registers its own session record, so the daemon no longer registers a
  `PeerInbox` for a native Claude pane.
- **Tests.** The translator replays three scrubbed real recordings in `agent/claude/fixtures`
  (CLI 2.1.292, 2026-10-07). An end-to-end test runs the loop against a fake `claude` script.

Verified live on 2026-10-07 through a private daemon and `zz_cli`: a plain reply, a tool turn that
edited files, an `AskUserQuestion` answered with `agent-respond`, and `restart-agent-pane` resuming
the session with its memory.

# zz commands

A prompt that starts with `//` is a zz command when the driver's `Ready` capabilities set `verbs`.
The host hands it to the runner outside the turn queue, so it works while a turn runs; `//steer` on
an idle pane becomes a plain prompt. The fanout keeps commands out of projected turn headers.

| Command | Claude Code | Codex |
| --- | --- | --- |
| `//btw`, `//side` | `side_question`; the answer never enters the conversation | an ephemeral `thread/fork` answers read-only, then is dropped |
| `//steer <text>` | `user` frame with `priority: "now"` | `turn/steer` |
| `//fork` | respawn with `--resume X --fork-session --session-id NEW` | `thread/fork` |
| `//rewind [n]` | the same fork with `--resume-session-at` set to the entry before the nth-last prompt | `thread/fork` with `beforeTurnId` |
| anything else | lists the commands | lists the commands |

`//rewind [n]` continues from before the last n prompts (1 by default) in a copy, so the full
conversation stays resumable; files on disk are not touched. Rewinding every prompt opens a new
session. A Claude fork has no session file until its first prompt, so the driver records the fork
(`_meta.zz.fork` on its notice) in the pane's journal, and a restart before that prompt forks again
under the same id.

# Background work, questions, subagents (v108)

Background agents and commands stay `in_progress` in their own rows after launch and finish with
their result when `task_notification` arrives (the agent's summary, or the tail of the command's
output file); `task_progress` summaries update an agent row while it runs. `background_tasks_changed`
becomes the pane state's `tasks` list (ambient watchers left out), and `AgentStopTask` sends
`stop_task`; the driver declares `perTaskStopAffordance`, so stopping a turn spares background
agents. `AskUserQuestion` (and Codex `item/tool/requestUserInput`) is one permission request with
a `questions` list, answered by `AgentAnswerQuestion`. Subagent tool calls carry
`_meta.zz.parent`, and the shared reducer exposes it as `tool_parent`. The wire details are in the
v108 entry of the [wire protocol](/protocol/wire-protocol.md).

Desktop and gpui-shared render these through shared zz-ui widgets: the question card
(`agent/question.rs`), the task tray chip in the composer (`agent/tasks.rs`), and the step fold in
`agent.rs`, which nests subagent steps under their agent row, collapsed to a step count. FFI carries
them in the agent snapshot (`permissions[].questions`, `tasks`, each entry's `parent`) and answers
with `zz_client_agent_answer_question` and `zz_client_agent_stop_task`. The card keeps answers
inside the wire limits before Submit, and the protocol traces print a card's answer counts, never
its text.

Turns the agent starts on its own (a background task reporting back, a peer message) arrive as
`Activity { busy }` stream items: Claude's `session_state_changed` and Codex's
`thread/status/changed` while no host turn is open. The host shows the pane as running, holds new
prompts in its queue until the agent is idle, and Stop sends a `Cancel` with turn 0, which the
drivers turn into an interrupt.

# Codex driver

`crates/zz-daemon/src/agent/codex/`, sharing the process plumbing in `agent/child.rs` with the
Claude driver. One `codex app-server` child per pane over stdio, as the other apps do, with
`experimentalApi: true`. `agent-command` defaults to `codex`; extra arguments go after
`app-server`.

- **Threads.** A new pane runs `thread/start {cwd}`; resume and switch run `thread/resume` and
  replay `thread/read {includeTurns}` items; a failed resume starts a new thread. The approval
  policy and sandbox come from the user's `~/.codex/config.toml` until the user picks a mode.
- **Turns.** `turn/start` with text and data-URL images; `turn/completed` settles the host turn
  (`interrupted` reads as cancelled); Cancel sends `turn/interrupt`. `/compact` runs
  `thread/compact/start` and `/review` runs `review/start` inline.
- **Requests.** Command, file-change, and permission approvals become permission requests (Allow,
  Always allow this session, Reject); the legacy `execCommandApproval` and `applyPatchApproval`
  answer in their own decision shape. `serverRequest/resolved` withdraws a request another client
  answered. Elicitations are declined and unknown requests get a method-not-found error.
- **Items.** Agent messages and reasoning stream from their deltas; command executions become
  execute rows (read and search actions get read and search rows) with streamed output and a
  failed status on a non-zero exit; file changes render their unified diffs as old and new text;
  MCP and dynamic tool calls, web searches, and collab agent calls get rows; `turn/plan/updated`
  is the plan; `thread/tokenUsage/updated` is the usage meter.
- **Subagents.** Codex streams a spawned agent's thread on the same connection. A
  `subAgentActivity` item (or a `spawnAgent` collab call's `receiverThreadIds`) opens an agent row
  and maps the child thread to it; the child's tool items become rows with `_meta.zz.parent`, so
  they nest as steps, and its latest message becomes the agent row's content. Live only: a resumed
  thread shows the agent rows but not their steps, which live in the child threads.
- **Settings.** Model and effort from `model/list`; the mode presets read-only, auto, and full
  access map to `approvalPolicy` and `sandboxPolicy` on the next `turn/start`.
- **Sessions.** `thread/list` filtered by cwd.

Verified live on 2026-10-07 with Codex 0.159.0: a plain reply, a turn with two approvals and both
file changes applied, `restart-agent-pane` resuming the thread with its memory, and `//fork`.

# Next

1. A "rewind to here" action on prompt rows that sends `//rewind n`.
2. Codex background terminals (`thread/backgroundTerminals/*`) in the task tray, and subagent
   steps on resume (`thread/read` of each child thread).
3. Orchestration: PiP panes and a decision on in-session tools. The groundwork is in:
   `agent-send --notify` submits to another agent pane and, when that turn ends, posts the reply
   back into the calling agent pane as a prompt queued behind its own turn (t3code's async
   completion, over the CLI agents already use). Both vendors can also host tools without a
   process: Claude through `sdkMcpServers` and `mcp_message` control requests, Codex through
   `dynamicTools` on `thread/start` and `item/tool/call`.
4. Keep-up: a weekly diff of `sdk.d.ts` and the Codex schema, and a minimum-version table. The
   survey's `archive/codex-host` tag is not in this clone; the Codex driver was written fresh.
5. Small UI edges: a subagent step split from its agent row by assistant text does not nest, and
   gpui-shared's Enter on an ordinary permission can also send attached images.
