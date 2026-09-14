use zz_protocol::{AgentProvider, MuxSnapshot, PaneId, PaneKindSnapshot, SessionId, WindowId};

use crate::navigation::{ordered_panes, pane_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusBarSettings {
    pub show_session: bool,
    pub badges: bool,
    pub show_agents: bool,
    pub show_host: bool,
    pub show_update: bool,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self {
            show_session: true,
            badges: true,
            show_agents: true,
            show_host: true,
            show_update: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarPane {
    pub id: PaneId,
    pub label: String,
    pub kind: PaneKindSnapshot,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarWindow {
    pub id: WindowId,
    pub index: u32,
    pub name: String,
    pub label: String,
    pub active: bool,
    pub bell: bool,
    pub activity: bool,
    pub agent_provider: Option<AgentProvider>,
    pub panes: Vec<StatusBarPane>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarAgent {
    pub id: PaneId,
    pub label: String,
    pub window_name: String,
    pub provider: AgentProvider,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarModel {
    pub session_name: Option<String>,
    pub windows: Vec<StatusBarWindow>,
    pub agents: Vec<StatusBarAgent>,
    pub host_name: Option<String>,
    pub show_update: bool,
}

impl StatusBarModel {
    #[must_use]
    pub fn from_snapshot(
        snapshot: &MuxSnapshot,
        attached_session: Option<SessionId>,
        host_name: Option<&str>,
        settings: StatusBarSettings,
    ) -> Self {
        let session = attached_session.and_then(|attached| {
            snapshot
                .sessions
                .iter()
                .find(|session| session.id == attached)
        });
        let focused_window = session.map(|session| snapshot.focused_window_for(session));
        let mut agents = Vec::new();
        let windows = session.map_or_else(Vec::new, |session| {
            session
                .windows
                .iter()
                .map(|window| {
                    let panes = ordered_panes(window);
                    let bell = panes.iter().any(|pane| pane.bell);
                    if settings.show_agents {
                        agents.extend(panes.iter().filter_map(|pane| {
                            let PaneKindSnapshot::Agent(agent) = &pane.kind else {
                                return None;
                            };
                            (!pane.dead).then(|| StatusBarAgent {
                                id: pane.id,
                                label: pane_label(pane),
                                window_name: window.name.clone(),
                                provider: agent.provider,
                            })
                        }));
                    }
                    let active_agent = window.panes.get(&window.active_pane).and_then(|pane| {
                        if let PaneKindSnapshot::Agent(agent) = &pane.kind {
                            Some((pane, agent.provider))
                        } else {
                            None
                        }
                    });
                    let agent_provider = active_agent.map(|(_, provider)| provider).or_else(|| {
                        panes.iter().find_map(|pane| {
                            if let PaneKindSnapshot::Agent(agent) = &pane.kind {
                                Some(agent.provider)
                            } else {
                                None
                            }
                        })
                    });
                    StatusBarWindow {
                        id: window.id,
                        index: window.index,
                        name: window.name.clone(),
                        label: window
                            .panes
                            .get(&window.active_pane)
                            .filter(|pane| {
                                window.automatic_rename
                                    && matches!(
                                        pane.kind,
                                        PaneKindSnapshot::Agent(_) | PaneKindSnapshot::Browser(_)
                                    )
                            })
                            .map_or_else(|| window.name.clone(), pane_label),
                        active: focused_window == Some(window.id),
                        bell: settings.badges && bell,
                        activity: settings.badges && window.activity,
                        agent_provider: agent_provider.filter(|_| settings.badges),
                        panes: panes
                            .into_iter()
                            .map(|pane| StatusBarPane {
                                id: pane.id,
                                label: match &pane.kind {
                                    PaneKindSnapshot::Browser(browser) => {
                                        if browser.url() == "about:blank" {
                                            "New tab".to_owned()
                                        } else {
                                            browser.url().to_owned()
                                        }
                                    }
                                    _ => pane_label(pane),
                                },
                                kind: pane.kind.clone(),
                                active: pane.id == window.active_pane,
                            })
                            .collect(),
                    }
                })
                .collect()
        });

        Self {
            session_name: session
                .filter(|_| settings.show_session)
                .map(|session| session.name.clone()),
            windows,
            agents,
            host_name: host_name.filter(|_| settings.show_host).map(str::to_owned),
            show_update: settings.show_update,
        }
    }
}

#[cfg(test)]
mod tests {
    use zz_protocol::{
        AgentDescriptor, LayoutNode, PaneId, PaneSnapshot, SessionSnapshot, WindowSnapshot,
    };

    use super::*;

    fn pane(id: u64, kind: PaneKindSnapshot, bell: bool, dead: bool) -> PaneSnapshot {
        PaneSnapshot {
            id: PaneId(id),
            title: format!("pane-{id}"),
            kind,
            synchronized_input: false,
            bell,
            dead,
            dead_status: None,
            border_colour: None,
            active_border_colour: None,
            border_status_text: String::new(),
        }
    }

    fn window(
        id: u64,
        index: u32,
        name: &str,
        activity: bool,
        panes: Vec<PaneSnapshot>,
    ) -> WindowSnapshot {
        let active_pane = panes.first().map_or(PaneId(0), |pane| pane.id);
        WindowSnapshot {
            id: WindowId(id),
            index,
            name: name.to_owned(),
            automatic_rename: true,
            active_pane,
            zoomed_pane: None,
            layout: LayoutNode::Pane(active_pane),
            panes: panes.into_iter().map(|pane| (pane.id, pane)).collect(),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label: String::new(),
            activity,
            pane_border_status: zz_protocol::PaneBorderStatus::Off,
            pane_border_lines: zz_protocol::PaneBorderLines::Single,
            pane_border_indicators: zz_protocol::PaneBorderIndicators::Colour,
            pane_order: Vec::new(),
            pane_z_order: Vec::new(),
        }
    }

    fn snapshot() -> MuxSnapshot {
        let primary = SessionSnapshot {
            id: SessionId(1),
            name: "main".to_owned(),
            active_window: WindowId(10),
            windows: vec![
                window(
                    10,
                    0,
                    "shell",
                    true,
                    vec![
                        pane(100, PaneKindSnapshot::Terminal, true, false),
                        pane(
                            101,
                            PaneKindSnapshot::Agent(AgentDescriptor::default()),
                            false,
                            true,
                        ),
                    ],
                ),
                window(
                    11,
                    1,
                    "agent",
                    false,
                    vec![pane(
                        110,
                        PaneKindSnapshot::Agent(AgentDescriptor::default()),
                        false,
                        false,
                    )],
                ),
            ],
            viewers: Vec::new(),
        };
        let secondary = SessionSnapshot {
            id: SessionId(2),
            name: "other".to_owned(),
            active_window: WindowId(20),
            windows: vec![window(
                20,
                0,
                "other-agent",
                false,
                vec![pane(
                    200,
                    PaneKindSnapshot::Agent(AgentDescriptor::default()),
                    false,
                    false,
                )],
            )],
            viewers: Vec::new(),
        };
        MuxSnapshot {
            generation: 7,
            sessions: vec![primary, secondary],
            focused_window: Some(WindowId(11)),
        }
    }

    #[test]
    fn pane_deck_preserves_layout_order_and_current_browser_tab_without_badges() {
        let mut snapshot = snapshot();
        let window = &mut snapshot.sessions[0].windows[0];
        let browser = zz_protocol::BrowserDescriptor {
            tabs: vec![
                "https://old.example".into(),
                "https://current.example".into(),
            ],
            active_tab: 1,
            profile: "default".into(),
        };
        window.panes.insert(
            PaneId(102),
            pane(102, PaneKindSnapshot::Browser(browser), false, false),
        );
        window.active_pane = PaneId(102);
        window.layout = LayoutNode::Split {
            id: zz_protocol::SplitId(1),
            axis: zz_protocol::Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Pane(PaneId(102))),
            second: Box::new(LayoutNode::Pane(PaneId(100))),
        };
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            None,
            StatusBarSettings {
                badges: false,
                ..Default::default()
            },
        );
        let panes = &model.windows[0].panes;
        assert_eq!(
            panes
                .iter()
                .map(|pane| (pane.id, pane.active))
                .collect::<Vec<_>>(),
            vec![
                (PaneId(102), true),
                (PaneId(100), false),
                (PaneId(101), false)
            ]
        );
        assert_eq!(panes[0].label, "https://current.example");
        assert!(matches!(panes[0].kind, PaneKindSnapshot::Browser(_)));
    }

    #[test]
    fn settings_defaults_enable_every_item() {
        assert_eq!(
            StatusBarSettings::default(),
            StatusBarSettings {
                show_session: true,
                badges: true,
                show_agents: true,
                show_host: true,
                show_update: true,
            }
        );
    }

    #[test]
    fn attached_session_owns_the_window_list_and_recipient_focus() {
        let snapshot = snapshot();
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            Some("remote.example"),
            StatusBarSettings::default(),
        );

        assert_eq!(model.session_name.as_deref(), Some("main"));
        assert_eq!(model.host_name.as_deref(), Some("remote.example"));
        assert_eq!(
            model
                .windows
                .iter()
                .map(|window| (window.id, window.index, window.name.as_str(), window.active))
                .collect::<Vec<_>>(),
            [
                (WindowId(10), 0, "shell", false),
                (WindowId(11), 1, "agent", true),
            ]
        );

        let secondary = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(2)),
            None,
            StatusBarSettings::default(),
        );
        assert_eq!(secondary.windows.len(), 1);
        assert!(secondary.windows[0].active);
    }

    #[test]
    fn badges_use_window_and_pane_flags_and_agents_include_only_live_panes() {
        let snapshot = snapshot();
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            None,
            StatusBarSettings::default(),
        );

        assert!(model.windows[0].bell);
        assert!(model.windows[0].activity);
        assert_eq!(model.windows[0].agent_provider, Some(AgentProvider::Codex));
        assert!(!model.windows[1].bell);
        assert!(!model.windows[1].activity);
        assert_eq!(model.windows[1].agent_provider, Some(AgentProvider::Codex));
        assert_eq!(
            model.agents,
            vec![StatusBarAgent {
                id: PaneId(110),
                label: "pane-110".to_owned(),
                window_name: "agent".to_owned(),
                provider: AgentProvider::Codex,
            }]
        );
    }

    #[test]
    fn automatic_agent_labels_follow_session_titles_and_provider_changes() {
        let mut snapshot = snapshot();
        for (title, provider, label) in [
            ("Fix titlebar", AgentProvider::Codex, "Fix titlebar"),
            (
                "  Review changes  ",
                AgentProvider::ClaudeCode,
                "Review changes",
            ),
            ("agent", AgentProvider::ClaudeCode, "New session"),
            ("", AgentProvider::Codex, "New session"),
        ] {
            let pane = snapshot.sessions[0].windows[1]
                .panes
                .get_mut(&PaneId(110))
                .unwrap();
            pane.title = title.to_owned();
            pane.kind = PaneKindSnapshot::Agent(AgentDescriptor {
                provider,
                ..AgentDescriptor::default()
            });
            let model = StatusBarModel::from_snapshot(
                &snapshot,
                Some(SessionId(1)),
                None,
                StatusBarSettings::default(),
            );

            assert_eq!(model.windows[1].name, "agent");
            assert_eq!(model.windows[1].label, label);
            assert_eq!(model.windows[1].agent_provider, Some(provider));
            assert_eq!(model.agents[0].id, PaneId(110));
            assert_eq!(model.agents[0].label, label);
            assert_eq!(model.agents[0].provider, provider);
        }
    }

    #[test]
    fn mixed_window_prefers_the_active_agent_and_keeps_terminal_window_names() {
        let mut snapshot = snapshot();
        snapshot.sessions[0].windows[0].panes.insert(
            PaneId(102),
            pane(
                102,
                PaneKindSnapshot::Agent(AgentDescriptor {
                    provider: AgentProvider::ClaudeCode,
                    ..AgentDescriptor::default()
                }),
                false,
                false,
            ),
        );
        for (active_pane, label, provider) in [
            (102, "pane-102", AgentProvider::ClaudeCode),
            (100, "shell", AgentProvider::Codex),
        ] {
            snapshot.sessions[0].windows[0].active_pane = PaneId(active_pane);
            let model = StatusBarModel::from_snapshot(
                &snapshot,
                Some(SessionId(1)),
                None,
                StatusBarSettings::default(),
            );

            assert_eq!(model.windows[0].name, "shell");
            assert_eq!(model.windows[0].label, label);
            assert_eq!(model.windows[0].agent_provider, Some(provider));
        }
    }

    #[test]
    fn automatic_browser_labels_follow_page_titles_and_current_url() {
        let mut snapshot = snapshot();
        for (title, urls, active_tab, expected) in [
            (
                "  Example page  ",
                vec!["https://example.com"],
                0,
                "Example page",
            ),
            (
                " ",
                vec!["https://old.example", "https://current.example"],
                1,
                "https://current.example",
            ),
            ("", vec![""], 0, "browser"),
        ] {
            let window = &mut snapshot.sessions[0].windows[0];
            window.name.clear();
            let pane = window.panes.get_mut(&window.active_pane).unwrap();
            pane.title = title.to_owned();
            pane.kind = PaneKindSnapshot::Browser(zz_protocol::BrowserDescriptor {
                tabs: urls.into_iter().map(str::to_owned).collect(),
                active_tab,
                profile: "default".into(),
            });
            let model = StatusBarModel::from_snapshot(
                &snapshot,
                Some(SessionId(1)),
                None,
                StatusBarSettings::default(),
            );
            assert_eq!(model.windows[0].label, expected);
            assert!(model.windows[0].name.is_empty());
        }
        let window = &mut snapshot.sessions[0].windows[0];
        window.name = "Research".to_owned();
        window.automatic_rename = false;
        window.panes.get_mut(&window.active_pane).unwrap().title = "Page title".to_owned();
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            None,
            StatusBarSettings::default(),
        );
        assert_eq!(model.windows[0].label, "Research");
        assert_eq!(model.windows[0].name, "Research");
    }

    #[test]
    fn explicit_window_names_override_agent_session_labels() {
        let mut snapshot = snapshot();
        let window = &mut snapshot.sessions[0].windows[1];
        window.name = "Reviews".to_owned();
        window.automatic_rename = false;
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            None,
            StatusBarSettings::default(),
        );

        assert_eq!(model.windows[1].name, "Reviews");
        assert_eq!(model.windows[1].label, "Reviews");
        assert_eq!(model.windows[1].agent_provider, Some(AgentProvider::Codex));
    }

    #[test]
    fn settings_hide_presentational_items_without_hiding_windows() {
        let snapshot = snapshot();
        let settings = StatusBarSettings {
            show_session: false,
            badges: false,
            show_agents: false,
            show_host: false,
            show_update: false,
        };
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            Some("remote.example"),
            settings,
        );

        assert_eq!(model.windows.len(), 2);
        assert!(
            model
                .windows
                .iter()
                .all(|window| !window.bell && !window.activity && window.agent_provider.is_none())
        );
        assert_eq!(model.session_name, None);
        assert!(model.agents.is_empty());
        assert_eq!(model.host_name, None);
        assert!(!model.show_update);
        assert_eq!(model.windows[1].label, "pane-110");
    }

    #[test]
    fn badges_do_not_control_the_agents_item() {
        let model = StatusBarModel::from_snapshot(
            &snapshot(),
            Some(SessionId(1)),
            None,
            StatusBarSettings {
                badges: false,
                ..StatusBarSettings::default()
            },
        );

        assert_eq!(
            model.agents,
            vec![StatusBarAgent {
                id: PaneId(110),
                label: "pane-110".to_owned(),
                window_name: "agent".to_owned(),
                provider: AgentProvider::Codex,
            }]
        );
        assert!(
            model
                .windows
                .iter()
                .all(|window| window.agent_provider.is_none())
        );
    }

    #[test]
    fn zero_agents_and_missing_attachments_are_empty() {
        let no_agents = MuxSnapshot {
            generation: 1,
            sessions: vec![SessionSnapshot {
                id: SessionId(3),
                name: "plain".to_owned(),
                active_window: WindowId(30),
                windows: vec![window(
                    30,
                    0,
                    "terminal",
                    false,
                    vec![pane(300, PaneKindSnapshot::Terminal, false, false)],
                )],
                viewers: Vec::new(),
            }],
            focused_window: None,
        };
        let model = StatusBarModel::from_snapshot(
            &no_agents,
            Some(SessionId(3)),
            None,
            StatusBarSettings::default(),
        );
        assert!(model.agents.is_empty());

        for attached in [None, Some(SessionId(99))] {
            let model = StatusBarModel::from_snapshot(
                &snapshot(),
                attached,
                Some("remote.example"),
                StatusBarSettings::default(),
            );
            assert_eq!(model.session_name, None);
            assert!(model.windows.is_empty());
            assert!(model.agents.is_empty());
            assert_eq!(model.host_name.as_deref(), Some("remote.example"));
        }
    }

    #[test]
    fn window_projection_keeps_every_attached_window() {
        let windows = (0..7)
            .map(|index| {
                window(
                    40 + u64::from(index),
                    index,
                    &format!("window-{index}"),
                    false,
                    vec![pane(
                        400 + u64::from(index),
                        PaneKindSnapshot::Terminal,
                        false,
                        false,
                    )],
                )
            })
            .collect();
        let snapshot = MuxSnapshot {
            generation: 1,
            sessions: vec![SessionSnapshot {
                id: SessionId(4),
                name: "many".to_owned(),
                active_window: WindowId(43),
                windows,
                viewers: Vec::new(),
            }],
            focused_window: None,
        };
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(4)),
            None,
            StatusBarSettings::default(),
        );

        assert_eq!(model.windows.len(), 7);
        assert_eq!(
            model.windows.iter().filter(|window| window.active).count(),
            1
        );
        assert!(model.windows[3].active);
    }
}
