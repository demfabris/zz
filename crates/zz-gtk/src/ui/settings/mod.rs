//! Preferences, as GNOME presents them: an adaptive sidebar dialog over the
//! window, one page per subject, closed by Escape like every other dialog on
//! the desktop.
//!
//! The apply path is the poller and only the poller. A row writes `zz/config`
//! and the 500 ms poll reads it back, so an edit made here and an edit made in
//! a text editor are the same code path — which is why the surface can never
//! disagree with the file. The one concession to feel is that a write also
//! invalidates the stamp and ticks immediately, so a click applies now rather
//! than up to half a second later; it is still the poll doing the applying.

mod config_editor;
mod mux;
mod pages;
mod rows;
mod zoom;

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

use adw::prelude::*;
use gtk::glib;
use zz_client::ChromeAction;
use zz_protocol::{CommandInvocation, MuxOptionSource};
use zz_terminal::{AppearanceProvenance, AppearanceSource, TerminalAppearance};

use crate::{
    config::{
        POLL_INTERVAL, Provenance, State, Store, import,
        schema::{self, Kind, Owner, Page, Setting},
    },
    engine::Engine,
};

use config_editor::ConfigEditor;
use mux::MuxEditor;
use pages::HostsPage;
use rows::{Row, Syncing, Write};
pub use zoom::UiZoom;

/// The preferences dialog, plus the file plumbing behind it.
pub struct Settings {
    engine: Arc<Engine>,
    chrome: RefCell<Option<Rc<dyn Fn(ChromeAction)>>>,
    changed: RefCell<Option<Rc<dyn Fn()>>>,
    config_editor: Rc<ConfigEditor>,
    import_row: RefCell<Option<adw::ActionRow>>,
    store: RefCell<Store>,
    rows: RefCell<Vec<Row>>,
    syncing: Syncing,
    dialog: adw::Dialog,
    toasts: adw::ToastOverlay,
    sidebar: adw::Sidebar,
    sidebar_section: adw::SidebarSection,
    pages: gtk::Stack,
    split: adw::NavigationSplitView,
    content_page: adw::NavigationPage,
    zoom_row: adw::ActionRow,
    mux: Rc<MuxEditor>,
    hosts: Rc<HostsPage>,
    zoom: UiZoom,
    css: gtk::CssProvider,
    open: Cell<bool>,
    prompted: Cell<bool>,
}

impl Settings {
    pub fn new(engine: Arc<Engine>) -> Rc<Self> {
        let syncing: Syncing = Rc::new(Cell::new(false));
        let mux = MuxEditor::new(Arc::clone(&engine));
        let zoom_row = adw::ActionRow::builder()
            .title("Interface zoom")
            .subtitle("100%")
            .build();
        let sidebar = adw::Sidebar::new();
        let sidebar_section = adw::SidebarSection::new();
        sidebar.append(sidebar_section.clone());

        let sidebar_toolbar = adw::ToolbarView::new();
        sidebar_toolbar.add_top_bar(&adw::HeaderBar::new());
        sidebar_toolbar.set_content(Some(&sidebar));
        let sidebar_page =
            adw::NavigationPage::with_tag(&sidebar_toolbar, "Preferences", "preferences");

        let pages = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hexpand(true)
            .vexpand(true)
            .build();
        let content_toolbar = adw::ToolbarView::new();
        content_toolbar.add_top_bar(&adw::HeaderBar::new());
        content_toolbar.set_content(Some(&pages));
        let content_page = adw::NavigationPage::with_tag(
            &content_toolbar,
            Page::Interface.title(),
            "preferences-page",
        );

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&content_page)
            .min_sidebar_width(190.0)
            .max_sidebar_width(260.0)
            .build();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&split));
        let dialog = adw::Dialog::builder()
            .title("Preferences")
            .content_width(900)
            .content_height(680)
            .width_request(360)
            .height_request(400)
            .child(&toasts)
            .build();
        let breakpoint = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 550sp")
                .expect("the preferences breakpoint is valid"),
        );
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        breakpoint.add_setter(&sidebar, "mode", Some(&adw::SidebarMode::Page.to_value()));
        dialog.add_breakpoint(breakpoint);

        let settings = Rc::new(Self {
            engine,
            chrome: RefCell::new(None),
            changed: RefCell::new(None),
            config_editor: ConfigEditor::new(),
            import_row: RefCell::new(None),
            store: RefCell::new(Store::load()),
            rows: RefCell::new(Vec::new()),
            syncing,
            dialog,
            toasts,
            sidebar,
            sidebar_section,
            pages,
            split,
            content_page,
            zoom_row,
            mux,
            hosts: HostsPage::new(),
            zoom: UiZoom::default(),
            css: gtk::CssProvider::new(),
            open: Cell::new(false),
            prompted: Cell::new(false),
        });
        let target = Rc::downgrade(&settings);
        settings.config_editor.connect_saved(Rc::new(move || {
            if let Some(settings) = target.upgrade() {
                settings.store.borrow_mut().invalidate();
                settings.tick();
            }
        }));
        settings.build_pages();
        settings.connect_host_edits();
        settings.install_css();
        settings.apply_file();

        let target = Rc::downgrade(&settings);
        settings.sidebar.connect_activated(move |_, position| {
            if let Some(settings) = target.upgrade() {
                settings.select_page(position as usize);
            }
        });

        let target = Rc::downgrade(&settings);
        settings.dialog.connect_closed(move |_| {
            if let Some(settings) = target.upgrade() {
                settings.open.set(false);
            }
        });
        // The poll is the apply path for everything in the file, dialog open or
        // not: the theme, the fleet, and the daemon's overrides all arrive
        // through it.
        let target = Rc::downgrade(&settings);
        glib::timeout_add_local(POLL_INTERVAL, move || tick(&target));
        settings
    }

    /// Adding and removing a host are file edits like any other setting: the
    /// line is written here and the poll below is what dials or drops the
    /// connection.
    fn connect_host_edits(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        let add: Rc<dyn Fn(&str)> = Rc::new(move |typed: &str| {
            let Some(route) = target.upgrade() else {
                return;
            };
            let existing: Vec<String> = route
                .store
                .borrow()
                .state()
                .fleet_hosts()
                .iter()
                .map(|host| host.name.clone())
                .collect();
            let request = match crate::ui::sidebar::parse_add_host(typed, &existing) {
                Ok(request) => request,
                Err(message) => return route.hosts.note(&message),
            };
            match crate::config::write_host(&request.name, Some(&request.endpoint)) {
                Ok(()) => {
                    route.hosts.clear_entry();
                    route.hosts.note("");
                    route.store.borrow_mut().invalidate();
                    route.tick();
                }
                Err(error) => route
                    .hosts
                    .note(&format!("Could not write zz/config: {error}")),
            }
        });
        let target = Rc::downgrade(self);
        let remove: Rc<dyn Fn(&str)> = Rc::new(move |name: &str| {
            let Some(route) = target.upgrade() else {
                return;
            };
            match crate::config::write_host(name, None) {
                Ok(()) => {
                    route.hosts.note("");
                    route.store.borrow_mut().invalidate();
                    route.tick();
                }
                Err(error) => route
                    .hosts
                    .note(&format!("Could not write zz/config: {error}")),
            }
        });
        self.hosts.connect(add, remove);
    }

    /// The window's own chrome handler, for the chords that reach the settings
    /// surface while it holds the keyboard.
    pub fn attach_chrome(&self, chrome: Rc<dyn Fn(ChromeAction)>) {
        self.chrome.replace(Some(chrome));
    }

    pub fn connect_changed(&self, changed: Rc<dyn Fn()>) {
        self.changed.replace(Some(changed));
    }

    pub fn zoom(&self) -> &UiZoom {
        &self.zoom
    }

    /// The zoom chords, and the repaint they need. True when the scale moved,
    /// which is the caller's cue to re-push the terminal appearance.
    pub fn adjust_zoom(&self, action: ChromeAction) -> bool {
        let moved = match action {
            ChromeAction::UiZoomIn => self.zoom.step(1),
            ChromeAction::UiZoomOut => self.zoom.step(-1),
            _ => self.zoom.reset(),
        };
        if moved {
            self.restyle();
            self.zoom_row
                .set_subtitle(&format!("{}%", self.zoom.percent()));
        }
        moved
    }

    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Present the dialog over the window, on a named page. `win.settings-page`
    /// reaches this, which is how a menu item, another surface, or a script
    /// deep-links into one page the way a desktop settings app does.
    pub fn open_at(&self, parent: &impl IsA<gtk::Widget>, name: &str) {
        if !self.open.replace(true) {
            self.dialog.present(Some(parent));
        }
        self.mux.refresh();
        self.config_editor.refresh();
        self.refresh_rows();
        if let Some(position) = Page::ALL.iter().position(|page| page.name() == name) {
            self.select_page(position);
        }
    }

    pub fn close(&self) {
        self.dialog.close();
    }

    pub fn toggle(&self, parent: &impl IsA<gtk::Widget>) {
        if self.is_open() {
            self.close();
        } else {
            self.open_at(parent, Page::Interface.name());
        }
    }

    /// Whether closing the window should stop the daemon. Read from the file
    /// rather than remembered, so a hand edit made a moment ago still counts.
    pub fn quit_daemon_on_exit(&self) -> bool {
        self.store
            .borrow()
            .state()
            .boolean("quit-daemon-on-exit", false)
    }

    /// Re-publish the daemon-owned overrides. A reconnected daemon knows
    /// nothing about them: the handshake carries the daemon's state, not the
    /// client's, so the client has to say it again.
    pub fn resend_overrides(&self) {
        self.send_overrides(self.store.borrow().state());
    }

    /// The daemon changed its mind about a value it owns. Its answer wins over
    /// anything this client believes it asked for.
    pub fn refresh_daemon_values(&self) {
        self.refresh_rows();
    }

    /// One `AdwPreferencesPage` per subject, every row generated from the key
    /// table.
    fn build_pages(self: &Rc<Self>) {
        let write = self.writer();
        let mut rows = Vec::new();
        for (position, page) in Page::ALL.into_iter().enumerate() {
            let content = match page {
                Page::Hosts => self.hosts.widget().clone(),
                _ => adw::PreferencesPage::new(),
            };
            content.set_title(page.title());
            content.set_icon_name(Some(page.icon()));
            content.set_name(Some(page.name()));
            if page == Page::Multiplexer {
                for group in self.mux.groups() {
                    content.add(group);
                }
            }
            for group in schema::groups(page) {
                let section = adw::PreferencesGroup::builder().title(group).build();
                for setting in schema::for_page(page).filter(|s| s.group == group) {
                    let row = Row::build(setting, &write, &self.syncing);
                    section.add(row.widget());
                    rows.push(row);
                }
                content.add(&section);
            }
            match page {
                Page::Interface => {
                    let zoom = adw::PreferencesGroup::builder().title("Zoom").build();
                    self.zoom_row.add_suffix(&self.zoom_buttons());
                    zoom.add(&self.zoom_row);
                    content.add(&zoom);
                }
                Page::Terminal => content.add(&self.import_group()),
                Page::System => {
                    content.add(self.config_editor.group());
                }
                _ => {}
            }
            self.pages.add_named(&content, Some(page.name()));
            self.sidebar_section.append(
                adw::SidebarItem::builder()
                    .title(page.title())
                    .icon_name(page.icon())
                    .build(),
            );
            if position == 0 {
                self.pages.set_visible_child_name(page.name());
            }
        }
        self.sidebar.set_selected(0);
        self.rows.replace(rows);
    }

    fn select_page(&self, position: usize) {
        let Some(page) = Page::ALL.get(position).copied() else {
            return;
        };
        self.sidebar.set_selected(position as u32);
        self.pages.set_visible_child_name(page.name());
        self.content_page.set_title(page.title());
        self.split.set_show_content(true);
        if page == Page::Multiplexer {
            self.mux.refresh();
        }
        if page == Page::System {
            self.config_editor.refresh();
        }
        if page == Page::Terminal
            && let Some(row) = self.import_row.borrow().as_ref()
        {
            let donor = zz_terminal::discover_ghostty_config();
            row.set_sensitive(donor.is_some());
            row.set_subtitle(&donor.map_or_else(
                || "No Ghostty configuration found.".to_owned(),
                |path| path.display().to_string(),
            ));
        }
    }

    fn import_group(self: &Rc<Self>) -> adw::PreferencesGroup {
        let donor = zz_terminal::discover_ghostty_config();
        let row = adw::ActionRow::builder()
            .title("Import Ghostty appearance…")
            .subtitle(donor.as_ref().map_or_else(
                || "No Ghostty configuration found.".to_owned(),
                |path| path.display().to_string(),
            ))
            .activatable(true)
            .sensitive(donor.is_some())
            .build();
        row.set_subtitle_lines(0);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let target = Rc::downgrade(self);
        row.connect_activated(move |_| {
            let Some(settings) = target.upgrade() else { return; };
            let dialog = adw::AlertDialog::new(Some("Import Ghostty appearance?"), Some("Replace the appearance settings supplied by Ghostty in zz/config. Other settings and the original Ghostty files stay intact."));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("import", "Import");
            dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
            dialog.set_close_response("cancel");
            let target = Rc::downgrade(&settings);
            dialog.connect_response(None, move |_, response| {
                if response == "import" && let Some(settings) = target.upgrade() { settings.run_import(); }
            });
            dialog.present(Some(&settings.dialog));
        });
        let group = adw::PreferencesGroup::builder().title("Import").build();
        group.add(&row);
        self.import_row.replace(Some(row));
        group
    }

    fn zoom_buttons(self: &Rc<Self>) -> gtk::Box {
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        controls.set_valign(gtk::Align::Center);
        controls.add_css_class("linked");
        for (icon, action) in [
            ("zoom-out-symbolic", ChromeAction::UiZoomOut),
            ("zoom-original-symbolic", ChromeAction::UiZoomReset),
            ("zoom-in-symbolic", ChromeAction::UiZoomIn),
        ] {
            let button = gtk::Button::from_icon_name(icon);
            button.set_tooltip_text(Some(match action {
                ChromeAction::UiZoomOut => "Zoom Out",
                ChromeAction::UiZoomReset => "Reset Zoom",
                _ => "Zoom In",
            }));
            let target = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(route) = target.upgrade()
                    && let Some(chrome) = route.chrome.borrow().as_ref()
                {
                    chrome(action);
                }
            });
            controls.append(&button);
        }
        controls
    }

    /// Every control writes through here: file first, apply second. Nothing in
    /// the surface mutates live state on its own.
    fn writer(self: &Rc<Self>) -> Write {
        let target = Rc::downgrade(self);
        Rc::new(move |setting: &'static Setting, value: Option<String>| {
            let Some(route) = target.upgrade() else {
                return;
            };
            if let Err(error) = crate::config::write(setting.key, value.as_deref()) {
                log::warn!("zz-gtk could not write {}: {error}", setting.key);
                route.report(&format!("Could not write {}: {error}", setting.key));
                route.refresh_rows();
                return;
            }
            route.store.borrow_mut().invalidate();
            route.tick();
        })
    }

    /// Where a write failure or an import result is said. The dialog carries it
    /// while it is up; otherwise it is the window's own toast, because a
    /// message nobody can see is a message nobody gets.
    fn report(&self, message: &str) {
        if self.open.get() {
            self.toasts.add_toast(adw::Toast::new(message));
        } else {
            self.engine.notify(message.to_owned());
        }
    }

    fn tick(self: &Rc<Self>) {
        if self.open.get() {
            self.mux.sync_controls();
        }
        let changed = self.store.borrow_mut().poll();
        if changed {
            self.apply_file();
        }
    }

    /// One poll's worth of consequences: the client-local half applied here,
    /// the daemon-owned half published, and the surface re-read from both.
    fn apply_file(self: &Rc<Self>) {
        let store = self.store.borrow();
        let state = store.state();
        crate::config::publish(state);
        apply_theme_mode(state);
        let chrome = state.chrome_keymap();
        // The fleet is a config value like any other: the poll is what adds and
        // removes hosts, so a hand edit and this surface cannot disagree.
        self.engine.set_fleet_hosts(state.fleet_hosts());
        let listed: Vec<(String, String)> = state
            .fleet_hosts()
            .iter()
            .map(|host| (host.name.clone(), host.endpoint.to_string()))
            .collect();
        drop(store);
        self.engine.set_chrome(chrome);
        self.hosts.refresh(&listed);
        self.restyle();
        self.send_overrides(self.store.borrow().state());
        self.refresh_rows();
        if let Some(changed) = self.changed.borrow().as_ref() {
            changed();
        }
    }

    fn send_overrides(&self, state: &State) {
        if !self.engine.supports_config_overrides() {
            log::warn!(
                "the daemon does not advertise config-overrides-v1; \
                 keeping daemon-owned zz/config entries local"
            );
            return;
        }
        self.engine
            .set_config_overrides(state.daemon_entries().to_vec());
    }

    fn install_css(&self) {
        let Some(display) = gtk::gdk::Display::default() else {
            return;
        };
        gtk::style_context_add_provider_for_display(
            &display,
            &self.css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }

    fn restyle(&self) {
        self.css.load_from_string(&self.zoom.css());
    }

    /// Re-read every row from the file and from what the daemon published. The
    /// daemon snapshots are taken once rather than per row: each accessor
    /// clones under the core's lock.
    fn refresh_rows(&self) {
        let appearance = self.engine.appearance();
        let provenance = self.engine.appearance_provenance();
        let options = self.engine.mux_options();
        let store = self.store.borrow();
        let state = store.state();
        for row in self.rows.borrow().iter() {
            let (value, source) = match row.setting.owner {
                Owner::Client => (
                    state
                        .value(row.setting.key)
                        .map_or_else(|| client_default(row.setting), str::to_owned),
                    if state.is_overridden(row.setting.key) {
                        Provenance::Override
                    } else {
                        Provenance::Default
                    },
                ),
                Owner::Appearance(key) => (
                    appearance_value(&appearance, key, state, row.setting),
                    appearance_provenance(&provenance, key),
                ),
                Owner::Mux(key) => options.get(key).map_or_else(
                    || (String::new(), Provenance::Default),
                    |option| (option.value.clone(), mux_provenance(option.source)),
                ),
            };
            row.sync(
                &value,
                source,
                state.is_overridden(row.setting.key),
                &self.syncing,
            );
        }
    }

    pub fn prompt_import(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>) {
        if self.prompted.replace(true) || !import::prompt_pending() {
            return;
        }
        let dialog = adw::AlertDialog::new(
            Some("Import Ghostty appearance?"),
            Some(
                "zz found a Ghostty configuration. Import its terminal colors and fonts into zz/config? You can also import them later from Terminal preferences.",
            ),
        );
        dialog.add_response("skip", "Not now");
        dialog.add_response("import", "Import");
        dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("import"));
        let target = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            import::mark_prompted();
            if response != "import" {
                return;
            }
            if let Some(route) = target.upgrade() {
                route.run_import();
            }
        });
        dialog.present(Some(parent));
    }

    fn run_import(self: &Rc<Self>) {
        let scheme = if adw::StyleManager::default().is_dark() {
            zz_terminal::TerminalColorScheme::Dark
        } else {
            zz_terminal::TerminalColorScheme::Light
        };
        match import::run(scheme) {
            Ok(report) => {
                self.store.borrow_mut().invalidate();
                self.tick();
                if report.imported_anything() {
                    self.engine.execute_on(
                        crate::engine::HostId::LOCAL,
                        CommandInvocation::new("reload-config", [] as [&str; 0]),
                    );
                }
                self.report(if report.imported_anything() {
                    "Imported Ghostty appearance."
                } else {
                    "Nothing to import."
                });
            }
            Err(error) => self.report(&format!("Import failed: {error}")),
        }
    }
}

fn tick(target: &Weak<Settings>) -> glib::ControlFlow {
    let Some(route) = target.upgrade() else {
        return glib::ControlFlow::Break;
    };
    route.tick();
    glib::ControlFlow::Continue
}

fn client_default(setting: &Setting) -> String {
    match setting.kind {
        Kind::Toggle { default } => schema::boolean(default).to_owned(),
        Kind::Number { default, .. } => schema::number(f64::from(default)),
        Kind::Choice { default, .. } => default.to_owned(),
        Kind::Color | Kind::Text { .. } => String::new(),
    }
}

/// The daemon's effective value wins; the raw file line is the fallback for the
/// keys it flattens away rather than re-emits, `theme` above all.
fn appearance_value(
    appearance: &TerminalAppearance,
    key: zz_terminal::AppearanceConfigKey,
    state: &State,
    setting: &Setting,
) -> String {
    schema::appearance_display(appearance, key)
        .or_else(|| state.value(setting.key).map(str::to_owned))
        .unwrap_or_default()
}

fn appearance_provenance(
    provenance: &AppearanceProvenance,
    key: zz_terminal::AppearanceConfigKey,
) -> Provenance {
    match provenance.source(key) {
        AppearanceSource::Default => Provenance::Default,
        AppearanceSource::ThemeFile => Provenance::ThemeFile,
        AppearanceSource::Ghostty => Provenance::Ghostty,
        AppearanceSource::Override => Provenance::Override,
    }
}

fn mux_provenance(source: MuxOptionSource) -> Provenance {
    match source {
        MuxOptionSource::Default => Provenance::Default,
        MuxOptionSource::TmuxConfig => Provenance::TmuxConfig,
        MuxOptionSource::Override => Provenance::Override,
        MuxOptionSource::RuntimeCommand => Provenance::RuntimeCommand,
    }
}

fn apply_theme_mode(state: &State) {
    adw::StyleManager::default().set_color_scheme(match state.value("theme-mode") {
        Some("light") => adw::ColorScheme::ForceLight,
        Some("dark") => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_defaults_are_spelled_the_way_the_file_spells_them() {
        let theme = setting("theme-mode");
        let quit = setting("quit-daemon-on-exit");

        assert_eq!(client_default(theme), "system");
        assert_eq!(client_default(quit), "false");
    }

    fn setting(key: &str) -> &'static Setting {
        schema::SETTINGS
            .iter()
            .find(|setting| setting.key == key)
            .expect("the key is in the table")
    }
}
