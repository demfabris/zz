use super::WebClient;
use crate::connection::Connection;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Hsla, ListSizingBehavior, MouseButton,
    ScrollStrategy, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use std::{collections::BTreeSet, rc::Rc};
use zz_client::{
    AgentAttentionStatus, ChromeAction,
    navigation::{RenameTarget, ordered_panes, pane_label, rename_prompt_command, session_label},
};
use zz_protocol::{CommandInvocation, MuxSnapshot, PaneId, SessionId, WindowId};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    navigation::{
        WORKSPACE_TREE_CONTENT_INSET, WORKSPACE_TREE_INDENT_WIDTH,
        WORKSPACE_TREE_MARKER_SLOT_WIDTH,
        sidebar::{
            TreeNavigation, TreeNavigationResult, TreeNavigationRow, tree_action_strip,
            tree_host_indicator, tree_host_marker, tree_navigation, tree_node_marker,
            tree_row_rename_menu, tree_window_layout_button,
        },
        tree::{IndentGuideColors, WorkspaceIndentGuides},
        workspace_tree_action_button, workspace_tree_disclosure, workspace_tree_row,
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

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum Badge {
    NeedsInput,
    Failed,
    Working,
    Finished,
}

impl Badge {
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::NeedsInput => cx.theme().warning,
            Self::Failed => cx.theme().danger,
            Self::Working => cx.theme().foreground.muted(),
            Self::Finished => cx.theme().success,
        }
    }
}

fn pane_badge(
    pane: PaneId,
    core: &zz_client::ClientCore,
    unseen: &BTreeSet<PaneId>,
) -> Option<Badge> {
    match zz_client::agent_attention_status(core.agent_state(pane)?) {
        AgentAttentionStatus::NeedsInput => Some(Badge::NeedsInput),
        AgentAttentionStatus::Failed => Some(Badge::Failed),
        AgentAttentionStatus::Working => Some(Badge::Working),
        AgentAttentionStatus::Idle => unseen.contains(&pane).then_some(Badge::Finished),
    }
}

fn rollup(
    panes: impl Iterator<Item = PaneId>,
    core: &zz_client::ClientCore,
    unseen: &BTreeSet<PaneId>,
) -> Option<Badge> {
    panes
        .filter_map(|pane| pane_badge(pane, core, unseen))
        .min()
}

pub(super) fn execute(connection: &Entity<Connection>, command: &CommandInvocation, cx: &mut App) {
    connection.update(cx, |connection, cx| {
        connection.command(
            &command.name,
            command
                .args
                .iter()
                .map(|arg| arg.as_str().to_owned())
                .collect(),
            cx,
        );
    });
}

fn rename_command(
    snapshot: &MuxSnapshot,
    target: Target,
) -> Option<(&'static str, CommandInvocation)> {
    match target {
        Target::Host => None,
        Target::Session(id) => snapshot
            .sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| rename_prompt_command(RenameTarget::Session(id), &session.name)),
        Target::Window(id) => snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .find(|window| window.id == id)
            .map(|window| rename_prompt_command(RenameTarget::Window(id), &window.name)),
        Target::Pane(id) => snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .find(|window| window.panes.contains_key(&id))
            .map(|window| rename_prompt_command(RenameTarget::Window(window.id), &window.name)),
    }
}

fn activate(connection: &Entity<Connection>, target: Target, cx: &mut App) {
    connection.update(cx, |connection, cx| {
        if !connection.connected {
            if target == Target::Host {
                connection.reconnect(cx);
            }
            return;
        }
        let owner = connection
            .core
            .snapshot()
            .sessions
            .iter()
            .find(|session| match target {
                Target::Host => false,
                Target::Session(id) => session.id == id,
                Target::Window(id) => session.windows.iter().any(|window| window.id == id),
                Target::Pane(id) => session
                    .windows
                    .iter()
                    .any(|window| window.panes.contains_key(&id)),
            })
            .map(|session| session.id);
        if let Some(owner) = owner
            && (matches!(target, Target::Session(_))
                || connection.core.attached_session() != Some(owner))
        {
            connection.attach(owner, cx);
        }
        match target {
            Target::Host | Target::Session(_) => {}
            Target::Window(id) => {
                connection.command("select-window", vec!["-t".into(), id.to_string()], cx);
            }
            Target::Pane(id) => {
                connection.command("select-pane", vec!["-t".into(), id.to_string()], cx);
            }
        }
    });
}

fn toggle(view: &mut WebClient, target: Target, cx: &mut Context<WebClient>) {
    view.sidebar_selection = Some(target);
    if !view.collapsed_tree.remove(&target) {
        view.collapsed_tree.insert(target);
    }
    cx.notify();
}

pub(super) fn reconcile(view: &mut WebClient, window: &Window, cx: &App) {
    let core = &view.connection.read(cx).core;
    let snapshot = core.snapshot();
    view.unseen_agents
        .retain(|pane| core.agent_state(*pane).is_some());
    if window.is_window_active()
        && let Some(pane) = view.focused_pane
    {
        view.unseen_agents.remove(&pane);
    }
    let session = snapshot
        .sessions
        .iter()
        .find(|session| Some(session.id) == core.attached_session());
    let mux_window = session.and_then(|session| {
        session
            .windows
            .iter()
            .find(|window| window.id == snapshot.focused_window_for(session))
    });
    let active = mux_window
        .map(|window| Target::Pane(window.active_pane))
        .or_else(|| session.map(|session| Target::Session(session.id)))
        .unwrap_or(Target::Host);
    if view.sidebar_active != Some(active) {
        view.sidebar_active = Some(active);
        view.collapsed_tree.remove(&Target::Host);
        if let Some(session) = session {
            view.collapsed_tree.remove(&Target::Session(session.id));
        }
        if let Some(window) = mux_window {
            view.collapsed_tree.remove(&Target::Window(window.id));
        }
        if !view.sidebar_focus.is_focused(window) || view.sidebar_pointer_selection {
            view.sidebar_selection = Some(active);
        }
    }
    let rows = tree_rows(
        snapshot,
        core.attached_session(),
        &view.collapsed_tree,
        core,
        &view.unseen_agents,
    );
    if !rows
        .iter()
        .any(|row| Some(row.target) == view.sidebar_selection)
    {
        view.sidebar_selection = rows
            .iter()
            .rfind(|row| row.on_active_path)
            .map(|row| row.target)
            .or(Some(Target::Host));
    }
}

pub(super) fn handle_key(
    view: &mut WebClient,
    action: ChromeAction,
    window: &mut Window,
    cx: &mut Context<WebClient>,
) -> bool {
    reconcile(view, window, cx);
    let core = &view.connection.read(cx).core;
    let rows = tree_rows(
        core.snapshot(),
        core.attached_session(),
        &view.collapsed_tree,
        core,
        &view.unseen_agents,
    );
    let selected = rows
        .iter()
        .position(|row| Some(row.target) == view.sidebar_selection);
    let direction = match action {
        ChromeAction::SidebarSelectUp => Some(TreeNavigation::Up),
        ChromeAction::SidebarSelectDown => Some(TreeNavigation::Down),
        ChromeAction::SidebarSelectFirst => Some(TreeNavigation::First),
        ChromeAction::SidebarSelectLast => Some(TreeNavigation::Last),
        ChromeAction::SidebarSelectLeft => Some(TreeNavigation::Left),
        ChromeAction::SidebarSelectRight => Some(TreeNavigation::Right),
        _ => None,
    };
    if let Some(direction) = direction {
        let navigation_rows = rows
            .iter()
            .enumerate()
            .map(|(index, row)| TreeNavigationRow {
                depth: row.depth,
                parent: rows[..index]
                    .iter()
                    .rposition(|parent| parent.depth < row.depth),
                expandable: row.expandable,
                expanded: row.expanded,
            })
            .collect::<Vec<_>>();
        view.sidebar_pointer_selection = false;
        match tree_navigation(&navigation_rows, selected, direction) {
            TreeNavigationResult::Select(index) => {
                view.sidebar_selection = Some(rows[index].target);
                view.sidebar_scroll
                    .scroll_to_item(index, ScrollStrategy::Nearest);
                cx.notify();
            }
            TreeNavigationResult::Toggle(index) => toggle(view, rows[index].target, cx),
            TreeNavigationResult::None => {}
        }
        return true;
    }
    match action {
        ChromeAction::SidebarCancel => {
            view.focused_pane = None;
            view.focus.focus(window, cx);
            cx.notify();
        }
        ChromeAction::SidebarConfirm => {
            if let Some(target) = view.sidebar_selection {
                if target == Target::Host && view.connection.read(cx).connected {
                    toggle(view, target, cx);
                } else {
                    activate(&view.connection, target, cx);
                    view.focused_pane = None;
                    view.focus.focus(window, cx);
                    cx.notify();
                }
            }
        }
        ChromeAction::SidebarRename => {
            if view.connection.read(cx).connected
                && !core.attached_read_only()
                && let Some((_, command)) = view
                    .sidebar_selection
                    .and_then(|target| rename_command(core.snapshot(), target))
            {
                execute(&view.connection, &command, cx);
            }
        }
        ChromeAction::SidebarCommandPalette => view.command("command-prompt", Vec::new(), cx),
        _ => return false,
    }
    true
}
struct TreeRow {
    target: Target,
    depth: u8,
    icon: IconName,
    label: String,
    on_active_path: bool,
    expanded: bool,
    expandable: bool,
    active_pane: Option<PaneId>,
    bell: bool,
    badge: Option<Badge>,
}
pub(super) struct Runtime {
    pub connection: Entity<Connection>,
    pub focus: FocusHandle,
    pub focused: bool,
    pub view: Entity<WebClient>,
    pub selected: Option<Target>,
    pub unseen_agents: BTreeSet<PaneId>,
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
    let rows = tree_rows(
        snapshot,
        attached,
        collapsed,
        &connection.read(cx).core,
        &runtime.unseen_agents,
    );
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
    core: &zz_client::ClientCore,
    unseen_agents: &BTreeSet<PaneId>,
) -> Vec<TreeRow> {
    let expanded = !collapsed.contains(&Target::Host);
    let mut rows = vec![TreeRow {
        target: Target::Host,
        depth: 0,
        icon: IconName::HardDrive,
        label: "zz daemon".into(),
        on_active_path: attached.is_some(),
        expanded,
        expandable: true,
        active_pane: None,
        bell: snapshot
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| window.panes.values())
            .any(|pane| pane.bell),
        badge: rollup(
            snapshot
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .flat_map(|window| window.panes.keys().copied()),
            core,
            unseen_agents,
        ),
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
            label: session_label(&session.name, session.id),
            on_active_path: active_session,
            expanded,
            expandable: !session.windows.is_empty(),
            active_pane: None,
            bell: session
                .windows
                .iter()
                .flat_map(|window| window.panes.values())
                .any(|pane| pane.bell),
            badge: rollup(
                session
                    .windows
                    .iter()
                    .flat_map(|window| window.panes.keys().copied()),
                core,
                unseen_agents,
            ),
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
                icon: IconName::AppWindow,
                label: window.name.clone(),
                on_active_path: active_window,
                expanded,
                expandable: !window.panes.is_empty(),
                active_pane: Some(window.active_pane),
                bell: window.panes.values().any(|pane| pane.bell),
                badge: rollup(window.panes.keys().copied(), core, unseen_agents),
            });
            if !expanded {
                continue;
            }
            rows.extend(ordered_panes(window).into_iter().map(|pane| TreeRow {
                target: Target::Pane(pane.id),
                depth: 3,
                icon: super::pane_icon(&pane.kind),
                label: pane_label(pane),
                on_active_path: active_window && window.active_pane == pane.id,
                expanded: false,
                expandable: false,
                active_pane: None,
                bell: pane.bell,
                badge: pane_badge(pane.id, core, unseen_agents),
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
        actions.push(tree_window_layout_button(
            format!("web-tree-layout-{id}"),
            disabled,
            move |horizontal, _, cx| {
                connection.update(cx, |connection, cx| {
                    connection.command(
                        "split-picker",
                        vec![
                            if horizontal { "-h" } else { "-v" }.into(),
                            "-t".into(),
                            pane.to_string(),
                        ],
                        cx,
                    );
                });
            },
            cx,
        ));
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
    if is_host && !connected {
        actions.insert(
            0,
            tree_host_indicator(
                "web-tree-host-indicator",
                connection.read(cx).status == "Connecting…",
                Some(connection.read(cx).status.clone().into()),
                cx,
            ),
        );
    }
    let actions = tree_action_strip(format!("web-tree-actions-{id}"), actions);
    let badge_color = entry.badge.map(|badge| badge.color(cx));
    let marker = if is_host {
        tree_host_marker(entry.bell, badge_color, cx)
    } else {
        tree_node_marker(
            Icon::new(entry.icon.clone()).small().text_color(color),
            entry.bell,
            badge_color,
            cx,
        )
    };
    let marker = if entry.expandable {
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
                view.sidebar_pointer_selection = true;
                toggle(view, target, cx);
            });
            cx.stop_propagation();
        })
        .into_any_element()
    } else {
        marker
    };
    let connection = connection.clone();
    let focus = runtime.focus.clone();
    let view = runtime.view.clone();
    let row = workspace_tree_row(
        format!("web-tree-row-{id}"),
        entry.depth,
        active,
        runtime.focused && runtime.selected == Some(target),
        runtime.focused,
        connected,
        entry.expandable || connected && !is_host,
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
        view.update(cx, |view, cx| {
            view.sidebar_selection = Some(target);
            view.sidebar_pointer_selection = true;
            if target == Target::Host && connection.read(cx).connected {
                toggle(view, target, cx);
            }
            cx.notify();
        });
        activate(&connection, target, cx);
        cx.stop_propagation();
    });
    if !disabled
        && let Some((label, command)) =
            rename_command(runtime.connection.read(cx).core.snapshot(), target)
    {
        let connection = runtime.connection.clone();
        tree_row_rename_menu(row, label.into(), move |_, cx| {
            execute(&connection, &command, cx);
        })
    } else {
        row.into_any_element()
    }
}
