---
type: Rust Crate
title: zz crate (the GPUI client)
description: The long-lived GPUI desktop client. Reconciles recursive pane layouts and hosts stable terminal, Chromium browser, and native Agent pane entities.
resource: crates/zz/src/lib.rs
tags: [gpui, crate, client, terminal, browser, agent, ui]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

`zz` (package and binary name `zz`) is the **long-lived GPUI desktop client**. It never owns
mux state itself: it holds a socket/named-pipe connection to the daemon
(`zz_daemon::InteractiveClient`, wrapped by `mux::client::MuxClient`), a set of local Chromium
sessions (`zz_browser::BrowserRuntime`, wrapped by `browser::controller::BrowserController`), and
and a viewport onto the daemon's ACP runtimes (`agent::AgentController`), then reconciles them against
`zz_protocol::MuxSnapshot` on every render. It also owns
the CEF *subprocess* entrypoints: `zz` itself re-execs as a renderer/GPU/utility subprocess
(detected via `--type=`), and a tiny sibling binary (`zz_helper`) is that subprocess executable
on every desktop CEF bundle. macOS also clones it into the Helper.app roles.

`MuxClient` retains each decoded `MuxSnapshot` behind an `Arc`, so `AppView`, the workspace sidebar,
and the command palette share one immutable allocation until the daemon publishes a replacement.
Render-time snapshot access is therefore an Arc bump rather than a recursive clone of every session,
window, pane, title, URL, and layout node. The sidebar compares a cheap revision of the snapshot
generation, attached session, and active target before constructing its `MuxTreeModel`, so
notifications unrelated to mux structure also skip the full tree projection.

The crate's job is narrow and mechanical by design: turn `MuxSnapshot` + protocol events into a
tree of stable GPUI entities (`Entity<PanePickerView>` / `Entity<TerminalView>` /
`Entity<BrowserView>` / `Entity<AgentView>`, one per `PaneId`,
created once and retained across re-renders) and two custom `gpui::Element` implementations
(`TerminalElement`, `BrowserElement`) that do the per-frame painting. All tmux-compatible
parsing, layout math, and session/window/pane lifecycle live in [`zz-mux`](/crates/zz-mux.md) and
[`zz-daemon`](/crates/zz-daemon.md); this crate renders, forwards input, and owns the GUI-scoped CEF
runtimes whose durable identity comes from daemon snapshots. Agent panes are no longer in that set:
the daemon spawns and owns the ACP child, and this crate reduces the stream it publishes.

# Process modes (`main.rs` / `lib.rs`)

`zz::run()` (in `lib.rs`, invoked from `main.rs`) branches before ever opening a window:

1. **CEF subprocess** . `is_cef_subprocess()` checks for a `--type`/`--type=` argument; if present,
   control passes straight into `zz_browser::bootstrap()` without touching diagnostics, the mux
   client, or a window (this is the path CEF's renderer/GPU/zygote processes take when they re-exec
   the same `zz` executable).
2. **`daemon` command** . runs `zz_daemon::Daemon::run_foreground()` in place (used when the app
   auto-spawns its own daemon, and by `cargo run -p zz -- daemon`).
3. **`proxy` command** . `zz_daemon::run_socket_proxy`, the stdio socket proxy used by in-process
   ssh (iOS russh, Windows-port shape).
4. **`attach` command** . hands off to [`zz-tui`](/designs/tui-client.md): a raw-terminal client
   that speaks the same wire protocol (`zz attach [session]`).
5. **CLI command mode** . any other leading argument (`list-panes`, `split-window`, `kill-server`,
   …) is sent through a short-lived `zz_daemon::CommandClient` to an existing or freshly spawned
   daemon. A read-only command can leave a newly spawned daemon empty; the first explicit
   `new-session` then receives numeric name `0` and zero-based ids. Output prints to stdout and the
   process exits without opening GPUI. `--version`/`-V` is
   answered in place, before that connection: routing it to the daemon would spawn one just to
   report an unknown command.
6. **GUI mode** . no arguments: `run_app` connects (or spawns-and-connects)
   `zz_daemon::InteractiveClient`; its default attach lazily creates session `0` if that daemon is
   empty. It boots CEF (`zz_browser::bootstrap`), builds `MuxClient` +
   `BrowserController` + `AgentController`, opens the one native window (`AppShell` → `AppView`),
   restores its last usable bounds through `window/state.rs`, and wires window close / app-quit to
   both shutdown paths so CEF drains its message loop before the process exits. ACP sessions are not
   in that teardown any more . they belong to the daemon and keep running.

Windows has a fifth entrypoint: `RunWinMain`, an `extern "C"` export called by the bundled CEF
sandbox bootstrap executable (`zz.exe` in a shipped bundle is *not* this crate's binary; it is
CEF's bootstrap, which loads the `zz.dll` beside it . derived from its own file name . and calls
this function). It is called for the browser process *and* for every `--type=` subprocess the
bootstrap relaunches itself as, so renderer and GPU processes enter here too. The `sandbox_info`
pointer is created by the bootstrap and forwarded untouched into `execute_process`/`initialize`;
a client DLL must never mint its own. `run_startup` (diagnostics, `--socket`, the command verbs)
is shared with the unix `run`, and the `#[global_allocator]` `main.rs` declares on unix is declared
in `lib.rs` on Windows because the application runs from the library there. The Cargo-built
`zz.exe` keeps serving everything that needs no window . `--version`, `daemon`, mux verbs,
askpass . and only refuses to open the GUI, pointing at the bundle instead.

# Application configuration (`config/mod.rs`)

On GUI startup, the app loads the first bounded [`zz/config`](/configuration/app-config.md) file from
the platform's user configuration roots into GPUI globals. `ConfigKey` enumerates thirty-two
client-local knobs: fifteen switches (including `auto-restart-stale-daemon`), six lengths, four
enumerated selectors, the browser element-selector hotkey, and six `chrome-*` palette overrides.
`AppConfig` stays `Copy`;
the string-valued browser shortcut is published through a separate `BrowserConfig` global. One
app-owned Agent key remains . an optional absolute `agent-working-directory` . because the three
adapter keys (`agent-command`, `agent-claude-code-command`, `agent-auto-approve`) became mux options
when the ACP runtime moved into the daemon; the parser recognizes them and partitions them into the
daemon set. The file also retains ordered raw entries for the supported
daemon-owned appearance and mux surface. Local knobs carry `Default`/`Override` provenance. The
complete daemon-owned entry vector crosses the wire on connect and every poll change, including an
empty vector that restores donor/default values.

The settings view is a Root-managed zz-ui dialog opened with `Cmd+,` on macOS or `Ctrl+,`
elsewhere. `SettingsSection::ALL` is nine pages, titled Interface, Editor, Panes, Multiplexer,
Browser, Terminal, Hosts, System, About:

| Section | Contents |
|---------|----------|
| Interface | Theme (`theme-mode`, transient UI zoom, and macOS app-icon pickers), Chroma Colors (presets and `chrome-*` colors), Tweaks (`animations`, `widget-corner-radius`, window blur, and Linux `use-system-titlebar`) |
| Editor | Editor-pane typography and display controls, when compiled in |
| Panes | Layout (`pane-gaps`, `pane-margin`, `pane-corner-radius`), Frame (`pane-border-width`); gapped panes carry subtle inset surface rings |
| Multiplexer | Full-file `zz/mux.conf` editor without line numbers, 12px text, Save, and tmux import |
| Browser | Browser-local shortcuts, beginning with the element-selector hotkey |
| Terminal | Structured terminal appearance controls with effective values, provenance, palette swatches, and Reset |
| Hosts | Configured `host-<name>` machines, live connection state, Remove, and an inline Add host form |
| System | Daemon (`quit-daemon-on-exit`), Diagnostics (`show-fps`), Experimental pane gates |
| About | Version, build identity, and project links |

A **Devices** pairing page sat in this list until 2026-08-01 and was deleted with QUIC pairing.
Fleet hosts are plain `host-<name>` config lines, added from Settings › Hosts, the sidebar host
menu's **Add host** dialog, or `zz fleet add`, and removed by **Close host** or `zz fleet remove`.

The ordinary controls, including Terminal, retain the typed comment-preserving writer.
Multiplexer's code editor saves its bounded file atomically; a clean editor reloads on navigation,
and its tmux import is donor-specific. No GUI callback changes effective state directly: a
background task polls candidate paths every 500ms, swaps globals, sends daemon overrides, rebuilds
the current theme, and refreshes windows, so hand edits and settings edits share one live-update
path. Adding a host is the one write that does not wait for the poll: `config::add_fleet_host`
republishes the `FleetHosts` global itself, so the new row starts dialing immediately.

UI zoom is the deliberate transient exception to that file-backed settings model. Global actions in
`ui_scale.rs` change `Theme::font_size`, the root installs it as the window rem, and all windows are
refreshed. `Cmd/Ctrl +/-` adjusts it, `Cmd/Ctrl 0` resets it, and Appearance's live UI zoom row calls
the same functions while showing the current percentage. Rem-based text, named control metrics, and
sidebar glyphs scale together; browser page zoom and terminal font zoom remain pane-local.

`quit-daemon-on-exit` is the one knob that changes shutdown semantics. `cx.on_app_quit` normally
calls `MuxClient::detach()`, leaving the daemon alive for the next launch; with the knob on it sends
`kill-server` instead, stopping the daemon even while sessions are live. It defaults off.

The background-blur switch becomes a platform blur request in `window/background.rs`. Window
creation uses the resolved value, and every watcher update reapplies it to all open GPUI windows in
place. macOS keeps a transparent GPUI surface and installs an AppKit semantic material behind it;
Windows uses DWM's system backdrop; Wayland uses GPUI's standard `ext-background-effect-v1` path
when advertised and retains the legacy KDE protocol as a fallback. X11 publishes KDE's rounded
blur-behind region and refreshes it as window bounds change. The request is capability-gated:
unsupported compositors retain opaque app paint instead of exposing an unblurred desktop.
`background-opacity` remains terminal-local: the client paints the terminal color at that alpha over
an opaque app-pane base. `1` shows the terminal background, while lower values mix toward the app
surface without exposing compositor blur. Ghostty's `background-blur` is ignored; the app-level
switch above is the only native blur request. The pane picker, Agent, Editor, Browser shell, waiting
state, and terminal cover the native backdrop. Browser blank, loading, error, and toolbar states
share that surface; Chromium page frames retain their own pixels.

The Linux-only system-titlebar switch requests GPUI server-side decorations for new and open
workspace/Settings windows. KDE can honor it on Wayland and X11; unsupported Wayland compositors
negotiate back to the existing client frame. Decoration changes are live, and returning the frame
to desktop ownership clears GPUI's client-side shadow inset while retaining the app's
navigation/status strip.

Terminal is again a structured mirror of daemon-resolved appearance, with provenance-aware Reset
controls writing `zz/config`. Multiplexer remains a bounded `zz/mux.conf` editor and can replace it
verbatim from tmux after confirming that a dirty buffer may be discarded.

# Application chrome (`theme.rs`)

**App chrome does not derive from terminal colors.** Colors come from zz-ui's own
`ThemeColor::light()` / `ThemeColor::dark()` palettes, following the OS appearance unless
`theme-mode` pins System, Light, or Dark. A Ghostty palette cannot repaint the window.

`theme.rs` keeps the latest immutable `TerminalAppearance` and `AppearanceProvenance` as GPUI
globals for chrome crossovers and Terminal Settings badges, and owns the two entry points that
install a theme: `refresh_current_theme` (the terminal appearance or `zz/config` changed) and
`sync_system_appearance` (the OS appearance changed). Both restore the zz-ui base first, then call
`apply_zz_overrides`, so the overrides survive a mode switch that would otherwise reset the palette.
A pinned `theme-mode` wins over the installed mode in both paths, which is how flipping the pin takes
effect without a restart.

`apply_zz_overrides` writes, in this order:

- the six `chrome-*` palette roots (`ChromeColor::ALL`) that are set. Only the roots are written;
  every elevation, hover state, muted text, and focus ring derives from them at paint time, so
  recoloring reaches the whole UI with no per-component plumbing. `CHROME_PRESETS` ships ten
  ready-made mode + roots pairs;
- `mono_font_family` from the terminal's resolved primary family, so Agent Markdown, tool previews,
  and code blocks match the terminal typeface;
- `radius` from `widget-corner-radius`, which reaches every widget through the theme.

The font is the sole terminal→chrome crossover: a typeface, never chroma. Terminal alpha is resolved
at the terminal pane's own paint boundary.

# Native window chrome (`app_shell.rs` / `window/frame.rs`)

`AppShell` renders the workspace, dialog layer, and notification layer. The main window has **no
title-bar row**: the sidebar's titlebar-height strip is the window's title bar (it holds the macOS
traffic lights, the `WindowControlArea::Drag` region, and the double-click handler), so the
workspace reaches the window's top edge and the panes occupy that height. `WindowCorners` therefore
gives the workspace the whole right edge (`.right()`), not only the bottom-right corner. Platforms
that draw their own minimize/maximize/close buttons have nowhere left to put them, so where
`draws_window_controls` holds . Windows, and Linux under client-side decorations . `AppShell` gives
that height back at the top of the content column as `shell::app_titlebar_strip`: a real 34px flex
row filled with `theme::chrome_background`, so it and the sidebar's strip read as one bar across the
window. It carries `zz_ui::WindowControls` at its right, is a `WindowControlArea::Drag` region left
of them (the area sits beside the buttons, never around them, or it would answer their hit tests),
and owns the exposed top-right corner . `AppView` drops to `.right().bottom()` while it is mounted.
The gate is re-read every render, so toggling `use-system-titlebar` moves the corner and the height
on the spot. macOS draws no buttons, mounts no strip, and keeps the panes at the top edge. On Linux it mounts them
inside an app-owned client-side frame because the forked zz-ui frame fixes its corner
radius at zero. The frame uses a fixed 13.5px macOS 27-style window radius on every
exposed corner, retains the existing shadow, border, and resize hit zones, and squares any corner
touching a tiled edge; a fully maximized window is square. `lib.rs` disables `Root`'s built-in border
and makes its background transparent only on Linux so the rounded frame is the sole visible outer
surface. macOS and Windows keep their native window shaping. The Settings window still uses
`zz_ui::TitleBar` for its content column, with zz-ui's default bottom border disabled so the header
flows directly into the content.

The main window's inner bounds, display UUID, and windowed/maximized/full-screen mode are client-owned
state rather than configuration. `window/state.rs` loads the bounded, versioned
`<data dir>/zz/window-state.json` before window creation, constrains the restored size and origin to
the selected display's usable area, and centers it on the primary display when the recorded monitor
is gone. Bounds changes are written atomically after a 250ms debounce and flushed during both the
window-close and app-quit paths. Settings windows are intentionally excluded.

GPUI's `overflow_hidden` mask is rectangular, so square-edged descendants (scrollbars, the Chromium
texture) escape rounded corners. The frame therefore registers a scene-wide rounded clip via
`Window::set_window_corner_mask` (a zz-patches carried gpui patch): every primitive except drop
shadows is clipped to the frame's outer arc in the wgpu fragment shaders. Because gpui paints a
div's border under its children and clipped content would cover it at the corners, the frame's 1px
border is painted as a `border_ring` canvas over the children (padding, not a border, insets the
content). `window/corners.rs` derives the currently exposed corners from Linux tiling state
and propagates them through split layouts for the concentric inner radii. The shared 1px border
width derives a fixed 12.5px inner radius for title bar/sidebar/workspace/modal surfaces and flush,
borderless terminal/browser pane content (including Chromium images). These frame curves are
internal presentation geometry; only pane corner radius and margin remain configurable. Tiled
edges remain square at every layer.

On macOS, `macos_app.rs` registers application actions and a native menu before any pane receives
keyboard input. Global bindings cover `Cmd+Q` (quit), `Cmd+H` / `Option+Cmd+H` (hide), `Cmd+M`
(minimize), `Cmd+W` (close the active window), and `Control+Cmd+F` (full screen); Settings remains
`Cmd+,`. The focused terminal or browser only receives a command-modified key when no application or
window action claims it. Quit still runs the existing app-quit barrier, and closing the main window
   still runs both browser and agent shutdown, so both routes detach from the persistent daemon
   without stopping its PTYs. `AgentController::shutdown` is a formality now . it flips a flag and
   returns ready, because the adapters are the daemon's children and a running turn is meant to
   outlive the window.

# Layout reconciliation and pane identity (`workspace/view.rs`)

`AppView::synchronize_panes` does the reconciliation, once per render, before the widget tree is
built:

- Walks the attached session's active-window pane map and builds `wanted_pickers` /
  `wanted_terminals` / `wanted_browsers` / `wanted_agents` (`BTreeSet<PaneId>`) from
  `PaneKindSnapshot::{Picker, Terminal, Browser, Agent}`.
- For every wanted pane not yet present in `self.pickers` / `self.terminals` / `self.browsers` /
  `self.agents`,
  creates the
  entity exactly once. That keeps a terminal's scrollback, selection, and search state
  alive across re-renders and layout changes: **the entity is keyed by `PaneId`, never recreated
  while the pane exists.**
- `PanePickerView` owns only a GPUI focus handle and the local row selection. Arrow keys or
  `hjkl` cycle Terminal/Browser/Agent; Enter issues `select-pane-kind`, after which reconciliation
  drops the picker and focuses the newly materialized entity under the same `PaneId`. Plain
  `t`/`b`/`a` activate those choices directly, while Escape issues `kill-pane` for the still-empty
  pane. Clicking its background issues `select-pane` before taking GPUI focus, keeping the mux-active
  state and inactive surface treatment synchronized. The picker presents compact, headerless,
  full-width washed rows with visible key badges.
- A post-snapshot state with zero sessions and no daemon error renders `NewSessionView` instead of
  the connection placeholder. It is a which-key panel rather than a card: a right-aligned key column
  beside its labels, with the two rows that work from here . **New session** (Enter, and the row is
  also the click target, so one action has one affordance) and **Settings** . above a captioned
  group of prefix bindings that need a session first (`c`, `%`, `"`, `s`, `?`). The prefix chip is
  the daemon's published one through `mux::prefix::display_keystroke`, falling back to `C-b` before
  the first `ServerHello`; Enter issues `new-session`. This is the steady state of a zero-session daemon: the daemon exits only when it has
  no sessions *and* no interactive clients, so closing the last pane leaves it running and reveals
  the panel. A daemon advertising `new-session-attach-v1` performs the attachment as part of that
  command; for an older same-protocol persistent daemon, `MuxClient` follows it with `attach-session`
  on the same ordered connection. The current daemon satisfies an initial default attach by lazily
  creating the workspace; an attach miss from an older daemon or a race still requests a resync
  rather than becoming a connection error. A late request-zero `PaneExited` **or `PaneNotAttached`** response
  from terminal input or resize racing last-pane teardown is non-fatal, so the resulting empty
  snapshot still reveals the panel. Both spellings matter: a pane-only teardown answers `PaneExited`,
  while closing the *last* pane ends the session and detaches the client first, so the same late
  keystroke returns `PaneNotAttached` . and nothing would clear that error afterwards, because with
  no session left no command remains to succeed. Generation zero is the startup/connecting state, so the card does not flash before the first
  real daemon snapshot.
- Drains queued `TerminalUiCommand` / `BrowserCommand` per pane (`MuxClient::take_terminal_commands`
  / `take_browser_commands`) and applies them to the matching entity.
- Retains only wanted terminals; for browsers no longer wanted, tells `BrowserController::close_pane`
  before dropping the entity, so the CEF session closes with the GPUI view.
- Gives `AgentController` the Agent-pane set from every daemon session, not only the currently
  attached one, and asks the daemon for a replay when a pane goes live. Dropping an inactive
  `AgentView` costs nothing . the ACP session is the daemon's. Removing the real daemon pane is what
  closes the route and stops that pane's provider process.
- Computes browser visibility per frame (`BrowserView::set_visible`): a browser is only visible if
  its window is the active window, it isn't hidden by zoom, and it isn't covered by command-output,
  choose-tree, or choose-buffer overlays; invisible CEF sessions stop consuming paint bandwidth.
- Resolves focus precedence: command palette > choose-buffer > choose-tree > display-panes >
  command-output > the requested/focused workspace sidebar > the
  empty-workspace card > the window's `active_pane`, focusing the
  corresponding `FocusHandle` only when it changed.

`AppView::render_layout` recursively renders `zz_protocol::LayoutNode::{Pane, Split}` into a plain
`div()` tree. Flush layouts use draggable 1px split-owned separators; the active pane colors only
its half of each adjacent segment with a washed foreground accent, including the projected edge at
nested T-junctions. Gapped layouts use the configured border width around each pane, replacing the
active pane's neutral border color with the same wash. Their half-pixel surface ring matches the
Settings stack and paints before the pane background without a directional tail.
`PaneChrome::dimmed` still fades inactive panes behind a scrim
(`INACTIVE_PANE_FADE` = 0.3 toward the opaque window background).
The scrim sits above pane content and below overlays, so `SYNC`, the waiting placeholder, and the
`display-panes` card keep full contrast. Divider drags preview the ratio locally and commit
`InputMessage::ResizeSplit` only on mouse-up.
Zoomed panes short-circuit straight to `render_layout(&LayoutNode::Pane(zoomed))`, skipping the rest
of the tree. `AppView` owns the current `CommandPaletteView` and renders it as an absolute overlay;
the palette uses `zz_ui::input::Input`, catalog-driven completions, live mux targets, and daemon-
owned prompt state. See [the command palette concept](/concepts/command-palette.md).

# The daemon connection (`mux/client.rs`)

`MuxClient` is the single GPUI entity that owns the wire connection. On construction it spawns a
dedicated `zz-mux-reader` OS thread that blocks on `InteractiveClient::recv` and forwards decoded
`ProtocolMessage`s through a **bounded `async_channel` of depth 1** into a `cx.spawn` task that
calls `handle_message`. That depth-1 bound is deliberate backpressure: if the GPUI thread falls
behind, the reader thread stops draining the socket instead of buffering unbounded terminal frames
client-side (pressure then lands on the daemon's own per-pane mailbox).

`handle_message` keeps the terminal hot path outside the shared core: full viewports, row patches,
and command-output frames go straight into `RetainedTerminalViewport` and `CommandOutputModel`,
preserving the history ring, row revisions, and diff scratch the painter consumes. Every other
message goes through `zz_client::ClientCore`; `MuxClient` drains its `Outbound` requests and typed
`CoreEvent`s into GPUI revisions, queued browser/terminal commands, clipboard effects, and local UI
state. For a browser-supported URL, the source pane's
layout-tree path selects the topologically nearest browser in the same mux window (layout distance
and forward layout order break ties), then queues `BrowserCommand::Navigate` without changing pane
focus. Unsupported schemes and windows without a browser fall back to `cx.open_url`. It also owns a
client-local terminal font-size offset (`Ctrl+-`/`Ctrl+=`) applied on top of whatever
`TerminalAppearance` the daemon publishes. The default Control-minus/equal/plus chords resolve as
`TerminalFontDecrease`/`TerminalFontIncrease` through `ChromeKeymap`, so `chrome-keybind` and
`chrome-unbind` can replace them without changing terminal input code.

`MuxClient` also sends the `ServerHello` appearance to `theme.rs` at construction and repeats that
handoff for every `AppearanceChanged` event after preserving the local font-size offset. These
events cover explicit daemon reloads and `set_color_scheme` adaptive-theme round-trips. The handoff
shares an immutable copy for chrome derivation; it does not modify the appearance used by terminal
views.

## Client-side scrollback

Each `RetainedTerminalViewport` carries a `HistoryRing` of rows the daemon has already scrolled off
the live grid, so scrolling back on a remote host costs no round trip. Live patches push evicted rows
into the ring as the grid shifts; `HistoryRequest`/`HistoryChunk` backfill prepends older ones. Both
paths cap the ring at `MAX_HISTORY_ROWS` = 10,000 and one request stays in flight per pane, clamped to
`MAX_HISTORY_CHUNK_ROWS` = 512 rows. The `history-trickle` mux option sets the idle backfill budget;
`0` disables backfill, and a scroll past what the ring holds raises the budget to the cap and
prefetches ahead of the target offset.

Two counters keep the ring honest without a wire epoch. `history_mutations` bumps on every ring
mutation, and an arriving chunk applies only when the counter still matches the value snapshotted at
request time. An at-cap pane keeps constant scrollbar numbers while its content slides, so scrollbar
equality alone cannot catch a stale chunk on a lossy lane. `history_invalidations` bumps only when
the ring is dropped, which is what the local-scroll overlay retires on; retiring on ordinary live
pushes would kill local scrollback on exactly the streaming panes it exists for. A full replacement,
a column change, or a scrollbar shrink is the client-observed invalidation signal.

## Reconnecting to a remote host

An attached remote host that drops never falls back to the local daemon. It enters
`HostState::Reconnecting { attempt }`, keeps its last frames frozen on screen, and `MuxClient` retries
on a 1/2/4/8/16/30-second backoff, re-attaching to the same session on success. Every
`HostConnection` carries a `reconnect_generation` that each armed timer captures; a timer whose
generation or attempt no longer matches is a superseded attempt and returns without dialing, so a
fast reconnect racing a slow one cannot resurrect the loser. The loop starts after the ssh-forwarded
local socket connection dies.

# Terminal rendering (`terminal/view.rs`, `terminal/element.rs`)

`TerminalView` is the per-pane (or per-command-output) GPUI entity: focus handle, IME/marked-text
state, mouse selection and autoscroll, search-prompt state, scrollbar drag, and cursor-blink task.
It translates GPUI input events into `zz_protocol::InputMessage` variants
(`TerminalView`/`CommandOutputView` wrapping `TerminalViewAction`, `Key`, `ResizeTerminal`,
`ResizeCommandOutput`) and sends them through `MuxClient::send_input`; it does not interpret VT
state itself. It also owns a `TerminalRenderAppearance` bundle containing the current appearance,
derived GPUI `Font`, and stable appearance hash. The mux observer replaces that bundle only when the
appearance changes, so steady-state renders neither rebuild font allocations nor rehash the full palette;
the cached hash is passed into each fresh `TerminalElement`. Hovered-link presentation keeps one bounded
layout entry per terminal view, keyed by URI, surface size, appearance hash, and display scale. It reuses
the truncated `SharedString` and exact shaped popup size across pointer-only renders, while every geometry
or typography input change forces a fresh measurement.

Two latency features live on `TerminalView` and exist only to make a WAN-attached pane feel local.

**Local scroll.** When the pane is in Live mode, mouse tracking is off, the scrollbar has something to
scroll, and the pane's `HistoryRing` is non-empty, a wheel or scrollbar drag repaints straight from
the ring and records a `LocalScroll { target_offset, started }`. A `ScrollToOffset` for that target
goes out after a `LOCAL_SCROLL_DEBOUNCE` of 120ms, so a flick sends one message instead of dozens.
The overlay retires when the server's offset reaches the target, when the ring is invalidated (the
`history_invalidations` counter moved), or after `LOCAL_SCROLL_TIMEOUT` of 2s. Scrolling near the
front of the ring triggers a prefetch of the next chunk.

**No predictive echo.** `terminal/predict.rs` and its `Predictor` were deleted on 2026-08-01: the
overlay only ever rendered above a 40 ms transport RTT, which the deleted QUIC arm was the reason
for. Should ssh latency ever justify bringing it back, the RTT it gated on has to come from an
application-level ping . the client no longer has a transport that reports one.

`TerminalElement` is a custom `gpui::Element` (not an entity) instantiated fresh each render but reading
the same `Arc<RwLock<RetainedTerminalViewport>>` and a persistent `Rc<RefCell<RowRenderCache>>`
owned by `TerminalView`. Each `prepaint` it shapes only the rows whose revision changed
(`RowRenderCache::prepare` invalidates on a `RowCacheSignature` covering dictionary generation,
scale, font, cell width, and appearance hash), builds background/text/overline/underline/cursor/
selection/scrollbar paint batches, and reports the resolved grid geometry back to `TerminalView` via
`update_geometry` (which is also where resize messages are triggered on grid-size change). See
[terminal rendering parity](/terminal/rendering-parity.md) for the full typography/color/cursor
contract this element implements.

# Browser rendering (`browser/view.rs`, `browser/element.rs`, `browser/controller.rs`)

`BrowserController` is the single entity owning one `zz_browser::BrowserRuntime` and a
`BTreeMap<PaneId, BrowserSession>` plus a `BTreeMap<PaneId, BrowserPaneFrame>` **latest-frame
cache**: each pane only ever retains its newest OSR frame, decoded exactly once into a shared
`Arc<RenderImage>` that views clone without copying pixels. Pane focus also drives the per-session
OSR paint ceiling (`set_focus`): focused panes use the configured `ZZ_BROWSER_FPS` ceiling,
visible-but-unfocused panes are capped at 30 FPS (`UNFOCUSED_FRAME_RATE_CAP`), and hidden panes stop
painting through `was_hidden`. It also owns CEF's external
message-pump scheduling (`schedule_pump`/`needs_active_pump`, capped at a 16ms active interval),
queues browser-creation requests that arrive before the runtime reaches `RuntimePhase::Running`
(`pending_browsers`). Each pending request carries its canonical profile and page zoom; a named
profile starts a CEF request context lazily and stays queued until that context reports ready. The
controller also runs a graceful-then-forced shutdown state machine
(`GRACEFUL_CLOSE_TIMEOUT` then `FORCED_CLOSE_TIMEOUT`) driven from `cx.on_app_quit`.
Cookie imports and current-origin data clears are asynchronous controller operations; they keep the
external message pump active and participate in the same shutdown barrier until their CEF callbacks
settle.

A browser pane for a local session browses from the client's network. For an attached ssh host, the
managed forward opens both the daemon's `ssh -L` socket and an `ssh -D` SOCKS listener. The client
derives a local `<profile>@egress-<hash8>` CEF context and points it at that SOCKS port, so remote
hostnames and `localhost` resolve from the attached host. `browser-egress` enables this by default;
Windows keeps client-local egress because its bridged-pipe attach path exposes no loopback SOCKS
listener. See [remote browser egress](/designs/remote-browser-egress.md).

`BrowserView` is the per-pane entity: owns a `zz_ui::input::InputState` child entity for
the toolbar URL bar, the current `Viewport` (size/scale/screen offset sent to CEF), the current frame
as an `Arc<RenderImage>` built by copying `OsrFrame::bgra` into an `image::ImageBuffer` in
`consume_frame`, component-based back/forward/reload/element-picker controls, and an error/retry
overlay for CEF startup or renderer-crash failures. It forwards pointer/wheel/key events to
`BrowserController` as `zz_browser` types. Browser page keys and committed text use explicit
Browser-surface protocol messages that bypass the daemon's root key table and reach CEF through the
synchronized Terminal/Browser sink resolver. `AppView` persists main-frame address changes with `set-browser-url` and
forwards document-title changes through tmux-compatible `select-pane -T`; URL state and the live
pane label therefore cannot overwrite each other.

Click-to-focus crosses both browser surfaces. Page-content mouse input, browser toolbar and address
clicks, and the native blank/recent-pages surface all issue `select-pane` for the owning `PaneId`
before focusing or navigating. The Browser settings page exposes
`browser-element-selector-hotkey` (`Cmd+Shift+C` on macOS, `Ctrl+Shift+C` elsewhere); the binding
is Browser-context-only and is installed again whenever the watched `BrowserConfig` value changes.

The browser root owns a `Browser` key context. On macOS `Cmd+=`/`Cmd++`, `Cmd+-`, and `Cmd+0`
zoom/reset the page; Linux and Windows use the corresponding `Ctrl` chords. Those actions run before
raw browser-key forwarding. `BrowserView` retains the current Chrome-style zoom percentage across
session recreation, and `AppView` recreates only the affected session when its daemon-persisted
profile name changes.

The native `about:blank` surface suppresses CEF's blank frame while retaining the `BrowserElement`
geometry pass, then paints the first-run hint or up to eight recent URLs over the browser's opaque
pane base. Those URLs are one-line, 360px-wide washed rows matching the pane picker's 40px height,
12px text, 4px spacing, and configured widget radius; there is no enclosing card or heading.
Loading and error overlays keep that base, and navigating away adds the CEF frame above it. When
the pane becomes inactive, the shared `PaneChrome` scrim covers both toolbar and page content;
Browser does not carry a separate toolbar-only inactive layer.

Keyboard input has two client-side gates before daemon routing. The configured tmux prefix wins from
every focus context through the window-root claim. Focused surfaces then resolve their `ui`,
`sidebar`, `browser`, or `terminal` table through `zz_client::ChromeKeymap`; the skin applies the
returned `ChromeAction`, which may stay local or send a protocol command.

- Terminal keys and committed text resolve the daemon's root key table. Browser page input uses a
  separate surface route that skips root bindings and passes directly through the synchronized
  sink resolver.
- Local text widgets (Agent composer, address bar, sidebar fields) can't round-trip, so
  `mux/prefix.rs` installs one capture-phase interceptor at the workspace-window root. It claims
  exactly the canonical prefix chord (plus every key while the daemon reports the sequence armed
  via `EventPayload::PrefixArmed`) and forwards them as ordinary key input with the active pane as
  source. Platform (`cmd`) chords are never claimed; autorepeat of a held claimed key is swallowed
  so holding the prefix cannot spam `send-prefix`; releases of claimed presses are forwarded and
  stopped, keeping the daemon's swallowed-key pairing balanced and the widget release-free.
- `ServerHello.key_tables` and `KeyTablesChanged` give the client every daemon table for labels,
  hints, and help. Pane semantics still resolve on the daemon.
- `bind -n` root bindings fire from Terminal panes. Browser pages and local text widgets expose
  only the captured prefix/armed sequence to the key tables.

The rightmost three-dot menu opens URLs externally, copies/reloads the current URL, shows and changes
the zz-owned browser profile, exposes page zoom controls, toggles the element picker, imports
cookies and history from a chosen stable Google Chrome source profile, retains bounded Cookie-Editor
JSON/Netscape `cookies.txt` file import as a fallback, and clears current-origin site data behind a
destructive zz-ui confirmation. `zz-chrome-import/src/profiles.rs` discovers stable signed-in and
signed-out profiles in the background from Chrome's bounded `Local State` display metadata,
preserves Chrome's picker order, and supplies both the isolated-storage switcher and read-only import
source picker. Source selection does not switch the pane: cookies enter its current zz profile and
history enters the app-owned list. On macOS discovery uses `NSHomeDirectory()` so app-bundle launches
do not depend on a shell `HOME` value.

`zz-chrome-import/src/cookie.rs` snapshots the selected Chrome cookie database and WAL sidecars into a
bounded private temporary directory, reads every usable unpartitioned host, deduplicates the CEF
identity by name/domain/path with later expiry winning, unlocks encrypted values through macOS
Keychain or Linux Secret Service, and normalizes large profiles in bounded chunks before the public
CEF `CookieManager` write. `zz-chrome-import/src/history.rs` independently snapshots `History`, extracts
the newest 5,000 HTTP(S) rows, and `browser/recent_pages.rs` merges them by URL with the newer visit winning.
Neither path writes Chrome's profile or transfers passwords, autofill, cache, or local storage.
Import/clear completion is surfaced through the mounted notification layer, and successful cookie
imports reload the page.

`BrowserElement` is the browser's custom `gpui::Element`: it does nothing but call
`window.paint_image` with the `Arc<RenderImage>` handed to it and report its content bounds back to
`BrowserView::update_content_bounds` (which resizes the CEF viewport). See
[OSR rendering](/browser/osr-rendering.md) for the CEF-side half of this pipeline.

# Sidebar, tree, and native overlays

- `workspace/sidebar.rs` builds a `MuxTreeModel` (session → window → pane, pane order taken from
  each window's `LayoutNode`) from `MuxSnapshot`, renders it as a full-height indented
  `uniform_list` tree whose top strip **is** the window's title bar (traffic lights, drag region,
  double-click), and issues `CommandInvocation`s (`new-session`, `split-picker`,
  `kill-session`/`kill-window`/`kill-pane`) from hover-revealed row actions. Row labels
  ellipsis-truncate (`min_w_0` + `overflow_hidden` + `text_ellipsis`) inside a **reserved action
  gutter**: the trailing action strip is `flex_none` and merely `invisible()` until hover, so it
  always occupies its `WORKSPACE_TREE_ACTION_INSET` width and rows never reflow when actions appear.
  Both selection signals are the same fill at two strengths, applied in that order: the **keyboard
  cursor** takes a washed `background.raised(2)`, the **mux-active** row the solid one with
  medium weight. A row that is both reads active. The
  toggle collapses the sidebar to a parked rail rather than removing it, so the sidebar always owns
  the window's left edge, and on macOS the traffic lights that sit on it. One
  `WORKSPACE_SIDEBAR_COLLAPSED_WIDTH` = 68px (zz-ui `navigation.rs`) applies on every platform, and
  clearing those lights is what sets it: `config::titlebar_options` scales them to 12pt starting 8px
  in, so the cluster spans x ≈ 8..59 and 68px leaves it near-symmetric margins. Nothing else claims
  that strip. The rail renders the attached
  session's windows as tinted rounded groups holding their panes as square tabs (browser tab groups
  stood on end): the active window's group carries the accent wash, the active pane keeps the tree's
  accent fill, tooltips carry window and pane names, and clicking a tab selects that pane. It is
  pointer-only and offers no resize edge. While collapsed the expand toggle
  (`workspace_rail_toggle`) heads the rail's own column, since the strip above it is the window's
  title bar and is too narrow for the control cluster. Below the tree,
  `workspace_sidebar_status` renders the daemon-expanded [tmux status line](/tmux/status-line.md):
  `status-left` and `status-right` stacked as two ellipsized lines, empty halves dropped so `status
  off` costs no height, and the whole section hidden while collapsed. `MuxClient::status_revision`
  drives the repaint, because the status moves on `status-interval` rather than with the snapshot. A
  `FocusSidebar` control event expands a collapsed sidebar, selects/reveals the active pane, and transfers
  focus; arrows plus `hjkl` and `g/G` navigate the visibly selected row, while Enter or `q`/Escape
  returns focus to the active pane. Plain `r` opens the existing native rename prompt for a selected
  session/window; a selected pane targets its parent window. Pane rows follow live terminal OSC and
  browser document titles without changing explicit session/window names.
- `workspace/tree.rs` is a small, standalone `UniformListDecoration` (`WorkspaceIndentGuides`) that
  paints the nested vertical indent guides behind the sidebar's tree rows; it is presentation-only
  and has no mux awareness (algorithm adapted from Zed's project panel).
- `chooser/tree.rs`, `chooser/buffer.rs`, `pane/display.rs` are three daemon-driven
  native modals: each owns a `revision`-gated `synchronize(state, revision, cx)` that no-ops unless
  the daemon's revision counter advanced, a 1×1 focus-trap `EntityInputHandler` used only to receive
  committed IME text for incremental search, and a keystroke-to-`InputMessage` mapping
  (`ChooseTreeAction`/`ChooseBufferAction`/`DisplayPanesAction`). `AppView` renders whichever is
  active as an absolute overlay and steals focus from the underlying pane while it is open.
- `config/settings.rs` is the client-only zz-ui dialog over structured app and Terminal controls
  plus the full-file `zz/mux.conf` editor. It confirms tmux imports, saves mux config atomically,
  and relies on the existing poller to apply `zz/config` control changes.
- The browser toolbar composes zz-ui widgets directly: `InputState`/`Input` provide native
  selection, IME, clipboard, loading, and submission behavior; `Button`, `PopupMenuItem`,
  `AlertDialog`, `Separator`, `Tag`, and themed icons provide the surrounding chrome and
  recovery/status surfaces.

# Diagnostics (`diagnostics/mod.rs`)

Process-wide `--verbose`/`--zz-verbose-log` wiring shared by every mode above: `env_logger` init
with per-target filters (terse `NORMAL_FILTER` vs. exhaustive `VERBOSE_FILTER` covering
`zz`/`zz_browser`/`zz_daemon`/`zz_terminal`/`zz_mux`/`cef`/`gpui`/`wgpu`), a panic hook that logs a
captured backtrace before delegating to the previous hook, a background process/process-tree
resource sampler (`sysinfo`, every 2s), and a periodic app-state sampler that calls
`log_diagnostic_snapshot` on both `MuxClient` and `BrowserController` (every 5s, plus on
`"startup"`/`"shutdown"`). `process_role()` labels every log line by process kind
(`app`/`daemon`/`command`/`cef-<type>`) so a single shared verbose log file can be filtered by
process across the whole CEF process tree.

Without `--verbose`, the long-running roles (`app`, `daemon`) still log: `init_production` sends
`NORMAL_FILTER` records through a `RingLogWriter` . `zz.<role>.log` in the platform log dir,
rotated by rename to one `.log.old` generation at 8 MiB, every record written straight through so
the tail survives a native crash (the motivating case: a CEF segfault after hours of uptime, where
stderr from a Finder launch had discarded everything). The panic hook and the `process_start` line
are installed on this path too; the samplers stay verbose-only. `cef_log_file()` names `cef.log`
in the same directory (renaming the previous session's aside, since CEF truncates on startup) and
`finish_bootstrap` hands it to `BrowserRuntime::set_log_file`, so CEF's own `WARNING`-severity
log . subprocesses included . lands beside the app's instead of on stderr. Command verbs keep
plain stderr logging.

Four always-on incident hooks ride that production log. `DebugMark` (`cmd-shift-m` /
`ctrl-shift-m`; a direct gpui binding on purpose, since the mux chord path is one of the things a
marker may be flagging) stamps `user_marker seq=N` plus a full `log_diagnostic_snapshot` pass into
the app log, forwards the `debug-marker` daemon verb so both ring logs carry the flag, and
confirms with a toast; `zz debug-marker [NOTE]` does the same from any shell or agent. The
main-thread stall watchdog (`start_main_thread_watchdog`) pairs a 100 ms foreground-task heartbeat
with a checker thread and logs `main_thread_stall` at onset and `main_thread_stall_recovered
duration_us=…` at recovery past a 500 ms threshold . a frozen main thread cannot log anything
itself. And the prefix-chord path logs each hop at info (`zz::diagnostics::input` /
`zz_daemon::diagnostics::input`): the client's claim/forward in `intercept_keystroke` (with the
no-active-pane drop at warn), both ends of the `PrefixArmed` handshake, and the daemon's
`key_decision` for every non-`Pass` key . ordinary typing stays unlogged so the persistent log
never becomes a keylogger. And `capture_stall_sample` (macOS only) shells out to `/usr/bin/sample`
for two seconds of all-thread stacks whenever the watchdog detects a stall (synchronously, while
the stall is still live) or a marker fires (on a throwaway thread, so the marker stays instant) .
the one question the other hooks cannot answer is what the frozen main thread was *doing*, and
the OS profiler can. Captures land beside the ring logs as `zz.stall-<unix-seconds>.sample.txt`,
newest 8 kept, one per 60 s cooldown; attachment works because the locally installed bundle is
signed without hardened runtime.

The default-off on-screen FPS diagnostics are separate from verbose logging. `diagnostics/fps.rs`'s
`AppFpsMeter` samples GPUI `FrameTiming` records for the main window, while each `BrowserView` owns a
`FrameRateSampler` counting fresh CEF OSR frames it consumes. One watched key, `show-fps`, enables
both badges; they remain two independent pipelines measuring different things and can disagree
without either being wrong, and neither replaces CEF's own frame-rate ceilings.

# Helper binaries (`src/bin/`)

| Binary | Role |
|--------|------|
| `zz_helper` | The CEF **subprocess entrypoint** on every desktop bundle. One line: calls `zz_browser::run_subprocess()` and exits with its code. CEF's multi-process model needs a plain, minimal executable to re-exec as renderer/GPU/zygote/utility processes; `Cargo.toml`'s `[package.metadata.cef.bundle] helper_name = "zz_helper"` tells `zz-xtask` to ship it. macOS nests copies of it into the Helper.app roles. (On Linux `zz` itself can also serve this role via `--type=`, but the bundle uses the dedicated helper for the renderer sandbox.) |
| `zz_cli` | The **`PATH` launcher**, shipped in the macOS bundle as `Contents/MacOS/cli` and symlinked onto `PATH` as `zz` by the Homebrew cask. It canonicalizes its own path and execs the `zz` beside it. Necessary because macOS resolves an app bundle from the launch path without following symlinks: symlinking the real executable would start zz with no `Info.plist` (no bundle identifier, no icon, no camera/microphone usage descriptions), and `current_exe` would additionally point the CEF framework lookup at the symlink's directory. See [the bundle playbook](/playbooks/build-cef-bundle.md). |
| `zz_browser_fixture` | A deterministic, loopback-only plain-TCP HTTP server (default port 9324, no CEF/GPUI/external network) used as a manual browser smoke-test target. Serves a fixture page with a BGRA color-channel proof, text input, title mutation, same-session navigation, a long scroll area, and persistent cookie/`localStorage` counters. |

# Key files

| File | Role |
|------|------|
| `crates/zz/src/main.rs` | Process entrypoint; calls `zz::run()` |
| `crates/zz/src/lib.rs` | `zz::run()` . CEF-subprocess/daemon/CLI/GUI mode dispatch, window creation, app-quit/window-close shutdown wiring |
| `crates/zz/src/fleet.rs` | `zz fleet add <name> <ssh-destination>` / `list` / `remove` . each one config-file edit, sharing `Endpoint::parse` and `config::validate_fleet_host` with the GUI dialog |
| `crates/zz/src/macos_app.rs` | macOS application/window actions, native app menu, and standard command-key bindings |
| `crates/zz/src/config/mod.rs` | Platform-aware bounded `zz/config` discovery/parsing, `host-<name>` fleet entries and their add/remove/republish helpers, the client-side ACP working directory, ordered daemon overrides (the three agent adapter keys among them), per-knob provenance, and comment-preserving atomic edits |
| `crates/zz/src/keymap.rs` | Bridges `ChromeKeymap` defaults and `chrome-keybind`/`chrome-unbind` overrides into GPUI bindings and action resolution |
| `crates/zz/src/config/settings.rs` | Root-managed native settings dialog with local and Terminal appearance controls, plus a compact full-file `zz/mux.conf` editor, explicit Save, and confirmed tmux import |
| `crates/zz/src/app_shell.rs` | `AppShell` . root application surface: floating window controls (non-macOS), dialog/notification layers, hosts `AppView` and the Linux client frame |
| `crates/zz/src/theme.rs` | Centralized theme customization over zz-ui's own light/dark palettes: retains the daemon appearance, applies the terminal `mono_font_family` crossover, and owns the translucent chrome plus opaque app-pane fills |
| `crates/zz/src/window/frame.rs` | Linux client-side shadow, fixed derived corner geometry, border, and resize hit zones |
| `crates/zz/src/window/corners.rs` | Exposed-corner geometry and split propagation shared by window surfaces |
| `crates/zz/src/window/state.rs` | Versioned main-window bounds persistence with display-aware validation, clamping, debounce, and atomic writes |
| `crates/zz/src/pane/layout.rs` | Normalized pane geometry for tmux-style active separator segments, including T-junction adjacency and live drag overrides |
| `crates/zz/src/workspace/view.rs` | `AppView` . pane reconciliation, recursive layout rendering, split-drag resize, pane indicators, zoom, browser metadata forwarding, and overlay hosting |
| `crates/zz/src/pane/picker.rs` | Headerless row-style Terminal/Browser/Agent pane picker with direct key badges, arrow/Vim navigation, click/Enter activation, and Escape close |
| `crates/zz/src/agent/controller.rs` | The client half of an Agent pane: the stream reducer over `zz_daemon::AgentStreamPayload`, per-pane replay cursors, capability-gated history listing, transactional load/new switching, close/delete, command/config discovery, sticky-setting reconciliation, and every user gesture turned into an `AgentRequest` on the wire |
| `crates/zz/src/agent/preferences.rs` | Bounded versioned client-owned store for provider/agent/workspace-scoped model, effort, and permission selections |
| `crates/zz/src/agent/view.rs` | Stable native Agent pane entity with provider and searchable virtualized history controls, live timeline, slash completion, sticky dynamic permission/model/effort controls, polished auto-growing composer, cancel, approval/auth actions, errors, and session status |
| `crates/zz/src/workspace/new_session.rs` | Zero-session which-key panel: the New session and Settings rows that work from an empty workspace, then the prefix bindings that need a session, with click and Enter activation |
| `crates/zz/src/mux/client.rs` | `MuxClient` . host-keyed `HostConnection` map (client `Arc`, reader thread + depth-1 channel, per-host state/snapshot), delegates non-frame protocol reduction to `ClientCore`, intercepts the retained terminal hot path, drains core effects into GPUI state, and owns `HistoryRing`, remote connect machinery, cross-host attach, and generation-guarded reconnect backoff |
| `crates/zz/src/mux/hosts.rs` | `HostId`/`HostRegistry`/`HostState` (including `Reconnecting { attempt }`) . fleet host identity from `host-*` config entries, `local` pinned at id 0, retained-host range for a removed-but-attached host |
| `crates/zz/src/mux/prefix.rs` | Window-root prefix claim, its display keystroke for key hints, and GPUI-to-terminal key translation shared by workspace and browser input |
| `crates/zz/src/terminal/view.rs` | `TerminalView` . per-pane terminal entity: input translation, selection, search, IME, cursor blink |
| `crates/zz/src/terminal/element.rs` | `TerminalElement` . custom `Element` painting the terminal grid; `RowRenderCache` shaped-row cache |
| `crates/zz/src/browser/view.rs` | `BrowserView` . per-pane browser entity: toolbar, address bar, CEF frame consumption, input translation, blank-page empty state |
| `crates/zz-chrome-import` | Google-Chrome profile/cookie/history import . extracted to its own crate; see [zz-chrome-import](/crates/zz-chrome-import.md) |
| `crates/zz/src/browser/recent_pages.rs` | App-owned browser history global . records live visits, merges Chrome imports by newest URL visit, persists `<data dir>/zz/browser/recent-pages`, feeds the browser empty state |
| `crates/zz/src/browser/element.rs` | `BrowserElement` . custom `Element` painting the CEF BGRA `RenderImage` |
| `crates/zz/src/browser/controller.rs` | `BrowserController` . owns `BrowserRuntime`, per-pane `BrowserSession`s, latest-frame mailbox, CEF pump scheduling, shutdown state machine |
| `crates/zz/src/workspace/sidebar.rs` | `WorkspaceSidebar` . one merged tree over every fleet host, each expanding into its own session/window/pane hierarchy; the collapsed rail, CRUD commands (local host only), and the per-host ellipsis menu (remote: Close host, New session; local: New session, Add host) |
| `crates/zz/src/workspace/add_host.rs` | The **Add host** dialog, opened from the local host row's menu: one `[user@]host[:port]` field, named after its host component, validated through `config::validate_fleet_host`, written and republished by `config::add_fleet_host` |
| `crates/zz/src/workspace/ssh_prompt.rs` | The dialog ssh's password and host-key questions appear in, opened from `MuxClient`'s `SshPromptRequest` event. Names the ssh destination, quotes ssh's own prompt, and answers a host key with exactly `yes`/`no`; dismissing it parks the host until Reconnect |
| `crates/zz/src/workspace/tree.rs` | `WorkspaceIndentGuides` . tree row indent-guide painting decoration |
| `crates/zz/src/chooser/tree.rs` | `ChooseTreeView` . native window/pane chooser overlay |
| `crates/zz/src/chooser/buffer.rs` | `ChooseBufferView` . native paste-buffer chooser overlay |
| `crates/zz/src/pane/display.rs` | `DisplayPanesView` . native pane-numbering overlay input target |
| `crates/zz/src/command/palette.rs` | `CommandPaletteView` . native prompt input, catalog/history completions, and submission |
| `crates/zz/src/command/completion.rs` | Token-aware command, option, enum, history, and live-target completion ranking |
| `crates/zz/src/diagnostics/mod.rs` | `--verbose` logging, panic hook, process/app-state samplers, process-role classification |
| `crates/zz/src/diagnostics/fps.rs` | Live GPUI frame-timing sampler plus shared browser FPS sampling policy |
| `crates/zz/src/file_picker.rs` | Feature-gated fuzzy path picker shared by Agent and Editor panes |
| `crates/zz/src/user_data.rs` | Platform user-data location and user-only file/directory permission policy |
| `crates/zz/src/bin/zz_helper.rs` | CEF subprocess entrypoint binary |
| `crates/zz/src/bin/zz_cli.rs` | The `PATH` launcher bundled as `Contents/MacOS/cli`; execs the real executable so the bundle identity survives a symlink |
| `crates/zz/src/bin/zz_browser_fixture.rs` | Loopback HTTP fixture server for manual browser smoke tests |
| `crates/zz/build.rs` | Linux-only: adds `$ORIGIN` rpath so the bundled CEF runtime libraries are found next to the executable |
| `crates/zz/Cargo.toml` | Declares the `zz`/`zz_helper`/`zz_browser_fixture` binaries and the `cef.bundle.helper_name` metadata `zz-xtask` reads |

# Related

- [Mux state machine](/crates/zz-mux.md) . owns layout math, tmux command parsing/execution, and key
  tables this crate only renders and forwards input to.
- [Protocol](/crates/zz-protocol.md) . `MuxSnapshot`, `InputMessage`, `EventPayload`, and the IDs
  (`PaneId`, `WindowId`, `SessionId`, `SplitId`) this crate keys every entity by.
- [Shared client core](/crates/zz-client.md) . renderer-free protocol reduction and chrome actions
  consumed by this GPUI shell.
- [Browser core](/crates/zz-browser.md) . `BrowserRuntime`/`BrowserSession`/`OsrFrame` consumed by
  `browser/controller.rs`.
- [Terminal core](/crates/zz-terminal.md) . `TerminalViewport`/`TerminalAppearance`/`PackedCell`
  consumed by `terminal/view.rs`/`terminal/element.rs`.
- [Command palette](/concepts/command-palette.md) . the native command/value prompt and completion
  overlay rendered by `AppView`.
- [Split-pane layout](/concepts/split-pane-layout.md) . the `LayoutNode` tree `render_layout`
  recurses over.
- [Terminal rendering parity](/terminal/rendering-parity.md) . the typography/cursor/selection
  contract `terminal/element.rs` implements.
- [OSR rendering](/browser/osr-rendering.md) . the CEF off-screen-rendering pipeline
  `browser/element.rs`/`browser/view.rs` consume frames from.
- [Architecture overview](/architecture/overview.md) and
  [process model](/architecture/process-model.md) . where this crate's process sits relative to the
  daemon and CEF subprocesses.
