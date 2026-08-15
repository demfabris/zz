use std::{rc::Rc, sync::Arc};

use adw::prelude::*;
use gtk::{gdk, glib};
use zz_protocol::{CommandInvocation, PaneId};

use crate::engine::Engine;

/// The daemon models an unclaimed split as `PaneKindSnapshot::Picker` and waits
/// for `select-pane-kind`. Terminal is the only kind this shell can draw —
/// browser, agent and editor panes are local runtimes the zz app owns — so it
/// is the only kind it offers to create. A pane of another kind made elsewhere
/// still renders its placeholder card.
const CHOICE: &str = "terminal";

/// The chooser a newly split pane shows until someone says what it is.
pub struct PanePicker {
    root: gtk::Box,
    engine: Arc<Engine>,
    pane: PaneId,
}

impl PanePicker {
    pub fn new(engine: Arc<Engine>, pane: PaneId) -> Rc<Self> {
        let row = adw::ActionRow::builder()
            .title("Terminal")
            .activatable(true)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("utilities-terminal-symbolic"));
        let key = gtk::Label::new(Some("t"));
        key.add_css_class("dim-label");
        key.add_css_class("monospace");
        row.add_suffix(&key);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.add_css_class("boxed-list");
        list.append(&row);

        let hint = gtk::Label::builder()
            .label("Enter choose · Esc close the pane")
            .build();
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

        let picker = Rc::new(Self { root, engine, pane });
        picker.connect(&row);
        picker
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub fn grab_focus(&self) -> bool {
        self.root.grab_focus()
    }

    fn connect(self: &Rc<Self>, row: &adw::ActionRow) {
        let target = Rc::downgrade(self);
        row.connect_activated(move |_| {
            if let Some(picker) = target.upgrade() {
                picker.choose();
            }
        });

        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(picker) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            picker.on_key(keyval, modifiers)
        });
        self.root.add_controller(keyboard);

        let click = gtk::GestureClick::new();
        let target = Rc::downgrade(self);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(picker) = target.upgrade() {
                picker.root.grab_focus();
                picker.engine.select_pane(picker.pane);
            }
        });
        self.root.add_controller(click);
    }

    /// A modified press is somebody else's chord; the picker only claims plain
    /// keys, which is what keeps the prefix reachable from an empty pane.
    fn on_key(&self, keyval: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        if modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        match keyval {
            gdk::Key::Escape => self.engine.kill_pane(self.pane),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter | gdk::Key::t => {
                self.choose();
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// `select-pane-kind` keeps the pane's id, so whatever the shell already
    /// laid out simply changes what it draws.
    fn choose(&self) {
        self.engine.execute(choose_command(self.pane));
    }
}

fn choose_command(pane: PaneId) -> CommandInvocation {
    CommandInvocation::new("select-pane-kind", ["-t", &pane.to_string(), CHOICE])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_claims_the_pane_the_daemon_is_waiting_on() {
        let command = choose_command(PaneId(7));

        assert_eq!(command.name, "select-pane-kind");
        assert_eq!(command.args, ["-t", "%7", "terminal"]);
    }
}
