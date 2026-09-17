---
type: Configuration
title: UI design conventions
description: The component, palette, and styling rules that keep zz application chrome consistent and theme-aware.
resource: crates/zz/src/command/palette.rs
tags: [ui, gpui, zz-ui, theme, chrome, clippy]
timestamp: 2026-09-17T00:00:00Z
---

# Overview

**`zz-ui` (`crates/zz-ui`) is zz's base component layer.** It is a full fork of `gpui-component`, not a
dependency on it: commit `7811a47` vendored the widget set and removed the upstream crate from the
workspace entirely. The fork revision and the per-module port notes live in `crates/zz-ui/UPSTREAM.md`;
read that file before widening or diverging the surface.

Application chrome must use zz-ui's components and theme vocabulary rather than maintaining a
parallel widget set or color palette. Import `zz_ui::ActiveTheme` (not `gpui_component::ActiveTheme`;
that path does not resolve) and read colors from `cx.theme()` at the point where a GPUI element
is rendered.

The house-style reference is `crates/zz/src/command/palette.rs`. It demonstrates `cx.theme()`
derivations alongside the `Input`/`InputState`, `ListItem`, `Kbd`, and `Tag` widgets.
The browser client in `clients/web` (`just web`) uses the shared GPUI components outside the desktop app.

# Rules

1. Every application-chrome color comes from a `cx.theme()` root or a `Colorize` derivation of one.
   The palette has six roots (`background`, `foreground`, `accent`, `success`, `warning`,
   `danger`) plus the fixed per-mode `scrim` in `crates/zz-ui/src/widget/foundation/theme_color.rs`; panels, hover fills, muted text, and
   focus rings are derived at paint time by `Colorize` in
   `crates/zz-ui/src/widget/foundation/color.rs`. Choose the nearest derivation, such as
   `background.raised(1)`, `background.hover()`, or `foreground.muted()`; do not color-match an old
   literal.
2. Check the zz-ui widget set before hand-rolling a control. A custom GPUI
   element is appropriate only when a component cannot preserve required input routing,
   rendering, or interaction behavior. When a fork-local widget must diverge from upstream, record
   why in `crates/zz-ui/UPSTREAM.md`.
3. `clippy.toml` enforces the chrome-color rule by disallowing calls to `gpui::rgb`, `gpui::rgba`,
   and `gpui::hsla`. Keep any exemption narrowly scoped and document why the color is not
   application chrome.
4. Do not scatter branding or palette literals through views, fixtures that model application
   chrome, or component state branches.
5. **Menu, picker, and list highlights use the same control treatment.** Rows use
   `Theme::selection_background()` (opaque accent) with a matching edge, the same
   `foreground` text and icons as unselected entries, and no shadow. `StyledExt::selection_highlight` applies the
   solid treatment; secondary text inherits the row color. `ListItem`, popup
   menu rows, select rows, and file/history picker rows use `Theme::menu_radius()`: the
   container radius minus 4px, without a fixed cap or Full-mode radius sentinel.
   Highlights use native row backgrounds and corner rounding. PopupMenu, model picker,
   and directory/session picker rows use `StyledExt::menu_item_corners`: fixed radii with
   smoothing 2.5, capped at 40% of their row height. Directory and session rows share
   26px heights and 12px labels. Custom menu content receives the highlight state so
   descriptions, counts, and timestamps brighten to the normal foreground.
   Their requested radius reaches the renderer without adaptive compression; the cap keeps
   them short of full pills. Menu containers keep the window's adaptive squircle shape. Open submenu parent rows use a dim neutral highlight; leaf selections keep the
   accent. Selected shortcut labels and icons brighten to the full foreground. The command palette matches dropdown typography with 12px medium labels, 10px descriptions, 16px line heights,
   26px selectable rows, and 2px gaps. It is at most 560px wide, with 8px list gutters,
   a 40px bare Input, 12px search text and inline Tag pills, and a 10px footer. Matched characters
   use semibold foreground; breadcrumb prefixes stay muted until selected. Host status dots
   use success, warning, or muted foreground; session/window running-agent summaries use accent dots.
   Agent pane rows use shared vendor icons and text status without a duplicate dot. All palette
   colors and corners follow the current Chroma theme. `PaletteRow::icon` and
   `PaletteRow::expanded` supply tree icons and disclosure state; `command_palette_tree_entry`
   gives the disclosure a toggle handler separate from row activation. Workspace and Navigate
   render selectable session/window/pane trees with these shared rows. Search flattens matching
   descendants into breadcrumbs. Shared row builders also render daemon command and value prompts.
6. Use one translucent signal instead of competing fills. Sidebar pointer hover, keyboard selection,
   mux focus, Settings navigation, and clickable native status windows use `workspace_row_highlight`, a
   `background.washed(2)` tint that preserves the desktop blur. Neutral buttons (Default, Secondary,
   and Ghost) use the same tint for hover, pressed, and selected states through the shared Button
   style. This covers toolbar icons, Back, and agent footer controls without per-button fills.
   Tree fills keep a 1px vertical inset,
   so adjacent rounded rows never merge into one slab. The final Add host action is the exception:
   its row stays unpainted and only its label moves from muted to foreground on hover.
7. The shared control edge is a 0.5px border at 10% foreground opacity with a small soft shadow.
   `StyledExt::control_surface` applies it to inputs, number fields, select triggers, dropdown
   surfaces. Settings stacks draw it around the whole group, with flat
   separators between entries. Reserve border width before hover or focus; change only its color
   between states. Inputs and number fields reserve a 1px border, using the accent color when focused.
   Settings value pickers use Button triggers and PopupMenu rows, including font, search engine,
   and multiplexer options. The shared Select API keeps their values and confirmation events;
   it has no separate trigger or row styling.
   The thicker stroke avoids faint diagonal coverage in Full mode. Use `Button::flat()` for actions embedded in another control, including tree-row
   close buttons, browser tab close buttons, input clear buttons, number steppers, and settings
   reset buttons. These retain the keyboard focus indicator without another raised edge. Tree-row
   actions and browser tab close buttons also use `Button::text()` so hover only brightens their
   foreground, without a button fill. Other embedded actions keep their wash.
   Gapped panes use the same soft shadow and foreground edge, retaining their configured border
   width and a stronger active-pane outline. The continuous app background sits beneath both pane and shadow. Flush panes have no
   outer shadow. Only shared divider segments touching the active flush pane use the accent
   color; exterior edges stay unoutlined. The settings preview follows the same rule.

   The local renderer makes blurred shadows follow the pane's corner smoothing, including
   its small 2px control shadow. Circular and fully rounded surfaces retain circular shadows.

   Active panes add a soft inset accent glow above content and below status overlays:
   6% opacity at the default strength, 96px blur, (16px, 24px) offset, and -8px spread.
   Settings > Panes > Selected pane glow scales its strength from 0–200%; 0 turns it off.
   `pane-glow-strength` stores the factor from 0–2, defaulting to 1.
   `PaneChrome::active` shares this top-left emphasis across desktop, browser, and preview callers.
   The glow fades in over 300ms when a pane becomes selected, paced at 30fps through
   `Animation::with_max_fps` because every animation frame redraws the whole window; reduced
   motion or disabled animations show the final strength immediately. The local Metal,
   WGPU, and DirectX renderers dither the fade to reduce banding. Pane content roots paint their own backgrounds using the configured pane opacity.

Settings > Panes scrolls its three-pane preview with the controls, like the Terminal preview. The shared
`settings::panes_preview::PanesPreview` uses sample Terminal, Agent, and Browser content inside
`pane_surface` and `pane_split_surface`, with full-size logical-pixel margins, corners, and borders.
A neutral backdrop makes pane transparency visible. Clicking a sample changes only the preview
selection and replays the selected-pane glow.
The Browser sample keeps its page opaque; its toolbar follows pane background opacity, and the
Agent sample keeps its composer opaque. Desktop pane edits and resets refresh the saved configuration
immediately, including valid numeric input while typing; the file watcher still handles external edits.

Settings > Status bar scrolls its live preview with the controls. The session switcher uses
a Layers/name/chevron button. Session and agent buttons share the active window pill’s background,
theme border, shadow, and 26px height, with widths sized to their contents. The 35px titlebar leaves 4.5px above and below each pill. Window pills always align left, and the right edge shows an
agent status dot and summary with a pane-selection menu. Time/date and alignment controls are absent. Window pills share the
production pane deck: 20px rounded cards with 14px icons overlap by 7px inside a vertically centered 26px pill, with a theme outline and directional
shadow. Cards stack left to right above their right-hand neighbors; hovering lifts a card above
inactive cards, with a fixed hit area to avoid hover flicker. The focused pane always paints last,
even above a hovered card or overflow, and sits 1px higher at
rest. Three pane icons plus `+N` bound the width. Tooltips show pane details or hidden pane names;
clicking a pane card selects it directly. Browser cards use cached favicons with a globe fallback.
The settings sample allows hover while selection actions remain inert.
Automatically named window pills use the active browser or agent pane title. Browser labels fall
back to the active URL and then “browser”; explicitly renamed windows keep their chosen names.

Browser headers use 28px tabs, URL fields, and browser buttons, with 8px outer padding and an 8px gap
between the tab and navigation rows. Pane split and close buttons keep their 24px size (`crates/zz-ui/src/browser.rs`, `BrowserHeader`).
A shared 24px pane drag handle sits between the split controls and Close in Terminal, Agent,
and Browser headers. Its icon is a two-column, three-row grid of dots. It keeps the existing
drag-to-split or swap behavior and availability rules.
The URL field contains a 24px site-controls button at its left edge. Its popup reads Chromium connection
status and tab sound state when opened, and offers the existing site-data clearing confirmation.
The URL field has no visible resting background, border, or shadow; focusing it adds a
`background.washed(1)` fill and the shared button border and shadow. Active tabs use the
same treatment. Both reserve the border width so state changes preserve layout.
Browser tabs start at 180px wide and shrink evenly to 112px before overflowing into horizontal
scrolling. Tab labels and URL text use 13px type with a 16px line height. Tab icons have 12px leading padding and a 6px gap before the label.
Omnibox suggestions use 32px single-line rows with title and muted URL text. Their rounded
hover and selection fills sit inside the dropdown's 4px padding. Tabs and history rows use
stored page favicons, with a globe fallback.

# Control density

Menus, select dropdowns, command palettes, and file/history/tree pickers share
`StyledExt::popover_style`: an opaque `background.raised(2)` surface, the shared half-pixel edge,
and a soft outer shadow scaled by the shadow-strength setting. Menus use 4px gutters, 26px
rows (20px for Small menus), 12px text, and half-pixel separators inset from the surface edge.
Picker search fields use the shared Input surface. Command palettes embed a bare shared Input
inside their raised container, with inline mode, host, and command Tag pills. They use 8px list
gutters; the container radius adds that inset to the theme radius. Rows retain the inset menu
radius, so the configured corner setting reaches both the surface and its contents.

The browser action menu puts the current profile and page zoom together. Its zoom stepper reads
live page zoom from the browser view, stays open after each click, and preserves the existing
zoom steps and limits. Its row stays at least 26px high to fit the embedded controls. With that row selected, Left/Right changes zoom and Enter resets it;
Escape dismisses the menu. Other menu actions retain their existing dismissal behavior.
`PopupMenuItem::stepper` owns those interactions so nested buttons cannot trigger a parent
menu dismissal.

Settings uses 13px primary row labels, so `Input::small()` renders its editable value at the same
13px rather than GPUI's 14px `text_sm`; `NumberInput::small()` inherits that field treatment. The
shared dialog surface is similarly compact: 400px default width, 12px gutters, 13px title, 12px
description, and Small action buttons. Explicitly sized content dialogs such as the attachment
preview retain their own width.

Dedicated pane/client tree and paste-buffer choosers use `zz-ui::chooser::ChooserModal`: 600px and 640px
maximum widths, 40px rows, 12px header/footer gutters, and the shared compact close button.
The surface grows with its row count up to ten visible rows and scrolls within the available
window height. It sits near the top of the workspace like the command palette. Tree rows use
the shared navigation icons, a muted target ID after the label, and a small checkmark for active
entries. Their selection uses `workspace_row_highlight`; the header and footer share the body
color. The outer radius adds the 4px row inset to the theme radius. The Commands & choosers
catalog includes the complete modal alongside the individual rows and search footer.
Daemon `choose-tree -s` and `choose-tree -w` requests use the compact Navigate palette instead,
preserving their initial tree expansion and selection. See the
[palette contract](/concepts/command-palette.md) for navigation and search behavior.

**A toast is that same surface.** `widget/overlay/notification.rs` imports the width, gutter and two
text sizes from `widget/overlay/dialog.rs` rather than restating them, so the two things that
interrupt a window . the modal it must answer and the toast it may ignore . speak at one size. Its
icon is a flex child aligned to the first line, not an absolutely positioned glyph the copy column
pads around.

**Every toast stacks at the top center of the workspace window**, whichever view raised it. The
close button sits in the toast's flex row, vertically centered beside its content. Settings is a route in
that window, and `AppShell` mounts both the dialog and notification layers for either route.
`crates/zz/src/window/toast.rs` keeps the workspace `WindowHandle<Root>` in a global and defers its
update until the current window callback finishes. Updating that handle during its own click
callback fails because GPUI has temporarily removed the window from its registry. Views that
already receive the workspace `Window` can call `window.push_notification` directly.

# UI font

Interface settings offers a UI font picker over GPUI’s system font families on macOS, Linux, and
Windows. `ui-font-family` in `zz/config` sets `Theme.font_family`; System default uses GPUI’s
`.SystemUIFont` alias. Config reload and OS appearance changes reapply the choice to open windows.
The `UiFontConfig` global holds this string separately so `AppConfig` remains `Copy`.
Native entrypoints register `AvailableFonts` from their GPUI platform text system. The picker queries
that catalog directly to avoid offering GPUI fallback names that are not installed.

# UI zoom and scalable metrics

Application UI zoom is transient whole-window content zoom in `crates/zz/src/ui_scale.rs`, expressed
as a percentage of the default scale: `Cmd/Ctrl +` and `Cmd/Ctrl -` move it by 10 percentage points
between 50% and 300%, and `Cmd/Ctrl 0` restores 100%. The Appearance page's **UI zoom** row is a
`NumberInput` over that same percentage, so its steppers and the shortcuts land on the same values.
Each change updates the global `UiZoom` and applies it to every open window with
`Window::set_zoom`; newly opened windows receive the current value too.

`Window::set_zoom` folds that multiplier into the window scale factor and logical viewport, so the
entire GPUI element tree follows UI zoom, including values expressed in pixels. Continue to use a
named `Size` or `zz_ui::rems_from_px` when that captures a component's semantic sizing contract, but
do not multiply metrics by `UiZoom` again. Browser viewports already receive the effective window
scale factor, so native-to-CEF geometry follows the same rule. Geometry that must remain physically
screen-sized needs to opt out explicitly with `UiZoom::unzoomed`.

# Terminal color exception

Terminal grid colors are a separate, renderer-neutral system described by
[terminal appearance](/terminal/appearance.md). Terminal foregrounds, backgrounds, palette
entries, selections, and cursor colors come from `TerminalAppearance`,
`AppearanceColor`, and `Color`; they are not application chrome colors. Exact terminal-color
conversion code and tests may therefore carry narrowly scoped lint exemptions. The surrounding
chrome (borders, labels, hover states, badges, and reset controls) still reads its colors from
`cx.theme()`.

# Chrome chroma is independent of the terminal

**Application colors come from zz-ui's own `ThemeColor::light()` / `ThemeColor::dark()` palettes,
an optional preset per mode (`chrome-preset-light`, `chrome-preset-dark`), and the `chrome-*`
overrides in `zz/config`, never from the
terminal.** The mode follows the OS appearance unless `theme-mode` pins one. There is no knob that
derives chrome from terminal colors, and a Ghostty palette cannot repaint the window.

`crates/zz-ui/src/chrome_palette.rs` owns the shared preset catalog and palette resolver used by
the desktop and browser clients. `crates/zz/src/theme.rs` applies desktop configuration. It holds the latest immutable
`TerminalAppearance` and `AppearanceProvenance` as GPUI globals (used by settings badges and the
detach action, not to color anything). `apply_zz_overrides` layers these values over the zz-ui base:

- the preset selected for the effective mode, when one is;
- the three optional `chrome-*` palette roots from `zz/config`, written over the preset so every
  elevation, hover, and focus ring derived from them at paint time follows the user's roots;
- `font_family` from `ui-font-family`, with the system UI font as the default;
- `mono_font_family` from the terminal's resolved primary family, so Agent Markdown and code blocks
  match the terminal typeface;
- `theme.radius` from `widget-corner-radius`, so one radius reaches every widget and survives a
  light/dark switch;
- `theme.contrast` from `chrome-contrast`, preserved across the same refreshes.

`chrome-contrast` defaults to 1.0 and accepts 0.5–2.0; other values are reported like any numeric
key. The Appearance page shows 50–200%, in steps of 5. It scales the elevation step in `raised()` and `washed()`, the amounts in `hover()` and
`active()`, the alpha factors in `fill()`, `glow()`, and `wash()`, and the foreground weight in
`border()`. It divides the `muted()` amount, keeping muted text closer to foreground as contrast
rises. Those factors clamp to 0–1. `on()`, `outline()`, `subtle()`, `floating()`, explicit opacity,
and color math keep their existing values; literals such as `foreground.opacity(0.1)` stay fixed.
The Rust derivations read a thread-local scalar set by `Theme::set_contrast`. The native macOS
client keeps the default derivation strengths.

Icon-only chrome controls use `Button::compact_icon`: a 24px hover surface around a Small 14px
glyph with a 0.5px downward optical adjustment. The titlebar, sidebar row actions, browser pane,
and Agent pane share this constructor, which fixes padding and icon scale in one place. Browser
controls override the hover surface to 28px while retaining the shared icon scale. Workspace settings
and sidebar buttons use 26px surfaces to match the titlebar pills, with the same 14px glyphs.

A radius is a *request*, not the final corner. GPUI caps one at half the shorter side . the point a
rounded rectangle stops existing . so one global setting applied to components of different sizes
would make each change shape category at a different value of it: a 24px icon button becoming a
circle at 11→12, a 30px input becoming a pill at 14→15. Windows therefore opt into
`Window::set_adaptive_corner_fraction` (`ADAPTIVE_CORNER_FRACTION`, `crates/zz/src/theme.rs`), which
resolves every radius against the element it rounds at paint time, in one place rather than at 72
call sites. An ordinary corner approaches 45% of the shorter side along `cap * tanh(radius / cap)`:
within a fraction of a pixel of the request where radii are actually set, bending rather than
stopping beyond it, so no component ever changes category *or* stops responding while its neighbours
keep moving.

Settings accepts widget radii from 0 through 25. Values through 24 keep the adaptive
corners described above. Moving past 24 enables **Full** mode: buttons, single-line fields,
select triggers, session tree highlights, titlebar chips, pane cards, and browser tabs use
`Theme::control_radius()` instead of `Theme::radius`. Menu and picker selections use their
separate inset corner rule.
The helper returns GPUI's `FULL_CORNER_RADIUS`, the same value as `rounded_full()`, so these
controls become pills or circles even with adaptive rounding enabled. Desktop settings apply
changes immediately; desktop and browser settings show “Full” beside the radius label.

Containers, dialogs, menu surfaces, multiline inputs, and pane surfaces retain their existing radius
rules. Switch tracks, status dots, avatars, and scrollbar thumbs still use `rounded_full()`
at any widget radius. Explicit button radius overrides remain in effect; the browser site
controls button opts into Full mode while keeping its existing 12px corner below that threshold.

`MuxClient` refreshes those globals from the initial `ServerHello` and every `AppearanceChanged`
event. `refresh_current_theme` restores the correct zz-ui base before reapplying the preset and
overrides, while `sync_system_appearance` records the actual OS mode separately before doing the
same after every light/dark refresh. Every application window must route appearance syncs through
that wrapper. The separate OS-mode global is load-bearing: the installed `Theme::mode` may be a
Light/Dark pin and cannot be reused as the System preference.

`theme-mode` is a `zz/config` key, and Settings' Appearance page opens with a Theme group whose first
card is the System / Light / Dark picker, three drawn window previews
(`config/settings.rs`, `theme_preview`). A pinned mode wins in both `refresh_current_theme` and
`sync_system_appearance`, so the pin survives an OS light/dark switch; returning to System restores
the last recorded OS mode. The previews paint each half from the preset selected for that mode.

Settings maps each `AppearanceSource` tier to one badge: `Default` → “Default,” `ThemeFile` → “From
theme,” `Ghostty` → “From Ghostty,” `Override` → “Overridden.” Those badges describe *terminal*
appearance provenance; chrome colors carry the client-local `Default`/`Overridden` provenance
instead, with “Preset” shown when an otherwise-unset root inherits from the selected family.

`ThemeColor` holds six palette roots plus the per-mode scrim. Selected menu, list, and picker
rows use the opaque accent through `selection_highlight`. The accent also marks checked switches,
selected tiles, active-pane borders and glow, keyboard focus, the Agent send button, effort pills,
and palette mode prefixes and running-agent dots. Use the shared component treatment for these
roles instead of choosing a separate highlight color. `border()` is an opaque Oklab mix
of 86% background and 14% foreground at the default contrast. Other colors derive through `Colorize` at paint time, so
overriding a root needs no parallel token table kept in step. Views read
`cx.theme()` and never receive a copied palette or local color literals.
The app shell paints one continuous chrome background beneath the pane layout. On Linux with
client-side decorations, `RoundedWindowFrame` paints that background through the outer frame and
`AppShell` leaves its inset surface transparent. This keeps the background beneath the antialiased
border instead of exposing the shadow at the join, without stacking translucent fills. Fixed sidebar,
titlebar, Settings, margins, split gaps, and rounded pane corners inherit that fill. The slideover
sidebar paints its own overlay surface. App-owned pane roots use `theme::app_pane_background`
with `Theme::pane_background_opacity`, controlled by Panes settings and defaulting to 50%.
The Agent composer card stays opaque and overlaps the transcript by 12 px at its top edge.
Prefix cards disable this overlap. The transcript clips before the footer, which inherits the pane background, and
the shared pane frame adds no background fill. The terminal blends its Ghostty tint with the
theme base before applying the factor. Browser pages remain opaque.

# Related

- `crates/zz-ui/UPSTREAM.md` . the fork's source revision and per-module record of what was ported,
  trimmed, or rewritten
- [GPUI revision pin](/references/gpui-revision.md) . the pinned GPUI sources
- [`app` crate](/crates/zz.md) . the GPUI client governed by these conventions
- [Terminal appearance](/terminal/appearance.md) . the independent terminal color model
