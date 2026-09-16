use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use gpui::App;
use zz_client::completion::{CompletionSuggestion, PaneKindAvailability};
use zz_protocol::{
    ChooseTreeState, ChooseTreeTarget, CommandSpec, CommandValueKind, MuxSnapshot,
    PaneKindSnapshot, command_specs,
};
use zz_ui::{
    IconName,
    command::{PalettePill, PaletteRow, PaletteStatus},
};

use crate::mux::{
    client::MuxClient,
    hosts::{HostId, HostState},
    nav::{
        MuxTreeHost, MuxTreeModel, MuxTreePane, MuxTreePaneKind, MuxTreeSession, MuxTreeWindow,
        TreeNode, TreeTarget,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteMode {
    Command,
    Window,
    Pane,
    Host,
}

impl PaletteMode {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Window => "Navigate",
            Self::Pane => "Pane",
            Self::Host => "Host",
        }
    }

    pub(super) const fn prefix(self, host_prefix: &'static str) -> &'static str {
        match self {
            Self::Command => ":",
            Self::Window => "@",
            Self::Pane => "%",
            Self::Host => host_prefix,
        }
    }

    pub(super) fn from_prefix(value: &str, host_prefix: &str) -> Option<Self> {
        match value {
            ":" => Some(Self::Command),
            "@" => Some(Self::Window),
            "%" => Some(Self::Pane),
            value if value == host_prefix => Some(Self::Host),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(super) enum PaletteAction {
    Host(HostId),
    Target {
        host: HostId,
        target: TreeTarget,
        label: String,
    },
    Command(&'static CommandSpec),
    History(String),
    Completion(CompletionSuggestion),
    ChooseWindow(u32),
}

#[derive(Clone)]
pub(super) struct PaletteEntry {
    pub row: PaletteRow,
    pub action: Option<PaletteAction>,
    pub hint: &'static str,
    pub score: usize,
}

impl PaletteEntry {
    fn row(row: PaletteRow, action: PaletteAction) -> Self {
        Self {
            row,
            action: Some(action),
            hint: "",
            score: 0,
        }
    }

    fn header(label: impl Into<gpui::SharedString>, hint: &'static str) -> Self {
        Self {
            row: PaletteRow {
                label: label.into(),
                ..Default::default()
            },
            action: None,
            hint,
            score: 0,
        }
    }

    fn matching(mut self, query: &str, metadata: &str) -> Option<Self> {
        if let Some((score, hits)) = fuzzy_match(query, &self.row.label) {
            self.score = score;
            self.row.matches = hits;
        } else if !query.is_empty() && !metadata.is_empty() {
            self.score = fuzzy_match(query, metadata)?.0.saturating_add(1000);
        } else {
            return None;
        }
        Some(self)
    }
}

pub(super) struct UnifiedPalette {
    pub mode: Option<PaletteMode>,
    pub host: Option<HostId>,
    pub command: Option<&'static CommandSpec>,
    pub tree: MuxTreeModel,
    pub rows: Vec<PaletteEntry>,
    pub collapsed: BTreeSet<HostId>,
    expanded: BTreeMap<TreeNode, bool>,
    chooser_current_window: Option<u32>,
    pub grouped: bool,
    pub host_prefix: &'static str,
    pub show_keys: bool,
}

impl UnifiedPalette {
    pub fn new(mode: Option<PaletteMode>) -> Self {
        Self {
            mode,
            host: None,
            command: None,
            tree: MuxTreeModel::default(),
            rows: Vec::new(),
            collapsed: BTreeSet::new(),
            expanded: BTreeMap::new(),
            chooser_current_window: None,
            grouped: true,
            host_prefix: "~",
            show_keys: true,
        }
    }

    pub fn pop_layer(&mut self) -> bool {
        self.command.take().is_some() || self.mode.take().is_some() || self.host.take().is_some()
    }

    pub fn navigate(&self, selected: Option<usize>, direction: isize) -> Option<usize> {
        let indices: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row.action.as_ref().map(|_| index))
            .collect();
        if indices.is_empty() {
            return None;
        }
        let current = selected
            .and_then(|selected| indices.iter().position(|index| *index == selected))
            .unwrap_or_default();
        let next = if direction < 0 {
            current.checked_sub(1).unwrap_or(indices.len() - 1)
        } else {
            (current + 1) % indices.len()
        };
        Some(indices[next])
    }

    pub fn is_navigation_tree(&self) -> bool {
        self.command.is_none() && matches!(self.mode, None | Some(PaletteMode::Window))
    }

    pub fn initial_selection(&self) -> Option<usize> {
        self.rows
            .iter()
            .position(|entry| {
                entry.row.right == "current"
                    && match entry.action {
                        Some(PaletteAction::Target {
                            target: TreeTarget::Window(_),
                            ..
                        }) => true,
                        Some(PaletteAction::ChooseWindow(index)) => {
                            self.chooser_current_window == Some(index)
                        }
                        _ => false,
                    }
            })
            .or_else(|| {
                self.rows
                    .iter()
                    .position(|entry| entry.action.is_some() && entry.row.right == "current")
            })
            .or_else(|| self.rows.iter().position(|entry| entry.action.is_some()))
    }

    pub fn set_expanded(&mut self, index: usize, expanded: bool) -> bool {
        let Some(entry) = self.rows.get(index) else {
            return false;
        };
        if entry.row.expanded.is_none_or(|current| current == expanded) {
            return false;
        }
        match entry.action {
            Some(PaletteAction::Target { host, target, .. }) => {
                self.expanded
                    .insert(TreeNode::Target(host, target), expanded);
            }
            Some(PaletteAction::Host(host)) => {
                if expanded {
                    self.collapsed.remove(&host);
                } else {
                    self.collapsed.insert(host);
                }
            }
            _ => return false,
        }
        true
    }

    pub fn parent_index(&self, index: usize) -> Option<usize> {
        let depth = self.rows.get(index)?.row.indent;
        self.rows[..index]
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, entry)| entry.action.is_some())
            .find_map(|(index, entry)| (entry.row.indent < depth).then_some(index))
    }

    pub fn child_index(&self, index: usize) -> Option<usize> {
        let entry = self.rows.get(index)?;
        (entry.row.expanded == Some(true)
            && self
                .rows
                .get(index + 1)
                .is_some_and(|child| child.action.is_some() && child.row.indent > entry.row.indent))
        .then_some(index + 1)
    }

    pub fn pills(&self) -> Vec<PalettePill> {
        let mut pills = Vec::new();
        if let Some(host) = self.host.and_then(|id| self.tree.host(id)) {
            pills.push(PalettePill::Host {
                label: host.name.clone().into(),
                status: host_status(&host.state),
            });
        }
        if let Some(mode) = self.mode {
            pills.push(PalettePill::Mode {
                prefix: mode.prefix(self.host_prefix).into(),
                label: mode.label().into(),
            });
        }
        if let Some(command) = self.command {
            pills.push(PalettePill::Command(command.name.into()));
        }
        pills
    }

    pub fn placeholder(&self) -> String {
        if let Some(command) = self.command {
            return if target_kind(command) == Some(CommandValueKind::Window) {
                "Target window"
            } else {
                "Target pane"
            }
            .to_owned();
        }
        match self.mode {
            Some(PaletteMode::Command) => "Type a command".to_owned(),
            Some(PaletteMode::Window) => "Search sessions, windows, panes".to_owned(),
            Some(PaletteMode::Pane) => "Choose a pane".to_owned(),
            Some(PaletteMode::Host) => "Choose a host or session".to_owned(),
            None => {
                let scope = self
                    .host
                    .and_then(|id| self.tree.host(id))
                    .map_or(String::new(), |host| format!(" {}", host.name));
                format!("Search{scope}, or type : @ {}", self.host_prefix)
            }
        }
    }

    pub fn usage(&self, selected: Option<usize>) -> Option<&'static str> {
        if self.mode != Some(PaletteMode::Command) {
            return None;
        }
        self.command
            .or_else(|| {
                selected
                    .and_then(|index| self.rows.get(index))
                    .and_then(|entry| match entry.action {
                        Some(PaletteAction::Command(spec)) => Some(spec),
                        _ => None,
                    })
            })
            .map(|spec| spec.usage)
    }

    pub fn rebuild_window_chooser(
        &mut self,
        state: &ChooseTreeState,
        query: &str,
        snapshot: &MuxSnapshot,
    ) {
        let query = query.trim();
        let tree = self.grouped && query.is_empty();
        let mut session_name = String::new();
        let mut window_name = String::new();
        let mut session_active = false;
        let mut window_active = false;
        self.rows.clear();
        self.chooser_current_window = None;
        for (index, item) in state.items.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            let name = if !item.text.is_empty() {
                item.text.as_str()
            } else if matches!(item.target, ChooseTreeTarget::Window(_))
                && let Some((prefix, name)) = item.label.split_once(':')
                && prefix.parse::<u32>().is_ok()
            {
                name
            } else {
                item.label.as_str()
            };
            let (path, active, icon) = match item.target {
                ChooseTreeTarget::Session(_) => {
                    name.clone_into(&mut session_name);
                    window_name.clear();
                    session_active = item.active();
                    window_active = false;
                    (String::new(), session_active, IconName::Folder)
                }
                ChooseTreeTarget::Window(_) => {
                    name.clone_into(&mut window_name);
                    window_active = session_active && item.active();
                    if window_active {
                        self.chooser_current_window = Some(index);
                    }
                    (session_name.clone(), window_active, IconName::PanelsTopLeft)
                }
                ChooseTreeTarget::Pane(pane) => (
                    [session_name.as_str(), window_name.as_str()]
                        .into_iter()
                        .filter(|name| !name.is_empty())
                        .collect::<Vec<_>>()
                        .join(" / "),
                    window_active && item.active(),
                    match item.pane_kind {
                        Some(zz_protocol::ChooseTreePaneKind::Terminal) => IconName::SquareTerminal,
                        Some(zz_protocol::ChooseTreePaneKind::Browser) => IconName::Globe,
                        Some(zz_protocol::ChooseTreePaneKind::Agent) => snapshot
                            .sessions
                            .iter()
                            .flat_map(|session| &session.windows)
                            .find_map(|window| window.panes.get(&pane))
                            .and_then(|pane| match &pane.kind {
                                PaneKindSnapshot::Agent(agent) => {
                                    Some(zz_ui::pane::agent_provider_icon(agent.provider))
                                }
                                _ => None,
                            })
                            .unwrap_or(IconName::Bot),
                        Some(zz_protocol::ChooseTreePaneKind::Editor) => IconName::File,
                        None => IconName::Plus,
                    },
                ),
                ChooseTreeTarget::Client(_) => continue,
            };
            let prefix = if !tree && !path.is_empty() {
                format!("{path} / ")
            } else {
                String::new()
            };
            let metadata = format!("{path} {} {} {}", item.label, item.detail, item.target);
            let row = PaletteRow {
                label: format!("{prefix}{name}").into(),
                detail: if item.text.is_empty() {
                    item.detail.clone()
                } else {
                    String::new()
                }
                .into(),
                muted_prefix: prefix.len(),
                right: if active { "current" } else { "" }.into(),
                icon: Some(icon),
                expanded: (tree && item.has_children()).then_some(item.expanded()),
                indent: if tree {
                    f32::from(item.depth) * 18.0
                } else {
                    0.0
                },
                ..Default::default()
            };
            if let Some(entry) = PaletteEntry::row(row, PaletteAction::ChooseWindow(index))
                .matching(query, &metadata)
            {
                self.rows.push(entry);
            }
        }
        if !tree {
            self.rows.sort_by_key(|entry| entry.score);
        }
    }

    fn navigation_entries(&self, query: &str, mux: &MuxClient) -> Vec<PaletteEntry> {
        let tree = self.grouped && query.is_empty() && self.is_navigation_tree();
        let target_kind = self.command.and_then(target_kind);
        let pane_only =
            self.mode == Some(PaletteMode::Pane) || target_kind == Some(CommandValueKind::Pane);
        let window_only = target_kind == Some(CommandValueKind::Window);
        let scope = self
            .host
            .or_else(|| self.command.map(|_| mux.attached_host()));
        let show_host = scope.is_none() && self.tree.hosts.len() > 1;
        let mut rows = Vec::new();
        for host in self
            .tree
            .hosts
            .iter()
            .filter(|host| host.connected() && scope.is_none_or(|scope| scope == host.id))
        {
            let host_prefix = if show_host {
                format!("{} / ", host.name)
            } else {
                String::new()
            };
            for session in &host.sessions {
                let session_open = self
                    .expanded
                    .get(&TreeNode::Target(host.id, session.target()))
                    .copied()
                    .unwrap_or(self.mode == Some(PaletteMode::Window) || session.active);
                if !pane_only && !window_only {
                    let mut entry = session_entry(host, session, mux);
                    entry.row.label = format!("{host_prefix}{}", session.name).into();
                    entry.row.muted_prefix = host_prefix.len();
                    entry.row.expanded =
                        (tree && !session.windows.is_empty()).then_some(session_open);
                    let metadata = format!("{} {}", session.id, entry.row.detail);
                    if let Some(entry) = entry.matching(query, &metadata) {
                        rows.push(entry);
                    }
                }
                if tree && !session_open {
                    continue;
                }
                for window in &session.windows {
                    let window_open = self
                        .expanded
                        .get(&TreeNode::Target(host.id, window.target()))
                        .copied()
                        .unwrap_or(false);
                    if !pane_only {
                        let mut entry = window_entry(host, session, window, mux, tree);
                        if !tree {
                            entry.row.label = format!("{host_prefix}{}", entry.row.label).into();
                            entry.row.muted_prefix += host_prefix.len();
                        }
                        entry.row.indent = if tree { 18.0 } else { 0.0 };
                        entry.row.expanded =
                            (tree && !window.panes.is_empty()).then_some(window_open);
                        let metadata = format!("{} {} {}", session.id, window.id, entry.row.detail);
                        if let Some(entry) = entry.matching(query, &metadata) {
                            rows.push(entry);
                        }
                    }
                    if window_only || (tree && !window_open) {
                        continue;
                    }
                    for pane in &window.panes {
                        let mut entry = pane_entry(host, session, window, pane, mux);
                        if tree {
                            entry.row.indent = 36.0;
                        } else {
                            let prefix =
                                format!("{host_prefix}{} / {} / ", session.name, window.name);
                            entry.row.label = format!("{prefix}{}", pane.label).into();
                            entry.row.muted_prefix = prefix.len();
                        }
                        let metadata = format!(
                            "{} {} {} {}",
                            session.id, window.id, pane.id, entry.row.detail
                        );
                        if let Some(entry) = entry.matching(query, &metadata) {
                            rows.push(entry);
                        }
                    }
                }
            }
        }
        if !tree {
            rows.sort_by_key(|entry| entry.score);
        }
        rows
    }

    pub fn rebuild(
        &mut self,
        query: &str,
        history: &[String],
        mux: &MuxClient,
        availability: PaneKindAvailability,
        cx: &App,
    ) {
        self.tree = MuxTreeModel::from_mux(mux);
        self.chooser_current_window = None;
        let query = query.trim();
        self.rows.clear();
        if self.command.is_some() {
            self.rows = self.navigation_entries(query, mux);
            return;
        }
        match self.mode {
            Some(PaletteMode::Window | PaletteMode::Pane) => {
                self.rows = self.navigation_entries(query, mux);
            }
            Some(PaletteMode::Host) => {
                for host in &self.tree.hosts {
                    let Some(entry) = host_entry(host, cx).matching(query, "") else {
                        continue;
                    };
                    let mut entry = entry;
                    entry.row.expanded = (query.is_empty() && host.connected())
                        .then_some(!self.collapsed.contains(&host.id));
                    self.rows.push(entry);
                    if query.is_empty() && host.connected() && !self.collapsed.contains(&host.id) {
                        for session in &host.sessions {
                            let mut entry = session_entry(host, session, mux);
                            entry.row.indent = 18.0;
                            self.rows.push(entry);
                        }
                    }
                }
                if !query.is_empty() {
                    self.rows.sort_by_key(|entry| entry.score);
                }
            }
            Some(PaletteMode::Command) => {
                self.rows = command_entries(query, mux, availability, self.show_keys);
            }
            None => {
                let navigation = self.navigation_entries(query, mux);
                append_section(
                    &mut self.rows,
                    if query.is_empty() {
                        "Workspace"
                    } else {
                        "Navigation"
                    },
                    "@",
                    navigation,
                );
                if query.is_empty() {
                    let mut seen_commands = BTreeSet::new();
                    append_section(
                        &mut self.rows,
                        "Recent commands",
                        ":",
                        history
                            .iter()
                            .filter(|value| {
                                seen_commands
                                    .insert(value.split_whitespace().next().unwrap_or(value))
                            })
                            .take(3)
                            .map(|value| {
                                PaletteEntry::row(
                                    PaletteRow {
                                        label: value
                                            .split_whitespace()
                                            .next()
                                            .unwrap_or(value)
                                            .to_owned()
                                            .into(),
                                        icon: Some(IconName::Terminal),
                                        detail: zz_protocol::catalog_command_spec(
                                            value.split_whitespace().next().unwrap_or(value),
                                        )
                                        .map_or("", |spec| spec.description)
                                        .into(),
                                        ..Default::default()
                                    },
                                    zz_protocol::catalog_command_spec(
                                        value.split_whitespace().next().unwrap_or(value),
                                    )
                                    .map_or_else(
                                        || PaletteAction::History(value.clone()),
                                        PaletteAction::Command,
                                    ),
                                )
                            })
                            .collect(),
                    );
                } else {
                    append_section(
                        &mut self.rows,
                        "Commands",
                        ":",
                        command_entries(query, mux, availability, self.show_keys),
                    );
                }
                if self.host.is_none() {
                    let mut hosts: Vec<_> = self
                        .tree
                        .hosts
                        .iter()
                        .filter_map(|host| host_entry(host, cx).matching(query, ""))
                        .collect();
                    hosts.sort_by_key(|entry| entry.score);
                    append_section(&mut self.rows, "Hosts", self.host_prefix, hosts);
                }
            }
        }
    }
}

fn append_section(
    rows: &mut Vec<PaletteEntry>,
    label: &'static str,
    hint: &'static str,
    entries: Vec<PaletteEntry>,
) {
    if entries.is_empty() {
        return;
    }
    rows.push(PaletteEntry::header(label, hint));
    rows.extend(entries);
}

fn command_entries(
    query: &str,
    mux: &MuxClient,
    availability: PaneKindAvailability,
    show_keys: bool,
) -> Vec<PaletteEntry> {
    let mut entries: Vec<_> = command_specs()
        .filter(|spec| availability.allows_command(spec.name))
        .filter_map(|spec| {
            let shortcut = show_keys
                .then(|| {
                    mux.prefix_bindings()
                        .iter()
                        .find(|binding| {
                            binding.commands.len() == 1 && binding.commands[0].name == spec.name
                        })
                        .map(|binding| {
                            format!(
                                "{} {}",
                                mux.canonical_prefix()
                                    .unwrap_or_else(|| "prefix".to_owned()),
                                binding.key
                            )
                            .into()
                        })
                })
                .flatten();
            PaletteEntry::row(
                PaletteRow {
                    label: spec.name.into(),
                    detail: spec.description.into(),
                    icon: Some(IconName::Terminal),
                    shortcut,
                    ..Default::default()
                },
                PaletteAction::Command(spec),
            )
            .matching(query, &spec.aliases.join(" "))
        })
        .collect();
    entries.sort_by_key(|entry| entry.score);
    entries
}

fn session_entry(host: &MuxTreeHost, session: &MuxTreeSession, mux: &MuxClient) -> PaletteEntry {
    PaletteEntry::row(
        PaletteRow {
            label: session.name.clone().into(),
            detail: count(session.windows.len(), "window").into(),
            right: if session.active { "current" } else { "" }.into(),
            icon: Some(IconName::Folder),
            running: session
                .windows
                .iter()
                .any(|window| running_agent(host, window, mux)),
            ..Default::default()
        },
        PaletteAction::Target {
            host: host.id,
            target: session.target(),
            label: format!("{} on {}", session.name, host.name),
        },
    )
}

fn window_entry(
    host: &MuxTreeHost,
    session: &MuxTreeSession,
    window: &MuxTreeWindow,
    mux: &MuxClient,
    grouped: bool,
) -> PaletteEntry {
    let prefix = format!("{} / ", session.name);
    let label = format!("{prefix}{}", window.name);
    PaletteEntry::row(
        PaletteRow {
            label: if grouped {
                window.name.clone()
            } else {
                label.clone()
            }
            .into(),
            muted_prefix: if grouped { 0 } else { prefix.len() },
            detail: count(window.panes.len(), "pane").into(),
            right: if window.active { "current" } else { "" }.into(),
            icon: Some(IconName::PanelsTopLeft),
            running: running_agent(host, window, mux),
            ..Default::default()
        },
        PaletteAction::Target {
            host: host.id,
            target: TreeTarget::Window(window.id),
            label,
        },
    )
}

fn pane_entry(
    host: &MuxTreeHost,
    session: &MuxTreeSession,
    window: &MuxTreeWindow,
    pane: &MuxTreePane,
    mux: &MuxClient,
) -> PaletteEntry {
    let detail = match pane.kind {
        MuxTreePaneKind::Agent(_) if host.id == mux.attached_host() => {
            match mux.agent_attention_status(pane.id) {
                Some(zz_client::AgentAttentionStatus::Working) => "Agent · running",
                Some(zz_client::AgentAttentionStatus::NeedsInput) => "Agent · waiting for input",
                Some(zz_client::AgentAttentionStatus::Idle) => "Agent · idle",
                Some(zz_client::AgentAttentionStatus::Failed) => "Agent · failed",
                None => "Agent",
            }
        }
        MuxTreePaneKind::Agent(_) => "Agent",
        MuxTreePaneKind::Terminal => "Terminal",
        MuxTreePaneKind::Browser => "Browser",
        MuxTreePaneKind::Editor => "Editor",
        MuxTreePaneKind::Picker => "New pane",
    };
    PaletteEntry::row(
        PaletteRow {
            label: pane.label.clone().into(),
            detail: detail.into(),
            icon: Some(match pane.kind {
                MuxTreePaneKind::Picker => IconName::Plus,
                MuxTreePaneKind::Terminal => IconName::SquareTerminal,
                MuxTreePaneKind::Browser => IconName::Globe,
                MuxTreePaneKind::Agent(provider) => zz_ui::pane::agent_provider_icon(provider),
                MuxTreePaneKind::Editor => IconName::File,
            }),
            right: if window.active && window.active_pane == pane.id {
                "current"
            } else {
                ""
            }
            .into(),
            ..Default::default()
        },
        PaletteAction::Target {
            host: host.id,
            target: TreeTarget::Pane(pane.id),
            label: format!("{} / {} / {}", session.name, window.name, pane.label),
        },
    )
}

fn running_agent(host: &MuxTreeHost, window: &MuxTreeWindow, mux: &MuxClient) -> bool {
    host.id == mux.attached_host()
        && window.panes.iter().any(|pane| {
            matches!(pane.kind, MuxTreePaneKind::Agent(_))
                && mux.agent_attention_status(pane.id)
                    == Some(zz_client::AgentAttentionStatus::Working)
        })
}

fn host_entry(host: &MuxTreeHost, cx: &App) -> PaletteEntry {
    let detail = if host.id == HostId::LOCAL {
        if cfg!(target_os = "macos") {
            "This Mac".to_owned()
        } else {
            "This computer".to_owned()
        }
    } else {
        crate::config::fleet_hosts(cx)
            .iter()
            .find(|entry| entry.name == host.name)
            .map_or_else(
                || "Remote host".to_owned(),
                |entry| entry.endpoint.to_string(),
            )
    };
    let right = match host.state {
        HostState::Connected => count(host.sessions.len(), "session"),
        HostState::Connecting => "connecting…".to_owned(),
        HostState::Reconnecting { .. } => "reconnecting…".to_owned(),
        HostState::Incompatible { .. } => "incompatible version".to_owned(),
        _ => "offline · ↵ to connect".to_owned(),
    };
    PaletteEntry::row(
        PaletteRow {
            label: host.name.clone().into(),
            detail: detail.into(),
            icon: Some(IconName::HardDrive),
            right: right.into(),
            status: Some(host_status(&host.state)),
            ..Default::default()
        },
        PaletteAction::Host(host.id),
    )
}

fn host_status(state: &HostState) -> PaletteStatus {
    match state {
        HostState::Connected => PaletteStatus::Online,
        HostState::Connecting | HostState::Reconnecting { .. } => PaletteStatus::Waiting,
        _ => PaletteStatus::Offline,
    }
}

pub(super) fn target_kind(spec: &CommandSpec) -> Option<CommandValueKind> {
    if !matches!(
        spec.name,
        "join-pane"
            | "move-pane"
            | "swap-pane"
            | "swap-window"
            | "move-window"
            | "link-window"
            | "unlink-window"
            | "kill-pane"
            | "kill-window"
            | "select-pane"
            | "select-window"
            | "resize-pane"
            | "capture-pane"
            | "respawn-pane"
            | "respawn-window"
            | "restart-agent-pane"
    ) {
        return None;
    }
    if spec.name == "join-pane" {
        return Some(CommandValueKind::Window);
    }
    spec.option("-t")
        .filter(|option| option.completable && !option.unsupported)
        .and_then(|option| option.value)
        .filter(|kind| matches!(kind, CommandValueKind::Window | CommandValueKind::Pane))
}

pub(super) fn needs_arguments(spec: &CommandSpec) -> bool {
    let mut optional_depth = 0_u32;
    spec.usage.chars().any(|character| match character {
        '[' => {
            optional_depth += 1;
            false
        }
        ']' => {
            optional_depth = optional_depth.saturating_sub(1);
            false
        }
        character => optional_depth == 0 && !character.is_whitespace(),
    })
}

fn count(value: usize, noun: &str) -> String {
    format!("{value} {noun}{}", if value == 1 { "" } else { "s" })
}

pub(super) fn fuzzy_match(query: &str, label: &str) -> Option<(usize, Vec<Range<usize>>)> {
    let mut needle = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .peekable();
    if needle.peek().is_none() {
        return Some((0, Vec::new()));
    }
    let mut hits = Vec::new();
    let mut first = None;
    let mut previous = 0;
    let mut gaps = 0;
    for (position, (byte, character)) in label.char_indices().enumerate() {
        if character
            .to_lowercase()
            .any(|ch| Some(&ch) == needle.peek())
        {
            needle.next();
            if first.is_none() {
                first = Some(position);
            } else {
                gaps += position - previous - 1;
            }
            previous = position;
            hits.push(byte..byte + character.len_utf8());
            if needle.peek().is_none() {
                return Some((first.unwrap_or_default() + 2 * gaps, hits));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use gpui::AppContext as _;

    use super::*;

    fn chooser_item(label: &str, target: ChooseTreeTarget) -> zz_protocol::ChooseTreeItem {
        zz_protocol::ChooseTreeItem {
            label: label.to_owned(),
            detail: String::new(),
            target,
            depth: 0,
            flags: 0,
            pane_kind: None,
            key: String::new(),
            text: String::new(),
        }
    }

    fn window_chooser_state() -> ChooseTreeState {
        use zz_protocol::{ClientId, PaneId, SessionId, WindowId};
        let mut items = vec![
            chooser_item("dev", ChooseTreeTarget::Session(SessionId(1))),
            chooser_item("3:server", ChooseTreeTarget::Window(WindowId(10))),
            chooser_item("shell", ChooseTreeTarget::Pane(PaneId(11))),
            chooser_item("scratch", ChooseTreeTarget::Session(SessionId(2))),
            chooser_item("viewer", ChooseTreeTarget::Client(ClientId(12))),
            chooser_item("4:docs", ChooseTreeTarget::Window(WindowId(20))),
            chooser_item("plain:name", ChooseTreeTarget::Window(WindowId(21))),
        ];
        items[0].detail = "1 window".to_owned();
        items[0].flags = zz_protocol::ChooseTreeItem::ACTIVE
            | zz_protocol::ChooseTreeItem::HAS_CHILDREN
            | zz_protocol::ChooseTreeItem::EXPANDED;
        items[1].detail = "2 panes".to_owned();
        items[1].flags = zz_protocol::ChooseTreeItem::ACTIVE
            | zz_protocol::ChooseTreeItem::HAS_CHILDREN
            | zz_protocol::ChooseTreeItem::EXPANDED;
        items[1].depth = 1;
        items[2].depth = 2;
        items[2].flags = zz_protocol::ChooseTreeItem::ACTIVE;
        items[2].pane_kind = Some(zz_protocol::ChooseTreePaneKind::Terminal);
        items[1].key = "1".to_owned();
        items[3].detail = "2 windows".to_owned();
        items[3].flags =
            zz_protocol::ChooseTreeItem::HAS_CHILDREN | zz_protocol::ChooseTreeItem::EXPANDED;
        items[5].text = "<<reference>>".to_owned();
        items[5].detail = "files".to_owned();
        items[5].flags =
            zz_protocol::ChooseTreeItem::ACTIVE | zz_protocol::ChooseTreeItem::HAS_CHILDREN;
        items[5].depth = 1;
        items[6].depth = 1;
        items[5].key = "a".to_owned();
        ChooseTreeState {
            items,
            search: None,
            selected: 1,
            kind: zz_protocol::ChooseTreeKind::Windows,
            filter_no_matches: false,
            prompt: String::new(),
            help: false,
        }
    }

    fn chooser_indices(state: &UnifiedPalette) -> Vec<u32> {
        state
            .rows
            .iter()
            .filter_map(|entry| match entry.action {
                Some(PaletteAction::ChooseWindow(index)) => Some(index),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn daemon_tree_keeps_source_indices_and_selectable_sessions_windows_and_panes() {
        let source = window_chooser_state();
        let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
        state.rebuild_window_chooser(&source, "", &MuxSnapshot::default());
        assert_eq!(chooser_indices(&state), [0, 1, 2, 3, 5, 6]);
        assert_eq!(
            state
                .rows
                .iter()
                .map(|entry| entry.row.label.as_ref())
                .collect::<Vec<_>>(),
            [
                "dev",
                "server",
                "shell",
                "scratch",
                "<<reference>>",
                "plain:name"
            ]
        );
        assert!(state.rows.iter().all(|entry| entry.action.is_some()));
        assert_eq!(state.rows[0].row.detail, "1 window");
        assert_eq!(state.rows[1].row.right, "current");
        assert_eq!(state.rows[2].row.right, "current");
        assert!(state.rows[4].row.right.is_empty());
        assert!(state.rows[4].row.detail.is_empty());
        assert!(state.rows.iter().all(|entry| entry.row.shortcut.is_none()));
        assert_eq!(state.rows[0].row.expanded, Some(true));
        assert_eq!(state.rows[1].row.expanded, Some(true));
        assert_eq!(state.rows[4].row.expanded, Some(false));
        assert_eq!(state.rows[2].row.expanded, None);
        assert_eq!(state.rows[2].row.indent, 36.0);
        assert_eq!(state.rows[2].row.icon, Some(IconName::SquareTerminal));
        assert_eq!(state.parent_index(2), Some(1));
        assert_eq!(state.parent_index(1), Some(0));
        assert_eq!(state.parent_index(3), None);
        assert_eq!(state.child_index(0), Some(1));
        assert_eq!(state.child_index(4), None);
        assert_eq!(state.initial_selection(), Some(1));

        state.grouped = false;
        state.rebuild_window_chooser(&source, "", &MuxSnapshot::default());
        assert_eq!(chooser_indices(&state), [0, 1, 2, 3, 5, 6]);
        assert_eq!(state.rows[1].row.label, "dev / server");
        assert_eq!(state.rows[2].row.label, "dev / server / shell");
        assert_eq!(state.rows[4].row.label, "scratch / <<reference>>");
        assert_eq!(state.rows[1].row.muted_prefix, "dev / ".len());
        assert!(
            state
                .rows
                .iter()
                .all(|entry| entry.row.indent == 0.0 && entry.row.expanded.is_none())
        );
    }

    #[test]
    fn daemon_window_search_keeps_custom_text_and_matches_hidden_target_metadata() {
        let source = window_chooser_state();
        let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
        for query in ["ref", "docs", "files", "@20"] {
            state.rebuild_window_chooser(&source, query, &MuxSnapshot::default());
            assert_eq!(chooser_indices(&state), [5], "query={query}");
            assert_eq!(state.rows[0].row.label, "scratch / <<reference>>");
            assert_eq!(state.rows[0].row.muted_prefix, "scratch / ".len());
        }
        state.rebuild_window_chooser(&source, "ref", &MuxSnapshot::default());
        assert!(!state.rows[0].row.matches.is_empty());
        state.rebuild_window_chooser(&source, "scratch", &MuxSnapshot::default());
        assert_eq!(chooser_indices(&state), [3, 5, 6]);
        state.rebuild_window_chooser(&source, "missing", &MuxSnapshot::default());
        assert!(state.rows.is_empty());
    }

    #[test]
    fn daemon_session_rows_remain_selectable_without_visible_children() {
        let mut source = window_chooser_state();
        source
            .items
            .retain(|item| matches!(item.target, ChooseTreeTarget::Session(_)));
        source.items[1].text = "<<scratch>>".to_owned();
        let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
        state.rebuild_window_chooser(&source, "", &MuxSnapshot::default());
        assert_eq!(chooser_indices(&state), [0, 1]);
        assert_eq!(state.rows[0].row.label, "dev");
        assert_eq!(state.rows[1].row.label, "<<scratch>>");
        state.rebuild_window_chooser(&source, "$2", &MuxSnapshot::default());
        assert_eq!(chooser_indices(&state), [1]);
    }

    fn navigation_tree() -> MuxTreeModel {
        use zz_protocol::{PaneId, SessionId, WindowId};
        let window = |id, name: &str, active, panes: &[&str]| MuxTreeWindow {
            id: WindowId(id),
            index: 0,
            name: name.to_owned(),
            active_pane: PaneId(id * 10),
            active,
            panes: panes
                .iter()
                .enumerate()
                .map(|(index, name)| MuxTreePane {
                    id: PaneId(id * 10 + index as u64),
                    label: (*name).to_owned(),
                    kind: MuxTreePaneKind::Terminal,
                    bell: false,
                })
                .collect(),
        };
        MuxTreeModel {
            hosts: vec![MuxTreeHost {
                id: HostId::LOCAL,
                name: "local".to_owned(),
                state: HostState::Connected,
                snapshot_loaded: true,
                sessions: vec![
                    MuxTreeSession {
                        id: SessionId(1),
                        name: "dev".to_owned(),
                        active: true,
                        windows: vec![
                            window(1, "editor", true, &["nvim", "cargo watch"]),
                            window(2, "logs", false, &["daemon logs"]),
                        ],
                    },
                    MuxTreeSession {
                        id: SessionId(2),
                        name: "scratch".to_owned(),
                        active: false,
                        windows: vec![window(3, "experiments", false, &["python hidden"])],
                    },
                ],
            }],
            active_target: Some(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(10)),
            )),
        }
    }

    fn disconnected_mux(cx: &mut App) -> gpui::Entity<MuxClient> {
        cx.new(|cx| {
            MuxClient::new(
                Err(zz_daemon::DaemonError::Thread("palette model".to_owned())),
                zz_daemon::default_socket_path(),
                cx,
            )
        })
    }

    fn row_labels(state: &UnifiedPalette) -> Vec<&str> {
        state
            .rows
            .iter()
            .map(|entry| entry.row.label.as_ref())
            .collect()
    }

    #[gpui::test]
    fn default_tree_expands_current_session_and_preserves_local_branch_choices(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let mux = disconnected_mux(cx);
            let mux = mux.read(cx);
            let mut state = UnifiedPalette::new(None);
            state.tree = navigation_tree();
            state.rows = state.navigation_entries("", mux);
            assert_eq!(row_labels(&state), ["dev", "editor", "logs", "scratch"]);
            assert_eq!(state.initial_selection(), Some(1));
            assert_eq!(state.rows[0].row.expanded, Some(true));
            assert_eq!(state.rows[1].row.expanded, Some(false));
            assert_eq!(state.rows[3].row.expanded, Some(false));
            assert!(state.set_expanded(1, true));
            state.rows = state.navigation_entries("", mux);
            assert_eq!(
                row_labels(&state),
                ["dev", "editor", "nvim", "cargo watch", "logs", "scratch"]
            );
            assert_eq!(state.rows[2].row.indent, 36.0);
            assert_eq!(state.rows[2].row.right, "current");
            assert_eq!(state.child_index(1), Some(2));
            assert_eq!(state.parent_index(2), Some(1));
            assert_eq!(state.parent_index(1), Some(0));
            assert!(!state.set_expanded(2, true));
            assert!(state.set_expanded(0, false));
            state.rows = state.navigation_entries("", mux);
            assert_eq!(row_labels(&state), ["dev", "scratch"]);
            assert_eq!(state.initial_selection(), Some(0));
            assert!(state.set_expanded(0, true));
            state.rows = state.navigation_entries("", mux);
            assert_eq!(
                row_labels(&state),
                ["dev", "editor", "nvim", "cargo watch", "logs", "scratch"]
            );
        });
    }

    #[gpui::test]
    fn navigate_entry_opens_sessions_and_search_finds_hidden_descendants(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let mux = disconnected_mux(cx);
            let mux = mux.read(cx);
            let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
            state.tree = navigation_tree();
            state.rows = state.navigation_entries("", mux);
            assert_eq!(
                row_labels(&state),
                ["dev", "editor", "logs", "scratch", "experiments"]
            );
            assert_eq!(PaletteMode::Window.label(), "Navigate");
            assert_eq!(state.placeholder(), "Search sessions, windows, panes");
            assert!(state.set_expanded(3, false));
            state.rows = state.navigation_entries("python", mux);
            assert_eq!(
                row_labels(&state),
                ["scratch / experiments / python hidden"]
            );
            assert_eq!(
                state.rows[0].row.muted_prefix,
                "scratch / experiments / ".len()
            );
            assert!(state.rows[0].row.expanded.is_none());
            assert_eq!(state.parent_index(0), None);
            assert!(!state.rows[0].row.matches.is_empty());
            state.rows = state.navigation_entries("%30", mux);
            assert_eq!(
                row_labels(&state),
                ["scratch / experiments / python hidden"]
            );
            state.rows = state.navigation_entries("scratch", mux);
            assert_eq!(
                row_labels(&state),
                [
                    "scratch",
                    "scratch / experiments",
                    "scratch / experiments / python hidden"
                ]
            );
            state.rows = state.navigation_entries("", mux);
            assert_eq!(row_labels(&state), ["dev", "editor", "logs", "scratch"]);
            state.command = zz_protocol::catalog_command_spec("join-pane");
            state.rows = state.navigation_entries("", mux);
            assert_eq!(
                row_labels(&state),
                ["dev / editor", "dev / logs", "scratch / experiments"]
            );
            assert!(state.rows.iter().all(|entry| matches!(
                entry.action,
                Some(PaletteAction::Target {
                    target: TreeTarget::Window(_),
                    ..
                })
            )));
        });
    }

    #[gpui::test]
    fn default_recent_commands_are_unique_and_precede_hosts(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mux = disconnected_mux(cx);
            let history = [
                "rename-window one",
                "rename-window two",
                "new-window",
                "list-panes",
                "kill-pane",
            ]
            .map(str::to_owned);
            let mut state = UnifiedPalette::new(None);
            state.rebuild(
                "",
                &history,
                mux.read(cx),
                PaneKindAvailability::default(),
                cx,
            );
            let headers = state
                .rows
                .iter()
                .filter(|entry| entry.action.is_none())
                .map(|entry| entry.row.label.as_ref())
                .collect::<Vec<_>>();
            assert_eq!(headers, ["Recent commands", "Hosts"]);
            let commands = state
                .rows
                .iter()
                .filter_map(|entry| match entry.action {
                    Some(PaletteAction::Command(spec)) => Some(spec.name),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(commands, ["rename-window", "new-window", "list-panes"]);
        });
    }

    #[test]
    fn fuzzy_matching_ignores_query_spaces_and_tracks_unicode_byte_ranges() {
        assert_eq!(fuzzy_match("α 界", "aΑ-界"), Some((3, vec![1..3, 4..7])));
        assert_eq!(
            fuzzy_match("pane", "join-pane"),
            Some((5, vec![5..6, 6..7, 7..8, 8..9]))
        );
        assert!(fuzzy_match("pnx", "pane").is_none());
    }

    #[test]
    fn backspace_pops_exactly_one_palette_layer() {
        let mut state = UnifiedPalette::new(Some(PaletteMode::Command));
        state.host = Some(HostId::LOCAL);
        state.command = zz_protocol::catalog_command_spec("join-pane");
        assert!(state.pop_layer());
        assert!(state.command.is_none());
        assert_eq!(state.mode, Some(PaletteMode::Command));
        assert!(state.pop_layer());
        assert_eq!(state.host, Some(HostId::LOCAL));
        assert!(state.pop_layer());
        assert!(!state.pop_layer());
    }

    #[test]
    fn host_prefix_is_configurable_without_stealing_the_alternative() {
        assert_eq!(PaletteMode::from_prefix("#", "#"), Some(PaletteMode::Host));
        assert_eq!(PaletteMode::from_prefix("~", "#"), None);
        assert_eq!(
            PaletteMode::from_prefix("@", "~"),
            Some(PaletteMode::Window)
        );
    }

    #[test]
    fn navigation_skips_headers_and_wraps_from_the_initial_selection() {
        let mut state = UnifiedPalette::new(None);
        state.rows = vec![
            PaletteEntry::header("Windows", "@"),
            PaletteEntry::row(PaletteRow::default(), PaletteAction::Host(HostId::LOCAL)),
            PaletteEntry::header("Commands", ":"),
            PaletteEntry::row(
                PaletteRow::default(),
                PaletteAction::History("list-panes".to_owned()),
            ),
        ];
        assert_eq!(state.navigate(Some(1), 1), Some(3));
        assert_eq!(state.navigate(Some(3), 1), Some(1));
        assert_eq!(state.navigate(Some(1), -1), Some(3));
        state.rows.clear();
        assert_eq!(state.navigate(None, 1), None);
    }

    #[test]
    fn target_step_supports_join_without_forcing_optional_new_window_targets() {
        assert_eq!(
            target_kind(zz_protocol::catalog_command_spec("join-pane").unwrap()),
            Some(CommandValueKind::Window)
        );
        assert_eq!(
            target_kind(zz_protocol::catalog_command_spec("kill-pane").unwrap()),
            Some(CommandValueKind::Pane)
        );
        assert_eq!(
            target_kind(zz_protocol::catalog_command_spec("new-window").unwrap()),
            None
        );
        assert_eq!(
            target_kind(zz_protocol::catalog_command_spec("split-window").unwrap()),
            None
        );
        assert_eq!(
            target_kind(zz_protocol::catalog_command_spec("rename-window").unwrap()),
            None
        );
        assert!(!needs_arguments(
            zz_protocol::catalog_command_spec("new-window").unwrap()
        ));
        assert!(!needs_arguments(
            zz_protocol::catalog_command_spec("split-window").unwrap()
        ));
        assert!(needs_arguments(
            zz_protocol::catalog_command_spec("rename-window").unwrap()
        ));
        assert!(needs_arguments(
            zz_protocol::catalog_command_spec("if-shell").unwrap()
        ));
    }
}
