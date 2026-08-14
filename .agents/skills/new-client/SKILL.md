---
name: new-client
description: Build or extend a zz client on the shared client core — a GTK, Qt, Swift, web, TUI, or any new presentation surface, in Rust or over the C ABI. Use this whenever the task involves creating a client for zz, consuming zz-client or zz-client-ffi, driving ClientCore from a new shell, adding chrome actions or binding tables, porting an existing surface onto the core, or asking how clients talk to the daemon. Reach for it even when the user only says "make a client", "attach from X", or names a UI toolkit — the contract and its pitfalls below are not guessable from the code alone.
---

# Building a zz client

Every zz client is a thin skin over the same stack. The daemon owns the product
(sessions, PTYs, key tables, copy-mode, search, command execution, status); the
client renders state and forwards intent. A new client's job is: render
viewports, forward keys, switch on actions. If you find yourself writing
key-chord logic, layout semantics, or protocol reduction, stop — it almost
certainly already exists one layer down, and duplicating it is how the pre-core
clients accumulated three copies of everything.

Read `knowledge/designs/client-core-and-contract.md` first — it is the decision
record behind this entire architecture and states what is deliberately client
territory. The knowledge bundle is the map; source is ground truth.

## The stack (bottom to top)

| Layer | Crate | What it gives a client |
|---|---|---|
| Contract | `zz-protocol` | Wire types, `PROTOCOL_VERSION`, key grammar (`canonical_key`, `input_key_name`), the shared resolver (`KeyTables::resolve_input`), command catalog (`COMMAND_SPECS`) |
| Transport | `zz-daemon` with `default-features = false` | `InteractiveClient` (connect/attach/send/recv), `Endpoint` incl. `ssh://` forwarding, fleet hosts — the pure client SDK, no daemon code |
| Terminal model | `zz-terminal` with `default-features = false` | `TerminalViewport`, `PackedCell`, `KeyInput`, `TerminalViewAction` — no libghostty |
| Brain | `zz-client` | `ClientCore` (sans-IO protocol reducer) and `ChromeKeymap` (client-local chrome bindings) |
| C ABI | `zz-client-ffi` | `include/zz-client.h` — handle, wake fd, typed events, acquire/release viewports |

## Pick your integration route

1. **C or any non-Rust toolkit (GTK, Qt, Swift, …)** — link `zz-client-ffi`
   (staticlib/cdylib) against `include/zz-client.h`. The complete working
   reference client is `crates/zz-client-ffi/tests/smoke.c` (~100 lines:
   connect, attach, list panes, resize, read rows, type, verify). Main-loop
   integration is fd-based by design: poll `zz_client_event_fd`, then drain
   `zz_client_next_event` until false — plugs into GSource, QSocketNotifier,
   or DispatchSource with no cross-thread callbacks.
2. **Rust surface** — depend on `zz-client` + `zz-daemon` (client half) +
   `zz-protocol` and drive `ClientCore` yourself. `crates/zz-tui` is the
   exemplar: reader thread reduces into `Arc<Mutex<ClientCore>>`, the main
   loop reads cached copies. Its dependency list is also the fence — if your
   client needs a dep the TUI doesn't have, question it.
3. **gpui-based client on a new platform** — don't write a client at all;
   recompile `crates/zz` through the `zz::engine` facade with an `AppProfile`,
   the way `crates/zz-ios` does.

## The shell contract for ClientCore

The core is sans-IO: you own the socket and threads, it owns state. The loop,
per received message, under one lock acquisition:

1. `core.handle_message(message)` — decoded `ProtocolMessage` in.
2. Drain `core.poll_outbound()` — send each `Outbound::RequestFull(pane)`
   back through your `InteractiveClient`. Wire this even if you think it can't
   fire; a silent drop here strands a corrupted viewport forever.
3. Drain `core.poll_event()` — typed `CoreEvent`s. State-change events are
   notifications (read the new value via accessors); side-effect events
   (Clipboard, OpenUri, AgentCommand, `Message(...)` pass-throughs) carry
   their payload because the core stores none of it. Handle the events after
   dropping the lock if handling can block.

Seed the core by hand: `InteractiveClient` consumes the handshake, so feed
`ProtocolMessage::ServerHello(client.server_hello().clone())` into the core
yourself — it never arrives via `recv()`.

Reconnects: a fresh core per connection is the simple correct shape (TUI). If
your UI keeps the last frame frozen on screen across a reconnect, use the split
primitives — `adopt_hello` alone keeps the frame; `handle_message(ServerHello)`
is `adopt_hello` + `clear_attachment` + `reset_session` and blanks it.

Frame path: for most clients the core's retention is exactly right — render on
`ViewportChanged { pane, damage }` from `core.viewport(pane)`, damage tells you
which rows. Only intercept `TerminalViewport`/`TerminalPatch` *before* the core
(the desktop does) when your painter needs retained state richer than
`TerminalViewport`; never add per-frame clones or extra locking to that path.

## Keys: never hardcode a chord

- **Pane and overlay semantics belong to the daemon.** Forward raw presses
  (`InputMessage::Key`/`Text`, `ChooseTreeAction::Key`, …); the daemon's key
  tables resolve them, so your client inherits the prefix, copy-mode vim,
  choosers, and every user rebind with zero key logic. The live tables arrive
  in `ServerHello.key_tables` and `KeyTablesChanged` — use them for hints,
  help overlays, and shortcut labels instead of hardcoded guesses.
- **Client-local chrome belongs to `ChromeKeymap`** (detach, sidebar focus,
  zoom, tabs…). Resolve presses against its tables and switch on the returned
  `ChromeAction`; add new actions/table entries in
  `crates/zz-client/src/chrome.rs`, never inline chord tests. Chrome extends
  the wire grammar with `D-` (Cmd/Super) and `S-` (Shift) because a pane can
  never receive those; the wire grammar itself must not grow them. User
  overrides ride `chrome-keybind` / `chrome-unbind` in `zz/config`.

## Testing a new client

- Boot a real daemon in-process:
  `Daemon::new(&socket).without_user_config()` on a short `/tmp` socket
  (`sun_path` caps length), create a session with a deterministic fixture like
  `"printf 'ready\r\n'; exec /bin/cat"`, connect, drive, assert. Both
  `crates/zz-client/tests/simulator.rs` (convergence oracles) and
  `crates/zz-client-ffi/tests/smoke.rs` (C client end-to-end) are copyable
  harnesses.
- The strongest oracle is patch/full duality: after quiescence, a freshly
  requested full frame must equal the patch-accumulated one.
- Gates before calling anything done: `cargo fmt --all`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`. A zz-daemon test failing under the
  full parallel run is usually load flake — re-run it solo first.

## Before you write code

Read `references/pitfalls.md` in this skill. Every entry there was a real bug
or dead-end hit while building the first three consumers of this stack
(desktop, TUI, C smoke client); each one costs an afternoon to rediscover.
