use std::{rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, ElementId, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, prelude::*,
};

use crate::{
    ActiveTheme as _, Colorize as _, StyledExt as _,
    kbd::Kbd,
    rems_from_px,
    which_key::{WhichKeyCap, WhichKeyHeader, WhichKeyRow, cap_text, groups},
};

const INSET: f32 = 10.0;
const ROW_HEIGHT: f32 = 44.0;
const FOOTER_HEIGHT: f32 = 40.0;
const CAP_MIN_WIDTH: f32 = 28.0;
const HINT: &str = "Tap a command, or type its key";

type PickHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;
type MoreHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct WhichKeyList {
    header: WhichKeyHeader,
    rows: Arc<[WhichKeyRow]>,
    on_pick: Option<PickHandler>,
    on_more: Option<MoreHandler>,
}

impl WhichKeyList {
    pub fn new(header: WhichKeyHeader, rows: Arc<[WhichKeyRow]>) -> Self {
        Self {
            header,
            rows,
            on_pick: None,
            on_more: None,
        }
    }

    #[must_use]
    pub fn on_pick(mut self, f: impl Fn(&SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(f));
        self
    }

    #[must_use]
    pub fn on_more(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_more = Some(Rc::new(f));
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

    fn tappable(id: ElementId, height: f32, cx: &App) -> gpui::Stateful<gpui::Div> {
        let pressed = cx.theme().background.washed(2);
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(rems_from_px(8.0))
            .min_w_0()
            .h(rems_from_px(height))
            .px(rems_from_px(8.0))
            .rounded(cx.theme().menu_radius())
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .active(move |style| style.bg(pressed))
    }

    fn row(&self, index: usize, row: &WhichKeyRow, cx: &App) -> AnyElement {
        let selector = format!("which-key-list-row-{}", row.id);
        let id = row.id.clone();
        let on_pick = self.on_pick.clone();
        Self::tappable(
            ElementId::named_usize("which-key-list-row", index),
            ROW_HEIGHT,
            cx,
        )
        .debug_selector(move || selector)
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
                .text_size(rems_from_px(13.0))
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
        let header = &self.header;
        let ordered: Vec<&WhichKeyRow> = groups(&self.rows)
            .into_iter()
            .flat_map(|group| group.rows)
            .collect();
        let rows: Vec<AnyElement> = ordered
            .iter()
            .enumerate()
            .map(|(index, row)| self.row(index, row, cx))
            .collect();
        let prefix = match header.prefix.clone() {
            Some(key) => Kbd::new(key).lowercase().into_any_element(),
            None => Self::cap(
                &WhichKeyCap {
                    keys: Vec::new(),
                    raw: header.prefix_raw.clone(),
                    yours: false,
                },
                cx,
            )
            .into_any_element(),
        };
        let footer = header.more.as_ref().map(|more| {
            let on_more = self.on_more.clone();
            Self::tappable("which-key-list-more".into(), FOOTER_HEIGHT, cx)
                .debug_selector(|| "which-key-list-more".to_owned())
                .flex_none()
                .text_size(rems_from_px(13.0))
                .text_color(cx.theme().foreground.muted())
                .child(Self::cap(more, cx))
                .child("All bindings")
                .on_click(move |_, window, cx| {
                    if let Some(on_more) = &on_more {
                        on_more(window, cx);
                    }
                })
        });
        div()
            .flex()
            .flex_col()
            .w_full()
            .max_h_full()
            .min_h_0()
            .px(rems_from_px(INSET))
            .child(
                div()
                    .id("which-key-list")
                    .debug_selector(|| "which-key-list".to_owned())
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_h_full()
                    .min_h_0()
                    .occlude()
                    .p(rems_from_px(6.0))
                    .gap(rems_from_px(2.0))
                    .popover_style(cx)
                    .font_family(cx.theme().font_family.clone())
                    .line_height(rems_from_px(16.0))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(rems_from_px(8.0))
                            .h(rems_from_px(32.0))
                            .px(rems_from_px(8.0))
                            .text_size(rems_from_px(12.0))
                            .child(prefix)
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(cx.theme().foreground.muted())
                                    .child(HINT),
                            ),
                    )
                    .child(
                        div()
                            .id("which-key-list-body")
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(2)
                                    .gap_x(rems_from_px(4.0))
                                    .children(rows),
                            ),
                    )
                    .children(footer),
            )
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
            let more = Rc::clone(&self.picked);
            div().w_full().h(gpui::px(300.0)).flex().flex_col().child(
                WhichKeyList::new(
                    WhichKeyHeader {
                        table: "prefix".into(),
                        prefix: Keystroke::parse("ctrl-b").ok(),
                        prefix_raw: "C-b".into(),
                        more: Some(WhichKeyCap {
                            keys: Keystroke::parse("?").into_iter().collect(),
                            raw: "?".into(),
                            yours: false,
                        }),
                    },
                    Arc::clone(&self.rows),
                )
                .on_pick(move |id, _, _| picked.borrow_mut().push(id.to_string()))
                .on_more(move |_, _| more.borrow_mut().push("more".into())),
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
    fn tapping_a_row_picks_its_id(cx: &mut TestAppContext) {
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
        assert_eq!(first.top(), second.top());
        assert!(second.left() > first.left());
        assert!(third.top() > first.top());

        cx.simulate_click(second.center(), Modifiers::none());
        redraw(cx);
        cx.simulate_click(third.center(), Modifiers::none());
        redraw(cx);
        let more = cx.debug_bounds("which-key-list-more").expect("more");
        cx.simulate_click(more.center(), Modifiers::none());
        assert_eq!(picked.borrow().as_slice(), ["n", "%", "more"]);
    }

    #[gpui::test]
    fn a_long_table_scrolls_inside_the_card(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let rows: Arc<[WhichKeyRow]> = (0..40)
            .map(|index| row(&format!("f{}", index + 1), "Thing", "Panes"))
            .collect();
        let (_, cx) = cx.add_window_view(move |_, _| Host {
            rows,
            picked: Rc::default(),
        });
        redraw(cx);
        let card = cx.debug_bounds("which-key-list").expect("card");
        assert!(card.size.height <= gpui::px(300.0));
        let more = cx.debug_bounds("which-key-list-more").expect("more");
        assert!(more.bottom() <= card.bottom());
        let first = cx.debug_bounds("which-key-list-row-f1").expect("f1");
        assert!(first.top() >= card.top());
    }
}
