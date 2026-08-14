use std::{cell::Cell, fmt::Write as _, rc::Rc};

use gtk::prelude::*;
use zz_client::ViewportDamage;
use zz_protocol::{CommandInvocation, PaneIndicator};
use zz_terminal::{SearchDirection, SearchStatus, TerminalMode, TerminalViewport};

use crate::ui::{search::SearchStrip, terminal::TerminalView};

/// A terminal surface plus the chrome that belongs on top of it: the copy-mode
/// indicator the daemon reports through the pane's own viewport, the number
/// `display-panes` paints over it, the find bar, and the marks that say whether
/// this pane is the focused one, the zoomed one, or one of a synchronized set.
pub struct TerminalPane {
    root: gtk::Box,
    view: TerminalView,
    badge: gtk::Label,
    number: gtk::Label,
    marks: gtk::Box,
    sync: gtk::Label,
    unzoom: gtk::Button,
    scrim: gtk::Box,
    search: Rc<SearchStrip>,
    shown: Cell<Indicator>,
    searching: Cell<Option<SearchStatus>>,
}

type Indicator = (TerminalMode, u32);

impl TerminalPane {
    pub fn new(view: TerminalView) -> Rc<Self> {
        let badge = gtk::Label::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .visible(false)
            .can_target(false)
            .build();
        badge.add_css_class("osd");
        badge.add_css_class("caption-heading");
        badge.add_css_class("zz-badge");

        let number = gtk::Label::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .visible(false)
            .can_target(false)
            .build();
        number.add_css_class("osd");
        number.add_css_class("zz-number");

        let sync = gtk::Label::builder().label("SYNC").visible(false).build();
        sync.add_css_class("osd");
        sync.add_css_class("caption-heading");
        sync.add_css_class("zz-badge");
        let unzoom = gtk::Button::builder()
            .icon_name("view-restore-symbolic")
            .tooltip_text("Unzoom this pane")
            .visible(false)
            .has_frame(false)
            .build();
        unzoom.add_css_class("osd");
        unzoom.add_css_class("circular");
        let marks = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .spacing(4)
            .build();
        marks.add_css_class("zz-marks");
        marks.append(&sync);
        marks.append(&unzoom);

        // The scrim only shades; presses have to reach the terminal under it or
        // clicking an inactive pane could never focus it.
        let scrim = gtk::Box::builder().visible(false).can_target(false).build();
        scrim.add_css_class("zz-scrim");

        let stack = gtk::Overlay::new();
        stack.set_child(Some(&view));
        stack.add_overlay(&scrim);
        stack.add_overlay(&badge);
        stack.add_overlay(&marks);
        stack.add_overlay(&number);

        let search = SearchStrip::new();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&stack);
        root.append(search.widget());

        let pane = Rc::new(Self {
            root,
            view,
            badge,
            number,
            marks,
            sync,
            unzoom,
            scrim,
            search,
            shown: Cell::new((TerminalMode::Live, 0)),
            searching: Cell::new(None),
        });
        pane.connect();
        pane
    }

    fn connect(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        self.view.set_search_handler(Rc::new(move || {
            if let Some(pane) = target.upgrade() {
                pane.open_search(SearchDirection::Forward, false);
            }
        }));

        let view = self.view.clone();
        let closed = self.view.clone();
        self.search.attach(
            Rc::new(move |action| view.view_action(action)),
            Rc::new(move || {
                closed.grab_focus();
            }),
        );

        let target = Rc::downgrade(self);
        self.unzoom.connect_clicked(move |_| {
            let Some(pane) = target.upgrade() else {
                return;
            };
            let Some(engine) = pane.view.engine() else {
                return;
            };
            engine.execute(CommandInvocation::new(
                "resize-pane",
                ["-Z", "-t", &pane.view.pane().to_string()],
            ));
        });
    }

    /// The `display-panes` number, or nothing when the daemon's overlay expired.
    pub fn set_number(&self, indicator: Option<PaneIndicator>) {
        match indicator {
            Some(indicator) => {
                self.number.set_text(&number_text(indicator));
                self.number.set_visible(true);
            }
            None => self.number.set_visible(false),
        }
    }

    /// Where this pane stands in its window: focused, zoomed over the others,
    /// and whether `synchronize-panes` is echoing input into it.
    pub fn set_marks(&self, active: bool, zoomed: bool, synchronized: bool) {
        self.scrim.set_visible(!active);
        self.sync.set_visible(synchronized);
        self.unzoom.set_visible(zoomed);
        self.marks.set_visible(synchronized || zoomed);
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub const fn view(&self) -> &TerminalView {
        &self.view
    }

    pub fn open_search(&self, direction: SearchDirection, from_daemon: bool) {
        self.search.open(direction, from_daemon);
    }

    pub fn search_is_open(&self) -> bool {
        self.search.is_open()
    }

    /// The frame path: neither the indicator nor the find bar is re-rendered
    /// unless the daemon actually changed what they show, so a busy pane
    /// allocates nothing here beyond the frame itself.
    pub fn apply_frame(&self, viewport: TerminalViewport, damage: &ViewportDamage) {
        let indicator = (viewport.mode, viewport.unseen_output);
        let search = viewport.search;
        self.view.apply_frame(viewport, damage);
        if self.searching.get() != search {
            self.searching.set(search);
            self.search.set_status(search);
        }
        if self.shown.get() == indicator {
            return;
        }
        self.shown.set(indicator);
        match badge_text(indicator) {
            Some(text) => {
                self.badge.set_text(&text);
                self.badge.set_visible(true);
            }
            None => self.badge.set_visible(false),
        }
    }
}

fn number_text(indicator: PaneIndicator) -> String {
    match indicator.selection_key() {
        Some(key) => format!("{}  {key}", indicator.index),
        None => indicator.index.to_string(),
    }
}

/// The desktop's pill, one line instead of two: a live pane only speaks up when
/// output it has not shown piled up behind a scroll.
fn badge_text((mode, unseen): Indicator) -> Option<String> {
    let mut text = match mode {
        TerminalMode::Live => String::new(),
        TerminalMode::Copy { position, total } => format!("COPY MODE  {position}/{total}"),
        TerminalMode::View { position, total } => {
            format!("VIEW MODE  {position}/{total}  ·  q close")
        }
    };
    if unseen > 0 && !matches!(mode, TerminalMode::View { .. }) {
        if !text.is_empty() {
            text.push_str("  ·  ");
        }
        write!(text, "+{unseen} output").expect("writing to a String cannot fail");
    }
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_panes_carry_no_indicator() {
        assert_eq!(badge_text((TerminalMode::Live, 0)), None);
    }

    #[test]
    fn a_pane_number_shows_the_key_that_selects_it() {
        let indicator = PaneIndicator {
            pane: zz_protocol::PaneId(4),
            index: 2,
            select_key: b'c',
            flags: 0,
        };

        assert_eq!(number_text(indicator), "2  c");
        assert_eq!(
            number_text(PaneIndicator {
                select_key: 0,
                ..indicator
            }),
            "2"
        );
    }

    #[test]
    fn scrolled_away_output_is_counted_in_every_mode_that_can_hide_it() {
        assert_eq!(
            badge_text((TerminalMode::Live, 12)).as_deref(),
            Some("+12 output")
        );
        assert_eq!(
            badge_text((
                TerminalMode::Copy {
                    position: 12,
                    total: 340
                },
                0
            ))
            .as_deref(),
            Some("COPY MODE  12/340")
        );
        assert_eq!(
            badge_text((
                TerminalMode::Copy {
                    position: 12,
                    total: 340
                },
                3
            ))
            .as_deref(),
            Some("COPY MODE  12/340  ·  +3 output")
        );
        assert_eq!(
            badge_text((
                TerminalMode::View {
                    position: 1,
                    total: 9
                },
                7
            ))
            .as_deref(),
            Some("VIEW MODE  1/9  ·  q close"),
            "the view pill already says how to leave; the counter would crowd it"
        );
    }
}
