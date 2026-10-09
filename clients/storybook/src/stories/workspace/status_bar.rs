use zz_client::AgentAttentionStatus;
use zz_gpui::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
    prelude::*, px,
};
use zz_protocol::{AgentProvider, PaneKindSnapshot};
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, h_flex,
    navigation::{
        WorkspaceStatusWindowState,
        status::{
            StatusAgentEntry, StatusPaneEntry, StatusSessionEntry, StatusWindowActions,
            StatusWindowEntry, status_agents, status_pane_deck, status_session, status_window,
            status_window_overflow,
        },
        workspace_status_item,
    },
    pane::agent_provider_icon,
    shell::{WorkspaceStatusSlots, workspace_status_bar},
};

use super::{
    fixtures::{agent_kind, browser_kind, editor_kind, noop, status_pane, workspace_panes},
    tree::chrome_controls,
};
use crate::story::{Section, Story, row, stateless, states};

pub const STORY: Story = Story {
    id: "status-bar",
    name: "Status bar",
    group: "Workspace",
    summary: "The titlebar that replaces the sidebar when it is hidden: session menu, window pills with their pane decks, agent activity and host items.",
    sections: &[
        Section {
            id: "bar",
            name: "Status bar",
            summary: "workspace_status_bar composed the way the thin client does it. Without pane gaps it draws a bottom rule; with gaps it floats.",
            build: |_, cx| stateless(bars, cx),
        },
        Section {
            id: "session",
            name: "Session menu",
            summary: "status_session opens the session list. It disables itself while the client is disconnected.",
            build: |_, cx| stateless(session, cx),
        },
        Section {
            id: "windows",
            name: "Windows",
            summary: "status_window: index, pane deck, name, then bell and activity dots. The close button appears on hover, and right-click offers rename and close.",
            build: |_, cx| stateless(windows, cx),
        },
        Section {
            id: "pane-deck",
            name: "Pane deck",
            summary: "status_pane_deck stacks up to three pane cards and counts the rest. The active card is lifted and always painted on top, even past the third slot.",
            build: |_, cx| stateless(pane_deck, cx),
        },
        Section {
            id: "agents",
            name: "Agent activity",
            summary: "status_agents summarizes every agent pane by the most urgent state: needs input, then failed, then running, then idle.",
            build: |_, cx| stateless(agents, cx),
        },
        Section {
            id: "overflow",
            name: "Window overflow",
            summary: "Past five windows the strip shows a window around the active one and an overflow menu with all of them.",
            build: |_, cx| stateless(overflow, cx),
        },
    ],
};

const WINDOWS: [(&str, bool, bool, bool); 5] = [
    ("editor", true, false, false),
    ("agents", false, true, true),
    ("logs", false, true, false),
    ("server", false, false, true),
    ("scratch", false, false, false),
];

fn window_panes(index: usize) -> Vec<StatusPaneEntry> {
    match index {
        0 => workspace_panes(),
        1 => vec![
            status_pane(
                11,
                "Rework status bar",
                agent_kind(AgentProvider::Codex),
                true,
            ),
            status_pane(
                12,
                "Fix flaky daemon test",
                agent_kind(AgentProvider::ClaudeCode),
                false,
            ),
        ],
        2 => vec![status_pane(
            21,
            "daemon logs",
            PaneKindSnapshot::Terminal,
            true,
        )],
        3 => vec![
            status_pane(31, "just site", PaneKindSnapshot::Terminal, true),
            status_pane(
                32,
                "localhost:4321",
                browser_kind("http://localhost:4321/"),
                false,
            ),
            status_pane(
                33,
                "index.astro",
                editor_kind("site/src/pages/index.astro"),
                false,
            ),
        ],
        _ => vec![status_pane(41, "zsh", PaneKindSnapshot::Terminal, true)],
    }
}

fn actions(closable: bool) -> StatusWindowActions {
    StatusWindowActions {
        select: noop(),
        close: closable.then(noop),
        rename: closable.then(|| (SharedString::from("Rename window…"), noop())),
    }
}

fn window(
    index: usize,
    name: &'static str,
    state: WorkspaceStatusWindowState,
    panes: Vec<StatusPaneEntry>,
    cx: &App,
) -> AnyElement {
    status_window(
        ("window", index),
        index.to_string().into(),
        name.into(),
        state,
        panes,
        actions(state.connected),
        cx,
    )
}

fn session_entries() -> Vec<StatusSessionEntry> {
    [("main · 5 windows", true), ("scratch · 1 window", false)]
        .into_iter()
        .map(|(label, active)| StatusSessionEntry {
            label: label.into(),
            active,
            select: noop(),
        })
        .collect()
}

fn agent(
    label: &str,
    provider: AgentProvider,
    status: Option<AgentAttentionStatus>,
) -> StatusAgentEntry {
    StatusAgentEntry {
        label: label.to_owned().into(),
        window_name: "agents".into(),
        icon: agent_provider_icon(provider),
        status,
        select: noop(),
    }
}

fn agent_entries() -> Vec<StatusAgentEntry> {
    vec![
        agent(
            "Rework status bar",
            AgentProvider::Codex,
            Some(AgentAttentionStatus::Working),
        ),
        agent(
            "Review sidebar",
            AgentProvider::ClaudeCode,
            Some(AgentAttentionStatus::NeedsInput),
        ),
        agent(
            "Write release notes",
            AgentProvider::ClaudeCode,
            Some(AgentAttentionStatus::Idle),
        ),
    ]
}

fn update_item(cx: &App) -> AnyElement {
    workspace_status_item("update", None, "v0.17.0".into(), cx)
        .flex_none()
        .px(px(6.0))
        .rounded(cx.theme().control_radius())
        .child(
            div()
                .flex_none()
                .size(px(5.0))
                .rounded_full()
                .bg(cx.theme().success),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
pub struct Bar {
    pub gaps: bool,
    pub connected: bool,
    pub session: bool,
    pub agents: bool,
    pub host: bool,
    pub windows: usize,
}

impl Bar {
    pub const FULL: Self = Self {
        gaps: false,
        connected: true,
        session: true,
        agents: true,
        host: true,
        windows: 3,
    };
}

pub fn titlebar(config: &Bar, cx: &App) -> zz_gpui::Stateful<zz_gpui::Div> {
    let state = |index: usize| {
        let (_, active, bell, activity) = WINDOWS[index % WINDOWS.len()];
        WorkspaceStatusWindowState {
            connected: config.connected,
            active,
            bell,
            activity,
        }
    };
    let mut windows = (0..config.windows.min(5))
        .map(|index| {
            window(
                index,
                WINDOWS[index].0,
                state(index),
                window_panes(index),
                cx,
            )
        })
        .collect::<Vec<_>>();
    if config.windows > 5 {
        windows.push(status_window_overflow(
            "overflow",
            (0..config.windows)
                .map(|index| StatusWindowEntry {
                    label: format!("{index} window-{index}").into(),
                    active: index == 0,
                    select: noop(),
                })
                .collect(),
            config.connected,
            cx,
        ));
    }
    let mut right = Vec::new();
    if config.agents {
        let mut agents = agent_entries();
        if !config.connected {
            for agent in &mut agents {
                agent.status = None;
            }
        }
        right.push(status_agents("agents", agents, config.connected, cx));
    }
    if config.host {
        right.push(
            workspace_status_item("host", Some(IconName::Globe), "devbox".into(), cx)
                .into_any_element(),
        );
        right.push(update_item(cx));
    }
    let session = config.session.then(|| {
        status_session(
            "session",
            "main".into(),
            session_entries(),
            config.connected,
            cx,
        )
    });
    workspace_status_bar(
        config.gaps,
        px(8.0),
        WorkspaceStatusSlots {
            session,
            windows,
            right,
            titlebar_controls: Some((chrome_controls(false).into_any_element(), px(56.0))),
            window_controls: None,
        },
        cx,
    )
}

fn bar(id: &'static str, config: &Bar, cx: &App) -> AnyElement {
    div()
        .id(id)
        .w_full()
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border())
        .bg(cx.theme().background.opaque())
        .child(titlebar(config, cx))
        .into_any_element()
}

fn bars(_: &mut Window, cx: &mut App) -> AnyElement {
    let full = Bar::FULL;
    states()
        .state("no gaps: a rule under the bar", bar("bar-flush", &full, cx))
        .state("gaps", bar("bar-gaps", &Bar { gaps: true, ..full }, cx))
        .state(
            "eight windows: five around the active one, the rest in the overflow menu",
            bar(
                "bar-overflow",
                &Bar {
                    windows: 8,
                    host: false,
                    ..full
                },
                cx,
            ),
        )
        .state(
            "session and agents hidden in settings",
            bar(
                "bar-minimal",
                &Bar {
                    session: false,
                    agents: false,
                    ..full
                },
                cx,
            ),
        )
        .state(
            "disconnected",
            bar(
                "bar-disconnected",
                &Bar {
                    connected: false,
                    host: false,
                    ..full
                },
                cx,
            ),
        )
        .into_any_element()
}

fn session(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .columns(3)
        .state(
            "connected",
            div().id("session-a").flex().child(status_session(
                "session",
                "main".into(),
                session_entries(),
                true,
                cx,
            )),
        )
        .state(
            "disconnected",
            div().id("session-b").flex().child(status_session(
                "session",
                "main".into(),
                session_entries(),
                false,
                cx,
            )),
        )
        .state(
            "long name",
            div().id("session-c").flex().child(status_session(
                "session",
                "fabrico-release-candidate-0.17".into(),
                session_entries(),
                true,
                cx,
            )),
        )
        .into_any_element()
}

fn windows(_: &mut Window, cx: &mut App) -> AnyElement {
    let connected = WorkspaceStatusWindowState {
        connected: true,
        ..Default::default()
    };
    states()
        .columns(2)
        .state(
            "active, six panes",
            window(
                0,
                "editor",
                WorkspaceStatusWindowState {
                    active: true,
                    ..connected
                },
                window_panes(0),
                cx,
            ),
        )
        .state(
            "bell and activity",
            window(
                1,
                "agents",
                WorkspaceStatusWindowState {
                    bell: true,
                    activity: true,
                    ..connected
                },
                window_panes(1),
                cx,
            ),
        )
        .state(
            "one pane",
            window(2, "logs", connected, window_panes(2), cx),
        )
        .state(
            "no panes yet",
            window(3, "server", connected, Vec::new(), cx),
        )
        .state(
            "disconnected",
            window(
                4,
                "scratch",
                WorkspaceStatusWindowState::default(),
                window_panes(3),
                cx,
            ),
        )
        .state(
            "long name",
            window(
                5,
                "release-candidate-integration",
                connected,
                window_panes(4),
                cx,
            ),
        )
        .into_any_element()
}

fn deck(id: &'static str, panes: Vec<StatusPaneEntry>, connected: bool, cx: &App) -> AnyElement {
    div()
        .id(id)
        .flex()
        .h(px(26.0))
        .items_center()
        .children(status_pane_deck(id, panes, connected, cx))
        .into_any_element()
}

fn active_at(index: usize) -> Vec<StatusPaneEntry> {
    let mut panes = workspace_panes();
    for (position, pane) in panes.iter_mut().enumerate() {
        pane.active = position == index;
    }
    panes
}

fn pane_deck(_: &mut Window, cx: &mut App) -> AnyElement {
    states()
        .columns(4)
        .state("one pane", deck("deck-1", window_panes(2), true, cx))
        .state("two panes", deck("deck-2", window_panes(1), true, cx))
        .state("three panes", deck("deck-3", window_panes(3), true, cx))
        .state(
            "six, first active",
            deck("deck-6-first", active_at(0), true, cx),
        )
        .state(
            "six, second active",
            deck("deck-6-second", active_at(1), true, cx),
        )
        .state(
            "six, sixth active",
            deck("deck-6-last", active_at(5), true, cx),
        )
        .state("disconnected", deck("deck-off", active_at(1), false, cx))
        .state(
            "every pane kind",
            h_flex().gap_2().children(
                [
                    ("terminal", PaneKindSnapshot::Terminal),
                    ("browser", browser_kind("https://zed.dev")),
                    ("codex", agent_kind(AgentProvider::Codex)),
                    ("claude", agent_kind(AgentProvider::ClaudeCode)),
                    ("editor", editor_kind("README.md")),
                    ("picker", PaneKindSnapshot::Picker),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (label, kind))| {
                    deck(
                        ["kind-0", "kind-1", "kind-2", "kind-3", "kind-4", "kind-5"][index],
                        vec![status_pane(index as u64, label, kind, true)],
                        true,
                        cx,
                    )
                }),
            ),
        )
        .into_any_element()
}

fn agents(_: &mut Window, cx: &mut App) -> AnyElement {
    use AgentAttentionStatus::{Failed, Idle, Working};
    let single = |status| vec![agent("Rework status bar", AgentProvider::Codex, status)];
    let summary = |id: &'static str, agents: Vec<StatusAgentEntry>, connected: bool| {
        div()
            .id(id)
            .flex()
            .child(status_agents(id, agents, connected, cx))
            .into_any_element()
    };
    states()
        .columns(3)
        .state(
            "needs input outranks the rest",
            summary("agents-mixed", agent_entries(), true),
        )
        .state(
            "failed",
            summary(
                "agents-failed",
                vec![
                    agent(
                        "Fix flaky daemon test",
                        AgentProvider::ClaudeCode,
                        Some(Failed),
                    ),
                    agent("Rework status bar", AgentProvider::Codex, Some(Working)),
                ],
                true,
            ),
        )
        .state(
            "running",
            summary("agents-running", single(Some(Working)), true),
        )
        .state(
            "all idle",
            summary(
                "agents-idle",
                vec![
                    agent("Rework status bar", AgentProvider::Codex, Some(Idle)),
                    agent("Write release notes", AgentProvider::ClaudeCode, Some(Idle)),
                ],
                true,
            ),
        )
        .state(
            "state unknown while connecting",
            summary("agents-unknown", single(None), true),
        )
        .state(
            "disconnected",
            summary("agents-disconnected", single(None), false),
        )
        .into_any_element()
}

fn overflow(_: &mut Window, cx: &mut App) -> AnyElement {
    let entries = || {
        (0..8)
            .map(|index| StatusWindowEntry {
                label: format!("{index} window-{index}").into(),
                active: index == 2,
                select: noop(),
            })
            .collect::<Vec<_>>()
    };
    states()
        .state(
            "connected and disconnected",
            row()
                .child(div().id("overflow-on").child(status_window_overflow(
                    "overflow",
                    entries(),
                    true,
                    cx,
                )))
                .child(div().id("overflow-off").child(status_window_overflow(
                    "overflow",
                    entries(),
                    false,
                    cx,
                ))),
        )
        .into_any_element()
}
