use std::{collections::BTreeSet, fmt};

use zz_protocol::{
    Axis, CommandInvocation, MuxSnapshot, PaneId, PaneKindSnapshot, PaneSnapshot, SessionId,
    WindowId, WindowSnapshot,
};

/// Which daemon a row belongs to. Only the local one is dialled today; the id
/// exists so a fleet layer can add roots without reshaping the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostId(pub u32);

impl HostId {
    pub const LOCAL: Self = Self(0);
}

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
    pub const fn host(self) -> HostId {
        match self {
            Self::Host(host) | Self::Target(host, _) => host,
        }
    }

    /// Round-trips through a `GAction` target, which is how a menu item or a
    /// gutter button names the row it acts on.
    pub fn parse(value: &str) -> Option<Self> {
        let (host, target) = value.split_once(':').unwrap_or((value, ""));
        let host = HostId(host.parse().ok()?);
        if target.is_empty() {
            return Some(Self::Host(host));
        }
        let target = match target.chars().next()? {
            '$' => TreeTarget::Session(target.parse().ok()?),
            '@' => TreeTarget::Window(target.parse().ok()?),
            '%' => TreeTarget::Pane(target.parse().ok()?),
            _ => return None,
        };
        Some(Self::Target(host, target))
    }
}

impl fmt::Display for TreeNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(host) => write!(formatter, "{}", host.0),
            Self::Target(host, TreeTarget::Session(id)) => write!(formatter, "{}:{id}", host.0),
            Self::Target(host, TreeTarget::Window(id)) => write!(formatter, "{}:{id}", host.0),
            Self::Target(host, TreeTarget::Pane(id)) => write!(formatter, "{}:{id}", host.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Picker,
    Terminal,
    Browser,
    Agent,
    Editor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Host,
    Session,
    Window { active_pane: PaneId },
    Pane(PaneKind),
}

/// One rendered line of the tree. Everything a row draws or acts on is decided
/// here, so the widget layer only maps fields onto GTK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub node: TreeNode,
    pub kind: RowKind,
    pub depth: u8,
    pub label: String,
    /// The row the mux itself is on: attached session, focused window, active
    /// pane. Distinct from the client-local keyboard selection.
    pub active: bool,
    /// This row or one of its ancestors is where the mux is; the rest of the
    /// tree is drawn muted.
    pub on_active_path: bool,
    /// A BEL nobody has answered, bubbled up through collapsed ancestors so a
    /// closed row still reports what is hidden underneath it.
    pub bell: bool,
    pub expandable: bool,
    pub expanded: bool,
}

/// What activating a row does. A window or pane on a session this client is not
/// attached to has to become attached first: the daemon resolves `-t` against
/// the attachment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    Attach(SessionId),
    Execute(CommandInvocation),
    AttachThenExecute(SessionId, CommandInvocation),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub hosts: Vec<TreeHost>,
    /// The node the mux is on right now, deepest first: pane, else window, else
    /// session.
    pub active: Option<TreeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeHost {
    pub id: HostId,
    pub name: String,
    pub sessions: Vec<TreeSession>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeSession {
    pub id: SessionId,
    pub name: String,
    pub active: bool,
    pub windows: Vec<TreeWindow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeWindow {
    pub id: WindowId,
    pub index: u32,
    pub name: String,
    pub active_pane: PaneId,
    pub active: bool,
    pub panes: Vec<TreePane>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreePane {
    pub id: PaneId,
    pub label: String,
    pub kind: PaneKind,
    pub bell: bool,
}

impl Tree {
    /// Project one daemon's snapshot into the merged host → session → window →
    /// pane tree the sidebar renders.
    pub fn from_snapshot_for(
        host: HostId,
        name: &str,
        snapshot: &MuxSnapshot,
        attached: Option<SessionId>,
    ) -> Self {
        let mut active = None;
        let sessions = snapshot
            .sessions
            .iter()
            .map(|session| {
                let session_active = Some(session.id) == attached;
                let focused = snapshot.focused_window_for(session);
                let windows: Vec<TreeWindow> = session
                    .windows
                    .iter()
                    .map(|window| {
                        let window_active = session_active && window.id == focused;
                        let panes = ordered_panes(window)
                            .into_iter()
                            .map(|pane| {
                                if window_active && pane.id == window.active_pane {
                                    active =
                                        Some(TreeNode::Target(host, TreeTarget::Pane(pane.id)));
                                }
                                TreePane::from_snapshot(pane)
                            })
                            .collect();
                        TreeWindow {
                            id: window.id,
                            index: window.index,
                            name: window.name.clone(),
                            active_pane: window.active_pane,
                            active: window_active,
                            panes,
                        }
                    })
                    .collect();

                if session_active && active.is_none() {
                    active = Some(windows.iter().find(|window| window.active).map_or(
                        TreeNode::Target(host, TreeTarget::Session(session.id)),
                        |window| TreeNode::Target(host, TreeTarget::Window(window.id)),
                    ));
                }

                TreeSession {
                    id: session.id,
                    name: session.name.clone(),
                    active: session_active,
                    windows,
                }
            })
            .collect();

        Self {
            hosts: vec![TreeHost {
                id: host,
                name: name.to_owned(),
                sessions,
            }],
            active,
        }
    }

    pub fn host(&self, id: HostId) -> Option<&TreeHost> {
        self.hosts.iter().find(|host| host.id == id)
    }

    /// The tree flattened to what is on screen, in render order.
    pub fn rows(&self, expanded: &BTreeSet<TreeNode>) -> Vec<Row> {
        let mut rows = Vec::new();
        for host in &self.hosts {
            let node = TreeNode::Host(host.id);
            let open = expanded.contains(&node);
            rows.push(Row {
                node,
                kind: RowKind::Host,
                depth: 0,
                label: host.name.clone(),
                active: false,
                on_active_path: true,
                bell: self.has_pending_bell(node),
                expandable: !host.sessions.is_empty(),
                expanded: open,
            });
            if !open {
                continue;
            }
            for session in &host.sessions {
                let node = TreeNode::Target(host.id, session.target());
                let open = expanded.contains(&node);
                rows.push(Row {
                    node,
                    kind: RowKind::Session,
                    depth: 1,
                    label: session.label(),
                    active: self.active == Some(node),
                    on_active_path: session.active,
                    bell: self.has_pending_bell(node),
                    expandable: !session.windows.is_empty(),
                    expanded: open,
                });
                if !open {
                    continue;
                }
                for window in &session.windows {
                    let node = TreeNode::Target(host.id, window.target());
                    let open = expanded.contains(&node);
                    rows.push(Row {
                        node,
                        kind: RowKind::Window {
                            active_pane: window.active_pane,
                        },
                        depth: 2,
                        label: window.label(),
                        active: self.active == Some(node),
                        on_active_path: window.active,
                        bell: self.has_pending_bell(node),
                        expandable: !window.panes.is_empty(),
                        expanded: open,
                    });
                    if !open {
                        continue;
                    }
                    for pane in &window.panes {
                        let node = TreeNode::Target(host.id, pane.target());
                        let active = window.active && pane.id == window.active_pane;
                        rows.push(Row {
                            node,
                            kind: RowKind::Pane(pane.kind),
                            depth: 3,
                            label: pane.label.clone(),
                            active,
                            on_active_path: active,
                            bell: pane.bell,
                            expandable: false,
                            expanded: false,
                        });
                    }
                }
            }
        }
        rows
    }

    /// Whether a node still exists, so an expansion recorded for a session the
    /// daemon has since killed does not outlive it.
    pub fn is_live(&self, node: TreeNode) -> bool {
        let Some(host) = self.host(node.host()) else {
            return false;
        };
        match node {
            TreeNode::Host(_) => true,
            TreeNode::Target(_, TreeTarget::Session(id)) => {
                host.sessions.iter().any(|session| session.id == id)
            }
            TreeNode::Target(_, TreeTarget::Window(id)) => host
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .any(|window| window.id == id),
            TreeNode::Target(_, TreeTarget::Pane(id)) => host
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .flat_map(|window| &window.panes)
                .any(|pane| pane.id == id),
        }
    }

    /// Whether this node or anything hidden below it has an uncleared bell.
    pub fn has_pending_bell(&self, node: TreeNode) -> bool {
        let Some(host) = self.host(node.host()) else {
            return false;
        };
        match node {
            TreeNode::Host(_) => host.sessions.iter().any(TreeSession::has_pending_bell),
            TreeNode::Target(_, TreeTarget::Session(id)) => host
                .sessions
                .iter()
                .find(|session| session.id == id)
                .is_some_and(TreeSession::has_pending_bell),
            TreeNode::Target(_, TreeTarget::Window(id)) => host
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .find(|window| window.id == id)
                .is_some_and(TreeWindow::has_pending_bell),
            TreeNode::Target(_, TreeTarget::Pane(id)) => host
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .flat_map(|window| &window.panes)
                .find(|pane| pane.id == id)
                .is_some_and(|pane| pane.bell),
        }
    }

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

    /// The session or window a node's rename would retitle: a pane row renames
    /// the window that holds it, because panes are titled by their program.
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

    /// The prompt a rename raises, prefilled with the target's current name.
    pub fn rename_activation_for_node(
        &self,
        node: TreeNode,
        attached: Option<SessionId>,
    ) -> Option<Activation> {
        let (host, target) = self.rename_target_for_node(node)?;
        let command = rename_prompt_command(target, self.renameable_name(host, target)?)?;
        Some(select_target_activation(
            self.session_for_target(host, target),
            attached,
            command,
        ))
    }

    pub fn activation_for_node(
        &self,
        node: TreeNode,
        attached: Option<SessionId>,
    ) -> Option<Activation> {
        match node {
            TreeNode::Host(_) => None,
            TreeNode::Target(_, TreeTarget::Session(session)) => Some(Activation::Attach(session)),
            TreeNode::Target(host, target @ TreeTarget::Window(window)) => {
                Some(select_target_activation(
                    self.session_for_target(host, target),
                    attached,
                    select_window_command(window),
                ))
            }
            TreeNode::Target(host, target @ TreeTarget::Pane(pane)) => {
                Some(select_target_activation(
                    self.session_for_target(host, target),
                    attached,
                    select_pane_command(pane),
                ))
            }
        }
    }
}

impl TreeSession {
    pub const fn target(&self) -> TreeTarget {
        TreeTarget::Session(self.id)
    }

    pub fn label(&self) -> String {
        session_label(&self.name, self.id)
    }

    fn has_pending_bell(&self) -> bool {
        self.windows.iter().any(TreeWindow::has_pending_bell)
    }
}

impl TreeWindow {
    pub const fn target(&self) -> TreeTarget {
        TreeTarget::Window(self.id)
    }

    /// The window's name, as every navigation surface writes it: the index is
    /// deliberately left out, and only stands in when the daemon has no name.
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            self.index.to_string()
        } else {
            self.name.clone()
        }
    }

    fn has_pending_bell(&self) -> bool {
        self.panes.iter().any(|pane| pane.bell)
    }
}

impl TreePane {
    pub fn from_snapshot(pane: &PaneSnapshot) -> Self {
        let kind = match pane.kind {
            PaneKindSnapshot::Picker => PaneKind::Picker,
            PaneKindSnapshot::Terminal => PaneKind::Terminal,
            PaneKindSnapshot::Browser(_) => PaneKind::Browser,
            PaneKindSnapshot::Agent(_) => PaneKind::Agent,
            PaneKindSnapshot::Editor(_) => PaneKind::Editor,
        };
        Self {
            id: pane.id,
            label: pane_label(pane),
            kind,
            bell: pane.bell,
        }
    }

    pub const fn target(&self) -> TreeTarget {
        TreeTarget::Pane(self.id)
    }
}

/// Panes in the order the split tree places them, with any pane the layout
/// forgot appended so nothing the daemon knows about is unreachable.
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

/// A session's label as every navigation surface writes it: its name, or a
/// stand-in when the daemon has none.
pub fn session_label(name: &str, id: SessionId) -> String {
    if name.trim().is_empty() {
        format!("session {id}")
    } else {
        name.to_owned()
    }
}

/// Open every ancestor of `node`, so the row the mux moved to is on screen.
pub fn expand_path_to(expanded: &mut BTreeSet<TreeNode>, tree: &Tree, node: TreeNode) {
    let TreeNode::Target(host, target) = node else {
        return;
    };
    let Some(host_model) = tree.host(host) else {
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

/// A command aimed at a target the client is not attached to has to ride an
/// attach: `-t` is resolved against the attachment.
pub fn select_target_activation(
    owner: Option<SessionId>,
    attached: Option<SessionId>,
    command: CommandInvocation,
) -> Activation {
    match owner {
        Some(session) if Some(session) != attached => {
            Activation::AttachThenExecute(session, command)
        }
        _ => Activation::Execute(command),
    }
}

pub fn new_window_command(session: SessionId) -> CommandInvocation {
    CommandInvocation::new("new-window", ["-t", &session.to_string()])
}

pub fn new_pane_command(pane: PaneId, axis: Axis) -> CommandInvocation {
    CommandInvocation::new(
        "new-pane",
        [
            match axis {
                Axis::Horizontal => "-h",
                Axis::Vertical => "-v",
            },
            "-t",
            &pane.to_string(),
        ],
    )
}

pub fn select_window_command(window: WindowId) -> CommandInvocation {
    CommandInvocation::new("select-window", ["-t", &window.to_string()])
}

pub fn select_pane_command(pane: PaneId) -> CommandInvocation {
    CommandInvocation::new("select-pane", ["-t", &pane.to_string()])
}

pub fn kill_target_command(target: TreeTarget) -> CommandInvocation {
    match target {
        TreeTarget::Session(session) => {
            CommandInvocation::new("kill-session", ["-t", &session.to_string()])
        }
        TreeTarget::Window(window) => {
            CommandInvocation::new("kill-window", ["-t", &window.to_string()])
        }
        TreeTarget::Pane(pane) => CommandInvocation::new("kill-pane", ["-t", &pane.to_string()]),
    }
}

/// The daemon owns the prompt: the client asks for one prefilled with the
/// current name and carrying the rename as its template.
pub fn rename_prompt_command(target: TreeTarget, current: &str) -> Option<CommandInvocation> {
    let (prompt, template) = match target {
        TreeTarget::Session(session) => (
            "rename-session: ",
            format!("rename-session -t '{session}' -- '%%'"),
        ),
        TreeTarget::Window(window) => (
            "rename-window: ",
            format!("rename-window -t '{window}' -- '%%'"),
        ),
        TreeTarget::Pane(_) => return None,
    };
    Some(CommandInvocation::new(
        "command-prompt",
        ["-p", prompt, "-I", current, template.as_str()],
    ))
}

#[cfg(test)]
mod tests {
    use zz_protocol::{LayoutNode, SessionSnapshot, SplitId, WindowSnapshot};

    use super::*;

    fn pane(id: u64, title: &str, bell: bool) -> PaneSnapshot {
        PaneSnapshot {
            id: PaneId(id),
            title: title.to_owned(),
            kind: PaneKindSnapshot::Terminal,
            synchronized_input: false,
            bell,
        }
    }

    fn window(id: u64, index: u32, name: &str, panes: Vec<PaneSnapshot>) -> WindowSnapshot {
        let layout = panes
            .iter()
            .map(|pane| LayoutNode::Pane(pane.id))
            .reduce(|first, second| LayoutNode::Split {
                id: SplitId(1),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(first),
                second: Box::new(second),
            })
            .expect("a window has at least one pane");
        WindowSnapshot {
            id: WindowId(id),
            index,
            name: name.to_owned(),
            active_pane: panes[0].id,
            zoomed_pane: None,
            layout,
            panes: panes.into_iter().map(|pane| (pane.id, pane)).collect(),
        }
    }

    fn snapshot(sessions: Vec<SessionSnapshot>) -> MuxSnapshot {
        MuxSnapshot {
            generation: 1,
            sessions,
            focused_window: None,
        }
    }

    fn session(id: u64, name: &str, windows: Vec<WindowSnapshot>) -> SessionSnapshot {
        SessionSnapshot {
            id: SessionId(id),
            name: name.to_owned(),
            active_window: windows[0].id,
            windows,
            viewers: Vec::new(),
        }
    }

    fn fixture() -> MuxSnapshot {
        snapshot(vec![
            session(
                1,
                "build",
                vec![
                    window(
                        1,
                        0,
                        "editor",
                        vec![pane(1, "vim", false), pane(2, "", true)],
                    ),
                    window(2, 1, "", vec![pane(3, "cargo", false)]),
                ],
            ),
            session(
                2,
                "",
                vec![window(3, 0, "shell", vec![pane(4, "zsh", false)])],
            ),
        ])
    }

    fn tree() -> Tree {
        Tree::from_snapshot_for(HostId::LOCAL, "studio", &fixture(), Some(SessionId(1)))
    }

    fn node(target: TreeTarget) -> TreeNode {
        TreeNode::Target(HostId::LOCAL, target)
    }

    #[test]
    fn the_attached_pane_is_the_active_row() {
        assert_eq!(tree().active, Some(node(TreeTarget::Pane(PaneId(1)))));
    }

    #[test]
    fn a_collapsed_tree_shows_only_its_host() {
        let expanded = BTreeSet::new();

        let rows = tree().rows(&expanded);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "studio");
        assert!(rows[0].expandable);
        assert!(!rows[0].expanded);
    }

    /// Every level of the projection: labels, depth, the mux's own row, and the
    /// fallbacks for an unnamed session, window and pane.
    #[test]
    fn an_open_tree_lists_every_level_in_order() {
        let tree = tree();
        let mut expanded = BTreeSet::from([TreeNode::Host(HostId::LOCAL)]);
        expand_path_to(&mut expanded, &tree, node(TreeTarget::Pane(PaneId(1))));

        let rows = tree.rows(&expanded);
        let shape: Vec<(u8, &str)> = rows
            .iter()
            .map(|row| (row.depth, row.label.as_str()))
            .collect();

        assert_eq!(
            shape,
            vec![
                (0, "studio"),
                (1, "build"),
                (2, "editor"),
                (3, "vim"),
                (3, "terminal"),
                (2, "1"),
                (1, "session $2"),
            ]
        );
        let active: Vec<&str> = rows
            .iter()
            .filter(|row| row.active)
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(active, vec!["vim"], "only the mux's own row is active");
        let path: Vec<&str> = rows
            .iter()
            .filter(|row| row.on_active_path)
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(path, vec!["studio", "build", "editor", "vim"]);
    }

    #[test]
    fn a_bell_bubbles_up_to_every_ancestor() {
        let tree = tree();
        let belled = [
            TreeNode::Host(HostId::LOCAL),
            node(TreeTarget::Session(SessionId(1))),
            node(TreeTarget::Window(WindowId(1))),
            node(TreeTarget::Pane(PaneId(2))),
        ];

        for node in belled {
            assert!(tree.has_pending_bell(node), "no bell on {node:?}");
        }
        assert!(!tree.has_pending_bell(node(TreeTarget::Window(WindowId(2)))));
        assert!(!tree.has_pending_bell(node(TreeTarget::Session(SessionId(2)))));
    }

    /// A row on the attached session executes; anything else attaches first,
    /// because the daemon resolves `-t` against the attachment.
    #[test]
    fn activation_attaches_before_it_selects_across_sessions() {
        let tree = tree();
        let attached = Some(SessionId(1));

        assert_eq!(
            tree.activation_for_node(node(TreeTarget::Session(SessionId(1))), attached),
            Some(Activation::Attach(SessionId(1)))
        );
        assert_eq!(
            tree.activation_for_node(node(TreeTarget::Pane(PaneId(3))), attached),
            Some(Activation::Execute(CommandInvocation::new(
                "select-pane",
                ["-t", "%3"]
            )))
        );
        assert_eq!(
            tree.activation_for_node(node(TreeTarget::Window(WindowId(3))), attached),
            Some(Activation::AttachThenExecute(
                SessionId(2),
                CommandInvocation::new("select-window", ["-t", "@3"])
            ))
        );
        assert_eq!(
            tree.activation_for_node(TreeNode::Host(HostId::LOCAL), attached),
            None
        );
    }

    /// A pane cannot be renamed, so its row renames the window holding it.
    #[test]
    fn rename_prefills_the_prompt_the_daemon_will_raise() {
        let tree = tree();

        assert_eq!(
            tree.rename_activation_for_node(node(TreeTarget::Pane(PaneId(1))), Some(SessionId(1))),
            Some(Activation::Execute(CommandInvocation::new(
                "command-prompt",
                [
                    "-p",
                    "rename-window: ",
                    "-I",
                    "editor",
                    "rename-window -t '@1' -- '%%'"
                ]
            )))
        );
        assert_eq!(
            tree.rename_activation_for_node(
                node(TreeTarget::Session(SessionId(1))),
                Some(SessionId(1))
            ),
            Some(Activation::Execute(CommandInvocation::new(
                "command-prompt",
                [
                    "-p",
                    "rename-session: ",
                    "-I",
                    "build",
                    "rename-session -t '$1' -- '%%'"
                ]
            )))
        );
        assert_eq!(
            tree.rename_activation_for_node(TreeNode::Host(HostId::LOCAL), Some(SessionId(1))),
            None
        );
    }

    #[test]
    fn row_targets_survive_the_trip_through_a_menu_item() {
        for node in [
            TreeNode::Host(HostId::LOCAL),
            node(TreeTarget::Session(SessionId(9))),
            node(TreeTarget::Window(WindowId(9))),
            node(TreeTarget::Pane(PaneId(9))),
        ] {
            assert_eq!(TreeNode::parse(&node.to_string()), Some(node), "{node:?}");
        }
        assert_eq!(TreeNode::parse("0:nope"), None);
        assert_eq!(TreeNode::parse(""), None);
    }

    #[test]
    fn killed_targets_do_not_keep_their_expansion() {
        let tree = tree();

        assert!(tree.is_live(node(TreeTarget::Window(WindowId(1)))));
        assert!(!tree.is_live(node(TreeTarget::Window(WindowId(9)))));
        assert!(tree.is_live(TreeNode::Host(HostId::LOCAL)));
    }
}
