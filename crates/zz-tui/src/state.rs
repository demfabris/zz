use std::{collections::HashMap, sync::Arc};

use zz_client::ClientCore;
use zz_daemon::{Endpoint, HostEntry};
use zz_protocol::{
    ChooseBufferState, ChooseTreeState, CommandPromptState, DisplayPanesState, MuxSnapshot, PaneId,
    PaneKindSnapshot, PaneSnapshot, SessionId, SessionSnapshot, StatusLine, StatusPosition,
    TmuxRange, WindowSnapshot,
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

/// Presentation state plus cheap caches of the reduced protocol state that
/// [`ClientCore`] owns. Caches are refreshed from the core when its events
/// arrive so painting and input never take the core lock.
pub(crate) struct Model {
    pub host_label: String,
    pub current_endpoint: Endpoint,
    pub snapshot: Arc<MuxSnapshot>,
    pub attached_session: Option<SessionId>,
    pub viewports: HashMap<PaneId, TerminalViewport>,
    pub appearance: TerminalAppearance,
    pub status: StatusLine,
    pub prefix_armed: bool,
    pub command_prompt: Option<CommandPromptState>,
    pub command_output: Option<(PaneId, TerminalViewport)>,
    pub choose_tree: Option<ChooseTreeState>,
    pub choose_buffer: Option<ChooseBufferState>,
    pub display_panes: Option<DisplayPanesState>,
    pub client_message: Option<String>,
    pub chrome: zz_client::ChromeKeymap,
    pub sidebar: sidebar::State,
    pub sidebar_edit: Option<SidebarEdit>,
    pub picker_pane: Option<PaneId>,
    pub picker_selection: usize,
    pub size: TerminalSize,
    pub layout: ResolvedLayout,
    pub last_sent_geometry: HashMap<PaneId, (u16, u16, u32, u32)>,
    pub mouse_option: bool,
    pub mouse_modes_active: bool,
    local_host_label: String,
    local_endpoint: Endpoint,
    fleet_hosts: Vec<HostEntry>,
}

impl Model {
    pub fn new(
        core: &ClientCore,
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
            snapshot: Arc::clone(core.snapshot()),
            attached_session: core.attached_session(),
            viewports: HashMap::new(),
            appearance: core.appearance().cloned().unwrap_or_default(),
            status: core.status().clone(),
            prefix_armed: core.prefix_armed(),
            command_prompt: core.command_prompt().cloned(),
            command_output: None,
            choose_tree: core.choose_tree().cloned(),
            choose_buffer: core.choose_buffer().cloned(),
            display_panes: core.display_panes().cloned(),
            client_message: None,
            chrome: zz_client::ChromeKeymap::new(),
            sidebar: sidebar::State::default(),
            sidebar_edit: None,
            picker_pane: None,
            picker_selection: 0,
            size,
            layout: ResolvedLayout::default(),
            last_sent_geometry: HashMap::new(),
            mouse_option: crate::app::mouse_option_enabled(core.mux_options()),
            mouse_modes_active: crate::app::mouse_option_enabled(core.mux_options()),
            local_host_label,
            local_endpoint,
            fleet_hosts,
        }
    }

    /// Reseeds the caches from a freshly handshaken core and drops the
    /// presentation state that belonged to the previous connection.
    pub fn reset_connection(&mut self, core: &ClientCore) {
        self.snapshot = Arc::clone(core.snapshot());
        self.attached_session = core.attached_session();
        self.viewports.clear();
        self.appearance = core.appearance().cloned().unwrap_or_default();
        self.status = core.status().clone();
        self.prefix_armed = core.prefix_armed();
        self.command_prompt = core.command_prompt().cloned();
        self.command_output = None;
        self.choose_tree = core.choose_tree().cloned();
        self.choose_buffer = core.choose_buffer().cloned();
        self.display_panes = core.display_panes().cloned();
        self.sidebar_edit = None;
        self.picker_pane = None;
        self.picker_selection = 0;
        self.last_sent_geometry.clear();
        self.layout = ResolvedLayout::default();
        self.clamp_sidebar();
    }

    pub fn update_snapshot(&mut self, snapshot: Arc<MuxSnapshot>) {
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

    /// Adopts a fresh status publication. Returns whether the block's
    /// geometry — row count, position, or suppression — changed, which the
    /// caller must treat as a layout event.
    pub fn set_status(&mut self, status: StatusLine) -> bool {
        let previous = (self.status_block_rows(), self.status_top());
        self.status = status;
        let changed = previous != (self.status_block_rows(), self.status_top());
        if changed {
            self.recompute_layout();
        }
        changed
    }

    /// Rows the tmux status block occupies: the published row count, or zero
    /// when status is off or the terminal cannot keep one pane-content row.
    pub fn status_block_rows(&self) -> u16 {
        let rows = u16::try_from(self.status.rows.len()).unwrap_or(u16::MAX);
        if rows == 0 || self.size.rows < rows.saturating_add(2) {
            0
        } else {
            rows
        }
    }

    pub fn status_top(&self) -> bool {
        self.status.position == StatusPosition::Top
    }

    pub fn status_origin_y(&self) -> u16 {
        if self.status_top() {
            0
        } else {
            self.size.rows.saturating_sub(self.status_block_rows())
        }
    }

    /// The main columns the status block spans: everything beside the sidebar.
    pub fn status_area(&self) -> (u16, u16) {
        let x = if self.sidebar_visible() {
            sidebar::WIDTH
                .saturating_add(sidebar::BORDER_WIDTH)
                .min(self.size.columns)
        } else {
            0
        };
        (x, self.size.columns.saturating_sub(x))
    }

    /// The screen row client messages and the command prompt replace: the
    /// block's `message_line` row, or one virtual row at the configured
    /// position while a message or prompt is active with the block hidden.
    pub fn message_row_y(&self) -> Option<u16> {
        let block = self.status_block_rows();
        if block > 0 {
            let line = u16::from(self.status.message_line).min(block.saturating_sub(1));
            Some(self.status_origin_y().saturating_add(line))
        } else if self.command_prompt.is_some() || self.client_message.is_some() {
            Some(if self.status_top() {
                0
            } else {
                self.size.rows.saturating_sub(1)
            })
        } else {
            None
        }
    }

    pub fn status_row_at(&self, row: u16) -> Option<usize> {
        let block = self.status_block_rows();
        let origin = self.status_origin_y();
        (block > 0 && row >= origin && row < origin.saturating_add(block))
            .then(|| usize::from(row - origin))
    }

    pub fn status_hit_target(&self, index: usize, column: u16) -> Option<TmuxRange> {
        let (_, width) = self.status_area();
        let row = self.status.rows.get(index)?;
        zz_client::compose_status_row(row, width, &self.status.base_style)
            .hit_target(column)
            .cloned()
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

    pub fn set_connected_host(&mut self, host: HostSwitch, core: &ClientCore) {
        self.host_label = host.label;
        self.current_endpoint = host.endpoint;
        self.reset_connection(core);
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
        let canvas = sidebar::canvas_rect(
            self.size.columns,
            self.size.rows,
            self.sidebar_visible(),
            self.status_block_rows(),
            self.status_top(),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::StatusPosition;

    fn make_model(columns: u16, rows: u16) -> Model {
        let core = ClientCore::new();
        let endpoint = Endpoint::parse("unix:///tmp/zz-state-test.sock").expect("test endpoint");
        Model::new(
            &core,
            TerminalSize {
                columns,
                rows,
                cell_width_px: 8,
                cell_height_px: 16,
            },
            "host".to_owned(),
            "host".to_owned(),
            endpoint.clone(),
            endpoint,
            Vec::new(),
        )
    }

    fn status(rows: Vec<&str>, position: StatusPosition, message_line: u8) -> StatusLine {
        StatusLine {
            rows: rows.into_iter().map(str::to_owned).collect(),
            position,
            message_line,
            ..StatusLine::default()
        }
    }

    #[test]
    fn status_rows_change_geometry_and_report_a_layout_event() {
        let mut model = make_model(79, 24);
        assert_eq!(model.status_block_rows(), 0);
        assert!(model.set_status(status(vec!["row"], StatusPosition::Bottom, 0)));
        assert_eq!(model.status_block_rows(), 1);
        assert_eq!(model.status_origin_y(), 23);
        assert!(!model.set_status(status(vec!["other"], StatusPosition::Bottom, 0)));
        assert!(model.set_status(status(vec!["a", "b"], StatusPosition::Top, 1)));
        assert_eq!(model.status_origin_y(), 0);
        assert!(model.set_status(StatusLine::default()));
        assert_eq!(model.status_block_rows(), 0);
    }

    #[test]
    fn the_block_is_suppressed_when_no_pane_content_row_would_remain() {
        let mut model = make_model(79, 3);
        model.set_status(status(vec!["a", "b"], StatusPosition::Bottom, 0));
        assert_eq!(
            model.status_block_rows(),
            0,
            "3 rows cannot host 2+header+content"
        );

        let mut roomy = make_model(79, 4);
        roomy.set_status(status(vec!["a", "b"], StatusPosition::Bottom, 0));
        assert_eq!(roomy.status_block_rows(), 2);
    }

    #[test]
    fn message_row_follows_message_line_and_becomes_virtual_when_off() {
        let mut model = make_model(79, 24);
        model.set_status(status(vec!["a", "b", "c"], StatusPosition::Bottom, 1));
        assert_eq!(model.message_row_y(), Some(22));

        model.set_status(status(vec!["a", "b", "c"], StatusPosition::Top, 2));
        assert_eq!(model.message_row_y(), Some(2));

        model.set_status(StatusLine::default());
        assert_eq!(model.message_row_y(), None);
        model.client_message = Some("hi".to_owned());
        assert_eq!(model.message_row_y(), Some(23));
        model.status.position = StatusPosition::Top;
        assert_eq!(model.message_row_y(), Some(0));
    }

    #[test]
    fn status_hit_targets_map_columns_to_window_ranges() {
        let mut model = make_model(79, 24);
        model.set_status(status(
            vec!["#[range=window|2]0:sh#[norange] rest"],
            StatusPosition::Bottom,
            0,
        ));
        assert_eq!(model.status_row_at(23), Some(0));
        assert_eq!(model.status_row_at(22), None);
        assert_eq!(model.status_hit_target(0, 2), Some(TmuxRange::Window(2)));
        assert_eq!(model.status_hit_target(0, 40), None);
    }

    #[test]
    fn the_status_block_spans_only_the_main_columns_beside_the_sidebar() {
        let model = make_model(100, 30);
        assert!(model.sidebar_visible());
        assert_eq!(model.status_area(), (29, 71));

        let narrow = make_model(79, 24);
        assert!(!narrow.sidebar_visible());
        assert_eq!(narrow.status_area(), (0, 79));
    }
}
