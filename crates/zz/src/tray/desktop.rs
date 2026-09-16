use async_channel::Sender;
use gpui::{AnyWindowHandle, App, Entity, Global, Task, WeakEntity};
use zz_protocol::CommandInvocation;

use crate::{
    config::{self, AppConfig},
    mux::{
        client::MuxClient,
        hosts::HostId,
        nav::{TreeTarget, activate_nav, activation_for_target},
    },
    workspace::{AppView, sidebar::WorkspaceSidebar},
};

use super::{
    QuitAction, Tray, TrayEvent,
    facts::{Source, TrayFacts, facts_from, menu},
    quit_action,
};

#[derive(Default)]
pub(crate) struct DesktopTray {
    tray: Option<Tray>,
    source: Source,
    facts: Option<TrayFacts>,
    active: bool,
    available: bool,
    pub(crate) window: Option<AnyWindowHandle>,
    mux: Option<WeakEntity<MuxClient>>,
    events: Option<Sender<TrayEvent>>,
    pub(crate) sidebar: Option<WeakEntity<WorkspaceSidebar>>,
    pub(crate) stopping_daemon: bool,
    _task: Option<Task<()>>,
}

impl Global for DesktopTray {}

impl DesktopTray {
    pub(crate) fn available(&self) -> bool {
        self.available
    }

    fn publish(&self) {
        self.source.set(menu(self.facts.as_ref(), self.active));
        if let Some(backend) = &self.tray {
            backend.set_attention(self.facts.as_ref().map_or(0, |facts| facts.attention.len()));
        }
    }
}

pub(crate) fn init_desktop(mux: &Entity<MuxClient>, window: AnyWindowHandle, cx: &mut App) {
    let (events, receiver) = async_channel::unbounded();
    let event_mux = mux.clone();
    let task = cx.spawn(async move |cx| {
        while let Ok(event) = receiver.recv().await {
            cx.update(|cx| match event {
                TrayEvent::Toggle => crate::toggle_from_tray(window, cx),
                TrayEvent::Quit => quit_and_stop_sessions(&event_mux, cx),
                TrayEvent::Available(available) => {
                    let tray = cx.global_mut::<DesktopTray>();
                    let was_available = tray.available;
                    tray.available = available && tray.tray.is_some();
                    if was_available && !tray.available {
                        show_if_hidden(window, cx);
                    }
                }
                event => handle_intent(event, &event_mux, window, cx),
            });
        }
    });
    let tray = cx.global_mut::<DesktopTray>();
    tray.window = Some(window);
    tray.mux = Some(mux.downgrade());
    tray.events = Some(events);
    tray._task = Some(task);
    tray.publish();
    sync_enabled(cx);
    refresh(mux, cx);
    cx.observe(mux, |mux, cx| refresh(&mux, cx)).detach();
    cx.observe_global::<AppConfig>(sync_enabled).detach();
}

fn sync_enabled(cx: &mut App) {
    let enabled = config::tray_enabled(cx);
    let tray = cx.global_mut::<DesktopTray>();
    if enabled == tray.tray.is_some() {
        return;
    }
    if enabled {
        if let Some(sender) = &tray.events {
            tray.tray = super::spawn(sender.clone(), tray.source.clone());
            tray.publish();
        }
    } else {
        tray.tray.take();
        tray.available = false;
        if let Some(window) = tray.window {
            show_if_hidden(window, cx);
        }
    }
}

fn refresh(mux: &Entity<MuxClient>, cx: &mut App) {
    let mux = mux.read(cx);
    let snapshot = mux
        .fleet_hosts()
        .find_map(|(host, _, _, snapshot)| (host == HostId::LOCAL).then_some(snapshot).flatten());
    let facts = facts_from(
        snapshot,
        |pane| mux.agent_attention_status(pane),
        mux.stale_daemon().is_some(),
    );
    let tray = cx.global_mut::<DesktopTray>();
    if tray.facts != facts {
        tray.facts = facts;
        tray.publish();
    }
}

pub(crate) fn set_active(active: bool, cx: &mut App) {
    let tray = cx.global_mut::<DesktopTray>();
    if tray.active != active {
        tray.active = active;
        tray.publish();
    }
}

pub(crate) fn hide(window: AnyWindowHandle, cx: &mut App) {
    set_active(false, cx);
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        cx.hide();
        set_activation_policy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
    }
    #[cfg(not(target_os = "macos"))]
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, _| window.set_window_visible(false));
    });
}

pub(crate) fn show(window: AnyWindowHandle, cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        set_activation_policy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
        cx.activate(true);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window.update(cx, |_, window, _| {
        window.set_window_visible(true);
        window.activate_window();
    });
}

#[cfg(target_os = "macos")]
fn set_activation_policy(policy: objc2_app_kit::NSApplicationActivationPolicy) {
    let Some(main_thread) = objc2::MainThreadMarker::new() else {
        log::warn!("could not change macOS activation policy outside the main thread");
        return;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(main_thread);
    if !app.setActivationPolicy(policy) {
        log::warn!("macOS rejected the activation policy");
    }
}

pub(crate) fn has_sessions(mux: &Entity<MuxClient>, cx: &App) -> bool {
    mux.read(cx).fleet_hosts().any(|(host, _, _, snapshot)| {
        host == HostId::LOCAL && snapshot.is_some_and(|snapshot| !snapshot.sessions.is_empty())
    })
}

pub(crate) fn hide_or_quit(mux: &Entity<MuxClient>, window: AnyWindowHandle, cx: &mut App) -> bool {
    if quit_action(
        config::tray_enabled(cx),
        cx.global::<DesktopTray>().available(),
        config::quit_daemon_on_exit(cx),
        has_sessions(mux, cx),
    ) == QuitAction::HideToTray
    {
        hide(window, cx);
        true
    } else {
        false
    }
}

pub(crate) fn quit_requested(cx: &mut App) {
    let tray = cx.global::<DesktopTray>();
    if let Some(window) = tray.window
        && let Some(mux) = tray.mux.as_ref().and_then(WeakEntity::upgrade)
        && hide_or_quit(&mux, window, cx)
    {
        return;
    }
    cx.quit();
}

fn quit_and_stop_sessions(mux: &Entity<MuxClient>, cx: &mut App) {
    cx.global_mut::<DesktopTray>().stopping_daemon = true;
    mux.read(cx).execute_on_host(
        HostId::LOCAL,
        CommandInvocation::new("kill-server", [] as [&str; 0]),
    );
    cx.quit();
}

fn handle_intent(event: TrayEvent, mux: &Entity<MuxClient>, handle: AnyWindowHandle, cx: &mut App) {
    show(handle, cx);
    let _ = handle.update(cx, |_, window, cx| {
        let sidebar = cx
            .global::<DesktopTray>()
            .sidebar
            .as_ref()
            .and_then(WeakEntity::upgrade);
        match event {
            TrayEvent::NewSession => {
                if let Some(sidebar) = sidebar {
                    sidebar.update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
                }
                mux.update(cx, |mux, cx| {
                    if mux.attach_to_host_default(HostId::LOCAL, cx) {
                        mux.new_session(HostId::LOCAL);
                    }
                });
            }
            TrayEvent::SwitchSession(name) => {
                let session = mux
                    .read(cx)
                    .fleet_hosts()
                    .find_map(|(host, _, _, snapshot)| {
                        (host == HostId::LOCAL)
                            .then_some(snapshot)
                            .flatten()?
                            .sessions
                            .iter()
                            .find(|session| session.name == name)
                            .map(|session| session.id)
                    });
                if let Some(session) = session {
                    if let Some(sidebar) = sidebar {
                        sidebar.update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
                    }
                    mux.update(cx, |mux, cx| {
                        mux.attach_to_host(HostId::LOCAL, session, cx);
                    });
                }
            }
            TrayEvent::FocusPane(pane) => {
                let client = mux.read(cx);
                let owner = client.fleet_hosts().find_map(|(host, _, _, snapshot)| {
                    if host != HostId::LOCAL {
                        return None;
                    }
                    snapshot?.sessions.iter().find_map(|session| {
                        session
                            .windows
                            .iter()
                            .find(|window| window.panes.contains_key(&pane))
                            .map(|window| (session.id, window.id))
                    })
                });
                let activation = owner.and_then(|(session, window)| {
                    activation_for_target(
                        HostId::LOCAL,
                        TreeTarget::Pane(pane),
                        Some(session),
                        Some(window),
                        client.attached_host(),
                        client.attached_session(),
                        true,
                    )
                });
                if let Some(activation) = activation {
                    if let Some(sidebar) = sidebar {
                        sidebar.update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
                    }
                    activate_nav(mux, activation, cx);
                }
            }
            TrayEvent::OpenSettings => {
                if let Some(sidebar) = sidebar {
                    sidebar.update(cx, |sidebar, cx| sidebar.open_settings(window, cx));
                }
            }
            TrayEvent::OpenLogs => crate::diagnostics::open_logs(cx),
            TrayEvent::RestartDaemon => AppView::prompt_daemon_update(mux, window, cx),
            _ => {}
        }
    });
}

fn show_if_hidden(window: AnyWindowHandle, cx: &mut App) {
    #[cfg(target_os = "macos")]
    let hidden = objc2::MainThreadMarker::new()
        .is_some_and(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).isHidden());
    #[cfg(not(target_os = "macos"))]
    let hidden = window
        .update(cx, |_, window, _| !window.is_window_visible())
        .unwrap_or(false);
    if hidden {
        show(window, cx);
    }
}
