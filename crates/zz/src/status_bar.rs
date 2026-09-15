use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, IntoElement, MouseButton, Pixels, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use zz_client::{StatusBarModel, StatusBarWindow};
use zz_ui::{
    ActiveTheme as _, IconName, StyledExt as _,
    navigation::{
        WorkspaceStatusWindowState,
        status::{
            MAX_VISIBLE_WINDOWS, StatusAgentEntry, StatusPaneEntry, StatusSessionEntry,
            StatusWindowActions, StatusWindowEntry, status_agents, status_session, status_window,
            status_window_overflow, visible_window_range,
        },
        workspace_controls_leading_inset, workspace_row_highlight, workspace_status_item,
    },
    tooltip::Tooltip,
};

use crate::mux::{
    client::MuxClient,
    hosts::HostId,
    nav::{
        MuxTreeModel, TreeNode, TreeTarget, activate_nav, kill_target_command,
        select_pane_commands, select_window_command,
    },
};

use zz_ui::shell::{WorkspaceStatusSlots, workspace_status_bar};

pub(crate) fn render_gui_status_bar(
    mux: &Entity<MuxClient>,
    titlebar_controls: Option<(AnyElement, Pixels)>,
    window_controls: Option<AnyElement>,
    _window: &mut Window,
    cx: &mut App,
) -> Stateful<gpui::Div> {
    let (snapshot, attached_host, attached, connected) = {
        let mux = mux.read(cx);
        (
            mux.snapshot(),
            mux.attached_host(),
            mux.attached_session(),
            mux.is_connected(),
        )
    };
    let tree_model = MuxTreeModel::from_mux(mux.read(cx));
    let host_name = if attached_host == HostId::LOCAL {
        None
    } else {
        tree_model
            .host(attached_host)
            .map(|host| host.name.as_str())
    };
    let model = StatusBarModel::from_snapshot(
        &snapshot,
        attached,
        host_name,
        crate::config::status_bar_settings(cx),
    );
    let active_index = model
        .windows
        .iter()
        .position(|window| window.active)
        .unwrap_or(0);
    let visible_range =
        visible_window_range(model.windows.len(), active_index, MAX_VISIBLE_WINDOWS);
    let visible_windows = model.windows[visible_range]
        .iter()
        .map(|window| render_status_window(window, connected, attached_host, &tree_model, mux, cx))
        .collect::<Vec<_>>();
    let overflow = (model.windows.len() > MAX_VISIBLE_WINDOWS)
        .then(|| render_window_overflow(&model.windows, connected, mux, cx));
    let session = model
        .session_name
        .as_deref()
        .map(|name| render_session(name, &snapshot, attached, attached_host, connected, mux, cx));
    let right = render_right_items(&model, mux, connected, cx);
    workspace_status_bar(
        crate::config::pane_gaps(cx),
        workspace_controls_leading_inset(cx),
        WorkspaceStatusSlots {
            session,
            windows: visible_windows.into_iter().chain(overflow).collect(),
            right,
            titlebar_controls,
            window_controls,
        },
        cx,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_session(
    name: &str,
    snapshot: &zz_protocol::MuxSnapshot,
    attached: Option<zz_protocol::SessionId>,
    host: HostId,
    connected: bool,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> AnyElement {
    let sessions = snapshot
        .sessions
        .iter()
        .map(|session| {
            let id = session.id;
            let mux = mux.clone();
            StatusSessionEntry {
                label: format!(
                    "{} · {} window{}",
                    session.name,
                    session.windows.len(),
                    if session.windows.len() == 1 { "" } else { "s" }
                )
                .into(),
                active: Some(id) == attached,
                select: Rc::new(move |_, cx| {
                    mux.update(cx, |mux, cx| {
                        mux.attach_to_host(host, id, cx);
                    });
                }),
            }
        })
        .collect();
    status_session(
        "gui-status-session",
        name.to_owned().into(),
        sessions,
        connected,
        cx,
    )
}

fn render_right_items(
    model: &StatusBarModel,
    mux: &Entity<MuxClient>,
    connected: bool,
    cx: &App,
) -> Vec<AnyElement> {
    let mut items = Vec::new();
    if !model.agents.is_empty() {
        let agents = model
            .agents
            .iter()
            .map(|agent| {
                let status = connected
                    .then(|| mux.read(cx).agent_attention_status(agent.id))
                    .flatten();
                let id = agent.id;
                let mux = mux.clone();
                StatusAgentEntry {
                    label: agent.label.clone().into(),
                    window_name: agent.window_name.clone().into(),
                    icon: zz_ui::pane::agent_provider_icon(agent.provider),
                    status,
                    select: Rc::new(move |_, cx| {
                        for command in select_pane_commands(id) {
                            mux.read(cx).execute(command);
                        }
                    }),
                }
            })
            .collect();
        items.push(status_agents("gui-status-agents", agents, connected, cx));
    }
    if let Some(host) = &model.host_name {
        items.push(
            workspace_status_item(
                "gui-status-host",
                Some(IconName::Globe),
                host.clone().into(),
                cx,
            )
            .into_any_element(),
        );
    }
    if let Some(update) = render_update(model.show_update, cx) {
        items.push(update);
    }
    items
}

fn render_update(show: bool, cx: &App) -> Option<AnyElement> {
    if !show {
        return None;
    }
    let status = crate::update::status(cx)?;
    let crate::update::CheckState::Available(release) = status.check else {
        return None;
    };
    let foreground = cx.theme().foreground;
    let highlight = workspace_row_highlight(cx);
    let tooltip: SharedString = format!("Install zz {}", release.version).into();
    Some(
        workspace_status_item(
            "gui-status-update",
            None,
            format!("v{}", release.version).into(),
            cx,
        )
        .flex_none()
        .px(px(6.0))
        .rounded(cx.theme().control_radius())
        .when(cx.theme().shadow, |item| {
            item.border(px(0.5)).border_color(gpui::transparent_white())
        })
        .cursor_pointer()
        .hover(move |item| {
            let item = item.bg(highlight).text_color(foreground);
            if cx.theme().shadow {
                item.control_highlight(cx)
            } else {
                item
            }
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            div()
                .flex_none()
                .size(px(5.0))
                .rounded_full()
                .bg(cx.theme().success),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, window, cx| {
            cx.stop_propagation();
            crate::update::install(window, cx);
        })
        .into_any_element(),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_status_window(
    window: &StatusBarWindow,
    connected: bool,
    attached_host: HostId,
    model: &MuxTreeModel,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> AnyElement {
    let id = window.id;
    let select_mux = mux.clone();
    let close_mux = mux.clone();
    let rename = model
        .rename_activation_for_node(
            TreeNode::Target(attached_host, TreeTarget::Window(id)),
            attached_host,
        )
        .map(|(label, activation)| {
            let mux = mux.clone();
            (
                label.into(),
                Rc::new(move |_: &mut Window, cx: &mut App| {
                    activate_nav(&mux, activation.clone(), cx);
                }) as zz_ui::navigation::status::StatusAction,
            )
        });
    status_window(
        ("gui-status-window", id.0),
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
                let pane_id = pane.id;
                let mux = mux.clone();
                let mut entry = StatusPaneEntry::from_pane(
                    pane,
                    Rc::new(move |_, cx| {
                        for command in select_pane_commands(pane_id) {
                            mux.read(cx).execute(command);
                        }
                    }),
                );
                if let zz_protocol::PaneKindSnapshot::Browser(browser) = &pane.kind {
                    entry.favicon =
                        crate::browser::recent_pages::favicon(&browser.profile, browser.url(), cx);
                    if let Some(title) =
                        crate::browser::recent_pages::title(&browser.profile, browser.url(), cx)
                    {
                        entry.label = title.into();
                    }
                }
                entry
            })
            .collect(),
        StatusWindowActions {
            select: Rc::new(move |_, cx| {
                select_mux.read(cx).execute(select_window_command(id));
            }),
            close: Some(Rc::new(move |_, cx| {
                close_mux
                    .read(cx)
                    .execute(kill_target_command(TreeTarget::Window(id)));
            })),
            rename,
        },
        cx,
    )
}

fn render_window_overflow(
    windows: &[StatusBarWindow],
    connected: bool,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> AnyElement {
    let entries = windows
        .iter()
        .map(|window| {
            let id = window.id;
            let mux = mux.clone();
            StatusWindowEntry {
                label: format!("{} {}", window.index, window.label).into(),
                active: window.active,
                select: Rc::new(move |_, cx| {
                    mux.read(cx).execute(select_window_command(id));
                }),
            }
        })
        .collect();
    status_window_overflow("gui-status-window-overflow", entries, connected, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_windows_stay_centered_on_the_active_window() {
        assert_eq!(visible_window_range(3, 1, 5), 0..3);
        assert_eq!(visible_window_range(9, 0, 5), 0..5);
        assert_eq!(visible_window_range(9, 4, 5), 2..7);
        assert_eq!(visible_window_range(9, 8, 5), 4..9);
    }
}
