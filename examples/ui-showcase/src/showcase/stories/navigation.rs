//! The workspace-navigation pieces: host-tree rows, sidebar controls, and the
//! titlebar strip.

use gpui::{
    AnyElement, App, Context, ParentElement as _, SharedString, Styled as _, div, prelude::*, px,
};
use zz_ui::navigation::{
    WORKSPACE_STRIP_GAP, WORKSPACE_TREE_ACTION_INSET, WORKSPACE_TREE_NODE_ICON_SIZE,
    sidebar_settings_button, workspace_layout_button, workspace_sidebar_attention,
    workspace_sidebar_controls, workspace_sidebar_status, workspace_strip_chip_connector,
    workspace_strip_group_separator, workspace_strip_session_badge, workspace_strip_window_pill,
    workspace_titlebar_strip, workspace_tree_disclosure, workspace_tree_marker, workspace_tree_row,
};
use zz_ui::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
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
                "The settings entry and workspace-layout menu sit at the leading end of the sidebar titlebar. The layout menu toggles the sidebar and splits the focused pane right or down. Without an active pane, it disables both split entries. The titlebar strip that replaces the sidebar keeps the pair on the same axis.",
                cx,
            )
            .child(
                specimens()
                    .child(specimen(
                        "layout menu · active pane",
                        layout_menu_button("nav-layout", true),
                        cx,
                    ))
                    .child(specimen(
                        "layout menu · no active pane",
                        layout_menu_button("nav-layout-disabled", false),
                        cx,
                    ))
                    .child(specimen(
                        "settings entry",
                        sidebar_settings_button("nav-settings"),
                        cx,
                    ))
                    .child(specimen(
                        "control cluster",
                        workspace_sidebar_controls(
                            sidebar_settings_button("nav-cluster-settings"),
                            layout_menu_button("nav-cluster-layout", true),
                        ),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Titlebar strip",
                "The other chrome: the sidebar column is gone and one titlebar-height strip spans the window, tmux-status-line style. Every session on the attached machine shows as a letter badge, and the attached session's windows follow as index:pane pills (one chain, dash-joined the way a status line writes it). The live chip takes the same fill a sidebar row takes, so the fleet reads the same in either chrome, and the text follows it: attached session and focused window at full strength, the rest dimmed. Window pills all take one width and ellipsize into their tooltip, because a pill is named after its active pane and a chain that hugged those names would reflow whenever a shell retitled itself; each reveals the tree's delete action under the pointer. The leading inset is dead space the macOS traffic lights sit in, and drag surface; the chrome controls follow it, on the axis the sidebar titlebar puts them, closed by a hairline that divides them from the chain. Content clips rather than wrapping (this height is the window's title bar) while the trailing status keeps its seat.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block("strip · at window width", strip_row(cx), cx))
                    .child(specimen(
                        "session badge · attached",
                        workspace_strip_session_badge("nav-strip-badge-on", "Z".into(), "zz", true, true, cx),
                        cx,
                    ))
                    .child(specimen(
                        "session badge · parked",
                        workspace_strip_session_badge("nav-strip-badge-off", "R".into(), "research", false, true, cx),
                        cx,
                    ))
                    .child(specimen(
                        "session badge · disconnected",
                        workspace_strip_session_badge("nav-strip-badge-down", "B".into(), "builds", false, false, cx),
                        cx,
                    ))
                    .child(specimen(
                        "window pill · focused",
                        workspace_strip_window_pill(
                            "nav-strip-pill-on",
                            "nav-strip-pill-on".into(),
                            "0:bash".into(),
                            true,
                            true,
                            strip_window_delete("nav-strip-pill-on-delete", cx),
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen(
                        "window pill · idle",
                        workspace_strip_window_pill(
                            "nav-strip-pill-off",
                            "nav-strip-pill-off".into(),
                            "1:claude".into(),
                            false,
                            true,
                            strip_window_delete("nav-strip-pill-off-delete", cx),
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen(
                        "controls separator",
                        workspace_strip_group_separator(cx),
                        cx,
                    ))
                    .child(specimen(
                        "chip connector",
                        workspace_strip_chip_connector(cx),
                        cx,
                    )),
            ),
        )
        .child(
            gallery(
                "Status section",
                "The sidebar's bottom section has two tenants: the agent attention rollup (which agents are blocked on the user, dead, or mid-turn, silent when every count is zero) and the auxiliary half of a tmux status line (the clock, script output) expanded by the daemon from status-left and status-right in .tmux.conf. One line; the tmux left half ellipsizes first so the ends stay legible.",
                cx,
            )
            .child(
                specimens()
                    .w_full()
                    .child(specimen_block(
                        "agent rollup",
                        sidebar_status_section(
                            "nav-status-attn",
                            Some(attention_rollup("nav-attn", cx)),
                            "",
                            "",
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen_block(
                        "rollup beside a tmux half",
                        sidebar_status_section(
                            "nav-status-attn-tmux",
                            Some(attention_rollup("nav-attn-tmux", cx)),
                            "",
                            "09:41 25-Jul-26",
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen_block(
                        "both tmux halves",
                        sidebar_status_section(
                            "nav-status-both",
                            None,
                            "batt 82%",
                            "09:41 25-Jul-26",
                            cx,
                        ),
                        cx,
                    ))
                    .child(specimen_block(
                        "a long left half ellipsizes",
                        sidebar_status_section(
                            "nav-status-long",
                            None,
                            "#(kubectl config current-context) staging-eu-west-1-primary",
                            "09:41",
                            cx,
                        ),
                        cx,
                    )),
            ),
        )
        .into_any_element()
}

fn sidebar_status_section(
    id: &'static str,
    attention: Option<AnyElement>,
    left: &'static str,
    right: &'static str,
    cx: &App,
) -> AnyElement {
    div()
        .w(px(256.0))
        .child(workspace_sidebar_status(
            id,
            attention,
            left.into(),
            right.into(),
            cx,
        ))
        .into_any_element()
}

fn attention_rollup(prefix: &'static str, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .child(workspace_sidebar_attention(
            (prefix, 0usize),
            "1 waiting".into(),
            cx.theme().warning,
            true,
            cx,
        ))
        .child(workspace_sidebar_attention(
            (prefix, 1usize),
            "1 failed".into(),
            cx.theme().danger,
            true,
            cx,
        ))
        .child(workspace_sidebar_attention(
            (prefix, 2usize),
            "2 running".into(),
            cx.theme().foreground.muted(),
            false,
            cx,
        ))
        .into_any_element()
}

fn strip_window_delete(id: &'static str, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .with_size(px(18.0))
        .icon(Icon::new(IconName::Delete).text_color(cx.theme().danger))
        .tooltip("Delete window")
}

fn layout_menu_button(id: &'static str, can_split: bool) -> impl IntoElement {
    workspace_layout_button(id).dropdown_menu(move |menu, _, _| {
        menu.item(PopupMenuItem::new("Toggle sidebar").icon(IconName::PanelLeft))
            .item(
                PopupMenuItem::new("Split right")
                    .icon(IconName::PanelRight)
                    .disabled(!can_split),
            )
            .item(
                PopupMenuItem::new("Split bottom")
                    .icon(IconName::PanelBottom)
                    .disabled(!can_split),
            )
    })
}

fn strip_row(cx: &App) -> AnyElement {
    let content = div()
        .flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .child(workspace_strip_session_badge(
            "nav-strip-row-s0",
            "Z".into(),
            "zz",
            true,
            true,
            cx,
        ))
        .child(workspace_strip_chip_connector(cx))
        .child(workspace_strip_session_badge(
            "nav-strip-row-s1",
            "R".into(),
            "research",
            false,
            true,
            cx,
        ))
        .child(workspace_strip_chip_connector(cx))
        .child(workspace_strip_window_pill(
            "nav-strip-row-w0",
            "nav-strip-row-w0".into(),
            "0:bash".into(),
            true,
            true,
            strip_window_delete("nav-strip-row-w0-delete", cx),
            cx,
        ))
        .child(workspace_strip_chip_connector(cx))
        .child(workspace_strip_window_pill(
            "nav-strip-row-w1",
            "nav-strip-row-w1".into(),
            "1:claude".into(),
            false,
            true,
            strip_window_delete("nav-strip-row-w1-delete", cx),
            cx,
        ))
        .child(workspace_strip_chip_connector(cx))
        .child(workspace_strip_window_pill(
            "nav-strip-row-w2",
            "nav-strip-row-w2".into(),
            "2:https://zed.dev".into(),
            false,
            true,
            strip_window_delete("nav-strip-row-w2-delete", cx),
            cx,
        ));
    let leading = div()
        .flex()
        .items_center()
        .gap(px(WORKSPACE_STRIP_GAP))
        .child(workspace_sidebar_controls(
            sidebar_settings_button("nav-strip-row-settings"),
            layout_menu_button("nav-strip-row-layout", true),
        ))
        .child(workspace_strip_group_separator(cx));
    let trailing = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(WORKSPACE_STRIP_GAP))
        .child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .text_color(cx.theme().foreground.muted())
                .child("09:41 25-Jul-26"),
        );
    workspace_titlebar_strip("nav-strip-row", leading, content, trailing, cx).into_any_element()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Host,
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
                RowKind::Host => 0,
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
        Some(expanded) => {
            workspace_tree_disclosure(marker, expanded, row_group.clone(), cx).into_any_element()
        }
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
                .child(
                    Icon::new(IconName::Close)
                        .xsmall()
                        .text_color(cx.theme().danger),
                )
                .into_any_element(),
        ),
    }
}

fn row_actions(row: &Row, group: &SharedString, cx: &App) -> AnyElement {
    let mut children = Vec::new();
    match row.kind {
        RowKind::Host => children.push(
            Button::new(format!("{}-menu", row.id))
                .ghost()
                .xsmall()
                .icon(IconName::Ellipsis)
                .tooltip("Host actions")
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, {
                    let connected = row.connected;
                    move |menu, _, _| {
                        menu.item(PopupMenuItem::new("Close host"))
                            .item(PopupMenuItem::new("New session").disabled(!connected))
                            .item(PopupMenuItem::new("Add host"))
                    }
                })
                .into_any_element(),
        ),
        RowKind::Session | RowKind::Window => children.push(
            Button::new(format!("{}-add", row.id))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .disabled(!row.connected)
                .tooltip(if row.kind == RowKind::Session {
                    "New window"
                } else {
                    "New pane"
                })
                .into_any_element(),
        ),
        RowKind::Pane => {}
    }
    if row.kind != RowKind::Host {
        children.push(
            Button::new(format!("{}-delete", row.id))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::Delete).text_color(cx.theme().danger))
                .tooltip("Delete")
                .disabled(!row.connected)
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
        .when(row.kind != RowKind::Host, |this| {
            this.invisible()
                .group_hover(group.clone(), gpui::Styled::visible)
        })
        .children(children)
        .into_any_element()
}
