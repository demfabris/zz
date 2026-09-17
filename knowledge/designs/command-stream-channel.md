---
type: Design Plan
title: Command stream channel
description: "One bounded channel for the caller's standard input and output on a command client: a single reader with a single cap, one byte-preserving carrier on the invocation, and three named sinks, so `source-file -`, `display-message -I`, `split-window -I`, `load-buffer -` and `save-buffer -` share a transport instead of owning five."
status: "Built for TUI-018; caller matrix corrections on protocol 104; closed-fd cells remain recorded; awaiting independent campaign review"
resource: crates/zz-protocol/src/message.rs
tags:
- tmux
- compatibility
- protocol
- cli
timestamp: 2026-09-17T00:00:00-03:00
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

**The reader** is `read_command_stdin` in `crates/zz-daemon/src/client.rs`. The CLI opts in
through `CommandClient::enable_stdin`; each command request carries `stdin_available`.
The daemon asks for `ClientFileOperation::ReadStdin` when a sink executes, including sinks
inside aliases and sourced files. Earlier members finish before the read starts. A sourced file
without a reader ignores open or oversized stdin. During a pending read, the CLI handles SIGTERM
by exiting 0; disconnect wakes the daemon's file waiter and cancels the remaining command queue.

**The cap** is `MAX_AGENT_SEND_BYTES` (1 MiB). The reader takes at most `cap + 1` bytes and
refuses that reader if the last byte arrives. Earlier state changes remain applied. Unused bytes
never reach the cap check. The runtime diagnostic uses `ServerError::InvalidCommand`, with exit 1;
it does not introduce a zz-native usage error.

**The carrier** is `CommandInvocation::stdin`, an `Option<RawText>` appended to the invocation in
protocol 103. `RawText` keeps the exact bytes beside a lossy `String`, so a payload that is not
valid UTF-8 crosses the wire unchanged. The payload rides with the invocation rather than beside it,
so every consumer that already receives the invocation - the mux, the daemon, a prepared alias
group - receives the stream with it and nothing has to be threaded. `format_command` does not print
it: a stream is not an argument, and the server log records the command the caller typed.

An expanded command alias keeps this carrier on its group invocation. Each member shares one
caller stream: the first reader consumes the bytes and a later reader receives the in-process
`CommandInvocation.stdin_spent` marker. Serde skips that marker. Protocol 104 adds the availability flag and deferred read operation. The mux group executor accounts for emitted stream effects, and the daemon's
prepared group queue routes the carrier after parsing the members. A nonreader leaves it available.
The daemon keeps the stream in the invoking client's `CommandStreams` request record, which it
removes when producing the response. Replayed files and alias members use that same record through
`ExecutionContext::replay_client`; they cannot restart or duplicate the reader.

**The sinks** are what the payload is for. `command_stdin_sink` in `crates/zz-daemon/src/daemon.rs`
is the one resolver; it takes a canonical command name and its arguments and answers at most one
sink. The daemon asks it when each command executes. `ConfigReplay` leaves the caller stream
available for readers in the sourced file. Each actual reader chooses whether to accept binary bytes.

| sink | commands | what the payload becomes | bytes |
|---|---|---|---|
| `Argument` | `load-buffer -`, `send-text -`, `agent-send -` | the command's own text argument, appended after the argument boundary | `load-buffer` binary, the other two UTF-8 |
| `Config` | `source-file -` | a configuration file named `-`, parsed and applied in place, its diagnostics spelled against `-` | UTF-8 |
| `PaneInput` | `display-message -I`, `split-window -I` | bytes written into a PTY-free pane's parser, as if a child had printed them | binary |

# The six things

**Standard input** is the reader above. A command with no sink never reads it, so a pipe into
`zz list-sessions` is still the caller's own business.

**Standard output** uses the existing response: the daemon answers with
`CommandResponse::Success { output: RawText, stdout_claim }`, and `StdoutClaim` says which of the
pin's two writers - `cmdq_print` or a raw `file_write` on `-` - owned the stream, so the client
knows whether to add the terminating newline. `save-buffer -` is a raw claim.

**Binary bytes** survive in both directions because `RawText` is the carrier in both directions.
The `Config` sink is the one that refuses them, because a configuration file is text; the reader
rejects a non-UTF-8 payload for that sink with the same message it uses for `send-text`.

**Backpressure** is the cap, enforced at the reader before the connection carries payload bytes. zz
refuses a stream larger than 1 MiB where pinned tmux streams it in 16 KiB acknowledged chunks with
no total bound. That is a deliberate difference: a caller stream is an argument-shaped payload - a
configuration, a message, a pane's seed text, a paste buffer - and bulk file transfer through a
command client is a workload zz does not serve, because an unbounded stream lets one caller grow
daemon memory without limit. Decided 2026-09-14 by the orchestrator under fabrico's TUI parity
contract of 2026-09-09; reversible.

**Cancellation** follows EOF or client disconnect. EOF completes the pending payload; the
reader then runs on those bytes. SIGTERM before, during or after the read exits the waiting command
client with status 0. Each command wait and each nested read saves and restores the previous
SIGTERM disposition with `sigaction`, including read errors and cap refusals. Disconnect releases
the daemon's file waiter and prevents pending payloads and following group members from running.
State from completed members remains. Source diagnostics reach CLI stderr as they occur through
`ClientMessage` error events; the client removes delivered text from the final accumulated response.

Destination validation precedes stdin acquisition. A missing `display-message -I` target returns
without consuming the stream, a running pane rejects it, and `split-window -I` resolves its target
and validates spawn options before requesting bytes. Control source read failures use the same
unframed `ControlSourceFile::ReadError` event for direct commands, aliases and file replay, preserving
both continuation and the attached control client's exit status.

The matrix records a remaining closed-descriptor difference under TUI-018. Rust replaces a closed
fd 0 with `/dev/null` before application entry; the pin instead reports a libevent EBADF error when
its source reader runs. This is distinct from EOF on an open descriptor and remains unasserted.

**Process lifetime** is the daemon's, never the caller's. The `PaneInput` sink writes into a pane
that has no child process at all: pinned tmux's `-I` forms require `PANE_EMPTY` and answer
`pane is not empty` otherwise, and zz answers the same from the same rule, because
`TerminalSession::feed` is defined to be ignored by a session with a live child. A pane built by
`split-window -I` outlives the caller, holds no process, and is killed like any other pane. The
daemon never adopts the caller's file descriptors, and the caller's exit does not end anything the
server started.

# What the existing three become

The daemon acquires `load-buffer -`, `send-text -` and `agent-send -` payloads through the same
request path as source and pane input. It uses `append_stdin_payload` to append Argument bytes
after the member's argument boundary. It does not format those bytes into an alias body.
Programmatic invocations can still carry an explicit payload in `CommandInvocation::stdin`.

A later `load-buffer -` reports `Bad file descriptor: -` and raises the caller's exit status to 1,
while following members still run, matching the pin's asynchronous read completion. A later
`source-file -` receives `SourceStream::Spent` through the mux's existing source-file effect. The
daemon reports that read failure and resumes the queue, preserving later stdout and state changes.
A missing source path still aborts the alias group.

An alias carries its members' stdout ownership to the response. A raw buffer writer keeps its
unterminated bytes and bypasses text sanitization; subsequent print output cannot reclaim that writer.
File replay keeps `RawText` through its transcript instead of converting it to a lossy string. A
second raw writer reports EBADF, matching the existing file-replay rule. Each child records its
latest writer with a sequence number, so a later print-only alias does not inherit an earlier
alias's raw classification.

# What this does not do

- No chunked or acknowledged transport. The payload is bounded, so it is one message.
- No streaming *out*: a command's stdout is one response, as it already was.
- No `-I` on any command pinned tmux does not give it to, and no zz-only stream forms.
- A daemon-start configuration has no caller stream and refuses `source-file -`. A command client
  sourcing a file retains its bounded stream throughout replay, including aliases and nested files.
  The first reader consumes it; later readers receive the spent marker. Attached and Control
  clients do not acquire a command-client stdin stream.
