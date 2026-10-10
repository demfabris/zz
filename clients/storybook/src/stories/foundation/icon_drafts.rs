use zz_gpui::{
    AnyElement, App, IntoElement, ParentElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use crate::glyphs;
use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "icon-drafts",
    name: "Icon drafts",
    group: "Foundation",
    summary: "Every zz icon drawn from the theme, in the Mac school (clients/storybook/icons/sets/mac.json), with Tabler for reference. Corners take half the theme radius in 24-grid units and the theme's corner smoothing, capped at 0.45 of the shorter side like every zz corner. Radius 0 is flat: sharp corners, square caps, miter joins.",
    sections: &[
        Section {
            id: "follows-the-theme",
            name: "Follows the theme",
            summary: "The set at 20px for the current Radius and Corner smoothing knobs, with Tabler under it.",
            build: |_, cx| stateless(follows_the_theme, cx),
        },
        Section {
            id: "radius-ramp",
            name: "Radius ramp",
            summary: "The same glyphs at theme radius 0, 2, 4, 6, 8, 12, 16 and 25, at the current smoothing.",
            build: |_, cx| stateless(radius_ramp, cx),
        },
        Section {
            id: "sizes",
            name: "Sizes",
            summary: "12, 14, 16 and 20px at the current radius.",
            build: |_, cx| stateless(sizes, cx),
        },
        Section {
            id: "in-chrome",
            name: "In chrome",
            summary: "A sidebar whose rows and icons share the theme radius, next to the same sidebar with Tabler.",
            build: |_, cx| stateless(in_chrome, cx),
        },
        Section {
            id: "construction",
            name: "Construction",
            summary: "48px at the current radius, to read the geometry.",
            build: |_, cx| stateless(construction, cx),
        },
    ],
};

#[derive(Clone, Copy)]
enum Set {
    Drawn(&'static str, f32, f32),
    Tabler,
}

impl Set {
    fn current(name: &'static str, cx: &App) -> Self {
        Self::at(name, f32::from(cx.theme().radius), cx)
    }

    fn at(name: &'static str, radius: f32, cx: &App) -> Self {
        Self::Drawn(name, radius, cx.theme().corner_smoothing)
    }

    fn icon(self, name: &str) -> Icon {
        let path = match self {
            Self::Drawn(set, radius, smoothing) => glyphs::path(set, name, radius * 0.5, smoothing),
            Self::Tabler => format!("tabler/{name}.svg"),
        };
        Icon::empty().path(path)
    }
}

fn strip(set: Set, size: f32) -> impl IntoElement {
    h_flex()
        .flex_wrap()
        .gap(px((size * 0.8).max(12.0)))
        .children(glyphs::names().map(move |name| set.icon(name).with_size(Size::Size(px(size)))))
}

fn follows_the_theme(_: &mut Window, cx: &mut App) -> AnyElement {
    let radius = f32::from(cx.theme().radius);
    states()
        .state(
            format!("mac, radius {radius}"),
            strip(Set::current("mac", cx), 20.0),
        )
        .state("tabler", strip(Set::Tabler, 20.0))
        .into_any_element()
}

fn radius_ramp(_: &mut Window, cx: &mut App) -> AnyElement {
    [0.0, 2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 25.0]
        .into_iter()
        .fold(states(), |states, radius| {
            states.state(
                format!("radius {radius}"),
                strip(Set::at("mac", radius, cx), 24.0),
            )
        })
        .into_any_element()
}

fn sizes(_: &mut Window, cx: &mut App) -> AnyElement {
    let set = Set::current("mac", cx);
    [12.0, 14.0, 16.0, 20.0]
        .into_iter()
        .fold(states(), |states, size| {
            states.state(format!("{size}px"), strip(set, size))
        })
        .into_any_element()
}

fn construction(_: &mut Window, cx: &mut App) -> AnyElement {
    strip(Set::current("mac", cx), 48.0).into_any_element()
}

fn in_chrome(_: &mut Window, cx: &mut App) -> AnyElement {
    let muted = cx.theme().foreground.muted();
    let mono = cx.theme().mono_font_family.clone();
    h_flex()
        .flex_wrap()
        .items_start()
        .gap_6()
        .children(
            [("mac", Set::current("mac", cx)), ("tabler", Set::Tabler)].map(|(label, set)| {
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .line_height(px(14.0))
                            .font_family(mono.clone())
                            .text_color(muted)
                            .child(label),
                    )
                    .child(sidebar(label, set, cx))
            }),
        )
        .into_any_element()
}

fn sidebar(id: &'static str, set: Set, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let button = |name: &'static str| {
        Button::new(format!("{id}-{name}"))
            .icon(set.icon(name))
            .ghost()
            .small()
    };
    let item = |icon: &'static str, label: &'static str, selected: bool| {
        h_flex()
            .h(px(28.0))
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .text_size(px(13.0))
            .text_color(theme.foreground.muted())
            .when(selected, |item| {
                item.bg(theme.foreground.wash())
                    .text_color(theme.foreground)
            })
            .child(set.icon(icon).with_size(Size::Size(px(16.0))))
            .child(div().child(label))
    };
    v_flex()
        .w(px(232.0))
        .p_1p5()
        .gap_0p5()
        .rounded(theme.radius * 1.5)
        .border_1()
        .border_color(theme.border())
        .bg(theme.background.raised(1))
        .child(
            h_flex()
                .justify_between()
                .pb_1()
                .child(button("panel-left"))
                .child(
                    h_flex()
                        .gap_0p5()
                        .child(button("search"))
                        .child(button("plus")),
                ),
        )
        .child(item("terminal", "zsh", true))
        .child(item("globe", "zzmux.sh", false))
        .child(item("bot", "claude", false))
        .child(item("folder", "~/dev/zz", false))
        .child(item("git-branch", "icon-drafts", false))
        .child(
            h_flex()
                .justify_between()
                .pt_1()
                .child(
                    h_flex()
                        .gap_0p5()
                        .child(button("settings"))
                        .child(button("bell")),
                )
                .child(
                    h_flex()
                        .gap_0p5()
                        .child(button("chevron-right"))
                        .child(button("xmark")),
                ),
        )
}
