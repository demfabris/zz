---
type: Design Plan
title: Agent lens - a rich view over the real CLI
description: Decision record and plan for replacing the ACP agent pane with a read-only rich view of a Claude Code, Codex, or pi session that runs unmodified in an ordinary terminal pane - the transcript file the CLI already writes is the source, three small readers built from the vendors' own code are the translation layer, and input goes back through the pane.
status: Proposed (2026-09-17, readers not started)
tags:
- agent
- agent-pane
- acp
- claude-code
- codex
- pi
- transcript
- design-plan
timestamp: 2026-09-17T22:30:00Z
---

# Overview

The agent pane speaks ACP through the Zed-maintained `claude-agent-acp` and `codex-acp` adapters.
That has three costs that do not go away with adapter bumps: the stable protocol (still
`protocolVersion: 1`) has no mid-turn steering, no queue editing, no subagents and no
agent-initiated work; the adapters are 18k and 26k lines of TypeScript that trail every vendor
release; and a user of the pane never sees the vendor's own TUI features on the day they ship.

Decision (fabrico, 2026-09-17): stop hosting agents. Run the real `claude`, `codex` or `pi` in a
terminal pane, and render the rich view from the transcript file the CLI writes to disk. zz keeps
its rule from the peer-bus work: it speaks what an agent already listens on and hosts nobody's loop.

The lens is a second view of a session that keeps running whether or not the lens can read it. When
a format change breaks a reader, the pane is still a terminal running the real CLI.

# What was ruled out

- **Screen scraping.** Markdown is already rendered to cells at pane width, so nothing rich can be
  recovered from the grid. State scraping is a treadmill: coder/agentapi was archived on
  2026-09-13 after Claude Code 2.1.83 broke its capture, Omnara dropped its wrapper, and luvus
  carries 3,200 lines of per-agent screen rules that its own comments admit keep breaking.
- **Native per-vendor protocols** (`claude -p --input-format stream-json`, `codex app-server`,
  `pi --mode rpc`). More power, but zz would host each vendor's loop again, which was withdrawn on
  2026-09-10, and a Claude subscription only works inside the unmodified `claude` binary.
- **Becoming a harness** like opencode, pi or fx. Those own the loop and call model HTTP APIs, which
  is why they are multi-model. zz is a host, not an agent.
- **ACP `session/load`** as the one wire format. It costs a Node subprocess per vendor, the replay is
  lossy, and load attaches to a session rather than viewing it.

# The translation layer

Nothing off the shelf reads Claude Code, Codex and pi transcripts into one model in a way zz can
adopt. Two surveys on 2026-09-17 (about 60 projects, six parsers run against the local corpus of
916 Claude files, 2,945 Codex rollouts and one pi session) settled the following.

- `skillsynchq/txcript` (Rust, Apache-2.0) is the only crate covering all three from disk. It is
  twelve weeks old with one author, has no tail API, drops message ids, sidechains and
  `toolUseResult`, and shows 97.6% of Codex tool calls as raw blobs.
- `kenn-io/agentsview` (Go, MIT) absorbs format changes fastest (about 150 contributors, its
  Claude and Codex parsers patched 5 to 13 times a month) but its parsers are unexportable and its
  model flattens content to one string. A port is 7k to 8k lines.
- The organisations with the most resources stopped hand-parsing Claude JSONL: Microsoft deleted
  its parser from Copilot Chat on 2026-03-17 after 36 fixes in eleven weeks and now calls the Agent
  SDK; Zed's adapter does the same. That SDK is not usable from Rust.
- No interchange format is real yet. ATIF (Harbor) has Python whole-file converters for Claude and
  Codex only; OpenTelemetry content export is opt-in per session with no history; Cursor's Agent
  Trace is code attribution.

The vendors' own readers are the solid sources, so zz writes three small tolerant readers from them.

## Codex, about 850 Rust lines

Every local rollout carries `session_meta.payload.history_mode = "paginated"`, and in that mode
Codex stores its own normalized items as `event_msg` / `item_completed` lines. Sixty recent files
held 1,561 `CommandExecution`, 1,367 `Reasoning`, 404 `SubAgentActivity`, 330 `AgentMessage`,
98 `FileChange`, 73 `McpToolCall`, 9 `ImageView` and 2 `ContextCompaction`. Third-party parsers
read the raw `response_item` records instead, where 83% of tool calls are one `exec` custom tool
whose input is a JavaScript script wrapping the real calls; reading `item_completed` needs no
unwrapping. `CommandExecutionItem` carries command, cwd, stdout, exit code and duration;
`FileChange::Update` carries a unified diff. Types: `codex-rs/protocol/src/items.rs`. Codex migrates
legacy rollouts in place (`thread-store/src/local/rollout_migration.rs`), so the legacy reducer is
not ported. A cold-file zstd compression flag exists and is off by default.

## pi, about 550 Rust lines

`earendil-works/pi` documents the format (`packages/coding-agent/docs/session-format.md`, version
3 with migrations from v1 and v2). The reader to port is
`packages/coding-agent/src/core/session-manager.ts`: the last entry in the file is the leaf, walk
`parentId` to the root, apply compaction entries. A v4 JSONL format exists under
`packages/agent/src/harness/session/jsonl/` and is experimental so far.

## Claude Code, about 950 Rust lines

Anthropic calls the format internal, and the wrapper fields around each line are the part that
changes (a viewer project logged 30 schema fixes in a year, nearly all "new line type broke a strict
parser"). The inner `message` is the public Messages API shape and parsed cleanly on all 203,039
local user and assistant lines. The reader therefore keeps only lines that carry a `uuid`, skips
line types it does not know, and never fails a file on one bad line.

Sources: the chain walk in `anthropics/claude-agent-sdk-python`
`_internal/sessions.py:897-1020` (MIT: terminals are entries nobody points to, prefer leaves that
are not `isSidechain`, `teamName` or `isMeta`, take the latest by file position, walk `parentUuid`
with a cycle guard); turn building and tool stitching in `microsoft/vscode`
`src/vs/platform/agentHost/node/claude/claudeReplayMapper.ts` (MIT); eleven real fixture
transcripts in the archived `microsoft/vscode-copilot-chat` `sessionParser/` tree. The TypeScript
SDK additionally recovers parallel tool results and skips history before the last compaction
boundary once a file passes about 5 MB; both are written by hand. Raw entries are kept so
`toolUseResult.structuredPatch` diffs and timestamps survive. Subagents live under
`<session>/subagents/agent-<id>.jsonl` with an `agent-<id>.meta.json` sidecar carrying the parent
link. 93% of thinking blocks on disk are signature-only, so a thinking row usually has a duration
and no text.

## Tailing

`microsoft/intelligent-terminal` `tools/wta/src/session_watcher/` (MIT, Rust) is the reference:
`notify` plus a byte offset per file. Claude rewrites the same `msg_` id across several lines while
streaming, so the tailer dedupes by `message.id`. Codex child rollouts begin with a copy of the
parent's history, which the reader skips.

## Watch list

Upstream files that move when a format moves: Claude `sessions.py` and the TypeScript SDK
changelog; Codex `codex-rs/protocol/src/items.rs`, `rollout/src/policy.rs` and
`app-server-protocol/schema/typescript/v2/ThreadItem.ts`; pi `session-manager.ts` and
`docs/session-format.md`. agentsview's commit log is where third-party breakage shows up first.
Regression fixtures: Entire CLI's before-and-after pairs under
`cmd/entire/cli/transcript/compact/testdata/`, the Microsoft fixtures above, and the Hugging Face
`format:agent-traces` datasets (licences vary).

# Pane to transcript

The daemon already tracks the foreground pid, cwd, tty and OSC title of every terminal pane
(`PaneRuntimeFacts`, `zz-mux/src/command.rs`), and `codex_terminal_context` in `daemon.rs` is a
worked example of turning those into an agent identity.

- Claude: `~/.claude/sessions/<pid>.json` names the session id (`claude_peers.rs` reads it); the
  transcript is `~/.claude/projects/<encoded cwd>/<session id>.jsonl`, with a glob on the session
  id as the fallback because `CLAUDE_CODE_PROJECT_DIR_NAME` can rename the folder.
- Codex: the running process holds its rollout file open (verified 2026-09-17 with `lsof` on a
  live `codex`: two `sessions/.../rollout-*.jsonl` descriptors, the parent thread plus a child).
  Read the pid's open descriptors (`/proc/<pid>/fd` on Linux, `proc_pidinfo` on macOS, where the
  daemon already uses it for the cwd) and take the rollout whose `session_meta` has no parent.
  Never derive a thread from rollout file mtimes; that route hit a live desktop session once.
- pi: newest session file under the cwd-keyed folder, confirmed by pid.

# The pane

There is no Agent pane kind any more. A terminal pane running a known CLI gains a lens: the same
pane rendered as a rich transcript instead of a cell grid, switched by a header toggle or a key.
The daemon detects the CLI from the pane's pid the way `claude_peers.rs` and `codex_queue.rs`
already do, so any terminal, local or over ssh, where the user types `claude` gets the lens
affordance without a separate command to spawn it. `new-agent-session` becomes a split that runs
the CLI with the lens on.

The daemon owns the reader, since it owns the pty, is where the files are local, and already has
the per-pane agent lane. Clients stay viewports. The reader emits the existing transcript model
(`AgentThreadEntry`: user, assistant, reasoning, tool with diff or text or terminal payloads,
plan) through `AgentStreamPayload::Update`, which is already an opaque JSON blob, so the wire
version does not move; only the reducer's decode of an ACP `SessionUpdate` changes. Replay after a
lag is a re-read of the file, so the daemon-side journal is redundant.

What the user sees, compared with the ACP pane:

- Same timeline widgets: markdown, highlighted code, diffs, mermaid, images, collapsed tool rows,
  the stick-to-bottom spring, sidebar badges, chimes. `crates/zz-ui` has no ACP reference and is
  untouched.
- Header: provider glyph, model, state chip, elapsed time and tokens from the usage records, and
  the lens/tui toggle.
- New rows the ACP pane never had: subagent activity (Claude sidecar files, Codex
  `SubAgentActivity`), Codex reasoning summaries, compaction boundaries, images inside tool
  results, the CLI's own session title.
- Text arrives per finished block (about 100 ms behind the TUI for Claude, per item for Codex),
  not per token. A long paragraph lands at once; the working state between blocks comes from the
  pid record and OSC title, with a pulse.
- Thinking rows carry a duration and usually no text.
- Composer: Send pastes into the pane; Queue uses the peer socket or `codex queue`; Stop sends the
  CLI's interrupt key. Slash commands are the CLI's own and are typed through.
- Permission card: allow and deny send the keys `agent-respond --on-block` already sends to a
  terminal pane. Anything richer, such as a multi-option question, hands over to the TUI view with
  focus.
- Gone: the history overlay (session list, switch, delete), the model, mode and config pickers,
  auth, the permission wizard's JSON-RPC path. Sessions, models and modes are the CLI's business
  through its own commands.

What changes in code:

| Area | Goes | Stays |
|---|---|---|
| `zz-daemon/agent/runtime.rs`, `fixture.rs`, `catalog.rs` | 2,400 lines of ACP JSON-RPC and adapter lifecycle | |
| `zz-daemon/agent/host.rs`, `fanout.rs` | session ops, auth, permission protocol, adapter spawn (~1,150) | pane threads, queue, git summary, lane, replay ring, publisher (~5,100) |
| `zz-daemon/agent/journal.rs` | all 1,610: the transcript file is the journal | |
| `zz-daemon/agent/environment.rs` | ACP wiring and npx warm (~300) | login-shell PATH probe (~480) |
| agent-pane-projection shadow terminal | all: the pane is a terminal, `capture-pane` works natively | |
| `zz-protocol` | `AgentSessionOpKind`, auth, config and mode ops, `PaneKind::Agent`, `AgentDescriptor` | `AgentUpdates`, `AgentState`, `AgentLagged`/`AgentReplay`, `AgentCommand`, `AgentPaneWire` |
| `zz/agent/controller.rs`, `view.rs` | ACP decode, session ops, pickers, auth, history overlay (~3,900) | transcript plumbing, composer, attachments, titles, timeline (~5,000) |
| `zz-client/agent_transcript.rs`, `agent_config.rs` | ACP translation (~900) | the model and capping (~380) |
| options | `agent-command`, `agent-claude-code-command`, `agent-auto-approve`, `experimental-agent-pane` | `@agent_state`, `#{agent_state}`, `agent-state-changed`, `zz wait-for`, `agent-send`, `agent-respond` |
| new | three readers (~2,300), tailer plus pid-to-file mapping (~500), lens toggle | |

Roughly 11,000 lines out and 3,000 in, with the 14,000-line widget layer and the wire unchanged.

# Input

The composer types into the pane through the verified bracketed-paste `send-text`, or through the
Claude peer socket (`/tmp/cc-socks/<pid>.sock`) and `codex queue --thread` where a queued delivery
is wanted. Permission prompts belong to the CLI: the lens shows them and hands over to the terminal.

# Open decisions

- Whether the lens is a toggle on one pane or a split beside the TUI. Toggle is the proposal.
- Whether the lens turns on by itself when a known CLI starts in a pane, or only on request.
- Whether ACP stays for agents that speak it natively (fx, opencode). The proposal deletes it and
  lets the lens grow readers instead; nothing in the widget layer depends on the answer.
