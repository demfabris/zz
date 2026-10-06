use gpui::{App, Div, ParentElement as _, Styled as _, div, prelude::*};

use crate::{ActiveTheme as _, rems_from_px};

const DOT: f32 = 5.0;
const ACTIVE_DOT: f32 = 12.0;
const DOT_GAP: f32 = 3.0;
const GROUP_GAP: f32 = 7.0;
const DIM: f32 = 0.3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageDot {
    pub active: bool,
    pub attention: bool,
}

pub fn page_dots(groups: &[Vec<PageDot>], cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(GROUP_GAP))
        .children(groups.iter().enumerate().map(|(group, dots)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(rems_from_px(DOT_GAP))
                .children(dots.iter().enumerate().map(|(index, dot)| {
                    let selector = format!("page-dot-{group}-{index}");
                    let color = if dot.active {
                        theme.foreground
                    } else if dot.attention {
                        theme.warning
                    } else {
                        theme.foreground.opacity(DIM)
                    };
                    div()
                        .debug_selector(move || selector)
                        .flex_none()
                        .h(rems_from_px(DOT))
                        .w(rems_from_px(if dot.active { ACTIVE_DOT } else { DOT }))
                        .rounded_full()
                        .bg(color)
                }))
        }))
}

#[cfg(test)]
mod tests {
    use gpui::{
        Bounds, Context, IntoElement, Pixels, Render, TestAppContext, VisualTestContext, Window,
    };

    use super::*;

    struct Host {
        groups: Vec<Vec<PageDot>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(page_dots(&self.groups, cx))
        }
    }

    fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector).expect(selector)
    }

    #[gpui::test]
    fn one_element_per_dot_and_the_active_one_is_wider(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let groups = vec![
            vec![
                PageDot::default(),
                PageDot {
                    active: true,
                    attention: false,
                },
            ],
            vec![
                PageDot {
                    active: false,
                    attention: true,
                },
                PageDot::default(),
                PageDot::default(),
            ],
        ];
        let (_, cx) = cx.add_window_view(|_, _| Host { groups });
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
        let rest = bounds(cx, "page-dot-0-0");
        let active = bounds(cx, "page-dot-0-1");
        assert!(active.size.width > rest.size.width);
        assert_eq!(active.size.height, rest.size.height);
        for selector in ["page-dot-1-0", "page-dot-1-1", "page-dot-1-2"] {
            assert_eq!(bounds(cx, selector).size, rest.size);
        }
        assert!(cx.debug_bounds("page-dot-0-2").is_none());
        assert!(cx.debug_bounds("page-dot-1-3").is_none());
        let inner = bounds(cx, "page-dot-1-1").left() - bounds(cx, "page-dot-1-0").right();
        let between = bounds(cx, "page-dot-1-0").left() - active.right();
        assert!(between > inner);
    }
}
