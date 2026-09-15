---
type: Design Plan
title: Command stream channel
description: "One bounded channel for the caller's standard input and output on a command client: a single reader with a single cap, one byte-preserving carrier on the invocation, and three named sinks, so `source-file -`, `display-message -I`, `split-window -I`, `load-buffer -` and `save-buffer -` share a transport instead of owning five."
status: "Agreed and built 2026-09-14 for TUI-018, milestone 5 of the tmux superset roadmap; carried on protocol 103"
resource: crates/zz-protocol/src/message.rs
tags:
- tmux
- compatibility
- protocol
- cli
timestamp: 2026-09-14T18:00:00-03:00
---

# Why

Five tmux commands move bytes between the caller's process and the server rather than between the
caller's arguments and the server. The roadmap's milestone 5 says to design one bounded channel for
them and not five bespoke transports. Before this document zz had three of the five working through
a caller-side trick - the payload was appended to the command's argument vector - and the other two
refused, because their payload is not an argument and never could be: `source-file -` has a path
where the bytes would go, and `display-message -I` has no argument slot at all.

# The channel

One reader, one cap, one carrier, three sinks.

**The reader** is `read_stdin_payload` in `crates/zz/src/lib.rs`. It runs at most once per
invocation, before the command is sent, and only when the resolver below says the command has a
sink. Nothing else in the CLI reads standard input for a command.

**The cap** is `MAX_AGENT_SEND_BYTES` (1 MiB), the same bound the agent and buffer payloads already
carry. The reader takes `cap + 1` bytes and refuses the whole invocation if the last one arrives, so
an over-long stream fails before the server sees a byte and nothing is half-applied.

**The carrier** is `CommandInvocation::stdin`, an `Option<RawText>` appended to the invocation in
protocol 103. `RawText` keeps the exact bytes beside a lossy `String`, so a payload that is not
valid UTF-8 crosses the wire unchanged. The payload rides with the invocation rather than beside it,
so every consumer that already receives the invocation - the mux, the daemon, a prepared alias
group - receives the stream with it and nothing has to be threaded. `format_command` does not print
it: a stream is not an argument, and the server log records the command the caller typed.

An expanded command alias keeps this carrier on its group invocation. Each member shares one
caller stream: the first reader consumes the bytes and a later reader receives the in-process
`CommandInvocation.stdin_spent` marker. Serde skips that marker; the wire remains at protocol 103
with no new field. The mux group executor accounts for emitted stream effects, and the daemon's
prepared group queue routes the carrier after parsing the members. A nonreader leaves it available.

**The sinks** are what the payload is for. `command_stdin_sink` in `crates/zz-daemon/src/daemon.rs`
is the one resolver; it takes a canonical command name and its arguments and answers at most one
sink, and both the CLI and the daemon ask it rather than matching on names of their own.

| sink | commands | what the payload becomes | bytes |
|---|---|---|---|
| `Argument` | `load-buffer -`, `send-text -`, `agent-send -` | the command's own text argument, appended after the argument boundary | `load-buffer` binary, the other two UTF-8 |
| `Config` | `source-file -` | a configuration file named `-`, parsed and applied in place, its diagnostics spelled against `-` | UTF-8 |
| `PaneInput` | `display-message -I`, `split-window -I` | bytes written into a PTY-free pane's parser, as if a child had printed them | binary |

# The six things

**Standard input** is the reader above. A command with no sink never reads it, so a pipe into
`zz list-sessions` is still the caller's own business.

**Standard output** is the half that already existed and is unchanged: the daemon answers with
`CommandResponse::Success { output: RawText, stdout_claim }`, and `StdoutClaim` says which of the
pin's two writers - `cmdq_print` or a raw `file_write` on `-` - owned the stream, so the client
knows whether to add the terminating newline. `save-buffer -` is a raw claim.

**Binary bytes** survive in both directions because `RawText` is the carrier in both directions.
The `Config` sink is the one that refuses them, because a configuration file is text; the reader
rejects a non-UTF-8 payload for that sink with the same message it uses for `send-text`.

**Backpressure** is the cap, enforced at the reader, before the connection carries anything. zz
refuses a stream larger than 1 MiB where pinned tmux streams it in 16 KiB acknowledged chunks with
no total bound. That is a deliberate difference: a caller stream is an argument-shaped payload - a
configuration, a message, a pane's seed text, a paste buffer - and bulk file transfer through a
command client is a workload zz does not serve, because an unbounded stream lets one caller grow
daemon memory without limit. Decided 2026-09-14 by the orchestrator under fabrico's TUI parity
contract of 2026-09-09; reversible.

**Cancellation** is the end of the stream. A caller that closes standard input early ends the read;
what arrived is the payload, and the command runs on it. A caller that dies before the invocation is
sent sends nothing and the server never sees a request, which is why the reader runs before the
request and not beside it. There is no half-applied state to unwind: the `Config` sink parses the
whole payload before applying a command from it, and the `PaneInput` sink writes bytes the pane
would have printed anyway.

**Process lifetime** is the daemon's, never the caller's. The `PaneInput` sink writes into a pane
that has no child process at all: pinned tmux's `-I` forms require `PANE_EMPTY` and answer
`pane is not empty` otherwise, and zz answers the same from the same rule, because
`TerminalSession::feed` is defined to be ignored by a session with a live child. A pane built by
`split-window -I` outlives the caller, holds no process, and is killed like any other pane. The
daemon never adopts the caller's file descriptors, and the caller's exit does not end anything the
server started.

# What the existing three become

Direct `load-buffer -`, `send-text -` and `agent-send -` calls still append their payload to the
command arguments. The CLI resolves an alias group's first stream sink in member order and keeps
its payload on the group's stdin carrier. After preparing the members, the daemon appends raw
Argument bytes to the first reader's arguments with the same `append_stdin_payload` helper the
CLI uses. It does not format those bytes into the command body: arbitrary bytes and the member's
argument boundary must survive preparation.

A later `load-buffer -` reports `Bad file descriptor: -` and raises the caller's exit status to 1,
while following members still run, matching the pin's asynchronous read completion. A later
`source-file -` receives `SourceStream::Spent` through the mux's existing source-file effect.

# What this does not do

- No chunked or acknowledged transport. The payload is bounded, so it is one message.
- No streaming *out*: a command's stdout is one response, as it already was.
- No `-I` on any command pinned tmux does not give it to, and no zz-only stream forms.
- `source-file -` inside a configuration file stays refused: a config being loaded has no caller and
  therefore no stream, which is the same reason the pin reads nothing there.
