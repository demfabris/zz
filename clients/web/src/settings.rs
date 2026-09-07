use gpui::{
    AnyElement, App, Context, Entity, IntoElement, Subscription, Window, div, prelude::*, px,
};
use serde::{Deserialize, Serialize};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, IconName, Sizable as _, Theme, ThemeMode,
    UiZoom,
    button::Button,
    chrome_palette::{
        CHROME_PRESETS, ChromeColor, ChromePresetId, ThemeModeSetting, inherited_chrome_colors,
        resolved_chrome_colors,
    },
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{InputEvent, InputState, NumberInput},
    select::{Select, SelectState},
    settings::{
        SettingEntry, SettingsSection, SettingsSelectItem, SettingsStack,
        appearance::{picker_tile, theme_preview},
        settings_control_fill, settings_page_description, settings_reset_button,
        settings_scroll_column,
    },
    switch::Switch,
};

use super::WebClient;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub sidebar: bool,
    pub gaps: bool,
    pub dark: bool,
    pub mode: Option<String>,
    pub preset: Option<String>,
    pub colors: [Option<String>; 6],
    pub zoom: f32,
    pub radius: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sidebar: true,
            gaps: false,
            dark: true,
            mode: None,
            preset: None,
            colors: Default::default(),
            zoom: 1.0,
            radius: 6.0,
        }
    }
}

impl Preferences {
    pub(super) fn load(_: &App) -> Self {
        #[cfg(target_family = "wasm")]
        if let Some(value) = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item("zz-web-preferences").ok().flatten())
            .and_then(|value| serde_json::from_str::<Self>(&value).ok())
        {
            return Self {
                zoom: if value.zoom.is_finite() {
                    value.zoom.clamp(0.5, 3.0)
                } else {
                    1.0
                },
                radius: if value.radius.is_finite() {
                    value.radius.clamp(0.0, 24.0)
                } else {
                    6.0
                },
                ..value
            };
        }
        Self::default()
    }

    pub(super) fn save(&self) {
        #[cfg(target_family = "wasm")]
        if let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
            && let Ok(value) = serde_json::to_string(self)
        {
            let _ = storage.set_item("zz-web-preferences", &value);
        }
    }

    pub(super) fn apply(&self, window: &mut Window, cx: &mut App) {
        if let Some(mode) = self.theme_mode().pinned() {
            Theme::change(mode, Some(window), cx);
        } else {
            Theme::sync_system_appearance(Some(window), cx);
        }
        let mode = cx.theme().mode;
        Theme::global_mut(cx).colors = resolved_chrome_colors(
            self.preset.as_deref().and_then(ChromePresetId::parse),
            mode,
            self.colors
                .clone()
                .map(|color| color.and_then(|value| zz_ui::parse_hex(&value).ok())),
        );
        Theme::global_mut(cx).radius = px(self.radius);
        cx.set_global(UiZoom(self.zoom));
        window.set_zoom(self.zoom);
        cx.refresh_windows();
    }

    fn theme_mode(&self) -> ThemeModeSetting {
        self.mode
            .as_deref()
            .and_then(ThemeModeSetting::parse)
            .unwrap_or(if self.dark {
                ThemeModeSetting::Dark
            } else {
                ThemeModeSetting::Light
            })
    }

    pub(super) fn change_zoom(&mut self, step: f32, window: &mut Window, cx: &mut App) {
        self.zoom = ((self.zoom + step) * 10.0).round().clamp(5.0, 30.0) / 10.0;
        self.save();
        self.apply(window, cx);
    }

    pub(super) fn reset_zoom(&mut self, window: &mut Window, cx: &mut App) {
        self.zoom = 1.0;
        self.save();
        self.apply(window, cx);
    }
}

pub(super) struct Controls {
    zoom: Entity<InputState>,
    radius: Entity<InputState>,
    colors: Vec<Entity<ColorPickerState>>,
    search_engine: Entity<SelectState<Vec<SettingsSelectItem>>>,
    _subscriptions: Vec<Subscription>,
}

impl Controls {
    pub(super) fn new(
        preferences: &Preferences,
        window: &mut Window,
        cx: &mut Context<WebClient>,
    ) -> Self {
        let zoom = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("{:.0}", preferences.zoom * 100.0))
                .step(10.0)
                .min(50.0)
                .max(300.0)
        });
        let radius = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("{:.0}", preferences.radius))
                .step(1.0)
                .min(0.0)
                .max(24.0)
        });
        let mut subscriptions = Vec::new();
        for (input, is_zoom) in [(&zoom, true), (&radius, false)] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, event, window, cx| {
                    let commit = matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. });
                    if !commit && !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let (min, max, previous) = if is_zoom {
                        (50.0, 300.0, this.preferences.zoom * 100.0)
                    } else {
                        (0.0, 24.0, this.preferences.radius)
                    };
                    let parsed = input
                        .read(cx)
                        .value()
                        .parse::<f32>()
                        .ok()
                        .filter(|value| value.is_finite());
                    let value = match parsed {
                        Some(value) if commit => value.clamp(min, max),
                        Some(value) if (min..=max).contains(&value) => value,
                        _ if commit => previous,
                        _ => return,
                    };
                    if is_zoom {
                        this.preferences.zoom = value / 100.0;
                    } else {
                        this.preferences.radius = value;
                    }
                    if commit {
                        input.update(cx, |input, cx| {
                            input.set_value(format!("{value:.0}"), window, cx);
                        });
                    }
                    this.preferences.save();
                    this.preferences.apply(window, cx);
                },
            ));
        }
        let colors = preferences
            .colors
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let color = value
                    .as_deref()
                    .and_then(|value| zz_ui::parse_hex(value).ok());
                let state = cx.new(|cx| ColorPickerState::new(color, window, cx));
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event, window, cx| {
                        let ColorPickerEvent::Change(color) = event;
                        this.preferences.colors[index] = color.map(zz_ui::to_hex);
                        this.preferences.save();
                        this.preferences.apply(window, cx);
                    },
                ));
                state
            })
            .collect();
        Self {
            zoom,
            radius,
            colors,
            search_engine: cx.new(|cx| SelectState::new(Vec::new(), None, window, cx)),
            _subscriptions: subscriptions,
        }
    }
}

impl WebClient {
    pub(super) fn sync_zoom_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_controls.zoom.update(cx, |input, cx| {
            input.set_value(format!("{:.0}", self.preferences.zoom * 100.0), window, cx);
        });
    }
    fn select_preset(
        &mut self,
        preset: Option<ChromePresetId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preferences.preset = preset.map(|id| id.as_str().to_owned());
        self.preferences.colors = Default::default();
        for color in &self.settings_controls.colors {
            color.update(cx, |color, cx| color.set_color(None, window, cx));
        }
        self.preferences.save();
        self.preferences.apply(window, cx);
    }

    pub(super) fn render_settings(
        &self,
        section: SettingsSection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows = Vec::new();
        match section {
            SettingsSection::Appearance => {
                let preset = self
                    .preferences
                    .preset
                    .as_deref()
                    .and_then(ChromePresetId::parse);
                let light = inherited_chrome_colors(preset, ThemeMode::Light);
                let dark = inherited_chrome_colors(preset, ThemeMode::Dark);
                let theme = div()
                    .flex()
                    .gap(px(8.0))
                    .children(ThemeModeSetting::ALL.map(|mode| {
                        picker_tile(
                            format!("web-theme-{}", mode.as_str()).into(),
                            mode.title(),
                            theme_preview(mode.pinned(), &light, &dark, cx),
                            self.preferences.theme_mode() == mode,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.preferences.mode = Some(mode.as_str().into());
                                this.preferences.save();
                                this.preferences.apply(window, cx);
                            },
                        ))
                    }));
                rows.push(
                    SettingEntry::new(
                        "Theme",
                        "Follow the system appearance or choose light or dark.",
                    )
                    .child(theme),
                );
                rows.push(
                    SettingEntry::new("UI zoom", "Scale the workspace, including pane contents.")
                        .title_actions(
                            settings_reset_button(
                                "web-zoom-reset",
                                "Reset UI zoom to 100%",
                                self.preferences.zoom != 1.0,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.preferences.reset_zoom(window, cx);
                                    this.sync_zoom_input(window, cx);
                                },
                            )),
                        )
                        .control(
                            div().w(px(120.0)).flex_none().child(
                                NumberInput::new(&self.settings_controls.zoom)
                                    .small()
                                    .bg(settings_control_fill(cx)),
                            ),
                        ),
                );
                let presets = div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(
                        picker_tile(
                            "web-preset-default".into(),
                            "Default",
                            theme_preview(
                                Some(cx.theme().mode),
                                &inherited_chrome_colors(None, ThemeMode::Light),
                                &inherited_chrome_colors(None, ThemeMode::Dark),
                                cx,
                            ),
                            preset.is_none(),
                            cx,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.select_preset(None, window, cx)),
                        ),
                    )
                    .children(CHROME_PRESETS.iter().map(|entry| {
                        let id = entry.id;
                        picker_tile(
                            format!("web-preset-{}", id.as_str()).into(),
                            entry.name,
                            theme_preview(
                                Some(cx.theme().mode),
                                &inherited_chrome_colors(Some(id), ThemeMode::Light),
                                &inherited_chrome_colors(Some(id), ThemeMode::Dark),
                                cx,
                            ),
                            preset == Some(id),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.select_preset(Some(id), window, cx),
                        ))
                    }));
                rows.push(
                    SettingEntry::new(
                        "Chroma Colors",
                        "Choose a palette for the application chrome.",
                    )
                    .child(presets),
                );
                let inherited = inherited_chrome_colors(preset, cx.theme().mode);
                for (index, color) in ChromeColor::ALL.into_iter().enumerate() {
                    rows.push(
                        SettingEntry::new(color.title(), color.description()).control(
                            ColorPicker::new(
                                &self.settings_controls.colors[index],
                                color.read(&inherited),
                            )
                            .label(color.title())
                            .small(),
                        ),
                    );
                }
                rows.push(
                    SettingEntry::new(
                        "Widget corner radius",
                        "Round buttons, fields, and pane corners.",
                    )
                    .control(
                        div().w(px(120.0)).flex_none().child(
                            NumberInput::new(&self.settings_controls.radius)
                                .small()
                                .bg(settings_control_fill(cx)),
                        ),
                    ),
                );
                rows.extend([
                    unavailable(
                        "System fonts",
                        "Browsers use the bundled interface and terminal fonts.",
                    ),
                    unavailable(
                        "Window background blur",
                        "Native desktop window blur is unavailable in a browser.",
                    ),
                    unavailable(
                        "App icon",
                        "Changing the installed app icon requires the desktop app.",
                    ),
                    unavailable(
                        "System titlebar",
                        "Your browser manages its own window controls.",
                    ),
                ]);
            }
            SettingsSection::StatusBar => {
                rows.push(
                    SettingEntry::new(
                        "Sidebar",
                        "Show sessions, windows, and panes beside the workspace.",
                    )
                    .control(
                        Switch::new("web-sidebar-preference")
                            .checked(self.sidebar)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                    ),
                );
                rows.push(
                    SettingEntry::new(
                        "Window tabs",
                        "Click a window name in the top bar to select it.",
                    )
                    .control("Visible"),
                );
            }
            SettingsSection::Panes => {
                rows.push(
                    SettingEntry::new(
                        "Pane gaps",
                        "Separate panes with space and rounded corners.",
                    )
                    .control(
                        Switch::new("web-pane-gaps")
                            .checked(self.preferences.gaps)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preferences.gaps = !this.preferences.gaps;
                                this.preferences.save();
                                cx.notify();
                            })),
                    ),
                );
                rows.push(
                    SettingEntry::new(
                        "Terminal panes",
                        "Run shells and terminal applications on the daemon host.",
                    )
                    .control("Available"),
                );
                rows.push(
                    SettingEntry::new(
                        "Agent panes",
                        "Send prompts and answer permission requests through the daemon.",
                    )
                    .control("Available"),
                );
                rows.push(unavailable("Browser panes", "Embedded Chromium runs in the desktop app. Existing URLs can open in browser tabs."));
                rows.push(unavailable(
                    "Editor panes",
                    "The current wire protocol does not publish file contents to browser clients.",
                ));
            }
            SettingsSection::Multiplexer => {
                rows.push(
                    SettingEntry::new(
                        "Key bindings",
                        "Terminal prefix bindings and copy mode follow the daemon configuration.",
                    )
                    .control(
                        Button::new("web-show-keys")
                            .small()
                            .label("Show bindings")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings = None;
                                this.command("list-keys", Vec::new(), cx);
                                cx.notify();
                            })),
                    ),
                );
                rows.push(unavailable("Edit mux.conf", "Edit this file on the daemon host. Browser clients do not access its filesystem."));
                rows.push(
                    SettingEntry::new(
                        "Reload configuration",
                        "Read the daemon host's tmux and zz configuration files again.",
                    )
                    .control(
                        Button::new("web-reload-config")
                            .small()
                            .label("Reload")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.command("reload-config", Vec::new(), cx);
                            })),
                    ),
                );
            }
            SettingsSection::Terminal => {
                rows.push(SettingEntry::new("Appearance", "Terminal colors, font size, and cursor settings come from the connected daemon.").control("Shared"));
                rows.push(SettingEntry::new("Copy and paste", "Select terminal text to copy. Paste through your browser's clipboard shortcut.").control("Available"));
                rows.push(unavailable(
                    "Import Ghostty appearance",
                    "Importing a local Ghostty file requires the desktop app.",
                ));
                rows.push(unavailable(
                    "Local font discovery",
                    "Browsers cannot enumerate the daemon host's installed fonts.",
                ));
            }
            SettingsSection::Browser => rows.extend([
                SettingEntry::new(
                    "Search engine",
                    "Changing the desktop URL bar's search engine requires the desktop app.",
                )
                .disabled(true)
                .control(
                    div().w(px(120.0)).flex_none().child(
                        Select::new(&self.settings_controls.search_engine)
                            .small()
                            .placeholder("Unavailable")
                            .disabled(true)
                            .bg(settings_control_fill(cx)),
                    ),
                ),
                unavailable(
                    "Embedded browser",
                    "Chromium pane rendering requires the desktop app.",
                ),
                unavailable(
                    "Chrome profiles",
                    "Browser sandboxes prevent access to installed Chrome profiles.",
                ),
                unavailable(
                    "Import cookies and history",
                    "Reading another application's data requires the desktop app.",
                ),
                unavailable(
                    "Element picker",
                    "Inspect embedded Chromium pages with the desktop app.",
                ),
                unavailable(
                    "Developer tools",
                    "Use your browser's developer tools to inspect this client.",
                ),
            ]),
            SettingsSection::Editor => rows.extend([
                unavailable(
                    "Open and save files",
                    "The current daemon connection does not expose editor file operations.",
                ),
                unavailable("Vim mode", "Editor panes require the desktop app."),
            ]),
            SettingsSection::Hosts => {
                rows.push(
                    SettingEntry::new(
                        "Connection",
                        "This browser connects through the zz web gateway.",
                    )
                    .control(self.connection.read(cx).status.clone()),
                );
                rows.push(
                    SettingEntry::new(
                        "Reconnect",
                        "Establish a new connection to the configured daemon.",
                    )
                    .control(
                        Button::new("web-settings-reconnect")
                            .small()
                            .label("Reconnect")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.connection
                                    .update(cx, crate::connection::Connection::reconnect);
                            })),
                    ),
                );
                rows.push(unavailable(
                    "SSH keys and agent",
                    "Configure remote forwarding on the gateway host.",
                ));
                rows.push(unavailable(
                    "Add SSH host",
                    "Browsers cannot create native SSH connections.",
                ));
            }
            SettingsSection::Advanced => rows.extend([
                unavailable(
                    "Launch at login",
                    "Operating system startup settings require the desktop app.",
                ),
                unavailable(
                    "Global shortcuts",
                    "Browsers only receive keys while this page has focus.",
                ),
                unavailable(
                    "Install shell integration",
                    "Install shell integration from the daemon host.",
                ),
                unavailable(
                    "Restart daemon",
                    "Manage the persistent daemon from its host.",
                ),
            ]),
            SettingsSection::About => {
                rows.push(
                    SettingEntry::new(
                        "zz in the browser",
                        "The same GPUI widgets, connected to your zz daemon.",
                    )
                    .control(env!("CARGO_PKG_VERSION")),
                );
                rows.push(
                    SettingEntry::new("Source code", "zz is open source.").control(
                        Button::new("web-source")
                            .small()
                            .icon(IconName::ExternalLink)
                            .label("GitHub")
                            .on_click(|_, _, cx| cx.open_url("https://github.com/demfabris/zz")),
                    ),
                );
            }
        }
        div()
            .size_full()
            .bg(cx.theme().background)
            .child(
                settings_scroll_column("web-settings-page")
                    .child(settings_page_description(section, cx))
                    .child(SettingsStack::new().children(rows))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(cx.theme().foreground.muted())
                            .child("Interface preferences are saved in this browser."),
                    ),
            )
            .into_any_element()
    }
}

fn unavailable(title: &str, reason: &str) -> SettingEntry {
    SettingEntry::new(title.to_owned(), reason.to_owned())
        .disabled(true)
        .control(
            Button::new(format!("web-unavailable-{title}"))
                .small()
                .label("Unavailable")
                .disabled(true),
        )
}
