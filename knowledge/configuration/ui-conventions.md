---
type: Configuration
title: UI design conventions
description: The component, palette, and styling rules that keep zz application chrome consistent and theme-aware.
resource: crates/zz/src/command/palette.rs
tags: [ui, gpui, zz-ui, theme, chrome, clippy]
timestamp: 2026-08-14T00:00:00Z
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
`examples/ui-showcase` renders the same widgets as a browsable gallery.

# Rules

1. Every application-chrome color comes from a `cx.theme()` root or a `Colorize` derivation of one.
   The palette is seven roots (`background`, `foreground`, `border`, `success`, `warning`, `danger`,
   `scrim`) in `crates/zz-ui/src/widget/foundation/theme_color.rs`; panels, hover fills, muted text, and
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
5. **Selection and list highlights derive from a palette root.** A highlighted menu, dropdown, or
   chooser row is a flat `background.raised(2)` fill
   (`crates/zz-ui/src/widget/select/state.rs::render_row`), so it follows the window's base plane.
   `list::ListItem` is the deliberate exception: its highlight is a `foreground.wash()` fill under a
   solid `foreground` outline (`crates/zz-ui/src/widget/list/mod.rs`). That outlined look belongs to
   lists only: `select` builds its own rows rather than reusing `ListItem`, because reusing it is
   what made dropdown rows read as outlined boxes.
6. Use one translucent signal instead of competing fills. Sidebar pointer hover, keyboard selection,
   mux focus, and clickable native status windows use `workspace_row_highlight`, a
   `background.washed(2)` tint that preserves the desktop blur. Tree fills keep a 1px vertical inset,
   so adjacent rounded rows never merge into one slab. The final Add host action is the exception:
   its row stays unpainted and only its label moves from muted to foreground on hover.

# Control density

Settings uses 13px primary row labels, so `Input::small()` renders its editable value at the same
13px rather than GPUI's 14px `text_sm`; `NumberInput::small()` inherits that field treatment. The
shared dialog surface is similarly compact: 400px default width, 12px gutters, 13px title, 12px
description, and Small action buttons. Explicitly sized content dialogs such as the attachment
preview retain their own width.

**A toast is that same surface.** `widget/overlay/notification.rs` imports the width, gutter and two
text sizes from `widget/overlay/dialog.rs` rather than restating them, so the two things that
interrupt a window . the modal it must answer and the toast it may ignore . speak at one size. Its
icon is a flex child aligned to the first line, not an absolutely positioned glyph the copy column
pads around.

**Every toast stacks on the workspace window**, whichever window raised it. Only `AppShell` mounts
`Root::render_notification_layer`; the Settings window mounts the dialog layer alone, because a
toast stacked in the corner of a window opened to flip one switch reads as a second, competing
chrome and disappears with that window. `crates/zz/src/window/toast.rs` holds the workspace
`WindowHandle<Root>` in a global (named once, as that window opens) and `toast::push` pushes
through it; Settings . config writes, saves, imports, the About page's copy button . calls that
instead of `window.push_notification`, which is why none of those paths carry a `&mut Window` they
would otherwise need. Views already inside the workspace window keep calling
`window.push_notification`: it reaches the same stack.

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
an optional paired `chrome-preset`, and the `chrome-*` overrides in `zz/config`, never from the
terminal.** The mode follows the OS appearance unless `theme-mode` pins one. There is no knob that
derives chrome from terminal colors, and a Ghostty palette cannot repaint the window.

`crates/zz/src/theme.rs` is small on purpose. It holds the latest immutable
`TerminalAppearance` and `AppearanceProvenance` as GPUI globals (used by settings badges and the
detach action, not to color anything). `apply_zz_overrides` layers these values over the zz-ui base:

- the active light or dark variant of `chrome-preset`, when selected;
- the six optional `chrome-*` palette roots from `zz/config`, written over the preset so every
  elevation, hover, and focus ring derived from them at paint time follows the user's roots;
- `mono_font_family` from the terminal's resolved primary family, so Agent Markdown and code blocks
  match the terminal typeface;
- `theme.radius` from `widget-corner-radius`, so one radius reaches every widget and survives a
  light/dark switch.

Icon-only chrome controls use `Button::compact_icon`: a 24px hover surface around a Small 14px
glyph with a 0.5px downward optical adjustment. The titlebar, sidebar row actions, browser pane,
and Agent pane share this constructor, which fixes padding and icon scale in one place.

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

Pill is therefore a category a widget declares, never somewhere a number arrives. A switch track and
thumb, a status dot, an avatar, a scrollbar thumb call `rounded_full()`, which is exempt from the
curve and resolves to exactly half at every setting including zero. This is the separation Radix
Themes draws between its radius scale and `--radius-full`, and that SwiftUI spells `.fixed(.infinity)`
. "as round as it could be". Widgets otherwise just call `.rounded(cx.theme().radius)`; a per-widget
cap or threshold is the wrong tool and was twice removed from this codebase.

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
the last recorded OS mode. The previews and selected `chrome-preset` use the same paired variants.

Settings maps each `AppearanceSource` tier to one badge: `Default` → “Default,” `ThemeFile` → “From
theme,” `Ghostty` → “From Ghostty,” `Override` → “Overridden.” Those badges describe *terminal*
appearance provenance; chrome colors carry the client-local `Default`/`Overridden` provenance
instead, with “Preset” shown when an otherwise-unset root inherits from the selected family.

`ThemeColor` is seven roots and every other color is derived through `Colorize` at paint time, so
overriding a root needs no parallel token table kept in step. Views read
`cx.theme()` and never receive a copied palette or local color literals.
Chrome tint is composited once per visible surface, and the sidebar titlebar inherits the sidebar
shell tint instead of repainting it. App-owned pane roots use `theme::app_pane_background` and stay
opaque. The terminal root uses the same opaque base, then paints Ghostty `background-opacity` as a
terminal-color tint inside the pane. Native blur remains confined to visible chrome planes.

# Related

- `crates/zz-ui/UPSTREAM.md` . the fork's source revision and per-module record of what was ported,
  trimmed, or rewritten
- [GPUI revision pin](/references/gpui-revision.md) . the pinned GPUI sources
- [`app` crate](/crates/zz.md) . the GPUI client governed by these conventions
- [Terminal appearance](/terminal/appearance.md) . the independent terminal color model
