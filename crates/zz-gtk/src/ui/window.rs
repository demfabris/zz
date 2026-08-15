use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use adw::prelude::*;
use gtk::{gio, glib};
use zz_client::{ChromeAction, ViewportDamage};
use zz_protocol::{
    CommandInvocation, DisplayPanesAction, InputMessage, LayoutNode, PaneId, PaneKindSnapshot,
    SessionId, WindowId,
};
use zz_terminal::{ClipboardTarget, KeyAction, TerminalAppearance};

use crate::{
    engine::{Engine, EngineEvent, HostId, SessionView},
    ui::{
        keys,
        overlay::Overlays,
        // ── gtk-termux: terminal UX surfaces ──
        pager::OutputPager,
        pane::TerminalPane,
        panes::{PaneGrid, layout_panes},
        picker::PanePicker,
        prefix,
        settings::Settings,
        sidebar::{Hooks, NewSessionPanel, Sidebar},
        ssh_prompt,
        terminal::TerminalView,
        tray,
    },
};

const STYLE: &str = "
.zz-panes { background-color: @headerbar_bg_color; }
.zz-status { padding: 2px 10px; }
.zz-status label { font-size: 0.9em; }
.zz-placeholder { padding: 24px; }
.zz-prompt { padding: 4px 8px; }
.zz-chooser { padding: 12px; }
.zz-badge { padding: 2px 8px; border-radius: 6px; }
.zz-number { font-size: 2.4em; font-weight: bold; padding: 8px 20px; border-radius: 12px; }
.zz-prefix {
    background-color: @accent_bg_color;
    color: @accent_fg_color;
    border-radius: 9999px;
    padding: 1px 8px;
}
.zz-sidebar-row { padding: 2px 4px; min-height: 28px; }
.zz-sidebar-active label { font-weight: bold; }
.zz-sidebar-disclosure, .zz-sidebar-action { min-width: 20px; min-height: 20px; padding: 0; }
.zz-sidebar-grip { background-color: alpha(currentColor, 0.08); }
.zz-bell { color: @warning_color; font-size: 0.7em; }
.zz-newsession { padding: 24px; }
.zz-newsession-section { margin-top: 12px; }
.zz-newsession-keys { min-width: 92px; }
.zz-kbd {
    font-family: monospace;
    font-size: 0.85em;
    padding: 1px 6px;
    border-radius: 6px;
    background-color: alpha(currentColor, 0.1);
}
.zz-search { padding: 4px 6px; }
.zz-search entry.error { color: @error_color; }
.zz-marks { margin: 8px; }
.zz-picker { padding: 24px; }
.zz-link label { padding: 2px 6px; font-size: 0.85em; }
";

const WORKSPACE_PAGE: &str = "workspace";
const EMPTY_PAGE: &str = "empty";
const FONT_STEP_POINTS: f32 = 1.0;
const MIN_FONT_POINTS: f32 = 1.0;
const MAX_FONT_POINTS: f32 = 256.0;

enum PaneWidget {
    Terminal(Rc<TerminalPane>),
    // ── gtk-termux: an unclaimed split shows the kind chooser ──
    Picker(Rc<PanePicker>),
    // ── end gtk-termux ──
    Other {
        widget: gtk::Widget,
        kind: &'static str,
    },
}

impl PaneWidget {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Terminal(pane) => pane.widget(),
            Self::Picker(picker) => picker.widget(),
            Self::Other { widget, .. } => widget.clone(),
        }
    }

    fn matches(&self, kind: &'static str) -> bool {
        match self {
            Self::Terminal(_) => kind == "terminal",
            Self::Picker(_) => kind == "picker",
            Self::Other { kind: current, .. } => *current == kind,
        }
    }
}

/// The libadwaita shell: the focused zz window's panes, the session tree beside
/// them, and a status bar the daemon's clock drives without ever touching the
/// grid. Windows are switched from the tree, so the workspace shows one grid.
pub struct Shell {
    engine: Arc<Engine>,
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    toasts: adw::ToastOverlay,
    workspace: gtk::Stack,
    prefix: gtk::Label,
    sidebar: Rc<Sidebar>,
    empty: Rc<NewSessionPanel>,
    overlays: Rc<Overlays>,
    settings: Rc<Settings>,
    grid: PaneGrid,
    widgets: RefCell<HashMap<PaneId, PaneWidget>>,
    focused_pane: Cell<Option<PaneId>>,
    font_offset: Cell<f32>,
    numbering: Cell<bool>,
    /// The host the pane widgets belong to. Pane ids are per daemon, so a
    /// switch cannot reuse a single widget however well the ids line up.
    host: Cell<HostId>,
    detaching: Cell<bool>,
    pager: RefCell<Option<OutputPager>>,
}

impl Shell {
    pub fn build(app: &adw::Application, engine: Arc<Engine>) -> Rc<Self> {
        install_style();

        let title = adw::WindowTitle::new("zz", "");
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        let prefix = gtk::Label::builder().label("PREFIX").visible(false).build();
        prefix.add_css_class("caption-heading");
        prefix.add_css_class("zz-prefix");
        header.pack_end(
            &gtk::MenuButton::builder()
                .icon_name("open-menu-symbolic")
                .tooltip_text("Main Menu")
                .menu_model(&primary_menu())
                .build(),
        );
        header.pack_end(&prefix);

        let sidebar = Sidebar::build(Arc::clone(&engine));
        header.pack_start(&sidebar.toggle_button());

        let grid = PaneGrid::new();
        let empty = NewSessionPanel::new(Arc::clone(&engine));
        let workspace = gtk::Stack::new();
        workspace.add_named(&grid, Some(WORKSPACE_PAGE));
        workspace.add_named(empty.widget(), Some(EMPTY_PAGE));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&workspace));

        let settings = Settings::new(Arc::clone(&engine));

        let floating = gtk::Overlay::new();
        floating.set_child(Some(&toolbar));
        sidebar.set_content(&floating);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(sidebar.widget()));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1024)
            .default_height(680)
            .title("zz")
            .icon_name(super::APP_ID)
            .content(&toasts)
            .build();

        let overlays = Overlays::new(Arc::clone(&engine), &window);

        // >>> palette agent: the prefix claim has to be installed before any
        // other window controller so no widget can swallow the chord, and the
        // tray's close hook before the shell's so hiding wins over detaching.
        prefix::install(&window, Arc::clone(&engine));
        tray::install(&window, Arc::clone(&engine));
        floating.add_overlay(overlays.palette());
        // <<< palette agent

        let shell = Rc::new(Self {
            engine,
            window,
            title,
            toasts,
            workspace,
            prefix,
            sidebar,
            empty,
            overlays,
            settings,
            grid,
            widgets: RefCell::new(HashMap::new()),
            focused_pane: Cell::new(None),
            font_offset: Cell::new(0.0),
            numbering: Cell::new(false),
            host: Cell::new(HostId::LOCAL),
            detaching: Cell::new(false),
            pager: RefCell::new(None),
        });
        shell.grid.set_engine(Arc::clone(&shell.engine));
        let target = Rc::downgrade(&shell);
        shell.settings.attach_chrome(Rc::new(move |action| {
            if let Some(shell) = target.upgrade() {
                shell.perform(action);
            }
        }));
        shell.install_actions();
        shell.connect_signals();
        shell.connect_sidebar();
        shell.pump_events();
        shell.pump_ssh_prompts();
        shell.sync();
        shell.sidebar.refresh_status();
        shell
    }

    /// The tree hands back the chrome it cannot answer, and asks for the
    /// keyboard to go to the focused pane; both belong to the window.
    fn connect_sidebar(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        let chrome: Rc<dyn Fn(ChromeAction)> = Rc::new(move |action| {
            if let Some(shell) = target.upgrade() {
                shell.perform(action);
            }
        });
        let target = Rc::downgrade(self);
        let focus_pane: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(shell) = target.upgrade() {
                shell.refocus();
            }
        });
        self.sidebar.connect(Hooks { chrome, focus_pane });
    }

    pub fn present(&self) {
        self.window.present();
        if let Some(view) = self.engine.session_view() {
            self.focus_active(view.active_pane);
        }
    }

    /// The window's own verbs. They are deliberately menu-driven: a global
    /// accelerator would swallow a chord the daemon's key tables own, and the
    /// prefix table already spells every one of these for the keyboard.
    fn install_actions(self: &Rc<Self>) {
        for (name, run) in Self::verbs() {
            let action = gio::SimpleAction::new(name, None);
            let target = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(shell) = target.upgrade() {
                    run(&shell);
                }
            });
            self.window.add_action(&action);
        }

        let focus = gio::SimpleAction::new("focus-pane", Some(glib::VariantTy::STRING));
        let target = Rc::downgrade(self);
        focus.connect_activate(move |_, parameter| {
            let Some(shell) = target.upgrade() else {
                return;
            };
            let Some(direction) = parameter.and_then(glib::Variant::str) else {
                return;
            };
            shell.on_active_pane(|pane| {
                CommandInvocation::new("select-pane", [direction, "-t", &pane.to_string()])
            });
        });
        self.window.add_action(&focus);

        // The deep link a menu item, another surface or a script uses to open
        // preferences on one page, the way a desktop settings app does.
        let page = gio::SimpleAction::new("settings-page", Some(glib::VariantTy::STRING));
        let target = Rc::downgrade(self);
        page.connect_activate(move |_, parameter| {
            let (Some(shell), Some(name)) =
                (target.upgrade(), parameter.and_then(glib::Variant::str))
            else {
                return;
            };
            shell.settings.open_at(&shell.window, name);
        });
        self.window.add_action(&page);
    }

    fn verbs() -> [(&'static str, fn(&Rc<Self>)); 11] {
        [
            ("new-window", |shell| {
                shell.on_session(|session| {
                    CommandInvocation::new("new-window", ["-t", &session.to_string()])
                });
            }),
            ("close-window", |shell| {
                shell.on_focused_window(|window| {
                    CommandInvocation::new("kill-window", ["-t", &window.to_string()])
                });
            }),
            ("next-window", |shell| {
                shell
                    .engine
                    .execute(CommandInvocation::new("next-window", [] as [&str; 0]));
            }),
            ("previous-window", |shell| {
                shell
                    .engine
                    .execute(CommandInvocation::new("previous-window", [] as [&str; 0]));
            }),
            ("split-right", |shell| {
                shell.on_active_pane(|pane| {
                    CommandInvocation::new("new-pane", ["-h", "-t", &pane.to_string()])
                });
            }),
            ("split-down", |shell| {
                shell.on_active_pane(|pane| {
                    CommandInvocation::new("new-pane", ["-v", "-t", &pane.to_string()])
                });
            }),
            ("zoom-pane", |shell| {
                shell.on_active_pane(|pane| {
                    CommandInvocation::new("resize-pane", ["-Z", "-t", &pane.to_string()])
                });
            }),
            ("close-pane", |shell| {
                shell.on_active_pane(|pane| {
                    CommandInvocation::new("kill-pane", ["-t", &pane.to_string()])
                });
            }),
            ("detach", |shell| shell.detach()),
            ("about", |shell| shell.present_about()),
            ("settings", |shell| shell.settings.toggle(&shell.window)),
        ]
    }

    fn on_session(&self, command: impl FnOnce(SessionId) -> CommandInvocation) {
        if let Some(view) = self.engine.session_view() {
            self.engine.execute(command(view.session));
        }
    }

    fn on_focused_window(&self, command: impl FnOnce(WindowId) -> CommandInvocation) {
        if let Some(view) = self.engine.session_view() {
            self.engine.execute(command(view.focused_window));
        }
    }

    fn on_active_pane(&self, command: impl FnOnce(PaneId) -> CommandInvocation) {
        if let Some(view) = self.engine.session_view() {
            self.engine.execute(command(view.active_pane));
        }
    }

    /// About is the system dialog, as GNOME spells it. What a zz client is
    /// actually asked about — which daemon it reached, on what protocol, with
    /// what capabilities — rides along as the troubleshooting section rather
    /// than as a settings page of its own.
    fn present_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("zz")
            .application_icon(super::APP_ID)
            .developer_name("zz")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("A GNOME client for zz daemon sessions.")
            .website("https://zzmux.sh")
            .license_type(gtk::License::MitX11)
            .debug_info(self.daemon_facts())
            .debug_info_filename("zz-gtk.txt")
            .build();
        about.present(Some(&self.window));
    }

    fn daemon_facts(&self) -> String {
        let capabilities = self.engine.capabilities();
        format!(
            "zz-gtk {}\nendpoint: {}\nprotocol: {}\ncapabilities: {}\n",
            env!("CARGO_PKG_VERSION"),
            self.engine.endpoint(),
            zz_protocol::PROTOCOL_VERSION,
            if capabilities.is_empty() {
                "none advertised".to_owned()
            } else {
                capabilities.join(", ")
            },
        )
    }

    fn connect_signals(self: &Rc<Self>) {
        // The one-time import offer needs a window to sit over, so it waits for
        // the tree to be mapped rather than firing during construction.
        let target = Rc::downgrade(self);
        self.window.connect_map(move |window| {
            if let Some(shell) = target.upgrade() {
                shell.settings.prompt_import(window);
            }
        });

        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(shell) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if !shell.numbering.get() || keys::is_modifier(keyval) {
                return glib::Propagation::Proceed;
            }
            shell.engine.send(InputMessage::DisplayPanes {
                action: DisplayPanesAction::Key(keys::key_input(
                    KeyAction::Press,
                    keyval,
                    modifiers,
                    None,
                )),
            });
            glib::Propagation::Stop
        });
        self.window.add_controller(keyboard);

        let target = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(shell) = target.upgrade() {
                shell.leave();
                shell.engine.events().close();
            }
            glib::Propagation::Proceed
        });
    }

    /// Detach is the explicit "leave it running" verb, so it is remembered and
    /// the close that follows never escalates to stopping the daemon.
    fn detach(&self) {
        self.detaching.set(true);
        self.engine.detach();
        self.window.close();
    }

    /// Closing the window detaches, unless `quit-daemon-on-exit` says the
    /// daemon should go with it. The value is read from the file at this
    /// moment, so a hand edit a second ago already counts.
    /// It is the local daemon that key is about — a host's daemon belongs to
    /// the machine it runs on, and quitting a client here has no business
    /// stopping it — so the leave is aimed rather than sent to whichever host
    /// the workspace happens to be showing.
    fn leave(&self) {
        if !self.detaching.get() && self.settings.quit_daemon_on_exit() {
            self.engine.execute_on(
                HostId::LOCAL,
                CommandInvocation::new("kill-server", [] as [&str; 0]),
            );
            return;
        }
        self.engine.detach_all();
    }

    /// Engine events reach the main context through the channel the reader
    /// thread writes; closing it on window close is what lets this future — and
    /// with it the shell's last strong reference — finish.
    fn pump_events(self: &Rc<Self>) {
        let events = self.engine.events();
        let shell = Rc::clone(self);
        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                shell.handle(event);
            }
        });
    }

    /// ssh's questions, answered in a native dialog. Declining parks the host:
    /// the connect thread is blocked on this reply, and dialling again would
    /// only ask the same question one rung later.
    fn pump_ssh_prompts(self: &Rc<Self>) {
        let prompts = self.engine.ssh_prompts();
        let shell = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(request) = prompts.recv().await {
                let Some(shell) = shell.upgrade() else {
                    return;
                };
                ssh_prompt::present(&shell.window, &shell.engine, &request);
            }
        });
    }

    fn handle(self: &Rc<Self>, event: EngineEvent) {
        // The local daemon's events arrive bare; a host's carry the host they
        // happened on. Everything below this line is about the host the
        // workspace is showing, so a background host is answered separately —
        // a machine nobody is looking at must never close the window or move
        // the panes.
        let (host, event) = match event {
            EngineEvent::Fleet(host, inner) => (host, *inner),
            EngineEvent::HostState(host) => return self.handle_host_state(host),
            EngineEvent::FleetChanged => return self.sidebar.sync(),
            other => (HostId::LOCAL, other),
        };
        if host != self.engine.active_host() {
            return self.handle_background(host, event);
        }
        if host != HostId::LOCAL
            && let EngineEvent::Disconnected(reason) = &event
        {
            self.toasts
                .add_toast(adw::Toast::new(&format!("Host disconnected: {reason}")));
            self.sidebar.sync();
            return;
        }
        match event {
            EngineEvent::FramesReady => self.apply_frames(),
            EngineEvent::StatusChanged => self.sidebar.refresh_status(),
            EngineEvent::SnapshotChanged | EngineEvent::Attached(_) => self.sync(),
            EngineEvent::FocusSidebar => self.sidebar.focus(),
            EngineEvent::OverlaysChanged => self.refresh_overlays(),
            EngineEvent::AppearanceChanged => {
                self.push_appearance();
                self.settings.refresh_daemon_values();
            }
            EngineEvent::MuxOptionsChanged => self.settings.refresh_daemon_values(),
            EngineEvent::Clipboard { target, text } => self.write_clipboard(target, &text),
            // ── gtk-termux: terminal UX surfaces ──
            EngineEvent::CommandOutputChanged => self.sync_pager(),
            EngineEvent::BeginSearch { pane, direction } => {
                if let Some(PaneWidget::Terminal(surface)) = self.widgets.borrow().get(&pane) {
                    surface.open_search(direction, true);
                }
            }
            // The ring grew under a scroll that is already on screen; the next
            // notch will reach further back, so nothing has to repaint now.
            // The three fleet variants are unwrapped above and cannot arrive
            // here at all.
            EngineEvent::HistoryChanged(_)
            | EngineEvent::Fleet(..)
            | EngineEvent::HostState(_)
            | EngineEvent::FleetChanged => {}
            EngineEvent::OpenUri { uri, .. } => self.open_uri(&uri),
            // ── end gtk-termux ──
            EngineEvent::Notice(text) => self.toasts.add_toast(adw::Toast::new(&text)),
            EngineEvent::Reconnecting { attempt } => {
                self.overlays.dismiss();
                self.sidebar.sync();
                if attempt == 1 {
                    self.toasts
                        .add_toast(adw::Toast::new("Reconnecting to the zz daemon…"));
                }
            }
            EngineEvent::Reconnected => {
                self.sidebar.sync();
                self.settings.resend_overrides();
                self.toasts.add_toast(adw::Toast::new("Reconnected"));
            }
            // A session ending under the client is not the end of the client:
            // the tree is still there to attach somewhere else, and a daemon
            // with nothing left offers the new-session card. Quitting closes
            // the window itself, before the daemon ever answers.
            EngineEvent::Detached => {
                self.overlays.dismiss();
                self.toasts.add_toast(adw::Toast::new("Session ended"));
                self.sync();
            }
            EngineEvent::Disconnected(_) => {
                self.overlays.dismiss();
                self.window.close();
            }
        }
    }

    /// A host that is not on screen still moves the tree: sessions come and go
    /// there, and its bells still bubble up. Nothing else about it reaches the
    /// workspace.
    fn handle_background(self: &Rc<Self>, host: HostId, event: EngineEvent) {
        match event {
            EngineEvent::SnapshotChanged
            | EngineEvent::Attached(_)
            | EngineEvent::Detached
            | EngineEvent::Disconnected(_) => self.sidebar.sync(),
            // A message from a machine that is not on screen has to say which
            // machine, or "the zz daemon stopped" reads as the one in front of
            // you.
            EngineEvent::Notice(text) => {
                let named = self
                    .engine
                    .host_name(host)
                    .map_or(text.clone(), |name| format!("{name}: {text}"));
                self.toasts.add_toast(adw::Toast::new(&named));
            }
            _ => {}
        }
    }

    /// A connection state moved. The row repaints either way; the workspace
    /// re-reads itself only when it is that host's panes on screen.
    fn handle_host_state(self: &Rc<Self>, host: HostId) {
        self.sidebar.sync();
        if host == self.engine.active_host() {
            self.sync();
        }
    }

    fn apply_frames(&self) {
        let widgets = self.widgets.borrow();
        for frame in self.engine.take_frames() {
            if let Some(PaneWidget::Terminal(pane)) = widgets.get(&frame.pane) {
                pane.apply_frame(frame.viewport, &frame.damage);
            }
        }
    }

    fn refresh_overlays(self: &Rc<Self>) {
        self.prefix.set_visible(self.engine.prefix_armed());
        self.refresh_pane_numbers();
        self.overlays.sync();
        if !self.overlays.is_open() && !self.sidebar.has_focus() {
            self.refocus();
        }
    }

    /// `display-panes` is timed by the daemon, so the numbers appear and vanish
    /// with the state it publishes rather than a timer of the client's own.
    fn refresh_pane_numbers(&self) {
        let state = self.engine.display_panes();
        self.numbering.set(state.is_some());
        for (pane, widget) in self.widgets.borrow().iter() {
            if let PaneWidget::Terminal(surface) = widget {
                surface.set_number(state.as_ref().and_then(|state| {
                    state
                        .indicators
                        .iter()
                        .find(|indicator| indicator.pane == *pane)
                        .copied()
                }));
            }
        }
    }

    fn write_clipboard(&self, target: ClipboardTarget, text: &str) {
        if text.is_empty() {
            return;
        }
        let display = WidgetExt::display(&self.window);
        match target {
            ClipboardTarget::Clipboard => display.clipboard().set_text(text),
            ClipboardTarget::Primary => display.primary_clipboard().set_text(text),
        }
    }

    fn sync(self: &Rc<Self>) {
        self.sidebar.sync();
        let host = self.engine.active_host();
        if self.host.replace(host) != host {
            // Another machine's panes: nothing on screen can be reused, and a
            // pane id that happens to match belongs to a different terminal.
            self.widgets.borrow_mut().clear();
            self.focused_pane.set(None);
        }
        let Some(view) = self.engine.session_view() else {
            self.sync_empty();
            return;
        };
        self.workspace.set_visible_child_name(WORKSPACE_PAGE);
        self.title.set_title(&view.name);
        self.title.set_subtitle(&window_subtitle(&view));
        self.sync_panes(&view);
        if !self.sidebar.has_focus() {
            self.focus_active(view.active_pane);
        }
    }

    /// A daemon with no sessions left is not an error: the workspace becomes
    /// the card that offers the one action available there. An attachment that
    /// is merely in flight leaves the last workspace on screen.
    fn sync_empty(&self) {
        if !self.engine.snapshot().sessions.is_empty() {
            return;
        }
        self.empty.refresh();
        self.workspace.set_visible_child_name(EMPTY_PAGE);
        self.title.set_title("zz");
        self.title.set_subtitle("");
    }

    fn sync_panes(self: &Rc<Self>, view: &SessionView) {
        let layout = view
            .zoomed_pane
            .map_or_else(|| view.layout.clone(), LayoutNode::Pane);
        let placed = layout_panes(&layout);
        let appearance = self.appearance();
        let mut widgets = self.widgets.borrow_mut();
        widgets.retain(|pane, _| placed.contains(pane));

        let mut children = Vec::with_capacity(placed.len());
        for pane in &placed {
            let snapshot = view.panes.get(pane);
            let kind = snapshot.map_or("terminal", |snapshot| kind_label(&snapshot.kind));
            if !widgets.get(pane).is_some_and(|widget| widget.matches(kind)) {
                widgets.insert(*pane, self.make_widget(*pane, kind, &appearance));
            }
            if let Some(widget) = widgets.get(pane) {
                // ── gtk-termux: focus, zoom and synchronize-panes marks ──
                if let PaneWidget::Terminal(surface) = widget {
                    surface.set_marks(
                        *pane == view.active_pane,
                        view.zoomed_pane == Some(*pane),
                        snapshot.is_some_and(|snapshot| snapshot.synchronized_input),
                    );
                }
                // ── end gtk-termux ──
                children.push((*pane, widget.widget()));
            }
        }
        drop(widgets);
        self.grid.set_panes(
            view.focused_window,
            layout,
            view.zoomed_pane.is_some(),
            children,
        );
    }

    fn make_widget(
        self: &Rc<Self>,
        pane: PaneId,
        kind: &'static str,
        appearance: &TerminalAppearance,
    ) -> PaneWidget {
        if kind == "terminal" {
            let surface = TerminalPane::new(self.terminal_view(pane, appearance));
            if let Some(viewport) = self.engine.viewport(pane) {
                surface.apply_frame(viewport, &ViewportDamage::All);
            }
            return PaneWidget::Terminal(surface);
        }
        // ── gtk-termux: an unclaimed split chooses what it becomes ──
        if kind == "picker" {
            return PaneWidget::Picker(PanePicker::new(Arc::clone(&self.engine), pane));
        }
        // ── end gtk-termux ──
        let label = gtk::Label::new(Some(&format!("{kind} panes need the zz app")));
        label.add_css_class("dim-label");
        label.add_css_class("zz-placeholder");
        PaneWidget::Other {
            widget: label.upcast(),
            kind,
        }
    }

    fn terminal_view(
        self: &Rc<Self>,
        pane: PaneId,
        appearance: &TerminalAppearance,
    ) -> TerminalView {
        let target = Rc::downgrade(self);
        let chrome: Rc<dyn Fn(ChromeAction)> = Rc::new(move |action| {
            if let Some(shell) = target.upgrade() {
                shell.perform(action);
            }
        });
        TerminalView::new(Arc::clone(&self.engine), pane, appearance.clone(), chrome)
    }

    /// The chrome a terminal surface hands up: everything that belongs to the
    /// window rather than to one pane.
    fn perform(&self, action: ChromeAction) {
        match action {
            ChromeAction::Detach => self.detach(),
            ChromeAction::OpenSettings => self.settings.toggle(&self.window),
            ChromeAction::TerminalFontIncrease => self.adjust_font(FONT_STEP_POINTS),
            ChromeAction::TerminalFontDecrease => self.adjust_font(-FONT_STEP_POINTS),
            zoom @ (ChromeAction::UiZoomIn
            | ChromeAction::UiZoomOut
            | ChromeAction::UiZoomReset) => {
                if self.settings.adjust_zoom(zoom) {
                    self.push_appearance();
                }
            }
            ChromeAction::ToggleSidebar => self.sidebar.toggle(),
            other => log::debug!(
                "zz-gtk has no handler for the {} chrome action",
                other.name()
            ),
        }
    }

    /// Font size is a client-local offset on top of whatever the daemon
    /// resolved, so a config reload keeps the user's zoom.
    fn adjust_font(&self, delta: f32) {
        let base = self.engine.appearance().font_size_points;
        let next = (base + self.font_offset.get() + delta).clamp(MIN_FONT_POINTS, MAX_FONT_POINTS);
        self.font_offset.set(next - base);
        self.push_appearance();
    }

    /// The daemon's font size, plus the pane-local zoom offset, times the
    /// transient interface zoom. The grid caches its cell metrics and only
    /// re-measures when the appearance value changes, so the zoom has to arrive
    /// as a different point size rather than as a style rule.
    fn appearance(&self) -> TerminalAppearance {
        let mut appearance = self.engine.appearance();
        appearance.font_size_points = ((appearance.font_size_points + self.font_offset.get())
            * self.settings.zoom().scale())
        .clamp(MIN_FONT_POINTS, MAX_FONT_POINTS);
        appearance
    }

    fn push_appearance(&self) {
        let appearance = self.appearance();
        for widget in self.widgets.borrow().values() {
            if let PaneWidget::Terminal(pane) = widget {
                pane.view().set_appearance(appearance.clone());
            }
        }
        if let Some(pager) = self.pager.borrow().as_ref() {
            pager.set_appearance(appearance);
        }
    }

    /// Focus only sticks once the widget is realized, so the record is kept
    /// only when the grab actually took; the next sync retries otherwise.
    fn focus_active(&self, pane: PaneId) {
        if self.focused_pane.get() == Some(pane) {
            return;
        }
        // ── gtk-termux: the pager and an open find bar own the keyboard ──
        if self.pager.borrow().is_some() {
            return;
        }
        let took = match self.widgets.borrow().get(&pane) {
            Some(PaneWidget::Terminal(surface)) if surface.search_is_open() => true,
            Some(PaneWidget::Terminal(surface)) => surface.view().grab_focus(),
            Some(PaneWidget::Picker(picker)) => picker.grab_focus(),
            // ── end gtk-termux ──
            _ => false,
        };
        if took {
            self.focused_pane.set(Some(pane));
        }
    }

    /// An overlay took the keyboard away; the record has to be dropped or the
    /// pane never gets it back.
    fn refocus(&self) {
        self.focused_pane.set(None);
        if let Some(view) = self.engine.session_view() {
            self.focus_active(view.active_pane);
        }
    }
}

// ── gtk-termux: the command-output pager and link activation ──
impl Shell {
    /// Bring the pager in line with the core. The daemon owns its lifetime
    /// entirely — it opens it, feeds it, and closes it — so this only mirrors.
    fn sync_pager(self: &Rc<Self>) {
        let Some((pane, viewport)) = self.engine.command_output() else {
            if let Some(pager) = self.pager.borrow_mut().take() {
                pager.close();
            }
            self.refocus();
            return;
        };
        let stale = self
            .pager
            .borrow()
            .as_ref()
            .is_some_and(|pager| pager.pane() != pane);
        if stale && let Some(pager) = self.pager.borrow_mut().take() {
            pager.close();
        }
        let opened = self.pager.borrow().is_none();
        if opened {
            let pager = OutputPager::present(&self.engine, &self.window, pane, self.appearance());
            self.pager.replace(Some(pager));
            self.focused_pane.set(None);
        }
        if let Some(pager) = self.pager.borrow().as_ref() {
            pager.apply(viewport);
        }
    }

    /// The daemon resolved a modified click into a URI. This client has no
    /// browser panes to route into, so everything goes to the desktop's opener.
    fn open_uri(&self, uri: &str) {
        gtk::UriLauncher::new(uri).launch(
            Some(&self.window),
            gtk::gio::Cancellable::NONE,
            |result| {
                if let Err(error) = result {
                    log::warn!("zz-gtk could not open the link: {error}");
                }
            },
        );
    }
}
// ── end gtk-termux ──

fn primary_menu() -> gio::Menu {
    let windows = gio::Menu::new();
    windows.append(Some("New Tab"), Some("win.new-window"));
    windows.append(Some("Next Tab"), Some("win.next-window"));
    windows.append(Some("Previous Tab"), Some("win.previous-window"));
    windows.append(Some("Close Tab"), Some("win.close-window"));

    let focus = gio::Menu::new();
    for (label, direction) in [
        ("Left", "-L"),
        ("Right", "-R"),
        ("Up", "-U"),
        ("Down", "-D"),
    ] {
        focus.append(Some(label), Some(&format!("win.focus-pane('{direction}')")));
    }

    let panes = gio::Menu::new();
    panes.append(Some("Split Right"), Some("win.split-right"));
    panes.append(Some("Split Down"), Some("win.split-down"));
    panes.append(Some("Toggle Zoom"), Some("win.zoom-pane"));
    panes.append_submenu(Some("Focus Pane"), &focus);
    panes.append(Some("Close Pane"), Some("win.close-pane"));

    let session = gio::Menu::new();
    session.append(Some("Settings"), Some("win.settings"));
    session.append(Some("Detach"), Some("win.detach"));
    session.append(Some("About zz"), Some("win.about"));

    let menu = gio::Menu::new();
    menu.append_section(None, &windows);
    menu.append_section(None, &panes);
    menu.append_section(None, &session);
    menu
}

fn kind_label(kind: &PaneKindSnapshot) -> &'static str {
    match kind {
        PaneKindSnapshot::Terminal => "terminal",
        PaneKindSnapshot::Browser(_) => "browser",
        PaneKindSnapshot::Agent(_) => "agent",
        PaneKindSnapshot::Editor(_) => "editor",
        PaneKindSnapshot::Picker => "picker",
    }
}

/// The header's second line: the focused window, the way the session tree names
/// it. A window the daemon left unnamed is spelled by its index.
fn window_subtitle(view: &SessionView) -> String {
    view.windows
        .iter()
        .find(|window| window.id == view.focused_window)
        .map(|window| window_label(window.index, &window.name))
        .unwrap_or_default()
}

/// A window the daemon never named carries its index as its name, and a header
/// reading `0` under a session called `0` says nothing at all.
fn window_label(index: u32, name: &str) -> String {
    if name.is_empty() || name == index.to_string() {
        format!("Window {index}")
    } else {
        name.to_owned()
    }
}

fn install_style() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nameless_window_is_titled_by_its_index() {
        assert_eq!(window_label(3, ""), "Window 3");
        assert_eq!(window_label(3, "3"), "Window 3");
        assert_eq!(window_label(3, "build"), "build");
    }
}
