//! The settings pieces: the page stack, its entries, the navigation buttons,
//! and the provenance and reset controls.

use gpui::{
    Anchor, AnyElement, App, Context, ParentElement as _, Styled as _, div, prelude::*, px,
};
use zz_ui::settings::{
    SettingEntry, SettingsNavigationGroup, SettingsSection, SettingsStack, settings_control_fill,
    settings_navigation_button, settings_navigation_group_label, settings_page_description,
    settings_provenance_badge, settings_reset_button,
};
use zz_ui::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    color_picker::ColorPicker,
    h_flex,
    input::{Input, NumberInput},
    menu::{DropdownMenu as _, PopupMenuItem},
    select::Select,
    switch::Switch,
};

const PRESET_FIXTURES: [(&str, [&str; 6]); 3] = [
    (
        "Tokyo Night",
        [
            "#1a1b26", "#c0caf5", "#292e42", "#9ece6a", "#e0af68", "#f7768e",
        ],
    ),
    (
        "Catppuccin Mocha",
        [
            "#1e1e2e", "#cdd6f4", "#313244", "#a6e3a1", "#f9e2af", "#f38ba8",
        ],
    ),
    (
        "Rosé Pine Dawn",
        [
            "#faf4ed", "#575279", "#dfdad9", "#286983", "#ea9d34", "#b4637a",
        ],
    ),
];

fn swatches(colors: &'static [&'static str; 6], cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .gap(px(2.0))
        .children(colors.iter().map(|hex| {
            div()
                .size(px(6.0))
                .rounded_full()
                .bg(zz_ui::parse_hex(hex).unwrap_or_else(|_| cx.theme().border))
        }))
}

use super::{Showcase, gallery, specimen, specimen_block, specimens, story_stack};

pub(super) fn render(showcase: &mut Showcase, cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Setting stack",
                "A run of settings sharing one surface, divided by hairlines inset to the copy's left edge. Replaces the stack of individually-bordered cards: the grouping is carried by adjacency, so only the rule between two rows is paid for.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "untitled stack",
                        SettingsStack::new()
                            .child(
                                SettingEntry::new(
                                    "App sidebar",
                                    "Customize sidebar item visibility, ordering, and badge style.",
                                )
                                .control(
                                    Button::new("stack-sidebar")
                                        .small()
                                        .ghost()
                                        .label("Customize"),
                                ),
                            )
                            .child(
                                SettingEntry::new(
                                    "Set clipboard",
                                    "Whether copy actions also update the system clipboard.",
                                )
                                .control(
                                    Select::new(&showcase.mux_set_clipboard)
                                        .small()
                                        .w(px(120.0))
                                        .bg(settings_control_fill(cx)),
                                ),
                            )
                            .child(
                                SettingEntry::new(
                                    "Window background blur",
                                    "Blur content behind translucent window areas when supported by the desktop.",
                                )
                                .title_actions(settings_provenance_badge("Default"))
                                .control(
                                    Switch::new("stack-blur")
                                        .checked(showcase.window_background_blur)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.window_background_blur = *checked;
                                            cx.notify();
                                        })),
                                ),
                            )
                            .child(
                                SettingEntry::new(
                                    "Synchronize panes",
                                    "Send input to every pane in the active window.",
                                )
                                .title_actions(settings_provenance_badge("From mux.conf"))
                                .control(
                                    Switch::new("stack-sync")
                                        .checked(showcase.synchronize_panes)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.synchronize_panes = *checked;
                                            cx.notify();
                                        })),
                                ),
                            ),
                        cx,
                    ))
                    .child(specimen_block(
                        "titled · reset & provenance · inert row",
                        SettingsStack::titled("Frame")
                            .description("Applies only while pane gaps are enabled.")
                            .child(
                                SettingEntry::new(
                                    "Pane corner radius",
                                    "Rounds every pane corner on all platforms, in logical pixels (0–256).",
                                )
                                .title_actions(
                                    h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(settings_reset_button(
                                            "stack-radius-reset",
                                            "Reset to the inherited or default value",
                                            true,
                                        ))
                                        .child(settings_provenance_badge("Overridden")),
                                )
                                .control(
                                    div().w(px(120.0)).child(
                                        NumberInput::new(&showcase.pane_corner_radius)
                                            .small()
                                            .bg(settings_control_fill(cx)),
                                    ),
                                ),
                            )
                            .child(
                                SettingEntry::new(
                                    "Background",
                                    "The window's base plane. Every panel, popover and hover state is this color, raised.",
                                )
                                .title_actions(settings_provenance_badge("Default"))
                                .control(
                                    ColorPicker::new(&showcase.chrome_background, cx.theme().background)
                                        .label("Background"),
                                ),
                            )
                            .child(
                                SettingEntry::new(
                                    "Pane shadow",
                                    "Draw a drop shadow around each pane while pane gaps are enabled.",
                                )
                                .title_actions(settings_provenance_badge("Default"))
                                .control(Switch::new("stack-shadow").checked(true))
                                .disabled(true),
                            ),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Entry controls",
                "Every control kind a settings page mounts, in one run. The control must be bounded: it keeps its natural width while the copy column shrinks, so one that grows with its content squeezes the copy to a character per line. Give it a width, or make it a single trigger that opens a menu.",
                cx,
            )
            .child(
                specimens().w_full().child(specimen_block(
                    "text · color · menu · action",
                    SettingsStack::new()
                        .child(
                            SettingEntry::new(
                                "Prefix",
                                "Key used before prefix-table shortcuts.",
                            )
                            .title_actions(settings_provenance_badge("From mux.conf"))
                            .control(
                                Input::new(&showcase.mux_prefix)
                                    .small()
                                    .w(px(200.0))
                                    .bg(settings_control_fill(cx)),
                            ),
                        )
                        .child(
                            SettingEntry::new(
                                "Background",
                                "The window's base plane. Every panel, popover and hover state is this color, raised.",
                            )
                            .title_actions(settings_provenance_badge("Default"))
                            .control(
                                ColorPicker::new(&showcase.chrome_background, cx.theme().background)
                                    .label("Background"),
                            ),
                        )
                        .child(
                            SettingEntry::new(
                                "Preset",
                                "Apply a complete palette. This overwrites the six colors below and the mode; each one stays editable afterwards.",
                            )
                            .control(
                                Button::new("entry-preset")
                                    .small()
                                    .label("Apply preset…")
                                    .dropdown_caret(true)
                                    .bg(settings_control_fill(cx))
                                    .dropdown_menu_with_anchor(Anchor::TopRight, |menu, _, _| {
                                        PRESET_FIXTURES.iter().fold(
                                            menu.min_w(px(230.0)),
                                            |menu, (name, colors)| {
                                                menu.item(PopupMenuItem::element(move |_, cx| {
                                                    h_flex()
                                                        .w_full()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap(px(12.0))
                                                        .py(px(2.0))
                                                        .child(*name)
                                                        .child(swatches(colors, cx))
                                                }))
                                            },
                                        )
                                    }),
                            ),
                        )
                        .child(
                            SettingEntry::new(
                                "Import configuration",
                                "Copy your Ghostty appearance into zz/config and your tmux configuration into zz/mux.conf. Run the import again any time to sync donor changes.",
                            )
                            .control(
                                Button::new("entry-import")
                                    .flex_none()
                                    .small()
                                    .label("Import…")
                                    .disabled(true)
                                    .tooltip("No Ghostty or tmux configuration was found to import"),
                            ),
                        ),
                    cx,
                )),
            ),
        )
        .child(
            gallery(
                "Section navigation",
                "Selected and unselected sidebar entries, their group labels, and the context line at the top of each page.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "selected",
                        settings_navigation_button(SettingsSection::Appearance, true, cx),
                        cx,
                    ))
                    .child(specimen(
                        "unselected",
                        settings_navigation_button(SettingsSection::Multiplexer, false, cx),
                        cx,
                    ))
                    .child(specimen(
                        "group label",
                        div()
                            .w(px(180.0))
                            .child(settings_navigation_group_label(
                                SettingsNavigationGroup::Tools,
                                cx,
                            )),
                        cx,
                    ))
                    .child(specimen(
                        "page description",
                        div()
                            .w(px(360.0))
                            .child(settings_page_description(SettingsSection::Panes, cx)),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Provenance & reset",
                "The badges that mark where an effective value came from, and the per-key reset button in both states.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen("default", settings_provenance_badge("Default"), cx))
                    .child(specimen("from tmux", settings_provenance_badge("From tmux"), cx))
                    .child(specimen("from ghostty", settings_provenance_badge("From Ghostty"), cx))
                    .child(specimen("runtime", settings_provenance_badge("Runtime"), cx))
                    .child(specimen("overridden", settings_provenance_badge("Overridden"), cx))
                    .child(specimen(
                        "reset · enabled",
                        settings_reset_button("set-reset-on", "Reset to the inherited or default value", true),
                        cx,
                    ))
                    .child(specimen(
                        "reset · disabled",
                        settings_reset_button("set-reset-off", "No zz/config override to reset", false),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}
