//! The catalog landing page.

use gpui::{AnyElement, Context, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, tag::Tag};

use super::{Showcase, gallery, story_stack};
use zz_ui::Colorize as _;

pub(super) fn render(cx: &mut Context<Showcase>) -> AnyElement {
    let primitives = [
        (
            IconName::Inspector,
            "Buttons",
            "Variants, sizes, icon-only, and states.",
        ),
        (
            IconName::Star,
            "Tags & badges",
            "Tag variants and the status badges built on them.",
        ),
        (
            IconName::CaseSensitive,
            "Inputs & selects",
            "Text, number, and dropdown fields.",
        ),
        (
            IconName::Loader,
            "Toggles, keys & feedback",
            "Switches, Kbd hints, spinners, dividers.",
        ),
    ];
    let compositions = [
        (
            IconName::PanelLeft,
            "Navigation",
            "Host-tree rows, the sidebar controls, the titlebar strip's chips, and the tmux status section.",
        ),
        (
            IconName::SquareTerminal,
            "Panes & terminal",
            "Pane indicators and terminal overlays.",
        ),
        (
            IconName::GalleryVerticalEnd,
            "Commands & choosers",
            "Palette rows and chooser rows.",
        ),
        (
            IconName::Globe,
            "Browser",
            "Toolbar controls, address bar, recovery states.",
        ),
        (
            IconName::File,
            "Code editor",
            "The rope-backed editor with syntax highlighting.",
        ),
        (
            IconName::Bot,
            "Agent",
            "The pane header and flat ACP transcript rows.",
        ),
        (
            IconName::Settings,
            "Settings",
            "Cards, rows, provenance, and reset controls.",
        ),
        (
            IconName::Bell,
            "Dialogs & notifications",
            "The shared confirmations, the prompt dialogs, and toast tones.",
        ),
    ];

    story_stack()
        .child(
            gallery(
                "Every atomic piece, on its own",
                "This catalog is the inventory of individual UI pieces the app is built from. Each page renders one kind of piece at a time, in the states that actually occur: no assembled screens. The primitives are the zz-ui widget layer; the compositions are the pieces the app builds from them, imported from the same shared crate the desktop app uses.",
                cx,
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(Tag::primary().child("4 primitive galleries"))
                    .child(Tag::secondary().child("8 composition galleries"))
                    .child(Tag::success().child("live GPUI / WASM"))
                    .child(Tag::primary().child("shared with desktop")),
            ),
        )
        .child(family_gallery("Primitives", &primitives, cx))
        .child(family_gallery("Compositions", &compositions, cx))
        .into_any_element()
}

fn family_gallery(
    title: &'static str,
    families: &[(IconName, &'static str, &'static str)],
    cx: &mut Context<Showcase>,
) -> gpui::Div {
    gallery(
        title,
        "Pick a page from the sidebar to inspect each piece.",
        cx,
    )
    .child(
        div()
            .flex()
            .flex_wrap()
            .gap_3()
            .children(families.iter().map(|(icon, name, description)| {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w(px(320.0))
                    .min_h(px(96.0))
                    .p_4()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_medium()
                            .child(Icon::new(icon.clone()).small())
                            .child(*name),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground.muted())
                            .child(*description),
                    )
            })),
    )
}
