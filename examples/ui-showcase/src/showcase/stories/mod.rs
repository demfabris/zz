//! Specimen galleries: each app piece rendered on its own, in the states that
//! occur, rather than as a composed screen.

mod agent;
mod browser;
mod commands;
mod editor;
mod feedback;
mod navigation;
mod overview;
mod panes;
mod primitives;
mod settings;

use gpui::{AnyElement, App, Context, Div, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::theme::Colorize as _;
use zz_ui::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, v_flex};

use super::{Showcase, StoryId};

pub(super) use agent::ThreadFixture;

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    match showcase.active {
        StoryId::Overview => overview::render(cx),
        StoryId::Buttons => primitives::buttons(cx),
        StoryId::TagsBadges => primitives::tags_badges(cx),
        StoryId::InputsSelects => primitives::inputs_selects(showcase, cx),
        StoryId::TogglesKeys => primitives::toggles_keys(cx),
        StoryId::Navigation => navigation::render(cx),
        StoryId::PanesTerminal => panes::render(cx),
        StoryId::CommandsChoosers => commands::render(showcase, cx),
        StoryId::Browser => browser::render(showcase, cx),
        StoryId::Editor => editor::render(showcase, cx),
        StoryId::Agent => agent::render(showcase, cx),
        StoryId::Settings => settings::render(showcase, cx),
        StoryId::Feedback => feedback::render(showcase, cx),
    }
}

pub(super) fn story_stack() -> Div {
    v_flex().gap_6().w_full()
}

pub(super) fn gallery(title: &'static str, description: &'static str, cx: &App) -> Div {
    v_flex()
        .gap_3()
        .w_full()
        .p_5()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.raised(1))
        .child(
            v_flex()
                .gap_0p5()
                .child(div().text_base().font_medium().child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground.muted())
                        .child(description),
                ),
        )
}

pub(super) fn specimens() -> Div {
    div().flex().flex_wrap().gap_3().items_start()
}

pub(super) fn specimen(label: impl Into<String>, piece: impl IntoElement, cx: &App) -> Div {
    specimen_cell(label, stage(cx).child(piece), cx)
}

pub(super) fn specimen_block(label: impl Into<String>, piece: impl IntoElement, cx: &App) -> Div {
    specimen_cell(label, stage(cx).w_full().justify_start().child(piece), cx).w_full()
}

pub(super) fn specimen_over_terminal(
    label: impl Into<String>,
    width: f32,
    height: f32,
    piece: impl IntoElement,
    cx: &App,
) -> Div {
    let stage = div()
        .relative()
        .w(px(width))
        .h(px(height))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .font_family(cx.theme().mono_font_family.clone())
        .child(mock_terminal(cx))
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(piece),
        );
    specimen_cell(label, stage, cx)
}

fn specimen_cell(label: impl Into<String>, stage: Div, cx: &App) -> Div {
    v_flex().gap_2().child(stage).child(
        div()
            .text_xs()
            .text_color(cx.theme().foreground.muted())
            .child(label.into()),
    )
}

fn stage(cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .min_h(px(56.0))
        .min_w(px(72.0))
        .px_4()
        .py_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border.subtle())
        .bg(cx.theme().background)
}

pub(super) fn mock_terminal(cx: &App) -> Div {
    let lines = [
        ("$ cargo watch -x check", false),
        ("[Running 'cargo check']", true),
        ("    Checking zz v0.1.0", false),
        ("    Finished dev profile in 1.42s", true),
        ("$ ", false),
    ];

    div()
        .relative()
        .flex()
        .flex_col()
        .size_full()
        .gap(px(5.0))
        .p(px(14.0))
        .overflow_hidden()
        .bg(cx.theme().background)
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(11.0))
        .text_color(cx.theme().foreground)
        .children(lines.into_iter().map(|(line, accent)| {
            div()
                .whitespace_nowrap()
                .when(accent, |this| this.text_color(cx.theme().success))
                .child(line)
        }))
        .child(
            div()
                .absolute()
                .left(px(27.0))
                .bottom(px(16.0))
                .w(px(7.0))
                .h(px(14.0))
                .bg(cx.theme().foreground),
        )
}

pub(super) fn mock_browser_page(cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(cx.theme().background.raised(2))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size_11()
                .rounded_full()
                .bg(cx.theme().background)
                .child(Icon::new(IconName::Globe).small()),
        )
        .child(div().text_sm().font_medium().child("GPUI on the web"))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().foreground.muted())
                .child("Chromium content fixture"),
        )
}
