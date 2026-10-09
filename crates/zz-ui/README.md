# zz-ui

zz's application UI on [`zz-kit`](../zz-kit): panes, terminal painting, the agent
interface, the command palette, settings, navigation, and the compact (phone) shell.
The desktop app and the thin clients (web, iOS) share it. It re-exports the kit, so
zz code imports UI from `zz_ui` alone; anything another app could reuse belongs in
`zz-kit` instead, which depends on nothing from zz.

The app-owned `shell`, `navigation`, and `pane` modules share one background at
`app_shell_surface`. The Linux client-decorated window paints that background at
`RoundedWindowFrame`, beneath its border, and leaves the inset shell transparent.
Fixed chrome inherits it; panes paint their own surfaces
above it using `Theme::pane_background_opacity`, defaulting to 50%. The Agent composer
card stays opaque; the footer inherits the pane background without repainting it.
The composer reserves its measured height, with a 12 px transcript overlap beneath the opaque
input edge. Prefix cards disable the overlap; the transcript clips before the footer.
Margins, split gaps, and rounded pane corners need no separate fill.
The app-owned `pane::drag` module shares a compact grip button, pane drag payload, and drag
preview. Pane views notify their workspace when dragging starts; the workspace owns drop targets
and mux commands. The handle uses the six-dot `grip-vertical` glyph and sits between
the split controls and Close in Terminal, Agent, and Browser headers.
Terminal and Agent header buttons use the same dimmed foreground at rest and brighten
on hover without a background, border, or shadow. `pane_header_icon_button` shares that
treatment for drag, split, and close controls; the clickable Agent title follows it too.
Split surfaces allow both child slots to shrink, including with gaps, so narrow mobile panes
keep their content inside the viewport. Desktop, web, and iOS use the same drop-preview
interpolation. SSH text prompts can show echoed challenges or mask secrets while sharing
the existing confirmation buttons and dialog styling.

The desktop and browser clients share chrome preset definitions and palette resolution in
`src/chrome_palette.rs`, and pane-picker rows in `src/pane.rs`. WebAssembly text fields accept
both Control and Command editing shortcuts so browser clients work on either desktop platform.

The shared workspace components also include window tabs and overflow menus in
`src/navigation/status.rs`, including session switching and agent activity menus with the window pill surface and height, and overlapping pane icon decks, hover details, and direct pane selection in
`src/navigation/status/pane_deck.rs`. The titlebar uses 26px pills and 20px pane cards with 7px overlap and 14px icons to leave vertical space around both surfaces. Workspace settings and sidebar buttons match the 26px pill height. Sidebar markers, actions, and keyboard navigation live in
`src/navigation/sidebar.rs`, and command palette/menu/confirmation presentation in
`src/command/`. `src/agent/slash.rs` renders provider command suggestions; the matching and
replacement rules live in `zz-client`. Desktop and browser both use these components.

`src/command/palette_model.rs` and `src/command/palette_view.rs` hold the one command palette
(model, view, and the `PaletteBackend` trait) that desktop, web, and iOS adapt to their own
connections.

`src/tmux_style.rs` resolves tmux style strings and styled segments (`#[fg=…,align=…]`) into
theme colors and highlighted text for status labels, popups, menus, and display-panes labels.
`ContextMenu` also opens on a touch long-press, so rows and pills with rename and close menus
work on iOS.

`src/terminal.rs` contains the portable GPUI terminal painter, including retained row shaping,
selection, cursor, scrollbar, and image placement. It consumes `zz-terminal` view data with the
engine features disabled. Each client owns its input, focus, image delivery, and connection
lifecycle; native daemon and operating-system dependencies stay outside `zz-ui`.

`src/compact/` is **zz-original**: phone-sized widgets for the thin clients, with no upstream
counterpart. `Pager` is a renderer-free model that pages one pane per screen from touch pans and
wheel scrolls (rubber-banded edges, a quarter-width or velocity commit, one page per gesture,
the fling ended after a release, a spring settle). `Lift` is the same kind of model for a drag up
the bar: progress follows the finger over 300px with resistance past the top, and a release past
a third or a flick springs it open, otherwise closed. The rest is the chrome around it: `page_dots`,
`compact_bar` with its grabber, `compact_bar_pill` (pane icon, title, a meta line and the dots), and
a 44px keyboard button, the transparent `compact_pane_header`, the
centered `compact_hud` badge (the terminal text size while pinching),
`bottom_sheet` (dragged down by its header, or by its content from the top, to close), the
grouped `WhichKeyList` a sheet shows, `swipe_back` for a back swipe that pops a pushed page over
the one beneath it, and the soft-keyboard `KeyRow` with app-level `StickyModifiers`. The sheet and
the swipe share one drag-and-settle model; `coast_guard`, mounted once at the host's root,
swallows and ends the fling momentum of a pan they took after they close. When the host sets
`touch::CoarsePointer` (narrow thin clients do), `Button::hit_slop`, switches, select triggers,
and number steppers answer taps in a 44px box around the visual, and workspace tree rows and popup
menu rows grow to 44px. `touch::touch_scale` renders desktop-sized chrome under a coarse pointer
with the rem size grown by 15/13, menus it opens included. Under a coarse pointer `SettingEntry`
puts its control on the title line and the description below it, `Switch` grows to 44 by 26,
`Select` opens a `compact::bottom_sheet` of `sheet_option` rows (search past 12 choices,
`SelectItem::font_family` previews fonts, `Select::title` names the sheet) instead of the popup
menu, and `ColorPicker` shows its hex and opens the hex field and swatches in a sheet. Sheets
opened from inside a page go through `compact::floating_sheet`, which lifts them onto the visible
part of the window; `bottom_sheet` is sized in pixels so a touch-scaled parent leaves it alone.
`sheet_action` is the same row with a muted icon in the check slot, for a command such as "New
session" after a sheet's choices.
`touch::press_feedback` and `press_highlight` track a press from touch-down (delayed 100ms unless
a scroll starts, fading in 120ms and out 180ms, shown at least 130ms); buttons skip hover and
`active` styles under a coarse pointer and draw this instead, as do settings rows that open a
page, tree rows, menu items, which-key rows, and key row keys. `yield_back_swipe` marks a
horizontal scroller that keeps a rightward pan while it can still scroll back, since `swipe_back`
now takes a pan that starts anywhere on the page. Its tool keys share one gesture: a tap does the key's own job, and a
hold or a slide raises a card above it, where releasing on an item picks it. `ArrowPad` is the
direction card and repeats while held; `PopoverKey` is a menu or chord grid (`ToolKeys` builds the
ctrl, alt, and prefix set). A long press released in place leaves the card open for taps. Colors
come from the theme; nothing here depends on a platform.
