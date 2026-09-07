use super::WebClient;
use crate::connection::Connection;
use gpui::{
    AnyElement, App, Entity, FocusHandle, ListSizingBehavior, MouseButton, UniformListScrollHandle,
    div, prelude::*, px, uniform_list,
};
use std::{collections::BTreeSet, rc::Rc};
use zz_protocol::{MuxSnapshot, PaneId, SessionId, WindowId};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    menu::{DropdownMenu as _, PopupMenuItem},
    navigation::{
        WORKSPACE_TREE_CONTENT_INSET, WORKSPACE_TREE_INDENT_WIDTH,
        WORKSPACE_TREE_MARKER_SLOT_WIDTH,
        tree::{IndentGuideColors, WorkspaceIndentGuides},
        workspace_tree_action_button, workspace_tree_disclosure, workspace_tree_marker,
        workspace_tree_row,
    },
    scroll::ScrollableElement as _,
};

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Target {
    Host,
    Session(SessionId),
    Window(WindowId),
    Pane(PaneId),
}
impl Target {
    fn id(self) -> String {
        match self {
            Self::Host => "host".into(),
            Self::Session(id) => format!("session-{id}"),
            Self::Window(id) => format!("window-{id}"),
            Self::Pane(id) => format!("pane-{id}"),
        }
    }
}
struct TreeRow {
    target: Target,
    depth: u8,
    icon: IconName,
    label: String,
    on_active_path: bool,
    expanded: bool,
    active_pane: Option<PaneId>,
}
pub(super) struct Runtime {
    pub connection: Entity<Connection>,
    pub focus: FocusHandle,
    pub focused: bool,
    pub view: Entity<WebClient>,
}

pub(super) fn session_tree(
    snapshot: &MuxSnapshot,
    attached: Option<SessionId>,
    runtime: Runtime,
    collapsed: &BTreeSet<Target>,
    scroll: &UniformListScrollHandle,
    cx: &App,
) -> AnyElement {
    let connection = &runtime.connection;
    let connected = connection.read(cx).connected;
    let rows = tree_rows(snapshot, attached, collapsed);
    let active_row = connected
        .then(|| rows.iter().rposition(|row| row.on_active_path))
        .flatten();
    let depths: Rc<[usize]> = rows.iter().map(|row| usize::from(row.depth)).collect();
    let guide_color = cx.theme().foreground.muted();
    let guides = WorkspaceIndentGuides::new(
        depths,
        active_row,
        px(WORKSPACE_TREE_INDENT_WIDTH),
        px(WORKSPACE_TREE_CONTENT_INSET + WORKSPACE_TREE_MARKER_SLOT_WIDTH / 2.0),
        px(4.0),
        IndentGuideColors {
            default: guide_color.wash(),
            active: guide_color,
        },
    );
    let list = uniform_list("web-session-tree-rows", rows.len(), move |range, _, cx| {
        range
            .map(|index| render_row(&rows[index], active_row == Some(index), &runtime, cx))
            .collect::<Vec<_>>()
    })
    .size_full()
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .track_scroll(scroll)
    .with_decoration(guides);
    div()
        .id("web-session-tree")
        .relative()
        .size_full()
        .min_h_0()
        .child(list)
        .vertical_scrollbar(scroll)
        .into_any_element()
}

fn tree_rows(
    snapshot: &MuxSnapshot,
    attached: Option<SessionId>,
    collapsed: &BTreeSet<Target>,
) -> Vec<TreeRow> {
    let expanded = !collapsed.contains(&Target::Host);
    let mut rows = vec![TreeRow {
        target: Target::Host,
        depth: 0,
        icon: IconName::HardDrive,
        label: "zz daemon".into(),
        on_active_path: attached.is_some(),
        expanded,
        active_pane: None,
    }];
    if !expanded {
        return rows;
    }
    for session in &snapshot.sessions {
        let active_session = attached == Some(session.id);
        let target = Target::Session(session.id);
        let expanded = !collapsed.contains(&target);
        rows.push(TreeRow {
            target,
            depth: 1,
            icon: IconName::Layers,
            label: session.name.clone(),
            on_active_path: active_session,
            expanded,
            active_pane: None,
        });
        if !expanded {
            continue;
        }
        for window in &session.windows {
            let active_window = active_session && snapshot.focused_window_for(session) == window.id;
            let target = Target::Window(window.id);
            let expanded = !collapsed.contains(&target);
            rows.push(TreeRow {
                target,
                depth: 2,
                icon: IconName::PanelsTopLeft,
                label: format!("{}  {}", window.index, window.name),
                on_active_path: active_window,
                expanded,
                active_pane: Some(window.active_pane),
            });
            if !expanded {
                continue;
            }
            rows.extend(window.panes.values().map(|pane| TreeRow {
                target: Target::Pane(pane.id),
                depth: 3,
                icon: super::pane_icon(&pane.kind),
                label: if pane.title.is_empty() {
                    pane.id.to_string()
                } else {
                    pane.title.clone()
                },
                on_active_path: active_window && window.active_pane == pane.id,
                expanded: false,
                active_pane: None,
            }));
        }
    }
    rows
}

fn render_row(entry: &TreeRow, active: bool, runtime: &Runtime, cx: &mut App) -> AnyElement {
    let connection = &runtime.connection;
    let connected = connection.read(cx).connected;
    let disabled = !connected || connection.read(cx).core.attached_read_only();
    let target = entry.target;
    let id = target.id();
    let group = format!("web-tree-group-{id}");
    let is_host = matches!(target, Target::Host);
    let color = if is_host || connected && entry.on_active_path {
        cx.theme().foreground
    } else {
        cx.theme().foreground.muted()
    };
    let mut actions = Vec::new();
    let add = match target {
        Target::Host => Some(("New session", "new-session", Vec::new())),
        Target::Session(session) => Some((
            "New window",
            "new-window",
            vec!["-t".into(), session.to_string()],
        )),
        _ => None,
    };
    if let Some((label, command, args)) = add {
        let connection = connection.clone();
        actions.push(
            workspace_tree_action_button(
                format!("web-tree-add-{id}"),
                IconName::Plus,
                label,
                disabled,
                cx,
            )
            .on_click(move |_, _, cx| {
                connection.update(cx, |connection, cx| {
                    connection.command(command, args.clone(), cx);
                });
                cx.stop_propagation();
            })
            .into_any_element(),
        );
    }
    if let Some(pane) = entry.active_pane {
        let connection = connection.clone();
        actions.push(
            workspace_tree_action_button(
                format!("web-tree-layout-{id}"),
                IconName::LayoutColumns,
                "Window layout",
                disabled,
                cx,
            )
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |mut menu, _, _| {
                for (label, icon, flag) in [
                    ("Split right", IconName::PanelRight, "-h"),
                    ("Split bottom", IconName::PanelBottom, "-v"),
                ] {
                    let connection = connection.clone();
                    menu = menu.item(PopupMenuItem::new(label).icon(icon).on_click(
                        move |_, _, cx| {
                            connection.update(cx, |connection, cx| {
                                connection.command(
                                    "split-picker",
                                    vec![flag.into(), "-t".into(), pane.to_string()],
                                    cx,
                                );
                            });
                        },
                    ));
                }
                menu
            })
            .into_any_element(),
        );
    }
    let close = match target {
        Target::Host => None,
        Target::Session(id) => Some(("Close session", "kill-session", id.to_string())),
        Target::Window(id) => Some(("Close window", "kill-window", id.to_string())),
        Target::Pane(id) => Some(("Close pane", "kill-pane", id.to_string())),
    };
    if let Some((label, command, target_id)) = close {
        let connection = connection.clone();
        actions.push(
            workspace_tree_action_button(
                format!("web-tree-close-{id}"),
                IconName::Xmark,
                label,
                disabled,
                cx,
            )
            .on_click(move |_, _, cx| {
                connection.update(cx, |connection, cx| {
                    connection.command(command, vec!["-t".into(), target_id.clone()], cx);
                });
                cx.stop_propagation();
            })
            .into_any_element(),
        );
    }
    let actions = div()
        .id(format!("web-tree-actions-{id}"))
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .children(actions);
    let marker = Icon::new(entry.icon.clone()).small().text_color(color);
    let marker = if matches!(target, Target::Pane(_)) {
        workspace_tree_marker(marker).into_any_element()
    } else {
        let view = runtime.view.clone();
        workspace_tree_disclosure(
            format!("web-tree-disclosure-{id}"),
            marker,
            entry.expanded,
            group.clone().into(),
            cx,
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            view.update(cx, |view, cx| {
                if !view.collapsed_tree.remove(&target) {
                    view.collapsed_tree.insert(target);
                }
                cx.notify();
            });
            cx.stop_propagation();
        })
        .into_any_element()
    };
    let connection = connection.clone();
    let focus = runtime.focus.clone();
    workspace_tree_row(
        format!("web-tree-row-{id}"),
        entry.depth,
        active,
        false,
        runtime.focused,
        connected,
        connected && !is_host,
        !is_host,
        group.into(),
        marker,
        div().text_color(color).child(entry.label.clone()),
        actions,
        cx,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        focus.focus(window, cx);
    })
    .on_click(move |_, _, cx| {
        connection.update(cx, |connection, cx| match target {
            Target::Host => {}
            Target::Session(session) => connection.attach(session, cx),
            Target::Window(window) => {
                connection.command("select-window", vec!["-t".into(), window.to_string()], cx);
            }
            Target::Pane(pane) => {
                connection.command("select-pane", vec!["-t".into(), pane.to_string()], cx);
            }
        });
        cx.stop_propagation();
    })
    .into_any_element()
}
