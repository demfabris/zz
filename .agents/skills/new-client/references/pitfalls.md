# Pitfalls — each one was hit for real

Failure modes discovered while building the desktop port, the TUI port, and
the C smoke client on this stack. They are ordered by how expensive they are
to rediscover.

## 1. Never gap-check `Event.sequence`

The daemon's outbound mailbox supersedes stale terminal frames under
backpressure — a newer full frame replaces a queued one, and the replaced
frame's sequence number is simply consumed. A healthy stream therefore
**legitimately skips sequences**. A client that treats a gap as loss and
requests a resync enters a permanent loop: the resync bundle itself arrives
with fresh gaps, triggering the next resync, forever (the convergence
simulator caught this on its first run — the stream never went quiet).

`Resync` is an error-path request only. The one legitimate trigger in the
existing clients: `CommandResponse::Error` with `ServerError::MissingTarget`
while unattached (see `retry_default_after_missing_session` in
`crates/zz/src/mux/client.rs`).

## 2. Scope pane lists to the attached session

The snapshot can contain sessions your client is not attached to. Iterating
every session's panes includes panes that receive no terminal frames because
the daemon limits frame delivery to the client's visible set.
The symptom is maddening: the pane "exists" in the snapshot, resize appears to
succeed, and no content ever arrives. Filter by
`core.attached_session() == session.id` (the C ABI's
`zz_client_terminal_panes` does this for you).

## 3. `StatusChanged` ticks forever

The status line carries a clock, so the daemon publishes `StatusChanged` about
once a second for as long as the connection lives. Any quiescence detection,
idle timer, or "wait until nothing arrives" test logic must exempt it, or it
never fires.

## 4. Compare viewports by resolved content, never raw ids

`PackedCell.style_id` indexes the frame's interned dictionary, and patch
streams *append* to that dictionary while a fresh full frame *rebuilds and
compacts* it — so the same visible screen has different style ids and
dictionary layouts depending on how it was reached. Equality checks (tests,
convergence oracles, cross-client diffing) must resolve each cell to its glyph
text + `PackedStyle` value first. `content_signature` in
`crates/zz-client/tests/simulator.rs` is the reference implementation.

## 5. The handshake hello never reaches `recv()`

`InteractiveClient::connect` consumes the `ServerHello` during the handshake.
Seed the core manually with
`core.handle_message(ProtocolMessage::ServerHello(client.server_hello().clone()))`
and drain the resulting `HelloReceived` before your reader starts, or your
core runs with default (empty) appearance, options, and key tables.

## 6. Reconnect resets more than you may want

`handle_message(ServerHello)` performs the full reset:
`adopt_hello` (settings) + `clear_attachment` (session, snapshot, viewports) +
`reset_session` (overlays, prefix arming). A client that keeps the last frame
frozen on screen while reconnecting must call `adopt_hello` only — the full
reset blanks the workspace for a whole round-trip (visible over ssh). The
desktop's `reconnect_reingests_hello_and_reattaches_the_remembered_session`
test pins this; keep it green.

## 7. The typed character beats the folded key name

Shift+`/` arrives as physical key `/` with typed text `?`. The wire fold
(`input_key_name`) yields `/`, so a lookup by folded name alone fires the
wrong binding. `KeyTables::resolve_input` and `ChromeKeymap::resolve` already
implement the correct precedence (typed single character first, then folded
name, then `Any`); use them rather than reimplementing lookups.

## 8. The wire grammar cannot spell desktop chrome chords

`input_key_name` returns an empty name for Command/Super chords. Chrome
keeps those chords in `zz-client`'s `ChromeKey` under `D-`; do not store them
expecting the daemon to resolve them. Since 2026-09-05 the shared fold preserves
shift on special keys as `S-` and names shifted Tab `BTab`, matching tmux.
Character-key chrome chords still preserve shift separately from the shared
character fold.

## 9. Frame-path costs are the one performance budget

The decode → retain → paint path runs per frame. The desktop deliberately
keeps its richer `RetainedTerminalViewport` outside the core because routing
frames through it would clone every grid and re-apply every patch twice. When
extending the core or a shell, never add a per-frame allocation, copy, or
extra lock acquisition to this path; everything else in the client is
human-rate and free.

## 10. Test-harness mechanics

- Unix socket paths have a low length cap (`sun_path`) — put test sockets
  directly under `/tmp`, short names.
- A real in-process daemon is cheap and beats mocks:
  start `Daemon::new(&socket).without_user_config()`, create a named session
  with `new-session -d -s fixture "printf 'ready\r\n'; exec /bin/cat"`, and
  attach to `fixture`. The daemon starts empty without configuration that
  creates sessions; `cat` echoes your input, then sits silent.
- Some zz-daemon tests are timing-sensitive under full-workspace parallel
  load; a failure there is only real if it reproduces solo
  (`cargo test -p zz-daemon <name>`).
- gpui keymaps only grow at runtime — there is no unbind API. Live rebinding
  in gpui-land works by re-binding plus `NoAction` shadows in the owning
  context; a chord moving *between* surfaces needs a restart. Design chrome
  features with that constraint in mind.

## 11. External crates need the workspace's libghostty patch line

Cargo `[patch]` sections are not inherited through path dependencies, so a
crate OUTSIDE the zz workspace that depends on `zz-client`/`zz-daemon` by path
will try to build the upstream `libghostty-vt-sys` (whose zig build rejects
the toolchain zz pins around). Replicate the one relevant entry from the
workspace `Cargo.toml`:

```toml
[patch."https://github.com/uzaaft/libghostty-rs"]
libghostty-vt-sys = { path = "/home/demfabris/dev/zz/third_party/rust/libghostty-vt-sys" }
```

With that line plus `CARGO_TARGET_DIR` pointed at the repo's `target/`, an
external client crate resolves identically to the workspace and builds against
the warm cache in seconds. (Both independent eval builds of an external client
hit this wall; the gpui/proc-macro-error2 patches are UI-only and not needed.)

## 12. A daemon can stay empty until its first default attach

`Shared::initialize_with_mux_config_files` leaves a fresh daemon empty unless
configuration creates a session. In `Shared::attach_target_with_materialization_observer`,
an Interactive or Control client calling `attach("")` creates the first numeric
session only when no sessions exist. On a fresh daemon this is session "0".
Named attaches and Command clients do not create a missing session.
The source and its tests live in `crates/zz-daemon/src/daemon.rs`.

- Create a named fixture session and attach to that name. If a prior default
  attach already created "0", account for that existing session instead of
  assuming your fixture is the only one (see pitfall 2).
- Default attach uses the daemon's existing default context when sessions
  exist. Use an explicit target when you need a particular session.
- Render an empty snapshot both before the first session and after removal
  of the last session. Handle daemon shutdown too: its `exit-empty` option
  can stop it after the last session ends.

## 13. Don't bump `PROTOCOL_VERSION` casually

The handshake hard-rejects any mismatch — no negotiation — so a bump forces
every running daemon to restart. Client-side work (all of the above) needs no
bump. If you do change the wire: postcard tags enum variants by index, so
append variants, never reorder, and update
`knowledge/protocol/wire-protocol.md` including its byte-level example.

## 14. Free must stop and join an FFI reader

The reader owns an `Arc<InteractiveClient>` while blocked in `recv()`. Dropping
only the public handle leaves the socket, daemon attachment, thread, and event
producer alive. It can also write to a wake fd after the caller closed the read
end. Keep the reader `JoinHandle`, shut down the connection to unblock `recv()`,
and join before dropping the wake fd. The C smoke client frees and reconnects in
one process to keep this lifecycle pinned.

## 15. Terminal rows need the grapheme dictionary

`PackedCell::glyph()` values with `GRAPHEME_TABLE_BIT` set index the viewport's
UTF-8 grapheme arena. Casting those ids to characters loses emoji and combining
clusters. Wide cells also carry spacer heads/tails that must not produce a
second glyph. Use `TerminalViewport::glyph` or the C ABI's
`zz_viewport_row_text`, preserve empty narrow cells as spaces for a row-shaped
string, and truncate only between complete UTF-8 sequences.
