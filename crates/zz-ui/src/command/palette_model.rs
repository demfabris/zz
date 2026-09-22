use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    ops::Range,
};

use gpui::SharedString;
use zz_client::completion::{CompletionSuggestion, PaneKindAvailability};
use zz_protocol::{
    ChooseTreeState, ChooseTreeTarget, CommandSpec, CommandValueKind, MuxSnapshot, PaneId,
    PaneKindSnapshot, SessionId, WindowId, command_specs,
};

use super::{PalettePill, PaletteRow, PaletteStatus};
use crate::IconName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    Command,
    Window,
    Pane,
    Host,
}

impl PaletteMode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Window => "Navigate",
            Self::Pane => "Pane",
            Self::Host => "Host",
        }
    }

    #[must_use]
    pub const fn prefix(self, host_prefix: &'static str) -> &'static str {
        match self {
            Self::Command => ":",
            Self::Window => "@",
            Self::Pane => "%",
            Self::Host => host_prefix,
        }
    }

    #[must_use]
    pub fn from_prefix(value: &str, host_prefix: &str) -> Option<Self> {
        match value {
            ":" => Some(Self::Command),
            "@" => Some(Self::Window),
            "%" => Some(Self::Pane),
            value if value == host_prefix => Some(Self::Host),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaletteHostId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaletteTarget {
    Session(SessionId),
    Window(WindowId),
    Pane(PaneId),
}

impl fmt::Display for PaletteTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(id) => id.fmt(formatter),
            Self::Window(id) => id.fmt(formatter),
            Self::Pane(id) => id.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteSettings {
    pub grouped: bool,
    pub host_prefix: &'static str,
    pub show_keys: bool,
}

impl Default for PaletteSettings {
    fn default() -> Self {
        Self {
            grouped: true,
            host_prefix: "~",
            show_keys: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PaletteTree {
    pub attached: PaletteHostId,
    pub hosts: Vec<PaletteTreeHost>,
}

impl PaletteTree {
    #[must_use]
    pub fn host(&self, id: PaletteHostId) -> Option<&PaletteTreeHost> {
        self.hosts.iter().find(|host| host.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct PaletteTreeHost {
    pub id: PaletteHostId,
    pub name: String,
    pub detail: String,
    pub right: String,
    pub status: PaletteStatus,
    pub sessions: Vec<PaletteTreeSession>,
}

impl PaletteTreeHost {
    #[must_use]
    pub fn connected(&self) -> bool {
        self.status == PaletteStatus::Online
    }
}

#[derive(Clone, Debug)]
pub struct PaletteTreeSession {
    pub id: SessionId,
    pub name: String,
    pub active: bool,
    pub windows: Vec<PaletteTreeWindow>,
}

#[derive(Clone, Debug)]
pub struct PaletteTreeWindow {
    pub id: WindowId,
    pub name: String,
    pub active: bool,
    pub active_pane: PaneId,
    pub panes: Vec<PaletteTreePane>,
}

impl PaletteTreeWindow {
    fn running(&self) -> bool {
        self.panes.iter().any(|pane| pane.running)
    }
}

#[derive(Clone, Debug)]
pub struct PaletteTreePane {
    pub id: PaneId,
    pub label: String,
    pub detail: SharedString,
    pub icon: IconName,
    pub running: bool,
}

#[derive(Clone)]
pub(crate) enum PaletteAction {
    Host(PaletteHostId),
    Target {
        host: PaletteHostId,
        target: PaletteTarget,
        label: String,
    },
    Command(&'static CommandSpec),
    History(String),
    Completion(CompletionSuggestion),
    ChooseWindow(u32),
}

#[derive(Clone)]
pub(crate) struct PaletteEntry {
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

    fn header(label: impl Into<SharedString>, hint: &'static str) -> Self {
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

pub(crate) struct UnifiedPalette {
    pub mode: Option<PaletteMode>,
    pub host: Option<PaletteHostId>,
    pub command: Option<&'static CommandSpec>,
    pub tree: PaletteTree,
    pub rows: Vec<PaletteEntry>,
    pub collapsed: BTreeSet<PaletteHostId>,
    expanded: BTreeMap<(PaletteHostId, PaletteTarget), bool>,
    chooser_current_window: Option<u32>,
    pub grouped: bool,
    pub host_prefix: &'static str,
    pub show_keys: bool,
}

impl UnifiedPalette {
    pub fn new(mode: Option<PaletteMode>) -> Self {
        let settings = PaletteSettings::default();
        Self {
            mode,
            host: None,
            command: None,
            tree: PaletteTree::default(),
            rows: Vec::new(),
            collapsed: BTreeSet::new(),
            expanded: BTreeMap::new(),
            chooser_current_window: None,
            grouped: settings.grouped,
            host_prefix: settings.host_prefix,
            show_keys: settings.show_keys,
        }
    }

    pub fn apply_settings(&mut self, settings: PaletteSettings) {
        self.grouped = settings.grouped;
        self.host_prefix = settings.host_prefix;
        self.show_keys = settings.show_keys;
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
                            target: PaletteTarget::Window(_),
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
                self.expanded.insert((host, target), expanded);
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
                status: host.status,
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
                                    Some(crate::pane::agent_provider_icon(agent.provider))
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

    fn navigation_entries(&self, query: &str) -> Vec<PaletteEntry> {
        let tree = self.grouped && query.is_empty() && self.is_navigation_tree();
        let target_kind = self.command.and_then(target_kind);
        let pane_only =
            self.mode == Some(PaletteMode::Pane) || target_kind == Some(CommandValueKind::Pane);
        let window_only = target_kind == Some(CommandValueKind::Window);
        let scope = self
            .host
            .or_else(|| self.command.map(|_| self.tree.attached));
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
                    .get(&(host.id, PaletteTarget::Session(session.id)))
                    .copied()
                    .unwrap_or(self.mode == Some(PaletteMode::Window) || session.active);
                if !pane_only && !window_only {
                    let mut entry = session_entry(host, session);
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
                        .get(&(host.id, PaletteTarget::Window(window.id)))
                        .copied()
                        .unwrap_or(false);
                    if !pane_only {
                        let mut entry = window_entry(host, session, window, tree);
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
                        let mut entry = pane_entry(host, session, window, pane);
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
        tree: PaletteTree,
        availability: PaneKindAvailability,
        shortcut: &dyn Fn(&str) -> Option<SharedString>,
    ) {
        self.tree = tree;
        self.chooser_current_window = None;
        let query = query.trim();
        self.rows.clear();
        if self.command.is_some() {
            self.rows = self.navigation_entries(query);
            return;
        }
        let show_keys = self.show_keys;
        let shortcut = move |name: &str| show_keys.then(|| shortcut(name)).flatten();
        match self.mode {
            Some(PaletteMode::Window | PaletteMode::Pane) => {
                self.rows = self.navigation_entries(query);
            }
            Some(PaletteMode::Host) => {
                let mut rows = Vec::new();
                for host in &self.tree.hosts {
                    let Some(mut entry) = host_entry(host).matching(query, "") else {
                        continue;
                    };
                    let open = !self.collapsed.contains(&host.id);
                    entry.row.expanded = (query.is_empty() && host.connected()).then_some(open);
                    rows.push(entry);
                    if query.is_empty() && host.connected() && open {
                        for session in &host.sessions {
                            let mut entry = session_entry(host, session);
                            entry.row.indent = 18.0;
                            rows.push(entry);
                        }
                    }
                }
                if !query.is_empty() {
                    rows.sort_by_key(|entry| entry.score);
                }
                self.rows = rows;
            }
            Some(PaletteMode::Command) => {
                self.rows = command_entries(query, availability, &shortcut);
            }
            None => {
                let mut rows = Vec::new();
                append_section(
                    &mut rows,
                    if query.is_empty() {
                        "Workspace"
                    } else {
                        "Navigation"
                    },
                    "@",
                    self.navigation_entries(query),
                );
                if query.is_empty() {
                    let mut seen_commands = BTreeSet::new();
                    append_section(
                        &mut rows,
                        "Recent commands",
                        ":",
                        history
                            .iter()
                            .map(|value| (value, value.split_whitespace().next().unwrap_or(value)))
                            .filter(|(_, name)| availability.allows_command(name))
                            .filter(|(_, name)| seen_commands.insert(*name))
                            .take(3)
                            .map(|(value, name)| {
                                let spec = zz_protocol::catalog_command_spec(name);
                                PaletteEntry::row(
                                    PaletteRow {
                                        label: name.to_owned().into(),
                                        icon: Some(IconName::Terminal),
                                        detail: spec.map_or("", |spec| spec.description).into(),
                                        ..Default::default()
                                    },
                                    spec.map_or_else(
                                        || PaletteAction::History(value.clone()),
                                        PaletteAction::Command,
                                    ),
                                )
                            })
                            .collect(),
                    );
                } else {
                    append_section(
                        &mut rows,
                        "Commands",
                        ":",
                        command_entries(query, availability, &shortcut),
                    );
                }
                if self.host.is_none() {
                    let mut hosts: Vec<_> = self
                        .tree
                        .hosts
                        .iter()
                        .filter_map(|host| host_entry(host).matching(query, ""))
                        .collect();
                    hosts.sort_by_key(|entry| entry.score);
                    append_section(&mut rows, "Hosts", self.host_prefix, hosts);
                }
                self.rows = rows;
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
    availability: PaneKindAvailability,
    shortcut: &dyn Fn(&str) -> Option<SharedString>,
) -> Vec<PaletteEntry> {
    let mut entries: Vec<_> = command_specs()
        .filter(|spec| availability.allows_command(spec.name))
        .filter_map(|spec| {
            PaletteEntry::row(
                PaletteRow {
                    label: spec.name.into(),
                    detail: spec.description.into(),
                    icon: Some(IconName::Terminal),
                    shortcut: shortcut(spec.name),
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

fn session_entry(host: &PaletteTreeHost, session: &PaletteTreeSession) -> PaletteEntry {
    PaletteEntry::row(
        PaletteRow {
            label: session.name.clone().into(),
            detail: count(session.windows.len(), "window").into(),
            right: if session.active { "current" } else { "" }.into(),
            icon: Some(IconName::Folder),
            running: session.windows.iter().any(PaletteTreeWindow::running),
            ..Default::default()
        },
        PaletteAction::Target {
            host: host.id,
            target: PaletteTarget::Session(session.id),
            label: format!("{} on {}", session.name, host.name),
        },
    )
}

fn window_entry(
    host: &PaletteTreeHost,
    session: &PaletteTreeSession,
    window: &PaletteTreeWindow,
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
            running: window.running(),
            ..Default::default()
        },
        PaletteAction::Target {
            host: host.id,
            target: PaletteTarget::Window(window.id),
            label,
        },
    )
}

fn pane_entry(
    host: &PaletteTreeHost,
    session: &PaletteTreeSession,
    window: &PaletteTreeWindow,
    pane: &PaletteTreePane,
) -> PaletteEntry {
    PaletteEntry::row(
        PaletteRow {
            label: pane.label.clone().into(),
            detail: pane.detail.clone(),
            icon: Some(pane.icon.clone()),
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
            target: PaletteTarget::Pane(pane.id),
            label: format!("{} / {} / {}", session.name, window.name, pane.label),
        },
    )
}

fn host_entry(host: &PaletteTreeHost) -> PaletteEntry {
    PaletteEntry::row(
        PaletteRow {
            label: host.name.clone().into(),
            detail: host.detail.clone().into(),
            icon: Some(IconName::HardDrive),
            right: host.right.clone().into(),
            status: Some(host.status),
            ..Default::default()
        },
        PaletteAction::Host(host.id),
    )
}

pub(crate) fn target_kind(spec: &CommandSpec) -> Option<CommandValueKind> {
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

pub(crate) fn needs_arguments(spec: &CommandSpec) -> bool {
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

pub(crate) fn fuzzy_match(query: &str, label: &str) -> Option<(usize, Vec<Range<usize>>)> {
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
#[path = "palette_model_tests.rs"]
mod tests;
