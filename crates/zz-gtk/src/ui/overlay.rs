use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use adw::prelude::*;
use gtk::{gdk, glib};
use zz_protocol::{
    ChooseBufferAction, ChooseBufferState, ChooseTreeAction, ChooseTreeItem, ChooseTreeKind,
    ChooseTreePaneKind, ChooseTreeState, ChooseTreeTarget, CommandPromptAction, InputMessage,
};
use zz_terminal::KeyAction;

use crate::{engine::Engine, ui::keys};

const CHOOSER_WIDTH: i32 = 680;
const CHOOSER_HEIGHT: i32 = 520;
const HINT: &str = "Enter select · / search · Esc close";

/// The daemon-owned overlays, rendered as GNOME chrome: the choosers as a
/// dialog over the window, the command prompt as a revealed bottom bar.
///
/// Every key press inside them is forwarded raw — the daemon's `choose-tree`,
/// `choose-buffer` and prompt tables own selection, search and submission, so
/// the client never advances a cursor of its own.
pub struct Overlays {
    engine: Arc<Engine>,
    parent: gtk::Widget,
    chooser: RefCell<Option<Chooser>>,
    prompt: Prompt,
}

/// Which chooser the daemon has open. Both carry the whole published list, so
/// a redraw is a comparison against the state that built the current rows.
#[derive(Clone, PartialEq)]
enum Chosen {
    Tree(ChooseTreeState),
    Buffer(ChooseBufferState),
}

impl Chosen {
    fn title(&self) -> &'static str {
        match self {
            Self::Tree(state) => match state.kind {
                ChooseTreeKind::Windows => "Choose window",
                ChooseTreeKind::Panes => "Choose pane",
            },
            Self::Buffer(_) => "Choose buffer",
        }
    }

    fn search(&self) -> Option<String> {
        let (query, reverse) = match self {
            Self::Tree(state) => {
                let search = state.search.as_ref()?;
                (&search.query, search.reverse)
            }
            Self::Buffer(state) => {
                let search = state.search.as_ref()?;
                (&search.query, search.reverse)
            }
        };
        Some(format!("{}{query}", if reverse { '?' } else { '/' }))
    }

    fn selected(&self) -> u32 {
        match self {
            Self::Tree(state) => state.selected,
            Self::Buffer(state) => state.selected,
        }
    }

    fn rows(&self) -> Vec<adw::ActionRow> {
        match self {
            Self::Tree(state) => state.items.iter().map(tree_row).collect(),
            Self::Buffer(state) => state
                .items
                .iter()
                .map(|item| {
                    row(
                        &item.name,
                        &format!("{} · {}", human_size(item.size_bytes), item.preview),
                    )
                })
                .collect(),
        }
    }

    fn key(&self, input: zz_terminal::KeyInput) -> InputMessage {
        match self {
            Self::Tree(_) => InputMessage::ChooseTree {
                action: ChooseTreeAction::Key(input),
            },
            Self::Buffer(_) => InputMessage::ChooseBuffer {
                action: ChooseBufferAction::Key(input),
            },
        }
    }

    fn activate(&self, index: u32) -> InputMessage {
        match self {
            Self::Tree(_) => InputMessage::ChooseTree {
                action: ChooseTreeAction::ActivateIndex(index),
            },
            Self::Buffer(_) => InputMessage::ChooseBuffer {
                action: ChooseBufferAction::PasteIndex(index),
            },
        }
    }

    /// True when the two states describe the same chooser rendered the same
    /// way, so only the selection has to move.
    fn same_shape(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Tree(current), Self::Tree(next)) => {
                current.kind == next.kind
                    && current.items == next.items
                    && current.search == next.search
            }
            (Self::Buffer(current), Self::Buffer(next)) => {
                current.items == next.items && current.search == next.search
            }
            _ => false,
        }
    }
}

struct Chooser {
    dialog: adw::Dialog,
    list: gtk::ListBox,
    search: gtk::Label,
    state: Chosen,
}

struct Prompt {
    revealer: gtk::Revealer,
    label: gtk::Label,
    entry: gtk::Entry,
    syncing: Cell<bool>,
}

impl Overlays {
    pub fn new(engine: Arc<Engine>, parent: &impl IsA<gtk::Widget>) -> Rc<Self> {
        let overlays = Rc::new(Self {
            engine,
            parent: parent.clone().upcast(),
            chooser: RefCell::new(None),
            prompt: build_prompt(),
        });
        overlays.connect_prompt();
        overlays
    }

    /// The command prompt bar, for the shell to mount as a bottom bar.
    pub fn prompt_bar(&self) -> &gtk::Widget {
        self.prompt.revealer.upcast_ref()
    }

    pub fn is_open(&self) -> bool {
        self.chooser.borrow().is_some() || self.prompt.revealer.reveals_child()
    }

    /// Bring every overlay in line with the core. Called for each notification;
    /// the diffing keeps a chooser's rows alive while only its cursor moves.
    pub fn sync(self: &Rc<Self>) {
        self.sync_prompt();
        self.sync_chooser();
    }

    /// Tear the overlays down without telling the daemon: used when the session
    /// goes away under the shell, where there is nothing left to inform.
    pub fn dismiss(&self) {
        if let Some(chooser) = self.chooser.borrow_mut().take() {
            chooser.dialog.force_close();
        }
        self.prompt.revealer.set_reveal_child(false);
    }

    fn sync_chooser(self: &Rc<Self>) {
        let desired = self
            .engine
            .choose_tree()
            .map(Chosen::Tree)
            .or_else(|| self.engine.choose_buffer().map(Chosen::Buffer));
        let Some(desired) = desired else {
            if let Some(chooser) = self.chooser.borrow_mut().take() {
                chooser.dialog.force_close();
            }
            return;
        };
        let reuse = self
            .chooser
            .borrow()
            .as_ref()
            .is_some_and(|chooser| chooser.state.same_shape(&desired));
        if reuse {
            let mut current = self.chooser.borrow_mut();
            let chooser = current.as_mut().expect("reuse implies an open chooser");
            chooser.state = desired;
            select(&chooser.list, chooser.state.selected());
            return;
        }
        if let Some(chooser) = self.chooser.borrow_mut().take() {
            chooser.dialog.force_close();
        }
        let chooser = self.build_chooser(desired);
        chooser.dialog.present(Some(&self.parent));
        self.chooser.replace(Some(chooser));
    }

    fn build_chooser(self: &Rc<Self>, state: Chosen) -> Chooser {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("boxed-list");
        for row in state.rows() {
            list.append(&row);
        }
        select(&list, state.selected());

        let target = Rc::downgrade(self);
        list.connect_row_activated(move |_, row| {
            let Some(overlays) = target.upgrade() else {
                return;
            };
            let index = u32::try_from(row.index()).unwrap_or(0);
            let message = overlays
                .chooser
                .borrow()
                .as_ref()
                .map(|chooser| chooser.state.activate(index));
            if let Some(message) = message {
                overlays.engine.send(message);
            }
        });

        let search = gtk::Label::builder().xalign(0.0).build();
        search.add_css_class("monospace");
        let hint = gtk::Label::builder().label(HINT).xalign(0.0).build();
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.add_css_class("zz-chooser");
        content.append(&search);
        content.append(&scroller);
        content.append(&hint);

        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        header.set_title_widget(Some(&adw::WindowTitle::new(state.title(), "")));
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content));

        let dialog = adw::Dialog::builder()
            .content_width(CHOOSER_WIDTH)
            .content_height(CHOOSER_HEIGHT)
            .title(state.title())
            .can_close(false)
            .child(&toolbar)
            .build();
        self.forward_keys(&dialog);

        let chooser = Chooser {
            dialog,
            list,
            search,
            state,
        };
        chooser.refresh_search();
        chooser
    }

    /// Chooser keys belong to the daemon's tables, so the dialog claims every
    /// press — including Escape, which is why it cannot close itself.
    fn forward_keys(self: &Rc<Self>, dialog: &adw::Dialog) {
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(overlays) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if keys::is_modifier(keyval) {
                return glib::Propagation::Proceed;
            }
            let input = keys::key_input(KeyAction::Press, keyval, modifiers, None);
            let message = overlays
                .chooser
                .borrow()
                .as_ref()
                .map(|chooser| chooser.state.key(input));
            let Some(message) = message else {
                return glib::Propagation::Proceed;
            };
            overlays.engine.send(message);
            glib::Propagation::Stop
        });
        dialog.add_controller(keyboard);
    }

    fn sync_prompt(&self) {
        let Some(state) = self.engine.command_prompt() else {
            if self.prompt.revealer.reveals_child() {
                self.prompt.revealer.set_reveal_child(false);
            }
            return;
        };
        self.prompt.syncing.set(true);
        self.prompt.label.set_text(&state.prompt);
        if self.prompt.entry.text() != state.input {
            self.prompt.entry.set_text(&state.input);
        }
        self.prompt
            .entry
            .set_position(i32::try_from(state.cursor).unwrap_or(-1));
        self.prompt.syncing.set(false);
        if self.prompt.revealer.reveals_child() {
            return;
        }
        self.prompt.revealer.set_reveal_child(true);
        self.prompt.entry.grab_focus_without_selecting();
    }

    fn connect_prompt(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        self.prompt.entry.connect_changed(move |entry| {
            let Some(overlays) = target.upgrade() else {
                return;
            };
            if overlays.prompt.syncing.get() {
                return;
            }
            overlays.engine.send(InputMessage::CommandPrompt {
                action: CommandPromptAction::Update {
                    input: entry.text().to_string(),
                    cursor: u32::try_from(entry.position()).unwrap_or(0),
                },
            });
        });

        let target = Rc::downgrade(self);
        self.prompt.entry.connect_activate(move |entry| {
            if let Some(overlays) = target.upgrade() {
                overlays.engine.send(InputMessage::CommandPrompt {
                    action: CommandPromptAction::Submit {
                        input: entry.text().to_string(),
                    },
                });
            }
        });

        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, _| {
            let Some(overlays) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if keyval != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            overlays.engine.send(InputMessage::CommandPrompt {
                action: CommandPromptAction::Close,
            });
            glib::Propagation::Stop
        });
        self.prompt.entry.add_controller(keyboard);
    }
}

impl Chooser {
    fn refresh_search(&self) {
        match self.state.search() {
            Some(query) => {
                self.search.set_text(&query);
                self.search.set_visible(true);
            }
            None => self.search.set_visible(false),
        }
    }
}

fn build_prompt() -> Prompt {
    let label = gtk::Label::new(None);
    label.add_css_class("dim-label");
    label.add_css_class("monospace");
    let entry = gtk::Entry::builder().hexpand(true).build();
    entry.add_css_class("monospace");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("toolbar");
    row.add_css_class("zz-prompt");
    row.append(&label);
    row.append(&entry);
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .child(&row)
        .build();
    Prompt {
        revealer,
        label,
        entry,
        syncing: Cell::new(false),
    }
}

fn select(list: &gtk::ListBox, index: u32) {
    let row = list.row_at_index(i32::try_from(index).unwrap_or(0));
    list.select_row(row.as_ref());
    if let Some(row) = row {
        row.grab_focus();
    }
}

fn tree_row(item: &ChooseTreeItem) -> adw::ActionRow {
    let row = row(&item.label, &item.detail);
    row.set_margin_start(i32::from(item.depth) * 12);
    row.add_prefix(&gtk::Image::from_icon_name(tree_icon(item)));
    if item.active() {
        row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
    }
    row
}

fn tree_icon(item: &ChooseTreeItem) -> &'static str {
    match item.pane_kind {
        Some(ChooseTreePaneKind::Terminal) => "utilities-terminal-symbolic",
        Some(ChooseTreePaneKind::Browser) => "web-browser-symbolic",
        Some(ChooseTreePaneKind::Agent) => "system-run-symbolic",
        Some(ChooseTreePaneKind::Editor) => "text-editor-symbolic",
        None => match item.target {
            ChooseTreeTarget::Session(_) => "view-grid-symbolic",
            ChooseTreeTarget::Window(_) if item.expanded() => "pan-down-symbolic",
            ChooseTreeTarget::Window(_) => "pan-end-symbolic",
            ChooseTreeTarget::Pane(_) => "utilities-terminal-symbolic",
        },
    }
}

/// Chooser labels come from user-named sessions and command output, and
/// libadwaita rows parse their text as Pango markup.
fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(glib::markup_escape_text(title))
        .subtitle(glib::markup_escape_text(subtitle))
        .build()
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kilobytes = bytes as f64 / 1024.0;
    if kilobytes < 1024.0 {
        format!("{kilobytes:.1} kB")
    } else {
        format!("{:.1} MB", kilobytes / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::{ChooseBufferItem, ChooseTreeSearchState};

    fn tree(selected: u32, query: Option<&str>) -> Chosen {
        Chosen::Tree(ChooseTreeState {
            items: vec![ChooseTreeItem {
                label: "0: zsh".to_owned(),
                detail: "1 pane".to_owned(),
                target: ChooseTreeTarget::Window(zz_protocol::WindowId(1)),
                depth: 1,
                flags: ChooseTreeItem::ACTIVE,
                pane_kind: None,
            }],
            search: query.map(|query| ChooseTreeSearchState {
                query: query.to_owned(),
                reverse: false,
            }),
            selected,
            kind: ChooseTreeKind::Windows,
        })
    }

    #[test]
    fn only_a_moved_cursor_keeps_the_rows_alive() {
        assert!(tree(0, None).same_shape(&tree(3, None)));
        assert!(!tree(0, None).same_shape(&tree(0, Some("z"))));
        assert!(
            !tree(0, None).same_shape(&Chosen::Buffer(ChooseBufferState {
                items: Vec::new(),
                search: None,
                selected: 0,
            }))
        );
    }

    #[test]
    fn a_reverse_search_is_spelled_the_way_the_daemon_started_it() {
        assert_eq!(tree(0, Some("cargo")).search().as_deref(), Some("/cargo"));
        assert_eq!(tree(0, None).search(), None);
    }

    #[test]
    fn buffer_previews_carry_a_readable_size() {
        let state = Chosen::Buffer(ChooseBufferState {
            items: vec![ChooseBufferItem {
                name: "buffer0".to_owned(),
                preview: "hello".to_owned(),
                size_bytes: 2048,
                created_unix_seconds: 0,
            }],
            search: None,
            selected: 0,
        });

        assert_eq!(state.title(), "Choose buffer");
        assert_eq!(human_size(2048), "2.0 kB");
        assert_eq!(human_size(12), "12 B");
        assert!(state.same_shape(&state.clone()));
    }
}
