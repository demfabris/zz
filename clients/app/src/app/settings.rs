use std::{cell::RefCell, rc::Rc, sync::Arc};

use zz_gpui::{
    AnyElement, App, Context, Entity, FocusHandle, IntoElement, SharedString, Subscription,
    TextInputMode, Window, canvas, div, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, Sizable as _, StyledExt as _, Theme, ThemeMode,
    UiZoom,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{InputEvent, InputState, NumberInput},
    interface_theme::{self, ThemeModeSetting},
    select::{Select, SelectEvent, SelectState},
    settings::{
        SettingEntry, SettingsSection, SettingsSelectItem, SettingsStack, StackPosition,
        appearance::{
            AppearancePageItem, SWATCH, ThemePicks, accent_swatches, appearance_page,
            appearance_page_items, contrast_slider, corner_radius_slider, percent_slider,
            picker_tile, theme_preview, themes_strip, tint_hue_slider, tint_slider, ui_font_select,
            zoom_slider,
        },
        settings_card_ground, settings_list_disclosure_header, settings_page_heading,
        settings_preview, settings_reset_button, settings_scroll_column,
    },
    switch::Switch,
};

use super::AppShell;
use crate::{
    connection::Connection,
    terminal::{TerminalDisplayPreferences, localized_font_appearance},
};

pub(super) use crate::preferences::Preferences;

const PALETTE_LAYOUTS: [(&str, &str); 2] = [("grouped", "Tree"), ("flat", "Flat")];
const HOST_PREFIXES: [(&str, &str); 2] = [("~", "~"), ("#", "#")];

fn palette_layout_value(preferences: &Preferences) -> &'static str {
    if preferences.palette_grouped {
        "grouped"
    } else {
        "flat"
    }
}

pub(super) const SECTIONS: [SettingsSection; 7] = [
    SettingsSection::Appearance,
    SettingsSection::StatusBar,
    SettingsSection::Panes,
    SettingsSection::Terminal,
    SettingsSection::Hosts,
    SettingsSection::Advanced,
    SettingsSection::About,
];

const PHONE_MAX_ZOOM: f32 = 1.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Setting {
    Section(SettingsSection),
    SystemTextSize,
    CommandPalette,
}

pub(super) fn shown(setting: Setting, phone: bool) -> bool {
    !phone
        || !matches!(
            setting,
            Setting::Section(SettingsSection::StatusBar | SettingsSection::Panes)
                | Setting::SystemTextSize
                | Setting::CommandPalette
        )
}

pub(crate) fn phone() -> bool {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    return zz_gpui_platform::mobile::phone();
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    false
}

pub(super) fn sections() -> Vec<SettingsSection> {
    let phone = phone();
    SECTIONS
        .into_iter()
        .filter(|section| shown(Setting::Section(*section), phone))
        .collect()
}

fn interface_zoom(preferences: &Preferences, system_scale: f32, phone: bool) -> f32 {
    if phone {
        preferences.zoom * system_scale.min(PHONE_MAX_ZOOM)
    } else if preferences.system_text_size {
        preferences.zoom * system_scale
    } else {
        preferences.zoom
    }
}

struct PlatformReduceMotion(bool);

impl zz_gpui::Global for PlatformReduceMotion {}

impl Preferences {
    pub(super) fn apply(&self, connection: &Entity<Connection>, window: &mut Window, cx: &mut App) {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let system = zz_gpui_platform::mobile::accessibility();
        #[cfg(any(target_os = "ios", target_os = "android"))]
        cx.set_global(PlatformReduceMotion(system.reduce_motion));
        if !cx.has_global::<PlatformReduceMotion>() {
            cx.set_global(PlatformReduceMotion(cx.reduce_motion()));
        }
        cx.set_reduce_motion(cx.global::<PlatformReduceMotion>().0 || !self.animations);
        cx.set_animation_speed(self.animation_speed);
        cx.set_global(zz_ui::program::ProgramColors(self.program_colors));
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let (contrast, text_scale) = (
            self.contrast * if system.increase_contrast { 1.25 } else { 1.0 },
            system.text_scale,
        );
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let (contrast, text_scale) = (self.contrast, 1.0);
        let zoom = interface_zoom(self, text_scale, phone());
        let pinned = interface_theme::pinned_theme_mode(self.theme_mode());
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            cx.set_window_appearance(pinned.map(|mode| {
                if mode.is_dark() {
                    zz_gpui::WindowAppearance::Dark
                } else {
                    zz_gpui::WindowAppearance::Light
                }
            }));
            let mode = pinned.unwrap_or_else(|| match cx.window_appearance() {
                zz_gpui::WindowAppearance::Light | zz_gpui::WindowAppearance::VibrantLight => {
                    ThemeMode::Light
                }
                _ => ThemeMode::Dark,
            });
            Theme::change(mode, Some(window), cx);
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        if let Some(mode) = pinned {
            Theme::change(mode, Some(window), cx);
        } else {
            Theme::sync_system_appearance(Some(window), cx);
        }
        let shown = self.shown_theme();
        let theme = Theme::global_mut(cx);
        theme.colors = shown.colors(theme.mode);
        shown.materials.apply(theme);
        shown.apply_roles(theme);
        window.set_default_corner_smoothing(zz_ui::CORNER_SMOOTHING);
        Theme::global_mut(cx).set_contrast(contrast);
        Theme::global_mut(cx).pip_opacity = self.pip_background_opacity;
        Theme::global_mut(cx).pane_glow_strength = self.pane_glow_strength;
        Theme::global_mut(cx).pane_frames = self.gaps;
        Theme::global_mut(cx).pane_margin = self.margin();
        let available_fonts = cx.text_system().all_font_names();
        let wanted_font = self.shown_ui_font();
        let ui_font = if available_fonts
            .iter()
            .any(|family| family.eq_ignore_ascii_case(wanted_font))
        {
            wanted_font.to_owned()
        } else if cfg!(any(
            target_family = "wasm",
            target_os = "ios",
            target_os = "android"
        )) {
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
        cx.set_global(UiZoom(zoom));
        window.set_zoom(zoom);
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
    accent: Entity<ColorPickerState>,
    panes: [Entity<InputState>; 4],
    ui_font: Entity<SelectState<Vec<SettingsSelectItem>>>,
    terminal_font: Entity<SelectState<Vec<SettingsSelectItem>>>,
    palette_layout: Entity<SelectState<Vec<SettingsSelectItem>>>,
    host_prefix: Entity<SelectState<Vec<SettingsSelectItem>>>,
    zoom: FocusHandle,
    /// Whether the Interface page lists the knobs under Advanced.
    advanced: bool,
    _subscriptions: Vec<Subscription>,
}

impl Controls {
    pub(super) fn new(
        preferences: &Preferences,
        window: &mut Window,
        cx: &mut Context<AppShell>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let accent = cx.new(|cx| ColorPickerState::new(custom_accent(preferences), window, cx));
        subscriptions.push(cx.subscribe_in(
            &accent,
            window,
            |this, _, event: &ColorPickerEvent, window, cx| {
                let ColorPickerEvent::Change(color) = event;
                this.preferences.overrides.accent =
                    color.map(|color| zz_ui::to_hex(color.opaque()));
                this.preferences.save();
                this.preferences.apply(&this.connection, window, cx);
            },
        ));
        let pane_values = [
            preferences.pip_background_opacity,
            preferences.pane_inactive_opacity,
            preferences.pane_glow_strength,
            preferences.pane_margin,
        ];
        let panes = PaneControl::ALL.map(|control| {
            let (min, max, step) = control.limits();
            let scale = if matches!(control, PaneControl::PipOpacity | PaneControl::Glow) {
                100.0
            } else {
                1.0
            };
            let (min, max, step) = (min * scale, max * scale, step * f64::from(scale));
            let value = pane_values[control as usize] * scale;
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .input_mode(TextInputMode::Decimal)
                    .default_value(number_text(value))
                    .min(f64::from(min))
                    .max(f64::from(max))
                    .step(step)
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, window, cx| {
                    number_field_event(event, window, cx);
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
                            input.set_value(number_text(value), window, cx);
                        });
                    }
                    this.preferences.save();
                    Theme::global_mut(cx).pip_opacity = this.preferences.pip_background_opacity;
                    Theme::global_mut(cx).pane_glow_strength = this.preferences.pane_glow_strength;
                    Theme::global_mut(cx).pane_margin = this.preferences.margin();
                    cx.refresh_windows();
                    cx.notify();
                },
            ));
            input
        });
        let ui_font = ui_font_select(Some(preferences.shown_ui_font()), window, cx);
        let mut families = cx.text_system().all_font_names();
        families.retain(|family| !family.starts_with('.') && !family.trim().is_empty());
        families.sort_by_cached_key(|family| family.to_lowercase());
        families.dedup();
        let items = std::iter::once(SettingsSelectItem::new("Follow host", ""))
            .chain(
                families
                    .into_iter()
                    .map(|family| SettingsSelectItem::new(family.clone(), family).preview_font()),
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
        let choice =
            |choices: &[(&str, &str)], current: &str, window: &mut Window, cx: &mut App| {
                let items = choices
                    .iter()
                    .map(|(value, title)| SettingsSelectItem::new(*title, *value))
                    .collect::<Vec<_>>();
                let index = choices.iter().position(|(value, _)| *value == current);
                cx.new(|cx| SelectState::new(items, index.map(zz_ui::IndexPath::new), window, cx))
            };
        let palette_layout = choice(
            &PALETTE_LAYOUTS,
            palette_layout_value(preferences),
            window,
            cx,
        );
        let host_prefix = choice(&HOST_PREFIXES, &preferences.palette_host_prefix, window, cx);
        for (select, layout) in [(&palette_layout, true), (&host_prefix, false)] {
            subscriptions.push(cx.subscribe_in(
                select,
                window,
                move |this, _, event: &SelectEvent<Vec<SettingsSelectItem>>, _, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    if layout {
                        this.preferences.palette_grouped = value == "grouped";
                    } else {
                        value.clone_into(&mut this.preferences.palette_host_prefix);
                    }
                    this.preferences.save();
                    cx.notify();
                },
            ));
        }
        Self {
            accent,
            panes,
            ui_font,
            terminal_font,
            palette_layout,
            host_prefix,
            zoom: cx.focus_handle(),
            advanced: false,
            _subscriptions: subscriptions,
        }
    }

    /// Shows the custom accent and UI font the preferences resolve to,
    /// after a theme change or a reset.
    fn sync_theme_inputs(&self, preferences: &Preferences, window: &mut Window, cx: &mut App) {
        let accent = custom_accent(preferences);
        self.accent
            .update(cx, |picker, cx| picker.set_color(accent, window, cx));
        let font = preferences.shown_ui_font().to_owned();
        self.ui_font.update(cx, |select, cx| {
            select.set_selected_value(&font, window, cx);
        });
    }
}

impl AppShell {
    fn theme_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_controls
            .sync_theme_inputs(&self.preferences, window, cx);
        self.preferences.save();
        self.preferences.apply(&self.connection, window, cx);
    }

    fn theme_saved(
        &mut self,
        saved: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use zz_ui::{WindowExt as _, notification::Notification};
        let notification = match saved {
            Ok(id) => {
                self.theme_changed(window, cx);
                Notification::success(format!("Saved the theme as {id}"))
            }
            Err(error) => Notification::error(error),
        };
        window.push_notification(notification, cx);
    }

    fn import_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let clipboard = cx.read_from_clipboard_async();
        cx.spawn_in(window, async move |this, cx| {
            let clipboard = clipboard.await;
            this.update_in(cx, |this, window, cx| {
                let saved = match clipboard {
                    Ok(item) => this
                        .preferences
                        .import_theme(&item.and_then(|item| item.text()).unwrap_or_default()),
                    Err(error) => Err(format!("Could not read the clipboard: {error}.")),
                };
                this.theme_saved(saved, window, cx);
            })
        })
        .detach();
    }

    pub(super) fn render_settings(
        &self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let narrow = AppShell::narrow(window);
        let sections = sections();
        let section = if sections.contains(&section) {
            section
        } else {
            SettingsSection::Appearance
        };
        if !narrow {
            return self.settings_page(section, narrow, cx);
        }
        let view = cx.entity().downgrade();
        let shell = cx.entity().downgrade();
        let picked = self.settings_picked;
        let attached = self.active_window(cx).is_some();
        zz_ui::compact::swipe_back(
            "settings-swipe-back",
            self.settings_route(section, picked, cx),
            move |window, cx| {
                let _ = view.update(cx, |this, cx| this.settings_back(window, cx));
            },
        )
        .enters(attached)
        .when(picked, |swipe| {
            swipe.under(self.settings_route(section, false, cx))
        })
        .when(!picked && attached, |swipe| {
            swipe.under_with(move |window, cx| {
                let top = window.fully_visible_bounds().top();
                let height = window.viewport_size().height;
                let backdrop = shell
                    .update(cx, |this, cx| {
                        this.compact_shell(Vec::new(), Vec::new(), window, cx)
                            .into_any_element()
                    })
                    .unwrap_or_else(|_| div().into_any_element());
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(-top)
                    .h(height)
                    .child(backdrop)
                    .into_any_element()
            })
        })
        .pushed(picked)
        .leaving(self.settings_leaving)
        .into_any_element()
    }

    pub(super) fn settings_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_leaving = false;
        if self.settings_picked {
            self.settings_picked = false;
        } else {
            self.settings = None;
            self.focused_pane = None;
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn settings_route(
        &self,
        section: SettingsSection,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content = if picked {
            self.settings_page(section, true, cx)
        } else {
            #[cfg(any(target_os = "ios", target_os = "android"))]
            let host = self
                .connection
                .read(cx)
                .endpoint()
                .map(super::hosts::host_title);
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            let host: Option<String> = None;
            zz_ui::settings::settings_section_index(
                &sections(),
                move |section| match section {
                    SettingsSection::Hosts => host.clone().map(Into::into),
                    SettingsSection::About => Some(env!("CARGO_PKG_VERSION").into()),
                    _ => None,
                },
                {
                    let view = cx.entity().downgrade();
                    move |choice, _, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.settings = Some(choice);
                            this.settings_picked = true;
                            cx.notify();
                        });
                    }
                },
                cx,
            )
            .into_any_element()
        };
        let title = if picked { section.title() } else { "Settings" };
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                zz_ui::compact::compact_nav_bar(cx)
                    .child(
                        zz_ui::compact::compact_bar_button("settings-back", IconName::ArrowLeft)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings_leaving = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(17.0))
                            .font_semibold()
                            .child(title),
                    ),
            )
            .child(
                div().flex_1().min_h_0().relative().child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .child(zz_ui::touch::touch_scale(content, cx)),
                ),
            )
            .into_any_element()
    }

    fn settings_page(
        &self,
        section: SettingsSection,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let phone = phone();
        match section {
            SettingsSection::Appearance => {
                let items = appearance_page_items(false, false, self.settings_controls.advanced)
                    .into_iter()
                    .filter(|item| {
                        matches!(
                            item,
                            AppearancePageItem::Description
                                | AppearancePageItem::Group { .. }
                                | AppearancePageItem::Advanced { .. }
                                | AppearancePageItem::ThemeMode
                                | AppearancePageItem::Accent
                                | AppearancePageItem::TintHue
                                | AppearancePageItem::Tint
                                | AppearancePageItem::Themes
                                | AppearancePageItem::ProgramColors
                                | AppearancePageItem::CornerRadius
                                | AppearancePageItem::ChromeContrast
                                | AppearancePageItem::UiFontFamily
                                | AppearancePageItem::UiZoom
                                | AppearancePageItem::Animations
                                | AppearancePageItem::AnimationSpeed
                        )
                    })
                    .filter(|item| !narrow || !matches!(item, AppearancePageItem::Description))
                    .collect::<Vec<_>>();
                let zoom = &self.settings_controls.zoom;
                let row_focus = items
                    .iter()
                    .map(|item| matches!(item, AppearancePageItem::UiZoom).then(|| zoom.clone()))
                    .collect::<Vec<_>>();
                let view = cx.entity();
                return appearance_page(items, move |item, position, _, cx| {
                    view.update(cx, |this, cx| {
                        this.appearance_item(item, position, narrow, cx)
                    })
                })
                .row_focus(row_focus)
                .into_any_element();
            }
            SettingsSection::Panes => {
                let [background, inactive, glow, margin] =
                    PaneControl::ALL.map(|control| self.pane_setting(control, cx));
                let gaps =
                    SettingEntry::new("Pane gaps", "Separate panes with spacing and borders.")
                        .title_actions(reset_button(
                            "settings-pane-gaps-reset",
                            self.preferences.gaps != Preferences::default().gaps,
                            |this, _, _| this.preferences.gaps = Preferences::default().gaps,
                            cx,
                        ))
                        .control(
                            Switch::new("settings-pane-gaps")
                                .checked(self.preferences.gaps)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.preferences.gaps = !this.preferences.gaps;
                                    this.preferences.save();
                                    this.preferences.apply(&this.connection, window, cx);
                                    cx.notify();
                                })),
                        );
                let agent = self.agent_panes_setting(cx);
                return zz_ui::settings::panes_page(
                    zz_ui::settings::panes_preview::PanesPreview {
                        gaps: self.preferences.gaps,
                        margin: self.preferences.pane_margin,
                        inactive_opacity: self.preferences.pane_inactive_opacity,
                    },
                    [gaps, agent],
                    [background],
                    [inactive, glow],
                    [margin],
                    cx,
                )
                .into_any_element();
            }
            SettingsSection::StatusBar => {
                let mut rows = vec![
                    SettingEntry::new(
                        "Sidebar",
                        "Show sessions, windows, and panes beside the workspace.",
                    )
                    .control(
                        Switch::new("settings-sidebar")
                            .checked(self.sidebar)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar = !this.sidebar;
                                this.preferences.sidebar = this.sidebar;
                                this.preferences.save();
                                cx.notify();
                            })),
                    ),
                ];
                for (id, title, description, checked) in [
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
                    rows.push(
                        SettingEntry::new(title, description)
                            .title_actions(reset_button(
                                format!("settings-status-{id}-reset"),
                                checked != default,
                                move |this, _, _| {
                                    *status_field(&mut this.preferences, id) = default;
                                },
                                cx,
                            ))
                            .control(
                                Switch::new(format!("settings-status-{id}"))
                                    .checked(checked)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let value = status_field(&mut this.preferences, id);
                                        *value = !*value;
                                        this.preferences.save();
                                        cx.notify();
                                    })),
                            ),
                    );
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
                let text_size = shown(Setting::SystemTextSize, phone).then(|| {
                    SettingEntry::new(
                        "Match system text size",
                        "Scale the interface with the iPadOS text size setting.",
                    )
                    .title_actions(reset_button(
                        "settings-system-text-size-reset",
                        self.preferences.system_text_size
                            != Preferences::default().system_text_size,
                        |this, _, _| {
                            this.preferences.system_text_size =
                                Preferences::default().system_text_size;
                        },
                        cx,
                    ))
                    .control(
                        Switch::new("settings-system-text-size")
                            .checked(self.preferences.system_text_size)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.preferences.system_text_size =
                                    !this.preferences.system_text_size;
                                this.preferences.save();
                                this.preferences.apply(&this.connection, window, cx);
                                cx.notify();
                            })),
                    )
                });
                return settings_scroll_column("settings-page")
                    .children((!narrow).then(|| {
                        settings_page_heading(
                            section.title(),
                            "Tune the command palette and display on this client.",
                            cx,
                        )
                    }))
                    .when(shown(Setting::CommandPalette, phone), |page| {
                        page.child(
                            SettingsStack::titled("Command palette")
                                .children(self.palette_settings(cx)),
                        )
                    })
                    .when(
                        !shown(Setting::Section(SettingsSection::Panes), phone),
                        |page| {
                            page.child(
                                SettingsStack::titled("Panes").child(self.agent_panes_setting(cx)),
                            )
                        },
                    )
                    .when(
                        cfg!(any(target_os = "ios", target_os = "android")),
                        |page| {
                            page.child(
                            SettingsStack::titled("Display").child(SettingEntry::new(
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
                                )).control(Switch::new("settings-bottom-safe-area")
                                    .checked(self.preferences.extend_bottom_safe_area)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.preferences.extend_bottom_safe_area =
                                            !this.preferences.extend_bottom_safe_area;
                                        this.preferences.save();
                                        cx.notify();
                                    }))))
                            .children(text_size)
                            .child(SettingEntry::new(
                                    "Keep the screen awake",
                                    "Stop the display from sleeping while connected to a host.",
                                )
                                .title_actions(reset_button(
                                    "settings-keep-awake-reset",
                                    self.preferences.keep_screen_awake
                                        != Preferences::default().keep_screen_awake,
                                    |this, _, cx| {
                                        this.preferences.keep_screen_awake =
                                            Preferences::default().keep_screen_awake;
                                        this.sync_idle_guard(cx);
                                    },
                                    cx,
                                )).control(Switch::new("settings-keep-awake")
                                    .checked(self.preferences.keep_screen_awake)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.preferences.keep_screen_awake =
                                            !this.preferences.keep_screen_awake;
                                        this.preferences.save();
                                        this.sync_idle_guard(cx);
                                        cx.notify();
                                    })))),
                        )
                        },
                    )
                    .into_any_element();
            }
            SettingsSection::About => return about_page(cx),
            _ => {}
        }
        self.hosts_page(
            "settings-page",
            (!narrow).then_some(section.title()),
            narrow,
            cx,
        )
    }

    fn appearance_item(
        &self,
        item: AppearancePageItem,
        position: StackPosition,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = match item {
            AppearancePageItem::ThemeMode => {
                let theme = self.preferences.shown_theme();
                let light = theme.colors(ThemeMode::Light);
                let dark = theme.colors(ThemeMode::Dark);
                let tiles = div().flex().flex_none().flex_wrap().gap(px(8.0)).children(
                    ThemeModeSetting::ALL.map(|mode| {
                        picker_tile(
                            format!("settings-theme-{}", mode.as_str()).into(),
                            mode.title(),
                            theme_preview(
                                interface_theme::pinned_theme_mode(mode),
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
                    }),
                );
                with_control(
                    SettingEntry::new("Mode", "Follow the system light/dark setting, or pin one.")
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
            AppearancePageItem::UiFontFamily => {
                SettingEntry::new("UI font", "Choose an available font for the interface.")
                    .title_actions(reset_button(
                        "settings-ui-font-reset",
                        self.preferences.ui_font_family != Preferences::default().ui_font_family,
                        |this, window, cx| {
                            this.preferences.ui_font_family = Preferences::default().ui_font_family;
                            let family = this.preferences.shown_ui_font().to_owned();
                            this.settings_controls.ui_font.update(cx, |select, cx| {
                                select.set_selected_value(&family, window, cx);
                            });
                        },
                        cx,
                    ))
                    .control(font_select(
                        &self.settings_controls.ui_font,
                        "UI font",
                        "System default",
                        cx,
                    ))
            }
            AppearancePageItem::UiZoom => {
                let view = cx.entity().downgrade();
                let slider = zoom_slider(
                    self.preferences.zoom,
                    &self.settings_controls.zoom,
                    move |zoom, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.zoom = zoom;
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        });
                    },
                    cx,
                );
                let (title, description) = item.text().unwrap_or_default();
                with_control(
                    SettingEntry::new(title, description).title_actions(
                        settings_reset_button(
                            "settings-zoom-reset",
                            "Reset UI zoom to 100%",
                            self.preferences.zoom != 1.0,
                        )
                        .on(settings_card_ground(cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preferences.reset_zoom(&this.connection, window, cx);
                        })),
                    ),
                    slider,
                    narrow,
                )
            }
            AppearancePageItem::ChromeContrast => {
                let (title, description) = item.text().unwrap_or_default();
                let view = cx.entity().downgrade();
                let slider = contrast_slider(
                    self.preferences.contrast,
                    move |contrast, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.contrast = contrast;
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        });
                    },
                    cx,
                );
                with_control(
                    SettingEntry::new(title, description).title_actions(
                        settings_reset_button(
                            "settings-contrast-reset",
                            "Reset contrast to 100%",
                            self.preferences.contrast != 1.0,
                        )
                        .on(settings_card_ground(cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preferences.contrast = 1.0;
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        })),
                    ),
                    slider,
                    narrow,
                )
            }
            AppearancePageItem::AnimationSpeed => {
                let (title, description) = item.text().unwrap_or_default();
                let view = cx.entity().downgrade();
                let slider = zz_ui::settings::appearance::animation_speed_slider(
                    self.preferences.animation_speed,
                    move |speed, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.animation_speed = speed;
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        });
                    },
                    cx,
                );
                let default = Preferences::default().animation_speed;
                with_control(
                    SettingEntry::new(title, description).title_actions(
                        settings_reset_button(
                            "settings-animation-speed-reset",
                            "Reset animation speed to 100%",
                            self.preferences.animation_speed != default,
                        )
                        .on(settings_card_ground(cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.preferences.animation_speed =
                                Preferences::default().animation_speed;
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        })),
                    ),
                    slider,
                    narrow,
                )
            }
            AppearancePageItem::ProgramColors => {
                let (title, description) = item.text().unwrap_or_default();
                SettingEntry::new(title, description)
                    .title_actions(reset_button(
                        "settings-program-colors-reset",
                        self.preferences.program_colors != Preferences::default().program_colors,
                        |this, _, _| {
                            this.preferences.program_colors = Preferences::default().program_colors;
                        },
                        cx,
                    ))
                    .control(
                        Switch::new("settings-program-colors")
                            .checked(self.preferences.program_colors)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.preferences.program_colors = !this.preferences.program_colors;
                                this.preferences.save();
                                this.preferences.apply(&this.connection, window, cx);
                            })),
                    )
            }
            AppearancePageItem::Animations => SettingEntry::new(
                "Animations",
                "Animate interface transitions, loading indicators, and image frames.",
            )
            .title_actions(reset_button(
                "settings-animations-reset",
                self.preferences.animations != Preferences::default().animations,
                |this, _, _| this.preferences.animations = Preferences::default().animations,
                cx,
            ))
            .control(
                Switch::new("settings-animations")
                    .checked(self.preferences.animations)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preferences.animations = !this.preferences.animations;
                        this.preferences.save();
                        this.preferences.apply(&this.connection, window, cx);
                    })),
            ),
            AppearancePageItem::Accent => {
                let (title, description) = item.text().unwrap_or_default();
                let custom =
                    ColorPicker::new(&self.settings_controls.accent, zz_gpui::transparent_black())
                        .label("Accent")
                        .swatch(px(SWATCH));
                let view = cx.entity().downgrade();
                let swatches = accent_swatches(
                    &theme_picks(&self.preferences),
                    cx.theme().mode,
                    custom,
                    move |name, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.overrides.accent = name.map(str::to_owned);
                            this.theme_changed(window, cx);
                        });
                    },
                    cx,
                );
                with_control(
                    SettingEntry::new(title, description).title_actions(reset_button(
                        "settings-accent-reset",
                        self.preferences.overrides.accent.is_some(),
                        |this, window, cx| {
                            this.preferences.overrides.accent = None;
                            this.settings_controls
                                .sync_theme_inputs(&this.preferences, window, cx);
                        },
                        cx,
                    )),
                    swatches,
                    narrow,
                )
            }
            AppearancePageItem::TintHue => {
                let (title, description) = item.text().unwrap_or_default();
                let hue = self
                    .preferences
                    .overrides
                    .tint_hue
                    .unwrap_or(interface_theme::DEFAULT_TINT_HUE);
                let view = cx.entity().downgrade();
                let slider = tint_hue_slider(
                    hue,
                    move |hue, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.overrides.tint_hue = Some(hue);
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        });
                    },
                    cx,
                );
                with_control(
                    SettingEntry::new(title, description).title_actions(reset_button(
                        "settings-tint-hue-reset",
                        self.preferences.overrides.tint_hue.is_some(),
                        |this, _, _| this.preferences.overrides.tint_hue = None,
                        cx,
                    )),
                    slider,
                    narrow,
                )
            }
            AppearancePageItem::Tint => {
                let (title, description) = item.text().unwrap_or_default();
                let tint = self.preferences.overrides.tint.unwrap_or(0.0);
                let view = cx.entity().downgrade();
                let slider = tint_slider(
                    tint,
                    move |tint, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.overrides.tint = Some(tint);
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        });
                    },
                    cx,
                );
                with_control(
                    SettingEntry::new(title, description).title_actions(reset_button(
                        "settings-tint-reset",
                        self.preferences.overrides.tint.is_some(),
                        |this, _, _| this.preferences.overrides.tint = None,
                        cx,
                    )),
                    slider,
                    narrow,
                )
            }
            AppearancePageItem::Themes => {
                let (title, description) = item.text().unwrap_or_default();
                let picks = theme_picks(&self.preferences);
                let ids = picks
                    .themes
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                let view = cx.entity().downgrade();
                let strip = themes_strip(
                    &picks,
                    cx.theme().mode,
                    move |index, window, cx| {
                        let id = ids[index].clone();
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.set_interface_theme(&id);
                            this.theme_changed(window, cx);
                        });
                    },
                    cx,
                );
                let selected = self.preferences.theme_id();
                let saved = self.preferences.saved_theme(selected).is_some();
                let actions = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.0))
                    .when(!narrow, |actions| {
                        actions
                            .child(
                                Button::new("settings-themes-save")
                                    .ghost()
                                    .small()
                                    .label("Save")
                                    .tooltip("Save the look on screen as a theme and use it")
                                    .on(settings_card_ground(cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let saved = this.preferences.duplicate_theme();
                                        this.theme_saved(saved, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-themes-import")
                                    .ghost()
                                    .small()
                                    .label("Import")
                                    .tooltip("Save the theme on the clipboard and use it")
                                    .on(settings_card_ground(cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.import_theme(window, cx);
                                    })),
                            )
                    })
                    .when(saved, |actions| {
                        actions.child(
                            Button::new("settings-themes-delete")
                                .ghost()
                                .small()
                                .label("Delete")
                                .tooltip("Delete this saved theme and use the default one")
                                .on(settings_card_ground(cx))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let id = this.preferences.theme_id().to_owned();
                                    this.preferences.delete_theme(&id);
                                    this.theme_changed(window, cx);
                                })),
                        )
                    });
                let reset = reset_button(
                    "settings-themes-reset",
                    selected != interface_theme::DEFAULT_THEME,
                    |this, window, cx| {
                        this.preferences
                            .set_interface_theme(interface_theme::DEFAULT_THEME);
                        this.settings_controls
                            .sync_theme_inputs(&this.preferences, window, cx);
                    },
                    cx,
                );
                let entry = SettingEntry::new(title, description);
                if narrow {
                    entry
                        .title_actions(reset)
                        .child(strip)
                        .when(saved, |entry| entry.child(actions))
                } else {
                    entry
                        .title_actions(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(8.0))
                                .child(reset)
                                .child(actions),
                        )
                        .child(strip)
                }
            }
            AppearancePageItem::Advanced { expanded } => {
                return settings_list_disclosure_header(
                    zz_ui::settings::appearance::ADVANCED_DISCLOSURE,
                    "Advanced",
                    Some("Corner radius, contrast, fonts and zoom."),
                    expanded,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_controls.advanced = !this.settings_controls.advanced;
                    cx.notify();
                }))
                .into_any_element();
            }
            AppearancePageItem::CornerRadius => {
                let (title, description) = item.text().unwrap_or_default();
                let view = cx.entity().downgrade();
                let slider = corner_radius_slider(
                    self.preferences.widget_corner_radius(),
                    move |radius, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.preferences.overrides.radius = Some(radius);
                            this.preferences.save();
                            this.preferences.apply(&this.connection, window, cx);
                        });
                    },
                    cx,
                );
                with_control(
                    SettingEntry::new(title, description).title_actions(reset_button(
                        "settings-corner-radius-reset",
                        self.preferences.overrides.radius.is_some(),
                        |this, _, _| this.preferences.overrides.radius = None,
                        cx,
                    )),
                    slider,
                    narrow,
                )
            }
            _ => return div().into_any_element(),
        };
        entry.position(position).into_any_element()
    }

    fn agent_panes_setting(&self, cx: &mut Context<Self>) -> SettingEntry {
        SettingEntry::new("Agent panes", "Allow creating agent panes on this client.").control(
            Switch::new("settings-agent-enabled")
                .checked(self.preferences.agent_enabled)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.preferences.agent_enabled = !this.preferences.agent_enabled;
                    this.preferences.save();
                    cx.notify();
                })),
        )
    }

    fn pane_setting(&self, control: PaneControl, cx: &mut Context<Self>) -> SettingEntry {
        let value = *control.value(&mut self.preferences.clone());
        let default = *control.value(&mut Preferences::default());
        let entry = SettingEntry::new(control.title(), control.description())
            .disabled(
                !matches!(control, PaneControl::PipOpacity | PaneControl::Opacity)
                    && !self.preferences.gaps,
            )
            .title_actions(
                settings_reset_button(
                    format!("settings-pane-reset-{}", control as usize),
                    "Reset to default",
                    value != default,
                )
                .on(settings_card_ground(cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    *control.value(&mut this.preferences) = default;
                    let scale = if matches!(control, PaneControl::PipOpacity | PaneControl::Glow) {
                        100.0
                    } else {
                        1.0
                    };
                    this.settings_controls.panes[control as usize].update(cx, |input, cx| {
                        input.set_value(number_text(default * scale), window, cx);
                    });
                    this.preferences.save();
                    this.preferences.apply(&this.connection, window, cx);
                })),
            );
        entry.control(number_control(
            &self.settings_controls.panes[control as usize],
            cx,
        ))
    }

    fn terminal_settings(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let available = cx.text_system().all_font_names();
        let appearance =
            localized_font_appearance(&self.connection.read(cx).core, &available, "Lilex", cx);
        let font = SettingEntry::new(
            "Font",
            "Use an available font on this client, or follow the host's configured font.",
        )
        .control(font_select(
            &self.settings_controls.terminal_font,
            "Terminal font",
            "Follow host",
            cx,
        ));
        let scale = SettingEntry::new(
            "Font scale",
            "Scale terminal text on this client from 50% to 300%.",
        )
        .title_actions(
            settings_reset_button(
                "settings-terminal-scale-reset",
                "Reset font scale to 100%",
                self.preferences.terminal_font_scale != 1.0,
            )
            .on(settings_card_ground(cx))
            .on_click(cx.listener(|this, _, window, cx| {
                this.preferences.terminal_font_scale = 1.0;
                this.preferences.save();
                this.preferences.apply(&this.connection, window, cx);
            })),
        );
        let view = cx.entity().downgrade();
        let scale = with_control(
            scale,
            percent_slider(
                "settings-terminal-scale",
                "Font scale",
                self.preferences.terminal_font_scale,
                move |scale, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.preferences.terminal_font_scale = scale;
                        this.preferences.save();
                        this.preferences.apply(&this.connection, window, cx);
                    });
                },
                cx,
            ),
            narrow,
        );
        let (host_title, host_description) = if phone() {
            (
                "Cursor shape and spacing",
                "Edit the Ghostty-compatible configuration on the daemon host to change these values. Colors follow the interface theme.",
            )
        } else {
            (
                "Colors, cursor, and spacing",
                "Edit the Ghostty-compatible configuration on the daemon host to change these values.",
            )
        };
        settings_scroll_column("settings-terminal")
            .children((!narrow).then(|| settings_page_heading("Terminal", "Adjust terminal text on this client. Colors, cursor, and spacing follow the host configuration.", cx)))
            .child(zz_ui::settings::settings_list_group_header("Preview", None, cx))
            .child(terminal_preview(appearance, cx))
            .child(SettingsStack::titled("Display").child(font).child(scale))
            .child(SettingsStack::titled("Host configuration")
                .child(SettingEntry::new(host_title, host_description).control(div().text_size(zz_ui::rems_from_px(13.0)).text_color(cx.theme().foreground.muted()).child("Shared"))))
            .into_any_element()
    }
}

impl AppShell {
    fn palette_settings(&self, cx: &Context<Self>) -> Vec<SettingEntry> {
        let defaults = Preferences::default();
        vec![
            SettingEntry::new(
                "Navigation layout",
                "Show sessions, windows, and panes as a tree or a flat list.",
            )
            .title_actions(reset_button(
                "settings-palette-layout-reset",
                self.preferences.palette_grouped != defaults.palette_grouped,
                |this, window, cx| {
                    this.preferences.palette_grouped = Preferences::default().palette_grouped;
                    let value = palette_layout_value(&this.preferences).to_owned();
                    this.settings_controls
                        .palette_layout
                        .update(cx, |select, cx| {
                            select.set_selected_value(&value, window, cx);
                        });
                },
                cx,
            ))
            .control(choice_select(
                &self.settings_controls.palette_layout,
                "Navigation layout",
                cx,
            )),
            SettingEntry::new(
                "Host prefix",
                "Type this character in an empty palette to browse hosts.",
            )
            .title_actions(reset_button(
                "settings-palette-host-prefix-reset",
                self.preferences.palette_host_prefix != defaults.palette_host_prefix,
                |this, window, cx| {
                    this.preferences.palette_host_prefix =
                        Preferences::default().palette_host_prefix;
                    let value = this.preferences.palette_host_prefix.clone();
                    this.settings_controls.host_prefix.update(cx, |select, cx| {
                        select.set_selected_value(&value, window, cx);
                    });
                },
                cx,
            ))
            .control(choice_select(
                &self.settings_controls.host_prefix,
                "Host prefix",
                cx,
            )),
            SettingEntry::new(
                "Command shortcuts",
                "Show keyboard shortcuts beside commands.",
            )
            .title_actions(reset_button(
                "settings-palette-show-keys-reset",
                self.preferences.palette_show_keys != defaults.palette_show_keys,
                |this, _, _| {
                    this.preferences.palette_show_keys = Preferences::default().palette_show_keys;
                },
                cx,
            ))
            .control(
                Switch::new("settings-palette-show-keys")
                    .checked(self.preferences.palette_show_keys)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.preferences.palette_show_keys = !this.preferences.palette_show_keys;
                        this.preferences.save();
                        cx.notify();
                    })),
            ),
        ]
    }
}

fn about_page(cx: &mut Context<AppShell>) -> AnyElement {
    use zz_ui::settings::about;

    static LOGOS: std::sync::LazyLock<[Arc<zz_gpui::Image>; 2]> = std::sync::LazyLock::new(|| {
        let [light, dark]: [&'static [u8]; 2] = if zz_protocol::app_identity::DEVELOPMENT {
            [
                include_bytes!("../../../../assets/zz-dev-light-512.png"),
                include_bytes!("../../../../assets/zz-dev-dark-512.png"),
            ]
        } else {
            [
                include_bytes!("../../../../assets/zz-light-512.png"),
                include_bytes!("../../../../assets/zz-dark-512.png"),
            ]
        };
        [light, dark].map(|bytes| {
            Arc::new(zz_gpui::Image::from_bytes(
                zz_gpui::ImageFormat::Png,
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
            zz_gpui::img(logo).size(px(about::ABOUT_LOGO_SIZE)),
            cx,
        ))
        .child(about::about_build_stack(
            platform,
            about::about_copy_button("settings-about-copy-build-info")
                .on(settings_card_ground(cx))
                .on_click(move |_, window, cx| {
                    use zz_ui::WindowExt as _;
                    cx.write_to_clipboard(zz_gpui::ClipboardItem::new_string(about::build_info(
                        &copied,
                    )));
                    window.push_notification(
                        zz_ui::notification::Notification::success("Copied build information"),
                        cx,
                    );
                }),
            cx,
        ))
        .child(about::about_project_stack(cx))
        .into_any_element()
}

pub(super) fn with_control(
    entry: SettingEntry,
    control: impl IntoElement,
    narrow: bool,
) -> SettingEntry {
    if narrow {
        entry.child(control)
    } else {
        entry.control(control)
    }
}

fn reset_button(
    id: impl Into<zz_gpui::ElementId>,
    changed: bool,
    reset: impl Fn(&mut AppShell, &mut Window, &mut Context<AppShell>) + 'static,
    cx: &Context<AppShell>,
) -> Button {
    settings_reset_button(id, "Reset to default", changed)
        .on(settings_card_ground(cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            reset(this, window, cx);
            this.preferences.save();
            this.preferences.apply(&this.connection, window, cx);
        }))
}

fn status_field<'a>(preferences: &'a mut Preferences, id: &str) -> &'a mut bool {
    match id {
        "badges" => &mut preferences.status_badges,
        _ => &mut preferences.status_agents,
    }
}

fn number_field_event(event: &InputEvent, window: &mut Window, cx: &mut App) {
    if matches!(event, InputEvent::PressEnter { .. }) {
        finish_field(window, cx);
    }
}

pub(super) fn finish_field(window: &mut Window, cx: &mut App) {
    if super::compact::keyboard_visible(window) {
        window.blur(cx);
    }
}

/// The color the custom accent picker holds: a hex accent, or none while
/// the accent is the theme's or a named one.
fn custom_accent(preferences: &Preferences) -> Option<zz_gpui::Hsla> {
    preferences
        .overrides
        .accent
        .as_deref()
        .filter(|accent| !interface_theme::ACCENTS.contains(accent))
        .and_then(|accent| interface_theme::accent_color(accent, ThemeMode::Dark))
}

/// The themes the Theme rows offer, the built-in first, with the active one
/// picked and the overrides set over it.
fn theme_picks(preferences: &Preferences) -> ThemePicks<'_> {
    let themes = interface_theme::builtin_themes()
        .iter()
        .map(|(id, theme)| (SharedString::from(*id), theme))
        .chain(
            preferences
                .themes
                .iter()
                .map(|(id, theme)| (SharedString::from(id.clone()), theme)),
        )
        .collect::<Vec<_>>();
    let active = preferences.theme_id();
    let selected = themes
        .iter()
        .position(|(id, _)| **id == *active)
        .unwrap_or(0);
    ThemePicks {
        themes,
        selected,
        overrides: &preferences.overrides,
    }
}

fn number_text(value: f32) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_owned()
}

fn font_select(
    select: &Entity<SelectState<Vec<SettingsSelectItem>>>,
    title: &'static str,
    placeholder: &'static str,
    cx: &App,
) -> zz_gpui::Div {
    div().w(px(200.0)).max_w_full().flex_none().child(
        Select::new(select)
            .title(title)
            .placeholder(placeholder)
            .on(settings_card_ground(cx)),
    )
}

fn choice_select(
    select: &Entity<SelectState<Vec<SettingsSelectItem>>>,
    title: &'static str,
    cx: &App,
) -> Select<Vec<SettingsSelectItem>> {
    Select::new(select)
        .title(title)
        .on(settings_card_ground(cx))
}

fn number_control(input: &Entity<InputState>, cx: &App) -> zz_gpui::Div {
    let width = if zz_ui::touch::CoarsePointer::get(cx) {
        110.0
    } else {
        120.0
    };
    div()
        .w(zz_ui::rems_from_px(width))
        .max_w_full()
        .flex_none()
        .child(NumberInput::new(input).on(settings_card_ground(cx)))
}

fn terminal_preview(appearance: zz_terminal::TerminalAppearance, _: &App) -> AnyElement {
    TerminalPreview { appearance }.into_any_element()
}

#[derive(IntoElement)]
struct TerminalPreview {
    appearance: zz_terminal::TerminalAppearance,
}

impl zz_gpui::RenderOnce for TerminalPreview {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use zz_ui::terminal::{
            RowRenderCache, TerminalRenderInput, terminal_background, terminal_font_for_style,
        };
        let mut appearance = self.appearance;
        let sample = sample_terminal_viewport(&appearance);
        appearance.local_colors = crate::terminal::phone_colors(&sample, &appearance.palette, cx);
        let viewport = Arc::new(match appearance.local_colors {
            Some(local) => local.viewport(&sample),
            None => sample,
        });
        let appearance = Arc::new(appearance);
        let cache = Rc::new(RefCell::new(RowRenderCache::default()));
        let paint_cache = Rc::clone(&cache);
        let background = if phone() {
            cx.theme()
                .layer(zz_ui::roles::SETTINGS_CARD, zz_ui::PaintState::Rest)
                .fill
        } else {
            cx.theme().pane_color(terminal_background(
                appearance.background,
                appearance.background_opacity,
            ))
        };
        let font = terminal_font_for_style(&appearance, &cx.theme().mono_font_family, false, false);
        let font_size = px(appearance.font_size_points);
        let probe = window.text_system().shape_line(
            "m".into(),
            font_size,
            &[zz_gpui::TextRun {
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
        settings_preview(background, cx)
            .w_full()
            .h(px(height.max(190.0)))
            .flex_none()
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
                                        scroll_pixel_offset: px(0.0),
                                        overscroll: px(0.0),
                                        extra_height: px(0.0),
                                        command_output: true,
                                        appearance: &appearance,
                                        appearance_hash: appearance.stable_hash(),
                                        text_opacity: 1.0,
                                        focused: true,
                                        cursor_blink_visible: true,
                                        marked_text: None,
                                        rows_above: None,
                                        rows_below: None,
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
    PipOpacity,
    Opacity,
    Glow,
    Margin,
}

impl PaneControl {
    const ALL: [Self; 4] = [Self::PipOpacity, Self::Opacity, Self::Glow, Self::Margin];

    fn value(self, preferences: &mut Preferences) -> &mut f32 {
        match self {
            Self::PipOpacity => &mut preferences.pip_background_opacity,
            Self::Opacity => &mut preferences.pane_inactive_opacity,
            Self::Glow => &mut preferences.pane_glow_strength,
            Self::Margin => &mut preferences.pane_margin,
        }
    }

    fn limits(self) -> (f32, f32, f64) {
        match self {
            Self::PipOpacity | Self::Opacity => (0.0, 1.0, 0.05),
            Self::Glow => (0.0, 2.0, 0.05),
            Self::Margin => (0.0, 32.0, 0.5),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::PipOpacity => "PiP background opacity",
            Self::Opacity => "Inactive pane opacity",
            Self::Glow => "Selected pane contrast",
            Self::Margin => "Pane margin",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::PipOpacity => {
                "How much of its own color a PiP lays over the panes under it, from 0% to 100%."
            }
            Self::Opacity => {
                "Visible strength of inactive pane content and chrome (0–1). Set to 1 to disable dimming."
            }
            Self::Glow => {
                "How far the selected pane stands apart, from 0% to 200%: it lightens while the others darken and their edges dim. Set to 0 to draw it like the others."
            }
            Self::Margin => "Space around each pane, in logical pixels (0–32).",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Preferences, SECTIONS, Setting, interface_zoom, number_text, sample_terminal_viewport,
        shown,
    };
    use zz_terminal::{ATTR_BOLD, ATTR_ITALIC, CursorStyle, TerminalAppearance};
    use zz_ui::settings::SettingsSection;

    #[test]
    fn phone_hides_pane_status_bar_and_system_text_settings_only() {
        let visible = |phone| {
            SECTIONS
                .into_iter()
                .filter(|section| shown(Setting::Section(*section), phone))
                .collect::<Vec<_>>()
        };
        assert_eq!(visible(false), SECTIONS);
        assert_eq!(
            visible(true),
            [
                SettingsSection::Appearance,
                SettingsSection::Terminal,
                SettingsSection::Hosts,
                SettingsSection::Advanced,
                SettingsSection::About,
            ]
        );
        for setting in [Setting::SystemTextSize, Setting::CommandPalette] {
            assert!(shown(setting, false));
            assert!(!shown(setting, true));
        }
    }

    #[test]
    fn phone_scale_is_ui_zoom_over_the_capped_system_text_size() {
        let zoomed = Preferences {
            zoom: 1.5,
            system_text_size: false,
            ..Preferences::default()
        };
        assert_eq!(interface_zoom(&zoomed, 1.12, false), 1.5);
        assert!((interface_zoom(&zoomed, 1.12, true) - 1.68).abs() < 1e-6);
        assert!((interface_zoom(&zoomed, 0.82, true) - 1.23).abs() < 1e-6);
        assert!((interface_zoom(&zoomed, 1.9, true) - 2.025).abs() < 1e-6);
        let following = Preferences {
            zoom: 1.5,
            ..Preferences::default()
        };
        assert!((interface_zoom(&following, 1.2, false) - 1.8).abs() < 1e-6);
    }

    #[test]
    fn number_fields_hide_float_noise() {
        assert_eq!(number_text(1.05_f32 * 100.0), "105");
        assert_eq!(number_text(0.85_f32 * 100.0), "85");
        assert_eq!(number_text(13.5), "13.5");
        assert_eq!(number_text(0.7), "0.7");
        assert_eq!(number_text(0.05), "0.05");
        assert_eq!(number_text(0.0), "0");
        assert_eq!(number_text(-0.001), "0");
    }

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

    #[cfg(not(target_family = "wasm"))]
    #[zz_gpui::test]
    fn apply_draws_a_saved_theme_in_its_colors_materials_and_roles(
        cx: &mut zz_gpui::TestAppContext,
    ) {
        use zz_gpui::{AppContext as _, px};
        use zz_ui::{ActiveTheme as _, ThemeMode};

        cx.update(zz_ui::init);
        let mut preferences = Preferences {
            mode: Some("dark".into()),
            ..Preferences::default()
        };
        preferences
            .import_theme(
                r##"{"name":"Neon","materials":{"radius":3,"outline":0.4,"float":{"strength":0.5}},"roles":{"button":{"shape":"pill"}},"dark":{"accent":"#ff00ff"}}"##,
            )
            .unwrap();
        let expected = preferences.interface_theme().colors(ThemeMode::Dark).accent;
        let window = cx.add_empty_window();
        let connection = window.update(|_, cx| cx.new(crate::connection::Connection::new));
        window.update(|window, cx| {
            preferences.apply(&connection, window, cx);
            let theme = cx.theme();
            assert_eq!(theme.mode, ThemeMode::Dark);
            assert_eq!(theme.accent, expected);
            assert_eq!(theme.radius, px(3.0));
            assert_eq!(theme.float_shadow.strength, 0.5);
            assert_eq!(theme.outline, px(0.4));
            assert_eq!(
                theme.style(zz_ui::Kind::Button.role()).shape,
                zz_ui::Shape::Pill
            );
        });
        preferences.overrides.radius = Some(5.0);
        preferences.overrides.accent = Some("green".into());
        window.update(|window, cx| {
            preferences.apply(&connection, window, cx);
            let theme = cx.theme();
            assert_eq!(
                Some(theme.accent),
                zz_ui::interface_theme::accent_color("green", ThemeMode::Dark)
            );
            assert_eq!(theme.radius, px(5.0));
            assert_eq!(theme.outline, px(0.4));
        });
    }
}
