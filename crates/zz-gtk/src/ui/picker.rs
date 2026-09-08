use std::{rc::Rc, sync::Arc};

use adw::prelude::*;
use gtk::{gdk, glib};
use zz_protocol::{CommandInvocation, PaneId};

use crate::engine::Engine;

const CHOICES: [(&str, &str, &str, char); 3] = [
    ("Terminal", "terminal", "utilities-terminal-symbolic", 't'),
    ("Browser", "browser", "web-browser-symbolic", 'b'),
    ("Agent", "agent", "system-run-symbolic", 'a'),
];

pub struct PanePicker {
    root: gtk::Box,
    list: gtk::ListBox,
    engine: Arc<Engine>,
    pane: PaneId,
}

impl PanePicker {
    pub fn new(engine: Arc<Engine>, pane: PaneId) -> Rc<Self> {
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        for (title, _, icon, key) in CHOICES {
            let row = adw::ActionRow::builder()
                .title(title)
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            let key = gtk::Label::new(Some(&key.to_string()));
            key.add_css_class("dim-label");
            row.add_suffix(&key);
            list.append(&row);
        }
        list.select_row(list.row_at_index(0).as_ref());
        let hint = gtk::Label::new(Some("Enter choose · Esc close the pane"));
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_halign(gtk::Align::Center);
        content.set_valign(gtk::Align::Center);
        content.append(&list);
        content.append(&hint);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("zz-picker");
        root.set_focusable(true);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&content);
        let picker = Rc::new(Self {
            root,
            list,
            engine,
            pane,
        });
        let target = Rc::downgrade(&picker);
        picker.list.connect_row_activated(move |_, row| {
            if let Some(picker) = target.upgrade() {
                picker.choose(row.index() as usize);
            }
        });
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(&picker);
        keyboard.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(picker) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if modifiers.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK,
            ) {
                return glib::Propagation::Proceed;
            }
            if key == gdk::Key::Escape {
                picker.engine.kill_pane(picker.pane);
            } else if let Some(index) = CHOICES
                .iter()
                .position(|choice| Some(choice.3) == key.to_unicode())
            {
                picker.choose(index);
            } else if matches!(
                key,
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
            ) {
                picker.choose(
                    picker
                        .list
                        .selected_row()
                        .map_or(0, |row| row.index() as usize),
                );
            } else {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        picker.root.add_controller(keyboard);
        let focus = gtk::EventControllerFocus::new();
        let target = Rc::downgrade(&picker);
        focus.connect_enter(move |_| {
            if let Some(picker) = target.upgrade() {
                picker.engine.select_pane(picker.pane);
            }
        });
        picker.root.add_controller(focus);
        picker
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub fn grab_focus(&self) -> bool {
        self.list
            .selected_row()
            .map_or_else(|| self.list.grab_focus(), |row| row.grab_focus())
    }

    fn choose(&self, index: usize) {
        if let Some((_, kind, _, _)) = CHOICES.get(index) {
            let mut args = vec!["-t".to_owned(), self.pane.to_string()];
            if *kind == "agent"
                && self.engine.active_host() == crate::engine::HostId::LOCAL
                && let Some(directory) = crate::config::current().agent_directory
            {
                args.extend(["-c".to_owned(), directory.to_string_lossy().into_owned()]);
            }
            args.push((*kind).to_owned());
            self.engine
                .execute(CommandInvocation::new("select-pane-kind", args));
        }
    }
}
