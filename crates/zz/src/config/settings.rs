//! Native settings route backed by the application configuration.

use std::{
    collections::BTreeMap,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, Focusable, IntoElement,
    KeyBinding, Render, SharedString, Subscription, Window, div, img, prelude::*, px,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, IndexPath, Sizable as _,
    ThemeColor, ThemeMode, WindowExt as _,
    button::{Button, ButtonVariants as _},
    code_editor::{CodeEditor, CodeEditorEvent, CodeEditorState},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    h_flex,
    input::{Input, InputEvent, InputState, NumberInput},
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    tag::Tag,
};

use crate::{
    app_icon::AppIconSetting,
    config::{
        self, AppConfig, BrowserConfig, ConfigKey, ConfigProvenance, ConfigValue,
        remove_config_key, set_chrome_preset, set_config_key,
    },
    diagnostics,
    keymap::ChromeChord,
    mux::{
        client::MuxClient,
        hosts::{HostId, HostState},
    },
    theme::{
        CHROME_PRESETS, ChromeColor, ChromePreset, ChromePresetId, ThemeModeSetting,
        inherited_chrome_colors,
    },
    window::toast,
    workspace::add_host,
};
use zz_browser::SearchProvider;
use zz_client::{ChromeAction, UI_TABLE};
use zz_protocol::ConfigOverrideEntry;
use zz_terminal::{TerminalColorScheme, discover_ghostty_config};
use zz_ui::feedback::import_configuration_file_alert;
use zz_ui::settings::{
    SettingEntry, SettingsScrollColumn, SettingsSection, SettingsSelectItem, SettingsStack,
    StackPosition, settings_control_fill, settings_list_group_header, settings_page_content,
    settings_page_description, settings_provenance_badge, settings_reset_button,
    settings_scroll_column, settings_virtual_column,
};

gpui::actions!(zz, [OpenSettings]);

/// What the Settings hint prints when the chrome keymap names no chord for
/// `open-settings`. The binding itself is data; see `zz_client::ChromeKeymap`.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) const KEYBIND: &str = "cmd-,";
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(crate) const KEYBIND: &str = "ctrl-,";
const CONTROL_WIDTH: f32 = 120.0;
const CONFIG_EDITOR_FONT_SIZE: f32 = 12.0;
const CONFIG_EDITOR_PADDING: f32 = 2.0;
const THEME_PREVIEW_WIDTH: f32 = 84.0;
const THEME_PREVIEW_HEIGHT: f32 = 56.0;
const THEME_PREVIEW_SIDEBAR_WIDTH: f32 = 20.0;
const APP_ICON_PREVIEW_SIZE: f32 = 48.0;
const ABOUT_LOGO_SIZE: f32 = 88.0;
const REPOSITORY_URL: &str = "https://github.com/demfabris/zz";
const RELEASES_URL: &str = "https://github.com/demfabris/zz/releases";
const ISSUES_URL: &str = "https://github.com/demfabris/zz/issues/new";

pub fn init(cx: &mut App) {
    crate::keymap::bind(cx, UI_TABLE, key_bindings);
}

fn key_bindings(chords: &[ChromeChord]) -> Vec<KeyBinding> {
    chords
        .iter()
        .filter_map(|chord| match chord.action() {
            ChromeAction::OpenSettings => Some(chord.binding(OpenSettings, None)),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigFileKind {
    Mux,
    Terminal,
}

struct ConfigFileEditor {
    path: Option<PathBuf>,
    editor: Entity<CodeEditorState>,
    saved: String,
    error: Option<String>,
}

struct HostsSectionState {
    input: Entity<InputState>,
    error: Option<SharedString>,
}

const PAGE_ROW_GAP: f32 = 8.0;

fn run_position<T: Copy>(items: &[T], index: usize, is_entry: impl Fn(T) -> bool) -> StackPosition {
    let bounded = |neighbour: Option<usize>| {
        neighbour.is_none_or(|at| items.get(at).copied().is_none_or(|it| !is_entry(it)))
    };
    StackPosition::new(bounded(index.checked_sub(1)), bounded(Some(index + 1)))
}

fn page_row(element: AnyElement, ends_run: bool) -> AnyElement {
    div()
        .w_full()
        .pb(px(if ends_run { PAGE_ROW_GAP } else { 0.0 }))
        .child(element)
        .into_any_element()
}

#[derive(Clone, Copy)]
enum AppearancePageItem {
    Description,
    Group {
        title: &'static str,
        description: Option<&'static str>,
    },
    ThemeMode,
    UiZoom,
    AppIcon,
    Preset,
    ChromeColor(ChromeColor),
    Animations,
    WidgetCornerRadius,
    WindowBackgroundBlur,
    #[cfg(target_os = "linux")]
    WindowCornerRadius,
    #[cfg(target_os = "linux")]
    UseSystemTitlebar,
}

impl AppearancePageItem {
    const fn is_entry(self) -> bool {
        !matches!(self, Self::Description | Self::Group { .. })
    }
}

fn appearance_page_items(has_window_blur: bool) -> Vec<AppearancePageItem> {
    let mut items = vec![
        AppearancePageItem::Description,
        AppearancePageItem::Group {
            title: "Theme",
            description: None,
        },
        AppearancePageItem::ThemeMode,
        AppearancePageItem::UiZoom,
    ];
    if cfg!(target_os = "macos") {
        items.push(AppearancePageItem::AppIcon);
    }
    items.extend([
        AppearancePageItem::Group {
            title: "Chroma Colors",
            description: Some(
                "Recolors the application chrome. Every panel, hover state, muted label and focus \
                 ring is derived from these six, so nothing else needs setting.",
            ),
        },
        AppearancePageItem::Preset,
    ]);
    items.extend(
        ChromeColor::ALL
            .into_iter()
            .map(AppearancePageItem::ChromeColor),
    );
    items.extend([
        AppearancePageItem::Group {
            title: "Tweaks",
            description: None,
        },
        AppearancePageItem::Animations,
        AppearancePageItem::WidgetCornerRadius,
    ]);
    if has_window_blur {
        items.push(AppearancePageItem::WindowBackgroundBlur);
    }
    #[cfg(target_os = "linux")]
    items.extend([
        AppearancePageItem::WindowCornerRadius,
        AppearancePageItem::UseSystemTitlebar,
    ]);
    items
}

pub(crate) struct SettingsView {
    mux: Entity<MuxClient>,
    observed: AppConfig,
    observed_browser: BrowserConfig,
    browser_element_selector_hotkey: Entity<InputState>,
    browser_search_provider: Entity<SelectState<Vec<SettingsSelectItem>>>,
    ui_zoom: Entity<InputState>,
    observed_ui_zoom: u32,
    pane_inactive_opacity: Entity<InputState>,
    pane_corner_radius: Entity<InputState>,
    pane_margin: Entity<InputState>,
    editor_font_size: Entity<InputState>,
    pane_border_width: Entity<InputState>,
    widget_corner_radius: Entity<InputState>,
    window_corner_radius: Entity<InputState>,
    chrome_pickers: BTreeMap<ChromeColor, Entity<ColorPickerState>>,
    mux_config_editor: Option<ConfigFileEditor>,
    observed_appearance_overrides: Vec<ConfigOverrideEntry>,
    terminal_config_editor: Option<ConfigFileEditor>,
    hosts_state: Option<HostsSectionState>,
    section: SettingsSection,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub(crate) fn new(mux: Entity<MuxClient>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::settings";
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let observed = config::resolved_config(cx);
        let observed_browser = config::browser_config(cx);
        let browser_element_selector_hotkey =
            text_value_input(&observed_browser.element_selector_hotkey.value, window, cx);
        let browser_search_provider =
            search_provider_select(observed_browser.search_provider.value, window, cx);
        let ui_zoom = ui_zoom_input(window, cx);
        let pane_inactive_opacity = numeric_value_input(
            ConfigKey::PaneInactiveOpacity,
            observed.pane_inactive_opacity.value,
            0.05,
            window,
            cx,
        );
        let pane_corner_radius = numeric_value_input(
            ConfigKey::PaneCornerRadius,
            observed.pane_corner_radius.value,
            1.0,
            window,
            cx,
        );
        let pane_margin = numeric_value_input(
            ConfigKey::PaneMargin,
            observed.pane_margin.value,
            1.0,
            window,
            cx,
        );
        let editor_font_size = numeric_value_input(
            ConfigKey::EditorFontSize,
            observed.editor_font_size.value,
            1.0,
            window,
            cx,
        );
        let pane_border_width = numeric_value_input(
            ConfigKey::PaneBorderWidth,
            observed.pane_border_width.value,
            1.0,
            window,
            cx,
        );
        let widget_corner_radius = numeric_value_input(
            ConfigKey::WidgetCornerRadius,
            observed.widget_corner_radius.value,
            1.0,
            window,
            cx,
        );
        let window_corner_radius = numeric_value_input(
            ConfigKey::WindowCornerRadius,
            observed.window_corner_radius.value,
            1.0,
            window,
            cx,
        );
        let chrome_pickers = chrome_pickers(observed, window, cx);
        let mut subscriptions = vec![
            cx.observe(&mux, |_, _, cx| cx.notify()),
            cx.observe_global::<config::FleetHosts>(|_, cx| cx.notify()),
            browser_hotkey_subscription(&browser_element_selector_hotkey, window, cx),
            search_provider_subscription(&browser_search_provider, window, cx),
            ui_zoom_subscription(&ui_zoom, window, cx),
            numeric_input_subscription(
                &pane_inactive_opacity,
                ConfigKey::PaneInactiveOpacity,
                window,
                cx,
            ),
            numeric_input_subscription(
                &pane_corner_radius,
                ConfigKey::PaneCornerRadius,
                window,
                cx,
            ),
            numeric_input_subscription(&pane_margin, ConfigKey::PaneMargin, window, cx),
            numeric_input_subscription(&pane_border_width, ConfigKey::PaneBorderWidth, window, cx),
            numeric_input_subscription(
                &widget_corner_radius,
                ConfigKey::WidgetCornerRadius,
                window,
                cx,
            ),
            numeric_input_subscription(
                &window_corner_radius,
                ConfigKey::WindowCornerRadius,
                window,
                cx,
            ),
        ];
        subscriptions.extend(chrome_picker_subscriptions(&chrome_pickers, window, cx));
        let observed_appearance_overrides = config::daemon_config_overrides(cx);
        let settings = Self {
            mux,
            observed,
            observed_browser,
            browser_element_selector_hotkey,
            browser_search_provider,
            ui_zoom,
            observed_ui_zoom: crate::ui_scale::percent(cx),
            pane_inactive_opacity,
            pane_corner_radius,
            pane_margin,
            editor_font_size,
            pane_border_width,
            widget_corner_radius,
            window_corner_radius,
            chrome_pickers,
            mux_config_editor: None,
            observed_appearance_overrides,
            terminal_config_editor: None,
            hosts_state: None,
            section: SettingsSection::Appearance,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        log::trace!(
            target: DIAGNOSTIC_TARGET,
            "initialize section={:?} subscriptions={} terminal_editor_initialized=false mux_editor_initialized=false elapsed_us={}",
            settings.section,
            settings._subscriptions.len(),
            diagnostics::elapsed_us(started),
        );
        settings
    }

    fn ensure_section_state(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::settings";
        if section == SettingsSection::Hosts && self.hosts_state.is_none() {
            let started = diagnostics::timer(DIAGNOSTIC_TARGET);
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(add_host::PLACEHOLDER));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |settings, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        settings.submit_add_host(window, cx);
                    }
                },
            );
            self.hosts_state = Some(HostsSectionState { input, error: None });
            self._subscriptions.push(subscription);
            log::trace!(
                target: DIAGNOSTIC_TARGET,
                "initialize_section section={section:?} subscriptions=1 elapsed_us={}",
                diagnostics::elapsed_us(started),
            );
            return true;
        }
        let (kind, slot) = match section {
            SettingsSection::Terminal if self.terminal_config_editor.is_none() => {
                (ConfigFileKind::Terminal, &mut self.terminal_config_editor)
            }
            SettingsSection::Multiplexer if self.mux_config_editor.is_none() => {
                (ConfigFileKind::Mux, &mut self.mux_config_editor)
            }
            _ => return false,
        };
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let file = config_file_editor(kind, window, cx);
        let editor = file.editor.clone();
        *slot = Some(file);
        self._subscriptions
            .push(config_editor_subscription(&editor, window, cx));
        log::trace!(
            target: DIAGNOSTIC_TARGET,
            "initialize_section section={section:?} subscriptions=1 elapsed_us={}",
            diagnostics::elapsed_us(started),
        );
        true
    }

    fn submit_add_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self
            .hosts_state
            .as_ref()
            .expect("Hosts state is initialized before submission")
            .input
            .clone();
        let value = input.read(cx).value();
        let existing = config::fleet_hosts(cx)
            .into_iter()
            .map(|host| host.name)
            .collect::<Vec<_>>();
        let request = match add_host::parse_add_host(&value, &existing) {
            Ok(request) => request,
            Err(message) => {
                self.hosts_state
                    .as_mut()
                    .expect("Hosts state is initialized before submission")
                    .error = Some(message.into());
                cx.notify();
                return;
            }
        };
        if let Err(error) = config::add_fleet_host(&request.name, &request.endpoint, cx) {
            self.hosts_state
                .as_mut()
                .expect("Hosts state is initialized before submission")
                .error = Some(format!("Could not write zz/config: {error}").into());
            cx.notify();
            return;
        }
        log::info!(
            target: "zz::config",
            "added fleet host name={} endpoint={}",
            request.name,
            request.endpoint,
        );
        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.hosts_state
            .as_mut()
            .expect("Hosts state is initialized before submission")
            .error = None;
        cx.notify();
    }

    pub(crate) const fn section(&self) -> SettingsSection {
        self.section
    }

    pub(crate) fn focus(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn set_section(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initialized = self.ensure_section_state(section, window, cx);
        if !initialized {
            self.reload_config_editor_if_clean(section, window, cx);
        }
        self.section = section;
        cx.notify();
    }

    fn synchronize_numeric_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AppConfig {
        let resolved = config::resolved_config(cx);
        if resolved != self.observed {
            self.observed = resolved;
            synchronize_f32_input(
                &self.pane_inactive_opacity,
                resolved.pane_inactive_opacity.value,
                window,
                cx,
            );
            synchronize_f32_input(
                &self.pane_corner_radius,
                resolved.pane_corner_radius.value,
                window,
                cx,
            );
            synchronize_f32_input(&self.pane_margin, resolved.pane_margin.value, window, cx);
            synchronize_f32_input(
                &self.editor_font_size,
                resolved.editor_font_size.value,
                window,
                cx,
            );
            synchronize_f32_input(
                &self.pane_border_width,
                resolved.pane_border_width.value,
                window,
                cx,
            );
            synchronize_f32_input(
                &self.widget_corner_radius,
                resolved.widget_corner_radius.value,
                window,
                cx,
            );
            synchronize_f32_input(
                &self.window_corner_radius,
                resolved.window_corner_radius.value,
                window,
                cx,
            );
            for (color, picker) in &self.chrome_pickers {
                let value = resolved.chrome(*color).value;
                picker.update(cx, |picker, cx| picker.set_color(value, window, cx));
            }
        }
        resolved
    }

    fn synchronize_browser_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> BrowserConfig {
        let resolved = config::browser_config(cx);
        if resolved != self.observed_browser {
            self.observed_browser.clone_from(&resolved);
            synchronize_text_input(
                &self.browser_element_selector_hotkey,
                &resolved.element_selector_hotkey.value,
                window,
                cx,
            );
            let provider = resolved.search_provider.value.as_str().to_owned();
            self.browser_search_provider.update(cx, |select, cx| {
                select.set_selected_value(&provider, window, cx);
            });
        }
        resolved
    }

    fn synchronize_ui_zoom_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let percent = crate::ui_scale::percent(cx);
        if percent == self.observed_ui_zoom {
            return;
        }
        self.observed_ui_zoom = percent;
        if self
            .ui_zoom
            .read(cx)
            .value()
            .parse::<f32>()
            .is_ok_and(|shown| crate::ui_scale::is_effective_percent(shown, cx))
        {
            return;
        }
        self.ui_zoom.update(cx, |input, cx| {
            input.set_value(percent.to_string(), window, cx);
        });
    }

    fn preview_ui_zoom(input: &Entity<InputState>, cx: &mut Context<Self>) {
        let Ok(percent) = input.read(cx).value().parse::<f32>() else {
            return;
        };
        if (crate::ui_scale::MIN_UI_ZOOM..=crate::ui_scale::MAX_UI_ZOOM).contains(&percent) {
            crate::ui_scale::set_percent(percent, cx);
        }
    }

    fn commit_ui_zoom_input(
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(percent) = input.read(cx).value().parse::<f32>() {
            crate::ui_scale::set_percent(percent, cx);
        }
        let percent = crate::ui_scale::percent(cx).to_string();
        if input.read(cx).value().as_ref() != percent {
            input.update(cx, |input, cx| input.set_value(percent, window, cx));
        }
    }

    fn synchronize_terminal_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let overrides = config::daemon_config_overrides(cx);
        if overrides == self.observed_appearance_overrides {
            return;
        }
        self.observed_appearance_overrides = overrides;
        if self.terminal_config_editor.is_some() {
            self.reload_config_editor_if_clean(SettingsSection::Terminal, window, cx);
        }
    }

    fn commit_numeric_input(
        &mut self,
        key: ConfigKey,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let candidate = input.read(cx).value().to_string();
        let value = match validate_numeric_value(key, &candidate) {
            Ok(value) => value,
            Err(message) => {
                toast::push(
                    Notification::error(format!("Invalid {}: {message}", key.as_str())),
                    cx,
                );
                let effective = numeric_config_value(self.observed, key).to_string();
                input.update(cx, |input, cx| input.set_value(effective, window, cx));
                return;
            }
        };
        let value = value.to_string();
        if input.read(cx).value().as_ref() != value {
            input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
        if let Err(error) = set_config_key(key, &value) {
            report_write_error("set", key.as_str(), &error, cx);
        }
    }

    fn commit_browser_hotkey(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = ConfigKey::BrowserElementSelectorHotkey;
        let candidate = input.read(cx).value().to_string();
        let value = match config::normalize_browser_hotkey(&candidate) {
            Ok(value) => value,
            Err(message) => {
                toast::push(
                    Notification::error(format!("Invalid {}: {message}", key.as_str())),
                    cx,
                );
                synchronize_text_input(
                    input,
                    &self.observed_browser.element_selector_hotkey.value,
                    window,
                    cx,
                );
                return;
            }
        };
        synchronize_text_input(input, &value, window, cx);
        if let Err(error) = set_config_key(key, &value) {
            report_write_error("set", key.as_str(), &error, cx);
        }
    }

    fn boolean_setting(
        key: ConfigKey,
        title: &'static str,
        description: &'static str,
        setting: ConfigValue<bool>,
        _cx: &Context<Self>,
    ) -> SettingEntry {
        SettingEntry::new(title, description)
            .title_actions(key_annotations(key, setting.provenance))
            .control(
                Switch::new(format!("settings-{}", key.as_str()))
                    .checked(setting.value)
                    .on_click(move |enabled, _, cx| {
                        if let Err(error) =
                            set_config_key(key, if *enabled { "true" } else { "false" })
                        {
                            report_write_error("set", key.as_str(), &error, cx);
                        }
                    }),
            )
    }

    fn numeric_setting(
        key: ConfigKey,
        title: &'static str,
        description: &'static str,
        setting: ConfigValue<f32>,
        input: &Entity<InputState>,
        cx: &Context<Self>,
    ) -> SettingEntry {
        SettingEntry::new(title, description)
            .title_actions(key_annotations(key, setting.provenance))
            .control(
                div().w(px(CONTROL_WIDTH)).flex_none().child(
                    NumberInput::new(input)
                        .small()
                        .bg(settings_control_fill(cx)),
                ),
            )
    }
}

impl SettingsView {
    fn scroll_column(id: &'static str) -> SettingsScrollColumn {
        settings_scroll_column(id)
    }

    fn appearance_section(&self, resolved: AppConfig, cx: &Context<Self>) -> AnyElement {
        let items: Arc<[AppearancePageItem]> =
            appearance_page_items(crate::profile::profile(cx).has_window_blur).into();
        let item_count = items.len();
        let inherited = inherited_chrome_colors(resolved.chrome_preset.value, cx.theme().mode);
        let view = cx.entity();
        settings_virtual_column("settings-appearance", item_count, move |index, _, cx| {
            let Some(item) = items.get(index).copied() else {
                return div().into_any_element();
            };
            let position = run_position(&items, index, AppearancePageItem::is_entry);
            let row = view.update(cx, |this, cx| {
                this.appearance_item(item, resolved, &inherited, position, cx)
            });
            page_row(row, !item.is_entry() || position.ends_run())
        })
        .into_any_element()
    }

    fn appearance_item(
        &self,
        item: AppearancePageItem,
        resolved: AppConfig,
        inherited: &ThemeColor,
        position: StackPosition,
        cx: &Context<Self>,
    ) -> AnyElement {
        match item {
            AppearancePageItem::Description => {
                return settings_page_description(SettingsSection::Appearance, cx)
                    .into_any_element();
            }
            AppearancePageItem::Group { title, description } => {
                return settings_list_group_header(title, description, cx).into_any_element();
            }
            _ => {}
        }

        let entry = match item {
            AppearancePageItem::Description | AppearancePageItem::Group { .. } => {
                unreachable!("returned above")
            }
            AppearancePageItem::ThemeMode => Self::theme_mode_setting(resolved, cx),
            AppearancePageItem::UiZoom => self.ui_zoom_setting(cx),
            AppearancePageItem::AppIcon => Self::app_icon_setting(resolved, cx),
            AppearancePageItem::Preset => Self::preset_setting(resolved, cx),
            AppearancePageItem::ChromeColor(color) => {
                self.chrome_color_setting(color, resolved, inherited)
            }
            AppearancePageItem::Animations => Self::boolean_setting(
                ConfigKey::Animations,
                "Animations",
                "Animate interface transitions, loading indicators, and image frames.",
                resolved.animations,
                cx,
            ),
            AppearancePageItem::WidgetCornerRadius => Self::numeric_setting(
                ConfigKey::WidgetCornerRadius,
                "Widget corner radius",
                "Rounds every widget: buttons, inputs, tags, menus, dialogs.",
                resolved.widget_corner_radius,
                &self.widget_corner_radius,
                cx,
            ),
            AppearancePageItem::WindowBackgroundBlur => Self::boolean_setting(
                ConfigKey::WindowBackgroundBlur,
                "Window blur",
                "Blur the desktop through the window chrome, if the compositor supports it.",
                resolved.window_background_blur,
                cx,
            ),
            #[cfg(target_os = "linux")]
            AppearancePageItem::WindowCornerRadius => Self::numeric_setting(
                ConfigKey::WindowCornerRadius,
                "Window corner radius",
                "Rounds the frame zz draws with client-side decorations.\
                 Match your compositor's rounding, or 0 for square. No effect while the \
                 system titlebar is on.",
                resolved.window_corner_radius,
                &self.window_corner_radius,
                cx,
            ),
            #[cfg(target_os = "linux")]
            AppearancePageItem::UseSystemTitlebar => Self::boolean_setting(
                ConfigKey::UseSystemTitlebar,
                "System titlebar",
                "Ask the desktop to draw the window titlebar and borders when supported.",
                resolved.use_system_titlebar,
                cx,
            ),
        };

        entry.position(position).into_any_element()
    }

    fn ui_zoom_setting(&self, cx: &Context<Self>) -> SettingEntry {
        let is_default = crate::ui_scale::is_default(cx);
        SettingEntry::new(
            "UI zoom",
            "Scales application text, icons, and controls, as a percentage of the default.",
        )
        .title_actions(
            settings_reset_button(
                "settings-ui-zoom-reset",
                if is_default {
                    "Already at 100%"
                } else {
                    "Reset UI zoom to 100%"
                },
                !is_default,
            )
            .on_click(|_, _, cx| crate::ui_scale::reset(cx)),
        )
        .control(
            div().w(px(CONTROL_WIDTH)).flex_none().child(
                NumberInput::new(&self.ui_zoom)
                    .small()
                    .bg(settings_control_fill(cx)),
            ),
        )
    }

    fn theme_mode_setting(resolved: AppConfig, cx: &Context<Self>) -> SettingEntry {
        let setting = resolved.theme_mode;
        SettingEntry::new("Theme", "Follow the system light/dark setting, or pin one.")
            .title_actions(key_annotations(ConfigKey::ThemeMode, setting.provenance))
            .control(div().flex().flex_none().gap(px(10.0)).children(
                ThemeModeSetting::ALL.into_iter().map(|mode| {
                    picker_tile(
                        format!("settings-theme-mode-{}", mode.as_str()).into(),
                        mode.title(),
                        theme_preview(mode, resolved.chrome_preset.value, cx),
                        mode == setting.value,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        if let Err(error) = set_config_key(ConfigKey::ThemeMode, mode.as_str()) {
                            report_write_error("set", ConfigKey::ThemeMode.as_str(), &error, cx);
                        }
                    })
                }),
            ))
    }

    fn app_icon_setting(resolved: AppConfig, cx: &Context<Self>) -> SettingEntry {
        let setting = resolved.app_icon;
        let appearance = cx.window_appearance();
        SettingEntry::new("App Icon", "Dock and app switcher.")
            .title_actions(key_annotations(ConfigKey::AppIcon, setting.provenance))
            .control(div().flex().flex_none().gap(px(10.0)).children(
                AppIconSetting::ALL.into_iter().map(|choice| {
                    picker_tile(
                        format!("settings-app-icon-{}", choice.as_str()).into(),
                        choice.title(),
                        img(crate::app_icon::icon_preview(choice.variant(appearance)))
                            .size(px(APP_ICON_PREVIEW_SIZE)),
                        choice == setting.value,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        if let Err(error) = set_config_key(ConfigKey::AppIcon, choice.as_str()) {
                            report_write_error("set", ConfigKey::AppIcon.as_str(), &error, cx);
                        }
                    })
                }),
            ))
    }

    fn preset_setting(resolved: AppConfig, cx: &Context<Self>) -> SettingEntry {
        let setting = resolved.chrome_preset;
        let label = setting
            .value
            .map_or("Color theme", |preset| preset.preset().name);
        SettingEntry::new(
            "Preset",
            "Choose a color theme or manually define your own below.",
        )
        .title_actions(key_annotations(ConfigKey::ChromePreset, setting.provenance))
        .control(
            Button::new("settings-preset")
                .small()
                .label(label)
                .dropdown_caret(true)
                .bg(settings_control_fill(cx))
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                    CHROME_PRESETS
                        .iter()
                        .fold(menu.min_w(px(230.0)), |menu, preset| {
                            menu.item(
                                PopupMenuItem::element(move |_, cx| {
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.0))
                                        .py(px(2.0))
                                        .child(preset.name)
                                        .child(preset_swatches(preset, cx))
                                })
                                .on_click(move |_, _, cx| apply_preset(preset, cx)),
                            )
                        })
                }),
        )
    }

    fn chrome_color_setting(
        &self,
        color: ChromeColor,
        resolved: AppConfig,
        inherited: &ThemeColor,
    ) -> SettingEntry {
        let setting = resolved.chrome(color);
        let key = ConfigKey::Chrome(color);
        let badge = if setting.provenance == ConfigProvenance::Default
            && resolved.chrome_preset.value.is_some()
        {
            settings_provenance_badge("Preset")
        } else {
            provenance_badge(setting.provenance)
        };
        SettingEntry::new(color.title(), color.description())
            .title_actions(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .child(config_reset_button(key, setting.provenance))
                    .child(badge),
            )
            .control(
                ColorPicker::new(&self.chrome_pickers[&color], color.read(inherited))
                    .label(color.title()),
            )
    }

    fn browser_section(
        &self,
        browser: BrowserConfig,
        resolved: AppConfig,
        cx: &Context<Self>,
    ) -> AnyElement {
        let setting = browser.element_selector_hotkey;
        let search = browser.search_provider;
        Self::scroll_column("settings-browser")
            .child(settings_page_description(SettingsSection::Browser, cx))
            .child(
                SettingsStack::titled("Network").child(Self::boolean_setting(
                    ConfigKey::BrowserEgress,
                    "Route through the attached host",
                    "Browser panes opened while attached to a remote host send their traffic \
                     through it, so localhost and private names resolve there. Panes that are \
                     already open keep the route they were created with.",
                    resolved.browser_egress,
                    cx,
                )),
            )
            .child(
                SettingsStack::titled("Search").child(
                    SettingEntry::new(
                        "Search engine",
                        "Default search engine used by free text queries in the URL bar.",
                    )
                    .title_actions(key_annotations(
                        ConfigKey::BrowserSearchProvider,
                        search.provenance,
                    ))
                    .control(
                        div().w(px(CONTROL_WIDTH)).flex_none().child(
                            Select::new(&self.browser_search_provider)
                                .small()
                                .bg(settings_control_fill(cx)),
                        ),
                    ),
                ),
            )
            .child(
                SettingsStack::titled("Shortcuts").child(
                    SettingEntry::new(
                        "Element selector",
                        "Toggle DOM element selector (a.k.a design mode). Use GPUI key syntax such as cmd-shift-c or ctrl-shift-c.",
                    )
                    .title_actions(key_annotations(
                        ConfigKey::BrowserElementSelectorHotkey,
                        setting.provenance,
                    ))
                    .control(
                        div().w(px(CONTROL_WIDTH)).flex_none().child(
                            Input::new(&self.browser_element_selector_hotkey)
                                .small()
                                .bg(settings_control_fill(cx)),
                        ),
                    ),
                ),
            )
            .into_any_element()
    }

    fn hosts_section(&self, cx: &Context<Self>) -> AnyElement {
        const INPUT_WIDTH: f32 = 220.0;

        let hosts = config::fleet_hosts(cx);
        let states = self
            .mux
            .read(cx)
            .fleet_hosts()
            .filter(|(id, _, _, _)| *id != HostId::LOCAL)
            .map(|(_, name, state, _)| (name.to_owned(), state.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut machine_entries = Vec::with_capacity(hosts.len().max(1));
        if hosts.is_empty() {
            machine_entries.push(SettingEntry::new(
                "No hosts yet",
                "Machines you add appear in the sidebar and start connecting immediately.",
            ));
        } else {
            for host in hosts {
                let name = host.name;
                let state = states
                    .get(&name)
                    .cloned()
                    .unwrap_or(HostState::Disconnected);
                let (glyph, color) = match &state {
                    HostState::Connected => (IconName::Globe, cx.theme().success),
                    HostState::Unreachable { .. } | HostState::Incompatible { .. } => {
                        (IconName::Globe, cx.theme().danger)
                    }
                    HostState::Connecting | HostState::Reconnecting { .. } => {
                        (IconName::Globe, cx.theme().warning)
                    }
                    HostState::Disconnected => (IconName::Globe, cx.theme().foreground.muted()),
                };
                let remove_name = name.clone();
                let remove_mux = self.mux.clone();
                let entry = SettingEntry::new(name.clone(), host.endpoint.to_string())
                    .title_icon(Icon::new(glyph).text_color(color))
                    .control(
                        Button::new(format!("settings-remove-host-{name}"))
                            .small()
                            .ghost()
                            .icon(IconName::Xmark)
                            .tooltip(format!("Remove host ({})", state.label()))
                            .on_click(move |_, _, cx| {
                                let host_id = remove_mux.read(cx).fleet_hosts().find_map(
                                    |(id, current_name, _, _)| {
                                        (id != HostId::LOCAL
                                            && current_name == remove_name.as_str())
                                        .then_some(id)
                                    },
                                );
                                if let Some(host_id) = host_id {
                                    crate::workspace::sidebar::close_host(
                                        &remove_mux,
                                        host_id,
                                        &remove_name,
                                        cx,
                                    );
                                } else {
                                    match config::remove_fleet_host_live(&remove_name, cx) {
                                        Ok(removed) => log::info!(
                                            target: "zz::config",
                                            "removed fleet host name={remove_name} config_removed={removed}",
                                        ),
                                        Err(error) => log::warn!(
                                            "could not remove host-{remove_name} from zz/config: {error}",
                                        ),
                                    }
                                }
                            }),
                    );
                machine_entries.push(entry);
            }
        }

        let state = self
            .hosts_state
            .as_ref()
            .expect("Hosts state is initialized before rendering");
        let input = state.input.clone();
        let mut destination = SettingEntry::new(
            "Destination",
            "user@host[:port] Authentication is handled by your own ssh machinery.",
        )
        .control(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .w(px(INPUT_WIDTH))
                        .flex_none()
                        .child(Input::new(&input).small().bg(settings_control_fill(cx))),
                )
                .child(
                    Button::new("settings-add-host")
                        .small()
                        .primary()
                        .label("Add")
                        .on_click(cx.listener(|settings, _, window, cx| {
                            settings.submit_add_host(window, cx);
                        })),
                ),
        );
        if let Some(error) = state.error.clone() {
            destination = destination.child(
                div()
                    .flex_none()
                    .text_size(zz_ui::rems_from_px(11.0))
                    .text_color(cx.theme().warning)
                    .child(error),
            );
        }

        Self::scroll_column("settings-hosts")
            .child(settings_page_description(SettingsSection::Hosts, cx))
            .child(SettingsStack::titled("Machines").children(machine_entries))
            .child(SettingsStack::titled("Add host").child(destination))
            .into_any_element()
    }

    fn advanced_section(resolved: AppConfig, cx: &Context<Self>) -> AnyElement {
        Self::scroll_column("settings-advanced")
            .child(settings_page_description(SettingsSection::Advanced, cx))
            .when(crate::profile::profile(cx).has_tray, |column| {
                column.child(SettingsStack::titled("Tray").child(Self::boolean_setting(
                    ConfigKey::Tray,
                    "Tray icon",
                    "Keep zz in the system tray: clicking the icon toggles the \
                     window, and closing the window hides it there while the \
                     daemon keeps running. Turning this off removes the icon \
                     entirely and makes closing the window quit again. Takes \
                     effect at the next launch.",
                    resolved.tray,
                    cx,
                )))
            })
            .when(crate::profile::profile(cx).has_daemon_lifecycle, |column| {
                column.child(SettingsStack::titled("Daemon").child(Self::boolean_setting(
                    ConfigKey::QuitDaemonOnExit,
                    "Quit daemon on exit",
                    "Stop the zz daemon when the app quits, even if sessions are still open.",
                    resolved.quit_daemon_on_exit,
                    cx,
                )))
            })
            .child(
                SettingsStack::titled("Diagnostics").child(Self::boolean_setting(
                    ConfigKey::ShowFps,
                    "Show FPS",
                    "Show a frame-rate overlay.",
                    resolved.show_fps,
                    cx,
                )),
            )
            .when(
                cfg!(any(feature = "agent-pane", feature = "editor-pane")),
                |column| {
                    column.child(
                        SettingsStack::titled("Experimental")
                            .when(cfg!(feature = "editor-pane"), |group| {
                                group.child(Self::boolean_setting(
                                    ConfigKey::ExperimentalEditorPane,
                                    "Editor pane",
                                    "Offer the Editor pane in the pane picker. Not ready \
                                     for prime time; expect rough edges. Turning this off \
                                     blocks new editor panes everywhere (picker, command \
                                     palette, CLI); editor panes that are already open keep \
                                     working.",
                                    resolved.experimental_editor_pane,
                                    cx,
                                ))
                            })
                            .when(cfg!(feature = "agent-pane"), |group| {
                                group.child(Self::boolean_setting(
                                    ConfigKey::ExperimentalAgentPane,
                                    "Agent pane",
                                    "Offer the Agent pane in the pane picker. Not ready \
                                     for prime time; expect rough edges. Turning this off \
                                     blocks new agent panes everywhere (picker, command \
                                     palette, CLI); agent panes that are already open keep \
                                     running.",
                                    resolved.experimental_agent_pane,
                                    cx,
                                ))
                            }),
                    )
                },
            )
            .into_any_element()
    }

    fn about_section(cx: &Context<Self>) -> AnyElement {
        Self::scroll_column("settings-about")
            .child(about_hero(cx))
            .child(
                SettingsStack::titled("Build")
                    .description("What to quote in a bug report.")
                    .child(
                        SettingEntry::new("Version", "The zz bundle version.")
                            .title_actions(copy_build_info_button())
                            .control(about_value(env!("CARGO_PKG_VERSION"), cx)),
                    )
                    .child(
                        SettingEntry::new(
                            "Platform",
                            "The operating system and processor architecture this build targets.",
                        )
                        .control(about_value(platform(), cx)),
                    )
                    .child(
                        SettingEntry::new("Renderer", "The GPUI revision this build links.")
                            .control(about_value(gpui_revision(), cx)),
                    ),
            )
            .child(
                SettingsStack::titled("Project")
                    .child(
                        SettingEntry::new(
                            "Source code",
                            "zz is open source. Read it, build it, or send a patch.",
                        )
                        .control(link_button(
                            "settings-about-repository",
                            "GitHub",
                            REPOSITORY_URL,
                            cx,
                        )),
                    )
                    .child(
                        SettingEntry::new(
                            "Releases",
                            "Every tagged build, with notes on what changed.",
                        )
                        .control(link_button(
                            "settings-about-releases",
                            "Releases",
                            RELEASES_URL,
                            cx,
                        )),
                    )
                    .child(
                        SettingEntry::new(
                            "Report an issue",
                            "Something broken or missing? Bring the build details above.",
                        )
                        .control(link_button(
                            "settings-about-issues",
                            "New issue",
                            ISSUES_URL,
                            cx,
                        )),
                    )
                    .child(
                        SettingEntry::new(
                            "License",
                            "Dual-licensed, at your option. Contributions land under both.",
                        )
                        .control(about_value("MIT or Apache-2.0", cx)),
                    ),
            )
            .into_any_element()
    }

    fn config_file_section(&self, kind: ConfigFileKind, cx: &mut Context<Self>) -> AnyElement {
        let file = self.config_file_editor(kind);
        let editor = file.editor.clone();
        let path = file.path.as_ref().map_or_else(
            || kind.file_name().to_owned(),
            |path| path.display().to_string(),
        );
        let dirty = file.editor.read(cx).value().as_ref() != file.saved;
        let error = file.error.clone();
        let donor = kind.donor_path();
        let donor_tooltip = donor.as_ref().map_or_else(
            || format!("No {} configuration was found to import", kind.donor_name()),
            |path| format!("Import {}", path.display()),
        );
        let has_config_import = crate::profile::profile(cx).has_config_import;

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .p(px(14.0))
            .child(
                settings_page_content()
                    .flex_1()
                    .min_h_0()
                    .gap(px(10.0))
                    .child(settings_page_description(kind.section(), cx))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(zz_ui::rems_from_px(11.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .text_ellipsis()
                                    .child(path),
                            )
                            .when(has_config_import, |row| {
                                row.child(
                                    Button::new(kind.import_button_id())
                                        .small()
                                        .label(kind.import_button_label())
                                        .tooltip(donor_tooltip)
                                        .disabled(donor.is_none())
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.confirm_config_import(kind, window, cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new(kind.save_button_id())
                                    .small()
                                    .label("Save")
                                    .disabled(!dirty)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.save_config_editor(kind, cx);
                                    })),
                            ),
                    )
                    .when_some(error, |page, error| {
                        page.child(
                            div()
                                .flex_none()
                                .text_size(zz_ui::rems_from_px(11.0))
                                .text_color(cx.theme().warning)
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .p(px(CONFIG_EDITOR_PADDING))
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().editor_background())
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(px(CONFIG_EDITOR_FONT_SIZE))
                            .child(CodeEditor::new(&editor)),
                    ),
            )
            .into_any_element()
    }

    fn panes_section(&self, resolved: AppConfig, cx: &Context<Self>) -> AnyElement {
        let gaps = resolved.pane_gaps.value;
        Self::scroll_column("settings-panes")
            .child(settings_page_description(SettingsSection::Panes, cx))
            .child(SettingsStack::titled("Layout").child(Self::boolean_setting(
                ConfigKey::PaneGaps,
                "Pane gaps",
                "Separate panes with card-like spacing and chrome.",
                resolved.pane_gaps,
                cx,
            )))
            .child(SettingsStack::titled("Focus").child(Self::numeric_setting(
                ConfigKey::PaneInactiveOpacity,
                "Inactive pane opacity",
                "Visible strength of inactive pane content and chrome (0–1). Set to 1 to disable dimming.",
                resolved.pane_inactive_opacity,
                &self.pane_inactive_opacity,
                cx,
            )))
            .child(
                SettingsStack::titled("Frame")
                    .description("Applies only while pane gaps are enabled.")
                    .child(
                        Self::numeric_setting(
                            ConfigKey::PaneMargin,
                            "Pane margin",
                            "Space around each pane on all platforms, in logical pixels (0–32).",
                            resolved.pane_margin,
                            &self.pane_margin,
                            cx,
                        )
                        .disabled(!gaps),
                    )
                    .child(
                        Self::numeric_setting(
                            ConfigKey::PaneCornerRadius,
                            "Pane corner radius",
                            "Rounds every pane corner on all platforms, in logical pixels (0–32).",
                            resolved.pane_corner_radius,
                            &self.pane_corner_radius,
                            cx,
                        )
                        .disabled(!gaps),
                    )
                    .child(
                        Self::numeric_setting(
                            ConfigKey::PaneBorderWidth,
                            "Pane border width",
                            "Border width for gapped panes, in logical pixels (0–8). Set to 0 to \
                             disable.",
                            resolved.pane_border_width,
                            &self.pane_border_width,
                            cx,
                        )
                        .disabled(!gaps),
                    ),
            )
            .into_any_element()
    }

    fn editor_section(&self, resolved: AppConfig, cx: &Context<Self>) -> AnyElement {
        Self::scroll_column("settings-editor")
            .child(settings_page_description(SettingsSection::Editor, cx))
            .child(
                SettingsStack::titled("Typography")
                    .description(
                        "The editor inherits the terminal's mono font family; only the size is \
                         its own.",
                    )
                    .child(Self::numeric_setting(
                        ConfigKey::EditorFontSize,
                        "Font size",
                        "Editor pane type size in logical pixels (8–32).",
                        resolved.editor_font_size,
                        &self.editor_font_size,
                        cx,
                    )),
            )
            .child(
                SettingsStack::titled("Display")
                    .child(Self::boolean_setting(
                        ConfigKey::EditorLineNumbers,
                        "Line numbers",
                        "Show a line-number gutter beside the buffer.",
                        resolved.editor_line_numbers,
                        cx,
                    ))
                    .child(Self::boolean_setting(
                        ConfigKey::EditorRelativeLineNumbers,
                        "Relative line numbers",
                        "Number the gutter by distance from the cursor line, which keeps its own \
                         absolute number.",
                        resolved.editor_relative_line_numbers,
                        cx,
                    ))
                    .child(Self::boolean_setting(
                        ConfigKey::EditorSoftWrap,
                        "Soft wrap",
                        "Wrap long lines at the pane edge instead of scrolling horizontally.",
                        resolved.editor_soft_wrap,
                        cx,
                    ))
                    .child(Self::boolean_setting(
                        ConfigKey::EditorVimMode,
                        "Vim mode",
                        "Modal editing in the editor pane: normal, insert and visual modes, with \
                         vim's motions, operators and text objects.",
                        resolved.editor_vim_mode,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn config_file_editor(&self, kind: ConfigFileKind) -> &ConfigFileEditor {
        let editor = match kind {
            ConfigFileKind::Mux => self.mux_config_editor.as_ref(),
            ConfigFileKind::Terminal => self.terminal_config_editor.as_ref(),
        };
        editor.expect("the editor is initialized before rendering or updating its section")
    }

    fn config_file_editor_mut(&mut self, kind: ConfigFileKind) -> &mut ConfigFileEditor {
        let editor = match kind {
            ConfigFileKind::Mux => self.mux_config_editor.as_mut(),
            ConfigFileKind::Terminal => self.terminal_config_editor.as_mut(),
        };
        editor.expect("the editor is initialized before rendering or updating its section")
    }

    fn reload_config_editor_if_clean(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = match section {
            SettingsSection::Multiplexer => ConfigFileKind::Mux,
            SettingsSection::Terminal => ConfigFileKind::Terminal,
            _ => return,
        };
        let file = self.config_file_editor(kind);
        if file.editor.read(cx).value().as_ref() == file.saved {
            self.reload_config_editor(kind, window, cx);
        }
    }

    fn reload_config_editor(
        &mut self,
        kind: ConfigFileKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self
            .config_file_editor(kind)
            .path
            .clone()
            .map_or_else(|| config_file_path(kind), Ok);
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                self.config_file_editor_mut(kind).error =
                    Some(format!("Could not locate {}: {error}", kind.file_name()));
                cx.notify();
                return;
            }
        };
        match config::read_config_editor_source(&path, kind.max_bytes()) {
            Ok(source) => {
                let source = kind.editor_view(source);
                let editor = self.config_file_editor(kind).editor.clone();
                editor.update(cx, |editor, cx| editor.set_value(&source, window, cx));
                let file = self.config_file_editor_mut(kind);
                file.path = Some(path);
                file.saved = source;
                file.error = None;
            }
            Err(error) => {
                let file = self.config_file_editor_mut(kind);
                file.path = Some(path);
                file.error = Some(format!("Could not read {}: {error}", kind.file_name()));
            }
        }
        cx.notify();
    }

    fn save_config_editor(&mut self, kind: ConfigFileKind, cx: &mut Context<Self>) {
        let source = self
            .config_file_editor(kind)
            .editor
            .read(cx)
            .value()
            .to_string();
        let path = self
            .config_file_editor(kind)
            .path
            .clone()
            .map_or_else(|| config_file_path(kind), Ok);
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                self.config_file_editor_mut(kind).error =
                    Some(format!("Could not locate {}: {error}", kind.file_name()));
                toast::push(
                    Notification::error(format!("Could not save {}: {error}", kind.file_name())),
                    cx,
                );
                cx.notify();
                return;
            }
        };
        let written = match kind {
            ConfigFileKind::Mux => {
                config::write_config_editor_source(&path, &source, kind.max_bytes())
            }
            ConfigFileKind::Terminal => config::save_appearance_editor(&path, &source),
        };
        if let Err(error) = written {
            let file = self.config_file_editor_mut(kind);
            file.path = Some(path);
            file.error = Some(format!("Could not save {}: {error}", kind.file_name()));
            toast::push(
                Notification::error(format!("Could not save {}: {error}", kind.file_name())),
                cx,
            );
            cx.notify();
            return;
        }

        let file = self.config_file_editor_mut(kind);
        file.path = Some(path.clone());
        file.saved = source;
        file.error = None;
        config::request_daemon_reload(cx);
        toast::push(
            Notification::success(format!("Saved {}", path.display())),
            cx,
        );
        cx.notify();
    }

    fn confirm_config_import(
        &mut self,
        kind: ConfigFileKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(donor) = kind.donor_path() else {
            toast::push(
                Notification::info(format!(
                    "No {} configuration was found to import",
                    kind.donor_name()
                )),
                cx,
            );
            return;
        };
        let target = match config_file_path(kind) {
            Ok(path) => path,
            Err(error) => {
                toast::push(
                    Notification::error(format!("Could not locate {}: {error}", kind.file_name())),
                    cx,
                );
                return;
            }
        };
        let mut description = kind.import_description(&donor, &target);
        let file = self.config_file_editor(kind);
        if file.editor.read(cx).value().as_ref() != file.saved {
            description.push_str(" Unsaved changes in this editor will be discarded.");
        }
        let settings = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let settings = settings.clone();
            import_configuration_file_alert(alert, kind.import_title(), description.clone()).on_ok(
                move |_, window, cx| {
                    settings.update(cx, |settings, cx| {
                        settings.run_config_import(kind, window, cx);
                    });
                    true
                },
            )
        });
    }

    fn run_config_import(
        &mut self,
        kind: ConfigFileKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = match kind {
            ConfigFileKind::Mux => {
                config::import::import_tmux_config().map(|report| report.mux_path)
            }
            ConfigFileKind::Terminal => {
                config::import::import_ghostty_config(import_color_scheme(cx))
                    .map(|report| report.config_path)
            }
        };
        match result {
            Ok(Some(path)) => {
                log::info!(
                    target: "zz::config",
                    "imported {} configuration into {}",
                    kind.donor_name(),
                    path.display(),
                );
                self.reload_config_editor(kind, window, cx);
                config::request_daemon_reload(cx);
                toast::push(
                    Notification::success(format!(
                        "Imported {} configuration into {}",
                        kind.donor_name(),
                        path.display()
                    )),
                    cx,
                );
            }
            Ok(None) => {
                toast::push(
                    Notification::info(format!(
                        "No {} configuration was found to import",
                        kind.donor_name()
                    )),
                    cx,
                );
            }
            Err(error) => {
                log::warn!(
                    target: "zz::config",
                    "could not import {} configuration error={error}",
                    kind.donor_name(),
                );
                toast::push(
                    Notification::error(format!(
                        "Could not import {} configuration: {error}",
                        kind.donor_name()
                    )),
                    cx,
                );
            }
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = self.ensure_section_state(self.section, window, cx);
        let resolved = self.synchronize_numeric_inputs(window, cx);
        let browser = self.synchronize_browser_input(window, cx);
        self.synchronize_ui_zoom_input(window, cx);
        self.synchronize_terminal_editor(window, cx);

        let content = match self.section {
            SettingsSection::Appearance => self.appearance_section(resolved, cx),
            SettingsSection::Browser => self.browser_section(browser, resolved, cx),
            SettingsSection::Editor => self.editor_section(resolved, cx),
            SettingsSection::Panes => self.panes_section(resolved, cx),
            SettingsSection::Hosts => self.hosts_section(cx),
            SettingsSection::Advanced => Self::advanced_section(resolved, cx),
            SettingsSection::Terminal => self.config_file_section(ConfigFileKind::Terminal, cx),
            SettingsSection::Multiplexer => self.config_file_section(ConfigFileKind::Mux, cx),
            SettingsSection::About => Self::about_section(cx),
        };
        div()
            .id("settings-route")
            .track_focus(&self.focus_handle)
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(crate::theme::chrome_background(cx))
            .text_color(cx.theme().foreground)
            .child(content)
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus()
    }
}

fn settings_select_state(
    choices: &[(&str, &str)],
    selected: &str,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<SelectState<Vec<SettingsSelectItem>>> {
    let selected_index = choices
        .iter()
        .position(|(_, value)| *value == selected)
        .map(|index| IndexPath::default().row(index));
    let items = choices
        .iter()
        .map(|(label, value)| SettingsSelectItem::new(*label, *value))
        .collect();
    cx.new(|cx| SelectState::new(items, selected_index, window, cx))
}

impl ConfigFileKind {
    const fn section(self) -> SettingsSection {
        match self {
            Self::Mux => SettingsSection::Multiplexer,
            Self::Terminal => SettingsSection::Terminal,
        }
    }

    const fn file_name(self) -> &'static str {
        match self {
            Self::Mux => "zz/mux.conf",
            Self::Terminal => "zz/config",
        }
    }

    const fn donor_name(self) -> &'static str {
        match self {
            Self::Mux => "tmux",
            Self::Terminal => "Ghostty",
        }
    }

    const fn import_title(self) -> &'static str {
        match self {
            Self::Mux => "Import from tmux?",
            Self::Terminal => "Import from Ghostty?",
        }
    }

    const fn import_button_label(self) -> &'static str {
        match self {
            Self::Mux => "Import tmux…",
            Self::Terminal => "Import Ghostty…",
        }
    }

    const fn import_button_id(self) -> &'static str {
        match self {
            Self::Mux => "settings-import-tmux",
            Self::Terminal => "settings-import-ghostty",
        }
    }

    const fn save_button_id(self) -> &'static str {
        match self {
            Self::Mux => "settings-save-mux-config",
            Self::Terminal => "settings-save-terminal-config",
        }
    }

    const fn placeholder(self) -> &'static str {
        match self {
            Self::Mux => "# zz multiplexer configuration",
            Self::Terminal => "# terminal appearance, in Ghostty's key = value dialect",
        }
    }

    const fn language(self) -> &'static str {
        match self {
            Self::Mux => "tmux",
            Self::Terminal => "text",
        }
    }

    const fn max_bytes(self) -> usize {
        match self {
            Self::Mux => config::import::MAX_MUX_CONFIG_BYTES,
            Self::Terminal => config::MAX_CONFIG_BYTES,
        }
    }

    fn editor_view(self, source: String) -> String {
        match self {
            Self::Mux => source,
            Self::Terminal => config::appearance_editor_view(&source),
        }
    }

    fn donor_path(self) -> Option<PathBuf> {
        match self {
            Self::Mux => config::import::discover_tmux_config(),
            Self::Terminal => discover_ghostty_config(),
        }
    }

    fn import_description(self, donor: &Path, target: &Path) -> String {
        match self {
            Self::Mux => format!(
                "This replaces {} with a verbatim copy of {}. The tmux file is not modified.",
                target.display(),
                donor.display(),
            ),
            Self::Terminal => format!(
                "This rewrites the appearance keys in {} from {}, replacing any you changed \
                 since the last import. The Ghostty file is not modified.",
                target.display(),
                donor.display(),
            ),
        }
    }
}

fn config_file_editor(
    kind: ConfigFileKind,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> ConfigFileEditor {
    let (path, saved, error) = match config_file_path(kind) {
        Ok(path) => match config::read_config_editor_source(&path, kind.max_bytes()) {
            Ok(source) => (Some(path), kind.editor_view(source), None),
            Err(error) => (
                Some(path),
                String::new(),
                Some(format!("Could not read {}: {error}", kind.file_name())),
            ),
        },
        Err(error) => (
            None,
            String::new(),
            Some(format!("Could not locate {}: {error}", kind.file_name())),
        ),
    };
    let editor = cx.new(|cx| {
        let mut editor = CodeEditorState::new(window, cx)
            .language(kind.language())
            .soft_wrap(false)
            .placeholder(kind.placeholder())
            .default_value(&saved);
        editor.set_line_numbers(false, cx);
        editor
    });
    ConfigFileEditor {
        path,
        editor,
        saved,
        error,
    }
}

fn config_editor_subscription(
    editor: &Entity<CodeEditorState>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Subscription {
    cx.subscribe_in(
        editor,
        window,
        move |_, _, event: &CodeEditorEvent, _, cx| {
            if matches!(event, CodeEditorEvent::Change) {
                cx.notify();
            }
        },
    )
}

fn config_file_path(kind: ConfigFileKind) -> io::Result<PathBuf> {
    match kind {
        ConfigFileKind::Mux => zz_daemon::mux_config_write_path().ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "cannot create zz/mux.conf because neither XDG_CONFIG_HOME nor HOME is available",
            )
        }),
        ConfigFileKind::Terminal => config::import_target_path(),
    }
}

fn text_value_input(
    value: &str,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}

fn browser_hotkey_subscription(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        |settings, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                settings.commit_browser_hotkey(input, window, cx);
            }
        },
    )
}

fn search_provider_select(
    selected: SearchProvider,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<SelectState<Vec<SettingsSelectItem>>> {
    let choices: Vec<(&str, &str)> = SearchProvider::ALL
        .into_iter()
        .map(|provider| (provider.title(), provider.as_str()))
        .collect();
    settings_select_state(&choices, selected.as_str(), window, cx)
}

fn search_provider_subscription(
    select: &Entity<SelectState<Vec<SettingsSelectItem>>>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Subscription {
    cx.subscribe_in(
        select,
        window,
        |_, _, event: &SelectEvent<Vec<SettingsSelectItem>>, _, cx| {
            let SelectEvent::Confirm(Some(provider)) = event else {
                return;
            };
            if let Err(error) = set_config_key(ConfigKey::BrowserSearchProvider, provider) {
                report_write_error("set", ConfigKey::BrowserSearchProvider.as_str(), &error, cx);
            }
        },
    )
}

fn numeric_value_input(
    key: ConfigKey,
    value: f32,
    step: f64,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<InputState> {
    let (min, max) = numeric_range(key);
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value.to_string())
            .step(step)
            .min(f64::from(min))
            .max(f64::from(max))
    })
}

fn ui_zoom_input(window: &mut Window, cx: &mut Context<SettingsView>) -> Entity<InputState> {
    let percent = crate::ui_scale::percent(cx);
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(percent.to_string())
            .step(f64::from(crate::ui_scale::UI_ZOOM_STEP))
            .min(f64::from(crate::ui_scale::MIN_UI_ZOOM))
            .max(f64::from(crate::ui_scale::MAX_UI_ZOOM))
    })
}

fn ui_zoom_subscription(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        move |_, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => SettingsView::preview_ui_zoom(input, cx),
            InputEvent::PressEnter { .. } | InputEvent::Blur => {
                SettingsView::commit_ui_zoom_input(input, window, cx);
            }
            _ => {}
        },
    )
}

fn numeric_input_subscription(
    input: &Entity<InputState>,
    key: ConfigKey,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        move |settings, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                settings.commit_numeric_input(key, input, window, cx);
            }
        },
    )
}

fn synchronize_f32_input(
    input: &Entity<InputState>,
    value: f32,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    let value = value.to_string();
    if input.read(cx).value().as_ref() != value {
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }
}

fn synchronize_text_input(
    input: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    if input.read(cx).value().as_ref() != value {
        input.update(cx, |input, cx| {
            input.set_value(value.to_owned(), window, cx);
        });
    }
}

fn numeric_range(key: ConfigKey) -> (f32, f32) {
    key.numeric_range()
        .expect("every Settings numeric field edits a numeric key")
}

fn validate_numeric_value(key: ConfigKey, value: &str) -> Result<f32, String> {
    let (min, max) = numeric_range(key);
    let rejected = || format!("enter a number between {min} and {max}");
    let value = value.parse::<f32>().map_err(|_| rejected())?;
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(rejected());
    }
    Ok(value)
}

fn numeric_config_value(config: AppConfig, key: ConfigKey) -> f32 {
    match key {
        ConfigKey::PaneInactiveOpacity => config.pane_inactive_opacity.value,
        ConfigKey::PaneCornerRadius => config.pane_corner_radius.value,
        ConfigKey::PaneMargin => config.pane_margin.value,
        ConfigKey::PaneBorderWidth => config.pane_border_width.value,
        ConfigKey::WidgetCornerRadius => config.widget_corner_radius.value,
        ConfigKey::WindowCornerRadius => config.window_corner_radius.value,
        ConfigKey::EditorFontSize => config.editor_font_size.value,
        ConfigKey::UseSystemTitlebar
        | ConfigKey::WindowBackgroundBlur
        | ConfigKey::Animations
        | ConfigKey::Tray
        | ConfigKey::ShowFps
        | ConfigKey::QuitDaemonOnExit
        | ConfigKey::AutoRestartStaleDaemon
        | ConfigKey::ExperimentalAgentPane
        | ConfigKey::ExperimentalEditorPane
        | ConfigKey::PaneGaps
        | ConfigKey::EditorLineNumbers
        | ConfigKey::EditorRelativeLineNumbers
        | ConfigKey::EditorSoftWrap
        | ConfigKey::EditorVimMode
        | ConfigKey::BrowserElementSelectorHotkey
        | ConfigKey::BrowserSearchProvider
        | ConfigKey::BrowserEgress
        | ConfigKey::ThemeMode
        | ConfigKey::AppIcon
        | ConfigKey::ChromePreset
        | ConfigKey::Chrome(_) => {
            unreachable!("only numeric settings use numeric inputs")
        }
    }
}

fn chrome_pickers(
    observed: AppConfig,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> BTreeMap<ChromeColor, Entity<ColorPickerState>> {
    ChromeColor::ALL
        .into_iter()
        .map(|color| {
            let value = observed.chrome(color).value;
            (color, cx.new(|cx| ColorPickerState::new(value, window, cx)))
        })
        .collect()
}

fn chrome_picker_subscriptions(
    pickers: &BTreeMap<ChromeColor, Entity<ColorPickerState>>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Vec<Subscription> {
    pickers
        .iter()
        .map(|(color, picker)| {
            let color = *color;
            cx.subscribe_in(
                picker,
                window,
                move |_, _, event: &ColorPickerEvent, _, cx| {
                    let ColorPickerEvent::Change(value) = event;
                    write_chrome_color(color, *value, cx);
                },
            )
        })
        .collect()
}

fn write_chrome_color(color: ChromeColor, value: Option<gpui::Hsla>, cx: &mut App) {
    let key = ConfigKey::Chrome(color);
    let result = match value {
        Some(value) => set_config_key(key, &zz_ui::to_hex(value)),
        None => remove_config_key(key),
    };
    if let Err(error) = result {
        let action = if value.is_some() { "set" } else { "reset" };
        report_write_error(action, key.as_str(), &error, cx);
    }
}

fn apply_preset(preset: &'static ChromePreset, cx: &mut App) {
    if let Err(error) = set_chrome_preset(preset.id) {
        report_write_error("set", ConfigKey::ChromePreset.as_str(), &error, cx);
    }
}

fn preset_swatches(preset: &'static ChromePreset, cx: &App) -> gpui::Div {
    div().flex().flex_col().flex_none().gap(px(2.0)).children(
        [ThemeMode::Light, ThemeMode::Dark].map(|mode| {
            div()
                .flex()
                .gap(px(2.0))
                .children(preset.colors(mode).iter().map(|hex| {
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .bg(zz_ui::parse_hex(hex).unwrap_or(cx.theme().border))
                }))
        }),
    )
}

fn picker_tile(
    id: SharedString,
    label: &'static str,
    preview: impl IntoElement,
    selected: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(gpui::ElementId::Name(id))
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .child(
            div()
                .p(px(3.0))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(if selected {
                    cx.theme().foreground
                } else {
                    gpui::transparent_black()
                })
                .child(preview),
        )
        .child(
            div()
                .text_size(zz_ui::rems_from_px(11.0))
                .text_color(if selected {
                    cx.theme().foreground
                } else {
                    cx.theme().foreground.muted()
                })
                .child(label),
        )
}

fn theme_preview(mode: ThemeModeSetting, preset: Option<ChromePresetId>, cx: &App) -> gpui::Div {
    match mode {
        ThemeModeSetting::Light => theme_preview_window(ThemeMode::Light, preset, cx),
        ThemeModeSetting::Dark => theme_preview_window(ThemeMode::Dark, preset, cx),
        ThemeModeSetting::System => {
            let light = inherited_chrome_colors(preset, ThemeMode::Light);
            let dark = inherited_chrome_colors(preset, ThemeMode::Dark);
            let split = THEME_PREVIEW_WIDTH / 2.0;
            div()
                .relative()
                .w(px(THEME_PREVIEW_WIDTH))
                .h(px(THEME_PREVIEW_HEIGHT))
                .rounded(cx.theme().radius)
                .bg(theme_preview_split_background(
                    light.background.raised(2),
                    dark.background,
                    0.5,
                ))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(THEME_PREVIEW_SIDEBAR_WIDTH))
                        .h_full()
                        .w(px(split - THEME_PREVIEW_SIDEBAR_WIDTH))
                        .bg(light.background),
                )
                .child(theme_preview_contents(&light))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .h_full()
                        .w(px(split))
                        .overflow_hidden()
                        .child(theme_preview_contents(&dark).absolute().top_0().right_0()),
                )
        }
    }
}

fn theme_preview_window(mode: ThemeMode, preset: Option<ChromePresetId>, cx: &App) -> gpui::Div {
    let colors = inherited_chrome_colors(preset, mode);
    let sidebar_fraction = THEME_PREVIEW_SIDEBAR_WIDTH / THEME_PREVIEW_WIDTH;
    div()
        .w(px(THEME_PREVIEW_WIDTH))
        .h(px(THEME_PREVIEW_HEIGHT))
        .rounded(cx.theme().radius)
        .bg(theme_preview_split_background(
            colors.background.raised(2),
            colors.background,
            sidebar_fraction,
        ))
        .child(theme_preview_contents(&colors))
}

fn theme_preview_contents(colors: &ThemeColor) -> gpui::Div {
    let text = colors.foreground.muted();
    let text_bar = move |width: f32| div().w(px(width)).h(px(3.0)).rounded_full().bg(text);
    div()
        .flex()
        .w(px(THEME_PREVIEW_WIDTH))
        .h(px(THEME_PREVIEW_HEIGHT))
        .child(
            div()
                .w(px(THEME_PREVIEW_SIDEBAR_WIDTH))
                .flex_none()
                .border_r_1()
                .border_color(colors.border),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(6.0))
                .p(px(7.0))
                .child(
                    div().flex().items_center().gap(px(3.0)).children(
                        [colors.danger, colors.warning, colors.success]
                            .map(|light| div().size(px(4.0)).rounded_full().bg(light)),
                    ),
                )
                .child(text_bar(36.0))
                .child(text_bar(22.0)),
        )
}

fn theme_preview_split_background(
    left: gpui::Hsla,
    right: gpui::Hsla,
    split: f32,
) -> gpui::Background {
    gpui::linear_gradient(
        90.0,
        gpui::linear_color_stop(left, split),
        gpui::linear_color_stop(right, split),
    )
}

fn about_hero(cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .pt(px(20.0))
        .pb(px(10.0))
        .child(img(crate::app_icon::about_logo(cx.theme().mode)).size(px(ABOUT_LOGO_SIZE)))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(5.0))
                .child(
                    zz_ui::StyledExt::font_medium(div().text_size(zz_ui::rems_from_px(22.0)))
                        .child("zz"),
                )
                .child(
                    div()
                        .text_size(zz_ui::rems_from_px(12.0))
                        .text_color(cx.theme().foreground.muted())
                        .child(SettingsSection::About.description()),
                )
                .child(settings_provenance_badge(concat!(
                    "v",
                    env!("CARGO_PKG_VERSION")
                ))),
        )
}

fn about_value(value: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(zz_ui::rems_from_px(11.0))
        .text_color(cx.theme().foreground.muted())
        .child(value.into())
}

fn link_button(id: &'static str, label: &'static str, url: &'static str, cx: &App) -> Button {
    Button::new(id)
        .small()
        .icon(zz_ui::IconName::ExternalLink)
        .label(label)
        .bg(settings_control_fill(cx))
        .on_click(move |_, _, cx| cx.open_url(url))
}

fn copy_build_info_button() -> Button {
    Button::new("settings-about-copy-build-info")
        .xsmall()
        .compact()
        .ghost()
        .icon(zz_ui::IconName::Copy)
        .tooltip("Copy build information")
        .on_click(|_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(build_info()));
            toast::push(Notification::success("Copied build information"), cx);
        })
}

fn build_info() -> String {
    format!(
        "zz {} ({} {}, gpui {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        gpui_revision(),
    )
}

fn platform() -> String {
    format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn gpui_revision() -> &'static str {
    let Some((_, revision)) = crate::GPUI_SOURCE.split_once('#') else {
        return crate::GPUI_SOURCE;
    };
    revision.get(..8).unwrap_or(revision)
}

fn provenance_badge(provenance: ConfigProvenance) -> Tag {
    settings_provenance_badge(match provenance {
        ConfigProvenance::Default => "Default",
        ConfigProvenance::Override => "Overridden",
    })
}

fn key_annotations(key: ConfigKey, provenance: ConfigProvenance) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .child(config_reset_button(key, provenance))
        .child(provenance_badge(provenance))
}

fn config_reset_button(key: ConfigKey, provenance: ConfigProvenance) -> Button {
    let tooltip = match provenance {
        ConfigProvenance::Default => "Already using the default value",
        ConfigProvenance::Override => "Reset to the inherited or default value",
    };
    settings_reset_button(
        format!("settings-{}-reset", key.as_str()),
        tooltip,
        provenance == ConfigProvenance::Override,
    )
    .on_click(move |_, _, cx| {
        if let Err(error) = remove_config_key(key) {
            report_write_error("reset", key.as_str(), &error, cx);
        }
    })
}

fn import_color_scheme(cx: &App) -> TerminalColorScheme {
    crate::theme::terminal_appearance(cx).map_or(TerminalColorScheme::Dark, |appearance| {
        appearance.color_scheme
    })
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn run_import(cx: &mut App) {
    match crate::config::import::import_external_config(import_color_scheme(cx)) {
        Ok(report) if report.imported_anything() => {
            log::info!(
                target: "zz::config",
                "imported external configuration ghostty_keys={} mux={}",
                report.ghostty_keys,
                report.mux_path.is_some(),
            );
            let mut copied = Vec::new();
            if let Some(path) = &report.config_path {
                copied.push(format!("Ghostty appearance into {}", path.display()));
            }
            if let Some(path) = &report.mux_path {
                copied.push(format!("tmux configuration into {}", path.display()));
            }
            toast::push(
                Notification::success(format!("Imported {}", copied.join(" and "))),
                cx,
            );
            config::request_daemon_reload(cx);
        }
        Ok(_) => {
            toast::push(
                Notification::info("Nothing to import: no Ghostty or tmux configuration found"),
                cx,
            );
        }
        Err(error) => {
            log::warn!(
                target: "zz::config",
                "could not import external configuration error={error}",
            );
            toast::push(
                Notification::error(format!("Could not import configuration: {error}")),
                cx,
            );
        }
    }
}

fn report_write_error(operation: &str, key: &str, error: &std::io::Error, cx: &mut App) {
    log::warn!(
        target: "zz::config",
        "could not {operation} configuration key={key} error={error}",
    );
    toast::push(
        Notification::error(format!("Could not update {key}: {error}")),
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_daemon::{DaemonError, Endpoint};

    #[test]
    fn animations_are_the_first_interface_tweak() {
        let items = appearance_page_items(true);
        let tweaks = items
            .iter()
            .position(|item| {
                matches!(
                    item,
                    AppearancePageItem::Group {
                        title: "Tweaks",
                        ..
                    }
                )
            })
            .expect("Tweaks group");

        assert!(matches!(
            items.get(tweaks + 1),
            Some(AppearancePageItem::Animations)
        ));
    }

    #[gpui::test]
    fn heavy_section_state_is_initialized_on_first_visit(cx: &mut gpui::TestAppContext) {
        use std::{cell::RefCell, rc::Rc};

        cx.update(zz_ui::init);
        let captured = Rc::new(RefCell::new(None));
        let captured_for_window = Rc::clone(&captured);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            captured_for_window.replace(Some(cx.entity()));
            SettingsView::new(mux, window, cx)
        });
        let settings = captured.borrow().clone().expect("captured settings view");

        settings.read_with(cx, |settings, _| {
            assert!(settings.terminal_config_editor.is_none());
            assert!(settings.mux_config_editor.is_none());
            assert!(settings.hosts_state.is_none());
        });

        let initialized = cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                settings.ensure_section_state(SettingsSection::Hosts, window, cx)
            })
        });
        assert!(initialized);
        settings.read_with(cx, |settings, _| {
            assert!(settings.hosts_state.is_some());
            assert!(settings.terminal_config_editor.is_none());
            assert!(settings.mux_config_editor.is_none());
        });

        let initialized = cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                settings.ensure_section_state(SettingsSection::Hosts, window, cx)
            })
        });
        assert!(!initialized);

        let initialized = cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                settings.ensure_section_state(SettingsSection::Terminal, window, cx)
            })
        });
        assert!(initialized);
        settings.read_with(cx, |settings, _| {
            assert!(settings.terminal_config_editor.is_some());
            assert!(settings.mux_config_editor.is_none());
        });

        let initialized = cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                settings.ensure_section_state(SettingsSection::Multiplexer, window, cx)
            })
        });
        assert!(initialized);
        settings.read_with(cx, |settings, _| {
            assert!(settings.terminal_config_editor.is_some());
            assert!(settings.mux_config_editor.is_some());
        });

        let initialized = cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                settings.ensure_section_state(SettingsSection::Multiplexer, window, cx)
            })
        });
        assert!(!initialized);
    }

    #[gpui::test]
    fn submit_add_host_rejects_a_duplicate_without_changing_config(cx: &mut gpui::TestAppContext) {
        use std::{cell::RefCell, rc::Rc};

        cx.update(zz_ui::init);
        let configured = config::HostEntry {
            name: "desktop".to_owned(),
            endpoint: Endpoint::parse("ssh://desktop").expect("valid test endpoint"),
        };
        cx.update(|cx| config::set_fleet_hosts_for_test(vec![configured.clone()], cx));
        let captured = Rc::new(RefCell::new(None));
        let captured_for_window = Rc::clone(&captured);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            captured_for_window.replace(Some(cx.entity()));
            SettingsView::new(mux, window, cx)
        });
        let settings = captured.borrow().clone().expect("captured settings view");

        cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                assert!(settings.ensure_section_state(SettingsSection::Hosts, window, cx));
                let input = settings
                    .hosts_state
                    .as_ref()
                    .expect("initialized Hosts state")
                    .input
                    .clone();
                input.update(cx, |input, cx| {
                    input.set_value("fabrico@desktop", window, cx);
                });
                settings.submit_add_host(window, cx);
            });
        });

        settings.read_with(cx, |settings, _| {
            assert_eq!(
                settings
                    .hosts_state
                    .as_ref()
                    .and_then(|state| state.error.as_deref()),
                Some("A host named `desktop` already exists."),
            );
        });
        cx.update(|_, cx| {
            assert_eq!(config::fleet_hosts(cx), vec![configured]);
        });
    }

    #[gpui::test]
    fn the_zoom_field_applies_complete_values_and_ignores_partial_ones(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::{cell::RefCell, rc::Rc};

        cx.update(zz_ui::init);
        let captured = Rc::new(RefCell::new(None));
        let captured_for_window = Rc::clone(&captured);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            captured_for_window.replace(Some(cx.entity()));
            SettingsView::new(mux, window, cx)
        });
        let settings = captured.borrow().clone().expect("captured settings view");
        let input = settings.read_with(cx, |settings, _| settings.ui_zoom.clone());

        let typed = |value: &str, cx: &mut gpui::VisualTestContext| {
            let value = value.to_owned();
            cx.update(|window, cx| {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
                settings.update(cx, |settings, cx| {
                    SettingsView::preview_ui_zoom(&input, cx);
                    settings.synchronize_ui_zoom_input(window, cx);
                    (crate::ui_scale::percent(cx), input.read(cx).value().clone())
                })
            })
        };

        assert_eq!(
            typed("1", cx).0,
            100,
            "a prefix must not zoom to the minimum"
        );
        assert_eq!(typed("15", cx), (100, "15".into()), "nor be rewritten");
        assert_eq!(typed("150", cx), (150, "150".into()));
        assert_eq!(typed("", cx), (150, "".into()), "clearing must not zoom");
        assert_eq!(
            typed("4000", cx),
            (150, "4000".into()),
            "out of range is left to the commit"
        );

        cx.update(|window, cx| {
            settings.update(cx, |_, cx| {
                SettingsView::commit_ui_zoom_input(&input, window, cx);
            });
        });
        cx.update(|_, cx| {
            let percent = crate::ui_scale::percent(cx);
            assert!(percent < 4000, "the commit clamps what the field holds");
            assert_eq!(input.read(cx).value().as_ref(), percent.to_string());
        });

        typed("250", cx);
        cx.update(|window, cx| {
            crate::ui_scale::reset(cx);
            settings.update(cx, |settings, cx| {
                settings.synchronize_ui_zoom_input(window, cx);
            });
        });
        cx.update(|_, cx| {
            assert_eq!(crate::ui_scale::percent(cx), 100);
            assert_eq!(input.read(cx).value().as_ref(), "100");
        });
    }

    #[test]
    fn config_editor_source_round_trips_and_preserves_file_on_oversize_save() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config");
        config::write_config_editor_source(&path, "theme-mode = dark\n", 64)
            .expect("initial write");
        assert_eq!(
            config::read_config_editor_source(&path, 64).expect("read source"),
            "theme-mode = dark\n"
        );

        let error = config::write_config_editor_source(&path, "too large", 4)
            .expect_err("oversized editor content must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(
            std::fs::read_to_string(path).expect("read unchanged file"),
            "theme-mode = dark\n"
        );
    }

    #[test]
    fn missing_config_editor_source_starts_empty() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert_eq!(
            config::read_config_editor_source(&directory.path().join("missing"), 64)
                .expect("missing source"),
            ""
        );
    }
}
