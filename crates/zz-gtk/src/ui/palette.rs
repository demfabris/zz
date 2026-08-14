//! The command palette: a floating top-center surface over the pane area that
//! renders whatever `command-prompt` the daemon has open for this client.
//!
//! The daemon owns when a prompt opens, its label, its template and its
//! execution; the client owns the text field and the completions. Those two
//! halves meet at `CommandPromptAction`.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use adw::prelude::*;
use gtk::{gdk, glib};
use zz_protocol::{
    CommandPromptAction, CommandPromptKind, CommandPromptState, InputMessage,
    MAX_COMMAND_PROMPT_BYTES, MuxSnapshot,
};

use crate::{
    engine::Engine,
    ui::completion::{
        CompletionKind, CompletionSuggestion, PaneKindAvailability, apply_completion,
        complete_command,
    },
};

const PALETTE_WIDTH: i32 = 620;
const MAX_VISIBLE_ROWS: i32 = 8;
const ROW_HEIGHT: i32 = 40;
const COMMAND_HINT: &str = "Tab complete · ↑↓ select · Enter run · Esc close";
const VALUE_HINT: &str = "Enter apply · Esc close";

/// The daemon can still make a browser pane this client renders as a
/// placeholder, so browser commands stay in the catalog; agent and editor panes
/// are experiments zz-gtk does not carry at all.
const AVAILABILITY: PaneKindAvailability = PaneKindAvailability {
    browser: true,
    agent: false,
    editor: false,
};

const STYLE: &str = "
.zz-palette {
    background-color: @popover_bg_color;
    border-radius: 12px;
    padding: 6px;
    margin-top: 22px;
    box-shadow: 0 2px 12px alpha(@window_fg_color, 0.22);
}
.zz-palette-kind {
    font-size: 0.68em;
    letter-spacing: 0.08em;
    opacity: 0.5;
}
.zz-palette-row { padding: 2px 6px; }
.zz-palette-hints { font-size: 0.8em; opacity: 0.55; padding: 2px 6px; }
";

/// What a daemon publication asks the surface to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteSync {
    /// No prompt is open.
    Closed,
    /// A prompt the surface has not shown yet: replace the field's contents.
    Opened,
    /// The prompt already on screen. Local edits stand — `Update` is never
    /// echoed, so the daemon's retained input lags behind what the user typed.
    Retained,
}

/// What pressing Enter means right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteEnter {
    /// A suggestion was highlighted, so Enter takes it instead of running.
    Accept {
        input: String,
        cursor: usize,
    },
    Submit(String),
}

/// The palette without a toolkit: prompt state in, ranked suggestions and text
/// edits out. Cursors cross this boundary as Unicode scalar counts, the way the
/// wire and `GtkEditable` both spell them; inside, they are byte offsets, the
/// way the completion engine spells them.
pub struct PaletteModel {
    kind: CommandPromptKind,
    prompt: String,
    history: Vec<String>,
    snapshot: Arc<MuxSnapshot>,
    availability: PaneKindAvailability,
    input: String,
    cursor: usize,
    suggestions: Vec<CompletionSuggestion>,
    selected: Option<usize>,
    engaged: bool,
    adopted: Option<CommandPromptState>,
}

impl PaletteModel {
    pub fn new() -> Self {
        Self {
            kind: CommandPromptKind::Command,
            prompt: String::new(),
            history: Vec::new(),
            snapshot: Arc::new(MuxSnapshot::default()),
            availability: AVAILABILITY,
            input: String::new(),
            cursor: 0,
            suggestions: Vec::new(),
            selected: None,
            engaged: false,
            adopted: None,
        }
    }

    pub fn sync(
        &mut self,
        state: Option<&CommandPromptState>,
        snapshot: Arc<MuxSnapshot>,
    ) -> PaletteSync {
        let Some(state) = state else {
            self.close();
            return PaletteSync::Closed;
        };
        let moved = self.snapshot.generation != snapshot.generation;
        if moved {
            self.snapshot = snapshot;
        }
        if self.adopted.as_ref() == Some(state) {
            if moved {
                self.recompute();
            }
            return PaletteSync::Retained;
        }
        self.adopted = Some(state.clone());
        self.kind = state.kind;
        self.prompt.clone_from(&state.prompt);
        self.history.clone_from(&state.history);
        self.input.clone_from(&state.input);
        self.cursor = char_to_byte(&self.input, state.cursor as usize);
        self.engaged = false;
        self.selected = None;
        self.recompute();
        PaletteSync::Opened
    }

    pub fn close(&mut self) {
        self.adopted = None;
        self.input.clear();
        self.cursor = 0;
        self.suggestions.clear();
        self.selected = None;
        self.engaged = false;
    }

    /// Record a local edit; true when it actually moved, which is what the
    /// caller reports to the daemon.
    pub fn edit(&mut self, input: String, cursor: usize) -> bool {
        let cursor = char_to_byte(&input, cursor);
        if self.input == input && self.cursor == cursor {
            return false;
        }
        self.input = input;
        self.cursor = cursor;
        self.engaged = false;
        self.recompute();
        true
    }

    /// Move the highlight. The first press engages the list rather than
    /// stepping it, so Down lands on the top suggestion and Up on the last.
    pub fn navigate(&mut self, direction: i32) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }
        let count = self.suggestions.len();
        let selected = if self.engaged {
            let current = self.selected.unwrap_or_default();
            if direction < 0 {
                current.checked_sub(1).unwrap_or(count - 1)
            } else {
                (current + 1) % count
            }
        } else if direction < 0 {
            count - 1
        } else {
            0
        };
        self.engaged = true;
        self.selected = Some(selected);
        true
    }

    pub fn accept(&mut self) -> Option<(String, usize)> {
        let index = self
            .selected
            .or((!self.suggestions.is_empty()).then_some(0))?;
        self.accept_at(index)
    }

    pub fn accept_at(&mut self, index: usize) -> Option<(String, usize)> {
        let suggestion = self.suggestions.get(index)?.clone();
        let (completed, cursor) = apply_completion(&self.input, &suggestion);
        if completed.len() > MAX_COMMAND_PROMPT_BYTES {
            return None;
        }
        self.engaged = false;
        self.input = completed;
        self.cursor = cursor;
        self.recompute();
        Some((self.input.clone(), byte_to_char(&self.input, self.cursor)))
    }

    pub fn enter(&mut self) -> PaletteEnter {
        if self.engaged
            && let Some((input, cursor)) = self.accept()
        {
            return PaletteEnter::Accept { input, cursor };
        }
        PaletteEnter::Submit(self.input.clone())
    }

    pub const fn kind(&self) -> CommandPromptKind {
        self.kind
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn cursor(&self) -> usize {
        byte_to_char(&self.input, self.cursor)
    }

    pub fn suggestions(&self) -> &[CompletionSuggestion] {
        &self.suggestions
    }

    /// The row to paint as selected: none until the user engages the list, so
    /// Enter keeps meaning "run" while they are still typing.
    pub const fn highlighted(&self) -> Option<usize> {
        if self.engaged { self.selected } else { None }
    }

    pub const fn is_open(&self) -> bool {
        self.adopted.is_some()
    }

    /// Value prompts substitute into a daemon-private template, so nothing the
    /// catalog knows can be suggested for them.
    fn recompute(&mut self) {
        self.suggestions = if self.kind == CommandPromptKind::Command {
            complete_command(
                &self.input,
                self.cursor,
                &self.history,
                &self.snapshot,
                self.availability,
            )
        } else {
            Vec::new()
        };
        self.selected = (!self.suggestions.is_empty()).then_some(
            self.selected
                .unwrap_or_default()
                .min(self.suggestions.len().saturating_sub(1)),
        );
    }
}

impl Default for PaletteModel {
    fn default() -> Self {
        Self::new()
    }
}

/// The palette surface. Mount [`CommandPalette::widget`] as a `GtkOverlay`
/// child so it floats over the pane area instead of resizing it.
pub struct CommandPalette {
    engine: Arc<Engine>,
    model: RefCell<PaletteModel>,
    root: gtk::Revealer,
    label: gtk::Label,
    entry: gtk::Entry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    hints: gtk::Label,
    rendered: RefCell<Vec<CompletionSuggestion>>,
    syncing: Cell<bool>,
    finishing: Cell<bool>,
}

impl CommandPalette {
    pub fn new(engine: Arc<Engine>) -> Rc<Self> {
        install_style();

        let label = gtk::Label::new(None);
        label.add_css_class("dim-label");
        label.add_css_class("monospace");
        let entry = gtk::Entry::builder()
            .hexpand(true)
            .has_frame(false)
            .placeholder_text("Type a tmux command…")
            .build();
        entry.add_css_class("monospace");
        let field = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        field.append(&label);
        field.append(&entry);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(MAX_VISIBLE_ROWS * ROW_HEIGHT)
            .visible(false)
            .child(&list)
            .build();

        let hints = gtk::Label::builder()
            .label(COMMAND_HINT)
            .xalign(0.0)
            .build();
        hints.add_css_class("zz-palette-hints");

        let surface = gtk::Box::new(gtk::Orientation::Vertical, 4);
        surface.add_css_class("zz-palette");
        surface.set_width_request(PALETTE_WIDTH);
        surface.append(&field);
        surface.append(&scroller);
        surface.append(&hints);

        let root = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .child(&surface)
            .build();

        let palette = Rc::new(Self {
            engine,
            model: RefCell::new(PaletteModel::new()),
            root,
            label,
            entry,
            list,
            scroller,
            hints,
            rendered: RefCell::new(Vec::new()),
            syncing: Cell::new(false),
            finishing: Cell::new(false),
        });
        palette.connect_signals();
        palette
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub fn is_open(&self) -> bool {
        self.model.borrow().is_open()
    }

    /// Bring the surface in line with the daemon. Called for every overlay
    /// notification, so it must leave a prompt the user is editing alone.
    pub fn sync(self: &Rc<Self>) {
        let state = self.engine.command_prompt();
        let snapshot = self.engine.snapshot();
        let outcome = self.model.borrow_mut().sync(state.as_ref(), snapshot);
        match outcome {
            PaletteSync::Closed => self.dismiss(),
            PaletteSync::Opened => self.adopt(),
            PaletteSync::Retained => self.refresh_rows(),
        }
    }

    /// Drop the surface without telling the daemon, for a session that went
    /// away underneath it.
    pub fn dismiss(&self) {
        if self.root.reveals_child() {
            self.root.set_reveal_child(false);
        }
        self.finishing.set(false);
        self.model.borrow_mut().close();
        self.clear_rows();
    }

    fn adopt(self: &Rc<Self>) {
        let (prompt, input, cursor, kind) = {
            let model = self.model.borrow();
            (
                model.prompt().to_owned(),
                model.input().to_owned(),
                model.cursor(),
                model.kind(),
            )
        };
        self.label.set_text(&prompt);
        self.hints.set_text(match kind {
            CommandPromptKind::Command => COMMAND_HINT,
            CommandPromptKind::Value => VALUE_HINT,
        });
        self.push_text(&input, cursor);
        self.refresh_rows();
        self.finishing.set(false);
        if !self.root.reveals_child() {
            self.root.set_reveal_child(true);
        }
        self.entry.grab_focus_without_selecting();
    }

    /// Write the field without the change handler mistaking it for a user edit.
    fn push_text(&self, input: &str, cursor: usize) {
        self.syncing.set(true);
        if self.entry.text() != input {
            self.entry.set_text(input);
        }
        self.entry.set_position(i32::try_from(cursor).unwrap_or(-1));
        self.syncing.set(false);
    }

    fn refresh_rows(self: &Rc<Self>) {
        let (suggestions, highlighted) = {
            let model = self.model.borrow();
            (model.suggestions().to_vec(), model.highlighted())
        };
        if *self.rendered.borrow() != suggestions {
            self.clear_rows();
            for suggestion in &suggestions {
                self.list.append(&row(suggestion));
            }
            self.rendered.replace(suggestions.clone());
        }
        self.scroller.set_visible(!suggestions.is_empty());
        let row = highlighted
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| self.list.row_at_index(index));
        self.list.select_row(row.as_ref());
        if let Some(row) = row {
            reveal(&self.scroller, &row);
        }
    }

    /// `GtkListBox` keeps children a `remove` refuses to take — an empty list
    /// still answers `first_child`, and draining it by hand spins forever.
    fn clear_rows(&self) {
        self.list.remove_all();
        self.rendered.borrow_mut().clear();
    }

    fn on_edit(self: &Rc<Self>) {
        if self.syncing.get() {
            return;
        }
        let cursor = usize::try_from(self.entry.position()).unwrap_or_default();
        let text = self.entry.text().to_string();
        if self.model.borrow_mut().edit(text.clone(), cursor) {
            self.send(CommandPromptAction::Update {
                input: text,
                cursor: u32::try_from(cursor).unwrap_or(u32::MAX),
            });
            self.refresh_rows();
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        let target = Rc::downgrade(self);
        self.entry.connect_changed(move |_| {
            if let Some(palette) = target.upgrade() {
                palette.on_edit();
            }
        });

        // Suggestions are cursor-sensitive — the token under the caret is what
        // gets completed — so moving the caret is an edit as far as this
        // surface and the daemon's stored prompt are concerned.
        let target = Rc::downgrade(self);
        self.entry
            .connect_notify_local(Some("cursor-position"), move |_, _| {
                if let Some(palette) = target.upgrade() {
                    palette.on_edit();
                }
            });

        let target = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(palette) = target.upgrade() {
                palette.enter();
            }
        });

        let target = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(palette) = target.upgrade() else {
                return;
            };
            let index = usize::try_from(row.index()).unwrap_or(0);
            let accepted = palette.model.borrow_mut().accept_at(index);
            if let Some((input, cursor)) = accepted {
                palette.commit(&input, cursor);
            }
            palette.entry.grab_focus_without_selecting();
        });

        // Capture beats the entry's own bindings: Tab would otherwise move
        // focus and Up/Down would sit unused inside a single-line field.
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = Rc::downgrade(self);
        keyboard.connect_key_pressed(move |_, keyval, _, modifiers| {
            let Some(palette) = target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            palette.on_key(keyval, modifiers)
        });
        self.entry.add_controller(keyboard);
    }

    fn on_key(
        self: &Rc<Self>,
        keyval: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> glib::Propagation {
        if modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
            return glib::Propagation::Proceed;
        }
        match keyval {
            gdk::Key::Escape => self.close(),
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab | gdk::Key::KP_Tab => {
                let accepted = self.model.borrow_mut().accept();
                if let Some((input, cursor)) = accepted {
                    self.commit(&input, cursor);
                }
            }
            gdk::Key::Up | gdk::Key::KP_Up => {
                if self.model.borrow_mut().navigate(-1) {
                    self.refresh_rows();
                }
            }
            gdk::Key::Down | gdk::Key::KP_Down => {
                if self.model.borrow_mut().navigate(1) {
                    self.refresh_rows();
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// An accepted completion is a local edit like any other: the field takes
    /// it, and the daemon is told so a later resync agrees with the screen.
    fn commit(self: &Rc<Self>, input: &str, cursor: usize) {
        self.push_text(input, cursor);
        self.send(CommandPromptAction::Update {
            input: input.to_owned(),
            cursor: u32::try_from(cursor).unwrap_or(u32::MAX),
        });
        self.refresh_rows();
    }

    fn enter(self: &Rc<Self>) {
        if self.finishing.get() {
            return;
        }
        let outcome = self.model.borrow_mut().enter();
        match outcome {
            PaletteEnter::Accept { input, cursor } => self.commit(&input, cursor),
            PaletteEnter::Submit(input) => {
                self.finishing.set(true);
                self.send(CommandPromptAction::Submit { input });
            }
        }
    }

    fn close(&self) {
        if self.finishing.replace(true) {
            return;
        }
        self.send(CommandPromptAction::Close);
    }

    fn send(&self, action: CommandPromptAction) {
        self.engine.send(InputMessage::CommandPrompt { action });
    }
}

/// Suggestion details carry user-named sessions and window titles; a plain
/// `GtkLabel` renders them literally, unlike the `AdwActionRow` the choosers use.
fn row(suggestion: &CompletionSuggestion) -> gtk::ListBoxRow {
    let kind = gtk::Label::builder()
        .label(kind_label(suggestion.kind))
        .width_chars(7)
        .xalign(0.0)
        .build();
    kind.add_css_class("zz-palette-kind");
    let label = gtk::Label::builder()
        .label(&suggestion.label)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    label.add_css_class("monospace");
    let detail = gtk::Label::builder()
        .label(&suggestion.detail)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    detail.add_css_class("dim-label");
    detail.add_css_class("caption");

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.add_css_class("zz-palette-row");
    content.append(&kind);
    content.append(&label);
    content.append(&detail);
    gtk::ListBoxRow::builder().child(&content).build()
}

const fn kind_label(kind: CompletionKind) -> &'static str {
    match kind {
        CompletionKind::History => "HISTORY",
        CompletionKind::Command => "COMMAND",
        CompletionKind::Option => "OPTION",
        CompletionKind::Value => "VALUE",
    }
}

/// Scroll a selected row into view without giving it the keyboard: focus has to
/// stay in the entry for the next keystroke to keep editing.
fn reveal(scroller: &gtk::ScrolledWindow, row: &gtk::ListBoxRow) {
    let Some(list) = row.parent() else {
        return;
    };
    let Some(bounds) = row.compute_bounds(&list) else {
        return;
    };
    let adjustment = scroller.vadjustment();
    let top = f64::from(bounds.y());
    let bottom = top + f64::from(bounds.height());
    if top < adjustment.value() {
        adjustment.set_value(top);
    } else if bottom > adjustment.value() + adjustment.page_size() {
        adjustment.set_value(bottom - adjustment.page_size());
    }
}

fn char_to_byte(value: &str, cursor: usize) -> usize {
    value
        .char_indices()
        .nth(cursor)
        .map_or(value.len(), |(index, _)| index)
}

fn byte_to_char(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    value[..index].chars().count()
}

fn install_style() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(input: &str, cursor: u32, history: &[&str]) -> CommandPromptState {
        CommandPromptState {
            prompt: ":".to_owned(),
            input: input.to_owned(),
            cursor,
            kind: CommandPromptKind::Command,
            history: history.iter().map(|entry| (*entry).to_owned()).collect(),
        }
    }

    fn labels(model: &PaletteModel) -> Vec<&str> {
        model
            .suggestions()
            .iter()
            .map(|suggestion| suggestion.label.as_str())
            .collect()
    }

    #[test]
    fn a_fresh_prompt_is_adopted_and_a_republished_one_is_not() {
        let mut model = PaletteModel::new();
        let state = command("", 0, &[]);

        assert_eq!(
            model.sync(Some(&state), Arc::new(MuxSnapshot::default())),
            PaletteSync::Opened
        );
        assert!(model.is_open());
        assert!(model.edit("ren".to_owned(), 3));
        assert_eq!(
            model.sync(Some(&state), Arc::new(MuxSnapshot::default())),
            PaletteSync::Retained,
            "the daemon never echoes an Update, so its input lags behind"
        );
        assert_eq!(model.input(), "ren");
        assert_eq!(
            model.sync(None, Arc::new(MuxSnapshot::default())),
            PaletteSync::Closed
        );
        assert!(!model.is_open());
        assert_eq!(model.input(), "");
    }

    #[test]
    fn value_prompts_are_never_completed() {
        let mut model = PaletteModel::new();
        let state = CommandPromptState {
            prompt: "rename-window: ".to_owned(),
            input: "notes".to_owned(),
            cursor: 5,
            kind: CommandPromptKind::Value,
            history: vec!["list-panes".to_owned()],
        };

        model.sync(Some(&state), Arc::new(MuxSnapshot::default()));

        assert_eq!(model.input(), "notes");
        assert_eq!(model.cursor(), 5);
        assert!(model.suggestions().is_empty());
        assert_eq!(model.enter(), PaletteEnter::Submit("notes".to_owned()));
    }

    #[test]
    fn navigation_engages_before_it_steps_and_wraps_both_ways() {
        let mut model = PaletteModel::new();
        model.sync(
            Some(&command("new-", 4, &[])),
            Arc::new(MuxSnapshot::default()),
        );
        let count = model.suggestions().len();
        assert!(count > 2, "the catalog has several new-* commands");

        assert_eq!(model.highlighted(), None);
        assert!(model.navigate(1));
        assert_eq!(model.highlighted(), Some(0));
        assert!(model.navigate(1));
        assert_eq!(model.highlighted(), Some(1));
        assert!(model.navigate(-1));
        assert_eq!(model.highlighted(), Some(0));
        assert!(model.navigate(-1));
        assert_eq!(model.highlighted(), Some(count - 1));

        assert!(
            !model.edit("new-".to_owned(), 4),
            "a redundant change signal is not an edit"
        );
        assert!(model.edit("new-w".to_owned(), 5));
        assert_eq!(model.highlighted(), None, "typing disengages the list");
        assert!(model.navigate(-1));
        assert_eq!(
            model.highlighted(),
            Some(model.suggestions().len() - 1),
            "Up opens at the end"
        );
    }

    #[test]
    fn enter_runs_while_typing_and_accepts_once_the_list_is_engaged() {
        let mut model = PaletteModel::new();
        model.sync(
            Some(&command("new-w", 5, &[])),
            Arc::new(MuxSnapshot::default()),
        );

        assert_eq!(model.enter(), PaletteEnter::Submit("new-w".to_owned()));
        model.navigate(1);
        assert_eq!(
            model.enter(),
            PaletteEnter::Accept {
                input: "new-window ".to_owned(),
                cursor: 11
            }
        );
        assert_eq!(model.highlighted(), None, "accepting disengages the list");
    }

    #[test]
    fn tab_accepts_the_top_suggestion_without_engaging_the_list() {
        let mut model = PaletteModel::new();
        model.sync(
            Some(&command("new-w", 5, &[])),
            Arc::new(MuxSnapshot::default()),
        );

        assert_eq!(
            model.accept(),
            Some(("new-window ".to_owned(), 11)),
            "Tab takes the top row when nothing is highlighted"
        );
        assert_eq!(model.input(), "new-window ");
    }

    #[test]
    fn history_leads_the_ranking_and_survives_a_snapshot_bump() {
        let mut model = PaletteModel::new();
        model.sync(
            Some(&command("", 0, &["list-panes", "split-window -h"])),
            Arc::new(MuxSnapshot::default()),
        );

        assert_eq!(
            labels(&model).first(),
            Some(&"split-window -h"),
            "the most recent command leads"
        );

        let moved = MuxSnapshot {
            generation: 9,
            ..MuxSnapshot::default()
        };
        assert_eq!(
            model.sync(
                Some(&command("", 0, &["list-panes", "split-window -h"])),
                Arc::new(moved)
            ),
            PaletteSync::Retained
        );
        assert_eq!(labels(&model).first(), Some(&"split-window -h"));
    }

    #[test]
    fn cursors_cross_the_boundary_as_scalars() {
        assert_eq!(char_to_byte("aα界", 0), 0);
        assert_eq!(char_to_byte("aα界", 2), 3);
        assert_eq!(char_to_byte("aα界", 3), 6);
        assert_eq!(char_to_byte("aα界", 9), 6);
        assert_eq!(byte_to_char("aα界", 0), 0);
        assert_eq!(byte_to_char("aα界", 3), 2);
        assert_eq!(byte_to_char("aα界", 6), 3);
        assert_eq!(
            byte_to_char("aα界", 4),
            2,
            "an index inside a scalar floors onto its start"
        );
        assert_eq!(byte_to_char("aα界", 99), 3);
    }

    #[test]
    fn a_unicode_prompt_keeps_its_cursor_through_a_round_trip() {
        let mut model = PaletteModel::new();
        model.sync(
            Some(&command("rename-window café", 18, &[])),
            Arc::new(MuxSnapshot::default()),
        );

        assert_eq!(model.cursor(), 18);
        assert_eq!(model.input().chars().count(), 18);
    }
}
