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

The daemon auto-creates a default session at boot, so "iterate every session's
panes" returns panes your client is not attached to — and those panes never
receive terminal frames (frame fanout is gated by the client's visible set).
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
  `Daemon::new(&socket).without_user_config()` + a fixture command like
  `"printf 'ready\r\n'; exec /bin/cat"` gives deterministic, quiescent pane
  content (`cat` echoes what you type, then sits silent).
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

## 12. A daemon can start and remain without sessions

As of 2026-09-07, starting a daemon through a command client need not create
session "0". Explicitly create the session required by a fixture; do not kill
or rename an assumed boot session. The GUI's default interactive attachment
can create its initial session through the current attach contract. A named
attachment and a command-client snapshot do not imply that creation happened.
Clients must also render and recover after the last session disappears.

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

## 16. An in-process test daemon never drops its clients

`kill-server` releases the socket *path*, but each client connection lives in
a detached thread whose loop only exits on client EOF — a client wired
straight to a test daemon can never observe a disconnect. Reconnect tests need
a cuttable transport: a ~70-line unix-socket relay with `cut()`/`restore()`
(see `Relay` in `crates/zz-gtk/tests/engine.rs`). Two corollaries: wait for
the old socket file to vanish before rebinding (the dying listener removes the
path on the way out, deleting a replacement's socket underneath it), and
rebuild a replacement session *out of* the boot session (`rename-session` +
`split-window` + `kill-pane`) rather than beside it, or a client retrying
mid-rebuild lands on the session you are about to kill.

## 17. The `MissingTarget` fallback is the normal reconnect path

Session ids start at `$0` and a restarted daemon renumbers from scratch, so a
remembered id usually does not exist after a restart. `attach("")` resolves to
the LOWEST session id, which makes a useful test discriminator: attach to a
non-boot session and a client that forgot its attachment provably lands wrong.

## 18. Replay geometry after a reconnect, don't just clear it

Widgets only re-measure when the toolkit re-allocates them, and a reconnect
does not cause a re-allocation — a cleared dedup cache alone means the new
daemon never learns any pane's size. Drain the cache into a replay list at
dial time and republish for every pane the re-attached session still has.

## 19. Per-client overlays cannot be driven from a `CommandClient`

`choose-tree`, `choose-buffer`, `command-prompt`, and `display-panes` reject
non-interactive clients ("requires an interactive client") and publish only to
the client that issued them — no test can open another client's chooser. Test
overlay view-models directly, drive the real key path through your own engine,
or hand verification to a human. The exception: `copy-mode -t %n` is
pane-scoped and visible to every attached client, so mode indicators are
headlessly checkable.

## 20. Native status and command-prompt publications are typed

Graphical clients derive native status from `zz_client::StatusBarModel` and
`MuxSnapshot`, rather than rendering `StatusLine` fragments. The TUI retains
the daemon's formatted status path. Track each `CommandPromptChanged`
publication with a revision: a newly opened prompt can have identical content
to the previous one, while unrelated overlay changes must preserve local edits.
Honor `CommandPromptMode` before offering completions or letting a text widget
consume keys.

## 21. `force_selection` means "the user is overriding the program"

Not "this pane isn't mouse-tracking". The daemon uses that bit to refuse
`OpenUri` on click and to route wheel notches past alternate-scroll — a
client that widens it (e.g. `shift || !mouse_tracking`) silently loses link
activation and full-screen-app scrolling with no error anywhere. Set it only
for Shift or a Ctrl/Cmd multi-click, exactly as the desktop does. Related:
link hover requires the modifier held AND fresh motion — synthesize one
`Mouse(Motion, button: None)` when the modifier is pressed over a stationary
pointer, or hover never lights.

## 22. A core-based client wants a backfill-only history ring

The desktop feeds its scrollback ring from the pre-patch viewport at
patch-apply time — that requires the second retention the desktop keeps
outside `ClientCore`, and adding it to a core client violates the frame-path
budget. Backfill on demand (`HistoryRequest`/`HistoryChunk`), and retire the
ring on `viewport.generation` change — `scrollbar.total`/`offset` are NOT
sufficient staleness witnesses, because a capped scrollback evicts a row per
new line without moving either.

## 23. The command-output pager has no keys of its own

`InputMessage::CommandOutputView` carries only view actions; keys travel as
ordinary `InputMessage::Key` on the anchor pane, because the daemon already
swapped that client's key table into copy-mode. Don't hardcode `q`. Also:
`mode-keys` defaults to emacs, so copy-mode search is `C-s` — a test that
sends `/` waits forever.

## 24. Command grammar corners that bite fixtures

`new-pane` opens a *picker* pane, not a terminal — use `split-window -h/-v`
for a terminal fixture. Both take a pane target (`%n`); window/session
spellings are rejected there. `list-panes` has no `-a`. The daemon's own enums
(`AppearanceConfigKey::ALL` ∪ `MuxOptionKey::ALL`) are the authoritative list
of daemon-owned config keys — never restate that set in a client.

## 25. NEVER inject synthetic input on a live desktop session

XTEST/ydotool/wtype keystrokes go to whatever window the compositor has
focused — during this project an agent's test strings landed in the user's
terminal, and `_NET_ACTIVE_WINDOW` had "confirmed" the wrong window first.
Verification alternatives, in order: engine-level tests against a real daemon
(the send-text path proves typing end to end), unit tests on the translation
layer, D-Bus action activation (`org.gtk.Actions`), and finally an explicit
human-verification handoff list in your report.

## 26. Multi-agent builds sharing one `CARGO_TARGET_DIR` race on the binary

Two worktrees building the same crate name overwrite each other's executable
— agents have screenshotted a sibling's build and debugged features that
weren't theirs. Build and `cp` to a private path in ONE shell invocation, then
verify the copy with a `strings` marker unique to your change before running
it. Never `pkill -f <pattern>` where the pattern matches your own wrapper
shell's command line (it kills your tool call; use `pkill -x`), and keep
scratch files in a private subdirectory of the shared scratchpad.

## 27. A freshly connected, unattached client has no snapshot

The daemon publishes `MuxSnapshot` only on change, so a fleet host you connect
to but do not attach stays empty forever. `request_resync()` immediately after
connect is the sanctioned initial-tree request — the desktop does exactly
this, and it does not contradict pitfall #1: having no snapshot at all is not
a sequence gap.

## 28. Multi-daemon state is per daemon — never key across hosts

Pane ids collide across daemons (`%1` on host A and `%1` on host B are
different terminals), so any pane→widget or pane→state map must be cleared on
a host switch. An undrained `FrameInbox` latches its wake flag — clear it when
its host leaves the screen or `FramesReady` never fires again. And once more
than one daemon is connected, AIM your commands: an unaimed `kill-server` on
quit stops whichever daemon is active — possibly a remote machine's.

## 29. Retiring a live connection takes more than a flag

A quiet connection blocks in `recv()` forever, so a reader thread never
notices a closed flag on its own. Set the flag AND provoke a response the
daemon will send (`detach()` triggers a snapshot publish). Related Rust trap
that cost a 300-second test hang: a `MutexGuard` temporary inside a `for`
iterator expression lives for the whole loop body — bind the collected `Vec`
to a variable before iterating, or any re-entrant lock deadlocks.

## 30. `unix://` is a first-class fleet endpoint

A `host-<name> = unix:///tmp/…` config line pointing at a second local daemon
is a complete fleet fixture — every layer above the transport (host rows,
per-host reconnect, frozen frames, host removal) is testable without ssh.

## 29. An "isolated" XDG_CONFIG_HOME is not isolated until you seed it

`zz`'s config candidate resolution falls back to the real
`~/.config/zz/config` when the isolated directory holds no config file — so a
scratch `XDG_CONFIG_HOME` sandbox silently reads (and side-effect files like
the first-run `import-prompted` marker land in) the user's real profile on
first launch. Always write a config file into the scratch dir before the
first run.
