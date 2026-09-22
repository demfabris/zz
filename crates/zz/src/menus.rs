#[cfg(not(target_os = "ios"))]
use std::{collections::BTreeSet, sync::Arc};

#[cfg(target_os = "macos")]
use gpui::SystemMenuType;
#[cfg(not(target_os = "ios"))]
use gpui::{App, Entity, Global, Menu, MenuItem, OsAction};

#[cfg(target_os = "macos")]
use crate::macos_app::{Hide, HideOthers, Minimize, Quit, ShowAll, Zoom};
#[cfg(not(target_os = "ios"))]
use crate::{
    browser::view::{GoBack, GoForward, Reload, ToggleDevTools},
    config::settings::OpenSettings,
    keymap::ChromeState,
    mux::client::MuxClient,
    ui_scale::{DecreaseUiZoom, IncreaseUiZoom, ResetUiZoom},
    workspace::ClosePane,
};

gpui::actions!(
    zz,
    [
        NewSession,
        NewWindow,
        OpenCommandPalette,
        ChooseWindow,
        SplitRight,
        SplitDown,
        NewBrowserPane,
        NewAgentPane,
        RenameSession,
        RenameWindow,
        KillWindow,
        KillSession,
        Detach,
        ZoomPane,
        CopyMode,
        ClearScrollback,
        Find,
        ToggleSidebar,
        CheckForUpdates,
        OpenLogs,
        ImportTmuxConfig,
        About,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        SelectAll,
    ]
);

#[cfg(not(target_os = "macos"))]
gpui::actions!(zz, [Quit]);

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Debug, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = zz, no_json)]
pub(crate) struct SwitchSession {
    pub(crate) name: String,
}

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Debug, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = zz, no_json)]
pub(crate) struct SelectPane {
    pub(crate) direction: String,
}

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Debug, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = zz, no_json)]
pub(crate) struct OpenUrl {
    pub(crate) url: String,
}

#[cfg(not(target_os = "ios"))]
pub(crate) const RELEASE_NOTES_URL: &str = "https://github.com/demfabris/zz/releases";

#[cfg(not(target_os = "ios"))]
#[derive(Default)]
struct MenuSessions(BTreeSet<String>);

#[cfg(not(target_os = "ios"))]
impl Global for MenuSessions {}

#[cfg(not(target_os = "ios"))]
pub(crate) fn install(cx: &mut App) {
    #[cfg(not(target_os = "macos"))]
    cx.on_action(|_: &Quit, cx| crate::tray::quit_requested(cx));
    cx.set_global(MenuSessions::default());
    rebuild(cx);
    cx.observe_global::<ChromeState>(|cx| cx.defer(rebuild))
        .detach();
    cx.observe_global::<MenuSessions>(rebuild).detach();
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn observe_mux(mux: &Entity<MuxClient>, cx: &mut App) {
    let mut snapshot = mux.read(cx).snapshot();
    update_sessions(&snapshot, cx);
    cx.observe(mux, move |mux, cx| {
        let next = mux.read(cx).snapshot();
        if !Arc::ptr_eq(&snapshot, &next) {
            snapshot = next;
            update_sessions(&snapshot, cx);
        }
    })
    .detach();
}

#[cfg(not(target_os = "ios"))]
fn update_sessions(snapshot: &zz_protocol::MuxSnapshot, cx: &mut App) {
    let names = snapshot
        .sessions
        .iter()
        .map(|session| session.name.clone())
        .collect::<BTreeSet<_>>();
    if cx
        .try_global::<MenuSessions>()
        .is_none_or(|sessions| sessions.0 != names)
    {
        cx.set_global(MenuSessions(names));
    }
}

#[cfg(not(target_os = "ios"))]
fn rebuild(cx: &mut App) {
    cx.set_menus(app_menus(cx));
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn app_menus(cx: &App) -> Vec<Menu> {
    let sessions = cx
        .try_global::<MenuSessions>()
        .map(|sessions| sessions.0.clone())
        .unwrap_or_default();
    menu_tree(&sessions)
}

#[cfg(not(target_os = "ios"))]
fn menu_tree(sessions: &BTreeSet<String>) -> Vec<Menu> {
    vec![
        Menu::new("zz").items([
            MenuItem::action("About zz", About),
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::action("Import tmux config…", ImportTmuxConfig),
            MenuItem::separator(),
            #[cfg(target_os = "macos")]
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            #[cfg(target_os = "macos")]
            MenuItem::separator(),
            #[cfg(target_os = "macos")]
            MenuItem::action("Hide zz", Hide),
            #[cfg(target_os = "macos")]
            MenuItem::action("Hide Others", HideOthers),
            #[cfg(target_os = "macos")]
            MenuItem::action("Show All", ShowAll),
            #[cfg(target_os = "macos")]
            MenuItem::separator(),
            MenuItem::action("Quit zz", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Session", NewSession),
            MenuItem::action("New Window", NewWindow),
            MenuItem::action("Split Right", SplitRight),
            MenuItem::action("Split Down", SplitDown),
            MenuItem::action("New Browser Pane", NewBrowserPane),
            MenuItem::action("New Agent Pane", NewAgentPane),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Switch Session").items(
                sessions.iter().map(|name| {
                    MenuItem::action(name.clone(), SwitchSession { name: name.clone() })
                }),
            )),
            MenuItem::action("Rename Session…", RenameSession),
            MenuItem::action("Rename Window…", RenameWindow),
            MenuItem::separator(),
            MenuItem::action("Close Pane", ClosePane),
            MenuItem::action("Kill Window", KillWindow),
            MenuItem::action("Kill Session…", KillSession),
            MenuItem::separator(),
            MenuItem::action("Detach", Detach),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find…", Find),
            MenuItem::action("Copy Mode", CopyMode),
            MenuItem::action("Clear Scrollback", ClearScrollback),
        ]),
        Menu::new("View").items([
            MenuItem::action("Command Palette…", OpenCommandPalette),
            MenuItem::action("Choose Window…", ChooseWindow),
            MenuItem::separator(),
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::separator(),
            MenuItem::action("Zoom Pane", ZoomPane),
            MenuItem::submenu(
                Menu::new("Select Pane").items(
                    [("Up", "U"), ("Down", "D"), ("Left", "L"), ("Right", "R")]
                        .into_iter()
                        .map(|(name, direction)| {
                            MenuItem::action(
                                name,
                                SelectPane {
                                    direction: direction.to_owned(),
                                },
                            )
                        }),
                ),
            ),
            MenuItem::separator(),
            MenuItem::action("Zoom In", IncreaseUiZoom),
            MenuItem::action("Zoom Out", DecreaseUiZoom),
            MenuItem::action("Reset Zoom", ResetUiZoom),
            MenuItem::separator(),
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::action("Reload", Reload),
            MenuItem::action("Developer Tools", ToggleDevTools),
        ]),
        #[cfg(target_os = "macos")]
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::separator(),
        ]),
        Menu::new("Help").items([
            open_url("zz Documentation", "https://zzmux.sh/docs"),
            open_url("Keyboard Shortcuts", "https://zzmux.sh/docs"),
            open_url("Release Notes", RELEASE_NOTES_URL),
            MenuItem::separator(),
            open_url(
                "Report an Issue",
                "https://github.com/demfabris/zz/issues/new",
            ),
            MenuItem::action("Open Logs", OpenLogs),
        ]),
    ]
}

#[cfg(not(target_os = "ios"))]
fn open_url(label: &str, url: &str) -> MenuItem {
    MenuItem::action(
        label.to_owned(),
        OpenUrl {
            url: url.to_owned(),
        },
    )
}

#[cfg(all(test, not(target_os = "ios")))]
mod tests {
    use gpui::AppContext as _;

    use super::*;

    #[gpui::test]
    fn session_menu_follows_replaced_snapshots_and_empty_workspaces(cx: &mut gpui::TestAppContext) {
        let mux = cx.new(|cx| {
            MuxClient::new(
                Err(zz_daemon::DaemonError::Thread("test client".to_owned())),
                zz_daemon::default_socket_path(),
                cx,
            )
        });
        cx.update(|cx| observe_mux(&mux, cx));
        assert!(cx.read(|cx| cx.global::<MenuSessions>().0.is_empty()));

        let snapshot = zz_protocol::MuxSnapshot {
            generation: 1,
            focused_window: None,
            sessions: vec![zz_protocol::SessionSnapshot {
                id: zz_protocol::SessionId(1),
                name: "work".to_owned(),
                active_window: zz_protocol::WindowId(1),
                windows: Vec::new(),
                viewers: Vec::new(),
            }],
        };
        mux.update(cx, |mux, cx| {
            mux.attach_snapshot_for_test(zz_protocol::SessionId(1), snapshot.clone(), cx);
        });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx.global::<MenuSessions>().0.clone()),
            BTreeSet::from(["work".to_owned()])
        );

        mux.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx.global::<MenuSessions>().0.clone()),
            BTreeSet::from(["work".to_owned()])
        );

        let mut renamed = snapshot;
        renamed.sessions[0].name = "renamed".to_owned();
        mux.update(cx, |mux, cx| {
            mux.attach_snapshot_for_test(zz_protocol::SessionId(1), renamed, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx.global::<MenuSessions>().0.clone()),
            BTreeSet::from(["renamed".to_owned()])
        );

        mux.update(cx, |mux, cx| {
            mux.handle_message_for_test(
                zz_protocol::ProtocolMessage::Event(zz_protocol::Event {
                    sequence: 0,
                    payload: zz_protocol::EventPayload::Snapshot(
                        zz_protocol::MuxSnapshot::default(),
                    ),
                }),
                cx,
            );
        });
        cx.run_until_parked();
        assert!(cx.read(|cx| cx.global::<MenuSessions>().0.is_empty()));
    }

    #[test]
    fn top_level_order_and_native_window_menu() {
        let menus = menu_tree(&BTreeSet::new());
        let names = menus
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect::<Vec<_>>();
        #[cfg(target_os = "macos")]
        assert_eq!(names, ["zz", "File", "Edit", "View", "Window", "Help"]);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(names, ["zz", "File", "Edit", "View", "Help"]);
        let window = menus.iter().find(|menu| menu.name.as_ref() == "Window");
        assert_eq!(window.is_some(), cfg!(target_os = "macos"));
        if let Some(window) = window {
            assert_eq!(window.items.len(), 3);
            assert!(matches!(window.items.last(), Some(MenuItem::Separator)));
        }
    }
}
