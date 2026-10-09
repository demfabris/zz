use zz_gpui::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
    prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, IconName,
    button::{Button, ButtonVariants as _},
    chooser::{
        CHOOSER_ROW_HEIGHT, ChooserDimensions, ChooserHint, ChooserModal, ChooserPaneKind,
        ChooserRowTheme, ChooserSearch, buffer_chooser_row, chooser_has_key_gutter,
        chooser_subtitle, tree_chooser_row,
    },
    v_flex,
};

use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "chooser",
    name: "Chooser",
    group: "Commands",
    summary: "The modal the tmux choosers draw: choose-tree, choose-pane and choose-buffer, with the search, prompt and help states the daemon can put them in.",
    sections: &[
        Section {
            id: "tree",
            name: "Tree",
            summary: "choose-tree: sessions, windows and panes with disclosures, the active path checked, and the key gutter tmux prints.",
            build: |_, cx| stateless(tree, cx),
        },
        Section {
            id: "panes",
            name: "Pane kinds",
            summary: "choose-pane with every pane kind, two panes tagged.",
            build: |_, cx| stateless(panes, cx),
        },
        Section {
            id: "search",
            name: "Search",
            summary: "A search swaps the footer hints for the query. A filter that matches nothing says so in the subtitle.",
            build: |_, cx| stateless(search, cx),
        },
        Section {
            id: "notices",
            name: "Prompt and help",
            summary: "The confirm prompt the chooser owns, and the help notice that swallows the next key.",
            build: |_, cx| stateless(notices, cx),
        },
        Section {
            id: "buffers",
            name: "Buffers",
            summary: "choose-buffer: name, preview, size and age, with tagged buffers.",
            build: |_, cx| stateless(buffers, cx),
        },
    ],
};

const TREE_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["left", "right"],
        label: "expand",
    },
    ChooserHint {
        keys: &["enter"],
        label: "choose",
    },
    ChooserHint {
        keys: &["/"],
        label: "search",
    },
    ChooserHint {
        keys: &["escape"],
        label: "close",
    },
];

const BUFFER_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["enter"],
        label: "paste",
    },
    ChooserHint {
        keys: &["d"],
        label: "delete",
    },
    ChooserHint {
        keys: &["/"],
        label: "search",
    },
    ChooserHint {
        keys: &["escape"],
        label: "close",
    },
];

struct TreeRow {
    key: &'static str,
    target: &'static str,
    label: &'static str,
    detail: &'static str,
    depth: u8,
    disclosure: &'static str,
    kind: Option<ChooserPaneKind>,
    active: bool,
    tagged: bool,
}

const fn node(
    key: &'static str,
    target: &'static str,
    label: &'static str,
    detail: &'static str,
    depth: u8,
    disclosure: &'static str,
    active: bool,
) -> TreeRow {
    TreeRow {
        key,
        target,
        label,
        detail,
        depth,
        disclosure,
        kind: None,
        active,
        tagged: false,
    }
}

const fn leaf(
    key: &'static str,
    target: &'static str,
    label: &'static str,
    detail: &'static str,
    kind: ChooserPaneKind,
    active: bool,
    tagged: bool,
) -> TreeRow {
    TreeRow {
        key,
        target,
        label,
        detail,
        depth: 2,
        disclosure: "",
        kind: Some(kind),
        active,
        tagged,
    }
}

const WINDOWS: &[TreeRow] = &[
    node("0", "$1", "zz", "2 windows", 0, "▾", true),
    node("1", "@1", "1:editor", "2 panes", 1, "▾", true),
    leaf(
        "2",
        "%10",
        "nvim",
        "~/dev/zz",
        ChooserPaneKind::Terminal,
        true,
        false,
    ),
    leaf(
        "3",
        "%11",
        "cargo watch",
        "~/dev/zz",
        ChooserPaneKind::Terminal,
        false,
        false,
    ),
    node("4", "@2", "2:agents", "2 panes", 1, "▸", false),
    node("5", "$2", "notes", "1 window", 0, "▸", false),
    node("6", "$3", "dotfiles", "3 windows", 0, "▸", false),
];

const PANES: &[TreeRow] = &[
    node("", "$1", "zz", "1 window", 0, "▾", true),
    node("", "@2", "2:agents", "4 panes", 1, "▾", true),
    leaf(
        "",
        "%12",
        "zsh",
        "~/dev/zz",
        ChooserPaneKind::Terminal,
        false,
        true,
    ),
    leaf(
        "",
        "%13",
        "localhost:8097",
        "zz storybook",
        ChooserPaneKind::Browser,
        false,
        false,
    ),
    leaf(
        "",
        "%14",
        "claude",
        "Reviewing the diff",
        ChooserPaneKind::Agent,
        true,
        true,
    ),
    leaf(
        "",
        "%15",
        "palette_view.rs",
        "crates/zz-ui/src/command",
        ChooserPaneKind::Editor,
        false,
        false,
    ),
];

fn close(id: &'static str) -> Button {
    Button::compact_icon(id, IconName::Xmark)
        .ghost()
        .flat()
        .tooltip("Close")
}

fn tree_rows(id: &'static str, rows: &[TreeRow], selected: usize, cx: &App) -> impl IntoElement {
    let theme = ChooserRowTheme::from_theme(cx);
    let font = cx.theme().mono_font_family.clone();
    let gutter = chooser_has_key_gutter(rows.iter().map(|row| row.key));
    v_flex()
        .w_full()
        .children(rows.iter().enumerate().map(|(index, row)| {
            tree_chooser_row(
                id,
                index,
                row.key,
                gutter,
                row.target,
                row.label,
                row.detail,
                row.depth,
                row.disclosure,
                row.kind,
                row.active,
                row.tagged,
                index == selected,
                theme,
                font.clone(),
            )
        }))
}

fn frame(id: &'static str, rows: usize, notices: usize, modal: ChooserModal) -> AnyElement {
    let height = 106.0 + rows.min(10) as f32 * CHOOSER_ROW_HEIGHT + notices as f32 * 36.0;
    div()
        .id(id)
        .relative()
        .w_full()
        .h(px(height))
        .flex()
        .justify_center()
        .child(modal)
        .into_any_element()
}

struct Tree<'a> {
    id: &'static str,
    title: &'static str,
    subtitle: SharedString,
    rows: &'a [TreeRow],
    selected: usize,
    search: Option<ChooserSearch>,
    prompt: Option<SharedString>,
    help: bool,
}

fn tree_modal(tree: Tree<'_>, cx: &App) -> AnyElement {
    let notices = usize::from(tree.help) + usize::from(tree.prompt.is_some());
    let count = tree.rows.len();
    frame(
        tree.id,
        count,
        notices,
        ChooserModal::new(
            tree.id,
            tree.title,
            tree.subtitle,
            ChooserDimensions {
                max_width: 600.0,
                row_count: count,
            },
            tree_rows(tree.id, tree.rows, tree.selected, cx),
            close(tree.id),
            cx.theme().mono_font_family.clone(),
        )
        .prompt(tree.prompt)
        .help(tree.help)
        .search(tree.search)
        .hints(TREE_HINTS),
    )
}

fn tree(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    tree_modal(
        Tree {
            id: "chooser-tree",
            title: "Choose window",
            subtitle: chooser_subtitle("7 sessions and windows", false).into(),
            rows: WINDOWS,
            selected: 3,
            search: None,
            prompt: None,
            help: false,
        },
        cx,
    )
}

fn panes(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    tree_modal(
        Tree {
            id: "chooser-panes",
            title: "Choose pane",
            subtitle: chooser_subtitle("6 sessions, windows, and panes", false).into(),
            rows: PANES,
            selected: 4,
            search: None,
            prompt: None,
            help: false,
        },
        cx,
    )
}

fn search(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .state(
            "searching forward",
            tree_modal(
                Tree {
                    id: "chooser-search",
                    title: "Choose window",
                    subtitle: chooser_subtitle("4 sessions and windows", false).into(),
                    rows: &WINDOWS[..4],
                    selected: 1,
                    search: Some(ChooserSearch {
                        prefix: "/".into(),
                        value: "edit".into(),
                    }),
                    prompt: None,
                    help: false,
                },
                cx,
            ),
        )
        .state(
            "filter with no matches, searching backward",
            tree_modal(
                Tree {
                    id: "chooser-filter",
                    title: "Choose window",
                    subtitle: chooser_subtitle("7 sessions and windows", true).into(),
                    rows: WINDOWS,
                    selected: 0,
                    search: Some(ChooserSearch {
                        prefix: "?".into(),
                        value: "#{==:#{pane_current_command},htop}".into(),
                    }),
                    prompt: None,
                    help: false,
                },
                cx,
            ),
        )
        .into_any_element()
}

fn notices(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    states()
        .state(
            "confirm prompt",
            tree_modal(
                Tree {
                    id: "chooser-prompt",
                    title: "Choose pane",
                    subtitle: chooser_subtitle("6 sessions, windows, and panes", false).into(),
                    rows: PANES,
                    selected: 2,
                    search: None,
                    prompt: Some("Kill 2 tagged panes? (y/n)".into()),
                    help: false,
                },
                cx,
            ),
        )
        .state(
            "help",
            tree_modal(
                Tree {
                    id: "chooser-help",
                    title: "Choose window",
                    subtitle: chooser_subtitle("4 sessions and windows", false).into(),
                    rows: &WINDOWS[..4],
                    selected: 1,
                    search: None,
                    prompt: None,
                    help: true,
                },
                cx,
            ),
        )
        .into_any_element()
}

const BUFFERS: &[(&str, &str, &str, &str, &str, bool)] = &[
    (
        "0",
        "buffer0",
        "cargo test -p zz-ui --lib palette",
        "33 B",
        "now",
        false,
    ),
    (
        "1",
        "buffer1",
        "error[E0308]: mismatched types --> src/stories/console/palette.rs:336:20",
        "1.2 KB",
        "2m",
        true,
    ),
    (
        "2",
        "notes",
        "TODO: the key gutter reads (M-a) for alt keys",
        "46 B",
        "1h",
        false,
    ),
    (
        "3",
        "buffer3",
        "https://zzmux.sh/docs/choosers",
        "30 B",
        "3d",
        true,
    ),
    (
        "M-a",
        "a-very-long-buffer-name-from-a-script",
        "#!/usr/bin/env bash set -euo pipefail",
        "18.4 KB",
        "12d",
        false,
    ),
];

fn buffers(_: &mut Window, cx: &mut App) -> AnyElement {
    let cx: &App = cx;
    let theme = ChooserRowTheme::from_theme(cx);
    let font = cx.theme().mono_font_family.clone();
    let gutter = chooser_has_key_gutter(BUFFERS.iter().map(|buffer| buffer.0));
    let rows = v_flex().w_full().children(BUFFERS.iter().enumerate().map(
        |(index, (key, name, preview, size, age, tagged))| {
            buffer_chooser_row(
                "chooser-buffer",
                index,
                *key,
                gutter,
                *name,
                *preview,
                *size,
                *age,
                *tagged,
                index == 1,
                theme,
                font.clone(),
            )
        },
    ));
    frame(
        "chooser-buffers",
        BUFFERS.len(),
        0,
        ChooserModal::new(
            "chooser-buffers-modal",
            "Paste buffer",
            chooser_subtitle(format!("{} buffers", BUFFERS.len()), false),
            ChooserDimensions {
                max_width: 640.0,
                row_count: BUFFERS.len(),
            },
            rows,
            close("chooser-buffers-close"),
            font.clone(),
        )
        .hints(BUFFER_HINTS),
    )
}
