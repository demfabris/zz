use crate::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Icon, IconName, Sizable as _, h_flex,
    input::{Input, InputState},
    tag::Tag,
    v_flex,
};
use gpui::{
    App, Div, ElementId, Entity, FontWeight, MouseButton, SharedString, Stateful, div, prelude::*,
    px, relative,
};

pub fn picker_overlay(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .bg(cx.theme().scrim)
        .occlude()
}

pub fn picker_modal(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    v_flex()
        .id(id)
        .relative()
        .w(relative(0.92))
        .max_w(px(660.0))
        .h(relative(0.82))
        .min_h(px(240.0))
        .max_h(px(720.0))
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.raised(1))
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn picker_header(cx: &App) -> Div {
    v_flex()
        .flex_none()
        .gap(px(CHROME_GAP))
        .border_b_1()
        .border_color(cx.theme().border)
        .p(px(CHROME_GAP))
}

pub fn picker_search(input: &Entity<InputState>, cx: &App) -> Div {
    h_flex()
        .w_full()
        .h(px(32.0))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background.raised(1))
        .px_2p5()
        .child(
            Icon::new(IconName::Search)
                .xsmall()
                .text_color(cx.theme().foreground.muted()),
        )
        .child(
            Input::new(input)
                .small()
                .flex_1()
                .min_w_0()
                .text_size(crate::rems_from_px(12.0))
                .appearance(false)
                .bordered(false)
                .focus_bordered(false),
        )
}

pub fn picker_list() -> Div {
    v_flex()
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .p(px(CHROME_GAP))
}

pub fn picker_empty(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .text_size(crate::rems_from_px(11.0))
        .text_color(cx.theme().foreground.muted())
        .child(label.into())
}

pub fn picker_footer(cx: &App) -> Div {
    h_flex()
        .w_full()
        .min_h(px(40.0))
        .flex_none()
        .gap(px(CHROME_GAP))
        .border_t_1()
        .border_color(cx.theme().border)
        .py(px(CHROME_GAP))
        .pl_4()
        .pr(px(CHROME_GAP))
        .text_size(crate::rems_from_px(10.0))
        .text_color(cx.theme().foreground.muted())
}

pub fn picker_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .items_center()
        .gap_2()
        .rounded(cx.theme().radius)
        .px_2p5()
        .cursor_pointer()
        .when(selected, |row| row.bg(cx.theme().background.hover()))
        .when(!selected, |row| {
            row.hover(|row| row.bg(cx.theme().background.hover()))
        })
}

pub fn directory_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    path_row(id, IconName::Folder, label, selected, cx)
}

pub fn path_row(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    picker_row(id, selected, cx)
        .h(px(26.0))
        .when(selected, |row| {
            row.bg(cx.theme().background.raised(2).wash())
        })
        .child(
            Icon::new(icon)
                .xsmall()
                .text_color(cx.theme().foreground.muted()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(crate::rems_from_px(12.0))
                .child(label.into()),
        )
}

pub fn history_row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    directory: impl Into<SharedString>,
    updated_at: Option<SharedString>,
    selected: bool,
    current: bool,
    cx: &App,
) -> Stateful<Div> {
    picker_row(id, selected, cx).h(px(52.0)).child(
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.0))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(crate::rems_from_px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.into()),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1()
                    .text_size(crate::rems_from_px(9.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(
                        Tag::secondary()
                            .xsmall()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(crate::rems_from_px(9.0))
                            .text_color(cx.theme().foreground.muted())
                            .child(directory.into()),
                    )
                    .when(current, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .rounded(px(999.0))
                                .bg(cx.theme().success.fill())
                                .px_1()
                                .text_size(crate::rems_from_px(8.0))
                                .text_color(cx.theme().success)
                                .child("CURRENT"),
                        )
                    })
                    .child(div().flex_1())
                    .when_some(updated_at, |row, timestamp| {
                        row.child(
                            div()
                                .flex_none()
                                .max_w(px(112.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(timestamp),
                        )
                    }),
            ),
    )
}
