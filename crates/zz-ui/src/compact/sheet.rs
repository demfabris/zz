use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ElementId, IntoElement, MouseButton,
    ParentElement as _, Pixels, SharedString, Styled, Window, div, ease_out_quint, prelude::*,
    relative,
};
use web_time::Duration;

use crate::{ActiveTheme as _, Colorize as _, StyledExt as _, rems_from_px};

const ENTER: Duration = Duration::from_millis(240);
const CORNER: f32 = 28.0;
const GRABBER_WIDTH: f32 = 36.0;
const GRABBER_HEIGHT: f32 = 5.0;
const HEADER_HEIGHT: f32 = 44.0;
const PADDING_X: f32 = 16.0;
const MAX_HEIGHT: f32 = 0.85;
const RISE: f32 = 0.3;

pub fn bottom_sheet(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    actions: impl IntoIterator<Item = AnyElement>,
    content: impl IntoElement,
    bottom_inset: Pixels,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let corner = rems_from_px(CORNER);
    let scrim = div()
        .id("bottom-sheet-scrim")
        .debug_selector(|| "bottom-sheet-scrim".to_owned())
        .absolute()
        .inset_0()
        .bg(theme.scrim)
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(move |_, window, cx| on_dismiss(window, cx))
        .with_animation(
            "bottom-sheet-scrim-enter",
            Animation::new(ENTER).with_easing(ease_out_quint()),
            Styled::opacity,
        );
    let panel = div()
        .id("bottom-sheet-panel")
        .debug_selector(|| "bottom-sheet-panel".to_owned())
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .max_h(relative(MAX_HEIGHT))
        .flex()
        .flex_col()
        .occlude()
        .rounded_tl(corner)
        .rounded_tr(corner)
        .bg(theme.background.raised(1).opaque())
        .text_color(theme.foreground)
        .font_family(theme.font_family.clone())
        .pb(bottom_inset)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .flex_none()
                .justify_center()
                .pt(rems_from_px(6.0))
                .child(
                    div()
                        .w(rems_from_px(GRABBER_WIDTH))
                        .h(rems_from_px(GRABBER_HEIGHT))
                        .rounded_full()
                        .bg(theme.foreground.opacity(0.25)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(rems_from_px(8.0))
                .h(rems_from_px(HEADER_HEIGHT))
                .px(rems_from_px(PADDING_X))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(rems_from_px(17.0))
                        .line_height(rems_from_px(22.0))
                        .font_semibold()
                        .child(title.into()),
                )
                .children(actions),
        )
        .child(
            div()
                .id("bottom-sheet-content")
                .flex()
                .flex_col()
                .min_h_0()
                .overflow_y_scroll()
                .child(div().flex().flex_col().flex_none().w_full().child(content)),
        )
        .with_animation(
            "bottom-sheet-panel-enter",
            Animation::new(ENTER).with_easing(ease_out_quint()),
            |panel, delta| panel.bottom(relative(-RISE * (1.0 - delta))),
        );
    div()
        .id(id.into())
        .absolute()
        .inset_0()
        .occlude()
        .child(scrim)
        .child(panel)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{Context, Modifiers, Render, TestAppContext, point, px};

    use super::*;

    struct Host {
        dismissed: Rc<Cell<usize>>,
        body: Pixels,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let dismissed = Rc::clone(&self.dismissed);
            div().size_full().child(bottom_sheet(
                "sheet",
                "Panes",
                Vec::new(),
                div()
                    .debug_selector(|| "sheet-body".to_owned())
                    .h(self.body)
                    .w_full(),
                px(20.0),
                move |_, _| dismissed.set(dismissed.get() + 1),
                cx,
            ))
        }
    }

    #[gpui::test]
    fn the_scrim_dismisses_and_the_panel_does_not(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let dismissed = Rc::new(Cell::new(0));
        let count = Rc::clone(&dismissed);
        let (_, cx) = cx.add_window_view(move |_, _| Host {
            dismissed: count,
            body: px(120.0),
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let panel = cx.debug_bounds("bottom-sheet-panel").expect("panel");
        let scrim = cx.debug_bounds("bottom-sheet-scrim").expect("scrim");
        assert!(panel.top() > scrim.top());
        assert_eq!(panel.bottom(), scrim.bottom());
        let body = cx.debug_bounds("sheet-body").expect("body");
        assert_eq!(body.size.height, px(120.0));
        assert!(panel.bottom() - body.bottom() >= px(20.0));

        cx.simulate_click(panel.center(), Modifiers::none());
        assert_eq!(dismissed.get(), 0);

        cx.simulate_click(
            point(scrim.center().x, scrim.top() + px(10.0)),
            Modifiers::none(),
        );
        assert_eq!(dismissed.get(), 1);
    }

    #[gpui::test]
    fn a_tall_sheet_stops_at_most_of_the_window(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(move |_, _| Host {
            dismissed: Rc::default(),
            body: px(5000.0),
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let panel = cx.debug_bounds("bottom-sheet-panel").expect("panel");
        let scrim = cx.debug_bounds("bottom-sheet-scrim").expect("scrim");
        assert!(panel.size.height <= scrim.size.height * MAX_HEIGHT + px(0.5));
        assert!(panel.size.height >= scrim.size.height * MAX_HEIGHT - px(0.5));
        let body = cx.debug_bounds("sheet-body").expect("body");
        assert_eq!(body.size.height, px(5000.0));
    }
}
