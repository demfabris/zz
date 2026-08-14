mod model;
mod panel;

use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
    sync::Arc,
};

use adw::prelude::*;
use gtk::{gdk, gio, glib, graphene};
use zz_client::{ChromeAction, ChromeKeymap, SIDEBAR_TABLE, UI_TABLE};
use zz_protocol::{Axis, CommandInvocation};
use zz_terminal::{KeyAction, KeyInput};

use crate::{engine::Engine, ui::keys};

pub use panel::NewSessionPanel;

use model::{
    Activation, HostId, PaneKind, Row, RowKind, Tree, TreeNode, TreeTarget, expand_path_to,
    kill_target_command, new_pane_command, new_window_command,
};

/// The desktop's drag limits, so a window narrow enough to matter cannot be
/// filled by the tree.
const MIN_WIDTH: f64 = 160.0;
const MAX_WIDTH: f64 = 640.0;
const DEFAULT_WIDTH: f64 = 256.0;
/// How far each level of the tree is inset, in pixels.
const INDENT: i32 = 16;

/// What the sidebar hands back up to the window: chrome it cannot answer on its
/// own, and the request to give the keyboard back to the focused pane.
pub struct Hooks {
    pub chrome: Rc<dyn Fn(ChromeAction)>,
    pub focus_pane: Rc<dyn Fn()>,
}

/// The session tree: every session, window and pane the local daemon holds,
/// with the row the mux is on marked and a client-local cursor of its own.
///
/// Nothing here is state — every row is projected from the snapshot the daemon
/// last published, and every interaction is a command back to it.
pub struct Sidebar {
    engine: Arc<Engine>,
    host: String,
    split: adw::OverlaySplitView,
    root: gtk::Box,
    scroller: gtk::ScrolledWindow,
    list: gtk::ListBox,
    menu: gtk::PopoverMenu,
    status_bar: gtk::Box,
    status_left: gtk::Label,
    status_right: gtk::Label,
    tree: RefCell<Tree>,
    rows: RefCell<Vec<Row>>,
    expanded: RefCell<BTreeSet<TreeNode>>,
    selected: Cell<Option<TreeNode>>,
    revealed: Cell<Option<TreeNode>>,
    seeded: Cell<bool>,
    focused: Cell<bool>,
    width: Cell<f64>,
    syncing: Cell<bool>,
    hooks: RefCell<Option<Hooks>>,
}

impl Sidebar {
    pub fn build(engine: Arc<Engine>) -> Rc<Self> {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("navigation-sidebar");

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let (status_bar, status_left, status_right) = build_status();

        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        header.set_title_widget(Some(&adw::WindowTitle::new("Sessions", "")));
        header.pack_end(
            &gtk::Button::builder()
                .icon_name("list-add-symbolic")
                .tooltip_text("New Session")
                .action_name("sidebar.new-session")
                .has_frame(false)
                .build(),
        );

        let toolbar = adw::ToolbarView::new();
        toolbar.set_hexpand(true);
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scroller));
        toolbar.add_bottom_bar(&status_bar);

        let grip = gtk::Box::builder().width_request(4).build();
        grip.add_css_class("zz-sidebar-grip");
        grip.set_cursor_from_name(Some("col-resize"));

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.append(&toolbar);
        root.append(&grip);

        let split = adw::OverlaySplitView::builder()
            .sidebar(&root)
            .sidebar_width_unit(adw::LengthUnit::Px)
            .min_sidebar_width(DEFAULT_WIDTH)
            .max_sidebar_width(DEFAULT_WIDTH)
            .build();

        // The menu hangs off the scroller rather than the list: a popover
        // parented to a `gtk::ListBox` is a child the list cannot remove, and
        // `remove_all` spins on it forever.
        let menu = gtk::PopoverMenu::builder().has_arrow(false).build();
        menu.set_parent(&scroller);

        let sidebar = Rc::new(Self {
            engine,
            host: host_name(),
            split,
            root,
            scroller,
            list,
            menu,
            status_bar,
            status_left,
            status_right,
            tree: RefCell::new(Tree::default()),
            rows: RefCell::new(Vec::new()),
            expanded: RefCell::new(BTreeSet::new()),
            selected: Cell::new(None),
            revealed: Cell::new(None),
            seeded: Cell::new(false),
            focused: Cell::new(false),
            width: Cell::new(DEFAULT_WIDTH),
            syncing: Cell::new(false),
            hooks: RefCell::new(None),
        });
        sidebar.install_actions();
        sidebar.connect_signals(&grip);
        sidebar.sync();
        sidebar.refresh_status();
        sidebar
    }

    /// The split view, for the window to hang its workspace in.
    pub fn widget(&self) -> &gtk::Widget {
        self.split.upcast_ref()
    }

    pub fn set_content(&self, content: &impl IsA<gtk::Widget>) {
        self.split.set_content(Some(content));
    }

    /// A header button that shows and hides the tree, bound to the split view
    /// so it always reads the real state rather than a copy of it.
    pub fn toggle_button(&self) -> gtk::ToggleButton {
        let button = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text("Toggle Sidebar")
            .has_frame(false)
            .build();
        self.split
            .bind_property("show-sidebar", &button, "active")
            .bidirectional()
            .sync_create()
            .build();
        button
    }

    pub fn connect(&self, hooks: Hooks) {
        self.hooks.replace(Some(hooks));
    }

    /// True while the keyboard belongs to the tree. The window asks before it
    /// hands focus back to a pane: sidebar focus and pane focus are exclusive,
    /// and a snapshot arriving must never move the keyboard on its own.
    pub fn has_focus(&self) -> bool {
        self.focused.get()
    }

    /// Show the tree and put the keyboard in it — what the daemon's
    /// `focus-sidebar` and the `-s`/`-w` choosers ask for.
    pub fn focus(&self) {
        self.split.set_show_sidebar(true);
        let active = self.tree.borrow().active;
        if let Some(active) = active {
            expand_path_to(&mut self.expanded.borrow_mut(), &self.tree.borrow(), active);
            self.rebuild_rows();
        }
        if self.selected.get().is_none() {
            self.select(active.or_else(|| self.first_node()));
        }
        self.grab_selected_row();
    }

    pub fn toggle(&self) {
        if !self.split.shows_sidebar() {
            self.focus();
            return;
        }
        if self.focused.get() {
            self.split.set_show_sidebar(false);
            self.blur();
            return;
        }
        self.focus();
    }

    /// Rebuild the tree from the snapshot the core holds. Identical rows are
    /// left alone, so a redraw costs nothing while the daemon's clock ticks.
    pub fn sync(&self) {
        let snapshot = self.engine.snapshot();
        let attached = self.engine.attached_session();
        let tree = Tree::from_snapshot_for(HostId::LOCAL, &self.host, &snapshot, attached);

        {
            let mut expanded = self.expanded.borrow_mut();
            if !self.seeded.replace(true) {
                expanded.insert(TreeNode::Host(HostId::LOCAL));
            }
            if let Some(active) = tree.active
                && self.revealed.get() != Some(active)
            {
                expand_path_to(&mut expanded, &tree, active);
                self.revealed.set(Some(active));
            }
            expanded.retain(|node| tree.is_live(*node));
        }
        self.tree.replace(tree);
        self.rebuild_rows();

        let live = self
            .selected
            .get()
            .filter(|node| self.rows.borrow().iter().any(|row| row.node == *node));
        self.select(live.or(self.tree.borrow().active));
    }

    /// The daemon's status line, stacked under the tree. It carries a clock and
    /// republishes about once a second; an empty line is the daemon's default
    /// and stays hidden rather than showing an empty bar.
    pub fn refresh_status(&self) {
        let status = self.engine.status();
        let left = status.left.trim();
        let right = status.right.trim();
        self.status_bar
            .set_visible(!left.is_empty() || !right.is_empty());
        self.status_left.set_text(left);
        self.status_left.set_visible(!left.is_empty());
        self.status_right.set_text(right);
        self.status_right.set_visible(!right.is_empty());
    }

    fn install_actions(self: &Rc<Self>) {
        let actions = gio::SimpleActionGroup::new();
        for (name, run) in Self::verbs() {
            let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
            let target = Rc::downgrade(self);
            action.connect_activate(move |_, parameter| {
                let Some(sidebar) = target.upgrade() else {
                    return;
                };
                let Some(node) = parameter
                    .and_then(glib::Variant::str)
                    .and_then(TreeNode::parse)
                else {
                    return;
                };
                run(&sidebar, node);
            });
            actions.add_action(&action);
        }

        let action = gio::SimpleAction::new("new-session", None);
        let target = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            if let Some(sidebar) = target.upgrade() {
                sidebar.engine.new_session();
            }
        });
        actions.add_action(&action);

        let action = gio::SimpleAction::new("add-host", None);
        action.connect_activate(|_, _| {
            log::info!("zz-gtk cannot add hosts yet: add a host- line to zz/config");
        });
        actions.add_action(&action);

        self.split.insert_action_group("sidebar", Some(&actions));
    }

    fn verbs() -> [(&'static str, fn(&Rc<Self>, TreeNode)); 5] {
        [
            ("toggle", |sidebar, node| {
                sidebar.select(Some(node));
                sidebar.toggle_node(node);
            }),
            ("rename", |sidebar, node| sidebar.rename(node)),
            ("kill", |sidebar, node| {
                if let TreeNode::Target(_, target) = node {
                    sidebar.engine.execute(kill_target_command(target));
                }
            }),
            ("new-window", |sidebar, node| {
                if let TreeNode::Target(_, TreeTarget::Session(session)) = node {
                    sidebar.engine.execute(new_window_command(session));
                }
            }),
            ("new-pane", |sidebar, node| {
                let pane = sidebar.rows.borrow().iter().find_map(|row| match row.kind {
                    RowKind::Window { active_pane } if row.node == node => Some(active_pane),
                    _ => None,
                });
                if let Some(pane) = pane {
                    sidebar
                        .engine
                        .execute(new_pane_command(pane, Axis::Horizontal));
                }
            }),
        ]
    }

    fn connect_signals(self: &Rc<Self>, grip: &gtk::Box) {
        let target = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(sidebar) = target.upgrade() else {
                return;
            };
            if let Some(node) = sidebar.node_at(row.index()) {
                sidebar.activate(node, false);
            }
        });

        let target = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, row| {
            let Some(sidebar) = target.upgrade() else {
                return;
            };
            if sidebar.syncing.get() {
                return;
            }
            if let Some(node) = row.and_then(|row| sidebar.node_at(row.index())) {
                sidebar.selected.set(Some(node));
            }
        });

        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(sidebar) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if keys::is_modifier(keyval) {
                return glib::Propagation::Proceed;
            }
            let input = keys::key_input(KeyAction::Press, keyval, modifiers, None);
            let Some(action) = resolve_chrome(sidebar.engine.chrome(), &input) else {
                return glib::Propagation::Proceed;
            };
            sidebar.perform(action);
            glib::Propagation::Stop
        });
        self.root.add_controller(keyboard);

        let focus = gtk::EventControllerFocus::new();
        let target = Rc::downgrade(self);
        focus.connect_enter(move |_| {
            if let Some(sidebar) = target.upgrade() {
                sidebar.focused.set(true);
                sidebar.select(sidebar.selected.get());
            }
        });
        let target = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(sidebar) = target.upgrade() {
                sidebar.focused.set(false);
                sidebar.syncing.set(true);
                sidebar.list.select_row(gtk::ListBoxRow::NONE);
                sidebar.syncing.set(false);
            }
        });
        self.root.add_controller(focus);

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gdk::BUTTON_SECONDARY);
        let target = Rc::downgrade(self);
        gesture.connect_pressed(move |_, _, x, y| {
            if let Some(sidebar) = target.upgrade() {
                sidebar.open_menu(x, y);
            }
        });
        self.list.add_controller(gesture);

        let drag = gtk::GestureDrag::new();
        let anchor = Rc::new(Cell::new(DEFAULT_WIDTH));
        let target = Rc::downgrade(self);
        let start = Rc::clone(&anchor);
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(sidebar) = target.upgrade() {
                start.set(sidebar.width.get());
            }
        });
        let target = Rc::downgrade(self);
        drag.connect_drag_update(move |_, offset, _| {
            if let Some(sidebar) = target.upgrade() {
                sidebar.set_width(anchor.get() + offset);
            }
        });
        grip.add_controller(drag);

        let menu = self.menu.clone();
        self.scroller.connect_destroy(move |_| menu.unparent());
    }

    /// Chrome resolved from the `sidebar` table. Anything the tree does not own
    /// goes back to the window, which is where detach and zoom live.
    fn perform(&self, action: ChromeAction) {
        match action {
            ChromeAction::SidebarSelectDown => self.move_selection(1),
            ChromeAction::SidebarSelectUp => self.move_selection(-1),
            ChromeAction::SidebarSelectLeft => self.collapse_or_ascend(),
            ChromeAction::SidebarSelectRight => self.expand_or_descend(),
            ChromeAction::SidebarSelectFirst => self.select_edge(true),
            ChromeAction::SidebarSelectLast => self.select_edge(false),
            ChromeAction::SidebarConfirm => {
                if let Some(node) = self.selected.get() {
                    self.activate(node, true);
                }
            }
            ChromeAction::SidebarCancel => self.blur(),
            ChromeAction::SidebarRename => {
                if let Some(node) = self.selected.get() {
                    self.rename(node);
                }
            }
            ChromeAction::SidebarCommandPalette => self
                .engine
                .execute(CommandInvocation::new("command-prompt", [] as [&str; 0])),
            ChromeAction::ToggleSidebar => self.toggle(),
            other => {
                let hooks = self.hooks.borrow();
                if let Some(hooks) = hooks.as_ref() {
                    (hooks.chrome)(other);
                }
            }
        }
    }

    /// Activating a row is the daemon's business: a session attaches, a window
    /// or pane selects — behind an attach when it belongs to another session,
    /// because the daemon resolves `-t` against the attachment. A row with
    /// nothing to activate (the host) opens instead.
    ///
    /// `release` is what the keyboard asks for: confirming a row hands the
    /// keyboard back to the pane, while clicking one leaves focus where the
    /// pointer left it.
    fn activate(&self, node: TreeNode, release: bool) {
        let activation = self
            .tree
            .borrow()
            .activation_for_node(node, self.engine.attached_session());
        let Some(activation) = activation else {
            self.toggle_node(node);
            return;
        };
        self.perform_activation(activation);
        if release {
            self.blur();
        }
    }

    fn rename(&self, node: TreeNode) {
        let activation = self
            .tree
            .borrow()
            .rename_activation_for_node(node, self.engine.attached_session());
        if let Some(activation) = activation {
            self.perform_activation(activation);
        }
    }

    fn perform_activation(&self, activation: Activation) {
        match activation {
            Activation::Attach(session) => self.engine.attach_session(session),
            Activation::Execute(command) => self.engine.execute(command),
            Activation::AttachThenExecute(session, command) => {
                self.engine.attach_session(session);
                self.engine.execute(command);
            }
        }
    }

    /// Give the keyboard back to the focused pane without hiding the tree.
    fn blur(&self) {
        let hooks = self.hooks.borrow();
        if let Some(hooks) = hooks.as_ref() {
            (hooks.focus_pane)();
        }
    }

    fn toggle_node(&self, node: TreeNode) {
        {
            let mut expanded = self.expanded.borrow_mut();
            if !expanded.remove(&node) {
                expanded.insert(node);
            }
        }
        self.rebuild_rows();
        self.select(self.selected.get());
        if self.focused.get() {
            self.grab_selected_row();
        }
    }

    fn move_selection(&self, delta: isize) {
        let next = {
            let rows = self.rows.borrow();
            if rows.is_empty() {
                return;
            }
            let current = self
                .selected
                .get()
                .and_then(|node| rows.iter().position(|row| row.node == node));
            let index = match current {
                Some(index) => (index as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
                None if delta > 0 => 0,
                None => rows.len() - 1,
            };
            rows[index].node
        };
        self.select(Some(next));
        self.grab_selected_row();
    }

    fn select_edge(&self, first: bool) {
        let node = {
            let rows = self.rows.borrow();
            if first {
                rows.first().map(|row| row.node)
            } else {
                rows.last().map(|row| row.node)
            }
        };
        if node.is_some() {
            self.select(node);
            self.grab_selected_row();
        }
    }

    /// Left closes an open row and otherwise climbs to its parent.
    fn collapse_or_ascend(&self) {
        let Some(node) = self.selected.get() else {
            return;
        };
        if self.expanded.borrow().contains(&node) {
            self.toggle_node(node);
            return;
        }
        let parent = self.parent_of(node);
        if parent.is_some() {
            self.select(parent);
            self.grab_selected_row();
        }
    }

    /// Right opens a closed row and otherwise steps into its first child.
    fn expand_or_descend(&self) {
        let Some(node) = self.selected.get() else {
            return;
        };
        let expandable = self
            .rows
            .borrow()
            .iter()
            .find(|row| row.node == node)
            .is_some_and(|row| row.expandable);
        if !expandable {
            return;
        }
        if self.expanded.borrow().contains(&node) {
            self.move_selection(1);
        } else {
            self.toggle_node(node);
        }
    }

    fn parent_of(&self, node: TreeNode) -> Option<TreeNode> {
        let rows = self.rows.borrow();
        let index = rows.iter().position(|row| row.node == node)?;
        let depth = rows[index].depth;
        rows[..index]
            .iter()
            .rev()
            .find(|row| row.depth < depth)
            .map(|row| row.node)
    }

    fn first_node(&self) -> Option<TreeNode> {
        self.rows.borrow().first().map(|row| row.node)
    }

    fn node_at(&self, index: i32) -> Option<TreeNode> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.rows.borrow().get(index).map(|row| row.node))
    }

    fn index_of(&self, node: TreeNode) -> Option<i32> {
        self.rows
            .borrow()
            .iter()
            .position(|row| row.node == node)
            .and_then(|index| i32::try_from(index).ok())
    }

    /// The keyboard cursor. GTK paints a selected row, and the desktop only
    /// shows its cursor while the tree has focus, so a blurred sidebar carries
    /// the selection without drawing it.
    fn select(&self, node: Option<TreeNode>) {
        self.selected.set(node);
        let row = node
            .filter(|_| self.focused.get())
            .and_then(|node| self.index_of(node))
            .and_then(|index| self.list.row_at_index(index));
        self.syncing.set(true);
        match row {
            Some(row) => self.list.select_row(Some(&row)),
            None => self.list.select_row(gtk::ListBoxRow::NONE),
        }
        self.syncing.set(false);
    }

    fn grab_selected_row(&self) {
        if let Some(row) = self
            .selected
            .get()
            .and_then(|node| self.index_of(node))
            .and_then(|index| self.list.row_at_index(index))
        {
            row.grab_focus();
        }
    }

    fn rebuild_rows(&self) {
        let rows = self.tree.borrow().rows(&self.expanded.borrow());
        if *self.rows.borrow() == rows {
            return;
        }
        self.list.remove_all();
        for row in &rows {
            self.list.append(&build_row(row));
        }
        self.rows.replace(rows);
    }

    fn open_menu(&self, x: f64, y: f64) {
        let Some(row) = self.list.row_at_y(y as i32) else {
            return;
        };
        let Some(node) = self.node_at(row.index()) else {
            return;
        };
        self.select(Some(node));
        let menu = row_menu(node, &self.rows.borrow());
        self.menu.set_menu_model(Some(&menu));
        let point = self
            .list
            .compute_point(&self.scroller, &graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| graphene::Point::new(x as f32, y as f32));
        self.menu.set_pointing_to(Some(&gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            1,
            1,
        )));
        self.menu.popup();
    }

    fn set_width(&self, width: f64) {
        let width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        if width == self.width.get() {
            return;
        }
        self.width.set(width);
        self.split.set_min_sidebar_width(width);
        self.split.set_max_sidebar_width(width);
    }
}

fn resolve_chrome(chrome: &ChromeKeymap, input: &KeyInput) -> Option<ChromeAction> {
    chrome
        .resolve(UI_TABLE, input)
        .or_else(|| chrome.resolve(SIDEBAR_TABLE, input))
}

fn build_status() -> (gtk::Box, gtk::Label, gtk::Label) {
    let left = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let right = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    right.add_css_class("dim-label");
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .build();
    bar.add_css_class("toolbar");
    bar.add_css_class("zz-status");
    bar.append(&left);
    bar.append(&right);
    (bar, left, right)
}

/// One row is a widget tree rather than an `adw::ActionRow` so the disclosure,
/// the kind marker and the hover gutter sit where the desktop puts them.
fn build_row(row: &Row) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.set_margin_start(i32::from(row.depth) * INDENT);
    content.add_css_class("zz-sidebar-row");

    let target = row.node.to_string().to_variant();
    if row.expandable {
        let disclosure = gtk::Button::builder()
            .icon_name(if row.expanded {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            })
            .has_frame(false)
            .action_name("sidebar.toggle")
            .action_target(&target)
            .build();
        disclosure.add_css_class("flat");
        disclosure.add_css_class("zz-sidebar-disclosure");
        content.append(&disclosure);
    } else {
        content.append(&gtk::Box::builder().width_request(16).build());
    }

    content.append(&gtk::Image::from_icon_name(row_icon(row.kind)));

    let label = gtk::Label::builder()
        .label(&row.label)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    if !row.on_active_path {
        label.add_css_class("dim-label");
    }
    content.append(&label);

    if row.bell {
        let bell = gtk::Label::new(Some("●"));
        bell.add_css_class("zz-bell");
        bell.set_tooltip_text(Some("A bell rang here"));
        content.append(&bell);
    }

    let gutter = build_gutter(row, &target);
    content.append(&gutter);

    let list_row = gtk::ListBoxRow::builder().child(&content).build();
    if row.active {
        list_row.add_css_class("zz-sidebar-active");
    }
    if !matches!(row.kind, RowKind::Host) {
        let motion = gtk::EventControllerMotion::new();
        let shown = gutter.clone();
        motion.connect_enter(move |_, _, _| shown.set_visible(true));
        let hidden = gutter;
        motion.connect_leave(move |_| hidden.set_visible(false));
        list_row.add_controller(motion);
    }
    list_row
}

/// The hover gutter: what a row can do without a menu. The host keeps its menu
/// button visible; everything else appears under the pointer.
fn build_gutter(row: &Row, target: &glib::Variant) -> gtk::Box {
    let gutter = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    gutter.set_valign(gtk::Align::Center);

    match row.kind {
        RowKind::Host => {
            gutter.append(
                &gtk::MenuButton::builder()
                    .icon_name("view-more-symbolic")
                    .tooltip_text("Host actions")
                    .menu_model(&host_menu())
                    .has_frame(false)
                    .build(),
            );
            return gutter;
        }
        RowKind::Session => gutter.append(&gutter_button(
            "tab-new-symbolic",
            "New window",
            "sidebar.new-window",
            target,
        )),
        RowKind::Window { .. } => gutter.append(&gutter_button(
            "list-add-symbolic",
            "Add pane",
            "sidebar.new-pane",
            target,
        )),
        RowKind::Pane(_) => {}
    }
    gutter.append(&gutter_button(
        "user-trash-symbolic",
        delete_label(row.kind),
        "sidebar.kill",
        target,
    ));
    gutter.set_visible(false);
    gutter
}

fn gutter_button(icon: &str, tooltip: &str, action: &str, target: &glib::Variant) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .has_frame(false)
        .action_name(action)
        .action_target(target)
        .build();
    button.add_css_class("flat");
    button.add_css_class("zz-sidebar-action");
    button
}

fn host_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("New session"), Some("sidebar.new-session"));
    menu.append(Some("Add host…"), Some("sidebar.add-host"));
    menu
}

/// The right-click menu. Rename is the daemon's value prompt — the client never
/// edits a name itself, it asks for the prompt the overlay then renders.
fn row_menu(node: TreeNode, rows: &[Row]) -> gio::Menu {
    let TreeNode::Target(_, target) = node else {
        return host_menu();
    };
    let Some(kind) = rows.iter().find(|row| row.node == node).map(|row| row.kind) else {
        return gio::Menu::new();
    };
    let menu = gio::Menu::new();
    let value = node.to_string().to_variant();
    menu.append_item(&item(
        match target {
            TreeTarget::Session(_) => "Rename Session…",
            _ => "Rename Window…",
        },
        "sidebar.rename",
        &value,
    ));
    match kind {
        RowKind::Session => menu.append_item(&item("New window", "sidebar.new-window", &value)),
        RowKind::Window { .. } => menu.append_item(&item("Add pane", "sidebar.new-pane", &value)),
        _ => {}
    }
    menu.append_item(&item(delete_label(kind), "sidebar.kill", &value));
    menu
}

/// `GAction` targets carry the row id, so a menu item is bound to its row
/// rather than to whatever happens to be selected when it fires.
fn item(label: &str, action: &str, target: &glib::Variant) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    item.set_action_and_target_value(Some(action), Some(target));
    item
}

const fn delete_label(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Session => "Delete session",
        RowKind::Window { .. } => "Delete window",
        _ => "Delete pane",
    }
}

const fn row_icon(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Host => "computer-symbolic",
        RowKind::Session => "view-grid-symbolic",
        RowKind::Window { .. } => "view-paged-symbolic",
        RowKind::Pane(PaneKind::Picker) => "list-add-symbolic",
        RowKind::Pane(PaneKind::Terminal) => "utilities-terminal-symbolic",
        RowKind::Pane(PaneKind::Browser) => "web-browser-symbolic",
        RowKind::Pane(PaneKind::Agent) => "system-run-symbolic",
        RowKind::Pane(PaneKind::Editor) => "text-editor-symbolic",
    }
}

fn host_name() -> String {
    let name = glib::host_name().to_string();
    if name.trim().is_empty() {
        "localhost".to_owned()
    } else {
        name
    }
}
