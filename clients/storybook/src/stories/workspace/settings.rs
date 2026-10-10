use std::sync::{Arc, LazyLock};

use zz_client::StatusBarSettings;
use zz_gpui::{
    AnyElement, App, AppContext as _, Entity, IntoElement, ParentElement as _, Styled as _, Window,
    div, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, IndexPath, Sizable as _, ThemeMode,
    chrome_palette::{
        ThemeModeSetting, chrome_presets, inherited_chrome_colors, pinned_theme_mode,
    },
    input::{InputState, NumberInput},
    interface_style::InterfaceStyle,
    select::{Select, SelectState},
    settings::{
        SettingEntry, SettingsNavigationGroup, SettingsSection, SettingsSelectItem, SettingsStack,
        about::{
            ABOUT_LOGO_SIZE, about_build_stack, about_copy_button, about_hero, about_project_stack,
        },
        appearance::{
            PickerStrip, interface_style_preview, palette_preview, picker_tile, theme_preview,
            ui_font_select,
        },
        panes_page,
        panes_preview::PanesPreview,
        settings_control_fill, settings_list_disclosure_header, settings_navigation_back_button,
        settings_navigation_back_row, settings_navigation_button, settings_navigation_group_label,
        settings_page_description, settings_provenance_badge, settings_reset_button,
        settings_scroll_column, settings_section_index,
        status_bar_preview::status_bar_page,
    },
    switch::Switch,
};

use super::{
    fixtures::{INACTIVE_OPACITY, PANE_BORDER, PANE_MARGIN, PANE_RADIUS, stateful},
    tree::sidebar,
};
use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "settings",
    name: "Settings",
    group: "Settings",
    summary: "The settings route: its sidebar and narrow-screen index, the stacked entry rows every page is built from, the appearance pickers, and the Panes, Status bar and About pages.",
    sections: &[
        Section {
            id: "navigation",
            name: "Navigation",
            summary: "On a wide window the sidebar swaps the workspace tree for the section list. On a narrow one the sections become a touch-size index.",
            build: |_, cx| stateless(navigation, cx),
        },
        Section {
            id: "entries",
            name: "Stacks and entries",
            summary: "SettingsStack glues SettingEntry rows into one surface and rounds only its ends. Controls sit at the right edge; reset and provenance sit beside the title.",
            build: |window, cx| stateful(window, cx, Controls::new, entries),
        },
        Section {
            id: "appearance",
            name: "Appearance pickers",
            summary: "Theme and style tiles, the per-mode palette strips that scroll sideways, the UI font select and the Advanced heading from the Interface page.",
            build: |window, cx| stateful(window, cx, Controls::new, appearance),
        },
        Section {
            id: "palettes",
            name: "Palettes",
            summary: "palette_preview for every built-in chroma preset, as the palette strips draw them.",
            build: |_, cx| stateless(palettes, cx),
        },
        Section {
            id: "panes-page",
            name: "Panes page",
            summary: "panes_page with the live preview above its Layout, Appearance, Focus and Frame stacks. Frame settings dim while pane gaps are off.",
            build: |window, cx| stateful(window, cx, Controls::new, panes),
        },
        Section {
            id: "status-bar-page",
            name: "Status bar page",
            summary: "status_bar_page: a preview of the titlebar that follows the switches below it.",
            build: |_, cx| stateless(status_bar, cx),
        },
        Section {
            id: "about",
            name: "About",
            summary: "The About page: the logo and version, the build details to quote in a bug report, and the project links.",
            build: |_, cx| stateless(about, cx),
        },
    ],
};

struct Controls {
    numbers: [Entity<InputState>; 8],
    choice: Entity<SelectState<Vec<SettingsSelectItem>>>,
    font: Entity<SelectState<Vec<SettingsSelectItem>>>,
}

impl Controls {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        let values = ["50", "0.7", "100", "6", "13.5", "0.5", "100", "80"];
        let numbers = std::array::from_fn(|index| {
            let value = values[index];
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        });
        let choice = cx.new(|cx| {
            SelectState::new(
                vec![
                    SettingsSelectItem::new("Tree", "tree"),
                    SettingsSelectItem::new("Flat list", "flat"),
                ],
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let font = ui_font_select(Some("Inter Variable"), window, cx);
        Self {
            numbers,
            choice,
            font,
        }
    }

    fn number(&self, index: usize, cx: &App) -> zz_gpui::Div {
        div().w(px(120.0)).flex_none().child(
            NumberInput::new(&self.numbers[index])
                .small()
                .bg(settings_control_fill(cx)),
        )
    }
}

fn navigation(window: &mut Window, cx: &mut App) -> AnyElement {
    let selected = SettingsSection::Appearance;
    let mut rows = Vec::new();
    let mut previous: Option<SettingsNavigationGroup> = None;
    for section in SettingsSection::ALL {
        let group = section.navigation_group();
        if previous != Some(group) {
            rows.push(settings_navigation_group_label(group, cx).into_any_element());
            previous = Some(group);
        }
        rows.push(settings_navigation_button(section, section == selected, cx).into_any_element());
    }
    let list = div()
        .id("settings-nav")
        .flex()
        .flex_col()
        .w_full()
        .gap(px(2.0))
        .px(px(6.0))
        .child(settings_navigation_back_row(
            settings_navigation_back_button("settings-back"),
        ))
        .children(rows);
    let index = settings_section_index(
        &SettingsSection::ALL,
        |section| match section {
            SettingsSection::Hosts => Some("devbox".into()),
            SettingsSection::About => Some("0.16.0".into()),
            _ => None,
        },
        |_, _, _| {},
        window,
        cx,
    );
    states()
        .columns(2)
        .state(
            "sidebar, Interface selected",
            div()
                .id("settings-sidebar")
                .flex()
                .h(px(560.0))
                .child(sidebar(list, true, cx)),
        )
        .state(
            "narrow screens: section index",
            div()
                .id("settings-index")
                .flex()
                .w(px(360.0))
                .h(px(660.0))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border())
                .child(index),
        )
        .into_any_element()
}

fn reset_actions(id: &'static str, changed: bool, source: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .child(settings_reset_button(id, "Reset to default", changed))
        .child(settings_provenance_badge(source))
}

fn entries(controls: &Controls, _: &mut Window, cx: &mut App) -> AnyElement {
    let chevron = Icon::new(IconName::ChevronRight)
        .size(px(14.0))
        .text_color(cx.theme().foreground.muted());
    states()
        .state("page heading", settings_page_description(SettingsSection::Panes, cx))
        .state(
            "titled stack: switch, number and select controls",
            SettingsStack::titled("Command palette")
                .description("Tune the command palette on this client.")
                .child(
                    SettingEntry::new(
                        "Command shortcuts",
                        "Show keyboard shortcuts beside commands.",
                    )
                    .control(Switch::new("shortcuts").checked(true)),
                )
                .child(
                    SettingEntry::new(
                        "Agent panes",
                        "Allow creating agent panes on this client.",
                    )
                    .control(Switch::new("agents").checked(false)),
                )
                .child(
                    SettingEntry::new(
                        "UI zoom",
                        "Scales application text, icons, and controls as a percentage of the default.",
                    )
                    .control(controls.number(6, cx)),
                )
                .child(
                    SettingEntry::new(
                        "Navigation layout",
                        "Show sessions, windows, and panes as a tree or a flat list.",
                    )
                    .control(
                        Select::new(&controls.choice)
                            .small()
                            .title("Navigation layout")
                            .bg(settings_control_fill(cx)),
                    ),
                ),
        )
        .state(
            "reset and provenance beside the title",
            SettingsStack::titled("Theme")
                .child(
                    SettingEntry::new("Animations", "Animate interface transitions.")
                        .title_actions(reset_actions("reset-changed", true, "zz/config"))
                        .control(Switch::new("animations").checked(false)),
                )
                .child(
                    SettingEntry::new("Shadow strength", "Strength of shadows around controls.")
                        .title_actions(reset_actions("reset-default", false, "default"))
                        .control(controls.number(7, cx)),
                ),
        )
        .state(
            "a single entry rounds all four corners",
            SettingsStack::new().child(
                SettingEntry::new("Sidebar", "Show sessions, windows, and panes beside the workspace.")
                    .control(Switch::new("sidebar").checked(true)),
            ),
        )
        .state(
            "disabled: no effect with the current configuration",
            SettingsStack::titled("Frame")
                .description("Applies only while pane gaps are enabled.")
                .child(
                    SettingEntry::new("Pane margin", "Space around each pane, in logical pixels (0-32).")
                        .disabled(true)
                        .control(controls.number(3, cx)),
                )
                .child(
                    SettingEntry::new("Pane border width", "Border width for gapped panes (0-8).")
                        .disabled(true)
                        .control(controls.number(5, cx)),
                ),
        )
        .state(
            "a whole row as a button, with a state glyph",
            SettingsStack::titled("Hosts").child(
                SettingEntry::new("devbox", "fabrico@devbox.local · connected")
                    .title_icon(Icon::new(IconName::CircleCheck).text_color(cx.theme().success))
                    .on_click("open-host", |_, _, _| {})
                    .control(chevron),
            ),
        )
        .state(
            "full-width content under the copy",
            SettingsStack::new().child(
                SettingEntry::new(
                    "Colors, cursor, and spacing",
                    "Edit the Ghostty-compatible configuration on the daemon host to change these values.",
                )
                .child(
                    div()
                        .p(px(10.0))
                        .rounded(cx.theme().radius)
                        .bg(settings_control_fill(cx))
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(px(12.0))
                        .child("font-family = Lilex\ncursor-style = bar\nwindow-padding-x = 8"),
                ),
            ),
        )
        .into_any_element()
}

fn appearance(controls: &Controls, _: &mut Window, cx: &mut App) -> AnyElement {
    let light = inherited_chrome_colors(None, ThemeMode::Light);
    let dark = inherited_chrome_colors(None, ThemeMode::Dark);
    let tiles =
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap(px(8.0))
            .children(ThemeModeSetting::ALL.map(|mode| {
                picker_tile(
                    format!("theme-{}", mode.as_str()).into(),
                    mode.title(),
                    theme_preview(pinned_theme_mode(mode), &light, &dark, cx),
                    mode == ThemeModeSetting::System,
                    cx,
                )
            }));
    let strip = |id: &'static str, mode: ThemeMode, selected: usize| {
        let presets = std::iter::once(None)
            .chain(chrome_presets(mode.is_dark()).map(|preset| Some(preset.id)))
            .collect::<Vec<_>>();
        PickerStrip::new(id, selected).tiles(presets.into_iter().map(|preset| {
            (
                preset.map_or("Default", |id| id.preset().name),
                palette_preview(&inherited_chrome_colors(preset, mode), cx),
            )
        }))
    };
    states()
        .state(
            "theme and font",
            SettingsStack::titled("Interface")
                .child(
                    SettingEntry::new("Theme", "Follow the system light/dark setting, or pin one.")
                        .title_actions(settings_reset_button(
                            "theme-reset",
                            "Reset to default",
                            false,
                        ))
                        .control(tiles),
                )
                .child(
                    SettingEntry::new(
                        "Style",
                        "Corners, outlines, shadows, and how menus and dialogs float.",
                    )
                    .control(div().flex().flex_none().gap(px(8.0)).children(
                        InterfaceStyle::ALL.map(|style| {
                            picker_tile(
                                format!("style-{}", style.as_str()).into(),
                                style.title(),
                                interface_style_preview(style, cx),
                                style == InterfaceStyle::DEFAULT,
                                cx,
                            )
                        }),
                    )),
                )
                .child(
                    SettingEntry::new("UI font", "Choose an available font for the interface.")
                        .control(
                            div().w(px(200.0)).flex_none().child(
                                Select::new(&controls.font)
                                    .small()
                                    .title("UI font")
                                    .placeholder("System default")
                                    .bg(settings_control_fill(cx)),
                            ),
                        ),
                ),
        )
        .state(
            "palette strips",
            SettingsStack::titled("Chroma Colors")
                .description("Pick a palette for each appearance, or set the base colors yourself.")
                .child(
                    SettingEntry::new("Light palette", "Used while the interface is light.")
                        .child(strip("strip-light", ThemeMode::Light, 0)),
                )
                .child(
                    SettingEntry::new("Dark palette", "Used while the interface is dark.")
                        .title_actions(settings_reset_button(
                            "dark-reset",
                            "Reset to default",
                            true,
                        ))
                        .child(strip("strip-dark", ThemeMode::Dark, 3)),
                ),
        )
        .state(
            "advanced heading, collapsed and expanded",
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .children([false, true].map(|expanded| {
                settings_list_disclosure_header(
                    if expanded {
                        "advanced-expanded"
                    } else {
                        "advanced-collapsed"
                    },
                    "Advanced",
                    Some(
                        "Fonts, zoom, base colors, and the corner and shadow sizes the style sets.",
                    ),
                    expanded,
                    cx,
                )
            })),
        )
        .into_any_element()
}

fn palettes(_: &mut Window, cx: &mut App) -> AnyElement {
    let group = |mode: ThemeMode| {
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .child(picker_tile(
                format!("palette-default-{}", mode.name()).into(),
                "Default",
                palette_preview(&inherited_chrome_colors(None, mode), cx),
                false,
                cx,
            ))
            .children(chrome_presets(mode.is_dark()).map(|preset| {
                picker_tile(
                    format!("palette-{}", preset.id.as_str()).into(),
                    preset.name,
                    palette_preview(&inherited_chrome_colors(Some(preset.id), mode), cx),
                    false,
                    cx,
                )
            }))
    };
    states()
        .state("light", group(ThemeMode::Light))
        .state("dark", group(ThemeMode::Dark))
        .into_any_element()
}

fn pane_entry(title: &'static str, description: &'static str) -> SettingEntry {
    SettingEntry::new(title, description)
}

fn panes(controls: &Controls, _: &mut Window, cx: &mut App) -> AnyElement {
    let page = |gaps: bool, cx: &App| {
        let reset = |id: &'static str| settings_reset_button(id, "Reset to default", false);
        panes_page(
            PanesPreview {
                gaps,
                margin: PANE_MARGIN,
                radius: PANE_RADIUS,
                border_width: PANE_BORDER,
                inactive_opacity: INACTIVE_OPACITY,
            },
            [
                pane_entry("Pane gaps", "Separate panes with spacing and borders.")
                    .title_actions(settings_reset_button("gaps-reset", "Reset to default", gaps))
                    .control(Switch::new("gaps").checked(gaps)),
                pane_entry("Agent panes", "Allow creating agent panes on this client.")
                    .control(Switch::new("agents").checked(true)),
            ],
            pane_entry(
                "Pane background opacity",
                "Background strength from 0% to 100%. Browser panes apply this to the toolbar only.",
            )
            .title_actions(reset("background-reset"))
            .control(controls.number(0, cx)),
            [
                pane_entry(
                    "Inactive pane opacity",
                    "Visible strength of inactive pane content and chrome (0-1). Set to 1 to disable dimming.",
                )
                .title_actions(reset("inactive-reset"))
                .control(controls.number(1, cx)),
                pane_entry(
                    "Selected pane glow",
                    "Glow strength from 0% to 200%. Set to 0 to turn it off.",
                )
                .title_actions(reset("glow-reset"))
                .control(controls.number(2, cx)),
            ],
            [
                pane_entry("Pane margin", "Space around each pane, in logical pixels (0-32).")
                    .disabled(!gaps)
                    .title_actions(reset("margin-reset"))
                    .control(controls.number(3, cx)),
                pane_entry("Pane corner radius", "Rounds every pane corner, in logical pixels (0-32).")
                    .disabled(!gaps)
                    .title_actions(reset("radius-reset"))
                    .control(controls.number(4, cx)),
                pane_entry(
                    "Pane border width",
                    "Border width for gapped panes, in logical pixels (0-8). Set to 0 to disable.",
                )
                .disabled(!gaps)
                .title_actions(reset("border-reset"))
                .control(controls.number(5, cx)),
            ],
            cx,
        )
    };
    states()
        .state(
            "gaps on",
            div()
                .id("panes-page")
                .flex()
                .h(px(1180.0))
                .child(page(true, cx)),
        )
        .into_any_element()
}

fn status_rows(settings: StatusBarSettings) -> SettingsStack {
    SettingsStack::new()
        .child(
            SettingEntry::new(
                "Sidebar",
                "Show sessions, windows, and panes beside the workspace.",
            )
            .control(Switch::new("sidebar").checked(false)),
        )
        .children(
            [
                (
                    "session",
                    "Session",
                    "Show the session menu in the titlebar.",
                    settings.show_session,
                ),
                (
                    "badges",
                    "Window badges",
                    "Show bell and activity markers on window items.",
                    settings.badges,
                ),
                (
                    "agents",
                    "Agent activity",
                    "Show agent activity in the titlebar.",
                    settings.show_agents,
                ),
            ]
            .map(|(id, title, description, checked)| {
                SettingEntry::new(title, description)
                    .title_actions(settings_reset_button(
                        format!("status-{id}-reset"),
                        "Reset to default",
                        !checked,
                    ))
                    .control(Switch::new(format!("status-{id}")).checked(checked))
            }),
        )
}

fn status_bar(_: &mut Window, cx: &mut App) -> AnyElement {
    let page = |id: &'static str, settings: StatusBarSettings, gaps: bool, cx: &App| {
        div().id(id).flex().h(px(470.0)).child(status_bar_page(
            settings,
            gaps,
            status_rows(settings),
            cx,
        ))
    };
    states()
        .state(
            "everything on",
            page("status-defaults", StatusBarSettings::default(), false, cx),
        )
        .state(
            "session, badges and agents off, pane gaps on",
            page(
                "status-minimal",
                StatusBarSettings {
                    show_session: false,
                    badges: false,
                    show_agents: false,
                    ..StatusBarSettings::default()
                },
                true,
                cx,
            ),
        )
        .into_any_element()
}

static LOGOS: LazyLock<[Arc<zz_gpui::Image>; 2]> = LazyLock::new(|| {
    [
        include_bytes!("../../../../../assets/zz-light-512.png").as_slice(),
        include_bytes!("../../../../../assets/zz-dark-512.png").as_slice(),
    ]
    .map(|bytes| {
        Arc::new(zz_gpui::Image::from_bytes(
            zz_gpui::ImageFormat::Png,
            bytes.to_vec(),
        ))
    })
});

fn about(_: &mut Window, cx: &mut App) -> AnyElement {
    let logo = Arc::clone(&LOGOS[usize::from(cx.theme().mode.is_dark())]);
    states()
        .state(
            "page",
            div().id("about").flex().h(px(780.0)).child(
                settings_scroll_column("settings-about")
                    .child(about_hero(zz_gpui::img(logo).size(px(ABOUT_LOGO_SIZE)), cx))
                    .child(about_build_stack(
                        "browser · wasm32",
                        about_copy_button("copy-build"),
                        cx,
                    ))
                    .child(about_project_stack(cx)),
            ),
        )
        .into_any_element()
}
