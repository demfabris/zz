---
type: Subsystem
title: Terminal appearance and color model
description: The renderer-neutral appearance model, the native zz/config override resolver, per-key provenance, the client-side Ghostty import loader, and embedded Ghostty/X11 colors.
resource: crates/zz-terminal/src/appearance.rs
tags: [appearance, color, ghostty, palette, x11-rgb, config]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

`appearance.rs` owns the renderer-neutral appearance for terminal panes: colors, palette, fonts, padding,
cursor policy, and the loader that resolves an appearance-only subset of Ghostty configuration. It contains
no GPUI types. The daemon resolves appearance as built-in defaults plus `zz/config` overrides at
startup and on explicit reload; it never reads a Ghostty *config*, though it does open the Ghostty
*theme file* a `theme` override names. The Ghostty config loader runs **client-side**, during the
explicit import flow
([Application configuration](/configuration/app-config.md)), which snapshots donor values into
`zz/config`. Every client sees the same daemon-resolved values (the
[rendering-parity](/terminal/rendering-parity.md) app converts it into a GPUI
`Font` and paint colors). The module also embeds `x11-rgb.txt`, the Ghostty/X.Org named-color table, so
`red`, `rebeccapurple`, etc. resolve identically on every host. The wire protocol carries a
complete per-key provenance map beside the appearance.

This is a **terminal-only** system. Application chrome has its own light/dark palette in
[zz-ui](/configuration/ui-conventions.md) and does not derive from these colors; only
`mono_font_family` still crosses the boundary, and it is not chroma. `background-opacity` remains
pane-local too. The renderer paints the terminal color at that alpha over an opaque app-pane base,
so `1` shows the configured terminal background and lower values mix toward the app surface. Window
blur does not enter terminal pixels.

# Model

`TerminalAppearance` is the wire-safe struct applied to every new libghostty terminal (via
`apply_terminal_appearance`, which pushes fg/bg/cursor + the full 256-entry palette before any PTY output):

| Field group | Fields |
| --- | --- |
| Fonts | regular/bold/italic/bold-italic family stacks, `font_size_points: f32`, `font_weight: u16`, `font_features: Vec<FontFeature>`, `font_synthetic_style`, `font_thicken` + strength, `cell_height_adjustment` |
| Padding | `padding_left/right/top/bottom: f32` (independent per edge) |
| Core colors | `foreground`, `background`, `cursor_color` (`Color` = RGB) |
| Overlay colors | `selection_foreground` (`Color`), `selection_background`, `search_match_color`, `search_current_color`, `link_color`, `copy_cursor_color` (`AppearanceColor` = RGBA) |
| Palette | `palette: TerminalPalette` (exactly `[Color; 256]`, tuple-serialized) |
| Policy | `minimum_contrast: f32`, `cursor_blink_policy` (Off/On/Terminal), `cursor_blink_interval_ms`, `rounded_selection`, `background_opacity`, `color_scheme` (Light/Dark) |

Supporting types: `AppearanceColor` (compact RGBA), `FontFeature` (`[u8;4]` tag + `u32` value),
`FontSyntheticStyle` (independent bold / italic / bold-italic permissions), `CellHeightAdjustment`
(None / Pixels / Percent), `CursorBlinkPolicy`, `TerminalColorScheme`.
`validate()` rejects non-finite / negative / oversized metrics and out-of-range counts before values reach
GPUI; `stable_hash()` gives a deterministic signature for renderer caches. `default_palette()` builds the
standard xterm layout: 16 ANSI colors, the 6×6×6 color cube, then 24 grays.

`AppearanceProvenance` maps every key in `AppearanceConfigKey::ALL` to one
`AppearanceSource::{Default, ThemeFile, Ghostty, Override}`, and `validate()` rejects a wire payload
that is missing any of them. The map carries one `Palette` entry for the whole 256-color palette, not
256 entries. `AppearanceLoad` carries the resolved appearance, this map, the `root` it loaded from,
the `theme_path` a `theme` directive selected, bounded diagnostics, supported/unsupported/invalid
counters, and `fatal` . set when a requested theme could not be resolved or parsed.

# Configuration loading

Discovery order (first existing file is the root): `$XDG_CONFIG_HOME/ghostty/config.ghostty`,
`.../config`, `$HOME/.config/ghostty/config.ghostty`, `.../config`. `config-file` includes resolve
relative to their containing file and load after it (matching Ghostty precedence); optional `?path`
includes are supported. `ConfigLoader` enforces bounded include depth (`MAX_CONFIG_DEPTH`), total bytes
(`MAX_CONFIG_BYTES` = 1 MiB), and retained diagnostics; repeated includes are ignored after their canonical
path loads and ancestry cycles produce a diagnostic rather than recursion.

zz reads this file for fonts, padding, cursor policy, explicitly written color keys, and `theme`. A
`theme` directive is resolved first, from the standard Ghostty theme directories, and its values land
at the `ThemeFile` tier; the config's own explicit keys then replace them at the `Ghostty` tier.
Adaptive `light:name,dark:name` values pick by the requested color scheme, and an unresolvable name
marks the load `fatal`.

| Key group | Keys |
| --- | --- |
| Ghostty (supported) | `theme`, `font-family` plus `-bold` / `-italic` / `-bold-italic` stacks, `font-size`, `font-feature`, `font-synthetic-style`, `font-thicken`, `font-thicken-strength`, `adjust-cell-height`, `window-padding-x/y`, `foreground`, `background`, `cursor-color`, `selection-foreground`, `selection-background`, `palette`, `minimum-contrast`, `cursor-style`, `cursor-style-blink`, `background-opacity`, `config-file` |
| zz/config-native extensions (Ghostty compatibility accepted) | `zz-font-weight`, `zz-cursor-blink-interval-ms`, `zz-search-match-color`, `zz-search-current-color`, `zz-link-color`, `zz-copy-cursor-color`, `zz-rounded-selection` |

Ghostty's `background-blur` is unsupported and ignored. zz's separate `window-background-blur`
setting applies to application chrome. Ghostty contributes the terminal background color and its
tint strength inside the opaque pane.

Each entry produces an `AppearanceConfigDiagnostic` with a disposition (`Applied`, `Included`,
`Unsupported`, `Invalid`, `NoOp`). Unsupported keys never fail startup; invalid values for supported
keys warn and keep the previous valid value. An empty value resets a key to its built-in default.
The default family stack leaves standard OpenType features enabled; no implicit `-liga` or `-calt`
entries are injected. Style-specific stacks inherit the regular stack when empty, synthetic styles
default to all three combinations, `font-thicken` defaults off with strength 255, and cell height
defaults to the font's natural metrics with no adjustment.

# zz/config override overlay

The app recognizes the native appearance subset in [`zz/config`](/configuration/app-config.md) but
does not parse values. It sends ordered raw entries through `SetConfigOverrides`; the
daemon stores the latest set and calls `apply_appearance_overrides` over a fresh defaults base
(`AppearanceLoad::defaults_for`). Resolution is:

```text
built-in default < theme file named by a `theme` override < the rest of the zz/config override set
```

`theme` is applied out of file order, deliberately: `apply_appearance_overrides` scans the entries
for it first, loads the named theme at the `ThemeFile` tier, and only then walks every non-theme
entry in file order at the `Override` tier. That makes the theme a base the rest of the file writes
over, rather than a late entry that would clobber explicit colors above it. A key the daemon does not
recognize is dropped with `ignored unsupported configuration override key`.

The overlay otherwise uses the same `ConfigLoader::apply` match and parsers as file loading. Repeated
`palette`, each face-specific `font-family*` stack, and `font-feature` entries retain file order.
`config-file` is rejected in the override pass, so a client cannot turn the wire message into
daemon-side arbitrary file inclusion; a resolved theme file is still loaded with includes followed,
since a theme may legitimately be split. An unresolvable theme name produces a diagnostic and marks
the load `fatal`, but the daemon applies the result regardless: the rest of the override set is
already resolved, and refusing it would strand the user on stale colors.

An empty override vector is a real update: resolving from a fresh base removes the previous overlay
and restores built-in defaults. The daemon repeats this fresh-base-plus-overlay sequence
when it handles `reload-config` and whenever the client reports a new OS color scheme, preventing
GUI choices from being reverted. Successful sets produce one daemon diagnostic line containing the
entry count and publish `AppearanceChanged` when the effective appearance or provenance changes.

The eight `zz-*` spellings are owned by this native `zz/config` overlay, not by Ghostty. The
Ghostty-file parser accepts them too . one `ConfigLoader::apply` match serves both files . which is
what lets an import carry a user's existing `zz-*` lines across.

The app's explicit import reverse-serializes every key the donor Ghostty config set into
`zz/config`. It writes exact RGB/RGBA colors, face-specific font
stacks, features, synthetic/thickening policy, geometry, cursor policy, and the native `zz-*` keys.
`theme` is the one key it emits nothing for: a resolved appearance carries no theme name, so whatever
the donor's theme contributed imports as concrete colors under their own keys.
Font families, features, and palette start with empty resets;
palette then emits only entries that differ from `default_palette()`. Applying those entries over
`AppearanceLoad::defaults_for` therefore reproduces the donor appearance with the Ghostty
root gone, which is the runtime model: after import, the Ghostty file is never read again
until the user re-imports.

# What reaches application chrome

Only one value crosses from terminal appearance into application chrome, and it is not a color:

| Value | Why it crosses |
| --- | --- |
| `mono_font_family` | Agent Markdown, tool previews, and code blocks use the same primary family the terminal resolved, so code reads in one typeface across the window. |

Everything else, including `background_opacity`, stays at the terminal paint boundary. Application
backgrounds, foregrounds, accents, and semantic families come from zz-ui's own light/dark palettes
and follow OS appearance. `crates/zz/src/theme.rs` holds the latest immutable `TerminalAppearance`
as a GPUI global; it derives no chrome color or alpha from it. The client consumes appearance
provenance while resolving daemon font stacks against fonts installed on the rendering machine, but
does not retain provenance as a global. Terminal panes read the localized appearance directly.

The client installs the initial value from `ServerHello`, then replaces it on every
`AppearanceChanged` event, published by override changes, explicit daemon config reload, and
`SetColorScheme` round-trips. Each host connection caches the latest raw appearance and provenance,
so switching hosts cannot resurrect stale handshake typography. OS appearance syncs re-apply the
font crossover after zz-ui refreshes its light/dark base; every window must route those syncs through
`theme::sync_system_appearance`.

# Color parsing and the X11 table

`parse_rgb` / `parse_rgba` first look up the trimmed, lowercased value in the embedded X11 table, then fall
back to `csscolorparser` (hex, `rgb()`, etc.); `parse_rgb` rejects alpha. The table is built once
(`OnceLock`) from `include_str!("x11-rgb.txt")`; each line is `R G B name`. Palette entries accept
`index=color` with decimal / `0x` / `0o` / `0b` indices.

`x11-rgb.txt` is copied verbatim from Ghostty's `src/terminal/res/rgb.txt` at upstream commit
`cf60af281bd7559a819aa25372cef01d623b8c5a`, sourced from the X.Org `rgb` project under the MIT/X11 license
(provenance in `third_party/ghostty-reference/UPSTREAM.md`). See
[ghostty-color-reference](/references/ghostty-color-reference.md).

# Key files

| File | Role |
| --- | --- |
| `src/appearance.rs` | Model, validation, `stable_hash`, Ghostty loader, color parsing, default palette. |
| `src/x11-rgb.txt` | Embedded Ghostty/X11 named-color data. |

# Related

- Applied to [`libghostty-vt`](/terminal/libghostty-vt.md) terminals by [`zz-terminal`](/crates/zz-terminal.md).
- Colors and fonts drive [rendering-parity](/terminal/rendering-parity.md); overlay colors are used by [interaction](/terminal/interaction.md).
- The resolved appearance is carried in `ServerHello` over the [wire protocol](/protocol/wire-protocol.md).
- Application chrome is a separate palette governed by the [UI design conventions](/configuration/ui-conventions.md).
- Named-color provenance: [ghostty-color-reference](/references/ghostty-color-reference.md).
