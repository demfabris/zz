//! The settings route.
//!
//! Not a dialog: like the desktop's `WorkspaceRoute::Settings`, this replaces
//! the pane area inside the same window, and the `ClosePane` chord closes it.
//!
//! The apply path is the poller and only the poller. A row writes `zz/config`
//! and the 500 ms poll reads it back, so an edit made here and an edit made in
//! a text editor are the same code path — which is why the surface can never
//! disagree with the file. The one concession to feel is that a write also
//! invalidates the stamp and ticks immediately, so a click applies now rather
//! than up to half a second later; it is still the poll doing the applying.

mod pages;
mod rows;
mod zoom;

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};

use adw::prelude::*;
use gtk::{gio, glib};
use zz_client::{ChromeAction, ChromeKeymap};
use zz_protocol::{CommandInvocation, MuxOptionSource};
use zz_terminal::{AppearanceProvenance, AppearanceSource, KeyAction, TerminalAppearance};

use crate::{
    config::{
        POLL_INTERVAL, Provenance, State, Store, import,
        schema::{self, Kind, Owner, Page, Setting},
    },
    engine::Engine,
    ui::keys,
};

use pages::{HostsPage, MuxEditor};
use rows::{Row, Syncing, Write};
pub use zoom::UiZoom;

const SIDEBAR_WIDTH: f64 = 220.0;

/// The whole settings surface, plus the file plumbing behind it.
pub struct SettingsRoute {
    engine: Arc<Engine>,
    chrome: RefCell<Option<Rc<dyn Fn(ChromeAction)>>>,
    store: RefCell<Store>,
    rows: RefCell<Vec<Row>>,
    syncing: Syncing,
    split: adw::NavigationSplitView,
    stack: gtk::Stack,
    list: gtk::ListBox,
    banner: adw::Banner,
    zoom_row: adw::ActionRow,
    mux: Rc<MuxEditor>,
    hosts: Rc<HostsPage>,
    zoom: UiZoom,
    css: gtk::CssProvider,
    route: RefCell<Option<gtk::Stack>>,
    prompted: Cell<bool>,
}

impl SettingsRoute {
    pub fn new(engine: Arc<Engine>) -> Rc<Self> {
        let syncing: Syncing = Rc::new(Cell::new(false));
        let banner = adw::Banner::new("");
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("navigation-sidebar");

        let mux = MuxEditor::new();
        let zoom_row = adw::ActionRow::builder()
            .title("Interface zoom")
            .subtitle("100%  ·  transient, never written to the file")
            .build();

        let route = Rc::new(Self {
            engine,
            chrome: RefCell::new(None),
            store: RefCell::new(Store::load()),
            rows: RefCell::new(Vec::new()),
            syncing,
            split: adw::NavigationSplitView::new(),
            stack,
            list,
            banner,
            zoom_row,
            mux,
            hosts: HostsPage::new(),
            zoom: UiZoom::default(),
            css: gtk::CssProvider::new(),
            route: RefCell::new(None),
            prompted: Cell::new(false),
        });
        route.build_surface();
        route.connect_mux_save();
        route.connect_host_edits();
        route.install_css();
        route.apply_file();
        route
    }

    fn connect_mux_save(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        self.mux.connect_save(Rc::new(move |source: &str| {
            let Some(route) = target.upgrade() else {
                return;
            };
            let Some(path) = route.mux.path() else {
                return;
            };
            match crate::config::file::write_editor_source(
                path,
                source,
                import::MAX_MUX_CONFIG_BYTES,
            ) {
                Ok(()) => {
                    route
                        .engine
                        .execute(CommandInvocation::new("reload-config", [] as [&str; 0]));
                    route.mux.note("Saved. The daemon is re-sourcing the file.");
                }
                Err(error) => route.mux.note(&format!("Could not save: {error}")),
            }
        }));
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

    /// Put the route in front of the pane area. The pane widget and this
    /// surface become the two children of one stack, so opening settings hides
    /// the panes exactly the way the desktop's route swap does.
    ///
    /// The toolbar's content is cleared first: `panes` is already its child at
    /// this point, and adding a parented widget to the stack leaves GTK
    /// unable to reconcile the two, which it reports forever as a failure to
    /// remove a non-child.
    pub fn install(self: &Rc<Self>, toolbar: &adw::ToolbarView, panes: &impl IsA<gtk::Widget>) {
        toolbar.set_content(gtk::Widget::NONE);
        let route = gtk::Stack::new();
        route.add_named(panes, Some("panes"));
        route.add_named(&self.split, Some("settings"));
        toolbar.set_content(Some(&route));
        self.route.replace(Some(route.clone()));

        let target = Rc::downgrade(self);
        glib::timeout_add_local(POLL_INTERVAL, move || tick(&target));

        let target = Rc::downgrade(self);
        route.connect_map(move |_| {
            if let Some(route) = target.upgrade() {
                route.install_window_action();
                route.prompt_import();
            }
        });
    }

    /// The window only exists once the tree is mapped, so the deep-link action
    /// is registered then rather than at construction.
    fn install_window_action(self: &Rc<Self>) {
        let Some(window) = self.split.root().and_downcast::<adw::ApplicationWindow>() else {
            return;
        };
        if window.lookup_action("settings-page").is_some() {
            return;
        }
        let action = gio::SimpleAction::new("settings-page", Some(glib::VariantTy::STRING));
        let target = Rc::downgrade(self);
        action.connect_activate(move |_, parameter| {
            let (Some(route), Some(name)) =
                (target.upgrade(), parameter.and_then(glib::Variant::str))
            else {
                return;
            };
            route.open_at(name);
        });
        window.add_action(&action);
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
                .set_subtitle(&format!("{}%  ·  transient", self.zoom.percent()));
        }
        moved
    }

    pub fn is_open(&self) -> bool {
        self.route
            .borrow()
            .as_ref()
            .is_some_and(|route| route.visible_child_name().as_deref() == Some("settings"))
    }

    pub fn open(&self) {
        self.open_at(Page::Interface.name());
    }

    /// Open the route on a named page. `win.settings-page` reaches this, which
    /// is how a menu item, another surface, or a script deep-links into one
    /// page the way a desktop settings app does.
    pub fn open_at(&self, name: &str) {
        let page = Page::ALL
            .iter()
            .position(|page| page.name() == name)
            .unwrap_or(0);
        self.show("settings");
        self.mux.reload();
        self.refresh_rows();
        self.list
            .select_row(self.list.row_at_index(page as i32).as_ref());
        self.split.grab_focus();
    }

    pub fn close(&self) {
        self.show("panes");
    }

    pub fn toggle(&self) {
        if self.is_open() {
            self.close();
        } else {
            self.open();
        }
    }

    fn show(&self, name: &str) {
        let Some(route) = self.route.borrow().clone() else {
            return;
        };
        route.set_visible_child_name(name);
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

    fn build_surface(self: &Rc<Self>) {
        let write = self.writer();
        let mut rows = Vec::new();
        for page in Page::ALL {
            let content = adw::PreferencesPage::new();
            for group in schema::groups(page) {
                let section = adw::PreferencesGroup::builder().title(group).build();
                for setting in schema::for_page(page).filter(|s| s.group == group) {
                    let row = Row::build(setting, &write, &self.syncing);
                    section.add(row.widget());
                    rows.push(row);
                }
                content.add(&section);
            }
            let child: gtk::Widget = match page {
                Page::Interface => {
                    let zoom = adw::PreferencesGroup::builder().title("Zoom").build();
                    self.zoom_row.add_suffix(&self.zoom_buttons());
                    zoom.add(&self.zoom_row);
                    content.add(&zoom);
                    content.upcast()
                }
                Page::Multiplexer => {
                    content.add(self.mux.group());
                    content.upcast()
                }
                Page::Hosts => self.hosts.widget().clone().upcast(),
                _ => content.upcast(),
            };
            self.stack.add_named(&child, Some(page.name()));
            self.list.append(&sidebar_row(page));
        }
        self.rows.replace(rows);

        let target = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, row| {
            let (Some(route), Some(row)) = (target.upgrade(), row) else {
                return;
            };
            let Some(page) = Page::ALL.get(row.index().max(0) as usize) else {
                return;
            };
            route.stack.set_visible_child_name(page.name());
        });

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&self.banner);
        content.append(&self.stack);
        self.stack.set_vexpand(true);
        self.split
            .set_sidebar(Some(&adw::NavigationPage::new(&self.sidebar(), "Settings")));
        self.split
            .set_content(Some(&adw::NavigationPage::new(&content, "Settings")));
        self.split.set_min_sidebar_width(SIDEBAR_WIDTH);
        self.split.set_max_sidebar_width(SIDEBAR_WIDTH);
        self.install_keys();
    }

    fn sidebar(self: &Rc<Self>) -> gtk::Box {
        let scroller = gtk::ScrolledWindow::builder()
            .child(&self.list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let done = gtk::Button::with_label("Done");
        done.set_margin_top(6);
        done.set_margin_bottom(6);
        done.set_margin_start(6);
        done.set_margin_end(6);
        let target = Rc::downgrade(self);
        done.connect_clicked(move |_| {
            if let Some(route) = target.upgrade() {
                route.close();
            }
        });
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&scroller);
        column.append(&done);
        column
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

    /// Escape and the `ClosePane` chord both leave, matching the desktop's route
    /// semantics; every other chrome chord is handed to the window so zoom and
    /// detach keep working while settings holds the keyboard.
    fn install_keys(self: &Rc<Self>) {
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(route) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if keyval == gtk::gdk::Key::Escape {
                route.close();
                return glib::Propagation::Stop;
            }
            if keys::is_modifier(keyval) {
                return glib::Propagation::Proceed;
            }
            let input = keys::key_input(KeyAction::Press, keyval, modifiers, None);
            let Some(action) = resolve(route.engine.chrome(), &input) else {
                return glib::Propagation::Proceed;
            };
            match action {
                ChromeAction::ClosePane | ChromeAction::OpenSettings => route.close(),
                other => {
                    if let Some(chrome) = route.chrome.borrow().as_ref() {
                        chrome(other);
                    }
                }
            }
            glib::Propagation::Stop
        });
        self.split.add_controller(keyboard);
    }

    /// Every control writes through here: file first, apply second. Nothing in
    /// the surface mutates live state on its own.
    fn writer(self: &Rc<Self>) -> Write {
        let target = Rc::downgrade(self);
        Rc::new(move |setting: &'static Setting, value: Option<String>| {
            let Some(route) = target.upgrade() else {
                return;
            };
            match crate::config::write(setting.key, value.as_deref()) {
                Ok(()) => route.banner.set_revealed(false),
                Err(error) => {
                    log::warn!("zz-gtk could not write {}: {error}", setting.key);
                    route
                        .banner
                        .set_title(&format!("Could not write {}: {error}", setting.key));
                    route.banner.set_revealed(true);
                    return;
                }
            }
            route.store.borrow_mut().invalidate();
            route.tick();
        })
    }

    fn tick(self: &Rc<Self>) {
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
        apply_theme_mode(state);
        // The fleet is a config value like any other: the poll is what adds and
        // removes hosts, so a hand edit and this surface cannot disagree.
        self.engine.set_fleet_hosts(state.fleet_hosts());
        let listed: Vec<(String, String)> = state
            .fleet_hosts()
            .iter()
            .map(|host| (host.name.clone(), host.endpoint.to_string()))
            .collect();
        drop(store);
        self.hosts.refresh(&listed);
        self.restyle();
        self.send_overrides(self.store.borrow().state());
        self.refresh_rows();
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

    /// The one-time offer to adopt an existing Ghostty or tmux configuration.
    /// The marker is written on either answer, so declining is remembered.
    fn prompt_import(self: &Rc<Self>) {
        if self.prompted.replace(true) || !import::prompt_pending() {
            return;
        }
        let dialog = adw::AlertDialog::new(
            Some("Import your existing configuration?"),
            Some(
                "zz found a Ghostty or tmux configuration. zz reads only its own files: the \
                 Ghostty appearance keys are copied into zz/config and your tmux configuration \
                 into zz/mux.conf. The originals are never modified, and you can import later \
                 from Settings.",
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
        dialog.present(Some(&self.split));
    }

    fn run_import(self: &Rc<Self>) {
        match import::run() {
            Ok(report) => {
                if report.mux_path.is_some() {
                    self.mux.reload();
                    self.engine
                        .execute(CommandInvocation::new("reload-config", [] as [&str; 0]));
                }
                self.store.borrow_mut().invalidate();
                self.tick();
                self.banner.set_title(if report.imported_anything() {
                    "Imported. Your Ghostty and tmux originals were not touched."
                } else {
                    "Nothing to import."
                });
                self.banner.set_revealed(true);
            }
            Err(error) => {
                self.banner.set_title(&format!("Import failed: {error}"));
                self.banner.set_revealed(true);
            }
        }
    }
}

fn tick(target: &Weak<SettingsRoute>) -> glib::ControlFlow {
    let Some(route) = target.upgrade() else {
        return glib::ControlFlow::Break;
    };
    route.tick();
    glib::ControlFlow::Continue
}

/// The same two tables a terminal surface consults, so a `chrome-bind` of
/// `ClosePane` in either of them still closes the route.
fn resolve(chrome: &ChromeKeymap, input: &zz_terminal::KeyInput) -> Option<ChromeAction> {
    chrome
        .resolve(zz_client::UI_TABLE, input)
        .or_else(|| chrome.resolve(zz_client::TERMINAL_TABLE, input))
}

fn sidebar_row(page: Page) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(page.title()).build();
    row.add_prefix(&gtk::Image::from_icon_name(page.icon()));
    row
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

    /// Every row the surface still offers is one this client or the daemon
    /// behind it acts on: a key nobody reads has no business being drawn.
    #[test]
    fn the_table_offers_no_key_this_shell_ignores() {
        let client_keys: Vec<&str> = schema::SETTINGS
            .iter()
            .filter(|setting| setting.owner == Owner::Client)
            .map(|setting| setting.key)
            .collect();

        assert_eq!(client_keys, ["theme-mode", "quit-daemon-on-exit"]);
    }

    fn setting(key: &str) -> &'static Setting {
        schema::SETTINGS
            .iter()
            .find(|setting| setting.key == key)
            .expect("the key is in the table")
    }
}
