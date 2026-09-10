---
type: Architecture
title: Agent peer bus
description: How the daemon exchanges prompts between Agent panes and local Claude Code sessions through Claude Code's peer registry and Unix inbox sockets.
resource: crates/zz-daemon/src/agent/claude_peers.rs
tags: [architecture, agent, messaging, claude-code, acp]
timestamp: 2026-09-10T00:00:00Z
---

The daemon joins Claude Code's local peer protocol in two directions. You can target a terminal
pane running Claude Code with `agent-send`, and Claude Code can discover and message an
[Agent pane](/concepts/agent-pane.md) through `ListAgents` and `SendMessage`.

The [agent messaging survey](/research/2026-09-10-agent-messaging-survey.md) records live probes
with Claude Code 2.1.267 on 2026-09-10. The implementation uses `peerProtocol: 1`; Claude Code has
not published the registry shape as a stable contract.

## Target selection

`agent-send -t %N TEXT` first resolves the target pane. For a terminal pane, the daemon looks for
a Claude Code registry record whose `tmux` field ends in the exact pane component `%N`, whose pid
is alive, and whose `updatedAt` falls within 24 hours. If it finds one, it posts the existing
command payload to that record's socket. Stdin, the `--context` header, and the 1 MiB payload
limit retain their existing command behavior.

Both plain `agent-send` and `--submit` send a message on this route; Claude Code has no remote
composer draft. `--wait` fails with exit 1 because this route does not collect a response.
Without a matching terminal peer, the command follows its existing Agent-pane target resolution.
Claude Code processes received messages between tool calls or starts a turn when idle, subject to
its own inbound policy.

## Registry record

The daemon reads `$CLAUDE_CONFIG_DIR/sessions` when set, otherwise `$HOME/.claude/sessions`.
It tolerates unknown JSON fields. Claude Code expects the filename `<pid>.json` to identify a
live process and `procStart` to match trimmed `TZ=UTC ps -p <pid> -o lstart=` output.

For an Agent pane, the daemon uses the ACP adapter child's pid. It discovers that direct child
of the daemon through its `ZZ_PANE=%N` environment entry, using `ps` on macOS or `/proc` on Linux.
The ACP library continues to own the child and its spawn path.

| Field | Value for an Agent pane |
|---|---|
| `pid`, `procStart` | Adapter pid and UTC process start text |
| `sessionId` | Fresh UUID for this registration |
| `name` | Pane `@name` option, or `zz-%N` |
| `cwd` | Pane working directory when known, otherwise the daemon's |
| `tmux` | `<session name>:@<window id>.%<pane id>` |
| `kind`, `entrypoint` | `interactive`, `cli` |
| `peerProtocol`, `peerFeatures` | `1`, `[]` |
| `version`, `pidDomain` | Daemon package version; `darwin` or `linux` |
| `messagingSocketPath` | This peer's Unix inbox socket |
| `status` | `busy` for Running or AwaitingPermission, otherwise `idle` |
| `startedAt`, `updatedAt`, `statusUpdatedAt` | Unix timestamps in milliseconds |
| `zz` | Ownership marker such as `{"pane":"%3"}` |

The daemon writes records through a temporary file and rename. It takes the socket directory from
an existing record's `messagingSocketPath` when available, otherwise `/tmp/cc-socks`, creating the
directory with mode 0700. Each peer listens on `<directory>/<pid>.sock` with mode 0600.

## Wire line

Peers exchange newline-delimited JSON over Unix stream sockets. An outbound message from a
registered Agent pane has this shape:

```json
{"msgV":1,"msg_id":"ad3b395d-02dd-46fa-a37c-f35170304716","type":"user","priority":"next","from":"uds:/tmp/cc-socks/4712.sock","message":{"role":"user","content":"<cross-session-message from=\"uds:/tmp/cc-socks/4712.sock\" from-name=\"zz-%3\" from-mode=\"prompting\">\nTEXT\n</cross-session-message>"}}
```

The sender generates a UUID v4 for `msg_id` and uses `from-mode="prompting"`. It replaces a
payload's `</cross-session-message>` with `<\/cross-session-message>` before wrapping it, so
payload text cannot end the attribution wrapper early.

For a registered sending Agent pane, `from-name` uses its peer name and `from` points back to its
socket. Otherwise the sender omits both the JSON `from` field and the wrapper's `from` attribute;
`from-name` is `zz %N` when the caller pane is known, or `zz`. Claude Code validates reply
addresses against its registry. The daemon sends no auth line and reads no reply from the outbound
connection.

One listener thread serves each registered Agent pane. It tolerates empty liveness connections,
ignores `type: auth` lines and unrecognized messages, and accepts `type: user` lines with a string
`message.content`. The listener applies a 30-second idle deadline to connections and reads sockets
without holding the daemon lock. It logs the sender and byte count at info level, then passes the
content verbatim, including its wrapper, to `submit_agent_prompt`. That existing path queues the
prompt while the Agent pane is busy.

## Lifecycle and limits

In `crates/zz-daemon/src/daemon.rs`, `publish_agent_state` registers a pane when its runtime first
reports Ready, then updates the record's status, timestamps, and name on phase changes, so a later
`@name` write shows up after the pane's next turn starts or ends. Restarting an
adapter removes the old record and socket before registering the replacement pid. The daemon
stops the listener and removes the record and socket when `close_agent_panes` closes a pane or
the daemon shuts down. At startup it removes stale records carrying the `zz` marker whose pid is
dead, together with their sockets; it leaves unmarked records alone.

The Claude provider's adapter runs Claude Code through the Agent SDK, and that inner Claude Code
registers its own peer record too (`entrypoint: "sdk-ts"`, an auto-generated name such as
`zz-5c`, no `tmux` field). A message sent to that name is answered inside the SDK session and
never appears in the pane, and when the pane's agent replies to a peer it does so from that inner
address. Claude Code exposes no switch for it, so a Claude-provider pane shows up twice in
`ListAgents`; the record with the `tmux` field and the `zz-%N` or `@name` name is the one whose
messages reach the pane. Verified on 2026-09-10 with Claude Code 2.1.267 and
`claude-agent-acp@0.76.0`.

On macOS, `setsockopt` fails with `EINVAL` on a Unix socket whose peer has already closed, even
though the buffered data is still readable, so the inbox treats a failed read-timeout call as
advisory instead of dropping the connection.

The module requires Unix and the daemon's `agent` feature, which belongs to the default feature
set. Windows retains the previous outbound routing. Terminal panes use Claude Code's own
registration; the daemon registers only Agent panes. This round has no socket `--wait`, idle
notifications, replies for senders without a registered socket, durable mailbox, or cross-machine
transport. It does not change `send-text`, install hooks, or host Codex app-server sessions.
