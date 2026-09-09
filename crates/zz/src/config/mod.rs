use std::{
    io::{self, ErrorKind},
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};

use gpui::{App, Corners, Global, Hsla, Pixels, WindowBackgroundAppearance, WindowDecorations, px};
use zz_browser::SearchProvider;
use zz_client::StatusBarSettings;
use zz_daemon::{Endpoint, InteractiveClient};
pub(crate) use zz_daemon::{HostEntry, RejectedHost, configured_fleet_hosts, validate_fleet_host};
use zz_protocol::{CommandInvocation, ConfigOverrideEntry, PROTOCOL_VERSION};

use crate::{
    app_icon::AppIconSetting,
    mux::hosts::{HostId, HostRegistry},
    theme::{ChromeColor, ChromePresetId, ThemeModeSetting},
    window::corners::WindowCorners,
};

pub(crate) mod import;
#[cfg(not(target_os = "ios"))]
pub(crate) mod import_prompt;
mod mux_bindings;
pub(crate) mod settings;

pub use zz_config::*;
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const WINDOW_FRAME_BORDER_SIZE: Pixels = px(1.0);
#[cfg(target_os = "linux")]
const UNBLURRED_WINDOW_BACKGROUND: WindowBackgroundAppearance =
    WindowBackgroundAppearance::Transparent;
#[cfg(not(target_os = "linux"))]
const UNBLURRED_WINDOW_BACKGROUND: WindowBackgroundAppearance = WindowBackgroundAppearance::Opaque;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppConfig(pub zz_config::AppConfig);
impl Global for AppConfig {}
impl From<zz_config::AppConfig> for AppConfig {
    fn from(value: zz_config::AppConfig) -> Self {
        Self(value)
    }
}
impl std::ops::Deref for AppConfig {
    type Target = zz_config::AppConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for AppConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrowserConfig(pub zz_config::BrowserConfig);
impl Global for BrowserConfig {}
impl std::ops::Deref for BrowserConfig {
    type Target = zz_config::BrowserConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for BrowserConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiFontConfig(pub zz_config::UiFontConfig);
impl Global for UiFontConfig {}
impl std::ops::Deref for UiFontConfig {
    type Target = zz_config::UiFontConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for UiFontConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentConfig(pub zz_config::AgentConfig);
impl Global for AgentConfig {}
impl std::ops::Deref for AgentConfig {
    type Target = zz_config::AgentConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for AgentConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn chrome_color([r, g, b, a]: [f32; 4]) -> Hsla {
    gpui::Rgba { r, g, b, a }.into()
}
impl AppConfig {
    pub fn chrome(&self, color: ChromeColor) -> ConfigValue<Option<Hsla>> {
        let value = self.0.chrome(color);
        ConfigValue {
            value: value.value.map(chrome_color),
            provenance: value.provenance,
        }
    }
}
fn load_config(path: &Path) -> io::Result<ParsedConfig> {
    zz_config::load_config(path, gpui::Font::default().family.as_ref())
}
#[cfg(test)]
fn parse_config(source: &str) -> ParsedConfig {
    zz_config::parse_config(source, gpui::Font::default().family.as_ref())
}
pub(crate) fn set_ui_font_family(family: Option<&str>) -> io::Result<()> {
    zz_config::set_ui_font_family(family, gpui::Font::default().family.as_ref())
}
#[cfg(test)]
fn write_ui_font_family_at(path: &Path, family: Option<&str>) -> io::Result<bool> {
    zz_config::write_ui_font_family_at(path, family, gpui::Font::default().family.as_ref())
}
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
        cx.set_global(UiFontConfig::default());
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
        "application configuration path={} pane_gaps={} pane_inactive_opacity={} pane_corner_radius={} pane_margin={} pane_border_width={} widget_corner_radius={} window_corner_radius={} editor_font_size={} editor_line_numbers={} editor_relative_line_numbers={} editor_soft_wrap={} editor_vim_mode={} browser_element_selector_hotkey={} browser_search_provider={} browser_egress={} use_system_titlebar={} window_background_blur={} animations={} tray={} show_fps={} quit_daemon_on_exit={} auto_restart_stale_daemon={} check_for_updates={} agent_working_directory={:?} daemon_override_entries={}",
        path.display(),
        parsed.config.pane_gaps.value,
        parsed.config.pane_inactive_opacity.value,
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
        parsed.config.check_for_updates.value,
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
    cx.set_global(AppConfig(parsed.config));
    cx.set_global(BrowserConfig(parsed.browser));
    cx.set_global(UiFontConfig(parsed.ui_font));
    apply_animations(cx);
    apply_window_background_appearance(cx);
    apply_window_decorations(cx);
    crate::app_icon::apply(cx);
    cx.set_global(AgentConfig(parsed.agent));
    cx.set_global(DaemonConfigOverrides {
        entries: parsed.daemon_entries,
    });
    send_current_config_overrides(cx);
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

/// Whether the GUI looks up the newest release once a day and offers it.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn check_for_updates(cx: &App) -> bool {
    resolved_config(cx).check_for_updates.value
}

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

pub(crate) fn pane_background_opacity(cx: &App) -> f32 {
    resolved_config(cx).pane_background_opacity.value
}

pub(crate) fn pane_inactive_opacity(cx: &App) -> f32 {
    resolved_config(cx).pane_inactive_opacity.value
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

pub(crate) fn shadow_strength(cx: &App) -> f32 {
    resolved_config(cx).shadow_strength.value
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
    resolved_config(cx)
        .chrome_colors
        .map(|value| value.value.map(chrome_color))
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

pub(crate) fn status_bar_settings(cx: &App) -> StatusBarSettings {
    let config = resolved_config(cx);
    StatusBarSettings {
        show_session: config.status_show_session.value,
        badges: config.status_badges.value,
        alignment: config.status_alignment.value,
        show_agents: config.status_agents.value,
        show_host: config.status_host.value,
        show_update: config.status_update.value,
        clock: config.status_clock.value,
    }
}

pub(crate) fn ui_font_family(cx: &App) -> ConfigValue<Option<String>> {
    cx.try_global::<UiFontConfig>()
        .cloned()
        .unwrap_or_default()
        .0
        .family
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

#[cfg(test)]
mod tests {
    use crate::keymap::ChromeOverride;
    use std::{env, fs, path::PathBuf};
    use zz_client::{StatusBarAlignment, StatusBarClock};
    use zz_protocol::MuxOptionKey;
    use zz_terminal::{
        AppearanceColor, AppearanceConfigKey, CellHeightAdjustment, Color, CursorBlinkPolicy,
        CursorStyle, TerminalAppearance,
    };

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
             pane-inactive-opacity = 0.85\n\
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
             check-for-updates = false\n\
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
        assert_f32_eq(parsed.config.pane_inactive_opacity.value, 0.85);
        assert_f32_eq(parsed.config.pane_corner_radius.value, 9.0);
        assert_f32_eq(parsed.config.pane_margin.value, 6.0);
        assert_f32_eq(parsed.config.pane_border_width.value, 2.5);
        assert_eq!(
            parsed.config.pane_gaps.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_inactive_opacity.provenance,
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
        assert!(!parsed.config.check_for_updates.value);
        assert_eq!(
            parsed.config.check_for_updates.provenance,
            ConfigProvenance::Override
        );
        assert!(AppConfig::default().check_for_updates.value);
    }

    #[test]
    fn status_bar_settings_parse_with_typed_defaults() {
        let defaults = AppConfig::default();
        assert!(defaults.status_show_session.value);
        assert!(defaults.status_badges.value);
        assert_eq!(defaults.status_alignment.value, StatusBarAlignment::Left);
        assert!(defaults.status_agents.value);
        assert!(defaults.status_host.value);
        assert!(defaults.status_update.value);
        assert_eq!(defaults.status_clock.value, StatusBarClock::TwentyFourHour);

        let parsed = parse_config(
            "status-show-session = false\n\
             status-badges = false\n\
             status-align = center\n\
             status-agents = false\n\
             status-host = false\n\
             status-update = false\n\
             status-clock = time-date\n",
        );

        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert!(!parsed.config.status_show_session.value);
        assert!(!parsed.config.status_badges.value);
        assert_eq!(
            parsed.config.status_alignment.value,
            StatusBarAlignment::Center
        );
        assert!(!parsed.config.status_agents.value);
        assert!(!parsed.config.status_host.value);
        assert!(!parsed.config.status_update.value);
        assert_eq!(
            parsed.config.status_clock.value,
            StatusBarClock::TimeAndDate
        );
        for provenance in [
            parsed.config.status_show_session.provenance,
            parsed.config.status_badges.provenance,
            parsed.config.status_alignment.provenance,
            parsed.config.status_agents.provenance,
            parsed.config.status_host.provenance,
            parsed.config.status_update.provenance,
            parsed.config.status_clock.provenance,
        ] {
            assert_eq!(provenance, ConfigProvenance::Override);
        }

        let invalid = parse_config("status-align = right\nstatus-clock = seconds\n");
        assert_eq!(invalid.diagnostics.len(), 2);
        assert_eq!(
            invalid.config.status_alignment.value,
            StatusBarAlignment::Left
        );
        assert_eq!(
            invalid.config.status_clock.value,
            StatusBarClock::TwentyFourHour
        );
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
                .map(|value| zz_ui::to_hex(chrome_color(value))),
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
                (zz_ui::ThemeMode::Light, preset.colors(false)),
                (zz_ui::ThemeMode::Dark, preset.colors(true)),
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
            zz_config::AppConfig {
                pane_gaps: ConfigValue {
                    value: true,
                    provenance: ConfigProvenance::Override,
                },
                ..zz_config::AppConfig::default()
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
        assert!(parsed.diagnostics[3].message.contains("expected a boolean"));
        assert!(parsed.diagnostics[4].message.contains("expected a boolean"));
        assert!(parsed.diagnostics[5].message.contains("unsupported key"));
        assert_eq!(parsed.diagnostics[6].message, "expected `key = value`");
    }

    #[test]
    fn parser_accepts_a_tmux_spelled_switch_and_validates_numeric_values() {
        let parsed =
            parse_config("pane-gaps = yes\npane-border-width = 9\npane-inactive-opacity = 1.1\n");

        assert!(parsed.config.pane_gaps.value);
        assert_f32_eq(parsed.config.pane_border_width.value, 0.5);
        assert_f32_eq(parsed.config.pane_inactive_opacity.value, 0.7);
        assert_eq!(
            parsed.config.pane_gaps.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_border_width.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(
            parsed.config.pane_inactive_opacity.provenance,
            ConfigProvenance::Override
        );
        assert_eq!(parsed.diagnostics.len(), 2);
        assert!(parsed.diagnostics[0].message.contains("between 0 and 8"));
        assert!(parsed.diagnostics[1].message.contains("between 0 and 1"));
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
            (ConfigKey::PaneInactiveOpacity, "0.85"),
            (ConfigKey::PaneBorderWidth, "2.5"),
        ] {
            write_config_edit_at(&path, key.as_str(), Some(value))
                .expect("write pane chrome setting");
        }

        let source = fs::read_to_string(path).expect("read pane chrome settings");
        let parsed = parse_config(&source);
        assert!(parsed.diagnostics.is_empty());
        assert!(parsed.config.pane_gaps.value);
        assert_f32_eq(parsed.config.pane_inactive_opacity.value, 0.85);
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
    fn status_bar_enums_round_trip_through_the_config_writer() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);

        for alignment in [StatusBarAlignment::Left, StatusBarAlignment::Center] {
            write_config_edit_at(
                &path,
                ConfigKey::StatusAlign.as_str(),
                Some(status_bar_alignment_value(alignment)),
            )
            .expect("write status bar alignment");
            let source = fs::read_to_string(&path).expect("read status bar alignment");
            let parsed = parse_config(&source);
            assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
            assert_eq!(parsed.config.status_alignment.value, alignment);
        }

        for clock in [
            StatusBarClock::TwentyFourHour,
            StatusBarClock::TwelveHour,
            StatusBarClock::TimeAndDate,
            StatusBarClock::Off,
        ] {
            write_config_edit_at(
                &path,
                ConfigKey::StatusClock.as_str(),
                Some(status_bar_clock_value(clock)),
            )
            .expect("write status bar clock");
            let source = fs::read_to_string(&path).expect("read status bar clock");
            let parsed = parse_config(&source);
            assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
            assert_eq!(parsed.config.status_clock.value, clock);
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
    fn ui_font_family_round_trips_names_and_preserves_other_settings() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        let source = "# keep header\nfont-family = Terminal Font\nui-font-family = Earlier\nui-font-family = Initial # keep comment\n";
        fs::write(&path, source).expect("write configuration");

        for family in [
            "Segoe UI",
            "Noto Sans CJK JP",
            "日本語",
            r#"A "Quoted" #Font\Family"#,
        ] {
            assert!(write_ui_font_family_at(&path, Some(family)).expect("write UI font"));
            let written = fs::read_to_string(&path).expect("read UI font");
            assert!(written.starts_with(
                "# keep header\nfont-family = Terminal Font\nui-font-family = Earlier\n"
            ));
            assert!(written.ends_with(" # keep comment\n"));
            let parsed = parse_config(&written);
            assert!(parsed.diagnostics.is_empty(), "{written}");
            assert_eq!(parsed.ui_font.family.value.as_deref(), Some(family));
            assert_eq!(parsed.ui_font.family.provenance, ConfigProvenance::Override);
            assert_eq!(
                parsed.daemon_entries,
                [("font-family".to_owned(), "Terminal Font".to_owned())]
            );
        }

        let before = fs::read_to_string(&path).expect("read before invalid writes");
        for family in ["", "  ", "Bad\nFont", "Bad\rFont", "Bad\0Font"] {
            assert!(write_ui_font_family_at(&path, Some(family)).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), before);
        }
        let invalid = parse_config("ui-font-family = Valid\nui-font-family = \"\"\n");
        assert_eq!(invalid.ui_font.family.value.as_deref(), Some("Valid"));
        assert_eq!(invalid.diagnostics.len(), 1);

        write_ui_font_family_at(&path, None).expect("select system default over earlier values");
        assert_eq!(load_config(&path).unwrap().ui_font.family.value, None);
        assert!(!write_ui_font_family_at(&path, None).expect("unchanged default"));
    }

    #[gpui::test]
    fn ui_font_family_reloads_with_the_theme_and_resets(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        cx.update(zz_ui::init);
        let mono = cx.update(|cx| zz_ui::Theme::global(cx).mono_font_family.clone());

        for family in [Some("Noto Sans"), Some("Arial"), None] {
            write_ui_font_family_at(&path, family).expect("write UI font");
            cx.update(|cx| {
                install_config(Some(&path), Some(load_config(&path)), cx);
                crate::theme::refresh_current_theme(cx);
                let expected =
                    family.map_or_else(|| gpui::Font::default().family, gpui::SharedString::from);
                assert_eq!(zz_ui::Theme::global(cx).font_family, expected);
                assert_eq!(zz_ui::Theme::global(cx).mono_font_family, mono);
                assert_eq!(ui_font_family(cx).value.as_deref(), family);
                crate::theme::sync_system_appearance(None, cx);
                assert_eq!(zz_ui::Theme::global(cx).font_family, expected);
            });
        }

        write_ui_font_family_at(&path, Some("Arial")).expect("write another override");
        write_config_edit_at(&path, ConfigKey::UiFontFamily.as_str(), None)
            .expect("reset override");
        cx.update(|cx| {
            install_config(Some(&path), Some(load_config(&path)), cx);
            crate::theme::refresh_current_theme(cx);
            assert_eq!(ui_font_family(cx).provenance, ConfigProvenance::Default);
            assert_eq!(
                zz_ui::Theme::global(cx).font_family,
                gpui::Font::default().family
            );
            let parsed = parse_config("ui-font-family = Arial\n");
            install_config(Some(&path), Some(Ok(parsed)), cx);
            install_config(None, None, cx);
            crate::theme::refresh_current_theme(cx);
            assert_eq!(ui_font_family(cx), UiFontConfig::default().family);
            assert_eq!(
                zz_ui::Theme::global(cx).font_family,
                gpui::Font::default().family
            );
        });
    }

    #[test]
    fn shadow_strength_defaults_to_full_and_validates_finite_factors() {
        let default = parse_config("").config.shadow_strength;
        assert_f32_eq(default.value, 1.0);
        assert_eq!(default.provenance, ConfigProvenance::Default);
        for (source, expected) in [("0", 0.0), ("0.35", 0.35), ("1", 1.0)] {
            let parsed = parse_config(&format!("shadow-strength = {source}\n"));
            assert!(parsed.diagnostics.is_empty());
            assert_f32_eq(parsed.config.shadow_strength.value, expected);
            assert_eq!(
                parsed.config.shadow_strength.provenance,
                ConfigProvenance::Override
            );
        }
        for value in ["-0.01", "1.01", "NaN", "inf", "nope"] {
            let parsed = parse_config(&format!("shadow-strength = {value}\n"));
            assert_eq!(parsed.diagnostics.len(), 1, "{value}");
            assert_f32_eq(parsed.config.shadow_strength.value, 1.0);
        }
    }

    #[gpui::test]
    fn shadow_strength_writes_reload_the_theme_and_reset_to_full(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        cx.update(zz_ui::init);

        for (value, expected, provenance) in [
            (Some("0.5"), 0.5, ConfigProvenance::Override),
            (Some("0"), 0.0, ConfigProvenance::Override),
            (None, 1.0, ConfigProvenance::Default),
        ] {
            write_config_edit_at(&path, ConfigKey::ShadowStrength.as_str(), value)
                .expect("write shadow strength");
            assert!(
                !write_config_edit_at(&path, ConfigKey::ShadowStrength.as_str(), value)
                    .expect("unchanged shadow strength is not rewritten")
            );
            cx.update(|cx| {
                install_config(Some(&path), Some(load_config(&path)), cx);
                crate::theme::refresh_current_theme(cx);
                assert_f32_eq(shadow_strength(cx), expected);
                assert_f32_eq(zz_ui::Theme::global(cx).shadow_strength, expected);
                assert_eq!(resolved_config(cx).shadow_strength.provenance, provenance);
            });
        }
    }

    #[gpui::test]
    fn pane_background_opacity_validates_reloads_and_resets_without_changing_shadows(
        cx: &mut gpui::TestAppContext,
    ) {
        for value in ["-0.01", "1.01", "NaN", "inf", "invalid"] {
            let parsed = parse_config(&format!("pane-background-opacity = {value}\n"));
            assert_eq!(parsed.diagnostics.len(), 1, "{value}");
            assert_f32_eq(parsed.config.pane_background_opacity.value, 0.5);
        }
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(CONFIG_FILE_NAME);
        write_config_edit_at(&path, ConfigKey::ShadowStrength.as_str(), Some("0.4"))
            .expect("write shadow strength");
        cx.update(zz_ui::init);
        let shadows = cx.update(|cx| {
            install_config(Some(&path), Some(load_config(&path)), cx);
            crate::theme::refresh_current_theme(cx);
            zz_ui::control_shadow(cx)
        });
        for (value, expected, provenance) in [
            (Some("0"), 0.0, ConfigProvenance::Override),
            (Some("0.25"), 0.25, ConfigProvenance::Override),
            (Some("1"), 1.0, ConfigProvenance::Override),
            (None, 0.5, ConfigProvenance::Default),
        ] {
            write_config_edit_at(&path, ConfigKey::PaneBackgroundOpacity.as_str(), value)
                .expect("write pane background opacity");
            cx.update(|cx| {
                install_config(Some(&path), Some(load_config(&path)), cx);
                crate::theme::refresh_current_theme(cx);
                assert_f32_eq(pane_background_opacity(cx), expected);
                assert_f32_eq(zz_ui::Theme::global(cx).pane_background_opacity, expected);
                assert_f32_eq(crate::theme::app_pane_background(cx).a, expected);
                assert_eq!(
                    resolved_config(cx).pane_background_opacity.provenance,
                    provenance
                );
                assert_eq!(zz_ui::control_shadow(cx), shadows);
            });
        }
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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));

        let resolved = cx.update(|cx| resolved_config(cx));
        assert_f32_eq(resolved.pane_corner_radius.value, 9.0);
        assert_f32_eq(resolved.pane_margin.value, 6.0);
    }

    #[gpui::test]
    fn status_bar_settings_project_from_the_live_app_config(cx: &mut gpui::TestAppContext) {
        let config = parse_config(
            "status-show-session = false\n\
             status-badges = false\n\
             status-align = center\n\
             status-agents = false\n\
             status-host = false\n\
             status-update = false\n\
             status-clock = off\n",
        )
        .config;
        cx.update(|cx| cx.set_global(AppConfig::from(config)));

        assert_eq!(
            cx.update(|cx| status_bar_settings(cx)),
            StatusBarSettings {
                show_session: false,
                badges: false,
                alignment: StatusBarAlignment::Center,
                show_agents: false,
                show_host: false,
                show_update: false,
                clock: StatusBarClock::Off,
            }
        );
    }

    #[gpui::test]
    fn install_config_publishes_and_clears_browser_config(cx: &mut gpui::TestAppContext) {
        let parsed = parse_config("browser-element-selector-hotkey = alt-shift-e\n");
        let expected = parsed.browser.clone();

        cx.update(|cx| {
            install_config(Some(Path::new("/tmp/zz/config")), Some(Ok(parsed)), cx);
        });
        assert_eq!(cx.update(|cx| browser_config(cx)).0, expected);

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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));

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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));

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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));

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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));
        assert!(cx.update(|cx| pane_gaps(cx)));
        assert_eq!(cx.update(|cx| pane_margin(cx)), px(6.0));
        assert_eq!(cx.update(|cx| pane_border_width(cx)), px(0.5));
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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));
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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));
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
        cx.update(|cx| cx.set_global(AppConfig::from(config)));

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
            cx.set_global(AppConfig(
                parse_config("pane-corner-radius = 9\npane-margin = 6\n").config,
            ));
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
    fn parse_boolean_accepts_the_tmux_flag_spellings() {
        for value in ["on", "yes", "1", "true"] {
            assert_eq!(parse_boolean(value), Ok(true), "{value}");
        }
        for value in ["off", "no", "0", "false"] {
            assert_eq!(parse_boolean(value), Ok(false), "{value}");
        }
        assert!(parse_boolean("sometimes").is_err());
    }

    #[test]
    fn experimental_agent_pane_on_forwards_to_the_daemon_and_enables_the_gui() {
        let parsed = parse_config("experimental-agent-pane = on\n");
        assert_eq!(
            parsed.daemon_entries,
            [("experimental-agent-pane".to_owned(), "on".to_owned())]
        );
        assert!(parsed.config.experimental_agent_pane.value);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn named_config_keys_and_chrome_colors_round_trip() {
        let named = [
            "use-system-titlebar",
            "window-corner-radius",
            "window-background-blur",
            "animations",
            "tray",
            "show-fps",
            "quit-daemon-on-exit",
            "auto-restart-stale-daemon",
            "check-for-updates",
            "status-show-session",
            "status-badges",
            "status-align",
            "status-agents",
            "status-host",
            "status-update",
            "status-clock",
            "experimental-agent-pane",
            "experimental-editor-pane",
            "pane-gaps",
            "pane-background-opacity",
            "pane-inactive-opacity",
            "pane-corner-radius",
            "pane-margin",
            "pane-border-width",
            "widget-corner-radius",
            "shadow-strength",
            "editor-font-size",
            "editor-line-numbers",
            "editor-relative-line-numbers",
            "editor-soft-wrap",
            "editor-vim-mode",
            "browser-element-selector-hotkey",
            "browser-search-provider",
            "browser-egress",
            "theme-mode",
            "ui-font-family",
            "app-icon",
            "chrome-preset",
        ];
        assert_eq!(ChromeColor::ALL.len(), 6);
        for key in named {
            assert_eq!(ConfigKey::parse(key).map(ConfigKey::as_str), Some(key));
        }
        for color in ChromeColor::ALL {
            assert!(
                ConfigKey::parse(color.as_str()).is_some(),
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
