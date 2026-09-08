use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;

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
