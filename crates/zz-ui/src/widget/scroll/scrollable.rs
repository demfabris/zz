//! The `.overflow_y_scrollbar()` / `.vertical_scrollbar(..)` extension trait
//! and the wrapper element it builds.

use std::{panic::Location, rc::Rc};

use gpui::{
    App, Div, Element, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    ScrollHandle, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder,
};

use crate::StyledExt as _;

use super::{Scrollbar, ScrollbarAxis, ScrollbarHandle};

/// Adds scrollbars to an element. The element it is called on *is* the scroll
/// area; it is not re-parented into a new one.
pub trait ScrollableElement: InteractiveElement + Styled + ParentElement + Element {
    /// Overlay a scrollbar driven by an existing handle.
    #[track_caller]
    #[must_use]
    fn scrollbar<H: ScrollbarHandle + Clone>(
        self,
        scroll_handle: &H,
        axis: impl Into<ScrollbarAxis>,
    ) -> Self {
        self.child(ScrollbarLayer {
            id: caller_id(),
            axis: axis.into(),
            scroll_handle: Rc::new(scroll_handle.clone()),
        })
    }

    #[track_caller]
    #[must_use]
    fn vertical_scrollbar<H: ScrollbarHandle + Clone>(self, scroll_handle: &H) -> Self {
        self.scrollbar(scroll_handle, ScrollbarAxis::Vertical)
    }

    #[track_caller]
    #[must_use]
    fn horizontal_scrollbar<H: ScrollbarHandle + Clone>(self, scroll_handle: &H) -> Self {
        self.scrollbar(scroll_handle, ScrollbarAxis::Horizontal)
    }

    /// Like [`StatefulInteractiveElement::overflow_scroll`], plus scrollbars on
    /// both axes. The source element stays the scrollable container.
    #[track_caller]
    #[must_use]
    fn overflow_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Both)
    }

    /// Like [`StatefulInteractiveElement::overflow_x_scroll`], plus a
    /// horizontal scrollbar. The source element stays the scrollable container.
    #[track_caller]
    #[must_use]
    fn overflow_x_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Horizontal)
    }

    /// Like [`StatefulInteractiveElement::overflow_y_scroll`], plus a vertical
    /// scrollbar. The source element stays the scrollable container.
    #[track_caller]
    #[must_use]
    fn overflow_y_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Vertical)
    }
}

impl ScrollableElement for Div {}
impl<E> ScrollableElement for Stateful<E>
where
    E: ParentElement + Styled + Element,
    Self: InteractiveElement,
{
}

/// Renders the wrapped element as a scroll area with scrollbars over it. The
/// scroll handle lives in window state keyed on the call site, so a caller that
/// never drives the offset does not have to hold one.
#[derive(IntoElement)]
pub struct Scrollable<E: InteractiveElement + Styled + ParentElement + Element> {
    id: ElementId,
    element: E,
    axis: ScrollbarAxis,
}

impl<E> Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    #[track_caller]
    fn new(element: E, axis: impl Into<ScrollbarAxis>) -> Self {
        Self {
            id: caller_id(),
            element,
            axis: axis.into(),
        }
    }
}

impl<E> Styled for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E> ParentElement for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.element.extend(elements);
    }
}

impl<E> InteractiveElement for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.element.interactivity()
    }
}

impl<E> RenderOnce for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element + 'static,
{
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll_handle = scroll_handle_for(&self.id, window, cx);

        let root_style = root_style_from(&mut self.element);

        let root_id = self.id.clone();
        let area_id = (self.id.clone(), "area");
        let content_id = (self.id.clone(), "content");
        let scrollbar_id = (self.id.clone(), "scrollbar");

        let content = self
            .element
            .id(content_id)
            .flex_none()
            .map(|this| match self.axis {
                ScrollbarAxis::Vertical => this.h_auto().min_h_full(),
                ScrollbarAxis::Horizontal => this.w_auto().min_w_full(),
                ScrollbarAxis::Both => this.size_auto().min_size_full(),
            });

        let scroll_area = div()
            .id(area_id)
            .size_full()
            .flex()
            .track_scroll(&scroll_handle)
            .map(|this| match self.axis {
                ScrollbarAxis::Vertical => this.flex_col().overflow_y_scroll(),
                ScrollbarAxis::Horizontal => this.flex_row().overflow_x_scroll(),
                ScrollbarAxis::Both => this.overflow_scroll(),
            })
            .child(content);

        div()
            .id(root_id)
            .size_full()
            .refine_style(&root_style)
            .relative()
            .child(scroll_area)
            .child(render_scrollbar(
                scrollbar_id,
                &scroll_handle,
                self.axis,
                window,
                cx,
            ))
    }
}

#[derive(IntoElement)]
struct ScrollbarLayer<H: ScrollbarHandle + Clone> {
    id: ElementId,
    axis: ScrollbarAxis,
    scroll_handle: Rc<H>,
}

impl<H> RenderOnce for ScrollbarLayer<H>
where
    H: ScrollbarHandle + Clone + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        render_scrollbar(self.id, self.scroll_handle.as_ref(), self.axis, window, cx)
    }
}

#[inline]
#[track_caller]
fn caller_id() -> ElementId {
    ElementId::CodeLocation(*Location::caller())
}

#[inline]
fn scroll_handle_for(id: &ElementId, window: &mut Window, cx: &mut App) -> ScrollHandle {
    window
        .use_keyed_state(id.clone(), cx, |_, _| ScrollHandle::default())
        .read(cx)
        .clone()
}

#[inline]
fn root_style_from<E>(element: &mut E) -> StyleRefinement
where
    E: Styled,
{
    let style = element.style();
    StyleRefinement {
        size: style.size.clone(),
        min_size: style.min_size.clone(),
        max_size: style.max_size.clone(),
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        flex_basis: style.flex_basis,
        align_self: style.align_self,
        ..Default::default()
    }
}

#[inline]
fn render_scrollbar<H: ScrollbarHandle + Clone>(
    id: impl Into<ElementId>,
    scroll_handle: &H,
    axis: ScrollbarAxis,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    if window.is_inspector_picking(cx) {
        return div();
    }

    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .child(Scrollbar::new(scroll_handle).id(id).axis(axis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sizable as _;
    use gpui::{
        AppContext as _, Context, Render, ScrollDelta, ScrollWheelEvent, TestAppContext,
        VisualTestContext, point, px,
    };

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    fn scroll(cx: &mut VisualTestContext, x: f32, y: f32, dx: f32, dy: f32) {
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(x), px(y)),
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            ..Default::default()
        });
        draw(cx);
    }

    fn row(selector: &'static str, height: f32) -> Div {
        div()
            .h(px(height))
            .flex_shrink_0()
            .debug_selector(move || selector.to_string())
    }

    fn plain_row(height: f32) -> Div {
        div().h(px(height)).flex_shrink_0()
    }

    fn item(selector: &'static str, width: f32) -> Div {
        div()
            .w(px(width))
            .h(px(20.))
            .flex_shrink_0()
            .debug_selector(move || selector.to_string())
    }

    fn plain_item(width: f32) -> Div {
        div().w(px(width)).h(px(20.)).flex_shrink_0()
    }

    struct SizeFullChildTest;

    impl Render for SizeFullChildTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(100.))
                .h(px(100.))
                .overflow_y_scrollbar()
                .child(
                    div()
                        .size_full()
                        .child(crate::v_flex().children((0..4).map(|ix| {
                            div().h(px(50.)).flex_shrink_0().when(ix == 3, |this| {
                                this.debug_selector(|| "last-row".to_string())
                            })
                        }))),
                )
        }
    }

    struct AutoHeightParentTest;

    impl Render for AutoHeightParentTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                crate::v_flex()
                    .w(px(200.))
                    .child(
                        crate::v_flex().flex_1().overflow_hidden().child(
                            div().flex_1().overflow_hidden().child(
                                crate::v_flex()
                                    .size_full()
                                    .overflow_y_scrollbar()
                                    .child(plain_row(50.))
                                    .child(plain_row(50.)),
                            ),
                        ),
                    )
                    .child(row("auto-height-footer", 10.)),
            )
        }
    }

    struct MaxHeightParentTest;

    impl Render for MaxHeightParentTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::v_flex()
                .w(px(200.))
                .max_h(px(100.))
                .child(
                    crate::v_flex().flex_1().overflow_hidden().child(
                        div().flex_1().overflow_hidden().child(
                            crate::v_flex()
                                .size_full()
                                .overflow_y_scrollbar()
                                .child(plain_row(50.))
                                .child(plain_row(50.))
                                .child(row("max-height-last-row", 50.)),
                        ),
                    ),
                )
                .child(row("max-height-footer", 10.))
        }
    }

    #[gpui::test]
    fn auto_height_parent_gets_content_height(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| AutoHeightParentTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let footer = cx.debug_bounds("auto-height-footer").unwrap();
        assert_eq!(footer.top(), px(100.));
    }

    #[gpui::test]
    fn max_height_parent_clamps_and_scrolls(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| MaxHeightParentTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let footer = cx.debug_bounds("max-height-footer").unwrap();
        assert_eq!(footer.top(), px(90.));

        let last_initial_y = cx.debug_bounds("max-height-last-row").unwrap().origin.y;
        scroll(cx, 10., 10., 0., -50.);
        assert!(cx.debug_bounds("max-height-last-row").unwrap().origin.y < last_initial_y);
    }

    struct GapLayoutTest;

    impl Render for GapLayoutTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::v_flex()
                .w(px(100.))
                .h(px(100.))
                .gap(px(10.))
                .overflow_y_scrollbar()
                .child(row("first-row", 20.))
                .child(row("second-row", 20.))
        }
    }

    struct GapRegressionTest;

    impl Render for GapRegressionTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(100.)).h(px(100.)).child(
                crate::v_flex()
                    .flex_1()
                    .gap(px(30.))
                    .overflow_y_scrollbar()
                    .px(px(12.))
                    .pb(px(16.))
                    .children((0..5).map(|ix| {
                        div()
                            .h(px(20.))
                            .flex_shrink_0()
                            .when(ix == 0, |this| {
                                this.debug_selector(|| "issue-first-card".to_string())
                            })
                            .when(ix == 1, |this| {
                                this.debug_selector(|| "issue-second-card".to_string())
                            })
                            .when(ix == 4, |this| {
                                this.debug_selector(|| "issue-last-card".to_string())
                            })
                    })),
            )
        }
    }

    struct HorizontalGapLayoutTest;

    impl Render for HorizontalGapLayoutTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::h_flex()
                .w(px(100.))
                .h(px(40.))
                .gap(px(10.))
                .overflow_x_scrollbar()
                .child(item("horizontal-first-item", 50.))
                .child(item("horizontal-second-item", 50.))
                .child(item("horizontal-last-item", 50.))
        }
    }

    struct OverflowScrollbarVerticalTest;

    impl Render for OverflowScrollbarVerticalTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::v_flex()
                .w(px(100.))
                .h(px(100.))
                .gap(px(10.))
                .overflow_scrollbar()
                .child(row("both-axis-vertical-first-row", 50.))
                .child(row("both-axis-vertical-second-row", 50.))
                .child(row("both-axis-vertical-last-row", 50.))
        }
    }

    struct OverflowScrollbarHorizontalTest;

    impl Render for OverflowScrollbarHorizontalTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::h_flex()
                .w(px(100.))
                .h(px(40.))
                .gap(px(10.))
                .overflow_scrollbar()
                .child(item("both-axis-horizontal-first-item", 50.))
                .child(item("both-axis-horizontal-second-item", 50.))
                .child(item("both-axis-horizontal-last-item", 50.))
        }
    }

    struct IndependentScrollablesTest;

    impl Render for IndependentScrollablesTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::h_flex()
                .w(px(220.))
                .h(px(100.))
                .gap(px(20.))
                .child(
                    div().w(px(100.)).h(px(100.)).overflow_y_scrollbar().child(
                        crate::v_flex()
                            .child(plain_row(50.))
                            .child(plain_row(50.))
                            .child(row("left-scrollable-last-row", 50.)),
                    ),
                )
                .child(
                    div().w(px(100.)).h(px(100.)).overflow_y_scrollbar().child(
                        crate::v_flex()
                            .child(plain_row(50.))
                            .child(plain_row(50.))
                            .child(row("right-scrollable-last-row", 50.)),
                    ),
                )
        }
    }

    struct NoOverflowTest;

    impl Render for NoOverflowTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::v_flex()
                .w(px(100.))
                .h(px(100.))
                .gap(px(10.))
                .overflow_y_scrollbar()
                .child(row("no-overflow-first-row", 20.))
                .child(row("no-overflow-second-row", 20.))
        }
    }

    struct WrappedTextTest;

    impl Render for WrappedTextTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().w(px(300.)).h(px(200.)).child(
                div().flex().flex_1().min_h_0().overflow_hidden().child(
                    crate::settings::settings_scroll_column("wrapped-text-column")
                        .child(
                            div()
                                .text_size(crate::rems_from_px(10.))
                                .child(gpui::SharedString::from("word ".repeat(60))),
                        )
                        .child(row("wrapped-text-tail", 20.)),
                ),
            )
        }
    }

    struct SettingsPageReplicaTest {
        pickers: Vec<gpui::Entity<crate::color_picker::ColorPickerState>>,
        numbers: Vec<gpui::Entity<crate::input::InputState>>,
    }

    impl Render for SettingsPageReplicaTest {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().w(px(760.)).h(px(300.)).child(
                div().flex().flex_1().min_h_0().overflow_hidden().child(
                    crate::settings::settings_scroll_column("replica-column")
                        .child(
                            div()
                                .text_size(crate::rems_from_px(11.))
                                .child("Changes are written to zz/config."),
                        )
                        .child(
                            crate::settings::SettingsStack::titled("Theme")
                                .description(
                                    "Recolors the application chrome. Every panel, hover state, \
                                     muted label and focus ring is derived from these six, so \
                                     nothing else needs setting.",
                                )
                                .children(self.pickers.iter().enumerate().map(|(ix, picker)| {
                                    crate::settings::SettingEntry::new(
                                        "Background",
                                        "The chrome root everything else is derived from, \
                                         wrapping over a couple of lines like the real rows do.",
                                    )
                                    .title_actions(
                                        div()
                                            .flex()
                                            .flex_none()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(crate::settings::settings_reset_button(
                                                ("replica-reset", ix),
                                                "Reset",
                                                false,
                                            ))
                                            .child(crate::settings::settings_provenance_badge(
                                                "default",
                                            )),
                                    )
                                    .control(
                                        crate::color_picker::ColorPicker::new(
                                            picker,
                                            gpui::black(),
                                        ),
                                    )
                                })),
                        )
                        .child(
                            crate::settings::SettingsStack::titled("Window")
                                .child(
                                    crate::settings::SettingEntry::new(
                                        "Window background opacity",
                                        "Set below 1 to reveal the desktop or blurred backdrop \
                                         through terminal and app chrome.",
                                    )
                                    .control(
                                        div().w(px(120.)).flex_none().child(
                                            crate::input::NumberInput::new(&self.numbers[0])
                                                .small(),
                                        ),
                                    ),
                                )
                                .child(
                                    crate::settings::SettingEntry::new(
                                        "Window background blur",
                                        "Blur content behind translucent window areas when \
                                         supported by the desktop.",
                                    )
                                    .control(
                                        crate::switch::Switch::new("replica-blur").checked(true),
                                    ),
                                ),
                        )
                        .child(
                            crate::settings::SettingsStack::titled("Interface").child(
                                crate::settings::SettingEntry::new(
                                    "Widget corner radius",
                                    "Rounds every widget (buttons, inputs, tags, menus, dialogs) \
                                     in logical pixels (0-256).",
                                )
                                .control(
                                    div().w(px(120.)).flex_none().child(
                                        crate::input::NumberInput::new(&self.numbers[1]).small(),
                                    ),
                                ),
                            ),
                        )
                        .child(row("replica-tail", 20.)),
                ),
            )
        }
    }

    #[gpui::test]
    fn settings_page_scroll_range_matches_content(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|window, cx| SettingsPageReplicaTest {
            pickers: (0..6)
                .map(|_| cx.new(|cx| crate::color_picker::ColorPickerState::new(None, window, cx)))
                .collect(),
            numbers: (0..2)
                .map(|_| cx.new(|cx| crate::input::InputState::new(window, cx)))
                .collect(),
        });
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        scroll(cx, 300., 150., 0., -100_000.);
        let tail = cx.debug_bounds("replica-tail").unwrap();
        assert!(
            tail.bottom() >= px(250.) && tail.bottom() <= px(300.),
            "after scrolling to the end, the last row must sit at the viewport bottom, \
             not pages above it (tail bottom: {:?})",
            tail.bottom()
        );
    }

    #[gpui::test]
    fn scroll_range_measures_wrapped_text_at_layout_width(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| WrappedTextTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let before = cx.debug_bounds("wrapped-text-tail").unwrap().origin.y;
        scroll(cx, 150., 100., 0., -10_000.);
        let after = cx.debug_bounds("wrapped-text-tail").unwrap().origin.y;
        assert_eq!(
            before, after,
            "content fits the viewport, so the wheel must not move it"
        );
    }

    #[gpui::test]
    fn vertical_scrollbar_scrolls_past_a_size_full_child(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| SizeFullChildTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let initial_y = cx.debug_bounds("last-row").unwrap().origin.y;
        scroll(cx, 10., 10., 0., -50.);

        assert!(cx.debug_bounds("last-row").unwrap().origin.y < initial_y);
    }

    #[gpui::test]
    fn vertical_scrollbar_preserves_source_gap(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| GapLayoutTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("first-row").unwrap();
        let second = cx.debug_bounds("second-row").unwrap();
        assert_eq!(second.top() - first.bottom(), px(10.));
    }

    #[gpui::test]
    fn overflow_y_scrollbar_preserves_gap_and_padding_while_scrolling(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| GapRegressionTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("issue-first-card").unwrap();
        let second = cx.debug_bounds("issue-second-card").unwrap();
        let last_initial_y = cx.debug_bounds("issue-last-card").unwrap().origin.y;

        assert_eq!(second.top() - first.bottom(), px(30.));
        assert_eq!(first.left(), px(12.));

        scroll(cx, 10., 10., 0., -50.);

        let first_after_scroll = cx.debug_bounds("issue-first-card").unwrap();
        let second_after_scroll = cx.debug_bounds("issue-second-card").unwrap();
        let last_after_scroll_y = cx.debug_bounds("issue-last-card").unwrap().origin.y;

        assert_eq!(
            second_after_scroll.top() - first_after_scroll.bottom(),
            px(30.)
        );
        assert_eq!(first_after_scroll.left(), px(12.));
        assert!(last_after_scroll_y < last_initial_y);
    }

    #[gpui::test]
    fn horizontal_scrollbar_preserves_source_gap_and_scrolls(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| HorizontalGapLayoutTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("horizontal-first-item").unwrap();
        let second = cx.debug_bounds("horizontal-second-item").unwrap();
        let last_initial_x = cx.debug_bounds("horizontal-last-item").unwrap().origin.x;

        assert_eq!(second.left() - first.right(), px(10.));

        scroll(cx, 10., 10., -50., 0.);

        let first_after_scroll = cx.debug_bounds("horizontal-first-item").unwrap();
        let second_after_scroll = cx.debug_bounds("horizontal-second-item").unwrap();
        let last_after_scroll_x = cx.debug_bounds("horizontal-last-item").unwrap().origin.x;

        assert_eq!(
            second_after_scroll.left() - first_after_scroll.right(),
            px(10.)
        );
        assert!(last_after_scroll_x < last_initial_x);
    }

    #[gpui::test]
    fn overflow_scrollbar_preserves_vertical_source_gap(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| OverflowScrollbarVerticalTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("both-axis-vertical-first-row").unwrap();
        let second = cx.debug_bounds("both-axis-vertical-second-row").unwrap();

        assert_eq!(second.top() - first.bottom(), px(10.));
    }

    #[gpui::test]
    fn overflow_scrollbar_preserves_gap_and_scrolls_horizontally(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| OverflowScrollbarHorizontalTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("both-axis-horizontal-first-item").unwrap();
        let second = cx.debug_bounds("both-axis-horizontal-second-item").unwrap();
        let last_initial_x = cx
            .debug_bounds("both-axis-horizontal-last-item")
            .unwrap()
            .origin
            .x;

        assert_eq!(second.left() - first.right(), px(10.));

        scroll(cx, 10., 10., -50., 0.);

        let first_after_scroll = cx.debug_bounds("both-axis-horizontal-first-item").unwrap();
        let second_after_scroll = cx.debug_bounds("both-axis-horizontal-second-item").unwrap();
        let last_after_scroll_x = cx
            .debug_bounds("both-axis-horizontal-last-item")
            .unwrap()
            .origin
            .x;

        assert_eq!(
            second_after_scroll.left() - first_after_scroll.right(),
            px(10.)
        );
        assert!(last_after_scroll_x < last_initial_x);
    }

    #[gpui::test]
    fn multiple_scrollables_keep_independent_scroll_state(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| IndependentScrollablesTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let left_initial = cx.debug_bounds("left-scrollable-last-row").unwrap();
        let right_initial = cx.debug_bounds("right-scrollable-last-row").unwrap();

        scroll(cx, 10., 10., 0., -50.);

        let left_after_scroll = cx.debug_bounds("left-scrollable-last-row").unwrap();
        let right_after_scroll = cx.debug_bounds("right-scrollable-last-row").unwrap();

        assert!(left_after_scroll.top() < left_initial.top());
        assert_eq!(right_after_scroll.top(), right_initial.top());
    }

    #[gpui::test]
    fn vertical_scrollbar_does_not_scroll_when_content_does_not_overflow(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| NoOverflowTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("no-overflow-first-row").unwrap();
        let second = cx.debug_bounds("no-overflow-second-row").unwrap();

        assert_eq!(second.top() - first.bottom(), px(10.));

        scroll(cx, 10., 10., 0., -50.);

        let first_after_scroll = cx.debug_bounds("no-overflow-first-row").unwrap();
        let second_after_scroll = cx.debug_bounds("no-overflow-second-row").unwrap();

        assert_eq!(first_after_scroll.top(), first.top());
        assert_eq!(second_after_scroll.top(), second.top());
        assert_eq!(
            second_after_scroll.top() - first_after_scroll.bottom(),
            px(10.)
        );
    }

    #[gpui::test]
    fn horizontal_scrollbar_does_not_scroll_when_content_does_not_overflow(
        cx: &mut TestAppContext,
    ) {
        struct HorizontalNoOverflowTest;

        impl Render for HorizontalNoOverflowTest {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                crate::h_flex()
                    .w(px(100.))
                    .h(px(40.))
                    .gap(px(10.))
                    .overflow_x_scrollbar()
                    .child(item("no-overflow-first-item", 20.))
                    .child(item("no-overflow-second-item", 20.))
                    .child(plain_item(20.))
            }
        }

        cx.update(crate::init);
        let (_, cx) = cx.add_window_view(|_, _| HorizontalNoOverflowTest);
        let cx: &mut VisualTestContext = cx;
        draw(cx);

        let first = cx.debug_bounds("no-overflow-first-item").unwrap();
        let second = cx.debug_bounds("no-overflow-second-item").unwrap();

        assert_eq!(second.left() - first.right(), px(10.));

        scroll(cx, 10., 10., -50., 0.);

        let first_after_scroll = cx.debug_bounds("no-overflow-first-item").unwrap();
        let second_after_scroll = cx.debug_bounds("no-overflow-second-item").unwrap();

        assert_eq!(first_after_scroll.left(), first.left());
        assert_eq!(second_after_scroll.left(), second.left());
        assert_eq!(
            second_after_scroll.left() - first_after_scroll.right(),
            px(10.)
        );
    }
}
