use std::{rc::Rc, sync::Arc};

use gpui::{App, Entity, KeyDownEvent, SharedString};
use zz_client::completion::PaneKindAvailability;
use zz_protocol::{CommandInvocation, InputMessage, MuxSnapshot, PaneId};
use zz_ui::{
    IconName,
    command::{
        PaletteBackend, PaletteHostId, PaletteSettings, PaletteStatus, PaletteTarget, PaletteTree,
        PaletteTreeHost, PaletteTreePane, PaletteTreeSession, PaletteTreeWindow,
    },
};

pub(crate) use zz_ui::command::{CommandPaletteEvent, CommandPaletteView, PaletteMode};

use crate::{
    mux::{
        client::MuxClient,
        hosts::{HostId, HostState},
        nav::{MuxTreeHost, MuxTreeModel, MuxTreePane, MuxTreePaneKind, TreeNode, TreeTarget},
        prefix::terminal_key_input,
    },
    terminal::view::TERMINAL_FONT,
};

pub(crate) fn palette_backend(mux: Entity<MuxClient>) -> Rc<dyn PaletteBackend> {
    Rc::new(MuxPalette(mux))
}

struct MuxPalette(Entity<MuxClient>);

impl PaletteBackend for MuxPalette {
    fn snapshot(&self, cx: &App) -> Arc<MuxSnapshot> {
        self.0.read(cx).snapshot()
    }

    fn host_snapshot<'a>(&self, host: PaletteHostId, cx: &'a App) -> Option<&'a MuxSnapshot> {
        let host = HostId::from(host);
        self.0
            .read(cx)
            .fleet_hosts()
            .find(|(id, ..)| *id == host)
            .and_then(|(.., snapshot)| snapshot)
    }

    fn tree(&self, cx: &App) -> PaletteTree {
        let mux = self.0.read(cx);
        let attached = mux.attached_host();
        PaletteTree {
            attached: attached.into(),
            hosts: MuxTreeModel::from_mux(mux)
                .hosts
                .iter()
                .map(|host| PaletteTreeHost {
                    id: host.id.into(),
                    name: host.name.clone(),
                    detail: host_detail(host, cx),
                    right: match host.state {
                        HostState::Connected => format!(
                            "{} session{}",
                            host.sessions.len(),
                            if host.sessions.len() == 1 { "" } else { "s" }
                        ),
                        HostState::Connecting => "connecting…".to_owned(),
                        HostState::Reconnecting { .. } => "reconnecting…".to_owned(),
                        HostState::Incompatible { .. } => "incompatible version".to_owned(),
                        _ => "offline · ↵ to connect".to_owned(),
                    },
                    status: match host.state {
                        HostState::Connected => PaletteStatus::Online,
                        HostState::Connecting | HostState::Reconnecting { .. } => {
                            PaletteStatus::Waiting
                        }
                        _ => PaletteStatus::Offline,
                    },
                    sessions: host
                        .sessions
                        .iter()
                        .map(|session| PaletteTreeSession {
                            id: session.id,
                            name: session.name.clone(),
                            active: session.active,
                            windows: session
                                .windows
                                .iter()
                                .map(|window| PaletteTreeWindow {
                                    id: window.id,
                                    name: window.name.clone(),
                                    active: window.active,
                                    active_pane: window.active_pane,
                                    panes: window
                                        .panes
                                        .iter()
                                        .map(|pane| tree_pane(pane, host.id == attached, mux))
                                        .collect(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn settings(&self, cx: &App) -> PaletteSettings {
        PaletteSettings {
            grouped: crate::config::palette_window_layout(cx)
                == crate::config::PaletteWindowLayout::Grouped,
            host_prefix: if crate::config::palette_host_prefix(cx) == '#' {
                "#"
            } else {
                "~"
            },
            show_keys: crate::config::palette_show_keys(cx),
        }
    }

    fn availability(&self, cx: &App) -> PaneKindAvailability {
        PaneKindAvailability {
            browser: crate::browser::controller::is_available(cx),
            agent: crate::config::agent_pane_enabled(cx),
            editor: crate::config::editor_pane_enabled(cx),
        }
    }

    fn command_shortcut(&self, command: &str, cx: &App) -> Option<SharedString> {
        let mux = self.0.read(cx);
        mux.prefix_bindings()
            .iter()
            .find(|binding| binding.commands.len() == 1 && binding.commands[0].name == command)
            .map(|binding| {
                format!(
                    "{} {}",
                    mux.canonical_prefix()
                        .unwrap_or_else(|| "prefix".to_owned()),
                    binding.key
                )
                .into()
            })
    }

    fn mono_font(&self, _: &App) -> SharedString {
        TERMINAL_FONT.into()
    }

    fn send_input(&self, input: InputMessage, cx: &mut App) {
        self.0.read(cx).send_input(input);
    }

    fn send_key(&self, pane: PaneId, event: &KeyDownEvent, cx: &mut App) {
        self.0.read(cx).send_input(InputMessage::Key {
            pane,
            input: terminal_key_input(&event.keystroke, zz_terminal::KeyAction::Press),
            text_follows: false,
        });
    }

    fn active_pane(&self, cx: &App) -> Option<PaneId> {
        self.0.read(cx).active_pane()
    }

    fn execute(&self, host: PaletteHostId, command: CommandInvocation, cx: &mut App) {
        self.0.read(cx).execute_on_host(host.into(), command);
    }

    fn connect_host(&self, host: PaletteHostId, cx: &mut App) {
        self.0
            .update(cx, |mux, cx| mux.retry_host_now(host.into(), cx));
    }

    fn activate(&self, host: PaletteHostId, target: PaletteTarget, cx: &mut App) -> bool {
        let mux = self.0.read(cx);
        let target = match target {
            PaletteTarget::Session(id) => TreeTarget::Session(id),
            PaletteTarget::Window(id) => TreeTarget::Window(id),
            PaletteTarget::Pane(id) => TreeTarget::Pane(id),
        };
        let Some(activation) = MuxTreeModel::from_mux(mux).activation_for_node(
            TreeNode::Target(host.into(), target),
            mux.attached_host(),
            mux.attached_session(),
        ) else {
            return false;
        };
        crate::mux::nav::activate_nav(&self.0, activation, cx);
        true
    }
}

fn host_detail(host: &MuxTreeHost, cx: &App) -> String {
    if host.id == HostId::LOCAL {
        if cfg!(target_os = "macos") {
            "This Mac"
        } else {
            "This computer"
        }
        .to_owned()
    } else {
        crate::config::fleet_hosts(cx)
            .iter()
            .find(|entry| entry.name == host.name)
            .map_or_else(
                || "Remote host".to_owned(),
                |entry| entry.endpoint.to_string(),
            )
    }
}

fn tree_pane(pane: &MuxTreePane, attached: bool, mux: &MuxClient) -> PaletteTreePane {
    let attention = matches!(pane.kind, MuxTreePaneKind::Agent(_))
        .then(|| {
            attached
                .then(|| mux.agent_attention_status(pane.id))
                .flatten()
        })
        .flatten();
    PaletteTreePane {
        id: pane.id,
        label: pane.label.clone(),
        detail: match pane.kind {
            MuxTreePaneKind::Agent(_) => match attention {
                Some(zz_client::AgentAttentionStatus::Working) => "Agent · running",
                Some(zz_client::AgentAttentionStatus::NeedsInput) => "Agent · waiting for input",
                Some(zz_client::AgentAttentionStatus::Idle) => "Agent · idle",
                Some(zz_client::AgentAttentionStatus::Failed) => "Agent · failed",
                None => "Agent",
            },
            MuxTreePaneKind::Terminal => "Terminal",
            MuxTreePaneKind::Browser => "Browser",
            MuxTreePaneKind::Editor => "Editor",
            MuxTreePaneKind::Picker => "New pane",
        }
        .into(),
        icon: match pane.kind {
            MuxTreePaneKind::Picker => IconName::Plus,
            MuxTreePaneKind::Terminal => IconName::SquareTerminal,
            MuxTreePaneKind::Browser => IconName::Globe,
            MuxTreePaneKind::Agent(provider) => zz_ui::pane::agent_provider_icon(provider),
            MuxTreePaneKind::Editor => IconName::File,
        },
        running: attention == Some(zz_client::AgentAttentionStatus::Working),
    }
}
