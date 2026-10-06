use gpui::{
    AnyElement, App, Div, ParentElement as _, Pixels, SharedString, Styled as _, div,
    linear_color_stop, linear_gradient, prelude::*,
};

use crate::{ActiveTheme as _, Colorize as _, Icon, IconName, rems_from_px};

pub const COMPACT_PANE_HEADER_HEIGHT: f32 = 36.0;

const PADDING_X: f32 = 12.0;
const ACTIONS_RIGHT: f32 = 6.0;
const ACTION_SLOT: f32 = 28.0;
const SHADE_ALPHA: f32 = 0.6;

pub fn compact_pane_header(
    icon: IconName,
    title: impl Into<SharedString>,
    actions: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> Div {
    let actions: Vec<AnyElement> = actions.into_iter().collect();
    let reserve = if actions.is_empty() {
        PADDING_X
    } else {
        ACTIONS_RIGHT + ACTION_SLOT * actions.len() as f32 + 4.0
    };
    div()
        .debug_selector(|| "compact-pane-header".to_owned())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .h(rems_from_px(COMPACT_PANE_HEADER_HEIGHT))
        .pl(rems_from_px(PADDING_X))
        .pr(rems_from_px(reserve))
        .gap(rems_from_px(8.0))
        .font_family(cx.theme().font_family.clone())
        .text_size(rems_from_px(13.0))
        .line_height(rems_from_px(16.0))
        .text_color(cx.theme().foreground.muted())
        .child(
            div()
                .flex_none()
                .relative()
                .top(rems_from_px(0.5))
                .opacity(0.8)
                .child(Icon::new(icon).size(rems_from_px(14.0))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(title.into()),
        )
        .when(!actions.is_empty(), |header| {
            header.child(
                div()
                    .debug_selector(|| "compact-pane-header-actions".to_owned())
                    .absolute()
                    .top_0()
                    .right(rems_from_px(ACTIONS_RIGHT))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(rems_from_px(4.0))
                    .children(actions),
            )
        })
}

pub fn top_shade(height: Pixels, cx: &App) -> Div {
    let shade = cx.theme().background.opaque();
    div()
        .debug_selector(|| "compact-top-shade".to_owned())
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(height)
        .bg(linear_gradient(
            180.0,
            linear_color_stop(shade.alpha(SHADE_ALPHA), 0.0),
            linear_color_stop(shade.alpha(0.0), 1.0),
        ))
}
