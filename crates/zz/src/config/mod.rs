use std::{
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use gpui::{
    App, Corners, Global, Hsla, Keystroke, Pixels, WindowBackgroundAppearance, WindowDecorations,
    px,
};
use zz_browser::SearchProvider;
use zz_daemon::{Endpoint, InteractiveClient};
pub(crate) use zz_daemon::{HostEntry, RejectedHost, configured_fleet_hosts, validate_fleet_host};
use zz_protocol::{
    CommandInvocation, ConfigOverrideEntry, MAX_GUI_TEXT_BYTES, MuxOptionKey, PROTOCOL_VERSION,
};
use zz_terminal::{
    AppearanceColor, AppearanceConfigKey, CellHeightAdjustment, Color, CursorBlinkPolicy,
    CursorStyle, TerminalAppearance,
};

use crate::{
    app_icon::AppIconSetting,
    keymap::ChromeOverride,
    mux::hosts::{HostId, HostRegistry},
    theme::{ChromeColor, ChromePresetId, ThemeModeSetting},
    window::corners::WindowCorners,
};

pub(crate) mod import;
#[cfg(not(target_os = "ios"))]
pub(crate) mod import_prompt;
pub(crate) mod settings;

const CONFIG_DIRECTORY_NAME: &str = "zz";
const CONFIG_FILE_NAME: &str = "config";
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(500);

const MAX_PANE_MARGIN: f32 = 32.0;
const MAX_PANE_CORNER_RADIUS: f32 = 32.0;
const MAX_PANE_BORDER_WIDTH: f32 = 8.0;
const MAX_WIDGET_CORNER_RADIUS: f32 = 24.0;
const MAX_WINDOW_CORNER_RADIUS: f32 = 32.0;

// Tangent-circle fit of the native macOS 27 window corner, measured off a screenshot.
const DEFAULT_WINDOW_CORNER_RADIUS: f32 = 13.5;
pub(crate) const WINDOW_FRAME_BORDER_SIZE: Pixels = px(1.0);

const DEFAULT_PANE_GAPS: bool = false;
const DEFAULT_PANE_CORNER_RADIUS: f32 = DEFAULT_WINDOW_CORNER_RADIUS;
const DEFAULT_PANE_MARGIN: f32 = 6.0;
const DEFAULT_PANE_BORDER_WIDTH: f32 = 1.0;
// The zz-ui theme's own default radius, restated here because the theme now reads it from here.
const DEFAULT_WIDGET_CORNER_RADIUS: f32 = 6.0;
const DEFAULT_USE_SYSTEM_TITLEBAR: bool = false;
const DEFAULT_WINDOW_BACKGROUND_BLUR: bool = false;
const DEFAULT_ANIMATIONS: bool = true;
const DEFAULT_TRAY: bool = true;
const DEFAULT_SHOW_FPS: bool = false;
const DEFAULT_QUIT_DAEMON_ON_EXIT: bool = false;
const DEFAULT_AUTO_RESTART_STALE_DAEMON: bool = false;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) const DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY: &str = "cmd-shift-c";
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(crate) const DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY: &str = "ctrl-shift-c";
/// Repeatable chrome binding overrides: `<table>:<key>=<action>` and
/// `<table>:<key>`.
const CHROME_KEYBIND_KEY: &str = "chrome-keybind";
const CHROME_UNBIND_KEY: &str = "chrome-unbind";
const DEFAULT_BROWSER_SEARCH_PROVIDER: SearchProvider = SearchProvider::Google;
const DEFAULT_BROWSER_EGRESS: bool = true;
const DEFAULT_EDITOR_FONT_SIZE: f32 = 13.0;
const MIN_EDITOR_FONT_SIZE: f32 = 8.0;
const MAX_EDITOR_FONT_SIZE: f32 = 32.0;
const DEFAULT_EDITOR_LINE_NUMBERS: bool = true;
const DEFAULT_EDITOR_RELATIVE_LINE_NUMBERS: bool = true;
const DEFAULT_EDITOR_SOFT_WRAP: bool = true;
const DEFAULT_EDITOR_VIM_MODE: bool = true;
const DEFAULT_EXPERIMENTAL_AGENT_PANE: bool = false;
const DEFAULT_EXPERIMENTAL_EDITOR_PANE: bool = false;
#[cfg(target_os = "linux")]
const UNBLURRED_WINDOW_BACKGROUND: WindowBackgroundAppearance =
    WindowBackgroundAppearance::Transparent;
#[cfg(not(target_os = "linux"))]
const UNBLURRED_WINDOW_BACKGROUND: WindowBackgroundAppearance = WindowBackgroundAppearance::Opaque;
static CONFIG_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The one agent key the client still owns: the adapter commands and the
/// auto-approve flag are mux options now, because the daemon spawns the
/// adapter. This one feeds pane creation, which is a client concern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentConfigKey {
    WorkingDirectory,
}

impl AgentConfigKey {
    fn from_str(key: &str) -> Option<Self> {
        match key {
            "agent-working-directory" => Some(Self::WorkingDirectory),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentConfig {
    pub(crate) working_directory: Option<PathBuf>,
}

impl Global for AgentConfig {}

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
    ExperimentalAgentPane,
    ExperimentalEditorPane,
    PaneGaps,
    PaneCornerRadius,
    PaneMargin,
    PaneBorderWidth,
    WidgetCornerRadius,
    EditorFontSize,
    EditorLineNumbers,
    EditorRelativeLineNumbers,
    EditorSoftWrap,
    EditorVimMode,
    BrowserElementSelectorHotkey,
    BrowserSearchProvider,
    BrowserEgress,
    ThemeMode,
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
            Self::ExperimentalAgentPane => "experimental-agent-pane",
            Self::ExperimentalEditorPane => "experimental-editor-pane",
            Self::PaneGaps => "pane-gaps",
            Self::PaneCornerRadius => "pane-corner-radius",
            Self::PaneMargin => "pane-margin",
            Self::PaneBorderWidth => "pane-border-width",
            Self::WidgetCornerRadius => "widget-corner-radius",
            Self::EditorFontSize => "editor-font-size",
            Self::EditorLineNumbers => "editor-line-numbers",
            Self::EditorRelativeLineNumbers => "editor-relative-line-numbers",
            Self::EditorSoftWrap => "editor-soft-wrap",
            Self::EditorVimMode => "editor-vim-mode",
            Self::BrowserElementSelectorHotkey => "browser-element-selector-hotkey",
            Self::BrowserSearchProvider => "browser-search-provider",
            Self::BrowserEgress => "browser-egress",
            Self::ThemeMode => "theme-mode",
            Self::AppIcon => "app-icon",
            Self::ChromePreset => "chrome-preset",
            Self::Chrome(color) => color.as_str(),
        }
    }

    fn from_str(key: &str) -> Option<Self> {
        match key {
            "use-system-titlebar" => Some(Self::UseSystemTitlebar),
            "window-corner-radius" => Some(Self::WindowCornerRadius),
            "window-background-blur" => Some(Self::WindowBackgroundBlur),
            "animations" => Some(Self::Animations),
            "tray" => Some(Self::Tray),
            "show-fps" => Some(Self::ShowFps),
            "quit-daemon-on-exit" => Some(Self::QuitDaemonOnExit),
            "auto-restart-stale-daemon" => Some(Self::AutoRestartStaleDaemon),
            "experimental-agent-pane" => Some(Self::ExperimentalAgentPane),
            "experimental-editor-pane" => Some(Self::ExperimentalEditorPane),
            "pane-gaps" => Some(Self::PaneGaps),
            "pane-corner-radius" => Some(Self::PaneCornerRadius),
            "pane-margin" => Some(Self::PaneMargin),
            "pane-border-width" => Some(Self::PaneBorderWidth),
            "widget-corner-radius" => Some(Self::WidgetCornerRadius),
            "editor-font-size" => Some(Self::EditorFontSize),
            "editor-line-numbers" => Some(Self::EditorLineNumbers),
            "editor-relative-line-numbers" => Some(Self::EditorRelativeLineNumbers),
            "editor-soft-wrap" => Some(Self::EditorSoftWrap),
            "editor-vim-mode" => Some(Self::EditorVimMode),
            "browser-element-selector-hotkey" => Some(Self::BrowserElementSelectorHotkey),
            "browser-search-provider" => Some(Self::BrowserSearchProvider),
            "browser-egress" => Some(Self::BrowserEgress),
            "theme-mode" => Some(Self::ThemeMode),
            "app-icon" => Some(Self::AppIcon),
            "chrome-preset" => Some(Self::ChromePreset),
            _ => ChromeColor::from_str(key).map(Self::Chrome),
        }
    }

    /// The inclusive range of logical pixels this key accepts, or `None` for a
    /// key that is not a geometry value.
    pub const fn geometry_range(self) -> Option<(f32, f32)> {
        match self {
            Self::PaneMargin => Some((0.0, MAX_PANE_MARGIN)),
            Self::PaneCornerRadius => Some((0.0, MAX_PANE_CORNER_RADIUS)),
            Self::PaneBorderWidth => Some((0.0, MAX_PANE_BORDER_WIDTH)),
            Self::WidgetCornerRadius => Some((0.0, MAX_WIDGET_CORNER_RADIUS)),
            Self::WindowCornerRadius => Some((0.0, MAX_WINDOW_CORNER_RADIUS)),
            Self::EditorFontSize => Some((MIN_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE)),
            Self::UseSystemTitlebar
            | Self::WindowBackgroundBlur
            | Self::Animations
            | Self::Tray
            | Self::ShowFps
            | Self::QuitDaemonOnExit
            | Self::AutoRestartStaleDaemon
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
    const fn from_default(value: T) -> Self {
        Self {
            value,
            provenance: ConfigProvenance::Default,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrowserConfig {
    pub(crate) element_selector_hotkey: ConfigValue<String>,
    pub(crate) search_provider: ConfigValue<SearchProvider>,
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

impl Global for BrowserConfig {}

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
    pub experimental_agent_pane: ConfigValue<bool>,
    pub experimental_editor_pane: ConfigValue<bool>,
    pub pane_gaps: ConfigValue<bool>,
    pub pane_corner_radius: ConfigValue<f32>,
    pub pane_margin: ConfigValue<f32>,
    pub pane_border_width: ConfigValue<f32>,
    pub widget_corner_radius: ConfigValue<f32>,
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
    pub chrome_colors: [ConfigValue<Option<Hsla>>; ChromeColor::ALL.len()],
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            use_system_titlebar: ConfigValue::from_default(DEFAULT_USE_SYSTEM_TITLEBAR),
            window_corner_radius: ConfigValue::from_default(DEFAULT_WINDOW_CORNER_RADIUS),
            window_background_blur: ConfigValue::from_default(DEFAULT_WINDOW_BACKGROUND_BLUR),
            animations: ConfigValue::from_default(DEFAULT_ANIMATIONS),
            tray: ConfigValue::from_default(DEFAULT_TRAY),
            show_fps: ConfigValue::from_default(DEFAULT_SHOW_FPS),
            quit_daemon_on_exit: ConfigValue::from_default(DEFAULT_QUIT_DAEMON_ON_EXIT),
            auto_restart_stale_daemon: ConfigValue::from_default(DEFAULT_AUTO_RESTART_STALE_DAEMON),
            experimental_agent_pane: ConfigValue::from_default(DEFAULT_EXPERIMENTAL_AGENT_PANE),
            experimental_editor_pane: ConfigValue::from_default(DEFAULT_EXPERIMENTAL_EDITOR_PANE),
            pane_gaps: ConfigValue::from_default(DEFAULT_PANE_GAPS),
            pane_corner_radius: ConfigValue::from_default(DEFAULT_PANE_CORNER_RADIUS),
            pane_margin: ConfigValue::from_default(DEFAULT_PANE_MARGIN),
            pane_border_width: ConfigValue::from_default(DEFAULT_PANE_BORDER_WIDTH),
            widget_corner_radius: ConfigValue::from_default(DEFAULT_WIDGET_CORNER_RADIUS),
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
            ConfigKey::ExperimentalAgentPane => Some(&mut self.experimental_agent_pane),
            ConfigKey::ExperimentalEditorPane => Some(&mut self.experimental_editor_pane),
            ConfigKey::PaneGaps => Some(&mut self.pane_gaps),
            ConfigKey::EditorLineNumbers => Some(&mut self.editor_line_numbers),
            ConfigKey::EditorRelativeLineNumbers => Some(&mut self.editor_relative_line_numbers),
            ConfigKey::EditorSoftWrap => Some(&mut self.editor_soft_wrap),
            ConfigKey::EditorVimMode => Some(&mut self.editor_vim_mode),
            ConfigKey::BrowserEgress => Some(&mut self.browser_egress),
            ConfigKey::WindowCornerRadius
            | ConfigKey::PaneCornerRadius
            | ConfigKey::PaneMargin
            | ConfigKey::PaneBorderWidth
            | ConfigKey::WidgetCornerRadius
            | ConfigKey::EditorFontSize
            | ConfigKey::BrowserElementSelectorHotkey
            | ConfigKey::BrowserSearchProvider
            | ConfigKey::ThemeMode
            | ConfigKey::AppIcon
            | ConfigKey::ChromePreset
            | ConfigKey::Chrome(_) => None,
        }
    }

    /// This root's override, if the user set one.
    pub fn chrome(&self, color: ChromeColor) -> ConfigValue<Option<Hsla>> {
        self.chrome_colors[chrome_index(color)]
    }
}

fn chrome_index(color: ChromeColor) -> usize {
    ChromeColor::ALL
        .iter()
        .position(|candidate| *candidate == color)
        .expect("ChromeColor::ALL contains every variant")
}

impl Global for AppConfig {}

#[derive(Clone, Copy)]
struct PlatformReduceMotion(bool);

impl Global for PlatformReduceMotion {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DaemonConfigOverrides {
    entries: Vec<ConfigOverrideEntry>,
}

impl Global for DaemonConfigOverrides {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FleetHosts {
    entries: Vec<HostEntry>,
}

impl Global for FleetHosts {}

#[derive(Default)]
struct ConfigOverrideTransport {
    client: Option<Weak<InteractiveClient>>,
    remote: bool,
}

impl Global for ConfigOverrideTransport {}

#[derive(Debug, Eq, PartialEq)]
struct ConfigDiagnostic {
    line: usize,
    message: String,
}

#[derive(Debug, Default, PartialEq)]
struct ParsedConfig {
    config: AppConfig,
    browser: BrowserConfig,
    agent: AgentConfig,
    hosts: Vec<HostEntry>,
    rejected_hosts: Vec<RejectedHost>,
    daemon_entries: Vec<ConfigOverrideEntry>,
    chrome_overrides: Vec<ChromeOverride>,
    diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ImportError {
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

pub fn init(cx: &mut App) {
    cx.set_global(PlatformReduceMotion(cx.reduce_motion()));
    cx.set_global(ConfigOverrideTransport::default());
    cx.set_global(AgentConfig::default());
    let candidates = config_candidates();
    let initial = ConfigFileStamp::detect(&candidates);
    let parsed = initial.path.as_deref().map(load_config);
    install_config(initial.path.as_deref(), parsed, cx);

    let background = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let mut observed = initial;
        loop {
            background.timer(CONFIG_POLL_INTERVAL).await;
            let previous = observed.clone();
            let candidates = candidates.clone();
            let changed = background
                .spawn(async move {
                    let next = ConfigFileStamp::detect(&candidates);
                    if next == previous {
                        return None;
                    }
                    let parsed = next.path.as_deref().map(load_config);
                    Some((next, parsed))
                })
                .await;
            let Some((next, parsed)) = changed else {
                continue;
            };
            observed = next;
            cx.update(|cx| {
                install_config(observed.path.as_deref(), parsed, cx);
                crate::theme::refresh_current_theme(cx);
            });
            cx.refresh();
        }
    })
    .detach();
}

fn install_config(path: Option<&Path>, parsed: Option<io::Result<ParsedConfig>>, cx: &mut App) {
    let Some(path) = path else {
        log::debug!(target: "zz::config", "configuration not found; using built-in defaults");
        cx.set_global(AppConfig::default());
        cx.set_global(BrowserConfig::default());
        apply_animations(cx);
        apply_window_background_appearance(cx);
        apply_window_decorations(cx);
        crate::app_icon::apply(cx);
        cx.set_global(AgentConfig::default());
        cx.set_global(FleetHosts::default());
        cx.set_global(DaemonConfigOverrides::default());
        crate::keymap::install(&[], DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY, cx);
        send_current_config_overrides(cx);
        return;
    };
    let parsed = match parsed.expect("a discovered configuration has a load result") {
        Ok(parsed) => parsed,
        Err(error) => {
            log::warn!(
                target: "zz::config",
                "could not load configuration path={} error={error}; using built-in defaults",
                path.display(),
            );
            ParsedConfig::default()
        }
    };

    for diagnostic in &parsed.diagnostics {
        log::warn!(
            target: "zz::config",
            "{}:{}: {}",
            path.display(),
            diagnostic.line,
            diagnostic.message,
        );
    }

    log::info!(
        target: "zz::config",
        "application configuration path={} pane_gaps={} pane_corner_radius={} pane_margin={} pane_border_width={} widget_corner_radius={} window_corner_radius={} editor_font_size={} editor_line_numbers={} editor_relative_line_numbers={} editor_soft_wrap={} editor_vim_mode={} browser_element_selector_hotkey={} browser_search_provider={} browser_egress={} use_system_titlebar={} window_background_blur={} animations={} tray={} show_fps={} quit_daemon_on_exit={} auto_restart_stale_daemon={} agent_working_directory={:?} daemon_override_entries={}",
        path.display(),
        parsed.config.pane_gaps.value,
        parsed.config.pane_corner_radius.value,
        parsed.config.pane_margin.value,
        parsed.config.pane_border_width.value,
        parsed.config.widget_corner_radius.value,
        parsed.config.window_corner_radius.value,
        parsed.config.editor_font_size.value,
        parsed.config.editor_line_numbers.value,
        parsed.config.editor_relative_line_numbers.value,
        parsed.config.editor_soft_wrap.value,
        parsed.config.editor_vim_mode.value,
        parsed.browser.element_selector_hotkey.value,
        parsed.browser.search_provider.value.as_str(),
        parsed.config.browser_egress.value,
        parsed.config.use_system_titlebar.value,
        parsed.config.window_background_blur.value,
        parsed.config.animations.value,
        parsed.config.tray.value,
        parsed.config.show_fps.value,
        parsed.config.quit_daemon_on_exit.value,
        parsed.config.auto_restart_stale_daemon.value,
        parsed.agent.working_directory,
        parsed.daemon_entries.len(),
    );
    cx.set_global(FleetHosts {
        entries: parsed.hosts,
    });
    log_fleet_hosts(cx);
    crate::keymap::install(
        &parsed.chrome_overrides,
        &parsed.browser.element_selector_hotkey.value,
        cx,
    );
    cx.set_global(parsed.config);
    cx.set_global(parsed.browser);
    apply_animations(cx);
    apply_window_background_appearance(cx);
    apply_window_decorations(cx);
    crate::app_icon::apply(cx);
    cx.set_global(parsed.agent);
    cx.set_global(DaemonConfigOverrides {
        entries: parsed.daemon_entries,
    });
    send_current_config_overrides(cx);
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ConfigFileStamp {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: Option<u64>,
}

impl ConfigFileStamp {
    fn detect(candidates: &[PathBuf]) -> Self {
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

/// Radius of the app-drawn window frame, visible only under Linux client-side
/// decorations. Every other platform shapes the window natively.
pub(crate) fn window_corner_radius(cx: &App) -> Pixels {
    px(resolved_config(cx).window_corner_radius.value)
}

/// Whether quitting the app stops the daemon even when live sessions remain.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn quit_daemon_on_exit(cx: &App) -> bool {
    resolved_config(cx).quit_daemon_on_exit.value
}

/// Whether the GUI should replace a stale local daemon without asking first.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn auto_restart_stale_daemon(cx: &App) -> bool {
    resolved_config(cx).auto_restart_stale_daemon.value
}

/// Whether to put zz in the system tray. Read once at startup.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn tray_enabled(cx: &App) -> bool {
    resolved_config(cx).tray.value
}

/// Whether a browser pane attached to a remote ssh host routes its traffic
/// through that host. Client-local: it never crosses the wire.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn browser_egress_enabled(cx: &App) -> bool {
    resolved_config(cx).browser_egress.value
}

/// Whether the experimental agent pane is enabled. Blocks creating new agent
/// panes while off; existing ones keep rendering. Always false in a build
/// without the `agent-pane` cargo feature.
pub fn agent_pane_enabled(cx: &App) -> bool {
    cfg!(feature = "agent-pane") && resolved_config(cx).experimental_agent_pane.value
}

/// Whether the experimental editor pane is enabled. Same contract as the agent
/// gate, and always false without the `editor-pane` cargo feature.
pub(crate) fn editor_pane_enabled(cx: &App) -> bool {
    cfg!(feature = "editor-pane") && resolved_config(cx).experimental_editor_pane.value
}

pub(crate) fn frame_content_corner_radius(cx: &App) -> Pixels {
    content_corner_radius(window_corner_radius(cx))
}

fn content_corner_radius(window_radius: Pixels) -> Pixels {
    (window_radius - WINDOW_FRAME_BORDER_SIZE).max(px(0.0))
}

pub(crate) fn pane_gaps(cx: &App) -> bool {
    resolved_config(cx).pane_gaps.value
}

pub(crate) fn pane_margin(cx: &App) -> Pixels {
    let config = resolved_config(cx);
    px(effective_pane_geometry(
        config.pane_gaps.value,
        config.pane_margin.value,
    ))
}

pub(crate) fn pane_border_width(cx: &App) -> Pixels {
    let config = resolved_config(cx);
    px(effective_pane_geometry(
        config.pane_gaps.value,
        config.pane_border_width.value,
    ))
}

/// The corner every widget turns. `zz::theme` pushes it onto the zz-ui theme.
pub(crate) fn widget_corner_radius(cx: &App) -> Pixels {
    px(resolved_config(cx).widget_corner_radius.value)
}

/// Editor pane type size. The family is not a knob: the editor inherits the
/// terminal's mono family through the zz-ui theme.
#[cfg(feature = "editor-pane")]
pub(crate) fn editor_font_size(cx: &App) -> Pixels {
    px(resolved_config(cx).editor_font_size.value)
}

#[cfg(feature = "editor-pane")]
pub(crate) fn editor_line_numbers(cx: &App) -> bool {
    resolved_config(cx).editor_line_numbers.value
}

/// Number the gutter by distance from the cursor line. Only visible while
/// `editor-line-numbers` is on.
#[cfg(feature = "editor-pane")]
pub(crate) fn editor_relative_line_numbers(cx: &App) -> bool {
    resolved_config(cx).editor_relative_line_numbers.value
}

#[cfg(feature = "editor-pane")]
pub(crate) fn editor_soft_wrap(cx: &App) -> bool {
    resolved_config(cx).editor_soft_wrap.value
}

/// The editor's vim layer. Off restores plain editing byte-for-byte.
#[cfg(feature = "editor-pane")]
pub(crate) fn editor_vim_mode(cx: &App) -> bool {
    resolved_config(cx).editor_vim_mode.value
}

pub(crate) fn theme_mode(cx: &App) -> ThemeModeSetting {
    resolved_config(cx).theme_mode.value
}

/// Which dock icon the app wears. Follows the OS appearance by default, even
/// when `theme-mode` pins the chrome to one palette.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn app_icon_setting(cx: &App) -> AppIconSetting {
    resolved_config(cx).app_icon.value
}

/// The selected paired chrome family, if any.
pub(crate) fn chrome_preset(cx: &App) -> Option<ChromePresetId> {
    resolved_config(cx).chrome_preset.value
}

/// Every chrome override, in [`ChromeColor::ALL`] order.
pub(crate) fn chrome_colors(cx: &App) -> [Option<Hsla>; ChromeColor::ALL.len()] {
    resolved_config(cx).chrome_colors.map(|value| value.value)
}

const fn effective_pane_geometry(gaps_enabled: bool, value: f32) -> f32 {
    if gaps_enabled { value } else { 0.0 }
}

/// Per-corner radii for the pane surface.
pub(crate) fn pane_content_radii(cx: &App, corners: WindowCorners) -> Corners<Pixels> {
    let config = resolved_config(cx);
    let base = px(effective_pane_geometry(
        config.pane_gaps.value,
        config.pane_corner_radius.value,
    ));
    let margin = effective_pane_geometry(config.pane_gaps.value, config.pane_margin.value);
    let exposed = if margin > 0.0 {
        base
    } else {
        base.max(frame_content_corner_radius(cx))
    };
    corners.surface_radii(exposed, base)
}

#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn window_background_appearance(cx: &App) -> WindowBackgroundAppearance {
    crate::window::background::native_appearance(requested_window_background_appearance(cx))
}

pub(crate) fn window_decorations(cx: &App) -> WindowDecorations {
    if cfg!(target_os = "linux") && resolved_config(cx).use_system_titlebar.value {
        WindowDecorations::Server
    } else {
        WindowDecorations::Client
    }
}

fn apply_animations(cx: &mut App) {
    let platform_reduce_motion = cx
        .try_global::<PlatformReduceMotion>()
        .is_some_and(|preference| preference.0);
    cx.set_reduce_motion(platform_reduce_motion || !resolved_config(cx).animations.value);
}

/// The titlebar every zz window opens with: a transparent strip, nothing else.
/// The macOS traffic lights keep their native size and placement.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn titlebar_options() -> gpui::TitlebarOptions {
    zz_ui::TitleBar::title_bar_options()
}

fn apply_window_decorations(cx: &mut App) {
    let decorations = window_decorations(cx);
    for window in cx.windows() {
        window
            .update(cx, |_, window, _| {
                if decorations == WindowDecorations::Server {
                    window.set_client_inset(px(0.0));
                }
                window.request_decorations(decorations);
            })
            .ok();
    }
}

fn requested_window_background_appearance(cx: &App) -> WindowBackgroundAppearance {
    if crate::theme::chrome_blur(cx) {
        WindowBackgroundAppearance::Blurred
    } else {
        UNBLURRED_WINDOW_BACKGROUND
    }
}

pub(crate) fn apply_window_background_appearance(cx: &mut App) {
    let requested_appearance = requested_window_background_appearance(cx);
    let native_appearance = crate::window::background::native_appearance(requested_appearance);
    let corner_radius = window_corner_radius(cx);
    for window in cx.windows() {
        window
            .update(cx, |_, window, _| {
                window.set_background_appearance(native_appearance);
                crate::window::background::apply(window, requested_appearance, corner_radius);
            })
            .ok();
    }
}

pub(crate) fn observe_window_background<T: 'static>(
    window: &mut gpui::Window,
    cx: &mut gpui::Context<T>,
) {
    let requested_appearance = requested_window_background_appearance(cx);
    window.set_background_appearance(crate::window::background::native_appearance(
        requested_appearance,
    ));
    crate::window::background::apply(window, requested_appearance, window_corner_radius(cx));

    #[cfg(target_os = "linux")]
    cx.observe_window_bounds(window, |_, window, cx| {
        crate::window::background::apply(
            window,
            requested_window_background_appearance(cx),
            window_corner_radius(cx),
        );
    })
    .detach();
}

pub fn resolved_config(cx: &App) -> AppConfig {
    cx.try_global::<AppConfig>().copied().unwrap_or_default()
}

pub(crate) fn browser_config(cx: &App) -> BrowserConfig {
    cx.try_global::<BrowserConfig>()
        .cloned()
        .unwrap_or_default()
}

#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn browser_search_provider(cx: &App) -> SearchProvider {
    browser_config(cx).search_provider.value
}

pub fn agent_config(cx: &App) -> AgentConfig {
    cx.try_global::<AgentConfig>().cloned().unwrap_or_default()
}

pub(crate) fn fleet_hosts(cx: &App) -> Vec<HostEntry> {
    cx.try_global::<FleetHosts>()
        .map(|hosts| hosts.entries.clone())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn set_fleet_hosts_for_test(entries: Vec<HostEntry>, cx: &mut App) {
    cx.set_global(FleetHosts { entries });
}

fn log_fleet_hosts(cx: &App) {
    let configured = fleet_hosts(cx);
    if configured.is_empty() {
        return;
    }
    let registry = HostRegistry::new(
        zz_daemon::default_socket_path(),
        &configured,
        crate::profile::LocalHostPolicy::Always,
    );
    let local = registry
        .get(HostId::LOCAL)
        .expect("host registry always contains local");
    debug_assert_eq!(local.name, "local");
    for (id, host) in registry.iter().filter(|(id, _)| *id != HostId::LOCAL) {
        let (lookup_id, lookup_host) = registry
            .get_by_name(&host.name)
            .expect("configured host is indexed by name");
        debug_assert_eq!(lookup_id, id);
        log::info!(
            target: "zz::config",
            "fleet host name={} endpoint={}",
            lookup_host.name,
            lookup_host.endpoint,
        );
    }
}

pub fn daemon_config_overrides(cx: &App) -> Vec<ConfigOverrideEntry> {
    cx.try_global::<DaemonConfigOverrides>()
        .map(|overrides| overrides.entries.clone())
        .unwrap_or_default()
}

pub(crate) fn register_config_override_client(
    client: &Arc<InteractiveClient>,
    remote: bool,
    cx: &mut App,
) {
    cx.set_global(ConfigOverrideTransport::default());
    let hello = client.server_hello();
    if hello.protocol_version != PROTOCOL_VERSION {
        log::warn!(
            target: "zz::config",
            "not sending configuration overrides across protocol skew client={} server={}",
            PROTOCOL_VERSION,
            hello.protocol_version,
        );
        return;
    }
    if !hello
        .capabilities
        .iter()
        .any(|capability| capability == "config-overrides-v1")
    {
        log::warn!(
            target: "zz::config",
            "daemon does not advertise config-overrides-v1; keeping daemon-owned zz/config entries local",
        );
        return;
    }
    cx.set_global(ConfigOverrideTransport {
        client: Some(Arc::downgrade(client)),
        remote,
    });
    send_current_config_overrides(cx);
}

pub(crate) fn config_overrides_for_host(
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

fn send_current_config_overrides(cx: &App) {
    let Some((client, remote)) = cx
        .try_global::<ConfigOverrideTransport>()
        .and_then(|transport| {
            transport
                .client
                .as_ref()
                .and_then(Weak::upgrade)
                .map(|client| (client, transport.remote))
        })
    else {
        return;
    };
    let entries = config_overrides_for_host(daemon_config_overrides(cx), remote);
    if let Err(error) = client.set_config_overrides(entries) {
        log::warn!(target: "zz::config", "failed to send configuration overrides: {error}");
    }
}

/// Ask the daemon to re-source `zz/mux.conf` after an import. A no-op with no
/// armed interactive client: the daemon reads the file at its next startup.
pub fn request_daemon_reload(cx: &App) {
    let Some(client) = cx
        .try_global::<ConfigOverrideTransport>()
        .and_then(|transport| transport.client.as_ref())
        .and_then(Weak::upgrade)
    else {
        log::info!(
            target: "zz::config",
            "skipping daemon configuration reload: no armed interactive client",
        );
        return;
    };
    if let Err(error) = client.execute(CommandInvocation::new("reload-config", [] as [&str; 0])) {
        log::warn!(
            target: "zz::config",
            "failed to request daemon configuration reload: {error}",
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigPlatform {
    #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
    Unix,
    #[cfg(any(test, target_os = "macos"))]
    Macos,
    #[cfg(any(test, target_os = "windows"))]
    Windows,
}

#[derive(Clone, Copy, Debug, Default)]
struct ConfigEnvironment<'a> {
    xdg_config_home: Option<&'a Path>,
    home: Option<&'a Path>,
    #[cfg(any(test, target_os = "windows"))]
    appdata: Option<&'a Path>,
    #[cfg(any(test, target_os = "windows"))]
    local_appdata: Option<&'a Path>,
    #[cfg(any(test, target_os = "windows"))]
    user_profile: Option<&'a Path>,
}

fn current_config_platform() -> ConfigPlatform {
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

fn config_candidates() -> Vec<PathBuf> {
    let xdg_config_home = nonempty_env("XDG_CONFIG_HOME");
    let home = nonempty_env("HOME");
    #[cfg(any(test, target_os = "windows"))]
    let appdata = nonempty_env("APPDATA");
    #[cfg(any(test, target_os = "windows"))]
    let local_appdata = nonempty_env("LOCALAPPDATA");
    #[cfg(any(test, target_os = "windows"))]
    let user_profile = nonempty_env("USERPROFILE");

    config_candidates_for(
        current_config_platform(),
        ConfigEnvironment {
            xdg_config_home: xdg_config_home.as_deref(),
            home: home.as_deref(),
            #[cfg(any(test, target_os = "windows"))]
            appdata: appdata.as_deref(),
            #[cfg(any(test, target_os = "windows"))]
            local_appdata: local_appdata.as_deref(),
            #[cfg(any(test, target_os = "windows"))]
            user_profile: user_profile.as_deref(),
        },
    )
}

fn config_candidates_for(
    platform: ConfigPlatform,
    environment: ConfigEnvironment<'_>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_config_candidate(&mut candidates, environment.xdg_config_home);

    match platform {
        #[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
        ConfigPlatform::Unix => {
            push_home_config_candidate(&mut candidates, environment.home);
        }
        #[cfg(any(test, target_os = "macos"))]
        ConfigPlatform::Macos => {
            push_home_config_candidate(&mut candidates, environment.home);
            if let Some(home) = environment.home {
                push_config_candidate_path(
                    &mut candidates,
                    &home.join("Library/Application Support"),
                );
            }
        }
        #[cfg(any(test, target_os = "windows"))]
        ConfigPlatform::Windows => {
            push_config_candidate(&mut candidates, environment.appdata);
            push_config_candidate(&mut candidates, environment.local_appdata);
            push_home_config_candidate(&mut candidates, environment.user_profile);
            push_home_config_candidate(&mut candidates, environment.home);
        }
    }

    candidates
}

fn push_home_config_candidate(candidates: &mut Vec<PathBuf>, home: Option<&Path>) {
    if let Some(home) = home {
        push_config_candidate_path(candidates, &home.join(".config"));
    }
}

fn push_config_candidate(candidates: &mut Vec<PathBuf>, base: Option<&Path>) {
    let Some(base) = base else {
        return;
    };
    push_config_candidate_path(candidates, base);
}

fn push_config_candidate_path(candidates: &mut Vec<PathBuf>, base: &Path) {
    if !base.is_absolute() {
        return;
    }
    let candidate = base.join(CONFIG_DIRECTORY_NAME).join(CONFIG_FILE_NAME);
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn discover_config_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
}

fn preferred_config_creation_path(
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

fn config_path_for_write() -> io::Result<PathBuf> {
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

fn nonempty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn load_config(path: &Path) -> io::Result<ParsedConfig> {
    read_config_source(path).map(|source| parse_config(&source))
}

fn read_config_source(path: &Path) -> io::Result<String> {
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

fn parse_config(source: &str) -> ParsedConfig {
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

        if let Some(agent_key) = AgentConfigKey::from_str(key) {
            if let Some(diagnostic) =
                apply_agent_key(&mut parsed.agent, agent_key, key, value, line_number)
            {
                parsed.diagnostics.push(diagnostic);
            }
            continue;
        }

        if matches!(key, CHROME_KEYBIND_KEY | CHROME_UNBIND_KEY) {
            let entry = if key == CHROME_KEYBIND_KEY {
                crate::keymap::parse_bind(value)
            } else {
                crate::keymap::parse_unbind(value)
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

        let Some(key) = ConfigKey::from_str(key) else {
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
            ConfigKey::PaneCornerRadius => &mut parsed.config.pane_corner_radius,
            ConfigKey::PaneMargin => &mut parsed.config.pane_margin,
            ConfigKey::PaneBorderWidth => &mut parsed.config.pane_border_width,
            ConfigKey::WidgetCornerRadius => &mut parsed.config.widget_corner_radius,
            ConfigKey::EditorFontSize => &mut parsed.config.editor_font_size,
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
                unreachable!("handled above")
            }
        };
        target.provenance = ConfigProvenance::Override;

        let range = key
            .geometry_range()
            .expect("every key reaching here is a geometry value");
        match parse_geometry_value(value, range) {
            Ok(value) => target.value = value,
            Err(message) => parsed.diagnostics.push(ConfigDiagnostic {
                line: line_number,
                message: format!("invalid `{}`: {message}", key.as_str()),
            }),
        }
    }

    parsed
}

pub(crate) fn write_fleet_host(name: &str, endpoint: &str) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_fleet_host_at(&path, name, endpoint)
}

/// Write `host-<name>` and publish it to the live fleet in one step, so the mux
/// dials the new host now instead of waiting for the config poller.
pub fn add_fleet_host(name: &str, endpoint: &str, cx: &mut App) -> io::Result<()> {
    write_fleet_host(name, endpoint)?;
    let entry = HostEntry {
        name: name.to_owned(),
        endpoint: Endpoint::parse(endpoint)
            .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error.to_string()))?,
    };
    let mut entries = cx
        .try_global::<FleetHosts>()
        .map(|hosts| hosts.entries.clone())
        .unwrap_or_default();
    entries.retain(|host| host.name != name);
    entries.push(entry);
    cx.set_global(FleetHosts { entries });
    Ok(())
}

fn write_fleet_host_at(path: &Path, name: &str, endpoint: &str) -> io::Result<()> {
    validate_fleet_host(name, endpoint)
        .map_err(|message| io::Error::new(ErrorKind::InvalidInput, message))?;
    write_config_edit_at(path, &format!("host-{name}"), Some(endpoint)).map(drop)
}

pub(crate) fn remove_fleet_host(name: &str) -> io::Result<bool> {
    let path = config_path_for_write()?;
    remove_fleet_host_at(&path, name)
}

/// Delete `host-<name>` and drop it from the live fleet in one step, the
/// counterpart to [`add_fleet_host`].
pub(crate) fn remove_fleet_host_live(name: &str, cx: &mut App) -> io::Result<bool> {
    let removed = remove_fleet_host(name)?;
    let mut entries = cx
        .try_global::<FleetHosts>()
        .map(|hosts| hosts.entries.clone())
        .unwrap_or_default();
    entries.retain(|host| host.name != name);
    cx.set_global(FleetHosts { entries });
    Ok(removed)
}

fn remove_fleet_host_at(path: &Path, name: &str) -> io::Result<bool> {
    let key = format!("host-{name}");
    let mut removed = false;
    while write_config_edit_at(path, &key, None)? {
        removed = true;
    }
    Ok(removed)
}

fn apply_agent_key(
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

fn apply_theme_key(
    config: &mut AppConfig,
    key: ConfigKey,
    value: &str,
    line_number: usize,
) -> Option<ConfigDiagnostic> {
    let message = match key {
        ConfigKey::Chrome(color) => {
            let target = &mut config.chrome_colors[chrome_index(color)];
            target.provenance = ConfigProvenance::Override;
            match zz_ui::parse_hex(value) {
                Ok(parsed) => {
                    target.value = Some(parsed);
                    return None;
                }
                Err(message) => message,
            }
        }
        ConfigKey::ThemeMode => {
            config.theme_mode.provenance = ConfigProvenance::Override;
            match ThemeModeSetting::from_str(value) {
                Some(mode) => {
                    config.theme_mode.value = mode;
                    return None;
                }
                None => "expected system, light or dark".to_owned(),
            }
        }
        ConfigKey::AppIcon => {
            config.app_icon.provenance = ConfigProvenance::Override;
            match AppIconSetting::from_str(value) {
                Some(setting) => {
                    config.app_icon.value = setting;
                    return None;
                }
                None => "expected automatic, light or dark".to_owned(),
            }
        }
        ConfigKey::ChromePreset => {
            config.chrome_preset.provenance = ConfigProvenance::Override;
            match ChromePresetId::from_str(value) {
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

fn config_value_without_comment(value: &str) -> &str {
    config_comment_start(value).map_or(value, |index| &value[..index])
}

fn config_comment_start(value: &str) -> Option<usize> {
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

pub(crate) fn normalize_browser_hotkey(value: &str) -> Result<String, String> {
    let hotkey = Keystroke::parse(value.trim()).map_err(|_| {
        "expected modifiers and a key, for example `cmd-shift-c` or `ctrl-shift-c`".to_owned()
    })?;
    if !(hotkey.modifiers.control
        || hotkey.modifiers.alt
        || hotkey.modifiers.platform
        || hotkey.modifiers.function)
    {
        return Err("expected Control, Alt, Command/Super, or Function as a modifier".into());
    }
    Ok(hotkey.to_string())
}

fn parse_geometry_value(value: &str, (min, max): (f32, f32)) -> Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|_| "expected a number of logical pixels".to_owned())?;
    if !value.is_finite() {
        return Err("value must be finite".to_owned());
    }
    if !(min..=max).contains(&value) {
        return Err(format!(
            "value must be between {min} and {max} logical pixels"
        ));
    }
    Ok(value)
}

fn parse_boolean(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("expected `true` or `false`".to_owned()),
    }
}

fn parse_config_string(value: &str) -> Result<String, String> {
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
pub(crate) fn import_appearance_values_at(
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

fn apply_import_edits(
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

fn is_cumulative_appearance_key(key: AppearanceConfigKey) -> bool {
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

fn is_app_config_key(key: &str) -> bool {
    ConfigKey::from_str(key).is_some()
        || AgentConfigKey::from_str(key).is_some()
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

fn replace_config_key_group(source: &str, key: &str, values: &[String]) -> String {
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

pub(crate) fn appearance_config_values(
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

fn serialize_font_families(families: &[String]) -> Vec<String> {
    families
        .iter()
        .map(|family| quote_appearance_value(family))
        .collect()
}

fn serialize_font_synthetic_style(styles: zz_terminal::FontSyntheticStyle) -> String {
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

fn serialize_rgb(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn serialize_rgba(color: AppearanceColor) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color.r, color.g, color.b, color.a
    )
}

fn quote_appearance_value(value: &str) -> String {
    format!("\"{value}\"")
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

fn write_chrome_preset_at(path: &Path, preset: ChromePresetId) -> io::Result<()> {
    let source = match read_config_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut without_explicit_roots = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let is_chrome_root =
            config_key_for_line(line).is_some_and(|key| ChromeColor::from_str(key).is_some());
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

fn set_config_key_name(key: &str, value: &str) -> io::Result<()> {
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

fn remove_config_key_name(key: &str) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_config_edit_at(&path, key, None).map(drop)
}

fn write_config_edit_at(path: &Path, key: &str, value: Option<&str>) -> io::Result<bool> {
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

fn edit_config_source(source: &str, key: &str, value: Option<&str>) -> String {
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

fn last_key_line_range(source: &str, key: &str) -> Option<std::ops::Range<usize>> {
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

fn config_key_for_line(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

fn replace_line_value(line: &str, value: &str) -> String {
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

fn append_config_line(source: &str, key: &str, value: &str) -> String {
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

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
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

fn create_config_temp_file(path: &Path, parent: &Path) -> io::Result<(PathBuf, File)> {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use zz_terminal::{
        AppearanceLoad, AppearanceSource, FontFeature, FontSyntheticStyle, TerminalColorScheme,
        apply_appearance_overrides, load_ghostty_appearance_from_for,
    };

    fn absolute_test_root(name: &str) -> PathBuf {
        env::current_dir()
            .expect("current directory")
            .join("target/config-path-tests")
            .join(name)
    }

    fn expected_config_path(base: &Path) -> PathBuf {
        base.join(CONFIG_DIRECTORY_NAME).join(CONFIG_FILE_NAME)
    }

    fn distinctive_appearance() -> TerminalAppearance {
        let mut appearance = TerminalAppearance {
            color_scheme: TerminalColorScheme::Light,
            font_families: vec!["Berkeley Mono".to_owned(), "Symbols # Fallback".to_owned()],
            font_families_bold: vec!["Berkeley Mono Bold".to_owned()],
            font_families_italic: vec!["Berkeley Mono Italic".to_owned()],
            font_families_bold_italic: vec!["Berkeley Mono Bold Italic".to_owned()],
            font_size_points: 15.25,
            font_weight: 575,
            font_features: vec![FontFeature::new(*b"liga", 0), FontFeature::new(*b"ss03", 1)],
            font_synthetic_style: FontSyntheticStyle {
                bold: false,
                italic: true,
                bold_italic: false,
            },
            font_thicken: true,
            font_thicken_strength: 173,
            cell_height_adjustment: CellHeightAdjustment::Percent(12.5),
            padding_left: 3.0,
            padding_right: 7.5,
            padding_top: 11.0,
            padding_bottom: 13.25,
            foreground: Color::rgb(0x12, 0x34, 0x56),
            background: Color::rgb(0x65, 0x43, 0x21),
            cursor_color: Color::rgb(0xab, 0xcd, 0xef),
            cursor_style: CursorStyle::Underline,
            selection_foreground: Color::rgb(0x0a, 0x0b, 0x0c),
            selection_background: AppearanceColor::rgba(0x10, 0x20, 0x30, 0x40),
            search_match_color: AppearanceColor::rgba(0x50, 0x60, 0x70, 0x80),
            search_current_color: AppearanceColor::rgba(0x90, 0xa0, 0xb0, 0xc0),
            link_color: Color::rgb(0xde, 0xad, 0x01),
            copy_cursor_color: AppearanceColor::rgba(0xca, 0xfe, 0xba, 0xbe),
            minimum_contrast: 4.5,
            cursor_blink_policy: CursorBlinkPolicy::On,
            cursor_blink_interval_ms: 725,
            rounded_selection: false,
            background_opacity: 0.875,
            ..TerminalAppearance::default()
        };
        appearance.palette[1] = Color::rgb(0x21, 0x43, 0x65);
        appearance.palette[42] = Color::rgb(0xfe, 0xdc, 0xba);
        appearance
    }

    fn parsed_appearance_entries(parsed: &ParsedConfig) -> Vec<ConfigOverrideEntry> {
        parsed
            .daemon_entries
            .iter()
            .filter(|(key, _)| AppearanceConfigKey::from_config_key(key).is_some())
            .cloned()
            .collect()
    }

    fn assert_f32_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn parser_applies_the_browser_search_provider() {
        let parsed = parse_config("browser-search-provider = duckduckgo\n");

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.browser.search_provider,
            ConfigValue {
                value: SearchProvider::DuckDuckGo,
                provenance: ConfigProvenance::Override,
            }
        );
    }

    #[test]
    fn browser_search_provider_rejects_unknown_engines() {
        let parsed = parse_config("browser-search-provider = bing\n");

        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(
            parsed.browser.search_provider.value,
            DEFAULT_BROWSER_SEARCH_PROVIDER
        );
        assert_eq!(
            parsed.browser.search_provider.provenance,
            ConfigProvenance::Override
        );
    }

    #[test]
    fn parser_collects_chrome_binding_overrides() {
        let parsed = parse_config(
            "chrome-keybind = browser:Cmd-Shift-p=browser-new-tab\n\
             chrome-keybind = terminal:C-S-y=terminal-copy  # a note\n\
             chrome-unbind = sidebar:q\n",
        );

        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(
            parsed.chrome_overrides,
            vec![
                ChromeOverride::Bind {
                    table: "browser",
                    key: "D-S-p".to_owned(),
                    action: "browser-new-tab".to_owned(),
                },
                ChromeOverride::Bind {
                    table: "terminal",
                    key: "C-S-y".to_owned(),
                    action: "terminal-copy".to_owned(),
                },
                ChromeOverride::Unbind {
                    table: "sidebar",
                    key: "q".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn chrome_binding_overrides_report_what_they_cannot_honour() {
        let parsed = parse_config(
            "chrome-keybind = browser:D-t\n\
             chrome-keybind = pane:D-t=browser-new-tab\n\
             chrome-keybind = browser:D-t=teleport\n\
             chrome-unbind = browser\n",
        );

        assert_eq!(parsed.diagnostics.len(), 4);
        assert!(parsed.chrome_overrides.is_empty());
        assert!(
            parsed.diagnostics[1]
                .message
                .contains("unknown chrome table"),
            "{:?}",
            parsed.diagnostics[1],
        );
        assert!(
            parsed.diagnostics[2]
                .message
                .contains("unknown chrome action"),
            "{:?}",
            parsed.diagnostics[2],
        );
    }

    #[test]
    fn parser_applies_and_normalizes_the_browser_element_selector_hotkey() {
        let parsed = parse_config("browser-element-selector-hotkey = shift-alt-e\n");

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.browser.element_selector_hotkey.value,
            normalize_browser_hotkey("shift-alt-e").expect("valid hotkey")
        );
        assert_eq!(
            parsed.browser.element_selector_hotkey.provenance,
            ConfigProvenance::Override
        );
    }

    #[test]
    fn browser_element_selector_hotkey_rejects_plain_or_shift_only_typing() {
        assert!(normalize_browser_hotkey("c").is_err());
        assert!(normalize_browser_hotkey("shift-c").is_err());
        let parsed = parse_config("browser-element-selector-hotkey = c\n");

        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(
            parsed.browser.element_selector_hotkey.value,
            DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY
        );
        assert_eq!(
            parsed.browser.element_selector_hotkey.provenance,
            ConfigProvenance::Override
        );
    }

    #[test]
    fn appearance_editor_view_hides_app_side_lines_and_round_trips() {
        let source = "# header comment\n\
                      pane-gaps = true\n\
                      font-family = \"Berkeley Mono\"\n\
                      \n\
                      prefix = C-a\n\
                      agent-command = codex\n\
                      host-blue = ssh blue\n\
                      background = #282C34\n\
                      unknown-key = kept\n\
                      chrome-preset = graphite\n";
        let view = appearance_editor_view(source);
        assert_eq!(
            view,
            "# header comment\n\
             font-family = \"Berkeley Mono\"\n\
             \n\
             background = #282C34\n\
             unknown-key = kept\n"
        );

        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config");
        fs::write(&path, source).expect("write configuration");
        save_appearance_editor(&path, &view).expect("save the unchanged view");
        let merged = fs::read_to_string(&path).expect("read merged configuration");
        assert_eq!(
            merged,
            "pane-gaps = true\n\
             prefix = C-a\n\
             agent-command = codex\n\
             host-blue = ssh blue\n\
             chrome-preset = graphite\n\
             # header comment\n\
             font-family = \"Berkeley Mono\"\n\
             \n\
             background = #282C34\n\
             unknown-key = kept\n"
        );
        assert_eq!(appearance_editor_view(&merged), view);
    }

    #[test]
    fn appearance_editor_rejects_app_side_keys_and_leaves_the_file_alone() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config");
        fs::write(&path, "pane-gaps = true\n").expect("write configuration");
        let error = save_appearance_editor(&path, "font-size = 14\npane-margin = 4\n")
            .expect_err("app-side keys must not pass through the appearance editor");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains("pane-margin"));
        assert_eq!(
            fs::read_to_string(&path).expect("read unchanged configuration"),
            "pane-gaps = true\n"
        );
    }

    #[test]
    fn appearance_editor_saves_into_a_missing_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config");
        save_appearance_editor(&path, "font-size = 14\n").expect("create the file");
        assert_eq!(
            fs::read_to_string(&path).expect("read created configuration"),
            "font-size = 14\n"
        );
    }

    #[test]
    fn parser_applies_all_supported_keys() {
        let parsed = parse_config(
            "\
             # $XDG_CONFIG_HOME/zz/config\n\
             pane-gaps = true\n\
             pane-corner-radius = 9 # inline comments are allowed\n\
             pane-margin = 6\n\
             pane-border-width = 2.5\n\
             window-corner-radius = 10\n\
             use-system-titlebar = true\n\
             window-background-blur = true\n\
             animations = false\n\
             show-fps = true\n\
             quit-daemon-on-exit = true\n\
             auto-restart-stale-daemon = true\n\
             experimental-agent-pane = true\n\
             experimental-editor-pane = true\n\
             editor-font-size = 15\n\
             editor-line-numbers = false\n\
             editor-relative-line-numbers = false\n\
             editor-soft-wrap = false\n\
             editor-vim-mode = false\n",
        );

        assert!(parsed.diagnostics.is_empty());
        assert!(parsed.config.experimental_agent_pane.value);
        assert!(parsed.config.experimental_editor_pane.value);
        assert_f32_eq(parsed.config.editor_font_size.value, 15.0);
        assert_eq!(
            parsed.config.editor_font_size.provenance,
            ConfigProvenance::Override
        );
        assert!(!parsed.config.editor_line_numbers.value);
        assert!(!parsed.config.editor_relative_line_numbers.value);
        assert!(!parsed.config.editor_soft_wrap.value);
        assert!(!parsed.config.editor_vim_mode.value);
        assert_eq!(
            parsed.config.editor_line_numbers.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.editor_relative_line_numbers.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.editor_soft_wrap.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.editor_vim_mode.provenance,
            ConfigProvenance::Override
        );
        assert!(parsed.config.pane_gaps.value);
        assert_f32_eq(parsed.config.pane_corner_radius.value, 9.0);
        assert_f32_eq(parsed.config.pane_margin.value, 6.0);
        assert_f32_eq(parsed.config.pane_border_width.value, 2.5);
        assert_eq!(
            parsed.config.pane_gaps.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_corner_radius.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_margin.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_border_width.provenance,
            ConfigProvenance::Override
        );
        assert_f32_eq(parsed.config.window_corner_radius.value, 10.0);
        assert_eq!(
            parsed.config.window_corner_radius.provenance,
            ConfigProvenance::Override
        );
        assert!(parsed.config.use_system_titlebar.value);
        assert_eq!(
            parsed.config.use_system_titlebar.provenance,
            ConfigProvenance::Override
        );
        assert!(parsed.config.window_background_blur.value);
        assert_eq!(
            parsed.config.window_background_blur.provenance,
            ConfigProvenance::Override
        );
        assert!(!parsed.config.animations.value);
        assert_eq!(
            parsed.config.animations.provenance,
            ConfigProvenance::Override
        );
        assert!(parsed.config.show_fps.value);
        assert_eq!(
            parsed.config.show_fps.provenance,
            ConfigProvenance::Override
        );
        assert!(parsed.config.quit_daemon_on_exit.value);
        assert_eq!(
            parsed.config.quit_daemon_on_exit.provenance,
            ConfigProvenance::Override
        );
        assert!(parsed.config.auto_restart_stale_daemon.value);
        assert_eq!(
            parsed.config.auto_restart_stale_daemon.provenance,
            ConfigProvenance::Override
        );
        assert!(!AppConfig::default().auto_restart_stale_daemon.value);
    }

    #[test]
    fn parser_applies_the_theme_keys() {
        let parsed = parse_config(
            "theme-mode = dark\n\
             app-icon = light\n\
             chrome-preset = tokyo-night\n\
             chrome-background = #1a1b26\n\
             chrome-foreground = #c0caf5\n\
             chrome-border = #292e42 # trailing comments still work\n\
             chrome-success = #9ece6a\n\
             chrome-warning = #e0af68\n\
             chrome-danger = #f7768e\n",
        );

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.config.theme_mode.value, ThemeModeSetting::Dark);
        assert_eq!(
            parsed.config.theme_mode.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(parsed.config.app_icon.value, AppIconSetting::Light);
        assert_eq!(
            parsed.config.app_icon.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.chrome_preset.value,
            Some(ChromePresetId::TokyoNight)
        );
        assert_eq!(
            parsed.config.chrome_preset.provenance,
            ConfigProvenance::Override
        );
        for color in ChromeColor::ALL {
            let setting = parsed.config.chrome(color);
            assert_eq!(setting.provenance, ConfigProvenance::Override, "{color:?}");
            assert!(setting.value.is_some(), "{color:?}");
        }
        assert_eq!(
            parsed
                .config
                .chrome(ChromeColor::Background)
                .value
                .map(zz_ui::to_hex),
            Some("#1a1b26".to_owned())
        );
    }

    #[test]
    fn unset_theme_keys_inherit_rather_than_defaulting_to_a_color() {
        let parsed = parse_config("pane-gaps = true\n");
        assert_eq!(parsed.config.theme_mode.value, ThemeModeSetting::System);
        assert_eq!(parsed.config.app_icon.value, AppIconSetting::Automatic);
        assert_eq!(parsed.config.chrome_preset.value, None);
        assert_eq!(
            parsed.config.chrome_preset.provenance,
            ConfigProvenance::Default
        );
        for color in ChromeColor::ALL {
            assert_eq!(parsed.config.chrome(color).value, None, "{color:?}");
            assert_eq!(
                parsed.config.chrome(color).provenance,
                ConfigProvenance::Default,
                "{color:?}"
            );
        }
    }

    #[test]
    fn invalid_theme_values_warn_and_keep_the_previous_value() {
        let parsed = parse_config(
            "theme-mode = midnight\n\
             app-icon = rainbow\n\
             chrome-preset = vaporwave\n\
             chrome-background = not-a-color\n",
        );

        assert_eq!(parsed.diagnostics.len(), 4);
        assert_eq!(parsed.config.theme_mode.value, ThemeModeSetting::System);
        assert_eq!(parsed.config.app_icon.value, AppIconSetting::Automatic);
        assert_eq!(parsed.config.chrome_preset.value, None);
        assert_eq!(parsed.config.chrome(ChromeColor::Background).value, None);
    }

    #[test]
    fn every_preset_variant_parses_and_covers_each_root() {
        for preset in &crate::theme::CHROME_PRESETS {
            assert_eq!(preset.id.preset().name, preset.name);
            for (mode, colors) in [
                (
                    zz_ui::ThemeMode::Light,
                    preset.colors(zz_ui::ThemeMode::Light),
                ),
                (
                    zz_ui::ThemeMode::Dark,
                    preset.colors(zz_ui::ThemeMode::Dark),
                ),
            ] {
                assert_eq!(colors.len(), ChromeColor::ALL.len(), "{}", preset.name);
                for hex in colors {
                    assert!(
                        zz_ui::parse_hex(hex).is_ok(),
                        "{} {mode:?}: {hex} does not parse",
                        preset.name
                    );
                }
            }
            let light_background =
                zz_ui::parse_hex(preset.light[0]).expect("light background parses");
            let dark_background = zz_ui::parse_hex(preset.dark[0]).expect("dark background parses");
            assert!(
                light_background.l > dark_background.l,
                "{} variants are reversed",
                preset.name
            );
        }
    }

    #[test]
    fn retired_keys_are_unsupported() {
        let parsed = parse_config(
            "frame-content-corner-radius = 12.5\n\
             pane-content-corner-radius = 12.5\n\
             show-app-fps = true\n\
             show-browser-fps = true\n\
             corner-shape = round\n\
             pane-shadow = false\n\
             pane-gaps = true\n",
        );

        assert_eq!(
            parsed.config,
            AppConfig {
                pane_gaps: ConfigValue {
                    value: true,
                    provenance: ConfigProvenance::Override,
                },
                ..AppConfig::default()
            }
        );
        assert_eq!(parsed.diagnostics.len(), 6);
        assert!(
            parsed
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.message.contains("unsupported key"))
        );
    }

    #[test]
    fn agent_adapter_keys_reach_the_daemon_while_the_working_directory_stays_local() {
        let working_directory = absolute_test_root("agent project");
        let parsed = parse_config(&format!(
            "agent-command = {{\"command\":\"node\",\"args\":[\"agent.js\"],\"env\":{{\"TOKEN\":\"#not-a-comment\"}}}} # command comment\n\
             agent-claude-code-command = claude-agent-acp --stdio\n\
             agent-auto-approve = false\n\
             agent-working-directory = {}\n",
            working_directory.display()
        ));

        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert_eq!(parsed.agent.working_directory, Some(working_directory));
        assert_eq!(
            parsed.daemon_entries,
            [
                (
                    "agent-command".to_owned(),
                    "{\"command\":\"node\",\"args\":[\"agent.js\"],\"env\":{\"TOKEN\":\"#not-a-comment\"}}"
                        .to_owned()
                ),
                (
                    "agent-claude-code-command".to_owned(),
                    "claude-agent-acp --stdio".to_owned()
                ),
                ("agent-auto-approve".to_owned(), "false".to_owned()),
            ],
            "the daemon spawns the adapter, so its keys travel as mux options"
        );
    }

    #[test]
    fn invalid_agent_working_directory_keeps_the_last_valid_value() {
        let valid_working_directory = absolute_test_root("valid-agent-directory");
        let parsed = parse_config(&format!(
            "agent-working-directory = {}\n\
             agent-working-directory = relative/path\n",
            valid_working_directory.display()
        ));

        assert_eq!(
            parsed.agent.working_directory,
            Some(valid_working_directory)
        );
        assert_eq!(parsed.diagnostics.len(), 1);
        assert!(
            parsed.diagnostics[0]
                .message
                .contains("expected an absolute path")
        );
    }

    #[test]
    fn remote_override_filter_strips_only_experimental_pane_keys() {
        let entries = vec![
            (
                MuxOptionKey::ExperimentalAgentPane.as_str().to_owned(),
                "true".to_owned(),
            ),
            ("background".to_owned(), "#101010".to_owned()),
            (MuxOptionKey::Prefix.as_str().to_owned(), "C-a".to_owned()),
            (
                MuxOptionKey::ExperimentalEditorPane.as_str().to_owned(),
                "true".to_owned(),
            ),
        ];

        assert_eq!(
            config_overrides_for_host(entries.clone(), false),
            entries,
            "local pushes stay byte-for-byte unchanged"
        );
        assert_eq!(
            config_overrides_for_host(entries, true),
            [
                ("background".to_owned(), "#101010".to_owned()),
                (MuxOptionKey::Prefix.as_str().to_owned(), "C-a".to_owned(),),
            ]
        );
    }

    #[test]
    fn fleet_hosts_parse_in_config_order_without_becoming_daemon_entries() {
        let parsed = parse_config(
            "\
             host-desktop = ssh://fabrico@desktop:2222\n\
             background = #101010\n\
             host-scratch = unix:///tmp/zz-scratch.sock\n\
             host-legacy = /tmp/zz-legacy.sock\n",
        );
        let expected = vec![
            HostEntry {
                name: "desktop".to_owned(),
                endpoint: Endpoint::parse("ssh://fabrico@desktop:2222").expect("desktop endpoint"),
            },
            HostEntry {
                name: "scratch".to_owned(),
                endpoint: Endpoint::parse("unix:///tmp/zz-scratch.sock").expect("scratch endpoint"),
            },
            HostEntry {
                name: "legacy".to_owned(),
                endpoint: Endpoint::parse("/tmp/zz-legacy.sock").expect("legacy endpoint"),
            },
        ];

        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert_eq!(parsed.hosts, expected);
        assert_eq!(
            parsed.daemon_entries,
            [("background".to_owned(), "#101010".to_owned())]
        );
    }

    #[test]
    fn valid_fleet_endpoint_is_retained_while_an_invalid_one_is_dropped() {
        let parsed = parse_config(
            "\
             host-gpu = ssh://gpu:7777\n\
             host-broken = quic://gpu:7777\n",
        );

        assert_eq!(
            parsed.hosts,
            [HostEntry {
                name: "gpu".to_owned(),
                endpoint: Endpoint::parse("ssh://gpu:7777").expect("ssh endpoint"),
            }]
        );
        assert!(parsed.daemon_entries.is_empty());
        assert_eq!(
            parsed.diagnostics,
            [ConfigDiagnostic {
                line: 2,
                message: "invalid `host-broken`: invalid endpoint URI `quic://gpu:7777`: quic endpoints were removed; use ssh://"
                    .to_owned(),
            }]
        );
        assert_eq!(
            parsed.rejected_hosts,
            [RejectedHost {
                name: "broken".to_owned(),
                value: "quic://gpu:7777".to_owned(),
                reason: "invalid endpoint URI `quic://gpu:7777`: quic endpoints were removed; use ssh://"
                    .to_owned(),
            }]
        );
    }

    #[test]
    fn duplicate_fleet_host_warns_and_moves_the_winner_to_its_config_position() {
        let parsed = parse_config(
            "\
             host-desktop = ssh://old-desktop\n\
             host-server = ssh://server\n\
             host-desktop = ssh://new-desktop\n",
        );

        assert_eq!(
            parsed.hosts,
            [
                HostEntry {
                    name: "server".to_owned(),
                    endpoint: Endpoint::parse("ssh://server").expect("server endpoint"),
                },
                HostEntry {
                    name: "desktop".to_owned(),
                    endpoint: Endpoint::parse("ssh://new-desktop")
                        .expect("replacement desktop endpoint"),
                },
            ]
        );
        assert!(parsed.daemon_entries.is_empty());
        assert_eq!(
            parsed.diagnostics,
            [ConfigDiagnostic {
                line: 3,
                message: "duplicate host `desktop`; last entry wins".to_owned(),
            }]
        );
    }

    #[test]
    fn removing_a_fleet_host_drops_every_duplicate_and_preserves_other_lines() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(
            &path,
            "# keep\nhost-desktop = ssh://old\nshow-fps = true\nhost-desktop=ssh://new:9922 # effective\nhost-server = ssh://server:9922\n",
        )
        .unwrap();

        assert!(remove_fleet_host_at(&path, "desktop").unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# keep\nshow-fps = true\nhost-server = ssh://server:9922\n"
        );
        assert!(!remove_fleet_host_at(&path, "desktop").unwrap());
    }

    #[test]
    fn invalid_fleet_host_names_warn_and_are_dropped() {
        let parsed = parse_config(
            "\
             host-local = ssh://local-alias\n\
             host- = ssh://unnamed\n\
             host-bad name = ssh://bad-name\n",
        );

        assert!(parsed.hosts.is_empty());
        assert!(parsed.daemon_entries.is_empty());
        assert_eq!(
            parsed.diagnostics,
            [
                ConfigDiagnostic {
                    line: 1,
                    message: "invalid `host-local`: host name `local` is reserved".to_owned(),
                },
                ConfigDiagnostic {
                    line: 2,
                    message: "invalid `host-`: host name must not be empty".to_owned(),
                },
                ConfigDiagnostic {
                    line: 3,
                    message: "invalid `host-bad name`: host name must not contain whitespace"
                        .to_owned(),
                },
            ]
        );
        assert_eq!(
            parsed
                .rejected_hosts
                .iter()
                .map(|host| host.name.as_str())
                .collect::<Vec<_>>(),
            ["local", "", "bad name"]
        );
    }

    #[test]
    fn fleet_host_writer_replaces_in_place_and_preserves_every_other_byte() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("zz/config");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = "# keep this comment\r\n\
                      host-desktop  = ssh://old-desktop  # keep this too\r\n\
                      show-fps = true\r\n";
        fs::write(&path, source).unwrap();

        write_fleet_host_at(&path, "desktop", "ssh://desktop:9922").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# keep this comment\r\n\
             host-desktop  = ssh://desktop:9922  # keep this too\r\n\
             show-fps = true\r\n"
        );

        write_fleet_host_at(&path, "desktop", "ssh://desktop:7444").unwrap();
        let edited = fs::read_to_string(&path).unwrap();
        assert!(edited.contains("host-desktop  = ssh://desktop:7444  # keep this too\r\n"));
        assert_eq!(edited.matches("host-desktop").count(), 1);
        assert!(validate_fleet_host("local", "ssh://desktop:7444").is_err());
    }

    #[test]
    fn an_added_ssh_host_round_trips_through_the_config_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, "show-fps = true\n").unwrap();

        write_fleet_host_at(&path, "arch-desktop", "ssh://fabrico@arch-desktop:2222").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "show-fps = true\nhost-arch-desktop = ssh://fabrico@arch-desktop:2222\n"
        );
        let parsed = load_config(&path).unwrap();
        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert_eq!(
            parsed.hosts,
            [HostEntry {
                name: "arch-desktop".to_owned(),
                endpoint: Endpoint::Ssh(zz_daemon::SshEndpoint {
                    user: Some("fabrico".to_owned()),
                    host: "arch-desktop".to_owned(),
                    port: Some(2222),
                    remote_socket: None,
                }),
            }]
        );
    }

    #[test]
    fn daemon_owned_entries_preserve_file_order_and_repeated_keys() {
        let parsed = parse_config(
            "\
             background = #101010\n\
             palette = 1=#112233\n\
             pane-corner-radius = 24\n\
             font-family = First Mono\n\
             palette = 2=#445566 # trailing comment\n\
             font-family = Emoji Fallback\n",
        );

        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert_eq!(
            parsed.daemon_entries,
            [
                ("background".to_owned(), "#101010".to_owned()),
                ("palette".to_owned(), "1=#112233".to_owned()),
                ("font-family".to_owned(), "First Mono".to_owned()),
                ("palette".to_owned(), "2=#445566".to_owned()),
                ("font-family".to_owned(), "Emoji Fallback".to_owned()),
            ]
        );
    }

    #[test]
    fn all_mux_options_are_collected_raw_in_file_order() {
        let parsed = parse_config(
            "\
             prefix = C-a\n\
             mode-keys = vi\n\
             history-limit = not-parsed-here\n\
             word-separators = !@#\n\
             copy-command = pbcopy --flag\n\
             set-clipboard = external\n\
             buffer-limit = 12\n\
             synchronize-panes = on\n\
             prefix = C-Space\n",
        );

        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert_eq!(
            parsed.daemon_entries,
            [
                ("prefix".to_owned(), "C-a".to_owned()),
                ("mode-keys".to_owned(), "vi".to_owned()),
                ("history-limit".to_owned(), "not-parsed-here".to_owned()),
                ("word-separators".to_owned(), "!@#".to_owned()),
                ("copy-command".to_owned(), "pbcopy --flag".to_owned()),
                ("set-clipboard".to_owned(), "external".to_owned()),
                ("buffer-limit".to_owned(), "12".to_owned()),
                ("synchronize-panes".to_owned(), "on".to_owned()),
                ("prefix".to_owned(), "C-Space".to_owned()),
            ]
        );
    }

    #[test]
    fn experimental_pane_flags_apply_locally_and_forward_to_the_daemon() {
        let parsed = parse_config(
            "\
             experimental-agent-pane = true\n\
             experimental-editor-pane = false\n",
        );

        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert!(parsed.config.experimental_agent_pane.value);
        assert!(!parsed.config.experimental_editor_pane.value);
        assert_eq!(
            parsed.daemon_entries,
            [
                ("experimental-agent-pane".to_owned(), "true".to_owned()),
                ("experimental-editor-pane".to_owned(), "false".to_owned()),
            ]
        );
    }

    #[test]
    fn hash_comment_rule_preserves_colors_separators_and_quoted_commands() {
        let parsed = parse_config(
            "background = #112233 # trailing color comment\n\
             word-separators = !\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~\n\
             copy-command = sh -c 'printf #copied' target file # trailing command comment\n",
        );

        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        assert_eq!(
            parsed.daemon_entries,
            [
                ("background".to_owned(), "#112233".to_owned()),
                (
                    "word-separators".to_owned(),
                    "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~".to_owned(),
                ),
                (
                    "copy-command".to_owned(),
                    "sh -c 'printf #copied' target file".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn writer_uses_the_same_hash_comment_boundary_as_the_parser() {
        let source = "background = #112233 # keep color comment\n\
                      palette = 1=#445566 # keep palette comment\n\
                      word-separators = !@# # keep mux comment\n";

        let edited = edit_config_source(source, "background", Some("#AABBCC"));
        let edited = edit_config_source(&edited, "palette", Some("1=#DDEEFF"));
        let edited = edit_config_source(&edited, "word-separators", Some("!#$"));

        assert_eq!(
            edited,
            "background = #AABBCC # keep color comment\n\
             palette = 1=#DDEEFF # keep palette comment\n\
             word-separators = !#$ # keep mux comment\n"
        );
        assert_eq!(
            parse_config(&edited).daemon_entries,
            [
                ("background".to_owned(), "#AABBCC".to_owned()),
                ("palette".to_owned(), "1=#DDEEFF".to_owned()),
                ("word-separators".to_owned(), "!#$".to_owned()),
            ]
        );
    }

    fn importable_values(
        appearance: &TerminalAppearance,
    ) -> Vec<(AppearanceConfigKey, Vec<String>)> {
        AppearanceConfigKey::ALL
            .into_iter()
            .filter_map(|key| {
                let group = appearance_config_values(appearance, key)
                    .unwrap_or_else(|error| panic!("serialize {key:?}: {error}"));
                (!group.is_empty()).then_some((key, group))
            })
            .collect()
    }

    #[test]
    fn import_round_trips_every_appearance_shape() {
        let appearance = distinctive_appearance();
        let values = importable_values(&appearance);
        let source = "# user header stays byte-identical\r\n\
                      unknown-key = untouched\r\n";

        let imported = apply_import_edits(source, &values).expect("import donor values");

        assert!(imported.starts_with(source));
        assert!(imported.contains("font-family = \"Berkeley Mono\"\r\n"));
        assert!(imported.contains("font-family = \"Symbols # Fallback\"\r\n"));
        assert!(imported.contains("font-family-bold = \"Berkeley Mono Bold\"\r\n"));
        assert!(imported.contains("font-family-italic = \"Berkeley Mono Italic\"\r\n"));
        assert!(imported.contains("font-family-bold-italic = \"Berkeley Mono Bold Italic\"\r\n"));
        assert!(imported.contains("font-synthetic-style = no-bold,italic,no-bold-italic\r\n"));
        assert!(imported.contains("font-thicken = true\r\n"));
        assert!(imported.contains("font-thicken-strength = 173\r\n"));
        assert!(imported.contains("cursor-style = underline\r\n"));
        let palette_lines = imported
            .lines()
            .filter(|line| line.starts_with("palette ="))
            .collect::<Vec<_>>();
        assert_eq!(
            palette_lines,
            ["palette = 1=#214365", "palette = 42=#FEDCBA"]
        );

        let parsed = parse_config(&imported);
        let resolved = apply_appearance_overrides(
            AppearanceLoad::defaults_for(appearance.color_scheme),
            &parsed_appearance_entries(&parsed),
        );
        assert_eq!(resolved.appearance, appearance);
        for (key, _) in values {
            assert_eq!(
                resolved.provenance.source(key),
                AppearanceSource::Override,
                "{key:?}"
            );
        }
    }

    #[test]
    fn import_replaces_existing_keys_in_place_and_preserves_comments() {
        let source = "# user header\n\
                      background = #123456 # keep comment\n\
                      unknown-key = untouched\n";

        let imported = apply_import_edits(
            source,
            &[
                (AppearanceConfigKey::Background, vec!["#654321".to_owned()]),
                (AppearanceConfigKey::Foreground, vec!["#ABCDEF".to_owned()]),
            ],
        )
        .expect("import replaces in place");

        assert_eq!(
            imported,
            "# user header\n\
             background = #654321 # keep comment\n\
             unknown-key = untouched\n\
             foreground = #ABCDEF\n"
        );
    }

    #[test]
    fn reimport_syncs_changed_donor_values_without_accumulation() {
        let first = apply_import_edits(
            "",
            &[
                (AppearanceConfigKey::FontSize, vec!["14".to_owned()]),
                (AppearanceConfigKey::Palette, vec!["1=#111111".to_owned()]),
            ],
        )
        .expect("first import");
        let second = apply_import_edits(
            &first,
            &[
                (AppearanceConfigKey::FontSize, vec!["16".to_owned()]),
                (AppearanceConfigKey::Palette, vec!["2=#222222".to_owned()]),
            ],
        )
        .expect("second import");

        assert_eq!(second, "font-size = 16\npalette = 2=#222222\n");
    }

    #[test]
    fn import_replaces_cumulative_groups_wholesale_and_keeps_neighbors() {
        let source = "palette = 1=#111111\n\
                      # palette comment stays\n\
                      palette = 2=#222222\n\
                      font-size = 14\n";

        let imported = apply_import_edits(
            source,
            &[(
                AppearanceConfigKey::Palette,
                vec!["1=#214365".to_owned(), "42=#FEDCBA".to_owned()],
            )],
        )
        .expect("import replaces the group");

        assert_eq!(
            imported,
            "# palette comment stays\n\
             font-size = 14\n\
             palette = 1=#214365\n\
             palette = 42=#FEDCBA\n"
        );
    }

    #[test]
    fn import_represents_natural_cell_height_with_a_reset() {
        let imported = apply_import_edits(
            "# keep\n",
            &[(AppearanceConfigKey::AdjustCellHeight, vec![String::new()])],
        )
        .expect("import natural cell height");
        assert_eq!(imported, "# keep\nadjust-cell-height = \n");

        let parsed = parse_config(&imported);
        let resolved = apply_appearance_overrides(
            AppearanceLoad::defaults_for(TerminalColorScheme::Dark),
            &parsed_appearance_entries(&parsed),
        );
        assert_eq!(
            resolved.appearance.cell_height_adjustment,
            CellHeightAdjustment::None
        );
    }

    #[test]
    fn import_without_values_is_byte_identical() {
        let source = "# comments\r\nunknown = bytes # exactly\r\n";
        assert_eq!(
            apply_import_edits(source, &[]).expect("no-op import"),
            source
        );
    }

    #[test]
    fn invalid_and_unknown_entries_warn_without_replacing_defaults() {
        let parsed = parse_config(
            "\
             pane-corner-radius = -1\n\
             pane-margin = NaN\n\
             window-corner-radius = 300\n\
             use-system-titlebar = sometimes\n\
             window-background-blur = perhaps\n\
             future-setting = 12\n\
             malformed\n",
        );

        assert_f32_eq(parsed.config.pane_corner_radius.value, 13.5);
        assert_f32_eq(parsed.config.pane_margin.value, 6.0);
        assert_f32_eq(parsed.config.window_corner_radius.value, 13.5);
        assert_eq!(
            parsed.config.window_corner_radius.provenance,
            ConfigProvenance::Override
        );
        assert!(!parsed.config.use_system_titlebar.value);
        assert_eq!(
            parsed.config.use_system_titlebar.provenance,
            ConfigProvenance::Override
        );
        assert!(!parsed.config.window_background_blur.value);
        assert_eq!(
            parsed.config.window_background_blur.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_corner_radius.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_margin.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(parsed.diagnostics.len(), 7);
        assert!(parsed.diagnostics[0].message.contains("between 0 and 32"));
        assert!(parsed.diagnostics[1].message.contains("finite"));
        assert!(parsed.diagnostics[2].message.contains("between 0 and 32"));
        assert!(parsed.diagnostics[3].message.contains("true` or `false"));
        assert!(parsed.diagnostics[4].message.contains("true` or `false"));
        assert!(parsed.diagnostics[5].message.contains("unsupported key"));
        assert_eq!(parsed.diagnostics[6].message, "expected `key = value`");
    }

    #[test]
    fn parser_validates_pane_chrome_switch_and_geometry() {
        let parsed = parse_config("pane-gaps = yes\npane-border-width = 9\n");

        assert!(!parsed.config.pane_gaps.value);
        assert_f32_eq(parsed.config.pane_border_width.value, 1.0);
        assert_eq!(
            parsed.config.pane_gaps.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_border_width.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(parsed.diagnostics.len(), 2);
        assert!(parsed.diagnostics[0].message.contains("true` or `false"));
        assert!(parsed.diagnostics[1].message.contains("between 0 and 8"));
    }

    #[test]
    fn later_valid_entries_replace_earlier_values() {
        let parsed = parse_config(
            "\
             pane-corner-radius = 12\n\
             pane-corner-radius = nope\n\
             pane-corner-radius = 16\n",
        );

        assert_f32_eq(parsed.config.pane_corner_radius.value, 16.0);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn invalid_duplicate_retains_the_previous_valid_value() {
        let parsed = parse_config(
            "pane-corner-radius = 12\n\
             pane-corner-radius = nope\n",
        );

        assert_f32_eq(parsed.config.pane_corner_radius.value, 12.0);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn writer_edits_only_the_last_occurrence_and_preserves_surrounding_bytes() {
        let source = "# keep this comment\r\n\
                      pane-corner-radius=12 # earlier value\r\n\
                      future syntax without equals\r\n\
                      pane-corner-radius = 20.0  # effective value\r\n\
                      unknown-key = untouched\r\n";

        assert_eq!(
            edit_config_source(source, ConfigKey::PaneCornerRadius.as_str(), Some("24"),),
            "# keep this comment\r\n\
             pane-corner-radius=12 # earlier value\r\n\
             future syntax without equals\r\n\
             pane-corner-radius = 24  # effective value\r\n\
             unknown-key = untouched\r\n"
        );
    }

    #[test]
    fn writer_preserves_compact_spacing_and_inline_comment_on_edited_line() {
        assert_eq!(
            edit_config_source(
                "quit-daemon-on-exit=true # keep\n",
                ConfigKey::QuitDaemonOnExit.as_str(),
                Some("false"),
            ),
            "quit-daemon-on-exit=false # keep\n"
        );
    }

    #[test]
    fn writer_reset_removes_only_the_last_matching_line() {
        let source = "# before\n\
                      pane-margin = 8\n\
                      unknown line stays byte-identical\n\
                      pane-margin=18 # reset this one\n\
                      # after\n";

        assert_eq!(
            edit_config_source(source, ConfigKey::PaneMargin.as_str(), None),
            "# before\n\
             pane-margin = 8\n\
             unknown line stays byte-identical\n\
             # after\n"
        );
    }

    #[test]
    fn pane_chrome_keys_round_trip_through_the_config_writer() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        for (key, value) in [
            (ConfigKey::PaneGaps, "true"),
            (ConfigKey::PaneBorderWidth, "2.5"),
        ] {
            write_config_edit_at(&path, key.as_str(), Some(value))
                .expect("write pane chrome setting");
        }

        let source = fs::read_to_string(path).expect("read pane chrome settings");
        let parsed = parse_config(&source);
        assert!(parsed.diagnostics.is_empty());
        assert!(parsed.config.pane_gaps.value);
        assert_f32_eq(parsed.config.pane_border_width.value, 2.5);
    }

    #[test]
    fn applying_a_preset_round_trips_through_the_config_writer() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        write_config_edit_at(
            &path,
            ConfigKey::ThemeMode.as_str(),
            Some(ThemeModeSetting::System.as_str()),
        )
        .expect("write theme mode");
        write_config_edit_at(
            &path,
            ConfigKey::Chrome(ChromeColor::Background).as_str(),
            Some("#123456"),
        )
        .expect("write explicit background");
        write_config_edit_at(
            &path,
            ConfigKey::Chrome(ChromeColor::Danger).as_str(),
            Some("#abcdef"),
        )
        .expect("write explicit danger");
        let preset = ChromePresetId::TokyoNight;
        write_chrome_preset_at(&path, preset).expect("write paired chrome preset");

        let source = fs::read_to_string(&path).expect("read theme settings");
        let parsed = parse_config(&source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert_eq!(parsed.config.theme_mode.value, ThemeModeSetting::System);
        assert_eq!(parsed.config.chrome_preset.value, Some(preset));
        for color in ChromeColor::ALL {
            assert_eq!(parsed.config.chrome(color).value, None, "{color:?}");
        }
        assert!(source.contains("theme-mode = system"));
        assert!(source.contains("chrome-preset = tokyo-night"));
        assert!(!source.contains("chrome-background"));
        assert!(!source.contains("chrome-danger"));
    }

    #[test]
    fn the_app_icon_setting_round_trips_through_the_config_writer() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        for setting in AppIconSetting::ALL {
            write_config_edit_at(&path, ConfigKey::AppIcon.as_str(), Some(setting.as_str()))
                .expect("write app icon");

            let source = fs::read_to_string(&path).expect("read app icon setting");
            let parsed = parse_config(&source);
            assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
            assert_eq!(parsed.config.app_icon.value, setting);
        }
    }

    #[test]
    fn resetting_a_chrome_color_returns_it_to_inherited() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        let key = ConfigKey::Chrome(ChromeColor::Background);
        write_config_edit_at(&path, key.as_str(), Some("#1a1b26")).expect("write");
        write_config_edit_at(&path, key.as_str(), None).expect("reset");

        let source = fs::read_to_string(path).expect("read");
        let parsed = parse_config(&source);
        assert_eq!(parsed.config.chrome(ChromeColor::Background).value, None);
        assert_eq!(
            parsed.config.chrome(ChromeColor::Background).provenance,
            ConfigProvenance::Default
        );
    }

    #[test]
    fn widget_corner_radius_defaults_to_the_theme_radius_and_takes_an_override() {
        let parsed = parse_config("");
        assert_f32_eq(
            parsed.config.widget_corner_radius.value,
            DEFAULT_WIDGET_CORNER_RADIUS,
        );
        assert_eq!(
            parsed.config.widget_corner_radius.provenance,
            ConfigProvenance::Default
        );

        let parsed = parse_config("widget-corner-radius = 12\n");
        assert!(parsed.diagnostics.is_empty());
        assert_f32_eq(parsed.config.widget_corner_radius.value, 12.0);
        assert_eq!(
            parsed.config.widget_corner_radius.provenance,
            ConfigProvenance::Override
        );

        let parsed = parse_config("widget-corner-radius = 900\n");
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn writer_creates_a_fresh_file_and_parent_directories_atomically() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("nested/zz/config");

        write_config_edit_at(&path, ConfigKey::PaneMargin.as_str(), Some("19"))
            .expect("create fresh configuration");

        assert_eq!(
            fs::read_to_string(&path).expect("read fresh configuration"),
            "pane-margin = 19\n"
        );
        let entries = fs::read_dir(path.parent().expect("configuration parent"))
            .expect("read configuration directory")
            .map(|entry| entry.expect("directory entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [std::ffi::OsString::from(CONFIG_FILE_NAME)]);
    }

    #[test]
    fn writer_rejects_an_edit_past_the_bound_without_touching_the_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        let source = " ".repeat(MAX_CONFIG_BYTES);
        fs::write(&path, &source).expect("write bounded configuration");

        let error = write_config_edit_at(&path, ConfigKey::ShowFps.as_str(), Some("true"))
            .expect_err("appending past the read bound must fail");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(
            fs::read_to_string(&path).expect("read unchanged configuration"),
            source
        );
    }

    #[test]
    fn import_rejects_growth_past_the_bound_without_touching_the_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        let source = " ".repeat(MAX_CONFIG_BYTES);
        fs::write(&path, &source).expect("write bounded configuration");

        let error = import_appearance_values_at(
            &path,
            &[(AppearanceConfigKey::Background, vec!["#654321".to_owned()])],
        )
        .expect_err("import past the bound must fail");

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeding the 65536-byte limit"));
        assert_eq!(
            fs::read_to_string(&path).expect("read unchanged configuration"),
            source
        );
    }

    #[test]
    fn deleting_ghostty_donors_after_import_changes_nothing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let ghostty_root = directory.path().join("xdg/ghostty/config");
        let theme = directory.path().join("xdg/ghostty/themes/M3 Distinctive");
        let zz_config = directory.path().join("xdg/zz/config");
        fs::create_dir_all(theme.parent().expect("theme parent")).expect("create theme directory");
        fs::create_dir_all(zz_config.parent().expect("zz config parent"))
            .expect("create zz directory");
        fs::write(
            &theme,
            "background = #102938\n\
             foreground = #F1E2D3\n\
             selection-background = #22446688\n\
             palette = 1=#A1B2C3\n\
             palette = 42=#0F1E2D\n\
             zz-link-color = #55AAEE\n",
        )
        .expect("write theme");
        fs::write(
            &ghostty_root,
            "theme = M3 Distinctive\n\
             cursor-color = #ABCDEF\n\
             font-family = M3 Mono Family\n\
             window-padding-x = 4,9\n\
             background-opacity = 0.83\n",
        )
        .expect("write Ghostty root");
        fs::write(
            &zz_config,
            "# existing local settings and unknown lines survive\n\
             pane-corner-radius = 12\n\
             future-key = byte-identical\n",
        )
        .expect("write zz config");

        let donor_appearance =
            load_ghostty_appearance_from_for(&ghostty_root, TerminalColorScheme::Dark);
        let values = crate::config::import::ghostty_import_values(&donor_appearance)
            .expect("serialize donor values");
        import_appearance_values_at(&zz_config, &values).expect("write imported zz config");

        fs::remove_file(&ghostty_root).expect("delete Ghostty donor");
        fs::remove_file(&theme).expect("delete Ghostty theme donor");
        assert!(!ghostty_root.exists());
        assert!(!theme.exists());

        let imported = fs::read_to_string(&zz_config).expect("read imported zz config");
        assert!(imported.starts_with(
            "# existing local settings and unknown lines survive\n\
             pane-corner-radius = 12\n\
             future-key = byte-identical\n"
        ));
        let parsed = parse_config(&imported);
        let resolved = apply_appearance_overrides(
            AppearanceLoad::defaults_for(TerminalColorScheme::Dark),
            &parsed_appearance_entries(&parsed),
        );
        assert_eq!(resolved.appearance, donor_appearance.appearance);
    }

    #[gpui::test]
    fn gpui_global_exposes_configured_pane_geometry(cx: &mut gpui::TestAppContext) {
        assert_eq!(cx.update(|cx| resolved_config(cx)), AppConfig::default());

        let config = parse_config("pane-corner-radius = 9\npane-margin = 6\n").config;
        cx.update(|cx| cx.set_global(config));

        let resolved = cx.update(|cx| resolved_config(cx));
        assert_f32_eq(resolved.pane_corner_radius.value, 9.0);
        assert_f32_eq(resolved.pane_margin.value, 6.0);
    }

    #[gpui::test]
    fn install_config_publishes_and_clears_browser_config(cx: &mut gpui::TestAppContext) {
        let parsed = parse_config("browser-element-selector-hotkey = alt-shift-e\n");
        let expected = parsed.browser.clone();

        cx.update(|cx| {
            install_config(Some(Path::new("/tmp/zz/config")), Some(Ok(parsed)), cx);
        });
        assert_eq!(cx.update(|cx| browser_config(cx)), expected);

        cx.update(|cx| install_config(None, None, cx));
        assert_eq!(cx.update(|cx| browser_config(cx)), BrowserConfig::default());
    }

    #[gpui::test]
    fn install_config_applies_and_resets_the_animation_setting(cx: &mut gpui::TestAppContext) {
        let parsed = parse_config("animations = false\n");

        cx.update(|cx| {
            install_config(Some(Path::new("/tmp/zz/config")), Some(Ok(parsed)), cx);
        });
        assert!(cx.update(|cx| cx.reduce_motion()));

        cx.update(|cx| install_config(None, None, cx));
        assert!(!cx.update(|cx| cx.reduce_motion()));
    }

    #[gpui::test]
    fn animation_setting_preserves_platform_reduced_motion(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(PlatformReduceMotion(true));
            install_config(None, None, cx);
        });
        assert!(cx.update(|cx| cx.reduce_motion()));

        let parsed = parse_config("animations = true\n");
        cx.update(|cx| {
            install_config(Some(Path::new("/tmp/zz/config")), Some(Ok(parsed)), cx);
        });
        assert!(cx.update(|cx| cx.reduce_motion()));
    }

    #[gpui::test]
    fn install_config_publishes_and_clears_fleet_hosts(cx: &mut gpui::TestAppContext) {
        let parsed = parse_config(
            "\
             host-desktop = ssh://desktop\n\
             host-scratch = unix:///tmp/zz-scratch.sock\n",
        );
        let expected = parsed.hosts.clone();

        cx.update(|cx| {
            install_config(Some(Path::new("/tmp/zz/config")), Some(Ok(parsed)), cx);
        });
        assert_eq!(cx.update(|cx| fleet_hosts(cx)), expected);

        cx.update(|cx| install_config(None, None, cx));
        assert!(cx.update(|cx| fleet_hosts(cx)).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn system_titlebar_selects_server_side_decorations(cx: &mut gpui::TestAppContext) {
        assert_eq!(
            cx.update(|cx| window_decorations(cx)),
            WindowDecorations::Client
        );

        let config = parse_config("use-system-titlebar = true\n").config;
        cx.update(|cx| cx.set_global(config));

        assert_eq!(
            cx.update(|cx| window_decorations(cx)),
            WindowDecorations::Server
        );
    }

    #[gpui::test]
    fn window_background_blur_selects_the_platform_blur_request(cx: &mut gpui::TestAppContext) {
        assert_eq!(
            cx.update(|cx| window_background_appearance(cx)),
            UNBLURRED_WINDOW_BACKGROUND
        );

        let config = parse_config("window-background-blur = true\n").config;
        cx.update(|cx| cx.set_global(config));

        assert_eq!(
            cx.update(|cx| requested_window_background_appearance(cx)),
            WindowBackgroundAppearance::Blurred
        );

        #[cfg(target_os = "macos")]
        assert_eq!(
            cx.update(|cx| window_background_appearance(cx)),
            WindowBackgroundAppearance::Transparent
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[gpui::test]
    fn translucent_terminal_content_keeps_the_window_opaque(cx: &mut gpui::TestAppContext) {
        let appearance = Arc::new(TerminalAppearance {
            background_opacity: 0.8,
            ..TerminalAppearance::default()
        });
        cx.update(|cx| {
            crate::theme::set_terminal_appearance(appearance, cx);
        });

        assert_eq!(
            cx.update(|cx| window_background_appearance(cx)),
            UNBLURRED_WINDOW_BACKGROUND
        );
    }

    #[gpui::test]
    fn pane_content_radius_matches_the_pane_surface(cx: &mut gpui::TestAppContext) {
        let config =
            parse_config("pane-gaps = true\npane-corner-radius = 9\npane-margin = 6\n").config;
        cx.update(|cx| cx.set_global(config));

        assert_eq!(
            cx.update(|cx| pane_content_radii(cx, WindowCorners::NONE)),
            Corners {
                top_left: px(9.0),
                top_right: px(9.0),
                bottom_right: px(9.0),
                bottom_left: px(9.0),
            }
        );
    }

    #[gpui::test]
    fn pane_gap_effective_values_follow_the_toggle(cx: &mut gpui::TestAppContext) {
        assert_eq!(cx.update(|cx| pane_margin(cx)), px(0.0));
        assert_eq!(cx.update(|cx| pane_border_width(cx)), px(0.0));

        let config = parse_config("pane-gaps = true\n").config;
        cx.update(|cx| cx.set_global(config));
        assert!(cx.update(|cx| pane_gaps(cx)));
        assert_eq!(cx.update(|cx| pane_margin(cx)), px(6.0));
        assert_eq!(cx.update(|cx| pane_border_width(cx)), px(1.0));
        assert_eq!(
            cx.update(|cx| pane_content_radii(cx, WindowCorners::NONE)),
            Corners {
                top_left: px(13.5),
                top_right: px(13.5),
                bottom_right: px(13.5),
                bottom_left: px(13.5),
            }
        );

        let config = parse_config(
            "pane-gaps = true\n\
             pane-margin = 3\n\
             pane-corner-radius = 4\n\
             pane-border-width = 0\n",
        )
        .config;
        cx.update(|cx| cx.set_global(config));
        assert_eq!(cx.update(|cx| pane_margin(cx)), px(3.0));
        assert_eq!(cx.update(|cx| pane_border_width(cx)), px(0.0));
        assert_eq!(
            cx.update(|cx| pane_content_radii(cx, WindowCorners::NONE)),
            Corners {
                top_left: px(4.0),
                top_right: px(4.0),
                bottom_right: px(4.0),
                bottom_left: px(4.0),
            }
        );

        let config =
            parse_config("pane-gaps = false\npane-margin = 2\npane-corner-radius = 3\n").config;
        cx.update(|cx| cx.set_global(config));
        assert_eq!(cx.update(|cx| pane_margin(cx)), px(0.0));
        assert_eq!(
            cx.update(|cx| pane_content_radii(cx, WindowCorners::NONE)),
            Corners {
                top_left: px(0.0),
                top_right: px(0.0),
                bottom_right: px(0.0),
                bottom_left: px(0.0),
            }
        );
        assert_eq!(cx.update(|cx| pane_border_width(cx)), px(0.0));
    }

    #[gpui::test]
    fn flush_panes_inherit_the_derived_frame_curve_at_exposed_corners(
        cx: &mut gpui::TestAppContext,
    ) {
        let config = parse_config("pane-corner-radius = 9\npane-margin = 0\n").config;
        cx.update(|cx| cx.set_global(config));

        assert_eq!(
            cx.update(|cx| pane_content_radii(
                cx,
                WindowCorners::from_tiling(gpui::Tiling::default()),
            )),
            Corners {
                top_left: px(12.5),
                top_right: px(12.5),
                bottom_right: px(12.5),
                bottom_left: px(12.5),
            }
        );
    }

    #[gpui::test]
    fn missing_watched_file_restores_built_in_defaults(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(parse_config("pane-corner-radius = 9\npane-margin = 6\n").config);
            install_config(None, None, cx);
        });

        assert_eq!(cx.update(|cx| resolved_config(cx)), AppConfig::default());
    }

    #[test]
    fn unix_candidates_follow_xdg_then_home_config() {
        let xdg = absolute_test_root("unix-xdg");
        let home = absolute_test_root("unix-home");
        let candidates = config_candidates_for(
            ConfigPlatform::Unix,
            ConfigEnvironment {
                xdg_config_home: Some(&xdg),
                home: Some(&home),
                ..ConfigEnvironment::default()
            },
        );

        assert_eq!(
            candidates,
            vec![
                expected_config_path(&xdg),
                expected_config_path(&home.join(".config")),
            ]
        );
    }

    #[test]
    fn fresh_write_path_prefers_xdg_then_home_config() {
        let xdg = absolute_test_root("write-xdg");
        let home = absolute_test_root("write-home");

        assert_eq!(
            preferred_config_creation_path(Some(&xdg), Some(&home)),
            Some(expected_config_path(&xdg))
        );
        assert_eq!(
            preferred_config_creation_path(Some(Path::new("relative")), Some(&home)),
            Some(expected_config_path(&home.join(".config")))
        );
        assert_eq!(preferred_config_creation_path(None, None), None);
    }

    #[test]
    fn macos_candidates_include_xdg_home_and_application_support() {
        let xdg = absolute_test_root("macos-xdg");
        let home = absolute_test_root("macos-home");
        let candidates = config_candidates_for(
            ConfigPlatform::Macos,
            ConfigEnvironment {
                xdg_config_home: Some(&xdg),
                home: Some(&home),
                ..ConfigEnvironment::default()
            },
        );

        assert_eq!(
            candidates,
            vec![
                expected_config_path(&xdg),
                expected_config_path(&home.join(".config")),
                expected_config_path(&home.join("Library/Application Support")),
            ]
        );
    }

    #[test]
    fn windows_candidates_include_roaming_local_and_config_fallbacks() {
        let xdg = absolute_test_root("windows-xdg");
        let home = absolute_test_root("windows-home");
        let appdata = absolute_test_root("windows-roaming");
        let local_appdata = absolute_test_root("windows-local");
        let user_profile = absolute_test_root("windows-profile");
        let candidates = config_candidates_for(
            ConfigPlatform::Windows,
            ConfigEnvironment {
                xdg_config_home: Some(&xdg),
                home: Some(&home),
                appdata: Some(&appdata),
                local_appdata: Some(&local_appdata),
                user_profile: Some(&user_profile),
            },
        );

        assert_eq!(
            candidates,
            vec![
                expected_config_path(&xdg),
                expected_config_path(&appdata),
                expected_config_path(&local_appdata),
                expected_config_path(&user_profile.join(".config")),
                expected_config_path(&home.join(".config")),
            ]
        );
    }

    #[test]
    fn candidates_ignore_relative_roots_and_remove_duplicates() {
        let appdata = absolute_test_root("duplicates");
        let candidates = config_candidates_for(
            ConfigPlatform::Windows,
            ConfigEnvironment {
                xdg_config_home: Some(Path::new("relative")),
                appdata: Some(&appdata),
                local_appdata: Some(&appdata),
                ..ConfigEnvironment::default()
            },
        );

        assert_eq!(candidates, vec![expected_config_path(&appdata)]);
    }

    #[test]
    fn discovery_selects_the_first_existing_candidate() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let missing = directory.path().join("missing/config");
        let existing = directory.path().join("existing/config");
        fs::create_dir_all(existing.parent().expect("config parent"))
            .expect("create config directory");
        fs::write(&existing, "pane-corner-radius = 12\n").expect("write config");

        assert_eq!(
            discover_config_path(&[missing, existing.clone()]),
            Some(existing)
        );
    }

    #[test]
    fn file_stamp_tracks_candidate_appearance_precedence_and_deletion() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let first = directory.path().join("first/config");
        let second = directory.path().join("second/config");
        let candidates = [first.clone(), second.clone()];

        assert_eq!(
            ConfigFileStamp::detect(&candidates),
            ConfigFileStamp::default()
        );

        fs::create_dir_all(second.parent().expect("second config parent"))
            .expect("create second config directory");
        fs::write(&second, "pane-corner-radius = 12\n").expect("write second config");
        assert_eq!(
            ConfigFileStamp::detect(&candidates).path,
            Some(second.clone())
        );

        fs::create_dir_all(first.parent().expect("first config parent"))
            .expect("create first config directory");
        fs::write(&first, "pane-corner-radius = 24\n").expect("write first config");
        assert_eq!(
            ConfigFileStamp::detect(&candidates).path,
            Some(first.clone())
        );

        fs::remove_file(&first).expect("remove first config");
        assert_eq!(
            ConfigFileStamp::detect(&candidates).path,
            Some(second.clone())
        );
        fs::remove_file(&second).expect("remove second config");
        assert_eq!(
            ConfigFileStamp::detect(&candidates),
            ConfigFileStamp::default()
        );
    }

    #[test]
    fn file_stamp_changes_when_configuration_length_changes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, "pane-corner-radius = 1\n").expect("write initial config");
        let initial = ConfigFileStamp::detect(std::slice::from_ref(&path));

        fs::write(&path, "pane-corner-radius = 120\n").expect("rewrite config");
        let changed = ConfigFileStamp::detect(std::slice::from_ref(&path));

        assert_ne!(initial, changed);
    }

    #[test]
    fn parse_boolean_rejects_tmux_on_yes_one() {
        assert_eq!(parse_boolean("true"), Ok(true));
        assert_eq!(parse_boolean("false"), Ok(false));
        for value in ["on", "yes", "1", "off", "no", "0"] {
            assert_eq!(
                parse_boolean(value),
                Err("expected `true` or `false`".to_owned()),
                "{value}"
            );
        }
    }

    #[test]
    fn experimental_agent_pane_on_forwards_to_the_daemon_and_leaves_the_gui_off() {
        let parsed = parse_config("experimental-agent-pane = on\n");
        assert_eq!(
            parsed.daemon_entries,
            [("experimental-agent-pane".to_owned(), "on".to_owned())]
        );
        assert!(!parsed.config.experimental_agent_pane.value);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("true` or `false"))
        );
    }

    #[test]
    fn config_key_surface_is_twenty_five_named_keys_plus_six_chrome_colors() {
        let named = [
            "use-system-titlebar",
            "window-corner-radius",
            "window-background-blur",
            "tray",
            "show-fps",
            "quit-daemon-on-exit",
            "auto-restart-stale-daemon",
            "experimental-agent-pane",
            "experimental-editor-pane",
            "pane-gaps",
            "pane-corner-radius",
            "pane-margin",
            "pane-border-width",
            "widget-corner-radius",
            "editor-font-size",
            "editor-line-numbers",
            "editor-relative-line-numbers",
            "editor-soft-wrap",
            "editor-vim-mode",
            "browser-element-selector-hotkey",
            "browser-search-provider",
            "browser-egress",
            "theme-mode",
            "app-icon",
            "chrome-preset",
        ];
        assert_eq!(named.len(), 25);
        assert_eq!(ChromeColor::ALL.len(), 6);
        for key in named {
            assert!(ConfigKey::from_str(key).is_some(), "{key}");
        }
        for color in ChromeColor::ALL {
            assert!(
                ConfigKey::from_str(color.as_str()).is_some(),
                "{}",
                color.as_str()
            );
        }
    }

    #[test]
    fn file_loader_rejects_oversized_configuration() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, vec![b' '; MAX_CONFIG_BYTES + 1]).expect("write test configuration");

        let error = load_config(&path).expect_err("oversized configuration should fail");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
