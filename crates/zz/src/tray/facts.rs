use std::sync::Arc;

use parking_lot::Mutex;
use zz_client::AgentAttentionStatus;
use zz_protocol::{MuxSnapshot, PaneId, PaneKindSnapshot};

use super::TrayEvent;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Session {
    pub name: String,
    pub windows: usize,
    pub attached: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Attention {
    pub pane: PaneId,
    pub session: String,
    pub window: u32,
    pub title: String,
    pub permission: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TrayFacts {
    pub sessions: Vec<Session>,
    pub attention: Vec<Attention>,
    pub update_available: bool,
}

#[derive(Clone, Default)]
pub(super) struct Source(Arc<Mutex<Vec<MenuEntry>>>);

#[derive(Clone)]
pub(super) enum MenuEntry {
    Separator,
    Item {
        title: String,
        hint: String,
        action: Option<TrayEvent>,
    },
}

impl MenuEntry {
    pub fn label(&self) -> String {
        match self {
            Self::Separator => String::new(),
            Self::Item { title, hint, .. } if !hint.is_empty() => format!("{title} · {hint}"),
            Self::Item { title, .. } => title.clone(),
        }
    }
}

impl Source {
    pub fn menu(&self) -> Vec<MenuEntry> {
        self.0.lock().clone()
    }

    pub fn set(&self, entries: Vec<MenuEntry>) {
        *self.0.lock() = entries;
    }
}

pub(super) fn facts_from(
    snapshot: Option<&MuxSnapshot>,
    attention: impl Fn(PaneId) -> Option<AgentAttentionStatus>,
    stale: bool,
) -> Option<TrayFacts> {
    let snapshot = snapshot?;
    let mut facts = TrayFacts {
        sessions: Vec::new(),
        attention: Vec::new(),
        update_available: stale,
    };
    for session in &snapshot.sessions {
        facts.sessions.push(Session {
            name: session.name.clone(),
            windows: session.windows.len(),
            attached: !session.viewers.is_empty(),
        });
        for window in &session.windows {
            for pane in window.panes.values() {
                if matches!(pane.kind, PaneKindSnapshot::Agent(_))
                    && attention(pane.id) == Some(AgentAttentionStatus::NeedsInput)
                {
                    facts.attention.push(Attention {
                        pane: pane.id,
                        session: session.name.clone(),
                        window: window.index,
                        title: pane.title.clone(),
                        permission: true,
                    });
                }
            }
        }
    }
    Some(facts)
}

fn item(title: impl Into<String>, hint: impl Into<String>, action: Option<TrayEvent>) -> MenuEntry {
    MenuEntry::Item {
        title: title.into(),
        hint: hint.into(),
        action,
    }
}

pub(super) fn menu(facts: Option<&TrayFacts>, active: bool) -> Vec<MenuEntry> {
    let mut menu = vec![
        item(
            if active { "Hide zz" } else { "Show zz" },
            "",
            Some(TrayEvent::Toggle),
        ),
        MenuEntry::Separator,
    ];
    if let Some(facts) = facts {
        if !facts.attention.is_empty() {
            menu.push(item("Needs attention", "", None));
            for pane in &facts.attention {
                menu.push(item(
                    format!("{} in {}:{}", pane.title, pane.session, pane.window),
                    if pane.permission {
                        "needs approval"
                    } else {
                        "waiting"
                    },
                    Some(TrayEvent::FocusPane(pane.pane)),
                ));
            }
            menu.push(MenuEntry::Separator);
        }
        menu.push(item("Sessions", "", None));
        for session in &facts.sessions {
            menu.push(item(
                &session.name,
                format!(
                    "{} windows{}",
                    session.windows,
                    if session.attached { " · attached" } else { "" }
                ),
                Some(TrayEvent::SwitchSession(session.name.clone())),
            ));
        }
    }
    menu.push(item("New Session", "", Some(TrayEvent::NewSession)));
    menu.push(MenuEntry::Separator);
    if let Some(facts) = facts {
        menu.push(item(format!("zz {}", env!("CARGO_PKG_VERSION")), "", None));
        if facts.update_available {
            menu.push(item(
                "Restart Daemon to Update…",
                "",
                Some(TrayEvent::RestartDaemon),
            ));
        }
    } else {
        menu.push(item("Daemon unavailable", "", None));
    }
    menu.extend([
        MenuEntry::Separator,
        item("Settings…", "", Some(TrayEvent::OpenSettings)),
        item("Open Logs", "", Some(TrayEvent::OpenLogs)),
        MenuEntry::Separator,
        item("Quit and Stop Sessions", "", Some(TrayEvent::Quit)),
    ]);
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_facts_include_sessions_and_only_agents_needing_input() {
        use std::collections::BTreeMap;
        use zz_protocol::{
            AgentDescriptor, LayoutNode, PaneBorderIndicators, PaneBorderLines, PaneBorderStatus,
            PaneSnapshot, SessionId, SessionSnapshot, SessionViewer, WindowId, WindowSnapshot,
        };

        let pane = |id, title: &str, kind| PaneSnapshot {
            id: PaneId(id),
            title: title.into(),
            kind,
            synchronized_input: false,
            bell: false,
            dead: false,
            dead_status: None,
            border_colour: None,
            active_border_colour: None,
            border_status_text: String::new(),
        };
        let terminal = pane(1, "Shell", PaneKindSnapshot::Terminal);
        let approval = pane(
            2,
            "Review",
            PaneKindSnapshot::Agent(AgentDescriptor::default()),
        );
        let working = pane(
            3,
            "Build",
            PaneKindSnapshot::Agent(AgentDescriptor::default()),
        );
        let window = WindowSnapshot {
            id: WindowId(1),
            index: 7,
            name: "main".into(),
            automatic_rename: false,
            active_pane: terminal.id,
            zoomed_pane: None,
            layout: LayoutNode::Pane(terminal.id),
            panes: BTreeMap::from([
                (terminal.id, terminal),
                (approval.id, approval),
                (working.id, working),
            ]),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
            status_label: String::new(),
            activity: false,
            pane_border_status: PaneBorderStatus::default(),
            pane_border_lines: PaneBorderLines::default(),
            pane_border_indicators: PaneBorderIndicators::default(),
            pane_order: vec![PaneId(1), PaneId(2), PaneId(3)],
            pane_z_order: vec![PaneId(1), PaneId(2), PaneId(3)],
        };
        let mut snapshot = MuxSnapshot {
            generation: 1,
            sessions: vec![SessionSnapshot {
                id: SessionId(1),
                name: "work".into(),
                active_window: window.id,
                viewers: vec![SessionViewer {
                    name: "desktop".into(),
                    window: window.id,
                    is_self: true,
                }],
                windows: vec![window],
            }],
            focused_window: None,
        };
        let attention = |pane| {
            Some(if pane == PaneId(3) {
                AgentAttentionStatus::Working
            } else {
                AgentAttentionStatus::NeedsInput
            })
        };
        let facts = facts_from(Some(&snapshot), attention, true).expect("snapshot facts");
        assert_eq!(
            facts.sessions,
            vec![Session {
                name: "work".into(),
                windows: 1,
                attached: true
            }]
        );
        assert_eq!(
            facts.attention,
            vec![Attention {
                pane: PaneId(2),
                session: "work".into(),
                window: 7,
                title: "Review".into(),
                permission: true
            }]
        );
        assert!(facts.update_available);
        snapshot.sessions[0].viewers.clear();
        let detached = facts_from(Some(&snapshot), attention, false).expect("snapshot facts");
        assert!(!detached.sessions[0].attached);
        assert!(!detached.update_available);
        assert!(facts_from(None, attention, true).is_none());
    }

    #[test]
    fn unavailable_facts_keep_static_actions() {
        let rows = menu(None, false);
        assert!(rows.iter().any(|row| matches!(
            row,
            MenuEntry::Item {
                action: Some(TrayEvent::NewSession),
                ..
            }
        )));
        assert!(!rows.iter().any(|row| matches!(row, MenuEntry::Item { title, .. } if title == "Sessions" || title == "Needs attention")));
    }
}
