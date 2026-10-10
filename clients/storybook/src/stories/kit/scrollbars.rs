use zz_gpui::{
    AnyView, App, AppContext as _, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, point, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, ScrollbarShow,
    scroll::{Scrollbar, ScrollbarAxis},
    v_flex,
};

use super::support::{Probe, hover, when_visible};
use crate::story::{Section, Story, states};

pub const STORY: Story = Story {
    id: "scrollbars",
    name: "Scrollbars",
    group: "Kit",
    summary: "Scrollbar overlays a scroll area driven by a ScrollHandle. The theme default on every platform is Scrolling: invisible at rest.",
    sections: &[
        Section {
            id: "show-modes",
            name: "Show modes",
            summary: "Always draws the thumb at rest, Hover draws it while the pointer is over the area (hovered here), Scrolling stays hidden until the area scrolls.",
            build: |window, cx| Modes::view(window, cx),
        },
        Section {
            id: "axes",
            name: "Axes",
            summary: "Vertical, horizontal and both, scrolled part way so the thumbs sit off their start.",
            build: |_, cx| cx.new(|_| Axes::new()).into(),
        },
    ],
};

const ROWS: usize = 40;

fn lines(cx: &App) -> impl IntoElement {
    let muted = cx.theme().foreground.muted();
    v_flex()
        .flex_none()
        .w(px(620.0))
        .p_2()
        .gap_1()
        .text_sm()
        .font_family(cx.theme().mono_font_family.clone())
        .children((1..=ROWS).map(move |line| {
            div()
                .whitespace_nowrap()
                .text_color(if line % 5 == 0 {
                    cx.theme().foreground
                } else {
                    muted
                })
                .child(format!(
                    "{line:>3}  cargo build --workspace --all-features --release --target wasm32"
                ))
        }))
}

fn area(
    id: &'static str,
    handle: &ScrollHandle,
    axis: ScrollbarAxis,
    show: ScrollbarShow,
    cx: &App,
) -> impl IntoElement {
    let scroll = div().id(id).size_full().track_scroll(handle);
    let scroll = match axis {
        ScrollbarAxis::Vertical => scroll.overflow_y_scroll(),
        ScrollbarAxis::Horizontal => scroll.overflow_x_scroll(),
        ScrollbarAxis::Both => scroll.overflow_scroll(),
    };
    div()
        .relative()
        .w_full()
        .h(px(180.0))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .overflow_hidden()
        .child(scroll.child(lines(cx)))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .child(
                    Scrollbar::new(handle)
                        .id(id)
                        .axis(axis)
                        .scrollbar_show(show),
                ),
        )
}

struct Modes {
    always: ScrollHandle,
    hover: ScrollHandle,
    scrolling: ScrollHandle,
    hovered: Probe,
    _bounds: Subscription,
}

impl Modes {
    fn view(window: &mut Window, cx: &mut App) -> AnyView {
        let hovered = Probe::default();
        let probe = hovered.clone();
        when_visible(&hovered, window, move |window, cx| {
            hover(&probe, window, cx);
        });
        cx.new(|cx| Self {
            always: ScrollHandle::new(),
            hover: ScrollHandle::new(),
            scrolling: ScrollHandle::new(),
            hovered,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                hover(&this.hovered, window, cx);
            }),
        })
        .into()
    }
}

impl Render for Modes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(3)
            .state(
                "Always",
                area(
                    "always",
                    &self.always,
                    ScrollbarAxis::Vertical,
                    ScrollbarShow::Always,
                    cx,
                ),
            )
            .state(
                "Hover, hovered",
                div()
                    .relative()
                    .child(area(
                        "hover",
                        &self.hover,
                        ScrollbarAxis::Vertical,
                        ScrollbarShow::Hover,
                        cx,
                    ))
                    .child(self.hovered.measure()),
            )
            .state(
                "Scrolling, at rest",
                area(
                    "scrolling",
                    &self.scrolling,
                    ScrollbarAxis::Vertical,
                    ScrollbarShow::Scrolling,
                    cx,
                ),
            )
    }
}

struct Axes {
    vertical: ScrollHandle,
    horizontal: ScrollHandle,
    both: ScrollHandle,
}

impl Axes {
    fn new() -> Self {
        let vertical = ScrollHandle::new();
        vertical.set_offset(point(px(0.0), px(-240.0)));
        let horizontal = ScrollHandle::new();
        horizontal.set_offset(point(px(-160.0), px(0.0)));
        let both = ScrollHandle::new();
        both.set_offset(point(px(-160.0), px(-240.0)));
        Self {
            vertical,
            horizontal,
            both,
        }
    }
}

impl Render for Axes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        states()
            .columns(3)
            .state(
                "vertical",
                area(
                    "axis-vertical",
                    &self.vertical,
                    ScrollbarAxis::Vertical,
                    ScrollbarShow::Always,
                    cx,
                ),
            )
            .state(
                "horizontal",
                area(
                    "axis-horizontal",
                    &self.horizontal,
                    ScrollbarAxis::Horizontal,
                    ScrollbarShow::Always,
                    cx,
                ),
            )
            .state(
                "both",
                area(
                    "axis-both",
                    &self.both,
                    ScrollbarAxis::Both,
                    ScrollbarShow::Always,
                    cx,
                ),
            )
    }
}
