//! Platform composition selected by each app entry point.

use gpui::{App, Global};

pub use zz_ui::settings::SettingsSection;

/// The app surface and platform capabilities available to a view.
#[derive(Clone, Debug)]
pub struct AppProfile {
    /// Settings pages, in navigation order.
    pub settings_sections: Vec<SettingsSection>,
    /// Advanced page: show the tray-icon row.
    pub has_tray: bool,
    /// Appearance page: show the window-background-blur row.
    pub has_window_blur: bool,
    /// Advanced page: show the quit-daemon-on-exit row.
    pub has_daemon_lifecycle: bool,
    /// Terminal and Multiplexer pages: show the Ghostty and tmux import buttons.
    pub has_config_import: bool,
    /// Whether to synthesize the implicit `local` host row.
    pub local_host: LocalHostPolicy,
    /// The window cannot be resized or dragged. Hide its resize and drag chrome.
    pub fixed_window: bool,
}

impl AppProfile {
    /// The full desktop application surface.
    #[must_use]
    pub fn desktop() -> Self {
        let settings_sections = SettingsSection::ALL
            .into_iter()
            .filter(|section| *section != SettingsSection::Editor || cfg!(feature = "editor-pane"))
            .collect();
        Self {
            settings_sections,
            has_tray: true,
            has_window_blur: true,
            has_daemon_lifecycle: true,
            has_config_import: true,
            local_host: LocalHostPolicy::Always,
            fixed_window: false,
        }
    }
}

impl Global for AppProfile {}

#[cfg(test)]
static TEST_PROFILE: std::sync::LazyLock<AppProfile> =
    std::sync::LazyLock::new(AppProfile::desktop);

/// Policy for the implicit host backed by a local daemon socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalHostPolicy {
    /// Desktop: always present, backed by the auto-started daemon.
    Always,
}

impl LocalHostPolicy {
    /// Whether a `local` host exists under this policy.
    pub fn synthesize_local(self) -> bool {
        match self {
            Self::Always => true,
        }
    }
}

#[must_use]
pub(crate) fn profile(cx: &App) -> &AppProfile {
    #[cfg(test)]
    {
        cx.try_global::<AppProfile>().unwrap_or(&TEST_PROFILE)
    }
    #[cfg(not(test))]
    {
        cx.global::<AppProfile>()
    }
}
