use zz_gpui::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    compact::{bottom_sheet, sheet_action, sheet_close, sheet_option},
    h_flex, v_flex,
};

use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "sheet",
    name: "Sheet",
    group: "Kit",
    summary: "The bottom sheet a touch screen opens in place of a dropdown. It rises from the bottom of its parent over a scrim and drags down to dismiss.",
    sections: &[Section {
        id: "bottom-sheet",
        name: "Bottom sheet",
        summary: "Two phone-sized frames: a picker with a checked option and an action row, and a short sheet of commands.",
        build: |_, cx| stateless(sheets, cx),
    }],
};

const LANGUAGES: [&str; 5] = ["English", "Deutsch", "Español", "Português", "Français"];

fn phone(content: impl IntoElement, sheet: impl IntoElement, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .relative()
        .w(px(390.0))
        .h(px(600.0))
        .overflow_hidden()
        .rounded(px(28.0))
        .border_1()
        .border_color(theme.border())
        .bg(theme.background.raised(1))
        .child(content)
        .child(sheet)
}

fn backdrop(title: &'static str, cx: &App) -> impl IntoElement {
    let muted = cx.theme().foreground.muted();
    v_flex()
        .p(px(20.0))
        .gap_3()
        .child(div().text_lg().child(title))
        .children(["work", "scratch", "logs"].into_iter().map(move |name| {
            h_flex()
                .gap_2()
                .text_sm()
                .text_color(muted)
                .child(Icon::new(IconName::SquareTerminal).small())
                .child(name)
        }))
}

fn sheets(_: &mut Window, cx: &mut App) -> AnyElement {
    let picker = bottom_sheet(
        "language-sheet",
        "Language",
        [sheet_close("language-close").into_any_element()],
        v_flex()
            .px(px(8.0))
            .pb(px(8.0))
            .children(LANGUAGES.iter().enumerate().map(|(ix, name)| {
                sheet_option(("language", ix), SharedString::from(*name), ix == 1, cx)
            }))
            .child(sheet_action(
                "language-add",
                IconName::Plus,
                "Add a language",
                cx,
            )),
        px(0.0),
        |_, _| {},
    );
    let commands = bottom_sheet(
        "pane-sheet",
        "Pane",
        [sheet_close("pane-close").into_any_element()],
        v_flex()
            .px(px(8.0))
            .pb(px(8.0))
            .child(sheet_action(
                "pane-split",
                IconName::PanelRight,
                "Split right",
                cx,
            ))
            .child(sheet_action("pane-zoom", IconName::ZoomIn, "Zoom pane", cx))
            .child(sheet_action(
                "pane-close-row",
                IconName::Xmark,
                "Close pane",
                cx,
            )),
        px(0.0),
        |_, _| {},
    );
    states()
        .columns(2)
        .state("picker", phone(backdrop("Settings", cx), picker, cx))
        .state("commands", phone(backdrop("work", cx), commands, cx))
        .into_any_element()
}
