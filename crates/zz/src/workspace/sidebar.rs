use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use gpui::{
    AnyElement, App, AppContext as _, Context, CursorStyle, Entity, EventEmitter, FocusHandle,
    Hsla, InteractiveElement as _, IntoElement, KeyBinding, ListSizingBehavior, MouseButton,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, UniformListScrollHandle, Window, WindowControlArea, div, img,
    prelude::FluentBuilder as _, px, uniform_list,
};
use zz_client::{ChromeAction, SIDEBAR_TABLE};
use zz_protocol::{
    Axis, CommandInvocation, MuxSnapshot, PaneId, SessionId, StatusLine, WindowId, WindowSnapshot,
};
use zz_ui::menu::DropdownMenu as _;
use zz_ui::navigation::{
    WORKSPACE_SIDEBAR_DEFAULT_WIDTH as SIDEBAR_DEFAULT_WIDTH, WORKSPACE_STRIP_GAP as STRIP_GAP,
    WORKSPACE_TREE_ACTION_INSET as TREE_ACTION_INSET,
    WORKSPACE_TREE_CONTENT_INSET as TREE_CONTENT_INSET,
    WORKSPACE_TREE_INDENT_WIDTH as TREE_INDENT_WIDTH,
    WORKSPACE_TREE_MARKER_SLOT_WIDTH as TREE_MARKER_SLOT_WIDTH,
    WORKSPACE_TREE_NODE_ICON_SIZE as TREE_NODE_ICON_SIZE, sidebar_settings_button,
    workspace_layout_button, workspace_sidebar_attention, workspace_sidebar_divider,
    workspace_sidebar_status, workspace_sidebar_surface, workspace_sidebar_titlebar,
    workspace_strip_chip_connector, workspace_strip_group_separator, workspace_strip_session_badge,
    workspace_strip_window_pill, workspace_tree_disclosure, workspace_tree_marker,
    workspace_tree_row,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, PopupMenuItem},
    notification::Notification,
    rems_from_px,
    scroll::ScrollableElement as _,
    settings::{settings_navigation_button, settings_navigation_group_label},
    spinner::Spinner,
    tooltip::Tooltip,
};

use crate::{
    agent::{
        AgentAttention, AgentController,
        sound::{AgentAttentionTracker, AgentBadge, AgentPaneStatus},
    },
    config::{frame_content_corner_radius, pane_gaps, settings::SettingsView},
    keymap::ChromeChord,
    mux::{
        client::MuxClient,
        hosts::{HostId, HostState},
        nav::{
            HostIndicator, MuxTreeHost, MuxTreeModel, MuxTreePaneKind, MuxTreeWindow, TreeNode,
            TreeNodeKind, TreeTarget, activate_nav as activate_sidebar, activation_for_target,
            active_tree_target, expand_path_to, kill_target_command, new_window_command,
            select_window_command, session_initial, session_label, split_picker_command,
        },
    },
    window::{corners::WindowCorners, drag::window_drag_handle},
    workspace::tree::{IndentGuideColors, WorkspaceIndentGuides},
};

#[cfg(test)]
use crate::mux::nav::{
    MuxTreePane, NavActivation as SidebarActivation, rename_prompt_command, select_pane_command,
};

const SIDEBAR_MIN_WIDTH: f32 = 160.0;
const SIDEBAR_MAX_WIDTH: f32 = 640.0;
const SIDEBAR_RESIZE_HANDLE_WIDTH: f32 = 8.0;
const TREE_INDENT_GUIDE_OFFSET: f32 = TREE_MARKER_SLOT_WIDTH / 2.0;
const TREE_INDENT_GUIDE_PADDING: f32 = 4.0;
const TREE_KEY_CONTEXT: &str = "WorkspaceTree";

gpui::actions!(
    workspace_tree,
    [
        TreeCancel,
        TreeCommandPalette,
        TreeConfirm,
        TreeRename,
        TreeSelectDown,
        TreeSelectFirst,
        TreeSelectLast,
        TreeSelectLeft,
        TreeSelectRight,
        TreeSelectUp
    ]
);

pub(crate) fn init(cx: &mut App) {
    crate::keymap::bind(cx, SIDEBAR_TABLE, workspace_tree_key_bindings);
}

fn workspace_tree_key_bindings(chords: &[ChromeChord]) -> Vec<KeyBinding> {
    let context = Some(TREE_KEY_CONTEXT);
    chords
        .iter()
        .filter_map(|chord| {
            Some(match chord.action() {
                ChromeAction::SidebarCancel => chord.binding(TreeCancel, context),
                ChromeAction::SidebarConfirm => chord.binding(TreeConfirm, context),
                ChromeAction::SidebarRename => chord.binding(TreeRename, context),
                ChromeAction::SidebarCommandPalette => chord.binding(TreeCommandPalette, context),
                ChromeAction::SidebarSelectUp => chord.binding(TreeSelectUp, context),
                ChromeAction::SidebarSelectDown => chord.binding(TreeSelectDown, context),
                ChromeAction::SidebarSelectLeft => chord.binding(TreeSelectLeft, context),
                ChromeAction::SidebarSelectRight => chord.binding(TreeSelectRight, context),
                ChromeAction::SidebarSelectFirst => chord.binding(TreeSelectFirst, context),
                ChromeAction::SidebarSelectLast => chord.binding(TreeSelectLast, context),
                _ => return None,
            })
        })
        .collect()
}

pub(crate) struct SidebarReleaseFocus;
pub(crate) struct SidebarModeChanged;
pub(crate) struct SidebarRouteChanged;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ChromeMode {
    #[default]
    Sidebar,
    Titlebar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkspaceRoute {
    #[default]
    App,
    Settings,
}

fn sidebar_resize_width(pointer_offset: f32, available_width: f32) -> f32 {
    let maximum = SIDEBAR_MAX_WIDTH
        .min(available_width * 0.5)
        .max(SIDEBAR_MIN_WIDTH);
    pointer_offset.clamp(SIDEBAR_MIN_WIDTH, maximum)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SidebarResizeDrag;

struct SidebarResizePreview;

impl Render for SidebarResizePreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.0)).opacity(0.0)
    }
}

pub struct WorkspaceSidebar {
    mux: Entity<MuxClient>,
    agents: Entity<AgentController>,
    tree_model: Rc<MuxTreeModel>,
    visible_entries: Rc<[VisibleTreeEntry]>,
    visible_indices: BTreeMap<TreeNode, usize>,
    expanded: BTreeSet<TreeNode>,
    selected: Option<TreeNode>,
    selection_from_pointer: bool,
    active_target: Option<TreeNode>,
    scroll_handle: UniformListScrollHandle,
    focus_handle: FocusHandle,
    mode: ChromeMode,
    route: WorkspaceRoute,
    settings: Option<Entity<SettingsView>>,
    slideover: bool,
    width: f32,
    local_hostname: SharedString,
    attention: AgentAttention,
    badges: Rc<BTreeMap<(HostId, PaneId), AgentBadge>>,
    tracker: AgentAttentionTracker<(HostId, PaneId)>,
}

impl WorkspaceSidebar {
    pub(crate) fn new(
        mux: Entity<MuxClient>,
        agents: &Entity<AgentController>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut observed_revision = SidebarRevision::for_mux(mux.read(cx));
        let observed_agents = agents.clone();
        cx.observe(&mux, move |sidebar, mux, cx| {
            let revision = SidebarRevision::for_mux(mux.read(cx));
            if revision != observed_revision {
                observed_revision = revision;
                sidebar.reconcile_attention(&observed_agents, cx);
                cx.notify();
            }
        })
        .detach();
        cx.observe(agents, |sidebar, agents, cx| {
            sidebar.reconcile_attention(&agents, cx);
        })
        .detach();
        let attention = agents.read(cx).attention();
        Self {
            mux,
            agents: agents.clone(),
            attention,
            tree_model: Rc::new(MuxTreeModel::default()),
            visible_entries: Rc::from([]),
            visible_indices: BTreeMap::new(),
            expanded: BTreeSet::new(),
            selected: Some(TreeNode::Host(HostId::LOCAL)),
            selection_from_pointer: false,
            active_target: None,
            scroll_handle: UniformListScrollHandle::default(),
            focus_handle: cx.focus_handle(),
            mode: ChromeMode::default(),
            route: WorkspaceRoute::default(),
            settings: None,
            slideover: false,
            width: SIDEBAR_DEFAULT_WIDTH,
            local_hostname: sidebar_hostname(sysinfo::System::host_name().as_deref()),
            badges: Rc::new(BTreeMap::new()),
            tracker: AgentAttentionTracker::default(),
        }
    }

    /// The fleet rollup and the per-pane badges land here together: one pass
    /// over the agent panes feeds the same transition detector that chimes.
    fn reconcile_attention(&mut self, agents: &Entity<AgentController>, cx: &mut Context<Self>) {
        let attention = agents.read(cx).attention();
        let attached_host = self.mux.read(cx).attached_host();
        let statuses = self.agent_pane_statuses(agents, cx);
        let watched = self.watched_pane(cx).map(|pane| (attached_host, pane));
        if let Some(chime) = self.tracker.observe(&statuses, watched) {
            crate::agent::sound::play(chime);
        }
        let badges = statuses
            .iter()
            .filter_map(|(&pane, &status)| Some((pane, self.tracker.badge(pane, status)?)))
            .collect::<BTreeMap<_, _>>();
        if attention != self.attention || badges != *self.badges {
            self.attention = attention;
            self.badges = Rc::new(badges);
            cx.notify();
        }
    }

    fn agent_pane_statuses(
        &self,
        agents: &Entity<AgentController>,
        cx: &App,
    ) -> BTreeMap<(HostId, PaneId), AgentPaneStatus> {
        let controller = agents.read(cx);
        let attached_host = self.mux.read(cx).attached_host();
        self.tree_model
            .hosts
            .iter()
            .filter(|host| host.id == attached_host)
            .flat_map(|host| &host.sessions)
            .flat_map(|session| &session.windows)
            .flat_map(|window| &window.panes)
            .filter(|pane| pane.kind == MuxTreePaneKind::Agent)
            .filter_map(|pane| Some(((attached_host, pane.id), controller.pane_status(pane.id)?)))
            .collect()
    }

    /// The pane the user is demonstrably watching: focused in an active window.
    fn watched_pane(&self, cx: &App) -> Option<PaneId> {
        cx.active_window()?;
        let mux = self.mux.read(cx);
        active_pane_for_split(&mux.snapshot(), mux.attached_session())
    }

    fn render_attention(
        &self,
        attached_host: HostId,
        attached: Option<SessionId>,
        connected: bool,
        cx: &App,
    ) -> Option<AnyElement> {
        let attention = self.attention;
        if attention.is_quiet() {
            return None;
        }
        let snapshot = self.mux.read(cx).snapshot();
        let mut segments: Vec<AnyElement> = Vec::new();
        let mut segment =
            |id: &'static str, count: usize, word: &str, color: Hsla, pane: Option<PaneId>| {
                if count == 0 {
                    return;
                }
                let label = SharedString::from(format!("{count} {word}"));
                let element = workspace_sidebar_attention(id, label, color, pane.is_some(), cx);
                segments.push(match pane {
                    Some(pane) => {
                        let mux = self.mux.clone();
                        let owner = session_owning_pane(&snapshot, pane);
                        element
                            .on_click(move |_, _, cx| {
                                if let Some(activation) = activation_for_target(
                                    attached_host,
                                    TreeTarget::Pane(pane),
                                    owner,
                                    attached_host,
                                    attached,
                                    connected,
                                ) {
                                    activate_sidebar(&mux, activation, cx);
                                }
                            })
                            .into_any_element()
                    }
                    None => element.into_any_element(),
                });
            };
        segment(
            "sidebar-agents-waiting",
            attention.waiting,
            "waiting",
            cx.theme().warning,
            attention.waiting_pane,
        );
        segment(
            "sidebar-agents-failed",
            attention.failed,
            "failed",
            cx.theme().danger,
            attention.failed_pane,
        );
        segment(
            "sidebar-agents-running",
            attention.running,
            "running",
            cx.theme().foreground.muted(),
            None,
        );
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.0))
                .children(segments)
                .into_any_element(),
        )
    }

    pub(crate) const fn mode(&self) -> ChromeMode {
        self.mode
    }

    pub const fn route(&self) -> WorkspaceRoute {
        self.route
    }

    pub(crate) fn settings_view(&self) -> Option<Entity<SettingsView>> {
        self.settings.clone()
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_none() {
            self.settings = Some(cx.new(|cx| SettingsView::new(self.mux.clone(), window, cx)));
        }
        self.route = WorkspaceRoute::Settings;
        self.slideover = false;
        cx.emit(SidebarRouteChanged);
        cx.notify();
    }

    pub fn close_settings(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.route == WorkspaceRoute::App {
            return;
        }
        self.route = WorkspaceRoute::App;
        cx.emit(SidebarRouteChanged);
        cx.notify();
    }

    pub(crate) const fn slideover_open(&self) -> bool {
        self.slideover
    }

    pub(crate) fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            ChromeMode::Sidebar => ChromeMode::Titlebar,
            ChromeMode::Titlebar => ChromeMode::Sidebar,
        };
        self.slideover = false;
        log::info!(
            target: "zz::diagnostics::sidebar",
            "sidebar mode={:?} width={}",
            self.mode,
            self.width,
        );
        cx.emit(SidebarModeChanged);
        cx.notify();
    }

    pub(crate) fn dismiss_slideover(&mut self, cx: &mut Context<Self>) {
        if self.slideover {
            self.slideover = false;
            cx.emit(SidebarReleaseFocus);
            cx.notify();
        }
    }

    pub(crate) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = MuxTreeModel::from_mux(self.mux.read(cx));
        self.reconcile_tree(model);

        if self.mode == ChromeMode::Titlebar {
            self.slideover = true;
        }
        if let Some(node) = self.active_target {
            expand_path_to(&mut self.expanded, &self.tree_model, node);
            self.selection_from_pointer = false;
            self.selected = Some(node);
            self.rebuild_projection();
            if let Some(index) = self.visible_indices.get(&node).copied() {
                self.scroll_handle
                    .scroll_to_item(index, ScrollStrategy::Nearest);
            }
        }
        self.refocus(window, cx);
    }

    pub(crate) fn refocus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        cx.defer_in(window, |_, _, cx| cx.notify());
    }

    pub(crate) fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn resize_to(
        &mut self,
        pointer_offset: f32,
        available_width: f32,
        cx: &mut Context<Self>,
    ) {
        let width = sidebar_resize_width(pointer_offset, available_width);
        if self.width.to_bits() == width.to_bits() {
            return;
        }
        self.width = width;
        log::trace!(
            target: "zz::diagnostics::sidebar",
            "sidebar resize pointer_offset={pointer_offset} available_width={available_width} width={width}",
        );
        cx.notify();
    }

    fn rebuild_projection(&mut self) {
        let projection = TreeProjection::new(&self.tree_model, &self.expanded);
        self.expanded
            .retain(|node| projection.expandable.contains(node));
        self.visible_entries = projection.entries.into();
        self.visible_indices = projection.visible_indices;
        if self
            .selected
            .is_some_and(|selected| !self.visible_indices.contains_key(&selected))
        {
            self.selected = self.tree_model.hosts.first().map(MuxTreeHost::node);
        }
    }

    fn toggle_expanded(&mut self, node: TreeNode, cx: &mut Context<Self>) {
        if !self.tree_model.is_expandable(node) {
            return;
        }
        let expanded = if self.expanded.remove(&node) {
            false
        } else {
            self.expanded.insert(node);
            true
        };
        self.selected = Some(node);
        self.rebuild_projection();
        log::trace!(
            target: "zz::diagnostics::sidebar",
            "tree node {} node={node:?}",
            if expanded { "expanded" } else { "collapsed" },
        );
        cx.notify();
    }

    fn select_or_toggle_node(&mut self, node: TreeNode, cx: &mut Context<Self>) {
        self.selection_from_pointer = true;
        if self.tree_model.is_expandable(node) {
            self.toggle_expanded(node, cx);
        } else {
            self.selected = Some(node);
            cx.notify();
        }
    }

    fn select_node_from_pointer(&mut self, node: TreeNode, cx: &mut Context<Self>) {
        self.selection_from_pointer = true;
        self.selected = Some(node);
        cx.notify();
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.visible_entries.get(index) else {
            return;
        };
        self.selection_from_pointer = false;
        self.selected = Some(entry.node);
        self.scroll_handle
            .scroll_to_item(index, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn on_select_up(&mut self, _: &TreeSelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.visible_entries.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|node| self.visible_indices.get(&node).copied())
            .unwrap_or(0);
        self.select_index(current.saturating_sub(1), cx);
    }

    fn on_select_down(&mut self, _: &TreeSelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.visible_entries.is_empty() {
            return;
        }
        let current = self
            .selected
            .and_then(|node| self.visible_indices.get(&node).copied())
            .unwrap_or(0);
        self.select_index((current + 1).min(self.visible_entries.len() - 1), cx);
    }

    fn on_select_first(&mut self, _: &TreeSelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        if !self.visible_entries.is_empty() {
            self.select_index(0, cx);
        }
    }

    fn on_select_last(&mut self, _: &TreeSelectLast, _: &mut Window, cx: &mut Context<Self>) {
        if !self.visible_entries.is_empty() {
            self.select_index(self.visible_entries.len() - 1, cx);
        }
    }

    fn on_cancel(&mut self, _: &TreeCancel, _: &mut Window, cx: &mut Context<Self>) {
        self.slideover = false;
        cx.emit(SidebarReleaseFocus);
        cx.notify();
    }

    fn on_select_left(&mut self, _: &TreeSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        if self.expanded.contains(&selected) && self.tree_model.is_expandable(selected) {
            self.selection_from_pointer = false;
            self.toggle_expanded(selected, cx);
            return;
        }
        if let Some(parent) = self
            .visible_indices
            .get(&selected)
            .and_then(|index| self.visible_entries.get(*index))
            .and_then(|entry| entry.parent)
            && let Some(index) = self.visible_indices.get(&parent).copied()
        {
            self.select_index(index, cx);
        }
    }

    fn on_select_right(&mut self, _: &TreeSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        if self.tree_model.is_expandable(selected) && !self.expanded.contains(&selected) {
            self.selection_from_pointer = false;
            self.toggle_expanded(selected, cx);
            return;
        }
        let Some(index) = self.visible_indices.get(&selected).copied() else {
            return;
        };
        let depth = self.visible_entries[index].depth;
        if self
            .visible_entries
            .get(index + 1)
            .is_some_and(|entry| entry.depth == depth + 1)
        {
            self.select_index(index + 1, cx);
        }
    }

    fn on_confirm(&mut self, _: &TreeConfirm, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        let activation = {
            let mux = self.mux.read(cx);
            self.tree_model.activation_for_node(
                selected,
                mux.attached_host(),
                mux.attached_session(),
            )
        };
        if let Some(activation) = activation {
            self.slideover = false;
            activate_sidebar(&self.mux, activation, cx);
            cx.emit(SidebarReleaseFocus);
        } else if self.tree_model.is_expandable(selected) {
            self.selection_from_pointer = false;
            self.toggle_expanded(selected, cx);
        }
    }

    fn on_rename(&mut self, _: &TreeRename, _: &mut Window, cx: &mut Context<Self>) {
        let Some((_, activation)) = self.selected.and_then(|node| {
            self.tree_model
                .rename_activation_for_node(node, self.mux.read(cx).attached_host())
        }) else {
            return;
        };
        activate_sidebar(&self.mux, activation, cx);
    }

    fn on_command_palette(
        &mut self,
        _: &TreeCommandPalette,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mux
            .read(cx)
            .execute(CommandInvocation::new("command-prompt", [] as [&str; 0]));
    }

    fn reconcile_tree(&mut self, model: MuxTreeModel) -> bool {
        if self.tree_model.as_ref() == &model {
            return false;
        }

        let previous_active = self.active_target;
        expand_new_hosts(&mut self.expanded, &self.tree_model, &model);
        self.active_target = model.active_target;
        self.tree_model = Rc::new(model);
        if previous_active != self.active_target
            && let Some(node) = self.active_target
        {
            expand_path_to(&mut self.expanded, &self.tree_model, node);
            self.selected = Some(node);
        }
        self.rebuild_projection();
        if previous_active != self.active_target
            && let Some(node) = self.active_target
            && let Some(index) = self.visible_indices.get(&node).copied()
        {
            self.scroll_handle
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }

        log::trace!(
            target: "zz::diagnostics::sidebar",
            "tree reconciled hosts={} sessions={} active_target={:?} expanded={}",
            self.tree_model.hosts.len(),
            self.tree_model.session_count(),
            self.active_target,
            self.expanded.len(),
        );
        true
    }
    fn render_navigation(
        &self,
        sidebar: &Entity<Self>,
        attached_host: HostId,
        attached: Option<SessionId>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        render_expanded_navigation(
            &self.visible_entries,
            &self.scroll_handle,
            &TreeRowRuntime {
                sidebar: sidebar.clone(),
                mux: self.mux.clone(),
                tree_model: Rc::clone(&self.tree_model),
                attached_host,
                attached_session: attached,
                focus_handle: self.focus_handle.clone(),
                active_target: self.active_target,
                selected: self.selected.filter(|_| !self.selection_from_pointer),
                focused: self.focus_handle.is_focused(window),
                local_hostname: self.local_hostname.clone(),
                badges: Rc::clone(&self.badges),
            },
            window,
            cx,
        )
    }

    fn render_settings_navigation(&self, sidebar: &Entity<Self>, cx: &App) -> AnyElement {
        let Some(settings) = self.settings_view() else {
            return div().into_any_element();
        };
        let selected = settings.read(cx).section();
        let close_sidebar = sidebar.clone();
        let back = Button::new("settings-back")
            .w_full()
            .px(px(8.0))
            .small()
            .ghost()
            .icon(IconName::ArrowLeft)
            .label("Back")
            .child(div().flex_1())
            .on_click(move |_, window, cx| {
                close_sidebar.update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
            });
        let mut items = vec![back.into_any_element()];
        let mut current_group = None;
        for &section in &crate::profile::profile(cx).settings_sections {
            let group = section.navigation_group();
            if current_group != Some(group) {
                items.push(settings_navigation_group_label(group, cx).into_any_element());
                current_group = Some(group);
            }
            let settings = settings.clone();
            let navigation_sidebar = sidebar.clone();
            items.push(
                settings_navigation_button(section, section == selected, cx)
                    .on_click(move |_, window, cx| {
                        settings.update(cx, |settings, cx| {
                            settings.set_section(section, window, cx);
                        });
                        navigation_sidebar.update(cx, |_, cx| cx.notify());
                    })
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(2.0))
            .px(px(6.0))
            .pt(px(6.0))
            .children(items)
            .into_any_element()
    }

    fn render_controls(
        &self,
        sidebar: &Entity<Self>,
        settings_id: &'static str,
        layout_id: &'static str,
        cx: &App,
    ) -> AnyElement {
        let settings_sidebar = sidebar.clone();
        let settings = sidebar_settings_button(settings_id)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                let route = settings_sidebar.read(cx).route();
                settings_sidebar.update(cx, |sidebar, cx| match route {
                    WorkspaceRoute::App => sidebar.open_settings(window, cx),
                    WorkspaceRoute::Settings => sidebar.close_settings(window, cx),
                });
            });
        let mode_sidebar = sidebar.clone();
        let split_mux = self.mux.clone();
        let active_pane = {
            let mux = self.mux.read(cx);
            mux.is_connected()
                .then(|| active_pane_for_split(&mux.snapshot(), mux.attached_session()))
                .flatten()
        };
        let layout = workspace_layout_button(layout_id).dropdown_menu(move |menu, _, _| {
            let mode_sidebar = mode_sidebar.clone();
            let right_mux = split_mux.clone();
            let bottom_mux = split_mux.clone();
            menu.item(
                PopupMenuItem::new("Toggle sidebar")
                    .icon(IconName::PanelLeft)
                    .on_click(move |_, _, cx| {
                        mode_sidebar.update(cx, WorkspaceSidebar::toggle_mode);
                    }),
            )
            .item(
                PopupMenuItem::new("Split right")
                    .icon(IconName::PanelRight)
                    .disabled(active_pane.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(pane) = active_pane {
                            right_mux
                                .read(cx)
                                .execute(split_picker_command(pane, Axis::Horizontal));
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("Split bottom")
                    .icon(IconName::PanelBottom)
                    .disabled(active_pane.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(pane) = active_pane {
                            bottom_mux
                                .read(cx)
                                .execute(split_picker_command(pane, Axis::Vertical));
                        }
                    }),
            )
        });
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(settings)
            .when(!crate::profile::profile(cx).fixed_window, |controls| {
                controls.child(layout)
            })
            .into_any_element()
    }

    pub(crate) fn render_strip_controls(&self, sidebar: &Entity<Self>, cx: &App) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(STRIP_GAP))
            .child(self.render_controls(
                sidebar,
                "workspace-strip-settings",
                "workspace-strip-toggle",
                cx,
            ))
            .child(workspace_strip_group_separator(cx))
            .into_any_element()
    }

    fn render_strip_window_delete(&self, window: WindowId, connected: bool, cx: &App) -> Button {
        let delete_mux = self.mux.clone();
        Button::new(("workspace-strip-window-delete", window.0))
            .ghost()
            .with_size(px(18.0))
            .icon(Icon::new(IconName::Delete).text_color(cx.theme().danger))
            .tooltip("Delete window")
            .disabled(!connected)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                delete_mux
                    .read(cx)
                    .execute(kill_target_command(TreeTarget::Window(window)));
            })
    }

    pub(crate) fn render_strip_content(&self, cx: &App) -> (AnyElement, AnyElement) {
        let mux = self.mux.read(cx);
        let snapshot = mux.snapshot();
        let attached_host = mux.attached_host();
        let attached = mux.attached_session();
        let connected = mux.is_connected();

        let badges = snapshot
            .sessions
            .iter()
            .map(|session| {
                let label = SharedString::from(session_label(&session.name, session.id));
                let id = session.id;
                let attach_mux = self.mux.clone();
                let badge = node_agent_badge(
                    &self.tree_model,
                    &self.badges,
                    TreeNode::Target(attached_host, TreeTarget::Session(id)),
                );
                let chip = workspace_strip_session_badge(
                    ("workspace-strip-session", id.0),
                    session_initial(&label),
                    label,
                    Some(id) == attached,
                    connected,
                    cx,
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    if let Some(activation) = activation_for_target(
                        attached_host,
                        TreeTarget::Session(id),
                        Some(id),
                        attached_host,
                        attached,
                        connected,
                    ) {
                        activate_sidebar(&attach_mux, activation, cx);
                    }
                })
                .into_any_element();
                with_strip_agent_badge(chip, badge, cx)
            })
            .collect::<Vec<_>>();

        let mut pills = Vec::new();
        if let Some(session) = snapshot
            .sessions
            .iter()
            .find(|session| Some(session.id) == attached)
        {
            let focused_window = snapshot.focused_window_for(session);
            for window in &session.windows {
                let id = window.id;
                let select_mux = self.mux.clone();
                let badge = node_agent_badge(
                    &self.tree_model,
                    &self.badges,
                    TreeNode::Target(attached_host, TreeTarget::Window(id)),
                );
                let chip = workspace_strip_window_pill(
                    ("workspace-strip-window", id.0),
                    format!("workspace-strip-window-{}", id.0).into(),
                    strip_window_label(window).into(),
                    id == focused_window,
                    connected,
                    self.render_strip_window_delete(id, connected, cx),
                    cx,
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    select_mux.read(cx).execute(select_window_command(id));
                })
                .into_any_element();
                pills.push(with_strip_agent_badge(chip, badge, cx));
            }
        }

        let mut chips = Vec::with_capacity((badges.len() + pills.len()) * 2);
        for chip in badges.into_iter().chain(pills) {
            if !chips.is_empty() {
                chips.push(workspace_strip_chip_connector(cx).into_any_element());
            }
            chips.push(chip);
        }
        let leading = div()
            .flex()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .children(chips)
            .into_any_element();

        let trailing = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(STRIP_GAP))
            .children(self.render_attention(attached_host, attached, connected, cx))
            .children(render_strip_status(mux.status(), cx))
            .into_any_element();

        (leading, trailing)
    }
}

fn active_pane_for_split(snapshot: &MuxSnapshot, attached: Option<SessionId>) -> Option<PaneId> {
    match active_tree_target(snapshot, attached) {
        Some(TreeTarget::Pane(pane)) => Some(pane),
        Some(TreeTarget::Session(_) | TreeTarget::Window(_)) | None => None,
    }
}

/// Bubbled like [`MuxTreeModel::has_pending_bell`]: a collapsed host, session,
/// or window still carries the most urgent badge hidden below it.
fn node_agent_badge(
    tree_model: &MuxTreeModel,
    badges: &BTreeMap<(HostId, PaneId), AgentBadge>,
    node: TreeNode,
) -> Option<AgentBadge> {
    if badges.is_empty() {
        return None;
    }
    let host = tree_model.host(node.host())?;
    match node {
        TreeNode::Host(_) => windows_agent_badge(
            host.id,
            host.sessions.iter().flat_map(|session| &session.windows),
            badges,
        ),
        TreeNode::Target(_, TreeTarget::Session(id)) => windows_agent_badge(
            host.id,
            host.sessions
                .iter()
                .filter(|session| session.id == id)
                .flat_map(|session| &session.windows),
            badges,
        ),
        TreeNode::Target(_, TreeTarget::Window(id)) => windows_agent_badge(
            host.id,
            host.sessions
                .iter()
                .flat_map(|session| &session.windows)
                .filter(|window| window.id == id),
            badges,
        ),
        TreeNode::Target(host, TreeTarget::Pane(id)) => badges.get(&(host, id)).copied(),
    }
}

fn windows_agent_badge<'a>(
    host: HostId,
    windows: impl Iterator<Item = &'a MuxTreeWindow>,
    badges: &BTreeMap<(HostId, PaneId), AgentBadge>,
) -> Option<AgentBadge> {
    let mut rollup = None;
    for pane in windows.flat_map(|window| &window.panes) {
        if let Some(&badge) = badges.get(&(host, pane.id)) {
            AgentBadge::merge_into(&mut rollup, badge);
        }
    }
    rollup
}

fn agent_badge_color(badge: AgentBadge, cx: &App) -> Hsla {
    match badge {
        AgentBadge::NeedsInput => cx.theme().warning,
        AgentBadge::Failed => cx.theme().danger,
        AgentBadge::Working => cx.theme().foreground.muted(),
        AgentBadge::Finished => cx.theme().success,
    }
}

fn agent_badge_dot(badge: AgentBadge, cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .size(px(5.0))
        .rounded_full()
        .bg(agent_badge_color(badge, cx))
}

/// The strip chips are fixed-size, so the flag rides the leading corner rather
/// than the trailing one the hover-revealed delete button owns.
fn with_strip_agent_badge(chip: AnyElement, badge: Option<AgentBadge>, cx: &App) -> AnyElement {
    let Some(badge) = badge else {
        return chip;
    };
    div()
        .relative()
        .flex()
        .flex_none()
        .child(chip)
        .child(
            agent_badge_dot(badge, cx)
                .absolute()
                .top(px(3.0))
                .left(px(3.0)),
        )
        .into_any_element()
}

fn expand_new_hosts(
    expanded: &mut BTreeSet<TreeNode>,
    previous: &MuxTreeModel,
    next: &MuxTreeModel,
) {
    let previous_hosts = previous
        .hosts
        .iter()
        .map(|host| host.id)
        .collect::<BTreeSet<_>>();
    expanded.extend(
        next.hosts
            .iter()
            .filter(|host| host.id == HostId::LOCAL && !previous_hosts.contains(&host.id))
            .map(MuxTreeHost::node),
    );
}

impl Render for WorkspaceSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (model, attached_host, attached, connected) = {
            let mux = self.mux.read(cx);
            (
                MuxTreeModel::from_mux(mux),
                mux.attached_host(),
                mux.attached_session(),
                mux.is_connected(),
            )
        };
        if self.reconcile_tree(model) {
            let agents = self.agents.clone();
            self.reconcile_attention(&agents, cx);
        }

        let sidebar = cx.entity().clone();
        let width = self.width;
        let settings_route = self.route == WorkspaceRoute::Settings;
        let navigation = if settings_route {
            self.render_settings_navigation(&sidebar, cx)
        } else {
            self.render_navigation(&sidebar, attached_host, attached, window, cx)
        };
        let status = if settings_route {
            None
        } else {
            let attention = self.render_attention(attached_host, attached, connected, cx);
            render_status_section(attention, self.mux.read(cx).status(), cx)
        };
        let controls = if settings_route {
            div().into_any_element()
        } else {
            self.render_controls(&sidebar, "sidebar-settings", "sidebar-toggle", cx)
        };
        let titlebar = workspace_sidebar_titlebar("workspace-sidebar-titlebar", controls, cx)
            .window_control_area(WindowControlArea::Drag);
        let titlebar = if crate::profile::profile(cx).fixed_window {
            titlebar
        } else {
            window_drag_handle("workspace-sidebar-titlebar-drag", titlebar, window, cx)
        };
        let corners = WindowCorners::for_window(window).left();
        let divider_hidden =
            (settings_route || matches!(self.mode(), ChromeMode::Sidebar)) && pane_gaps(cx);
        let shell = corners.round_div(
            workspace_sidebar_surface("workspace-sidebar", width, titlebar, navigation, status, cx)
                .bg(crate::theme::chrome_background(cx))
                .when(divider_hidden, |this| {
                    this.border_color(gpui::transparent_black())
                }),
            frame_content_corner_radius(cx),
        );
        let resize_handle = div()
            .id("workspace-sidebar-resize-handle")
            .absolute()
            .top(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0))
            .w(px(SIDEBAR_RESIZE_HANDLE_WIDTH))
            .cursor(CursorStyle::ResizeLeftRight)
            .occlude()
            .when(divider_hidden, |this| {
                this.hover(|this| {
                    this.border_r_1()
                        .border_color(workspace_sidebar_divider(cx))
                })
            })
            .on_drag(SidebarResizeDrag, |_: &SidebarResizeDrag, _, _, cx| {
                cx.new(|_| SidebarResizePreview)
            });
        div()
            .id("workspace-sidebar-clip")
            .relative()
            .h_full()
            .w(px(width))
            .flex()
            .flex_none()
            .overflow_hidden()
            .child(shell)
            .when(!crate::profile::profile(cx).fixed_window, |sidebar| {
                sidebar.child(resize_handle)
            })
    }
}

impl EventEmitter<SidebarReleaseFocus> for WorkspaceSidebar {}
impl EventEmitter<SidebarModeChanged> for WorkspaceSidebar {}
impl EventEmitter<SidebarRouteChanged> for WorkspaceSidebar {}

fn strip_window_label(window: &WindowSnapshot) -> String {
    format!("{}:{}", window.index, window.name)
}

#[derive(Clone, Debug)]
struct VisibleTreeEntry {
    node: TreeNode,
    parent: Option<TreeNode>,
    label: SharedString,
    depth: u8,
    kind: TreeNodeKind,
    expandable: bool,
    expanded: bool,
    on_active_path: bool,
}

impl VisibleTreeEntry {
    const fn target(&self) -> Option<(HostId, TreeTarget)> {
        match self.node {
            TreeNode::Host(_) => None,
            TreeNode::Target(host, target) => Some((host, target)),
        }
    }
}

struct TreeProjection {
    entries: Vec<VisibleTreeEntry>,
    visible_indices: BTreeMap<TreeNode, usize>,
    expandable: BTreeSet<TreeNode>,
}

impl TreeProjection {
    fn new(model: &MuxTreeModel, expanded: &BTreeSet<TreeNode>) -> Self {
        let mut entries = Vec::new();
        let mut visible_indices = BTreeMap::new();
        let expandable = expandable_nodes(model);

        for host in &model.hosts {
            let host_node = host.node();
            let host_expanded = expanded.contains(&host_node);
            push_visible_entry(
                &mut entries,
                &mut visible_indices,
                VisibleTreeEntry {
                    node: host_node,
                    parent: None,
                    label: host.name.clone().into(),
                    depth: 0,
                    kind: TreeNodeKind::Host,
                    expandable: true,
                    expanded: host_expanded,
                    on_active_path: false,
                },
            );

            if !host_expanded {
                continue;
            }
            for session in &host.sessions {
                let session_node = TreeNode::Target(host.id, session.target());
                let session_expandable = !session.windows.is_empty();
                let session_expanded = session_expandable && expanded.contains(&session_node);
                push_visible_entry(
                    &mut entries,
                    &mut visible_indices,
                    VisibleTreeEntry {
                        node: session_node,
                        parent: Some(host_node),
                        label: session.label().into(),
                        depth: 1,
                        kind: TreeNodeKind::Session,
                        expandable: session_expandable,
                        expanded: session_expanded,
                        on_active_path: session.active,
                    },
                );

                if !session_expanded {
                    continue;
                }
                for window in &session.windows {
                    let window_node = TreeNode::Target(host.id, window.target());
                    let window_expandable = !window.panes.is_empty();
                    let window_expanded = window_expandable && expanded.contains(&window_node);
                    push_visible_entry(
                        &mut entries,
                        &mut visible_indices,
                        VisibleTreeEntry {
                            node: window_node,
                            parent: Some(session_node),
                            label: window.label().into(),
                            depth: 2,
                            kind: TreeNodeKind::Window {
                                active_pane: window.active_pane,
                            },
                            expandable: window_expandable,
                            expanded: window_expanded,
                            on_active_path: window.active,
                        },
                    );

                    if !window_expanded {
                        continue;
                    }
                    for pane in &window.panes {
                        push_visible_entry(
                            &mut entries,
                            &mut visible_indices,
                            VisibleTreeEntry {
                                node: TreeNode::Target(host.id, pane.target()),
                                parent: Some(window_node),
                                label: pane.label.clone().into(),
                                depth: 3,
                                kind: TreeNodeKind::Pane { kind: pane.kind },
                                expandable: false,
                                expanded: false,
                                on_active_path: window.active && pane.id == window.active_pane,
                            },
                        );
                    }
                }
            }
        }

        Self {
            entries,
            visible_indices,
            expandable,
        }
    }
}

fn expandable_nodes(model: &MuxTreeModel) -> BTreeSet<TreeNode> {
    let mut expandable = BTreeSet::new();
    for host in &model.hosts {
        expandable.insert(host.node());
        for session in &host.sessions {
            if !session.windows.is_empty() {
                expandable.insert(TreeNode::Target(host.id, session.target()));
            }
            for window in &session.windows {
                if !window.panes.is_empty() {
                    expandable.insert(TreeNode::Target(host.id, window.target()));
                }
            }
        }
    }
    expandable
}

fn push_visible_entry(
    entries: &mut Vec<VisibleTreeEntry>,
    visible_indices: &mut BTreeMap<TreeNode, usize>,
    entry: VisibleTreeEntry,
) {
    visible_indices.insert(entry.node, entries.len());
    entries.push(entry);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FleetHostRevision {
    id: HostId,
    name: String,
    state: HostState,
    generation: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SidebarRevision {
    hosts: Vec<FleetHostRevision>,
    attached_host: HostId,
    attached_session: Option<SessionId>,
    active_target: Option<TreeNode>,
    status: u64,
}

impl SidebarRevision {
    fn for_mux(mux: &MuxClient) -> Self {
        let attached_host = mux.attached_host();
        let attached_session = mux.attached_session();
        let mut active_target = None;
        let hosts = mux
            .fleet_hosts()
            .map(|(id, name, state, snapshot)| {
                if id == attached_host {
                    active_target = snapshot
                        .and_then(|snapshot| active_tree_target(snapshot, attached_session))
                        .map(|target| TreeNode::Target(id, target));
                }
                FleetHostRevision {
                    id,
                    name: name.to_owned(),
                    state: state.clone(),
                    generation: snapshot.map(|snapshot| snapshot.generation),
                }
            })
            .collect();
        Self {
            hosts,
            attached_host,
            attached_session,
            active_target,
            status: mux.status_revision(),
        }
    }
}

#[derive(Clone)]
struct TreeRowRuntime {
    sidebar: Entity<WorkspaceSidebar>,
    mux: Entity<MuxClient>,
    tree_model: Rc<MuxTreeModel>,
    attached_host: HostId,
    attached_session: Option<SessionId>,
    focus_handle: FocusHandle,
    active_target: Option<TreeNode>,
    selected: Option<TreeNode>,
    focused: bool,
    local_hostname: SharedString,
    badges: Rc<BTreeMap<(HostId, PaneId), AgentBadge>>,
}

fn render_status_section(
    attention: Option<AnyElement>,
    status: &StatusLine,
    cx: &App,
) -> Option<AnyElement> {
    let [left, right] = [status.left.as_str(), status.right.as_str()]
        .map(|half| SharedString::from(half.trim().to_owned()));
    if attention.is_none() && left.is_empty() && right.is_empty() {
        return None;
    }
    Some(
        workspace_sidebar_status("workspace-sidebar-status", attention, left, right, cx)
            .into_any_element(),
    )
}

fn render_strip_status(status: &StatusLine, cx: &App) -> Option<gpui::Div> {
    let [left, right] = [status.left.as_str(), status.right.as_str()]
        .map(|half| SharedString::from(half.trim().to_owned()));
    if left.is_empty() && right.is_empty() {
        return None;
    }
    let left = (!left.is_empty()).then(|| {
        div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(left)
    });
    let right = (!right.is_empty()).then(|| div().flex_none().whitespace_nowrap().child(right));
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(STRIP_GAP))
            .text_xs()
            .text_color(cx.theme().foreground.muted())
            .children(left)
            .children(right),
    )
}

fn session_owning_pane(snapshot: &MuxSnapshot, pane: PaneId) -> Option<SessionId> {
    snapshot
        .sessions
        .iter()
        .find(|session| {
            session
                .windows
                .iter()
                .any(|window| window.panes.contains_key(&pane))
        })
        .map(|session| session.id)
}

fn render_expanded_navigation(
    entries: &Rc<[VisibleTreeEntry]>,
    scroll_handle: &UniformListScrollHandle,
    runtime: &TreeRowRuntime,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let depths: Rc<[usize]> = entries
        .iter()
        .map(|entry| usize::from(entry.depth))
        .collect::<Vec<_>>()
        .into();
    let active_row = if runtime.active_target.is_some_and(|node| {
        runtime
            .tree_model
            .host(node.host())
            .is_some_and(MuxTreeHost::connected)
    }) {
        entries.iter().rposition(|entry| entry.on_active_path)
    } else {
        None
    };
    let guide_color = cx.theme().foreground.muted();
    let decoration = WorkspaceIndentGuides::new(
        depths,
        active_row,
        px(TREE_INDENT_WIDTH),
        px(TREE_CONTENT_INSET + TREE_INDENT_GUIDE_OFFSET),
        px(TREE_INDENT_GUIDE_PADDING),
        IndentGuideColors {
            default: guide_color.wash(),
            active: guide_color,
        },
    );
    let row_entries = Rc::clone(entries);
    let row_runtime = runtime.clone();
    let rows = uniform_list("workspace-tree-rows", entries.len(), move |range, _, cx| {
        range
            .filter_map(|index| {
                row_entries
                    .get(index)
                    .map(|entry| render_tree_row(index, entry, &row_runtime, cx))
            })
            .collect::<Vec<_>>()
    })
    .size_full()
    .with_sizing_behavior(ListSizingBehavior::Auto)
    .track_scroll(scroll_handle)
    .with_decoration(decoration);

    div()
        .id("workspace-tree")
        .key_context(TREE_KEY_CONTEXT)
        .track_focus(&runtime.focus_handle)
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_cancel))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_command_palette))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_confirm))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_rename))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_select_first))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_select_last))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_select_left))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_select_right))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_select_up))
        .on_action(window.listener_for(&runtime.sidebar, WorkspaceSidebar::on_select_down))
        .size_full()
        .min_h_0()
        .child(rows)
        .vertical_scrollbar(scroll_handle)
        .into_any_element()
}

fn row_is_active(active_target: Option<TreeNode>, node: TreeNode) -> bool {
    active_target == Some(node)
}

fn render_tree_row(
    index: usize,
    entry: &VisibleTreeEntry,
    runtime: &TreeRowRuntime,
    cx: &mut App,
) -> AnyElement {
    let row_group: SharedString =
        format!("workspace-tree-row-group-{}", entry.node.tree_id()).into();
    let node = entry.node;
    let target = entry.target();
    let connected = runtime
        .tree_model
        .host(node.host())
        .is_some_and(MuxTreeHost::connected);
    let expandable = entry.expandable;
    let select_sidebar = runtime.sidebar.clone();
    let activate_mux = runtime.mux.clone();
    let focus_handle = runtime.focus_handle.clone();
    let activation = runtime.tree_model.activation_for_node(
        node,
        runtime.attached_host,
        runtime.attached_session,
    );
    let active = row_is_active(runtime.active_target, node);
    let selected = runtime.focused && runtime.selected == Some(node);
    let host_indicator = if matches!(&entry.kind, TreeNodeKind::Host) {
        runtime
            .tree_model
            .host(node.host())
            .and_then(MuxTreeHost::indicator)
    } else {
        None
    };
    let node_color = if target.is_none() || connected && entry.on_active_path {
        cx.theme().foreground
    } else {
        cx.theme().foreground.muted()
    };

    let label = div()
        .flex()
        .min_w_0()
        .items_baseline()
        .gap_2()
        .text_color(node_color)
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(if node == TreeNode::Host(HostId::LOCAL) {
                    runtime.local_hostname.clone()
                } else {
                    entry.label.clone()
                }),
        );
    let trailing = div()
        .flex()
        .items_center()
        .gap_1()
        .when_some(host_indicator, |this, indicator| {
            this.child(render_host_indicator(index, indicator, cx))
        })
        .child(render_node_actions(entry, runtime, cx));
    let marker = render_node_marker(
        entry,
        connected && entry.on_active_path,
        runtime.tree_model.has_pending_bell(node),
        node_agent_badge(&runtime.tree_model, &runtime.badges, node),
        cx,
    );
    let marker = if expandable {
        let toggle_sidebar = runtime.sidebar.clone();
        workspace_tree_disclosure(marker, entry.expanded, row_group.clone(), cx)
            .id(("workspace-tree-disclosure", index))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                toggle_sidebar.update(cx, |sidebar, cx| {
                    sidebar.select_or_toggle_node(node, cx);
                });
            })
            .into_any_element()
    } else {
        marker
    };
    let hover_actions = !matches!(&entry.kind, TreeNodeKind::Host);
    let row = workspace_tree_row(
        ("workspace-tree-row", index),
        entry.depth,
        active,
        selected,
        runtime.focused,
        connected || target.is_none(),
        expandable || connected && target.is_some(),
        hover_actions,
        row_group,
        marker,
        label,
        trailing,
        cx,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        focus_handle.focus(window, cx);
    })
    .on_click(move |_, _, cx| {
        select_sidebar.update(cx, |sidebar, cx| {
            if activation.is_some() {
                sidebar.select_node_from_pointer(node, cx);
                sidebar.dismiss_slideover(cx);
            } else {
                sidebar.select_or_toggle_node(node, cx);
            }
        });
        if let Some(activation) = activation.clone() {
            activate_sidebar(&activate_mux, activation, cx);
        }
        cx.stop_propagation();
    });

    render_tree_row_context_menu(row, target, runtime)
}

fn render_host_indicator(index: usize, indicator: HostIndicator, cx: &mut App) -> AnyElement {
    match indicator {
        HostIndicator::Connecting => Spinner::new()
            .xsmall()
            .color(cx.theme().foreground.muted())
            .into_any_element(),
        HostIndicator::Failed { detail } => div()
            .id(("workspace-tree-host-indicator", index))
            .flex()
            .flex_none()
            .when_some(detail, |this, detail| {
                let toast_detail = detail.clone();
                this.cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        window.push_notification(Notification::warning(toast_detail.clone()), cx);
                    })
            })
            .child(
                Icon::new(IconName::Close)
                    .xsmall()
                    .text_color(cx.theme().danger),
            )
            .into_any_element(),
    }
}

fn render_tree_row_context_menu(
    row: gpui::Stateful<gpui::Div>,
    target: Option<(HostId, TreeTarget)>,
    runtime: &TreeRowRuntime,
) -> AnyElement {
    let rename_prompt = target.and_then(|(host, target)| {
        runtime
            .tree_model
            .rename_activation_for_node(TreeNode::Target(host, target), runtime.attached_host)
    });
    let Some((menu_label, activation)) = rename_prompt else {
        return row.into_any_element();
    };
    let rename_mux = runtime.mux.clone();
    row.context_menu(move |menu, _, _| {
        let rename_mux = rename_mux.clone();
        let activation = activation.clone();
        menu.item(PopupMenuItem::new(menu_label).on_click(move |_, _, cx| {
            activate_sidebar(&rename_mux, activation.clone(), cx);
        }))
    })
    .into_any_element()
}

const fn pane_kind_icon(kind: MuxTreePaneKind) -> IconName {
    match kind {
        MuxTreePaneKind::Picker => IconName::Plus,
        MuxTreePaneKind::Terminal => IconName::SquareTerminal,
        MuxTreePaneKind::Browser => IconName::Globe,
        MuxTreePaneKind::Agent => IconName::Bot,
        MuxTreePaneKind::Editor => IconName::File,
    }
}

fn render_node_marker(
    entry: &VisibleTreeEntry,
    on_active_path: bool,
    belled: bool,
    badge: Option<AgentBadge>,
    cx: &mut App,
) -> AnyElement {
    let icon = match &entry.kind {
        TreeNodeKind::Host => {
            return workspace_tree_marker(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        img(crate::app_icon::sidebar_logo())
                            .size(rems_from_px(TREE_NODE_ICON_SIZE)),
                    )
                    .when(belled, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(-1.0))
                                .right(px(-1.0))
                                .size(px(5.0))
                                .rounded_full()
                                .bg(cx.theme().warning),
                        )
                    })
                    .when_some(badge, |this, badge| {
                        this.child(
                            agent_badge_dot(badge, cx)
                                .absolute()
                                .bottom(px(-1.0))
                                .right(px(-1.0)),
                        )
                    }),
            )
            .into_any_element();
        }
        TreeNodeKind::Session => IconName::Layers,
        TreeNodeKind::Window { .. } => IconName::AppWindow,
        TreeNodeKind::Pane { kind } => pane_kind_icon(*kind),
    };

    let icon = Icon::new(icon)
        .size(rems_from_px(TREE_NODE_ICON_SIZE))
        .text_color(if on_active_path {
            cx.theme().foreground
        } else {
            cx.theme().foreground.muted()
        });
    workspace_tree_marker(
        div()
            .relative()
            .flex_none()
            .child(icon)
            .when(belled, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(-1.0))
                        .right(px(-1.0))
                        .size(px(5.0))
                        .rounded_full()
                        .bg(cx.theme().warning),
                )
            })
            .when_some(badge, |this, badge| {
                this.child(
                    agent_badge_dot(badge, cx)
                        .absolute()
                        .bottom(px(-1.0))
                        .right(px(-1.0)),
                )
            }),
    )
    .into_any_element()
}

fn sidebar_hostname(hostname: Option<&str>) -> SharedString {
    hostname
        .map(str::trim)
        .filter(|hostname| !hostname.is_empty())
        .unwrap_or("localhost")
        .to_owned()
        .into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeAction {
    HostMenu(HostId),
    NewWindow(HostId, SessionId),
    NewPane(HostId, WindowId, PaneId),
    Delete(HostId, TreeTarget),
}

fn node_actions(entry: &VisibleTreeEntry) -> Vec<NodeAction> {
    let mut actions = Vec::with_capacity(2);
    match (&entry.kind, entry.node) {
        (TreeNodeKind::Host, TreeNode::Host(host)) => actions.push(NodeAction::HostMenu(host)),
        (TreeNodeKind::Session, TreeNode::Target(host, TreeTarget::Session(session))) => {
            actions.push(NodeAction::NewWindow(host, session));
        }
        (
            TreeNodeKind::Window { active_pane },
            TreeNode::Target(host, TreeTarget::Window(window)),
        ) => actions.push(NodeAction::NewPane(host, window, *active_pane)),
        _ => {}
    }
    if let Some((host, target)) = entry.target() {
        actions.push(NodeAction::Delete(host, target));
    }
    actions
}

fn render_node_actions(
    entry: &VisibleTreeEntry,
    runtime: &TreeRowRuntime,
    cx: &mut App,
) -> AnyElement {
    let actions = node_actions(entry)
        .into_iter()
        .map(|action| match action {
            NodeAction::HostMenu(host) => render_host_actions(host, runtime),
            NodeAction::NewWindow(host, session) => {
                render_new_window_action(host, session, runtime)
            }
            NodeAction::NewPane(host, window, active_pane) => {
                render_new_pane_action(host, window, active_pane, runtime)
            }
            NodeAction::Delete(host, target) => render_delete_action(host, target, runtime, cx),
        })
        .collect::<Vec<_>>();

    div()
        .id(format!("workspace-tree-actions-{}", entry.node.tree_id()))
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .pr(px(TREE_ACTION_INSET))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .children(actions)
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostMenuAction {
    CloseHost,
    NewSession,
    AddHost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HostMenuItem {
    action: HostMenuAction,
    enabled: bool,
}

impl HostMenuAction {
    const fn label(self) -> &'static str {
        match self {
            Self::CloseHost => "Close host",
            Self::NewSession => "New session",
            Self::AddHost => "Add host",
        }
    }
}

fn host_menu_items(host: HostId, connected: bool) -> Vec<HostMenuItem> {
    let new_session = HostMenuItem {
        action: HostMenuAction::NewSession,
        enabled: connected,
    };
    if host == HostId::LOCAL {
        return vec![
            new_session,
            HostMenuItem {
                action: HostMenuAction::AddHost,
                enabled: true,
            },
        ];
    }
    vec![
        HostMenuItem {
            action: HostMenuAction::CloseHost,
            enabled: true,
        },
        new_session,
    ]
}

fn render_host_actions(host: HostId, runtime: &TreeRowRuntime) -> AnyElement {
    let tree_host = runtime.tree_model.host(host);
    let connected = tree_host.is_some_and(MuxTreeHost::connected);
    let name = SharedString::from(tree_host.map(|host| host.name.clone()).unwrap_or_default());
    let items = host_menu_items(host, connected);
    let menu_mux = runtime.mux.clone();
    Button::new(format!(
        "sidebar-host-actions-{}",
        TreeNode::Host(host).tree_id()
    ))
    .ghost()
    .xsmall()
    .icon(IconName::Ellipsis)
    .tooltip("Host actions")
    .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
        items.iter().fold(menu, |menu, item| {
            let item_mux = menu_mux.clone();
            let name = name.clone();
            let action = item.action;
            menu.item(
                PopupMenuItem::new(action.label())
                    .disabled(!item.enabled)
                    .on_click(move |_, window, cx| match action {
                        HostMenuAction::CloseHost => close_host(&item_mux, host, &name, cx),
                        HostMenuAction::NewSession => item_mux.read(cx).new_session(host),
                        HostMenuAction::AddHost => crate::workspace::add_host::open(window, cx),
                    }),
            )
        })
    })
    .into_any_element()
}

pub(crate) fn close_host(mux: &Entity<MuxClient>, host: HostId, name: &str, cx: &mut App) {
    mux.update(cx, |mux, cx| mux.release_host(host, cx));
    match crate::config::remove_fleet_host_live(name, cx) {
        Ok(removed) => {
            log::info!(target: "zz::config", "closed fleet host name={name} config_removed={removed}");
        }
        Err(error) => log::warn!("could not remove host-{name} from zz/config: {error}"),
    }
}

fn render_new_window_action(
    host: HostId,
    session: SessionId,
    runtime: &TreeRowRuntime,
) -> AnyElement {
    let new_window_mux = runtime.mux.clone();
    Button::new(format!(
        "sidebar-new-window-{}",
        TreeNode::Target(host, TreeTarget::Session(session)).tree_id()
    ))
    .ghost()
    .xsmall()
    .icon(IconName::Plus)
    .tooltip("New window")
    .disabled(
        !runtime
            .tree_model
            .host(host)
            .is_some_and(MuxTreeHost::connected),
    )
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        new_window_mux
            .read(cx)
            .execute_on_host(host, new_window_command(session));
    })
    .into_any_element()
}

fn render_new_pane_action(
    host: HostId,
    window: WindowId,
    active_pane: PaneId,
    runtime: &TreeRowRuntime,
) -> AnyElement {
    let mux = runtime.mux.clone();
    Button::new(format!(
        "sidebar-add-pane-{}",
        TreeNode::Target(host, TreeTarget::Window(window)).tree_id()
    ))
    .ghost()
    .xsmall()
    .icon(IconName::Plus)
    .tooltip("Add pane")
    .disabled(
        !runtime
            .tree_model
            .host(host)
            .is_some_and(MuxTreeHost::connected),
    )
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        mux.read(cx)
            .execute_on_host(host, split_picker_command(active_pane, Axis::Horizontal));
    })
    .into_any_element()
}

fn render_delete_action(
    host: HostId,
    target: TreeTarget,
    runtime: &TreeRowRuntime,
    cx: &mut App,
) -> AnyElement {
    let delete_mux = runtime.mux.clone();
    Button::new(format!(
        "sidebar-delete-{}",
        TreeNode::Target(host, target).tree_id()
    ))
    .ghost()
    .xsmall()
    .icon(Icon::new(IconName::Delete).text_color(cx.theme().danger))
    .tooltip(match target {
        TreeTarget::Session(_) => "Delete session",
        TreeTarget::Window(_) => "Delete window",
        TreeTarget::Pane(_) => "Delete pane",
    })
    .disabled(
        !runtime
            .tree_model
            .host(host)
            .is_some_and(MuxTreeHost::connected),
    )
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        delete_mux
            .read(cx)
            .execute_on_host(host, kill_target_command(target));
    })
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
    };

    use gpui::TestAppContext;
    use zz_daemon::DaemonError;
    use zz_protocol::{
        Axis, BrowserDescriptor, LayoutNode, PaneKindSnapshot, PaneSnapshot, SessionSnapshot,
        SplitId, WindowSnapshot,
    };

    use super::*;
    use crate::config::AgentConfig;

    #[gpui::test]
    fn workspace_tree_key_bindings_register(cx: &mut TestAppContext) {
        cx.update(init);
    }

    #[gpui::test]
    fn settings_view_is_lazy_and_retained(cx: &mut TestAppContext) {
        cx.update(zz_ui::init);
        let captured = Rc::new(RefCell::new(None));
        let captured_for_window = Rc::clone(&captured);
        let (_, cx) = cx.add_window_view(move |_, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let agents = cx.new(|_| AgentController::new(AgentConfig::default()));
            captured_for_window.replace(Some(cx.entity()));
            WorkspaceSidebar::new(mux, &agents, cx)
        });
        let sidebar = captured.borrow().clone().expect("captured sidebar");

        assert_eq!(
            sidebar.read_with(cx, |sidebar, _| sidebar.route()),
            WorkspaceRoute::App
        );
        assert!(
            sidebar
                .read_with(cx, |sidebar, _| sidebar.settings_view())
                .is_none()
        );

        let first = cx.update(|window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.open_settings(window, cx);
                sidebar.settings_view().expect("settings created on entry")
            })
        });
        assert_eq!(
            sidebar.read_with(cx, |sidebar, _| sidebar.route()),
            WorkspaceRoute::Settings
        );

        cx.update(|window, cx| {
            sidebar.update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
        });
        assert_eq!(
            sidebar.read_with(cx, |sidebar, _| sidebar.route()),
            WorkspaceRoute::App
        );

        let reopened = cx.update(|window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.open_settings(window, cx);
                sidebar
                    .settings_view()
                    .expect("settings retained on re-entry")
            })
        });
        assert_eq!(first.entity_id(), reopened.entity_id());
    }

    struct ShellProbe {
        sidebar: Entity<WorkspaceSidebar>,
        workspace: Entity<WorkspaceProbe>,
        slideover_frames: Rc<Cell<usize>>,
    }

    impl Render for ShellProbe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if self.sidebar.read(cx).slideover_open() {
                self.slideover_frames.set(self.slideover_frames.get() + 1);
            }
            div().child(self.workspace.clone())
        }
    }

    struct WorkspaceProbe {
        sidebar: Entity<WorkspaceSidebar>,
        pending_focus: Rc<Cell<bool>>,
    }

    impl Render for WorkspaceProbe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if self.pending_focus.replace(false) {
                self.sidebar
                    .update(cx, |sidebar, cx| sidebar.focus(window, cx));
            }
            div()
        }
    }

    #[gpui::test]
    fn raising_the_slideover_mid_draw_schedules_the_frame_that_mounts_it(cx: &mut TestAppContext) {
        let slideover_frames = Rc::new(Cell::new(0));
        let pending_focus = Rc::new(Cell::new(true));
        let counted_frames = Rc::clone(&slideover_frames);
        let (_, cx) = cx.add_window_view(move |_, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let agents = cx.new(|_| AgentController::new(AgentConfig::default()));
            let sidebar = cx.new(|cx| WorkspaceSidebar::new(mux, &agents, cx));
            sidebar.update(cx, WorkspaceSidebar::toggle_mode);
            assert_eq!(sidebar.read(cx).mode(), ChromeMode::Titlebar);
            let workspace = cx.new(|_| WorkspaceProbe {
                sidebar: sidebar.clone(),
                pending_focus,
            });
            ShellProbe {
                sidebar,
                workspace,
                slideover_frames: counted_frames,
            }
        });
        cx.run_until_parked();

        assert!(
            slideover_frames.get() > 0,
            "the raise never asked for the frame that mounts it",
        );
    }

    #[test]
    fn plain_keys_bind_only_inside_the_focused_workspace_tree() {
        let keymap = gpui::Keymap::new(workspace_tree_key_bindings(&crate::keymap::test_chords(
            SIDEBAR_TABLE,
        )));
        let tree_context =
            gpui::KeyContext::parse(TREE_KEY_CONTEXT).expect("valid workspace tree context");
        let root_context = gpui::KeyContext::parse("Root").expect("valid root context");
        for (key, action) in [
            ("r", std::any::TypeId::of::<TreeRename>()),
            (":", std::any::TypeId::of::<TreeCommandPalette>()),
        ] {
            let key = gpui::Keystroke::parse(key).expect("valid tree key");

            let (tree_bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&key),
                std::slice::from_ref(&tree_context),
            );
            assert_eq!(tree_bindings.len(), 1);
            assert_eq!(tree_bindings[0].action().as_any().type_id(), action);
            assert!(!pending);

            let (root_bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&key),
                std::slice::from_ref(&root_context),
            );
            assert!(root_bindings.is_empty());
            assert!(!pending);
        }
    }

    fn terminal_pane(id: u64, title: &str) -> PaneSnapshot {
        PaneSnapshot {
            id: PaneId(id),
            title: title.to_owned(),
            kind: PaneKindSnapshot::Terminal,
            synchronized_input: false,
            bell: false,
            dead: false,
            dead_status: None,
        }
    }

    fn browser_pane(id: u64, title: &str, url: &str) -> PaneSnapshot {
        PaneSnapshot {
            id: PaneId(id),
            title: title.to_owned(),
            kind: PaneKindSnapshot::Browser(BrowserDescriptor::single(
                url.to_owned(),
                "default".to_owned(),
            )),
            synchronized_input: false,
            bell: false,
            dead: false,
            dead_status: None,
        }
    }

    fn mux_window(id: u64, index: u32, name: &str, pane: u64) -> WindowSnapshot {
        let pane = terminal_pane(pane, name);
        WindowSnapshot {
            id: WindowId(id),
            index,
            name: name.to_owned(),
            automatic_rename: true,
            active_pane: pane.id,
            zoomed_pane: None,
            layout: LayoutNode::Pane(pane.id),
            panes: BTreeMap::from([(pane.id, pane)]),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
        }
    }

    fn snapshot_with_two_panes() -> MuxSnapshot {
        let terminal = terminal_pane(101, "");
        let browser = browser_pane(202, "", "https://zed.dev");
        let window = WindowSnapshot {
            id: WindowId(11),
            index: 2,
            name: "work".to_owned(),
            automatic_rename: true,
            active_pane: browser.id,
            zoomed_pane: None,
            layout: LayoutNode::Split {
                id: SplitId(1),
                axis: Axis::Horizontal,
                ratio: 5000.0,
                first: Box::new(LayoutNode::Pane(browser.id)),
                second: Box::new(LayoutNode::Pane(terminal.id)),
            },
            panes: BTreeMap::from([(terminal.id, terminal), (browser.id, browser)]),
            layout_dump: String::new(),
            visible_layout_dump: String::new(),
        };
        MuxSnapshot {
            generation: 7,
            focused_window: None,
            sessions: vec![SessionSnapshot {
                id: SessionId(1),
                name: "main workspace".to_owned(),
                active_window: window.id,
                windows: vec![window],
                viewers: Vec::new(),
            }],
        }
    }

    fn local_model(snapshot: &MuxSnapshot, attached: Option<SessionId>) -> MuxTreeModel {
        let connected = HostState::Connected;
        MuxTreeModel::from_hosts(
            HostId::LOCAL,
            attached,
            [(HostId::LOCAL, "local", &connected, Some(snapshot))],
        )
    }

    fn host_ids(names: &[&str]) -> Vec<HostId> {
        let configured = names
            .iter()
            .map(|name| crate::config::HostEntry {
                name: (*name).to_owned(),
                endpoint: zz_daemon::Endpoint::parse(&format!("ssh://{name}"))
                    .expect("test endpoint"),
            })
            .collect::<Vec<_>>();
        let registry = crate::mux::hosts::HostRegistry::new(
            std::path::PathBuf::from("/tmp/zz-sidebar-local.sock"),
            &configured,
            crate::profile::LocalHostPolicy::Always,
        );
        names
            .iter()
            .map(|name| registry.get_by_name(name).unwrap().0)
            .collect()
    }

    #[test]
    fn sidebar_resize_clamps_to_a_readable_column() {
        assert_eq!(sidebar_resize_width(0.0, 1_000.0), SIDEBAR_MIN_WIDTH);
        assert_eq!(sidebar_resize_width(300.0, 1_000.0), 300.0);
        assert_eq!(sidebar_resize_width(800.0, 1_000.0), 500.0);
        assert_eq!(sidebar_resize_width(800.0, 2_000.0), SIDEBAR_MAX_WIDTH);
    }

    #[test]
    fn attention_owner_resolves_from_the_live_snapshot() {
        let snapshot = snapshot_with_two_panes();
        assert_eq!(
            session_owning_pane(&snapshot, PaneId(101)),
            Some(SessionId(1))
        );
        assert_eq!(
            session_owning_pane(&snapshot, PaneId(202)),
            Some(SessionId(1))
        );
        assert_eq!(session_owning_pane(&snapshot, PaneId(999)), None);
    }

    #[test]
    fn agent_badges_bubble_to_every_collapsed_ancestor() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        let badges = BTreeMap::from([
            ((HostId::LOCAL, PaneId(101)), AgentBadge::Finished),
            ((HostId::LOCAL, PaneId(202)), AgentBadge::NeedsInput),
        ]);
        let badge = |node| node_agent_badge(&model, &badges, node);

        assert_eq!(
            badge(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(101))
            )),
            Some(AgentBadge::Finished)
        );
        for node in [
            TreeNode::Host(HostId::LOCAL),
            TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1))),
            TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
        ] {
            assert_eq!(badge(node), Some(AgentBadge::NeedsInput));
        }
        assert_eq!(
            node_agent_badge(&model, &BTreeMap::new(), TreeNode::Host(HostId::LOCAL)),
            None
        );
    }

    #[test]
    fn agent_badges_do_not_cross_host_id_boundaries() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let snapshot = snapshot_with_two_panes();
        let model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&snapshot)),
                (remote, "studio", &connected, Some(&snapshot)),
            ],
        );
        let badges = BTreeMap::from([((HostId::LOCAL, PaneId(202)), AgentBadge::NeedsInput)]);

        assert_eq!(
            node_agent_badge(
                &model,
                &badges,
                TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(202)))
            ),
            Some(AgentBadge::NeedsInput)
        );
        assert_eq!(
            node_agent_badge(
                &model,
                &badges,
                TreeNode::Target(remote, TreeTarget::Pane(PaneId(202)))
            ),
            None
        );
        assert_eq!(
            node_agent_badge(&model, &badges, TreeNode::Host(remote)),
            None
        );
    }

    #[test]
    fn strip_chips_name_every_session_even_an_unnamed_one() {
        assert_eq!(session_label("builds", SessionId(3)), "builds");
        assert_eq!(session_label("   ", SessionId(3)), "session $3");
        assert_eq!(session_initial("builds").as_ref(), "B");
        assert_eq!(session_initial("  research").as_ref(), "R");
        assert_eq!(session_initial("").as_ref(), "?");
    }

    #[test]
    fn strip_window_chips_use_the_engine_window_name() {
        let snapshot = snapshot_with_two_panes();
        let window = &snapshot.sessions[0].windows[0];
        assert_eq!(strip_window_label(window), "2:work");
        let mut pinned = window.clone();
        pinned.automatic_rename = false;
        assert_eq!(strip_window_label(&pinned), "2:work");
        assert_eq!(
            strip_window_label(&mux_window(4, 1, "agents", 9)),
            "1:agents"
        );

        let mut orphaned = mux_window(5, 3, "scratch", 12);
        orphaned.panes.clear();
        assert_eq!(strip_window_label(&orphaned), "3:scratch");
    }

    #[test]
    fn sidebar_host_row_formats_the_hostname_with_a_local_fallback() {
        assert_eq!(
            sidebar_hostname(Some("studio.local")).as_ref(),
            "studio.local"
        );
        assert_eq!(sidebar_hostname(Some("  ")).as_ref(), "localhost");
        assert_eq!(sidebar_hostname(None).as_ref(), "localhost");
    }

    #[test]
    fn only_the_active_workspace_row_reads_as_active() {
        let pane = TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(7)));
        let other_pane = TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(8)));
        let window = TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(3)));
        let remote_host = TreeNode::Host(host_ids(&["remote"])[0]);

        assert!(row_is_active(Some(pane), pane));
        assert!(!row_is_active(Some(pane), other_pane,));
        assert!(!row_is_active(Some(pane), window));
        assert!(row_is_active(Some(window), window));
        assert!(row_is_active(Some(remote_host), remote_host));
        assert!(!row_is_active(None, pane));
        assert!(!row_is_active(Some(pane), TreeNode::Host(HostId::LOCAL)));
    }

    #[test]
    fn flattened_projection_starts_with_the_local_host() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        let expanded = BTreeSet::from([
            TreeNode::Host(HostId::LOCAL),
            TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1))),
            TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
        ]);
        let projection = TreeProjection::new(&model, &expanded);

        assert_eq!(
            projection
                .entries
                .iter()
                .map(|entry| entry.depth)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 3]
        );
        assert_eq!(projection.entries[0].node, TreeNode::Host(HostId::LOCAL));
        assert!(matches!(projection.entries[0].kind, TreeNodeKind::Host));
        assert!(projection.entries[0].expandable);
        assert_eq!(
            projection.entries.last().map(|entry| entry.node),
            Some(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(101))
            ))
        );
    }

    #[test]
    fn an_added_host_takes_the_next_row_while_it_connects() {
        let studio = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let snapshot = snapshot_with_two_panes();
        let before = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [(HostId::LOCAL, "local", &connected, Some(&snapshot))],
        );
        assert_eq!(
            TreeProjection::new(&before, &BTreeSet::new())
                .entries
                .iter()
                .map(|entry| entry.node)
                .collect::<Vec<_>>(),
            [TreeNode::Host(HostId::LOCAL)]
        );

        let connecting = HostState::Connecting;
        let after = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&snapshot)),
                (studio, "studio", &connecting, None),
            ],
        );
        assert_eq!(
            TreeProjection::new(&after, &BTreeSet::new())
                .entries
                .iter()
                .map(|entry| entry.node)
                .collect::<Vec<_>>(),
            [TreeNode::Host(HostId::LOCAL), TreeNode::Host(studio)]
        );
        assert_eq!(
            after.host(studio).and_then(MuxTreeHost::indicator),
            Some(HostIndicator::Connecting)
        );
    }

    #[test]
    fn active_hierarchy_marks_session_window_and_focused_pane() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        let expanded = BTreeSet::from([
            TreeNode::Host(HostId::LOCAL),
            TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1))),
            TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
        ]);
        let projection = TreeProjection::new(&model, &expanded);

        assert_eq!(
            projection
                .entries
                .iter()
                .map(|entry| entry.on_active_path)
                .collect::<Vec<_>>(),
            vec![false, true, true, true, false]
        );
    }

    #[test]
    fn split_actions_require_an_attached_active_pane() {
        let mut snapshot = snapshot_with_two_panes();

        assert_eq!(
            active_pane_for_split(&snapshot, Some(SessionId(1))),
            Some(PaneId(202))
        );
        assert_eq!(active_pane_for_split(&snapshot, None), None);

        snapshot.sessions[0].windows.clear();
        assert_eq!(active_pane_for_split(&snapshot, Some(SessionId(1))), None);
    }

    #[test]
    fn stamped_window_focus_drives_the_local_active_tree() {
        let mut snapshot = snapshot_with_two_panes();
        let focused_window = mux_window(12, 3, "logs", 303);
        snapshot.sessions[0].windows.push(focused_window);
        snapshot.focused_window = Some(WindowId(12));

        let model = local_model(&snapshot, Some(SessionId(1)));
        let session = &model.host(HostId::LOCAL).unwrap().sessions[0];
        assert!(!session.windows[0].active);
        assert!(session.windows[1].active);
        assert_eq!(
            model.active_target,
            Some(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(303))
            ))
        );
        assert_eq!(
            active_tree_target(&snapshot, Some(SessionId(1))),
            Some(TreeTarget::Pane(PaneId(303)))
        );
    }

    fn remote_snapshot() -> MuxSnapshot {
        MuxSnapshot {
            generation: 9,
            focused_window: None,
            sessions: vec![
                SessionSnapshot {
                    id: SessionId(1),
                    name: "same id elsewhere".to_owned(),
                    active_window: WindowId(11),
                    windows: vec![mux_window(11, 0, "remote window", 202)],
                    viewers: Vec::new(),
                },
                SessionSnapshot {
                    id: SessionId(9),
                    name: "builds".to_owned(),
                    active_window: WindowId(21),
                    windows: vec![mux_window(21, 1, "compile", 303)],
                    viewers: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn remote_hosts_project_their_cached_snapshot_beside_the_local_tree() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let local_snapshot = snapshot_with_two_panes();
        let remote_snapshot = remote_snapshot();
        let model = MuxTreeModel::from_hosts(
            remote,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );
        let local_session = TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1)));
        let local_window = TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11)));
        let remote_session = TreeNode::Target(remote, TreeTarget::Session(SessionId(1)));
        let remote_window = TreeNode::Target(remote, TreeTarget::Window(WindowId(11)));
        let remote_pane = TreeNode::Target(remote, TreeTarget::Pane(PaneId(202)));
        let expanded = BTreeSet::from([
            TreeNode::Host(HostId::LOCAL),
            TreeNode::Host(remote),
            local_session,
            local_window,
            remote_session,
            remote_window,
        ]);
        let projection = TreeProjection::new(&model, &expanded);

        assert_eq!(
            model.hosts.iter().map(|host| host.id).collect::<Vec<_>>(),
            [HostId::LOCAL, remote]
        );
        assert!(model.is_expandable(TreeNode::Host(remote)));
        assert!(model.is_expandable(remote_session));
        assert!(model.is_expandable(remote_window));
        assert_eq!(model.active_target, Some(remote_pane));
        assert!(row_is_active(model.active_target, remote_pane));
        assert_eq!(
            projection
                .entries
                .iter()
                .map(|entry| (entry.node, entry.depth))
                .collect::<Vec<_>>(),
            [
                (TreeNode::Host(HostId::LOCAL), 0),
                (local_session, 1),
                (local_window, 2),
                (
                    TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(202))),
                    3,
                ),
                (
                    TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(101))),
                    3,
                ),
                (TreeNode::Host(remote), 0),
                (remote_session, 1),
                (remote_window, 2),
                (remote_pane, 3),
                (
                    TreeNode::Target(remote, TreeTarget::Session(SessionId(9))),
                    1
                ),
            ]
        );
        assert!(
            projection.entries[projection.visible_indices[&remote_pane]].on_active_path,
            "the attached machine's focused pane row reads as active"
        );

        let collapsed =
            TreeProjection::new(&model, &BTreeSet::from([TreeNode::Host(HostId::LOCAL)]));
        assert!(collapsed.expandable.contains(&TreeNode::Host(remote)));
        assert!(!collapsed.visible_indices.contains_key(&remote_session));
    }

    #[test]
    fn remote_rows_attach_to_the_owning_session_then_select_the_clicked_row() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let disconnected = HostState::Disconnected;
        let local_snapshot = snapshot_with_two_panes();
        let remote_snapshot = remote_snapshot();
        let model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );
        let activation = |node| model.activation_for_node(node, HostId::LOCAL, Some(SessionId(1)));

        assert_eq!(
            activation(TreeNode::Target(remote, TreeTarget::Session(SessionId(1)))),
            Some(SidebarActivation::Attach {
                host: remote,
                session: SessionId(1),
            })
        );
        assert_eq!(
            activation(TreeNode::Target(remote, TreeTarget::Session(SessionId(9)))),
            Some(SidebarActivation::Attach {
                host: remote,
                session: SessionId(9),
            })
        );
        assert_eq!(
            activation(TreeNode::Target(remote, TreeTarget::Window(WindowId(21)))),
            Some(SidebarActivation::AttachThenExecute {
                host: remote,
                session: SessionId(9),
                command: select_window_command(WindowId(21)),
            })
        );
        assert_eq!(
            activation(TreeNode::Target(remote, TreeTarget::Pane(PaneId(303)))),
            Some(SidebarActivation::AttachThenExecute {
                host: remote,
                session: SessionId(9),
                command: select_pane_command(PaneId(303)),
            })
        );
        assert_eq!(
            activation(TreeNode::Target(remote, TreeTarget::Pane(PaneId(202)))),
            Some(SidebarActivation::AttachThenExecute {
                host: remote,
                session: SessionId(1),
                command: select_pane_command(PaneId(202)),
            })
        );
        assert_eq!(
            activation(TreeNode::Host(remote)),
            Some(SidebarActivation::AttachHost(remote))
        );

        let offline = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &disconnected, Some(&remote_snapshot)),
            ],
        );
        assert_eq!(
            offline.activation_for_node(
                TreeNode::Target(remote, TreeTarget::Session(SessionId(9))),
                HostId::LOCAL,
                Some(SessionId(1)),
            ),
            None,
            "a stale subtree stays inert until its machine is back"
        );
        assert_eq!(
            offline.activation_for_node(TreeNode::Host(remote), HostId::LOCAL, Some(SessionId(1))),
            Some(SidebarActivation::Reconnect(remote))
        );
    }

    #[test]
    fn every_expandable_row_carries_a_disclosure_even_when_clicking_it_activates() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let local_snapshot = snapshot_with_two_panes();
        let remote_snapshot = remote_snapshot();
        let model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );
        let remote_host = TreeNode::Host(remote);
        let remote_session = TreeNode::Target(remote, TreeTarget::Session(SessionId(9)));
        let remote_window = TreeNode::Target(remote, TreeTarget::Window(WindowId(21)));
        let remote_pane = TreeNode::Target(remote, TreeTarget::Pane(PaneId(303)));

        assert!(
            model
                .activation_for_node(remote_host, HostId::LOCAL, Some(SessionId(1)))
                .is_some()
        );

        let collapsed = TreeProjection::new(&model, &BTreeSet::new());
        let row = |projection: &TreeProjection, node| {
            let entry = &projection.entries[projection.visible_indices[&node]];
            (entry.expandable, entry.expanded)
        };
        assert_eq!(row(&collapsed, remote_host), (true, false));

        let expanded = TreeProjection::new(
            &model,
            &BTreeSet::from([remote_host, remote_session, remote_window]),
        );
        assert_eq!(row(&expanded, remote_host), (true, true));
        assert_eq!(row(&expanded, remote_session), (true, true));
        assert_eq!(row(&expanded, remote_window), (true, true));
        assert_eq!(row(&expanded, remote_pane), (false, false));
        let half_open = TreeProjection::new(&model, &BTreeSet::from([remote_host]));
        assert_eq!(row(&half_open, remote_session), (true, false));
    }

    #[test]
    fn row_controls_and_rename_actions_belong_to_their_machine() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let local_snapshot = snapshot_with_two_panes();
        let remote_snapshot = remote_snapshot();
        let model = MuxTreeModel::from_hosts(
            remote,
            Some(SessionId(9)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );
        let projection = TreeProjection::new(
            &model,
            &BTreeSet::from([
                TreeNode::Host(HostId::LOCAL),
                TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1))),
                TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
                TreeNode::Host(remote),
                TreeNode::Target(remote, TreeTarget::Session(SessionId(9))),
                TreeNode::Target(remote, TreeTarget::Window(WindowId(21))),
            ]),
        );
        let actions = |node| node_actions(&projection.entries[projection.visible_indices[&node]]);

        assert_eq!(
            actions(TreeNode::Host(remote)),
            [NodeAction::HostMenu(remote)]
        );
        assert_eq!(
            actions(TreeNode::Target(remote, TreeTarget::Session(SessionId(9)))),
            [
                NodeAction::NewWindow(remote, SessionId(9)),
                NodeAction::Delete(remote, TreeTarget::Session(SessionId(9))),
            ]
        );
        assert_eq!(
            actions(TreeNode::Target(remote, TreeTarget::Window(WindowId(21)))),
            [
                NodeAction::NewPane(remote, WindowId(21), PaneId(303)),
                NodeAction::Delete(remote, TreeTarget::Window(WindowId(21))),
            ]
        );
        assert_eq!(
            actions(TreeNode::Target(remote, TreeTarget::Pane(PaneId(303)))),
            [NodeAction::Delete(remote, TreeTarget::Pane(PaneId(303)))]
        );
        assert_eq!(
            actions(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Window(WindowId(11))
            )),
            [
                NodeAction::NewPane(HostId::LOCAL, WindowId(11), PaneId(202)),
                NodeAction::Delete(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
            ]
        );

        assert_eq!(
            model.renameable_name(remote, TreeTarget::Session(SessionId(9))),
            Some("builds")
        );
        assert_eq!(
            model.renameable_name(remote, TreeTarget::Window(WindowId(21))),
            Some("compile")
        );
        assert_eq!(
            model.rename_activation_for_node(
                TreeNode::Target(remote, TreeTarget::Window(WindowId(21))),
                remote,
            ),
            Some((
                "Rename Window…",
                SidebarActivation::Execute {
                    host: remote,
                    command: rename_prompt_command(TreeTarget::Window(WindowId(21)), "compile",)
                        .unwrap()
                        .1,
                },
            ))
        );
        assert_eq!(
            model.rename_activation_for_node(
                TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(202))),
                remote,
            ),
            Some((
                "Rename Window…",
                SidebarActivation::AttachThenExecute {
                    host: HostId::LOCAL,
                    session: SessionId(1),
                    command: rename_prompt_command(TreeTarget::Window(WindowId(11)), "work")
                        .unwrap()
                        .1,
                },
            ))
        );

        let disconnected = HostState::Disconnected;
        let offline = MuxTreeModel::from_hosts(
            remote,
            Some(SessionId(9)),
            [
                (HostId::LOCAL, "local", &disconnected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );
        assert_eq!(
            offline.rename_activation_for_node(
                TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
                remote,
            ),
            None,
        );
    }

    #[test]
    fn pending_bells_bubble_through_collapsed_remote_ancestors() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let local_snapshot = snapshot_with_two_panes();
        let mut remote_snapshot = remote_snapshot();
        remote_snapshot.sessions[1].windows[0]
            .panes
            .get_mut(&PaneId(303))
            .unwrap()
            .bell = true;
        let model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );

        for node in [
            TreeNode::Host(remote),
            TreeNode::Target(remote, TreeTarget::Session(SessionId(9))),
            TreeNode::Target(remote, TreeTarget::Window(WindowId(21))),
            TreeNode::Target(remote, TreeTarget::Pane(PaneId(303))),
        ] {
            assert!(model.has_pending_bell(node), "missing bell on {node:?}");
        }
        assert!(!model.has_pending_bell(TreeNode::Host(HostId::LOCAL)));
    }

    #[test]
    fn the_host_menu_gates_only_the_item_that_needs_a_live_connection() {
        let remote = host_ids(&["studio"])[0];
        let item = |action, enabled| HostMenuItem { action, enabled };

        assert_eq!(
            host_menu_items(remote, true),
            [
                item(HostMenuAction::CloseHost, true),
                item(HostMenuAction::NewSession, true),
            ]
        );
        assert_eq!(
            host_menu_items(remote, false),
            [
                item(HostMenuAction::CloseHost, true),
                item(HostMenuAction::NewSession, false),
            ]
        );
        assert_eq!(
            host_menu_items(HostId::LOCAL, true),
            [
                item(HostMenuAction::NewSession, true),
                item(HostMenuAction::AddHost, true),
            ]
        );
        assert_eq!(
            host_menu_items(HostId::LOCAL, false),
            [
                item(HostMenuAction::NewSession, false),
                item(HostMenuAction::AddHost, true),
            ]
        );
    }

    #[test]
    fn attaching_to_a_machine_expands_it_down_to_the_active_pane() {
        let remote = host_ids(&["studio"])[0];
        let connected = HostState::Connected;
        let local_snapshot = snapshot_with_two_panes();
        let remote_snapshot = remote_snapshot();
        let model = MuxTreeModel::from_hosts(
            remote,
            Some(SessionId(9)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (remote, "studio", &connected, Some(&remote_snapshot)),
            ],
        );
        let active = model
            .active_target
            .expect("the attached machine names an active pane");
        let mut expanded = BTreeSet::from([TreeNode::Host(HostId::LOCAL)]);

        expand_path_to(&mut expanded, &model, active);

        assert_eq!(
            active,
            TreeNode::Target(remote, TreeTarget::Pane(PaneId(303)))
        );
        assert_eq!(
            expanded,
            BTreeSet::from([
                TreeNode::Host(HostId::LOCAL),
                TreeNode::Host(remote),
                TreeNode::Target(remote, TreeTarget::Session(SessionId(9))),
                TreeNode::Target(remote, TreeTarget::Window(WindowId(21))),
            ])
        );
        let projection = TreeProjection::new(&model, &expanded);
        assert!(projection.visible_indices.contains_key(&active));
    }

    #[test]
    fn host_indicators_cover_loading_connecting_and_failure_states() {
        let ids = host_ids(&[
            "ready", "loading", "dialing", "roaming", "offline", "skewed", "idle",
        ]);
        let connected = HostState::Connected;
        let connecting = HostState::Connecting;
        let reconnecting = HostState::Reconnecting { attempt: 4 };
        let unreachable = HostState::Unreachable {
            reason: "connection refused\nssh detail".to_owned(),
        };
        let incompatible = HostState::Incompatible {
            local: 33,
            remote: 31,
        };
        let disconnected = HostState::Disconnected;
        let loaded_snapshot = MuxSnapshot::default();
        let model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            None,
            [
                (ids[0], "ready", &connected, Some(&loaded_snapshot)),
                (ids[1], "loading", &connected, None),
                (ids[2], "dialing", &connecting, None),
                (ids[3], "roaming", &reconnecting, None),
                (ids[4], "offline", &unreachable, None),
                (ids[5], "skewed", &incompatible, None),
                (ids[6], "idle", &disconnected, None),
            ],
        );

        assert_eq!(
            model
                .hosts
                .iter()
                .map(MuxTreeHost::indicator)
                .collect::<Vec<_>>(),
            [
                None,
                Some(HostIndicator::Connecting),
                Some(HostIndicator::Connecting),
                Some(HostIndicator::Connecting),
                Some(HostIndicator::Failed {
                    detail: Some("connection refused\nssh detail".into()),
                }),
                Some(HostIndicator::Failed {
                    detail: Some(
                        "This zz speaks protocol v33; that machine speaks v31.\nUpgrade whichever \
                         side is older, then reconnect."
                            .into(),
                    ),
                }),
                Some(HostIndicator::Failed { detail: None }),
            ]
        );
    }

    #[test]
    fn host_activation_switches_connected_machines_and_retries_failed_rows() {
        let ids = host_ids(&["connected", "roaming", "offline", "skewed"]);
        let connected = HostState::Connected;
        let reconnecting = HostState::Reconnecting { attempt: 2 };
        let unreachable = HostState::Unreachable {
            reason: "connection refused".to_owned(),
        };
        let incompatible = HostState::Incompatible {
            local: 33,
            remote: 31,
        };
        let remote_snapshot = MuxSnapshot {
            generation: 2,
            focused_window: None,
            sessions: vec![SessionSnapshot {
                id: SessionId(7),
                name: "remote".to_owned(),
                active_window: WindowId(0),
                windows: Vec::new(),
                viewers: Vec::new(),
            }],
        };
        let model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [
                (ids[0], "connected", &connected, Some(&remote_snapshot)),
                (ids[1], "roaming", &reconnecting, None),
                (ids[2], "offline", &unreachable, None),
                (ids[3], "skewed", &incompatible, None),
            ],
        );

        assert_eq!(
            model.activation_for_node(TreeNode::Host(ids[0]), HostId::LOCAL, Some(SessionId(1)),),
            Some(SidebarActivation::AttachHost(ids[0]))
        );
        assert_eq!(
            model.activation_for_node(TreeNode::Host(ids[1]), HostId::LOCAL, Some(SessionId(1)),),
            Some(SidebarActivation::Reconnect(ids[1]))
        );
        assert_eq!(
            model.activation_for_node(TreeNode::Host(ids[2]), HostId::LOCAL, Some(SessionId(1)),),
            Some(SidebarActivation::Reconnect(ids[2]))
        );
        assert_eq!(
            model.activation_for_node(TreeNode::Host(ids[3]), HostId::LOCAL, Some(SessionId(1)),),
            Some(SidebarActivation::Reconnect(ids[3]))
        );

        let local_snapshot = MuxSnapshot::default();
        let switch_back_model = MuxTreeModel::from_hosts(
            ids[0],
            Some(SessionId(7)),
            [
                (HostId::LOCAL, "local", &connected, Some(&local_snapshot)),
                (ids[0], "connected", &connected, Some(&remote_snapshot)),
            ],
        );
        assert_eq!(
            switch_back_model.activation_for_node(
                TreeNode::Host(HostId::LOCAL),
                ids[0],
                Some(SessionId(7)),
            ),
            Some(SidebarActivation::AttachHost(HostId::LOCAL))
        );
        assert_eq!(
            switch_back_model.activation_for_node(
                TreeNode::Host(ids[0]),
                ids[0],
                Some(SessionId(7)),
            ),
            None
        );

        let local_model = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            Some(SessionId(1)),
            [(HostId::LOCAL, "local", &unreachable, None)],
        );
        assert_eq!(
            local_model.activation_for_node(
                TreeNode::Host(HostId::LOCAL),
                HostId::LOCAL,
                Some(SessionId(1)),
            ),
            Some(SidebarActivation::Reconnect(HostId::LOCAL))
        );
    }

    #[test]
    fn only_the_local_machine_auto_expands_when_it_joins_the_fleet() {
        let ids = host_ids(&["studio", "server"]);
        let disconnected = HostState::Disconnected;
        let previous = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            None,
            [
                (HostId::LOCAL, "local", &disconnected, None),
                (ids[0], "studio", &disconnected, None),
            ],
        );
        let next = MuxTreeModel::from_hosts(
            HostId::LOCAL,
            None,
            [
                (HostId::LOCAL, "local", &disconnected, None),
                (ids[0], "studio", &disconnected, None),
                (ids[1], "server", &disconnected, None),
            ],
        );
        let mut expanded = BTreeSet::new();

        expand_new_hosts(&mut expanded, &MuxTreeModel::default(), &previous);
        assert_eq!(expanded, BTreeSet::from([TreeNode::Host(HostId::LOCAL)]),);

        expand_new_hosts(&mut expanded, &previous, &next);

        assert_eq!(expanded, BTreeSet::from([TreeNode::Host(HostId::LOCAL)]));
    }

    #[test]
    fn projection_is_exactly_session_window_pane_and_uses_layout_order() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        let sessions = &model.host(HostId::LOCAL).unwrap().sessions;

        assert_eq!(model.max_depth(), 3);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].windows.len(), 1);
        assert_eq!(
            sessions[0].windows[0]
                .panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            vec![PaneId(202), PaneId(101)]
        );
        assert_eq!(sessions[0].windows[0].panes[0].label, "https://zed.dev");
        assert_eq!(
            sessions[0].windows[0].panes[0].kind,
            MuxTreePaneKind::Browser
        );
        assert_eq!(sessions[0].windows[0].panes[1].label, "terminal");
        assert_eq!(
            sessions[0].windows[0].panes[1].kind,
            MuxTreePaneKind::Terminal
        );
        assert_eq!(
            model.active_target,
            Some(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(202))
            ))
        );
    }

    #[test]
    fn sidebar_window_labels_omit_the_window_index() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));

        assert_eq!(
            model.host(HostId::LOCAL).unwrap().sessions[0].windows[0].label(),
            "work"
        );
    }

    #[test]
    fn pending_picker_panes_have_a_distinct_sidebar_kind_and_fallback_label() {
        let pane = PaneSnapshot {
            id: PaneId(303),
            title: String::new(),
            kind: PaneKindSnapshot::Picker,
            synchronized_input: false,
            bell: false,
            dead: false,
            dead_status: None,
        };

        let projected = MuxTreePane::from_snapshot(&pane);
        assert_eq!(projected.kind, MuxTreePaneKind::Picker);
        assert_eq!(projected.label, "new pane");
    }

    #[test]
    fn agent_panes_have_a_distinct_sidebar_kind_and_fallback_label() {
        let pane = PaneSnapshot {
            id: PaneId(404),
            title: String::new(),
            kind: PaneKindSnapshot::Agent(zz_protocol::AgentDescriptor::default()),
            synchronized_input: false,
            bell: false,
            dead: false,
            dead_status: None,
        };

        let projected = MuxTreePane::from_snapshot(&pane);
        assert_eq!(projected.kind, MuxTreePaneKind::Agent);
        assert_eq!(projected.label, "agent");
    }

    #[test]
    fn every_session_projects_all_windows_and_panes() {
        let first_window = mux_window(11, 0, "shell", 101);
        let second_window = mux_window(22, 1, "web", 202);
        let snapshot = MuxSnapshot {
            generation: 8,
            focused_window: None,
            sessions: vec![
                SessionSnapshot {
                    id: SessionId(1),
                    name: "main".to_owned(),
                    active_window: first_window.id,
                    windows: vec![first_window],
                    viewers: Vec::new(),
                },
                SessionSnapshot {
                    id: SessionId(2),
                    name: "research".to_owned(),
                    active_window: second_window.id,
                    windows: vec![second_window],
                    viewers: Vec::new(),
                },
            ],
        };

        let model = local_model(&snapshot, Some(SessionId(2)));
        let sessions = &model.host(HostId::LOCAL).unwrap().sessions;
        assert_eq!(sessions[0].windows.len(), 1);
        assert!(!sessions[0].active);
        assert_eq!(sessions[0].windows[0].panes.len(), 1);
        assert!(!sessions[0].windows[0].active);
        assert_eq!(sessions[0].windows[0].index, 0);
        assert_eq!(sessions[1].windows.len(), 1);
        assert!(sessions[1].active);
        assert!(sessions[1].windows[0].active);
        assert_eq!(sessions[1].windows[0].index, 1);
        assert_eq!(
            model.session_for_target(HostId::LOCAL, TreeTarget::Window(WindowId(22))),
            Some(SessionId(2))
        );
        assert_eq!(
            model.session_for_target(HostId::LOCAL, TreeTarget::Pane(PaneId(202))),
            Some(SessionId(2))
        );
    }

    #[test]
    fn tree_ids_are_stable_and_namespaced_by_host_and_node_type() {
        assert_eq!(TreeNode::Host(HostId::LOCAL).tree_id(), "host:HostId(0)");
        assert_eq!(
            TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(9))).tree_id(),
            "host:HostId(0):session:$9"
        );
        assert_eq!(TreeTarget::Session(SessionId(9)).tree_id(), "session:$9");
        assert_eq!(TreeTarget::Window(WindowId(9)).tree_id(), "window:@9");
        assert_eq!(TreeTarget::Pane(PaneId(9)).tree_id(), "pane:%9");
    }

    #[test]
    fn rebuild_restores_expansion_without_reopening_a_collapsed_window() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        let host = TreeNode::Host(HostId::LOCAL);
        let session = TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1)));
        let window = TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11)));
        let pane = TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(202)));
        let expanded = BTreeSet::from([host, session, window]);

        let first = TreeProjection::new(&model, &expanded);
        assert_eq!(first.visible_indices[&pane], 3);

        let collapsed_window = BTreeSet::from([host, session]);
        let rebuilt = TreeProjection::new(&model, &collapsed_window);
        assert!(rebuilt.visible_indices.contains_key(&session));
        assert!(rebuilt.visible_indices.contains_key(&window));
        assert!(!rebuilt.visible_indices.contains_key(&pane));
    }

    #[test]
    fn projection_keeps_parent_identity_and_hidden_expansion_metadata() {
        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        let host = TreeNode::Host(HostId::LOCAL);
        let session = TreeNode::Target(HostId::LOCAL, TreeTarget::Session(SessionId(1)));
        let window = TreeNode::Target(HostId::LOCAL, TreeTarget::Window(WindowId(11)));
        let pane = TreeNode::Target(HostId::LOCAL, TreeTarget::Pane(PaneId(202)));
        let collapsed = TreeProjection::new(&model, &BTreeSet::from([host]));

        assert_eq!(collapsed.entries.len(), 2);
        assert!(collapsed.expandable.contains(&host));
        assert!(collapsed.visible_indices.contains_key(&session));
        assert!(!collapsed.visible_indices.contains_key(&window));
        assert!(collapsed.expandable.contains(&session));
        assert!(collapsed.expandable.contains(&window));
        assert!(!collapsed.expandable.contains(&pane));

        let expanded = TreeProjection::new(&model, &BTreeSet::from([host, session, window]));
        assert_eq!(
            expanded.entries[expanded.visible_indices[&session]].parent,
            Some(host)
        );
        assert_eq!(
            expanded.entries[expanded.visible_indices[&window]].parent,
            Some(session)
        );
        assert_eq!(
            expanded.entries[expanded.visible_indices[&pane]].parent,
            Some(window)
        );
    }

    #[test]
    fn disconnected_tree_disables_activation_without_hiding_targets() {
        let target = TreeTarget::Pane(PaneId(21));
        assert_eq!(
            activation_for_target(
                HostId::LOCAL,
                target,
                Some(SessionId(2)),
                HostId::LOCAL,
                Some(SessionId(1)),
                false,
            ),
            None
        );
        assert_eq!(
            activation_for_target(
                HostId::LOCAL,
                target,
                Some(SessionId(1)),
                HostId::LOCAL,
                Some(SessionId(1)),
                true,
            ),
            Some(SidebarActivation::Execute {
                host: HostId::LOCAL,
                command: select_pane_command(PaneId(21)),
            })
        );
    }

    #[test]
    fn selecting_a_target_in_another_session_attaches_before_selecting() {
        assert_eq!(
            activation_for_target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(21)),
                Some(SessionId(2)),
                HostId::LOCAL,
                Some(SessionId(1)),
                true,
            ),
            Some(SidebarActivation::AttachThenExecute {
                host: HostId::LOCAL,
                session: SessionId(2),
                command: select_pane_command(PaneId(21)),
            })
        );
        assert_eq!(
            activation_for_target(
                HostId::LOCAL,
                TreeTarget::Window(WindowId(13)),
                Some(SessionId(2)),
                HostId::LOCAL,
                None,
                true,
            ),
            Some(SidebarActivation::AttachThenExecute {
                host: HostId::LOCAL,
                session: SessionId(2),
                command: select_window_command(WindowId(13)),
            })
        );
    }

    #[test]
    fn sidebar_commands_use_stable_mux_targets() {
        assert_eq!(
            new_window_command(SessionId(8)),
            CommandInvocation::new("new-window", ["-t", "$8"])
        );
        assert_eq!(
            select_window_command(WindowId(13)),
            CommandInvocation::new("select-window", ["-t", "@13"])
        );
        assert_eq!(
            select_pane_command(PaneId(21)),
            CommandInvocation::new("select-pane", ["-t", "%21"])
        );
        assert_eq!(
            kill_target_command(TreeTarget::Session(SessionId(8))),
            CommandInvocation::new("kill-session", ["-t", "$8"])
        );
        assert_eq!(
            kill_target_command(TreeTarget::Window(WindowId(13))),
            CommandInvocation::new("kill-window", ["-t", "@13"])
        );
        assert_eq!(
            kill_target_command(TreeTarget::Pane(PaneId(21))),
            CommandInvocation::new("kill-pane", ["-t", "%21"])
        );
        assert_eq!(
            split_picker_command(PaneId(21), Axis::Horizontal),
            CommandInvocation::new("split-picker", ["-h", "-t", "%21"])
        );
        assert_eq!(
            split_picker_command(PaneId(21), Axis::Vertical),
            CommandInvocation::new("split-picker", ["-v", "-t", "%21"])
        );
    }

    #[test]
    fn rename_prompts_prefill_exact_names_and_keep_stable_targets() {
        let (session_label, session_command) =
            rename_prompt_command(TreeTarget::Session(SessionId(8)), "main workspace")
                .expect("sessions are renameable");
        assert_eq!(session_label, "Rename Session…");
        assert_eq!(
            session_command,
            CommandInvocation::new(
                "command-prompt",
                [
                    "-p",
                    "rename-session: ",
                    "-I",
                    "main workspace",
                    "rename-session -t '$8' -- '%%'",
                ],
            )
        );

        let (window_label, window_command) =
            rename_prompt_command(TreeTarget::Window(WindowId(13)), "editor pane")
                .expect("windows are renameable");
        assert_eq!(window_label, "Rename Window…");
        assert_eq!(
            window_command,
            CommandInvocation::new(
                "command-prompt",
                [
                    "-p",
                    "rename-window: ",
                    "-I",
                    "editor pane",
                    "rename-window -t '@13' -- '%%'",
                ],
            )
        );
        assert!(rename_prompt_command(TreeTarget::Pane(PaneId(21)), "shell").is_none());

        let snapshot = snapshot_with_two_panes();
        let model = local_model(&snapshot, Some(SessionId(1)));
        assert_eq!(
            model.renameable_name(HostId::LOCAL, TreeTarget::Session(SessionId(1))),
            Some("main workspace")
        );
        assert_eq!(
            model.renameable_name(HostId::LOCAL, TreeTarget::Window(WindowId(11))),
            Some("work")
        );
        assert_eq!(
            model.renameable_name(HostId::LOCAL, TreeTarget::Pane(PaneId(202))),
            None
        );
        assert_eq!(
            model.rename_target_for_node(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Session(SessionId(1))
            )),
            Some((HostId::LOCAL, TreeTarget::Session(SessionId(1))))
        );
        assert_eq!(
            model.rename_target_for_node(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Window(WindowId(11))
            )),
            Some((HostId::LOCAL, TreeTarget::Window(WindowId(11))))
        );
        assert_eq!(
            model.rename_target_for_node(TreeNode::Target(
                HostId::LOCAL,
                TreeTarget::Pane(PaneId(202))
            )),
            Some((HostId::LOCAL, TreeTarget::Window(WindowId(11))))
        );
        assert_eq!(
            model.rename_target_for_node(TreeNode::Host(HostId::LOCAL)),
            None
        );
    }
}
