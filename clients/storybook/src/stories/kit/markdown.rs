use std::sync::Arc;

use zz_gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _,
    Render, StyleRefinement, Styled as _, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    text::{TextView, TextViewState, TextViewStyle},
};

use super::support::{Probe, after_first_frame, when_visible};
use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "markdown",
    name: "Markdown",
    group: "Kit",
    summary: "TextView renders markdown with the theme's syntax colors. The agent pane, help and release notes all go through it.",
    sections: &[
        Section {
            id: "headings",
            name: "Headings",
            summary: "Six heading levels from a 14px base, each followed by body text to show the rhythm.",
            build: |_, cx| stateless(headings, cx),
        },
        Section {
            id: "inline",
            name: "Inline marks",
            summary: "Bold, italic, strikethrough, inline code, links and hard breaks inside one paragraph.",
            build: |_, cx| stateless(inline, cx),
        },
        Section {
            id: "blocks",
            name: "Blocks",
            summary: "Block quotes, nested lists, ordered lists, task items and a rule.",
            build: |_, cx| stateless(blocks, cx),
        },
        Section {
            id: "code",
            name: "Code blocks",
            summary: "Fenced blocks with tree-sitter highlighting for Rust, JSON, TOML and markdown, one without a language, and one with code_block_actions.",
            build: |_, cx| stateless(code, cx),
        },
        Section {
            id: "tables",
            name: "Tables",
            summary: "Column alignment, inline marks in cells, and a wide table that scrolls inside its border.",
            build: |_, cx| stateless(tables, cx),
        },
        Section {
            id: "selection",
            name: "Selection",
            summary: "A selectable TextView with select_all applied. The selection paints across paragraphs, list items and code.",
            build: |window, cx| Selected::view(window, cx),
        },
        Section {
            id: "streaming",
            name: "Streaming",
            summary: "TextView::new(&state).streaming(true) while text arrives through push_str. With motion on, the newest rows fade in under the veil.",
            build: |window, cx| Streaming::view(window, cx),
        },
    ],
};

fn style(cx: &App) -> TextViewStyle {
    TextViewStyle {
        highlight_theme: Arc::clone(&cx.theme().highlight_theme),
        is_dark: cx.theme().is_dark(),
        ..TextViewStyle::default()
    }
}

fn markdown(id: &'static str, source: &'static str, cx: &App) -> TextView {
    TextView::markdown(id, source).style(style(cx))
}

const HEADINGS: &str = "# Heading one\nSessions hold windows, windows hold panes.\n\n## Heading two\nThe daemon owns every PTY.\n\n### Heading three\nClients attach and detach freely.\n\n#### Heading four\nKeys go through the prefix table first.\n\n##### Heading five\nCopy mode is explicit.\n\n###### Heading six\nThe status bar reads tmux formats.";

const INLINE: &str = "Plain text with **bold**, *italic*, ***bold italic***, ~~struck~~ and `inline code`. A [link to the docs](https://zzmux.sh) and a bare https://zzmux.sh/docs autolink.\nA soft break joins this line,  \nwhile two trailing spaces force a hard break.\n\nA second paragraph mixes `zz attach -t work` with **`bold code`** and *[an italic link](https://zzmux.sh)*.";

const BLOCKS: &str = "> The daemon outlives the app.\n>\n> > A nested quote keeps its own rail.\n\n- Sessions\n  - work\n  - scratch\n    - logs\n- Windows\n\n1. Install zz\n2. Run `zz attach`\n3. Split with the prefix\n\n- [x] Copy mode redesign\n- [x] Session resurrect\n- [ ] Editor pane LSP\n\n---\n\nText after the rule.";

const CODE: &str = "```rust\nfn split(pane: &Pane, axis: Axis) -> Result<PaneId> {\n    let size = pane.size().half(axis);\n    pane.session().spawn(size, \"zsh\")\n}\n```\n\n```json\n{ \"theme\": \"nord\", \"radius\": 6, \"motion\": false, \"panes\": [1, 2, 3] }\n```\n\n```toml\n[status]\nleft = \"#S\"\ninterval = 5\n```\n\n```markdown\n# Notes\n- **bold** and `code`\n```\n\n```\nplain text with no language\n```";

const ACTIONS: &str = "```rust\nlet pane = session.active_pane();\n```";

const TABLES: &str = "| Command | Key | Notes |\n|:--|:--:|--:|\n| New window | `C-a c` | **often** |\n| Split right | `C-a %` | keeps cwd |\n| Detach | `C-a d` | daemon stays |\n\n| Option | Default | Scope | Type | Description | Since | Example |\n|---|---|---|---|---|---|---|\n| status-left | `#S` | session | string | Text at the left of the status bar | 0.1 | `#[bold]#S` |\n| history-limit | 2000 | pane | number | Lines of scrollback each pane keeps | 0.1 | 50000 |";

fn headings(_: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .w(px(640.0))
        .child(markdown("md-headings", HEADINGS, cx))
        .into_any_element()
}

fn inline(_: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .w(px(640.0))
        .child(markdown("md-inline", INLINE, cx))
        .into_any_element()
}

fn blocks(_: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .w(px(640.0))
        .child(markdown("md-blocks", BLOCKS, cx))
        .into_any_element()
}

fn code(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "rust, json, toml, markdown, no language",
            div().w(px(640.0)).child(markdown("md-code", CODE, cx)),
        )
        .state(
            "code_block_actions",
            div()
                .w(px(640.0))
                .child(
                    markdown("md-code-actions", ACTIONS, cx).code_block_actions(|_, _, _| {
                        div().absolute().top_1().right_1().child(
                            Button::new("md-copy")
                                .icon(IconName::Copy)
                                .ghost()
                                .xsmall()
                                .tooltip("Copy"),
                        )
                    }),
                ),
        )
        .into_any_element()
}

fn tables(_: &mut Window, cx: &mut App) -> AnyElement {
    let mut table = StyleRefinement::default();
    table.overflow.x = Some(zz_gpui::Overflow::Scroll);
    let scrolling =
        TextView::markdown("md-tables-scroll", TABLES).style(TextViewStyle { table, ..style(cx) });
    states()
        .state(
            "fit to width",
            div().w(px(640.0)).child(markdown("md-tables", TABLES, cx)),
        )
        .state(
            "overflow_x scroll on the table style",
            div().w(px(640.0)).child(scrolling),
        )
        .into_any_element()
}

struct Selected {
    state: Entity<TextViewState>,
    bounds: Probe,
}

impl Selected {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let state = cx.new(|cx| TextViewState::markdown(BLOCKS, cx));
        let bounds = Probe::default();
        let target = state.clone();
        when_visible(&bounds, window, move |_, cx| {
            target.update(cx, TextViewState::select_all);
        });
        cx.new(|_| Self { state, bounds }).into()
    }
}

impl Render for Selected {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .w(px(640.0))
            .child(TextView::new(&self.state).style(style(cx)).selectable(true))
            .child(self.bounds.measure())
    }
}

const STREAM_HEAD: &str = "Splitting the pane now. The new pane inherits the working directory, so `cargo` keeps its target.\n\n";
const STREAM_TAIL: &str =
    "1. Read the layout\n2. Split along the long edge\n3. Start `zsh` in the new pane and";

struct Streaming {
    state: Entity<TextViewState>,
}

impl Streaming {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let state = cx.new(|cx| TextViewState::markdown(STREAM_HEAD, cx));
        let target = state.clone();
        after_first_frame(window, move |_, cx| {
            target.update(cx, |state, cx| state.push_str(STREAM_TAIL, cx));
        });
        cx.new(|_| Self { state }).into()
    }
}

impl Render for Streaming {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(640.0))
            .child(TextView::new(&self.state).style(style(cx)).streaming(true))
    }
}
