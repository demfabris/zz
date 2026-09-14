use std::rc::Rc;

use gpui::{AnyElement, App, Context, Entity, IntoElement, px};
use zz_client::StatusBarModel;
use zz_ui::{
    navigation::{
        WorkspaceStatusWindowState,
        status::{
            MAX_VISIBLE_WINDOWS, StatusAction, StatusAgentEntry, StatusPaneEntry,
            StatusSessionEntry, StatusWindowActions, StatusWindowEntry, status_agents,
            status_session, status_window, status_window_overflow, visible_window_range,
        },
    },
    shell::{WorkspaceStatusSlots, workspace_status_bar},
};

use super::{WebClient, sidebar};
use crate::connection::Connection;

pub(super) fn render(view: &WebClient, cx: &mut Context<WebClient>) -> AnyElement {
    let connection = view.connection.read(cx);
    let core = &connection.core;
    let connected = connection.connected;
    let writable = connected && !core.attached_read_only();
    let model = StatusBarModel::from_snapshot(
        core.snapshot(),
        core.attached_session(),
        None,
        view.preferences.status_bar_settings(),
    );
    let active = model
        .windows
        .iter()
        .position(|window| window.active)
        .unwrap_or(0);
    let visible = visible_window_range(model.windows.len(), active, MAX_VISIBLE_WINDOWS);
    let mut windows = model.windows[visible]
        .iter()
        .map(|window| {
            let id = window.id;
            let rename = writable.then(|| {
                let (label, command) = zz_client::navigation::rename_prompt_command(
                    zz_client::navigation::RenameTarget::Window(id),
                    &window.name,
                );
                let connection = view.connection.clone();
                (
                    label.into(),
                    Rc::new(move |_: &mut gpui::Window, cx: &mut App| {
                        sidebar::execute(&connection, &command, cx);
                    }) as StatusAction,
                )
            });
            status_window(
                ("web-status-window", id.0),
                window.index.to_string().into(),
                window.label.clone().into(),
                WorkspaceStatusWindowState {
                    connected,
                    active: window.active,
                    bell: window.bell,
                    activity: window.activity,
                },
                window
                    .panes
                    .iter()
                    .map(|pane| {
                        let connection = view.connection.clone();
                        let target = pane.id.to_string();
                        StatusPaneEntry::from_pane(
                            pane,
                            Rc::new(move |_, cx| {
                                connection.update(cx, |connection, cx| {
                                    connection.command(
                                        "select-window",
                                        vec!["-t".into(), target.clone()],
                                        cx,
                                    );
                                    connection.command(
                                        "select-pane",
                                        vec!["-t".into(), target.clone()],
                                        cx,
                                    );
                                });
                            }),
                        )
                    })
                    .collect(),
                StatusWindowActions {
                    select: command_action(&view.connection, "select-window", id.to_string()),
                    close: writable
                        .then(|| command_action(&view.connection, "kill-window", id.to_string())),
                    rename,
                },
                cx,
            )
        })
        .collect::<Vec<_>>();
    if model.windows.len() > MAX_VISIBLE_WINDOWS {
        let entries = model
            .windows
            .iter()
            .map(|window| StatusWindowEntry {
                label: format!("{} {}", window.index, window.label).into(),
                active: window.active,
                select: command_action(&view.connection, "select-window", window.id.to_string()),
            })
            .collect();
        windows.push(status_window_overflow(
            "web-status-window-overflow",
            entries,
            connected,
            cx,
        ));
    }
    let mut right = Vec::new();
    if !model.agents.is_empty() {
        let agents = model
            .agents
            .iter()
            .map(|agent| {
                let connection = view.connection.clone();
                let target = agent.id.to_string();
                StatusAgentEntry {
                    label: agent.label.clone().into(),
                    window_name: agent.window_name.clone().into(),
                    icon: zz_ui::pane::agent_provider_icon(agent.provider),
                    status: connected
                        .then(|| {
                            core.agent_state(agent.id)
                                .map(zz_client::agent_attention_status)
                        })
                        .flatten(),
                    select: Rc::new(move |_, cx| {
                        connection.update(cx, |connection, cx| {
                            connection.command(
                                "select-window",
                                vec!["-t".into(), target.clone()],
                                cx,
                            );
                            connection.command(
                                "select-pane",
                                vec!["-t".into(), target.clone()],
                                cx,
                            );
                        });
                    }),
                }
            })
            .collect();
        right.push(status_agents("web-status-agents", agents, connected, cx));
    }
    let session = model.session_name.map(|name| {
        let sessions = core
            .snapshot()
            .sessions
            .iter()
            .map(|session| {
                let id = session.id;
                let connection = view.connection.clone();
                StatusSessionEntry {
                    label: format!(
                        "{} · {} window{}",
                        session.name,
                        session.windows.len(),
                        if session.windows.len() == 1 { "" } else { "s" }
                    )
                    .into(),
                    active: Some(id) == core.attached_session(),
                    select: Rc::new(move |_, cx| {
                        connection.update(cx, |connection, cx| connection.attach(id, cx));
                    }),
                }
            })
            .collect();
        status_session("web-status-session", name.into(), sessions, connected, cx)
    });
    workspace_status_bar(
        view.preferences.gaps,
        px(8.0),
        WorkspaceStatusSlots {
            session,
            windows,
            right,
            titlebar_controls: (!view.sidebar).then(|| (view.controls(cx), px(52.0))),
            window_controls: None,
        },
        cx,
    )
    .into_any_element()
}

fn command_action(
    connection: &Entity<Connection>,
    command: &'static str,
    target: String,
) -> StatusAction {
    let connection = connection.clone();
    Rc::new(move |_, cx| {
        connection.update(cx, |connection, cx| {
            connection.command(command, vec!["-t".into(), target.clone()], cx);
        });
    })
}
