---
type: Design Plan
title: Client core & contract - one brain, every face
description: Decision record for the shared client contract - command and key ownership moved to zz-protocol, v52 publishes live tables, zz-client provides sans-IO reduction and chrome actions, and a narrow C ABI proves the integration shape.
status: Contract consolidation, v52 key-table publication, daemon chooser tables, ClientCore reduction, ChromeKeymap, and desktop/TUI adoption shipped 2026-08-14. The native iPhone client extended zz-client-ffi with mux snapshots, styled terminal planes, raw keys, focus, scroll, damage, and disconnect events on 2026-08-15. Broader connection, history, Kitty, and non-terminal viewport extraction remain open. GPUI cross-surface rebinding still needs a restart
tags:
- client
- ffi
- keybindings
- protocol
- architecture
- multi-client
timestamp: 2026-08-26T00:00:00-03:00
---

# Overview

The shipped split landed on protocol v52. v53 added the agent runtime lane, v54 completed its
session-control shape, and v55 added stable client identity for owned Agent draft recovery. The
live `PROTOCOL_VERSION` is 77; see the [wire protocol](/protocol/wire-protocol.md).
`zz-protocol` owns the command catalog, key grammar, tables,
and resolver; the daemon publishes every live table. `zz-client::ClientCore` reduces decoded
messages into shared state and typed effects, while `ChromeKeymap` owns client-side chords. The TUI
uses both directly. The GPUI `MuxClient` sends non-frame messages through the core but keeps terminal
frames in `RetainedTerminalViewport`, avoiding a second patch application on the paint hot path.

`zz-client-ffi` proves the C integration shape with a pollable wake fd and a from-scratch smoke
client. The native iPhone client extended the hand-maintained header with raw key forwarding,
style/grapheme tables, generation counters, mux/session/pane snapshots, damage rows, terminal focus
and scrolling, appearance, and disconnect events. Catalog/table access, chrome actions, history,
Kitty images, and non-terminal viewport models remain outside the ABI.

The sections below retain the original proposal and its acceptance criteria. The rung ladder marks
the parts that shipped and the parts that remain design intent.

# Historical starting point (verified before v52 on 2026-08-14)

- **Pane keys resolve on the daemon.** One `KeyEngine` cursor per client
  (`key_engines` in `crates/zz-daemon/src/daemon.rs`; the engine now lives in
  `crates/zz-protocol/src/key.rs`) runs the `root`/`prefix`/`copy-mode`/
  `copy-mode-vi` tables. The TUI forwarded pane keys but still held local chrome
  and sidebar chord matches.
- **Terminal content crosses the wire render-ready** — packed cell grids with
  shared style/grapheme dictionaries (`encode_viewport_into` in
  `crates/zz-protocol/src/terminal_codec.rs`), never raw PTY bytes. This format
  is already the renderer-neutral render contract this plan reuses over FFI.
- **The old publish pattern existed in miniature**: v51 carried
  `ServerHello.prefix_bindings` and `PrefixBindingsChanged`. v52 replaced them
  with `ServerHello.key_tables` and `KeyTablesChanged`; `PrefixArmed` remains.
- **Commands travel tokenized-but-unparsed** (`CommandInvocation`); parsing,
  aliases, flags, and target resolution are daemon-side
  (`crates/zz-mux/src/command.rs`).
- **Two composition seams existed**: `zz::engine` + `AppProfile` let the former GPUI iPad app
  recompile the desktop client on another backend, while `zz-daemon` with
  `default-features = false` supplied a pure client SDK. The GPUI iOS seam was deleted when the
  [native iPhone client](/designs/ios-client.md) moved onto `zz-client-ffi`.

# The gap - what every new client re-hand-rolls today

| Logic | Desktop copy | TUI copy |
|---|---|---|
| Wire `KeyInput` encoding | `terminal_key_input` in `crates/zz/src/mux/prefix.rs` | `key_input` in `crates/zz-tui/src/input.rs` |
| `LayoutNode` ratio → rects | `pane_rects` in `crates/zz/src/pane/layout.rs` | `resolve` in `crates/zz-tui/src/layout.rs` |
| Client brain (reconnect, viewport retention, patch apply, history) | `MuxClient` in `crates/zz/src/mux/client.rs` (a gpui `Entity`; backoff ladder + `HistoryRing`) | own loop in `crates/zz-tui/src/app.rs` (one-shot reconnect, no history ring) |
| Chooser vi/emacs navigation | keystroke→action maps in `crates/zz/src/chooser/tree.rs` / `buffer.rs` | second copy in `crates/zz-tui/src/input.rs` |
| Key spelling normalization | `canonical_prefix` in `crates/zz/src/mux/prefix.rs`, kept in deliberate lockstep | (n/a - forwards raw) |
| Sidebar tree projection | `crates/zz/src/mux/nav.rs` + `workspace/sidebar.rs` | `crates/zz-tui/src/sidebar.rs` |
| Kitty image assembly | `KittyImageCache` on the terminal element | `KittyImageAssembler` in `crates/zz-tui/src/kitty.rs` |

At the starting point, GPUI bindings were programmatic `KeyBinding::new` calls or raw
`on_key_down` matches, and only two actions were configurable. Chooser and sidebar maps were
duplicated across clients, while new-session hints reconciled hardcoded guesses against the lone
published prefix table. Full table publication and `ChromeKeymap` removed those specific splits;
widget-internal editing and the zz-ui editor's modal vim layer remain separate by design.

# Design

## Pillar 1 - zz-protocol becomes the contract crate

Move three renderer-free pieces out of `zz-mux` (which already depends on
`zz-protocol`, so the direction is clean):

- **`canonical_key`** and the key spelling grammar. Kills the client mirror
  `canonical_prefix` outright.
- **`KeyTables` / `Binding` / `KeyEngine` / `KeyDecision`** — the data model
  *and* the resolver. The engine is a pure state machine with no daemon
  dependency; moving it lets the client core run an identical second instance
  (Pillar 5).
- **`COMMAND_SPECS`** (the command catalog). Today it is one of only two reasons
  the desktop client links `zz-mux` at all (`crates/zz/src/command/completion.rs`);
  in the contract crate every client gets palette, completion, and help from the
  same table the daemon dispatches against, and it can be dumped as JSON for the
  docs site.

`zz-mux` re-exports during the transition, then becomes purely the daemon's
execution engine. No protocol bump; pure refactor.

## Pillar 2 - the daemon publishes its live tables

Replace the v51 prefix-only publication with the whole `KeyTables`:

- `ServerHello.key_tables: Vec<KeyTableSnapshot>` (table name, key spelling,
  bound `CommandInvocation`s, repeat flag, note)
- `EventPayload::KeyTablesChanged` on any `bind-key`/`unbind-key`/`source-file`

Resolution stays daemon-side — the input path does not change. Publication is for
everything clients currently fake: which-key/help overlays, shortcut labels,
hint-key reconciliation, and conflict detection against chrome chords.
`list-keys` remains the human view; this is the machine view. Protocol bump.

## Pillar 3 - overlay key maps move daemon-side

The daemon already owns chooser/prompt/display-panes *state* and clients already
send opaque `ChooseTreeAction`/`ChooseBufferAction`/`CommandPromptAction`
navigation; the only client-side piece is the keystroke→action mapping,
duplicated in desktop and TUI. Add daemon tables — `choose-tree`,
`choose-buffer`, `prompt`, `display-panes` — seeded with today's vi/emacs
defaults, and let clients forward raw keys into them exactly as they do for
panes. Deletes both hardcoded maps and makes chooser vim-nav rebindable via
`bind-key -T choose-tree`. Free for every future client.

## Pillar 4 - zz-client: original target, partially landed

New crate sitting between the contract and any skin. Contents (lifted from
`MuxClient` and `zz-tui`, decoupled from gpui):

- connection + handshake + the reconnect/backoff ladder + fleet host handling
- snapshot store, per-pane viewport retention, patch application, `HistoryRing`
  scrollback + chunked backfill
- `LayoutNode` → rect solver, unit-parameterized (px for GUI skins, cells for
  TTY) — one implementation replaces both copies
- platform-keystroke → wire `KeyInput` encoding
- the chrome `KeyEngine` instance (Pillar 5) and action emission
- kitty image assembly

The API discipline is the load-bearing decision: **a handle, messages in, events
+ snapshots out; no async types, no gpui types, no generics on the public
surface**. Desktop and TUI consume this same C-shaped API natively — if the
desktop grows a Rust-only convenience layer the core does not export, the
GTK/Qt/Swift clients become second-class again, so the FFI shim doubles as the
API's conformance test. The native iPhone app consumes that same surface through
`zz-client-ffi`; it does not import desktop GPUI modules.

## Pillar 5 - chrome bindings are data, resolved by the same engine

What remains genuinely client-local after Pillar 3 is chrome: browser
tabs/omnibox, UI zoom, sidebar navigation, settings, terminal font-size chords.
These become a default keymap (data compiled into `zz-client`) using the same
`bind` spelling, overridable from `zz/config`, resolved by a second `KeyEngine`
instance over client tables (`ui`, `sidebar`, `browser`). Skins never define or
parse chords — the core emits named actions and the skin switches on them.

| Authority | Tables | Rebind via |
|---|---|---|
| daemon | root, prefix, copy-mode(-vi), overlay tables | `zz/mux.conf` / `bind-key` (unchanged) |
| zz-client | ui, sidebar, browser | default keymap + `zz/config` overrides |

The zz-ui widget layer's internal bindings (text inputs, menus) and the code
editor's modal vim engine stay where they are — widget-internal editing behavior
is not chrome and not part of the contract.

## Pillar 6 - zz-client-ffi target and shipped proof

The shipped `#[no_mangle]` shim and hand-maintained header now cover connection, pollable event
wake/drain, attach, typed mux snapshots, caller-owned styled terminal viewports, raw key and text
input, command execution, resize, separate client-window and terminal pane focus, scrolling, damage,
appearance, and disconnect events. The
viewport is the render contract: a flat cell plane plus style table, grapheme arena, cursor, colors,
and generation counters remain alive until the caller releases the handle. The reader stays inside
the core and toolkits integrate its wake fd with GSource, `QSocketNotifier`, or DispatchSource; Rust
threads never call toolkit code.

The C smoke compiles and links the contract, creates sessions and panes, renders styled content,
types through the raw-key path, reports client focus and blur, kills the attached session, reattaches
a survivor and recovers its viewport, then frees and reconnects in one process. Catalog and live key-table access, resolved
chrome action events, history, Kitty images, and non-terminal viewport models remain outside the ABI.

The desktop shell caches its desired window-focus state outside the pane-focus path. Construction
seeds `true` only when the window is already active; an inactive window waits for its first real
activation callback. The shell sends the desired state only after attachment is ready and replays it
once after each `Attached` event, so a reconnect, host switch, or session attach receives a fresh
client-focus notification even when the OS window never changes activation. A failed same-connection
session attach restores the old ready epoch and flushes a focus change cached during the request.
An unrelated request-zero error leaves both pending and ready focus epochs unchanged.
`zz_client_attach` returning true confirms that the client wrote the request, not that the daemon
attached it. FFI shells wait for `ZZ_EVENT_ATTACHED` before calling `zz_client_set_focused`. The
iPhone client follows that contract for initial, selected-session, recovery, and recreated-session
attachments without replaying pane focus.

The TUI assumes its outer terminal is foregrounded when it enters focus-reporting mode. It caches
later `FocusGained` and `FocusLost` events while attachment is pending, then sends the latest
`ClientFocus` value once after each `Attached` event. A failed sidebar session attach restores the
retained session's ready epoch. A separate protocol-owned attach-attempt marker owns missing-target
retry and fallback, returns to idle on success or terminal failure, and ignores unrelated request-zero
errors. Repeated reports with the same value do not send another client-focus notification.
Attachment replay does not synthesize `TerminalViewAction::Focus`; real outer-terminal focus events
retain pane focus when the active pane is a terminal.

# What stays out of the core

- **CEF and ACP are process runtimes, not state.** The core exposes descriptors
  (`BrowserDescriptor`, `AgentDescriptor`) and a provider seam like the TUI's
  `BrowserFrameProvider` (`crates/zz-tui/src/browser.rs`); a GTK skin shows
  placeholder cards or brings WebKitGTK, Qt brings QtWebEngine, Swift brings
  WKWebView. The agent *reducer* (streaming state machine in
  `crates/zz/src/agent/controller.rs`) is pure state and can migrate into the
  core later so agent panes work everywhere; the provider process stays with the
  skin.
- **The zz-ui editor vim layer** — an editor engine behind a config flag, not a
  binding surface.
- **gpui anything.** The core must build for a musl target with no display
  server as its CI smoke.

# Performance

The extraction is a lateral move on the hot paths, not a new layer — held by two
rules and one gate:

- **Daemon + wire untouched.** Rungs 1-3 are compile-time moves plus one event
  (`KeyTablesChanged`) that fires on `bind-key`, not per keystroke.
- **Desktop frame path keeps its shape.** Today: reader thread decodes →
  depth-1 bounded channel (`MAX_PENDING_DECODED_MESSAGES` backpressure in
  `crates/zz/src/mux/client.rs`) → per-pane `Arc<RwLock<RetainedTerminalViewport>>`
  → paint. With the core: reader thread decodes into core state → per-pane
  generation bump → fd wake → skin drains. Same hop count, same backpressure,
  same `Arc` handoffs; desktop links the core as a normal Rust crate, so LTO
  inlines across it — the C-shaped API constrains signatures, not codegen.
- **FFI cost is refcount + pointer handoff.** Decode happens once either way;
  `zz_viewport_acquire`/`release` hand out Arc-backed pointers.
- **Rule 1 - no over-invalidation**: the core must surface per-pane damage
  (viewport generations already exist), never a global dirty bit, or skins
  repaint the world.
- **Rule 2 - the FFI never copies a viewport**: torn-state safety comes from
  acquire/release refcounting, not defensive memcpy.
- **The gate**: `bench/run.sh` measures terminal throughput end to end; rung 4's
  acceptance criterion is desktop-on-core benchmarks within noise of
  desktop-today, or it does not ship.

# Simulation testing - the sans-IO core and its simulator

The load-bearing test decision doubles as an API decision: **the core's state
machine does no IO and owns no threads** — `handle_bytes`, `handle_tick(now)`
with an injected clock, `poll_event`; no `Instant::now()` inside, no
iteration-order nondeterminism (key tables are already `BTreeMap`). The reader
thread and eventfd live in a thin shell around it. Prior art: quinn-proto and
str0m (sans-IO protocol cores); FoundationDB and TigerBeetle's VOPR
(deterministic simulation). This is only possible because of the extraction —
today the brain is welded to gpui entities and cannot run without a display
server.

**The simulator** (one binary, run continuously in CI): `zz-daemon` is already a
library, so spin a real daemon plus N simulated client cores in one process over
in-memory duplex pipes. A seeded PRNG scheduler interleaves client inputs,
dribbles the byte stream one byte at a time, splits frames at random offsets,
disconnects mid-patch, duplicates and delays ticks, and reattaches at arbitrary
points. Every failure is a seed; every seed replays exactly; proptest shrinks
failing sequences.

**Oracles asserted after every scheduler step:**

- **Convergence** — at quiescence every client's retained viewport is byte-equal
  to the daemon's authoritative one; N clients on one session converge
  identically (tests multi-device-attach for free).
- **Patch/full duality** — the protocol's built-in oracle: applying the patch
  stream must equal requesting a full frame at any point; divergence is a bug by
  construction.
- **Resync soundness** — injected sequence gaps always converge via `Resync`;
  detach/reattach at any point is idempotent.
- **Key-engine equivalence** — a key sequence replayed through the daemon's
  `KeyEngine` and through a client engine loaded from the published
  `KeyTableSnapshot` yields identical decisions; keeps the published tables
  honest forever.
- **Bounded memory** — history-ring and dictionary caps hold across thousands of
  reconnects.

**Around the simulator:** cargo-fuzz on `decode_protocol_frame` and patch
application (sans-IO means the fuzz target is the whole brain, not just the
codec); recorded real sessions as a golden trace-replay corpus (the wire is
length-prefixed frames — trivially recordable); property tests for the pure
functions (layout solver: no overlap, full coverage, px/cell agreement;
`canonical_key` round-trips); and an FFI conformance harness that runs the same
simulation through the C header under ASan/TSan and asserts the Rust API and C
API emit identical event sequences.

# Rung ladder and result

1. **Contract consolidation: shipped.** `canonical_key`, `key.rs`, and `COMMAND_SPECS` moved to
   `zz-protocol`; `zz-mux` re-exports them.
2. **Publish key tables: shipped in v52.** `ServerHello.key_tables` and `KeyTablesChanged` replaced
   prefix-only publication.
3. **Overlay tables daemon-side: shipped.** Chooser keys resolve through daemon tables.
4. **Extract `zz-client`: partially shipped.** `ClientCore` owns shared protocol reduction and the
   simulator covers convergence. Connection lifecycle, layout, history, key encoding, and Kitty
   assembly remain in their shells; desktop keeps its retained terminal frame path.
5. **Chrome keymap: shipped.** Desktop and TUI resolve client-owned actions through profile tables;
   desktop config supports `chrome-keybind` and `chrome-unbind`.
6. **`zz-client-ffi`: proof surface shipped.** The C smoke client attaches, reads rows, types,
   creates a session and pane, frees, and reconnects. The full catalog/action contract above remains
   open, and the header is hand-maintained rather than generated by cbindgen.

# Hard parts

- **`MuxClient` is a gpui `Entity`** with entity-context plumbing throughout;
  the extraction is a rewrite of its ownership model (plain struct + event
  queue), not a move. The TUI's `app.rs` loop is the better starting skeleton.
- **Per-client geometry pressure**: more client kinds exercise
  latest-active-wins PTY sizing and per-view state more than today's
  multi-GUI setups; no protocol change expected, but it is the seam to watch.
- **FFI surface stability**: once GTK/Qt/Swift skins exist, `zz_viewport_t`
  layout changes become ABI breaks. Version the header alongside
  `PROTOCOL_VERSION` and keep the struct mirroring the wire format so the two
  can only drift together.
- **Keymap conflict semantics**: with chrome tables resolving client-side and
  pane tables daemon-side, the precedence rule must be written down once
  (proposal: prefix claim > chrome table > raw forward > daemon tables, i.e.
  today's observable order, made explicit and documented in
  [key tables](/tmux/key-tables.md) when built).
- **Fork surface**: none — this plan touches no gpui fork patches; `zz-ui`
  widget bindings are explicitly out of scope.

# Related

- [TUI client](/designs/tui-client.md) - the existing proof that a protocol-only
  client works; rung 4 subsumes its client brain.
- [iOS client](/designs/ios-client.md) - the recompile-the-desktop pattern this
  plan leaves intact.
- [Fleet attach](/designs/fleet-attach.md) - host handling the core inherits.
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) - doctrine for what
  the daemon owns.
- [Layered configuration](/designs/layered-config-and-settings-view.md) - the
  `zz/config` override channel the chrome keymap rides.
