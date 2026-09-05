---
type: Configuration
title: Application configuration search paths
description: Platform-aware discovery, bounded parsing, ACP launch settings, settings editing, and one-shot donor import for client-local knobs plus ordered daemon-owned overrides.
resource: crates/zz/src/config/mod.rs
tags:
- configuration
- gpui
- window
- appearance
- mux
timestamp: 2026-09-05T00:00:00-03:00
---

# Overview

The GUI process loads the first existing `zz/config` file from the user's platform configuration
roots. `crates/zz/src/config/mod.rs` resolves the candidates when `run_app` enters the GPUI application
closure, parses named client-local behavior/layout/diagnostic/theme/browser knobs plus
chrome-color entries into typed
`AppConfig` and `BrowserConfig` values, parses the one app-owned ACP key into `AgentConfig`,
collects repeatable `chrome-keybind`/`chrome-unbind` entries for the client-local keymap, and collects the supported
daemon-owned appearance and mux entries as ordered raw `(key, value)` pairs. Client-reserved
`host-<name> = <uri>` entries ([fleet attach](/designs/fleet-attach.md)) are matched before any
of that: validated via `zz_daemon::Endpoint::parse`, published through a dedicated `FleetHosts`
global (not `AppConfig`, which stays `Copy`), and never forwarded to any daemon.
Three surfaces write these lines and nothing else: `zz fleet add <name> <ssh-destination>` from the
CLI, the sidebar's final **Add host** row, and the inline form in **Settings › Hosts**. The
two GUI fields share the same `parse_add_host` validation and name the entry after the destination's
host component. The underlying writer replaces an existing `host-<name>` value in place while
preserving every other byte, comments included; the GUI fields reject a duplicate name before they
call it. `zz fleet list` prints name and endpoint; `zz fleet remove <name>` deletes every matching
host line, as does a remote row's **Close host** . which additionally republishes `FleetHosts`
(`config::remove_fleet_host_live`) so the running fleet drops the machine immediately. There is no bootstrap step, no key pinning, and no daemon-side setup . ssh already owns
identity. These local schema entries retain `Default`/`Override` provenance. Daemon-owned value
grammar deliberately stays in the zz-terminal appearance loader or
mux `set-option` engine.

The GUI polls candidate paths every 500ms (path, mtime, and size) and live-applies edits, file
appearance, precedence changes, and deletion. On connect and every poll change it sends the complete
ordered daemon-owned set through `SetConfigOverrides`, including an empty set after the
last override is removed. The daemon never reads `zz/config`; it stores the latest appearance and
mux subsets, layers appearance above built-in defaults (it reads no donor appearance config of its
own; the one external file it opens is the Ghostty theme a `theme` override names, see
[Terminal appearance](/terminal/appearance.md)), and
dispatches mux values through the existing global `set-option` grammar. Appearance overrides replay
after `reload-config` and system color-scheme changes; mux overrides replay after every
`zz/mux.conf` load so a reload cannot revert a GUI choice.

**zz's configuration is authoritative; external configs are read only during an explicit import.**
A one-time first-run prompt (shown only when a Ghostty or tmux config exists; remembered via a
marker file in the platform data directory) runs the combined import in
`crates/zz/src/config/import.rs`. Multiplexer additionally imports tmux (`~/.tmux.conf`, then the
XDG locations) verbatim into `zz/mux.conf`, the only mux file the daemon sources. Re-importing tmux
syncs that file again. Neither donor file is ever modified.

# Discovery and loading

| Platform | Search order |
| --- | --- |
| Linux and other Unix | `$XDG_CONFIG_HOME/zz/config`, then `$HOME/.config/zz/config` |
| macOS | `$XDG_CONFIG_HOME/zz/config`, `$HOME/.config/zz/config`, then `$HOME/Library/Application Support/zz/config` |
| Windows | `%XDG_CONFIG_HOME%\zz\config`, `%APPDATA%\zz\config`, `%LOCALAPPDATA%\zz\config`, `%USERPROFILE%\.config\zz\config`, then `%HOME%\.config\zz\config` |

Only absolute configuration roots are accepted, as required for `XDG_CONFIG_HOME`; duplicate paths
are removed while retaining precedence. A missing file or unavailable configuration root silently
selects built-in defaults. Other I/O or UTF-8 errors warn and select defaults. Reads are capped at
64 KiB before parsing so an accidentally large file cannot create unbounded configuration work.

When a settings edit must create the file, it uses `$XDG_CONFIG_HOME/zz/config` when the root is
absolute, otherwise `$HOME/.config/zz/config`, creating parent directories as needed. Once any
candidate exists, edits continue to target the first existing file selected by normal discovery.

# Schema

## Repeatable chrome bindings

`chrome-keybind = <table>:<key>=<action>` replaces or adds one binding, and
`chrome-unbind = <table>:<key>` removes one. The allowed tables are `ui`, `sidebar`, `browser`, and
`terminal`; action names come from `zz_client::ChromeAction`. Keys use the chrome grammar from
`zz-client`, including `D-` for Command/Super and `S-` for Shift. These directives may appear more
than once, preserve file order, and stay client-side. They are separate from the 35 named scalar
knobs below and from the daemon-owned tmux tables in `zz/mux.conf`.

## Client-local keys

The client-local schema includes these scalar settings and chrome colors.

| Key | Default | Valid range | Consumer |
| --- | ---: | --- | --- |
| `use-system-titlebar` | `false` | `true` or `false` | On Linux, request a desktop-owned titlebar and window border instead of client-side window decorations |
| `window-corner-radius` | `13.5` | `0..=32` | The app-drawn window frame's corner, visible only under Linux client-side decorations |
| `window-background-blur` | `false` | `true` or `false` | Whether the desktop shows through the window chrome, blurred |
| `animations` | `true` | `true` or `false` | Whether interface transitions, loading indicators, scrollbar fades, and animated UI images move |
| `tray` | `true` | `true` or `false` | Whether zz puts an icon in the system tray (macOS status item, Windows notification icon, Linux StatusNotifierItem), toggling the window on click with a menu carrying Show/Hide and Quit. A live tray turns the close button into hide-to-tray on every platform (macOS hides the app); without a tray host (bare GNOME) close quits as before. Surfaced in Settings under Advanced; read once at startup |
| `show-fps` | `false` | `true` or `false` | Whether the titlebar GPUI meter **and** each browser pane's CEF meter are shown |
| `quit-daemon-on-exit` | `false` | `true` or `false` | Whether quitting the app stops the daemon even while sessions are live |
| `auto-restart-stale-daemon` | `false` | `true` or `false` | Whether a protocol-mismatched local daemon is terminated and replaced on connect. Off by default because it ends every running session |
| `check-for-updates` | `true` | `true` or `false` | Whether the GUI fetches the GitHub release list ten seconds after launch and daily after that, offering a newer release as a toast. The channel follows the running build (a `-beta.N` version takes prereleases). One anonymous request; nothing else is sent. Release builds only unless `ZZ_UPDATE_CHECK=1`; `ZZ_UPDATE_CHECK=0` silences every build. Surfaced in Settings under About |
| `status-show-session` | `true` | `true` or `false` | Whether the native desktop status bar shows the attached session chip. Clicking the chip focuses the session picker in the sidebar |
| `status-badges` | `true` | `true` or `false` | Whether window entries show bell, activity, and Agent markers. The window strip remains visible when this is off |
| `status-align` | `left` | `left` or `center` | Align the native window strip at the left edge or center it in the available status-bar space |
| `status-agents` | `true` | `true` or `false` | Whether the bar may show the count of non-dead Agent panes in the attached session. The item stays hidden at zero |
| `status-host` | `true` | `true` or `false` | Whether the bar shows the host name while attached to a remote host. Local attachment has no host item |
| `status-update` | `true` | `true` or `false` | Whether an available release appears in the bar with its version and an install action. `check-for-updates` separately controls the release check |
| `status-clock` | `24-hour` | `24-hour`, `12-hour`, `time-date`, or `off` | Select the desktop clock format. It has no seconds; `time-date` renders 24-hour time plus abbreviated month and day |
| `experimental-agent-pane` | `false` | `true` or `false` | Whether new Agent panes can be created at all . picker row, palette completion, and the daemon's `select-pane-kind agent` |
| `experimental-editor-pane` | `false` | `true` or `false` | Whether new Editor panes can be created at all . picker row, palette completion, and the daemon's `select-pane-kind editor` |
| `pane-gaps` | `false` | `true` or `false` | Whether panes use the gapped border, radius, surface ring, and divider treatment |
| `pane-inactive-opacity` | `0.7` | `0..=1` | Retained strength of inactive pane content and chrome; `1` disables dimming |
| `pane-margin` | `6` | `0..=32` | Inset around each pane, on every platform; applies only with `pane-gaps` |
| `pane-corner-radius` | `13.5` | `0..=32` | All four corners of every pane, on every platform; applies only with `pane-gaps` |
| `pane-border-width` | `0.5` | `0..=8` | Border width while pane gaps are enabled; `0` disables the border |
| `widget-corner-radius` | `6` | `0..=24` | The corner every zz-ui widget turns . buttons, inputs, tags, menus, dialogs |
| `shadow-strength` | `1` | `0..=1` | Multiplier for shadows around controls and gapped panes; `0` turns them off |
| `editor-font-size` | `13` | `8..=32` | Buffer text size in editor panes, in pixels |
| `editor-line-numbers` | `true` | `true` or `false` | Whether editor panes draw the line-number rail |
| `editor-relative-line-numbers` | `true` | `true` or `false` | Number the rail by distance from the cursor line, which keeps its absolute number |
| `editor-soft-wrap` | `true` | `true` or `false` | Wrap long lines at the pane edge instead of scrolling horizontally |
| `editor-vim-mode` | `true` | `true` or `false` | Modal vim editing in editor panes: normal, insert, visual and visual-line modes |
| `browser-element-selector-hotkey` | `cmd-shift-c` on macOS; `ctrl-shift-c` elsewhere | One GPUI keystroke containing Control, Alt, Command/Super, or Function; Shift is optional | Toggle the element selector while a Browser pane owns the Browser key context |
| `browser-search-provider` | `google` | `google`, `duckduckgo`, `brave` | Where a Browser pane's address bar sends an entry that is not an address |
| `browser-egress` | `true` | `true` or `false` | Whether a Browser pane opened while attached to a remote ssh host routes its traffic through that host; panes already open keep the route they were created with |
| `theme-mode` | `system` | `system`, `light`, `dark` | Follow the OS appearance, or pin one mode |
| `app-icon` | `automatic` | `automatic`, `light`, `dark` | Which render of `assets/zz.icon` the macOS Dock tile wears; `automatic` defers to the bundle's compiled icon when packaged (so tinted/clear dock styles work) and follows the OS appearance in bare builds, independently of `theme-mode` |
| `chrome-preset` | unset | `tokyo-night`, `catppuccin`, `gruvbox`, `nord`, `breeze`, `adwaita`, `ubuntu`, `rose-pine`, `ayu`, `solarized`, `macos-classic` | Select a paired light/dark chrome family; the active variant follows the effective `theme-mode` |
| `chrome-background` | unset | `#rgb`, `#rrggbb`, `#rrggbbaa` | zz-ui's `ThemeColor::background` . the window's base plane |
| `chrome-foreground` | unset | same | `ThemeColor::foreground` . default text, and the source of muted text, rings, links |
| `chrome-border` | unset | same | `ThemeColor::border` . every edge |
| `chrome-success` | unset | same | `ThemeColor::success` |
| `chrome-warning` | unset | same | `ThemeColor::warning` |
| `chrome-danger` | unset | same | `ThemeColor::danger` |

The status settings project directly into `zz_client::StatusBarSettings` and apply through the
normal watched-config refresh. The clock never shows seconds. `AppShell` aligns its first wake to
the next minute boundary, then requests one redraw per minute while the bar and clock are visible;
`time-date` renders `%H:%M · %b %d`.

`widget-corner-radius`, `shadow-strength`, and the theme keys land on the **zz-ui theme** rather than being read
per-frame by a renderer: `zz::theme::apply_zz_overrides` pushes them onto the `Theme` global, and
every widget already reads from there, so no component is plumbed individually. Because the preset
and overrides are reapplied on every theme rebuild, they survive a light/dark switch, the same
treatment `mono_font_family` gets. `widget-corner-radius` is distinct
from `pane-corner-radius`, which is pane *geometry* and is resolved per corner in
`config::pane_content_radii`.

Every rounded surface is a **squircle** . iOS-style continuous corners, set once as the GPUI window
default (`theme::CORNER_SMOOTHING`) rather than plumbed per widget. This is not configurable: the
retired `corner-shape` key offered a `round` circular arc alongside it and produced two nearly
identical windows. Shapes whose radius reaches half their shorter side . switch tracks and thumbs,
status dots, avatars, scrollbar thumbs . stay circular regardless: a pill or a circle is a shape in
its own right, and smoothing one turns it into a lozenge. The squircle therefore reads on rounded
rectangles, and the larger `widget-corner-radius` is, the more visible it gets (the corner departs
from a circular arc by roughly a fifth of the radius, so it is sub-pixel below about 4px).

`browser-element-selector-hotkey` lives in a separate `BrowserConfig` GPUI global so `AppConfig`
can remain `Copy`. The parser canonicalizes GPUI keystroke syntax and rejects keys without a
non-Shift modifier; an invalid present line keeps the built-in value but retains `Override`
provenance so Settings can still expose Reset. Browser key installation observes that global, so a
watched config edit applies without restarting.

`browser-search-provider` shares that global, and is read at submit time rather than cached per
pane, so an edit reaches every open Browser pane on the next Enter. The engine set is
`zz_browser::SearchProvider`, which owns the query endpoints; the address bar's decision between
navigating and searching is `zz_browser::resolve_address` (see
[input translation](/browser/input-translation.md)).

`animations = false` sets GPUI's reduced-motion state. GPUI transitions, spinners, switches,
dialogs, notifications, and animated UI images settle on a static frame; the custom scrollbar holds
at full opacity and then disappears without a fade. iOS also honors UIKit Reduce Motion, even when
the file enables animations. Cursor blinking and Chromium page animation are content behavior and
keep their own controls. Desktop GPUI does not currently publish the operating system's motion
preference, so macOS, Linux, and Windows follow this key directly.

## Chrome theming

The six `chrome-*` keys overwrite zz-ui's **palette roots** (`ThemeColor`, seven fields). Every other
color the UI paints (elevations, hover and pressed fills, muted text, focus rings, status washes)
is derived from those roots by `Colorize` at paint time, so setting a root recolors everything built
on it with no further plumbing. That is also why there are six knobs and not a table: `scrim` is
omitted because it is black in both modes and only its alpha is meaningful, and a larger table could
disagree with itself.

Resolution is `zz-ui base for the effective mode < chrome-preset variant < explicit chrome-*`.
An unset root is therefore **inherited** from the active preset variant or, with no preset, the
zz-ui base. `AppConfig` stores `Option<Hsla>` per root rather than a default color, and Settings
labels a preset-inherited root accordingly.

The built-ins in `zz::theme::CHROME_PRESETS` pair light and dark variants under one stable family
ID. Applying one atomically removes the six explicit roots and writes `chrome-preset`; it does not
change `theme-mode`. Switching System/Light/Dark, or an OS appearance change while on System, picks
the matching variant. A subsequent per-root edit is an override on both modes, and Reset returns
that root to the active preset variant. Existing configs containing only `chrome-*` remain valid as
fixed overrides.

Chrome chroma comes from `zz/config` alone, never from the terminal's palette: there is no key that
makes application chrome follow terminal colors.

`show-fps` is a single switch over two independent pipelines. `diagnostics/fps.rs`'s `AppFpsMeter` samples
GPUI's per-window `FrameTimingCollector` for the titlebar badge, while each `BrowserView` keeps its
own `FrameRateSampler` counting only fresh OSR images it consumes. They are actual rates
rather than limits and **can legitimately disagree**: demand-driven GPUI can draw slowly while idle,
a static CEF page can publish zero frames, and animated browser content can run under CEF's separate
focus ceiling. One switch is the whole surface: `show-app-fps` and `show-browser-fps` are **not**
aliases for it, and leaving either in the file produces the normal `unsupported key` diagnostic.

`quit-daemon-on-exit` changes what app quit sends: `true` issues `kill-server`, `false` (the default)
issues `detach()`. Leaving it off is what preserves the multiplexer guarantee that live sessions
outlive the window; see [session persistence](/concepts/session-persistence.md) for the daemon's own
exit rule.

The editor pane is additionally **compiled out by default**: the `editor-pane` cargo feature on the
app crate (mirrored by `editor` on `zz-ui`) gates the implementation behind a facade module
(`crates/zz/src/editor/mod.rs`) whose stub keeps every call site compiling. In a build without the
feature the matching config flag below reads false regardless of the file, the Settings switch and
Editor page disappear, and a pane handed over by a featureful daemon renders a labelled placeholder.
Dev builds opt back in with `ZZ_CARGO_FEATURES=editor-pane just run mac` (or `--features` on cargo
directly). The `agent-pane` feature works the same way and keeps the same facade
(`crates/zz/src/agent/mod.rs`), but it is **in the default set** since 0.2.0-beta.2, so
`experimental-agent-pane` is the only gate a stock build has.

`experimental-agent-pane` and `experimental-editor-pane` are **hard capability gates** with two
consumers of the same config entry. Client-side they gate the pane picker rows (and the `a`/`e`
hotkeys) in `pane::picker::choices` plus the palette's `select-pane-kind` completions.
Daemon-side the entries are forwarded through the config-override push (they are also
`MuxOptionKey`s), and the mux engine rejects `select-pane-kind … agent|editor` while the flag is
off . closing the palette, CLI, and `mux.conf` `bind-key` routes. The one remaining power-user
escape hatch is deliberate: `set-option -g experimental-agent-pane on` (in `mux.conf` or at
runtime) flips the daemon gate directly, though the picker row follows the `zz/config` entry, not
the mux option. Panes that already exist keep rendering on reattach when the switch is off;
flipping it never destroys pane state.

`use-system-titlebar` maps to GPUI server-side decorations for the main and Settings windows on
Linux. It is applied at window creation and re-requested for every open window after a watched
configuration change, so switching modes does not require a restart. KDE supports the request on
both Wayland and X11 in the pinned GPUI backend. A Wayland compositor without server-decoration
support keeps client-side decorations; GPUI reports the negotiated mode and the app-owned rounded
frame follows that result. Returning to server decorations also clears the client frame inset.
The in-app navigation/status strip remains available, but its Linux window controls disappear when
the desktop owns them.

`window-background-blur` requests `WindowBackgroundAppearance::Blurred` for both newly created and
already-open windows. `window/background.rs` maps that request to a transparent GPUI surface plus
WindowServer's background-blur radius on macOS, retains GPUI's optional KDE
`org_kde_kwin_blur_manager` protocol on Wayland, and sets KDE's
`_KDE_NET_WM_BLUR_BEHIND_REGION` property on X11. The X11 region follows window size, scale, and
exposed rounded corners so resize and tiling do not leave a stale rectangular effect.

**The blur is gated on the compositor actually blurring.** A translucent chrome over an unblurred
desktop is just a hole, so `window/background.rs` probes once at startup
(`detect_compositor_support`): macOS and Windows always pass; a Wayland session passes when the
registry advertises `org_kde_kwin_blur_manager` (KWin ≤ 6.3) or its staging successor
`ext_background_effect_manager_v1` (Hyprland, KWin master, niri); an X11 session passes when the
root window's property list announces `_KDE_NET_WM_BLUR_BEHIND_REGION` . KWin's blur effect posts
and retracts that atom with the effect itself, the same signal
`KWindowEffects::isEffectAvailable` reads, so it means "blur is on", not just "KWin is running".
Everywhere else `theme::chrome_blur` stays false and the toggle is an honest no-op . opaque
chrome, `Opaque`/`Transparent` native request . logged at startup. picom ignores the KDE blur
property entirely (open upstream feature request), so picom setups stay in the opaque fallback by
design.

**Only window chrome reveals the compositor blur.** All platform requests remain window-wide, and
app paint decides where the backdrop appears. While blur is active, chrome uses
`BLURRED_CHROME_ALPHA` through `theme::chrome_background`. Pane roots force an opaque theme base.
The pane picker, Agent, Editor, Browser shell, waiting state, and terminal cover the native
backdrop. Browser blank, loading, error, and toolbar states share the same pane base; Chromium
pages keep their own pixels above it.

A terminal paints its resolved Ghostty background color as a tint over that opaque pane base.
`background-opacity = 1` shows the terminal color, and lower values mix it toward the app pane
color. The setting no longer exposes the desktop or chrome blur. Ghostty's `background-blur` stays
ignored; `window-background-blur` owns the native compositor request.

Each chrome region paints its tint once; 0.93 over 0.93 becomes 0.995 and hides the
backdrop. During active blur the workspace root leaves pane rectangles unpainted. The outer margin,
split gaps, and rounded corner wedges paint chrome around opaque pane interiors. The Settings
window has no workspace chrome plane and stays opaque.

Linux window geometry starts from `window-corner-radius` (default 13.5px, targeting the macOS 27
window radius . ~13.5pt tangent-circle equivalent, measured from a macOS 27 screenshot against the
12pt traffic lights) and `frame_content_corner_radius(cx)` derives the inner curve by subtracting
the frame's shared 1px border width (12.5px at the default). The key exists because Linux desktops
agree on no radius and expose no protocol to query one: users match their compositor's rounding
(Hyprland `decoration:rounding`, KWin effects) or set `0` for square, by hand. It only draws where
the frame is app-owned . tiled/maximized Linux edges remain square, and `WindowCorners::for_window`
returns no app-owned corners under server-side decorations or on macOS and Windows, where native
window shaping remains authoritative. `frame-content-corner-radius` and `pane-content-corner-radius`
are still derived, not configuration keys; leaving one in the file produces the normal
unsupported-key diagnostic.

On Wayland compositors using `ext-background-effect-v1`, GPUI sends that same exposed-corner mask as
the effect region. KWin changes pixels even where the client surface is transparent, so the region
does not extend into the client-side shadow inset. Transparent shadow and corner pixels retain the
compositor's unmodified background; tiled and square corners remain square.

`pane-gaps = false` keeps the regular workspace flush: margin, corner radius, and border width
resolve to `0`, and split nodes retain their neutral 1px hairline. The active pane colors only its
half of each adjacent separator with a washed foreground accent; nested T-junctions limit the
segment to the edge the pane touches. With `pane-gaps = true` every value applies exactly as
configured, defaults included . a 6px margin and a 13.5px radius, the window's own corner, so a
maximized grid reads as one rounded surface. There is
no second tier of gapped defaults: the number Settings shows is the number the panes use, which is
what lets the disabled Frame rows stay honest. The border uses `pane-border-width` and the theme
border color; the active pane keeps that exact width and replaces the color with a washed foreground
accent. A theme-derived half-pixel inset ring follows the pane's exact curve, matching the Settings
stack edge without painting outside the pane. Borderless gapped panes keep the ring. This treatment
is built in; the retired `pane-shadow` key produces the normal unsupported-key diagnostic.
Split divider visuals disappear in this mode because the gap is the separator, but the unchanged
16px drag target still resizes the split.

`pane-inactive-opacity` is independent of pane gaps. The default `0.7` preserves the previous 30%
fade. Terminal panes apply it to glyphs and decorations while keeping their background unchanged.
Browser panes apply it to the toolbar while keeping Chromium page pixels unchanged. Other native
pane surfaces fade toward the opaque window background. Setting it to `1` removes every inactive
pane dimming treatment.

All four frame keys render app-side and therefore apply on every platform. While the effective margin
is zero, window-exposed pane corners keep the larger of the pane radius and derived frame-content
curve; a nonzero margin detaches panes from the frame, so the pane radius alone shapes all four
corners. The margin is single-counted everywhere: the layout root insets the window edge by one
margin and each split's slot *is* the one inter-pane gap, so `pane-margin = 2` yields exactly 2px
between panes and 2px at the frame; gaps never sum to double between neighbors. The gap paints the
window's base plane (`chrome-background`) from three places: the frame inset is a border band on the
layout root rather than padding, the split slot is the divider's own fill, and each rounded pane
fills the four wedges its arc leaves bare inside its own box with a band clipped to that box. Pane
roots cover the native backdrop inside those bounds. In browser panes the toolbar owns the pane's
top arc: the Chromium surface and the overlays that replace it (error panel, empty state) keep
square top corners and only follow the pane curve at the bottom.

## App-owned Agent keys

Exactly one is left. The three adapter keys are [mux options](#daemon-owned-mux-option-keys) now,
because the daemon spawns the adapter; this one feeds pane creation, which is a client concern.

| Key | Default | Validation / effect |
| --- | --- | --- |
| `agent-working-directory` | unset | Absolute path only; overrides the donor cwd for brand-new Agent sessions |

The adapter command values may be unquoted, wrapped in matching single/double quotes, or supplied as
raw JSON when explicit arguments/environment variables are needed. zz does not log the configured
command, because a JSON environment map may contain credentials. A configured working directory takes
precedence when creating a new pane; a daemon-persisted descriptor cwd takes precedence when loading
an existing ACP session. None of the four has a row in the Settings dialog.

The daemon captures and caches the user's login-shell `PATH` before the first ACP launch,
merges in inherited entries, and injects the result only when the parsed command has no explicit
`PATH` environment entry. The capture is bounded and falls back to `/usr/libexec/path_helper` on
macOS, so a launch-agent or Finder-started daemon can resolve Homebrew and user-local executables
while raw JSON or a leading
`PATH=...` assignment remains authoritative.

## Daemon-owned appearance keys

The following keys use the same spelling and value grammar as zz-terminal's supported Ghostty
subset. The client only recognizes and transports them; it does not validate their values.

The set is `AppearanceConfigKey::ALL` in `crates/zz-terminal/src/appearance.rs`; read it there
rather than trusting a count here.

| Group | Keys |
| --- | --- |
| Theme | `theme` |
| Colors | `background`, `foreground`, `cursor-color`, `selection-foreground`, `selection-background`, `palette` |
| Fonts and geometry | `font-family` plus `-bold` / `-italic` / `-bold-italic` stacks, `font-size`, `font-feature`, `font-synthetic-style`, `font-thicken`, `font-thicken-strength` (macOS only), `adjust-cell-height`, `window-padding-x`, `window-padding-y` |
| Policy | `minimum-contrast`, `background-opacity`, `cursor-style`, `cursor-style-blink` |
| zz extensions | `zz-font-weight`, `zz-cursor-blink-interval-ms`, `zz-search-match-color`, `zz-search-current-color`, `zz-link-color`, `zz-copy-cursor-color`, `zz-rounded-selection` |

File order is preserved, including repeated `palette`, face-specific font-family, and `font-feature`
entries. `config-file` is not part of the native override surface.

`theme = NAME` is transported like every other daemon-owned key. The daemon resolves it first, ahead
of the rest of the override set: it looks `NAME` up in the standard Ghostty theme directories
(`$XDG_CONFIG_HOME/ghostty/themes`, `~/.config/ghostty/themes`, the macOS Application Support
directory, `$XDG_DATA_DIRS/ghostty/themes`, and the installed Ghostty resources), loads that file,
and tags every key it sets `theme-file`. The remaining override entries then apply on top, so an
explicit `background = …` beside a `theme` line wins. Adaptive `light:name,dark:name` values select
by the current color scheme. A name that cannot be resolved produces a daemon diagnostic and leaves
the rest of the set applied. Settings has no theme picker . `theme` is a file-only key.

The `zz-*` extension keys are **zz/config-native** daemon-owned settings. The Ghostty parser also
accepts their spellings, which is what makes a Ghostty file importable, but they are zz's keys.

## Daemon-owned mux option keys

The seventeen mux keys are also transported raw and in file order. The daemon turns each entry into
a global `set-option`, so repeated keys retain normal last-writer behavior and invalid entries
produce a daemon diagnostic without blocking later entries. `mouse` and `escape-time` joined the
config-writable roster on 2026-08-21 when Wave B2/B3 took ownership of their behavior, and
`prefix2` — the last publication-only key — joined the same day when Wave C run 2 took ownership of
second-prefix arming and `send-prefix -2`; a reload reapplies them exactly like the existing keys
(`from_config_key` routes the entry, the daemon replays it as a global `set-option` with
`Override` provenance, and unset restores the underlay).

| Key | Built-in default | Accepted value / effect |
| --- | --- | --- |
| `prefix` | `C-b` | tmux-style prefix key name; canonicalization is owned by `KeyTables::set_prefix` |
| `prefix2` | `None` | optional second prefix key (or `None`); arms the prefix table beside `prefix` and feeds `send-prefix -2`; canonicalization is owned by `KeyTables::set_prefix2` |
| `mode-keys` | `emacs` | `vi` or `emacs`; retargets native copy-mode tables |
| `history-limit` | `10000` | integer `0..=1000000`; inherited by newly created panes |
| `word-separators` | tmux punctuation set | one-line string up to 8 KiB; updates live terminal word selection |
| `copy-command` | empty | one-line command up to 8 KiB used by copy-pipe fallbacks |
| `set-clipboard` | `external` | `on`, `external`, or `off` |
| `buffer-limit` | `50` | integer `1..=2147483647`; updates automatic paste-buffer eviction |
| `synchronize-panes` | `off` | `on` or `off`; controls global synchronized input inheritance |
| `experimental-agent-pane` | `off` | flag value (`on`/`off`/`true`/`false`/…); gates `select-pane-kind agent` in the engine |
| `experimental-editor-pane` | `off` | flag value; gates `select-pane-kind editor` in the engine |
| `history-trickle` | `2000` | integer `0..=10000`; background scrollback backfill budget. `0` disables trickle and leaves scroll-driven prefetch intact |
| `agent-command` | `npx -y @agentclientprotocol/codex-acp@1.3.0` | Nonempty command string or an `AcpAgentConfig` JSON object (`{"command", "args", "env"}`), up to 4 KiB; what the daemon spawns for a Codex pane |
| `agent-claude-code-command` | `npx -y @agentclientprotocol/claude-agent-acp@0.68.0` | Same, for Claude Code panes |
| `agent-auto-approve` | `reads` | `off`, `reads` or `all` (the flag spellings `on`/`yes`/`true`/`1` still parse as `all`, `off`/`no`/`false`/`0` as `off`); `reads` answers only read-only tool kinds (`read`, `search`, `fetch`, `think`) daemon-side and sends `execute`, `edit`, `delete`, `move`, an absent kind and any unrecognised kind to the permission wizard; when the tier answers, a kinded `session/request_permission` is answered daemon-side with the agent's preferred allow option (`allow_always`, else `allow_once`) and the tool call is still published to the stream. A request with no allow option always falls through to the permission wizard |
| `mouse` | `on` (the pin builds with `-DTMUX_MOUSE=1`) | flag value; session-effective per client on the wire. zz-tui gates its outer-terminal mouse modes on it and the daemon rejects mouse input from terminal-surface clients when off; the GUI's native mouse is ungated (decision 6) |
| `escape-time` | `10` | integer milliseconds; zz-tui's escape-sequence fold timeout (`0` clamps to 1 like the pin's `tty_keys_next`) |

The three agent keys became mux options when the [Agent pane](/concepts/agent-pane.md)'s ACP runtime
moved into the daemon at wire v53 . the daemon spawns the adapter, so the daemon owns what it spawns.
They are still written in `zz/config` like any other key; the client's parser recognizes them and
partitions them into the ordered daemon set, exactly as it does for the appearance and other mux
keys. Changing one reconfigures the runtime without restarting the children already running.

# Parsing contract

- Each nonblank entry is `key = value`; surrounding whitespace is ignored.
- Full-line `#` comments are accepted. Outside single/double quotes, an inline `#` starts a comment
  only after a nonempty value and when immediately preceded by whitespace. Leading and adjacent
  hashes remain data, so `background = #112233`, the normal tmux punctuation set, and quoted hashes
  in copy commands survive transport unchanged. This bounded rule intentionally leaves an unquoted
  whitespace-prefixed hash ambiguous.
- Client-local booleans, pane geometry, and Agent values are validated in the client; later valid entries
  replace earlier values and invalid entries warn while retaining the preceding/default value.
- Daemon-owned values are retained as trimmed raw strings in file order and parsed only by the
  daemon-side appearance loader or mux command engine. Supported appearance and mux keys do not
  produce client warnings.
- The two `experimental-*-pane` flags are the one dual-consumer exception: they parse as
  client-local booleans **and** forward raw to the daemon, so the picker gate and the engine gate
  read the same entry.
- Unknown keys and malformed lines warn and are ignored.

# Provenance

Every client-local knob resolves to `(effective value, provenance)`. Provenance is `Default` when the
loaded file contains no assignment for that key and `Override` when the key is present. Presence is
recorded even when its value is invalid; the diagnostic still follows the warn-and-keep rule, so an
invalid override can display the preceding/default effective value with an `Overridden` badge. This
definition lets Reset remove a stale invalid line instead of incorrectly presenting it as absent.

Appearance provenance is resolved by the daemon and arrives with `ServerHello` and
`AppearanceChanged` as a complete per-key map. `AppearanceSource` has four tiers . `default`,
`theme-file`, `ghostty`, and `override`. Only three of them can reach a client: the daemon resolves
appearance from built-in defaults plus the override set, so it emits `default`, `theme-file` (keys
a `theme = NAME` override pulled in), and `override`. The `ghostty` tier belongs to the client-side
import loader, which reads a donor Ghostty config in-process and never publishes a provenance map.
Palette provenance is one entry for the whole palette, not 256 entries. Settings no longer renders
per-palette-entry provenance; Terminal renders the palette as one inherited/overridden group and
shows per-key provenance for the rest of its structured appearance controls.

Mux option state is also daemon-resolved. `ServerHello.mux_options` and `MuxOptionsChanged` carry a
complete ordered map of the ten effective display strings. Each value has the last-writer tier
`default`, `tmux-config`, `override`, or `runtime-command`. The `tmux-config` wire tier kept its name
for compatibility but now means "set by the sourced `zz/mux.conf`". Settings no longer mirrors that
map into option controls; Multiplexer edits `zz/mux.conf` directly.

# Comment-preserving writer

`config/mod.rs` exposes typed set/remove operations for the client-local Settings controls through
the same bounded writer. They use parser-compatible key matching and edit the **last** occurrence,
matching later-entry precedence:

- set replaces only the value bytes, preserving the assignment's key spelling, whitespace, inline
  `#` comment, line ending, and every other file byte;
- remove deletes that one complete line, so an earlier duplicate becomes effective;
- set appends a canonical `key = value` line when the key is absent;
- reads and the resulting edited file retain the same UTF-8 and 64 KiB bounds as normal loading;
- writes use a uniquely created temporary file in the config file's directory, flush it, and rename
  it over the destination atomically.

Key matching and value replacement use the same quote-aware, whitespace-prefixed `#` boundary as
the parser. A hash inside a color, palette assignment, word-separator set, or quoted command is never
mistaken for the line's preserved trailing comment.

# Import

`crates/zz/src/config/import.rs` owns the one-shot import. `import_external_config(scheme)`
discovers donors (Ghostty via `discover_ghostty_config`; tmux via `~/.tmux.conf`, then
`$XDG_CONFIG_HOME/tmux/tmux.conf`, then `~/.config/tmux/tmux.conf`), parses the Ghostty config
client-side with the zz-terminal loader, and serializes every key the donor set, directly
(`Ghostty` provenance) or through its `theme` directive (`ThemeFile`), into concrete
`zz/config` values. Theme-derived values are flattened for the current color scheme; an import is a
snapshot.

`config/mod.rs` `import_appearance_values_at` applies those values with **donor-wins replace
semantics**: scalar keys edit the last occurrence in place (preserving spelling, spacing, inline
comments, and every unrelated byte); the cumulative `palette`/`font-family*`/`font-feature` groups
remove every prior occurrence and re-append the group at end of file, led by an empty-value reset so
the result is donor-independent. Palette writes only indices that differ from the built-in palette.
Both the input and result honor the 64 KiB bound, and the write is the normal atomic writer.

The tmux config is copied to `zz/mux.conf` **verbatim** (bounded at 1 MiB), with no filtering and no
grammar translation, so bindings, status formats, and options all keep working through the daemon's
existing tmux-grammar sourcing. Mux options are deliberately *not* written into `zz/config`; they
live in `zz/mux.conf`'s `tmux-config` tier with `zz/config` overrides layered above, exactly as
before. After a successful import the client asks the daemon to `reload-config`; when no daemon is
connected the file is picked up at the next daemon startup.

# Settings view

`crates/zz/src/config/settings.rs` renders the `WorkspaceRoute::Settings` route in the main window.
`Cmd+,` on macOS or `Ctrl+,` elsewhere opens it. `SettingsSection::ALL` is **nine pages**, titled
Interface, Editor, Panes, Multiplexer, Browser, Terminal, Hosts, System, About. The sidebar labels
them as Appearance, Tools, and Advanced groups. There is deliberately no "General": the page that name described
had accumulated window chrome, a global widget metric, daemon lifecycle and a debug overlay. The
device/pairing page that sat before Terminal was deleted on 2026-08-01 along with pairing itself;
fleet hosts live in `zz/config` and can be managed from the sidebar or **Settings › Hosts**.

Each page is a column of `SettingsGroup`s (a titled run of cards) rather than a flat card list.
The group is also where a *dependency* is expressed: `SettingCard::disabled` dims a card and lays an
occluding sheet over it, which is how the Frame group shows that `pane-margin`,
`pane-corner-radius`, and `pane-border-width` are all inert while `pane-gaps` is off
(`config` forces them to `0` there). Layout holds the `pane-gaps` switch alone. Focus holds the
always-live inactive-opacity factor.

| Page | Groups |
| --- | --- |
| Interface | **Theme** (`theme-mode` as three drawn window previews, transient `UI zoom`, macOS `app-icon` as three icon tiles) · **Chroma Colors** (paired `chrome-preset`, the six `chrome-*` pickers) · **Status bar** (`status-show-session`, `status-badges`, `status-align`, `status-agents`, `status-host`, `status-update`, `status-clock`) · **Tweaks** (`animations`, `widget-corner-radius`, `shadow-strength`, `window-background-blur` as "Window blur", Linux `window-corner-radius` and `use-system-titlebar`) |
| Browser | **Search** (`browser-search-provider`) · **Shortcuts** (`browser-element-selector-hotkey`) |
| Editor | **Typography** (`editor-font-size`) · **Display** (`editor-line-numbers`, `editor-relative-line-numbers`, `editor-soft-wrap`, `editor-vim-mode`) |
| Panes | **Layout** (`pane-gaps`) · **Focus** (`pane-inactive-opacity`) · **Frame** (`pane-margin`, `pane-corner-radius`, `pane-border-width` . all disabled without gaps) |
| Hosts | **Machines** (configured hosts, live connection state, Remove) · **Add host** (an inline ssh destination field) |
| System | **Tray** (`tray`, only where the profile has one) · **Daemon** (`quit-daemon-on-exit`) · **Diagnostics** (`show-fps`) · **Experimental** (`experimental-editor-pane`, `experimental-agent-pane`, each row present only with its cargo feature). `auto-restart-stale-daemon` is a file key with no Settings row |
| Multiplexer | Full-file editor for `zz/mux.conf`, with Save and donor-specific **Import tmux…** |
| Terminal | **Import Ghostty…** · **Font** · **Colors** · **Cursor** · **Selection & highlights** · **Padding**, covering every settable daemon appearance key except file-only `theme` and `background-opacity` |
| About | Centered mark (the Dock render at 88pt), name, tagline and version badge · **Updates** (`check-for-updates`, plus a Latest-release row that reads the update state: Check now, or Update / What's new once a newer release is known; desktop only) · **Build** (`CARGO_PKG_VERSION`, OS · arch, the short `ZZ_GPUI_SOURCE` revision, with a copy button on Version that puts all three on one line) · **Project** (repository, releases, new issue, license) |

Every structured row shows its effective client-local or daemon-resolved appearance provenance and
a Reset button that removes the corresponding `zz/config` key. The Multiplexer file editor
deliberately has no per-key badges or Reset controls. All chrome colors come from `cx.theme()`
semantic values.

The Theme group is two picker cards rather than rows of buttons, because both choices are about
appearance: `theme-mode` offers three tiles carrying a drawn 84×56 window mockup . sidebar strip,
title-bar lights, two lines of body text . painted from the selected preset's matching variant (or
`ThemeColor::for_mode` with no preset), with System showing one window in both palettes (the dark
copy is drawn full width and clipped to the right half).
`app-icon` offers three tiles of the real artwork through `crate::app_icon::icon_preview` . macOS's
own renders of `assets/zz.icon`, committed as `assets/zz-{light,dark}-512.png` . where Automatic
renders the variant `cx.window_appearance()` resolves to right now. The preview is Lanczos-filtered
once from 512px to a cached 96px raster before GPUI paints it into the 48pt tile, avoiding the
rounded-edge aliasing caused by a large atlas downscale without mipmaps. The card is macOS-only,
since X11 takes its icon from the window options and Wayland from the desktop file. Both pickers use
the same selection treatment: a plain foreground border around the selected preview (transparent
border otherwise, so nothing shifts), no fill and no hover state, keeping the artwork unchanged.

The Theme group also carries one non-persisted row: **UI zoom**. It is the same bounded `NumberInput`
the geometry rows use, over the root rem as a percentage of the 16px default, with the reset control
in the title where every other row keeps it. Because it writes nothing to disk it is also the one
field here that applies as it changes . a stepper press zooms on the press, and a typed value zooms
once it is a complete number in range, so a prefix such as `1` of `150` never drags the whole UI to
the minimum. Leaving the field clamps and normalizes what it holds. This setting intentionally does
not enter `zz/config`;
it follows Zed's per-run UI font-size adjustment and leaves browser page zoom and terminal font size
under their existing pane-local controls.

The Chroma Colors group's color rows use `zz_ui::color_picker::ColorPicker`: a swatch trigger opening a
popover with a `#rrggbb` field and a swatch grid. It is *not* a port of upstream's picker (no
saturation/lightness area, no hue slider), because chrome colors are pasted from a published palette
or nudged, not explored. Clearing the field writes a key removal, which is how a root returns to
the selected preset or built-in palette. The preset menu shows both variants as stacked swatch rows;
its atomic writer stores the family and clears explicit roots without changing `theme-mode`.
`synchronize_geometry_inputs` echoes the resulting file back into the pickers through the silent
`set_color` setter, so a preset (or a hand-edit of `zz/config`) updates the swatches without
re-entering the writer.

Terminal mirrors daemon-resolved appearance into its controls and writes edits back through the
bounded, comment-preserving `zz/config` writer. Multiplexer mounts the native rope-backed
`CodeEditor` for `zz/mux.conf` with line numbers disabled, 12px monospace text, tmux-grammar
highlighting (`tree-sitter-tmux`, upstream's own `highlights.scm`), and a deliberately square 2px-
inset frame . a file surface, not a control. Save uses the
1 MiB bounded atomic writer; a clean editor reloads when entered, while unsaved text is retained. A
successful mux save asks the daemon to `reload-config`.

Multiplexer owns the confirmed **Import tmux…** action, which replaces `zz/mux.conf` verbatim and
warns when it will discard unsaved editor text. Terminal owns the other half: a confirmed **Import
Ghostty…** row under the page description re-reads the Ghostty donor into `zz/config`, rewriting
every appearance key that donor sets (a `theme = …` is flattened against the active scheme, so the
import stores concrete colors) and requesting a daemon reload. The button is disabled when no donor
exists, and its row names the path that will be read. The one-time first-run prompt
(`crates/zz/src/config/import_prompt.rs`, marker file `<data-dir>/zz/import-prompted`) retains the
combined Ghostty-and-tmux import.

Shadow strength appears in Interface's Tweaks group beside Widget corner radius. The numeric control
shows 0–100%, steps by five percentage points, and stores a 0–1 factor. Valid edits save as they are
typed; partial or invalid values stay in the field until Enter or blur restores the effective value.
Reset removes the override and restores 100% through the normal config watcher.

Structured control callbacks, including Terminal, only write `zz/config`; they never mutate the
GPUI config global directly. Multiplexer writes `zz/mux.conf` and requests a daemon reload. The
existing 500 ms watcher remains the single `zz/config` apply path, updates effective values and
provenance, and refreshes the open dialog.

# Examples

```ini
# Let KDE (or another capable Linux desktop) own the titlebar and window border.
use-system-titlebar = true

# Blend the terminal color over its opaque pane base.
background-opacity = 0.85

# Ask GPUI and the platform compositor to blur app chrome.
window-background-blur = true

# Set every interface transition to its static state.
animations = false

# The toggle uses a 6px margin, 13.5px radius, 0.5px border, and surface ring.
pane-gaps = true

# Keep inactive pane content at 70% strength. Set 1 to disable dimming.
pane-inactive-opacity = 0.7

# Optional explicit chrome overrides
pane-margin = 8
pane-corner-radius = 6
pane-border-width = 0.5

# One switch drives both FPS readouts; they are separate pipelines.
show-fps = true

# Off by default . the daemon outliving the app is what preserves sessions.
quit-daemon-on-exit = false

# Opt in to the experimental panes; off, no surface (picker, palette, CLI)
# can create them.
experimental-agent-pane = true
experimental-editor-pane = true
```

```ini
# Native appearance override; repeated keys retain order.
background = #1e1e2e
font-family = JetBrains Mono
font-family = Symbols Nerd Font Mono
palette = 4=#89b4fa
```

```ini
# Native mux overrides; the daemon applies these above zz/mux.conf.
prefix = C-a
mode-keys = vi
history-limit = 50000
set-clipboard = external
synchronize-panes = off
```

# Related

- [`zz`](/crates/zz.md) owns configuration loading and all consumers.
- [End-to-end data flow](/architecture/data-flow.md) distinguishes client-local and daemon-owned
  configuration.
- [Split-pane layout](/concepts/split-pane-layout.md) propagates exposed window corners into panes.
