---
type: Research Report
title: Agent-to-agent messaging survey
description: How Claude Code, Codex, Gemini CLI, herdr, gastown, MCP Agent Mail, A2A, and ACP let one coding agent message another, with Claude Code's peer bus verified live on this machine, the delivery-timing taxonomy every tool converged on, and what it means for a zz message backbone.
tags:
- agent
- messaging
- claude-code
- codex
- herdr
- gastown
- a2a
- acp
- survey
timestamp: 2026-09-10T00:00:00Z
---

# Question

Claude Code sessions can now list and message each other. Does anyone else have this, what
delivery model did each tool pick, and what should zz build so that agent panes, Claude Code in a
terminal pane, and Codex in a terminal pane can talk?

# Method

Three web sweeps on 2026-09-10 (Claude Code docs, terminal-hosting orchestrators, protocol-side
ecosystems), then live probes on this machine with Claude Code 2.1.267 and Codex CLI 0.153.4 inside
throwaway zz daemons. Every "verified" claim below was observed, not read.

# Claude Code cross-session messaging (verified)

**Registry.** Each session writes `~/.claude/sessions/<pid>.json` and a `<pid>.<sha256>.key` token
file. Record fields seen: `pid`, `sessionId`, `cwd`, `startedAt`, `procStart` (ctime string in UTC),
`version`, `peerProtocol` (1), `peerFeatures` (`notify_idle`, `reply_across_default_dirs`,
`artifact_yield`), `kind` (`interactive`), `entrypoint`, `pidDomain`, `tmux` (`q:@0.%1` for a
session in a zz pane, read from `TMUX_PANE`), `messagingSocketPath`, `name`, `nameSource`,
`nameSince`, `status` (`idle` or `busy`), `updatedAt`, `statusUpdatedAt`. Discovery is a directory
scan; the binary adopts records with `kind == interactive`, `peerProtocol >= 1`, updated within 24 h.

**Socket.** One Unix socket per session at `/tmp/cc-socks/<pid>.sock` (or `$XDG_RUNTIME_DIR/cc-socks`,
or `/tmp/cc-socks-<uid>`), mode 0600. Protocol is newline-delimited JSON. An optional first line
`{"type":"auth","token":"<CLAUDE_CODE_MESSAGING_TOKEN>"}`; then message lines. The binary's own
debug log prints the injection recipe:

```sh
{ echo '{"type":"auth","token":"'"$CLAUDE_CODE_MESSAGING_TOKEN"'"}'
  echo '{"type":"user","message":{"role":"user","content":"hello"}}'; } | socat - UNIX-CONNECT:$SOCK
```

**Wire format between sessions.** Captured by registering a Python listener as a peer and messaging
it from a real session:

```json
{"msgV":1,"msg_id":"2845f58b-…","type":"user","priority":"next","from":"uds:/tmp/cc-socks/4712.sock",
 "message":{"role":"user","content":"<cross-session-message from=\"uds:/tmp/cc-socks/4712.sock\" from-name=\"agenticuse\" from-mode=\"bypass\">\n…\n</cross-session-message>"}}
```

Liveness probes arrive as empty connections first. Replies and idle notices travel back to the
`from` address as `peer_message_status` and `peer_idle_notice` actions.

**Delivery.** Between tool calls during a turn, never interrupting a tool; a new turn when idle.
The receiver shows a dim one-line preview and tells the model the text came from another session.
`notify_when_idle` is a one-shot subscription answered with the turn's end time and a one-line
status; it expires after 12 h. Inbound policy is `crossSessionInbound = accept | hold | refuse`; with
no setting, a prompting-class receiver delivers and holds only messages from bypass-class senders,
a bypass-class receiver holds everything except bypass-class senders. Loops are throttled: rate
limit per sender, duplicate drop, 50 queued, 100 held.

**Two facts that matter for zz, both verified:**

1. A plain same-user process can post a `type: user` line into any session's socket, no token, and
   the session treats it as a peer message ("Another Claude session sent a message") and acts on it.
2. A plain same-user process can register a record plus a socket and appears in `ListAgents` as a
   peer; real sessions send it the wire format above. The receiving side is whatever that process
   decides to do with the line.

# Codex CLI 0.153.4

Multi-agent is an in-process tree: v1 (`features.multi_agent`, on) gives parent-to-child
`spawn_agent`, `send_input` (steers a running turn, `interrupt=true` cancels first), `wait_agent`;
v2 (`multi_agent_v2`, off) adds path addressing (`/root/researcher`), sibling and child-to-parent
`send_message`, and `followup_task` delivered "at message boundaries while sampling, or after the
pending tool call". Two independent TUIs cannot see each other. The sanctioned external surface is
the app-server: `turn/start`, `turn/steer`, `thread/inject_items`, now fronted by a shared local
daemon (`~/.codex/ipc/ipc.sock`) with `codex agents` to browse it and `codex queue --thread
<uuid|name> --message TEXT` to append a message that runs as the next user turn.

**Verified trap.** `codex queue` reaches whichever client owns the thread on the shared daemon. A
standalone `codex` TUI in a zz pane runs its own core and did not pick up a queued message; the
Codex desktop app did, for a thread it had open, and answered. Take the thread id from the TUI's
`/status`, never from the newest rollout file. Codex issue #35542 (open) is the same gap: no
supported way for a local process to wake an idle TUI session.

# Everyone else

| system | inter-agent model | reaches an independent session on the same machine |
|---|---|---|
| Gemini CLI | synchronous subagents, no nesting; remote peers via A2A agent cards; `gemini-cli-a2a-server` hosts its own headless core (protocol 0.3.0) | no route into a running TUI |
| A2A 1.0 | agent cards, tasks, contexts, SSE, webhooks; follow-ups only in `input-required`; a message to a `WORKING` task is unspecified | only if the session is wrapped as a server (a2abridge delivers on the human's next prompt; a2acode spawns its own ACP agent) |
| ACP v1 and v2 | client to agent only; concurrent `session/prompt` unspecified, implementations queue; v2 mentions "symmetric connections" in one field; Session Notices RFD is agent to client | no |
| Zed | `spawn_agent` depth 1 with follow-ups by session id; sibling threads uncontrollable | no |
| Managed Agents | coordinator to roster threads, `send_to_agent`, one level | cloud only |
| herdr 0.9.0 | none; `agent.prompt` types into the pane, `agent.wait`, `events.subscribe` (no replay); server-side queue is discussion #2401, unanswered | typing only |
| gastown 1.2.1 | `gt nudge --mode wait-idle\|queue\|immediate` plus `gt mail` on Dolt beads with groups, queues (claim), channels; mail read by Claude Code hooks (`UserPromptSubmit` runs `gt mail check --inject`) | yes, through hooks and typing |
| MCP Agent Mail | pull-only mailbox (Git markdown plus SQLite), adjective-noun identities, threads, ack, file leases | yes, if the agent polls |
| cmux, agent-deck, NTM, uzi | type text into a pane; agent-deck adds parent notification on child state change | typing only |

# Three cross-provider buses, read closely

| project | what it is | how a message reaches a running agent | verdict for zz |
|---|---|---|---|
| ClementWalter/one-conv-cli (Python, 5 stars, no license) | reads your own chat history across cloud products and local session files; not a messaging tool | never into a running agent: it launches `claude --print --resume <uuid>` or `codex exec` with a seed prompt | transcript locations per agent and the lossy Claude project-dir trap; nothing else |
| Cotal-AI/Cotal (TypeScript, 273 stars, Apache-2.0, 94 releases, one author) | a NATS/JetStream mesh spec with connectors for six CLIs; unicast, channel, and role addressing; A2A cards for presence | Claude Code: plugin with the experimental `claude/channel` MCP push plus `additionalContext` at SessionStart and UserPromptSubmit. Codex: the host owns `codex app-server`, wakes with `turn/start`, folds directed messages into a live turn with `turn/steer`, and attaches the human's TUI with `codex resume --remote ws://…`. Never keystrokes | the Codex app-server hosting pattern, ack on `turn/completed`, attention modes `open`/`dnd`/`focus`, sender stamped by the transport not the payload. Do not borrow the broker, JWTs, or ACLs for one box |
| fujibee/agmsg (bash plus SQLite, 1499 stars, MIT, 41 releases, one author) | team roster plus one SQLite inbox; drivers for nine CLIs; a Tauri desktop app that owns PTYs | Claude Code: a SessionStart directive asks the model to run its Monitor tool on a 5 s poller. Others: Stop hook checks the inbox. Codex: a `codex` wrapper starts app-server and a bridge. Desktop app: writes to stdin immediately, holds Enter 300 ms because Codex reads a same-burst text plus Enter as a paste | they deleted idle-waiting: immediate injection beat gating on PTY quiet, since both CLIs queue typed input while working. Their state classifier lessons (compare derived text, debounce, match the prompt glyph). Never intercept the user's CLI or rewrite project settings |

None of the three uses Claude Code's sessions registry or inbox socket. The only CLI sockets anyone drives are Codex's app-server and Claude Code's experimental channel capability. agmsg's #163 postmortem is the cautionary tale for pollers: a cron job launching a full Codex session every 3 minutes to check an inbox grew logs to 2 GB and forced a jetsam restart.

# Delivery timing, the converged taxonomy

- **Type into the pane.** herdr, cmux, agent-deck, uzi, NTM, gastown `immediate`. Every author who
  documents it also documents the failure: swallowed Enter, double submit on retry, agent-deck's
  80 s timeout on a busy target, gastown's rule "never raw tmux send-keys".
- **Wait for idle, then type.** gastown's default. Needs prompt detection; degrades to queue.
- **Queue for the next turn boundary.** gastown `queue`, Codex `followup_task` and `codex queue`,
  Claude Code's typed-while-working queue, zz's `agent-send --submit`. Gastown's 1.0.1 note is the
  hazard: hook-time injection of all open mail on every prompt blew context to 60 to 70 percent.
- **Steer between tool calls without aborting.** Claude Code cross-session delivery, Codex v1
  `send_input`, pi-intercom. Only the process that owns the model loop can do this.
- **Pull.** MCP Agent Mail, gastown mail without nudge. Durable and auditable, latency accepted.

Two rules every source agrees on: the receiver must be told the message is from another agent, not
its user; and protocol chatter must not become durable mail (gastown moved lifecycle traffic from
mail to ephemeral nudges and cut its commit volume by 80 percent).

# What zz has today

`agent-send` drafts into an Agent pane's composer, `--submit` queues on a busy pane, `--wait` is a
request and reply call with the text on stdout, `--on-block` surfaces permissions. That is the
queue tier for ACP panes, owned by the daemon. `send-text` is the type-into-the-pane tier with echo
verification. `@agent_state` plus `wait-for`, the bell flag, and Codex's OSC 0 title give idle
detection for terminal panes. There is no inbox, no sender attribution on `send-text`, and no way
to reach a Claude Code or Codex TUI other than typing.

# Implications for zz

1. **Join Claude Code's bus instead of inventing one.** The daemon can register one record and
   socket per Agent pane (name from `@name` or the pane id) so every Claude Code session lists them
   as peers and messages them by name, and it can post into any Claude Code session running in a
   pane through that session's socket, with attribution and turn-boundary delivery, no hooks. Both
   halves are verified above and cost a JSON file and a newline-delimited socket. The registry
   record is not a published contract; pin `peerProtocol` 1 and treat a bump as a compat item.
2. **One verb, delivery chosen by target kind.** `agent-send` to an Agent pane queues at the turn
   boundary (exists). To a pane whose `pane_current_command` is a Claude Code session with a
   registry record, post to its socket. To any other TUI, `send-text` immediately: agmsg measured
   that idle-gating degrades into forced timeouts mid-spin, and both CLIs queue typed input while
   working; `send-text` already verifies the echo before Enter, which is the fix for Codex reading
   a same-burst text plus Enter as a paste. Every delivery carries a `from` naming the sending
   pane, which `send-text` cannot add today.
3. **Defer durable mail, threads, and broadcast.** Claude Code has none of the three and is doing
   fine; gastown regretted the volume. A bounded per-pane inbox with replay in the daemon, like the
   existing agent event ring, covers the "target was busy or absent" case.
4. **Codex TUIs stay on the typing tier** until OpenAI closes #35542, unless zz hosts the thread.
   Cotal's pattern makes Codex first-class without keystrokes: the daemon runs `codex app-server`,
   the pane runs `codex resume --remote ws://… <threadId>` so the human keeps the real TUI, and zz
   gets `turn/start` to wake, `turn/steer` to fold a message into a live turn, and `turn/completed`
   to ack. This is a launch-mode change for Codex panes, not a scrape.
5. **Cross-machine later, over ssh.** Claude Code routes cross-machine messages through Anthropic
   servers via Remote Control. zz's daemon-to-daemon ssh transport could carry the same lines
   privately between hosts.

# Open questions

- Whether Claude Code keeps the registry record shape stable across `peerProtocol` bumps.
- How a registered zz peer should report `status` and `tempo` so `notify_when_idle` works
  (advertise `notify_idle` and answer `peer_idle_notice`).
- Whether the hold class matters in practice: zz panes run whatever permission mode the user
  picked, and a bypass-class receiver holds messages from a peer that asserts no class.
- Codex TUI thread discovery from outside (`/status` is a screen, not an API).

# Sources

- https://code.claude.com/docs/en/cross-session-messaging
- https://code.claude.com/docs/en/agent-teams
- https://learn.chatgpt.com/docs/app-server and https://learn.chatgpt.com/docs/config-file/config-reference
- https://github.com/openai/codex/issues/35542 and https://github.com/openai/codex/pull/17749
- https://github.com/herdrdev/herdr/discussions/2401 and https://herdr.dev/docs/socket-api/
- https://github.com/gastownhall/gastown (nudge.go, docs/HOOKS.md, CHANGELOG.md)
- https://github.com/Dicklesworthstone/mcp_agent_mail
- https://a2a-protocol.org/latest/specification/
- https://agentclientprotocol.com/rfds/v2/overview
- https://github.com/nicobailon/pi-intercom
- https://github.com/vbcherepanov/a2abridge and https://github.com/kanywst/a2acode
