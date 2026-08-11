use std::collections::HashMap;

use zz_daemon::{Endpoint, HostEntry};
use zz_protocol::{
    ChooseBufferState, ChooseTreeState, CommandPromptState, DisplayPanesState, MuxSnapshot, PaneId,
    PaneKindSnapshot, PaneSnapshot, ServerHello, SessionId, SessionSnapshot, StatusLine,
    WindowSnapshot,
};
use zz_terminal::{TerminalAppearance, TerminalViewport};

use crate::{
    layout::{PaneRect, ResolvedLayout, resolve},
    picker,
    sidebar::{
        self, Edit as SidebarEdit, EditKind as SidebarEditKind, Row as SidebarRow,
        Target as SidebarTarget,
    },
    tty::TerminalSize,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostSwitch {
    pub label: String,
    pub endpoint: Endpoint,
}

pub(crate) struct Model {
    pub host_label: String,
    pub current_endpoint: Endpoint,
    pub snapshot: MuxSnapshot,
    pub attached_session: Option<SessionId>,
    pub viewports: HashMap<PaneId, TerminalViewport>,
    pub appearance: TerminalAppearance,
    pub status: StatusLine,
    pub capabilities: Vec<String>,
    pub prefix_armed: bool,
    pub command_prompt: Option<CommandPromptState>,
    pub command_output: Option<(PaneId, TerminalViewport)>,
    pub choose_tree: Option<ChooseTreeState>,
    pub choose_buffer: Option<ChooseBufferState>,
    pub display_panes: Option<DisplayPanesState>,
    pub client_message: Option<String>,
    pub sidebar: sidebar::State,
    pub sidebar_edit: Option<SidebarEdit>,
    pub picker_pane: Option<PaneId>,
    pub picker_selection: usize,
    pub size: TerminalSize,
    pub layout: ResolvedLayout,
    pub last_sent_geometry: HashMap<PaneId, (u16, u16, u32, u32)>,
    local_host_label: String,
    local_endpoint: Endpoint,
    fleet_hosts: Vec<HostEntry>,
}

impl Model {
    pub fn new(
        hello: &ServerHello,
        size: TerminalSize,
        host_label: String,
        local_host_label: String,
        current_endpoint: Endpoint,
        local_endpoint: Endpoint,
        fleet_hosts: Vec<HostEntry>,
    ) -> Self {
        Self {
            host_label,
            current_endpoint,
            snapshot: MuxSnapshot::default(),
            attached_session: None,
            viewports: HashMap::new(),
            appearance: hello.appearance.clone(),
            status: hello.status.clone(),
            capabilities: hello.capabilities.clone(),
            prefix_armed: false,
            command_prompt: None,
            command_output: None,
            choose_tree: None,
            choose_buffer: None,
            display_panes: None,
            client_message: None,
            sidebar: sidebar::State::default(),
            sidebar_edit: None,
            picker_pane: None,
            picker_selection: 0,
            size,
            layout: ResolvedLayout::default(),
            last_sent_geometry: HashMap::new(),
            local_host_label,
            local_endpoint,
            fleet_hosts,
        }
    }

    pub fn reset_connection(&mut self, hello: &ServerHello) {
        self.snapshot = MuxSnapshot::default();
        self.attached_session = None;
        self.viewports.clear();
        self.appearance = hello.appearance.clone();
        self.status = hello.status.clone();
        self.capabilities.clone_from(&hello.capabilities);
        self.prefix_armed = false;
        self.command_prompt = None;
        self.command_output = None;
        self.choose_tree = None;
        self.choose_buffer = None;
        self.display_panes = None;
        self.sidebar_edit = None;
        self.picker_pane = None;
        self.picker_selection = 0;
        self.last_sent_geometry.clear();
        self.layout = ResolvedLayout::default();
        self.clamp_sidebar();
    }

    pub fn update_snapshot(&mut self, snapshot: MuxSnapshot) {
        self.snapshot = snapshot;
        let active_picker = self.active_picker();
        if active_picker != self.picker_pane {
            self.picker_selection = 0;
        }
        self.picker_pane = active_picker;
        self.clamp_sidebar();
        self.recompute_layout();
    }

    pub fn set_size(&mut self, size: TerminalSize) {
        self.size = size;
        self.sidebar.reconcile_width(size.columns);
        if !self.sidebar_visible() {
            self.sidebar_edit = None;
        }
        self.clamp_sidebar();
        self.recompute_layout();
    }

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar.visible(self.size.columns)
    }

    pub fn sidebar_rows(&self) -> Vec<SidebarRow> {
        sidebar::flatten(
            &self.snapshot,
            self.attached_session,
            &self.host_label,
            &self.fleet_hosts,
            &self.local_endpoint,
            &self.current_endpoint,
        )
    }

    pub const fn sidebar_tree_height(&self) -> u16 {
        sidebar::tree_height(self.size.rows)
    }

    pub fn focus_sidebar(&mut self) -> bool {
        let was_visible = self.sidebar_visible();
        self.sidebar.focus(self.size.columns);
        let changed = was_visible != self.sidebar_visible();
        if changed {
            self.recompute_layout();
        }
        changed
    }

    pub fn toggle_sidebar_focus(&mut self) -> bool {
        let was_visible = self.sidebar_visible();
        self.sidebar.toggle_focus(self.size.columns);
        let changed = was_visible != self.sidebar_visible();
        if changed {
            self.recompute_layout();
        }
        changed
    }

    pub fn hide_sidebar(&mut self) -> bool {
        let was_visible = self.sidebar_visible();
        self.sidebar.hide();
        let changed = was_visible != self.sidebar_visible();
        if changed {
            self.recompute_layout();
        }
        changed
    }

    pub fn move_sidebar_selection(&mut self, delta: isize) {
        let row_count = self.sidebar_rows().len();
        self.sidebar
            .move_selection(delta, row_count, self.sidebar_tree_height());
    }

    pub fn scroll_sidebar(&mut self, delta: isize) {
        let row_count = self.sidebar_rows().len();
        self.sidebar
            .scroll(delta, row_count, self.sidebar_tree_height());
    }

    pub fn select_sidebar_row(&mut self, visible_row: u16) -> Option<SidebarTarget> {
        let rows = self.sidebar_rows();
        let selected = self.sidebar.scroll.saturating_add(usize::from(visible_row));
        let target = rows.get(selected)?.target;
        self.sidebar
            .select(selected, rows.len(), self.sidebar_tree_height());
        target
    }

    pub fn selected_sidebar_target(&self) -> Option<SidebarTarget> {
        self.sidebar_rows()
            .get(self.sidebar.selected)
            .and_then(|row| row.target)
    }

    pub fn begin_sidebar_rename(&mut self) {
        let Some(target) = self.selected_sidebar_target() else {
            return;
        };
        let (kind, name) = match target {
            SidebarTarget::Session(session) => {
                let Some(snapshot) = self
                    .snapshot
                    .sessions
                    .iter()
                    .find(|snapshot| snapshot.id == session)
                else {
                    return;
                };
                (
                    SidebarEditKind::RenameSession(session),
                    snapshot.name.clone(),
                )
            }
            SidebarTarget::Window(window) => {
                let Some(snapshot) = self
                    .snapshot
                    .sessions
                    .iter()
                    .flat_map(|session| &session.windows)
                    .find(|snapshot| snapshot.id == window)
                else {
                    return;
                };
                (SidebarEditKind::RenameWindow(window), snapshot.name.clone())
            }
            _ => return,
        };
        self.sidebar_edit = Some(SidebarEdit::new(kind, name));
        self.clamp_sidebar();
    }

    pub fn begin_add_host(&mut self) {
        self.sidebar_edit = Some(SidebarEdit::new(SidebarEditKind::AddHost, String::new()));
        self.clamp_sidebar();
    }

    pub fn sidebar_edit_row(&self) -> Option<usize> {
        let target = self.sidebar_edit.as_ref()?.kind.target();
        self.sidebar_rows()
            .iter()
            .position(|row| row.target == Some(target))
    }

    pub fn refresh_fleet_hosts(&mut self, fleet_hosts: Vec<HostEntry>) {
        self.fleet_hosts = fleet_hosts;
        self.clamp_sidebar();
    }

    pub fn host_switch(&self, target: SidebarTarget) -> Option<HostSwitch> {
        let switch = match target {
            SidebarTarget::LocalHost => HostSwitch {
                label: self.local_host_label.clone(),
                endpoint: self.local_endpoint.clone(),
            },
            SidebarTarget::FleetHost(index) => {
                let host = self.fleet_hosts.get(index)?;
                HostSwitch {
                    label: host.name.clone(),
                    endpoint: host.endpoint.clone(),
                }
            }
            _ => return None,
        };
        (switch.endpoint != self.current_endpoint).then_some(switch)
    }

    pub fn set_connected_host(&mut self, host: HostSwitch, hello: &ServerHello) {
        self.host_label = host.label;
        self.current_endpoint = host.endpoint;
        self.reset_connection(hello);
    }

    pub fn active_picker(&self) -> Option<PaneId> {
        let pane = self.active_pane()?;
        matches!(self.pane_snapshot(pane)?.kind, PaneKindSnapshot::Picker).then_some(pane)
    }

    pub fn move_picker_selection(&mut self, delta: isize) {
        self.picker_selection = self
            .picker_selection
            .saturating_add_signed(delta)
            .min(picker::CHOICES.len().saturating_sub(1));
    }

    pub fn session(&self) -> Option<&SessionSnapshot> {
        let attached = self.attached_session?;
        self.snapshot
            .sessions
            .iter()
            .find(|session| session.id == attached)
    }

    pub fn window(&self) -> Option<&WindowSnapshot> {
        let session = self.session()?;
        let focused = self.snapshot.focused_window_for(session);
        session.windows.iter().find(|window| window.id == focused)
    }

    pub fn active_pane(&self) -> Option<PaneId> {
        self.window().map(|window| window.active_pane)
    }

    pub fn active_viewport(&self) -> Option<&TerminalViewport> {
        self.active_pane()
            .and_then(|pane| self.viewports.get(&pane))
    }

    pub fn pane_snapshot(&self, pane: PaneId) -> Option<&PaneSnapshot> {
        self.window()?.panes.get(&pane)
    }

    pub fn pane_rect(&self, pane: PaneId) -> Option<PaneRect> {
        self.layout
            .panes
            .iter()
            .find(|entry| entry.pane == pane)
            .copied()
    }

    pub fn pane_at(&self, column: u16, row: u16) -> Option<PaneRect> {
        self.layout
            .panes
            .iter()
            .find(|entry| entry.rect.contains(column, row))
            .copied()
    }

    pub fn terminal_geometries(&self) -> Vec<(PaneId, (u16, u16, u32, u32))> {
        let Some(window) = self.window() else {
            return Vec::new();
        };
        self.layout
            .panes
            .iter()
            .filter_map(|entry| {
                let pane = window.panes.get(&entry.pane)?;
                if !matches!(pane.kind, PaneKindSnapshot::Terminal) {
                    return None;
                }
                let content = entry.rect.content();
                (content.width > 0 && content.height > 0).then_some((
                    entry.pane,
                    (
                        content.width,
                        content.height,
                        self.size.cell_width_px,
                        self.size.cell_height_px,
                    ),
                ))
            })
            .collect()
    }

    fn recompute_layout(&mut self) {
        let canvas =
            sidebar::canvas_rect(self.size.columns, self.size.rows, self.sidebar_visible());
        self.layout = self
            .window()
            .map_or_else(ResolvedLayout::default, |window| {
                if let Some(pane) = window.zoomed_pane {
                    ResolvedLayout {
                        panes: vec![PaneRect { pane, rect: canvas }],
                        dividers: Vec::new(),
                    }
                } else {
                    resolve(&window.layout, canvas, window.active_pane)
                }
            });
    }

    fn clamp_sidebar(&mut self) {
        let rows = self.sidebar_rows();
        if let Some(target) = self.sidebar_edit.as_ref().map(|edit| edit.kind.target()) {
            if let Some(index) = rows.iter().position(|row| row.target == Some(target)) {
                self.sidebar.selected = index;
            } else {
                self.sidebar_edit = None;
            }
        }
        self.sidebar
            .clamp(rows.len(), sidebar::tree_height(self.size.rows));
    }
}
