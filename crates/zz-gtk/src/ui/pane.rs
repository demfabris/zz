use std::{cell::Cell, fmt::Write as _};

use gtk::prelude::*;
use zz_client::ViewportDamage;
use zz_protocol::PaneIndicator;
use zz_terminal::{SearchStatus, TerminalMode, TerminalViewport};

use crate::ui::terminal::TerminalView;

/// A terminal surface plus the chrome that belongs on top of it: the copy-mode
/// and search indicator the daemon reports through the pane's own viewport, and
/// the number `display-panes` paints over it.
pub struct TerminalPane {
    root: gtk::Overlay,
    view: TerminalView,
    badge: gtk::Label,
    number: gtk::Label,
    shown: Cell<Indicator>,
}

type Indicator = (TerminalMode, Option<SearchStatus>);

impl TerminalPane {
    pub fn new(view: TerminalView) -> Self {
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

        let root = gtk::Overlay::new();
        root.set_child(Some(&view));
        root.add_overlay(&badge);
        root.add_overlay(&number);
        Self {
            root,
            view,
            badge,
            number,
            shown: Cell::new((TerminalMode::Live, None)),
        }
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

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    pub const fn view(&self) -> &TerminalView {
        &self.view
    }

    /// The frame path: the indicator is only re-rendered when the daemon
    /// actually changed mode or search, so a busy pane allocates nothing here.
    pub fn apply_frame(&self, viewport: TerminalViewport, damage: &ViewportDamage) {
        let indicator = (viewport.mode, viewport.search);
        self.view.apply_frame(viewport, damage);
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

fn badge_text((mode, search): Indicator) -> Option<String> {
    let mut text = match mode {
        TerminalMode::Live => String::new(),
        TerminalMode::Copy { position, total } => format!("COPY {position}/{total}"),
        TerminalMode::View { position, total } => format!("VIEW {position}/{total}"),
    };
    if let Some(search) = search {
        if !text.is_empty() {
            text.push_str(" · ");
        }
        write!(text, "find {}/{}", search.current(), search.total)
            .expect("writing to a String cannot fail");
    }
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_panes_carry_no_indicator() {
        assert_eq!(badge_text((TerminalMode::Live, None)), None);
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
    fn copy_mode_and_search_share_one_indicator() {
        assert_eq!(
            badge_text((
                TerminalMode::Copy {
                    position: 12,
                    total: 340
                },
                None
            ))
            .as_deref(),
            Some("COPY 12/340")
        );
        let search = SearchStatus::new(2, 7);
        assert_eq!(
            badge_text((TerminalMode::Live, Some(search))).as_deref(),
            Some("find 2/7")
        );
        assert_eq!(
            badge_text((
                TerminalMode::View {
                    position: 1,
                    total: 9
                },
                Some(search)
            ))
            .as_deref(),
            Some("VIEW 1/9 · find 2/7")
        );
    }
}
