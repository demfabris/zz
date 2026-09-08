use std::time::Duration;

use gpui::{
    AnyElement, App, Div, ElementId, EntityId, IntoElement, SharedString, Stateful, Transformation,
    div, ease_in_out, percentage, prelude::*, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, button::Button, h_flex,
    pulse::pulse_phase, v_flex,
};

pub use zz_client::agent_config::{MAX_RENDERED_ERROR_BYTES, rendered_error};

pub fn error_card(error: &str, cx: &App) -> Div {
    v_flex()
        .w_full()
        .gap_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().danger.outline())
        .bg(cx.theme().danger.fill())
        .p_3()
        .text_size(crate::rems_from_px(11.0))
        .child(rendered_error(error))
}

pub fn empty_state(
    message: impl Into<SharedString>,
    busy: bool,
    view: EntityId,
    cx: &mut App,
) -> Div {
    let phase = if busy {
        ease_in_out(pulse_phase(Duration::from_millis(800), view, cx))
    } else {
        0.0
    };
    v_flex()
        .w_full()
        .py(px(48.0))
        .items_center()
        .gap_2()
        .text_size(crate::rems_from_px(12.0))
        .text_color(cx.theme().foreground.muted())
        .when(busy, |column| {
            column.child(
                Icon::new(IconName::Loader)
                    .small()
                    .text_color(cx.theme().foreground.muted())
                    .transform(Transformation::rotate(percentage(phase))),
            )
        })
        .child(message.into())
}

pub fn permission_option(
    id: impl Into<ElementId>,
    index: usize,
    highlighted: bool,
    button: Button,
    cx: &App,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .items_center()
        .gap_2()
        .rounded(cx.theme().radius)
        .px_1()
        .py(px(2.0))
        .when(highlighted, |row| row.bg(cx.theme().background.hover()))
        .when(index < 9, |row| {
            row.child(
                div()
                    .flex_none()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background.raised(2))
                    .px_2()
                    .py(px(2.0))
                    .text_size(crate::rems_from_px(9.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(format!("{}", index + 1)),
            )
        })
        .child(button)
}

pub fn permission_card(
    title: impl Into<SharedString>,
    counter: Option<SharedString>,
    options: Vec<AnyElement>,
    cancel: impl IntoElement,
    cx: &App,
) -> Div {
    v_flex()
        .w_full()
        .gap_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().warning.outline())
        .bg(cx.theme().warning.fill())
        .p_3()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .text_size(crate::rems_from_px(12.0))
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .small()
                        .flex_none()
                        .text_color(cx.theme().warning),
                )
                .child(div().min_w_0().flex_1().child(title.into()))
                .when_some(counter, |row, counter| {
                    row.child(
                        div()
                            .flex_none()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().background.raised(2))
                            .px_2()
                            .py(px(2.0))
                            .text_size(crate::rems_from_px(9.0))
                            .text_color(cx.theme().foreground.muted())
                            .child(counter),
                    )
                }),
        )
        .child(v_flex().w_full().gap_1().children(options))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(crate::rems_from_px(9.0))
                        .text_color(cx.theme().foreground.muted())
                        .child("1-9 picks · enter confirms · esc cancels"),
                )
                .child(cancel),
        )
}
