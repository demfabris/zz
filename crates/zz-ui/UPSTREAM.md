# zz-ui: the fork of gpui-component

`zz-ui` owns zz's entire widget layer. It began as a thin facade over
[`gpui-component`][upstream] and was progressively forked, module by module,
until the dependency could be deleted outright. **`gpui-component` is no longer
a dependency of this workspace**; nothing outside `gpui` itself is left.

- Forked from: `longbridge/gpui-component`, now [longbridge/gpui-kit][upstream]
- Original source revision: `b004e595cf5de98a73b6b561394a559a94ae1e2a`
- Upstream license: Apache-2.0, retained here as `LICENSE-APACHE`
  (© 2024–2025 Longbridge). Bundled icon artwork in `assets/icons` is
  [Tabler Icons][tabler] outline, MIT, retained as `assets/icons/LICENSE-TABLER`
  (© 2020–2026 Paweł Kuna); it replaced the Iconoir set.
  The vendor brand marks (`openai`, `claude`) are [Simple Icons][simple-icons],
  CC0-1.0.
- `zz-ui`'s own code is `MIT OR Apache-2.0` like the rest of the workspace;
  the ported portions remain under upstream's Apache-2.0 terms.

## Why fork

Upstream hardcodes visual decisions we needed to change: the dropdown item
text size, for one, was a literal `text_sm` with no theme token or builder
behind it. Owning the render code is the only way to change that, so the widget
moved in. Once that was true of enough widgets, keeping a partial dependency
cost more than finishing.

## Layout

```
src/widget/
  foundation/   theme, palette, shared traits, window border
  highlighter/  syntax-highlight theme data + optional tree-sitter engine
  code_editor/  rope-backed native editor
  <widget>/     one directory per widget
```

Everything is reached through `zz_ui::<widget>`, mirroring upstream's namespace,
so the fork never moved a call site.

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

## Ported modules and local deltas

| Module | Style | Notable local delta |
| --- | --- | --- |
| `foundation` | mixed | dropped upstream's JSON theme registry + schema (~1.4k lines, and the `schemars` dep): zz shares its palette definitions in `zz_ui::chrome_palette`, so nothing deserialized a theme. Palette values ported verbatim. Metrics are down to two: `radius` (upstream's `radius_lg` is gone, and no widget derives halves or doubles off it any more) and a `CHROME_GAP` const. `rems_from_px` keeps named typography and control metrics on the 16px design baseline so changing GPUI's root rem scales them together; custom `Size::Size(px)` remains the fixed-pixel escape hatch. Added `oklab_lightness`, which exposes the L of the already-vendored Oklab conversion so the app crate can assert perceptual distance between two theme roots rather than eyeballing HSL. |
| `separator`, `spinner` | trimmed | reduced to the variants the app uses |
| `tag` | trimmed | Three variants: Primary, Secondary, and Success; theme-driven radius |
| `kbd` | trimmed | one muted pill: upstream's `appearance(false)` plain-text mode and its outline/primary treatments are dropped, since every hint reads as a caption beside its label. Added `lowercase()` for hints that read as prose (`t`, `b`, `a`) rather than as a keycap legend. |
| `switch` | trimmed | dropped inline label/`Side`/custom color; kept the animated thumb. The thumb uses `foreground` for visibility against the track. The checked track is the `accent` root, the only chromatic fill a neutral control gets |
| `menu` | close-to-source | Owns its actions (`zz_menu`), key context (`ZzPopupMenu`) and `init()`; upstream's native `AppMenuBar` not carried over. Local rows use 12px text, 26px height (20px Small), 4px surface gutters, inset half-pixel separators, and the inset menu radius. Open submenu parents highlight in neutral gray; selected shortcut labels and icons brighten to full foreground. `PopupMenuItem::stepper` supports live values, pointer adjustments and Left/Right/Enter without dismissing; the browser menu uses it for page zoom. |
| `icon` | trimmed | `IconName` is a **hand-written** enum instead of upstream's build-time proc-macro codegen; SVGs live in `assets/icons` and are embedded by our own `Assets`, replacing `gpui-component-assets`; `Globe` uses Tabler’s round `world` artwork. The dedicated `window-close` glyph extends the Tabler X to the same 18-unit span as maximize, keeping its 2-unit stroke and the general-purpose `xmark` unchanged |
| `tooltip` | trimmed | hangs off gpui's `.tooltip()` rather than upstream's `Root`-owned overlay; dropped `ComponentTooltip` after nothing adopted it |
| `popover` | trimmed | owns its `Cancel` action and `ZzPopover` context; `on_dismiss` runs once for every close path, including trigger toggles |
| `slider` | **zz-original** | `DiscreteSlider` displays compact pills filled through the selected step, per-pill tooltips, a trailing selected value, keyboard navigation, and a slider accessibility value |
| `list` | trimmed | `ListItem` only; upstream's virtualized delegate `List` is unused. Rows use the inset menu radius and solid accent highlight retaining the normal foreground text and icons instead of a foreground outline. |
| `scroll` | close-to-source | custom-painted scrollbar kept faithful. zz fixes upstream's track-hover ordering bug: it compares the previous axis before storing the new one, so entering a hover-only track requests its repaint. |
| `button` | close-to-source | reimplements upstream's `pub(crate)` `ButtonIcon` on our `Spinner`; `ButtonRounded` keeps `Medium` (the theme control radius) and `Size(px)` for explicit overrides; `Button::compact_icon` fixes shared chrome controls at a 24px surface, Small 14px glyph, and 0.5px optical drop. Default, Secondary, and Ghost variants use `background.washed(2)` for hover, pressed, and selected fills, matching sidebar highlights and preserving background blur. Neutral controls use the shared half-pixel edge and soft shadow, with the edge transparent at rest for Ghost buttons. `Button::flat()` keeps nested actions free of extra borders and shadows while retaining their wash and keyboard focus indicator. Local `ButtonVariant::Accent` is Primary's solid shape on the `accent` root instead of `foreground`; upstream's `Info` variant has no counterpart. |
| `title_bar` | mixed | every `cfg!(target_os)` branch carried verbatim (macOS traffic lights, Linux/Windows client-side controls, WASM). `WindowControls` is public, unlike upstream's: the main window has no bar . its sidebar strip owns the drag region so the panes reach the top edge . and mounts the cluster on its own through `shell::app_titlebar_strip`, the matching strip above the content column that only the platforms drawing their own buttons reserve. `TitleBar` itself is now the Settings window's |
| `select` | trimmed | Retains value state and confirmation subscriptions, but renders the shared Button and PopupMenu used by pane-split settings. Uses left checkmarks with an 8px label gap, compact menu rows, bounded scrolling, and the same trigger treatment. Dropdown button labels use 13px text, one pixel larger than their menu entries. Settings triggers fit the selected label; menus fit their widest option within the popup width limit and stay at least as wide as the trigger. No separate Select row renderer. |
| `overlay` | close-to-source | `Root` + dialog + notification + `WindowExt`; dropped the sheet layer, upstream's `FocusTrapManager` (Tab is trapped by walking the top dialog's own focus handle) and the macOS accessibility hit-test forwarder. Toasts default to the top center of the window, with a vertically centered close button in the content row. Dialog shadows use the `overlay` theme token rather than upstream's hardcoded `hsla` (see `clippy.toml`). zz's default dialog is deliberately compact: 400px wide, 12px gutters, 13px/12px title and body, and Small actions. `ROOT_KEY_CONTEXT` is public so the host app can bind root-rem UI scaling below pane-specific browser and terminal zoom. Local addition: `Notification::key` plus `Root::dismiss_notification`/`WindowExt::dismiss_notification`, so a toast raised for a daemon-timed status message can be retired by identity when the daemon clears it (upstream can only clear the whole stack). |
| `input` | **written fresh** | not a port. Upstream's is ~12k lines because it doubles as a code editor (rope, LSP, tree-sitter, masking, OTP, in-input search); ours is a text field. Plain `String` storage, grapheme-safe indexing via `unicode-segmentation`, gpui's `EntityInputHandler` for IME. Owns `zz_input` actions and the `ZzInput` context. `text_align` is applied by the layout's index↔position math, not just at paint, so a centered field hit-tests correctly . upstream's does not. Small fields use an explicit 13px value size so compact form text matches Settings' primary row labels. |
| `code_editor/{state,input,element,mode}` | trimmed | ported from upstream `input/` at the revision above as a sibling of zz's small text field. Local `CodeEditor::background_opacity` controls text and gutter base fills without fading text or changing embedded Settings editors. Renamed the public surface to `CodeEditorState`/`CodeEditor`; keeps a rope buffer, line numbers, soft wrap, IME, single-cursor editing and upstream's tab default. Removed the `Root` downcast and mapped all chrome to zz's semantic theme. zz adds a frame-to-frame `ShapedCache` (element.rs): upstream re-shaped the whole buffer every prepaint; zz re-shapes only when content, wrap width, typography, or theme change, keyed by a `layout_generation` counter every content mutation bumps. |
| `code_editor/{movement,selection,cursor,blink_cursor,change,history,indent,rope_ext}` | close-to-source | upstream rope movement, grapheme-safe selection, cursor blink, edit grouping, undo/redo and indentation mechanics. Multi-cursor behavior is deliberately omitted. History pops only the contiguous version block at the top of a stack; a buried matching version cannot pull unrelated edits into the group. |
| `code_editor/display_map` | close-to-source | upstream buffer/wrap/fold mapping retained as a resync seam. Folding compiles but has no exposed UI in zz's first editor surface. |
| `code_editor/vim` | **zz-original** | not upstream code and not a port of anyone's: upstream has no vim layer, and the whole thing is hand-rolled on the vendored editor with zero new dependencies. Split into a pure core (`parser` keystroke grammar, `motion`, `text_object` . all plain functions over a `Rope`, no GPUI, no `CodeEditorState`) and a thin `executor` that spends the editor's existing primitives. Inert unless `set_vim_enabled(true)`: `CodeEditorState::vim` is `None` by default and every interception is behind that check. The hooks it needed in the ported files are small and marked in place . text input is diverted in `state.rs`'s `replace_text_in_range`/`replace_and_mark_text_in_range`, the bound keys (Escape, Enter, Backspace, Tab, arrows, Home/End, PageUp/Down) ask `vim_key` first, vim's control chords bind against a second `vim` key-context identifier `input.rs` adds only when the layer is on, and `element.rs` paints a block cursor plus an optional relative rail. Five vendored helpers widened from private to `pub(super)` so the executor can spend them instead of reimplementing them: `break_typing_group`, `pause_blink_cursor`, `indent_selection`, `outdent_selection`, `viewport_rows`. One deliberate structural change to `element.rs`: the line-number rail left `ShapedText`/`ShapedCache` and is now shaped per frame for visible rows only, because relative numbering depends on the cursor line and must never invalidate the buffer shaping. |
| `text` | close-to-source | renderer only . `markdown_ast` is `markdown::mdast`, so parsing comes from the `markdown` crate. Dropped upstream's HTML path (`html5ever`), which zz never rendered. Owns the window text-selection host that `overlay::Root` mounts. Heading base sizes are design pixels converted through the live root rem, keeping Markdown headings aligned with scaled body text. `InlineState` retains the hovered glyph across GPUI's frame-owned mouse handlers, so a stationary glyph does not invalidate the window on each move event. |
| `highlighter` | mixed | keeps the upstream non-tree-sitter stub as the default and ports the real parser behind zz-ui's optional `tree-sitter` feature. The registry is trimmed to Rust, Markdown, JSON and TOML; colors come from the already-vendored `HighlightTheme` palettes rather than upstream's JSON theme registry. Incremental editor parses complete synchronously because zz does not vendor upstream's background-parser task. |

## Two gotchas

**Syntax highlighting is target- and feature-gated.** With default features,
including the browser client (`clients/web`), `highlighter/syntax.rs` still returns no styles
and its parity test remains the contract. The desktop `zz` app enables `zz-ui/tree-sitter`,
which activates the vendored parser for Rust, Markdown, JSON and TOML. Both paths use the same `HighlightTheme` colour
tables; no second theme registry exists.

**The foundation flipped last, on purpose.** `Theme` and the shared traits sit
underneath every widget. Vendoring them early would not have failed to
compile. It would have left two theme globals, with the app writing one and the
still-upstream widgets reading the other, so half the UI would have silently
stopped following the terminal palette. The same coupling ran the other way through
`Root`: upstream's `input` and `text` downcast the window root to *upstream's*
`Root` by concrete type, so swapping ours in early would have panicked every
focused text field. Both are why `input`, `text`, `overlay` and `foundation`
landed in a single commit.

The shared Interface settings page includes a font picker using the shared Button and scrollable PopupMenu.
Native entrypoints register `AvailableFonts` with their platform text system so the list excludes
GPUI's hardcoded fallback names. The browser client uses GPUI's font list. The picker includes
System default and filters internal dot-prefixed font aliases.

## Conventions

`Theme::control_radius()` returns the configured radius through 24px and GPUI's full-rounding
value above 24px. Buttons, single-line inputs, number fields, select triggers, navigation
chips and tree highlights, and browser controls use it so Full mode can produce
pills and circles under adaptive rounding. Effort-slider pills follow the same rule. Popup menus,
select and list rows, pane and file/history pickers, URL suggestions, and agent suggestions use `Theme::menu_radius()`: a
highlight radius equal to the container radius minus 4px, without a fixed cap or Full-mode sentinel. Container surfaces and multiline inputs
keep `radius`.

`StyledExt::popover_style` shares an opaque raised surface, half-pixel edge, and scaled soft outer
shadow across menus and picker containers. The shared command palette uses a 560px surface, a 40px bare Input, 12px search text,
12px inline Tag pills and medium labels, 10px details and footer hints, and 8px list gutters.
Rows use 16px line heights and occupy 26px with 2px gaps, matching dropdown typography. `command/palette.rs` supplies sections, fuzzy text highlights,
breadcrumbs, shortcut tags, host status dots, and session/window running-agent summary dots for
the unified desktop palette. Agent pane rows use shared vendor icons and text status.
`PaletteRow::icon` and `PaletteRow::expanded` add navigation icons and disclosure state to the
same rows. `command_palette_tree_entry` exposes a separate disclosure toggle handler so clicking
the chevron expands a session or window while clicking its row activates it. The desktop uses
these components for Workspace and Navigate trees and daemon session/window/pane rows; each
adapter owns expansion, search, and activation state. The tree retains the palette's compact
typography and Chroma treatment.
Daemon prompt rows share that renderer. `InputState::set_placeholder` updates the field hint
when the palette changes mode without replacing its editing state. Menu and picker
rows use `selection_highlight`: an opaque accent fill and matching edge, the normal foreground
text and icons, and no shadow. Inputs and number fields reserve a 1px border for clearer circular edges and use the accent when focused.
Button keyboard focus rings also use the accent.

PopupMenu, model picker, and directory/session picker rows use `StyledExt::menu_item_corners`
with GPUI's per-element fixed radius mode and smoothing 2.5. Their radius is capped at 40%
of the row height. Custom menu content receives the highlight state so descriptions brighten
to the normal foreground. Directory and session rows share 26px heights and 12px labels.
The menu surface retains the window's adaptive squircle rounding. Fill and border use native
GPUI painting; no highlight painter or row masks are involved.

Picker, chooser, command palette, popup menu, and dialog surfaces share a 160ms entrance:
they fade in while settling upward by 6px, using GPUI's reduced-motion behavior.
Dialog movement starts near its final position instead of sliding from the window edge.
Each opened dialog has its own animation identity so replacement dialogs animate again.

- **Close-to-source ports** keep upstream's structure so a future re-sync is a
  small diff. They carry a module-level
  `#![allow(clippy::pedantic, clippy::style, clippy::complexity)]` because they
  follow upstream's style, not this workspace's pedantic lints. Correctness,
  perf and suspicious lints stay active.
- **Trimmed rewrites** port only the API the app calls and document the
  omissions, so a deliberate omission is never mistaken for a missing feature.
- **Chrome colors come from `cx.theme()`**, never a literal; `clippy.toml`
  enforces it, and the ports were corrected where upstream hardcoded a color.
- Inputs, number fields, select triggers, and dropdown surfaces use
  `StyledExt::control_surface`: a half-pixel foreground edge and the shared soft shadow.
  Input and select focus changes only the border color, preserving their layout. Clear buttons,
  steppers, browser tab close actions, and tree-row actions stay flat inside the outer control.
  Tree actions end at the row highlight edge; browser tab close actions have no extra right padding.
  Both use the Text button variant for foreground-only hover feedback.
  Browser URL fields show a washed background and the shared button edge and shadow only while focused.
  Active browser tabs use the same treatment; both reserve border width across state changes.
  The browser header uses 28px tabs, URL fields, and browser buttons, with 8px outer padding and row spacing.
  Pane split and close buttons keep their 24px size.
  A 24px sliders button inside the URL field opens connection, sound, and site-data controls.

Re-syncing a module against a newer upstream revision means updating the
revision above, re-applying that module's delta, and re-running the workspace
build, the tests, and the browser client (`just web-build`).

## Cherry-picked since the fork revision

The fork revision above is *not* moved by these: each is an individual upstream
fix re-applied by hand on top of zz's local delta, so the next wholesale
re-sync knows what it already has. Every one carries a regression test.

On 2026-09-12, we checked upstream through
[`84f57fdfcb4910623fb0bb7f795b077e249f9271`][checked-2026-09-12] and selected the
menu, scrollbar, and Markdown fixes listed below. We retained zz's widget APIs,
theme rules, text selection, and editor changes. This records selective ports;
the original source revision above remains the base for future comparisons.

| Upstream | What it fixes here |
| --- | --- |
| [`3de68cd1`][u1] | `Language::Plain` carried the **JSON** grammar, and `SyntaxHighlighter::new` falls back to `"text"` for anything unregistered . so every ` ```bash ` / ` ```python ` fence in an agent transcript built a JSON parser and parsed the whole block to produce no styles. `LanguageConfig::language` is now `Option`, plain text is grammarless, and `SyntaxHighlighter::build_inert` never parses. |
| [`66cadafb`][u2] + [`98af8912`][u3] | `ContextMenuExt::context_menu` derived its fallback element ID from a **stack address**, so a redraw from the event-dispatch path silently dropped the open menu without a `DismissEvent` and stranded focus on it. Now `#[track_caller]` + `ElementId::CodeLocation`, plus `PopupMenu::previous_focus_handle` so dismiss restores focus without moving action dispatch. |
| [`1a667218`][u4] | `render_list_item` matched only `Paragraph` and `List` and dropped everything else on the floor . a fenced code block, table, blockquote or heading nested in a list item rendered as **nothing**, which is most of what an LLM puts in a numbered list. |
| [`be3c8413`][u5] | `CodeBlock` captured the `HighlightTheme` at *parse* time and memoized styles against it, so markdown code blocks kept the palette that was active when they were parsed across a light/dark switch. The theme is now read at render and travels with the memo. |
| [`5a2a96e6`][u6] | Menu dismissal preserves focus that an item handler moved to another control. Dismissal still restores the previous focus when the menu owns it. |
| [`d8376ad5`][u7] | Clicking a scrollbar track requests a repaint after changing the offset, including when a custom handle drives content through notifications. |
| [`c3937a36`][u8] + [`b0a1836b`][u9] | Markdown hard breaks render as newlines. Soft LF, CRLF, and CR breaks reflow as spaces inside text nodes, with mark ranges measured after normalization. |
| [`26cc9366`][u10] | Mixed text and image paragraphs split at hard newlines before wrapping. The text shaper receives one line at a time, and inline images after a hard break appear on the next line. |

Checked against this fork and deliberately **not** taken:

- `accb9616` (scroll mask wheel routing) . `ScrollableMask`/`horizontal_scroll_area` were dropped from `scroll` on purpose; nothing here uses them.
- `c48fb6f0` (IME selection range) . both `input` and `code_editor` already compute the selection with `utf8_offset` from the replacement start; the port never had the bug.
- `4ac87b15` (IME composition underline) and `3c270ed2` (unset gutter background) . neither the fresh `input` nor `code_editor` paints an IME underline or a gutter fill, so there is nothing to correct.
- `f03f3713` (stub styles lose the text color) . `Inline::request_layout` already emits a trailing run in the base text style when the highlight list is empty, so the non-`tree-sitter` stub can keep returning nothing. `highlighter::syntax`'s inertness test stays the contract.
- `bc174a7e` (nested submenu paint priority) . it wraps *every* menu in `deferred().with_priority(..)`, changing paint order app-wide to fix a bug that needs two nested submenu levels; only `browser.rs` opens a submenu, and only one deep.
- `630b664f` (Linux client-side decorations) . Linux-only shadow/border cosmetics that cannot be verified from this workstation, landing in a file whose local delta (`resize_hit_size`, the restore/inset double-counting fix) is load-bearing. zz also solves the rounded-corner problem upstream works around here with its own `Window::set_window_corner_mask` gpui patch.

[u1]: https://github.com/longbridge/gpui-component/commit/3de68cd11752e09b5f8834d27a8a3cb43032ad3d
[u2]: https://github.com/longbridge/gpui-component/commit/66cadafb58061e2d65fe6ddbc58fbc72db2b6f64
[u3]: https://github.com/longbridge/gpui-component/commit/98af8912ab0b3fe08df519dff7acd96a77b19586
[u4]: https://github.com/longbridge/gpui-component/commit/1a66721833a75d796b949e59370a3374baba793d
[u5]: https://github.com/longbridge/gpui-component/commit/be3c8413766cafc736a0c1c80306ff0f293e04f3
[u6]: https://github.com/longbridge/gpui-kit/commit/5a2a96e63e22d8d3f3b9131cf6215623a45f2ebf
[u7]: https://github.com/longbridge/gpui-kit/commit/d8376ad5635b963b2e751fbd4e328f9195d565a5
[u8]: https://github.com/longbridge/gpui-kit/commit/c3937a36dbe97347fd84f453761fc19784374ae2
[u9]: https://github.com/longbridge/gpui-kit/commit/b0a1836b1e2e3053b8998517f1816ab47e4474ae
[u10]: https://github.com/longbridge/gpui-kit/commit/26cc9366abb27ccedce386ac99a615a8fa7018da
[checked-2026-09-12]: https://github.com/longbridge/gpui-kit/commit/84f57fdfcb4910623fb0bb7f795b077e249f9271

[upstream]: https://github.com/longbridge/gpui-kit
[tabler]: https://tabler.io/icons
[simple-icons]: https://simpleicons.org

The desktop and browser clients share chrome preset definitions and palette resolution in
`src/chrome_palette.rs`, and pane-picker rows in `src/pane.rs`. WebAssembly text fields accept
both Control and Command editing shortcuts so browser clients work on either desktop platform.

The shared workspace components also include window tabs and overflow menus in
`src/navigation/status.rs`, including session switching and agent activity menus with the window pill surface and height, and overlapping pane icon decks, hover details, and direct pane selection in
`src/navigation/status/pane_deck.rs`. The titlebar uses 26px pills and 20px pane cards with 7px overlap and 14px icons to leave vertical space around both surfaces. Workspace settings and sidebar buttons match the 26px pill height. Sidebar markers, actions, and keyboard navigation live in
`src/navigation/sidebar.rs`, and command palette/menu/confirmation presentation in
`src/command/`. `src/agent/slash.rs` renders provider command suggestions; the matching and
replacement rules live in `zz-client`. Desktop and browser both use these components.

`src/terminal.rs` contains the portable GPUI terminal painter, including retained row shaping,
selection, cursor, scrollbar, and image placement. It consumes `zz-terminal` view data with the
engine features disabled. Each client owns its input, focus, image delivery, and connection
lifecycle; native daemon and operating-system dependencies stay outside `zz-ui`.
