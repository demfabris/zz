#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use async_channel::Sender;

pub(crate) use desktop::{
    DesktopTray, hide, hide_or_quit, init_desktop, quit_requested, set_active, show,
};

/// What a tray interaction asks of the app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrayEvent {
    Toggle,
    Quit,
    Available(bool),
    NewSession,
    SwitchSession(String),
    FocusPane(zz_protocol::PaneId),
    OpenSettings,
    OpenLogs,
    RestartDaemon,
}

/// A live tray icon. Dropping it removes the icon.
pub(crate) struct Tray {
    #[cfg(target_os = "linux")]
    _backend: linux::Service,
    #[cfg(target_os = "macos")]
    _backend: macos::StatusItem,
    #[cfg(target_os = "windows")]
    _backend: windows::NotifyIcon,
}

fn spawn(sender: Sender<TrayEvent>, source: facts::Source) -> Option<Tray> {
    #[cfg(target_os = "macos")]
    let backend = macos::spawn(sender, source)?;
    #[cfg(target_os = "linux")]
    let backend = linux::spawn(sender, source)?;
    #[cfg(target_os = "windows")]
    let backend = windows::spawn(sender, source)?;
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return None;
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        Some(Tray { _backend: backend })
    }
}

impl Tray {
    fn set_attention(&self, count: usize) {
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
        self._backend.set_attention(count);
    }
}

/// What clicking the tray icon should do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToggleAction {
    Hide,
    Raise,
    Show,
}

/// Maps window visibility and activation to what a tray click should do.
pub(crate) const fn toggle_action(visible: bool, active: bool) -> ToggleAction {
    match (visible, active) {
        (true, true) => ToggleAction::Hide,
        (true, false) => ToggleAction::Raise,
        (false, _) => ToggleAction::Show,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuitAction {
    HideToTray,
    Quit,
}

#[allow(clippy::fn_params_excessive_bools)]
#[must_use]
pub(crate) const fn quit_action(
    tray_enabled: bool,
    available: bool,
    quit_daemon_on_exit: bool,
    has_sessions: bool,
) -> QuitAction {
    if tray_enabled && available && !quit_daemon_on_exit && has_sessions {
        QuitAction::HideToTray
    } else {
        QuitAction::Quit
    }
}

#[cfg(test)]
mod tests {
    use super::{QuitAction, ToggleAction, quit_action, toggle_action};

    #[test]
    fn quit_hides_only_with_an_available_tray_and_preserved_sessions() {
        assert_eq!(quit_action(true, true, false, true), QuitAction::HideToTray);
        assert_eq!(quit_action(false, true, false, true), QuitAction::Quit);
        assert_eq!(quit_action(true, false, false, true), QuitAction::Quit);
        assert_eq!(quit_action(true, true, true, true), QuitAction::Quit);
        assert_eq!(quit_action(true, true, false, false), QuitAction::Quit);
    }

    #[test]
    fn the_icon_dismisses_a_window_that_already_has_focus() {
        assert_eq!(toggle_action(true, true), ToggleAction::Hide);
    }

    #[test]
    fn a_buried_window_is_summoned_rather_than_dismissed() {
        assert_eq!(toggle_action(true, false), ToggleAction::Raise);
    }

    #[test]
    fn a_hidden_window_comes_back_however_focus_reads() {
        assert_eq!(toggle_action(false, false), ToggleAction::Show);
        assert_eq!(toggle_action(false, true), ToggleAction::Show);
    }
}
mod desktop;
mod facts;
