mod input;
mod runtime;

pub use runtime::{run_subprocess, shutdown};

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

use adw::prelude::*;
use gtk::{gdk, glib};
use zz_browser::{BrowserCursor, BrowserEvent, BrowserSession, OsrFrame, Viewport};
use zz_client::ChromeAction;
use zz_protocol::{
    BrowserCommand, BrowserDescriptor, CommandInvocation, GuiResponse, KeyToken, PaneId,
};

use crate::engine::{Engine, HostId};

struct BrowserTab {
    owner: RefCell<Weak<BrowserPane>>,
    page: adw::TabPage,
    picture: gtk::Picture,
    status: gtk::Label,
    url: RefCell<String>,
    session: RefCell<Option<BrowserSession>>,
    failed: Cell<bool>,
    back: Cell<bool>,
    forward: Cell<bool>,
    pointer: Cell<(i32, i32)>,
}

thread_local! {
    static TRANSFERRED: RefCell<std::collections::HashMap<adw::TabPage, Rc<BrowserTab>>> = RefCell::new(std::collections::HashMap::new());
}

impl BrowserTab {
    fn viewport(&self) -> Viewport {
        Viewport {
            width: self.picture.width().max(1) as u32,
            height: self.picture.height().max(1) as u32,
            scale_factor: self.picture.scale_factor() as f32,
            window_zoom: 1.0,
            screen_x: 0,
            screen_y: 0,
            visible: self.picture.is_mapped(),
        }
    }

    fn with_session(&self, action: impl FnOnce(&BrowserSession)) {
        if let Some(session) = self.session.borrow().as_ref() {
            action(session);
        }
    }
}

impl Drop for BrowserTab {
    fn drop(&mut self) {
        if let Some(session) = self.session.get_mut().take() {
            runtime::retire(session);
        }
    }
}

pub struct BrowserPane {
    root: gtk::Box,
    view: adw::TabView,
    address: gtk::Entry,
    back: gtk::Button,
    forward: gtk::Button,
    reload: gtk::Button,
    tabs: RefCell<Vec<Rc<BrowserTab>>>,
    descriptor: RefCell<BrowserDescriptor>,
    synchronizing: Cell<bool>,
    engine: Arc<Engine>,
    host: HostId,
    pane: PaneId,
    remote_egress: bool,
}

impl BrowserPane {
    pub fn new(engine: Arc<Engine>, pane: PaneId, descriptor: &BrowserDescriptor) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        let view = adw::TabView::new();
        view.set_shortcuts(adw::TabViewShortcuts::empty());
        view.set_vexpand(true);
        let bar = adw::TabBar::builder().view(&view).autohide(true).build();
        let add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("New browser tab")
            .has_frame(false)
            .build();
        bar.set_end_action_widget(Some(&add));
        root.append(&bar);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        toolbar.add_css_class("toolbar");
        let back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Back")
            .has_frame(false)
            .build();
        let forward = gtk::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text("Forward")
            .has_frame(false)
            .build();
        let reload = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Reload")
            .has_frame(false)
            .build();
        let address = gtk::Entry::builder()
            .placeholder_text("Search or enter an address")
            .hexpand(true)
            .build();
        toolbar.append(&back);
        toolbar.append(&forward);
        toolbar.append(&reload);
        toolbar.append(&address);
        root.append(&toolbar);
        root.append(&view);
        let host = engine.active_host();
        let this = Rc::new(Self {
            root,
            view,
            address,
            back,
            forward,
            reload,
            tabs: RefCell::new(Vec::new()),
            descriptor: RefCell::new(descriptor.clone()),
            synchronizing: Cell::new(false),
            engine,
            host,
            pane,
            remote_egress: crate::config::current().browser_egress,
        });
        this.connect(&add);
        this.rebuild(descriptor);
        runtime::register(Rc::downgrade(&this));
        this
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub fn grab_focus(&self) -> bool {
        self.active().is_some_and(|tab| tab.picture.grab_focus())
    }

    pub fn focus_address(&self) {
        self.address.grab_focus();
        self.address.select_region(0, -1);
    }

    pub fn perform(self: &Rc<Self>, action: ChromeAction) -> bool {
        let Some(tab) = self.active() else {
            return false;
        };
        match action {
            ChromeAction::BrowserFocusAddress => self.focus_address(),
            ChromeAction::BrowserNewTab => self.open_popup("about:blank", true, None),
            ChromeAction::ClosePane => self.view.close_page(&tab.page),
            ChromeAction::BrowserNextTab
            | ChromeAction::BrowserPreviousTab
            | ChromeAction::BrowserSelectTab(_)
            | ChromeAction::BrowserSelectLastTab => {
                let tabs = self.tabs.borrow();
                let current = tabs
                    .iter()
                    .position(|candidate| candidate.page == tab.page)
                    .unwrap_or(0);
                let next = match action {
                    ChromeAction::BrowserNextTab => (current + 1) % tabs.len(),
                    ChromeAction::BrowserPreviousTab => (current + tabs.len() - 1) % tabs.len(),
                    ChromeAction::BrowserSelectTab(index) => usize::from(index),
                    _ => tabs.len() - 1,
                };
                let page = tabs.get(next).map(|tab| tab.page.clone());
                drop(tabs);
                if let Some(page) = page {
                    self.view.set_selected_page(&page);
                }
            }
            ChromeAction::BrowserBack => self.command(BrowserCommand::Back),
            ChromeAction::BrowserForward => self.command(BrowserCommand::Forward),
            ChromeAction::BrowserReload => self.command(BrowserCommand::Reload),
            ChromeAction::BrowserZoomIn => tab.with_session(|session| {
                let _ = session.zoom_in();
            }),
            ChromeAction::BrowserZoomOut => tab.with_session(|session| {
                let _ = session.zoom_out();
            }),
            ChromeAction::BrowserZoomReset => tab.with_session(|session| {
                let _ = session.reset_zoom();
            }),
            ChromeAction::BrowserDevTools => tab.with_session(BrowserSession::toggle_dev_tools),
            ChromeAction::BrowserUndo
            | ChromeAction::BrowserRedo
            | ChromeAction::BrowserCut
            | ChromeAction::BrowserCopy
            | ChromeAction::BrowserPaste
            | ChromeAction::BrowserPasteAndMatchStyle
            | ChromeAction::BrowserSelectAll => {
                let command = match action {
                    ChromeAction::BrowserUndo => zz_browser::EditCommand::Undo,
                    ChromeAction::BrowserRedo => zz_browser::EditCommand::Redo,
                    ChromeAction::BrowserCut => zz_browser::EditCommand::Cut,
                    ChromeAction::BrowserCopy => zz_browser::EditCommand::Copy,
                    ChromeAction::BrowserPaste => zz_browser::EditCommand::Paste,
                    ChromeAction::BrowserPasteAndMatchStyle => {
                        zz_browser::EditCommand::PasteAndMatchStyle
                    }
                    _ => zz_browser::EditCommand::SelectAll,
                };
                tab.with_session(|session| session.edit(command));
            }
            _ => return false,
        }
        true
    }

    pub fn update(self: &Rc<Self>, descriptor: &BrowserDescriptor) {
        if *self.descriptor.borrow() == *descriptor {
            return;
        }
        if self.descriptor.borrow().profile != descriptor.profile
            || self.tabs.borrow().len() != descriptor.tabs.len()
        {
            self.rebuild(descriptor);
            return;
        }
        self.synchronizing.set(true);
        for (tab, url) in self.tabs.borrow().iter().zip(&descriptor.tabs) {
            if *tab.url.borrow() != *url {
                tab.url.replace(url.clone());
                tab.with_session(|session| session.navigate(url));
            }
        }
        if let Some(tab) = self.tabs.borrow().get(descriptor.active_tab) {
            self.view.set_selected_page(&tab.page);
        }
        self.descriptor.replace(descriptor.clone());
        self.synchronizing.set(false);
        self.refresh_chrome();
    }

    fn rebuild(self: &Rc<Self>, descriptor: &BrowserDescriptor) {
        self.synchronizing.set(true);
        let old = self.tabs.take();
        for tab in old {
            self.view.close_page(&tab.page);
        }
        self.descriptor.replace(descriptor.clone());
        for url in &descriptor.tabs {
            self.add_tab(url);
        }
        if self.tabs.borrow().is_empty() {
            self.add_tab("about:blank");
        }
        if let Some(tab) = self.tabs.borrow().get(descriptor.active_tab) {
            self.view.set_selected_page(&tab.page);
        }
        self.synchronizing.set(false);
        self.refresh_chrome();
    }

    fn add_tab(self: &Rc<Self>, url: &str) {
        let picture = gtk::Picture::builder()
            .hexpand(true)
            .vexpand(true)
            .can_shrink(true)
            .focusable(true)
            .content_fit(gtk::ContentFit::Fill)
            .build();
        let status = gtk::Label::builder()
            .label("Starting browser…")
            .wrap(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        status.add_css_class("dim-label");
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&picture));
        overlay.add_overlay(&status);
        let page = self.view.append(&overlay);
        page.set_title(if url == "about:blank" { "New tab" } else { url });
        let tab = Rc::new(BrowserTab {
            owner: RefCell::new(Rc::downgrade(self)),
            page,
            picture,
            status,
            url: RefCell::new(url.to_owned()),
            session: RefCell::new(None),
            failed: Cell::new(false),
            back: Cell::new(false),
            forward: Cell::new(false),
            pointer: Cell::new((0, 0)),
        });
        input::connect(&tab);
        self.tabs.borrow_mut().push(tab);
    }

    fn connect(self: &Rc<Self>, add: &gtk::Button) {
        let keyboard = gtk::EventControllerLegacy::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pressed = Rc::new(RefCell::new(std::collections::HashSet::new()));
        let focus = gtk::EventControllerFocus::new();
        let focused_keys = Rc::clone(&pressed);
        focus.connect_leave(move |_| focused_keys.borrow_mut().clear());
        self.root.add_controller(focus);
        let weak = Rc::downgrade(self);
        keyboard.connect_event(move |_, event| {
            let Some(key) = event.downcast_ref::<gdk::KeyEvent>() else {
                return glib::Propagation::Proceed;
            };
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match event.event_type() {
                gdk::EventType::KeyRelease => {
                    if pressed.borrow_mut().remove(&key.keycode()) {
                        return glib::Propagation::Stop;
                    }
                }
                gdk::EventType::KeyPress => {
                    if pressed.borrow().contains(&key.keycode()) {
                        return glib::Propagation::Stop;
                    }
                    if this.engine.prefix_armed() {
                        return glib::Propagation::Proceed;
                    }
                    let input = crate::ui::keys::key_input(
                        zz_terminal::KeyAction::Press,
                        key.keyval(),
                        event.modifier_state(),
                        None,
                    );
                    let action = this
                        .engine
                        .chrome()
                        .resolve(zz_client::BROWSER_TABLE, &input);
                    if action.is_some_and(|action| this.perform(action)) {
                        pressed.borrow_mut().insert(key.keycode());
                        return glib::Propagation::Stop;
                    }
                }
                _ => {}
            }
            glib::Propagation::Proceed
        });
        self.root.add_controller(keyboard);
        let weak = Rc::downgrade(self);
        add.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.synchronizing.set(true);
                this.add_tab("about:blank");
                if let Some(tab) = this.tabs.borrow().last() {
                    this.view.set_selected_page(&tab.page);
                }
                this.synchronizing.set(false);
                this.persist();
                this.focus_address();
            }
        });
        let weak = Rc::downgrade(self);
        self.view.connect_close_page(move |view, page| {
            let Some(this) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if this.synchronizing.get() {
                return glib::Propagation::Proceed;
            }
            if this.tabs.borrow().len() == 1 {
                this.pane_command("kill-pane");
                return glib::Propagation::Stop;
            }
            this.synchronizing.set(true);
            this.tabs.borrow_mut().retain(|tab| tab.page != *page);
            view.close_page_finish(page, true);
            this.synchronizing.set(false);
            this.persist();
            this.refresh_chrome();
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        self.view.connect_selected_page_notify(move |_| {
            if let Some(this) = weak.upgrade()
                && !this.synchronizing.get()
            {
                this.persist();
                this.refresh_chrome();
            }
        });
        let weak = Rc::downgrade(self);
        self.view.connect_page_reordered(move |view, _, _| {
            if let Some(this) = weak.upgrade() {
                this.tabs
                    .borrow_mut()
                    .sort_by_key(|tab| view.page_position(&tab.page));
                this.persist();
            }
        });
        let weak = Rc::downgrade(self);
        self.view.connect_page_detached(move |_, page, _| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            if this.synchronizing.get() {
                return;
            }
            let position = this.tabs.borrow().iter().position(|tab| tab.page == *page);
            let Some(position) = position else {
                return;
            };
            let tab = this.tabs.borrow_mut().remove(position);
            TRANSFERRED.with_borrow_mut(|tabs| tabs.insert(page.clone(), tab));
            if this.tabs.borrow().is_empty() {
                this.pane_command("kill-pane");
            } else {
                this.persist();
                this.refresh_chrome();
            }
        });
        let weak = Rc::downgrade(self);
        self.view.connect_page_attached(move |view, page, _| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let Some(tab) = TRANSFERRED.with_borrow_mut(|tabs| tabs.remove(page)) else {
                return;
            };
            if tab.owner.borrow().upgrade().is_some_and(|owner| {
                owner.host != this.host
                    || owner.descriptor.borrow().profile != this.descriptor.borrow().profile
            }) {
                if let Some(session) = tab.session.take() {
                    runtime::retire(session);
                }
                tab.failed.set(false);
            }
            tab.owner.replace(Rc::downgrade(&this));
            this.tabs.borrow_mut().push(tab);
            this.tabs
                .borrow_mut()
                .sort_by_key(|tab| view.page_position(&tab.page));
            this.persist();
            this.refresh_chrome();
        });
        let weak = Rc::downgrade(self);
        self.address.connect_activate(move |address| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            match zz_browser::resolve_address(
                address.text().as_str(),
                crate::config::current().browser_search,
            ) {
                Ok(url) => {
                    this.command(BrowserCommand::Navigate(url));
                    this.grab_focus();
                }
                Err(error) => this.engine.notify(error.to_string()),
            }
        });
        for (button, command) in [
            (&self.back, BrowserCommand::Back),
            (&self.forward, BrowserCommand::Forward),
            (&self.reload, BrowserCommand::Reload),
        ] {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.command(command.clone());
                }
            });
        }
    }

    fn active(&self) -> Option<Rc<BrowserTab>> {
        let selected = self.view.selected_page()?;
        self.tabs
            .borrow()
            .iter()
            .find(|tab| tab.page == selected)
            .cloned()
    }

    fn persist(&self) {
        if self.synchronizing.get() {
            return;
        }
        let selected = self.view.selected_page();
        let tabs = self.tabs.borrow();
        let active_tab = tabs
            .iter()
            .position(|tab| Some(&tab.page) == selected.as_ref())
            .unwrap_or(0);
        let urls = tabs
            .iter()
            .map(|tab| tab.url.borrow().clone())
            .collect::<Vec<_>>();
        let mut descriptor = self.descriptor.borrow_mut();
        if descriptor.tabs == urls && descriptor.active_tab == active_tab {
            return;
        }
        descriptor.tabs.clone_from(&urls);
        descriptor.active_tab = active_tab;
        let mut args = vec![
            "-t".to_owned(),
            self.pane.to_string(),
            "-a".to_owned(),
            active_tab.to_string(),
            "--".to_owned(),
        ];
        args.extend(urls);
        self.engine
            .execute_on(self.host, CommandInvocation::new("set-browser-tabs", args));
    }

    fn pane_command(&self, command: &str) {
        self.engine.execute_on(
            self.host,
            CommandInvocation::new(command, ["-t", &self.pane.to_string()]),
        );
    }

    fn refresh_chrome(&self) {
        if let Some(tab) = self.active() {
            if !self.address.has_focus() {
                self.address.set_text(&tab.url.borrow());
            }
            self.back.set_sensitive(tab.back.get());
            self.forward.set_sensitive(tab.forward.get());
        }
    }

    pub fn command(&self, command: BrowserCommand) {
        let Some(tab) = self.active() else {
            if let BrowserCommand::Screenshot { request_id, .. } = command {
                self.engine.respond_to_request_on(
                    self.host,
                    GuiResponse::Error {
                        request_id,
                        message: "The browser has no active tab to capture.".into(),
                    },
                );
            }
            return;
        };
        if let BrowserCommand::Screenshot { request_id, path } = command {
            let result = tab
                .picture
                .paintable()
                .and_downcast::<gdk::Texture>()
                .ok_or_else(|| "The browser has not rendered a frame yet".to_owned())
                .and_then(|texture| {
                    texture
                        .save_to_png(&path)
                        .map_err(|error| error.to_string())
                });
            self.engine.respond_to_request_on(
                self.host,
                match result {
                    Ok(()) => GuiResponse::Success {
                        request_id,
                        output: path,
                    },
                    Err(message) => GuiResponse::Error {
                        request_id,
                        message,
                    },
                },
            );
            return;
        }
        if let BrowserCommand::Navigate(url) = &command {
            tab.url.replace(url.clone());
            tab.failed.set(false);
            self.persist();
            self.refresh_chrome();
        }
        tab.with_session(|session| match command {
            BrowserCommand::Navigate(url) => session.navigate(&url),
            BrowserCommand::Reload => session.reload(),
            BrowserCommand::Back => session.go_back(),
            BrowserCommand::Forward => session.go_forward(),
            BrowserCommand::Key(input) => session.send_key(input::translate(&input)),
            BrowserCommand::SendKeys(keys) => send_tokens(session, &keys),
            BrowserCommand::SendKeysRepeated { keys, count } => {
                for _ in 0..count.min(zz_protocol::MAX_BROWSER_KEY_REPEAT) {
                    send_tokens(session, &keys);
                }
            }
            BrowserCommand::Screenshot { .. } => unreachable!(),
        });
    }

    fn tick(self: &Rc<Self>) {
        let tabs = self.tabs.borrow().clone();
        let egress = match if self.remote_egress {
            self.engine.browser_egress(self.host)
        } else {
            Ok(None)
        } {
            Ok(egress) => egress,
            Err(error) => {
                for tab in tabs {
                    if let Some(session) = tab.session.take() {
                        runtime::retire(session);
                    }
                    tab.status.set_text(&error);
                    tab.status.set_visible(true);
                    tab.picture.set_paintable(gdk::Paintable::NONE);
                    tab.failed.set(false);
                }
                return;
            }
        };
        for tab in tabs {
            if tab.session.borrow().is_none() && !tab.failed.get() {
                match runtime::create(
                    &self.descriptor.borrow().profile,
                    egress.as_ref(),
                    &tab.url.borrow(),
                    tab.viewport(),
                ) {
                    Ok(Some(session)) => {
                        session.set_focus(tab.picture.has_focus());
                        tab.session.replace(Some(session));
                    }
                    Ok(None) => continue,
                    Err(error) => {
                        tab.status.set_text(&error);
                        tab.failed.set(true);
                        continue;
                    }
                }
            }
            let Some(mut session) = tab.session.take() else {
                continue;
            };
            if let Some((_, port)) = &egress
                && let Err(error) = runtime::refresh_route(session.profile(), *port)
            {
                tab.status.set_text(&error);
                tab.status.set_visible(true);
                runtime::retire(session);
                continue;
            }
            session.set_viewport(tab.viewport());
            if tab.picture.is_mapped() {
                session.send_external_begin_frame();
            }
            while let Ok(event) = session.events().try_recv() {
                match event {
                    BrowserEvent::Created { .. } => session.mark_ready(),
                    BrowserEvent::AddressChanged { url, .. } => {
                        tab.url.replace(url.to_string());
                        self.persist();
                        self.refresh_chrome();
                    }
                    BrowserEvent::TitleChanged { title, .. } => tab.page.set_title(&title),
                    BrowserEvent::LoadingChanged {
                        loading,
                        can_go_back,
                        can_go_forward,
                        ..
                    } => {
                        tab.page.set_loading(loading);
                        tab.back.set(can_go_back);
                        tab.forward.set(can_go_forward);
                        self.refresh_chrome();
                    }
                    BrowserEvent::CursorChanged { cursor, .. } => {
                        tab.picture.set_cursor_from_name(Some(cursor_name(cursor)));
                    }
                    BrowserEvent::LoadFailed {
                        code, description, ..
                    } if code != -3 => {
                        tab.status.set_text(&description);
                        tab.status.set_visible(true);
                    }
                    BrowserEvent::RenderProcessTerminated { status, .. } => {
                        session.mark_crashed();
                        tab.status.set_text(&status);
                        tab.status.set_visible(true);
                    }
                    BrowserEvent::Closed { .. } => session.mark_closed(),
                    BrowserEvent::ContextMenuRequested { request, .. } => {
                        self.context_menu(&tab, &request);
                    }
                    BrowserEvent::PopupRequested {
                        url, foreground, ..
                    } => {
                        self.open_popup(&url, foreground, None);
                    }
                    BrowserEvent::PopupCreated {
                        popup,
                        url,
                        foreground,
                        ..
                    } => {
                        self.open_popup(&url, foreground, session.take_popup(popup));
                    }
                    _ => {}
                }
            }
            if let Some(OsrFrame::OwnedBgra(frame)) = session.take_frame() {
                let bytes = glib::Bytes::from_owned(frame.bgra);
                let texture = gdk::MemoryTexture::new(
                    frame.width as i32,
                    frame.height as i32,
                    gdk::MemoryFormat::B8g8r8a8Premultiplied,
                    &bytes,
                    frame.width as usize * 4,
                );
                tab.picture.set_paintable(Some(&texture));
                tab.status.set_visible(false);
            }
            tab.session.replace(Some(session));
        }
    }

    fn close_sessions(&self) {
        for tab in self.tabs.borrow().iter() {
            if let Some(session) = tab.session.take() {
                runtime::retire(session);
            }
            tab.failed.set(true);
        }
    }

    fn context_menu(
        self: &Rc<Self>,
        tab: &Rc<BrowserTab>,
        request: &zz_browser::ContextMenuRequest,
    ) {
        let menu = gtk::Popover::new();
        menu.set_parent(&tab.picture);
        menu.set_has_arrow(false);
        menu.set_pointing_to(Some(&gdk::Rectangle::new(request.x, request.y, 1, 1)));
        let items = gtk::Box::new(gtk::Orientation::Vertical, 0);
        if let Some(url) = &request.link_url {
            let open = gtk::Button::with_label("Open link in new tab");
            open.set_has_frame(false);
            let weak = Rc::downgrade(self);
            let copied_url = url.to_string();
            let url = url.to_string();
            let popover = menu.clone();
            open.connect_clicked(move |_| {
                popover.popdown();
                if let Some(pane) = weak.upgrade() {
                    pane.open_popup(&url, true, None);
                }
            });
            items.append(&open);
            let copy = gtk::Button::with_label("Copy link address");
            copy.set_has_frame(false);
            let clipboard = tab.picture.clipboard();
            let popover = menu.clone();
            copy.connect_clicked(move |_| {
                clipboard.set_text(&copied_url);
                popover.popdown();
            });
            items.append(&copy);
        }
        for (label, command, enabled) in [
            (
                "Cut",
                zz_browser::EditCommand::Cut,
                request.edit_flags.can_cut,
            ),
            (
                "Copy",
                zz_browser::EditCommand::Copy,
                request.edit_flags.can_copy,
            ),
            (
                "Paste",
                zz_browser::EditCommand::Paste,
                request.edit_flags.can_paste,
            ),
            (
                "Select all",
                zz_browser::EditCommand::SelectAll,
                request.edit_flags.can_select_all,
            ),
        ] {
            let button = gtk::Button::with_label(label);
            button.set_has_frame(false);
            button.set_sensitive(enabled);
            let weak = Rc::downgrade(tab);
            let popover = menu.clone();
            button.connect_clicked(move |_| {
                popover.popdown();
                if let Some(tab) = weak.upgrade() {
                    tab.with_session(|session| session.edit(command));
                }
            });
            items.append(&button);
        }
        menu.set_child(Some(&items));
        menu.connect_closed(WidgetExt::unparent);
        menu.popup();
    }

    fn open_popup(self: &Rc<Self>, url: &str, foreground: bool, session: Option<BrowserSession>) {
        self.synchronizing.set(true);
        self.add_tab(url);
        if let Some(tab) = self.tabs.borrow().last() {
            tab.session.replace(session);
            if foreground {
                self.view.set_selected_page(&tab.page);
            }
        }
        self.synchronizing.set(false);
        self.persist();
        self.refresh_chrome();
    }
}

fn send_tokens(session: &BrowserSession, keys: &[KeyToken]) {
    for token in keys {
        match token {
            KeyToken::Literal(text) => session.send_text(text),
            KeyToken::Named(name) => {
                if let Some(input) = input::named(name) {
                    session.send_key(input);
                }
            }
            KeyToken::Raw(_) => {}
        }
    }
}

fn cursor_name(cursor: BrowserCursor) -> &'static str {
    match cursor {
        BrowserCursor::Arrow => "default",
        BrowserCursor::IBeam => "text",
        BrowserCursor::PointingHand => "pointer",
        BrowserCursor::Crosshair => "crosshair",
        BrowserCursor::Wait => "wait",
        BrowserCursor::Help => "help",
        BrowserCursor::Move => "move",
        BrowserCursor::ResizeHorizontal => "ew-resize",
        BrowserCursor::ResizeVertical => "ns-resize",
        BrowserCursor::ResizeNorthEastSouthWest => "nesw-resize",
        BrowserCursor::ResizeNorthWestSouthEast => "nwse-resize",
        BrowserCursor::NotAllowed => "not-allowed",
        BrowserCursor::Grab => "grab",
        BrowserCursor::Grabbing => "grabbing",
        BrowserCursor::None => "none",
    }
}
