#[path = "agent_pane.rs"]
mod agent_pane;
#[cfg(target_os = "ios")]
#[path = "authentication.rs"]
mod authentication;
#[path = "floating.rs"]
mod floating;
#[path = "picker.rs"]
mod picker;
#[path = "settings.rs"]
mod settings;
#[path = "sidebar.rs"]
mod sidebar;
#[path = "status_bar.rs"]
mod status_bar;

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    rc::Rc,
    sync::Arc,
};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, Context, Corners, DragMoveEvent, Entity,
    FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, Pixels, Point, Render,
    ScrollStrategy, Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use zz_client::{
    ChromeAction, ChromeKeymap, ChromeProfile, DropZone, PaneRect, UI_TABLE, coerced_drop_zone,
    drop_preview_bounds, drop_zone_at, pane_drop_command, pane_rects, predicted_drop_layout,
};
use zz_protocol::{
    ChooseBufferAction, ChooseBufferItem, ChooseTreeAction, ChooseTreeKind, ChooseTreePaneKind,
    ConfirmAction, DisplayPanesAction, InputMessage, PaneId, PaneKindSnapshot, ProtocolMessage,
    WindowSnapshot,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, ElementExt as _, Icon, IconName, Root,
    Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    chooser::{
        ChooserDimensions, ChooserHint, ChooserModal, ChooserPaneKind, ChooserRowTheme,
        ChooserSearch, buffer_chooser_row, chooser_has_key_gutter, chooser_subtitle,
        tree_chooser_row,
    },
    navigation::{
        workspace_chrome_controls, workspace_layout_button, workspace_settings_button,
        workspace_sidebar_surface, workspace_sidebar_titlebar,
    },
    pane::{
        DropPreview, DropPreviewFrame, PaneChrome, PaneDrag, PaneDragOverlayState, PaneSplitAxis,
        PaneSplitHighlight, PaneSplitSide, TerminalPaneAction, pane_border_color, pane_drag_button,
        pane_drag_overlay, pane_drag_preview, pane_drop_preview, pane_split_hit_target,
        pane_split_surface, pane_surface, terminal_pane_header,
    },
    settings::{SettingsSection, settings_navigation_button, settings_navigation_group_label},
    shell::{app_shell_surface, app_workspace_surface},
};

use crate::{
    command_palette::{CommandPaletteEvent, CommandPaletteView, palette_backend},
    connection::Connection,
    terminal::TerminalPane,
};
use agent_pane::AgentPane;

const TREE_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["left", "right"],
        label: "expand",
    },
    ChooserHint {
        keys: &["enter"],
        label: "choose",
    },
    ChooserHint {
        keys: &["/"],
        label: "search",
    },
    ChooserHint {
        keys: &["escape"],
        label: "close",
    },
];

const BUFFER_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["enter"],
        label: "paste",
    },
    ChooserHint {
        keys: &["d"],
        label: "delete",
    },
    ChooserHint {
        keys: &["/"],
        label: "search",
    },
    ChooserHint {
        keys: &["escape"],
        label: "close",
    },
];

#[cfg(target_os = "ios")]
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = zz, no_json)]
pub(crate) struct OpenSession {
    pub name: String,
}

#[cfg(target_os = "ios")]
pub(crate) fn session_from_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let name = rest.strip_prefix("attach/")?.trim_end_matches('/');
    let name = percent_encoding::percent_decode_str(name)
        .decode_utf8()
        .ok()?;
    (!name.is_empty()).then(|| name.into_owned())
}

pub(crate) struct AppShell {
    connection: Entity<Connection>,
    connection_status: String,
    connected: bool,
    idle_guard: Option<gpui::Task<()>>,
    #[cfg(target_os = "ios")]
    auth_prompt_id: Option<u64>,
    terminals: HashMap<PaneId, Entity<TerminalPane>>,
    waiting_panes: BTreeSet<PaneId>,
    agents: HashMap<PaneId, Entity<AgentPane>>,
    pickers: HashMap<PaneId, Entity<picker::PanePicker>>,
    sidebar: bool,
    slideover: bool,
    settings: Option<SettingsSection>,
    preferences: settings::Preferences,
    settings_controls: settings::Controls,
    pub(crate) focus: FocusHandle,
    sidebar_focus: FocusHandle,
    sidebar_scroll: UniformListScrollHandle,
    collapsed_tree: BTreeSet<sidebar::Target>,
    sidebar_selection: Option<sidebar::Target>,
    sidebar_pointer_selection: bool,
    sidebar_active: Option<sidebar::Target>,
    unseen_agents: BTreeSet<PaneId>,
    chrome: ChromeKeymap,
    prompt: Option<Entity<CommandPaletteView>>,
    prompt_revision: u64,
    chooser_revision: u64,
    local_palette_revisions: Option<(u64, u64)>,
    modal_open: bool,
    chooser_scroll: UniformListScrollHandle,
    chooser_selection: Option<(bool, u32)>,
    menu_selection: Option<usize>,
    focused_pane: Option<PaneId>,
    popup_terminal: Option<(PaneId, Entity<TerminalPane>)>,
    output_terminal: Option<(u64, Entity<TerminalPane>)>,
    split_drag: Option<SplitDragState>,
    terminal_resize_suppressed: Rc<Cell<bool>>,
    pane_drag: Option<PaneDragState>,
    pane_layout_override: Option<PaneLayoutOverride>,
    pane_canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    pane_bounds: Rc<RefCell<HashMap<PaneId, Bounds<Pixels>>>>,
    rendered_drop_preview: Rc<Cell<DropPreviewFrame>>,
    _subscriptions: Vec<Subscription>,
}

impl AppShell {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let connection = cx.new(Connection::new);
        let window_handle = window.window_handle();
        let key_listener = cx.listener(move |this, event: &gpui::KeystrokeEvent, window, cx| {
            if window.window_handle() == window_handle {
                this.key_down(
                    &KeyDownEvent {
                        keystroke: event.keystroke.clone(),
                        is_held: event.is_held,
                        prefer_character_input: false,
                    },
                    window,
                    cx,
                );
            }
        });
        let key_events = cx.intercept_keystrokes(key_listener);
        let observer = cx.observe(&connection, |this, connection, cx| {
            let connection = connection.read(cx);
            if this.connected != connection.connected || this.connection_status != connection.status
            {
                this.connected = connection.connected;
                this.connection_status.clone_from(&connection.status);
                this.sync_idle_guard(cx);
                cx.notify();
            }
        });
        let events = cx.subscribe_in(
            &connection,
            window,
            |this, connection, event: &zz_client::CoreEvent, window, cx| {
                match event {
                    zz_client::CoreEvent::ViewportChanged { pane, .. } => {
                        if this.waiting_panes.remove(pane) {
                            cx.notify();
                        }
                        return;
                    }
                    zz_client::CoreEvent::StatusChanged
                    | zz_client::CoreEvent::AgentUpdates { .. } => return,
                    zz_client::CoreEvent::AgentStateChanged { pane, attention } => {
                        if *attention == Some(zz_client::AgentAttentionEdge::Done)
                            && (!window.is_window_active() || this.focused_pane != Some(*pane))
                        {
                            this.unseen_agents.insert(*pane);
                        }
                    }
                    zz_client::CoreEvent::HelloReceived | zz_client::CoreEvent::Attached { .. } => {
                        if matches!(event, zz_client::CoreEvent::Attached { .. }) {
                            connection.update(cx, Connection::set_color_scheme);
                        }
                        if matches!(event, zz_client::CoreEvent::Attached { .. }) {
                            this.prompt_revision = this.prompt_revision.wrapping_add(1).max(1);
                            this.chooser_revision = this.chooser_revision.wrapping_add(1).max(1);
                        }
                        this.focused_pane = None;
                        this.waiting_panes.clear();
                        this.popup_terminal = None;
                        this.output_terminal = None;
                        this.set_split_drag(None);
                        this.pane_drag = None;
                        this.pane_layout_override = None;
                    }
                    zz_client::CoreEvent::SnapshotChanged => {
                        if this.split_drag.is_some_and(|state| {
                            state.committed_generation.is_some_and(|generation| {
                                generation < connection.read(cx).core.snapshot().generation
                            })
                        }) {
                            this.set_split_drag(None);
                        }
                    }
                    zz_client::CoreEvent::ClientMessage {
                        kind,
                        text,
                        duration_ms,
                        message_id,
                        ..
                    } => {
                        use zz_ui::{WindowExt as _, notification::Notification};
                        let notification = match kind {
                            zz_protocol::ClientMessageKind::Info => {
                                Notification::info(text.clone())
                            }
                            zz_protocol::ClientMessageKind::Success => {
                                Notification::success(text.clone())
                            }
                            zz_protocol::ClientMessageKind::Warning => {
                                Notification::warning(text.clone())
                            }
                            zz_protocol::ClientMessageKind::Error => {
                                Notification::error(text.clone())
                            }
                        };
                        let notification = match duration_ms {
                            Some(0) => notification.autohide(false),
                            Some(duration) => notification.autohide_after(
                                std::time::Duration::from_millis(u64::from(*duration)),
                            ),
                            None => notification,
                        };
                        let notification = if let Some(id) = message_id {
                            notification.key(format!("web-message-{id}"))
                        } else {
                            notification
                        };
                        window.push_notification(notification, cx);
                    }
                    zz_client::CoreEvent::ClientMessageCleared { message_id } => {
                        use zz_ui::WindowExt as _;
                        window.dismiss_notification(&format!("web-message-{message_id}"), cx);
                    }
                    zz_client::CoreEvent::CommandResponse(
                        zz_protocol::CommandResponse::Error { .. },
                    ) => {
                        this.set_split_drag(None);
                        this.pane_layout_override = None;
                    }
                    zz_client::CoreEvent::FocusSidebar => this.focus_sidebar(window, cx),
                    zz_client::CoreEvent::CommandPromptChanged => {
                        this.prompt_revision = this.prompt_revision.wrapping_add(1).max(1);
                    }
                    zz_client::CoreEvent::ChooseTreeChanged => {
                        this.chooser_revision = this.chooser_revision.wrapping_add(1).max(1);
                    }
                    zz_client::CoreEvent::MenuChanged => {
                        this.menu_selection = connection
                            .read(cx)
                            .core
                            .menu()
                            .and_then(|menu| menu.selected)
                            .map(|index| index as usize);
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
        let preferences = settings::Preferences::load();
        let appearance_view = cx.weak_entity();
        let appearance_observer = window.observe_window_appearance(move |window, cx| {
            let _ = appearance_view.update(cx, |this, cx| {
                this.preferences.apply(&this.connection, window, cx);
            });
        });
        let activation_observer = cx.observe_window_activation(window, |this, window, cx| {
            this.connection.update(cx, |connection, cx| {
                connection.set_focused(window.is_window_active(), cx);
            });
        });
        let settings_controls = settings::Controls::new(&preferences, window, cx);
        let this = Self {
            connection,
            connection_status: String::new(),
            connected: false,
            idle_guard: None,
            #[cfg(target_os = "ios")]
            auth_prompt_id: None,
            terminals: HashMap::new(),
            waiting_panes: BTreeSet::new(),
            agents: HashMap::new(),
            pickers: HashMap::new(),
            sidebar: preferences.sidebar,
            slideover: false,
            settings: None,
            preferences,
            settings_controls,
            focus: cx.focus_handle(),
            sidebar_focus: cx.focus_handle(),
            sidebar_scroll: UniformListScrollHandle::new(),
            collapsed_tree: BTreeSet::new(),
            sidebar_selection: None,
            sidebar_pointer_selection: false,
            sidebar_active: None,
            unseen_agents: BTreeSet::new(),
            chrome: ChromeKeymap::for_profile(if cfg!(target_os = "ios") {
                ChromeProfile::DesktopApple
            } else {
                ChromeProfile::Desktop
            }),
            prompt: None,
            prompt_revision: 1,
            chooser_revision: 1,
            local_palette_revisions: None,
            modal_open: false,
            chooser_scroll: UniformListScrollHandle::new(),
            chooser_selection: None,
            menu_selection: None,
            focused_pane: None,
            popup_terminal: None,
            output_terminal: None,
            split_drag: None,
            terminal_resize_suppressed: Rc::new(Cell::new(false)),
            pane_drag: None,
            pane_layout_override: None,
            pane_canvas_bounds: Rc::default(),
            pane_bounds: Rc::default(),
            rendered_drop_preview: Rc::default(),
            _subscriptions: vec![
                key_events,
                observer,
                events,
                appearance_observer,
                activation_observer,
            ],
        };
        this.connection.update(cx, Connection::start);
        this.preferences.apply(&this.connection, window, cx);
        #[cfg(target_os = "ios")]
        cx.set_menus(this.menus());
        this
    }

    pub(super) fn sync_idle_guard(&mut self, cx: &mut Context<Self>) {
        let keep_awake = self.preferences.keep_screen_awake && self.connected;
        if keep_awake == self.idle_guard.is_some() {
            return;
        }
        self.idle_guard = keep_awake.then(|| {
            let guard = cx.prevent_idle_sleep("zz connection");
            cx.spawn(async move |_, _| {
                let _guard = guard.await;
                std::future::pending::<()>().await;
            })
        });
    }

    fn send_input(&self, input: InputMessage, cx: &mut App) {
        self.connection.update(cx, |connection, cx| {
            connection.send(ProtocolMessage::Input(input), cx);
        });
    }

    fn command(&self, command: &str, args: Vec<String>, cx: &mut App) {
        self.connection
            .update(cx, |connection, cx| connection.command(command, args, cx));
    }

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if Self::narrow(window) {
            if self.slideover {
                self.release_sidebar_focus(window, cx);
            } else {
                self.focus_sidebar(window, cx);
            }
            return;
        }
        self.sidebar = !self.sidebar;
        self.slideover = false;
        self.focused_pane = None;
        self.preferences.sidebar = self.sidebar;
        self.preferences.save();
        cx.notify();
    }

    fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slideover = !self.inline_sidebar(window);
        self.settings = None;
        self.focused_pane = self.active_window(cx).map(|window| window.active_pane);
        self.sidebar_pointer_selection = false;
        self.sidebar_selection = self.focused_pane.map(sidebar::Target::Pane);
        self.sidebar_active = None;
        self.sidebar_focus.focus(window, cx);
        sidebar::reconcile(self, window, cx);
        cx.notify();
    }

    fn release_sidebar_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slideover = false;
        self.focused_pane = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn slideover(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let sidebar = self.sidebar(window, cx);
        div()
            .id("web-slideover-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .bg(cx.theme().scrim.opacity(0.25))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.release_sidebar_focus(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_none()
                    .bg(cx.theme().background.opaque())
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(sidebar),
            )
            .into_any_element()
    }

    fn active_window(&self, cx: &App) -> Option<WindowSnapshot> {
        let core = &self.connection.read(cx).core;
        let snapshot = core.snapshot();
        let session = snapshot
            .sessions
            .iter()
            .find(|session| Some(session.id) == core.attached_session())?;
        session
            .windows
            .iter()
            .find(|window| window.id == snapshot.focused_window_for(session))
            .cloned()
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = window
            .root::<Root>()
            .flatten()
            .is_some_and(|root| root.read(cx).has_active_dialog());
        let pending = self.connection.update(cx, |connection, cx| {
            connection.reconcile_dialog_prefix(dialog, cx)
        });
        if dialog {
            return;
        }
        if pending && !event.keystroke.modifiers.platform && !event.keystroke.modifiers.function {
            cx.stop_propagation();
            return;
        }
        if self.settings.is_some() && event.keystroke.key == "escape" {
            self.settings = None;
            self.focused_pane = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let input = crate::terminal::key_input(event);
        let core = &self.connection.read(cx).core;
        if self.prompt.as_ref().is_some_and(|palette| {
            let palette = palette.read(cx);
            palette.is_local() || palette.is_window_chooser()
        }) {
            return;
        }
        let overlay = if core.choose_tree().is_some() {
            Some(InputMessage::ChooseTree {
                action: ChooseTreeAction::Key(input.clone()),
            })
        } else if core.choose_buffer().is_some() {
            Some(InputMessage::ChooseBuffer {
                action: ChooseBufferAction::Key(input.clone()),
            })
        } else if core.display_panes().is_some() {
            Some(InputMessage::DisplayPanes {
                action: DisplayPanesAction::Key(input.clone()),
            })
        } else {
            None
        };
        if let Some(overlay) = overlay {
            self.send_input(overlay, cx);
            cx.stop_propagation();
            return;
        }
        if core.command_prompt().is_some() {
            return;
        }
        if let Some(menu) = core.menu() {
            let selected = self.menu_selection;
            match zz_client::resolve_menu_key(menu, selected, &input) {
                zz_client::MenuKeyResult::Action(action) => {
                    self.send_input(InputMessage::Menu { action }, cx);
                }
                zz_client::MenuKeyResult::Select(index) => {
                    self.menu_selection = index;
                    cx.notify();
                }
                zz_client::MenuKeyResult::Consumed => {}
            }
            cx.stop_propagation();
            return;
        }
        if let Some(confirm) = core.confirm() {
            let reply = zz_ui::command::floating::confirm_accepts(
                confirm.confirm_key,
                confirm.default_yes,
                &event.keystroke,
            );
            self.send_input(
                InputMessage::Confirm {
                    action: ConfirmAction::Reply(reply),
                },
                cx,
            );
            cx.stop_propagation();
            return;
        }
        if core.popup().is_some() || core.command_output().is_some() {
            return;
        }
        if self.settings.is_none()
            && self.connection.read(cx).connected
            && !event.keystroke.modifiers.platform
            && !event.keystroke.modifiers.function
            && claims_daemon_prefix(core.mux_options(), core.prefix_armed(), &input)
        {
            if !event.is_held
                && let Some(pane) = self.active_window(cx).map(|window| window.active_pane)
            {
                self.send_input(
                    InputMessage::Key {
                        pane,
                        input,
                        text_follows: false,
                    },
                    cx,
                );
            }
            cx.stop_propagation();
            return;
        }
        if self.sidebar_focus.is_focused(window)
            && self.settings.is_none()
            && let Some(action) = self.chrome.resolve(zz_client::SIDEBAR_TABLE, &input)
            && sidebar::handle_key(self, action, window, cx)
        {
            cx.stop_propagation();
            return;
        }
        let action = self.chrome.resolve(UI_TABLE, &input).or_else(|| {
            ChromeKeymap::for_profile(ChromeProfile::DesktopApple).resolve(UI_TABLE, &input)
        });
        match action {
            Some(
                action @ (ChromeAction::OpenCommandPalette
                | ChromeAction::OpenSettings
                | ChromeAction::ToggleSidebar
                | ChromeAction::UiZoomIn
                | ChromeAction::UiZoomOut
                | ChromeAction::UiZoomReset
                | ChromeAction::ClosePane),
            ) => self.chrome_command(action, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn chrome_command(
        &mut self,
        action: ChromeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ChromeAction::OpenCommandPalette => {
                self.open_palette(None, window, cx);
            }
            ChromeAction::OpenSettings => {
                self.settings = Some(SettingsSection::Appearance);
                cx.notify();
            }
            ChromeAction::ToggleSidebar => {
                if self.slideover {
                    self.release_sidebar_focus(window, cx);
                } else if !self.sidebar {
                    self.focus_sidebar(window, cx);
                } else {
                    self.toggle_sidebar(window, cx);
                }
            }
            ChromeAction::UiZoomIn => {
                self.preferences
                    .change_zoom(0.1, &self.connection, window, cx);
                self.sync_zoom_input(window, cx);
            }
            ChromeAction::UiZoomOut => {
                self.preferences
                    .change_zoom(-0.1, &self.connection, window, cx);
                self.sync_zoom_input(window, cx);
            }
            ChromeAction::UiZoomReset => {
                self.preferences.reset_zoom(&self.connection, window, cx);
                self.sync_zoom_input(window, cx);
            }
            ChromeAction::ClosePane => {
                if self.settings.take().is_none()
                    && self.connection.read(cx).connected
                    && !self.connection.read(cx).core.attached_read_only()
                {
                    self.command("kill-pane", Vec::new(), cx);
                }
                cx.notify();
            }
            ChromeAction::NewSession => self.tmux_command("new-session", cx),
            ChromeAction::NewWindow => self.tmux_command("new-window", cx),
            ChromeAction::SplitRight => self.tmux_command("split-window -h", cx),
            ChromeAction::SplitDown => self.tmux_command("split-window -v", cx),
            _ => {}
        }
    }

    fn tmux_command(&mut self, command: &str, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        if !connection.connected || connection.core.attached_read_only() {
            return;
        }
        let mut words = command.split_whitespace();
        if let Some(name) = words.next() {
            self.command(name, words.map(str::to_owned).collect(), cx);
        }
        cx.notify();
    }

    #[cfg(target_os = "ios")]
    fn menus(&self) -> Vec<gpui::Menu> {
        let bindings = self.chrome.bindings();
        let chrome = |title: &'static str, action: ChromeAction| {
            gpui::MenuItem::action(
                title,
                zz_gpui_ios::MenuCommand {
                    id: action.name().into(),
                    shortcut: bindings
                        .iter()
                        .find(|(table, _, bound)| table == UI_TABLE && *bound == action)
                        .map(|(_, key, _)| key.clone().into()),
                },
            )
        };
        let tmux = |title: &'static str, command: &'static str| {
            gpui::MenuItem::action(
                title,
                zz_gpui_ios::MenuCommand {
                    id: format!("tmux:{command}").into(),
                    shortcut: None,
                },
            )
        };
        vec![
            gpui::Menu::new("zz").items([chrome("Settings…", ChromeAction::OpenSettings)]),
            gpui::Menu::new("File").items([
                chrome("New Session", ChromeAction::NewSession),
                chrome("New Window", ChromeAction::NewWindow),
                chrome("Split Right", ChromeAction::SplitRight),
                chrome("Split Down", ChromeAction::SplitDown),
                gpui::MenuItem::separator(),
                chrome("Close Pane", ChromeAction::ClosePane),
                tmux("Kill Window", "kill-window"),
            ]),
            gpui::Menu::new("View").items([
                chrome("Command Palette…", ChromeAction::OpenCommandPalette),
                tmux("Choose Window…", "choose-tree -w"),
                chrome("Toggle Sidebar", ChromeAction::ToggleSidebar),
                tmux("Zoom Pane", "resize-pane -Z"),
                gpui::MenuItem::separator(),
                chrome("Zoom In", ChromeAction::UiZoomIn),
                chrome("Zoom Out", ChromeAction::UiZoomOut),
                chrome("Reset Zoom", ChromeAction::UiZoomReset),
            ]),
        ]
    }

    #[cfg(target_os = "ios")]
    fn menu_command(
        &mut self,
        command: &zz_gpui_ios::MenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = command.id.strip_prefix("tmux:") {
            self.tmux_command(command, cx);
        } else if let Some(action) = ChromeAction::from_name(&command.id) {
            self.chrome_command(action, window, cx);
        }
    }

    pub(super) fn open_palette(
        &mut self,
        mode: Option<crate::command_palette::PaletteMode>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let core = &self.connection.read(cx).core;
        if core.popup().is_some() || core.menu().is_some() || core.confirm().is_some() {
            return;
        }
        let close_prompt = core.command_prompt().is_some();
        let close_tree = core.choose_tree().is_some();
        if close_prompt {
            self.send_input(
                InputMessage::CommandPrompt {
                    action: zz_protocol::CommandPromptAction::Close,
                },
                cx,
            );
        }
        if close_tree {
            self.send_input(
                InputMessage::ChooseTree {
                    action: ChooseTreeAction::Close,
                },
                cx,
            );
        }
        let backend = self.palette_backend();
        let palette = cx.new(|cx| CommandPaletteView::new_unified(backend, mode, window, cx));
        self.observe_palette(&palette, window, cx);
        palette.read(cx).focus(cx).focus(window, cx);
        self.prompt = Some(palette);
        self.local_palette_revisions = Some((self.prompt_revision, self.chooser_revision));
        self.settings = None;
        self.slideover = false;
        self.modal_open = true;
        cx.notify();
    }

    fn palette_backend(&self) -> Rc<dyn zz_ui::command::PaletteBackend> {
        palette_backend(
            self.connection.clone(),
            self.preferences.palette_settings(),
            self.preferences.agent_enabled,
        )
    }

    fn observe_palette(
        &self,
        palette: &Entity<CommandPaletteView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe(palette, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            palette,
            window,
            |this, palette, event: &CommandPaletteEvent, window, cx| match event {
                CommandPaletteEvent::ReturnToDefault => {
                    this.local_palette_revisions =
                        Some((this.prompt_revision, this.chooser_revision));
                    palette.update(cx, |palette, cx| palette.return_to_default(window, cx));
                    cx.notify();
                }
            },
        )
        .detach();
    }

    fn controls(&self, cx: &Context<Self>) -> AnyElement {
        let settings = workspace_settings_button("web-settings")
            .selected(self.settings.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings = if this.settings.is_some() {
                    None
                } else {
                    Some(SettingsSection::Appearance)
                };
                this.focused_pane = None;
                cx.notify();
            }));
        let layout = workspace_layout_button("web-sidebar")
            .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)));
        workspace_chrome_controls(settings, Some(layout.into_any_element()))
            .when(cfg!(target_os = "ios"), |controls| {
                controls.child(
                    zz_ui::navigation::workspace_palette_button("web-palette").on_click(
                        cx.listener(|this, _, window, cx| this.open_palette(None, window, cx)),
                    ),
                )
            })
            .into_any_element()
    }

    pub(super) fn controls_width(window: &Window) -> Pixels {
        zz_ui::navigation::workspace_chrome_controls_width_for(
            if cfg!(target_os = "ios") { 3 } else { 2 },
            window,
        )
    }

    pub(super) fn narrow(window: &Window) -> bool {
        window.fully_visible_bounds().size.width < px(640.0)
    }

    pub(super) fn inline_sidebar(&self, window: &Window) -> bool {
        !Self::narrow(window) && (self.sidebar || self.settings.is_some())
    }

    pub(super) fn sidebar_width(&self, window: &Window) -> f32 {
        let width = f32::from(window.fully_visible_bounds().size.width);
        if Self::narrow(window) {
            zz_ui::navigation::WORKSPACE_SIDEBAR_DEFAULT_WIDTH.min(width - 40.0)
        } else {
            self.preferences.sidebar_width(width)
        }
    }

    fn sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        sidebar::reconcile(self, window, cx);
        let navigation = if let Some(selected) = self.settings {
            let mut rows = Vec::new();
            let mut previous_group = None;
            for section in settings::SECTIONS {
                let group = section.navigation_group();
                if previous_group != Some(group) {
                    rows.push(settings_navigation_group_label(group, cx).into_any_element());
                    previous_group = Some(group);
                }
                rows.push(
                    settings_navigation_button(section, section == selected, cx)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings = Some(section);
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            div()
                .id("web-settings-nav")
                .flex()
                .flex_col()
                .w_full()
                .gap(px(2.0))
                .px(px(6.0))
                .child(zz_ui::settings::settings_navigation_back_row(
                    zz_ui::settings::settings_navigation_back_button("web-settings-back").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.settings = None;
                            this.focused_pane = None;
                            cx.notify();
                        }),
                    ),
                ))
                .children(rows)
                .into_any_element()
        } else {
            let core = &self.connection.read(cx).core;
            sidebar::session_tree(
                core.snapshot(),
                core.attached_session(),
                sidebar::Runtime {
                    connection: self.connection.clone(),
                    focus: self.sidebar_focus.clone(),
                    focused: self.sidebar_focus.is_focused(window),
                    view: cx.entity(),
                    selected: (!self.sidebar_pointer_selection)
                        .then_some(self.sidebar_selection)
                        .flatten(),
                    unseen_agents: self.unseen_agents.clone(),
                },
                &self.collapsed_tree,
                &self.sidebar_scroll,
                cx,
            )
        };
        let divider_hidden = self.preferences.gaps && self.inline_sidebar(window);
        workspace_sidebar_surface(
            "web-sidebar-surface",
            self.sidebar_width(window),
            workspace_sidebar_titlebar("web-sidebar-titlebar", self.controls(cx), cx),
            navigation,
            cx,
        )
        .when(divider_hidden, |surface| {
            surface.border_color(gpui::transparent_black())
        })
        .track_focus(&self.sidebar_focus)
        .child(
            div()
                .id("web-sidebar-resize-handle")
                .absolute()
                .top(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .w(px(8.0))
                .cursor(gpui::CursorStyle::ResizeLeftRight)
                .occlude()
                .when(divider_hidden, |handle| {
                    handle.hover(|handle| {
                        handle
                            .border_r_1()
                            .border_color(zz_ui::navigation::workspace_sidebar_divider(cx))
                    })
                })
                .on_drag(SidebarResizeDrag, |_: &SidebarResizeDrag, _, _, cx| {
                    cx.new(|_| SidebarResizePreview)
                })
                .child({
                    let view = cx.entity().downgrade();
                    touch_drag_handle(move |event, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            match event.phase {
                                gpui::TouchPhase::Started => window.prevent_default(),
                                gpui::TouchPhase::Moved => {
                                    this.preferences.sidebar_width = f32::from(
                                        event.position.x - window.fully_visible_bounds().left(),
                                    );
                                    this.preferences.sidebar_width = this.sidebar_width(window);
                                    cx.notify();
                                }
                                gpui::TouchPhase::Ended => this.preferences.save(),
                                gpui::TouchPhase::Cancelled => {}
                            }
                            cx.stop_propagation();
                        });
                    })
                }),
        )
        .into_any_element()
    }

    fn status_bar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        status_bar::render(self, window, cx)
    }

    fn focus_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = Some(pane);
        if let Some(terminal) = self.terminals.get(&pane) {
            terminal.read(cx).focus_handle(cx).focus(window, cx);
        } else if let Some(agent) = self.agents.get(&pane) {
            agent.read(cx).focus_handle(cx).focus(window, cx);
        } else if let Some(picker) = self.pickers.get(&pane) {
            picker.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.command(
            "select-pane",
            vec!["-Z".into(), "-t".into(), pane.to_string()],
            cx,
        );
    }

    fn dismiss_pane_prefix(&self, pane: PaneId, cx: &mut App) {
        if !self.connection.read(cx).core.prefix_armed() {
            return;
        }
        for action in [
            zz_terminal::KeyAction::Press,
            zz_terminal::KeyAction::Release,
        ] {
            self.send_input(
                InputMessage::Key {
                    pane,
                    input: zz_terminal::KeyInput {
                        key: zz_terminal::KeyCode::Escape,
                        action,
                        modifiers: zz_terminal::Modifiers::default(),
                        text: None,
                        unshifted_codepoint: None,
                    },
                    text_follows: false,
                },
                cx,
            );
        }
    }

    fn on_pane_drag_start(&mut self, drag: PaneDrag, cx: &mut Context<Self>) {
        let Some(active) = self.active_window(cx) else {
            return;
        };
        if !self.connection.read(cx).connected
            || self.connection.read(cx).core.attached_read_only()
            || active.zoomed_pane.is_some()
            || active.panes.len() < 2
            || !active.panes.contains_key(&drag.pane)
        {
            return;
        }
        self.rendered_drop_preview.set(DropPreviewFrame::default());
        self.pane_drag = Some(PaneDragState {
            drag,
            window: active.id,
            layout: active.layout,
            target: None,
            touch: false,
            preview: None,
        });
        cx.notify();
    }

    fn reconcile_pane_drag(
        &mut self,
        active: &WindowSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let core = &self.connection.read(cx).core;
        if self.pane_layout_override.as_ref().is_some_and(|pending| {
            pending.window != active.id
                || pending.generation < core.snapshot().generation
                || active.zoomed_pane.is_some()
        }) {
            self.pane_layout_override = None;
        }
        let invalid = self.pane_drag.as_ref().is_some_and(|state| {
            state.window != active.id
                || state.layout != active.layout
                || active.zoomed_pane.is_some()
                || !self.connection.read(cx).connected
                || core.attached_read_only()
                || (state.drag.requires_prefix && !core.prefix_armed())
        });
        if invalid {
            self.pane_drag = None;
            cx.stop_active_drag(window);
        } else if self.pane_drag.as_ref().is_some_and(|state| !state.touch) && !cx.has_active_drag()
        {
            self.finish_pane_drag(cx);
        }
    }

    fn on_pane_drag_move(
        &mut self,
        event: &DragMoveEvent<PaneDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = event.drag(cx).pane;
        if self
            .pane_drag
            .as_ref()
            .is_some_and(|state| state.drag.pane == source)
        {
            self.update_drop_target(event.event.position, cx);
        }
    }

    fn touch_pane_drag(
        &mut self,
        pane: PaneId,
        event: &gpui::TouchDragEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.phase {
            gpui::TouchPhase::Started => {
                self.on_pane_drag_start(
                    PaneDrag {
                        pane,
                        requires_prefix: false,
                    },
                    cx,
                );
                if let Some(state) = self.pane_drag.as_mut() {
                    state.touch = true;
                    window.prevent_default();
                }
            }
            gpui::TouchPhase::Moved => self.update_drop_target(event.position, cx),
            gpui::TouchPhase::Ended => self.drop_pane(
                &PaneDrag {
                    pane,
                    requires_prefix: false,
                },
                event.position,
                cx,
            ),
            gpui::TouchPhase::Cancelled => self.finish_pane_drag(cx),
        }
        cx.stop_propagation();
    }

    fn on_pane_drop(&mut self, drag: &PaneDrag, window: &mut Window, cx: &mut Context<Self>) {
        self.drop_pane(drag, window.mouse_position(), cx);
    }

    fn drop_pane(&mut self, drag: &PaneDrag, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(state) = self.pane_drag.take_if(|state| state.drag.pane == drag.pane) else {
            return;
        };
        if self.connection.read(cx).connected
            && !self.connection.read(cx).core.attached_read_only()
            && let Some((target, zone)) = state.target_at(position, &self.pane_bounds.borrow())
            && let Some(command) = pane_drop_command(drag.pane, target, zone)
        {
            self.pane_layout_override =
                predicted_drop_layout(&state.layout, drag.pane, target, zone).map(|layout| {
                    PaneLayoutOverride {
                        window: state.window,
                        layout,
                        generation: self.connection.read(cx).core.snapshot().generation,
                    }
                });
            sidebar::execute(&self.connection, &command, cx);
        }
        if drag.requires_prefix {
            self.dismiss_pane_prefix(drag.pane, cx);
        }
        cx.notify();
    }

    fn finish_pane_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.pane_drag.take() {
            if state.drag.requires_prefix {
                self.dismiss_pane_prefix(state.drag.pane, cx);
            }
            cx.notify();
        }
    }

    fn update_drop_target(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(state) = self.pane_drag.as_mut() else {
            return;
        };
        let panes = self.pane_bounds.borrow();
        let target = state.target_at(position, &panes);
        if state.target == target {
            return;
        }
        state.target = target;
        let rendered = self.rendered_drop_preview.get();
        let to = target
            .and_then(|(pane, zone)| {
                panes.get(&pane).map(|bounds| {
                    let canvas = self.pane_canvas_bounds.get();
                    let rect = drop_preview_bounds(
                        PaneRect {
                            x: f32::from(bounds.left() - canvas.left()),
                            y: f32::from(bounds.top() - canvas.top()),
                            width: f32::from(bounds.size.width),
                            height: f32::from(bounds.size.height),
                        },
                        zone,
                        if self.preferences.gaps {
                            self.preferences.pane_margin.max(1.0)
                        } else {
                            1.0
                        },
                    );
                    DropPreviewFrame {
                        bounds: Bounds::new(
                            gpui::point(px(rect.x), px(rect.y)),
                            gpui::size(px(rect.width), px(rect.height)),
                        ),
                        opacity: 1.0,
                    }
                })
            })
            .unwrap_or(DropPreviewFrame {
                opacity: 0.0,
                ..rendered
            });
        state.preview = Some(DropPreview {
            from: if rendered.opacity > 0.0 {
                rendered
            } else {
                DropPreviewFrame { opacity: 0.0, ..to }
            },
            to,
            sequence: state.preview.map_or(0, |preview| preview.sequence + 1),
            duration: std::time::Duration::from_millis(if target.is_some() { 180 } else { 140 }),
        });
        cx.notify();
    }

    fn pane_drop_preview(&self, cx: &App) -> Option<AnyElement> {
        let preview = self.pane_drag.as_ref()?.preview?;
        let rendered = Rc::clone(&self.rendered_drop_preview);
        let surface = pane_drop_preview(
            px(if self.preferences.gaps {
                self.preferences.pane_radius
            } else {
                0.0
            }),
            px(if self.preferences.gaps {
                self.preferences.pane_border_width.max(1.0)
            } else {
                1.0
            }),
            cx,
        );
        if !self.preferences.animations || cx.reduce_motion() {
            rendered.set(preview.to);
            return Some(
                surface
                    .left(preview.to.bounds.left())
                    .top(preview.to.bounds.top())
                    .w(preview.to.bounds.size.width)
                    .h(preview.to.bounds.size.height)
                    .opacity(preview.to.opacity)
                    .into_any_element(),
            );
        }
        Some(
            surface
                .with_animation(
                    ("pane-drop-preview", preview.sequence),
                    Animation::new(preview.duration).with_easing(gpui::ease_out_quint()),
                    move |surface, delta| {
                        let frame = preview.at(delta);
                        rendered.set(frame);
                        surface
                            .left(frame.bounds.left())
                            .top(frame.bounds.top())
                            .w(frame.bounds.size.width)
                            .h(frame.bounds.size.height)
                            .opacity(frame.opacity)
                    },
                )
                .into_any_element(),
        )
    }

    fn workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(section) = self.settings {
            self.slideover = false;
            if self.pane_drag.take().is_some() {
                cx.stop_active_drag(window);
            }
            self.pane_layout_override = None;
            return self.render_settings(section, window, cx);
        }
        let Some(active_window) = self.active_window(cx) else {
            if self.pane_drag.take().is_some() {
                cx.stop_active_drag(window);
            }
            self.pane_layout_override = None;
            let connected = self.connection.read(cx).connected;
            let has_sessions = !self.connection.read(cx).core.snapshot().sessions.is_empty();
            return div()
                .flex()
                .flex_col()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .child(Icon::new(IconName::SquareTerminal).size(px(32.0)))
                .child(div().text_size(px(16.0)).child(if connected {
                    "Your workspace"
                } else {
                    "Connect to zz"
                }))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(cx.theme().foreground.muted())
                        .child(self.connection.read(cx).status.clone()),
                )
                .child(if connected {
                    Button::new("web-create-session")
                        .small()
                        .label(if has_sessions {
                            "Attach to session"
                        } else {
                            "Create session"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            let first = this
                                .connection
                                .read(cx)
                                .core
                                .snapshot()
                                .sessions
                                .first()
                                .map(|session| session.id);
                            if let Some(session) = first {
                                this.connection
                                    .update(cx, |connection, cx| connection.attach(session, cx));
                            } else {
                                this.command("new-session", Vec::new(), cx);
                            }
                        }))
                } else {
                    Button::new("web-reconnect")
                        .small()
                        .label("Reconnect")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.connection.update(cx, Connection::reconnect);
                        }))
                })
                .into_any_element();
        };
        self.reconcile_pane_drag(&active_window, window, cx);
        self.reconcile_split_drag(&active_window, cx);
        let mut layout = active_window.zoomed_pane.map_or_else(
            || {
                self.pane_layout_override.as_ref().map_or_else(
                    || active_window.layout.clone(),
                    |pending| pending.layout.clone(),
                )
            },
            zz_protocol::LayoutNode::Pane,
        );
        if let Some(state) = self
            .split_drag
            .filter(|state| state.drag.window == active_window.id)
        {
            update_split_ratio(&mut layout, state.drag.split, state.ratio);
        }
        let rects = pane_rects(&layout);
        let core = &self.connection.read(cx).core;
        let can_drag = self.connection.read(cx).connected
            && !core.attached_read_only()
            && active_window.zoomed_pane.is_none()
            && active_window.panes.len() > 1;
        let prefix_armed = core.prefix_armed();
        let follows_pointer = core
            .mux_options()
            .get(zz_protocol::MuxOptionKey::FocusFollowsMouse)
            .is_some_and(|option| option.value == "on");
        let can_focus = self.prompt.is_none()
            && !self.sidebar_focus.is_focused(window)
            && core.choose_tree().is_none()
            && core.choose_buffer().is_none()
            && core.command_prompt().is_none()
            && core.menu().is_none()
            && core.confirm().is_none()
            && core.popup().is_none()
            && core.command_output().is_none();
        let output = core
            .command_output_id()
            .zip(core.command_output().map(|(pane, _)| pane));
        let existing: HashMap<_, _> = self
            .connection
            .read(cx)
            .core
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| {
                window
                    .panes
                    .iter()
                    .map(|(id, pane)| (*id, pane.kind.clone()))
            })
            .collect();
        if self.pickers.keys().any(|pane| {
            self.focused_pane == Some(*pane)
                && !matches!(existing.get(pane), Some(PaneKindSnapshot::Picker))
        }) {
            self.focused_pane = None;
        }
        self.terminals
            .retain(|pane, _| matches!(existing.get(pane), Some(PaneKindSnapshot::Terminal)));
        self.waiting_panes
            .retain(|pane| existing.contains_key(pane));
        self.agents
            .retain(|pane, _| matches!(existing.get(pane), Some(PaneKindSnapshot::Agent(_))));
        self.pickers
            .retain(|pane, _| matches!(existing.get(pane), Some(PaneKindSnapshot::Picker)));
        let mut panes = HashMap::new();
        self.pane_bounds
            .borrow_mut()
            .retain(|pane, _| active_window.panes.contains_key(pane));
        for (pane_id, _) in rects {
            let Some(pane) = active_window.panes.get(&pane_id) else {
                continue;
            };
            let radii = Corners::all(if self.preferences.gaps {
                px(self.preferences.pane_radius)
            } else {
                px(0.0)
            });
            let dead_label = pane.dead.then(|| {
                pane.dead_status
                    .map_or_else(|| "Dead".to_owned(), |status| format!("Dead · {status}"))
            });
            let content = match &pane.kind {
                PaneKindSnapshot::Terminal => {
                    let terminal = if let Some(id) = output
                        .filter(|(_, pane)| *pane == pane_id)
                        .map(|(id, _)| id)
                    {
                        if self
                            .output_terminal
                            .as_ref()
                            .is_none_or(|(previous, _)| *previous != id)
                        {
                            let connection = self.connection.clone();
                            let terminal = cx.new(|cx| {
                                TerminalPane::new_command_output(pane_id, connection, cx)
                            });
                            terminal.read(cx).focus_handle(cx).focus(window, cx);
                            self.output_terminal = Some((id, terminal));
                        }
                        &self
                            .output_terminal
                            .as_ref()
                            .expect("command output terminal")
                            .1
                    } else {
                        self.terminals.entry(pane_id).or_insert_with(|| {
                            let connection = self.connection.clone();
                            let suppressed = Rc::clone(&self.terminal_resize_suppressed);
                            cx.new(|cx| {
                                TerminalPane::new(pane_id, connection, cx)
                                    .with_resize_suppression(suppressed)
                            })
                        })
                    };
                    if can_focus
                        && pane_id == active_window.active_pane
                        && self.focused_pane != Some(pane_id)
                    {
                        terminal.read(cx).focus_handle(cx).focus(window, cx);
                        self.focused_pane = Some(pane_id);
                    }
                    terminal.update(cx, |terminal, cx| {
                        terminal.set_text_dimmed(
                            active_window.active_pane != pane_id,
                            self.preferences.pane_inactive_opacity,
                            cx,
                        );
                        terminal.set_pane_status(
                            dead_label.clone(),
                            pane.synchronized_input,
                            active_window.zoomed_pane == Some(pane_id),
                            cx,
                        );
                        terminal.set_corner_radii(
                            Corners {
                                top_left: px(0.0),
                                top_right: px(0.0),
                                ..radii
                            },
                            cx,
                        );
                    });
                    terminal.clone().into_any_element()
                }
                PaneKindSnapshot::Agent(descriptor) => {
                    let view = cx.weak_entity();
                    let touch_view = view.clone();
                    let agent = self.agents.entry(pane_id).or_insert_with(|| {
                        let connection = self.connection.clone();
                        cx.new(|cx| {
                            let mut agent =
                                AgentPane::new(pane_id, descriptor.clone(), connection, window, cx);
                            agent.set_header_drag_handler(move |drag, _, cx| {
                                let _ =
                                    view.update(cx, |this, cx| this.on_pane_drag_start(*drag, cx));
                            });
                            agent.set_header_touch_drag_handler(move |event, window, cx| {
                                let _ = touch_view.update(cx, |this, cx| {
                                    this.touch_pane_drag(pane_id, event, window, cx);
                                });
                            });
                            agent
                        })
                    });
                    agent.update(cx, |agent, cx| {
                        agent.update_descriptor(descriptor, cx);
                        agent.set_corner_radii(radii, cx);
                    });
                    if can_focus
                        && pane_id == active_window.active_pane
                        && self.focused_pane != Some(pane_id)
                    {
                        agent.read(cx).focus_handle(cx).focus(window, cx);
                        self.focused_pane = Some(pane_id);
                    }
                    agent.clone().into_any_element()
                }
                PaneKindSnapshot::Browser(descriptor) => unsupported_pane(
                    "Browser",
                    IconName::Globe,
                    "Embedded Chromium needs the desktop app. Open this page in a browser tab.",
                    descriptor.tabs.get(descriptor.active_tab).cloned(),
                    radii,
                    cx,
                ),
                PaneKindSnapshot::Editor(_) => unsupported_pane(
                    "Editor",
                    IconName::File,
                    "The daemon does not share editor file contents with browser clients yet.",
                    None,
                    radii,
                    cx,
                ),
                PaneKindSnapshot::Picker => {
                    let picker = self.pickers.entry(pane_id).or_insert_with(|| {
                        let connection = self.connection.clone();
                        cx.new(|cx| {
                            picker::PanePicker::new(
                                pane_id,
                                connection,
                                self.preferences.agent_enabled,
                                cx,
                            )
                        })
                    });
                    if can_focus
                        && pane_id == active_window.active_pane
                        && self.focused_pane != Some(pane_id)
                    {
                        picker.read(cx).focus_handle(cx).focus(window, cx);
                        self.focused_pane = Some(pane_id);
                    }
                    picker.update(cx, |picker, cx| {
                        picker.set_agent_enabled(self.preferences.agent_enabled);
                        picker.set_corner_radii(radii, cx);
                    });
                    picker.clone().into_any_element()
                }
            };
            let content = if matches!(pane.kind, PaneKindSnapshot::Terminal) {
                let title = zz_client::navigation::pane_label(pane);
                let view = cx.entity();
                let touch_view = cx.weak_entity();
                let connection = self.connection.clone();
                let background = output
                    .filter(|(_, pane)| *pane == pane_id)
                    .and(self.output_terminal.as_ref())
                    .map_or(&self.terminals[&pane_id], |(_, terminal)| terminal)
                    .read(cx)
                    .pane_background(cx);
                let header = terminal_pane_header(
                    active_window.active_pane == pane_id,
                    title.clone(),
                    pane_drag_button(
                        ("web-terminal-drag", pane_id.0),
                        pane_id,
                        title,
                        can_drag,
                        move |drag, _, cx| {
                            view.update(cx, |this, cx| this.on_pane_drag_start(*drag, cx));
                        },
                        cx,
                    )
                    .relative()
                    .when(can_drag, |handle| {
                        handle.child(touch_drag_handle(move |event, window, cx| {
                            let _ = touch_view.update(cx, |this, cx| {
                                this.touch_pane_drag(pane_id, event, window, cx);
                            });
                        }))
                    }),
                    move |action, _, cx| {
                        connection.update(cx, |connection, cx| {
                            if !connection.connected || connection.core.attached_read_only() {
                                return;
                            }
                            let (name, mut args) = match action {
                                TerminalPaneAction::SplitBottom => (
                                    "split-window",
                                    vec!["--kind".into(), "picker".into(), "-v".into()],
                                ),
                                TerminalPaneAction::SplitRight => (
                                    "split-window",
                                    vec!["--kind".into(), "picker".into(), "-h".into()],
                                ),
                                TerminalPaneAction::Close => ("kill-pane", Vec::new()),
                            };
                            args.extend(["-t".into(), pane_id.to_string()]);
                            connection.command(name, args, cx);
                        });
                    },
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus_pane(pane_id, window, cx);
                }));
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(
                        div()
                            .flex_none()
                            .rounded_tl(radii.top_left)
                            .rounded_tr(radii.top_right)
                            .bg(background)
                            .child(
                                div()
                                    .opacity(if active_window.active_pane == pane_id {
                                        1.0
                                    } else {
                                        self.preferences.pane_inactive_opacity
                                    })
                                    .child(header),
                            ),
                    )
                    .child(div().flex_1().min_h_0().min_w_0().child(content))
                    .into_any_element()
            } else {
                content
            };
            let chrome = PaneChrome::new(
                radii,
                px(if self.preferences.gaps {
                    self.preferences.pane_border_width
                } else {
                    0.0
                }),
                pane_border_color(active_window.active_pane == pane_id, cx),
                self.preferences.gaps,
            )
            .active(active_window.active_pane == pane_id)
            .dimmed(
                active_window.active_pane != pane_id
                    && !matches!(pane.kind, PaneKindSnapshot::Terminal),
                self.preferences.pane_inactive_opacity,
            );
            let terminal_pane = matches!(&pane.kind, PaneKindSnapshot::Terminal);
            let mut status_tags = Vec::new();
            if !terminal_pane && let Some(label) = dead_label {
                status_tags.push(
                    zz_ui::pane::pane_status_badge(IconName::CircleX, label, cx).into_any_element(),
                );
            }
            if terminal_pane && self.connection.read(cx).core.viewport(pane_id).is_none() {
                self.waiting_panes.insert(pane_id);
                status_tags.push(
                    zz_ui::pane::pane_waiting_state(format!("Waiting for {pane_id}"), cx)
                        .into_any_element(),
                );
            }
            if !terminal_pane && pane.synchronized_input {
                status_tags.push(zz_ui::pane::pane_sync_badge(cx).into_any_element());
            }
            if !terminal_pane && active_window.zoomed_pane == Some(pane_id) {
                let connection = self.connection.clone();
                let control = div()
                    .id(("web-unzoom-pane", pane_id.0))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        connection.update(cx, |connection, cx| {
                            if !connection.core.attached_read_only() {
                                connection.command(
                                    "resize-pane",
                                    vec!["-Z".into(), "-t".into(), pane_id.to_string()],
                                    cx,
                                );
                            }
                        });
                        cx.stop_propagation();
                    })
                    .child(zz_ui::pane::pane_unzoom_control());
                status_tags.push(control.into_any_element());
            }
            let mut status_overlays = if status_tags.is_empty() {
                Vec::new()
            } else {
                vec![
                    zz_ui::pane::pane_overlay_stack(
                        zz_ui::pane::PaneOverlayCorner::TopRight,
                        status_tags,
                    )
                    .when(terminal_pane, |stack| {
                        stack.top(px(zz_ui::pane::TERMINAL_HEADER_HEIGHT + 8.0))
                    })
                    .occlude()
                    .into_any_element(),
                ]
            };
            if can_drag && (prefix_armed || self.pane_drag.is_some()) {
                let state = if self
                    .pane_drag
                    .as_ref()
                    .is_some_and(|state| state.drag.pane == pane_id)
                {
                    PaneDragOverlayState::Source
                } else {
                    PaneDragOverlayState::Armed
                };
                let view = cx.entity();
                let title = zz_client::navigation::pane_label(pane);
                status_overlays.push(
                    pane_drag_overlay(("web-pane-drag-overlay", pane_id.0), state, radii, cx)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.focus_pane(pane_id, window, cx);
                            this.dismiss_pane_prefix(pane_id, cx);
                        }))
                        .on_drag(
                            PaneDrag {
                                pane: pane_id,
                                requires_prefix: true,
                            },
                            move |drag, grab, _, cx| {
                                view.update(cx, |this, cx| this.on_pane_drag_start(*drag, cx));
                                pane_drag_preview(drag.pane, title.clone(), grab, cx)
                            },
                        )
                        .on_drop(cx.listener(Self::on_pane_drop))
                        .into_any_element(),
                );
            }
            if let Some(indicator) =
                self.connection
                    .read(cx)
                    .core
                    .display_panes()
                    .and_then(|display| {
                        display
                            .indicators
                            .iter()
                            .find(|indicator| indicator.pane == pane_id)
                    })
            {
                let connection = self.connection.clone();
                let pane = indicator.pane;
                let key = indicator
                    .selection_key()
                    .and_then(|key| gpui::Keystroke::parse(&key.to_string()).ok())
                    .map_or_else(
                        || {
                            div()
                                .text_xs()
                                .text_color(cx.theme().foreground.muted())
                                .child("click")
                                .into_any_element()
                        },
                        |key| zz_ui::kbd::Kbd::new(key).into_any_element(),
                    );
                let card = zz_ui::pane::pane_indicator_card(
                    ("web-pane-indicator", pane.0),
                    indicator.index.to_string(),
                    key,
                    indicator.active(),
                    cx.theme().mono_font_family.clone(),
                    cx,
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    connection.update(cx, |connection, cx| {
                        connection.send(
                            ProtocolMessage::Input(InputMessage::DisplayPanes {
                                action: DisplayPanesAction::Select(pane),
                            }),
                            cx,
                        );
                    });
                    cx.stop_propagation();
                });
                let label = (!indicator.label.is_empty()).then(|| {
                    let foreground = cx.theme().foreground;
                    let background = cx.theme().background;
                    let bucket = |segments: &[zz_protocol::StyledSegment]| {
                        zz_ui::tmux_style::tmux_styled_segments_text(
                            segments, foreground, background, cx,
                        )
                        .into_styled_text()
                    };
                    let [left, centre, right] =
                        zz_ui::tmux_style::split_tmux_alignment(&indicator.label);
                    div()
                        .absolute()
                        .top(px(8.0))
                        .left(px(8.0))
                        .right(px(8.0))
                        .overflow_hidden()
                        .flex()
                        .justify_between()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .text_color(foreground)
                        .child(bucket(&left))
                        .child(bucket(&centre))
                        .child(bucket(&right))
                });
                status_overlays.push(
                    zz_ui::pane::pane_indicator_overlay(card)
                        .children(label)
                        .into_any_element(),
                );
            }
            let measured = Rc::clone(&self.pane_bounds);
            panes.insert(
                pane_id,
                pane_surface(
                    ("web-pane", pane_id.0),
                    content,
                    status_overlays,
                    chrome,
                    cx,
                )
                .when(
                    follows_pointer && active_window.active_pane != pane_id,
                    |surface| {
                        surface.on_mouse_move(cx.listener(
                            move |this, event: &gpui::MouseMoveEvent, _, cx| {
                                if event.pressed_button.is_none() {
                                    this.command(
                                        "select-pane",
                                        vec!["-t".into(), pane_id.to_string()],
                                        cx,
                                    );
                                }
                            },
                        ))
                    },
                )
                .capture_any_mouse_down(cx.listener(move |this, _, window, cx| {
                    if this
                        .active_window(cx)
                        .is_some_and(|active| active.active_pane != pane_id)
                    {
                        this.focus_pane(pane_id, window, cx);
                    }
                }))
                .on_prepaint(move |bounds, _, _| {
                    measured.borrow_mut().insert(pane_id, bounds);
                })
                .into_any_element(),
            );
        }
        let content = self.render_layout(&layout, &active_window, &mut panes, cx);
        let mut overlays = Vec::new();
        if !self.connection.read(cx).connected {
            overlays.push(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_0()
                    .p(px(8.0))
                    .bg(cx.theme().background.raised(2))
                    .text_size(px(12.0))
                    .text_color(cx.theme().warning)
                    .child(self.connection.read(cx).status.clone())
                    .into_any_element(),
            );
        }
        let canvas_bounds = Rc::clone(&self.pane_canvas_bounds);
        let release_view = cx.weak_entity();
        let preview = self.pane_drop_preview(cx);
        div()
            .id("web-pane-canvas")
            .relative()
            .size_full()
            .on_prepaint(move |bounds, _, _| canvas_bounds.set(bounds))
            .on_drag_move::<PaneDrag>(cx.listener(Self::on_pane_drag_move))
            .on_drop(cx.listener(Self::on_pane_drop))
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                if this.pane_drag.as_ref().is_some_and(|state| !state.touch)
                    && !cx.has_active_drag()
                {
                    this.finish_pane_drag(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _, window, cx| {
                    cx.defer_in(window, |this, _, cx| this.finish_pane_drag(cx));
                }),
            )
            .on_mouse_exit(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.pane_drag.as_mut() {
                    state.target = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .size_full()
                    .p(px(if self.preferences.gaps {
                        self.preferences.pane_margin
                    } else {
                        0.0
                    }))
                    .when(!self.inline_sidebar(window), gpui::Styled::pt_0)
                    .child(content),
            )
            .children(overlays)
            .children(preview)
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, _, cx| {
                            if phase.capture() && event.button == MouseButton::Left {
                                let _ = release_view.update(cx, Self::commit_split);
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }

    fn render_layout(
        &self,
        node: &zz_protocol::LayoutNode,
        active: &WindowSnapshot,
        panes: &mut HashMap<PaneId, AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let zz_protocol::LayoutNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } = node
        else {
            let zz_protocol::LayoutNode::Pane(pane) = node else {
                unreachable!()
            };
            return panes
                .remove(pane)
                .unwrap_or_else(|| div().into_any_element());
        };
        let first = self.render_layout(first, active, panes, cx);
        let second = self.render_layout(second, active, panes, cx);
        let split = *id;
        let axis = *axis;
        let ratio = *ratio;
        let drag = SplitDrag {
            window: active.id,
            split,
            axis,
            start_ratio: ratio,
        };
        let split_axis = match axis {
            zz_protocol::Axis::Horizontal => PaneSplitAxis::Horizontal,
            zz_protocol::Axis::Vertical => PaneSplitAxis::Vertical,
        };
        let gap = px(if self.preferences.gaps {
            self.preferences.pane_margin
        } else {
            0.0
        });
        let highlight = zz_client::pane_separator::pane_separator(node, active.active_pane, None)
            .map(|separator| {
                PaneSplitHighlight::new(
                    separator.span().start(),
                    separator.span().length(),
                    match separator.side() {
                        zz_client::pane_separator::SeparatorSide::First => PaneSplitSide::First,
                        zz_client::pane_separator::SeparatorSide::Second => PaneSplitSide::Second,
                    },
                    cx.theme().accent,
                )
            });
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let touch_bounds = Rc::clone(&bounds);
        let view = cx.entity().downgrade();
        let enabled = self.connection.read(cx).connected
            && !self.connection.read(cx).core.attached_read_only()
            && self.pane_drag.is_none();
        let handle = if enabled {
            pane_split_hit_target(("split-divider", split.0), split_axis, ratio, gap)
                .on_drag(drag, |_: &SplitDrag, _, _, cx| cx.new(|_| SplitDragPreview))
                .child(touch_drag_handle(move |event, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        match event.phase {
                            gpui::TouchPhase::Started => {
                                this.set_split_drag(Some(SplitDragState {
                                    drag,
                                    ratio,
                                    touch: true,
                                    committed_generation: None,
                                }));
                                window.prevent_default();
                            }
                            gpui::TouchPhase::Moved => {
                                if this
                                    .split_drag
                                    .is_some_and(|state| state.drag.split == split)
                                {
                                    this.move_split(drag, event.position, touch_bounds.get(), cx);
                                }
                            }
                            gpui::TouchPhase::Ended => this.commit_split(cx),
                            gpui::TouchPhase::Cancelled => {
                                this.set_split_drag(None);
                                cx.notify();
                            }
                        }
                        cx.stop_propagation();
                    });
                }))
                .into_any_element()
        } else {
            div().absolute().into_any_element()
        };
        pane_split_surface(
            ("mux-split", split.0),
            split_axis,
            ratio,
            self.split_drag
                .is_some_and(|state| state.drag.split == split),
            self.preferences.gaps,
            gap,
            None,
            highlight,
            first,
            second,
            handle,
            cx,
        )
        .on_prepaint(move |measured, _, _| bounds.set(measured))
        .on_drag_move::<SplitDrag>(cx.listener(
            move |this, event: &DragMoveEvent<SplitDrag>, _, cx| {
                let drag = *event.drag(cx);
                if drag.split == split {
                    this.move_split(drag, event.event.position, event.bounds, cx);
                    cx.stop_propagation();
                }
            },
        ))
        .into_any_element()
    }

    fn move_split(
        &mut self,
        drag: SplitDrag,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self
            .split_drag
            .is_some_and(|state| state.committed_generation.is_some())
        {
            return;
        }
        let (offset, size) = match drag.axis {
            zz_protocol::Axis::Horizontal => (position.x - bounds.left(), bounds.size.width),
            zz_protocol::Axis::Vertical => (position.y - bounds.top(), bounds.size.height),
        };
        let (start_ratio, touch) = self.split_drag.map_or((drag.start_ratio, false), |state| {
            (state.drag.start_ratio, state.touch)
        });
        self.set_split_drag(Some(SplitDragState {
            drag: SplitDrag {
                start_ratio,
                ..drag
            },
            ratio: if f32::from(size) > 0.0 {
                (f32::from(offset) / f32::from(size)).clamp(0.0, 1.0)
            } else {
                0.5
            },
            touch,
            committed_generation: None,
        }));
        cx.notify();
    }

    fn set_split_drag(&mut self, state: Option<SplitDragState>) {
        self.split_drag = state;
        self.terminal_resize_suppressed.set(state.is_some());
    }

    fn reconcile_split_drag(&mut self, active: &WindowSnapshot, cx: &mut Context<Self>) {
        let Some(state) = self.split_drag else {
            return;
        };
        if state.drag.window != active.id
            || active.zoomed_pane.is_some()
            || !active.layout.contains_split(state.drag.split)
        {
            self.set_split_drag(None);
        } else if !state.touch && state.committed_generation.is_none() && !cx.has_active_drag() {
            self.commit_split(cx);
        }
    }

    fn commit_split(&mut self, cx: &mut Context<Self>) {
        let Some(mut state) = self.split_drag else {
            return;
        };
        if state.committed_generation.is_some() {
            return;
        }
        let ratio_basis_points = split_ratio_basis(state.ratio);
        if ratio_basis_points == split_ratio_basis(state.drag.start_ratio) {
            self.set_split_drag(None);
        } else {
            state.ratio = f32::from(ratio_basis_points) / f32::from(zz_protocol::SPLIT_RATIO_BASIS);
            state.committed_generation = Some(self.connection.read(cx).core.snapshot().generation);
            self.set_split_drag(Some(state));
            self.send_input(
                InputMessage::ResizeSplit {
                    window: state.drag.window,
                    split: state.drag.split,
                    ratio_basis_points,
                },
                cx,
            );
        }
        cx.notify();
    }

    fn overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let core = &self.connection.read(cx).core;
        let tree = core.choose_tree().cloned();
        let buffer = core.choose_buffer().cloned();
        let prompt = core.command_prompt().cloned();
        let window_chooser = tree
            .as_ref()
            .filter(|state| state.kind == ChooseTreeKind::Windows);
        if self.prompt.as_ref().is_some_and(|palette| {
            let palette = palette.read(cx);
            if palette.is_local() {
                palette.is_finished()
            } else if palette.is_window_chooser() {
                window_chooser.is_none()
            } else {
                prompt.is_none()
            }
        }) {
            self.prompt = None;
            self.focused_pane = None;
        }
        let daemon_reopened =
            self.local_palette_revisions
                .is_none_or(|(prompt_revision, chooser_revision)| {
                    (prompt.is_some() && prompt_revision != self.prompt_revision)
                        || (window_chooser.is_some() && chooser_revision != self.chooser_revision)
                });
        if !daemon_reopened
            && let Some(palette) = self
                .prompt
                .as_ref()
                .filter(|palette| palette.read(cx).is_local())
        {
            palette.update(cx, |palette, cx| palette.refresh(window, cx));
            self.modal_open = true;
            return Some(palette.clone().into_any_element());
        }
        if let Some(state) = window_chooser {
            let opened = self
                .prompt
                .as_ref()
                .is_none_or(|palette| !palette.read(cx).is_window_chooser());
            if opened {
                let backend = self.palette_backend();
                let revision = self.chooser_revision;
                let palette = cx.new(|cx| {
                    CommandPaletteView::new_window_chooser(backend, state, revision, window, cx)
                });
                self.observe_palette(&palette, window, cx);
                palette.read(cx).focus(cx).focus(window, cx);
                self.prompt = Some(palette);
            }
            let palette = self.prompt.as_ref().unwrap();
            palette.update(cx, |palette, cx| {
                palette.synchronize_window_chooser(state, self.chooser_revision, window, cx);
            });
            self.modal_open = true;
            return Some(palette.clone().into_any_element());
        }

        let menu = core.menu().cloned();
        let confirm = core.confirm().cloned();
        if prompt.is_none()
            && (tree.is_some() || buffer.is_some() || menu.is_some() || confirm.is_some())
            && !self.modal_open
        {
            self.focus.focus(window, cx);
        }
        if tree.is_none()
            && buffer.is_none()
            && prompt.is_none()
            && (menu.is_some() || confirm.is_some())
        {
            self.modal_open = true;
            return None;
        }
        let chooser_rows;
        let row_count;
        let subtitle;
        let mut max_width = 640.0;
        let mut chooser_prompt = None;
        let hints: &'static [ChooserHint];
        if tree.is_none() && buffer.is_none() {
            self.chooser_selection = None;
        }
        let (title, search, help, close) = if let Some(state) = tree {
            let count = state.items.len();
            let selected = state.selected as usize;
            let selection = Some((true, state.selected));
            if self.chooser_selection != selection && selected < count {
                self.chooser_scroll
                    .scroll_to_item(selected, ScrollStrategy::Center);
            }
            self.chooser_selection = selection;
            let show_key_gutter =
                chooser_has_key_gutter(state.items.iter().map(|item| item.key.as_str()));
            let theme = ChooserRowTheme::from_theme(cx);
            let font = cx.theme().mono_font_family.clone();
            let connection = self.connection.clone();
            chooser_rows = Some(
                uniform_list(
                    "web-tree-rows",
                    count,
                    cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                        range
                            .filter_map(|index| {
                                let item = state.items.get(index)?;
                                let disclosure = if item.has_children() {
                                    if item.expanded() { "▾" } else { "▸" }
                                } else {
                                    ""
                                };
                                let pane_kind = item.pane_kind.map(|kind| match kind {
                                    ChooseTreePaneKind::Terminal => ChooserPaneKind::Terminal,
                                    ChooseTreePaneKind::Browser => ChooserPaneKind::Browser,
                                    ChooseTreePaneKind::Agent => ChooserPaneKind::Agent,
                                    ChooseTreePaneKind::Editor => ChooserPaneKind::Editor,
                                });
                                let (label, detail) = if item.text.is_empty() {
                                    (item.label.clone(), item.detail.clone())
                                } else {
                                    (item.text.clone(), String::new())
                                };
                                let connection = connection.clone();
                                Some(
                                    tree_chooser_row(
                                        "web-tree-choice",
                                        index,
                                        item.key.clone(),
                                        show_key_gutter,
                                        item.target.to_string(),
                                        label,
                                        detail,
                                        item.depth,
                                        disclosure,
                                        pane_kind,
                                        item.active(),
                                        item.tagged(),
                                        index == selected,
                                        theme,
                                        font.clone(),
                                    )
                                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                        let action = if event.click_count >= 2 {
                                            ChooseTreeAction::ActivateIndex(index as u32)
                                        } else {
                                            ChooseTreeAction::Select(index as u32)
                                        };
                                        connection.update(cx, |connection, cx| {
                                            connection.send(
                                                ProtocolMessage::Input(InputMessage::ChooseTree {
                                                    action,
                                                }),
                                                cx,
                                            );
                                        });
                                        cx.stop_propagation();
                                    })
                                    .into_any_element(),
                                )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .track_scroll(&self.chooser_scroll)
                .into_any_element(),
            );
            let (title, targets) = match state.kind {
                ChooseTreeKind::Windows => ("Choose window", "sessions and windows"),
                ChooseTreeKind::Panes => ("Choose pane", "sessions, windows, and panes"),
                ChooseTreeKind::Clients => ("Choose client", "clients"),
            };
            subtitle = chooser_subtitle(format!("{count} {targets}"), state.filter_no_matches);
            max_width = 600.0;
            row_count = Some(count);
            chooser_prompt = (!state.prompt.is_empty()).then_some(state.prompt.into());
            hints = TREE_HINTS;
            (
                title.to_owned(),
                state.search.map(|search| ChooserSearch {
                    prefix: if search.reverse { "?" } else { "/" }.into(),
                    value: search.query.into(),
                }),
                state.help,
                InputMessage::ChooseTree {
                    action: ChooseTreeAction::Close,
                },
            )
        } else if let Some(state) = buffer {
            let count = state.items.len();
            let selected = state.selected as usize;
            let selection = Some((false, state.selected));
            if self.chooser_selection != selection && selected < count {
                self.chooser_scroll
                    .scroll_to_item(selected, ScrollStrategy::Center);
            }
            self.chooser_selection = selection;
            let show_key_gutter =
                chooser_has_key_gutter(state.items.iter().map(|item| item.key.as_str()));
            let theme = ChooserRowTheme::from_theme(cx);
            let font = cx.theme().mono_font_family.clone();
            let connection = self.connection.clone();
            let now = unix_seconds();
            chooser_rows = Some(
                uniform_list(
                    "web-buffer-rows",
                    count,
                    cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                        range
                            .filter_map(|index| {
                                let item = state.items.get(index)?;
                                let (name, preview, size, age) = buffer_row_text(item, now);
                                let connection = connection.clone();
                                Some(
                                    buffer_chooser_row(
                                        "web-buffer-choice",
                                        index,
                                        item.key.clone(),
                                        show_key_gutter,
                                        name,
                                        preview,
                                        size,
                                        age,
                                        item.tagged,
                                        index == selected,
                                        theme,
                                        font.clone(),
                                    )
                                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                        let action = if event.click_count >= 2 {
                                            ChooseBufferAction::PasteIndex(index as u32)
                                        } else {
                                            ChooseBufferAction::Select(index as u32)
                                        };
                                        connection.update(cx, |connection, cx| {
                                            connection.send(
                                                ProtocolMessage::Input(
                                                    InputMessage::ChooseBuffer { action },
                                                ),
                                                cx,
                                            );
                                        });
                                        cx.stop_propagation();
                                    })
                                    .into_any_element(),
                                )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .track_scroll(&self.chooser_scroll)
                .into_any_element(),
            );
            subtitle = chooser_subtitle(format!("{count} buffers"), state.filter_no_matches);
            row_count = Some(count);
            hints = BUFFER_HINTS;
            (
                "Paste buffer".to_owned(),
                state.search.map(|search| ChooserSearch {
                    prefix: if search.reverse { "?" } else { "/" }.into(),
                    value: search.query.into(),
                }),
                state.help,
                InputMessage::ChooseBuffer {
                    action: ChooseBufferAction::Close,
                },
            )
        } else if let Some(state) = prompt {
            let snapshot = Arc::clone(self.connection.read(cx).core.snapshot());
            if self.prompt.as_ref().is_some_and(|palette| {
                palette.read(cx).is_local() || palette.read(cx).is_window_chooser()
            }) {
                self.prompt = None;
            }
            if self.prompt.is_none() {
                let backend = self.palette_backend();
                let revision = self.prompt_revision;
                let palette = cx.new(|cx| {
                    CommandPaletteView::new(
                        backend,
                        &state,
                        revision,
                        Arc::clone(&snapshot),
                        window,
                        cx,
                    )
                });
                self.observe_palette(&palette, window, cx);
                palette.read(cx).focus(cx).focus(window, cx);
                self.prompt = Some(palette);
            }
            let palette = self.prompt.as_ref().expect("daemon prompt palette");
            palette.update(cx, |palette, cx| {
                palette.synchronize(&state, self.prompt_revision, &snapshot, window, cx);
                palette.refresh(window, cx);
            });
            self.modal_open = true;
            return Some(palette.clone().into_any_element());
        } else {
            self.menu_selection = None;
            if self.modal_open {
                self.focused_pane = None;
                cx.notify();
            }
            self.prompt = None;
            self.modal_open = false;
            return None;
        };
        self.modal_open = true;
        let connection = self.connection.clone();
        let close_input = close.clone();
        let close_button = Button::compact_icon("web-overlay-close", IconName::Xmark)
            .ghost()
            .flat()
            .tooltip("Close")
            .on_click(move |_, _, cx| {
                connection.update(cx, |connection, cx| {
                    connection.send(ProtocolMessage::Input(close_input.clone()), cx);
                });
                cx.stop_propagation();
            });
        let count = row_count.unwrap_or(2);
        let rows = chooser_rows?;
        let modal = ChooserModal::new(
            "web-daemon-overlay",
            title,
            subtitle,
            ChooserDimensions {
                max_width,
                row_count: count,
            },
            rows,
            close_button,
            cx.theme().mono_font_family.clone(),
        )
        .prompt(chooser_prompt)
        .help(help)
        .search(search)
        .hints(hints);
        let connection = self.connection.clone();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_start()
                .justify_center()
                .px(px(24.0))
                .py(px(22.0))
                .bg(cx.theme().scrim)
                .occlude()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    connection.update(cx, |connection, cx| {
                        connection.send(ProtocolMessage::Input(close.clone()), cx);
                    });
                    cx.stop_propagation();
                })
                .child(modal)
                .into_any_element(),
        )
    }

    fn terminal_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let core = &self.connection.read(cx).core;
        let popup = core.popup().cloned();
        let output = core
            .command_output_id()
            .zip(core.command_output().map(|(pane, _)| pane));
        if let Some(popup) = popup {
            if self
                .popup_terminal
                .as_ref()
                .is_none_or(|(pane, _)| *pane != popup.pane)
            {
                let connection = self.connection.clone();
                let terminal = cx.new(|cx| TerminalPane::new_popup(popup.pane, connection, cx));
                terminal.read(cx).focus_handle(cx).focus(window, cx);
                self.popup_terminal = Some((popup.pane, terminal));
            }
            let terminal = self.popup_terminal.as_ref()?.1.clone();
            let bordered = popup.border_lines != zz_protocol::PopupBorderLines::None;
            let frame = zz_ui::command::floating::floating_frame(
                popup.left,
                popup.top,
                popup.width,
                popup.height,
                popup.client_columns,
                popup.client_rows,
                popup.cell_width_px,
                popup.cell_height_px,
                bordered,
                gpui::point(px(0.0), px(0.0)),
                self.floating_canvas_size(window),
                window.scale_factor(),
            );
            let background = floating::style_color(
                &popup.style,
                "bg",
                cx.theme().background.raised(1).opaque(),
                cx,
            );
            let foreground = floating::style_color(&popup.style, "fg", cx.theme().foreground, cx);
            let border_color =
                floating::style_color(&popup.border_style, "fg", cx.theme().border(), cx);
            return Some(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .child(
                        div()
                            .absolute()
                            .left(frame.bounds.origin.x)
                            .top(frame.bounds.origin.y)
                            .w(frame.bounds.size.width)
                            .h(frame.bounds.size.height)
                            .child(
                                zz_ui::pane::FloatingSurface::new(
                                    "web-display-popup",
                                    terminal,
                                    cx,
                                )
                                .title(popup.title)
                                .content_inset(frame.inset_x, frame.inset_y)
                                .colors(background, foreground, border_color)
                                .bordered(bordered),
                            ),
                    )
                    .into_any_element(),
            );
        }
        let popup_closed = self.popup_terminal.take().is_some();
        let output_closed = output.is_none() && self.output_terminal.take().is_some();
        if popup_closed || output_closed {
            self.focused_pane = None;
            cx.notify();
        }
        None
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(target_os = "ios")]
        self.sync_authentication(window, cx);
        let dialog = window
            .root::<Root>()
            .flatten()
            .is_some_and(|root| root.read(cx).has_active_dialog());
        self.connection.update(cx, |connection, cx| {
            connection.reconcile_dialog_prefix(dialog, cx)
        });
        if !self.inline_sidebar(window)
            && !self.slideover
            && self.settings.is_none()
            && self.sidebar_focus.is_focused(window)
        {
            self.release_sidebar_focus(window, cx);
        }
        let sidebar = if self.inline_sidebar(window) {
            self.sidebar(window, cx)
        } else {
            div().into_any_element()
        };
        let titlebar = (self.settings.is_none() && !self.inline_sidebar(window))
            .then(|| self.status_bar(window, cx));
        let workspace = self.workspace(window, cx);
        let mut terminal_overlays = self
            .terminal_overlay(window, cx)
            .into_iter()
            .collect::<Vec<_>>();
        terminal_overlays.extend(self.floating_overlay(window, cx));
        let mut overlays = Vec::new();
        if self.slideover && !self.inline_sidebar(window) && self.settings.is_none() {
            overlays.push(self.slideover(window, cx));
        }
        overlays.extend(self.overlay(window, cx));
        overlays.extend(Root::render_dialog_layer(window, cx).map(IntoElement::into_any_element));
        overlays
            .extend(Root::render_notification_layer(window, cx).map(IntoElement::into_any_element));
        let shell = app_shell_surface(
            "web-client",
            cx.theme().background,
            sidebar,
            titlebar,
            app_workspace_surface("web-workspace", workspace, terminal_overlays, cx),
            overlays,
        )
        .track_focus(&self.focus)
        .map(|shell| {
            #[cfg(target_os = "ios")]
            let shell = shell
                .on_action(cx.listener(Self::menu_command))
                .on_action(cx.listener(|this, action: &OpenSession, _, cx| {
                    this.connection.update(cx, |connection, cx| {
                        connection.open_session(action.name.clone(), cx);
                    });
                }));
            shell
        })
        .on_drag_move::<SidebarResizeDrag>(cx.listener(
            |this, event: &DragMoveEvent<SidebarResizeDrag>, window, cx| {
                let previous = this.preferences.sidebar_width;
                this.preferences.sidebar_width =
                    f32::from(event.event.position.x - event.bounds.origin.x);
                this.preferences.sidebar_width = this.sidebar_width(window);
                if this.preferences.sidebar_width != previous {
                    this.preferences.save();
                    cx.notify();
                }
                cx.stop_propagation();
            },
        ))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.defer_in(window, |this, _, cx| this.finish_pane_drag(cx));
                this.commit_split(cx);
            }),
        );
        let visible = window.fully_visible_bounds();
        let bottom = if self.preferences.extend_bottom_safe_area {
            window.visual_viewport_bounds().bottom()
        } else {
            visible.bottom()
        };
        div()
            .size_full()
            .bg(cx.theme().background)
            .pt(visible.top())
            .pb((window.viewport_size().height - bottom).max(px(0.0)))
            .pl(visible.left())
            .pr(window.viewport_size().width - visible.right())
            .child(shell)
    }
}

pub(super) fn touch_drag_handle(
    handler: impl Fn(&gpui::TouchDragEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    gpui::canvas(
        |bounds, window, cx| {
            (
                window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
                window.use_state(cx, |_, _| false),
            )
        },
        move |_, (hitbox, claimed), window, _| {
            window.on_mouse_event(move |event: &gpui::TouchDragEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                if event.phase == gpui::TouchPhase::Started {
                    if window.default_prevented()
                        || !hitbox.is_hovered_at(event.start_position, window)
                    {
                        return;
                    }
                    handler(event, window, cx);
                    claimed.update(cx, |claimed, _| *claimed = window.default_prevented());
                } else if *claimed.read(cx) {
                    handler(event, window, cx);
                    if matches!(
                        event.phase,
                        gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled
                    ) {
                        claimed.update(cx, |claimed, _| *claimed = false);
                    }
                }
            });
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

struct PaneDragState {
    drag: PaneDrag,
    window: zz_protocol::WindowId,
    layout: zz_protocol::LayoutNode,
    target: Option<(PaneId, DropZone)>,
    touch: bool,
    preview: Option<DropPreview>,
}

impl PaneDragState {
    fn target_at(
        &self,
        position: Point<Pixels>,
        bounds: &HashMap<PaneId, Bounds<Pixels>>,
    ) -> Option<(PaneId, DropZone)> {
        bounds.iter().find_map(|(pane, bounds)| {
            if *pane == self.drag.pane || !bounds.contains(&position) {
                return None;
            }
            let slot = PaneRect {
                x: f32::from(bounds.left()),
                y: f32::from(bounds.top()),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            };
            Some((
                *pane,
                coerced_drop_zone(
                    &self.layout,
                    self.drag.pane,
                    *pane,
                    drop_zone_at(slot, (f32::from(position.x), f32::from(position.y))),
                ),
            ))
        })
    }
}

struct PaneLayoutOverride {
    window: zz_protocol::WindowId,
    layout: zz_protocol::LayoutNode,
    generation: u64,
}

#[derive(Clone, Copy)]
struct SidebarResizeDrag;

struct SidebarResizePreview;

impl Render for SidebarResizePreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.0)).opacity(0.0)
    }
}

#[derive(Clone, Copy)]
struct SplitDrag {
    window: zz_protocol::WindowId,
    split: zz_protocol::SplitId,
    axis: zz_protocol::Axis,
    start_ratio: f32,
}

#[derive(Clone, Copy)]
struct SplitDragState {
    drag: SplitDrag,
    ratio: f32,
    touch: bool,
    committed_generation: Option<u64>,
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamped ratio is converted to a bounded protocol fixed-point value"
)]
fn split_ratio_basis(ratio: f32) -> u16 {
    (ratio.clamp(0.0, 1.0) * f32::from(zz_protocol::SPLIT_RATIO_BASIS)).round() as u16
}

struct SplitDragPreview;

impl Render for SplitDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.0)).opacity(0.0)
    }
}

fn update_split_ratio(node: &mut zz_protocol::LayoutNode, split: zz_protocol::SplitId, value: f32) {
    if let zz_protocol::LayoutNode::Split {
        id,
        ratio,
        first,
        second,
        ..
    } = node
    {
        if *id == split {
            *ratio = value;
        } else {
            update_split_ratio(first, split, value);
            update_split_ratio(second, split, value);
        }
    }
}

fn claims_daemon_prefix(
    options: &zz_protocol::MuxOptions,
    armed: bool,
    input: &zz_terminal::KeyInput,
) -> bool {
    let name = zz_protocol::input_key_name(input);
    armed
        || [
            zz_protocol::MuxOptionKey::Prefix,
            zz_protocol::MuxOptionKey::Prefix2,
        ]
        .into_iter()
        .filter_map(|key| options.get(key))
        .any(|option| zz_protocol::canonical_key(&option.value) == name.as_str())
}

fn pane_icon(kind: &PaneKindSnapshot) -> IconName {
    match kind {
        PaneKindSnapshot::Terminal => IconName::SquareTerminal,
        PaneKindSnapshot::Browser(_) => IconName::Globe,
        PaneKindSnapshot::Agent(agent) => zz_ui::pane::agent_provider_icon(agent.provider),
        PaneKindSnapshot::Editor(_) => IconName::File,
        PaneKindSnapshot::Picker => IconName::Plus,
    }
}

fn unsupported_pane(
    title: &str,
    icon: IconName,
    reason: &str,
    url: Option<String>,
    radii: Corners<Pixels>,
    cx: &App,
) -> AnyElement {
    let toolbar = (title == "Browser").then(|| {
        let control = |id, icon| {
            Button::compact_icon(id, icon)
                .disabled(true)
                .tooltip("Requires the desktop browser runtime")
        };
        zz_ui::browser::BrowserToolbar::new(
            control("web-browser-back", IconName::ArrowLeft),
            control("web-browser-forward", IconName::ArrowRight),
            control("web-browser-reload", IconName::Redo2),
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_size(px(12.0))
                .text_color(cx.theme().foreground.muted())
                .child(url.clone().unwrap_or_else(|| "Browser".into())),
            control("web-browser-picker", IconName::Inspector),
            control("web-browser-more", IconName::Ellipsis),
        )
        .into_any_element()
    });
    let message = div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(12.0))
        .p(px(24.0))
        .rounded_bl(radii.bottom_left)
        .rounded_br(radii.bottom_right)
        .when(toolbar.is_none(), |message| {
            message
                .rounded_tl(radii.top_left)
                .rounded_tr(radii.top_right)
        })
        .bg(cx
            .theme()
            .background
            .opaque()
            .opacity(if title == "Browser" {
                1.0
            } else {
                cx.theme().pane_background_opacity
            }))
        .child(Icon::new(icon).size(px(26.0)))
        .child(title.to_owned())
        .child(
            div()
                .max_w(px(400.0))
                .text_size(px(12.0))
                .text_color(cx.theme().foreground.muted())
                .child(reason.to_owned()),
        )
        .children(url.map(|url| {
            Button::new("web-open-browser-page")
                .small()
                .icon(IconName::ExternalLink)
                .label("Open in browser tab")
                .on_click(move |_, _, cx| cx.open_url(&url))
        }));
    div()
        .flex()
        .flex_col()
        .size_full()
        .children(toolbar.map(|toolbar| {
            div()
                .flex_none()
                .rounded_tl(radii.top_left)
                .rounded_tr(radii.top_right)
                .bg(cx
                    .theme()
                    .background
                    .opaque()
                    .opacity(cx.theme().pane_background_opacity))
                .child(toolbar)
        }))
        .child(message)
        .into_any_element()
}

fn buffer_row_text(item: &ChooseBufferItem, now: u64) -> (String, String, String, String) {
    if !item.text.is_empty() {
        return (
            item.text.clone(),
            String::new(),
            String::new(),
            String::new(),
        );
    }
    let bytes = item.size_bytes;
    let size = if bytes < 1_024 {
        format!("{bytes} B")
    } else {
        let (unit, label) = if bytes < 1_048_576 {
            (1_024, "KiB")
        } else if bytes < 1_073_741_824 {
            (1_048_576, "MiB")
        } else {
            (1_073_741_824, "GiB")
        };
        let tenths = bytes.saturating_mul(10).saturating_add(unit / 2) / unit;
        format!("{}.{:01} {label}", tenths / 10, tenths % 10)
    };
    let age = now.saturating_sub(item.created_unix_seconds);
    let age = match age {
        0..=59 => format!("{age}s"),
        60..=3_599 => format!("{}m", age / 60),
        3_600..=86_399 => format!("{}h", age / 3_600),
        _ => format!("{}d", age / 86_400),
    };
    (item.name.clone(), item.preview.clone(), size, age)
}

fn unix_seconds() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1_000.0) as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[cfg(test)]
mod prefix_tests {
    use super::claims_daemon_prefix;
    use zz_protocol::{MuxOptionKey, MuxOptionSource, MuxOptions};
    use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers};

    #[test]
    fn pane_drop_targets_follow_canvas_offset_and_pixel_aspect_ratio() {
        use gpui::{Bounds, point, px, size};
        use zz_client::DropZone;
        use zz_protocol::{Axis, LayoutNode, PaneId, SplitId, WindowId};
        let state = super::PaneDragState {
            drag: zz_ui::pane::PaneDrag {
                pane: PaneId(1),
                requires_prefix: false,
            },
            window: WindowId(1),
            layout: LayoutNode::Split {
                id: SplitId(1),
                axis: Axis::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Pane(PaneId(1))),
                second: Box::new(LayoutNode::Pane(PaneId(2))),
            },
            target: None,
            touch: false,
            preview: None,
        };
        let canvas = Bounds::new(point(px(250.0), px(40.0)), size(px(1000.0), px(400.0)));
        let bounds = [
            (
                PaneId(1),
                Bounds::new(canvas.origin, size(px(497.0), px(400.0))),
            ),
            (
                PaneId(2),
                Bounds::new(point(px(753.0), px(40.0)), size(px(497.0), px(400.0))),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            state.target_at(point(px(1000.0), px(240.0)), &bounds),
            Some((PaneId(2), DropZone::Center))
        );
        assert_eq!(
            state.target_at(point(px(1000.0), px(45.0)), &bounds),
            Some((PaneId(2), DropZone::Top))
        );
        assert_eq!(
            state.target_at(point(px(1240.0), px(240.0)), &bounds),
            Some((PaneId(2), DropZone::Right))
        );
        assert_eq!(state.target_at(point(px(500.0), px(240.0)), &bounds), None);
        assert_eq!(state.target_at(point(px(1300.0), px(240.0)), &bounds), None);
        assert_eq!(state.target_at(point(px(1000.0), px(20.0)), &bounds), None);
    }

    #[test]
    fn buffer_rows_show_compact_metadata_and_respect_custom_formats() {
        let mut item = zz_protocol::ChooseBufferItem {
            name: "buffer0".into(),
            preview: "hello".into(),
            size_bytes: 1_536,
            created_unix_seconds: 10,
            key: "0".into(),
            text: String::new(),
            tagged: true,
        };
        assert_eq!(
            super::buffer_row_text(&item, 80),
            (
                "buffer0".into(),
                "hello".into(),
                "1.5 KiB".into(),
                "1m".into()
            )
        );
        assert_eq!(super::buffer_row_text(&item, 0).3, "0s");
        item.text = "<<buffer0>>".into();
        assert_eq!(
            super::buffer_row_text(&item, 80),
            (
                "<<buffer0>>".into(),
                String::new(),
                String::new(),
                String::new()
            )
        );
    }

    fn input(character: char, control: bool, alt: bool) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key: KeyCode::Character(character),
            modifiers: Modifiers::new(false, control, alt, false),
            text: Some(character.to_string().into_boxed_str()),
            unshifted_codepoint: Some(character),
        }
    }

    #[test]
    fn prefix_capture_follows_both_live_options_and_armed_state() {
        let mut options = MuxOptions::default();
        options.set(
            MuxOptionKey::Prefix,
            "Ctrl-a",
            MuxOptionSource::RuntimeCommand,
        );
        options.set(
            MuxOptionKey::Prefix2,
            "Alt-Space",
            MuxOptionSource::RuntimeCommand,
        );

        assert!(claims_daemon_prefix(
            &options,
            false,
            &input('a', true, false)
        ));
        assert!(claims_daemon_prefix(
            &options,
            false,
            &input(' ', false, true)
        ));
        assert!(!claims_daemon_prefix(
            &options,
            false,
            &input('b', true, false)
        ));
        assert!(!claims_daemon_prefix(
            &options,
            false,
            &input('x', false, false)
        ));
        assert!(claims_daemon_prefix(
            &options,
            true,
            &input('x', false, false)
        ));

        options.set(
            MuxOptionKey::Prefix2,
            "none",
            MuxOptionSource::RuntimeCommand,
        );
        assert!(!claims_daemon_prefix(
            &options,
            false,
            &input(' ', false, true)
        ));
    }
}
