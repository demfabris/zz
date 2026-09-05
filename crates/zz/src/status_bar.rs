use std::ops::Range;

use chrono::Local;
use gpui::{
    AnyElement, App, Entity, IntoElement, MouseButton, Pixels, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use zz_client::{StatusBarAlignment, StatusBarClock, StatusBarModel, StatusBarWindow};
use zz_ui::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    navigation::{
        WorkspaceStatusWindowState, workspace_controls_leading_inset, workspace_row_highlight,
        workspace_status_item, workspace_status_window,
    },
    tooltip::Tooltip,
};

use crate::{
    mux::{
        client::MuxClient,
        hosts::HostId,
        nav::{
            MuxTreeModel, TreeNode, TreeTarget, activate_nav, kill_target_command,
            select_window_command,
        },
    },
    theme::chrome_background,
    workspace::sidebar::WorkspaceSidebar,
};

const MAX_VISIBLE_WINDOWS: usize = 5;

use zz_ui::shell::{WorkspaceStatusSlots, workspace_status_bar};

pub(crate) fn render_gui_status_bar(
    mux: &Entity<MuxClient>,
    sidebar: &Entity<WorkspaceSidebar>,
    titlebar_controls: Option<(AnyElement, Pixels)>,
    window_controls: Option<AnyElement>,
    _window: &mut Window,
    cx: &mut App,
) -> Stateful<gpui::Div> {
    let background = chrome_background(cx);
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
        .map(|name| render_session(name, sidebar, cx));
    let right = render_right_items(&model, cx);
    workspace_status_bar(
        model.alignment == StatusBarAlignment::Center,
        crate::config::pane_gaps(cx),
        background,
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

fn render_session(name: &str, sidebar: &Entity<WorkspaceSidebar>, cx: &App) -> AnyElement {
    let foreground = cx.theme().foreground;
    let highlight = workspace_row_highlight(cx);
    let focus_sidebar = sidebar.clone();
    workspace_status_item(
        "gui-status-session",
        Some(IconName::SquareTerminal),
        name.to_owned().into(),
        cx,
    )
    .flex_none()
    .px(px(8.0))
    .rounded(cx.theme().radius)
    .bg(highlight)
    .when(cx.theme().shadow, |item| {
        item.border(px(0.5)).control_highlight(cx)
    })
    .text_color(foreground)
    .cursor_pointer()
    .hover(move |item| item.bg(highlight).text_color(foreground))
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        focus_sidebar.update(cx, |sidebar, cx| sidebar.focus(window, cx));
    })
    .into_any_element()
}

fn render_right_items(model: &StatusBarModel, cx: &App) -> Vec<AnyElement> {
    let mut items = Vec::new();
    if let Some(count) = model.agent_count {
        let label = if count == 1 {
            "1 agent".to_owned()
        } else {
            format!("{count} agents")
        };
        items.push(
            workspace_status_item("gui-status-agents", Some(IconName::Bot), label.into(), cx)
                .into_any_element(),
        );
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
    if let Some(clock) = render_clock(model.clock, cx) {
        items.push(clock);
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
        .rounded(cx.theme().radius)
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

fn render_clock(clock: StatusBarClock, cx: &App) -> Option<AnyElement> {
    let now = Local::now();
    let label = match clock {
        StatusBarClock::TwentyFourHour => now.format("%H:%M").to_string(),
        StatusBarClock::TwelveHour => now.format("%I:%M %p").to_string(),
        StatusBarClock::TimeAndDate => now.format("%H:%M · %b %d").to_string(),
        StatusBarClock::Off => return None,
    };
    Some(
        workspace_status_item("gui-status-clock", Some(IconName::Clock), label.into(), cx)
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
    let tooltip: SharedString = format!("{}:{}", window.index, window.name).into();
    let id = window.id;
    let select_mux = mux.clone();
    let rename = model.rename_activation_for_node(
        TreeNode::Target(attached_host, TreeTarget::Window(id)),
        attached_host,
    );
    let rename_mux = mux.clone();
    let close_mux = mux.clone();
    let item = workspace_status_window(
        ("gui-status-window", id.0),
        window.index.to_string().into(),
        window.name.clone().into(),
        tooltip,
        WorkspaceStatusWindowState {
            connected,
            active: window.active,
            bell: window.bell,
            activity: window.activity,
            agent: window.agent,
        },
        cx,
    )
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        if connected {
            select_mux.read(cx).execute(select_window_command(id));
        }
    });
    if !connected {
        return item.into_any_element();
    }
    item.context_menu(move |menu, _, _| {
        let menu = if let Some((label, activation)) = rename.clone() {
            let rename_mux = rename_mux.clone();
            menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                activate_nav(&rename_mux, activation.clone(), cx);
            }))
        } else {
            menu
        };
        let close_mux = close_mux.clone();
        menu.item(
            PopupMenuItem::new("Close Window")
                .icon(IconName::Xmark)
                .on_click(move |_, _, cx| {
                    close_mux
                        .read(cx)
                        .execute(kill_target_command(TreeTarget::Window(id)));
                }),
        )
    })
    .into_any_element()
}

fn render_window_overflow(
    windows: &[StatusBarWindow],
    connected: bool,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> AnyElement {
    let windows = windows.to_vec();
    let menu_mux = mux.clone();
    Button::new("gui-status-window-overflow")
        .ghost()
        .xsmall()
        .compact()
        .icon(IconName::Ellipsis)
        .hover_bg(workspace_row_highlight(cx))
        .tooltip("All windows")
        .disabled(!connected)
        .dropdown_menu(move |menu, _, _| {
            windows.iter().fold(menu, |menu, window| {
                let id = window.id;
                let select_mux = menu_mux.clone();
                menu.item(
                    PopupMenuItem::new(native_window_label(window))
                        .icon(if window.active {
                            IconName::Check
                        } else {
                            IconName::AppWindow
                        })
                        .on_click(move |_, _, cx| {
                            select_mux.read(cx).execute(select_window_command(id));
                        }),
                )
            })
        })
        .into_any_element()
}

fn visible_window_range(total: usize, active: usize, limit: usize) -> Range<usize> {
    if total <= limit || limit == 0 {
        return 0..total;
    }
    let mut start = active.saturating_sub(limit / 2);
    start = start.min(total - limit);
    start..start + limit
}

fn native_window_label(window: &StatusBarWindow) -> SharedString {
    format!("{} {}", window.index, window.name).into()
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
