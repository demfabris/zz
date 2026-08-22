# zz-ui: the fork of gpui-component

`zz-ui` owns zz's entire widget layer. It began as a thin facade over
[`gpui-component`][upstream] and was progressively forked, module by module,
until the dependency could be deleted outright. **`gpui-component` is no longer
a dependency of this workspace**; nothing outside `gpui` itself is left.

- Forked from: <https://github.com/longbridge/gpui-component>
- At revision: `b004e595cf5de98a73b6b561394a559a94ae1e2a`
- Upstream license: Apache-2.0, retained here as `LICENSE-APACHE`
  (© 2024–2025 Longbridge). Bundled icon artwork in `assets/icons` is
  [Iconoir][iconoir] regular, MIT, retained as `assets/icons/LICENSE-ICONOIR`
  (© 2021 Luca Burgio); it replaced the Lucide set the fork arrived with.
  The vendor brand marks (`openai`, `claude`) are [Simple Icons][simple-icons],
  CC0-1.0, and the `window-*` control glyphs are zz's own.
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

## Ported modules and local deltas

| Module | Style | Notable local delta |
| --- | --- | --- |
| `foundation` | mixed | dropped upstream's JSON theme registry + schema (~1.4k lines, and the `schemars` dep): zz builds its palette in `zz::theme`, so nothing deserialized a theme. Palette values ported verbatim. Metrics are down to two: `radius` (upstream's `radius_lg` is gone, and no widget derives halves or doubles off it any more) and a `CHROME_GAP` const. `rems_from_px` keeps named typography and control metrics on the 16px design baseline so changing GPUI's root rem scales them together; custom `Size::Size(px)` remains the fixed-pixel escape hatch. Added `oklab_lightness`, which exposes the L of the already-vendored Oklab conversion so the app crate can assert perceptual distance between two theme roots rather than eyeballing HSL. |
| `separator`, `spinner` | trimmed | reduced to the variants the app uses |
| `tag` | trimmed | 4 of upstream's variants; theme-driven radius |
| `kbd` | trimmed | one muted pill: upstream's `appearance(false)` plain-text mode and its outline/primary treatments are dropped, since every hint reads as a caption beside its label. Added `lowercase()` for hints that read as prose (`t`, `b`, `a`) rather than as a keycap legend. |
| `switch` | trimmed | dropped inline label/`Side`/custom color; kept the animated thumb |
| `menu` | close-to-source | item text `text_sm` → **`text_xs`** (the change that started the fork); owns its actions (`zz_menu`), key context (`ZzPopupMenu`) and `init()`; upstream's native `AppMenuBar` not carried over |
| `icon` | trimmed | `IconName` is a **hand-written** enum instead of upstream's build-time proc-macro codegen; SVGs live in `assets/icons` and are embedded by our own `Assets`, replacing `gpui-component-assets` |
| `tooltip` | trimmed | hangs off gpui's `.tooltip()` rather than upstream's `Root`-owned overlay; dropped `ComponentTooltip` after nothing adopted it |
| `popover` | trimmed | owns its `Cancel` action and `ZzPopover` context |
| `list` | trimmed | `ListItem` only; upstream's virtualized delegate `List` is unused |
| `scroll` | close-to-source | custom-painted scrollbar kept faithful. zz fixes upstream's track-hover ordering bug: it compares the previous axis before storing the new one, so entering a hover-only track requests its repaint. |
| `button` | close-to-source | reimplements upstream's `pub(crate)` `ButtonIcon` on our `Spinner`; `ButtonRounded` keeps only `Medium` (the theme radius) and `Size(px)`, since a button turns the same corner as everything else |
| `title_bar` | mixed | every `cfg!(target_os)` branch carried verbatim (macOS traffic lights, Linux/Windows client-side controls, WASM). `WindowControls` is public, unlike upstream's: the main window has no bar . its sidebar strip owns the drag region so the panes reach the top edge . and mounts the cluster on its own through `shell::app_titlebar_strip`, the matching strip above the content column that only the platforms drawing their own buttons reserve. `TitleBar` itself is now the Settings window's |
| `select` | trimmed | **one** entity instead of upstream's three; stores the picked *item*, not an index, so the selection survives filtering. Rows are built in `select` rather than reused from `list::ListItem`, so the highlight is a flat `background.raised(2)` fill like every other menu's, not `ListItem`'s outlined box |
| `overlay` | close-to-source | `Root` + dialog + notification + `WindowExt`; dropped the sheet layer, upstream's `FocusTrapManager` (Tab is trapped by walking the top dialog's own focus handle) and the macOS accessibility hit-test forwarder. Dialog shadows use the `overlay` theme token rather than upstream's hardcoded `hsla` (see `clippy.toml`). zz's default dialog is deliberately compact: 400px wide, 12px gutters, 13px/12px title and body, and Small actions. `ROOT_KEY_CONTEXT` is public so the host app can bind root-rem UI scaling below pane-specific browser and terminal zoom. Local addition: `Notification::key` plus `Root::dismiss_notification`/`WindowExt::dismiss_notification`, so a toast raised for a daemon-timed status message can be retired by identity when the daemon clears it (upstream can only clear the whole stack). |
| `input` | **written fresh** | not a port. Upstream's is ~12k lines because it doubles as a code editor (rope, LSP, tree-sitter, masking, OTP, in-input search); ours is a text field. Plain `String` storage, grapheme-safe indexing via `unicode-segmentation`, gpui's `EntityInputHandler` for IME. Owns `zz_input` actions and the `ZzInput` context. `text_align` is applied by the layout's index↔position math, not just at paint, so a centered field hit-tests correctly . upstream's does not. Small fields use an explicit 13px value size so compact form text matches Settings' primary row labels. |
| `code_editor/{state,input,element,mode}` | trimmed | ported from upstream `input/` at the revision above as a sibling of zz's small text field. Renamed the public surface to `CodeEditorState`/`CodeEditor`; keeps a rope buffer, line numbers, soft wrap, IME, single-cursor editing and upstream's tab default. Removed the `Root` downcast and mapped all chrome to zz's semantic theme. zz adds a frame-to-frame `ShapedCache` (element.rs): upstream re-shaped the whole buffer every prepaint; zz re-shapes only when content, wrap width, typography, or theme change, keyed by a `layout_generation` counter every content mutation bumps. |
| `code_editor/{movement,selection,cursor,blink_cursor,change,history,indent,rope_ext}` | close-to-source | upstream rope movement, grapheme-safe selection, cursor blink, edit grouping, undo/redo and indentation mechanics. Multi-cursor behavior is deliberately omitted. History pops only the contiguous version block at the top of a stack; a buried matching version cannot pull unrelated edits into the group. |
| `code_editor/display_map` | close-to-source | upstream buffer/wrap/fold mapping retained as a resync seam. Folding compiles but has no exposed UI in zz's first editor surface. |
| `code_editor/vim` | **zz-original** | not upstream code and not a port of anyone's: upstream has no vim layer, and the whole thing is hand-rolled on the vendored editor with zero new dependencies. Split into a pure core (`parser` keystroke grammar, `motion`, `text_object` . all plain functions over a `Rope`, no GPUI, no `CodeEditorState`) and a thin `executor` that spends the editor's existing primitives. Inert unless `set_vim_enabled(true)`: `CodeEditorState::vim` is `None` by default and every interception is behind that check. The hooks it needed in the ported files are small and marked in place . text input is diverted in `state.rs`'s `replace_text_in_range`/`replace_and_mark_text_in_range`, the bound keys (Escape, Enter, Backspace, Tab, arrows, Home/End, PageUp/Down) ask `vim_key` first, vim's control chords bind against a second `vim` key-context identifier `input.rs` adds only when the layer is on, and `element.rs` paints a block cursor plus an optional relative rail. Five vendored helpers widened from private to `pub(super)` so the executor can spend them instead of reimplementing them: `break_typing_group`, `pause_blink_cursor`, `indent_selection`, `outdent_selection`, `viewport_rows`. One deliberate structural change to `element.rs`: the line-number rail left `ShapedText`/`ShapedCache` and is now shaped per frame for visible rows only, because relative numbering depends on the cursor line and must never invalidate the buffer shaping. |
| `text` | close-to-source | renderer only . `markdown_ast` is `markdown::mdast`, so parsing comes from the `markdown` crate. Dropped upstream's HTML path (`html5ever`), which zz never rendered. Owns the window text-selection host that `overlay::Root` mounts. Heading base sizes are design pixels converted through the live root rem, keeping Markdown headings aligned with scaled body text. `InlineState` retains the hovered glyph across GPUI's frame-owned mouse handlers, so a stationary glyph does not invalidate the window on each move event. |
| `highlighter` | mixed | keeps the upstream non-tree-sitter stub as the default and ports the real parser behind zz-ui's optional `tree-sitter` feature. The registry is trimmed to Rust, Markdown, JSON and TOML; colors come from the already-vendored `HighlightTheme` palettes rather than upstream's JSON theme registry. Incremental editor parses complete synchronously because zz does not vendor upstream's background-parser task. |

## Two gotchas

**Syntax highlighting is target- and feature-gated.** With default features,
including the WASM showcase, `highlighter/syntax.rs` still returns no styles
and its parity test remains the contract. The desktop `zz` app and native UI
showcase enable `zz-ui/tree-sitter`, which activates the vendored parser for
Rust, Markdown, JSON and TOML. Both paths use the same `HighlightTheme` colour
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

## Conventions

- **Close-to-source ports** keep upstream's structure so a future re-sync is a
  small diff. They carry a module-level
  `#![allow(clippy::pedantic, clippy::style, clippy::complexity)]` because they
  follow upstream's style, not this workspace's pedantic lints. Correctness,
  perf and suspicious lints stay active.
- **Trimmed rewrites** port only the API the app calls and document the
  omissions, so a deliberate omission is never mistaken for a missing feature.
- **Chrome colors come from `cx.theme()`**, never a literal; `clippy.toml`
  enforces it, and the ports were corrected where upstream hardcoded a color.

Re-syncing a module against a newer upstream revision means updating the
revision above, re-applying that module's delta, and re-running the workspace
build, the tests, and the UI showcase.

## Cherry-picked since the fork revision

The fork revision above is *not* moved by these: each is an individual upstream
fix re-applied by hand on top of zz's local delta, so the next wholesale
re-sync knows what it already has. Every one carries a regression test.

| Upstream | What it fixes here |
| --- | --- |
| [`3de68cd1`][u1] | `Language::Plain` carried the **JSON** grammar, and `SyntaxHighlighter::new` falls back to `"text"` for anything unregistered . so every ` ```bash ` / ` ```python ` fence in an agent transcript built a JSON parser and parsed the whole block to produce no styles. `LanguageConfig::language` is now `Option`, plain text is grammarless, and `SyntaxHighlighter::build_inert` never parses. |
| [`66cadafb`][u2] + [`98af8912`][u3] | `ContextMenuExt::context_menu` derived its fallback element ID from a **stack address**, so a redraw from the event-dispatch path silently dropped the open menu without a `DismissEvent` and stranded focus on it. Now `#[track_caller]` + `ElementId::CodeLocation`, plus `PopupMenu::previous_focus_handle` so dismiss restores focus without moving action dispatch. |
| [`1a667218`][u4] | `render_list_item` matched only `Paragraph` and `List` and dropped everything else on the floor . a fenced code block, table, blockquote or heading nested in a list item rendered as **nothing**, which is most of what an LLM puts in a numbered list. |
| [`be3c8413`][u5] | `CodeBlock` captured the `HighlightTheme` at *parse* time and memoized styles against it, so markdown code blocks kept the palette that was active when they were parsed across a light/dark switch. The theme is now read at render and travels with the memo. |

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

[upstream]: https://github.com/longbridge/gpui-component
[iconoir]: https://iconoir.com
[simple-icons]: https://simpleicons.org
