//! The two pages the key table cannot generate: a whole-file editor, and the
//! fleet.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use adw::prelude::*;

use crate::config::{file, import};

/// `zz/mux.conf`, edited whole. The daemon owns the tmux dialect, so there is
/// nothing sensible to render as typed rows: the file is the interface, and
/// saving is explicit because half a config is worse than none.
pub struct MuxEditor {
    group: adw::PreferencesGroup,
    view: gtk::TextView,
    status: gtk::Label,
    path: Option<PathBuf>,
    save: RefCell<Option<Rc<dyn Fn(&str)>>>,
}

impl MuxEditor {
    pub fn new() -> Rc<Self> {
        let path = zz_daemon::mux_config_write_path();
        let view = gtk::TextView::builder()
            .monospace(true)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .min_content_height(320)
            .vexpand(true)
            .build();
        scroller.add_css_class("card");

        let status = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .margin_top(6)
            .build();
        status.add_css_class("dim-label");
        status.add_css_class("caption");

        let button = gtk::Button::with_label("Save and reload");
        button.add_css_class("suggested-action");
        button.set_valign(gtk::Align::Center);
        button.set_sensitive(path.is_some());

        let group = adw::PreferencesGroup::builder()
            .title("zz/mux.conf")
            .description(path.as_ref().map_or_else(
                || "No writable location for zz/mux.conf.".to_owned(),
                |path| path.display().to_string(),
            ))
            .header_suffix(&button)
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&scroller);
        column.append(&status);
        group.add(&column);

        let editor = Rc::new(Self {
            group,
            view,
            status,
            path,
            save: RefCell::new(None),
        });
        let target = Rc::downgrade(&editor);
        button.connect_clicked(move |_| {
            let Some(editor) = target.upgrade() else {
                return;
            };
            let save = editor.save.borrow().clone();
            if let Some(save) = save {
                save(&editor.text());
            }
        });
        editor.reload();
        editor
    }

    /// Saving needs the route that owns the daemon connection, which does not
    /// exist yet when the widget tree is built.
    pub fn connect_save(&self, save: Rc<dyn Fn(&str)>) {
        self.save.replace(Some(save));
    }

    pub fn group(&self) -> &adw::PreferencesGroup {
        &self.group
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    pub fn text(&self) -> String {
        let buffer = self.view.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }

    pub fn note(&self, message: &str) {
        self.status.set_text(message);
    }

    /// Re-read the file. Only called when the editor is opened or a save
    /// succeeds, so an in-progress edit is never thrown away underneath.
    pub fn reload(&self) {
        let Some(path) = self.path.as_deref() else {
            self.note("Set XDG_CONFIG_HOME or HOME to give zz/mux.conf a home.");
            return;
        };
        match file::read_editor_source(path, import::MAX_MUX_CONFIG_BYTES) {
            Ok(source) => {
                self.view.buffer().set_text(&source);
                self.note("");
            }
            Err(error) => self.note(&format!("Could not read the file: {error}")),
        }
    }
}

/// The fleet, as `zz/config` spells it. Every row here is one `host-<name>`
/// line: adding writes one, removing deletes every copy of one, and the poll is
/// what turns either into a connection — so this page and the sidebar's Add
/// host are the same edit made from two places.
pub struct HostsPage {
    page: adw::PreferencesPage,
    list: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
    entry: adw::EntryRow,
    status: gtk::Label,
    remove: RefCell<Option<Rc<dyn Fn(&str)>>>,
    add: RefCell<Option<Rc<dyn Fn(&str)>>>,
}

impl HostsPage {
    pub fn new() -> Rc<Self> {
        let page = adw::PreferencesPage::new();
        let list = adw::PreferencesGroup::builder()
            .title("Hosts")
            .description(
                "A host is another machine's zz daemon, reached over plain ssh. Sessions on it \
                 appear in the sidebar under their own root.",
            )
            .build();
        page.add(&list);

        let entry = adw::EntryRow::builder().title("user@desktop").build();
        let button = gtk::Button::from_icon_name("list-add-symbolic");
        button.add_css_class("flat");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some("Add this host"));
        entry.add_suffix(&button);

        let status = gtk::Label::builder().xalign(0.0).wrap(true).build();
        status.add_css_class("dim-label");
        status.add_css_class("caption");

        let form = adw::PreferencesGroup::builder().title("Add a host").build();
        form.add(&entry);
        form.add(&status);
        page.add(&form);

        let hosts = Rc::new(Self {
            page,
            list,
            rows: RefCell::new(Vec::new()),
            entry,
            status,
            remove: RefCell::new(None),
            add: RefCell::new(None),
        });
        let target = Rc::downgrade(&hosts);
        button.connect_clicked(move |_| {
            if let Some(hosts) = target.upgrade() {
                hosts.submit();
            }
        });
        let target = Rc::downgrade(&hosts);
        hosts.entry.connect_entry_activated(move |_| {
            if let Some(hosts) = target.upgrade() {
                hosts.submit();
            }
        });
        hosts
    }

    pub fn widget(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// Both edits need the route that owns the file writer, which does not
    /// exist yet when the widget tree is built.
    pub fn connect(&self, add: Rc<dyn Fn(&str)>, remove: Rc<dyn Fn(&str)>) {
        self.add.replace(Some(add));
        self.remove.replace(Some(remove));
    }

    pub fn note(&self, message: &str) {
        self.status.set_text(message);
    }

    pub fn clear_entry(&self) {
        self.entry.set_text("");
    }

    /// Re-list from the file. Called on every poll that changed something, so
    /// a host added by hand in an editor shows up here too.
    pub fn refresh(self: &Rc<Self>, hosts: &[(String, String)]) {
        for row in self.rows.borrow_mut().drain(..) {
            self.list.remove(&row);
        }
        if hosts.is_empty() {
            let empty = adw::ActionRow::builder()
                .title("No hosts")
                .subtitle("Add one below, or from the sidebar's host menu.")
                .build();
            self.list.add(&empty);
            self.rows.borrow_mut().push(empty);
            return;
        }
        for (name, endpoint) in hosts {
            let row = adw::ActionRow::builder()
                .title(name)
                .subtitle(endpoint)
                .build();
            let button = gtk::Button::from_icon_name("user-trash-symbolic");
            button.add_css_class("flat");
            button.set_valign(gtk::Align::Center);
            button.set_tooltip_text(Some("Remove this host"));
            let target = Rc::downgrade(self);
            let name = name.clone();
            button.connect_clicked(move |_| {
                let Some(hosts) = target.upgrade() else {
                    return;
                };
                let remove = hosts.remove.borrow().clone();
                if let Some(remove) = remove {
                    remove(&name);
                }
            });
            row.add_suffix(&button);
            self.list.add(&row);
            self.rows.borrow_mut().push(row);
        }
    }

    fn submit(&self) {
        let typed = self.entry.text().to_string();
        if typed.trim().is_empty() {
            return;
        }
        let add = self.add.borrow().clone();
        if let Some(add) = add {
            add(&typed);
        }
    }
}
