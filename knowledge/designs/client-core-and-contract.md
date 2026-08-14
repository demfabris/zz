---
type: Design Plan
title: Client core & contract - one brain, every face
description: Proposed plan to finish detaching zz's client from gpui - consolidate one exportable contract (command catalog + key grammar + key resolver) in zz-protocol, publish live key tables over the wire, extract a C-shaped sans-IO zz-client brain crate hardened by deterministic simulation, and export it over FFI so GTK, Qt, and Swift clients become thin skins.
status: Rungs 1-6 landed 2026-08-14 - contract consolidated in zz-protocol (key tables, engine, fold, resolve_input, catalog); full key tables published over v52; chooser keymaps daemon-side; zz-client sans-IO core gated by a daemon-backed convergence simulator; ChromeKeymap engine with zz-tui as first consumer; zz-client-ffi C ABI proven by a from-scratch C smoke client. Follow-ups landed same day - zz-tui and MuxClient reduce through ClientCore (the desktop frame path deliberately stays client-side - RetainedTerminalViewport carries painter state the core does not model, keeping the hot path byte-identical, which satisfies the bench gate by construction), and the desktop chrome chords resolve from ChromeKeymap with chrome-keybind/chrome-unbind config overrides, a D-/S- chrome-only grammar extension, and per-profile default tables. Known limit - gpui keymaps only grow, so a live cross-surface rebind needs a restart; same-surface rebinds and unbinds apply live via NoAction shadows
tags:
- client
- ffi
- keybindings
- protocol
- architecture
- multi-client
timestamp: 2026-08-14T00:00:00Z
---

# Overview

The daemon-side inversion is done: sessions, layout, key tables, copy mode, search,
selection, command execution, and status expansion all live behind protocol v51, and
the [TUI client](/designs/tui-client.md) proved a complete second client needs only
`zz-protocol` + `zz-daemon` (client half) + `zz-terminal` (model half). What is
**not** done is the client side of the same inversion: there is no shared client
brain. The GPUI app and the TUI each hand-roll connection lifecycle, layout
geometry, key encoding, scrollback, and snapshot reconciliation, and the binding
story is split between rebindable daemon key tables and ~15 hardcoded
`gpui::KeyBinding` sites of which exactly two are user-configurable.

This plan finishes the job in two moves that share one constraint:

1. **One contract.** The command catalog, the key spelling grammar, the key-table
   data model, and the key resolver consolidate into `zz-protocol` and become
   queryable — statically (linked catalog) and dynamically (daemon publishes its
   live tables over the wire).
2. **One brain.** A new `zz-client` crate absorbs everything desktop and TUI
   duplicate today, with its public API designed *as if C were consuming it* — no
   async types, no gpui types, no generics on the surface — so a thin
   `zz-client-ffi` shim + cbindgen header makes GTK, Qt, and Swift clients
   first-class skins, not ports.

End state: daemon key tables = pane semantics, one client keymap = chrome, one
client crate = shared brain. A new client starts at "render viewports, forward
keys, switch on actions" instead of ~7k lines.

# What is already true (verified 2026-08-14)

- **Clients send raw keys; the daemon resolves everything.** One `KeyEngine`
  cursor per client (`key_engines` in `crates/zz-daemon/src/daemon.rs`,
  engine in `crates/zz-mux/src/key.rs`) runs the `root`/`prefix`/
  `copy-mode`/`copy-mode-vi` tables. The TUI forwards essentially every key
  (`crates/zz-tui/src/input.rs`) and contains zero binding logic.
- **Terminal content crosses the wire render-ready** — packed cell grids with
  shared style/grapheme dictionaries (`encode_viewport_into` in
  `crates/zz-protocol/src/terminal_codec.rs`), never raw PTY bytes. This format
  is already the renderer-neutral render contract this plan reuses over FFI.
- **The publish pattern exists in miniature**: `ServerHello.prefix_bindings`,
  `EventPayload::PrefixBindingsChanged`, and `PrefixArmed`
  (`crates/zz-protocol/src/message.rs`) already push binding truth to clients.
- **Commands travel tokenized-but-unparsed** (`CommandInvocation`); parsing,
  aliases, flags, and target resolution are daemon-side
  (`crates/zz-mux/src/command.rs`).
- **Two composition seams exist**: `zz::engine` + `AppProfile`
  (`crates/zz/src/engine.rs`, `crates/zz/src/profile.rs`) let
  [zz-ios](/designs/ios-client.md) recompile the desktop client on another gpui
  backend, and `zz-daemon` with `default-features = false` is already a pure
  client SDK (`InteractiveClient`, `Endpoint`, fleet hosts).

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

Chrome-binding facts that motivate the keymap half: there is no keymap file
anywhere — every gpui binding is a programmatic `KeyBinding::new` call or raw
`on_key_down` matcher; only the browser element-selector hotkey and the editor
vim-mode toggle are user-configurable (`crates/zz/src/config/mod.rs`); vim
bindings live in four independent homes (daemon `copy-mode-vi` table, desktop
chooser/sidebar maps, the TUI's second chooser copy, and the modal vim layer in
`crates/zz-ui/src/widget/code_editor/vim/`); and the new-session hint keys are
hardcoded guesses reconciled against live `prefix_bindings`
(`resolve_binding_key` in `crates/zz/src/workspace/new_session.rs`) — a hack
that generalizes away once full tables are published.

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

Generalize the `prefix_bindings` pattern to the whole `KeyTables`:

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

## Pillar 4 - zz-client: the shared brain, C-shaped from day one

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
API's conformance test. `zz-ios` keeps riding the desktop through `zz::engine`
unchanged.

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

## Pillar 6 - zz-client-ffi

Thin `#[no_mangle]` shim + cbindgen header, libghostty-style — the same shape zz
already embeds and trusts. C ABI is the lowest common denominator that covers all
three announced targets natively (GTK is C; Qt eats C or a small RAII wrapper;
Swift imports C headers first-class). No UniFFI: it covers only Swift of the
three and adds codegen machinery cbindgen makes unnecessary.

- **Render contract**: the packed viewport *is* the FFI render type — flat cell
  buffer + style table + grapheme arena + generation counters, handed out as
  `const zz_viewport_t*` snapshots. Do not invent a second representation.
- **Main-loop integration**: the core owns its reader thread and exposes an
  eventfd/pipe fd plus `drain_events()` — plugs into GSource, `QSocketNotifier`,
  and DispatchSource without cross-thread callback footguns. No callbacks from
  Rust threads into toolkit land.
- **Exports**: `zz_commands()` (static catalog), `zz_key_tables()` (live,
  refreshed on `KeyTablesChanged`), `zz_send_key()` (raw forwarding),
  `zz_viewport()` / `zz_send_text()` / resize / command execution, and an event
  queue yielding `ZZ_EVENT_ACTION { action }` for resolved chrome actions.

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

# Rung ladder

Ship each rung independently; never let a higher rung block a lower one.

1. **Contract consolidation.** `canonical_key` + `key.rs` + `COMMAND_SPECS` move
   to `zz-protocol`; `zz-mux` re-exports; delete `canonical_prefix`. Pure
   refactor, no bump.
2. **Publish key tables** (`ServerHello.key_tables` + `KeyTablesChanged`);
   replace the new-session hint reconciliation with published truth. Bump.
3. **Overlay tables daemon-side**; delete both chooser keymaps. Same or next
   bump.
4. **Extract `zz-client`**, TUI first (it is closest to the model), then port
   `MuxClient`/desktop onto it. The gpui-free dep tree is the compile-time fence,
   exactly as it was for zz-tui; the simulator lands with this rung and gates it.
5. **Chrome keymap + second engine**; convert gpui `KeyBinding` sites to action
   consumers.
6. **`zz-client-ffi`** + cbindgen header + a smoke consumer (a ~300-line C or
   GTK proof client that attaches, renders one terminal pane, forwards keys).

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
