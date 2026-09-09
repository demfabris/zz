#[path = "agent_pane.rs"]
mod agent_pane;
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
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gpui::{
    AnyElement, App, Context, Corners, DragMoveEvent, Entity, FocusHandle, Focusable, IntoElement,
    KeyDownEvent, MouseButton, Render, ScrollStrategy, Subscription, UniformListScrollHandle,
    Window, div, prelude::*, px, relative, uniform_list,
};
use zz_client::{ChromeAction, ChromeKeymap, ChromeProfile, UI_TABLE, pane_rects};
use zz_protocol::{
    ChooseBufferAction, ChooseBufferItem, ChooseTreeAction, ChooseTreeKind, ChooseTreePaneKind,
    ConfirmAction, DisplayPanesAction, InputMessage, PaneId, PaneKindSnapshot, ProtocolMessage,
    WindowSnapshot,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Root, Selectable as _,
    Sizable as _,
    button::{Button, ButtonVariants as _},
    chooser::{
        ChooserDimensions, ChooserHint, ChooserModal, ChooserPaneKind, ChooserRowTheme,
        ChooserSearch, buffer_chooser_row, chooser_has_key_gutter, chooser_subtitle,
        tree_chooser_row,
    },
    navigation::{
        WORKSPACE_SIDEBAR_DEFAULT_WIDTH, workspace_chrome_controls, workspace_layout_button,
        workspace_settings_button, workspace_sidebar_surface, workspace_sidebar_titlebar,
    },
    pane::{PaneChrome, PaneSplitAxis, pane_border_color, pane_split_hit_target, pane_surface},
    settings::{SettingsSection, settings_navigation_button, settings_navigation_group_label},
    shell::{app_shell_surface, app_workspace_surface},
};

use crate::{command_palette::CommandPaletteView, connection::Connection, terminal::TerminalPane};
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

pub(crate) struct WebClient {
    connection: Entity<Connection>,
    connection_status: String,
    connected: bool,
    terminals: HashMap<PaneId, Entity<TerminalPane>>,
    waiting_panes: BTreeSet<PaneId>,
    agents: HashMap<PaneId, Entity<AgentPane>>,
    pickers: HashMap<PaneId, Entity<picker::PanePicker>>,
    sidebar: bool,
    settings: Option<SettingsSection>,
    preferences: settings::Preferences,
    settings_controls: settings::Controls,
    focus: FocusHandle,
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
    modal_open: bool,
    chooser_scroll: UniformListScrollHandle,
    chooser_selection: Option<(bool, u32)>,
    menu_selection: Option<usize>,
    focused_pane: Option<PaneId>,
    popup_terminal: Option<(PaneId, Entity<TerminalPane>)>,
    output_terminal: Option<(u64, Entity<TerminalPane>)>,
    split_drag: Option<SplitDragState>,
    _subscriptions: Vec<Subscription>,
}

impl WebClient {
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
                        this.focused_pane = None;
                        this.waiting_panes.clear();
                        this.popup_terminal = None;
                        this.output_terminal = None;
                        this.split_drag = None;
                    }
                    zz_client::CoreEvent::SnapshotChanged => {
                        if this.split_drag.is_some_and(|state| {
                            state.committed_generation.is_some_and(|generation| {
                                generation < connection.read(cx).core.snapshot().generation
                            })
                        }) {
                            this.split_drag = None;
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
                        zz_protocol::CommandResponse::Error { error, .. },
                    ) => {
                        use zz_ui::{WindowExt as _, notification::Notification};
                        window.push_notification(Notification::error(error.to_string()), cx);
                        this.split_drag = None;
                    }
                    zz_client::CoreEvent::FocusSidebar => this.focus_sidebar(window, cx),
                    zz_client::CoreEvent::CommandPromptChanged => {
                        this.prompt_revision = this.prompt_revision.wrapping_add(1);
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
        let preferences = settings::Preferences::load(cx);
        let appearance_view = cx.weak_entity();
        let appearance_observer = window.observe_window_appearance(move |window, cx| {
            let _ = appearance_view.update(cx, |this, cx| this.preferences.apply(window, cx));
        });
        let settings_controls = settings::Controls::new(&preferences, window, cx);
        let this = Self {
            connection,
            connection_status: String::new(),
            connected: false,
            terminals: HashMap::new(),
            waiting_panes: BTreeSet::new(),
            agents: HashMap::new(),
            pickers: HashMap::new(),
            sidebar: preferences.sidebar,
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
            chrome: ChromeKeymap::for_profile(ChromeProfile::Desktop),
            prompt: None,
            prompt_revision: 0,
            modal_open: false,
            chooser_scroll: UniformListScrollHandle::new(),
            chooser_selection: None,
            menu_selection: None,
            focused_pane: None,
            popup_terminal: None,
            output_terminal: None,
            split_drag: None,
            _subscriptions: vec![key_events, observer, events, appearance_observer],
        };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        this.connection.update(cx, Connection::start);
        this.preferences.apply(window, cx);
        this
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

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        self.preferences.sidebar = self.sidebar;
        self.preferences.save();
        cx.notify();
    }

    fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar = true;
        self.preferences.sidebar = true;
        self.preferences.save();
        self.settings = None;
        self.focused_pane = self.active_window(cx).map(|window| window.active_pane);
        self.sidebar_pointer_selection = false;
        self.sidebar_focus.focus(window, cx);
        sidebar::reconcile(self, window, cx);
        cx.notify();
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
        let input = crate::terminal::key_input(event);
        let core = &self.connection.read(cx).core;
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
            Some(ChromeAction::OpenSettings) => {
                self.settings = Some(SettingsSection::Appearance);
                cx.notify();
            }
            Some(ChromeAction::ToggleSidebar) => self.toggle_sidebar(cx),
            Some(ChromeAction::UiZoomIn) => {
                self.preferences.change_zoom(0.1, window, cx);
                self.sync_zoom_input(window, cx);
            }
            Some(ChromeAction::UiZoomOut) => {
                self.preferences.change_zoom(-0.1, window, cx);
                self.sync_zoom_input(window, cx);
            }
            Some(ChromeAction::UiZoomReset) => {
                self.preferences.reset_zoom(window, cx);
                self.sync_zoom_input(window, cx);
            }
            Some(ChromeAction::ClosePane) => {
                if self.settings.take().is_none()
                    && self.connection.read(cx).connected
                    && !self.connection.read(cx).core.attached_read_only()
                {
                    self.command("kill-pane", Vec::new(), cx);
                }
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
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
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)));
        workspace_chrome_controls(settings, Some(layout.into_any_element())).into_any_element()
    }

    fn sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        sidebar::reconcile(self, window, cx);
        let navigation = if let Some(selected) = self.settings {
            let mut rows = Vec::new();
            let mut previous_group = None;
            for section in SettingsSection::ALL {
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
                .gap(px(2.0))
                .px(px(8.0))
                .child(
                    Button::new("web-settings-back")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
                        .label("Back to workspace")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings = None;
                            this.focused_pane = None;
                            cx.notify();
                        })),
                )
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
        workspace_sidebar_surface(
            "web-sidebar-surface",
            WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
            workspace_sidebar_titlebar("web-sidebar-titlebar", self.controls(cx), cx),
            navigation,
            cx,
        )
        .track_focus(&self.sidebar_focus)
        .into_any_element()
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        status_bar::render(self, cx)
    }

    fn workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(section) = self.settings {
            return self.render_settings(section, window, cx);
        }
        let Some(active_window) = self.active_window(cx) else {
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
        let mut layout = active_window.zoomed_pane.map_or_else(
            || active_window.layout.clone(),
            zz_protocol::LayoutNode::Pane,
        );
        if let Some(state) = self
            .split_drag
            .filter(|state| state.drag.window == active_window.id)
        {
            update_split_ratio(&mut layout, state.drag.split, state.ratio);
        }
        let rects = pane_rects(&layout);
        let mut dividers = Vec::new();
        collect_split_bounds(&layout, &rects, &mut dividers);
        let core = &self.connection.read(cx).core;
        let can_focus = !self.sidebar_focus.is_focused(window)
            && core.choose_tree().is_none()
            && core.choose_buffer().is_none()
            && core.command_prompt().is_none()
            && core.menu().is_none()
            && core.confirm().is_none()
            && core.popup().is_none()
            && core.command_output().is_none();
        let existing: Vec<_> = self
            .connection
            .read(cx)
            .core
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .flat_map(|window| window.panes.keys().copied())
            .collect();
        self.terminals.retain(|pane, _| existing.contains(pane));
        self.waiting_panes.retain(|pane| existing.contains(pane));
        self.agents.retain(|pane, _| existing.contains(pane));
        self.pickers.retain(|pane, _| existing.contains(pane));
        let mut panes = Vec::new();
        for (pane_id, rect) in rects {
            let Some(pane) = active_window.panes.get(&pane_id) else {
                continue;
            };
            let content = match &pane.kind {
                PaneKindSnapshot::Terminal => {
                    let terminal = self.terminals.entry(pane_id).or_insert_with(|| {
                        let connection = self.connection.clone();
                        cx.new(|cx| TerminalPane::new(pane_id, connection, cx))
                    });
                    if can_focus
                        && pane_id == active_window.active_pane
                        && self.focused_pane != Some(pane_id)
                    {
                        terminal.read(cx).focus_handle(cx).focus(window, cx);
                        self.focused_pane = Some(pane_id);
                    }
                    terminal.clone().into_any_element()
                }
                PaneKindSnapshot::Agent(descriptor) => {
                    let agent = self.agents.entry(pane_id).or_insert_with(|| {
                        let connection = self.connection.clone();
                        cx.new(|cx| {
                            AgentPane::new(pane_id, descriptor.clone(), connection, window, cx)
                        })
                    });
                    agent.update(cx, |agent, cx| agent.update_descriptor(descriptor, cx));
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
                    cx,
                ),
                PaneKindSnapshot::Editor(_) => unsupported_pane(
                    "Editor",
                    IconName::File,
                    "The daemon does not share editor file contents with browser clients yet.",
                    None,
                    cx,
                ),
                PaneKindSnapshot::Picker => {
                    let picker = self.pickers.entry(pane_id).or_insert_with(|| {
                        let connection = self.connection.clone();
                        cx.new(|cx| picker::PanePicker::new(pane_id, connection, cx))
                    });
                    if can_focus
                        && pane_id == active_window.active_pane
                        && self.focused_pane != Some(pane_id)
                    {
                        picker.read(cx).focus_handle(cx).focus(window, cx);
                        self.focused_pane = Some(pane_id);
                    }
                    picker.clone().into_any_element()
                }
            };
            let margin = if self.preferences.gaps {
                self.preferences.pane_margin
            } else {
                0.5
            };
            let radii = Corners::all(if self.preferences.gaps {
                px(self.preferences.pane_radius)
            } else {
                px(0.0)
            });
            let chrome = PaneChrome::new(
                radii,
                px(if self.preferences.gaps {
                    self.preferences.pane_border_width
                } else {
                    0.5
                }),
                pane_border_color(active_window.active_pane == pane_id, cx),
                self.preferences.gaps,
            )
            .active(active_window.active_pane == pane_id)
            .dimmed(
                active_window.active_pane != pane_id,
                self.preferences.pane_inactive_opacity,
            );
            let mut status_tags = Vec::new();
            if pane.dead {
                let label = pane
                    .dead_status
                    .map_or_else(|| "dead".to_owned(), |status| format!("dead ({status})"));
                status_tags.push(zz_ui::pane::pane_waiting_state(label).into_any_element());
            }
            if matches!(&pane.kind, PaneKindSnapshot::Terminal)
                && self.connection.read(cx).core.viewport(pane_id).is_none()
            {
                self.waiting_panes.insert(pane_id);
                status_tags.push(
                    zz_ui::pane::pane_waiting_state(format!("waiting for {pane_id}"))
                        .into_any_element(),
                );
            }
            if pane.synchronized_input {
                status_tags.push(zz_ui::pane::pane_sync_badge(cx).into_any_element());
            }
            if active_window.zoomed_pane == Some(pane_id) {
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
            let status_overlays = if status_tags.is_empty() {
                Vec::new()
            } else {
                vec![
                    zz_ui::pane::pane_overlay_stack(
                        zz_ui::pane::PaneOverlayCorner::TopRight,
                        status_tags,
                    )
                    .into_any_element(),
                ]
            };
            panes.push(
                div()
                    .absolute()
                    .left(relative(rect.x))
                    .top(relative(rect.y))
                    .w(relative(rect.width))
                    .h(relative(rect.height))
                    .pl(px(if rect.x <= 0.0 { margin } else { margin / 2.0 }))
                    .pt(px(if rect.y <= 0.0 { margin } else { margin / 2.0 }))
                    .pr(px(if rect.x + rect.width >= 1.0 {
                        margin
                    } else {
                        margin / 2.0
                    }))
                    .pb(px(if rect.y + rect.height >= 1.0 {
                        margin
                    } else {
                        margin / 2.0
                    }))
                    .child(
                        pane_surface(
                            ("web-pane", pane_id.0),
                            content,
                            status_overlays,
                            chrome,
                            cx,
                        )
                        .capture_any_mouse_down(cx.listener(
                            move |this, _, window, cx| {
                                if this
                                    .active_window(cx)
                                    .is_some_and(|active| active.active_pane != pane_id)
                                {
                                    this.focused_pane = Some(pane_id);
                                    if let Some(agent) = this.agents.get(&pane_id) {
                                        agent.read(cx).focus_handle(cx).focus(window, cx);
                                    }
                                    this.command(
                                        "select-pane",
                                        vec!["-t".into(), format!("%{}", pane_id.0)],
                                        cx,
                                    );
                                }
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }
        if self.connection.read(cx).connected && !self.connection.read(cx).core.attached_read_only()
        {
            for (split, axis, ratio, rect) in dividers {
                let drag = SplitDrag {
                    window: active_window.id,
                    split,
                    axis,
                    start_ratio: ratio,
                };
                panes.push(
                    div()
                        .id(("web-split-bounds", split.0))
                        .absolute()
                        .left(relative(rect.x))
                        .top(relative(rect.y))
                        .w(relative(rect.width))
                        .h(relative(rect.height))
                        .child(
                            pane_split_hit_target(
                                ("web-split-divider", split.0),
                                match axis {
                                    zz_protocol::Axis::Horizontal => PaneSplitAxis::Horizontal,
                                    zz_protocol::Axis::Vertical => PaneSplitAxis::Vertical,
                                },
                                ratio,
                                px(if self.preferences.gaps { 6.0 } else { 1.0 }),
                            )
                            .on_drag(drag, |_: &SplitDrag, _, _, cx| cx.new(|_| SplitDragPreview)),
                        )
                        .on_drag_move::<SplitDrag>(cx.listener(
                            move |this, event: &DragMoveEvent<SplitDrag>, _, cx| {
                                let drag = *event.drag(cx);
                                if drag.split != split
                                    || this
                                        .split_drag
                                        .is_some_and(|state| state.committed_generation.is_some())
                                {
                                    return;
                                }
                                let (offset, size) = match drag.axis {
                                    zz_protocol::Axis::Horizontal => (
                                        event.event.position.x - event.bounds.left(),
                                        event.bounds.size.width,
                                    ),
                                    zz_protocol::Axis::Vertical => (
                                        event.event.position.y - event.bounds.top(),
                                        event.bounds.size.height,
                                    ),
                                };
                                let ratio = (f32::from(offset) / f32::from(size).max(1.0))
                                    .clamp(0.01, 0.99);
                                this.split_drag = Some(SplitDragState {
                                    drag,
                                    ratio,
                                    committed_generation: None,
                                });
                                cx.notify();
                                cx.stop_propagation();
                            },
                        ))
                        .into_any_element(),
                );
            }
        }
        if let Some(display) = self.connection.read(cx).core.display_panes() {
            for indicator in &display.indicators {
                let Some((_, rect)) = pane_rects(&layout)
                    .into_iter()
                    .find(|(pane, _)| *pane == indicator.pane)
                else {
                    continue;
                };
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
                    div()
                        .absolute()
                        .top(px(8.0))
                        .left(px(8.0))
                        .right(px(8.0))
                        .overflow_hidden()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .text_color(cx.theme().foreground)
                        .child(indicator.label.clone())
                });
                panes.push(
                    div()
                        .absolute()
                        .left(relative(rect.x))
                        .top(relative(rect.y))
                        .w(relative(rect.width))
                        .h(relative(rect.height))
                        .child(zz_ui::pane::pane_indicator_overlay(card).children(label))
                        .into_any_element(),
                );
            }
        }
        if !self.connection.read(cx).connected {
            panes.push(
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
        div()
            .relative()
            .size_full()
            .children(panes)
            .into_any_element()
    }

    fn overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let core = &self.connection.read(cx).core;
        let tree = core.choose_tree().cloned();
        let buffer = core.choose_buffer().cloned();
        let prompt = core.command_prompt().cloned();
        if prompt.is_none() {
            self.prompt = None;
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
            let pane = self.active_window(cx).map(|window| window.active_pane);
            let opened = self.prompt.is_none();
            let palette = self.prompt.get_or_insert_with(|| {
                let connection = self.connection.clone();
                cx.new(|cx| {
                    CommandPaletteView::new(
                        connection,
                        pane,
                        &state,
                        self.prompt_revision,
                        Arc::clone(&snapshot),
                        window,
                        cx,
                    )
                })
            });
            palette.update(cx, |palette, cx| {
                palette.synchronize(&state, self.prompt_revision, &snapshot, pane, window, cx);
            });
            if opened {
                palette.read(cx).focus(cx).focus(window, cx);
            }
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
        let (terminal, title, close) = if let Some((id, pane)) = output {
            if self
                .output_terminal
                .as_ref()
                .is_none_or(|(previous, _)| *previous != id)
            {
                let connection = self.connection.clone();
                let terminal = cx.new(|cx| TerminalPane::new_command_output(pane, connection, cx));
                terminal.read(cx).focus_handle(cx).focus(window, cx);
                self.output_terminal = Some((id, terminal));
            }
            (
                self.output_terminal.as_ref()?.1.clone(),
                "Command output".to_owned(),
                InputMessage::CommandOutputView {
                    action: zz_terminal::TerminalViewAction::CopyMode(
                        zz_terminal::CopyModeAction::Cancel,
                    ),
                },
            )
        } else {
            if self.popup_terminal.take().is_some() || self.output_terminal.take().is_some() {
                self.focused_pane = None;
                cx.notify();
            }
            return None;
        };
        let connection = self.connection.clone();
        let content = div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(32.0))
                    .items_center()
                    .px(px(8.0))
                    .child(div().flex_1().text_size(px(12.0)).child(title))
                    .child(
                        Button::compact_icon("web-terminal-overlay-close", IconName::Xmark)
                            .tooltip("Close")
                            .on_click(move |_, _, cx| {
                                connection.update(cx, |connection, cx| {
                                    connection.send(ProtocolMessage::Input(close.clone()), cx);
                                });
                            }),
                    ),
            )
            .child(div().flex_1().min_h_0().child(terminal));
        let frame = div()
            .absolute()
            .left(px(32.0))
            .right(px(32.0))
            .top(px(48.0))
            .bottom(px(32.0))
            .child(zz_ui::pane::FloatingSurface::new(
                "web-terminal-overlay",
                content,
                cx,
            ));
        Some(
            div()
                .absolute()
                .inset_0()
                .bg(cx.theme().scrim.opacity(0.25))
                .occlude()
                .child(frame)
                .into_any_element(),
        )
    }
}

impl Render for WebClient {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = if self.sidebar || self.settings.is_some() {
            self.sidebar(window, cx)
        } else {
            div().into_any_element()
        };
        let titlebar = (self.settings.is_none()).then(|| self.status_bar(cx));
        let workspace = self.workspace(window, cx);
        let mut terminal_overlays = self
            .terminal_overlay(window, cx)
            .into_iter()
            .collect::<Vec<_>>();
        terminal_overlays.extend(self.floating_overlay(window, cx));
        let mut overlays = self.overlay(window, cx).into_iter().collect::<Vec<_>>();
        overlays.extend(Root::render_dialog_layer(window, cx).map(IntoElement::into_any_element));
        overlays
            .extend(Root::render_notification_layer(window, cx).map(IntoElement::into_any_element));
        app_shell_surface(
            "web-client",
            cx.theme().background,
            sidebar,
            titlebar,
            app_workspace_surface("web-workspace", workspace, terminal_overlays, cx),
            overlays,
        )
        .track_focus(&self.focus)
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                let Some(mut state) = this.split_drag else {
                    return;
                };
                if state.committed_generation.is_some() {
                    return;
                }
                let ratio_basis_points = (state.ratio * 10_000.0).round() as u16;
                if ratio_basis_points == (state.drag.start_ratio * 10_000.0).round() as u16 {
                    this.split_drag = None;
                } else {
                    state.committed_generation =
                        Some(this.connection.read(cx).core.snapshot().generation);
                    this.split_drag = Some(state);
                    this.send_input(
                        InputMessage::ResizeSplit {
                            window: state.drag.window,
                            split: state.drag.split,
                            ratio_basis_points,
                        },
                        cx,
                    );
                }
                cx.notify();
            }),
        )
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
    committed_generation: Option<u64>,
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

fn collect_split_bounds(
    node: &zz_protocol::LayoutNode,
    panes: &[(PaneId, zz_client::NormalizedPaneRect)],
    splits: &mut Vec<(
        zz_protocol::SplitId,
        zz_protocol::Axis,
        f32,
        zz_client::NormalizedPaneRect,
    )>,
) -> zz_client::NormalizedPaneRect {
    match node {
        zz_protocol::LayoutNode::Pane(id) => panes
            .iter()
            .find(|(pane, _)| pane == id)
            .map_or(zz_client::NormalizedPaneRect::default(), |(_, rect)| *rect),
        zz_protocol::LayoutNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let first = collect_split_bounds(first, panes, splits);
            let second = collect_split_bounds(second, panes, splits);
            let x = first.x.min(second.x);
            let y = first.y.min(second.y);
            let rect = zz_client::NormalizedPaneRect {
                x,
                y,
                width: (first.x + first.width).max(second.x + second.width) - x,
                height: (first.y + first.height).max(second.y + second.height) - y,
            };
            splits.push((*id, *axis, *ratio, rect));
            rect
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
        PaneKindSnapshot::Agent(_) => IconName::RobotFace,
        PaneKindSnapshot::Editor(_) => IconName::File,
        PaneKindSnapshot::Picker => IconName::Plus,
    }
}

fn unsupported_pane(
    title: &str,
    icon: IconName,
    reason: &str,
    url: Option<String>,
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

    #[test]
    fn nested_dividers_follow_shared_pane_geometry_after_a_resize() {
        use zz_protocol::{Axis, LayoutNode, PaneId, SplitId};
        let mut layout = LayoutNode::Split {
            id: SplitId(1),
            axis: Axis::Horizontal,
            ratio: 0.25,
            first: Box::new(LayoutNode::Pane(PaneId(1))),
            second: Box::new(LayoutNode::Split {
                id: SplitId(2),
                axis: Axis::Vertical,
                ratio: 0.6,
                first: Box::new(LayoutNode::Pane(PaneId(2))),
                second: Box::new(LayoutNode::Pane(PaneId(3))),
            }),
        };
        super::update_split_ratio(&mut layout, SplitId(1), 0.4);
        let panes = zz_client::pane_rects(&layout);
        let mut splits = Vec::new();
        super::collect_split_bounds(&layout, &panes, &mut splits);
        let (_, axis, ratio, bounds) = splits.iter().find(|(id, ..)| *id == SplitId(2)).unwrap();
        assert_eq!(*axis, Axis::Vertical);
        assert_eq!(*ratio, 0.6);
        assert_eq!(
            *bounds,
            zz_client::NormalizedPaneRect {
                x: 0.4,
                y: 0.0,
                width: 0.6,
                height: 1.0
            }
        );
        assert_eq!(
            splits.last().unwrap().3,
            zz_client::NormalizedPaneRect::FULL
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
