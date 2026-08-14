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
    engine::{Engine, EngineEvent, SessionView},
    ui::{
        keys,
        overlay::Overlays,
        pane::TerminalPane,
        panes::{PaneGrid, layout_panes},
        terminal::TerminalView,
    },
};

const STYLE: &str = "
.zz-panes { background-color: @headerbar_bg_color; }
.zz-status { padding: 2px 10px; }
.zz-status label { font-size: 0.9em; }
.zz-placeholder { padding: 24px; }
.zz-prompt { padding: 4px 8px; }
.zz-chooser { padding: 12px; }
.zz-badge { margin: 8px; padding: 2px 8px; border-radius: 6px; }
.zz-number { font-size: 2.4em; font-weight: bold; padding: 8px 20px; border-radius: 12px; }
.zz-prefix {
    background-color: @accent_bg_color;
    color: @accent_fg_color;
    border-radius: 9999px;
    padding: 1px 8px;
}
";

const FONT_STEP_POINTS: f32 = 1.0;
const MIN_FONT_POINTS: f32 = 1.0;
const MAX_FONT_POINTS: f32 = 256.0;

enum PaneWidget {
    Terminal(TerminalPane),
    Other {
        widget: gtk::Widget,
        kind: &'static str,
    },
}

impl PaneWidget {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Terminal(pane) => pane.widget(),
            Self::Other { widget, .. } => widget.clone(),
        }
    }

    fn matches(&self, kind: &'static str) -> bool {
        match self {
            Self::Terminal(_) => kind == "terminal",
            Self::Other { kind: current, .. } => *current == kind,
        }
    }
}

/// The libadwaita shell: one tab per zz window, the focused window's panes laid
/// out underneath, and a status bar the daemon's clock drives without ever
/// touching the grid.
pub struct Shell {
    engine: Arc<Engine>,
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    toasts: adw::ToastOverlay,
    tabs: adw::TabView,
    prefix: gtk::Label,
    status_bar: gtk::CenterBox,
    status_left: gtk::Label,
    status_right: gtk::Label,
    overlays: Rc<Overlays>,
    grid: PaneGrid,
    widgets: RefCell<HashMap<PaneId, PaneWidget>>,
    pages: RefCell<Vec<(WindowId, adw::TabPage)>>,
    grid_host: Cell<Option<WindowId>>,
    focused_pane: Cell<Option<PaneId>>,
    font_offset: Cell<f32>,
    numbering: Cell<bool>,
    syncing: Cell<bool>,
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

        let tabs = adw::TabView::new();
        let tab_bar = adw::TabBar::builder().view(&tabs).autohide(false).build();
        tab_bar.set_end_action_widget(Some(
            &gtk::Button::builder()
                .icon_name("tab-new-symbolic")
                .tooltip_text("New Tab")
                .action_name("win.new-window")
                .has_frame(false)
                .build(),
        ));

        let status_left = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let status_right = gtk::Label::builder()
            .xalign(1.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        status_right.add_css_class("dim-label");
        let status_bar = gtk::CenterBox::builder().visible(false).build();
        status_bar.add_css_class("toolbar");
        status_bar.add_css_class("zz-status");
        status_bar.set_start_widget(Some(&status_left));
        status_bar.set_end_widget(Some(&status_right));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&tab_bar);
        toolbar.set_content(Some(&tabs));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&toolbar));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1024)
            .default_height(680)
            .title("zz")
            .icon_name(super::APP_ID)
            .content(&toasts)
            .build();

        let overlays = Overlays::new(Arc::clone(&engine), &window);
        toolbar.add_bottom_bar(overlays.prompt_bar());
        toolbar.add_bottom_bar(&status_bar);

        let shell = Rc::new(Self {
            engine,
            window,
            title,
            toasts,
            tabs,
            prefix,
            status_bar,
            status_left,
            status_right,
            overlays,
            grid: PaneGrid::new(),
            widgets: RefCell::new(HashMap::new()),
            pages: RefCell::new(Vec::new()),
            grid_host: Cell::new(None),
            focused_pane: Cell::new(None),
            font_offset: Cell::new(0.0),
            numbering: Cell::new(false),
            syncing: Cell::new(false),
        });
        shell.install_actions();
        shell.connect_signals();
        shell.pump_events();
        shell.sync();
        shell.refresh_status();
        shell
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
    }

    fn verbs() -> [(&'static str, fn(&Rc<Self>)); 10] {
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
            ("detach", |shell| {
                shell.engine.detach();
                shell.window.close();
            }),
            ("about", |shell| shell.present_about()),
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

    fn present_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("zz")
            .application_icon(super::APP_ID)
            .developer_name("zz")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("A GNOME client for zz daemon sessions.")
            .website("https://zzmux.sh")
            .license_type(gtk::License::MitX11)
            .build();
        about.present(Some(&self.window));
    }

    fn connect_signals(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        self.tabs.connect_selected_page_notify(move |tabs| {
            let Some(shell) = target.upgrade() else {
                return;
            };
            if shell.syncing.get() {
                return;
            }
            let Some(page) = tabs.selected_page() else {
                return;
            };
            let selected = shell
                .pages
                .borrow()
                .iter()
                .find(|(_, candidate)| *candidate == page)
                .map(|(window, _)| *window);
            if let Some(window) = selected {
                shell.engine.select_window(window);
            }
        });

        let target = Rc::downgrade(self);
        self.tabs.connect_close_page(move |tabs, page| {
            let Some(shell) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            shell.request_close(page);
            tabs.close_page_finish(page, false);
            glib::Propagation::Stop
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
                shell.engine.detach();
                shell.engine.events().close();
            }
            glib::Propagation::Proceed
        });
    }

    /// The daemon owns window lifetime, so the tab's close button asks it to
    /// kill the window and lets the snapshot that comes back remove the tab.
    /// Closing the page locally would leave the strip disagreeing with the mux.
    fn request_close(&self, page: &adw::TabPage) {
        let closing = self
            .pages
            .borrow()
            .iter()
            .find(|(_, candidate)| candidate == page)
            .map(|(window, _)| *window);
        if let Some(window) = closing {
            self.engine.execute(CommandInvocation::new(
                "kill-window",
                ["-t", &window.to_string()],
            ));
        }
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

    fn handle(self: &Rc<Self>, event: EngineEvent) {
        match event {
            EngineEvent::FramesReady => self.apply_frames(),
            EngineEvent::StatusChanged => self.refresh_status(),
            EngineEvent::SnapshotChanged | EngineEvent::Attached(_) => self.sync(),
            EngineEvent::OverlaysChanged => self.refresh_overlays(),
            EngineEvent::AppearanceChanged => self.push_appearance(),
            EngineEvent::Clipboard { target, text } => self.write_clipboard(target, &text),
            EngineEvent::Notice(text) => self.toasts.add_toast(adw::Toast::new(&text)),
            EngineEvent::Reconnecting { attempt } => {
                self.overlays.dismiss();
                if attempt == 1 {
                    self.toasts
                        .add_toast(adw::Toast::new("Reconnecting to the zz daemon…"));
                }
            }
            EngineEvent::Reconnected => {
                self.toasts.add_toast(adw::Toast::new("Reconnected"));
            }
            EngineEvent::Detached | EngineEvent::Disconnected(_) => {
                self.overlays.dismiss();
                self.window.close();
            }
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

    /// The status line carries a clock and republishes about once a second, so
    /// it may never reach the grid. An empty line is the daemon's default and
    /// stays hidden rather than showing an empty bar.
    fn refresh_status(&self) {
        let status = self.engine.status();
        self.status_bar.set_visible(!status.is_empty());
        self.status_left.set_text(&status.left);
        self.status_right.set_text(&status.right);
    }

    fn refresh_overlays(self: &Rc<Self>) {
        self.prefix.set_visible(self.engine.prefix_armed());
        self.refresh_pane_numbers();
        self.overlays.sync();
        if !self.overlays.is_open() {
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
        let Some(view) = self.engine.session_view() else {
            return;
        };
        self.title.set_title(&view.name);
        self.title.set_subtitle(&window_subtitle(&view));
        self.sync_panes(&view);
        self.sync_tabs(&view);
        self.focus_active(view.active_pane);
    }

    fn sync_tabs(&self, view: &SessionView) {
        self.syncing.set(true);
        let current: Vec<WindowId> = self
            .pages
            .borrow()
            .iter()
            .map(|(window, _)| *window)
            .collect();
        let desired: Vec<WindowId> = view.windows.iter().map(|window| window.id).collect();
        if current == desired {
            for ((_, page), tab) in self.pages.borrow().iter().zip(&view.windows) {
                page.set_title(&tab_title(tab.index, &tab.name));
            }
        } else {
            self.rebuild_tabs(view);
        }
        let selected = self
            .pages
            .borrow()
            .iter()
            .find(|(window, _)| *window == view.focused_window)
            .map(|(_, page)| page.clone());
        if let Some(page) = selected {
            self.host_grid(view.focused_window, &page);
            self.tabs.set_selected_page(&page);
        }
        self.syncing.set(false);
    }

    fn rebuild_tabs(&self, view: &SessionView) {
        let stale: Vec<adw::TabPage> = self
            .pages
            .borrow()
            .iter()
            .map(|(_, page)| page.clone())
            .collect();
        self.grid_host.set(None);
        detach_grid(&self.grid);
        for page in stale {
            self.tabs.close_page(&page);
        }
        let mut pages = self.pages.borrow_mut();
        pages.clear();
        for tab in &view.windows {
            let host = adw::Bin::new();
            let page = self.tabs.append(&host);
            page.set_title(&tab_title(tab.index, &tab.name));
            pages.push((tab.id, page));
        }
    }

    /// The grid is a single widget moved between tab pages: only the focused
    /// window renders panes, so building one grid per tab would cost frames for
    /// windows nobody is looking at.
    fn host_grid(&self, window: WindowId, page: &adw::TabPage) {
        if self.grid_host.get() == Some(window) {
            return;
        }
        detach_grid(&self.grid);
        if let Ok(host) = page.child().downcast::<adw::Bin>() {
            host.set_child(Some(&self.grid));
            self.grid_host.set(Some(window));
        }
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
            let kind = view
                .panes
                .get(pane)
                .map_or("terminal", |snapshot| kind_label(&snapshot.kind));
            if !widgets.get(pane).is_some_and(|widget| widget.matches(kind)) {
                widgets.insert(*pane, self.make_widget(*pane, kind, &appearance));
            }
            if let Some(widget) = widgets.get(pane) {
                children.push((*pane, widget.widget()));
            }
        }
        drop(widgets);
        self.grid.set_panes(layout, children);
    }

    fn make_widget(
        self: &Rc<Self>,
        pane: PaneId,
        kind: &'static str,
        appearance: &TerminalAppearance,
    ) -> PaneWidget {
        if kind == "terminal" {
            let target = Rc::downgrade(self);
            let chrome: Rc<dyn Fn(ChromeAction)> = Rc::new(move |action| {
                if let Some(shell) = target.upgrade() {
                    shell.perform(action);
                }
            });
            let view =
                TerminalView::new(Arc::clone(&self.engine), pane, appearance.clone(), chrome);
            let surface = TerminalPane::new(view);
            if let Some(viewport) = self.engine.viewport(pane) {
                surface.apply_frame(viewport, &ViewportDamage::All);
            }
            return PaneWidget::Terminal(surface);
        }
        let label = gtk::Label::new(Some(&format!("{kind} panes need the zz app")));
        label.add_css_class("dim-label");
        label.add_css_class("zz-placeholder");
        PaneWidget::Other {
            widget: label.upcast(),
            kind,
        }
    }

    /// The chrome a terminal surface hands up: everything that belongs to the
    /// window rather than to one pane.
    fn perform(&self, action: ChromeAction) {
        match action {
            ChromeAction::Detach => {
                self.engine.detach();
                self.window.close();
            }
            ChromeAction::TerminalFontIncrease | ChromeAction::UiZoomIn => {
                self.adjust_font(FONT_STEP_POINTS);
            }
            ChromeAction::TerminalFontDecrease | ChromeAction::UiZoomOut => {
                self.adjust_font(-FONT_STEP_POINTS);
            }
            ChromeAction::UiZoomReset => {
                self.font_offset.set(0.0);
                self.push_appearance();
            }
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

    fn appearance(&self) -> TerminalAppearance {
        let mut appearance = self.engine.appearance();
        appearance.font_size_points = (appearance.font_size_points + self.font_offset.get())
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
    }

    /// Focus only sticks once the widget is realized, so the record is kept
    /// only when the grab actually took; the next sync retries otherwise.
    fn focus_active(&self, pane: PaneId) {
        if self.focused_pane.get() == Some(pane) {
            return;
        }
        if let Some(PaneWidget::Terminal(surface)) = self.widgets.borrow().get(&pane)
            && surface.view().grab_focus()
        {
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
    session.append(Some("Detach"), Some("win.detach"));
    session.append(Some("About zz"), Some("win.about"));

    let menu = gio::Menu::new();
    menu.append_section(None, &windows);
    menu.append_section(None, &panes);
    menu.append_section(None, &session);
    menu
}

fn detach_grid(grid: &PaneGrid) {
    if let Some(host) = grid.parent().and_downcast::<adw::Bin>() {
        host.set_child(gtk::Widget::NONE);
    }
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

fn window_subtitle(view: &SessionView) -> String {
    view.windows
        .iter()
        .find(|window| window.id == view.focused_window)
        .map(|window| tab_title(window.index, &window.name))
        .unwrap_or_default()
}

fn tab_title(index: u32, name: &str) -> String {
    if name.is_empty() {
        index.to_string()
    } else {
        format!("{index}: {name}")
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
        assert_eq!(tab_title(3, ""), "3");
        assert_eq!(tab_title(3, "build"), "3: build");
    }
}
