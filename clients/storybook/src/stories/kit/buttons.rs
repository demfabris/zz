use zpui::{
    AnyElement, AnyView, App, AppContext as _, Context, IntoElement, MouseButton,
    ParentElement as _, Render, Styled as _, Subscription, Window, div, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariant, ButtonVariants as _},
};

use super::support::{Probe, after_first_frame, hover, press, when_visible};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "buttons",
    name: "Buttons",
    group: "Kit",
    summary: "One button, ten variants. Neutral variants share the washed hover tint; Accent is the one call to action a surface owns.",
    sections: &[
        Section {
            id: "variants",
            name: "Variants",
            summary: "Every ButtonVariant at the default size.",
            build: |_, cx| stateless(variants, cx),
        },
        Section {
            id: "sizes",
            name: "Sizes",
            summary: "Sizable: xsmall, small, medium and large, with and without labels.",
            build: |_, cx| stateless(sizes, cx),
        },
        Section {
            id: "states",
            name: "States",
            summary: "Disabled, selected and loading, plus icons, carets, outlines and tooltips.",
            build: |_, cx| stateless(button_states, cx),
        },
        Section {
            id: "custom-and-shapes",
            name: "Custom and shapes",
            summary: "ButtonCustomVariant colors, flat buttons without a resting surface, compact padding and rounded overrides.",
            build: |_, cx| stateless(custom_and_shapes, cx),
        },
        Section {
            id: "hovered",
            name: "Hovered",
            summary: "The pointer rests on the first button. Neutral variants lift with a washed tint and a highlight ring.",
            build: |window, cx| Pointer::view(Gesture::Hover, window, cx),
        },
        Section {
            id: "pressed",
            name: "Pressed",
            summary: "The primary button is held down and shows its active fill next to a resting copy.",
            build: |window, cx| Pointer::view(Gesture::Press, window, cx),
        },
        Section {
            id: "focused",
            name: "Focused",
            summary: "Keyboard focus draws an accent ring outside the border. Only tab focus reaches it; a click does not.",
            build: |window, cx| Pointer::view(Gesture::Focus, window, cx),
        },
    ],
};

const VARIANTS: [(&str, ButtonVariant); 10] = [
    ("Default", ButtonVariant::Default),
    ("Primary", ButtonVariant::Primary),
    ("Accent", ButtonVariant::Accent),
    ("Secondary", ButtonVariant::Secondary),
    ("Ghost", ButtonVariant::Ghost),
    ("Danger", ButtonVariant::Danger),
    ("Success", ButtonVariant::Success),
    ("Warning", ButtonVariant::Warning),
    ("Link", ButtonVariant::Link),
    ("Text", ButtonVariant::Text),
];

fn variants(_: &mut Window, _: &mut App) -> AnyElement {
    row()
        .children(VARIANTS.iter().map(|(name, variant)| {
            Button::new(format!("variant-{name}"))
                .label(*name)
                .with_variant(*variant)
        }))
        .into_any_element()
}

fn sizes(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .state(
            "label",
            row()
                .child(Button::new("size-xs").label("Extra small").xsmall())
                .child(Button::new("size-sm").label("Small").small())
                .child(Button::new("size-md").label("Medium"))
                .child(Button::new("size-lg").label("Large").large()),
        )
        .state(
            "icon",
            row()
                .child(
                    Button::new("icon-xs")
                        .icon(IconName::Bell)
                        .xsmall()
                        .tooltip("Notifications"),
                )
                .child(
                    Button::new("icon-sm")
                        .icon(IconName::Bell)
                        .small()
                        .tooltip("Notifications"),
                )
                .child(
                    Button::new("icon-md")
                        .icon(IconName::Bell)
                        .tooltip("Notifications"),
                )
                .child(
                    Button::new("icon-lg")
                        .icon(IconName::Bell)
                        .large()
                        .tooltip("Notifications"),
                )
                .child(Button::compact_icon("icon-compact", IconName::Ellipsis).tooltip("More")),
        )
        .into_any_element()
}

fn button_states(_: &mut Window, _: &mut App) -> AnyElement {
    states()
        .columns(2)
        .state(
            "disabled",
            row().children(VARIANTS.iter().take(5).map(|(name, variant)| {
                Button::new(format!("disabled-{name}"))
                    .label(*name)
                    .with_variant(*variant)
                    .disabled(true)
            })),
        )
        .state(
            "selected",
            row()
                .child(
                    Button::new("selected-ghost")
                        .label("Ghost")
                        .ghost()
                        .selected(true),
                )
                .child(
                    Button::new("selected-default")
                        .label("Default")
                        .selected(true),
                ),
        )
        .state(
            "loading",
            row()
                .child(Button::new("loading-default").label("Saving").loading(true))
                .child(
                    Button::new("loading-accent")
                        .label("Sending")
                        .accent()
                        .loading(true),
                ),
        )
        .state(
            "icon and label",
            row()
                .child(Button::new("copy").icon(IconName::Copy).label("Copy"))
                .child(
                    Button::new("open")
                        .icon(IconName::ExternalLink)
                        .label("Open")
                        .ghost(),
                ),
        )
        .state(
            "dropdown caret",
            row()
                .child(Button::new("caret").label("Model").dropdown_caret(true))
                .child(
                    Button::new("caret-ghost")
                        .label("Branch")
                        .ghost()
                        .dropdown_caret(true),
                ),
        )
        .state(
            "outline",
            row()
                .child(Button::new("outline").label("Outline").outline())
                .child(
                    Button::new("outline-danger")
                        .label("Delete")
                        .danger()
                        .outline(),
                ),
        )
        .into_any_element()
}

fn custom_and_shapes(_: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let violet = zz_ui::parse_hex("#bb9af7").unwrap_or(theme.accent);
    let teal = zz_ui::parse_hex("#2ac3de").unwrap_or(theme.accent);
    states()
        .columns(2)
        .state(
            "custom",
            row()
                .child(
                    Button::new("custom-violet").label("Violet").custom(
                        ButtonCustomVariant::new(cx)
                            .color(violet)
                            .active(violet.opacity(0.8)),
                    ),
                )
                .child(
                    Button::new("custom-teal").label("Teal").custom(
                        ButtonCustomVariant::new(cx)
                            .color(teal)
                            .active(teal.opacity(0.8))
                            .shadow(true),
                    ),
                )
                .child(
                    Button::new("custom-selected")
                        .label("Selected")
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(violet)
                                .active(violet.opacity(0.8)),
                        )
                        .selected(true),
                ),
        )
        .state(
            "flat",
            row()
                .child(Button::new("flat-default").label("Default").flat())
                .child(Button::new("flat-ghost").label("Ghost").ghost().flat())
                .child(
                    Button::new("flat-secondary")
                        .label("Secondary")
                        .secondary()
                        .flat(),
                ),
        )
        .state(
            "compact",
            row()
                .child(Button::new("compact-sm").label("Run").small().compact())
                .child(Button::new("compact-md").label("Run").compact())
                .child(Button::new("normal-md").label("Run"))
                .child(Button::compact_icon("compact-icon", IconName::Plus).tooltip("Add")),
        )
        .state(
            "rounded",
            row()
                .child(
                    Button::new("rounded-0")
                        .label("Square")
                        .rounded(ButtonRounded::Size(px(0.0))),
                )
                .child(Button::new("rounded-theme").label("Theme radius"))
                .child(
                    Button::new("rounded-pill")
                        .label("Pill")
                        .rounded(ButtonRounded::Size(px(999.0))),
                )
                .child(
                    Button::new("rounded-icon")
                        .icon(IconName::Plus)
                        .rounded(ButtonRounded::Size(px(999.0))),
                ),
        )
        .state(
            "loading with an icon",
            row()
                .child(
                    Button::new("loading-icon")
                        .icon(IconName::Copy)
                        .label("Copying")
                        .loading(true),
                )
                .child(
                    Button::new("loading-icon-only")
                        .icon(IconName::Copy)
                        .loading(true),
                ),
        )
        .state(
            "children",
            row().child(
                Button::new("children").child(
                    div().flex().items_center().gap_2().child("Branch").child(
                        div()
                            .text_xs()
                            .text_color(theme.foreground.muted())
                            .child("main"),
                    ),
                ),
            ),
        )
        .into_any_element()
}

#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    Hover,
    Press,
    Focus,
}

struct Pointer {
    gesture: Gesture,
    target: Probe,
    _bounds: Subscription,
}

impl Pointer {
    fn view(gesture: Gesture, window: &mut Window, cx: &mut App) -> AnyView {
        let target = Probe::default();
        let probe = target.clone();
        match gesture {
            Gesture::Hover => when_visible(&target, window, move |window, cx| {
                hover(&probe, window, cx);
            }),
            Gesture::Press => when_visible(&target, window, move |window, cx| {
                press(&probe, MouseButton::Left, window, cx);
            }),
            Gesture::Focus => after_first_frame(window, Window::focus_next),
        }
        cx.new(|cx| Self {
            gesture,
            _bounds: cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                if this.gesture != Gesture::Focus {
                    hover(&this.target, window, cx);
                }
            }),
            target,
        })
        .into()
    }

    fn pair(&self, label: &str, variant: ButtonVariant) -> impl IntoElement {
        row()
            .child(
                div()
                    .relative()
                    .child(
                        Button::new("pointer-target")
                            .label(label.to_string())
                            .with_variant(variant),
                    )
                    .child(self.target.measure()),
            )
            .child(
                Button::new("pointer-rest")
                    .label(label.to_string())
                    .with_variant(variant)
                    .tab_stop(false),
            )
    }
}

impl Render for Pointer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (label, variant) = match self.gesture {
            Gesture::Hover => ("Default", ButtonVariant::Default),
            Gesture::Press => ("Primary", ButtonVariant::Primary),
            Gesture::Focus => ("Focused", ButtonVariant::Default),
        };
        let caption = match self.gesture {
            Gesture::Hover => "hovered, resting",
            Gesture::Press => "pressed, resting",
            Gesture::Focus => "focused, resting",
        };
        states().state(caption, self.pair(label, variant))
    }
}
