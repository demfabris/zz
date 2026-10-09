---
type: Design Plan
title: Floating panes . the popup becomes window-owned
description: Floating panes as real mux panes in a per-window layer beside the tiled tree, ordered by the existing z_order, with display-popup rebuilt as a modal floating pane the way tmux master 34cd5da4 does it; covers the 3.8 command surface, the appended v108 snapshot fields, sizing, zoom, hit-testing, what each client draws, and the float.core, float.clients and float.keys lanes with their acceptance clauses.
status: proposed
resource: crates/zz-mux/src/model.rs
tags:
- tmux
- floating-panes
- popup
- tmux-3.8
- mux
- design-plan
timestamp: 2026-10-09T00:00:00-03:00
---

# Decisions

Ruling: fabrico, 2026-10-09 ([research](/research/2026-10-09-tmux-compat-revisit.md)). Pointers are at
`13f8a5361`; re-locate by symbol.

1. A floating pane is an ordinary `Pane` in `Window.panes`, placed by a floating layer on the window,
   never by a leaf in `CellLayout`.
2. `display-popup` becomes `new-pane -O` with popup defaults, as in tmux master 34cd5da4. The
   per-client popup, its synthetic pane id and its overlay wire are deleted.
3. Commands follow the 3.8 tag. `display-popup` follows master, because 3.8's per-client popup is what
   this removes; 3.8's `-N` and `popup-*` options stay accepted.
4. Geometry is in window cells, owned by the daemon, the same on every client.
5. Wire: two appended snapshot fields under v108, no new messages. GUI move and resize are commands.
6. A window's tiled layer is never empty (zz divergence, open question 1).

# Model (zz-mux)

`Window` (`crates/zz-mux/src/model.rs:235`) gains:

```rust
pub struct FloatCell { pub xoff: i32, pub yoff: i32, pub sx: u16, pub sy: u16 }
pub struct Floating { pub cell: FloatCell, pub over_zoom: bool, pub home: Option<TileHome> }
pub struct TileHome { pub neighbour: PaneId, pub axis: Axis, pub before: bool }
pub struct Modal { pub pane: PaneId, pub last: PaneId, pub capture_keys: bool,
                   pub close_on_click: bool, pub close_on_cancel: bool }

floating: BTreeMap<PaneId, Floating>,
float_memory: BTreeMap<PaneId, FloatCell>,
modal: Option<Modal>,
cascade: (i32, i32),
```

`FloatCell` is tmux's `lc->g`: the content box, borders outside it, signed offsets because `-X`/`-Y`
reach `-sx` and nudges do not clamp. `float_memory` is `lc->fg`, the start geometry for a later
`break-pane -W`. `cascade` is `last_new_pane_x/y`. `home` is the neighbour and side `break-pane -W`
took the tile from; tiling back splits it there, or the last active tiled pane if it is gone.

Invariants: a pane is in exactly one of `layout` or `floating`; `z_order` (`model.rs:259`, tmux
`w->z_index`, front first) holds the modal, then floats front to back, then tiled panes; `modal.pane`
is floating and active; the tiled layer has a pane. A new or activated float goes to the front, right
behind any modal; tiling sends a pane to the tail (`window.c:834, 1173-1208`). `select_layout_string`
(`model.rs:1623`) rebuilds only the tiled tail (it overwrites the whole list at `:1667` today). Layout
walks stay tiled-only; `displayed_pane_cell` (`model.rs:346`) returns a float's cell, or the full
extent when it is zoomed.

# Commands (3.8 surface)

`new-pane` shares `split-window`'s parse and spawn path (`crates/zz-mux/src/command.rs:6936`) as
`cmd_split_window_exec` does, and leaves `UNIMPLEMENTED_TMUX_COMMANDS` (`catalog.rs:761`).

| Surface | Behaviour (3.8 source) |
|---|---|
| `new-pane` | Floating unless `-L` (then a tiled split; a float target answers `can't split a floating pane`). Size `w/2` x `h/4`; cascade from 4,2 by +4,+2, wrapping on overflow (`layout.c:1727-1831`). `-x/-y` outer size (minus 2 with borders), `-X/-Y` outer corner (plus 1), percents of the window. Per-pane `-B` lines, `-T` title, `-s/-S/-R` styles, `-k/-m` remain-on-exit; `-W` waits for exit (`MuxEffect::PaneWaitForExit`); `-E -I -c -e -d -P -F -Z` as split-window. |
| `new-pane -A` | Over zoom: visible above a zoomed pane, zoom kept. |
| `new-pane -O` | Modal, implies `-A`; `modal pane must be floating`, `window already has a modal pane`. With `-O`: `-K` every non-mouse key reaches the pane, prefix included; `-C` click outside kills it; `-D` Escape or C-c kills it. |
| `new-pane -M` | Sized by the invoking mouse drag (`cmd-split-window.c:356`); no event, no-op. |
| `break-pane -W -x -y -X -Y` | Floats the source in place, tile to its neighbour, front of z, active unless `-d`. `pane is modal`, `pane is already floating`, `can't float a pane while window is zoomed`, `failed to float pane: ...`. |
| `move-pane -P` | 13 placements (`top-left` ... `bottom-right-centre`, `centre`/`center`) and `front back forward backward forward-loop backward-loop` (`cmd-join-pane.c:82-190`) on `-t`; tiled target: `pane is not floating`. |
| `move-pane -X -Y`, `-U -D -L -R [n]`, `-z N` | Absolute corner; unclamped nudge (default 1); z among visible floats, 0 is front. |
| `join-pane`, plain `move-pane` | A float joined to itself tiles (prefix `@`, menu Tile). Floating source to tiled target joins normally; floating target: `size or position can't split a floating pane`; modal either side: `pane is modal`. |
| `swap-pane` | Swaps cells and z slots, so floating state swaps. `-U/-D` skip floats; on a float: `cannot swap up/down on floating pane`. |
| `resize-pane` on a float | `-x/-y` outer size; `-R/-D` grow right/bottom, `-L/-U` grow left/up and shift; never unzooms; `-Z` zooms the float. |
| `split-window` on a float | Adjacent float of equal size, both shrunk to fit the 3,1 / sx-3,sy-1 margins (`layout.c:1834-1961`). |
| `kill-pane`, exit | Cell removed. Killing an over-zoom float keeps the zoom, any other unzooms. A dead modal returns focus to `modal.last`. |
| `select-layout`, `next-layout` | Unzoom, arrange tiled panes, floats untouched. |
| Formats | `pane_floating_flag`, `pane_modal_flag`, `window_modal_pane`, `pane_z` (floats: floats in front; tiled: floats plus 1), pane flags `F A O`, window flag `O`. |
| `choose-tree -O z` | Panes sorted by `pane_z` (`TmuxSortOrder::Z`, `crates/zz-mux/src/sort.rs:14`). |
| `pane-border-status` | `top-floating`/`bottom-floating` are off for tiled panes, top/bottom for floats; floats read the pane value; `pane-border-lines none` turns it off (`pane-border-lines` needs 3.8's pane scope). |

**Last tiled pane (zz).** `break-pane -W` on the only tiled pane answers
`can't float the last tiled pane`. When the last tiled pane dies with floats alive, the front
non-modal float (else the modal, which stops being modal) is tiled into the whole window.

# display-popup becomes a modal pane

`display_popup` (`crates/zz-daemon/src/daemon.rs:18335`) builds a `new-pane -O` request. Placement
keeps `popup_position` (`:43372`), `parse_popup_dimension` (`:49960`) and the position variables
(`:43236`, `:43327`), measured against the target window instead of the client
(`popup_client_geometry`, `:43136`). Mapping, from master `cmd-display-menu.c:372-569`:

| Flag | Becomes |
|---|---|
| none | `remain-on-exit on`, close on cancel |
| `-E` / `-EE` | `remain-on-exit off` / `failed`, no close on cancel |
| `-k` | `remain-on-exit key` (`failed-key` with `-EE`, which arrives with pin.move) |
| always | capture keys, over zoom, `remain-on-exit-format ""` |
| `-B` / `-b` | pane `pane-border-lines none` / the value (`rounded` to `single`, `padded` to `spaces`) |
| `-s` / `-S` | pane `window-style` + `window-active-style` / `pane-border-style` + `pane-active-border-style`; absent, from 3.8's `popup-style`, `popup-border-style`, `popup-border-lines` |
| `-T` | pane title, pane `pane-border-status top`, pane `pane-border-format "#{pane_title}"` |
| `-w -h` | default half the window, percents of it, clamped; under 3x3 bordered (1x1 bare) does nothing |
| `-x -y` | same vocabulary (`C R P M L W` for x, `C P M L S W` for y) in window cells; y is the bottom edge |
| `-d -e -c -t` | start directory, environment, client (window, mouse, status), pane (window, `P`/`R`) |
| `-C` | kills the target window's modal, whoever made it |
| `-N` | accepted, no effect |
| modal present | silently ignored |

A command client blocks on the `-W` wait (`PopupWait`, `:37916`, folds into it) and exits with the
pane's status, 129 if it was killed.

Deleted: `Client.popup` (`:35190`), `PopupSession` (`:37987`), `PopupPlacement` (`:38019`), the
synthetic `PaneId(u64::MAX - token)` (`:18689`) and the sites matching it (`ctrl.rs:591, 1051`;
`daemon.rs:26567, 26940, 27343, 30508, 46546`; `watchers.rs:355-405, 797`), `popup_pointer` (`:21415`),
the popup menu and `popup_make_pane` (`:50054`, `:21769`), `refit_client_popup` (`:18919`),
`restyle_client_popup` (`:19032`), `watch_popup`, `finish_popup`, `retire_popup`, `take_popup`, and the
popup arm of `dismiss_overlays` (`:38192`).

For users: every client on the window sees and types into it (smaller clients clip it); it is a pane (`%N` in `list-panes`, hooks fire, `ZZ_PANE` inside names the popup, not the
target); detach, reattach and window switches leave it running; one per window, so a second
`display-popup` there does nothing; the prefix stays dead while its command runs, as in 3.8; the popup
right-click menu is gone, the pane menu's Tile and Move items replace it.

# Geometry, zoom, focus, mouse

**PTY size** is the float's `sx` x `sy`. `apply_terminal_size_report` (`daemon.rs:21237`) ignores
size reports for floats, so no client fights the daemon over them.

**Clamping** ports `layout_clamp_floating_panes` (`layout.c:772-812`): shrink to the window minus
border pads (minimum 1), then move left or up only to cure right or bottom overflow; negative offsets
stay, floats never grow back. It runs on every window extent change (`recalculate_window_extents`,
`daemon.rs:40501`).

**Zoom.** `zoomed_pane` may be a float: drawn over the whole window, cell kept. While zoomed, a float
shows only if zoomed or `over_zoom`. A new float without `-A`/`-O` unzooms.

**Hit order**, in the mux and every client (`window.c:848-927`): the modal only (outside hits nothing);
else visible floats front to back, border included unless `pane-border-lines none`; else tiled panes.
With a modal, focus moves to other panes are silent no-ops.

**Mouse** uses 3.8's bound commands, which the raw TUI already sends as `InputMessage::MouseKey`:
`resize-pane -M` on a float border (`cmd-resize-pane.c:231-354`: corners and side edges resize, the top
edge moves, the cell beside a corner is the corner), `move-pane -M` (pointer delta, unclamped),
`new-pane -M` (drag anchor to pointer). They extend `resize_pane_from_mouse` (`command.rs:8188`), which
already latches its target on button-down the way `popup_pointer` owns a drag until release.
`popup_pointer`'s own policy (left drag moves, right drag resizes) goes.

# Snapshot and wire (append under v108)

```rust
pub struct FloatingPaneSnapshot {
    pub pane: PaneId, pub xoff: i32, pub yoff: i32, pub sx: u16, pub sy: u16,
    pub visible: bool, pub border_lines: PaneBorderLines, pub border_status: PaneBorderStatus,
}
pub struct ModalPaneSnapshot {
    pub pane: PaneId, pub capture_keys: bool, pub close_on_click: bool, pub close_on_cancel: bool,
}
// appended to WindowSnapshot (snapshot.rs:543) after pane_z_order, #[serde(default)]
pub floating: Vec<FloatingPaneSnapshot>,   // front to back
pub modal: Option<ModalPaneSnapshot>,
```

`TreeOp::WindowLayout` (`tree_delta.rs:43`) appends the same two fields; diff (`:265`) and apply
(`:457`) carry them. `LayoutNode` stays tiled; floats sit in `panes`, so `retain_snapshot_panes`
(`crates/zz-client/src/core.rs:1381`) loses its popup case and nothing else. `EventPayload::Popup`,
`InputMessage::Popup` and their types (`message.rs:2252, 3117-3170, 3601`) keep their tags, are never
sent or read, and go at the next version. The v108 section of `knowledge/protocol/wire-protocol.md`
gains a paragraph.

# Layout strings (with pin.layout-v2)

v1 stays tiled only, as 3.8 dumps only the tiled copy. The v2 writer takes the tiled tree and the
float list and appends each float to the root's `c` array in z order as
`{"t":"p","w":sx,"h":sy,"x":xoff,"y":yoff,["a":true|"l":n,]"i":n,"z":z,"I":"%id"}`, wrapping a leaf
root in a `"v"` node first, as `layout_floating_pane` does. That is 3.8's output whenever the floats
came from the last tiled leaf (every single-pane window); otherwise 3.8 puts the leaf after its origin
cell, a difference recorded in `knowledge/tmux/divergences.md`. Zoomed, `#{window_visible_layout}`
holds the zoomed pane and visible floats. The parser lifts every `"z"` leaf into the float list and
collapses nodes left with one child; v1 input keeps existing floats.

# Clients

**Desktop** (`crates/zz/src/workspace/view.rs`). `render` (`:3656`) draws visible floats back to
front after `render_layout` and before `overlays`: an absolute `FloatingSurface`
(`crates/zz-ui/src/pane.rs:279`) around the pane's normal surface (terminal, browser, agent) at
`xoff * cell_w, yoff * cell_h` from the canvas origin with this client's cell metrics, clipped,
titled from `border_status_text`, borderless for `None`. Title-bar and edge drags preview locally and
on release send `move-pane -t %N -X x -Y y` or `resize-pane -t %N -x w -y h` (outer cells). A click
focuses it like a tiled pane, and the mux raises it. A modal adds an occluding scrim (a click sends `kill-pane` only with
`close_on_click`), and with `capture_keys` the chrome keymap forwards every key unresolved. Delete
`popup_overlay` (`:3433`), `PopupPane` (`:575`), `TerminalView::new_popup` and the popup handling in
`crates/zz/src/mux/client.rs:3992`.

**Web and iOS** (`clients/gpui-shared/src/app.rs`). The same, inside `workspace` (`:1514`), replacing
`terminal_overlay` (`:2687`). Compact iPhone pages floats like any pane (a page zooms its pane); the
modal and visible over-zoom floats draw over the page. Native browser views under a visible float
hide, as under popups today (`:2853`).

**Raw TUI** (`crates/zz-tui/src`). `recompute_layout` (`state.rs:1088`) appends float rects (border
and content, clipped) after the tiled ones; `paint_workspace` (`render.rs:730`) paints them back to
front after the tiled loop with `paint_popup`'s fill and `paint_floating_border` (`render.rs:1611,
2898`), status text on the border row from `xoff + 2`; `pane_at` (`state.rs:1005`) uses the hit order.
Kitty placements of a covered pane are withdrawn while covered. Delete the popup branch of
`input::handle` (`input.rs:199`) and `popup_pointer_action` (`:1851`).

# zz-only extensions

- `new-pane --kind terminal|browser|picker|agent [--profile] [--provider]`, inherited from
  `split-window` (`catalog.rs:1777`): floating browser and agent panes, drawn with their own surface.
- GUI drag and resize by title bar and edges, committed as commands (GUI clients load no tmux mouse
  tables). iPhone pages floats.

# Implementation split

float.core lands after pin.layout-v2; float.clients and float.keys then run in parallel.

**float.core.** Zones: `crates/zz-mux/src`, `crates/zz-protocol/src/{snapshot,tree_delta,message,catalog}.rs`,
`crates/zz-daemon/src`, `knowledge/protocol/wire-protocol.md`, `compat/scenarios`,
`compat/tmux-gaps.json`, `knowledge/tmux/divergences.md`. Clients compile unchanged; their popup code
goes dead until float.clients.

1. `compat/scenarios/floating-panes.txt` matches 3.8 under `--strict-geometry`, printing
   `list-panes -F '#{pane_id} #{pane_width}x#{pane_height} #{pane_x},#{pane_y} #{pane_z} #{pane_floating_flag}#{pane_active}#{pane_modal_flag} #{pane_flags}'`
   after each of: two `new-pane`, `break-pane -W -x 20 -y 6 -X 10 -Y 3`, `move-pane -P bottom-right`,
   `move-pane -L 2`, `move-pane -z 1`, `resize-pane -x 30 -y 8`, `split-window -h` on a float,
   `select-pane` of the back float, `swap-pane` float with tiled, self `join-pane`, `kill-pane` of a
   float, `resize-window -x 30 -y 10`.
2. Every error string in the commands table answers verbatim (scenario or zz-mux test).
3. Zoom: after `resize-pane -Z`, `new-pane -A` keeps `window_zoomed_flag` 1 with the float in
   `#{window_visible_layout}`; plain `new-pane` unzooms; killing the `-A` float keeps the zoom.
4. Modal: after `new-pane -O`, `select-pane` elsewhere keeps it active, `break-pane`, `join-pane` and
   `swap-pane` answer `pane is modal`, a second `-O` errors, killing it restores the previous pane;
   `window_modal_pane` and window flag `O` match 3.8.
5. `#{window_layout}` for one tiled pane plus two floats equals 3.8's string, and `select-layout` with
   it restores the floats.
6. Daemon tests: with two attached clients, `display-popup -E 'sleep 1'` puts one modal in both
   snapshots, sends no `EventPayload::Popup`, and closes on exit; from a command client
   `display-popup -E 'exit 3'` exits 3; `display-popup -C` kills a `new-pane -O` modal; a second
   `display-popup` leaves the pane count alone; no `u64::MAX` pane id remains in `crates/zz-daemon/src`.
7. A float's PTY is `sx` x `sy` after creation, resize and clamp, and a client size report for it
   changes nothing.
8. A round-trip test pins the appended fields (postcard and JSON); one `move-pane -X` emits exactly one
   `TreeOp::WindowLayout`.
9. The last-tiled-pane rule holds both ways; `choose-tree -O z` lists panes in `pane_z` order.
10. `pane.floating-model` turns from native to adopted and each item closes with the behaviour that
    proves it (`option:editor` in float.keys; `semantic:nested-attach-in-popup` keeps its fixture
    requirement); the two divergences are recorded; `just compat check` passes.

**float.clients.** Zones: `crates/zz/src/{workspace,mux/client.rs,terminal/view.rs}`,
`crates/zz-ui/src/pane.rs`, `clients/gpui-shared/src`, `crates/zz-client/src`, `crates/zz-tui/src`,
`compat/tui-floating.sh`.

1. `compat/tui-floating.sh` matches tmux 3.8 cell for cell on: two overlapping floats over a vertical
   split, before and after `select-pane` raises the back one; a `-T` float under
   `pane-border-status top-floating`; `pane-border-lines none`; a `display-popup` over a zoomed pane.
2. Same fixture: a click on the overlap activates the front float; a click outside a modal changes
   nothing, and kills a `new-pane -O -C` modal.
3. A browser pane covered by a float has no kitty placement while covered.
4. A unit test covers the shared cell-to-pixel rect helper (clipping included) and the drag commit
   commands; the lane report has desktop and web screenshots of a float, a modal and an over-zoom
   float.
5. A zz-client test: a focused modal with `capture_keys` sends the prefix key to the pane.
6. `rg 'PopupState|PopupAction|EventPayload::Popup'` finds nothing under `crates/zz`, `crates/zz-tui`,
   `crates/zz-client` or `clients/`.

**float.keys.** Zones: `crates/zz-protocol/src/key.rs`, `crates/zz-mux/src`, `crates/zz-daemon/src`
(`keys.rs:123`, mouse commands), `compat/scenarios/smoke`, `compat/tui-mouse.sh`,
`compat/tmux-gaps.json`.

1. `list-keys` answers 3.8's lines for prefix `*` `@` `g` `Tab` `BTab` and root `C-MouseDrag1Pane`,
   `C-MouseDrag1Empty`, `M-MouseDrag1Pane`, `M-MouseDrag1Border`, `MouseDown1Control7`; the `move`
   table keeps its 19 rows (`key.rs:178-430`).
2. Attached TUI: prefix `*` creates a float, prefix `g 1` moves it top-left, prefix `@` floats and
   tiles, pane menu `f`/`t`/Move work, geometry equal to tmux.
3. `compat/tui-mouse.sh`: C-drag creates, M-drag moves, border drag resizes, top-border drag moves,
   matching tmux; `flag:move-pane:-M` closes in `mouse.bound-context`.
4. With `editor` a script appending a line, choose-buffer `e` opens a modal at 90% centred and an exit
   0 updates `show-buffer`; non-zero leaves it; customize-mode `e` edits a string option the same way.
5. `keys.move-table` closes, `keys.default-prefix` drops `*` `@` `g` `Tab` `BTab`, `option:editor`
   closes.

# Open questions for fabrico

1. Allow a window with no tiled panes, as 3.8 does? Refusing keeps `LayoutNode` and three clients
   free of an empty-layout case; allowing it costs that variant everywhere.
2. Accept the v2 leaf position difference, or have float.core remember origin cells so dumps match
   3.8 in every case?
