use std::{rc::Rc, sync::Arc};

use adw::prelude::*;
use gtk::glib;
use zz_protocol::{CommandInvocation, KeyBindingSnapshot};

use crate::engine::Engine;

/// The daemon's own default, printed until it publishes one of its own.
pub const DEFAULT_PREFIX: &str = "C-b";

struct Hint {
    prefixed: bool,
    key: &'static str,
    label: &'static str,
}

/// A hint whose key is whatever the daemon's prefix table actually binds to the
/// command, so a rebound key is the one the card teaches.
struct BindingHint {
    hint: Hint,
    matches: fn(&CommandInvocation) -> bool,
}

const NEW_SESSION: Hint = Hint {
    prefixed: false,
    key: "Enter",
    label: "New session",
};

const BINDINGS: [BindingHint; 5] = [
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "c",
            label: "New window",
        },
        matches: |command| command.name == "new-window",
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "%",
            label: "Split right",
        },
        matches: |command| {
            command.name == "split-window" && command.args.iter().any(|arg| arg == "-h")
        },
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "\"",
            label: "Split down",
        },
        matches: |command| {
            command.name == "split-window" && command.args.iter().all(|arg| arg != "-h")
        },
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "s",
            label: "Sessions and windows",
        },
        matches: |command| matches!(command.name.as_str(), "focus-sidebar" | "choose-tree"),
    },
    BindingHint {
        hint: Hint {
            prefixed: true,
            key: "?",
            label: "Every key binding",
        },
        matches: |command| command.name == "list-keys",
    },
];

/// The zero-session surface: the one action available here, and the keys it
/// unlocks. The hints are read from the daemon's published prefix table rather
/// than guessed, so a user who rebound them reads their own chords.
pub struct NewSessionPanel {
    engine: Arc<Engine>,
    root: gtk::Box,
    hints: gtk::Box,
}

impl NewSessionPanel {
    pub fn new(engine: Arc<Engine>) -> Rc<Self> {
        let hints = gtk::Box::new(gtk::Orientation::Vertical, 2);

        let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
        card.add_css_class("zz-newsession");
        card.set_halign(gtk::Align::Center);
        card.set_valign(gtk::Align::Center);
        card.set_vexpand(true);

        let action = gtk::Button::builder()
            .child(&hint_row(&NEW_SESSION, None, Some(NEW_SESSION.key), true))
            .has_frame(false)
            .build();
        card.append(&action);
        card.append(&hints);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&card);

        let panel = Rc::new(Self {
            engine,
            root,
            hints,
        });
        let target = Rc::downgrade(&panel);
        action.connect_clicked(move |_| {
            if let Some(panel) = target.upgrade() {
                panel.engine.new_session();
            }
        });
        panel.refresh();
        panel
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// Rebuild the hint list from the daemon's current prefix table.
    pub fn refresh(&self) {
        while let Some(child) = self.hints.first_child() {
            self.hints.remove(&child);
        }
        let prefix = self
            .engine
            .prefix_chord()
            .unwrap_or_else(|| DEFAULT_PREFIX.to_owned());
        let bindings = self.engine.prefix_bindings();
        let section = gtk::Label::builder()
            .label("In a session")
            .xalign(0.0)
            .build();
        section.add_css_class("dim-label");
        section.add_css_class("caption");
        section.add_css_class("zz-newsession-section");
        self.hints.append(&section);
        for hint in &BINDINGS {
            if let Some(key) = resolve_binding_key(hint, &bindings) {
                self.hints
                    .append(&hint_row(&hint.hint, Some(&prefix), Some(&key), false));
            }
        }
    }
}

/// The stock key stands in until the daemon publishes a table, and a binding
/// the user moved wins over the stock spelling of the same command.
fn resolve_binding_key(hint: &BindingHint, bindings: &[KeyBindingSnapshot]) -> Option<String> {
    if bindings.is_empty() {
        return Some(hint.hint.key.to_owned());
    }
    let binding = bindings
        .iter()
        .filter(|binding| {
            binding
                .commands
                .first()
                .is_some_and(|command| (hint.matches)(command))
        })
        .min_by_key(|binding| binding.key == hint.hint.key)?;
    if binding.key.is_empty() {
        return Some(hint.hint.key.to_owned());
    }
    Some(binding.key.clone())
}

fn hint_row(hint: &Hint, prefix: Option<&str>, key: Option<&str>, strong: bool) -> gtk::Box {
    let keys = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    keys.set_halign(gtk::Align::End);
    keys.add_css_class("zz-newsession-keys");
    if hint.prefixed
        && let Some(prefix) = prefix
    {
        keys.append(&kbd(prefix));
    }
    if let Some(key) = key {
        keys.append(&kbd(key));
    }

    let label = gtk::Label::builder()
        .label(hint.label)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    if !strong {
        label.add_css_class("dim-label");
    }

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.append(&keys);
    row.append(&label);
    row
}

/// Keys arrive in tmux grammar (`C-b`, `%`, `"`); they are printed as the
/// daemon spells them, which is the spelling a user's config binds.
fn kbd(key: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&glib::markup_escape_text(key)));
    label.add_css_class("zz-kbd");
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(key: &str, name: &str, args: &[&str]) -> KeyBindingSnapshot {
        KeyBindingSnapshot {
            key: key.to_owned(),
            commands: vec![CommandInvocation::new(name, args.to_vec())],
            repeat: false,
            note: None,
        }
    }

    #[test]
    fn an_empty_table_teaches_the_stock_keys() {
        for hint in &BINDINGS {
            assert_eq!(
                resolve_binding_key(hint, &[]).as_deref(),
                Some(hint.hint.key)
            );
        }
    }

    #[test]
    fn a_rebound_key_wins_over_the_stock_one() {
        let bindings = [
            binding("%", "split-window", &["-h"]),
            binding("|", "split-window", &["-h"]),
        ];

        assert_eq!(
            resolve_binding_key(&BINDINGS[1], &bindings).as_deref(),
            Some("|")
        );
    }

    #[test]
    fn a_command_nothing_binds_drops_its_row() {
        let bindings = [binding("c", "new-window", &[])];

        assert_eq!(
            resolve_binding_key(&BINDINGS[0], &bindings).as_deref(),
            Some("c")
        );
        assert_eq!(resolve_binding_key(&BINDINGS[1], &bindings), None);
    }

    #[test]
    fn an_unspellable_binding_falls_back_to_the_stock_key() {
        let bindings = [binding("", "list-keys", &[])];

        assert_eq!(
            resolve_binding_key(&BINDINGS[4], &bindings).as_deref(),
            Some("?")
        );
    }
}
