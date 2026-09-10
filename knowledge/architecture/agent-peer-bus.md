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
[Agent pane](/concepts/agent-pane.md) or a named terminal pane through `ListAgents` and
`SendMessage`.

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
composer draft. `--wait` collects a reply through a temporary daemon peer, as described below.
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
| `zz` | Ownership marker `{"pane":"%3","kind":"agent"}` |

Terminal and daemon peers share the record fields above with these differences:

| Field | Terminal peer | Transient daemon peer |
|---|---|---|
| `pid`, `procStart` | Pane shell pid and its UTC start text | Daemon pid and its UTC start text |
| `name` | Pane `@name` value | `zz` |
| `tmux` | `<session name>:@<window id>.%<pane id>` | Omitted |
| `peerFeatures`, `status` | `[]`, `idle` | `[]`, `idle` |
| `zz` | `{"pane":"%3","kind":"terminal"}` | `{"daemon":true}` |

The daemon writes records through a temporary file and rename. It takes the socket directory from
an existing record's `messagingSocketPath` when available, otherwise `/tmp/cc-socks`, creating the
directory with mode 0700. Each peer listens on `<directory>/<pid>.sock` with mode 0600.

## Terminal peers

Set a pane's `@name` option to make a terminal agent available to Claude Code:

```sh
zz set-option -p -t %3 @name codex-1
```

A pane-scoped `@name@%N` option signal triggers registration. The daemon uses the shell pid from
`pane_runtime_facts`, without searching descendants. Claude Code sessions with their own matching
record keep that registration. Agent panes retain their existing registration lifecycle.

The inbox passes each user message verbatim, including its attribution wrapper, to
`Shared::paste_and_submit`. That method uses the same paste, echo polling, and Enter path as
`send-text`, with a 2000 ms deadline and no daemon lock held during the poll. Failures produce a
warning. Terminal peers ignore control lines.

Messages submit input to the terminal, so do not name a bare shell pane. Changing `@name` updates
`name` and `nameSince` in place. Unsetting the option or closing the pane removes its peer record
and socket.

This typed path is the whole story for Codex, Gemini, and any other terminal agent, by decision on
2026-09-10. A version of zz hosted Codex's app-server per pane behind a `split-codex` verb, which
gave steering, typed state, and approvals from the CLI; it was withdrawn the same day because it
embedded one vendor's loop and its own verb into zz. The archive tag `archive/codex-host` keeps
that work. What remains vendor-neutral: name the pane, deliver by verified paste, read state from
the bell and the title, and let the human answer approvals in the pane.

## Waiting for a reply

For a Claude Code terminal target, `agent-send --wait` requires a command or control client. The
first pending wait registers a peer for the daemon's own pid, named `zz`, with no `tmux` field.
Concurrent waits share that inbox; the last caller to finish removes its record and socket.

The daemon registers the message id and target socket before posting. The payload retains the
sender pane's peer name, or the `zz %N` / `zz` fallback, but its `from` address points to the daemon
inbox. Claude Code can therefore validate the reply address through the registry. A plain send
from a caller without a peer stays fire-and-forget by decision: a caller that needs the answer uses
`--wait`, and a pane that should be reachable sets `@name`; there is no daemon inbox for strays.

Replies have no reply-to id. A `type: user` line resolves the oldest unresolved wait whose target
socket equals the line's `from` address (`uds:<target socket>`). Other targets keep waiting. The
daemon logs and drops replies without a matching caller. Stdout receives the trimmed text between
the wrapper's first line and closing tag; an unwrapped reply retains its raw content. On success,
the caller receives the pane id on stderr.

Delivery updates match `action: peer_message_status`, regardless of `type`. The daemon correlates
`orig_msg_id` with the posted message id, and also handles ids listed in `dropped_msg_ids`.
`held` and `delivered` keep waiting. `expired`, `dropped`, or `status_detail: refused` fail with exit 1
and report the status and supplied `reason` or `drop_reason`. A dead target pid also fails with
exit 1. The timeout defaults to 600 seconds, and `0` waits forever. Timeout errors name the pane
and use the ACP wait's wording; a timeout does not cancel the remote turn.

```json
{"type":"control","action":"peer_message_status","orig_msg_id":"sent-message-id","status":"dropped","reason":"peer policy","from":"uds:/tmp/cc-socks/4712.sock"}
```

## Control lines and pid verification

Claude Code sends two kinds of lines to a peer socket: `type: user` messages and `type: control`
lines such as `notify_when_idle` subscriptions and `peer_message_status` reports. Verified on
2026-09-10: it delivers messages to any socket named in a record, but it sends control lines only
when the process that accepted the connection is the record's `pid`. A record whose socket is
served by another process, which is every zz pane peer because the daemon holds the socket while
the record names the adapter or shell, never receives a subscription, and the sender reports "the
subscription could not be sent". The transient daemon peer is the exception, since its record
names the daemon's own pid, which is why delivery status reports reach pending waits.

zz therefore advertises no `peerFeatures` on pane peers and offers no idle notices. A Claude Code
session that wants to wait for a zz pane runs `zz wait-for agent_state@%N` through the shim in its
pane. Offering notices would need a helper process per pane that owns both the record pid and the
socket.

The subscription line, for reference, is:

```json
{"type":"control","action":"notify_when_idle","from":"uds:/tmp/cc-socks/4712.sock","from_mode":"bypass","msgV":1,"msg_id":"f05b57fb-0000-4000-8000-000000000000"}
```

and the notice a peer that could answer would post to that `from` address is:

```json
{"type":"control","action":"peer_idle_notice","msgV":1,"orig_msg_id":"f05b57fb-0000-4000-8000-000000000000","state":"idle","detail":"one short line","from":"uds:/tmp/cc-socks/<own pid>.sock"}
```

## Wire line

Peers exchange newline-delimited JSON over Unix stream sockets. An outbound message from a
registered Agent pane has this shape:

```json
{"msgV":1,"msg_id":"ad3b395d-02dd-46fa-a37c-f35170304716","type":"user","priority":"next","from":"uds:/tmp/cc-socks/4712.sock","message":{"role":"user","content":"<cross-session-message from=\"uds:/tmp/cc-socks/4712.sock\" from-name=\"zz-%3\" from-mode=\"prompting\">\nTEXT\n</cross-session-message>"}}
```

The sender generates a UUID v4 for `msg_id` and uses `from-mode="prompting"`. It replaces a
payload's `</cross-session-message>` with `<\/cross-session-message>` before wrapping it, so
payload text cannot end the attribution wrapper early.

For a registered sending Agent or terminal pane, `from-name` uses its peer name and `from` points back to its
socket. Otherwise the sender omits both the JSON `from` field and the wrapper's `from` attribute;
`from-name` is `zz %N` when the caller pane is known, or `zz`. Claude Code validates reply
addresses against its registry. The daemon sends no auth line and reads no reply from the outbound
connection.

One listener thread serves each registered peer. It tolerates empty liveness connections,
ignores `type: auth` lines and unrecognized messages, and accepts `type: user` lines with a string
`message.content`. The listener applies a 30-second idle deadline to connections and reads sockets
without holding the daemon lock. It logs the sender and byte count at info level, then passes the
content verbatim, including its wrapper, to `submit_agent_prompt` for Agent peers. That existing
path queues the prompt while the Agent pane is busy. Terminal peers use the paste path; the
daemon peer routes replies to pending waits.

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
set. Windows retains the previous outbound routing. Plain sends from callers without a registered
peer still omit a reply address; socket waits provide the transient daemon address. This protocol
has no durable mailbox or cross-machine transport. It does not change the `send-text` command
surface, install hooks, or host Codex app-server sessions.
