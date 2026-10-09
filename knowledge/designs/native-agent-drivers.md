---
type: Design Plan
title: Native agent drivers
description: Agent panes drive each vendor's own protocol from the daemon. Claude Code runs over stream-json and its control protocol, Codex over a private `codex app-server` per pane, and ACP stays for the agents that speak it natively, all emitting the stream the shared reducer already renders.
status: In progress (Claude Code and Codex drivers, zz commands, question cards, the task tray and subagent steps landed 2026-10-07; the minimal pane with one trace line per turn landed 2026-10-08; the Changes pane and orchestration remain)
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
2. zz's own verbs (`/btw`, `/side`, `/steer`, `/fork`, `/rewind`) share the `/` menu with the
   vendor's commands and are mapped per vendor inside the driver; every other `/` command passes
   through to the vendor. (The 2026-10-07 build used a `//` prefix; that was a misreading of an
   escaped `/` and was dropped on 2026-10-08.)
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

A prompt `/name …` whose name is one of `zz_protocol::agent_stream::AGENT_VERBS` is a zz command when
the driver's `Ready` capabilities set `verbs`; any other `/` command goes to the vendor, and the
composer's `/` menu lists the vendor's commands with zz's in place of any it intercepts
(`zz_client::agent_completion::pane_commands`).
The host hands it to the runner outside the turn queue, so it works while a turn runs; `/steer` on
an idle pane becomes a plain prompt. The fanout keeps commands out of projected turn headers.

| Command | Claude Code | Codex |
| --- | --- | --- |
| `/btw`, `/side` | `side_question`; the answer never enters the conversation | an ephemeral `thread/fork` answers read-only, then is dropped |
| `/steer <text>` | `user` frame with `priority: "now"` | `turn/steer` |
| `/fork` | respawn with `--resume X --fork-session --session-id NEW` | `thread/fork` |
| `/rewind [n]` | the same fork with `--resume-session-at` set to the entry before the nth-last prompt | `thread/fork` with `beforeTurnId` |
| a verb without its argument | lists the commands | lists the commands |

`/rewind [n]` continues from before the last n prompts (1 by default) in a copy, so the full
conversation stays resumable; files on disk are not touched. `/rewind <id>` goes back to before
the prompt row with that message id: a prompt row's id is the id the vendor stores (the `uuid` zz
puts on Claude's user frame, which lands in the session file, and Codex's `clientUserMessageId`,
which comes back as the item's `clientId`), so live and replayed rows match. Rewinding every prompt opens a new
session. A Claude fork has no session file until its first prompt, so the driver records the fork
(`_meta.zz.fork` on its notice) in the pane's journal, and a restart before that prompt forks again
under the same id.

Prompt rows offer **Rewind to here** on hover while the pane is idle and its driver sets `verbs`;
it sends `/rewind <id>` through the composer's send path. The shared reducer keeps each prompt
row's message id (`AgentThreadEntry::User.message_id`; a row the client added itself takes it from
the daemon's echo) and `rewind_id` skips rows that start with `/`, since vendor and zz commands are
not rewind points. Clients send a zz command without opening a local turn or prompt row, because
the daemon echoes the command itself and never queues it.

# Timeline status and zz replies (2026-10-08)

- **Tool status.** A tool step's glyph spins while it runs in the live turn, turns warning while it
  waits for approval, and turns danger when it fails, with an `exit N` tag when the driver sent
  `_meta.zz.exitCode` (Codex `exitCode`, Claude's `Exit code N` line on a failed Bash) and `failed`
  otherwise. A canceled step gets a `canceled` tag.
- **zz replies.** The drivers put `_meta.zz.reply` (the echoed command's message id) on a `/btw`
  answer and on a command's notice. The shared reducer records it as
  `AgentThreadEntry::Assistant.aside { side, reply_to }`, and the timeline folds the reply into the
  command's prompt row (`TimelineGroupKind::Reply`), drawn as a popover under the bubble. A side
  answer is captioned "Side answer · not in the conversation". A notice whose command row is gone,
  such as the note a fork leaves in the new session, is one muted line with an info glyph.
- **Plan.** The plan's `- [ ]`, `- [~]`, `- [x]` lines render with their own markers: pending is an
  empty box, in progress an accent box with a dot and medium text, done a filled check with muted,
  struck-through text.
- **Turns.** A prompt row that is not the first gets 16px more space above it.
- The task tray chip's loader spins on the shared pulse clock, like the empty state's.

# Minimal pane (2026-10-08)

fabrico's direction: the agent pane does the conversation and nothing bigger. Anything larger than a
line (a command's output, a file, later a diff) opens as its own zz pane, and the same widgets have
to work on the desktop and the phone. The design board is the "Quiet timeline" artifact.

- **One row per turn.** Every thought, tool call, plan update and message after a prompt folds into
  one `TimelineGroupKind::Turn` row (`timeline_group_kind` takes `Assistant` without an aside and
  `Plan` too). `split_turn` cuts it into the trace and the answer: the messages after the last
  thought or tool call. Text followed by another tool call is narration and stays in the trace.
- **The trace line.** While the turn runs it shows the current step (the running tool's label,
  "Thinking", or "Waiting for you" with the tool that needs approval), the step count and a clock,
  and the end of the latest thought in two muted lines. After the turn it reads "Worked 2m 14s"
  with no leading icon, then one icon per kind (read, search, edit, command, fetch, other tool,
  agent, and a red warning for failures), each with its count in a badge on its top right and a
  tooltip naming it ("3 commands"). The clock lives in `AgentTimelineStore` (`tick_turn_clocks`,
  run on every timeline render), so turns replayed from history show no time.
- **Opened.** A click lists the turn in order: "Thought" with its text, narration in muted 12px, and
  one line per tool. Subagent steps are counted on their agent's line ("N steps"); their own view is
  the PiP work, parked.
- **Failures.** When the turn ended on a failed tool, that tool's line and its last two output lines
  stay under the trace.
- **Open output.** A finished command with output opens it in a split beside the agent: the clients
  run `split-window -h -T <command> sh -c 'printf %s "$0" | base64 -d | less -R --tilde +G' <base64>`
  (`zz_client::agent_output::output_pane_args`), so the text reaches `less` without any shell
  parsing it, capped to its last 256 KiB.
- **Dropped.** Tool input and output blocks, inline diffs and edit rows, the plan card in the
  timeline and nested step folds are gone, along with the tool content cache in the store. The
  Changes pane for diffs comes later; there is no editor pane yet, so reads open nothing.
- **Composer chips.** `task_tray` puts the plan chip ("Plan 2/4" and the current item) on the left
  and the background-task chip on the right; each opens its list above them (`TrayPanel`). The plan
  chip hides once every item is done and the agent is idle.
- **Header status.** `agent_status_pill` shows Running, Stopping, Waiting for you, Exited (with
  Restart, the same retry as the error card) and Offline next to the pane actions; idle shows
  nothing. Desktop and the web client wire it; the iOS client compiles the same source.

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

Desktop and zz-app render these through shared zz-ui widgets: the question card
(`agent/question.rs`), the task tray chip in the composer (`agent/tasks.rs`), and the step fold in
`agent.rs`, which nests subagent steps under their agent row, collapsed to a step count. A step
leaves its place in the timeline for its agent's row wherever it arrives, before the agent or after
other rows, and steps keep their arrival order; a loop of parent links renders flat. FFI carries
them in the agent snapshot (`permissions[].questions`, `tasks`, each entry's `parent`, a prompt
row's `message_id`) and answers
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
  they nest as steps, and its latest message becomes the agent row's content. Resuming a thread
  reads each child thread (up to 16) before the replay and puts its steps right after their agent
  row.
- **Background terminals.** A command Codex leaves running stays an open row after its turn. The
  driver lists `thread/backgroundTerminals/list` when a turn completes and puts each one in the
  task tray (kind `shell`, its process id, its row); the row's `item/completed` takes it out, and
  Stop runs `thread/backgroundTerminals/terminate`. Codex sends no change event, so the list is not
  polled between turns.
- **Settings.** Model and effort from `model/list`; the mode presets read-only, auto, and full
  access map to `approvalPolicy` and `sandboxPolicy` on the next `turn/start`.
- **Sessions.** `thread/list` filtered by cwd.

Verified live on 2026-10-07 with Codex 0.159.0: a plain reply, a turn with two approvals and both
file changes applied, `restart-agent-pane` resuming the thread with its memory, and `/fork`.

# Tested versions

| Agent | Version | Since |
| --- | --- | --- |
| Claude Code | 2.1.293 (Agent SDK types 0.3.293), live tests on Haiku 5.5 | 2026-10-07 |
| Codex | 0.159.0 (`multi_agent` on by default), live tests on GPT-6-Luna | 2026-10-07 |

Older CLIs may lack frames or methods the drivers use: `side_question`, `stop_task`,
`--resume-session-at`, `session_state_changed`, and `background_tasks_changed` on Claude Code;
`beforeTurnId`, `clientUserMessageId`, `subAgentActivity`, and `thread/backgroundTerminals/*` on
Codex. The drivers have not been run against older versions.

# Next

1. The Changes pane: a pane type for a session's diffs that the trace's edit steps open.
2. Orchestration: PiP panes and a decision on in-session tools. The groundwork is in:
   `agent-send --notify` submits to another agent pane and, when that turn ends, posts the reply
   back into the calling agent pane as a prompt queued behind its own turn (t3code's async
   completion, over the CLI agents already use). Both vendors can also host tools without a
   process: Claude through `sdkMcpServers` and `mcp_message` control requests, Codex through
   `dynamicTools` on `thread/start` and `item/tool/call`.
3. Keep-up: a weekly diff of `sdk.d.ts` and the Codex schema against the tested versions above.
   The survey's `archive/codex-host` tag is not in this clone; the Codex driver was written fresh.
