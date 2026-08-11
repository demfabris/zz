//! PARKED (not compiled): the iPad-owned, full-screen Settings presentation.

use std::{
    collections::BTreeMap,
    io::{self, ErrorKind},
    path::PathBuf,
};

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use zz::engine::{
    config::{self, AppConfig, ConfigKey, ConfigProvenance, ConfigValue, MAX_CONFIG_BYTES},
    theme::{CHROME_PRESETS, ChromeColor, ChromePreset, ThemeModeSetting, inherited_chrome_colors},
    ui_scale,
    workspace::WorkspaceSidebar,
};
use zz::{AppProfile, SettingsSection};
use zz_protocol::ConfigOverrideEntry;
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _, UiZoom,
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    code_editor::{CodeEditor, CodeEditorEvent, CodeEditorState},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{InputEvent, InputState, NumberInput},
    notification::Notification,
    settings::settings_list_group_header,
    switch::Switch,
};

const HEADER_HEIGHT: f32 = 44.0;
const HEADER_SIDE_WIDTH: f32 = 132.0;
const ROW_MIN_HEIGHT: f32 = 56.0;
const PAGE_PADDING: f32 = 12.0;
const GROUP_GAP: f32 = 14.0;
const MUX_CONFIG_MAX_BYTES: usize = 1024 * 1024;
const CODE_EDITOR_FONT_SIZE: f32 = 12.0;

const REPOSITORY_URL: &str = "https://github.com/demfabris/zz";
const RELEASES_URL: &str = "https://github.com/demfabris/zz/releases";
const ISSUES_URL: &str = "https://github.com/demfabris/zz/issues/new";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Page {
    Root,
    Section(SettingsSection),
    Editor(EditorKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditorKind {
    Terminal,
    Multiplexer,
}

impl EditorKind {
    const fn section(self) -> SettingsSection {
        match self {
            Self::Terminal => SettingsSection::Terminal,
            Self::Multiplexer => SettingsSection::Multiplexer,
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Multiplexer => "Multiplexer",
        }
    }

    const fn file_name(self) -> &'static str {
        match self {
            Self::Terminal => "zz/config",
            Self::Multiplexer => "zz/mux.conf",
        }
    }

    const fn language(self) -> &'static str {
        match self {
            Self::Terminal => "text",
            Self::Multiplexer => "tmux",
        }
    }

    const fn placeholder(self) -> &'static str {
        match self {
            Self::Terminal => "# terminal appearance, in Ghostty's key = value dialect",
            Self::Multiplexer => "# zz multiplexer configuration",
        }
    }

    const fn max_bytes(self) -> usize {
        match self {
            Self::Terminal => MAX_CONFIG_BYTES,
            Self::Multiplexer => MUX_CONFIG_MAX_BYTES,
        }
    }

    fn editor_view(self, source: String) -> String {
        match self {
            Self::Terminal => config::appearance_editor_view(&source),
            Self::Multiplexer => source,
        }
    }
}

struct ConfigFileEditor {
    path: Option<PathBuf>,
    editor: Entity<CodeEditorState>,
    saved: String,
    error: Option<String>,
}

pub(super) struct IosSettings {
    sidebar: Entity<WorkspaceSidebar>,
    page: Page,
    observed: AppConfig,
    observed_zoom: u32,
    observed_appearance_overrides: Vec<ConfigOverrideEntry>,
    ui_zoom: Entity<InputState>,
    widget_corner_radius: Entity<InputState>,
    pane_margin: Entity<InputState>,
    pane_corner_radius: Entity<InputState>,
    pane_border_width: Entity<InputState>,
    chrome_pickers: BTreeMap<ChromeColor, Entity<ColorPickerState>>,
    terminal_editor: Option<ConfigFileEditor>,
    mux_editor: Option<ConfigFileEditor>,
    _subscriptions: Vec<Subscription>,
}

impl IosSettings {
    pub(super) fn new(
        sidebar: Entity<WorkspaceSidebar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observed = config::resolved_config(cx);
        let ui_zoom = zoom_input(window, cx);
        let widget_corner_radius = geometry_input(
            ConfigKey::WidgetCornerRadius,
            observed.widget_corner_radius.value,
            window,
            cx,
        );
        let pane_margin = geometry_input(
            ConfigKey::PaneMargin,
            observed.pane_margin.value,
            window,
            cx,
        );
        let pane_corner_radius = geometry_input(
            ConfigKey::PaneCornerRadius,
            observed.pane_corner_radius.value,
            window,
            cx,
        );
        let pane_border_width = geometry_input(
            ConfigKey::PaneBorderWidth,
            observed.pane_border_width.value,
            window,
            cx,
        );
        let chrome_pickers = ChromeColor::ALL
            .into_iter()
            .map(|color| {
                let value = observed.chrome(color).value;
                (color, cx.new(|cx| ColorPickerState::new(value, window, cx)))
            })
            .collect::<BTreeMap<_, _>>();

        let mut subscriptions = vec![
            zoom_subscription(&ui_zoom, window, cx),
            geometry_subscription(
                &widget_corner_radius,
                ConfigKey::WidgetCornerRadius,
                window,
                cx,
            ),
            geometry_subscription(&pane_margin, ConfigKey::PaneMargin, window, cx),
            geometry_subscription(&pane_corner_radius, ConfigKey::PaneCornerRadius, window, cx),
            geometry_subscription(&pane_border_width, ConfigKey::PaneBorderWidth, window, cx),
            cx.observe_global::<AppConfig>(|_, cx| cx.notify()),
            cx.observe_global::<UiZoom>(|_, cx| cx.notify()),
        ];
        subscriptions.extend(chrome_pickers.iter().map(|(color, picker)| {
            let color = *color;
            cx.subscribe_in(
                picker,
                window,
                move |_, _, event: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(value) = event;
                    write_chrome_color(color, *value, window, cx);
                },
            )
        }));

        Self {
            sidebar,
            page: Page::Root,
            observed,
            observed_zoom: ui_scale::percent(cx),
            observed_appearance_overrides: config::daemon_config_overrides(cx),
            ui_zoom,
            widget_corner_radius,
            pane_margin,
            pane_corner_radius,
            pane_border_width,
            chrome_pickers,
            terminal_editor: None,
            mux_editor: None,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_current_page(window, cx);
        self.page = Page::Root;
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
        cx.notify();
    }

    fn show_section(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_current_page(window, cx);
        self.page = Page::Section(section);
        cx.notify();
    }

    fn show_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_current_page(window, cx);
        self.page = Page::Root;
        cx.notify();
    }

    fn show_editor(&mut self, kind: EditorKind, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_current_page(window, cx);
        self.ensure_editor(kind, window, cx);
        self.reload_editor_if_clean(kind, window, cx);
        self.page = Page::Editor(kind);
        cx.notify();
    }

    fn show_editor_section(&mut self, kind: EditorKind, cx: &mut Context<Self>) {
        self.page = Page::Section(kind.section());
        cx.notify();
    }

    fn commit_current_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Page::Section(section) = self.page else {
            return;
        };
        match section {
            SettingsSection::Appearance => {
                let input = self.widget_corner_radius.clone();
                self.commit_geometry(ConfigKey::WidgetCornerRadius, &input, window, cx);
                self.commit_zoom(window, cx);
            }
            SettingsSection::Panes => {
                for (key, input) in [
                    (ConfigKey::PaneMargin, self.pane_margin.clone()),
                    (ConfigKey::PaneCornerRadius, self.pane_corner_radius.clone()),
                    (ConfigKey::PaneBorderWidth, self.pane_border_width.clone()),
                ] {
                    self.commit_geometry(key, &input, window, cx);
                }
            }
            _ => {}
        }
    }

    fn reconcile(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AppConfig {
        let resolved = config::resolved_config(cx);
        if resolved != self.observed {
            self.observed = resolved;
            synchronize_input(
                &self.widget_corner_radius,
                resolved.widget_corner_radius.value,
                window,
                cx,
            );
            synchronize_input(&self.pane_margin, resolved.pane_margin.value, window, cx);
            synchronize_input(
                &self.pane_corner_radius,
                resolved.pane_corner_radius.value,
                window,
                cx,
            );
            synchronize_input(
                &self.pane_border_width,
                resolved.pane_border_width.value,
                window,
                cx,
            );
            for (color, picker) in &self.chrome_pickers {
                picker.update(cx, |picker, cx| {
                    picker.set_color(resolved.chrome(*color).value, window, cx);
                });
            }
        }

        let zoom = ui_scale::percent(cx);
        if zoom != self.observed_zoom {
            self.observed_zoom = zoom;
            let shown = self.ui_zoom.read(cx).value();
            if !shown
                .parse::<f32>()
                .is_ok_and(|value| ui_scale::is_effective_percent(value, cx))
            {
                self.ui_zoom.update(cx, |input, cx| {
                    input.set_value(zoom.to_string(), window, cx);
                });
            }
        }

        let overrides = config::daemon_config_overrides(cx);
        if overrides != self.observed_appearance_overrides {
            self.observed_appearance_overrides = overrides;
            self.reload_editor_if_clean(EditorKind::Terminal, window, cx);
        }
        resolved
    }

    fn commit_geometry(
        &mut self,
        key: ConfigKey,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let candidate = input.read(cx).value().to_string();
        let value = match validate_geometry_value(key, &candidate) {
            Ok(value) => value,
            Err(message) => {
                window.push_notification(
                    Notification::error(format!("Invalid {}: {message}", key.as_str())),
                    cx,
                );
                synchronize_input(input, geometry_value(self.observed, key), window, cx);
                return;
            }
        };
        synchronize_input(input, value, window, cx);
        if let Err(error) = config::set_config_key(key, &value.to_string()) {
            report_write_error("set", key.as_str(), &error, window, cx);
        }
    }

    fn preview_zoom(&mut self, cx: &mut Context<Self>) {
        let Ok(value) = self.ui_zoom.read(cx).value().parse::<f32>() else {
            return;
        };
        if (ui_scale::MIN_UI_ZOOM..=ui_scale::MAX_UI_ZOOM).contains(&value) {
            ui_scale::set_percent(value, cx);
        }
    }

    fn commit_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let candidate = self.ui_zoom.read(cx).value().to_string();
        let rejected = || {
            format!(
                "enter a number between {} and {}",
                ui_scale::MIN_UI_ZOOM,
                ui_scale::MAX_UI_ZOOM
            )
        };
        let value = candidate.parse::<f32>().ok().filter(|value| {
            value.is_finite() && (ui_scale::MIN_UI_ZOOM..=ui_scale::MAX_UI_ZOOM).contains(value)
        });
        let Some(value) = value else {
            window.push_notification(
                Notification::error(format!("Invalid UI zoom: {}", rejected())),
                cx,
            );
            self.ui_zoom.update(cx, |input, cx| {
                input.set_value(self.observed_zoom.to_string(), window, cx);
            });
            return;
        };
        ui_scale::set_percent(value, cx);
        self.ui_zoom.update(cx, |input, cx| {
            input.set_value(ui_scale::percent(cx).to_string(), window, cx);
        });
    }

    fn ensure_editor(&mut self, kind: EditorKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor(kind).is_some() {
            return;
        }
        let (path, saved, error) = load_editor_source(kind);
        let editor = cx.new(|cx| {
            let mut editor = CodeEditorState::new(window, cx)
                .language(kind.language())
                .soft_wrap(false)
                .placeholder(kind.placeholder())
                .default_value(&saved);
            editor.set_line_numbers(false, cx);
            editor
        });
        self._subscriptions.push(cx.subscribe_in(
            &editor,
            window,
            |_, _, event: &CodeEditorEvent, _, cx| {
                if matches!(event, CodeEditorEvent::Change) {
                    cx.notify();
                }
            },
        ));
        *self.editor_slot_mut(kind) = Some(ConfigFileEditor {
            path,
            editor,
            saved,
            error,
        });
    }

    fn reload_editor_if_clean(
        &mut self,
        kind: EditorKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.editor(kind) else {
            return;
        };
        if file.editor.read(cx).value().as_ref() != file.saved {
            return;
        }
        let (path, saved, error) = load_editor_source(kind);
        let file = self.editor_mut(kind).expect("checked above");
        file.editor
            .update(cx, |editor, cx| editor.set_value(&saved, window, cx));
        file.path = path;
        file.saved = saved;
        file.error = error;
    }

    fn save_editor(&mut self, kind: EditorKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.editor(kind) else {
            return;
        };
        let source = file.editor.read(cx).value().to_string();
        let path = file.path.clone().map_or_else(|| editor_path(kind), Ok);
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                let message = format!("Could not locate {}: {error}", kind.file_name());
                self.editor_mut(kind).expect("editor exists").error = Some(message.clone());
                window.push_notification(Notification::error(message), cx);
                cx.notify();
                return;
            }
        };
        let result = match kind {
            EditorKind::Terminal => config::save_appearance_editor(&path, &source),
            EditorKind::Multiplexer => {
                config::write_config_editor_source(&path, &source, kind.max_bytes())
            }
        };
        if let Err(error) = result {
            let message = error.to_string();
            let file = self.editor_mut(kind).expect("editor exists");
            file.path = Some(path);
            file.error = Some(message.clone());
            window.push_notification(Notification::error(message), cx);
            cx.notify();
            return;
        }

        let file = self.editor_mut(kind).expect("editor exists");
        file.path = Some(path.clone());
        file.saved = source;
        file.error = None;
        config::request_daemon_reload(cx);
        window.push_notification(
            Notification::success(format!("Saved {}", path.display())),
            cx,
        );
        cx.notify();
    }

    fn editor_slot_mut(&mut self, kind: EditorKind) -> &mut Option<ConfigFileEditor> {
        match kind {
            EditorKind::Terminal => &mut self.terminal_editor,
            EditorKind::Multiplexer => &mut self.mux_editor,
        }
    }

    fn editor(&self, kind: EditorKind) -> Option<&ConfigFileEditor> {
        match kind {
            EditorKind::Terminal => self.terminal_editor.as_ref(),
            EditorKind::Multiplexer => self.mux_editor.as_ref(),
        }
    }

    fn editor_mut(&mut self, kind: EditorKind) -> Option<&mut ConfigFileEditor> {
        self.editor_slot_mut(kind).as_mut()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let settings = cx.entity().clone();
        let (leading, title, trailing) = match self.page {
            Page::Root => {
                let done = Button::new("ios-settings-done")
                    .ghost()
                    .with_size(px(HEADER_HEIGHT))
                    .compact()
                    .label("Done")
                    .on_click(move |_, window, cx| {
                        settings.update(cx, |settings, cx| settings.close(window, cx));
                    })
                    .into_any_element();
                (Some(done), "Settings", None)
            }
            Page::Section(section) => {
                let back = Button::new("ios-settings-back-root")
                    .ghost()
                    .with_size(px(HEADER_HEIGHT))
                    .compact()
                    .icon(IconName::ArrowLeft)
                    .label("Settings")
                    .on_click(move |_, window, cx| {
                        settings.update(cx, |settings, cx| settings.show_root(window, cx));
                    })
                    .into_any_element();
                (Some(back), section.title(), None)
            }
            Page::Editor(kind) => {
                let editor_settings = settings.clone();
                let back = Button::new("ios-settings-back-section")
                    .ghost()
                    .with_size(px(HEADER_HEIGHT))
                    .compact()
                    .icon(IconName::ArrowLeft)
                    .label(kind.title())
                    .on_click(move |_, _, cx| {
                        editor_settings.update(cx, |settings, cx| {
                            settings.show_editor_section(kind, cx);
                        });
                    })
                    .into_any_element();
                let dirty = self
                    .editor(kind)
                    .is_some_and(|file| file.editor.read(cx).value().as_ref() != file.saved);
                let save = dirty.then(|| {
                    Button::new("ios-settings-save-editor")
                        .ghost()
                        .with_size(px(HEADER_HEIGHT))
                        .compact()
                        .label("Save")
                        .on_click(move |_, window, cx| {
                            settings.update(cx, |settings, cx| {
                                settings.save_editor(kind, window, cx);
                            });
                        })
                        .into_any_element()
                });
                (Some(back), kind.title(), save)
            }
        };

        div()
            .h(px(HEADER_HEIGHT))
            .w_full()
            .flex()
            .flex_none()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .w(px(HEADER_SIDE_WIDTH))
                    .h_full()
                    .flex()
                    .items_center()
                    .children(leading),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_center()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .w(px(HEADER_SIDE_WIDTH))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .children(trailing),
            )
            .into_any_element()
    }

    fn root_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let sections = cx.global::<AppProfile>().settings_sections.clone();
        let mut groups = Vec::new();
        let mut start = 0;
        while start < sections.len() {
            let group = sections[start].navigation_group();
            let end = sections[start..]
                .iter()
                .position(|section| section.navigation_group() != group)
                .map_or(sections.len(), |offset| start + offset);
            let rows = sections[start..end]
                .iter()
                .copied()
                .map(|section| self.section_navigation_row(section, cx))
                .collect();
            groups.push(Self::list_group(group.title(), None, rows, cx));
            start = end;
        }
        Self::scroll_page("ios-settings-root", groups).into_any_element()
    }

    fn section_navigation_row(
        &self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = cx.entity().clone();
        div()
            .id(format!("ios-settings-section-{:?}", section))
            .min_h(px(HEADER_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .on_click(move |_, window, cx| {
                settings.update(cx, |settings, cx| {
                    settings.show_section(section, window, cx);
                });
            })
            .child(Icon::new(section.icon()).small())
            .child(div().flex_1().min_w_0().child(section.title()))
            .child(
                Icon::new(IconName::ChevronRight)
                    .small()
                    .text_color(cx.theme().foreground.muted()),
            )
            .into_any_element()
    }

    fn section_page(
        &self,
        section: SettingsSection,
        resolved: AppConfig,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match section {
            SettingsSection::Appearance => self.appearance_page(resolved, cx),
            SettingsSection::Panes => self.panes_page(resolved, cx),
            SettingsSection::Advanced => self.advanced_page(resolved, cx),
            SettingsSection::Terminal => self.editor_launcher_page(EditorKind::Terminal, cx),
            SettingsSection::Multiplexer => self.editor_launcher_page(EditorKind::Multiplexer, cx),
            SettingsSection::About => self.about_page(cx),
            _ => Self::scroll_page(
                "ios-settings-empty-section",
                vec![Self::section_description(section, cx)],
            )
            .into_any_element(),
        }
    }

    fn appearance_page(&self, resolved: AppConfig, cx: &mut Context<Self>) -> AnyElement {
        let mut groups = vec![Self::section_description(SettingsSection::Appearance, cx)];
        groups.push(Self::list_group(
            "Theme",
            Some("Follow the system appearance or pin a light or dark palette."),
            vec![self.theme_mode_row(resolved, cx), self.zoom_row(cx)],
            cx,
        ));
        groups.push(self.preset_group(resolved, cx));

        let inherited = inherited_chrome_colors(resolved.chrome_preset.value, cx.theme().mode);
        groups.push(Self::list_group(
            "Custom colors",
            Some("Override one chrome color while the other colors keep following the preset."),
            ChromeColor::ALL
                .into_iter()
                .map(|color| self.chrome_color_row(color, resolved, &inherited, cx))
                .collect(),
            cx,
        ));
        groups.push(Self::list_group(
            "Details",
            None,
            vec![self.geometry_row(
                ConfigKey::WidgetCornerRadius,
                "Widget corner radius",
                "Rounds buttons, inputs, menus, tags, and dialogs.",
                resolved.widget_corner_radius,
                &self.widget_corner_radius,
                false,
                cx,
            )],
            cx,
        ));
        Self::scroll_page("ios-settings-appearance", groups).into_any_element()
    }

    fn theme_mode_row(&self, resolved: AppConfig, cx: &mut Context<Self>) -> AnyElement {
        let tiles = ThemeModeSetting::ALL.into_iter().map(|mode| {
            div()
                .id(format!("ios-theme-mode-{}", mode.as_str()))
                .h(px(HEADER_HEIGHT))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.0))
                .cursor_pointer()
                .when(mode == resolved.theme_mode.value, |tile| {
                    tile.bg(cx.theme().background.raised(3))
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .on_click(move |_, window, cx| {
                    if let Err(error) = config::set_config_key(ConfigKey::ThemeMode, mode.as_str())
                    {
                        report_write_error(
                            "set",
                            ConfigKey::ThemeMode.as_str(),
                            &error,
                            window,
                            cx,
                        );
                    }
                })
                .child(mode.title())
        });
        let control = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(
                resolved.theme_mode.provenance == ConfigProvenance::Override,
                |row| row.child(reset_button(ConfigKey::ThemeMode)),
            )
            .child(div().w(px(270.0)).flex().gap(px(4.0)).children(tiles));
        Self::setting_row(
            "ios-setting-theme-mode",
            "Theme",
            "System, Light, or Dark.",
            control.into_any_element(),
            false,
            cx,
        )
    }

    fn zoom_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let control = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(!ui_scale::is_default(cx), |row| {
                row.child(
                    Button::new("ios-reset-ui-zoom")
                        .ghost()
                        .with_size(px(HEADER_HEIGHT))
                        .compact()
                        .label("Reset")
                        .on_click(|_, _, cx| {
                            cx.stop_propagation();
                            ui_scale::reset(cx);
                        }),
                )
            })
            .child(
                NumberInput::new(&self.ui_zoom)
                    .with_size(px(HEADER_HEIGHT))
                    .w(px(174.0)),
            );
        Self::setting_row(
            "ios-setting-ui-zoom",
            "UI zoom",
            "Scales application text, icons, and controls.",
            control.into_any_element(),
            false,
            cx,
        )
    }

    fn preset_group(&self, resolved: AppConfig, cx: &mut Context<Self>) -> AnyElement {
        let mut rows = Vec::with_capacity(CHROME_PRESETS.len() + 1);
        if resolved.chrome_preset.provenance == ConfigProvenance::Override {
            rows.push(
                div()
                    .min_h(px(HEADER_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child("Use the built-in colors")
                    .child(reset_button(ConfigKey::ChromePreset))
                    .into_any_element(),
            );
        }
        rows.extend(
            CHROME_PRESETS
                .iter()
                .map(|preset| self.preset_row(preset, resolved, cx)),
        );
        Self::list_group(
            "Color preset",
            Some("Applying a preset clears the six explicit chrome colors in one write."),
            rows,
            cx,
        )
    }

    fn preset_row(
        &self,
        preset: &'static ChromePreset,
        resolved: AppConfig,
        cx: &App,
    ) -> AnyElement {
        div()
            .id(format!("ios-chrome-preset-{}", preset.id.as_str()))
            .h(px(HEADER_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .when(resolved.chrome_preset.value == Some(preset.id), |row| {
                row.bg(cx.theme().background.raised(3))
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .on_click(move |_, window, cx| {
                if let Err(error) = config::set_chrome_preset(preset.id) {
                    report_write_error("set", ConfigKey::ChromePreset.as_str(), &error, window, cx);
                }
            })
            .child(div().flex_1().min_w_0().child(preset.name))
            .child(preset_swatches(preset, cx))
            .when(resolved.chrome_preset.value == Some(preset.id), |row| {
                row.child(Icon::new(IconName::Check).small())
            })
            .into_any_element()
    }

    fn chrome_color_row(
        &self,
        color: ChromeColor,
        resolved: AppConfig,
        inherited: &zz_ui::ThemeColor,
        cx: &App,
    ) -> AnyElement {
        let setting = resolved.chrome(color);
        let control = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(setting.provenance == ConfigProvenance::Override, |row| {
                row.child(reset_button(ConfigKey::Chrome(color)))
            })
            .child(
                ColorPicker::new(&self.chrome_pickers[&color], color.read(inherited))
                    .with_size(px(HEADER_HEIGHT))
                    .label(color.title()),
            );
        Self::setting_row(
            format!("ios-chrome-color-{}", color.as_str()),
            color.title(),
            color.description(),
            control.into_any_element(),
            false,
            cx,
        )
    }

    fn panes_page(&self, resolved: AppConfig, cx: &mut Context<Self>) -> AnyElement {
        let gaps = resolved.pane_gaps.value;
        let groups = vec![
            Self::section_description(SettingsSection::Panes, cx),
            Self::list_group(
                "Layout",
                None,
                vec![Self::toggle_row(
                    ConfigKey::PaneGaps,
                    "Pane gaps",
                    "Separate panes with card-like spacing and chrome.",
                    resolved.pane_gaps,
                    cx,
                )],
                cx,
            ),
            Self::list_group(
                "Frame",
                Some("These values apply while pane gaps are enabled."),
                vec![
                    self.geometry_row(
                        ConfigKey::PaneMargin,
                        "Pane margin",
                        "Space around each pane, in logical pixels.",
                        resolved.pane_margin,
                        &self.pane_margin,
                        !gaps,
                        cx,
                    ),
                    self.geometry_row(
                        ConfigKey::PaneCornerRadius,
                        "Pane corner radius",
                        "Rounds each pane corner, in logical pixels.",
                        resolved.pane_corner_radius,
                        &self.pane_corner_radius,
                        !gaps,
                        cx,
                    ),
                    self.geometry_row(
                        ConfigKey::PaneBorderWidth,
                        "Pane border width",
                        "Set the border width to 0 to disable it.",
                        resolved.pane_border_width,
                        &self.pane_border_width,
                        !gaps,
                        cx,
                    ),
                ],
                cx,
            ),
        ];
        Self::scroll_page("ios-settings-panes", groups).into_any_element()
    }

    fn advanced_page(&self, resolved: AppConfig, cx: &mut Context<Self>) -> AnyElement {
        let groups = vec![
            Self::section_description(SettingsSection::Advanced, cx),
            Self::list_group(
                "Diagnostics",
                None,
                vec![Self::toggle_row(
                    ConfigKey::ShowFps,
                    "Show FPS",
                    "Show a frame-rate overlay above the workspace.",
                    resolved.show_fps,
                    cx,
                )],
                cx,
            ),
        ];
        Self::scroll_page("ios-settings-advanced", groups).into_any_element()
    }

    fn editor_launcher_page(&self, kind: EditorKind, cx: &mut Context<Self>) -> AnyElement {
        let settings = cx.entity().clone();
        let row = div()
            .id(format!("ios-open-{}-editor", kind.file_name()))
            .min_h(px(ROW_MIN_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .cursor_pointer()
            .on_click(move |_, window, cx| {
                settings.update(cx, |settings, cx| {
                    settings.show_editor(kind, window, cx);
                });
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child("Edit configuration")
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(cx.theme().foreground.muted())
                            .child(kind.file_name()),
                    ),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .small()
                    .text_color(cx.theme().foreground.muted()),
            )
            .into_any_element();
        Self::scroll_page(
            match kind {
                EditorKind::Terminal => "ios-settings-terminal",
                EditorKind::Multiplexer => "ios-settings-multiplexer",
            },
            vec![
                Self::section_description(kind.section(), cx),
                Self::list_group("Configuration", None, vec![row], cx),
            ],
        )
        .into_any_element()
    }

    fn about_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let build_rows = vec![
            Self::value_row("Version", env!("CARGO_PKG_VERSION"), cx),
            Self::value_row("Platform", &platform(), cx),
            Self::value_row("Renderer", gpui_revision(), cx),
            Self::copy_build_row(cx),
        ];
        let project_rows = vec![
            Self::link_row("Source code", REPOSITORY_URL, cx),
            Self::link_row("Releases", RELEASES_URL, cx),
            Self::link_row("Report an issue", ISSUES_URL, cx),
        ];
        Self::scroll_page(
            "ios-settings-about",
            vec![
                Self::section_description(SettingsSection::About, cx),
                Self::list_group("Build", None, build_rows, cx),
                Self::list_group("Project", None, project_rows, cx),
            ],
        )
        .into_any_element()
    }

    fn value_row(title: &'static str, value: &str, cx: &App) -> AnyElement {
        div()
            .min_h(px(HEADER_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().flex_1().min_w_0().child(title))
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(12.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(value.to_owned()),
            )
            .into_any_element()
    }

    fn copy_build_row(cx: &App) -> AnyElement {
        div()
            .id("ios-copy-build-information")
            .h(px(HEADER_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .on_click(|_, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(build_info()));
                window.push_notification(Notification::success("Copied build information"), cx);
            })
            .child(Icon::new(IconName::Copy).small())
            .child(div().flex_1().child("Copy build information"))
            .into_any_element()
    }

    fn link_row(title: &'static str, url: &'static str, cx: &App) -> AnyElement {
        div()
            .id(format!("ios-about-{}", title.replace(' ', "-")))
            .h(px(HEADER_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .on_click(move |_, _, cx| cx.open_url(url))
            .child(div().flex_1().child(title))
            .child(
                Icon::new(IconName::ExternalLink)
                    .small()
                    .text_color(cx.theme().foreground.muted()),
            )
            .into_any_element()
    }

    fn editor_page(&self, kind: EditorKind, cx: &mut Context<Self>) -> AnyElement {
        let file = self.editor(kind).expect("editor is created before entry");
        let path = file.path.as_ref().map_or_else(
            || kind.file_name().to_owned(),
            |path| path.display().to_string(),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_hidden()
            .when_some(file.error.clone(), |page, error| {
                page.child(
                    div()
                        .flex_none()
                        .px(px(12.0))
                        .py(px(8.0))
                        .bg(cx.theme().warning.opacity(0.12))
                        .text_size(px(12.0))
                        .text_color(cx.theme().warning)
                        .child(error),
                )
            })
            .child(
                div()
                    .h(px(30.0))
                    .flex()
                    .flex_none()
                    .items_center()
                    .px(px(12.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(11.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(path),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .m(px(8.0))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().editor_background())
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(CODE_EDITOR_FONT_SIZE))
                    .child(CodeEditor::new(&file.editor)),
            )
            .into_any_element()
    }

    fn geometry_row(
        &self,
        key: ConfigKey,
        title: &'static str,
        description: &'static str,
        setting: ConfigValue<f32>,
        input: &Entity<InputState>,
        disabled: bool,
        cx: &App,
    ) -> AnyElement {
        let control = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(setting.provenance == ConfigProvenance::Override, |row| {
                row.child(reset_button(key).disabled(disabled))
            })
            .child(
                NumberInput::new(input)
                    .with_size(px(HEADER_HEIGHT))
                    .w(px(174.0))
                    .disabled(disabled),
            );
        Self::setting_row(
            format!("ios-setting-{}", key.as_str()),
            title,
            description,
            control.into_any_element(),
            disabled,
            cx,
        )
    }

    fn toggle_row(
        key: ConfigKey,
        title: &'static str,
        description: &'static str,
        setting: ConfigValue<bool>,
        cx: &App,
    ) -> AnyElement {
        let next = !setting.value;
        let control = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(setting.provenance == ConfigProvenance::Override, |row| {
                row.child(reset_button(key))
            })
            .child(
                div()
                    .size(px(HEADER_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Switch::new(format!("ios-switch-{}", key.as_str()))
                            .small()
                            .checked(setting.value)
                            .on_click(move |value, window, cx| {
                                cx.stop_propagation();
                                write_boolean(key, *value, window, cx);
                            }),
                    ),
            );
        div()
            .id(format!("ios-setting-{}", key.as_str()))
            .min_h(px(ROW_MIN_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .on_click(move |_, window, cx| write_boolean(key, next, window, cx))
            .child(Self::setting_copy(title, description, cx))
            .child(control)
            .into_any_element()
    }

    fn setting_row(
        id: impl Into<gpui::ElementId>,
        title: &'static str,
        description: &'static str,
        control: AnyElement,
        disabled: bool,
        cx: &App,
    ) -> AnyElement {
        div()
            .id(id)
            .min_h(px(ROW_MIN_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .border_b_1()
            .border_color(cx.theme().border)
            .when(disabled, |row| row.opacity(0.5))
            .child(Self::setting_copy(title, description, cx))
            .child(control)
            .into_any_element()
    }

    fn setting_copy(title: &'static str, description: &'static str, cx: &App) -> gpui::Div {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .flex_col()
            .gap(px(2.0))
            .child(title)
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(cx.theme().foreground.muted())
                    .child(description),
            )
    }

    fn section_description(section: SettingsSection, cx: &App) -> AnyElement {
        div()
            .px(px(2.0))
            .text_size(px(13.0))
            .text_color(cx.theme().foreground.muted())
            .child(section.description())
            .into_any_element()
    }

    fn list_group(
        title: &'static str,
        description: Option<&'static str>,
        rows: Vec<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(settings_list_group_header(title, description, cx))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .rounded(px(10.0))
                    .bg(cx.theme().background.raised(1))
                    .children(rows),
            )
            .into_any_element()
    }

    fn scroll_page(id: &'static str, children: Vec<AnyElement>) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex()
            .flex_1()
            .min_h_0()
            .flex_col()
            .gap(px(GROUP_GAP))
            .overflow_y_scroll()
            .p(px(PAGE_PADDING))
            .children(children)
    }
}

impl Render for IosSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let resolved = self.reconcile(window, cx);
        let content = match self.page {
            Page::Root => self.root_page(cx),
            Page::Section(section) => self.section_page(section, resolved, cx),
            Page::Editor(kind) => self.editor_page(kind, cx),
        };
        div()
            .id("ios-settings")
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_hidden()
            .bg(zz::engine::theme::chrome_background(cx))
            .text_color(cx.theme().foreground)
            .child(self.render_header(cx))
            .child(content)
    }
}

fn geometry_input(
    key: ConfigKey,
    value: f32,
    window: &mut Window,
    cx: &mut Context<IosSettings>,
) -> Entity<InputState> {
    let (min, max) = key
        .geometry_range()
        .expect("every mobile geometry field edits a geometry key");
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value.to_string())
            .step(1.0)
            .min(f64::from(min))
            .max(f64::from(max))
    })
}

fn zoom_input(window: &mut Window, cx: &mut Context<IosSettings>) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(ui_scale::percent(cx).to_string())
            .step(f64::from(ui_scale::UI_ZOOM_STEP))
            .min(f64::from(ui_scale::MIN_UI_ZOOM))
            .max(f64::from(ui_scale::MAX_UI_ZOOM))
    })
}

fn geometry_subscription(
    input: &Entity<InputState>,
    key: ConfigKey,
    window: &mut Window,
    cx: &mut Context<IosSettings>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        move |settings, input, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                settings.commit_geometry(key, input, window, cx);
            }
        },
    )
}

fn zoom_subscription(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<IosSettings>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        |settings, _, event: &InputEvent, window, cx| match event {
            InputEvent::Change => settings.preview_zoom(cx),
            InputEvent::PressEnter { .. } | InputEvent::Blur => {
                settings.commit_zoom(window, cx);
            }
            _ => {}
        },
    )
}

fn validate_geometry_value(key: ConfigKey, value: &str) -> Result<f32, String> {
    let (min, max) = key.geometry_range().expect("geometry key");
    let rejected = || format!("enter a number between {min} and {max}");
    let value = value.parse::<f32>().map_err(|_| rejected())?;
    value
        .is_finite()
        .then_some(value)
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(rejected)
}

fn geometry_value(config: AppConfig, key: ConfigKey) -> f32 {
    match key {
        ConfigKey::PaneMargin => config.pane_margin.value,
        ConfigKey::PaneCornerRadius => config.pane_corner_radius.value,
        ConfigKey::PaneBorderWidth => config.pane_border_width.value,
        ConfigKey::WidgetCornerRadius => config.widget_corner_radius.value,
        _ => unreachable!("only mobile geometry fields call geometry_value"),
    }
}

fn synchronize_input(
    input: &Entity<InputState>,
    value: f32,
    window: &mut Window,
    cx: &mut Context<IosSettings>,
) {
    let value = value.to_string();
    if input.read(cx).value().as_ref() != value {
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }
}

fn write_boolean(key: ConfigKey, value: bool, window: &mut Window, cx: &mut App) {
    if let Err(error) = config::set_config_key(key, if value { "true" } else { "false" }) {
        report_write_error("set", key.as_str(), &error, window, cx);
    }
}

fn write_chrome_color(
    color: ChromeColor,
    value: Option<gpui::Hsla>,
    window: &mut Window,
    cx: &mut App,
) {
    let key = ConfigKey::Chrome(color);
    let result = match value {
        Some(value) => config::set_config_key(key, &zz_ui::to_hex(value)),
        None => config::remove_config_key(key),
    };
    if let Err(error) = result {
        report_write_error("set", key.as_str(), &error, window, cx);
    }
}

fn reset_button(key: ConfigKey) -> Button {
    Button::new(format!("ios-reset-{}", key.as_str()))
        .ghost()
        .with_size(px(HEADER_HEIGHT))
        .compact()
        .label("Reset")
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            if let Err(error) = config::remove_config_key(key) {
                report_write_error("reset", key.as_str(), &error, window, cx);
            }
        })
}

fn report_write_error(
    action: &str,
    key: &str,
    error: &io::Error,
    window: &mut Window,
    cx: &mut App,
) {
    log::warn!(target: "zz::config", "could not {action} {key}: {error}");
    window.push_notification(
        Notification::error(format!("Could not {action} {key}: {error}")),
        cx,
    );
}

fn editor_path(kind: EditorKind) -> io::Result<PathBuf> {
    match kind {
        EditorKind::Terminal => config::import_target_path(),
        EditorKind::Multiplexer => zz::engine::mux_config_write_path().ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "cannot create zz/mux.conf because neither XDG_CONFIG_HOME nor HOME is available",
            )
        }),
    }
}

fn load_editor_source(kind: EditorKind) -> (Option<PathBuf>, String, Option<String>) {
    match editor_path(kind) {
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
    }
}

fn preset_swatches(preset: &ChromePreset, cx: &App) -> gpui::Div {
    div()
        .flex()
        .gap(px(2.0))
        .children(preset.colors(cx.theme().mode).iter().map(|hex| {
            div()
                .w(px(12.0))
                .h(px(16.0))
                .bg(zz_ui::parse_hex(hex).unwrap_or_default())
        }))
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
    let source = zz::engine::gpui_source();
    let Some((_, revision)) = source.split_once('#') else {
        return source;
    };
    revision.get(..8).unwrap_or(revision)
}
