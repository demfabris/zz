use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::*};
use zz_terminal::{
    SearchCase, SearchDirection, SearchMode, SearchQuery, SearchStatus, TerminalViewAction,
};

/// What Enter means, which is the one thing the two ways of opening the strip
/// disagree about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Behavior {
    /// Opened by the client's own chord: Enter walks the matches.
    Navigate,
    /// Opened by the daemon's `copy-mode-search-prompt`: Enter accepts the
    /// match and closes the strip *locally*, leaving the daemon's search alive
    /// so its highlights and `n`/`N` keep working.
    AcceptAndClose,
}

/// The find bar for one terminal pane.
///
/// The query is entirely client-owned — the daemon never echoes it back — while
/// the match count, the pending flag and the invalid-pattern flag ride the
/// pane's own viewport as [`SearchStatus`]. So this widget writes
/// [`TerminalViewAction`]s out and reads `viewport.search` in.
pub struct SearchStrip {
    revealer: gtk::Revealer,
    entry: gtk::SearchEntry,
    status: gtk::Label,
    mode: gtk::ToggleButton,
    case: gtk::Button,
    direction: gtk::ToggleButton,
    query: RefCell<SearchQuery>,
    behavior: Cell<Behavior>,
    syncing: Cell<bool>,
    send: RefCell<Option<Rc<dyn Fn(TerminalViewAction)>>>,
    closed: RefCell<Option<Rc<dyn Fn()>>>,
}

impl SearchStrip {
    pub fn new() -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .hexpand(true)
            .placeholder_text("Find in pane")
            .build();

        let status = gtk::Label::builder().xalign(1.0).build();
        status.add_css_class("dim-label");
        status.add_css_class("numeric");

        let mode = gtk::ToggleButton::builder()
            .label(".*")
            .tooltip_text("Regular expression (Alt+R)")
            .has_frame(false)
            .build();
        mode.add_css_class("flat");
        let case = gtk::Button::builder()
            .label(case_label(SearchCase::Smart))
            .tooltip_text("Case sensitivity (Alt+C)")
            .has_frame(false)
            .build();
        case.add_css_class("flat");
        let direction = gtk::ToggleButton::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Search backwards")
            .has_frame(false)
            .build();
        direction.add_css_class("flat");

        let previous = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Previous match (Shift+Enter)")
            .has_frame(false)
            .build();
        let next = gtk::Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text("Next match (Enter)")
            .has_frame(false)
            .build();
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Close (Escape)")
            .has_frame(false)
            .build();

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.add_css_class("toolbar");
        row.add_css_class("zz-search");
        row.append(&entry);
        row.append(&status);
        row.append(&mode);
        row.append(&case);
        row.append(&direction);
        row.append(&previous);
        row.append(&next);
        row.append(&close);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .child(&row)
            .build();

        let strip = Rc::new(Self {
            revealer,
            entry,
            status,
            mode,
            case,
            direction,
            query: RefCell::new(SearchQuery::default()),
            behavior: Cell::new(Behavior::Navigate),
            syncing: Cell::new(false),
            send: RefCell::new(None),
            closed: RefCell::new(None),
        });
        strip.connect(&previous, &next, &close);
        strip
    }

    /// Wire the strip to its pane. `send` forwards one view action; `closed`
    /// fires whenever the strip stops being visible, so the pane can take the
    /// keyboard back.
    pub fn attach(&self, send: Rc<dyn Fn(TerminalViewAction)>, closed: Rc<dyn Fn()>) {
        self.send.replace(Some(send));
        self.closed.replace(Some(closed));
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.revealer.upcast_ref()
    }

    pub fn is_open(&self) -> bool {
        self.revealer.reveals_child()
    }

    /// Open a fresh search. Re-opening resets the query, exactly as the desktop
    /// does when its chord fires while the strip is already up.
    pub fn open(&self, direction: SearchDirection, from_daemon: bool) {
        let query = SearchQuery {
            direction,
            ..SearchQuery::default()
        };
        self.behavior.set(if from_daemon {
            Behavior::AcceptAndClose
        } else {
            Behavior::Navigate
        });
        self.query.replace(query.clone());
        self.syncing.set(true);
        self.entry.set_text("");
        self.mode.set_active(false);
        self.direction
            .set_active(direction == SearchDirection::Backward);
        self.case.set_label(case_label(SearchCase::Smart));
        self.syncing.set(false);
        self.set_status(None);
        self.revealer.set_reveal_child(true);
        self.entry.grab_focus();
        self.dispatch(TerminalViewAction::SearchBegin(query));
    }

    /// Close and tell the daemon to drop its search.
    pub fn close(&self) {
        self.hide();
        self.dispatch(TerminalViewAction::SearchClose);
    }

    /// Close without a message: the daemon keeps the search alive so its
    /// highlights and copy-mode `n`/`N` still work.
    fn accept(&self) {
        self.hide();
    }

    fn hide(&self) {
        if !self.revealer.reveals_child() {
            return;
        }
        self.revealer.set_reveal_child(false);
        if let Some(closed) = self.closed.borrow().clone() {
            closed();
        }
    }

    /// Show what the daemon found. Called only when the pane's reported search
    /// status actually changed, so a busy pane spends nothing here.
    pub fn set_status(&self, status: Option<SearchStatus>) {
        self.status.set_text(&status_text(status));
        let invalid = status.is_some_and(SearchStatus::invalid_pattern);
        if invalid {
            self.entry.add_css_class("error");
        } else {
            self.entry.remove_css_class("error");
        }
    }

    fn connect(self: &Rc<Self>, previous: &gtk::Button, next: &gtk::Button, close: &gtk::Button) {
        let target = Rc::downgrade(self);
        self.entry.connect_search_changed(move |entry| {
            let Some(strip) = target.upgrade() else {
                return;
            };
            if strip.syncing.get() {
                return;
            }
            strip.query.borrow_mut().text = entry.text().to_string();
            strip.update();
        });

        for (button, backward) in [(previous, true), (next, false)] {
            let target = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(strip) = target.upgrade() {
                    strip.jump(backward);
                }
            });
        }

        let target = Rc::downgrade(self);
        close.connect_clicked(move |_| {
            if let Some(strip) = target.upgrade() {
                strip.close();
            }
        });

        let target = Rc::downgrade(self);
        self.mode.connect_toggled(move |button| {
            let Some(strip) = target.upgrade() else {
                return;
            };
            if strip.syncing.get() {
                return;
            }
            strip.query.borrow_mut().mode = if button.is_active() {
                SearchMode::Regex
            } else {
                SearchMode::Literal
            };
            strip.update();
        });

        let target = Rc::downgrade(self);
        self.case.connect_clicked(move |_| {
            if let Some(strip) = target.upgrade() {
                strip.cycle_case();
            }
        });

        let target = Rc::downgrade(self);
        self.direction.connect_toggled(move |button| {
            let Some(strip) = target.upgrade() else {
                return;
            };
            if strip.syncing.get() {
                return;
            }
            strip.query.borrow_mut().direction = if button.is_active() {
                SearchDirection::Backward
            } else {
                SearchDirection::Forward
            };
            strip.update();
        });

        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(strip) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            strip.on_key(keyval, modifiers)
        });
        self.entry.add_controller(keyboard);
    }

    /// The chords the desktop's prompt owns. Alt+R toggles the *mode*, not the
    /// direction — the label the desktop paints says `Alt+R / Alt+C` next to
    /// `[direction, mode, case]`, and only the last two move.
    fn on_key(&self, keyval: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);
        match keyval {
            gdk::Key::Escape => self.close(),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                if self.behavior.get() == Behavior::AcceptAndClose {
                    self.accept();
                } else {
                    self.jump(modifiers.contains(gdk::ModifierType::SHIFT_MASK));
                }
            }
            gdk::Key::r | gdk::Key::R if alt => self.mode.set_active(!self.mode.is_active()),
            gdk::Key::c | gdk::Key::C if alt => self.cycle_case(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Shift flips whichever way the query is pointing, so Shift+Enter always
    /// walks the matches the other way.
    fn jump(&self, flip: bool) {
        let backward = (self.query.borrow().direction == SearchDirection::Backward) ^ flip;
        self.dispatch(if backward {
            TerminalViewAction::SearchPrevious
        } else {
            TerminalViewAction::SearchNext
        });
    }

    fn cycle_case(&self) {
        let case = {
            let mut query = self.query.borrow_mut();
            query.case = match query.case {
                SearchCase::Smart => SearchCase::Sensitive,
                SearchCase::Sensitive => SearchCase::Insensitive,
                SearchCase::Insensitive => SearchCase::Smart,
            };
            query.case
        };
        self.case.set_label(case_label(case));
        self.update();
    }

    fn update(&self) {
        let query = self.query.borrow().clone();
        self.dispatch(TerminalViewAction::SearchUpdate(query));
    }

    fn dispatch(&self, action: TerminalViewAction) {
        if let Some(send) = self.send.borrow().clone() {
            send(action);
        }
    }
}

fn case_label(case: SearchCase) -> &'static str {
    match case {
        SearchCase::Smart => "Aa?",
        SearchCase::Sensitive => "Aa",
        SearchCase::Insensitive => "aa",
    }
}

/// The desktop prompt's tail, minus its `[direction, mode, case]` legend, which
/// the toolbar buttons carry here instead.
fn status_text(status: Option<SearchStatus>) -> String {
    let Some(status) = status else {
        return String::new();
    };
    if status.invalid_pattern() {
        "invalid pattern".to_owned()
    } else if status.pending() {
        "searching…".to_owned()
    } else if status.total == 0 {
        "0/0".to_owned()
    } else {
        format!("{}/{}", status.current(), status.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_spells_out_every_state_the_daemon_reports() {
        assert_eq!(status_text(None), "");
        assert_eq!(status_text(Some(SearchStatus::new(3, 17))), "3/17");
        assert_eq!(status_text(Some(SearchStatus::new(0, 0))), "0/0");
        assert_eq!(
            status_text(Some(SearchStatus::new(0, 0).with_pending(true))),
            "searching…"
        );
        assert_eq!(
            status_text(Some(
                SearchStatus::new(2, 5)
                    .with_pending(true)
                    .with_invalid_pattern(true)
            )),
            "invalid pattern",
            "an unusable pattern outranks a search still running"
        );
    }

    #[test]
    fn case_cycles_through_all_three_modes() {
        assert_eq!(case_label(SearchCase::Smart), "Aa?");
        assert_eq!(case_label(SearchCase::Sensitive), "Aa");
        assert_eq!(case_label(SearchCase::Insensitive), "aa");
    }
}
