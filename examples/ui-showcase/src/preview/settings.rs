use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

use super::Preview;
use gpui::{
    AnyElement, App, Context, Entity, Image, ImageFormat, IntoElement, Subscription, Window, div,
    img, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, IndexPath, Sizable as _, Theme, ThemeColor, ThemeMode,
    button::Button,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{InputEvent, InputState, NumberInput},
    select::{Select, SelectState},
    settings::{
        SettingEntry, SettingsSection, SettingsSelectItem,
        appearance::{
            AppearancePageItem, appearance_page, appearance_page_items, picker_tile, theme_preview,
        },
        settings_control_fill, settings_provenance_badge, settings_reset_button,
    },
    switch::Switch,
};

pub(super) fn sidebar_logo() -> Arc<Image> {
    static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../../assets/linux/hicolor/256x256/apps/zz.png").to_vec(),
        ))
    });
    LOGO.clone()
}

fn app_icon(dark: bool) -> Arc<Image> {
    static ICONS: LazyLock<[Arc<Image>; 2]> = LazyLock::new(|| {
        [
            include_bytes!("../../../../assets/zz-light-512.png").as_slice(),
            include_bytes!("../../../../assets/zz-dark-512.png").as_slice(),
        ]
        .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec())))
    });
    ICONS[usize::from(dark)].clone()
}

const COLORS: [(&str, &str); 6] = [
    (
        "Background",
        "The window's base plane. Every panel, popover and hover state is this color, raised.",
    ),
    (
        "Foreground",
        "Default text, and the source of muted text, focus rings, links and selection.",
    ),
    (
        "Border",
        "Every edge: panel borders, dividers, input outlines, the window frame.",
    ),
    ("Success", "Something completed or is healthy."),
    ("Warning", "Something needs attention but still works."),
    ("Danger", "Something failed or is destructive."),
];

pub(super) struct SettingsFixture {
    numbers: BTreeMap<&'static str, Entity<InputState>>,
    colors: Vec<Entity<ColorPickerState>>,
    alignment: Entity<SelectState<Vec<SettingsSelectItem>>>,
    clock: Entity<SelectState<Vec<SettingsSelectItem>>>,
    toggles: BTreeMap<&'static str, bool>,
    app_icon: usize,
    theme_mode: usize,
    _subscriptions: Vec<Subscription>,
}

impl SettingsFixture {
    pub fn new(window: &mut Window, cx: &mut Context<Preview>) -> Self {
        let mut subscriptions = Vec::new();
        let options = cx.global::<super::PreviewOptions>().clone();
        let numbers = [
            ("zoom", options.zoom * 100.0, 50.0, 300.0, 5.0),
            ("radius", options.radius, 0.0, 24.0, 1.0),
            ("opacity", options.inactive_opacity, 0.0, 1.0, 0.1),
            ("margin", options.pane_margin, 0.0, 32.0, 1.0),
            ("pane-radius", options.pane_radius, 0.0, 32.0, 0.5),
            ("border", options.pane_border, 0.0, 8.0, 0.5),
        ]
        .into_iter()
        .map(|(key, value, min, max, step)| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value.to_string())
                    .min(min)
                    .max(max)
                    .step(step)
            });
            subscriptions.push(cx.subscribe(&input, move |this, input, event, cx| {
                if matches!(event, InputEvent::Change)
                    && let Ok(value) = input.read(cx).value().parse::<f32>()
                    && value.is_finite()
                    && (min..=max).contains(&f64::from(value))
                {
                    match key {
                        "zoom" => this.options.zoom = value / 100.0,
                        "radius" => {
                            this.options.radius = value;
                            Theme::global_mut(cx).radius = px(value);
                        }
                        "opacity" => this.options.inactive_opacity = value,
                        "margin" => this.options.pane_margin = value,
                        "pane-radius" => this.options.pane_radius = value,
                        "border" => this.options.pane_border = value,
                        _ => {}
                    }
                    this.remember(cx);
                    cx.notify();
                }
            }));
            (key, input)
        })
        .collect();
        let select =
            |items: Vec<SettingsSelectItem>, window: &mut Window, cx: &mut Context<Preview>| {
                cx.new(|cx| SelectState::new(items, Some(IndexPath::default()), window, cx))
            };
        let colors = (0..6)
            .map(|index| {
                let picker = cx.new(|cx| {
                    ColorPickerState::new(
                        options.chrome_colors[index]
                            .as_deref()
                            .and_then(|color| zz_ui::parse_hex(color).ok()),
                        window,
                        cx,
                    )
                });
                subscriptions.push(cx.subscribe(
                    &picker,
                    move |this, _, event: &ColorPickerEvent, cx| {
                        let ColorPickerEvent::Change(color) = event;
                        this.options.chrome_colors[index] = color.map(zz_ui::to_hex);
                        this.remember(cx);
                        cx.notify();
                    },
                ));
                picker
            })
            .collect();
        Self {
            numbers,
            colors,
            alignment: select(
                vec![
                    SettingsSelectItem::new("Left", "left"),
                    SettingsSelectItem::new("Center", "center"),
                ],
                window,
                cx,
            ),
            clock: select(
                [
                    ("24-hour", "24-hour"),
                    ("12-hour", "12-hour"),
                    ("Time and date", "time-date"),
                    ("Off", "off"),
                ]
                .map(|(a, b)| SettingsSelectItem::new(a, b))
                .to_vec(),
                window,
                cx,
            ),
            toggles: BTreeMap::new(),
            app_icon: 0,
            theme_mode: if options.dark { 2 } else { 1 },
            _subscriptions: subscriptions,
        }
    }
}

impl Preview {
    fn annotations(id: &'static str) -> gpui::Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .child(settings_reset_button(
                format!("preview-reset-{id}"),
                "Already using the default value",
                false,
            ))
            .child(settings_provenance_badge("Default"))
    }

    fn number(
        &self,
        id: &'static str,
        title: &'static str,
        description: &'static str,
        cx: &App,
    ) -> SettingEntry {
        SettingEntry::new(title, description)
            .title_actions(Self::annotations(id))
            .control(
                div().w(px(120.0)).flex_none().child(
                    NumberInput::new(&self.settings_state.numbers[id])
                        .small()
                        .bg(settings_control_fill(cx)),
                ),
            )
    }

    fn toggle(
        &self,
        id: &'static str,
        title: &'static str,
        description: &'static str,
        cx: &mut Context<Self>,
    ) -> SettingEntry {
        let checked = match id {
            "gaps" => self.options.gaps,
            "blur" => self.options.blur,
            _ => *self.settings_state.toggles.get(id).unwrap_or(&true),
        };
        SettingEntry::new(title, description)
            .title_actions(Self::annotations(id))
            .control(
                Switch::new(format!("preview-setting-{id}"))
                    .checked(checked)
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.settings_state.toggles.insert(id, *checked);
                        if id == "gaps" {
                            this.options.gaps = *checked;
                        } else if id == "blur" {
                            this.options.blur = *checked;
                        }
                        this.remember(cx);
                        cx.notify();
                    })),
            )
    }

    pub(super) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = if self.settings == SettingsSection::Panes {
            zz_ui::settings::panes_page(
self.toggle("gaps", "Pane gaps", "Separate panes with card-like spacing and chrome.", cx),
self.number("opacity", "Inactive pane opacity", "Visible strength of inactive pane content and chrome (0–1). Set to 1 to disable dimming.", cx),
self.number("margin", "Pane margin", "Space around each pane on all platforms, in logical pixels (0–32).", cx).disabled(!self.options.gaps),
self.number("pane-radius", "Pane corner radius", "Rounds every pane corner on all platforms, in logical pixels (0–32).", cx).disabled(!self.options.gaps),
self.number("border", "Pane border width", "Border width for gapped panes, in logical pixels (0–8). Set to 0 to disable.", cx).disabled(!self.options.gaps), cx).into_any_element()
        } else {
            let view = cx.entity();
            appearance_page(
                appearance_page_items(0..6, self.options.macos, self.options.macos),
                move |item, position, _, cx| {
                    view.update(cx, |this, cx| {
                        this.appearance_entry(item, cx)
                            .position(position)
                            .into_any_element()
                    })
                },
            )
            .into_any_element()
        };
        div()
            .id("settings-route")
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(self.chrome_background(cx))
            .text_color(cx.theme().foreground)
            .child(content)
            .into_any_element()
    }

    fn appearance_entry(
        &self,
        item: AppearancePageItem<usize>,
        cx: &mut Context<Self>,
    ) -> SettingEntry {
        match item {
            AppearancePageItem::ThemeMode => {
                SettingEntry::new("Theme", "Follow the system light/dark setting, or pin one.")
                    .title_actions(Self::annotations("theme"))
                    .control(
                        div().flex().flex_none().gap(px(10.0)).children(
                            [
                                (None, "System"),
                                (Some(ThemeMode::Light), "Light"),
                                (Some(ThemeMode::Dark), "Dark"),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(i, (mode, title))| {
                                picker_tile(
                                    format!("preview-theme-{i}").into(),
                                    title,
                                    theme_preview(
                                        mode,
                                        &ThemeColor::light(),
                                        &ThemeColor::dark(),
                                        cx,
                                    ),
                                    self.settings_state.theme_mode == i,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        let mode =
                                            mode.unwrap_or(ThemeMode::from(cx.window_appearance()));
                                        this.settings_state.theme_mode = i;
                                        this.options.dark = mode.is_dark();
                                        Theme::change(mode, Some(window), cx);
                                        Theme::global_mut(cx).radius = px(this.options.radius);
                                        this.remember(cx);
                                        cx.notify();
                                    },
                                ))
                            }),
                        ),
                    )
            }
            AppearancePageItem::UiZoom => self.number(
                "zoom",
                "UI zoom",
                "Scales application text, icons, and controls, as a percentage of the default.",
                cx,
            ),
            AppearancePageItem::AppIcon => SettingEntry::new("App Icon", "Dock and app switcher.")
                .title_actions(Self::annotations("app-icon"))
                .control(
                    div().flex().flex_none().gap(px(10.0)).children(
                        ["Automatic", "Light", "Dark"]
                            .into_iter()
                            .enumerate()
                            .map(|(i, title)| {
                                picker_tile(
                                    format!("preview-app-icon-{i}").into(),
                                    title,
                                    img(app_icon(if i == 0 { self.options.dark } else { i == 2 }))
                                        .size(px(48.0)),
                                    self.settings_state.app_icon == i,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.settings_state.app_icon = i;
                                        cx.notify();
                                    },
                                ))
                            }),
                    ),
                ),
            AppearancePageItem::Preset => SettingEntry::new(
                "Preset",
                "Choose a color theme or manually define your own below.",
            )
            .title_actions(Self::annotations("preset"))
            .control(
                Button::new("preview-preset")
                    .small()
                    .label("Color theme")
                    .dropdown_caret(true)
                    .bg(settings_control_fill(cx)),
            ),
            AppearancePageItem::ChromeColor(i) => {
                let color = [
                    cx.theme().background,
                    cx.theme().foreground,
                    cx.theme().border,
                    cx.theme().success,
                    cx.theme().warning,
                    cx.theme().danger,
                ][i];
                SettingEntry::new(COLORS[i].0, COLORS[i].1)
                    .title_actions(Self::annotations(COLORS[i].0))
                    .control(
                        ColorPicker::new(&self.settings_state.colors[i], color).label(COLORS[i].0),
                    )
            }
            AppearancePageItem::StatusShowSession => self.toggle(
                "session",
                "Session name",
                "Show the current session as a chip.",
                cx,
            ),
            AppearancePageItem::StatusBadges => self.toggle(
                "badges",
                "Window badges",
                "Show bell, activity, and agent markers on window items.",
                cx,
            ),
            AppearancePageItem::StatusAlignment => {
                SettingEntry::new("Alignment", "Align the window strip to the left or center.")
                    .title_actions(Self::annotations("alignment"))
                    .control(
                        div().w(px(120.0)).flex_none().child(
                            Select::new(&self.settings_state.alignment)
                                .small()
                                .bg(settings_control_fill(cx)),
                        ),
                    )
            }
            AppearancePageItem::StatusAgents => self.toggle(
                "agents",
                "Agents",
                "Show the number of running agent panes.",
                cx,
            ),
            AppearancePageItem::StatusHost => self.toggle(
                "host",
                "Host",
                "Show the attached host when it is remote.",
                cx,
            ),
            AppearancePageItem::StatusUpdate => self.toggle(
                "update",
                "Update",
                "Show an available version and install it from the bar.",
                cx,
            ),
            AppearancePageItem::StatusClock => {
                SettingEntry::new("Clock", "Choose the clock format, or hide it.")
                    .title_actions(Self::annotations("clock"))
                    .control(
                        div().w(px(120.0)).flex_none().child(
                            Select::new(&self.settings_state.clock)
                                .small()
                                .bg(settings_control_fill(cx)),
                        ),
                    )
            }
            AppearancePageItem::Animations => self.toggle(
                "animations",
                "Animations",
                "Animate interface transitions, loading indicators, and image frames.",
                cx,
            ),
            AppearancePageItem::WidgetCornerRadius => self.number(
                "radius",
                "Widget corner radius",
                "Rounds every widget: buttons, inputs, tags, menus, dialogs.",
                cx,
            ),
            AppearancePageItem::WindowBackgroundBlur => self.toggle(
                "blur",
                "Window blur",
                "Blur the desktop through the window chrome, if the compositor supports it.",
                cx,
            ),
            #[cfg(target_os = "linux")]
            AppearancePageItem::WindowCornerRadius | AppearancePageItem::UseSystemTitlebar => {
                SettingEntry::new(
                    "System titlebar",
                    "Ask the desktop to draw the window titlebar and borders when supported.",
                )
            }
            AppearancePageItem::Description | AppearancePageItem::Group { .. } => unreachable!(),
        }
    }
}
