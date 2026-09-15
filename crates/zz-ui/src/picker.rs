use crate::{
    ActiveTheme as _, CHROME_GAP, Colorize as _, Icon, IconName, Sizable as _, StyledExt as _,
    h_flex,
    input::{Input, InputState},
    tag::Tag,
    v_flex,
};
use gpui::{
    AnimationElement, App, Div, ElementId, Entity, FontWeight, MouseButton, SharedString, Stateful,
    div, prelude::*, px, relative,
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

pub fn picker_modal(id: impl Into<ElementId>, cx: &App) -> AnimationElement<Stateful<Div>> {
    picker_modal_sized(id, 660.0, cx)
}

pub fn picker_modal_sized(
    id: impl Into<ElementId>,
    width: f32,
    cx: &App,
) -> AnimationElement<Stateful<Div>> {
    let id = id.into();
    let surface = v_flex()
        .id(id.clone())
        .relative()
        .w(relative(0.92))
        .max_w(px(width))
        .h(relative(0.82))
        .min_h(px(240.0))
        .max_h(px(720.0))
        .overflow_hidden()
        .popover_style(cx)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    crate::widget::foundation::surface_enter(surface, (id, "picker-open"), px(0.0))
}

pub fn picker_header(cx: &App) -> Div {
    v_flex()
        .flex_none()
        .gap(px(CHROME_GAP))
        .border_b(px(0.5))
        .border_color(cx.theme().foreground.opacity(0.1))
        .p(px(8.0))
}

pub fn picker_search(input: &Entity<InputState>, cx: &App) -> Div {
    div().w_full().child(
        Input::new(input)
            .small()
            .h(px(32.0))
            .w_full()
            .min_w_0()
            .text_size(crate::rems_from_px(12.0))
            .prefix(
                Icon::new(IconName::Search)
                    .xsmall()
                    .text_color(cx.theme().foreground.muted()),
            ),
    )
}

pub fn picker_list() -> Div {
    v_flex()
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .p(px(8.0))
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
        .border_t(px(0.5))
        .border_color(cx.theme().foreground.opacity(0.1))
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
        .rounded(cx.theme().menu_radius())
        .border(px(0.5))
        .border_color(gpui::transparent_white())
        .px_2p5()
        .cursor_pointer()
        .when(selected, |row| row.selection_highlight(cx))
        .when(!selected, |row| {
            row.hover(|row| row.selection_highlight(cx))
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
        .child(
            Icon::new(icon)
                .xsmall()
                .opacity(if selected { 1.0 } else { 0.8 }),
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
                    .opacity(if selected { 1.0 } else { 0.8 })
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
                                .bg(cx.theme().success.opaque())
                                .px_1()
                                .text_size(crate::rems_from_px(8.0))
                                .text_color(cx.theme().success.on())
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
