#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use async_channel::Sender;

/// What a tray interaction asks of the app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrayEvent {
    Toggle,
    Quit,
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

/// Puts the icon up. Call from the main thread: `AppKit` requires it for a
/// status item, and the handle's drop lands there too.
#[cfg(target_os = "macos")]
pub(crate) fn spawn(sender: Sender<TrayEvent>) -> Option<Tray> {
    macos::spawn(sender).map(|backend| Tray { _backend: backend })
}

#[cfg(target_os = "linux")]
pub(crate) fn spawn(sender: Sender<TrayEvent>) -> Option<Tray> {
    linux::spawn(sender).map(|backend| Tray { _backend: backend })
}

#[cfg(target_os = "windows")]
pub(crate) fn spawn(sender: Sender<TrayEvent>) -> Option<Tray> {
    windows::spawn(sender).map(|backend| Tray { _backend: backend })
}

/// No tray on this platform.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn spawn(_sender: Sender<TrayEvent>) -> Option<Tray> {
    None
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

#[cfg(test)]
mod tests {
    use super::{ToggleAction, toggle_action};

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
