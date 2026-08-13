//! Shared navigation model and activation machinery for mux-backed shells.

use std::collections::BTreeSet;

use gpui::{App, Entity, SharedString};
use zz_protocol::{
    Axis, CommandInvocation, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot, SessionId,
    WindowId, WindowSnapshot,
};

use super::{
    client::MuxClient,
    hosts::{HostId, HostState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TreeTarget {
    Session(SessionId),
    Window(WindowId),
    Pane(PaneId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TreeNode {
    Host(HostId),
    Target(HostId, TreeTarget),
}

impl TreeNode {
    #[must_use]
    pub fn tree_id(self) -> SharedString {
        match self {
            Self::Host(host) => format!("host:{host:?}").into(),
            Self::Target(host, target) => format!("host:{host:?}:{}", target.tree_id()).into(),
        }
    }

    #[must_use]
    pub const fn host(self) -> HostId {
        match self {
            Self::Host(host) | Self::Target(host, _) => host,
        }
    }
}

impl TreeTarget {
    #[must_use]
    pub fn tree_id(self) -> SharedString {
        match self {
            Self::Session(id) => format!("session:{id}").into(),
            Self::Window(id) => format!("window:{id}").into(),
            Self::Pane(id) => format!("pane:{id}").into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuxTreePaneKind {
    Picker,
    Terminal,
    Browser,
    Agent,
    Editor,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MuxTreeModel {
    pub hosts: Vec<MuxTreeHost>,
    pub active_target: Option<TreeNode>,
}

impl MuxTreeModel {
    #[must_use]
    pub fn from_mux(mux: &MuxClient) -> Self {
        Self::from_hosts(
            mux.attached_host(),
            mux.attached_session(),
            mux.fleet_hosts(),
        )
    }

    #[must_use]
    pub fn from_hosts<'a>(
        attached_host: HostId,
        attached: Option<SessionId>,
        hosts: impl IntoIterator<Item = (HostId, &'a str, &'a HostState, Option<&'a MuxSnapshot>)>,
    ) -> Self {
        let mut active_target = None;
        let hosts = hosts
            .into_iter()
            .map(|(host, name, state, snapshot)| {
                let snapshot_loaded = snapshot.is_some();
                let sessions = snapshot
                    .into_iter()
                    .flat_map(|snapshot| &snapshot.sessions)
                    .map(|session| {
                        let active = host == attached_host && Some(session.id) == attached;
                        let focused_window = snapshot.map_or(session.active_window, |snapshot| {
                            snapshot.focused_window_for(session)
                        });
                        let windows = session
                            .windows
                            .iter()
                            .map(|window| {
                                let window_active = active && window.id == focused_window;
                                let panes = ordered_panes(window)
                                    .into_iter()
                                    .map(|pane| {
                                        let pane_active =
                                            window_active && pane.id == window.active_pane;
                                        if pane_active {
                                            active_target = Some(TreeNode::Target(
                                                host,
                                                TreeTarget::Pane(pane.id),
                                            ));
                                        }
                                        MuxTreePane::from_snapshot(pane)
                                    })
                                    .collect();
                                MuxTreeWindow {
                                    id: window.id,
                                    index: window.index,
                                    name: window.name.clone(),
                                    active_pane: window.active_pane,
                                    active: window_active,
                                    panes,
                                }
                            })
                            .collect::<Vec<_>>();

                        if active && active_target.is_none() {
                            active_target = windows.iter().find(|window| window.active).map_or(
                                Some(TreeNode::Target(host, TreeTarget::Session(session.id))),
                                |window| {
                                    Some(TreeNode::Target(host, TreeTarget::Window(window.id)))
                                },
                            );
                        }

                        MuxTreeSession {
                            id: session.id,
                            name: session.name.clone(),
                            active,
                            windows,
                        }
                    })
                    .collect();
                MuxTreeHost {
                    id: host,
                    name: name.to_owned(),
                    state: state.clone(),
                    snapshot_loaded,
                    sessions,
                }
            })
            .collect();

        Self {
            hosts,
            active_target,
        }
    }

    #[must_use]
    pub fn is_expandable(&self, node: TreeNode) -> bool {
        match node {
            TreeNode::Host(host) => self.host(host).is_some(),
            TreeNode::Target(host, TreeTarget::Session(id)) => self
                .host(host)
                .and_then(|host| host.sessions.iter().find(|session| session.id == id))
                .is_some_and(|session| !session.windows.is_empty()),
            TreeNode::Target(host, TreeTarget::Window(id)) => self
                .host(host)
                .and_then(|host| {
                    host.sessions
                        .iter()
                        .flat_map(|session| &session.windows)
                        .find(|window| window.id == id)
                })
                .is_some_and(|window| !window.panes.is_empty()),
            TreeNode::Target(_, TreeTarget::Pane(_)) => false,
        }
    }

    #[must_use]
    pub fn host(&self, id: HostId) -> Option<&MuxTreeHost> {
        self.hosts.iter().find(|host| host.id == id)
    }

    #[must_use]
    pub fn session_count(&self) -> usize {
        self.hosts.iter().map(|host| host.sessions.len()).sum()
    }

    #[must_use]
    pub fn session_for_target(&self, host: HostId, target: TreeTarget) -> Option<SessionId> {
        let sessions = &self.host(host)?.sessions;
        match target {
            TreeTarget::Session(session) => Some(session),
            TreeTarget::Window(window) => sessions
                .iter()
                .find(|session| {
                    session
                        .windows
                        .iter()
                        .any(|candidate| candidate.id == window)
                })
                .map(|session| session.id),
            TreeTarget::Pane(pane) => sessions
                .iter()
                .find(|session| {
                    session
                        .windows
                        .iter()
                        .any(|window| window.panes.iter().any(|candidate| candidate.id == pane))
                })
                .map(|session| session.id),
        }
    }

    /// The current name a rename prompt would prefill, or `None` where the
    /// target cannot be renamed.
    #[must_use]
    pub fn renameable_name(&self, host: HostId, target: TreeTarget) -> Option<&str> {
        let sessions = &self.host(host)?.sessions;
        match target {
            TreeTarget::Session(session) => sessions
                .iter()
                .find(|candidate| candidate.id == session)
                .map(|session| session.name.as_str()),
            TreeTarget::Window(window) => sessions
                .iter()
                .flat_map(|session| &session.windows)
                .find(|candidate| candidate.id == window)
                .map(|window| window.name.as_str()),
            TreeTarget::Pane(_) => None,
        }
    }

    /// The session or window a node's rename would retitle. Whether that rename
    /// can actually be raised is [`Self::renameable_name`]'s call.
    #[must_use]
    pub fn rename_target_for_node(&self, node: TreeNode) -> Option<(HostId, TreeTarget)> {
        match node {
            TreeNode::Target(host, target @ (TreeTarget::Session(_) | TreeTarget::Window(_))) => {
                Some((host, target))
            }
            TreeNode::Target(host, TreeTarget::Pane(pane)) => self
                .host(host)?
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .find(|window| window.panes.iter().any(|candidate| candidate.id == pane))
                .map(|window| (host, window.target())),
            TreeNode::Host(_) => None,
        }
    }

    /// Build the prompt action for a tree rename. A prompt belongs to the
    /// daemon connection that raised it, so a target on another machine must
    /// become attached before that daemon opens the prompt.
    #[must_use]
    pub fn rename_activation_for_node(
        &self,
        node: TreeNode,
        attached_host: HostId,
    ) -> Option<(&'static str, NavActivation)> {
        let (host, target) = self.rename_target_for_node(node)?;
        if !self.host(host)?.connected() {
            return None;
        }
        let (label, command) = rename_prompt_command(target, self.renameable_name(host, target)?)?;
        let activation = if host == attached_host {
            NavActivation::Execute { host, command }
        } else {
            NavActivation::AttachThenExecute {
                host,
                session: self.session_for_target(host, target)?,
                command,
            }
        };
        Some((label, activation))
    }

    /// Whether this node or anything hidden below it has an uncleared bell.
    /// Bubbling the level state keeps a collapsed host/session/window useful as
    /// a notification surface.
    #[must_use]
    pub fn has_pending_bell(&self, node: TreeNode) -> bool {
        let Some(host) = self.host(node.host()) else {
            return false;
        };
        match node {
            TreeNode::Host(_) => host.sessions.iter().any(MuxTreeSession::has_pending_bell),
            TreeNode::Target(_, TreeTarget::Session(id)) => host
                .sessions
                .iter()
                .find(|session| session.id == id)
                .is_some_and(MuxTreeSession::has_pending_bell),
            TreeNode::Target(_, TreeTarget::Window(id)) => host
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .find(|window| window.id == id)
                .is_some_and(MuxTreeWindow::has_pending_bell),
            TreeNode::Target(_, TreeTarget::Pane(id)) => host
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .flat_map(|window| &window.panes)
                .find(|pane| pane.id == id)
                .is_some_and(|pane| pane.bell),
        }
    }

    #[must_use]
    pub fn activation_for_node(
        &self,
        node: TreeNode,
        attached_host: HostId,
        attached_session: Option<SessionId>,
    ) -> Option<NavActivation> {
        let host = self.host(node.host())?;
        match node {
            TreeNode::Host(id) if id != attached_host && host.connected() => {
                Some(NavActivation::AttachHost(id))
            }
            TreeNode::Host(id)
                if matches!(
                    host.state,
                    HostState::Disconnected
                        | HostState::Reconnecting { .. }
                        | HostState::Unreachable { .. }
                        | HostState::Incompatible { .. }
                ) =>
            {
                Some(NavActivation::Reconnect(id))
            }
            TreeNode::Host(_) => None,
            TreeNode::Target(id, target) => activation_for_target(
                id,
                target,
                self.session_for_target(id, target),
                attached_host,
                attached_session,
                host.connected(),
            ),
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn max_depth(&self) -> usize {
        if self.hosts.is_empty() {
            0
        } else if self
            .hosts
            .iter()
            .flat_map(|host| &host.sessions)
            .any(|session| {
                session
                    .windows
                    .iter()
                    .any(|window| !window.panes.is_empty())
            })
        {
            3
        } else if self
            .hosts
            .iter()
            .flat_map(|host| &host.sessions)
            .any(|session| !session.windows.is_empty())
        {
            2
        } else {
            usize::from(self.hosts.iter().any(|host| !host.sessions.is_empty()))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuxTreeHost {
    pub id: HostId,
    pub name: String,
    pub state: HostState,
    pub snapshot_loaded: bool,
    pub sessions: Vec<MuxTreeSession>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostIndicator {
    Connecting,
    /// A host that is not going to connect on its own. `detail` is
    /// [`HostState::failure_detail`]: the typed ssh advice a shell should keep
    /// visible.
    Failed {
        detail: Option<SharedString>,
    },
}

impl MuxTreeHost {
    #[must_use]
    pub const fn node(&self) -> TreeNode {
        TreeNode::Host(self.id)
    }

    #[must_use]
    pub fn connected(&self) -> bool {
        self.state == HostState::Connected
    }

    #[must_use]
    pub fn indicator(&self) -> Option<HostIndicator> {
        match self.state {
            HostState::Connected if self.snapshot_loaded => None,
            HostState::Connected | HostState::Connecting | HostState::Reconnecting { .. } => {
                Some(HostIndicator::Connecting)
            }
            HostState::Disconnected
            | HostState::Unreachable { .. }
            | HostState::Incompatible { .. } => Some(HostIndicator::Failed {
                detail: self.state.failure_detail().map(SharedString::from),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuxTreeSession {
    pub id: SessionId,
    pub name: String,
    pub active: bool,
    pub windows: Vec<MuxTreeWindow>,
}

impl MuxTreeSession {
    #[must_use]
    pub const fn target(&self) -> TreeTarget {
        TreeTarget::Session(self.id)
    }

    #[must_use]
    pub fn label(&self) -> String {
        session_label(&self.name, self.id)
    }

    fn has_pending_bell(&self) -> bool {
        self.windows.iter().any(MuxTreeWindow::has_pending_bell)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuxTreeWindow {
    pub id: WindowId,
    pub index: u32,
    pub name: String,
    pub active_pane: PaneId,
    pub active: bool,
    pub panes: Vec<MuxTreePane>,
}

impl MuxTreeWindow {
    #[must_use]
    pub const fn target(&self) -> TreeTarget {
        TreeTarget::Window(self.id)
    }

    #[must_use]
    pub fn label(&self) -> String {
        self.name.clone()
    }

    fn has_pending_bell(&self) -> bool {
        self.panes.iter().any(|pane| pane.bell)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuxTreePane {
    pub id: PaneId,
    pub label: String,
    pub kind: MuxTreePaneKind,
    /// A BEL rang here and nobody has been back since.
    pub bell: bool,
}

impl MuxTreePane {
    #[must_use]
    pub fn from_snapshot(pane: &PaneSnapshot) -> Self {
        let kind = match pane.kind {
            PaneKindSnapshot::Picker => MuxTreePaneKind::Picker,
            PaneKindSnapshot::Terminal => MuxTreePaneKind::Terminal,
            PaneKindSnapshot::Browser(_) => MuxTreePaneKind::Browser,
            PaneKindSnapshot::Agent(_) => MuxTreePaneKind::Agent,
            PaneKindSnapshot::Editor(_) => MuxTreePaneKind::Editor,
        };
        Self {
            id: pane.id,
            label: pane_label(pane),
            kind,
            bell: pane.bell,
        }
    }

    #[must_use]
    pub const fn target(&self) -> TreeTarget {
        TreeTarget::Pane(self.id)
    }
}

#[must_use]
pub fn ordered_panes(window: &WindowSnapshot) -> Vec<&PaneSnapshot> {
    let mut layout_order = Vec::with_capacity(window.panes.len());
    window.layout.panes(&mut layout_order);
    let mut seen = BTreeSet::new();
    let mut panes = Vec::with_capacity(window.panes.len());
    for id in layout_order {
        if seen.insert(id)
            && let Some(pane) = window.panes.get(&id)
        {
            panes.push(pane);
        }
    }
    for (id, pane) in &window.panes {
        if seen.insert(*id) {
            panes.push(pane);
        }
    }
    panes
}

#[must_use]
pub fn pane_label(pane: &PaneSnapshot) -> String {
    let title = pane.title.trim();
    if !title.is_empty() {
        return title.to_owned();
    }
    match &pane.kind {
        PaneKindSnapshot::Picker => "new pane".to_owned(),
        PaneKindSnapshot::Terminal => "terminal".to_owned(),
        PaneKindSnapshot::Browser(browser) if !browser.url().trim().is_empty() => {
            browser.url().trim().to_owned()
        }
        PaneKindSnapshot::Browser(_) => "browser".to_owned(),
        PaneKindSnapshot::Agent(_) => "agent".to_owned(),
        PaneKindSnapshot::Editor(_) => "editor".to_owned(),
    }
}

#[derive(Clone, Debug)]
pub enum TreeNodeKind {
    Host,
    Session,
    Window { active_pane: PaneId },
    Pane { kind: MuxTreePaneKind },
}

#[must_use]
pub fn active_tree_target(
    snapshot: &MuxSnapshot,
    attached: Option<SessionId>,
) -> Option<TreeTarget> {
    let session = snapshot
        .sessions
        .iter()
        .find(|session| Some(session.id) == attached)?;
    let Some(window) = session
        .windows
        .iter()
        .find(|window| window.id == snapshot.focused_window_for(session))
    else {
        return Some(TreeTarget::Session(session.id));
    };
    if window.panes.contains_key(&window.active_pane) && window.layout.contains(window.active_pane)
    {
        Some(TreeTarget::Pane(window.active_pane))
    } else {
        Some(TreeTarget::Window(window.id))
    }
}

/// A session's label as every navigation surface writes it: its name, or a
/// stand-in when the daemon has none.
#[must_use]
pub fn session_label(name: &str, id: SessionId) -> String {
    if name.trim().is_empty() {
        format!("session {id}")
    } else {
        name.to_owned()
    }
}

/// The single character standing in for a session: the first letter of its
/// label, uppercased.
#[must_use]
pub fn session_initial(label: &str) -> SharedString {
    label.trim().chars().next().map_or_else(
        || SharedString::from("?"),
        |first| first.to_uppercase().to_string().into(),
    )
}

/// Open every ancestor of `node`, so attaching anywhere in the fleet reveals
/// the row the mux moved to.
pub fn expand_path_to(expanded: &mut BTreeSet<TreeNode>, model: &MuxTreeModel, node: TreeNode) {
    let TreeNode::Target(host, target) = node else {
        return;
    };
    let Some(host_model) = model.host(host) else {
        return;
    };
    expanded.insert(TreeNode::Host(host));
    for session in &host_model.sessions {
        if session.target() == target {
            return;
        }
        for window in &session.windows {
            if window.target() == target {
                expanded.insert(TreeNode::Target(host, session.target()));
                return;
            }
            if window.panes.iter().any(|pane| pane.target() == target) {
                expanded.insert(TreeNode::Target(host, session.target()));
                expanded.insert(TreeNode::Target(host, window.target()));
                return;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavActivation {
    AttachHost(HostId),
    Attach {
        host: HostId,
        session: SessionId,
    },
    Execute {
        host: HostId,
        command: CommandInvocation,
    },
    AttachThenExecute {
        host: HostId,
        session: SessionId,
        command: CommandInvocation,
    },
    Reconnect(HostId),
}

#[must_use]
pub fn activation_for_target(
    host: HostId,
    target: TreeTarget,
    owner_session: Option<SessionId>,
    attached_host: HostId,
    attached_session: Option<SessionId>,
    connected: bool,
) -> Option<NavActivation> {
    if !connected {
        return None;
    }

    match target {
        TreeTarget::Session(session) => Some(NavActivation::Attach { host, session }),
        TreeTarget::Window(window) => Some(select_target_activation(
            host,
            owner_session,
            attached_host,
            attached_session,
            select_window_command(window),
        )),
        TreeTarget::Pane(pane) => Some(select_target_activation(
            host,
            owner_session,
            attached_host,
            attached_session,
            select_pane_command(pane),
        )),
    }
}

#[must_use]
pub fn select_target_activation(
    host: HostId,
    owner_session: Option<SessionId>,
    attached_host: HostId,
    attached_session: Option<SessionId>,
    command: CommandInvocation,
) -> NavActivation {
    match owner_session {
        Some(session) if host != attached_host || Some(session) != attached_session => {
            NavActivation::AttachThenExecute {
                host,
                session,
                command,
            }
        }
        _ => NavActivation::Execute { host, command },
    }
}

pub fn activate_nav(mux: &Entity<MuxClient>, activation: NavActivation, cx: &mut App) {
    mux.update(cx, |mux, cx| match activation {
        NavActivation::AttachHost(host) => {
            mux.attach_to_host_default(host, cx);
        }
        NavActivation::Attach { host, session } => {
            mux.attach_to_host(host, session, cx);
        }
        NavActivation::Execute { host, command } => mux.execute_on_host(host, command),
        NavActivation::AttachThenExecute {
            host,
            session,
            command,
        } => {
            if mux.attach_to_host(host, session, cx) {
                mux.execute_on_host(host, command);
            }
        }
        NavActivation::Reconnect(host) => mux.retry_host_now(host, cx),
    });
}

#[must_use]
pub fn new_window_command(session: SessionId) -> CommandInvocation {
    CommandInvocation::new("new-window", vec!["-t".to_owned(), session.to_string()])
}

#[must_use]
pub fn select_window_command(window: WindowId) -> CommandInvocation {
    CommandInvocation::new("select-window", vec!["-t".to_owned(), window.to_string()])
}

#[must_use]
pub fn select_pane_command(pane: PaneId) -> CommandInvocation {
    CommandInvocation::new("select-pane", vec!["-t".to_owned(), pane.to_string()])
}

#[must_use]
pub fn rename_prompt_command(
    target: TreeTarget,
    current_name: &str,
) -> Option<(&'static str, CommandInvocation)> {
    let (menu_label, prompt, template) = match target {
        TreeTarget::Session(session) => (
            "Rename Session…",
            "rename-session: ",
            format!("rename-session -t '{session}' -- '%%'"),
        ),
        TreeTarget::Window(window) => (
            "Rename Window…",
            "rename-window: ",
            format!("rename-window -t '{window}' -- '%%'"),
        ),
        TreeTarget::Pane(_) => return None,
    };
    Some((
        menu_label,
        CommandInvocation::new(
            "command-prompt",
            ["-p", prompt, "-I", current_name, template.as_str()],
        ),
    ))
}

#[must_use]
pub fn kill_target_command(target: TreeTarget) -> CommandInvocation {
    match target {
        TreeTarget::Session(session) => {
            CommandInvocation::new("kill-session", ["-t".to_owned(), session.to_string()])
        }
        TreeTarget::Window(window) => {
            CommandInvocation::new("kill-window", ["-t".to_owned(), window.to_string()])
        }
        TreeTarget::Pane(pane) => {
            CommandInvocation::new("kill-pane", ["-t".to_owned(), pane.to_string()])
        }
    }
}

#[must_use]
pub fn new_pane_command(pane: PaneId, axis: Axis) -> CommandInvocation {
    CommandInvocation::new(
        "new-pane",
        vec![
            match axis {
                Axis::Horizontal => "-h",
                Axis::Vertical => "-v",
            }
            .to_owned(),
            "-t".to_owned(),
            pane.to_string(),
        ],
    )
}
