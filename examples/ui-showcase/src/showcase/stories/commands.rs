//! The command palette and daemon chooser pieces.

use gpui::{AnyElement, App, Context, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::ActiveTheme as _;
use zz_ui::chooser::{
    ChooserHint, ChooserPaneKind, ChooserRowTheme, ChooserSearch, buffer_chooser_row,
    chooser_footer, tree_chooser_row,
};
use zz_ui::command::{command_kind_badge, command_palette_input, command_palette_row};

use super::{Showcase, gallery, specimen_block, specimens, story_stack};
use zz_ui::Colorize as _;

const MONO: &str = "Menlo";

const TREE_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["left", "right"],
        label: "collapse",
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

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Palette input",
                "The prefixed terminal input that heads the command palette and rename prompts.",
                cx,
            )
            .child(specimens().w_full().child(specimen_block(
                "prompt + input",
                div()
                    .w(px(520.0))
                    .child(command_palette_input(&showcase.command_input, ":", MONO, cx)),
                cx,
            ))),
        )
        .child(
            gallery(
                "Completion rows",
                "Selection appears only after keyboard navigation begins; the kind badge distinguishes commands, options, targets, and history.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "command · selected",
                        completion_row("split-window", "Create a terminal pane", "COMMAND", true, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "option",
                        completion_row("-h", "Split along the horizontal axis", "OPTION", false, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "target",
                        completion_row("%3", "browser · GPUI components", "TARGET", false, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "history",
                        completion_row("workspace", "Current value", "HISTORY", false, cx),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Tree chooser rows",
                "The daemon-owned session/window/pane chooser renders these rows with hierarchy, target IDs, type badges, and active/selected state.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "session · expanded",
                        tree_row(0, "$0", "design", "2 windows", 0, true, false, None, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "window · expanded",
                        tree_row(1, "@0", "workspace", "3 panes", 1, true, false, None, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "terminal pane · active + selected",
                        tree_row(2, "%1", "editor", "cargo watch", 2, false, true, Some("TERM"), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "browser pane",
                        tree_row(3, "%2", "GPUI components", "https://gpui.rs", 2, false, false, Some("WEB"), cx),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Paste-buffer rows",
                "The buffer picker shares the chooser frame, swapping hierarchy for name, preview, byte size, and age columns.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "buffer · selected",
                        buffer_row(0, "buffer0004", "cargo test -p zz --lib", "22 B", "now", true, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "buffer",
                        buffer_row(1, "buffer0003", "https://gpui.rs/docs", "20 B", "2m", false, cx),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Chooser footers",
                "The keyboard-hint strip, and the live query strip that replaces it during search.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "hint strip",
                        footer(None, cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "search strip",
                        footer(
                            Some(ChooserSearch {
                                prefix: "/".into(),
                                value: "browser".into(),
                            }),
                            cx,
                        ),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

fn completion_row(
    label: &'static str,
    detail: &'static str,
    kind: &'static str,
    selected: bool,
    cx: &App,
) -> AnyElement {
    command_palette_row(
        format!("cmd-row-{kind}-{label}"),
        label,
        detail,
        command_kind_badge(kind, MONO),
        selected,
        cx.theme().background.hover(),
        cx.theme().foreground.muted(),
        MONO,
    )
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn tree_row(
    index: usize,
    target: &'static str,
    label: &'static str,
    detail: &'static str,
    depth: u8,
    expanded: bool,
    selected: bool,
    kind: Option<&'static str>,
    cx: &App,
) -> AnyElement {
    let pane_kind = kind.map(|kind| {
        if kind == "WEB" {
            ChooserPaneKind::Browser
        } else {
            ChooserPaneKind::Terminal
        }
    });
    tree_chooser_row(
        "cmd-tree-row",
        index,
        index.to_string(),
        true,
        target,
        label,
        detail,
        depth,
        if kind.is_none() {
            if expanded { "▾" } else { "▸" }
        } else {
            ""
        },
        pane_kind,
        target == "%1",
        false,
        selected,
        ChooserRowTheme::from_theme(cx),
        cx.theme().mono_font_family.clone(),
    )
    .into_any_element()
}

fn buffer_row(
    index: usize,
    name: &'static str,
    preview: &'static str,
    size: &'static str,
    age: &'static str,
    selected: bool,
    cx: &App,
) -> AnyElement {
    buffer_chooser_row(
        "cmd-buffer-row",
        index,
        index.to_string(),
        true,
        name,
        preview,
        size,
        age,
        selected,
        ChooserRowTheme::from_theme(cx),
        cx.theme().mono_font_family.clone(),
    )
    .into_any_element()
}

fn footer(search: Option<ChooserSearch>, cx: &App) -> impl IntoElement {
    let hints: &[ChooserHint] = if search.is_some() { &[] } else { TREE_HINTS };
    div()
        .w(px(560.0))
        .max_w_full()
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(chooser_footer(
            search,
            hints,
            cx.theme().mono_font_family.clone(),
            cx,
        ))
}
