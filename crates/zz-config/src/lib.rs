use keymap::ChromeOverride;
use std::{
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};
use zz_client::chrome_palette::{AppIconSetting, ChromeColor, ChromePresetId, ThemeModeSetting};
use zz_client::url_input::SearchProvider;
use zz_client::{StatusBarAlignment, StatusBarClock, StatusBarSettings};
pub use zz_daemon::{HostEntry, RejectedHost, configured_fleet_hosts, validate_fleet_host};
use zz_protocol::{ConfigOverrideEntry, MAX_GUI_TEXT_BYTES, MuxOptionKey};
use zz_terminal::{
    AppearanceColor, AppearanceConfigKey, CellHeightAdjustment, Color, CursorBlinkPolicy,
    CursorStyle, TerminalAppearance,
};
pub mod agent_preferences;
pub mod import;
pub mod keymap;
pub mod mux_bindings;
pub mod update;

pub const CONFIG_DIRECTORY_NAME: &str = "zz";
pub const CONFIG_FILE_NAME: &str = "config";
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

pub const MAX_PANE_MARGIN: f32 = 32.0;
pub const MAX_PANE_CORNER_RADIUS: f32 = 32.0;
pub const MAX_PANE_BORDER_WIDTH: f32 = 8.0;
pub const MAX_WIDGET_CORNER_RADIUS: f32 = 24.0;
pub const MAX_WINDOW_CORNER_RADIUS: f32 = 32.0;

// Tangent-circle fit of the native macOS 27 window corner, measured off a screenshot.
pub const DEFAULT_WINDOW_CORNER_RADIUS: f32 = 13.5;

pub const DEFAULT_PANE_GAPS: bool = false;
pub const DEFAULT_PANE_INACTIVE_OPACITY: f32 = 0.7;
pub const MIN_PANE_INACTIVE_OPACITY: f32 = 0.0;
pub const MAX_PANE_INACTIVE_OPACITY: f32 = 1.0;
pub const DEFAULT_PANE_CORNER_RADIUS: f32 = DEFAULT_WINDOW_CORNER_RADIUS;
pub const DEFAULT_PANE_MARGIN: f32 = 6.0;
pub const DEFAULT_PANE_BORDER_WIDTH: f32 = 0.5;
// The zz-ui theme's own default radius, restated here because the theme now reads it from here.
pub const DEFAULT_WIDGET_CORNER_RADIUS: f32 = 6.0;
pub const DEFAULT_SHADOW_STRENGTH: f32 = 1.0;
pub const DEFAULT_USE_SYSTEM_TITLEBAR: bool = false;
pub const DEFAULT_WINDOW_BACKGROUND_BLUR: bool = false;
pub const DEFAULT_ANIMATIONS: bool = true;
pub const DEFAULT_TRAY: bool = true;
pub const DEFAULT_SHOW_FPS: bool = false;
pub const DEFAULT_QUIT_DAEMON_ON_EXIT: bool = false;
pub const DEFAULT_AUTO_RESTART_STALE_DAEMON: bool = false;
pub const DEFAULT_CHECK_FOR_UPDATES: bool = true;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub const DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY: &str = "cmd-shift-c";
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub const DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY: &str = "ctrl-shift-c";
/// Repeatable chrome binding overrides: `<table>:<key>=<action>` and
/// `<table>:<key>`.
pub const CHROME_KEYBIND_KEY: &str = "chrome-keybind";
pub const CHROME_UNBIND_KEY: &str = "chrome-unbind";
pub const DEFAULT_BROWSER_SEARCH_PROVIDER: SearchProvider = SearchProvider::Google;
pub const DEFAULT_BROWSER_EGRESS: bool = true;
pub const DEFAULT_EDITOR_FONT_SIZE: f32 = 13.0;
pub const MIN_EDITOR_FONT_SIZE: f32 = 8.0;
pub const MAX_EDITOR_FONT_SIZE: f32 = 32.0;
pub const DEFAULT_EDITOR_LINE_NUMBERS: bool = true;
pub const DEFAULT_EDITOR_RELATIVE_LINE_NUMBERS: bool = true;
pub const DEFAULT_EDITOR_SOFT_WRAP: bool = true;
pub const DEFAULT_EDITOR_VIM_MODE: bool = true;
pub const DEFAULT_EXPERIMENTAL_AGENT_PANE: bool = false;
pub const DEFAULT_EXPERIMENTAL_EDITOR_PANE: bool = false;
static CONFIG_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The one agent key the client still owns: the adapter commands and the
/// auto-approve flag are mux options now, because the daemon spawns the
/// adapter. This one feeds pane creation, which is a client concern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentConfigKey {
    WorkingDirectory,
}

impl AgentConfigKey {
    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "agent-working-directory" => Some(Self::WorkingDirectory),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentConfig {
    pub working_directory: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigKey {
    UseSystemTitlebar,
    WindowCornerRadius,
    WindowBackgroundBlur,
    Animations,
    Tray,
    ShowFps,
    QuitDaemonOnExit,
    AutoRestartStaleDaemon,
    CheckForUpdates,
    StatusShowSession,
    StatusBadges,
    StatusAlign,
    StatusAgents,
    StatusHost,
    StatusUpdate,
    StatusClock,
    ExperimentalAgentPane,
    ExperimentalEditorPane,
    PaneGaps,
    PaneInactiveOpacity,
    PaneCornerRadius,
    PaneMargin,
    PaneBorderWidth,
    WidgetCornerRadius,
    ShadowStrength,
    EditorFontSize,
    EditorLineNumbers,
    EditorRelativeLineNumbers,
    EditorSoftWrap,
    EditorVimMode,
    BrowserElementSelectorHotkey,
    BrowserSearchProvider,
    BrowserEgress,
    ThemeMode,
    UiFontFamily,
    AppIcon,
    ChromePreset,
    Chrome(ChromeColor),
}

impl ConfigKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UseSystemTitlebar => "use-system-titlebar",
            Self::WindowCornerRadius => "window-corner-radius",
            Self::WindowBackgroundBlur => "window-background-blur",
            Self::Animations => "animations",
            Self::Tray => "tray",
            Self::ShowFps => "show-fps",
            Self::QuitDaemonOnExit => "quit-daemon-on-exit",
            Self::AutoRestartStaleDaemon => "auto-restart-stale-daemon",
            Self::CheckForUpdates => "check-for-updates",
            Self::StatusShowSession => "status-show-session",
            Self::StatusBadges => "status-badges",
            Self::StatusAlign => "status-align",
            Self::StatusAgents => "status-agents",
            Self::StatusHost => "status-host",
            Self::StatusUpdate => "status-update",
            Self::StatusClock => "status-clock",
            Self::ExperimentalAgentPane => "experimental-agent-pane",
            Self::ExperimentalEditorPane => "experimental-editor-pane",
            Self::PaneGaps => "pane-gaps",
            Self::PaneInactiveOpacity => "pane-inactive-opacity",
            Self::PaneCornerRadius => "pane-corner-radius",
            Self::PaneMargin => "pane-margin",
            Self::PaneBorderWidth => "pane-border-width",
            Self::WidgetCornerRadius => "widget-corner-radius",
            Self::ShadowStrength => "shadow-strength",
            Self::EditorFontSize => "editor-font-size",
            Self::EditorLineNumbers => "editor-line-numbers",
            Self::EditorRelativeLineNumbers => "editor-relative-line-numbers",
            Self::EditorSoftWrap => "editor-soft-wrap",
            Self::EditorVimMode => "editor-vim-mode",
            Self::BrowserElementSelectorHotkey => "browser-element-selector-hotkey",
            Self::BrowserSearchProvider => "browser-search-provider",
            Self::BrowserEgress => "browser-egress",
            Self::ThemeMode => "theme-mode",
            Self::UiFontFamily => "ui-font-family",
            Self::AppIcon => "app-icon",
            Self::ChromePreset => "chrome-preset",
            Self::Chrome(color) => color.as_str(),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "use-system-titlebar" => Some(Self::UseSystemTitlebar),
            "window-corner-radius" => Some(Self::WindowCornerRadius),
            "window-background-blur" => Some(Self::WindowBackgroundBlur),
            "animations" => Some(Self::Animations),
            "tray" => Some(Self::Tray),
            "show-fps" => Some(Self::ShowFps),
            "quit-daemon-on-exit" => Some(Self::QuitDaemonOnExit),
            "auto-restart-stale-daemon" => Some(Self::AutoRestartStaleDaemon),
            "check-for-updates" => Some(Self::CheckForUpdates),
            "status-show-session" => Some(Self::StatusShowSession),
            "status-badges" => Some(Self::StatusBadges),
            "status-align" => Some(Self::StatusAlign),
            "status-agents" => Some(Self::StatusAgents),
            "status-host" => Some(Self::StatusHost),
            "status-update" => Some(Self::StatusUpdate),
            "status-clock" => Some(Self::StatusClock),
            "experimental-agent-pane" => Some(Self::ExperimentalAgentPane),
            "experimental-editor-pane" => Some(Self::ExperimentalEditorPane),
            "pane-gaps" => Some(Self::PaneGaps),
            "pane-inactive-opacity" => Some(Self::PaneInactiveOpacity),
            "pane-corner-radius" => Some(Self::PaneCornerRadius),
            "pane-margin" => Some(Self::PaneMargin),
            "pane-border-width" => Some(Self::PaneBorderWidth),
            "widget-corner-radius" => Some(Self::WidgetCornerRadius),
            "shadow-strength" => Some(Self::ShadowStrength),
            "editor-font-size" => Some(Self::EditorFontSize),
            "editor-line-numbers" => Some(Self::EditorLineNumbers),
            "editor-relative-line-numbers" => Some(Self::EditorRelativeLineNumbers),
            "editor-soft-wrap" => Some(Self::EditorSoftWrap),
            "editor-vim-mode" => Some(Self::EditorVimMode),
            "browser-element-selector-hotkey" => Some(Self::BrowserElementSelectorHotkey),
            "browser-search-provider" => Some(Self::BrowserSearchProvider),
            "browser-egress" => Some(Self::BrowserEgress),
            "theme-mode" => Some(Self::ThemeMode),
            "ui-font-family" => Some(Self::UiFontFamily),
            "app-icon" => Some(Self::AppIcon),
            "chrome-preset" => Some(Self::ChromePreset),
            _ => ChromeColor::parse(key).map(Self::Chrome),
        }
    }

    /// The inclusive range this numeric key accepts, or `None` for a key that
    /// is not numeric.
    pub const fn numeric_range(self) -> Option<(f32, f32)> {
        match self {
            Self::PaneInactiveOpacity => {
                Some((MIN_PANE_INACTIVE_OPACITY, MAX_PANE_INACTIVE_OPACITY))
            }
            Self::PaneMargin => Some((0.0, MAX_PANE_MARGIN)),
            Self::PaneCornerRadius => Some((0.0, MAX_PANE_CORNER_RADIUS)),
            Self::PaneBorderWidth => Some((0.0, MAX_PANE_BORDER_WIDTH)),
            Self::WidgetCornerRadius => Some((0.0, MAX_WIDGET_CORNER_RADIUS)),
            Self::ShadowStrength => Some((0.0, 1.0)),
            Self::WindowCornerRadius => Some((0.0, MAX_WINDOW_CORNER_RADIUS)),
            Self::EditorFontSize => Some((MIN_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE)),
            Self::UseSystemTitlebar
            | Self::WindowBackgroundBlur
            | Self::Animations
            | Self::Tray
            | Self::ShowFps
            | Self::QuitDaemonOnExit
            | Self::AutoRestartStaleDaemon
            | Self::CheckForUpdates
            | Self::StatusShowSession
            | Self::StatusBadges
            | Self::StatusAlign
            | Self::StatusAgents
            | Self::StatusHost
            | Self::StatusUpdate
            | Self::StatusClock
            | Self::ExperimentalAgentPane
            | Self::ExperimentalEditorPane
            | Self::PaneGaps
            | Self::EditorLineNumbers
            | Self::EditorRelativeLineNumbers
            | Self::EditorSoftWrap
            | Self::EditorVimMode
            | Self::BrowserElementSelectorHotkey
            | Self::BrowserSearchProvider
            | Self::BrowserEgress
            | Self::ThemeMode
            | Self::UiFontFamily
            | Self::AppIcon
            | Self::ChromePreset
            | Self::Chrome(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConfigProvenance {
    #[default]
    Default,
    Override,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConfigValue<T> {
    pub value: T,
    pub provenance: ConfigProvenance,
}

impl<T> ConfigValue<T> {
    pub const fn from_default(value: T) -> Self {
        Self {
            value,
            provenance: ConfigProvenance::Default,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrowserConfig {
    pub element_selector_hotkey: ConfigValue<String>,
    pub search_provider: ConfigValue<SearchProvider>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            element_selector_hotkey: ConfigValue::from_default(
                DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY.to_owned(),
            ),
            search_provider: ConfigValue::from_default(DEFAULT_BROWSER_SEARCH_PROVIDER),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiFontConfig {
    pub family: ConfigValue<Option<String>>,
}

impl Default for UiFontConfig {
    fn default() -> Self {
        Self {
            family: ConfigValue::from_default(None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppConfig {
    pub use_system_titlebar: ConfigValue<bool>,
    pub window_corner_radius: ConfigValue<f32>,
    pub window_background_blur: ConfigValue<bool>,
    pub animations: ConfigValue<bool>,
    pub tray: ConfigValue<bool>,
    pub show_fps: ConfigValue<bool>,
    pub quit_daemon_on_exit: ConfigValue<bool>,
    pub auto_restart_stale_daemon: ConfigValue<bool>,
    pub check_for_updates: ConfigValue<bool>,
    pub status_show_session: ConfigValue<bool>,
    pub status_badges: ConfigValue<bool>,
    pub status_alignment: ConfigValue<StatusBarAlignment>,
    pub status_agents: ConfigValue<bool>,
    pub status_host: ConfigValue<bool>,
    pub status_update: ConfigValue<bool>,
    pub status_clock: ConfigValue<StatusBarClock>,
    pub experimental_agent_pane: ConfigValue<bool>,
    pub experimental_editor_pane: ConfigValue<bool>,
    pub pane_gaps: ConfigValue<bool>,
    pub pane_inactive_opacity: ConfigValue<f32>,
    pub pane_corner_radius: ConfigValue<f32>,
    pub pane_margin: ConfigValue<f32>,
    pub pane_border_width: ConfigValue<f32>,
    pub widget_corner_radius: ConfigValue<f32>,
    pub shadow_strength: ConfigValue<f32>,
    pub editor_font_size: ConfigValue<f32>,
    pub editor_line_numbers: ConfigValue<bool>,
    pub editor_relative_line_numbers: ConfigValue<bool>,
    pub editor_soft_wrap: ConfigValue<bool>,
    pub editor_vim_mode: ConfigValue<bool>,
    pub browser_egress: ConfigValue<bool>,
    pub theme_mode: ConfigValue<ThemeModeSetting>,
    pub app_icon: ConfigValue<AppIconSetting>,
    /// A paired light/dark palette family. Individual `chrome-*` keys remain
    /// higher-priority overrides over its active variant.
    pub chrome_preset: ConfigValue<Option<ChromePresetId>>,
    /// Chrome palette overrides, in [`ChromeColor::ALL`] order. `None` inherits
    /// the active preset or the built-in palette.
    pub chrome_colors: [ConfigValue<Option<[f32; 4]>>; ChromeColor::ALL.len()],
}

impl Default for AppConfig {
    fn default() -> Self {
        let status_bar = StatusBarSettings::default();
        Self {
            use_system_titlebar: ConfigValue::from_default(DEFAULT_USE_SYSTEM_TITLEBAR),
            window_corner_radius: ConfigValue::from_default(DEFAULT_WINDOW_CORNER_RADIUS),
            window_background_blur: ConfigValue::from_default(DEFAULT_WINDOW_BACKGROUND_BLUR),
            animations: ConfigValue::from_default(DEFAULT_ANIMATIONS),
            tray: ConfigValue::from_default(DEFAULT_TRAY),
            show_fps: ConfigValue::from_default(DEFAULT_SHOW_FPS),
            quit_daemon_on_exit: ConfigValue::from_default(DEFAULT_QUIT_DAEMON_ON_EXIT),
            auto_restart_stale_daemon: ConfigValue::from_default(DEFAULT_AUTO_RESTART_STALE_DAEMON),
            check_for_updates: ConfigValue::from_default(DEFAULT_CHECK_FOR_UPDATES),
            status_show_session: ConfigValue::from_default(status_bar.show_session),
            status_badges: ConfigValue::from_default(status_bar.badges),
            status_alignment: ConfigValue::from_default(status_bar.alignment),
            status_agents: ConfigValue::from_default(status_bar.show_agents),
            status_host: ConfigValue::from_default(status_bar.show_host),
            status_update: ConfigValue::from_default(status_bar.show_update),
            status_clock: ConfigValue::from_default(status_bar.clock),
            experimental_agent_pane: ConfigValue::from_default(DEFAULT_EXPERIMENTAL_AGENT_PANE),
            experimental_editor_pane: ConfigValue::from_default(DEFAULT_EXPERIMENTAL_EDITOR_PANE),
            pane_gaps: ConfigValue::from_default(DEFAULT_PANE_GAPS),
            pane_inactive_opacity: ConfigValue::from_default(DEFAULT_PANE_INACTIVE_OPACITY),
            pane_corner_radius: ConfigValue::from_default(DEFAULT_PANE_CORNER_RADIUS),
            pane_margin: ConfigValue::from_default(DEFAULT_PANE_MARGIN),
            pane_border_width: ConfigValue::from_default(DEFAULT_PANE_BORDER_WIDTH),
            widget_corner_radius: ConfigValue::from_default(DEFAULT_WIDGET_CORNER_RADIUS),
            shadow_strength: ConfigValue::from_default(DEFAULT_SHADOW_STRENGTH),
            editor_font_size: ConfigValue::from_default(DEFAULT_EDITOR_FONT_SIZE),
            editor_line_numbers: ConfigValue::from_default(DEFAULT_EDITOR_LINE_NUMBERS),
            editor_relative_line_numbers: ConfigValue::from_default(
                DEFAULT_EDITOR_RELATIVE_LINE_NUMBERS,
            ),
            editor_soft_wrap: ConfigValue::from_default(DEFAULT_EDITOR_SOFT_WRAP),
            editor_vim_mode: ConfigValue::from_default(DEFAULT_EDITOR_VIM_MODE),
            browser_egress: ConfigValue::from_default(DEFAULT_BROWSER_EGRESS),
            theme_mode: ConfigValue::from_default(ThemeModeSetting::System),
            app_icon: ConfigValue::from_default(AppIconSetting::Automatic),
            chrome_preset: ConfigValue::from_default(None),
            chrome_colors: [ConfigValue::from_default(None); ChromeColor::ALL.len()],
        }
    }
}

impl AppConfig {
    fn boolean_value_mut(&mut self, key: ConfigKey) -> Option<&mut ConfigValue<bool>> {
        match key {
            ConfigKey::UseSystemTitlebar => Some(&mut self.use_system_titlebar),
            ConfigKey::WindowBackgroundBlur => Some(&mut self.window_background_blur),
            ConfigKey::Animations => Some(&mut self.animations),
            ConfigKey::Tray => Some(&mut self.tray),
            ConfigKey::ShowFps => Some(&mut self.show_fps),
            ConfigKey::QuitDaemonOnExit => Some(&mut self.quit_daemon_on_exit),
            ConfigKey::AutoRestartStaleDaemon => Some(&mut self.auto_restart_stale_daemon),
            ConfigKey::CheckForUpdates => Some(&mut self.check_for_updates),
            ConfigKey::StatusShowSession => Some(&mut self.status_show_session),
            ConfigKey::StatusBadges => Some(&mut self.status_badges),
            ConfigKey::StatusAgents => Some(&mut self.status_agents),
            ConfigKey::StatusHost => Some(&mut self.status_host),
            ConfigKey::StatusUpdate => Some(&mut self.status_update),
            ConfigKey::ExperimentalAgentPane => Some(&mut self.experimental_agent_pane),
            ConfigKey::ExperimentalEditorPane => Some(&mut self.experimental_editor_pane),
            ConfigKey::PaneGaps => Some(&mut self.pane_gaps),
            ConfigKey::EditorLineNumbers => Some(&mut self.editor_line_numbers),
            ConfigKey::EditorRelativeLineNumbers => Some(&mut self.editor_relative_line_numbers),
            ConfigKey::EditorSoftWrap => Some(&mut self.editor_soft_wrap),
            ConfigKey::EditorVimMode => Some(&mut self.editor_vim_mode),
            ConfigKey::BrowserEgress => Some(&mut self.browser_egress),
            ConfigKey::WindowCornerRadius
            | ConfigKey::PaneInactiveOpacity
            | ConfigKey::PaneCornerRadius
            | ConfigKey::PaneMargin
            | ConfigKey::PaneBorderWidth
            | ConfigKey::WidgetCornerRadius
            | ConfigKey::ShadowStrength
            | ConfigKey::EditorFontSize
            | ConfigKey::BrowserElementSelectorHotkey
            | ConfigKey::BrowserSearchProvider
            | ConfigKey::StatusAlign
            | ConfigKey::StatusClock
            | ConfigKey::ThemeMode
            | ConfigKey::UiFontFamily
            | ConfigKey::AppIcon
            | ConfigKey::ChromePreset
            | ConfigKey::Chrome(_) => None,
        }
    }

    /// This root's override, if the user set one.
    pub fn chrome(&self, color: ChromeColor) -> ConfigValue<Option<[f32; 4]>> {
        self.chrome_colors[chrome_index(color)]
    }
}

pub fn chrome_index(color: ChromeColor) -> usize {
    ChromeColor::ALL
        .iter()
        .position(|candidate| *candidate == color)
        .expect("ChromeColor::ALL contains every variant")
}

#[derive(Debug, Eq, PartialEq)]
pub struct ConfigDiagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct ParsedConfig {
    pub config: AppConfig,
    pub browser: BrowserConfig,
    pub ui_font: UiFontConfig,
    pub agent: AgentConfig,
    pub hosts: Vec<HostEntry>,
    pub rejected_hosts: Vec<RejectedHost>,
    pub daemon_entries: Vec<ConfigOverrideEntry>,
    pub chrome_overrides: Vec<ChromeOverride>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ImportError {
    UnserializableValue { key: &'static str, reason: String },
    TooLarge { bytes: usize },
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnserializableValue { key, reason } => {
                write!(formatter, "cannot import `{key}`: {reason}")
            }
            Self::TooLarge { bytes } => write!(
                formatter,
                "configuration import would produce {bytes} bytes, exceeding the {MAX_CONFIG_BYTES}-byte limit"
            ),
        }
    }
}

impl std::error::Error for ImportError {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigFileStamp {
    pub path: Option<PathBuf>,
    pub modified: Option<SystemTime>,
    pub len: Option<u64>,
}

impl ConfigFileStamp {
    pub fn detect(candidates: &[PathBuf]) -> Self {
        let Some(path) = discover_config_path(candidates) else {
            return Self::default();
        };
        let metadata = fs::metadata(&path).ok();
        Self {
            modified: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok()),
            len: metadata.as_ref().map(fs::Metadata::len),
            path: Some(path),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigPlatform {
    Unix,

    Macos,

    Windows,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigEnvironment<'a> {
    pub xdg_config_home: Option<&'a Path>,
    pub home: Option<&'a Path>,

    pub appdata: Option<&'a Path>,

    pub local_appdata: Option<&'a Path>,

    pub user_profile: Option<&'a Path>,
}

pub fn current_config_platform() -> ConfigPlatform {
    #[cfg(target_os = "macos")]
    {
        ConfigPlatform::Macos
    }
    #[cfg(target_os = "windows")]
    {
        ConfigPlatform::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ConfigPlatform::Unix
    }
}

pub fn config_candidates() -> Vec<PathBuf> {
    let xdg_config_home = nonempty_env("XDG_CONFIG_HOME");
    let home = nonempty_env("HOME");

    let appdata = nonempty_env("APPDATA");

    let local_appdata = nonempty_env("LOCALAPPDATA");

    let user_profile = nonempty_env("USERPROFILE");

    config_candidates_for(
        current_config_platform(),
        ConfigEnvironment {
            xdg_config_home: xdg_config_home.as_deref(),
            home: home.as_deref(),

            appdata: appdata.as_deref(),

            local_appdata: local_appdata.as_deref(),

            user_profile: user_profile.as_deref(),
        },
    )
}

pub fn config_candidates_for(
    platform: ConfigPlatform,
    environment: ConfigEnvironment<'_>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_config_candidate(&mut candidates, environment.xdg_config_home);

    match platform {
        ConfigPlatform::Unix => {
            push_home_config_candidate(&mut candidates, environment.home);
        }

        ConfigPlatform::Macos => {
            push_home_config_candidate(&mut candidates, environment.home);
            if let Some(home) = environment.home {
                push_config_candidate_path(
                    &mut candidates,
                    &home.join("Library/Application Support"),
                );
            }
        }

        ConfigPlatform::Windows => {
            push_config_candidate(&mut candidates, environment.appdata);
            push_config_candidate(&mut candidates, environment.local_appdata);
            push_home_config_candidate(&mut candidates, environment.user_profile);
            push_home_config_candidate(&mut candidates, environment.home);
        }
    }

    candidates
}

pub fn push_home_config_candidate(candidates: &mut Vec<PathBuf>, home: Option<&Path>) {
    if let Some(home) = home {
        push_config_candidate_path(candidates, &home.join(".config"));
    }
}

pub fn push_config_candidate(candidates: &mut Vec<PathBuf>, base: Option<&Path>) {
    let Some(base) = base else {
        return;
    };
    push_config_candidate_path(candidates, base);
}

pub fn push_config_candidate_path(candidates: &mut Vec<PathBuf>, base: &Path) {
    if !base.is_absolute() {
        return;
    }
    let candidate = base.join(CONFIG_DIRECTORY_NAME).join(CONFIG_FILE_NAME);
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

pub fn discover_config_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
}

pub fn preferred_config_creation_path(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    push_config_candidate(&mut candidates, xdg_config_home);
    if candidates.is_empty() {
        push_home_config_candidate(&mut candidates, home);
    }
    candidates.into_iter().next()
}

pub fn config_path_for_write() -> io::Result<PathBuf> {
    let candidates = config_candidates();
    if let Some(path) = discover_config_path(&candidates) {
        return Ok(path);
    }

    let xdg_config_home = nonempty_env("XDG_CONFIG_HOME");
    let home = nonempty_env("HOME");
    preferred_config_creation_path(xdg_config_home.as_deref(), home.as_deref()).ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            "cannot create zz/config because neither XDG_CONFIG_HOME nor HOME is available",
        )
    })
}

pub fn import_target_path() -> io::Result<PathBuf> {
    config_path_for_write()
}

pub fn nonempty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn load_config(path: &Path, system_font_family: &str) -> io::Result<ParsedConfig> {
    read_config_source(path).map(|source| parse_config(&source, system_font_family))
}

#[cfg(not(target_os = "ios"))]
pub fn tray_enabled_from_files(candidates: &[PathBuf]) -> bool {
    discover_config_path(candidates)
        .and_then(|path| load_config(&path, "").ok())
        .map_or(DEFAULT_TRAY, |parsed| parsed.config.tray.value)
}

pub fn read_config_source(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let byte_limit = u64::try_from(MAX_CONFIG_BYTES).unwrap_or(u64::MAX - 1);
    let mut source = String::new();
    file.take(byte_limit + 1).read_to_string(&mut source)?;
    if source.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    Ok(source)
}

/// Read a settings editor file, capped at `max_bytes`. A missing file starts as
/// an empty editor.
pub fn read_config_editor_source(path: &Path, max_bytes: usize) -> io::Result<String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    let byte_limit = u64::try_from(max_bytes).unwrap_or(u64::MAX - 1);
    let mut source = String::new();
    file.take(byte_limit + 1).read_to_string(&mut source)?;
    if source.len() > max_bytes {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {max_bytes}-byte editor limit"),
        ));
    }
    Ok(source)
}

/// Atomically replace a settings editor file after enforcing its surface's cap.
pub fn write_config_editor_source(path: &Path, source: &str, max_bytes: usize) -> io::Result<()> {
    if source.len() > max_bytes {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {max_bytes}-byte editor limit"),
        ));
    }
    atomic_write(path, source.as_bytes())
}

pub fn parse_config(source: &str, system_font_family: &str) -> ParsedConfig {
    let mut parsed = ParsedConfig::default();

    for (index, raw_line) in source.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            parsed.diagnostics.push(ConfigDiagnostic {
                line: line_number,
                message: "expected `key = value`".to_owned(),
            });
            continue;
        };
        let key = key.trim();
        let value = config_value_without_comment(value).trim();

        if let Some(name) = key.strip_prefix("host-") {
            if let Some(message) = zz_daemon::apply_fleet_host_entry(
                &mut parsed.hosts,
                &mut parsed.rejected_hosts,
                key,
                name,
                value,
            ) {
                parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message,
                });
            }
            continue;
        }

        if let Some(agent_key) = AgentConfigKey::parse(key) {
            if let Some(diagnostic) =
                apply_agent_key(&mut parsed.agent, agent_key, key, value, line_number)
            {
                parsed.diagnostics.push(diagnostic);
            }
            continue;
        }

        if matches!(key, CHROME_KEYBIND_KEY | CHROME_UNBIND_KEY) {
            let entry = if key == CHROME_KEYBIND_KEY {
                keymap::parse_bind(value)
            } else {
                keymap::parse_unbind(value)
            };
            match entry {
                Ok(entry) => parsed.chrome_overrides.push(entry),
                Err(message) => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!("invalid `{key}`: {message}"),
                }),
            }
            continue;
        }

        let Some(key) = ConfigKey::parse(key) else {
            if AppearanceConfigKey::from_config_key(key).is_some()
                || MuxOptionKey::from_config_key(key).is_some()
            {
                parsed
                    .daemon_entries
                    .push((key.to_owned(), value.to_owned()));
                continue;
            }
            parsed.diagnostics.push(ConfigDiagnostic {
                line: line_number,
                message: format!("unsupported key `{key}`"),
            });
            continue;
        };

        if matches!(
            key,
            ConfigKey::ExperimentalAgentPane | ConfigKey::ExperimentalEditorPane
        ) {
            parsed
                .daemon_entries
                .push((key.as_str().to_owned(), value.to_owned()));
        }

        if let Some(target) = parsed.config.boolean_value_mut(key) {
            target.provenance = ConfigProvenance::Override;
            match parse_boolean(value) {
                Ok(enabled) => target.value = enabled,
                Err(message) => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!("invalid `{}`: {message}", key.as_str()),
                }),
            }
            continue;
        }

        if key == ConfigKey::UiFontFamily {
            let target = &mut parsed.ui_font.family;
            target.provenance = ConfigProvenance::Override;
            match parse_config_string(value).and_then(|family| {
                validate_ui_font_family(&family).map_err(str::to_owned)?;
                Ok(family)
            }) {
                Ok(family) => {
                    target.value = (family != system_font_family).then_some(family);
                }
                Err(message) => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!("invalid `{}`: {message}", key.as_str()),
                }),
            }
            continue;
        }

        if key == ConfigKey::BrowserElementSelectorHotkey {
            let target = &mut parsed.browser.element_selector_hotkey;
            target.provenance = ConfigProvenance::Override;
            match normalize_browser_hotkey(value) {
                Ok(value) => target.value = value,
                Err(message) => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!("invalid `{}`: {message}", key.as_str()),
                }),
            }
            continue;
        }

        if key == ConfigKey::BrowserSearchProvider {
            let target = &mut parsed.browser.search_provider;
            target.provenance = ConfigProvenance::Override;
            match SearchProvider::parse(value) {
                Some(provider) => target.value = provider,
                None => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!(
                        "invalid `{}`: expected one of {}",
                        key.as_str(),
                        SearchProvider::ALL.map(SearchProvider::as_str).join(", "),
                    ),
                }),
            }
            continue;
        }

        if key == ConfigKey::StatusAlign {
            let target = &mut parsed.config.status_alignment;
            target.provenance = ConfigProvenance::Override;
            match parse_status_bar_alignment(value) {
                Some(alignment) => target.value = alignment,
                None => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!("invalid `{}`: expected left or center", key.as_str()),
                }),
            }
            continue;
        }

        if key == ConfigKey::StatusClock {
            let target = &mut parsed.config.status_clock;
            target.provenance = ConfigProvenance::Override;
            match parse_status_bar_clock(value) {
                Some(clock) => target.value = clock,
                None => parsed.diagnostics.push(ConfigDiagnostic {
                    line: line_number,
                    message: format!(
                        "invalid `{}`: expected 24-hour, 12-hour, time-date or off",
                        key.as_str(),
                    ),
                }),
            }
            continue;
        }

        if let Some(diagnostic) = apply_theme_key(&mut parsed.config, key, value, line_number) {
            parsed.diagnostics.push(diagnostic);
            continue;
        }
        if matches!(
            key,
            ConfigKey::ThemeMode
                | ConfigKey::AppIcon
                | ConfigKey::ChromePreset
                | ConfigKey::Chrome(_)
        ) {
            continue;
        }

        let target = match key {
            ConfigKey::WindowCornerRadius => &mut parsed.config.window_corner_radius,
            ConfigKey::PaneInactiveOpacity => &mut parsed.config.pane_inactive_opacity,
            ConfigKey::PaneCornerRadius => &mut parsed.config.pane_corner_radius,
            ConfigKey::PaneMargin => &mut parsed.config.pane_margin,
            ConfigKey::PaneBorderWidth => &mut parsed.config.pane_border_width,
            ConfigKey::WidgetCornerRadius => &mut parsed.config.widget_corner_radius,
            ConfigKey::ShadowStrength => &mut parsed.config.shadow_strength,
            ConfigKey::EditorFontSize => &mut parsed.config.editor_font_size,
            ConfigKey::UseSystemTitlebar
            | ConfigKey::WindowBackgroundBlur
            | ConfigKey::Animations
            | ConfigKey::Tray
            | ConfigKey::ShowFps
            | ConfigKey::QuitDaemonOnExit
            | ConfigKey::AutoRestartStaleDaemon
            | ConfigKey::CheckForUpdates
            | ConfigKey::StatusShowSession
            | ConfigKey::StatusBadges
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
            | ConfigKey::StatusAlign
            | ConfigKey::StatusAgents
            | ConfigKey::StatusHost
            | ConfigKey::StatusUpdate
            | ConfigKey::StatusClock
            | ConfigKey::ThemeMode
            | ConfigKey::UiFontFamily
            | ConfigKey::AppIcon
            | ConfigKey::ChromePreset
            | ConfigKey::Chrome(_) => {
                unreachable!("handled above")
            }
        };
        target.provenance = ConfigProvenance::Override;

        let range = key
            .numeric_range()
            .expect("every key reaching here is a numeric value");
        match parse_numeric_value(key, value, range) {
            Ok(value) => target.value = value,
            Err(message) => parsed.diagnostics.push(ConfigDiagnostic {
                line: line_number,
                message: format!("invalid `{}`: {message}", key.as_str()),
            }),
        }
    }

    parsed
}

pub fn write_fleet_host(name: &str, endpoint: &str) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_fleet_host_at(&path, name, endpoint)
}

pub fn write_fleet_host_at(path: &Path, name: &str, endpoint: &str) -> io::Result<()> {
    validate_fleet_host(name, endpoint)
        .map_err(|message| io::Error::new(ErrorKind::InvalidInput, message))?;
    write_config_edit_at(path, &format!("host-{name}"), Some(endpoint)).map(drop)
}

pub fn remove_fleet_host(name: &str) -> io::Result<bool> {
    let path = config_path_for_write()?;
    remove_fleet_host_at(&path, name)
}

pub fn remove_fleet_host_at(path: &Path, name: &str) -> io::Result<bool> {
    let key = format!("host-{name}");
    let mut removed = false;
    while write_config_edit_at(path, &key, None)? {
        removed = true;
    }
    Ok(removed)
}

pub fn apply_agent_key(
    agent: &mut AgentConfig,
    agent_key: AgentConfigKey,
    key: &str,
    value: &str,
    line_number: usize,
) -> Option<ConfigDiagnostic> {
    let invalid = |message: &str| {
        Some(ConfigDiagnostic {
            line: line_number,
            message: format!("invalid `{key}`: {message}"),
        })
    };
    let string_value = || parse_config_string(value);
    match agent_key {
        AgentConfigKey::WorkingDirectory => match string_value() {
            Ok(value) => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return invalid("expected an absolute path");
                }
                if path.as_os_str().as_encoded_bytes().len() > MAX_GUI_TEXT_BYTES {
                    return invalid("path exceeds the wire byte limit");
                }
                agent.working_directory = Some(path);
            }
            Err(message) => return invalid(&message),
        },
    }
    None
}

pub fn apply_theme_key(
    config: &mut AppConfig,
    key: ConfigKey,
    value: &str,
    line_number: usize,
) -> Option<ConfigDiagnostic> {
    let message = match key {
        ConfigKey::Chrome(color) => {
            let target = &mut config.chrome_colors[chrome_index(color)];
            target.provenance = ConfigProvenance::Override;
            match zz_client::chrome_palette::parse_hex(value) {
                Ok(parsed) => {
                    target.value = Some(parsed);
                    return None;
                }
                Err(message) => message,
            }
        }
        ConfigKey::ThemeMode => {
            config.theme_mode.provenance = ConfigProvenance::Override;
            match ThemeModeSetting::parse(value) {
                Some(mode) => {
                    config.theme_mode.value = mode;
                    return None;
                }
                None => "expected system, light or dark".to_owned(),
            }
        }
        ConfigKey::AppIcon => {
            config.app_icon.provenance = ConfigProvenance::Override;
            match AppIconSetting::parse(value) {
                Some(setting) => {
                    config.app_icon.value = setting;
                    return None;
                }
                None => "expected automatic, light or dark".to_owned(),
            }
        }
        ConfigKey::ChromePreset => {
            config.chrome_preset.provenance = ConfigProvenance::Override;
            match ChromePresetId::parse(value) {
                Some(preset) => {
                    config.chrome_preset.value = Some(preset);
                    return None;
                }
                None => {
                    "expected tokyo-night, catppuccin, gruvbox, nord, breeze, adwaita, ubuntu, \
                     rose-pine, ayu, solarized or macos-classic"
                        .to_owned()
                }
            }
        }
        _ => return None,
    };
    Some(ConfigDiagnostic {
        line: line_number,
        message: format!("invalid `{}`: {message}", key.as_str()),
    })
}

pub fn config_value_without_comment(value: &str) -> &str {
    config_comment_start(value).map_or(value, |index| &value[..index])
}

pub fn config_comment_start(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut saw_value = false;
    let mut previous_was_whitespace = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            saw_value |= !character.is_whitespace();
            previous_was_whitespace = character.is_whitespace();
            continue;
        }
        match character {
            '\\' if quote.is_some() => escaped = true,
            '"' | '\'' if quote == Some(character) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(character),
            '#' if quote.is_none() && saw_value && previous_was_whitespace => {
                return Some(index);
            }
            _ => {}
        }
        saw_value |= !character.is_whitespace();
        previous_was_whitespace = character.is_whitespace();
    }
    None
}

pub fn normalize_browser_hotkey(value: &str) -> Result<String, String> {
    let hotkey = keymap::parse_keystroke(value.trim()).ok_or_else(|| {
        "expected modifiers and a key, for example `cmd-shift-c` or `ctrl-shift-c`".to_owned()
    })?;
    if !(hotkey.key.control || hotkey.key.alt || hotkey.key.command || hotkey.function) {
        return Err("expected Control, Alt, Command/Super, or Function as a modifier".into());
    }
    Ok(hotkey.spelling())
}

pub fn parse_numeric_value(
    key: ConfigKey,
    value: &str,
    (min, max): (f32, f32),
) -> Result<f32, String> {
    let unit = if key == ConfigKey::PaneInactiveOpacity {
        ""
    } else {
        " logical pixels"
    };
    let value = value
        .parse::<f32>()
        .map_err(|_| format!("expected a number{unit}"))?;
    if !value.is_finite() {
        return Err("value must be finite".to_owned());
    }
    if !(min..=max).contains(&value) {
        return Err(format!("value must be between {min} and {max}{unit}"));
    }
    Ok(value)
}

pub fn parse_boolean(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "yes" | "1" | "true" => Ok(true),
        "off" | "no" | "0" | "false" => Ok(false),
        _ => Err("expected a boolean (`true`/`on`/`yes`/`1` or `false`/`off`/`no`/`0`)".to_owned()),
    }
}

pub const fn status_bar_alignment_value(alignment: StatusBarAlignment) -> &'static str {
    match alignment {
        StatusBarAlignment::Left => "left",
        StatusBarAlignment::Center => "center",
    }
}

pub fn parse_status_bar_alignment(value: &str) -> Option<StatusBarAlignment> {
    match value {
        "left" => Some(StatusBarAlignment::Left),
        "center" => Some(StatusBarAlignment::Center),
        _ => None,
    }
}

pub const fn status_bar_clock_value(clock: StatusBarClock) -> &'static str {
    match clock {
        StatusBarClock::TwentyFourHour => "24-hour",
        StatusBarClock::TwelveHour => "12-hour",
        StatusBarClock::TimeAndDate => "time-date",
        StatusBarClock::Off => "off",
    }
}

pub fn parse_status_bar_clock(value: &str) -> Option<StatusBarClock> {
    match value {
        "24-hour" => Some(StatusBarClock::TwentyFourHour),
        "12-hour" => Some(StatusBarClock::TwelveHour),
        "time-date" => Some(StatusBarClock::TimeAndDate),
        "off" => Some(StatusBarClock::Off),
        _ => None,
    }
}

pub fn parse_config_string(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("value must not be empty".to_owned());
    }
    let bytes = value.as_bytes();
    if matches!(bytes.first(), Some(b'\'' | b'"')) {
        let quote = bytes[0];
        if bytes.last() != Some(&quote) || bytes.len() < 2 {
            return Err("unterminated quoted value".to_owned());
        }
        let mut parsed = String::with_capacity(value.len().saturating_sub(2));
        let mut escaped = false;
        for character in value[1..value.len() - 1].chars() {
            if escaped {
                parsed.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else {
                parsed.push(character);
            }
        }
        if escaped {
            return Err("quoted value ends with an incomplete escape".to_owned());
        }
        if parsed.is_empty() {
            return Err("value must not be empty".to_owned());
        }
        Ok(parsed)
    } else {
        Ok(value.to_owned())
    }
}

/// Write imported appearance values into `zz/config`, donor wins: each key is
/// replaced in place when it exists and appended otherwise. Cumulative keys
/// drop every prior occurrence; an empty group is a pure removal.
pub fn import_appearance_values_at(
    path: &Path,
    values: &[(AppearanceConfigKey, Vec<String>)],
) -> io::Result<()> {
    let source = match read_config_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let edited = apply_import_edits(&source, values)?;
    atomic_write(path, edited.as_bytes())
}

pub fn apply_import_edits(
    source: &str,
    values: &[(AppearanceConfigKey, Vec<String>)],
) -> io::Result<String> {
    let mut edited = source.to_owned();
    for (key, group) in values {
        edited = if !is_cumulative_appearance_key(*key)
            && let [value] = group.as_slice()
        {
            edit_config_source(&edited, key.as_str(), Some(value))
        } else {
            replace_config_key_group(&edited, key.as_str(), group)
        };
        if edited.len() > MAX_CONFIG_BYTES {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                ImportError::TooLarge {
                    bytes: edited.len(),
                }
                .to_string(),
            ));
        }
    }
    Ok(edited)
}

pub fn is_cumulative_appearance_key(key: AppearanceConfigKey) -> bool {
    matches!(
        key,
        AppearanceConfigKey::Palette
            | AppearanceConfigKey::FontFamily
            | AppearanceConfigKey::FontFamilyBold
            | AppearanceConfigKey::FontFamilyItalic
            | AppearanceConfigKey::FontFamilyBoldItalic
            | AppearanceConfigKey::FontFeature
    )
}

pub fn is_app_config_key(key: &str) -> bool {
    ConfigKey::parse(key).is_some()
        || AgentConfigKey::parse(key).is_some()
        || MuxOptionKey::from_config_key(key).is_some()
        || key.starts_with("host-")
}

/// The Terminal page's editor buffer: `zz/config` with every app-side line
/// removed. Comments, blank lines, and unrecognized keys stay visible.
pub fn appearance_editor_view(source: &str) -> String {
    source
        .split_inclusive('\n')
        .filter(|line| !config_key_for_line(line).is_some_and(is_app_config_key))
        .collect()
}

/// Replace the appearance view of the file at `path` with `edited`, keeping every
/// app-side line. The inverse of [`appearance_editor_view`]. App-side keys in the
/// buffer are rejected, not spliced.
pub fn save_appearance_editor(path: &Path, edited: &str) -> io::Result<()> {
    for (index, line) in edited.lines().enumerate() {
        if let Some(key) = config_key_for_line(line).filter(|key| is_app_config_key(key)) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "line {}: `{key}` is managed by the other Settings pages, not this editor",
                    index + 1
                ),
            ));
        }
    }
    let source = match read_config_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut merged: String = source
        .split_inclusive('\n')
        .filter(|line| config_key_for_line(line).is_some_and(is_app_config_key))
        .collect();
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(edited);
    if merged.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration edit exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    atomic_write(path, merged.as_bytes())
}

pub fn replace_config_key_group(source: &str, key: &str, values: &[String]) -> String {
    let mut edited = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        if config_key_for_line(line) != Some(key) {
            edited.push_str(line);
        }
    }
    for value in values {
        edited = append_config_line(&edited, key, value);
    }
    edited
}

pub fn appearance_config_values(
    appearance: &TerminalAppearance,
    key: AppearanceConfigKey,
) -> Result<Vec<String>, ImportError> {
    let values = match key {
        AppearanceConfigKey::Theme => Vec::new(),
        AppearanceConfigKey::Background => vec![serialize_rgb(appearance.background)],
        AppearanceConfigKey::Foreground => vec![serialize_rgb(appearance.foreground)],
        AppearanceConfigKey::CursorColor => vec![serialize_rgb(appearance.cursor_color)],
        AppearanceConfigKey::SelectionForeground => {
            vec![serialize_rgb(appearance.selection_foreground)]
        }
        AppearanceConfigKey::SelectionBackground => {
            vec![serialize_rgba(appearance.selection_background)]
        }
        AppearanceConfigKey::Palette => {
            let defaults = TerminalAppearance::default();
            appearance
                .palette
                .as_array()
                .iter()
                .zip(defaults.palette.as_array())
                .enumerate()
                .filter(|(_, (color, default))| color != default)
                .map(|(index, (color, _))| format!("{index}={}", serialize_rgb(*color)))
                .collect()
        }
        AppearanceConfigKey::FontFamily => serialize_font_families(&appearance.font_families),
        AppearanceConfigKey::FontFamilyBold => {
            serialize_font_families(&appearance.font_families_bold)
        }
        AppearanceConfigKey::FontFamilyItalic => {
            serialize_font_families(&appearance.font_families_italic)
        }
        AppearanceConfigKey::FontFamilyBoldItalic => {
            serialize_font_families(&appearance.font_families_bold_italic)
        }
        AppearanceConfigKey::FontSize => vec![appearance.font_size_points.to_string()],
        AppearanceConfigKey::FontFeature => {
            let mut seen_tags = Vec::with_capacity(appearance.font_features.len());
            let mut values = Vec::new();
            for feature in &appearance.font_features {
                if seen_tags.contains(&feature.tag) {
                    return Err(ImportError::UnserializableValue {
                        key: key.as_str(),
                        reason: format!(
                            "resolved appearance contains duplicate `{}` feature tags",
                            feature.tag_string()
                        ),
                    });
                }
                seen_tags.push(feature.tag);
                values.push(format!("{}={}", feature.tag_string(), feature.value));
            }
            values
        }
        AppearanceConfigKey::FontSyntheticStyle => {
            vec![serialize_font_synthetic_style(
                appearance.font_synthetic_style,
            )]
        }
        AppearanceConfigKey::FontThicken => vec![appearance.font_thicken.to_string()],
        AppearanceConfigKey::FontThickenStrength => {
            vec![appearance.font_thicken_strength.to_string()]
        }
        AppearanceConfigKey::AdjustCellHeight => match appearance.cell_height_adjustment {
            CellHeightAdjustment::None => vec![String::new()],
            CellHeightAdjustment::Pixels(value) => vec![value.to_string()],
            CellHeightAdjustment::Percent(value) => vec![format!("{value}%")],
        },
        AppearanceConfigKey::WindowPaddingX => vec![format!(
            "{},{}",
            appearance.padding_left, appearance.padding_right
        )],
        AppearanceConfigKey::WindowPaddingY => vec![format!(
            "{},{}",
            appearance.padding_top, appearance.padding_bottom
        )],
        AppearanceConfigKey::MinimumContrast => vec![appearance.minimum_contrast.to_string()],
        AppearanceConfigKey::BackgroundOpacity => vec![appearance.background_opacity.to_string()],
        AppearanceConfigKey::CursorStyle => vec![
            match appearance.cursor_style {
                CursorStyle::Bar => "bar",
                CursorStyle::Block => "block",
                CursorStyle::Underline => "underline",
                CursorStyle::BlockHollow => "block_hollow",
            }
            .to_owned(),
        ],
        AppearanceConfigKey::CursorStyleBlink => vec![
            match appearance.cursor_blink_policy {
                CursorBlinkPolicy::Off => "false",
                CursorBlinkPolicy::On => "true",
                CursorBlinkPolicy::Terminal => "terminal",
            }
            .to_owned(),
        ],
        AppearanceConfigKey::ZzFontWeight => vec![appearance.font_weight.to_string()],
        AppearanceConfigKey::ZzCursorBlinkIntervalMs => {
            vec![appearance.cursor_blink_interval_ms.to_string()]
        }
        AppearanceConfigKey::ZzSearchMatchColor => {
            vec![serialize_rgba(appearance.search_match_color)]
        }
        AppearanceConfigKey::ZzSearchCurrentColor => {
            vec![serialize_rgba(appearance.search_current_color)]
        }
        AppearanceConfigKey::ZzLinkColor => vec![serialize_rgb(appearance.link_color)],
        AppearanceConfigKey::ZzCopyCursorColor => {
            vec![serialize_rgba(appearance.copy_cursor_color)]
        }
        AppearanceConfigKey::ZzRoundedSelection => {
            vec![appearance.rounded_selection.to_string()]
        }
    };
    Ok(values)
}

pub fn serialize_font_families(families: &[String]) -> Vec<String> {
    families
        .iter()
        .map(|family| quote_appearance_value(family))
        .collect()
}

pub fn serialize_font_synthetic_style(styles: zz_terminal::FontSyntheticStyle) -> String {
    if styles.bold && styles.italic && styles.bold_italic {
        return "true".to_owned();
    }
    if !styles.bold && !styles.italic && !styles.bold_italic {
        return "false".to_owned();
    }
    [
        ("bold", styles.bold),
        ("italic", styles.italic),
        ("bold-italic", styles.bold_italic),
    ]
    .into_iter()
    .map(|(style, enabled)| {
        if enabled {
            style.to_owned()
        } else {
            format!("no-{style}")
        }
    })
    .collect::<Vec<_>>()
    .join(",")
}

pub fn serialize_rgb(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

pub fn serialize_rgba(color: AppearanceColor) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color.r, color.g, color.b, color.a
    )
}

pub fn quote_appearance_value(value: &str) -> String {
    format!("\"{value}\"")
}

pub fn validate_ui_font_family(family: &str) -> Result<(), &'static str> {
    if family.trim().is_empty() || family.chars().any(char::is_control) {
        return Err("font family must be nonempty text without control characters");
    }
    Ok(())
}

pub fn set_ui_font_family(family: Option<&str>, system_font_family: &str) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_ui_font_family_at(&path, family, system_font_family).map(drop)
}

pub fn write_ui_font_family_at(
    path: &Path,
    family: Option<&str>,
    system_font_family: &str,
) -> io::Result<bool> {
    let family = family.unwrap_or(system_font_family);
    validate_ui_font_family(family)
        .map_err(|message| io::Error::new(ErrorKind::InvalidInput, message))?;
    let escaped = family.replace('\\', "\\\\").replace('"', "\\\"");
    write_config_edit_at(
        path,
        ConfigKey::UiFontFamily.as_str(),
        Some(&format!("\"{escaped}\"")),
    )
}

pub fn set_config_key(key: ConfigKey, value: &str) -> io::Result<()> {
    set_config_key_name(key.as_str(), value)
}

/// Select a paired chrome family and clear every explicit palette root in one
/// atomic edit, so a preset click cannot flash through partial states.
pub fn set_chrome_preset(preset: ChromePresetId) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_chrome_preset_at(&path, preset)
}

pub fn write_chrome_preset_at(path: &Path, preset: ChromePresetId) -> io::Result<()> {
    let source = match read_config_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut without_explicit_roots = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let is_chrome_root =
            config_key_for_line(line).is_some_and(|key| ChromeColor::parse(key).is_some());
        if !is_chrome_root {
            without_explicit_roots.push_str(line);
        }
    }
    let edited = edit_config_source(
        &without_explicit_roots,
        ConfigKey::ChromePreset.as_str(),
        Some(preset.as_str()),
    );
    if edited.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration edit exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    atomic_write(path, edited.as_bytes())
}

pub fn set_config_key_name(key: &str, value: &str) -> io::Result<()> {
    if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "configuration values must fit on one line",
        ));
    }
    let path = config_path_for_write()?;
    write_config_edit_at(&path, key, Some(value)).map(drop)
}

pub fn remove_config_key(key: ConfigKey) -> io::Result<()> {
    remove_config_key_name(key.as_str())
}

pub fn remove_config_key_name(key: &str) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_config_edit_at(&path, key, None).map(drop)
}

pub fn write_config_edit_at(path: &Path, key: &str, value: Option<&str>) -> io::Result<bool> {
    let source = match read_config_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let edited = edit_config_source(&source, key, value);
    if edited == source {
        return Ok(false);
    }
    if edited.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration edit exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    atomic_write(path, edited.as_bytes())?;
    Ok(true)
}

pub fn edit_config_source(source: &str, key: &str, value: Option<&str>) -> String {
    let last_line = last_key_line_range(source, key);
    match (last_line, value) {
        (Some(line), Some(value)) => {
            let replacement = replace_line_value(&source[line.clone()], value);
            let mut edited =
                String::with_capacity(source.len() + replacement.len().saturating_sub(line.len()));
            edited.push_str(&source[..line.start]);
            edited.push_str(&replacement);
            edited.push_str(&source[line.end..]);
            edited
        }
        (Some(line), None) => {
            let mut edited = String::with_capacity(source.len().saturating_sub(line.len()));
            edited.push_str(&source[..line.start]);
            edited.push_str(&source[line.end..]);
            edited
        }
        (None, Some(value)) => append_config_line(source, key, value),
        (None, None) => source.to_owned(),
    }
}

pub fn last_key_line_range(source: &str, key: &str) -> Option<std::ops::Range<usize>> {
    let mut offset = 0;
    let mut last = None;
    for line in source.split_inclusive('\n') {
        let end = offset + line.len();
        if config_key_for_line(line) == Some(key) {
            last = Some(offset..end);
        }
        offset = end;
    }
    last
}

pub fn config_key_for_line(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

pub fn replace_line_value(line: &str, value: &str) -> String {
    let without_lf = line.strip_suffix('\n').unwrap_or(line);
    let body = without_lf.strip_suffix('\r').unwrap_or(without_lf);
    let equals = body
        .find('=')
        .expect("a matched configuration line has an equals sign");
    let value_area_start = equals + 1;
    let comment_start = config_comment_start(&body[value_area_start..])
        .map_or(body.len(), |index| value_area_start + index);
    let value_area = &body[value_area_start..comment_start];
    let (value_start, value_end) = if value_area.trim().is_empty() {
        (comment_start, comment_start)
    } else {
        (
            value_area_start + value_area.len() - value_area.trim_start().len(),
            value_area_start + value_area.trim_end().len(),
        )
    };

    let mut replacement = String::with_capacity(line.len() + value.len());
    replacement.push_str(&line[..value_start]);
    replacement.push_str(value);
    replacement.push_str(&line[value_end..]);
    replacement
}

pub fn append_config_line(source: &str, key: &str, value: &str) -> String {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut edited = String::with_capacity(source.len() + key.len() + value.len() + 4);
    edited.push_str(source);
    if !source.is_empty() && !source.ends_with('\n') {
        edited.push_str(newline);
    }
    edited.push_str(key);
    edited.push_str(" = ");
    edited.push_str(value);
    edited.push_str(newline);
    edited
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "configuration path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let (temporary_path, mut temporary_file) = create_config_temp_file(path, parent)?;
    let write_result = (|| {
        if let Ok(metadata) = fs::metadata(path) {
            temporary_file.set_permissions(metadata.permissions())?;
        }
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()
    })();
    drop(temporary_file);

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

pub fn create_config_temp_file(path: &Path, parent: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(CONFIG_FILE_NAME))
        .to_string_lossy();
    for _ in 0..128 {
        let nonce = CONFIG_TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary_path =
            parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique temporary configuration file",
    ))
}

pub fn config_overrides_for_host(
    entries: Vec<ConfigOverrideEntry>,
    remote: bool,
) -> Vec<ConfigOverrideEntry> {
    if !remote {
        return entries;
    }
    entries
        .into_iter()
        .filter(|(key, _)| {
            !matches!(
                MuxOptionKey::from_config_key(key),
                Some(MuxOptionKey::ExperimentalAgentPane | MuxOptionKey::ExperimentalEditorPane)
            )
        })
        .collect()
}

pub mod settings;
