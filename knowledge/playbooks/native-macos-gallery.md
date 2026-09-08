---
type: Playbook
title: Native macOS component gallery
description: Build, run, and verify the native Swift component library, with zz-ui coverage and the Rust client boundary.
resource: clients/macos/Package.swift
tags:
- swift
- macos
- ui
- client
timestamp: 2026-09-07T20:08:21Z
---

# Overview

`clients/macos` contains a reusable `ZZUI` Swift library and the `ZZComponentGallery`
macOS app. Its default window reproduces the desktop workspace composition:
256-point translucent sidebar, six-point pane gutters, browser recents, and
the compact agent composer. The appearance menu opens a separate component
catalog with 13 pages; Option-Command-K opens it directly.
The gallery exercises the native component port with local fixtures.
It does not connect to a daemon or start terminal, browser, or agent processes.

Swift owns presentation and widget editing. The functional `ZZNative` terminal client consumes
`zz-client-ffi`, following the existing iOS client's ownership and wake-fd pattern.
See [Native macOS terminal client](/playbooks/native-macos-client.md).
Session state, protocol reduction, command execution, transport, terminal data,
and configurable client key bindings stay in Rust. See
[Client core and contract](/designs/client-core-and-contract.md).

# Build and run

Use macOS with Xcode selected through `xcode-select`. The package uses Swift 6 and
targets macOS 14 or later. The component gallery build needs no Rust build or external
Swift packages. The combined Swift tests also build a private Rust daemon fixture.

Run from the repository root:

```sh
just macos-gallery
just macos-gallery-build release
just macos-gallery-test
```

The build recipe creates and ad-hoc signs
`clients/macos/dist/zz Native Gallery.app`. It uses a separate bundle identifier,
`sh.zzmux.native-gallery`, and reuses zz's app icon. It does not replace the
installed GPUI client. Build products stay inside this worktree.

Open `clients/macos/Package.swift` in Xcode for development. The gallery supports
`--page inputs` to open the catalog and `--appearance dark` launch arguments. Page names match the
`GalleryPage` cases in `Sources/ZZComponentGallery/GalleryApp.swift`.

# Visual contract

The native library follows [UI conventions](/configuration/ui-conventions.md).
`Foundation.swift` carries the seven palette roots, the Oklab color derivations,
the control size scale, and the adaptive corner curve from the GPUI source.
Components read `EnvironmentValues.zzTheme`; callers can supply a `ZZTheme`
through `ZZThemeContainer`.

| Property | Reference |
|---|---|
| Light and dark roots | `crates/zz-ui/src/widget/foundation/palette.rs` |
| Desktop macOS Classic preset | `crates/zz-ui/src/chrome_palette.rs` |
| Surface and foreground derivations | `crates/zz-ui/src/widget/foundation/color.rs` |
| 24/28/36/40-point control heights | `crates/zz-ui/src/widget/foundation/styled.rs`, `Size::control_h` |
| Half-point edge and soft control shadow | `styled.rs`, `control_shadow`, `surface_ring` |
| Compact icon target and optical offset | `widget/button/button.rs`, `Button::compact_icon` |
| Shared radius and chrome gap | `widget/foundation/theme.rs` |
| Adaptive corner curve | `crates/zz/src/theme.rs`, `ADAPTIVE_CORNER_FRACTION` |

Native substitutions are deliberate: SF Symbols replace the SVG icon set;
macOS supplies traffic lights, materials, menu tracking, popovers, scrolling,
text selection, IME composition, clipboard services, undo, and the editor find
bar. These follow the platform's accessibility preferences. The gallery's theme
inspector changes radius, font family, shadows, and disabled state without
editing zz configuration. `ZZTheme` also accepts a monospaced font family and
shadow strength for the client adapter.

The desktop preview starts with the reference screenshot's configuration:
macOS Classic dark, white foreground, 24-point widget and pane radii, and a
93-percent background tint over the native window material. Its appearance
popover switches to the stock preset with six-point widget corners and
13.5-point pane corners. Pane radius is independent of widget radius; the
half-point border changes color on activation without changing width.
These are explicit preview settings, not a Swift copy of the Rust config parser.

# Component coverage

The inventory follows zz-ui's exported surface, including its application
compositions. It does not use the larger upstream gpui-component catalog as a
checklist: zz-ui exports no checkbox, slider, calendar, table, or avatar widget.

| zz-ui surface | Swift surface | Gallery page |
|---|---|---|
| Foundation, theme, sizes, color derivations | `ZZTheme`, `ZZColor`, `ZZControlSize`, `ZZRoundedRectangle`, surface modifiers | Foundations |
| Button, variants, outline, loading, flat and compact actions | `ZZButton`, `ZZButtonStyle`, `ZZIconButton` | Buttons |
| Icon, tag, key hint, spinner, separator, list item, scroll | `ZZIcon`, `ZZTag`, `ZZKbd`, `ZZSpinner`, `ZZSeparator`, `ZZListItem`, `ZZScrollView` | Tags, keys & lists |
| Input and number input | `ZZTextField`, `ZZTextEditor`, `ZZNumberInput` | Inputs & selection |
| Switch, select, color picker | `ZZSwitch`, `ZZSelect`, `ZZColorPicker` | Inputs & selection |
| Dialog, popover, menu, tooltip, notification root | `ZZDialog`, `ZZPopover`, `ZZMenu`, tooltip/context-menu modifiers, `ZZToastCenter`, root toast modifier | Menus & overlays |
| Markdown, text, highlighter, code editor, rendered diagrams | `ZZMarkdown`, `ZZCodeBlock`, `ZZSyntaxSpan`, `ZZSyntaxHighlighter`, `ZZCodeEditor`, `ZZDiagram` | Text & editor |
| Navigation, tree rows, indent guides, status windows, shell | `ZZWorkspaceTreeRow`, `ZZWorkspaceActionRow`, `ZZWorkspaceSidebar`, `ZZWorkspaceIndentGuides`, status views, `ZZAppShell` | Workspace & navigation |
| Sidebar badges, host indicators, layout and rename actions | `ZZWorkspaceMarker`, `ZZHostIndicator`, `ZZWindowLayoutMenu`, `zzRenameMenu`; tree-row indicator parameters | Workspace & navigation |
| Status session, close/rename actions, window overflow, agent count and clock | `ZZStatusSession`, `ZZWorkspaceStatusWindow` callbacks, `ZZStatusWindowOverflow`, `ZZStatusAgentCount`, `ZZStatusClock` | Workspace & navigation |
| Pane and floating surfaces, split and drag presentation, indicators, terminal search/status | `ZZPane`, `ZZPaneCorners`, `ZZFloatingSurface`, `ZZPaneSplit`, `ZZWorkspaceSplit`, drag views, `ZZPaneIndicator`, `ZZFrameRateBadge`, `ZZTerminalSearch`, `ZZTerminalStatus` | Panes & terminal chrome; desktop preview |
| Browser toolbar, address, tabs, start page, history, omnibox, error, picker status and menu | `ZZBrowserToolbar`, `ZZBrowserAddress`, `ZZBrowserTabStrip`, start/recent/omnibox/error/status/menu views | Browser chrome |
| Command palette, rows, key hints | `ZZCommandPalette`, `ZZCommandPaletteRow`, `ZZShortcutHints` | Commands & choosers |
| Floating command menu and confirmation | `ZZFloatingMenuRow`, `ZZFloatingMenuSeparator`, `ZZConfirmPrompt` | Commands & choosers |
| Chooser modal, tree and buffer rows, footer | `ZZChooserModal`, `ZZTreeChooserRow`, `ZZBufferChooserRow`, `ZZChooserFooter` | Commands & choosers |
| New-pane choices | `ZZPanePicker`, `ZZPanePickerRow` | Panes & terminal chrome |
| Directory and history pickers | `ZZPickerOverlay`, `ZZPickerModal`, `ZZPickerSearch`, `ZZPickerEmpty`, `ZZPathRow`, `ZZDirectoryRow`, `ZZHistoryRow` | Commands & choosers |
| Settings navigation, page, stack, entries, separators, provenance, reset and theme preview | `ZZSettingsNavigation`, `ZZSettingsPage`, `ZZSettingsStack`, `ZZSettingEntry`, `ZZSettingsDivider`, `ZZThemeTile` | Settings |
| Agent header, timeline, messages, activities, plans, tools, diffs, permissions and composer | `ZZAgentHeader`, `ZZAgentTimeline`, message/activity/plan/tool/diff/permission views, `ZZAgentComposer` | Agent |
| Agent empty/error states, numbered permissions and slash suggestions | `ZZAgentEmptyState`, `ZZAgentError`, `ZZAgentPermissionCard`, `ZZAgentPermissionOption`, `ZZAgentSuggestionList`, `ZZAgentSuggestionRow` | Agent |
| Attachment strip, thumbnails and image preview | `ZZAttachmentThumbnail`, `ZZAttachmentStrip`, `ZZAttachmentPreview` | Agent; Feedback & attachments |
| Add-host, SSH, clear-site-data and import prompts | `ZZAddHostPrompt`, `ZZSSHSecretPrompt`, `ZZSSHConfirmPrompt`, `ZZClearSiteDataPrompt`, `ZZImportConfigurationPrompt` | Feedback & attachments |

The editor accepts highlight spans and converts Rust UTF-8 ranges to native
UTF-16 ranges. It owns native text editing, not a second Rust syntax engine or
command resolver. The gallery's preview highlighting lives in its fixture code.
The GPUI editor's Vim engine and extension plumbing are not copied into Swift.
`ZZDiagram` displays rendered content supplied by the caller, with pending,
error, source-copy, and full-preview states. The gallery supplies a local image;
the full client can feed it the existing Rust Mermaid renderer's output.

Application components take values, bindings, content builders, and action
closures. The gallery uses those callbacks to change fixture state. The terminal client
supplies the workspace views with Rust-owned snapshots and forwards their
actions through the C ABI. Agent and browser adapters remain to be connected.
The composer accepts caller-owned `canSend` readiness, including attachment-only
messages. Pane borders and per-corner radii come from the caller's layout state.

The shared surfaces added in `d13e2a06` map to `NavigationPresentation.swift`,
`CommandFloating.swift`, and `AgentPresentation.swift`. They follow
`navigation/sidebar.rs`, `navigation/status.rs`, `command/floating.rs`,
`agent/presentation.rs`, and `agent/slash.rs`. Error cards accept display-ready
text; the Rust adapter applies the shared error sanitization. The caller also
supplies visible status windows, clock text, permission decisions, and completion
results. Gallery keyboard handlers only exercise local examples. Tree navigation,
completion replacement, tmux confirmation rules, and grid-coordinate mapping
are not implemented again in Swift.

`crates/zz-ui/src/terminal.rs` now owns the existing GPUI terminal painter.
This gallery still shows a terminal fixture. `ZZNative` renders the shared Rust
viewport through AppKit and forwards input, selection, copy, scrolling, and
resize reports. Terminal images and the full search presentation remain outside
that adapter. The native terminal implements AppKit text input and an IME caret.

# Verification

`just macos-gallery-test` checks palette and color math, adaptive corners,
numeric editing and stepping, select filtering and navigation, color override
semantics, Markdown presentation, native text range conversion, toast lifetime,
composition sizing, and timeline behavior during native scrolling. It also runs
the native client integration test against an isolated daemon. Build both
debug and release before comparing runtime cost.

Use the gallery to inspect both appearances, resize the window, and change the
radius in the inspector. Exercise typing, selection, copy/paste, numeric bounds,
keyboard selection, disabled controls, popover dismissal, sheet actions, toast
replacement, editor undo/find, and the workspace fixture actions. Match the
actual desktop composition and its source callers when evaluating visual changes;
the base widget catalog alone does not capture desktop spacing or configuration.

Performance numbers from this gallery measure component presentation only.
Terminal and full-client comparisons require the Rust-backed adapter and the
same workload on both clients.
