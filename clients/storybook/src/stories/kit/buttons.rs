use zpui::{AnyElement, App, IntoElement, ParentElement as _, Window};
use zz_ui::{
    Disableable as _, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
};

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
