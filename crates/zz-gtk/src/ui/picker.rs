use std::{rc::Rc, sync::Arc};

use adw::prelude::*;
use gtk::{gdk, glib};
use zz_protocol::{CommandInvocation, PaneId};

use crate::engine::Engine;

/// What a fresh split can become. The daemon models an unclaimed pane as
/// `PaneKindSnapshot::Picker` and waits for `select-pane-kind`; the order and
/// the shortcuts are the desktop's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Terminal,
    Browser,
    Editor,
    Agent,
}

const CHOICES: [Choice; 4] = [
    Choice::Terminal,
    Choice::Browser,
    Choice::Editor,
    Choice::Agent,
];

impl Choice {
    const fn argument(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Browser => "browser",
            Self::Editor => "editor",
            Self::Agent => "agent",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Browser => "Browser",
            Self::Editor => "Editor",
            Self::Agent => "Agent",
        }
    }

    const fn shortcut(self) -> &'static str {
        match self {
            Self::Terminal => "t",
            Self::Browser => "b",
            Self::Editor => "e",
            Self::Agent => "a",
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::Terminal => "utilities-terminal-symbolic",
            Self::Browser => "web-browser-symbolic",
            Self::Editor => "text-editor-symbolic",
            Self::Agent => "system-run-symbolic",
        }
    }

    /// Only terminals render here. The daemon would happily materialize the
    /// other three, and then this client would have nothing to draw — so the
    /// row says so instead of creating a pane nobody can use.
    const fn available(self) -> bool {
        matches!(self, Self::Terminal)
    }
}

/// The chooser a newly split pane shows until someone says what it is.
pub struct PanePicker {
    root: gtk::Box,
    list: gtk::ListBox,
    engine: Arc<Engine>,
    pane: PaneId,
}

impl PanePicker {
    pub fn new(engine: Arc<Engine>, pane: PaneId) -> Rc<Self> {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("boxed-list");
        for choice in CHOICES {
            list.append(&row(choice));
        }
        list.select_row(list.row_at_index(0).as_ref());

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

        let picker = Rc::new(Self {
            root,
            list,
            engine,
            pane,
        });
        picker.connect();
        picker
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub fn grab_focus(&self) -> bool {
        self.root.grab_focus()
    }

    fn connect(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(picker) = target.upgrade() else {
                return;
            };
            let index = usize::try_from(row.index()).unwrap_or(0);
            if let Some(choice) = CHOICES.get(index).copied() {
                picker.activate(choice);
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
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                if let Some(choice) = self.selected() {
                    self.activate(choice);
                }
            }
            gdk::Key::j | gdk::Key::Down => self.step(1),
            gdk::Key::k | gdk::Key::Up => self.step(-1),
            _ => {
                let Some(choice) = keyval
                    .to_unicode()
                    .map(|character| character.to_ascii_lowercase())
                    .and_then(|character| {
                        CHOICES
                            .into_iter()
                            .find(|choice| choice.shortcut() == character.to_string())
                    })
                else {
                    return glib::Propagation::Proceed;
                };
                self.activate(choice);
            }
        }
        glib::Propagation::Stop
    }

    fn selected(&self) -> Option<Choice> {
        let index = usize::try_from(self.list.selected_row()?.index()).ok()?;
        CHOICES.get(index).copied()
    }

    fn step(&self, delta: i32) {
        let count = i32::try_from(CHOICES.len()).unwrap_or(1);
        let current = self.list.selected_row().map_or(0, |row| row.index());
        let next = (current + delta).rem_euclid(count);
        self.list.select_row(self.list.row_at_index(next).as_ref());
    }

    /// `select-pane-kind` keeps the pane's id, so whatever the shell already
    /// laid out simply changes what it draws.
    fn activate(&self, choice: Choice) {
        if !choice.available() {
            return;
        }
        self.engine.execute(CommandInvocation::new(
            "select-pane-kind",
            ["-t", &self.pane.to_string(), choice.argument()],
        ));
    }
}

fn row(choice: Choice) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(choice.title())
        .activatable(true)
        .build();
    if !choice.available() {
        row.set_subtitle("Requires the zz app");
        row.set_sensitive(false);
    }
    row.add_prefix(&gtk::Image::from_icon_name(choice.icon()));
    let key = gtk::Label::new(Some(choice.shortcut()));
    key.add_css_class("dim-label");
    key.add_css_class("monospace");
    row.add_suffix(&key);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_and_their_shortcuts_match_the_desktop() {
        let spelled: Vec<(&str, &str)> = CHOICES
            .into_iter()
            .map(|choice| (choice.argument(), choice.shortcut()))
            .collect();

        assert_eq!(
            spelled,
            vec![
                ("terminal", "t"),
                ("browser", "b"),
                ("editor", "e"),
                ("agent", "a"),
            ]
        );
    }

    #[test]
    fn only_terminals_can_be_created_from_this_client() {
        assert!(Choice::Terminal.available());
        for choice in [Choice::Browser, Choice::Editor, Choice::Agent] {
            assert!(!choice.available(), "{} must stay gated", choice.title());
        }
    }
}
