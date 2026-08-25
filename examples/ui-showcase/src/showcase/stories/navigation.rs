//! The workspace-navigation pieces: host-tree rows and sidebar controls.

use gpui::{
    AnyElement, App, Context, ParentElement as _, SharedString, Styled as _, div, prelude::*, px,
};
use zz_ui::navigation::{
    WORKSPACE_STATUS_CONTENT_HEIGHT, WORKSPACE_TREE_ACTION_INSET, WORKSPACE_TREE_NODE_ICON_SIZE,
    WorkspaceStatusWindowState, workspace_chrome_controls, workspace_layout_button,
    workspace_settings_button, workspace_status_item, workspace_status_window,
    workspace_tree_action_button, workspace_tree_action_row, workspace_tree_disclosure,
    workspace_tree_marker, workspace_tree_row,
};
use zz_ui::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    rems_from_px,
    spinner::Spinner,
    tooltip::Tooltip,
};

use super::{Showcase, gallery, specimen, specimen_block, specimens, story_stack};
use zz_ui::Colorize as _;

pub(super) fn render(cx: &mut Context<Showcase>) -> AnyElement {
    story_stack()
        .child(
            gallery(
                "Host tree rows",
                "The sidebar is one tree over the whole fleet: a row per machine, then its sessions, windows, and panes beneath it. Depth, activation, keyboard cursor, and connection are encoded per row; session and window rows carry a rename context menu (right-click). The app marks a machine with its logo; these substitute a glyph, since the logo is an app asset.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "host · attached, expanded",
                        tree_row(&Row::host("nav-host-local", "studio").active(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "host · connecting",
                        tree_row(&Row::host("nav-host-dialing", "desktop").collapsed().connecting(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "host · unreachable (the reason hangs off the mark)",
                        tree_row(
                            &Row::host("nav-host-down", "builder").collapsed().failed(
                                "ssh: connect to host builder port 22: connection refused",
                            ),
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen_block(
                        "add host · final muted row",
                        tree_row(&Row::add_host("nav-host-add"), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "session · attached, tree focused",
                        tree_row(&Row::session("nav-session", "design").active(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "session · attached, tree unfocused",
                        tree_row(&Row::session("nav-session-blur", "design").active().unfocused(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "session · another viewer is here",
                        tree_row(&Row::session("nav-session-peer", "review").detail("⌁ nara → 1: agents"), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "window · keyboard cursor",
                        tree_row(&Row::window("nav-window", "0: workspace").selected(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "terminal pane",
                        tree_row(&Row::pane("nav-pane-term", "editor · cargo watch", IconName::SquareTerminal), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "browser pane · active + cursor",
                        tree_row(&Row::pane("nav-pane-web", "GPUI components", IconName::Globe).active().selected(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "pane · disconnected",
                        tree_row(&Row::pane("nav-pane-off", "waiting for daemon", IconName::SquareTerminal).disconnected(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "pane · long title truncates before the action gutter",
                        tree_row(&Row::pane("nav-pane-long", "brew update && brew upgrade && brew cleanup --prune=all", IconName::SquareTerminal), cx),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Row disclosure",
                "An expandable row has no triangle column of its own: hovering swaps the node's own marker for a chevron, so the marker slot is the expand target and the rest of the row stays the activation. Hover a specimen to see the swap.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "expanded · hover the marker",
                        tree_row(&Row::session("nav-disc-open", "design").expanded(), cx),
                        cx,
                    ))
                    .child(specimen_block(
                        "collapsed · hover the marker",
                        tree_row(&Row::session("nav-disc-closed", "research").collapsed(), cx),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Sidebar controls",
                "Settings and the sidebar toggle share one compact cluster at the leading end of both sidebar and titlebar modes.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "sidebar toggle",
                        workspace_layout_button("nav-layout"),
                        cx,
                    ))
                    .child(specimen(
                        "control cluster",
                        workspace_chrome_controls(
                            workspace_settings_button("nav-settings"),
                            Some(workspace_layout_button("nav-layout-cluster").into_any_element()),
                        ),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Native tmux status rail",
                "tmux still supplies status-left, one formatted label per window, status-right, and their styles. GPUI owns the regions, spacing, active and bell states, clipping, hover, menus, and overflow instead of painting one terminal-cell row.",
                cx,
            )
            .child(
                specimens().w_full().child(specimen_block(
                    "left · native windows · right",
                    native_status_rail(cx),
                    cx,
                )),
            ),
        )
        .into_any_element()
}

fn native_status_rail(cx: &App) -> AnyElement {
    let foreground = cx.theme().foreground;
    div()
        .flex()
        .w_full()
        .h(zz_ui::TITLE_BAR_HEIGHT)
        .items_center()
        .gap(px(6.0))
        .px(px(6.0))
        .overflow_hidden()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .text_color(foreground)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(workspace_status_item(
                    "nav-status-session",
                    Some(IconName::SquareTerminal),
                    "0".into(),
                    cx,
                ))
                .child(workspace_status_item(
                    "nav-status-host",
                    None,
                    "macbook".into(),
                    cx,
                )),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .h(WORKSPACE_STATUS_CONTENT_HEIGHT)
                .items_center()
                .gap(px(2.0))
                .overflow_hidden()
                .child(workspace_status_window(
                    "nav-status-window-editor",
                    "0".into(),
                    "editor".into(),
                    "0:editor".into(),
                    WorkspaceStatusWindowState {
                        connected: true,
                        active: true,
                        ..WorkspaceStatusWindowState::default()
                    },
                    cx,
                ))
                .child(workspace_status_window(
                    "nav-status-window-server",
                    "1".into(),
                    "server".into(),
                    "1:server".into(),
                    WorkspaceStatusWindowState {
                        connected: true,
                        bell: true,
                        ..WorkspaceStatusWindowState::default()
                    },
                    cx,
                ))
                .child(workspace_status_window(
                    "nav-status-window-docs",
                    "2".into(),
                    "docs".into(),
                    "2:docs".into(),
                    WorkspaceStatusWindowState {
                        connected: true,
                        zoomed: true,
                        ..WorkspaceStatusWindowState::default()
                    },
                    cx,
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(workspace_status_item(
                    "nav-status-command",
                    None,
                    "bash".into(),
                    cx,
                ))
                .child(workspace_status_item(
                    "nav-status-branch",
                    Some(IconName::GitBranch),
                    "main".into(),
                    cx,
                ))
                .child(workspace_status_item(
                    "nav-status-clock",
                    Some(IconName::Clock),
                    "17:49".into(),
                    cx,
                ))
                .child(workspace_status_item(
                    "nav-status-calendar",
                    Some(IconName::Calendar),
                    "23 Aug".into(),
                    cx,
                )),
        )
        .into_any_element()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Host,
    AddHost,
    Session,
    Window,
    Pane,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reachability {
    Connected,
    Connecting,
    Failed(&'static str),
}

#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
struct Row {
    id: &'static str,
    kind: RowKind,
    label: &'static str,
    detail: Option<&'static str>,
    icon: IconName,
    depth: u8,
    active: bool,
    selected: bool,
    focused: bool,
    connected: bool,
    expanded: Option<bool>,
    reachability: Reachability,
}

impl Row {
    fn new(kind: RowKind, id: &'static str, label: &'static str, icon: IconName) -> Self {
        Self {
            id,
            kind,
            label,
            detail: None,
            icon,
            depth: match kind {
                RowKind::Host | RowKind::AddHost => 0,
                RowKind::Session => 1,
                RowKind::Window => 2,
                RowKind::Pane => 3,
            },
            active: false,
            selected: false,
            focused: true,
            connected: true,
            expanded: None,
            reachability: Reachability::Connected,
        }
    }

    fn host(id: &'static str, label: &'static str) -> Self {
        Self {
            expanded: Some(true),
            ..Self::new(RowKind::Host, id, label, IconName::HardDrive)
        }
    }

    fn add_host(id: &'static str) -> Self {
        Self {
            connected: false,
            expanded: None,
            ..Self::new(RowKind::AddHost, id, "Add host", IconName::Plus)
        }
    }

    fn session(id: &'static str, label: &'static str) -> Self {
        Self::new(RowKind::Session, id, label, IconName::Layers)
    }

    fn window(id: &'static str, label: &'static str) -> Self {
        Self::new(RowKind::Window, id, label, IconName::AppWindow)
    }

    fn pane(id: &'static str, label: &'static str, icon: IconName) -> Self {
        Self::new(RowKind::Pane, id, label, icon)
    }

    fn active(mut self) -> Self {
        self.active = true;
        self
    }

    fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    fn unfocused(mut self) -> Self {
        self.focused = false;
        self
    }

    fn detail(mut self, detail: &'static str) -> Self {
        self.detail = Some(detail);
        self
    }

    fn expanded(mut self) -> Self {
        self.expanded = Some(true);
        self
    }

    fn collapsed(mut self) -> Self {
        self.expanded = Some(false);
        self
    }

    fn disconnected(mut self) -> Self {
        self.connected = false;
        self
    }

    fn connecting(mut self) -> Self {
        self.reachability = Reachability::Connecting;
        self.connected = false;
        self
    }

    fn failed(mut self, reason: &'static str) -> Self {
        self.reachability = Reachability::Failed(reason);
        self.connected = false;
        self
    }
}

fn tree_row(row: &Row, cx: &App) -> AnyElement {
    if row.kind == RowKind::AddHost {
        return workspace_tree_action_row(row.id, row.depth, row.icon.clone(), row.label, cx)
            .into_any_element();
    }
    let row_group = SharedString::from(format!("{}-group", row.id));
    let strong = row.kind == RowKind::Host || row.connected && row.active;
    let color = if strong {
        cx.theme().foreground
    } else {
        cx.theme().foreground.muted()
    };
    let marker = workspace_tree_marker(
        Icon::new(row.icon.clone())
            .size(rems_from_px(WORKSPACE_TREE_NODE_ICON_SIZE))
            .text_color(color),
    );
    let marker = match row.expanded {
        Some(expanded) => workspace_tree_disclosure(
            format!("{}-disclosure", row.id),
            marker,
            expanded,
            row_group.clone(),
            cx,
        )
        .into_any_element(),
        None => marker.into_any_element(),
    };

    let built = workspace_tree_row(
        row.id,
        row.depth,
        row.active,
        row.selected,
        row.focused,
        row.connected || row.kind == RowKind::Host,
        row.expanded.is_some() || row.connected && row.kind != RowKind::Host,
        row.kind != RowKind::Host,
        row_group.clone(),
        marker,
        row_label(row, color),
        row_trailing(row, &row_group, cx),
        cx,
    );

    if matches!(row.kind, RowKind::Session | RowKind::Window) {
        built
            .context_menu(|menu, _, _| menu.item(PopupMenuItem::new("Rename…")))
            .into_any_element()
    } else {
        built.into_any_element()
    }
}

fn row_label(row: &Row, color: gpui::Hsla) -> AnyElement {
    div()
        .flex()
        .min_w_0()
        .items_baseline()
        .gap_2()
        .text_color(color)
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(row.label),
        )
        .when_some(row.detail, |this, detail| {
            this.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_xs()
                    .child(detail),
            )
        })
        .into_any_element()
}

fn row_trailing(row: &Row, group: &SharedString, cx: &App) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .when_some(reachability_mark(row, cx), gpui::ParentElement::child)
        .child(row_actions(row, group, cx))
        .into_any_element()
}

fn reachability_mark(row: &Row, cx: &App) -> Option<AnyElement> {
    match row.reachability {
        Reachability::Connected => None,
        Reachability::Connecting => Some(
            Spinner::new()
                .xsmall()
                .color(cx.theme().foreground.muted())
                .into_any_element(),
        ),
        Reachability::Failed(reason) => Some(
            div()
                .id((row.id, 0usize))
                .flex()
                .flex_none()
                .tooltip(move |window, cx| Tooltip::new(reason).build(window, cx))
                .child(Icon::new(IconName::Xmark).xsmall())
                .into_any_element(),
        ),
    }
}

fn row_actions(row: &Row, group: &SharedString, cx: &App) -> AnyElement {
    let mut children = Vec::new();
    match row.kind {
        RowKind::Host => children.push(
            workspace_tree_action_button(
                format!("{}-new-session", row.id),
                IconName::Plus,
                "New session",
                !row.connected,
                cx,
            )
            .into_any_element(),
        ),
        RowKind::Session => children.push(
            workspace_tree_action_button(
                format!("{}-add", row.id),
                IconName::Plus,
                "New window",
                !row.connected,
                cx,
            )
            .into_any_element(),
        ),
        RowKind::Window => children.push(
            workspace_tree_action_button(
                format!("{}-layout", row.id),
                IconName::LayoutColumns,
                "Window layout",
                !row.connected,
                cx,
            )
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, |menu, _, _| {
                menu.item(PopupMenuItem::new("Split right").icon(IconName::PanelRight))
                    .item(PopupMenuItem::new("Split bottom").icon(IconName::PanelBottom))
            })
            .into_any_element(),
        ),
        RowKind::AddHost | RowKind::Pane => {}
    }
    if matches!(row.kind, RowKind::Session | RowKind::Window | RowKind::Pane) {
        children.push(
            workspace_tree_action_button(
                format!("{}-delete", row.id),
                IconName::Xmark,
                "Delete",
                !row.connected,
                cx,
            )
            .into_any_element(),
        );
    }
    div()
        .flex()
        .h_full()
        .flex_none()
        .items_center()
        .justify_center()
        .pr(px(WORKSPACE_TREE_ACTION_INSET))
        .when(row.kind == RowKind::Host, |this| {
            this.invisible()
                .group_hover(group.clone(), gpui::Styled::visible)
        })
        .when(row.kind != RowKind::Host, |this| {
            this.invisible()
                .group_hover(group.clone(), gpui::Styled::visible)
        })
        .children(children)
        .into_any_element()
}
