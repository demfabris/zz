//! The atomic widget-layer primitives, each shown on its own.

use gpui::{AnyElement, Context, Keystroke, ParentElement as _, Styled as _, div, prelude::*, px};
use zz_ui::command::palette_shortcut_hint;
use zz_ui::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputContentType, NumberInput},
    kbd::Kbd,
    select::Select,
    separator::Separator,
    spinner::Spinner,
    switch::Switch,
    tag::Tag,
};

use super::{Showcase, gallery, specimen, specimens, story_stack};
use zz_ui::Colorize as _;

pub(super) fn buttons(cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Variants",
                "The full ButtonVariants set. Each carries its own hover, active, and disabled treatment.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen("default", Button::new("v-default").label("Button"), cx))
                    .child(specimen(
                        "primary",
                        Button::new("v-primary").primary().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "secondary",
                        Button::new("v-secondary").secondary().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "outline",
                        Button::new("v-outline").outline().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "ghost",
                        Button::new("v-ghost").ghost().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "danger",
                        Button::new("v-danger").danger().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "warning",
                        Button::new("v-warning").warning().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "success",
                        Button::new("v-success").success().label("Button"),
                        cx,
                    ))
                                        .child(specimen("link", Button::new("v-link").link().label("Button"), cx))
                    .child(specimen("text", Button::new("v-text").text().label("Button"), cx)),
            ),
        )
        .child(
            gallery(
                "Icon-only",
                "The compact icon buttons that fill toolbars, tree-row actions, and titlebars.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "ghost · plus",
                        Button::new("i-plus").ghost().icon(IconName::Plus),
                        cx,
                    ))
                    .child(specimen(
                        "ghost · settings",
                        Button::new("i-settings").ghost().icon(IconName::Settings),
                        cx,
                    ))
                    .child(specimen(
                        "ghost · close",
                        Button::new("i-close").ghost().icon(IconName::Xmark),
                        cx,
                    ))
                    .child(specimen(
                        "ghost · more",
                        Button::new("i-more").ghost().icon(IconName::EllipsisVertical),
                        cx,
                    ))
                    .child(specimen(
                        "outline · reload",
                        Button::new("i-reload").outline().icon(IconName::Redo2),
                        cx,
                    ))
                    .child(specimen(
                        "ghost · delete",
                        Button::new("i-delete").ghost().icon(IconName::Xmark),
                        cx,
                    ))
                    .child(specimen(
                        "xsmall",
                        Button::new("i-xs").ghost().xsmall().icon(IconName::Plus),
                        cx,
                    ))
                    .child(specimen(
                        "small",
                        Button::new("i-sm").ghost().small().icon(IconName::Plus),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Sizes & composition",
                "The three sizes, compact padding, an icon paired with a label, and the dropdown caret.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "xsmall",
                        Button::new("s-xs").outline().xsmall().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "small",
                        Button::new("s-sm").outline().small().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "medium",
                        Button::new("s-md").outline().label("Button"),
                        cx,
                    ))
                    .child(specimen(
                        "compact",
                        Button::new("s-compact").outline().small().compact().label("OK"),
                        cx,
                    ))
                    .child(specimen(
                        "icon + label",
                        Button::new("s-il")
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label("New window"),
                        cx,
                    ))
                    .child(specimen(
                        "dropdown caret",
                        Button::new("s-dc")
                            .outline()
                            .small()
                            .label("Session")
                            .dropdown_caret(true),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "States",
                "Selected, disabled, and loading, plus a hover tooltip (hover the last cell).",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "default",
                        Button::new("st-default").primary().label("Save"),
                        cx,
                    ))
                    .child(specimen(
                        "selected",
                        Button::new("st-selected").ghost().selected(true).label("Active"),
                        cx,
                    ))
                    .child(specimen(
                        "disabled",
                        Button::new("st-disabled").primary().disabled(true).label("Save"),
                        cx,
                    ))
                    .child(specimen(
                        "loading",
                        Button::new("st-loading").primary().loading(true).label("Saving"),
                        cx,
                    ))
                    .child(specimen(
                        "tooltip (hover)",
                        Button::new("st-tooltip")
                            .outline()
                            .icon(IconName::Settings)
                            .tooltip("Settings"),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

pub(super) fn tags_badges(cx: &mut Context<Showcase>) -> AnyElement {
    let mono = "Menlo";
    story_stack()
        .child(
            gallery("Tag variants", "The four Tag variants, filled and outline.", cx).child(
                specimens()
                    .child(specimen("primary", Tag::primary().child("primary"), cx))
                    .child(specimen("secondary", Tag::secondary().child("secondary"), cx))
                    .child(specimen("success", Tag::success().child("success"), cx))
                    .child(specimen("info", Tag::primary().child("info"), cx))
                    .child(specimen(
                        "primary · outline",
                        Tag::primary().outline().child("primary"),
                        cx,
                    ))
                    .child(specimen(
                        "secondary · outline",
                        Tag::secondary().outline().child("secondary"),
                        cx,
                    ))
                    .child(specimen(
                        "success · outline",
                        Tag::success().outline().child("success"),
                        cx,
                    ))
                    .child(specimen(
                        "small",
                        Tag::secondary().small().outline().child("small"),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Status badges",
                "The typed badges the app builds from tags and text: command kinds, settings provenance, and the diagnostic frame-rate readout.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "kind · COMMAND",
                        zz_ui::command::command_kind_badge("COMMAND", mono),
                        cx,
                    ))
                    .child(specimen(
                        "kind · OPTION",
                        zz_ui::command::command_kind_badge("OPTION", mono),
                        cx,
                    ))
                    .child(specimen(
                        "kind · TARGET",
                        zz_ui::command::command_kind_badge("TARGET", mono),
                        cx,
                    ))
                    .child(specimen(
                        "provenance · default",
                        zz_ui::settings::settings_provenance_badge("Default"),
                        cx,
                    ))
                    .child(specimen(
                        "provenance · tmux",
                        zz_ui::settings::settings_provenance_badge("From tmux"),
                        cx,
                    ))
                    .child(specimen(
                        "provenance · overridden",
                        zz_ui::settings::settings_provenance_badge("Overridden"),
                        cx,
                    ))
                    .child(specimen(
                        "frame rate · live",
                        zz_ui::pane::frame_rate_badge("BF", Some(60.0), cx),
                        cx,
                    ))
                    .child(specimen(
                        "frame rate · stalled",
                        zz_ui::pane::frame_rate_badge("BF", None, cx),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

pub(super) fn inputs_selects(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Text inputs",
                "The InputState-backed field in the appearances the app uses.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "default",
                        Input::new(&showcase.value_input).small().w(px(200.0)),
                        cx,
                    ))
                    .child(specimen(
                        "with prefix",
                        Input::new(&showcase.command_input)
                            .small()
                            .w(px(200.0))
                            .prefix(Icon::new(IconName::Search).xsmall()),
                        cx,
                    ))
                    .child(specimen(
                        "cleanable",
                        Input::new(&showcase.mux_prefix)
                            .small()
                            .w(px(200.0))
                            .cleanable(true),
                        cx,
                    ))
                    .child(specimen(
                        "borderless",
                        Input::new(&showcase.mux_history)
                            .small()
                            .w(px(200.0))
                            .appearance(false),
                        cx,
                    ))
                    .child(specimen(
                        "masked",
                        Input::new(&showcase.secret_input)
                            .small()
                            .w(px(200.0))
                            .content_type(InputContentType::Password),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Number input",
                "The bounded, steppable NumberInput used by the pane-geometry settings.",
                cx,
            )
            .child(
                specimens().child(specimen(
                    "bounded 0–256",
                    div()
                        .w(px(120.0))
                        .child(NumberInput::new(&showcase.pane_corner_radius).small()),
                    cx,
                )),
            ),
        )
        .child(
            gallery(
                "Selects",
                "The dropdown that backs every enumerated multiplexer option: mode keys and clipboard ownership are the two the app renders.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "mode keys",
                        Select::new(&showcase.mux_mode_keys).small().w(px(160.0)),
                        cx,
                    ))
                    .child(specimen(
                        "set clipboard",
                        Select::new(&showcase.mux_set_clipboard).small().w(px(160.0)),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

pub(super) fn toggles_keys(cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Switches",
                "The toggle in every state the settings surfaces render.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen("off", Switch::new("tk-off").small(), cx))
                    .child(specimen(
                        "on",
                        Switch::new("tk-on").small().checked(true),
                        cx,
                    ))
                    .child(specimen(
                        "disabled off",
                        Switch::new("tk-doff").small().disabled(true),
                        cx,
                    ))
                    .child(specimen(
                        "disabled on",
                        Switch::new("tk-don").small().checked(true).disabled(true),
                        cx,
                    ))
                    .child(specimen(
                        "tooltip (hover)",
                        Switch::new("tk-tip")
                            .small()
                            .checked(true)
                            .tooltip("Toggle me"),
                        cx,
                    ))
                    .child(specimen("medium", Switch::new("tk-md").checked(true), cx)),
            ),
        )
        .child(
            gallery(
                "Keyboard hints",
                "One muted pill, and the palette shortcut hint built from it. Every hint surface (palette, chooser, pane picker, pane indicator) renders the same treatment.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen("single key", kbd("cmd-k"), cx))
                    .child(specimen("named key", kbd("escape"), cx))
                    .child(specimen("chord", kbd("cmd-shift-p"), cx))
                    .child(specimen(
                        "palette hint",
                        palette_shortcut_hint(["tab"], "complete"),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Spinners & dividers",
                "The loading spinner across sizes and tints, and the vertical rule.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen("spinner · xsmall", Spinner::new().xsmall(), cx))
                    .child(specimen("spinner · small", Spinner::new().small(), cx))
                    .child(specimen("spinner · medium", Spinner::new(), cx))
                    .child(specimen(
                        "spinner · primary",
                        Spinner::new().small().color(cx.theme().foreground),
                        cx,
                    ))
                    .child(specimen(
                        "spinner · warning",
                        Spinner::new().small().color(cx.theme().warning),
                        cx,
                    ))
                    .child(specimen(
                        "vertical separator",
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .h(px(20.0))
                            .text_sm()
                            .text_color(cx.theme().foreground.muted())
                            .child("left")
                            .child(Separator::vertical())
                            .child("right"),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

fn kbd(shortcut: &'static str) -> Kbd {
    Kbd::new(Keystroke::parse(shortcut).expect("static showcase keystroke"))
}
