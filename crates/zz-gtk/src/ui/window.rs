use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use adw::prelude::*;
use gtk::glib;
use zz_client::ViewportDamage;
use zz_protocol::{CommandInvocation, LayoutNode, PaneId, PaneKindSnapshot, WindowId};
use zz_terminal::TerminalAppearance;

use crate::{
    engine::{Engine, EngineEvent, SessionView},
    ui::{
        panes::{PaneGrid, layout_panes},
        terminal::TerminalView,
    },
};

const STYLE: &str = "
.zz-panes { background-color: @headerbar_bg_color; }
.zz-status { padding: 2px 10px; }
.zz-placeholder { padding: 24px; }
";

enum PaneWidget {
    Terminal(TerminalView),
    Other {
        widget: gtk::Widget,
        kind: &'static str,
    },
}

impl PaneWidget {
    fn widget(&self) -> gtk::Widget {
        match self {
            Self::Terminal(view) => view.clone().upcast(),
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
    status_bar: gtk::CenterBox,
    status_left: gtk::Label,
    status_right: gtk::Label,
    grid: PaneGrid,
    widgets: RefCell<HashMap<PaneId, PaneWidget>>,
    pages: RefCell<Vec<(WindowId, adw::TabPage)>>,
    grid_host: Cell<Option<WindowId>>,
    focused_pane: Cell<Option<PaneId>>,
    syncing: Cell<bool>,
}

impl Shell {
    pub fn build(app: &adw::Application, engine: Arc<Engine>) -> Rc<Self> {
        install_style();

        let title = adw::WindowTitle::new("zz", "");
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));

        let tabs = adw::TabView::new();
        let tab_bar = adw::TabBar::builder().view(&tabs).autohide(false).build();

        let status_left = gtk::Label::builder().xalign(0.0).build();
        let status_right = gtk::Label::builder().xalign(1.0).build();
        let status_bar = gtk::CenterBox::builder().visible(false).build();
        status_bar.add_css_class("toolbar");
        status_bar.add_css_class("zz-status");
        status_bar.set_start_widget(Some(&status_left));
        status_bar.set_end_widget(Some(&status_right));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&tab_bar);
        toolbar.set_content(Some(&tabs));
        toolbar.add_bottom_bar(&status_bar);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&toolbar));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1024)
            .default_height(680)
            .title("zz")
            .content(&toasts)
            .build();

        let shell = Rc::new(Self {
            engine,
            window,
            title,
            toasts,
            tabs,
            status_bar,
            status_left,
            status_right,
            grid: PaneGrid::new(),
            widgets: RefCell::new(HashMap::new()),
            pages: RefCell::new(Vec::new()),
            grid_host: Cell::new(None),
            focused_pane: Cell::new(None),
            syncing: Cell::new(false),
        });
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

    fn handle(&self, event: EngineEvent) {
        match event {
            EngineEvent::FramesReady => self.apply_frames(),
            EngineEvent::StatusChanged => self.refresh_status(),
            EngineEvent::SnapshotChanged | EngineEvent::Attached(_) => self.sync(),
            EngineEvent::AppearanceChanged => {
                let appearance = self.engine.appearance();
                for widget in self.widgets.borrow().values() {
                    if let PaneWidget::Terminal(view) = widget {
                        view.set_appearance(appearance.clone());
                    }
                }
            }
            EngineEvent::Notice(text) => self.toasts.add_toast(adw::Toast::new(&text)),
            EngineEvent::Reconnecting { attempt } => {
                if attempt == 1 {
                    self.toasts
                        .add_toast(adw::Toast::new("Reconnecting to the zz daemon…"));
                }
            }
            EngineEvent::Reconnected => {
                self.toasts.add_toast(adw::Toast::new("Reconnected"));
            }
            EngineEvent::Detached | EngineEvent::Disconnected(_) => self.window.close(),
        }
    }

    fn apply_frames(&self) {
        let widgets = self.widgets.borrow();
        for frame in self.engine.take_frames() {
            if let Some(PaneWidget::Terminal(view)) = widgets.get(&frame.pane) {
                view.apply_frame(frame.viewport, &frame.damage);
            }
        }
    }

    /// The status line carries a clock and republishes about once a second, so
    /// it may never reach the grid.
    fn refresh_status(&self) {
        let status = self.engine.status();
        self.status_bar.set_visible(!status.is_empty());
        self.status_left.set_text(&status.left);
        self.status_right.set_text(&status.right);
    }

    fn sync(&self) {
        let Some(view) = self.engine.session_view() else {
            return;
        };
        self.title.set_title(&view.name);
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

    fn sync_panes(&self, view: &SessionView) {
        let layout = view
            .zoomed_pane
            .map_or_else(|| view.layout.clone(), LayoutNode::Pane);
        let placed = layout_panes(&layout);
        let appearance = self.engine.appearance();
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
        &self,
        pane: PaneId,
        kind: &'static str,
        appearance: &TerminalAppearance,
    ) -> PaneWidget {
        if kind == "terminal" {
            let view = TerminalView::new(Arc::clone(&self.engine), pane, appearance.clone());
            if let Some(viewport) = self.engine.viewport(pane) {
                view.apply_frame(viewport, &ViewportDamage::All);
            }
            return PaneWidget::Terminal(view);
        }
        let label = gtk::Label::new(Some(&format!("{kind} panes need the zz app")));
        label.add_css_class("dim-label");
        label.add_css_class("zz-placeholder");
        PaneWidget::Other {
            widget: label.upcast(),
            kind,
        }
    }

    /// Focus only sticks once the widget is realized, so the record is kept
    /// only when the grab actually took; the next sync retries otherwise.
    fn focus_active(&self, pane: PaneId) {
        if self.focused_pane.get() == Some(pane) {
            return;
        }
        if let Some(PaneWidget::Terminal(view)) = self.widgets.borrow().get(&pane)
            && view.grab_focus()
        {
            self.focused_pane.set(Some(pane));
        }
    }
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
