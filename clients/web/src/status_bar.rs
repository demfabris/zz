use std::rc::Rc;

use gpui::{AnyElement, App, Context, Entity, IntoElement, px};
use zz_client::{StatusBarAlignment, StatusBarModel, StatusBarSettings};
use zz_ui::{
    navigation::{
        WorkspaceStatusWindowState,
        status::{
            MAX_VISIBLE_WINDOWS, StatusAction, StatusWindowActions, StatusWindowEntry,
            status_agent_count, status_session, status_window, status_window_overflow,
            visible_window_range,
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
        StatusBarSettings::default(),
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
                window.name.clone().into(),
                WorkspaceStatusWindowState {
                    connected,
                    active: window.active,
                    bell: window.bell,
                    activity: window.activity,
                    agent: window.agent,
                },
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
                label: format!("{} {}", window.index, window.name).into(),
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
    if let Some(count) = model.agent_count {
        right.push(status_agent_count("web-status-agents", count, cx));
    }
    #[cfg(target_family = "wasm")]
    right.push(zz_ui::navigation::status::status_clock(
        "web-status-clock",
        clock_label().into(),
        cx,
    ));
    let entity = cx.entity();
    let session = model.session_name.map(|name| {
        status_session(
            "web-status-session",
            name.into(),
            move |window, cx| {
                entity.update(cx, |view, cx| view.focus_sidebar(window, cx));
            },
            cx,
        )
    });
    workspace_status_bar(
        model.alignment == StatusBarAlignment::Center,
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

#[cfg(target_family = "wasm")]
fn clock_label() -> String {
    let now = js_sys::Date::new_0();
    format!("{:02}:{:02}", now.get_hours(), now.get_minutes())
}
