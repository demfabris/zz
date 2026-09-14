#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use async_channel::Sender;

pub(crate) use desktop::{DesktopTray, focused, inactive, init_desktop};
pub(crate) use host::{run_if_requested, start_daemon_helper};

/// What a tray interaction asks of the app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrayEvent {
    Toggle,
    Quit,
    Available(bool),
    NewSession,
    SwitchSession(String),
    FocusPane(String),
    OpenSettings,
    OpenLogs,
    RestartDaemon,
    Attention(usize),
}

/// A live tray icon. Dropping it removes the icon.
pub(crate) struct Tray {
    _stop: std::sync::mpsc::Sender<()>,
    #[cfg(target_os = "linux")]
    _backend: linux::Service,
    #[cfg(target_os = "macos")]
    _backend: macos::StatusItem,
    #[cfg(target_os = "windows")]
    _backend: windows::NotifyIcon,
}

fn spawn(sender: Sender<TrayEvent>, source: facts::Source) -> Option<Tray> {
    #[cfg(target_os = "macos")]
    let backend = macos::spawn(sender.clone(), source.clone())?;
    #[cfg(target_os = "linux")]
    let backend = linux::spawn(sender.clone(), source.clone())?;
    #[cfg(target_os = "windows")]
    let backend = windows::spawn(sender.clone(), source.clone())?;
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return None;
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let (stop, stopped) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("zz-tray-attention".into())
            .spawn(move || {
                loop {
                    let count = facts::attention_count(
                        &source.socket,
                        source.server_id,
                        std::time::Duration::from_millis(300),
                    );
                    let delay = if count.is_ok() { 5 } else { 15 };
                    if let Ok(count) = count
                        && sender.try_send(TrayEvent::Attention(count)).is_err()
                    {
                        break;
                    }
                    if !matches!(
                        stopped.recv_timeout(std::time::Duration::from_secs(delay)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
            });
        Some(Tray {
            _backend: backend,
            _stop: stop,
        })
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
mod desktop;
mod facts;
mod host;
mod ipc;
mod settings;
