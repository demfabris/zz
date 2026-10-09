---
type: Design Plan
title: Floating panes . the popup becomes window-owned
description: Floating panes as flagged leaves of the window's layout tree, exactly where tmux 3.8 keeps them, with display-popup rebuilt as a modal floating pane the way tmux master 34cd5da4 does it; covers the 3.8 command surface including windows with no tiled pane, the appended v108 wire, sizing, zoom, hit-testing, mouse drags, what each client draws, and the float.core, float.clients and float.keys lanes with their acceptance clauses.
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

Ruling: fabrico, 2026-10-09 ([research](/research/2026-10-09-tmux-compat-revisit.md)); review rulings
by the orchestrator the same day: match 3.8, invent no zz rules. Pointers are at `13f8a5361`;
re-locate by symbol.

1. A floating pane is an ordinary `Pane` whose leaf in the window's `CellLayout` is flagged floating,
   in the tree position 3.8 gives it (`LAYOUT_CELL_FLOATING`). Tiled arithmetic skips those leaves,
   so the tree remembers what 3.8 remembers: where a float sits, where it returns when tiled, and its
   saved geometry.
2. A window may have no tiled pane, as in 3.8.
3. `display-popup` becomes `new-pane -O` with popup defaults, as in tmux master 34cd5da4. The
   per-client popup, its synthetic pane id and its overlay wire are deleted.
4. Commands follow the 3.8 tag. `display-popup` follows master, because 3.8's per-client popup is what
   this removes; 3.8's `-N` and `popup-*` options stay accepted.
5. Geometry is in window cells, owned by the daemon, the same on every client.
6. Wire, appended under v108: two snapshot fields, `LayoutNode::Empty`, and the press cell on
   `MouseKey`. GUI move and resize are ordinary commands.

# Model (zz-mux)

`CellNode` (`crates/zz-mux/src/layout.rs:25`) gains a floating leaf, and tiled leaves keep tmux's
saved floating geometry:

```rust
pub struct FloatCell { pub xoff: i32, pub yoff: i32, pub sx: u16, pub sy: u16 }
enum CellNode {
    Leaf { pane: PaneId, geometry: CellGeometry, saved: Option<FloatCell> },  // lc->fg
    Float { pane: PaneId, cell: FloatCell, saved: Option<FloatCell> },        // lc->g, flagged
    Node { axis: Axis, geometry: CellGeometry, children: Vec<CellChild> },
}
```

`FloatCell` is the content box, borders outside it; offsets are signed because `-X`/`-Y` reach `-sx`
and nudges do not clamp. `Window` (`model.rs:235`) gains `modal: Option<Modal { pane, last,
capture_keys, close_on_click, close_on_cancel }>`, `cascade` (`last_new_pane_x/y`), `was_zoomed`, and
an explicit extent (`w->sx/sy`), because `layout.extent()` reads the root, which can now be a float.
`Pane` gains `over_zoom` (`PANE_FLOATOVERZOOM`).

Port these 3.8 functions as written, with their tiled tests (`layout_cell_is_tiled`,
`layout_cell_has_tiled_child`, `layout.c:237-332`) applied in every existing walk: `layout_floating_pane`
(insert after the target's cell, wrapping a root leaf in a top-bottom node, `:1503`),
`layout_remove_tile` (give the space to the nearest tiled neighbour cell, zero the geometry, keep the
position, `:1963`), `layout_insert_tile` (take half of the nearest neighbour cell, a whole subtree if
that is what sits beside it; with none, tile the parent first; at the root, the whole window, `:2003`),
`layout_destroy_cell` (a float leaves with no redistribution; a parent left with one child collapses,
so the root can become a float, `:694`), `layout_cell_get_neighbour` (`:673`),
`layout_split_floating_cell`, `layout_clamp_floating_panes`, `layout_resize_floating_pane(_to)`, and
`layout_set_link_floating` for presets. `project()` (`layout.rs:640`) drops floats and collapses
one-child nodes.

`z_order` (`model.rs:259`, tmux `w->z_index`, front first) holds the modal, then floats, then tiled
panes. A new float goes to the front, right behind any modal; activating or floating a pane raises it;
tiling sends it to the tail (`window.c:834, 1173-1208`). Zoom moves a zoomed float to the tail and
unzoom puts it back (`window.c:970-1079`). `select_layout_string` (`model.rs:1623`) rebuilds `z_order`
from the parsed `"z"` values instead of the tree order (`:1667`).

# Commands (3.8 surface)

`new-pane` shares `split-window`'s parse and spawn path (`crates/zz-mux/src/command.rs:6936`) as
`cmd_split_window_exec` does, and leaves `UNIMPLEMENTED_TMUX_COMMANDS` (`catalog.rs:761`).

| Surface | Behaviour (3.8 source) |
|---|---|
| `new-pane` | Floating unless `-L` (then a tiled split; a float target answers `can't split a floating pane`). Size `w/2` x `h/4`; cascade from 4,2 by +4,+2, wrapping on overflow, reset when the window has no float (`layout.c:1727-1831`). `-x/-y` outer size (minus 2 with borders), `-X/-Y` outer corner (plus 1), percents of the window. Per-pane `-B` lines, `-T` title, `-s/-S/-R` styles, `-k/-m` remain-on-exit; `-W` waits for exit (`MuxEffect::PaneWaitForExit`); `-E -I -c -e -d -P -F -Z` as split-window. |
| `new-pane -A` | Over zoom: visible above a zoomed pane, zoom kept. |
| `new-pane -O` | Modal, implies `-A`; `modal pane must be floating`, `window already has a modal pane`. With `-O`: `-K` every non-mouse key reaches the pane, prefix included; `-C` click outside kills it; `-D` Escape or C-c kills it. |
| `new-pane -M` | Sized by the invoking mouse drag (`cmd-split-window.c:356`); no event, no-op. |
| `break-pane -W -x -y -X -Y` | `layout_remove_tile` in place, geometry from `saved` and the flags, front of z, active unless `-d`. `pane is modal`, `pane is already floating`, `can't float a pane while window is zoomed`, `failed to float pane: ...`. |
| `move-pane -P` | 13 placements (`top-left` ... `bottom-right-centre`, `centre`/`center`) and `front back forward backward forward-loop backward-loop` (`cmd-join-pane.c:82-190`) on `-t`; tiled target: `pane is not floating`. |
| `move-pane -X -Y`, `-U -D -L -R [n]`, `-z N` | Absolute corner; unclamped nudge (default 1); z among visible floats, 0 is front. |
| `join-pane`, plain `move-pane` | A float joined to itself is tiled by `layout_insert_tile` and saves its cell in `saved` (prefix `@`, menu Tile). Floating source to tiled target joins normally; floating target: `size or position can't split a floating pane`; modal either side: `pane is modal`. |
| `swap-pane` | Swaps cells and z slots, so floating state and `saved` swap. `-U/-D` skip floats; on a float: `cannot swap up/down on floating pane`. |
| `resize-pane` on a float | `-x/-y` outer size; `-R/-D` grow right/bottom, `-L/-U` grow left/up and shift; never unzooms; `-Z` zooms the float. |
| `split-window` on a float | Adjacent float of equal size, both shrunk to fit the 3,1 / sx-3,sy-1 margins (`layout.c:1834-1961`). |
| `kill-pane`, exit | `layout_destroy_cell`; zoom follows `server_destroy_pane` (`push_zoom(w, 0, over_zoom)`, so an over-zoom float keeps the zoom). |
| `select-layout`, `next-layout` | Unzoom; presets need two tiled panes; floats re-link under the new root (`layout-set.c:139-190`). |
| Formats | `pane_floating_flag`, `pane_modal_flag`, `window_modal_pane`, `pane_z` (floats: floats in front; tiled: floats plus 1), pane flags `F A O`, window flag `O`. |
| `choose-tree -O z` | Panes sorted by `pane_z` (`TmuxSortOrder::Z`, `crates/zz-mux/src/sort.rs:14`). |
| `pane-border-status` | `top-floating`/`bottom-floating` are off for tiled panes, top/bottom for floats; floats read the pane value; `pane-border-lines none` turns it off (`pane-border-lines` needs 3.8's pane scope). |

**Modal close.** When the modal goes, the active pane becomes `modal.last` if it is still in the
window, else the head of `last_panes`, else the previous pane in pane order, else the next; killing or
moving `modal.last` away clears it first (`window_lost_pane`, `window.c:1185-1213`).

**No tiled pane.** A window keeps living while any pane does (`window_count_panes(w, 1)` counts
floats). It loses its last tiled pane through `break-pane -W` of it, its kill or exit, or a plain
`break-pane`, `join-pane` or `move-pane` that takes it to another window; the root is then a float or
a node with no tiled descendant. In that state the active pane is a float, presets do nothing,
`split-window` on a float makes a float, `new-pane -L` and joins onto a float answer
`can't split a floating pane`, and `resize-pane -Z` works with two panes. A float joined to itself
takes the whole window (`layout_insert_tile` at the root), which is also how the 3.8 empty-space menu
item `{ new-pane; join-pane }` works.

# display-popup becomes a modal pane

`display_popup` (`crates/zz-daemon/src/daemon.rs:18335`) builds a `new-pane -O` request. Placement
keeps `popup_position` (`:43372`), `parse_popup_dimension` (`:49960`) and the position variables
(`:43236`, `:43327`), measured against the target window instead of the client
(`popup_client_geometry`, `:43136`). Mapping, from master `cmd-display-menu.c:372-569`:

| Flag | Becomes |
|---|---|
| none | `remain-on-exit on`, close on cancel |
| `-E` / `-EE` | `remain-on-exit off` / `failed`, no close on cancel |
| `-k` | `remain-on-exit key` (`failed-key` with `-EE`; remain-on-exit takes failed-key since pin.formats-options) |
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

A single empty command argument (`display-popup ""`) is dropped before the request is built, as
master does (`cmd-display-menu.c:490-491`), so the pane runs `default-command` or the shell; passed
through, `new-pane` would read it as an empty pane with no process.

A command client blocks on the `-W` wait (`PopupWait`, `:37916`, folds into it) and exits with the
pane's status, 129 if it was killed.

Deleted: `Client.popup` (`:35190`), `PopupSession` (`:37987`), `PopupPlacement` (`:38019`), the
synthetic `PaneId(u64::MAX - token)` (`:18689`) and the sites matching it (`ctrl.rs:591, 1051`;
`daemon.rs:26567, 26940, 27343, 30508, 46546`; `watchers.rs:355-405, 797`), `popup_pointer`'s
popup policy (`:21415`), the popup menu and `popup_make_pane` (`:50054`, `:21769`),
`refit_client_popup` (`:18919`), `restyle_client_popup` (`:19032`), `watch_popup`, `finish_popup`,
`retire_popup`, `take_popup`, and the popup arm of `dismiss_overlays` (`:38192`).

For users: every client on the window sees and types into it (smaller clients clip it); it is a pane
(`%N` in `list-panes`, hooks fire, `ZZ_PANE` inside names the popup, not the target); detach, reattach
and window switches leave it running; one per window, so a second `display-popup` there does nothing;
the prefix stays dead while its command runs, as in 3.8; the popup right-click menu is gone, the pane
menu's Tile and Move items replace it.

# Geometry, zoom, focus, mouse

**PTY size** is the pane's allocation, as `layout_fix_panes` (`layout.c:420-485`) gives it: a float's
`sx` x `sy`; the zoom target, float or not, the whole window under the same pane-status rule as a
zoomed tile (a 20x6 float zoomed in 80x24 runs at the window size and returns to 20x6 on unzoom); a
float hidden by zoom keeps its last size. `apply_terminal_size_report` (`daemon.rs:21237`) ignores
size reports for floats.

**Clamping** ports `layout_clamp_floating_panes` (`layout.c:772-812`): shrink to the window minus
border pads (minimum 1), then move left or up only to cure right or bottom overflow; negative offsets
stay, floats never grow back. It runs on every window extent change (`recalculate_window_extents`,
`daemon.rs:40501`).

**Zoom** ports `window_zoom`, `window_unzoom`, `window_push_zoom` and `window_pop_zoom`
(`window.c:970-1145`) onto `zoomed_pane` and `was_zoomed`. Zoom needs two panes, floats counted; the
target may be a float; an active over-zoom float stays active; while zoomed only the target and
over-zoom floats show. A new float follows `layout_get_floating_cell` (`layout.c:1716-1721`):
with `-A`/`-O` the zoom is kept; without them, if the active pane is an over-zoom float the window
stays zoomed and `window_pop_zoom` zooms the new active pane (it zooms the active pane unless that is
an over-zoom float); otherwise the window unzooms.

**Hit order**, in the mux and every client (`window.c:848-927`): the modal only (outside hits nothing);
else visible floats front to back, border included unless `pane-border-lines none`; else tiled panes;
else the empty location. With a modal, focus moves to other panes are silent no-ops.

**Mouse** uses 3.8's bound commands, which the raw TUI sends as `InputMessage::MouseKey`:
`resize-pane -M` on a float border (`cmd-resize-pane.c:231-354`: corners and side edges resize, the top
edge moves, the cell beside a corner is the corner), `move-pane -M` (pointer delta, unclamped) and
`new-pane -M` (press cell to pointer). The anchor is the press: 3.8 starts a drag at `m->lx/ly`
(`server-client.c:912-915`) and keeps it in `mouse_drag_x/y`. `MouseKey` carries only the current cell,
and its `press_action` needs a pane (`crates/zz-tui/src/input.rs:1408-1433`), so a drag starting on
empty space has no anchor today; `MouseKey` appends `press: Option<(u16, u16)>`, the client cell of the
button-down that latched the gesture (`latch.press`), sent on every drag and release report and
translated to window cells like `column`/`row`. These commands then arm a per-client drag, the
`c->tty.mouse_drag_update` of 3.8: later reports of the same gesture go to it, not to the key tables,
until release. That drag state is `popup_pointer`'s `PopupPointerState` (`daemon.rs:38035`),
generalised from the popup to a pane; its move-left/resize-right popup policy goes.

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
// appended to LayoutNode and LayoutNodeWire (snapshot.rs:78, :90)
Empty,                                     // no tiled pane
// appended to InputMessage::MouseKey (message.rs:2275), #[serde(default)]
press: Option<(u16, u16)>,
```

`WindowSnapshot.layout` is `project()` of the tiled part, `Empty` when there is none.
`TreeOp::WindowLayout` (`tree_delta.rs:43`) appends `floating` and `modal`; diff (`:265`) and apply
(`:457`) carry them. Floats sit in `panes`, so `retain_snapshot_panes`
(`crates/zz-client/src/core.rs:1381`) only loses its popup case. `EventPayload::Popup`,
`InputMessage::Popup` and their types (`message.rs:2252, 3117-3170, 3601`) keep their tags, are never
sent or read, and go at the next version. The v108 section of `knowledge/protocol/wire-protocol.md`
gains a paragraph.

# Layout strings (with pin.layout-v2)

Because floats live in the tree, the v2 writer is `layout_append_v2` (`layout-custom.c:343`) over it:
floats appear where 3.8 has them, with `"z"` from a port of `layout_cell_zindex` (`:312`). Two plain
`new-pane` on a one-pane window give `{"t":"v",...,"c":[tile, first float, second float]}`. Zoomed,
`#{window_layout}` dumps the real tree and `#{window_visible_layout}` the tree `window_zoom` builds
(the target as root, wrapped in a `"v"` node with each over-zoom float inserted right after it, so the
last one first). The v1 string is the tiled copy (`layout_custom_create_compat`, `:495`), and with no
tiled pane it is whatever 3.8's failed dump prints. The parser places `"z"` leaves as floats in place
(`layout-custom.c:1056-1066`); v1 input keeps existing floats (`layout-custom.c:693-702`).

pin.layout-v2 left the places to fill in (`crates/zz-mux/src/layout.rs`): the writer asks a
`LeafState` per leaf and emits `"z"` before `"I"` when `z` is set, and the v1 writer already drops
those leaves and collapses their parents, so floats need only `Window::leaf_state` to return the
`layout_cell_zindex` port. The parser keeps each floating leaf's z-index and signed offsets in
`ParsedLeaf::float`, trimming already removes a floating bottom-right cell without a gift, and the
size checks skip floating cells; `MuxState::select_layout_string` refuses a layout that still has
one (`floating panes are not supported`), which is the branch that places them.

# Clients

**Desktop** (`crates/zz/src/workspace/view.rs`). `render` (`:3656`) draws visible floats back to
front after `render_layout` and before `overlays`: an absolute `FloatingSurface`
(`crates/zz-ui/src/pane.rs:279`) around the pane's normal surface (terminal, browser, agent) at
`xoff * cell_w, yoff * cell_h` from the canvas origin with this client's cell metrics, clipped,
titled from `border_status_text`, borderless for `None`. `render_layout` draws `LayoutNode::Empty` as
the bare canvas, with no drop targets. Title-bar and edge drags preview locally and on release send
`move-pane -t %N -X x -Y y` for a move and `resize-pane -t %N -x w -y h` for a resize (outer cells).
`resize-pane` keeps the offsets (`layout.c:907-935`), so when a resize also moved the origin (left or
top edge, or their corners) the commit is one command list,
`resize-pane -t %N -x w -y h ; move-pane -t %N -X x -Y y`, which lands on the previewed rectangle. A click focuses it like a
tiled pane, and the mux raises it. A modal adds an occluding scrim (a click sends `kill-pane` only
with `close_on_click`), and with `capture_keys` the chrome keymap forwards every key unresolved.
Delete `popup_overlay` (`:3433`), `PopupPane` (`:575`), `TerminalView::new_popup` and the popup
handling in `crates/zz/src/mux/client.rs:3992`.

**Web and iOS** (`clients/app/src/app.rs`). The same, inside `workspace` (`:1514`), replacing
`terminal_overlay` (`:2687`). Compact iPhone pages floats like any pane (a page zooms its pane; a
window whose only pane is a float shows it at its cell); the modal and visible over-zoom floats draw
over the page. Native browser views under a visible float hide, as under popups today (`:2853`).

**Raw TUI** (`crates/zz-tui/src`). `recompute_layout` (`state.rs:1088`) appends float rects (border
and content, clipped) after the tiled ones, and `Empty` yields no tiled rect; `paint_workspace`
(`render.rs:730`) paints unowned cells as 3.8 paints them, then visible floats back to front with
`paint_popup`'s fill and `paint_floating_border` (`render.rs:1611, 2898`), status text on the border
row from `xoff + 2`; `pane_at` (`state.rs:1005`) uses the hit order. Kitty placements of a covered pane
are withdrawn while covered. Delete the popup branch of `input::handle` (`input.rs:199`) and
`popup_pointer_action` (`:1851`).

# Where the float.core build differs

Recorded by float.core (sessions 1 and 2, 2026-10-09); 3.8 wins where this plan and the tag disagree.

- No `FloatCell`: `CellNode::Float { pane, geometry }` reuses `CellGeometry` with signed offsets,
  and the window keeps its extent in `CellLayout` once no tile is left.
- 3.8 counts a pane as floating only while its current layout cell is (`window_pane_is_floating`):
  a zoomed float and a float hidden by zoom read as tiled in `pane_floating_flag`, the `F` flag and
  `pane_z`. `Window::shows_floating` is that rule; `Window::is_floating` stays the tree's flag.
- Select-pane while zoomed keeps the zoom when the target is visible (the zoom target or an
  over-zoom float), as `cmd_select_pane_exec` pushes and pops only for hidden panes.
- Directional `select-pane` uses `window_pane_find_*`'s cell arithmetic over every pane once the
  window has a float; tiled-only windows keep the older normalized walk. Compass targets treat every
  float as bordered, because the mux model does not see `pane-border-lines none`.
- `display-popup` hands `remain-on-exit`, `remain-on-exit-format` and, with `-T`, the border status
  and format to `new-pane` through `ExecutionContext::set_spawn_pane_options`, which sets them on
  the new pane before the daemon spawns its process. A command client waits through `new-pane -W`,
  so a signal exits 128+N and a popup killed by `kill-pane` or `-C` before its command exits answers
  129, as `window_pane_wait_finish` does (`wait-pane --exit` keeps answering 0 for a killed pane).
  Control clients still get nothing, and a popup still needs a target client. `-C` also clears the
  target client's menus and other overlays, as 3.8's `server_client_clear_overlay` did, before it
  kills the modal.
- `new-pane -M` answers as an unsupported flag, as `move-pane -M` does: creating and sizing a float
  from the drag needs the per-client drag float.keys builds, so `flag:new-pane:-M` is tracked there.
- Modal `-D` and `-K` act in the daemon's key path before the key tables, and a click outside the
  modal is dropped (or kills a `-C` modal) in its mouse path, both keyed on the pane the client
  reports; drawing the modal and hit-testing floats on the client side is float.clients.
- `PopupPointerState` went with the per-client popup, because it was keyed to the old `PopupPointer`
  input; float.keys builds the per-client drag on `MouseKey.press` instead. The daemon ignores
  `press` until then.
- `pane-border-status top-floating` and `bottom-floating` reach a float's snapshot as `Top` and
  `Bottom`; `PaneBorderStatus` has no floating variants.
- `FloatingPaneSnapshot` lists every float in the tree, a zoomed one with `visible: false`.
- zz back-solves the window extent from one pane's size report; while a float is active that pane is
  the most recent tiled one.

# Where the float.clients build differs

Recorded by float.clients (2026-10-09).

- `compat/tui-floating.sh` compares the decoded glyphs of every cell, the cursor and the pane list
  against tmux 3.8; it does not compare colours. While `display-popup` is up it compares the screen
  only, because 3.8's popup is a client overlay and zz's is a modal pane.
- The drag commit `resize-pane -x -y ; move-pane -X -Y` is one request,
  `run-shell -C "resize-pane ... ; move-pane ..."`, which zz parses as a tmux command list
  (`zz_client::floating::float_drag_command`); the daemon still publishes a snapshot after each
  command of the list. The height it sends takes back the row `resize-pane -y` adds for a float on
  the row under a top pane status or above a bottom one.
- `EventPayload::Popup` and `InputMessage::Popup` are renamed `RetiredPopup` in Rust with their serde
  names kept, so clients match the retired tag without naming the popup.
- The raw TUI fills a window with no tiled pane with the default `fill-character` inside cell
  (`bg=themedarkgrey`); a user `fill-character` is not read.
- `WindowSnapshot` and `TreeOp::WindowLayout` also append the window's `sx` and `sy`: with no tiled
  pane the layout dump carries no window height, and the drag's bottom-status correction needs it.
- `PaneBorderPresentation` also appends each pane's expanded `window-style` and, for the active
  pane, `window-active-style`. The raw TUI paints a pane's default cells in them, per ground as
  `tty_default_colours` takes them, with theme colours resolved through the status line's slots, so
  a `display-popup`'s `popup-style` (`bg=themedarkgrey,fg=themewhite`) matches 3.8. The desktop and
  the web/iOS clients keep drawing the daemon's pane appearance, where `window-style` is already
  applied but a theme colour maps to zz's own terminal palette slot.
- `compat/tui-overlays.sh` asserts its popup cases with three known differences rewritten out, each
  from the modal-pane ruling (gap `display-popup.modal-pane`): the title one column right of 3.8's
  box, the window's `O` flag on the status row, and a centred popup a row higher at an odd height.

# Where the float.keys build differs

Recorded by float.keys (2026-10-09).

- `new-pane -M` and `move-pane -M` are supported flags now, which supersedes float.core's note
  above that both answer as unsupported.
- The per-client drag is `Client.mouse_drag` in the daemon, armed by `MuxEffect::ArmMouseDrag`
  from `new-pane -M`, `move-pane -M` and `resize-pane -M`. A later drag report of the same client
  runs `resize-pane -M -t <pane>` with the armed drag in the invoking mouse event and no hooks, which
  dispatches to the update the arming command chose; any report that is not a drag or a wheel ends
  it. The raw TUI keeps sending a gesture's drag and release reports once one of them was bound,
  whatever key they name, and `MouseKey.press` carries the press cell on each.
- A tiled border drag arms the drag too, so its later reports skip the key tables; the update is the
  existing absolute resize to the pointer, which lands where 3.8's relative one does.
- `new-pane` into a window whose root is a float wraps it in a node carrying the float's offsets, as
  `layout_replace_with_node` does, so the layout check accepts a non-zero root offset when no tile is
  left.
- The editor modal is `new-pane -O -c /tmp` with outer `-x -y -X -Y` that give the 90% content box
  and `remain-on-exit off`; `choose-buffer`'s `e` is the `edit` row of zz's `choose-buffer` table.
- A `display-menu` run from a mouse binding takes mouse keys, as 3.8 does when the event is valid
  (`MENU_NOMOUSE` only without one and without `-M`), so the pane menu's Move item is chosen by a
  mouse release and its submenu opens.
- Mouse rows reach the drag code in window cells: the top status lines are subtracted and a row on
  a bottom status line is clamped to the window's last row, as `cmd_resize_pane_mouse_*`,
  `cmd_join_pane_mouse_move` and `cmd_split_window_mouse_resize` adjust `m->y` and `m->ly`.
- The buffer editor writes back only into the buffer it opened (same name and same data), emits
  `paste-buffer-changed`, and input reaches the editor while the buffer chooser stays open on the
  pane beneath it.

# zz-only extensions

- `new-pane --kind terminal|browser|picker|agent [--profile] [--provider]`, inherited from
  `split-window` (`catalog.rs:1777`): floating browser and agent panes, drawn with their own surface.
- GUI drag and resize by title bar and edges, committed as commands (GUI clients load no tmux mouse
  tables). iPhone pages floats.

# Implementation split

float.core lands after pin.layout-v2; float.clients and float.keys then run in parallel.

**float.core.** Zones: `crates/zz-mux/src`, `crates/zz-protocol/src/{snapshot,tree_delta,message,catalog}.rs`,
`crates/zz-daemon/src`, `knowledge/protocol/wire-protocol.md`, `compat/scenarios`,
`compat/tmux-gaps.json`. Clients compile with a no-op arm for `LayoutNode::Empty`; their popup code
goes dead until float.clients.

1. `compat/scenarios/floating-panes.txt` matches 3.8 under `--strict-geometry`, printing
   `list-panes -F '#{pane_id} #{pane_width}x#{pane_height} #{pane_x},#{pane_y} #{pane_z} #{pane_floating_flag}#{pane_active}#{pane_modal_flag} #{pane_flags}'`
   and `#{window_layout}` after each of: two `new-pane`, `new-pane` with the first of two tiles active,
   `new-pane -t` a float, `break-pane -W -x 20 -y 6 -X 10 -Y 3`, `move-pane -P bottom-right`,
   `move-pane -L 2`, `move-pane -z 1`, `resize-pane -x 30 -y 8`, `split-window -h` on a float,
   `select-pane` of the back float, `swap-pane` float with tiled, self `join-pane`, `kill-pane` of a
   float, `select-layout even-horizontal`, `resize-window -x 30 -y 10`.
2. In `[A, (B / C)]`, `break-pane -W -t A` then `join-pane -s A -t A` restores A beside the whole B/C
   column with 3.8's geometry.
3. Every error string in the commands table answers verbatim (scenario or zz-mux test).
4. Zoom: after `resize-pane -Z` on a tile, `new-pane -A` keeps `window_zoomed_flag` 1 with the float in
   `#{window_visible_layout}`; a plain `new-pane` from that over-zoom float keeps the window zoomed on
   the new pane; a plain `new-pane` from a tile unzooms; killing the `-A` float keeps the zoom;
   `resize-pane -Z` on a 20x6 float gives it the window's size and unzoom gives back 20x6; all as 3.8.
5. Modal: after `new-pane -O`, `select-pane` elsewhere keeps it active, `break-pane`, `join-pane` and
   `swap-pane` answer `pane is modal`, a second `-O` errors, killing it restores the previous pane,
   and killing the previous pane first makes the close fall back as `window_lost_pane` does;
   `window_modal_pane` and window flag `O` match 3.8.
6. `select-layout` with each `#{window_layout}` from clause 1 restores the same tree and floats.
7. No tiled pane: `break-pane -W` of the sole tile, its `kill-pane`, and a plain `break-pane` or
   `join-pane` of it into another window each leave a window of floats whose `list-panes` and
   `#{window_layout}` match 3.8; a self `join-pane` there tiles the float to the whole window; the
   window closes with its last pane; the snapshot carries `LayoutNode::Empty`.
8. Daemon tests: with two attached clients, `display-popup -E 'sleep 1'` puts one modal in both
   snapshots, sends no `EventPayload::Popup`, and closes on exit; from a command client
   `display-popup -E 'exit 3'` exits 3; `display-popup -C` kills a `new-pane -O` modal; a second
   `display-popup` leaves the pane count alone; `display-popup ""` runs `default-command` (or the
   shell) in a live modal, not an empty pane; no `u64::MAX` pane id remains in `crates/zz-daemon/src`.
9. PTY sizes follow the allocation rule (float, zoomed float, hidden float, clamp), and a client size
   report for a float changes nothing.
10. A round-trip test pins the appended snapshot, delta, `LayoutNode::Empty` and `MouseKey.press`
    encodings; one `move-pane -X` emits exactly one `TreeOp::WindowLayout`.
11. `choose-tree -O z` lists panes in `pane_z` order. `pane.floating-model` turns from native to adopted
    and each item closes with the behaviour that proves it (`option:editor` in float.keys;
    `semantic:nested-attach-in-popup` keeps its fixture requirement); `just compat check` passes.

**float.clients.** Zones: `crates/zz/src/{workspace,mux/client.rs,terminal/view.rs}`,
`crates/zz-ui/src/pane.rs`, `clients/app/src`, `crates/zz-client/src`, `crates/zz-tui/src`,
`compat/tui-floating.sh`.

1. `compat/tui-floating.sh` matches tmux 3.8 cell for cell on: two overlapping floats over a vertical
   split, before and after `select-pane` raises the back one; a `-T` float under
   `pane-border-status top-floating`; `pane-border-lines none`; a `display-popup` over a zoomed pane;
   a window with no tiled pane and two floats.
2. Same fixture: a click on the overlap activates the front float; a click outside a modal changes
   nothing, and kills a `new-pane -O -C` modal.
3. A browser pane covered by a float has no kitty placement while covered.
4. A unit test covers the shared cell-to-pixel rect helper (clipping included) and the drag commit
   commands: a move sends `move-pane -X -Y`, a right or bottom edge resize sends `resize-pane -x -y`, a
   left, top or top-left corner resize sends `resize-pane -x -y ; move-pane -X -Y` as one command
   list whose result is the previewed rectangle; the lane report has desktop and web screenshots of a float, a modal, an over-zoom float
   and a window with no tiled pane.
5. A zz-client test: a focused modal with `capture_keys` sends the prefix key to the pane.
6. `rg 'PopupState|PopupAction|EventPayload::Popup'` finds nothing under `crates/zz`, `crates/zz-tui`,
   `crates/zz-client` or `clients/`.

**float.keys.** Zones: `crates/zz-protocol/src/key.rs`, `crates/zz-mux/src`, `crates/zz-daemon/src`
(`keys.rs:123`, mouse commands, drag state), `crates/zz-tui/src/input.rs`, `compat/scenarios/smoke`,
`compat/tui-mouse.sh`, `compat/tmux-gaps.json`.

1. `list-keys` answers 3.8's lines for prefix `*` `@` `g` `Tab` `BTab` and root `C-MouseDrag1Pane`,
   `C-MouseDrag1Empty`, `M-MouseDrag1Pane`, `M-MouseDrag1Border`, `MouseDown1Control7`; the `move`
   table keeps its 19 rows (`key.rs:178-430`).
2. Attached TUI: prefix `*` creates a float, prefix `g 1` moves it top-left, prefix `@` floats and
   tiles, pane menu `f`/`t`/Move work, geometry equal to tmux.
3. `compat/tui-mouse.sh`, geometry equal to tmux: C-drag from a pane and from empty space creates a
   float spanning press to pointer (the TUI sends `press`), M-drag moves, border drag resizes, top-border
   drag moves, and each gesture's later reports drive the same pane until release;
   `flag:move-pane:-M` closes in `mouse.bound-context`.
4. With `editor` a script appending a line, choose-buffer `e` opens a modal at 90% centred and an exit
   0 updates `show-buffer`; non-zero leaves it; customize-mode `e` edits a string option the same way.
5. `keys.move-table` closes, `keys.default-prefix` drops `*` `@` `g` `Tab` `BTab`, `option:editor`
   closes.
