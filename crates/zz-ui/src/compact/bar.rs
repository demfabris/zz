use gpui::{
    App, Div, ElementId, IntoElement, MouseButton, ParentElement as _, SharedString, Stateful,
    Styled as _, div, prelude::*,
};

use crate::{
    ActiveTheme as _, Icon, IconName, StyledExt as _,
    button::{Button, ButtonVariants as _},
    rems_from_px,
};

pub const COMPACT_BAR_HEIGHT: f32 = 52.0;

const BUTTON: f32 = 44.0;
const BUTTON_ICON: f32 = 20.0;
const PADDING_X: f32 = 4.0;

pub fn compact_bar(
    leading: impl IntoElement,
    center: impl IntoElement,
    trailing: impl IntoElement,
    cx: &App,
) -> Div {
    div()
        .debug_selector(|| "compact-bar".to_owned())
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .h(rems_from_px(COMPACT_BAR_HEIGHT))
        .px(rems_from_px(PADDING_X))
        .gap(rems_from_px(4.0))
        .font_family(cx.theme().font_family.clone())
        .text_color(cx.theme().foreground)
        .child(div().flex().flex_none().child(leading))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .justify_center()
                .child(center),
        )
        .child(div().flex().flex_none().child(trailing))
}

pub fn compact_bar_button(id: impl Into<ElementId>, icon: IconName) -> Button {
    Button::new(id)
        .ghost()
        .compact()
        .tab_stop(false)
        .size(rems_from_px(BUTTON))
        .child(Icon::new(icon).size(rems_from_px(BUTTON_ICON)))
}

pub fn compact_bar_title(
    id: impl Into<ElementId>,
    icon: IconName,
    title: impl Into<SharedString>,
    dots: impl IntoElement,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(|| "compact-bar-title".to_owned())
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(rems_from_px(6.0))
        .min_w_0()
        .max_w_full()
        .h_full()
        .px(rems_from_px(8.0))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .child(
            div()
                .flex()
                .items_center()
                .gap(rems_from_px(6.0))
                .min_w_0()
                .max_w_full()
                .text_size(rems_from_px(14.0))
                .line_height(rems_from_px(18.0))
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(
                    div()
                        .flex_none()
                        .opacity(0.8)
                        .child(Icon::new(icon).size(rems_from_px(14.0))),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title.into()),
                ),
        )
        .child(dots)
}
