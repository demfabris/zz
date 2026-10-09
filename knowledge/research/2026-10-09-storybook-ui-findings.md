---
type: Research
title: zz-ui and zz-gpui-kit findings from the first storybook pass
description: Visual bugs, crashes and missing APIs found while building every storybook section on 2026-10-09, grouped by area with the story and section that shows each one.
resource: clients/storybook/src/stories
tags: [storybook, zz-ui, zz-gpui-kit, zz-gpui-platform, bugs, wasm]
timestamp: 2026-10-09T00:00:00Z
---

# How to use this

Each item names the storybook section that shows it: `just storybook run`, then open
`#/<story>/<section>`, or `just storybook shot <story>/<section> out.png`. Check light, dark and a
few presets. Fixed items should be deleted from this page.

# Crashes and broken rendering on web

- **Mermaid flowcharts panic on wasm.** merman-render 0.6.2 calls `std::time::Instant::now()` for
  flowchart, state, mindmap and architecture diagrams. An agent answer with a flowchart takes the
  web client down. (`agent/mermaid` uses class and sequence diagrams to stay up.)
- **Mermaid diagrams have no text on web**, and class diagram edges show black wedges. Fonts likely
  never reach the SVG renderer on wasm. (`agent/mermaid`)
- **Bold and medium weights render regular with Inter Variable.** Font ids are per face, a variable
  font has one face, and `CosmicTextSystem::render_glyph_image` builds the swash scaler without a
  `wght` variation (`crates/zz-gpui-platform/src/wgpu/cosmic_text_system.rs`). Likely affects variable fonts
  on Linux too. (`markdown/headings`, `agent/answer`)
- **CJK text renders as empty boxes**: the page loads only Inter and Lilex. (`terminal-grid/wide`)

# Agent pane (`crates/zz-ui/src/agent*.rs`)

- A `/btw` asked during a running turn hides the live state: only the last row counts as live, so
  the running turn reads "Worked" with no spinner (`AgentTimeline::render`, `live_row`).
- Prompts over 512 lines become a 420px scroll box only about 260px wide.
- The expanded steps column is 4px wider than its turn (`render_trace_steps`, `.w_full().ml_1()`).
- `task_tray`: the panel's `.p_1()` overrides the plan's `.px_2()`, so plan items sit 4px from the
  border while task rows sit 12px.
- A narrow pane with many count badges clips the disclosure chevron.
- Think tools count toward "N steps" and "failed" but have no kind badge.
- A canceled turn looks completed when collapsed.
- Subagent steps show "2 steps" with no way to open them.
- Narration in an expanded trace aligns with the icons, not the labels.
- The composer's argument hint sits 2px left of the input text.
- The highlighted permission option is a grey band on the amber card; the 10th option loses its
  digit slot.
- A disabled model picker dims the model name but not the effort label.
- Stop has no disc, unlike Send and Queue; thumbnail remove (×) has no backing.
- A non-side reply bubble looks like a user prompt.
- The "1-9 picks" hint shows on free-text and secret questions.
- "Waiting for you" yellow has low contrast in light theme.
- Once the answer streams, the live header reads "Worked 2s" as if finished.

# Workspace chrome (`crates/zz-ui/src/{navigation,pane,browser,settings}*`)

- Tree labels cut without "…": `workspace_tree_row` sets the ellipsis on the wrapper, not the label
  div; hidden hover actions also take label width. (`sidebar/tree-rows`)
- Hidden actions take title space: inactive terminal headers cut titles early; browser tabs at the
  112px minimum keep about 45px for the label. (`panes/terminal-header`, `browser/tabs`)
- Pane deck "+N" card reads "-3": the card before it covers the "+". (`status-bar/pane-deck`)
- The status bar at about 800px drops window names entirely. (`status-bar/bar`)
- The tab strip does not scroll to the active tab on first render: the scroll request runs before
  the scroller knows its size (zz-gpui `div.rs`). (`browser/tabs`)
- Display-panes cards are see-through. (`panes/display-panes`)
- The Panes settings preview prints "❯", which Lilex lacks; the status bar preview hard-codes
  "v0.9.0". (`settings/panes-page`)
- Pane tags come in two sizes (28px with 13px text, and 11px); Unzoom and Copy mode share an icon.
- Agent badge dots have no outline and the "working" grey is nearly invisible.
- A bell-only window pill dot sits inside an empty activity slot.
- The agents button looks enabled while disconnected; both status buttons always carry the fill.
- `FloatingSurface` long titles run into the frame corner.
- Four preset names are cut ("Catppuccin Moc…") with no tooltip. (`settings/palettes`)
- Light theme: the active-pane glow tints the terminal so it looks dimmer than an inactive pane;
  the Switch thumb is near black.
- The browser error panel breaks URLs mid-scheme ("http:/ /localhost").
- The web client's unsupported-pane toolbar uses 24px buttons, not the 28px browser ones
  (`clients/app/src/app.rs`, `unsupported_pane`).

# Commands, terminal and phone

- Selected palette rows are hard to read: near invisible on catppuccin-mocha's lavender, dark on
  saturated blue in light theme. Fuzzy-match letters only turn semibold.
- Running and offline status dots become the same hollow ring when selected.
- Footer hints mix glyphs and words (`↑↓ ↵ esc` next to `Backspace`); chooser key text is bigger
  than its label; wasm spells cmd as "Win".
- Numeric, key and single prompts show the "Enter a value…" placeholder; an empty prompt prefix
  leaves a gap.
- Which-key caps wider than the column clip on the left; one wide cap widens every group; `fit`
  can leave group titles stranded with no scroll cue.
- The phone bindings list has no shared cap column, so labels zig-zag.
- Chooser TAGGED badges sit after the age column; the two choosers style tags differently.
- The floating menu selected row is barely visible in dark theme.
- tmux theme colours: blue, cyan and magenta resolve to foreground; lightgrey is darker than
  darkgrey in dark mode; white and black swap in light mode. Underline variants all draw straight
  in UI text; named colours use the fixed xterm palette.
- Terminal: palette blue is too dark on the default background; `https://` splits into
  `https: //` because cells break the Lilex ligature; a hovered link draws two underlines; heavy
  and double box lines do not join.
- Git letters in the path picker are all muted, including conflicted "C"; the add-host error uses
  the warning colour.

# Kit (`crates/zz-gpui-kit`)

- CodeEditor: horizontally scrolled code paints over the line-number rail; `set_selected_range`
  scrolls a visible line to the top; the focus border uses `foreground.outline()` while Input uses
  accent; hidden line numbers leave text flush on the border.
- PopupMenu `SelectDown` from no highlight lands on a label row.
- Select anchors to the wrapper's bottom-right, so a stretched select opens at the far edge; its
  max height is half the window.
- Markdown h5 and h6 are smaller than body text; narrow table cells break words mid-word and leave
  empty inline-code chips.
- Tooltip has no max width or wrapping and covers part of its trigger.
- Contrast: Warning button and custom variants in light theme; ListItem hovered and selected look
  the same and muted detail is unreadable on accent; nord's highlighted menu row is light on light.
- Label-only loading buttons show no spinner; selected Ghost and Default look heavy in light theme.
- NumberInput does not dim minus at the minimum, and its disabled state is dimmed twice.
- Multi-line inputs and dialog bodies that scroll show no scrollbar or other cue.
- Stacked dialogs: the lower dialog is not dimmed and its title peeks above the top one; the dark
  scrim is barely visible.
- ColorPicker: disabled swatch not dimmed, sizes only change the hit area, the grid does not mark
  the current colour, dark swatches vanish on the dark popover, inherited looks like an override.
- Custom dialog footers need `.small()` buttons to match the default footer.

# Missing APIs the stories worked around

- No controlled open for Popover, DropdownMenu, ContextMenu, ColorPicker and the model and mode
  pickers; stories send synthetic pointer events.
- `PopupMenu::set_selected_index` is crate-private; stories focus the menu and dispatch
  `zz_menu::SelectDown`.
- Select opens only through focus plus `zz_select::Confirm`; DiscreteSlider is not a tab stop.
- The agent turn clock runs on wall time, so finished turns cannot show a duration.
- The title editor's edit mode and the rewind button are reachable only by pointer.
- The palette's input and recent-commands history are private, so typed queries need real
  keystrokes.
- `zz_ui` does not re-export `sheet` (reachable through `zz_ui::compact`); Notification has a fixed
  width.
- The status bar preview is private; the whole `status_bar_page` is shown instead.
