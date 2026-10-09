use std::rc::Rc;

use zz_gpui::{
    AnyElement, App, IntoElement, ListSizingBehavior, ParentElement as _, SharedString,
    Styled as _, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Selectable as _, Sizable as _, h_flex,
    navigation::{
        WORKSPACE_SIDEBAR_DEFAULT_WIDTH, WorkspaceStatusWindowState,
        sidebar::{
            tree_action_strip, tree_host_indicator, tree_host_marker, tree_node_marker,
            tree_window_layout_button,
        },
        workspace_layout_button, workspace_palette_button, workspace_settings_button,
        workspace_sidebar_titlebar, workspace_status_item, workspace_status_window,
        workspace_tree_action_button, workspace_tree_action_row, workspace_tree_disclosure,
        workspace_tree_marker, workspace_tree_row,
    },
};

use super::{
    fixtures::stateful,
    tree::{
        Badge, Node, TreeState, chrome_controls, guides, host_only, render_node, sidebar,
        tree_list, workspace_tree,
    },
};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "sidebar",
    name: "Sidebar",
    group: "Workspace",
    summary: "The workspace sidebar: the host, session, window and pane tree with its markers, badges, indent guides and row actions, plus the titlebar controls and window pills.",
    sections: &[
        Section {
            id: "sidebar",
            name: "Sidebar",
            summary: "workspace_sidebar_surface around the tree the thin client builds from a mux snapshot. Unfocused, the active pane is bold; with keyboard focus it fills and the cursor gets its own fill; disconnected, the host warns and every row goes muted.",
            build: |window, cx| stateful(window, cx, |_, _| sidebar_scrolls(), sidebars),
        },
        Section {
            id: "tree-rows",
            name: "Tree rows",
            summary: "workspace_tree_row in each state. Actions on session, window and pane rows stay hidden until the pointer is over the row; the host row always shows its own.",
            build: |_, cx| stateless(tree_rows, cx),
        },
        Section {
            id: "markers",
            name: "Markers and badges",
            summary: "The icon slot at the start of each row: the zz host logo, node icons, the bell dot at top right and the agent badge at bottom right.",
            build: |_, cx| stateless(markers, cx),
        },
        Section {
            id: "row-actions",
            name: "Disclosure and actions",
            summary: "An expandable row swaps its icon for a chevron on hover. Trailing actions sit in a strip that swallows clicks so the row does not activate.",
            build: |_, cx| stateless(row_actions, cx),
        },
        Section {
            id: "indent-guides",
            name: "Indent guides",
            summary: "WorkspaceIndentGuides drawn as a uniform_list decoration. Only the guide of the active row's nearest ancestor lights up.",
            build: |window, cx| stateful(window, cx, |_, _| sidebar_scrolls(), indent_guides),
        },
        Section {
            id: "chrome-controls",
            name: "Titlebar controls",
            summary: "The settings, sidebar and palette buttons at the top of the sidebar, or at the start of the status bar when the sidebar is hidden.",
            build: |_, cx| stateless(chrome, cx),
        },
        Section {
            id: "window-pills",
            name: "Window pills",
            summary: "workspace_status_window and workspace_status_item, the pieces the status bar is made of. Hovering a connected pill fills it.",
            build: |_, cx| stateless(window_pills, cx),
        },
    ],
};

fn sidebar_scrolls() -> [UniformListScrollHandle; 3] {
    std::array::from_fn(|_| UniformListScrollHandle::new())
}

fn sidebar_frame(
    id: &'static str,
    nodes: Vec<Node>,
    state: TreeState,
    scroll: &UniformListScrollHandle,
    cx: &App,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .h(px(470.0))
        .child(sidebar(tree_list(nodes, state, scroll, cx), false, cx))
}

fn sidebars(scrolls: &[UniformListScrollHandle; 3], _: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .columns(3)
        .state(
            "unfocused",
            sidebar_frame(
                "sidebar-unfocused",
                workspace_tree(),
                TreeState::CONNECTED,
                &scrolls[0],
                cx,
            ),
        )
        .state(
            "focused, cursor on Review sidebar",
            sidebar_frame(
                "sidebar-focused",
                workspace_tree(),
                TreeState {
                    focused: true,
                    selected: Some("review"),
                    ..TreeState::CONNECTED
                },
                &scrolls[1],
                cx,
            ),
        )
        .state(
            "disconnected",
            sidebar_frame(
                "sidebar-disconnected",
                host_only(),
                TreeState {
                    connected: false,
                    ..TreeState::CONNECTED
                },
                &scrolls[2],
                cx,
            ),
        )
        .into_any_element()
}

fn row_box(id: impl Into<SharedString>, width: f32, content: impl IntoElement) -> AnyElement {
    div()
        .id(zz_gpui::ElementId::Name(id.into()))
        .w(px(width))
        .child(content)
        .into_any_element()
}

fn single(node: &Node, active: bool, state: TreeState, width: f32, cx: &App) -> AnyElement {
    row_box(
        format!("single-{}-{}", node.key, width as u32),
        width,
        render_node(node, active, state, cx),
    )
}

fn tree_rows(_: &mut Window, cx: &mut App) -> AnyElement {
    let nodes = workspace_tree();
    let find = |key: &str| {
        nodes
            .iter()
            .find(|node| node.key == key)
            .expect("fixture node")
    };
    let focused = TreeState {
        focused: true,
        ..TreeState::CONNECTED
    };
    let disconnected = TreeState {
        connected: false,
        ..TreeState::CONNECTED
    };
    states()
        .columns(2)
        .state(
            "rest",
            row_box(
                "rest",
                300.0,
                render_node(find("docs"), false, TreeState::CONNECTED, cx),
            ),
        )
        .state(
            "on the active path, sidebar unfocused: bold, no fill",
            row_box(
                "active",
                300.0,
                render_node(find("nvim"), true, TreeState::CONNECTED, cx),
            ),
        )
        .state(
            "active, sidebar focused: filled",
            row_box(
                "active-focused",
                300.0,
                render_node(find("nvim"), true, focused, cx),
            ),
        )
        .state(
            "keyboard cursor",
            row_box(
                "selected",
                300.0,
                render_node(
                    find("review"),
                    false,
                    TreeState {
                        selected: Some("review"),
                        ..focused
                    },
                    cx,
                ),
            ),
        )
        .state(
            "disconnected: muted, actions disabled",
            row_box(
                "disconnected",
                300.0,
                render_node(find("editor"), false, disconnected, cx),
            ),
        )
        .state(
            "host: actions always visible",
            row_box(
                "host",
                300.0,
                render_node(find("host"), false, TreeState::CONNECTED, cx),
            ),
        )
        .state(
            "host while connecting",
            row_box(
                "host-connecting",
                300.0,
                render_node(
                    &host_only()[0],
                    false,
                    TreeState {
                        connected: false,
                        connecting: true,
                        ..TreeState::CONNECTED
                    },
                    cx,
                ),
            ),
        )
        .state(
            "long label at 220px",
            single(find("flaky"), false, TreeState::CONNECTED, 220.0, cx),
        )
        .state(
            "depth 0 to 3",
            div()
                .flex()
                .flex_col()
                .children(["host", "main", "editor", "nvim"].map(|key| {
                    row_box(
                        format!("depth-{key}"),
                        300.0,
                        render_node(find(key), false, TreeState::CONNECTED, cx),
                    )
                })),
        )
        .into_any_element()
}

fn labeled(label: &'static str, item: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .min_w(px(64.0))
        .child(
            div()
                .flex()
                .h(px(32.0))
                .items_center()
                .justify_center()
                .child(item),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(label),
        )
        .into_any_element()
}

fn node_icon(icon: IconName, cx: &App) -> Icon {
    Icon::new(icon).small().text_color(cx.theme().foreground)
}

fn markers(_: &mut Window, cx: &mut App) -> AnyElement {
    let badge = |badge: Badge| Some(badge.color(cx));
    states()
        .state(
            "host",
            row()
                .child(labeled("rest", tree_host_marker(false, None, cx), cx))
                .child(labeled("bell", tree_host_marker(true, None, cx), cx))
                .child(labeled(
                    "needs input",
                    tree_host_marker(false, badge(Badge::NeedsInput), cx),
                    cx,
                ))
                .child(labeled(
                    "both",
                    tree_host_marker(true, badge(Badge::NeedsInput), cx),
                    cx,
                )),
        )
        .state(
            "nodes",
            row().children(
                [
                    ("session", IconName::Layers),
                    ("window", IconName::AppWindow),
                    ("terminal", IconName::SquareTerminal),
                    ("claude", IconName::Claude),
                    ("codex", IconName::Openai),
                    ("browser", IconName::Globe),
                    ("editor", IconName::File),
                    ("picker", IconName::Plus),
                ]
                .map(|(label, icon)| {
                    labeled(
                        label,
                        tree_node_marker(node_icon(icon, cx), false, None, cx),
                        cx,
                    )
                }),
            ),
        )
        .state(
            "agent badges and the bell",
            row()
                .child(labeled(
                    "needs input",
                    tree_node_marker(
                        node_icon(IconName::Claude, cx),
                        false,
                        badge(Badge::NeedsInput),
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "failed",
                    tree_node_marker(
                        node_icon(IconName::Claude, cx),
                        false,
                        badge(Badge::Failed),
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "working",
                    tree_node_marker(
                        node_icon(IconName::Openai, cx),
                        false,
                        badge(Badge::Working),
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "finished, unseen",
                    tree_node_marker(
                        node_icon(IconName::Claude, cx),
                        false,
                        badge(Badge::Finished),
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "bell",
                    tree_node_marker(node_icon(IconName::SquareTerminal, cx), true, None, cx),
                    cx,
                ))
                .child(labeled(
                    "bell and badge",
                    tree_node_marker(
                        node_icon(IconName::AppWindow, cx),
                        true,
                        badge(Badge::Failed),
                        cx,
                    ),
                    cx,
                )),
        )
        .state(
            "host connection indicator",
            row()
                .child(labeled(
                    "connecting",
                    tree_host_indicator("indicator-connecting", true, None, cx),
                    cx,
                ))
                .child(labeled(
                    "lost, detail in tooltip",
                    tree_host_indicator(
                        "indicator-lost",
                        false,
                        Some("Connection lost: devbox closed the ssh session".into()),
                        cx,
                    ),
                    cx,
                )),
        )
        .into_any_element()
}

fn row_actions(_: &mut Window, cx: &mut App) -> AnyElement {
    let marker = || tree_node_marker(node_icon(IconName::AppWindow, cx), false, None, cx);
    states()
        .state(
            "disclosure: expanded and collapsed, chevron on hover",
            row()
                .child(labeled(
                    "expanded",
                    workspace_tree_disclosure(
                        "disclosure-open",
                        marker(),
                        true,
                        "group-open".into(),
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "collapsed",
                    workspace_tree_disclosure(
                        "disclosure-closed",
                        marker(),
                        false,
                        "group-closed".into(),
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "bare slot",
                    workspace_tree_marker(node_icon(IconName::Layers, cx)),
                    cx,
                )),
        )
        .state(
            "action strip",
            row()
                .child(labeled(
                    "session",
                    tree_action_strip(
                        "strip-session",
                        vec![
                            workspace_tree_action_button(
                                "add",
                                IconName::Plus,
                                "New window",
                                false,
                                cx,
                            )
                            .into_any_element(),
                            workspace_tree_action_button(
                                "close",
                                IconName::Xmark,
                                "Close session",
                                false,
                                cx,
                            )
                            .into_any_element(),
                        ],
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "window",
                    tree_action_strip(
                        "strip-window",
                        vec![
                            tree_window_layout_button("layout", false, |_, _, _| {}, cx),
                            workspace_tree_action_button(
                                "close",
                                IconName::Xmark,
                                "Close window",
                                false,
                                cx,
                            )
                            .into_any_element(),
                        ],
                        cx,
                    ),
                    cx,
                ))
                .child(labeled(
                    "read-only",
                    tree_action_strip(
                        "strip-disabled",
                        vec![
                            tree_window_layout_button("layout", true, |_, _, _| {}, cx),
                            workspace_tree_action_button(
                                "close",
                                IconName::Xmark,
                                "Close window",
                                true,
                                cx,
                            )
                            .into_any_element(),
                        ],
                        cx,
                    ),
                    cx,
                )),
        )
        .state(
            "action row",
            div()
                .id("action-row")
                .w(px(300.0))
                .child(workspace_tree_action_row(
                    "new-session",
                    1,
                    IconName::Plus,
                    "New session",
                    cx,
                )),
        )
        .into_any_element()
}

const GUIDE_DEPTHS: [(u8, IconName, &str); 11] = [
    (0, IconName::HardDrive, "zz daemon"),
    (1, IconName::Layers, "main"),
    (2, IconName::AppWindow, "editor"),
    (3, IconName::SquareTerminal, "nvim"),
    (3, IconName::Claude, "Review sidebar"),
    (3, IconName::Globe, "zed.dev"),
    (2, IconName::AppWindow, "agents"),
    (3, IconName::Openai, "Rework status bar"),
    (1, IconName::Layers, "scratch"),
    (2, IconName::AppWindow, "shell"),
    (3, IconName::SquareTerminal, "zsh"),
];

fn guide_list(
    id: &'static str,
    active_row: Option<usize>,
    scroll: &UniformListScrollHandle,
    cx: &App,
) -> AnyElement {
    let depths: Rc<[usize]> = GUIDE_DEPTHS
        .iter()
        .map(|(depth, _, _)| usize::from(*depth))
        .collect();
    let list = uniform_list("guide-rows", GUIDE_DEPTHS.len(), move |range, _, cx| {
        range
            .map(|index| {
                let (depth, icon, label) = GUIDE_DEPTHS[index].clone();
                let active = active_row == Some(index);
                workspace_tree_row(
                    ("guide-row", index),
                    depth,
                    active,
                    false,
                    false,
                    true,
                    true,
                    true,
                    format!("guide-group-{index}").into(),
                    tree_node_marker(
                        Icon::new(icon)
                            .small()
                            .text_color(cx.theme().foreground.muted()),
                        false,
                        None,
                        cx,
                    ),
                    div().child(label),
                    div(),
                    cx,
                )
                .into_any_element()
            })
            .collect::<Vec<_>>()
    })
    .size_full()
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .track_scroll(scroll)
    .with_decoration(guides(depths, active_row, cx));
    div()
        .id(id)
        .w(px(WORKSPACE_SIDEBAR_DEFAULT_WIDTH))
        .h(px(32.0 * GUIDE_DEPTHS.len() as f32))
        .child(list)
        .into_any_element()
}

fn indent_guides(
    scrolls: &[UniformListScrollHandle; 3],
    _: &mut Window,
    cx: &mut App,
) -> AnyElement {
    states()
        .columns(3)
        .state(
            "active row: Review sidebar",
            guide_list("guides-review", Some(4), &scrolls[0], cx),
        )
        .state(
            "active row: zsh, two sessions down",
            guide_list("guides-zsh", Some(10), &scrolls[1], cx),
        )
        .state(
            "no active row while disconnected",
            guide_list("guides-none", None, &scrolls[2], cx),
        )
        .into_any_element()
}

fn chrome(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .state(
            "buttons",
            row()
                .child(labeled(
                    "settings",
                    workspace_settings_button("settings"),
                    cx,
                ))
                .child(labeled(
                    "settings open",
                    workspace_settings_button("settings-open").selected(true),
                    cx,
                ))
                .child(labeled("sidebar", workspace_layout_button("layout"), cx))
                .child(labeled(
                    "palette (iOS)",
                    workspace_palette_button("palette"),
                    cx,
                )),
        )
        .state(
            "sidebar titlebar",
            div()
                .id("titlebar")
                .w(px(WORKSPACE_SIDEBAR_DEFAULT_WIDTH))
                .border_1()
                .border_color(cx.theme().border())
                .rounded(cx.theme().radius)
                .child(workspace_sidebar_titlebar(
                    "sidebar-titlebar",
                    chrome_controls(false),
                    cx,
                )),
        )
        .state(
            "sidebar titlebar on the settings route",
            div()
                .id("titlebar-settings")
                .w(px(WORKSPACE_SIDEBAR_DEFAULT_WIDTH))
                .border_1()
                .border_color(cx.theme().border())
                .rounded(cx.theme().radius)
                .child(workspace_sidebar_titlebar(
                    "sidebar-titlebar",
                    chrome_controls(true),
                    cx,
                )),
        )
        .into_any_element()
}

fn pill(
    id: &'static str,
    index: &'static str,
    name: &'static str,
    state: WorkspaceStatusWindowState,
    cx: &App,
) -> AnyElement {
    workspace_status_window(
        id,
        index.into(),
        name.into(),
        format!("{index}:{name}").into(),
        state,
        None,
        cx,
    )
    .into_any_element()
}

fn window_pills(_: &mut Window, cx: &mut App) -> AnyElement {
    let connected = WorkspaceStatusWindowState {
        connected: true,
        ..Default::default()
    };
    states()
        .state(
            "window pills",
            h_flex()
                .flex_wrap()
                .gap_2()
                .child(pill(
                    "pill-active",
                    "0",
                    "editor",
                    WorkspaceStatusWindowState {
                        active: true,
                        ..connected
                    },
                    cx,
                ))
                .child(pill("pill-rest", "1", "agents", connected, cx))
                .child(pill(
                    "pill-bell",
                    "2",
                    "logs",
                    WorkspaceStatusWindowState {
                        bell: true,
                        ..connected
                    },
                    cx,
                ))
                .child(pill(
                    "pill-activity",
                    "3",
                    "server",
                    WorkspaceStatusWindowState {
                        activity: true,
                        ..connected
                    },
                    cx,
                ))
                .child(pill(
                    "pill-both",
                    "4",
                    "ci",
                    WorkspaceStatusWindowState {
                        bell: true,
                        activity: true,
                        ..connected
                    },
                    cx,
                ))
                .child(pill(
                    "pill-disconnected",
                    "5",
                    "offline",
                    WorkspaceStatusWindowState::default(),
                    cx,
                ))
                .child(pill(
                    "pill-long",
                    "6",
                    "a window whose name is far too long for the pill",
                    connected,
                    cx,
                )),
        )
        .state(
            "status items",
            h_flex()
                .gap_4()
                .child(workspace_status_item(
                    "item-host",
                    Some(IconName::Globe),
                    "devbox".into(),
                    cx,
                ))
                .child(workspace_status_item(
                    "item-plain",
                    None,
                    "v0.16.0".into(),
                    cx,
                ))
                .child(workspace_status_item(
                    "item-long",
                    Some(IconName::HardDrive),
                    "fabrico@build-server-eu-west-3.internal".into(),
                    cx,
                )),
        )
        .into_any_element()
}
