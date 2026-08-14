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

`input_key_name` returns an empty name for Command/Super chords and folds
Shift away next to Control — correct for panes (a PTY can never receive
Cmd-anything), wrong for chrome. Chrome chords use the extended spelling
(`D-`, `S-`) that exists only in `zz-client`'s `ChromeKey`. Do not "fix" the
wire fold to know these modifiers, and do not store chrome chords expecting
the daemon to resolve them.

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

## 12. The daemon is never session-less

`Shared::initialize` auto-creates session "0" at boot when nothing restores,
and `attach("")` resolves to that default session. Two consequences:

- A test that wants exactly one session with a controlled name should
  **rename the boot session** (`rename-session`) rather than create a second
  one — otherwise default-attach lands on "0" while your fixture session sits
  unattached and frameless (see pitfall 2).
- "Attach to the default session" in a user-facing client means session "0"
  on a fresh daemon, not the most recently created session.

## 13. Don't bump `PROTOCOL_VERSION` casually

The handshake hard-rejects any mismatch — no negotiation — so a bump forces
every running daemon to restart. Client-side work (all of the above) needs no
bump. If you do change the wire: postcard tags enum variants by index, so
append variants, never reorder, and update
`knowledge/protocol/wire-protocol.md` including its byte-level example.

## 14. An in-process test daemon never drops its clients

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

## 15. The `MissingTarget` fallback is the normal reconnect path

Session ids start at `$0` and a restarted daemon renumbers from scratch, so a
remembered id usually does not exist after a restart. `attach("")` resolves to
the LOWEST session id, which makes a useful test discriminator: attach to a
non-boot session and a client that forgot its attachment provably lands wrong.

## 16. Replay geometry after a reconnect, don't just clear it

Widgets only re-measure when the toolkit re-allocates them, and a reconnect
does not cause a re-allocation — a cleared dedup cache alone means the new
daemon never learns any pane's size. Drain the cache into a replay list at
dial time and republish for every pane the re-attached session still has.

## 17. Per-client overlays cannot be driven from a `CommandClient`

`choose-tree`, `choose-buffer`, `command-prompt`, and `display-panes` reject
non-interactive clients ("requires an interactive client") and publish only to
the client that issued them — no test can open another client's chooser. Test
overlay view-models directly, drive the real key path through your own engine,
or hand verification to a human. The exception: `copy-mode -t %n` is
pane-scoped and visible to every attached client, so mode indicators are
headlessly checkable.

## 18. The daemon publishes status text pre-stripped and prompts un-echoed

`#[fg=…]` style directives are removed during expansion — `StatusLine`
segments are plain text; style them with your toolkit, never parse markup.
And the daemon deliberately never echoes `CommandPromptAction::Update`: its
retained prompt input stays at the open value, so re-reading prompt state on a
generic overlay notification wipes what the user typed. Distinguish a
genuinely new prompt from a republication.

## 19. `force_selection` means "the user is overriding the program"

Not "this pane isn't mouse-tracking". The daemon uses that bit to refuse
`OpenUri` on click and to route wheel notches past alternate-scroll — a
client that widens it (e.g. `shift || !mouse_tracking`) silently loses link
activation and full-screen-app scrolling with no error anywhere. Set it only
for Shift or a Ctrl/Cmd multi-click, exactly as the desktop does. Related:
link hover requires the modifier held AND fresh motion — synthesize one
`Mouse(Motion, button: None)` when the modifier is pressed over a stationary
pointer, or hover never lights.

## 20. A core-based client wants a backfill-only history ring

The desktop feeds its scrollback ring from the pre-patch viewport at
patch-apply time — that requires the second retention the desktop keeps
outside `ClientCore`, and adding it to a core client violates the frame-path
budget. Backfill on demand (`HistoryRequest`/`HistoryChunk`), and retire the
ring on `viewport.generation` change — `scrollbar.total`/`offset` are NOT
sufficient staleness witnesses, because a capped scrollback evicts a row per
new line without moving either.

## 21. The command-output pager has no keys of its own

`InputMessage::CommandOutputView` carries only view actions; keys travel as
ordinary `InputMessage::Key` on the anchor pane, because the daemon already
swapped that client's key table into copy-mode. Don't hardcode `q`. Also:
`mode-keys` defaults to emacs, so copy-mode search is `C-s` — a test that
sends `/` waits forever.

## 22. Command grammar corners that bite fixtures

`new-pane` opens a *picker* pane, not a terminal — use `split-window -h/-v`
for a terminal fixture. Both take a pane target (`%n`); window/session
spellings are rejected there. `list-panes` has no `-a`. The daemon's own enums
(`AppearanceConfigKey::ALL` ∪ `MuxOptionKey::ALL`) are the authoritative list
of daemon-owned config keys — never restate that set in a client.

## 23. NEVER inject synthetic input on a live desktop session

XTEST/ydotool/wtype keystrokes go to whatever window the compositor has
focused — during this project an agent's test strings landed in the user's
terminal, and `_NET_ACTIVE_WINDOW` had "confirmed" the wrong window first.
Verification alternatives, in order: engine-level tests against a real daemon
(the send-text path proves typing end to end), unit tests on the translation
layer, D-Bus action activation (`org.gtk.Actions`), and finally an explicit
human-verification handoff list in your report.

## 24. Multi-agent builds sharing one `CARGO_TARGET_DIR` race on the binary

Two worktrees building the same crate name overwrite each other's executable
— agents have screenshotted a sibling's build and debugged features that
weren't theirs. Build and `cp` to a private path in ONE shell invocation, then
verify the copy with a `strings` marker unique to your change before running
it. Never `pkill -f <pattern>` where the pattern matches your own wrapper
shell's command line (it kills your tool call; use `pkill -x`), and keep
scratch files in a private subdirectory of the shared scratchpad.
