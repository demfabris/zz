---
type: Design Plan
title: Native agent drivers
description: Agent panes drive each vendor's own protocol from the daemon. Claude Code runs over stream-json and its control protocol (on main 2026-10-07), Codex will run a private `codex app-server` per pane, and ACP stays for the agents that speak it natively, all emitting the stream the shared reducer already renders.
status: In progress (Claude Code driver, zz verbs, question cards, subagent rows, and the task tray landed 2026-10-07; side answers, the Codex driver, and orchestration remain)
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

# Next

1. Stream vocabulary in v108: a raw row for unknown frames and side answers. The question card,
   subagent child rows, and the background task tray ship on desktop and gpui-shared through the
   shared zz-ui widgets (`agent/question.rs`, `agent/tasks.rs`, the step fold in `agent.rs`); FFI
   carries them in the agent snapshot (`permissions[].questions`, `tasks`, each entry's `parent`)
   and answers them with `zz_client_agent_answer_question` and `zz_client_agent_stop_task`.
2. `//` verbs: side and btw (`side_question`), fork (`--resume X --fork-session`), branch
   (`--resume-session-at`), rewind, steer (`priority: "now"`).
3. Turns the CLI starts on its own (a background task finishing, a peer message) do not reach the
   host's phase yet, so badges miss that work.
4. Codex driver: a `codex app-server` stdio child per pane, starting from `archive/codex-host`.
5. Orchestration: a zz MCP server passed through `mcpServers` and the Codex thread config; PiP
   panes.
6. Keep-up: a weekly diff of `sdk.d.ts` and the Codex schema, and a minimum-version table.
