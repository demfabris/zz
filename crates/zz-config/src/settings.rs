use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io, path::PathBuf};
use zz_client::chrome_palette::{CHROME_PRESETS, ChromeColor, ChromePresetId, chrome_presets};
use zz_protocol::KeyBindingSnapshot;

pub const MAX_MUX_CONFIG_BYTES: usize = 1024 * 1024;

mod file_rows;

#[derive(Serialize)]
pub struct Setting {
    pub key: &'static str,
    pub title: String,
    pub section: &'static str,
    pub value: Value,
    pub default_value: Value,
    pub overridden: bool,
    pub enabled: bool,
    pub control: &'static str,
    pub range: Option<(f32, f32)>,
    pub choices: Vec<Choice>,
}
#[derive(Serialize)]
pub struct Choice {
    pub value: String,
    pub title: String,
}

fn value(parsed: &ParsedConfig, key: ConfigKey) -> (Value, ConfigProvenance) {
    match key {
        ConfigKey::PaletteWindowLayout => {
            let setting = &parsed.config.palette_window_layout;
            (json!(setting.value.as_str()), setting.provenance)
        }
        ConfigKey::PaletteHostPrefix => {
            let setting = &parsed.config.palette_host_prefix;
            (json!(setting.value.as_str()), setting.provenance)
        }
        ConfigKey::PaletteShowKeys => {
            let setting = &parsed.config.palette_show_keys;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::UseSystemTitlebar => {
            let setting = &parsed.config.use_system_titlebar;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::WindowCornerRadius => {
            let setting = &parsed.config.window_corner_radius;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::WindowBackgroundBlur => {
            let setting = &parsed.config.window_background_blur;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::Animations => {
            let setting = &parsed.config.animations;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::Tray => {
            let setting = &parsed.config.tray;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::ShowFps => {
            let setting = &parsed.config.show_fps;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::QuitDaemonOnExit => {
            let setting = &parsed.config.quit_daemon_on_exit;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::AutoRestartStaleDaemon => {
            let setting = &parsed.config.auto_restart_stale_daemon;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::CheckForUpdates => {
            let setting = &parsed.config.check_for_updates;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::StatusShowSession => {
            let setting = &parsed.config.status_show_session;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::StatusBadges => {
            let setting = &parsed.config.status_badges;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::StatusAgents => {
            let setting = &parsed.config.status_agents;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::StatusHost => {
            let setting = &parsed.config.status_host;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::StatusUpdate => {
            let setting = &parsed.config.status_update;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::ExperimentalAgentPane => {
            let setting = &parsed.config.experimental_agent_pane;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::ExperimentalEditorPane => {
            let setting = &parsed.config.experimental_editor_pane;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneGaps => {
            let setting = &parsed.config.pane_gaps;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneBackgroundOpacity => {
            let setting = &parsed.config.pane_background_opacity;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneGlowStrength => {
            let setting = &parsed.config.pane_glow_strength;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneInactiveOpacity => {
            let setting = &parsed.config.pane_inactive_opacity;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneCornerRadius => {
            let setting = &parsed.config.pane_corner_radius;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneMargin => {
            let setting = &parsed.config.pane_margin;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::PaneBorderWidth => {
            let setting = &parsed.config.pane_border_width;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::ChromeContrast => {
            let setting = &parsed.config.chrome_contrast;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::WidgetCornerRadius => {
            let setting = &parsed.config.widget_corner_radius;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::ShadowStrength => {
            let setting = &parsed.config.shadow_strength;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::EditorFontSize => {
            let setting = &parsed.config.editor_font_size;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::EditorLineNumbers => {
            let setting = &parsed.config.editor_line_numbers;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::EditorRelativeLineNumbers => {
            let setting = &parsed.config.editor_relative_line_numbers;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::EditorSoftWrap => {
            let setting = &parsed.config.editor_soft_wrap;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::EditorVimMode => {
            let setting = &parsed.config.editor_vim_mode;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::BrowserElementSelectorHotkey => {
            let setting = &parsed.browser.element_selector_hotkey;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::BrowserRemoteDebuggingPort => {
            let setting = &parsed.browser.remote_debugging_port;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::BrowserSearchProvider => {
            let setting = &parsed.browser.search_provider;
            (json!(setting.value.as_str()), setting.provenance)
        }
        ConfigKey::BrowserEgress => {
            let setting = &parsed.config.browser_egress;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::ThemeMode => {
            let setting = &parsed.config.theme_mode;
            (json!(setting.value.as_str()), setting.provenance)
        }
        ConfigKey::UiFontFamily => {
            let setting = &parsed.ui_font.family;
            (json!(setting.value), setting.provenance)
        }
        ConfigKey::AppIcon => {
            let setting = &parsed.config.app_icon;
            (json!(setting.value.as_str()), setting.provenance)
        }
        ConfigKey::ChromePreset { dark } => {
            let setting = parsed.config.chrome_preset(dark);
            (
                json!(setting.value.map(ChromePresetId::as_str)),
                setting.provenance,
            )
        }
        ConfigKey::Chrome(color) => {
            let setting = parsed.config.chrome(color);
            (json!(setting.value.map(hex)), setting.provenance)
        }
    }
}
fn hex([r, g, b, a]: [f32; 4]) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", channel(r), channel(g), channel(b))
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel(r),
            channel(g),
            channel(b),
            channel(a)
        )
    }
}
fn choices(key: ConfigKey) -> Vec<Choice> {
    let pairs: Vec<(&str, &str)> = match key {
        ConfigKey::PaletteWindowLayout => vec![("grouped", "Grouped"), ("flat", "Flat")],
        ConfigKey::PaletteHostPrefix => vec![("~", "~"), ("#", "#")],
        ConfigKey::ThemeMode => vec![("system", "System"), ("light", "Light"), ("dark", "Dark")],
        ConfigKey::AppIcon => vec![
            ("automatic", "Automatic"),
            ("light", "Light"),
            ("dark", "Dark"),
        ],
        ConfigKey::BrowserSearchProvider => SearchProvider::ALL
            .into_iter()
            .map(|provider| (provider.as_str(), provider.title()))
            .collect(),
        ConfigKey::ChromePreset { dark } => chrome_presets(dark)
            .map(|preset| (preset.id.as_str(), preset.name))
            .collect(),
        _ => Vec::new(),
    };
    pairs
        .into_iter()
        .map(|(value, title)| Choice {
            value: value.to_owned(),
            title: title.to_owned(),
        })
        .collect()
}
fn section(key: ConfigKey) -> &'static str {
    match key {
        ConfigKey::StatusShowSession
        | ConfigKey::StatusBadges
        | ConfigKey::StatusAgents
        | ConfigKey::StatusHost
        | ConfigKey::StatusUpdate => "status",
        ConfigKey::PaneGaps
        | ConfigKey::PaneBackgroundOpacity
        | ConfigKey::PaneGlowStrength
        | ConfigKey::PaneInactiveOpacity
        | ConfigKey::PaneCornerRadius
        | ConfigKey::PaneMargin
        | ConfigKey::PaneBorderWidth => "panes",
        ConfigKey::EditorFontSize
        | ConfigKey::EditorLineNumbers
        | ConfigKey::EditorRelativeLineNumbers
        | ConfigKey::EditorSoftWrap
        | ConfigKey::EditorVimMode => "editor",
        ConfigKey::BrowserElementSelectorHotkey
        | ConfigKey::BrowserSearchProvider
        | ConfigKey::BrowserEgress => "browser",
        ConfigKey::Tray
        | ConfigKey::PaletteWindowLayout
        | ConfigKey::PaletteHostPrefix
        | ConfigKey::PaletteShowKeys
        | ConfigKey::ShowFps
        | ConfigKey::QuitDaemonOnExit
        | ConfigKey::AutoRestartStaleDaemon
        | ConfigKey::ExperimentalAgentPane
        | ConfigKey::ExperimentalEditorPane => "system",
        ConfigKey::CheckForUpdates => "about",
        _ => "interface",
    }
}
fn title(key: ConfigKey) -> String {
    match key {
        ConfigKey::UiFontFamily => "Interface font".to_owned(),
        ConfigKey::ChromeContrast => "Contrast".to_owned(),
        ConfigKey::PaneGlowStrength => "Selected pane glow".to_owned(),
        ConfigKey::BrowserElementSelectorHotkey => "Element selector shortcut".to_owned(),
        ConfigKey::BrowserEgress => "Route remote browsing through SSH".to_owned(),
        ConfigKey::ShowFps => "Show frame rate".to_owned(),
        ConfigKey::StatusShowSession => "Show session".to_owned(),
        ConfigKey::StatusAgents => "Agent activity".to_owned(),
        ConfigKey::StatusHost => "Host name".to_owned(),
        ConfigKey::StatusUpdate => "Update indicator".to_owned(),
        ConfigKey::Chrome(color) => color.title().to_owned(),
        _ => {
            let mut title = key.as_str().replace('-', " ");
            if let Some(first) = title.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            title
        }
    }
}

pub fn settings(parsed: &ParsedConfig) -> Vec<Setting> {
    let defaults = ParsedConfig::default();
    [
        ConfigKey::PaletteWindowLayout,
        ConfigKey::PaletteHostPrefix,
        ConfigKey::PaletteShowKeys,
        ConfigKey::UseSystemTitlebar,
        ConfigKey::WindowCornerRadius,
        ConfigKey::WindowBackgroundBlur,
        ConfigKey::Animations,
        ConfigKey::Tray,
        ConfigKey::ShowFps,
        ConfigKey::QuitDaemonOnExit,
        ConfigKey::AutoRestartStaleDaemon,
        ConfigKey::CheckForUpdates,
        ConfigKey::StatusShowSession,
        ConfigKey::StatusBadges,
        ConfigKey::StatusAgents,
        ConfigKey::StatusHost,
        ConfigKey::StatusUpdate,
        ConfigKey::ExperimentalAgentPane,
        ConfigKey::ExperimentalEditorPane,
        ConfigKey::PaneGaps,
        ConfigKey::PaneBackgroundOpacity,
        ConfigKey::PaneGlowStrength,
        ConfigKey::PaneInactiveOpacity,
        ConfigKey::PaneCornerRadius,
        ConfigKey::PaneMargin,
        ConfigKey::PaneBorderWidth,
        ConfigKey::WidgetCornerRadius,
        ConfigKey::ChromeContrast,
        ConfigKey::ShadowStrength,
        ConfigKey::EditorFontSize,
        ConfigKey::EditorLineNumbers,
        ConfigKey::EditorRelativeLineNumbers,
        ConfigKey::EditorSoftWrap,
        ConfigKey::EditorVimMode,
        ConfigKey::BrowserElementSelectorHotkey,
        ConfigKey::BrowserSearchProvider,
        ConfigKey::BrowserEgress,
        ConfigKey::ThemeMode,
        ConfigKey::UiFontFamily,
        ConfigKey::AppIcon,
        ConfigKey::ChromePreset { dark: false },
        ConfigKey::ChromePreset { dark: true },
    ]
    .into_iter()
    .chain(ChromeColor::ALL.map(ConfigKey::Chrome))
    .map(|key| {
        let (value, provenance) = value(parsed, key);
        let choices = choices(key);
        let range = key.numeric_range();
        let control = if value.is_boolean() {
            "boolean"
        } else if range.is_some() {
            "number"
        } else if matches!(key, ConfigKey::Chrome(_)) {
            "color"
        } else if !choices.is_empty() {
            "choice"
        } else {
            "text"
        };
        let enabled = !matches!(
            key,
            ConfigKey::PaneCornerRadius | ConfigKey::PaneMargin | ConfigKey::PaneBorderWidth
        ) || parsed.config.pane_gaps.value;
        Setting {
            key: key.as_str(),
            title: title(key),
            section: section(key),
            default_value: self::value(&defaults, key).0,
            value,
            overridden: provenance == ConfigProvenance::Override,
            enabled,
            control,
            range,
            choices,
        }
    })
    .collect()
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum SettingsAction {
    Set {
        key: String,
        value: Value,
    },
    Reset {
        key: String,
    },
    SetAppearance {
        key: String,
        value: Option<String>,
    },
    SetMux {
        key: String,
        value: Option<String>,
    },
    Preset {
        value: String,
    },
    SaveTerminal {
        source: String,
    },
    SaveMux {
        source: String,
    },
    ImportGhostty {
        dark: bool,
    },
    AddHost {
        name: String,
        endpoint: String,
    },
    RemoveHost {
        name: String,
    },
    SplitBinding {
        horizontal: bool,
        key: String,
        kind: String,
    },
}

pub struct SettingsModel {
    pub parsed: ParsedConfig,
    pub revision: u64,
    pub error: Option<String>,
    pub system_font: String,
    explicit_config: Option<PathBuf>,
    explicit_mux: Option<PathBuf>,
    stamp: (ConfigFileStamp, ConfigFileStamp),
}
impl SettingsModel {
    pub fn new(system_font: String, config: Option<PathBuf>, mux: Option<PathBuf>) -> Self {
        let mut model = Self {
            parsed: ParsedConfig::default(),
            revision: 0,
            error: None,
            system_font,
            explicit_config: config,
            explicit_mux: mux,
            stamp: Default::default(),
        };
        model.reload();
        model
    }
    pub fn config_path(&self) -> io::Result<PathBuf> {
        self.explicit_config
            .clone()
            .map_or_else(config_path_for_write, Ok)
    }
    pub fn mux_path(&self) -> io::Result<PathBuf> {
        self.explicit_mux
            .clone()
            .or_else(zz_daemon::mux_config_write_path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "No multiplexer configuration path is available.",
                )
            })
    }
    fn stamps(&self) -> (ConfigFileStamp, ConfigFileStamp) {
        let candidates = self
            .explicit_config
            .clone()
            .map_or_else(config_candidates, |path| vec![path]);
        let mux = self.mux_path().ok().into_iter().collect::<Vec<_>>();
        (
            ConfigFileStamp::detect(&candidates),
            ConfigFileStamp::detect(&mux),
        )
    }
    pub fn poll(&mut self) -> bool {
        if self.stamps() == self.stamp {
            return false;
        }
        self.reload();
        true
    }
    pub fn reload(&mut self) {
        self.error = None;
        self.stamp = self.stamps();
        self.parsed = match self
            .stamp
            .0
            .path
            .as_deref()
            .map(|path| load_config(path, &self.system_font))
            .transpose()
        {
            Ok(parsed) => parsed.unwrap_or_default(),
            Err(error) => {
                self.error = Some(error.to_string());
                ParsedConfig::default()
            }
        };
        self.revision = self.revision.saturating_add(1);
    }
    pub fn snapshot(&self, bindings: &[KeyBindingSnapshot]) -> Value {
        let config_path = self.config_path().ok();
        let mux_path = self.mux_path().ok();
        let terminal_source = config_path
            .as_deref()
            .map(|path| {
                read_config_editor_source(path, MAX_CONFIG_BYTES)
                    .map(|source| appearance_editor_view(&source))
            })
            .transpose();
        let mux_source = mux_path
            .as_deref()
            .map(|path| read_config_editor_source(path, MAX_MUX_CONFIG_BYTES))
            .transpose();
        let split = |direction| {
            let binding = mux_bindings::preferred_split_binding(bindings, direction);
            let kind = binding
                .and_then(|binding| mux_bindings::split_binding_kind(binding, direction))
                .map(|kind| match kind {
                    mux_bindings::SplitPaneKind::Picker => "picker",
                    mux_bindings::SplitPaneKind::Terminal => "terminal",
                    mux_bindings::SplitPaneKind::Browser => "browser",
                });
            json!({"key":binding.map(|binding|&binding.key),"kind":kind,"editable":binding.is_none() || kind.is_some()})
        };
        let hosts = self
            .parsed
            .hosts
            .iter()
            .map(|host| json!({"name":host.name,"endpoint":host.endpoint.to_string()}))
            .collect::<Vec<_>>();
        let overrides = self
            .parsed
            .chrome_overrides
            .iter()
            .map(|entry| match entry {
                keymap::ChromeOverride::Bind { table, key, action } => {
                    json!({"table":table,"key":key,"action":action})
                }
                keymap::ChromeOverride::Unbind { table, key } => {
                    json!({"table":table,"key":key,"action":null})
                }
            })
            .collect::<Vec<_>>();
        let mut rows = settings(&self.parsed);
        rows.extend(file_rows::file_settings(
            terminal_source
                .as_ref()
                .ok()
                .and_then(|value| value.as_deref())
                .unwrap_or_default(),
            mux_source
                .as_ref()
                .ok()
                .and_then(|value| value.as_deref())
                .unwrap_or_default(),
        ));
        json!({"revision":self.revision,"settings":rows,"config_path":config_path,"mux_path":mux_path,
            "terminal_source":terminal_source.as_ref().ok().and_then(|value|value.as_ref()),"mux_source":mux_source.as_ref().ok().and_then(|value|value.as_ref()),
            "editor_error":terminal_source.err().or_else(||mux_source.err()).map(|error|error.to_string()),
            "hosts":hosts,"diagnostics":self.parsed.diagnostics.iter().map(|error|json!({"line":error.line,"message":error.message})).collect::<Vec<_>>(),
            "ghostty_path":zz_terminal::discover_ghostty_config(),
            "mux_sources":zz_daemon::mux_config_candidates(),"error":self.error,"chrome_overrides":overrides,
            "horizontal":split(mux_bindings::SplitDirection::Horizontal),"vertical":split(mux_bindings::SplitDirection::Vertical),
            "prefix_bindings":bindings.iter().map(|binding|json!({"key":binding.key,"command":binding.commands.iter().map(zz_mux::format_command).collect::<Vec<_>>().join(" ; ")})).collect::<Vec<_>>(),
            "presets":CHROME_PRESETS.iter().map(|preset|json!({"id":preset.id.as_str(),"name":preset.name,"dark":preset.dark,
                "background":preset.background,"foreground":preset.foreground,"accent":preset.accent,
                "success":preset.success,"warning":preset.warning,"danger":preset.danger})).collect::<Vec<_>>(),
            "version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH})
    }
    pub fn mux_commands(&self) -> io::Result<Vec<zz_protocol::CommandInvocation>> {
        let source = read_config_editor_source(&self.mux_path()?, MAX_MUX_CONFIG_BYTES)?;
        let parsed =
            zz_mux::MuxEngine::parse_config_without_variable_expansion("iPad preferences", &source);
        if let Some(error) = parsed.diagnostics.first() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                error.message.clone(),
            ));
        }
        Ok(parsed.commands)
    }

    pub fn action(
        &mut self,
        action: SettingsAction,
        bindings: &[KeyBindingSnapshot],
    ) -> io::Result<bool> {
        let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidInput, message);
        let mut mux_changed = false;
        match action {
            SettingsAction::SetAppearance { key, value } => {
                let key = AppearanceConfigKey::from_config_key(&key)
                    .ok_or_else(|| invalid("Unknown terminal setting."))?;
                if let Some(value) = value.as_ref() {
                    file_options::validate_value(value)?;
                    if key != AppearanceConfigKey::Theme {
                        let parsed = zz_terminal::apply_appearance_overrides(
                            zz_terminal::AppearanceLoad::defaults_for(
                                zz_terminal::TerminalColorScheme::Dark,
                            ),
                            &[(key.as_str().to_owned(), value.clone())],
                        );
                        if parsed.invalid > 0 || parsed.fatal {
                            return Err(invalid(&parsed.diagnostics.first().map_or_else(
                                || "Invalid terminal setting.".to_owned(),
                                |diagnostic| diagnostic.message.clone(),
                            )));
                        }
                    }
                }
                file_options::write_appearance_option(&self.config_path()?, key, value.as_deref())?;
            }
            SettingsAction::SetMux { key, value } => {
                let key = zz_protocol::MuxOptionKey::from_config_key(&key)
                    .ok_or_else(|| invalid("Unknown multiplexer setting."))?;
                if let Some(value) = value.as_deref() {
                    file_options::validate_value(value)?;
                    zz_mux::MuxEngine::default()
                        .execute(
                            &mut zz_mux::ExecutionContext::default(),
                            &zz_protocol::CommandInvocation::new(
                                "set-option",
                                ["-g", key.as_str(), value],
                            ),
                        )
                        .map_err(|error| invalid(&error.to_string()))?;
                }
                let path = self.mux_path()?;
                let source = read_config_editor_source(&path, MAX_MUX_CONFIG_BYTES)?;
                let edited = file_options::edit_mux_option(&source, key, value.as_deref())?;
                write_config_editor_source(&path, &edited, MAX_MUX_CONFIG_BYTES)?;
                mux_changed = true;
            }
            SettingsAction::Set { key, value } => {
                let key = ConfigKey::parse(&key).ok_or_else(|| invalid("Unknown setting."))?;
                let text = match value {
                    Value::String(value) => value,
                    Value::Bool(value) => value.to_string(),
                    Value::Number(value) => value.to_string(),
                    _ => return Err(invalid("Enter a setting value.")),
                };
                if key == ConfigKey::UiFontFamily {
                    write_ui_font_family_at(&self.config_path()?, Some(&text), &self.system_font)?;
                } else {
                    if text.contains(['\n', '\r']) {
                        return Err(invalid("Setting values must fit on one line."));
                    }
                    let parsed =
                        parse_config(&format!("{} = {text}", key.as_str()), &self.system_font);
                    if let Some(error) = parsed.diagnostics.first() {
                        return Err(invalid(&error.message));
                    }
                    if let ConfigKey::ChromePreset { dark } = key {
                        write_chrome_preset_at(
                            &self.config_path()?,
                            dark,
                            parsed.config.chrome_preset(dark).value,
                        )?;
                    } else {
                        let text = if key == ConfigKey::BrowserElementSelectorHotkey {
                            normalize_browser_hotkey(&text).map_err(|error| invalid(&error))?
                        } else {
                            text
                        };
                        write_config_edit_at(&self.config_path()?, key.as_str(), Some(&text))?;
                    }
                }
            }
            SettingsAction::Reset { key } => {
                let key = ConfigKey::parse(&key).ok_or_else(|| invalid("Unknown setting."))?;
                if let ConfigKey::ChromePreset { dark } = key {
                    write_chrome_preset_at(&self.config_path()?, dark, None)?;
                } else {
                    write_config_edit_at(&self.config_path()?, key.as_str(), None)?;
                }
            }
            SettingsAction::Preset { value } => {
                let preset =
                    ChromePresetId::parse(&value).ok_or_else(|| invalid("Unknown palette."))?;
                write_chrome_preset_at(&self.config_path()?, preset.dark(), Some(preset))?;
            }
            SettingsAction::SaveTerminal { source } => {
                save_appearance_editor(&self.config_path()?, &source)?;
            }
            SettingsAction::SaveMux { source } => {
                write_config_editor_source(&self.mux_path()?, &source, MAX_MUX_CONFIG_BYTES)?;
                mux_changed = true;
            }
            SettingsAction::ImportGhostty { dark } => {
                let path = zz_terminal::discover_ghostty_config()
                    .ok_or_else(|| invalid("No Ghostty configuration was found."))?;
                let scheme = if dark {
                    zz_terminal::TerminalColorScheme::Dark
                } else {
                    zz_terminal::TerminalColorScheme::Light
                };
                let values = import::ghostty_import_values(
                    &zz_terminal::load_ghostty_appearance_from_for(&path, scheme),
                )?;
                import_appearance_values_at(&self.config_path()?, &values)?;
            }
            SettingsAction::AddHost { name, endpoint } => {
                if self.parsed.hosts.iter().any(|host| host.name == name) {
                    return Err(invalid("A host with this name already exists."));
                }
                write_fleet_host_at(&self.config_path()?, &name, &endpoint)?;
            }
            SettingsAction::RemoveHost { name } => {
                remove_fleet_host_at(&self.config_path()?, &name)?;
            }
            SettingsAction::SplitBinding {
                horizontal,
                key,
                kind,
            } => {
                let direction = if horizontal {
                    mux_bindings::SplitDirection::Horizontal
                } else {
                    mux_bindings::SplitDirection::Vertical
                };
                let kind = match kind.as_str() {
                    "picker" => mux_bindings::SplitPaneKind::Picker,
                    "terminal" => mux_bindings::SplitPaneKind::Terminal,
                    "browser" => mux_bindings::SplitPaneKind::Browser,
                    _ => return Err(invalid("Choose a pane type.")),
                };
                let path = self.mux_path()?;
                let source = read_config_editor_source(&path, MAX_MUX_CONFIG_BYTES)?;
                let original = mux_bindings::preferred_split_binding(bindings, direction);
                let edited = mux_bindings::update_split_binding(
                    &source, bindings, original, direction, &key, kind,
                )
                .map_err(|error| invalid(&error))?;
                write_config_editor_source(&path, &edited, MAX_MUX_CONFIG_BYTES)?;
                mux_changed = true;
            }
        }
        self.error = None;
        self.reload();
        Ok(mux_changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlebar_settings_exclude_clock_and_alignment() {
        let parsed = parse_config(
            "status-align = center\nstatus-clock = time-date\n",
            "System",
        );
        assert_eq!(parsed.diagnostics.len(), 2);
        assert!(parsed.daemon_entries.is_empty());
        assert_eq!(parsed.config, AppConfig::default());
        for key in ["status-align", "status-clock"] {
            assert_eq!(ConfigKey::parse(key), None);
            assert!(!settings(&parsed).iter().any(|setting| setting.key == key));
        }
    }

    #[test]
    fn invalid_terminal_preferences_preserve_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        let source = "font-size = 16\nbackground = #123456\n";
        std::fs::write(&config, source).unwrap();
        let mut model = SettingsModel::new("System".to_owned(), Some(config.clone()), None);
        for (key, value) in [
            ("font-size", "enormous"),
            ("background", "not-a-color"),
            ("cursor-style", "triangle"),
            ("window-padding-x", "-12"),
            ("background-opacity", "NaN"),
        ] {
            assert!(
                model
                    .action(
                        SettingsAction::SetAppearance {
                            key: key.to_owned(),
                            value: Some(value.to_owned()),
                        },
                        &[]
                    )
                    .is_err(),
                "{key}={value}"
            );
            assert_eq!(std::fs::read_to_string(&config).unwrap(), source);
        }
        model
            .action(
                SettingsAction::SetAppearance {
                    key: "theme".to_owned(),
                    value: Some("Dracula".to_owned()),
                },
                &[],
            )
            .unwrap();
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains("Dracula")
        );
    }

    #[test]
    fn invalid_mux_preferences_preserve_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let mux = directory.path().join("mux.conf");
        let source = "set -g prefix C-a\n";
        std::fs::write(&mux, source).unwrap();
        let mut model = SettingsModel::new(
            "System".to_owned(),
            Some(directory.path().join("config")),
            Some(mux.clone()),
        );
        for (key, value) in [
            ("prefix", "not-a-key"),
            ("mode-keys", "vim"),
            ("history-limit", "-1"),
            ("escape-time", "later"),
            ("mouse", "sometimes"),
        ] {
            assert!(
                model
                    .action(
                        SettingsAction::SetMux {
                            key: key.to_owned(),
                            value: Some(value.to_owned()),
                        },
                        &[]
                    )
                    .is_err(),
                "{key}={value}"
            );
            assert_eq!(std::fs::read_to_string(&mux).unwrap(), source);
        }
        model
            .action(
                SettingsAction::SetMux {
                    key: "prefix".to_owned(),
                    value: Some("C-b".to_owned()),
                },
                &[],
            )
            .unwrap();
        assert!(std::fs::read_to_string(&mux).unwrap().contains("C-b"));
    }

    #[test]
    fn full_widget_radius_persists_and_resets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config");
        let mut model = SettingsModel::new("System".to_owned(), Some(path.clone()), None);
        for value in [24.0, 25.0] {
            model
                .action(
                    SettingsAction::Set {
                        key: "widget-corner-radius".to_owned(),
                        value: json!(value),
                    },
                    &[],
                )
                .unwrap();
            assert_eq!(
                load_config(&path, "System")
                    .unwrap()
                    .config
                    .widget_corner_radius
                    .value,
                value
            );
        }
        assert!(
            model
                .action(
                    SettingsAction::Set {
                        key: "widget-corner-radius".to_owned(),
                        value: json!(26),
                    },
                    &[]
                )
                .is_err()
        );
        assert_eq!(
            load_config(&path, "System")
                .unwrap()
                .config
                .widget_corner_radius
                .value,
            25.0
        );
        model
            .action(
                SettingsAction::Reset {
                    key: "widget-corner-radius".to_owned(),
                },
                &[],
            )
            .unwrap();
        assert_eq!(
            model.parsed.config.widget_corner_radius,
            ConfigValue::from_default(DEFAULT_WIDGET_CORNER_RADIUS)
        );
    }

    #[test]
    fn contrast_settings_persist_validate_and_reset_with_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config");
        let mut model = SettingsModel::new("System".to_owned(), Some(path.clone()), None);
        let set = |model: &mut SettingsModel, value: f64| {
            model.action(
                SettingsAction::Set {
                    key: "chrome-contrast".to_owned(),
                    value: json!(value),
                },
                &[],
            )
        };
        set(&mut model, 1.5).unwrap();
        for value in [4.0, -1.0] {
            assert!(set(&mut model, value).is_err(), "{value}");
        }
        let loaded = load_config(&path, "System").unwrap();
        assert_eq!(loaded.config.chrome_contrast.value, 1.5);
        let setting = settings(&loaded)
            .into_iter()
            .find(|setting| setting.key == "chrome-contrast")
            .unwrap();
        assert!(setting.overridden);
        assert_eq!(setting.range, Some((0.5, 2.0)));
        model
            .action(
                SettingsAction::Reset {
                    key: "chrome-contrast".to_owned(),
                },
                &[],
            )
            .unwrap();
        assert_eq!(
            model.parsed.config.chrome_contrast,
            ConfigValue::from_default(1.0)
        );
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("chrome-contrast")
        );
    }

    #[test]
    fn native_settings_validate_before_writing_and_preserve_other_surfaces() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        let mux = directory.path().join("mux.conf");
        let source = "# retained\npane-margin = 6\nfont-size = 14\nhost-dev = ssh://example.test\n";
        std::fs::write(&config, source).unwrap();
        let mut model =
            SettingsModel::new("System".to_owned(), Some(config.clone()), Some(mux.clone()));
        assert!(
            model
                .action(
                    SettingsAction::Set {
                        key: "pane-margin".to_owned(),
                        value: json!(1000)
                    },
                    &[]
                )
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), source);
        model
            .action(
                SettingsAction::Set {
                    key: "pane-margin".to_owned(),
                    value: json!(12),
                },
                &[],
            )
            .unwrap();
        model
            .action(
                SettingsAction::SaveTerminal {
                    source: "font-size = 18\n".to_owned(),
                },
                &[],
            )
            .unwrap();
        let saved = std::fs::read_to_string(&config).unwrap();
        assert!(saved.contains("pane-margin = 12"));
        assert!(saved.contains("host-dev = ssh://example.test"));
        assert!(saved.contains("font-size = 18"));
        assert_eq!(model.parsed.config.pane_margin.value, 12.0);
        model
            .action(
                SettingsAction::SaveMux {
                    source: "set -g prefix C-a\n".to_owned(),
                },
                &[],
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&mux).unwrap(),
            "set -g prefix C-a\n"
        );
        assert!(
            model
                .action(
                    SettingsAction::SaveMux {
                        source: "x".repeat(MAX_MUX_CONFIG_BYTES + 1)
                    },
                    &[]
                )
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(&mux).unwrap(),
            "set -g prefix C-a\n"
        );
    }

    #[test]
    fn native_settings_poll_reload_and_deletion_restore_defaults_and_overrides() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        let mut model = SettingsModel::new(
            "System".to_owned(),
            Some(config.clone()),
            Some(directory.path().join("mux.conf")),
        );
        assert!(!model.poll());
        std::fs::write(
            &config,
            "pane-gaps = true\nfont-size = 18\nchrome-background = #123456\n",
        )
        .unwrap();
        assert!(model.poll());
        assert!(model.parsed.config.pane_gaps.value);
        assert_eq!(
            model.parsed.daemon_entries,
            vec![("font-size".to_owned(), "18".to_owned())]
        );
        model
            .action(
                SettingsAction::Preset {
                    value: "nord".to_owned(),
                },
                &[],
            )
            .unwrap();
        assert!(
            model
                .parsed
                .config
                .chrome(ChromeColor::Background)
                .value
                .is_none()
        );
        assert_eq!(
            model.parsed.config.chrome_preset(true).value,
            ChromePresetId::parse("nord")
        );
        std::fs::remove_file(&config).unwrap();
        assert!(model.poll());
        assert_eq!(model.parsed, ParsedConfig::default());
        assert!(model.parsed.daemon_entries.is_empty());
        assert!(!model.poll());
    }

    #[test]
    fn browser_shortcuts_keep_a_parseable_spelling() {
        for value in ["cmd-shift-c", "ctrl-alt-x", "super-C", "fn-f1"] {
            let normalized = normalize_browser_hotkey(value).unwrap();
            assert_eq!(normalize_browser_hotkey(&normalized).unwrap(), normalized);
        }
        assert!(normalize_browser_hotkey("shift-c").is_err());
        assert!(normalize_browser_hotkey("ctrl-").is_err());
    }
}
