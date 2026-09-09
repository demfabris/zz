use gpui::{App, Div, ElementId, IntoElement, Pixels, SharedString, Stateful, div, prelude::*, px};

use crate::{ActiveTheme as _, Colorize as _, h_flex, v_flex};

const ROW_HEIGHT: f32 = 52.0;
const MAX_VISIBLE_ROWS: u8 = 6;

pub fn suggestion_list_height(count: usize) -> Pixels {
    px(ROW_HEIGHT
        * f32::from(
            u8::try_from(count)
                .unwrap_or(MAX_VISIBLE_ROWS)
                .min(MAX_VISIBLE_ROWS),
        ))
}

pub fn suggestion_row(
    id: impl Into<ElementId>,
    name: &str,
    description: Option<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .h(px(ROW_HEIGHT))
        .items_center()
        .gap_3()
        .rounded(cx.theme().radius)
        .px_3()
        .cursor_pointer()
        .when(selected, |row| row.bg(cx.theme().background.hover()))
        .when(!selected, |row| {
            row.hover(|row| row.bg(cx.theme().background.hover()))
        })
        .child(
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
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(format!("/{name}")),
                )
                .when_some(description, |column, description| {
                    column.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(crate::rems_from_px(10.0))
                            .text_color(cx.theme().foreground.muted())
                            .child(description),
                    )
                }),
        )
}

pub fn suggestion_list(rows: impl IntoElement, cx: &App) -> Div {
    v_flex()
        .w_full()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(cx.theme().background.raised(1))
        .p_1()
        .shadow_md()
        .overflow_hidden()
        .child(rows)
}
