use std::{rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, ElementId, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, prelude::*,
};

use crate::{
    ActiveTheme as _, Colorize as _, StyledExt as _, rems_from_px,
    which_key::{WhichKeyCap, WhichKeyRow, cap_text, groups},
};

const INSET: f32 = 8.0;
const ROW_HEIGHT: f32 = 44.0;
const HEADER_HEIGHT: f32 = 28.0;
const CAP_MIN_WIDTH: f32 = 44.0;

type PickHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct WhichKeyList {
    rows: Arc<[WhichKeyRow]>,
    on_pick: Option<PickHandler>,
}

impl WhichKeyList {
    pub fn new(rows: Arc<[WhichKeyRow]>) -> Self {
        Self {
            rows,
            on_pick: None,
        }
    }

    #[must_use]
    pub fn on_pick(mut self, f: impl Fn(&SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(f));
        self
    }

    fn cap(cap: &WhichKeyCap, cx: &App) -> impl IntoElement {
        div()
            .flex_none()
            .whitespace_nowrap()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(rems_from_px(12.0))
            .text_color(if cap.yours {
                cx.theme().accent
            } else {
                cx.theme().foreground.muted()
            })
            .child(cap_text(cap))
    }

    fn row(&self, index: usize, row: &WhichKeyRow, cx: &App) -> AnyElement {
        let selector = format!("which-key-list-row-{}", row.id);
        let id = row.id.clone();
        let on_pick = self.on_pick.clone();
        let pressed = cx.theme().background.washed(2);
        div()
            .id(ElementId::named_usize("which-key-list-row", index))
            .debug_selector(move || selector)
            .flex()
            .flex_none()
            .items_center()
            .gap(rems_from_px(12.0))
            .min_w_0()
            .h(rems_from_px(ROW_HEIGHT))
            .px(rems_from_px(INSET))
            .rounded(cx.theme().menu_radius())
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .active(move |style| style.bg(pressed))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .min_w(rems_from_px(CAP_MIN_WIDTH))
                    .gap(rems_from_px(6.0))
                    .children(row.caps.iter().map(|cap| Self::cap(cap, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(row.label.clone()),
            )
            .on_click(move |_, window, cx| {
                if let Some(on_pick) = &on_pick {
                    on_pick(&id, window, cx);
                }
            })
            .into_any_element()
    }
}

impl RenderOnce for WhichKeyList {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut children = Vec::new();
        let mut index = 0;
        for group in groups(&self.rows) {
            if let Some(title) = group.title.clone() {
                children.push(
                    div()
                        .flex()
                        .flex_none()
                        .items_end()
                        .h(rems_from_px(HEADER_HEIGHT))
                        .px(rems_from_px(INSET))
                        .pb(rems_from_px(4.0))
                        .text_size(rems_from_px(12.0))
                        .text_color(cx.theme().foreground.muted())
                        .font_medium()
                        .child(title)
                        .into_any_element(),
                );
            }
            for row in group.rows {
                children.push(self.row(index, row, cx));
                index += 1;
            }
        }
        div()
            .id("which-key-list")
            .debug_selector(|| "which-key-list".to_owned())
            .flex()
            .flex_col()
            .w_full()
            .px(rems_from_px(INSET))
            .pb(rems_from_px(INSET))
            .font_family(cx.theme().font_family.clone())
            .text_size(rems_from_px(14.0))
            .line_height(rems_from_px(18.0))
            .children(children)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{Context, Keystroke, Modifiers, Render, TestAppContext, VisualTestContext};

    use super::*;

    fn row(key: &str, label: &str, group: &str) -> WhichKeyRow {
        WhichKeyRow {
            id: key.to_owned().into(),
            caps: vec![WhichKeyCap {
                keys: Keystroke::parse(key).into_iter().collect(),
                raw: key.to_owned().into(),
                yours: false,
            }],
            label: label.to_owned().into(),
            group: Some(group.to_owned().into()),
            repeat: false,
        }
    }

    struct Host {
        rows: Arc<[WhichKeyRow]>,
        picked: Rc<RefCell<Vec<String>>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let picked = Rc::clone(&self.picked);
            div().w_full().child(
                WhichKeyList::new(Arc::clone(&self.rows))
                    .on_pick(move |id, _, _| picked.borrow_mut().push(id.to_string())),
            )
        }
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn rows_stack_by_group_and_a_tap_picks_the_key(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let picked = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&picked);
        let rows: Arc<[WhichKeyRow]> = Arc::from(vec![
            row("c", "New window", "Windows"),
            row("%", "Split right", "Panes"),
            row("n", "Next window", "Windows"),
        ]);
        let (_, cx) = cx.add_window_view(move |_, _| Host { rows, picked: log });
        redraw(cx);

        let first = cx.debug_bounds("which-key-list-row-c").expect("c");
        let second = cx.debug_bounds("which-key-list-row-n").expect("n");
        let third = cx.debug_bounds("which-key-list-row-%").expect("%");
        assert_eq!(first.left(), second.left());
        assert!(second.top() > first.top());
        assert!(third.top() > second.top());

        cx.simulate_click(second.center(), Modifiers::none());
        redraw(cx);
        cx.simulate_click(third.center(), Modifiers::none());
        assert_eq!(picked.borrow().as_slice(), ["n", "%"]);
    }
}
