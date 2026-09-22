use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Context, Entity, IntoElement, Subscription, Window, canvas, div, prelude::*,
    px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, Sizable as _, StyledExt as _, Theme, ThemeMode,
    UiZoom,
    button::{Button, ButtonVariants as _},
    chrome_palette::{
        ChromeColor, ChromePresetId, ThemeModeSetting, chrome_presets, inherited_chrome_colors,
        resolved_chrome_colors,
    },
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{InputEvent, InputState, NumberInput},
    menu::{DropdownMenu as _, PopupMenuItem},
    select::{Select, SelectEvent, SelectState},
    settings::{
        SettingEntry, SettingsSection, SettingsSelectItem, SettingsStack, StackPosition,
        appearance::{
            AppearancePageItem, PickerStrip, appearance_page, appearance_page_items,
            palette_preview, picker_tile, theme_preview, ui_font_select,
        },
        settings_control_fill, settings_reset_button, settings_scroll_column,
    },
    switch::Switch,
};

use super::AppShell;
use crate::{
    connection::Connection,
    terminal::{TerminalDisplayPreferences, localized_font_appearance},
};

pub(super) use crate::preferences::Preferences;

pub(super) const SECTIONS: [SettingsSection; 7] = [
    SettingsSection::Appearance,
    SettingsSection::StatusBar,
    SettingsSection::Panes,
    SettingsSection::Terminal,
    SettingsSection::Hosts,
    SettingsSection::Advanced,
    SettingsSection::About,
];

struct PlatformReduceMotion(bool);

impl gpui::Global for PlatformReduceMotion {}

impl Preferences {
    pub(super) fn apply(&self, connection: &Entity<Connection>, window: &mut Window, cx: &mut App) {
        if !cx.has_global::<PlatformReduceMotion>() {
            cx.set_global(PlatformReduceMotion(cx.reduce_motion()));
        }
        cx.set_reduce_motion(cx.global::<PlatformReduceMotion>().0 || !self.animations);
        let pinned = zz_ui::chrome_palette::pinned_theme_mode(self.theme_mode());
        #[cfg(target_os = "ios")]
        {
            cx.set_window_appearance(pinned.map(|mode| {
                if mode.is_dark() {
                    gpui::WindowAppearance::Dark
                } else {
                    gpui::WindowAppearance::Light
                }
            }));
            let mode = pinned.unwrap_or_else(|| match cx.window_appearance() {
                gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => {
                    ThemeMode::Light
                }
                _ => ThemeMode::Dark,
            });
            Theme::change(mode, Some(window), cx);
        }
        #[cfg(not(target_os = "ios"))]
        if let Some(mode) = pinned {
            Theme::change(mode, Some(window), cx);
        } else {
            Theme::sync_system_appearance(Some(window), cx);
        }
        let mode = cx.theme().mode;
        Theme::global_mut(cx).colors = resolved_chrome_colors(
            self.preset(mode),
            mode,
            self.colors
                .clone()
                .map(|color| color.and_then(|value| zz_ui::parse_hex(&value).ok())),
        );
        Theme::global_mut(cx).radius = px(self.radius);
        Theme::global_mut(cx).set_contrast(self.contrast);
        Theme::global_mut(cx).shadow_strength = self.shadow_strength;
        Theme::global_mut(cx).pane_background_opacity = self.pane_background_opacity;
        Theme::global_mut(cx).pane_glow_strength = self.pane_glow_strength;
        let available_fonts = cx.text_system().all_font_names();
        let ui_font = if available_fonts
            .iter()
            .any(|family| family.eq_ignore_ascii_case(&self.ui_font_family))
        {
            self.ui_font_family.clone()
        } else if cfg!(any(target_family = "wasm", target_os = "ios")) {
            "Inter Variable".to_owned()
        } else {
            ".SystemUIFont".to_owned()
        };
        Theme::global_mut(cx).font_family = ui_font.into();
        cx.set_global(TerminalDisplayPreferences {
            font_family: self.terminal_font_family.clone(),
            font_scale: self.terminal_font_scale,
        });
        let terminal =
            localized_font_appearance(&connection.read(cx).core, &available_fonts, "Lilex", cx);
        Theme::global_mut(cx).mono_font_family = terminal.font_families[0].clone().into();
        cx.set_global(UiZoom(self.zoom));
        window.set_zoom(self.zoom);
        connection.update(cx, Connection::set_color_scheme);
        cx.refresh_windows();
    }

    pub(super) fn change_zoom(
        &mut self,
        step: f32,
        connection: &Entity<Connection>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.zoom = ((self.zoom + step) * 10.0).round().clamp(5.0, 30.0) / 10.0;
        self.save();
        self.apply(connection, window, cx);
    }

    pub(super) fn reset_zoom(
        &mut self,
        connection: &Entity<Connection>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.zoom = 1.0;
        self.save();
        self.apply(connection, window, cx);
    }
}

pub(super) struct Controls {
    zoom: Entity<InputState>,
    radius: Entity<InputState>,
    contrast: Entity<InputState>,
    shadow_strength: Entity<InputState>,
    panes: [Entity<InputState>; 6],
    colors: Vec<Entity<ColorPickerState>>,
    ui_font: Entity<SelectState<Vec<SettingsSelectItem>>>,
    terminal_font: Entity<SelectState<Vec<SettingsSelectItem>>>,
    terminal_scale: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Controls {
    pub(super) fn new(
        preferences: &Preferences,
        window: &mut Window,
        cx: &mut Context<AppShell>,
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
                .max(25.0)
        });
        let contrast = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value((preferences.contrast * 100.0).to_string())
                .step(5.0)
                .min(50.0)
                .max(200.0)
        });
        let shadow_strength = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value((preferences.shadow_strength * 100.0).to_string())
                .step(5.0)
                .min(0.0)
                .max(100.0)
        });
        let terminal_scale = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value((preferences.terminal_font_scale * 100.0).to_string())
                .step(10.0)
                .min(50.0)
                .max(300.0)
        });
        let mut subscriptions = Vec::new();
        for (input, key) in [
            (&zoom, "zoom"),
            (&radius, "radius"),
            (&contrast, "contrast"),
            (&shadow_strength, "shadow-strength"),
            (&terminal_scale, "terminal-scale"),
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, input, event, window, cx| {
                    let commit = matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. });
                    if !commit && !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let (min, max, previous) = match key {
                        "zoom" => (50.0, 300.0, this.preferences.zoom * 100.0),
                        "contrast" => (50.0, 200.0, this.preferences.contrast * 100.0),
                        "shadow-strength" => (0.0, 100.0, this.preferences.shadow_strength * 100.0),
                        "terminal-scale" => {
                            (50.0, 300.0, this.preferences.terminal_font_scale * 100.0)
                        }
                        _ => (0.0, 25.0, this.preferences.radius),
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
                    match key {
                        "zoom" => this.preferences.zoom = value / 100.0,
                        "contrast" => this.preferences.contrast = value / 100.0,
                        "shadow-strength" => this.preferences.shadow_strength = value / 100.0,
                        "terminal-scale" => this.preferences.terminal_font_scale = value / 100.0,
                        _ => this.preferences.radius = value,
                    }
                    if commit {
                        input.update(cx, |input, cx| {
                            input.set_value(format!("{value:.0}"), window, cx);
                        });
                    }
                    this.preferences.save();
                    this.preferences.apply(&this.connection, window, cx);
                },
            ));
        }
        let pane_values = [
            preferences.pane_background_opacity,
            preferences.pane_inactive_opacity,
            preferences.pane_glow_strength,
            preferences.pane_margin,
            preferences.pane_radius,
            preferences.pane_border_width,
        ];
        let panes = PaneControl::ALL.map(|control| {
            let (min, max, step) = control.limits();
            let scale = if matches!(control, PaneControl::BackgroundOpacity | PaneControl::Glow) {
                100.0
            } else {
                1.0
            };
            let (min, max, step) = (min * scale, max * scale, step * f64::from(scale));
            let value = pane_values[control as usize] * scale;
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value.to_string())
                    .min(f64::from(min))
                    .max(f64::from(max))
                    .step(step)
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, window, cx| {
                    let commit = matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. });
                    if !commit && !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let parsed = input
                        .read(cx)
                        .value()
                        .parse::<f32>()
                        .ok()
                        .filter(|value| value.is_finite());
                    let previous = *control.value(&mut this.preferences) * scale;
                    let value = match parsed {
                        Some(value) if commit => value.clamp(min, max),
                        Some(value) if (min..=max).contains(&value) => value,
                        _ if commit => previous,
                        _ => return,
                    };
                    *control.value(&mut this.preferences) = value / scale;
                    if commit {
                        input.update(cx, |input, cx| {
                            input.set_value(value.to_string(), window, cx);
                        });
                    }
                    this.preferences.save();
                    Theme::global_mut(cx).pane_background_opacity =
                        this.preferences.pane_background_opacity;
                    Theme::global_mut(cx).pane_glow_strength = this.preferences.pane_glow_strength;
                    cx.refresh_windows();
                    cx.notify();
                },
            ));
            input
        });
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
                        this.preferences.apply(&this.connection, window, cx);
                    },
                ));
                state
            })
            .collect();
        let ui_font = ui_font_select(Some(&preferences.ui_font_family), window, cx);
        let mut families = cx.text_system().all_font_names();
        families.retain(|family| !family.starts_with('.') && !family.trim().is_empty());
        families.sort_by_cached_key(|family| family.to_lowercase());
        families.dedup();
        let items = std::iter::once(SettingsSelectItem::new("Follow host", ""))
            .chain(
                families
                    .into_iter()
                    .map(|family| SettingsSelectItem::new(family.clone(), family)),
            )
            .collect::<Vec<_>>();
        let terminal_font = cx.new(|cx| SelectState::new(items, None, window, cx));
        terminal_font.update(cx, |select, cx| {
            select.set_selected_value(
                &preferences.terminal_font_family.clone().unwrap_or_default(),
                window,
                cx,
            );
        });
        for (select, terminal) in [(&ui_font, false), (&terminal_font, true)] {
            subscriptions.push(cx.subscribe_in(
                select,
                window,
                move |this, _, event: &SelectEvent<Vec<SettingsSelectItem>>, window, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    if terminal {
                        this.preferences.terminal_font_family =
                            (!value.is_empty()).then(|| value.clone());
                    } else {
                        this.preferences.ui_font_family.clone_from(value);
                    }
                    this.preferences.save();
                    this.preferences.apply(&this.connection, window, cx);
                },
            ));
        }
        Self {
            zoom,
            radius,
            contrast,
            shadow_strength,
            panes,
            colors,
            ui_font,
            terminal_font,
            terminal_scale,
            _subscriptions: subscriptions,
        }
    }
}

impl AppShell {
    pub(super) fn sync_zoom_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_controls.zoom.update(cx, |input, cx| {
            input.set_value(format!("{:.0}", self.preferences.zoom * 100.0), window, cx);
        });
    }
    fn select_preset(
        &mut self,
        mode: ThemeMode,
        preset: Option<ChromePresetId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = preset.map(|id| id.as_str().to_owned());
        if mode.is_dark() {
            self.preferences.preset_dark = value;
        } else {
            self.preferences.preset_light = value;
        }
        self.preferences.colors = Default::default();
        for color in &self.settings_controls.colors {
            color.update(cx, |color, cx| color.set_color(None, window, cx));
        }
        self.preferences.save();
        self.preferences.apply(&self.connection, window, cx);
    }

    pub(super) fn render_settings(
        &self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let narrow = AppShell::narrow(window);
        let section = if SECTIONS.contains(&section) {
            section
        } else {
            SettingsSection::Appearance
        };
        let content = self.settings_page(section, narrow, cx);
        let view = cx.entity().downgrade();
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .when(narrow, |page| {
                page.child(
                    div()
                        .flex()
                        .items_center()
                        .h(zz_ui::TITLE_BAR_HEIGHT)
                        .flex_none()
                        .px(px(8.0))
                        .gap(px(4.0))
                        .child(
                            Button::compact_icon("settings-close", IconName::ArrowLeft)
                                .tooltip("Back to workspace")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.settings = None;
                                    this.focused_pane = None;
                                    this.focus.focus(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("settings-section")
                                .ghost()
                                .small()
                                .label(section.title())
                                .icon(IconName::ChevronDown)
                                .dropdown_menu(move |menu, _, _| {
                                    SECTIONS.into_iter().fold(menu, |menu, choice| {
                                        let view = view.clone();
                                        menu.item(
                                            PopupMenuItem::new(choice.title())
                                                .icon(choice.icon())
                                                .on_click(move |_, window, cx| {
                                                    let _ = view.update(cx, |this, cx| {
                                                        this.settings = Some(choice);
                                                        this.focus.focus(window, cx);
                                                        cx.notify();
                                                    });
                                                }),
                                        )
                                    })
                                }),
                        ),
                )
            })
            .child(content)
            .into_any_element()
    }

    fn settings_page(
        &self,
        section: SettingsSection,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match section {
            SettingsSection::Appearance => {
                let items = appearance_page_items(ChromeColor::ALL, false, false)
                    .into_iter()
                    .filter(|item| {
                        matches!(
                            item,
                            AppearancePageItem::Description
                                | AppearancePageItem::Group { .. }
                                | AppearancePageItem::ThemeMode
                                | AppearancePageItem::UiFontFamily
                                | AppearancePageItem::UiZoom
                                | AppearancePageItem::Preset(_)
                                | AppearancePageItem::ChromeColor(_)
                                | AppearancePageItem::ChromeContrast
                                | AppearancePageItem::Animations
                                | AppearancePageItem::WidgetCornerRadius
                                | AppearancePageItem::ShadowStrength
                        )
                    })
                    .collect();
                let view = cx.entity();
                return appearance_page(items, move |item, position, _, cx| {
                    view.update(cx, |this, cx| {
                        this.appearance_item(item, position, narrow, cx)
                    })
                })
                .into_any_element();
            }
            SettingsSection::Panes => {
                let [background, inactive, glow, margin, radius, border] =
                    PaneControl::ALL.map(|control| self.pane_setting(control, narrow, cx));
                let gaps = with_control(
                    SettingEntry::new("Pane gaps", "Separate panes with spacing and borders.")
                        .title_actions(reset_button(
                            "settings-pane-gaps-reset",
                            self.preferences.gaps != Preferences::default().gaps,
                            |this, _, _| this.preferences.gaps = Preferences::default().gaps,
                            cx,
                        )),
                    Switch::new("settings-pane-gaps")
                        .checked(self.preferences.gaps)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.gaps = !this.preferences.gaps;
                            this.preferences.save();
                            cx.notify();
                        })),
                    narrow,
                );
                let agent = with_control(
                    SettingEntry::new("Agent panes", "Allow creating agent panes on this client."),
                    Switch::new("settings-agent-enabled")
                        .checked(self.preferences.agent_enabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.agent_enabled = !this.preferences.agent_enabled;
                            this.preferences.save();
                            cx.notify();
                        })),
                    narrow,
                );
                return zz_ui::settings::panes_page(
                    zz_ui::settings::panes_preview::PanesPreview {
                        gaps: self.preferences.gaps,
                        margin: self.preferences.pane_margin,
                        radius: self.preferences.pane_radius,
                        border_width: self.preferences.pane_border_width,
                        inactive_opacity: self.preferences.pane_inactive_opacity,
                    },
                    [gaps, agent],
                    background,
                    [inactive, glow],
                    [margin, radius, border],
                    cx,
                )
                .into_any_element();
            }
            SettingsSection::StatusBar => {
                let mut rows = vec![with_control(
                    SettingEntry::new(
                        "Sidebar",
                        "Show sessions, windows, and panes beside the workspace.",
                    ),
                    Switch::new("settings-sidebar")
                        .checked(self.sidebar)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar = !this.sidebar;
                            this.preferences.sidebar = this.sidebar;
                            this.preferences.save();
                            cx.notify();
                        })),
                    narrow,
                )];
                for (id, title, description, checked) in [
                    (
                        "session",
                        "Session",
                        "Show the session menu in the titlebar.",
                        self.preferences.status_show_session,
                    ),
                    (
                        "badges",
                        "Window badges",
                        "Show bell and activity markers on window items.",
                        self.preferences.status_badges,
                    ),
                    (
                        "agents",
                        "Agent activity",
                        "Show agent activity in the titlebar.",
                        self.preferences.status_agents,
                    ),
                ] {
                    let default = *status_field(&mut Preferences::default(), id);
                    rows.push(with_control(
                        SettingEntry::new(title, description).title_actions(reset_button(
                            format!("settings-status-{id}-reset"),
                            checked != default,
                            move |this, _, _| *status_field(&mut this.preferences, id) = default,
                            cx,
                        )),
                        Switch::new(format!("settings-status-{id}"))
                            .checked(checked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let value = status_field(&mut this.preferences, id);
                                *value = !*value;
                                this.preferences.save();
                                cx.notify();
                            })),
                        narrow,
                    ));
                }
                return zz_ui::settings::status_bar_preview::status_bar_page(
                    self.preferences.status_bar_settings(),
                    self.preferences.gaps,
                    SettingsStack::new().children(rows),
                    cx,
                )
                .into_any_element();
            }
            SettingsSection::Terminal => return self.terminal_settings(narrow, cx),
            SettingsSection::Advanced => {
                return settings_scroll_column("settings-page")
                    .child(settings_heading(
                        section.title(),
                        "Tune the command palette and display on this client.",
                        cx,
                    ))
                    .child(
                        SettingsStack::titled("Command palette")
                            .children(self.palette_settings(narrow, cx)),
                    )
                    .when(cfg!(target_os = "ios"), |page| {
                        page.child(
                            SettingsStack::titled("Display").child(with_control(
                                SettingEntry::new(
                                    "Draw under the home indicator",
                                    "Extend the workspace and settings into the bottom safe area.",
                                )
                                .title_actions(reset_button(
                                    "settings-bottom-safe-area-reset",
                                    self.preferences.extend_bottom_safe_area
                                        != Preferences::default().extend_bottom_safe_area,
                                    |this, _, _| {
                                        this.preferences.extend_bottom_safe_area =
                                            Preferences::default().extend_bottom_safe_area;
                                    },
                                    cx,
                                )),
                                Switch::new("settings-bottom-safe-area")
                                    .checked(self.preferences.extend_bottom_safe_area)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.preferences.extend_bottom_safe_area =
                                            !this.preferences.extend_bottom_safe_area;
                                        this.preferences.save();
                                        cx.notify();
                                    })),
                                narrow,
                            )),
                        )
                    })
                    .into_any_element();
            }
            SettingsSection::About => return about_page(cx),
            _ => {}
        }
        let rows = match section {
            SettingsSection::Hosts => vec![
                with_control(
                    SettingEntry::new(
                        "Connection",
                        if cfg!(target_os = "ios") {
                            "Connect to the daemon using the endpoint configured for this app."
                        } else {
                            "Connect to the daemon through the zz web gateway."
                        },
                    ),
                    self.connection.read(cx).status.clone(),
                    narrow,
                ),
                #[cfg(target_os = "ios")]
                with_control(
                    SettingEntry::new(
                        "SSH key",
                        "Add this key to ~/.ssh/authorized_keys on the host to sign in without a password.",
                    ),
                    Button::new("settings-copy-ssh-key")
                        .small()
                        .label("Copy")
                        .on_click(|_, window, cx| match zz_daemon::ios_ssh_public_key() {
                            Ok(key) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(key)),
                            Err(error) => {
                                use zz_ui::WindowExt as _;
                                window.push_notification(
                                    zz_ui::notification::Notification::error(error.to_string()),
                                    cx,
                                );
                            }
                        }),
                    narrow,
                ),
                with_control(
                    SettingEntry::new(
                        "Reconnect",
                        "Establish a new connection to the configured daemon.",
                    ),
                    Button::new("settings-reconnect")
                        .small()
                        .label("Reconnect")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.connection.update(cx, Connection::reconnect);
                        })),
                    narrow,
                ),
            ],
            _ => Vec::new(),
        };
        settings_scroll_column("settings-page")
            .child(settings_heading(
                section.title(),
                if section == SettingsSection::Hosts {
                    "Connect this client to your zz daemon."
                } else {
                    section.description()
                },
                cx,
            ))
            .child(SettingsStack::new().children(rows))
            .into_any_element()
    }

    fn appearance_item(
        &self,
        item: AppearancePageItem<ChromeColor>,
        position: StackPosition,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = match item {
            AppearancePageItem::ThemeMode => {
                let light = inherited_chrome_colors(
                    self.preferences.preset(ThemeMode::Light),
                    ThemeMode::Light,
                );
                let dark = inherited_chrome_colors(
                    self.preferences.preset(ThemeMode::Dark),
                    ThemeMode::Dark,
                );
                let tiles =
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(8.0))
                        .children(ThemeModeSetting::ALL.map(|mode| {
                            picker_tile(
                                format!("settings-theme-{}", mode.as_str()).into(),
                                mode.title(),
                                theme_preview(
                                    zz_ui::chrome_palette::pinned_theme_mode(mode),
                                    &light,
                                    &dark,
                                    cx,
                                ),
                                self.preferences.theme_mode() == mode,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.preferences.mode = Some(mode.as_str().into());
                                    this.preferences.save();
                                    this.preferences.apply(&this.connection, window, cx);
                                },
                            ))
                        }));
                with_control(
                    SettingEntry::new("Theme", "Follow the system light/dark setting, or pin one.")
                        .title_actions(reset_button(
                            "settings-theme-reset",
                            self.preferences.theme_mode() != Preferences::default().theme_mode(),
                            |this, _, _| this.preferences.mode = Preferences::default().mode,
                            cx,
                        )),
                    tiles,
                    narrow,
                )
            }
            AppearancePageItem::UiFontFamily => with_control(
                SettingEntry::new("UI font", "Choose an available font for the interface.")
                    .title_actions(reset_button(
                        "settings-ui-font-reset",
                        self.preferences.ui_font_family != Preferences::default().ui_font_family,
                        |this, window, cx| {
                            let family = Preferences::default().ui_font_family;
                            this.settings_controls.ui_font.update(cx, |select, cx| {
                                select.set_selected_value(&family, window, cx);
                            });
                            this.preferences.ui_font_family = family;
                        },
                        cx,
                    )),
                div().w(px(200.0)).max_w_full().flex_none().child(
                    Select::new(&self.settings_controls.ui_font)
                        .small()
                        .placeholder("System default")
                        .bg(settings_control_fill(cx)),
                ),
                narrow,
            ),
            AppearancePageItem::UiZoom => with_control(
                SettingEntry::new(
                    "UI zoom",
                    "Scales application text, icons, and controls as a percentage of the default.",
                )
                .title_actions(
                    settings_reset_button(
                        "settings-zoom-reset",
                        "Reset UI zoom to 100%",
                        self.preferences.zoom != 1.0,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preferences.reset_zoom(&this.connection, window, cx);
                        this.sync_zoom_input(window, cx);
                    })),
                ),
                number_control(&self.settings_controls.zoom, cx),
                narrow,
            ),
            AppearancePageItem::Preset(mode) => {
                let dark = mode.is_dark();
                let selected = self.preferences.preset(mode);
                let (title, description, strip) = if dark {
                    (
                        "Dark palette",
                        "Used while the interface is dark.",
                        "settings-presets-dark",
                    )
                } else {
                    (
                        "Light palette",
                        "Used while the interface is light.",
                        "settings-presets-light",
                    )
                };
                let presets = std::iter::once(None)
                    .chain(chrome_presets(dark).map(|preset| Some(preset.id)))
                    .collect::<Vec<_>>();
                let selected = presets
                    .iter()
                    .position(|preset| *preset == selected)
                    .unwrap_or(0);
                let view = cx.entity().downgrade();
                let tiles = PickerStrip::new(strip, selected)
                    .tiles(presets.iter().map(|preset| {
                        (
                            preset.map_or("Default", |id| id.preset().name),
                            palette_preview(&inherited_chrome_colors(*preset, mode), cx),
                        )
                    }))
                    .on_select(move |index, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.select_preset(mode, presets[index], window, cx);
                        });
                    });
                SettingEntry::new(title, description)
                    .title_actions(reset_button(
                        format!("{strip}-reset"),
                        self.preferences.preset(mode) != Preferences::default().preset(mode),
                        move |this, _, _| {
                            let defaults = Preferences::default();
                            if dark {
                                this.preferences.preset_dark = defaults.preset_dark;
                            } else {
                                this.preferences.preset_light = defaults.preset_light;
                            }
                        },
                        cx,
                    ))
                    .child(tiles)
            }
            AppearancePageItem::ChromeColor(color) => {
                let index = ChromeColor::ALL
                    .iter()
                    .position(|candidate| *candidate == color)
                    .unwrap();
                let mode = cx.theme().mode;
                let inherited = inherited_chrome_colors(self.preferences.preset(mode), mode);
                with_control(
                    SettingEntry::new(color.title(), color.description()).title_actions(
                        reset_button(
                            format!("settings-{}-reset", color.as_str()),
                            self.preferences.colors[index].is_some(),
                            move |this, window, cx| {
                                this.preferences.colors[index] = None;
                                this.settings_controls.colors[index].update(cx, |picker, cx| {
                                    picker.set_color(None, window, cx);
                                });
                            },
                            cx,
                        ),
                    ),
                    ColorPicker::new(
                        &self.settings_controls.colors[index],
                        zz_ui::chrome_palette::read_chrome_color(color, &inherited),
                    )
                    .label(color.title())
                    .small(),
                    narrow,
                )
            }
            AppearancePageItem::ChromeContrast => with_control(
                SettingEntry::new(
                    "Contrast",
                    "Adjust surface, text, and edge contrast from 50% to 200%.",
                )
                .title_actions(
                    settings_reset_button(
                        "settings-contrast-reset",
                        "Reset contrast to 100%",
                        self.preferences.contrast != 1.0,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preferences.contrast = 1.0;
                        this.settings_controls
                            .contrast
                            .update(cx, |input, cx| input.set_value("100", window, cx));
                        this.preferences.save();
                        this.preferences.apply(&this.connection, window, cx);
                    })),
                ),
                number_control(&self.settings_controls.contrast, cx),
                narrow,
            ),
            AppearancePageItem::Animations => with_control(
                SettingEntry::new(
                    "Animations",
                    "Animate interface transitions, loading indicators, and image frames.",
                )
                .title_actions(reset_button(
                    "settings-animations-reset",
                    self.preferences.animations != Preferences::default().animations,
                    |this, _, _| this.preferences.animations = Preferences::default().animations,
                    cx,
                )),
                Switch::new("settings-animations")
                    .checked(self.preferences.animations)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preferences.animations = !this.preferences.animations;
                        this.preferences.save();
                        this.preferences.apply(&this.connection, window, cx);
                    })),
                narrow,
            ),
            AppearancePageItem::WidgetCornerRadius => with_control(
                SettingEntry::new(
                    if self.preferences.radius > 24.0 {
                        "Widget corner radius (Full)"
                    } else {
                        "Widget corner radius"
                    },
                    "Set corners from 0 to 24px. At 25, controls become pills.",
                )
                .title_actions(reset_button(
                    "settings-radius-reset",
                    self.preferences.radius != Preferences::default().radius,
                    |this, window, cx| {
                        this.preferences.radius = Preferences::default().radius;
                        let value = format!("{:.0}", this.preferences.radius);
                        this.settings_controls
                            .radius
                            .update(cx, |input, cx| input.set_value(value, window, cx));
                    },
                    cx,
                )),
                number_control(&self.settings_controls.radius, cx),
                narrow,
            ),
            AppearancePageItem::ShadowStrength => with_control(
                SettingEntry::new(
                    "Shadow strength",
                    "Strength of shadows around controls and gapped panes, from 0% (off) to 100%.",
                )
                .title_actions(
                    settings_reset_button(
                        "settings-shadow-reset",
                        "Reset shadow strength to 100%",
                        self.preferences.shadow_strength != 1.0,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preferences.shadow_strength = 1.0;
                        this.settings_controls
                            .shadow_strength
                            .update(cx, |input, cx| input.set_value("100", window, cx));
                        this.preferences.save();
                        this.preferences.apply(&this.connection, window, cx);
                    })),
                ),
                number_control(&self.settings_controls.shadow_strength, cx),
                narrow,
            ),
            _ => return div().into_any_element(),
        };
        entry.position(position).into_any_element()
    }

    fn pane_setting(
        &self,
        control: PaneControl,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> SettingEntry {
        let value = *control.value(&mut self.preferences.clone());
        let default = *control.value(&mut Preferences::default());
        let entry = SettingEntry::new(control.title(), control.description())
            .disabled(
                !matches!(
                    control,
                    PaneControl::BackgroundOpacity | PaneControl::Opacity | PaneControl::Glow
                ) && !self.preferences.gaps,
            )
            .title_actions(
                settings_reset_button(
                    format!("settings-pane-reset-{}", control as usize),
                    "Reset to default",
                    value != default,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    *control.value(&mut this.preferences) = default;
                    let scale =
                        if matches!(control, PaneControl::BackgroundOpacity | PaneControl::Glow) {
                            100.0
                        } else {
                            1.0
                        };
                    this.settings_controls.panes[control as usize].update(cx, |input, cx| {
                        input.set_value((default * scale).to_string(), window, cx);
                    });
                    this.preferences.save();
                    this.preferences.apply(&this.connection, window, cx);
                })),
            );
        with_control(
            entry,
            number_control(&self.settings_controls.panes[control as usize], cx),
            narrow,
        )
    }

    fn terminal_settings(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let available = cx.text_system().all_font_names();
        let appearance =
            localized_font_appearance(&self.connection.read(cx).core, &available, "Lilex", cx);
        let font = with_control(
            SettingEntry::new(
                "Font",
                "Use an available font on this client, or follow the host's configured font.",
            ),
            div().w(px(200.0)).max_w_full().flex_none().child(
                Select::new(&self.settings_controls.terminal_font)
                    .small()
                    .placeholder("Follow host")
                    .bg(settings_control_fill(cx)),
            ),
            narrow,
        );
        let scale = with_control(
            SettingEntry::new(
                "Font scale",
                "Scale terminal text on this client from 50% to 300%.",
            )
            .title_actions(
                settings_reset_button(
                    "settings-terminal-scale-reset",
                    "Reset font scale to 100%",
                    self.preferences.terminal_font_scale != 1.0,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.preferences.terminal_font_scale = 1.0;
                    this.settings_controls
                        .terminal_scale
                        .update(cx, |input, cx| input.set_value("100", window, cx));
                    this.preferences.save();
                    this.preferences.apply(&this.connection, window, cx);
                })),
            ),
            number_control(&self.settings_controls.terminal_scale, cx),
            narrow,
        );
        settings_scroll_column("settings-terminal")
            .child(settings_heading("Terminal", "Adjust terminal text on this client. Colors, cursor, and spacing follow the host configuration.", cx))
            .child(zz_ui::settings::settings_list_group_header("Preview", None, cx))
            .child(terminal_preview(appearance, cx))
            .child(SettingsStack::titled("Display").child(font).child(scale))
            .child(SettingsStack::titled("Host configuration")
                .child(SettingEntry::new("Colors, cursor, and spacing", "Edit the Ghostty-compatible configuration on the daemon host to change these values.").control("Shared")))
            .into_any_element()
    }
}

impl AppShell {
    fn palette_settings(&self, narrow: bool, cx: &Context<Self>) -> Vec<SettingEntry> {
        let defaults = Preferences::default();
        let choice = |id: &'static str,
                      label: &'static str,
                      choices: &'static [(&'static str, &'static str)],
                      current: String,
                      apply: fn(&mut Preferences, &str)| {
            let view = cx.entity().downgrade();
            Button::new(id)
                .small()
                .label(
                    choices
                        .iter()
                        .find(|(value, _)| *value == current)
                        .map_or(label, |(_, label)| *label),
                )
                .dropdown_caret(true)
                .bg(settings_control_fill(cx))
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                    choices.iter().fold(menu, |menu, &(value, label)| {
                        let view = view.clone();
                        menu.item(
                            PopupMenuItem::new(label)
                                .checked(value == current)
                                .on_click(move |_, _, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        apply(&mut this.preferences, value);
                                        this.preferences.save();
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                })
        };
        let layout = if self.preferences.palette_grouped {
            "grouped"
        } else {
            "flat"
        };
        vec![
            with_control(
                SettingEntry::new(
                    "Navigation layout",
                    "Show sessions, windows, and panes as a tree or a flat list.",
                )
                .title_actions(reset_button(
                    "settings-palette-layout-reset",
                    self.preferences.palette_grouped != defaults.palette_grouped,
                    |this, _, _| {
                        this.preferences.palette_grouped = Preferences::default().palette_grouped;
                    },
                    cx,
                )),
                choice(
                    "settings-palette-layout",
                    "Tree",
                    &[("grouped", "Tree"), ("flat", "Flat")],
                    layout.to_owned(),
                    |preferences, value| preferences.palette_grouped = value == "grouped",
                ),
                narrow,
            ),
            with_control(
                SettingEntry::new(
                    "Host prefix",
                    "Type this character in an empty palette to browse hosts.",
                )
                .title_actions(reset_button(
                    "settings-palette-host-prefix-reset",
                    self.preferences.palette_host_prefix != defaults.palette_host_prefix,
                    |this, _, _| {
                        this.preferences.palette_host_prefix =
                            Preferences::default().palette_host_prefix;
                    },
                    cx,
                )),
                choice(
                    "settings-palette-host-prefix",
                    "~",
                    &[("~", "~"), ("#", "#")],
                    self.preferences.palette_host_prefix.clone(),
                    |preferences, value| value.clone_into(&mut preferences.palette_host_prefix),
                ),
                narrow,
            ),
            with_control(
                SettingEntry::new(
                    "Command shortcuts",
                    "Show keyboard shortcuts beside commands.",
                )
                .title_actions(reset_button(
                    "settings-palette-show-keys-reset",
                    self.preferences.palette_show_keys != defaults.palette_show_keys,
                    |this, _, _| {
                        this.preferences.palette_show_keys =
                            Preferences::default().palette_show_keys;
                    },
                    cx,
                )),
                Switch::new("settings-palette-show-keys")
                    .checked(self.preferences.palette_show_keys)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.preferences.palette_show_keys = !this.preferences.palette_show_keys;
                        this.preferences.save();
                        cx.notify();
                    })),
                narrow,
            ),
        ]
    }
}

fn about_page(cx: &mut Context<AppShell>) -> AnyElement {
    use zz_ui::settings::about;

    static LOGOS: std::sync::LazyLock<[Arc<gpui::Image>; 2]> = std::sync::LazyLock::new(|| {
        let [light, dark]: [&'static [u8]; 2] = if zz_protocol::app_identity::DEVELOPMENT {
            [
                include_bytes!("../../../assets/zz-dev-light-512.png"),
                include_bytes!("../../../assets/zz-dev-dark-512.png"),
            ]
        } else {
            [
                include_bytes!("../../../assets/zz-light-512.png"),
                include_bytes!("../../../assets/zz-dark-512.png"),
            ]
        };
        [light, dark].map(|bytes| {
            Arc::new(gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                bytes.to_vec(),
            ))
        })
    });
    let platform = if cfg!(target_family = "wasm") {
        "browser · wasm32".to_owned()
    } else {
        format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH)
    };
    let copied = platform.clone();
    let logo = Arc::clone(&LOGOS[usize::from(cx.theme().mode.is_dark())]);
    settings_scroll_column("settings-about")
        .child(about::about_hero(
            gpui::img(logo).size(px(about::ABOUT_LOGO_SIZE)),
            cx,
        ))
        .child(about::about_build_stack(
            platform,
            about::about_copy_button("settings-about-copy-build-info").on_click(
                move |_, window, cx| {
                    use zz_ui::WindowExt as _;
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(about::build_info(
                        &copied,
                    )));
                    window.push_notification(
                        zz_ui::notification::Notification::success("Copied build information"),
                        cx,
                    );
                },
            ),
            cx,
        ))
        .child(about::about_project_stack(cx))
        .into_any_element()
}

fn with_control(entry: SettingEntry, control: impl IntoElement, narrow: bool) -> SettingEntry {
    if narrow {
        entry.child(control)
    } else {
        entry.control(control)
    }
}

fn reset_button(
    id: impl Into<gpui::ElementId>,
    changed: bool,
    reset: impl Fn(&mut AppShell, &mut Window, &mut Context<AppShell>) + 'static,
    cx: &Context<AppShell>,
) -> Button {
    settings_reset_button(id, "Reset to default", changed).on_click(cx.listener(
        move |this, _, window, cx| {
            reset(this, window, cx);
            this.preferences.save();
            this.preferences.apply(&this.connection, window, cx);
        },
    ))
}

fn status_field<'a>(preferences: &'a mut Preferences, id: &str) -> &'a mut bool {
    match id {
        "session" => &mut preferences.status_show_session,
        "badges" => &mut preferences.status_badges,
        _ => &mut preferences.status_agents,
    }
}

fn number_control(input: &Entity<InputState>, cx: &App) -> gpui::Div {
    div().w(px(120.0)).max_w_full().flex_none().child(
        NumberInput::new(input)
            .small()
            .bg(settings_control_fill(cx)),
    )
}

fn settings_heading(title: &'static str, description: &'static str, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .font_medium()
                .text_size(zz_ui::rems_from_px(20.0))
                .child(title),
        )
        .child(
            div()
                .text_size(zz_ui::rems_from_px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(description),
        )
}

fn terminal_preview(appearance: zz_terminal::TerminalAppearance, _: &App) -> AnyElement {
    TerminalPreview { appearance }.into_any_element()
}

#[derive(IntoElement)]
struct TerminalPreview {
    appearance: zz_terminal::TerminalAppearance,
}

impl gpui::RenderOnce for TerminalPreview {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use zz_ui::terminal::{
            RowRenderCache, TerminalRenderInput, terminal_background, terminal_font_for_style,
        };
        let appearance = Arc::new(self.appearance);
        let viewport = Arc::new(sample_terminal_viewport(&appearance));
        let cache = Rc::new(RefCell::new(RowRenderCache::default()));
        let paint_cache = Rc::clone(&cache);
        let background = cx
            .theme()
            .background
            .opaque()
            .blend(terminal_background(
                appearance.background,
                appearance.background_opacity,
            ))
            .opacity(cx.theme().pane_background_opacity);
        let font = terminal_font_for_style(&appearance, &cx.theme().mono_font_family, false, false);
        let font_size = px(appearance.font_size_points);
        let probe = window.text_system().shape_line(
            "m".into(),
            font_size,
            &[gpui::TextRun {
                len: 1,
                font: font.clone(),
                color: cx.theme().foreground,
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        let font_id = probe.runs.first().map_or_else(
            || window.text_system().resolve_font(&font),
            |run| run.font_id,
        );
        let natural_height =
            probe.ascent + probe.descent + window.text_system().line_gap(font_id, font_size);
        let scale = window.scale_factor();
        let cell_height = (appearance
            .cell_height_adjustment
            .apply(f32::from(natural_height))
            .max(1.0)
            * scale)
            .round()
            / scale;
        let height = cell_height * 8.0 + appearance.padding_top + appearance.padding_bottom + 2.0;
        div()
            .w_full()
            .h(px(height.max(190.0)))
            .flex_none()
            .control_surface(cx)
            .rounded(cx.theme().radius)
            .overflow_hidden()
            .bg(background)
            .child(
                div()
                    .size_full()
                    .pl(px(appearance.padding_left))
                    .pr(px(appearance.padding_right))
                    .pt(px(appearance.padding_top))
                    .pb(px(appearance.padding_bottom))
                    .font(font)
                    .text_size(font_size)
                    .line_height(px(appearance
                        .cell_height_adjustment
                        .apply(f32::from(font_size))
                        .max(1.0)))
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                cache.borrow_mut().prepaint(
                                    TerminalRenderInput {
                                        viewport: &viewport,
                                        row_revisions: &[1, 2, 3, 4, 5, 6, 7, 8],
                                        revision_epoch: 0,
                                        history: None,
                                        images: None,
                                        local_scroll_target: None,
                                        command_output: true,
                                        appearance: &appearance,
                                        appearance_hash: appearance.stable_hash(),
                                        text_opacity: 1.0,
                                        focused: true,
                                        cursor_blink_visible: true,
                                        marked_text: None,
                                    },
                                    bounds,
                                    window,
                                    cx,
                                )
                            },
                            move |bounds, mut paint, window, cx| {
                                paint_cache
                                    .borrow_mut()
                                    .paint(&mut paint, bounds, window, cx);
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

fn sample_terminal_viewport(
    appearance: &zz_terminal::TerminalAppearance,
) -> zz_terminal::TerminalViewport {
    use zz_terminal::{
        ATTR_BOLD, ATTR_ITALIC, CellWidth, Cursor, PackedCell, PackedStyle, SessionStatus,
        TerminalViewport, UnderlineStyle,
    };
    const COLUMNS: u16 = 72;
    const ROWS: u16 = 8;
    let mut viewport =
        TerminalViewport::blank_with_appearance(COLUMNS, ROWS, SessionStatus::Running, appearance);
    let mut styles = vec![PackedStyle::new(
        appearance.foreground,
        appearance.background,
        None,
        0,
        UnderlineStyle::None,
    )];
    for index in 0..16 {
        styles.push(PackedStyle::new(
            appearance.palette[index],
            appearance.background,
            None,
            0,
            UnderlineStyle::None,
        ));
    }
    styles.push(PackedStyle::new(
        appearance.foreground,
        appearance.background,
        None,
        ATTR_BOLD,
        UnderlineStyle::None,
    ));
    styles.push(PackedStyle::new(
        appearance.foreground,
        appearance.background,
        None,
        ATTR_ITALIC,
        UnderlineStyle::None,
    ));
    let cells = Arc::make_mut(&mut viewport.cells);
    let mut write = |row: usize, column: usize, text: &str, style: u16| {
        for (offset, character) in text.chars().take(usize::from(COLUMNS) - column).enumerate() {
            cells[row * usize::from(COLUMNS) + column + offset] =
                PackedCell::new(character as u32, style, CellWidth::Narrow);
        }
    };
    write(0, 0, "~/projects/zz", 5);
    write(0, 15, "main", 3);
    write(1, 0, ">", 3);
    write(1, 2, "printf 'hello, terminal\\n'", 0);
    write(2, 0, "hello, terminal", 17);
    write(3, 0, "Regular", 0);
    write(3, 10, "Bold", 17);
    write(3, 17, "Italic", 18);
    for index in 0..8 {
        write(5, index * 4, "██", u16::try_from(index + 1).unwrap());
        write(6, index * 4, "██", u16::try_from(index + 9).unwrap());
    }
    write(7, 0, ">", 3);
    Arc::make_mut(&mut viewport.dictionary).styles = styles.into();
    viewport.cursor = Some(Cursor::new(
        2,
        7,
        true,
        true,
        false,
        appearance.cursor_style,
        appearance.cursor_color,
    ));
    viewport
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaneControl {
    BackgroundOpacity,
    Opacity,
    Glow,
    Margin,
    Radius,
    Border,
}

impl PaneControl {
    const ALL: [Self; 6] = [
        Self::BackgroundOpacity,
        Self::Opacity,
        Self::Glow,
        Self::Margin,
        Self::Radius,
        Self::Border,
    ];

    fn value(self, preferences: &mut Preferences) -> &mut f32 {
        match self {
            Self::BackgroundOpacity => &mut preferences.pane_background_opacity,
            Self::Opacity => &mut preferences.pane_inactive_opacity,
            Self::Glow => &mut preferences.pane_glow_strength,
            Self::Margin => &mut preferences.pane_margin,
            Self::Radius => &mut preferences.pane_radius,
            Self::Border => &mut preferences.pane_border_width,
        }
    }

    fn limits(self) -> (f32, f32, f64) {
        match self {
            Self::BackgroundOpacity | Self::Opacity => (0.0, 1.0, 0.05),
            Self::Glow => (0.0, 2.0, 0.05),
            Self::Margin | Self::Radius => (0.0, 32.0, 0.5),
            Self::Border => (0.0, 8.0, 0.5),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::BackgroundOpacity => "Pane background opacity",
            Self::Opacity => "Inactive pane opacity",
            Self::Glow => "Selected pane glow",
            Self::Margin => "Pane margin",
            Self::Radius => "Pane corner radius",
            Self::Border => "Pane border width",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::BackgroundOpacity => {
                "Background strength from 0% to 100%. Browser panes apply this to the toolbar only."
            }
            Self::Opacity => {
                "Visible strength of inactive pane content and chrome (0–1). Set to 1 to disable dimming."
            }
            Self::Glow => "Glow strength from 0% to 200%. Set to 0 to turn it off.",
            Self::Margin => "Space around each pane, in logical pixels (0–32).",
            Self::Radius => "Rounds every pane corner, in logical pixels (0–32).",
            Self::Border => {
                "Border width for gapped panes, in logical pixels (0–8). Set to 0 to disable."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sample_terminal_viewport;
    use zz_terminal::{ATTR_BOLD, ATTR_ITALIC, CursorStyle, TerminalAppearance};

    #[test]
    fn terminal_preview_uses_host_palette_styles_and_cursor() {
        let mut appearance = TerminalAppearance::default();
        appearance.palette[1] = appearance.palette[6];
        appearance.cursor_style = CursorStyle::Underline;
        appearance.cursor_color = appearance.palette[3];
        let viewport = sample_terminal_viewport(&appearance);
        let swatch = viewport.cell(5, 4).unwrap();
        assert_eq!(
            viewport.style(swatch).unwrap().foreground(),
            appearance.palette[1]
        );
        for (column, style) in [(10, ATTR_BOLD), (17, ATTR_ITALIC)] {
            let cell = viewport.cell(3, column).unwrap();
            assert_eq!(viewport.style(cell).unwrap().attributes() & style, style);
        }
        let cursor = viewport.cursor.unwrap();
        assert_eq!(cursor.style(), appearance.cursor_style);
        assert_eq!(cursor.color(), appearance.cursor_color);
        assert!(cursor.column() < viewport.columns && cursor.row() < viewport.rows);
    }
}
