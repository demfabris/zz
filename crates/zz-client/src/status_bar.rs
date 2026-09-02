use zz_protocol::{MuxSnapshot, PaneKindSnapshot, SessionId, WindowId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusBarAlignment {
    #[default]
    Left,
    Center,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusBarClock {
    #[default]
    TwentyFourHour,
    TwelveHour,
    TimeAndDate,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusBarSettings {
    pub show_session: bool,
    pub badges: bool,
    pub alignment: StatusBarAlignment,
    pub show_agents: bool,
    pub show_host: bool,
    pub show_update: bool,
    pub clock: StatusBarClock,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self {
            show_session: true,
            badges: true,
            alignment: StatusBarAlignment::Left,
            show_agents: true,
            show_host: true,
            show_update: true,
            clock: StatusBarClock::TwentyFourHour,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarWindow {
    pub id: WindowId,
    pub index: u32,
    pub name: String,
    pub active: bool,
    pub bell: bool,
    pub activity: bool,
    pub agent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarModel {
    pub session_name: Option<String>,
    pub windows: Vec<StatusBarWindow>,
    pub agent_count: Option<usize>,
    pub host_name: Option<String>,
    pub alignment: StatusBarAlignment,
    pub show_update: bool,
    pub clock: StatusBarClock,
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
        let mut live_agents = 0;
        let windows = session.map_or_else(Vec::new, |session| {
            session
                .windows
                .iter()
                .map(|window| {
                    let (bell, agent, window_live_agents) = window.panes.values().fold(
                        (false, false, 0),
                        |(bell, agent, live_agents), pane| {
                            let is_agent = matches!(pane.kind, PaneKindSnapshot::Agent(_));
                            (
                                bell || pane.bell,
                                agent || is_agent,
                                live_agents + usize::from(is_agent && !pane.dead),
                            )
                        },
                    );
                    live_agents += window_live_agents;
                    StatusBarWindow {
                        id: window.id,
                        index: window.index,
                        name: window.name.clone(),
                        active: focused_window == Some(window.id),
                        bell: settings.badges && bell,
                        activity: settings.badges && window.activity,
                        agent: settings.badges && agent,
                    }
                })
                .collect()
        });

        Self {
            session_name: session
                .filter(|_| settings.show_session)
                .map(|session| session.name.clone()),
            windows,
            agent_count: (settings.show_agents && live_agents > 0).then_some(live_agents),
            host_name: host_name.filter(|_| settings.show_host).map(str::to_owned),
            alignment: settings.alignment,
            show_update: settings.show_update,
            clock: settings.clock,
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
    fn settings_defaults_enable_every_item() {
        assert_eq!(
            StatusBarSettings::default(),
            StatusBarSettings {
                show_session: true,
                badges: true,
                alignment: StatusBarAlignment::Left,
                show_agents: true,
                show_host: true,
                show_update: true,
                clock: StatusBarClock::TwentyFourHour,
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
    fn badges_use_window_and_pane_flags_and_agents_count_only_live_panes() {
        let snapshot = snapshot();
        let model = StatusBarModel::from_snapshot(
            &snapshot,
            Some(SessionId(1)),
            None,
            StatusBarSettings::default(),
        );

        assert!(model.windows[0].bell);
        assert!(model.windows[0].activity);
        assert!(model.windows[0].agent);
        assert!(!model.windows[1].bell);
        assert!(!model.windows[1].activity);
        assert!(model.windows[1].agent);
        assert_eq!(model.agent_count, Some(1));
    }

    #[test]
    fn settings_hide_presentational_items_without_hiding_windows() {
        let snapshot = snapshot();
        let settings = StatusBarSettings {
            show_session: false,
            badges: false,
            alignment: StatusBarAlignment::Center,
            show_agents: false,
            show_host: false,
            show_update: false,
            clock: StatusBarClock::Off,
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
                .all(|window| !window.bell && !window.activity && !window.agent)
        );
        assert_eq!(model.session_name, None);
        assert_eq!(model.agent_count, None);
        assert_eq!(model.host_name, None);
        assert_eq!(model.alignment, StatusBarAlignment::Center);
        assert!(!model.show_update);
        assert_eq!(model.clock, StatusBarClock::Off);
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

        assert_eq!(model.agent_count, Some(1));
        assert!(model.windows.iter().all(|window| !window.agent));
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
        assert_eq!(model.agent_count, None);

        for attached in [None, Some(SessionId(99))] {
            let model = StatusBarModel::from_snapshot(
                &snapshot(),
                attached,
                Some("remote.example"),
                StatusBarSettings::default(),
            );
            assert_eq!(model.session_name, None);
            assert!(model.windows.is_empty());
            assert_eq!(model.agent_count, None);
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
