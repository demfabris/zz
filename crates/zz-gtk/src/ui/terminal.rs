use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use gtk::{gdk, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
use zz_client::{ChromeAction, ChromeKeymap, ViewportDamage};
use zz_protocol::{InputMessage, PaneId};
use zz_terminal::{
    CellWidth, ClipboardTarget, Cursor, CursorStyle, Glyph, KeyAction, KeyInput, OverlayKind,
    PackedCell, PackedStyle, PointerCellEvent, TerminalAppearance, TerminalMouseButton,
    TerminalMouseInput, TerminalMousePhase, TerminalViewAction, TerminalViewport, UnderlineStyle,
};

use crate::{
    engine::Engine,
    ui::{colors, keys},
};

/// Cell geometry in logical pixels, taken straight from the font's own advance
/// so a run of monospace glyphs lands on the column grid without rounding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

impl Default for CellMetrics {
    fn default() -> Self {
        Self {
            width: 8.0,
            height: 16.0,
        }
    }
}

/// What the input method did with a press.
enum ImOutcome {
    /// A composition is in flight; the eventual commit carries the text.
    Composing,
    /// The press produced text, whether or not the IM claimed the event.
    Text(Option<String>),
}

mod imp {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    use gtk::{gdk, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
    use zz_client::ChromeAction;
    use zz_protocol::PaneId;
    use zz_terminal::{PointerCellEvent, TerminalAppearance, TerminalViewport};

    use super::{CellMetrics, DEFAULT_COLUMNS, DEFAULT_ROWS};
    use crate::engine::Engine;

    #[derive(Default)]
    pub struct TerminalView {
        pub engine: RefCell<Option<Arc<Engine>>>,
        pub chrome: RefCell<Option<Rc<dyn Fn(ChromeAction)>>>,
        pub pane: Cell<PaneId>,
        pub viewport: RefCell<Option<TerminalViewport>>,
        pub appearance: RefCell<TerminalAppearance>,
        pub font: RefCell<pango::FontDescription>,
        pub metrics: Cell<CellMetrics>,
        pub rows: RefCell<Vec<Option<gsk::RenderNode>>>,
        pub im: RefCell<Option<gtk::IMMulticontext>>,
        pub pending_commit: RefCell<Option<String>>,
        pub in_key_press: Cell<bool>,
        pub composing: Cell<bool>,
        pub dragging: Cell<bool>,
        pub anchor: Cell<Option<PointerCellEvent>>,
        pub extent: Cell<bool>,
        pub pointer: Cell<(f64, f64)>,
        pub scroll_remainder: Cell<f32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TerminalView {
        const NAME: &'static str = "ZzTerminalView";
        type Type = super::TerminalView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for TerminalView {
        fn constructed(&self) {
            self.parent_constructed();
            let widget = self.obj();
            widget.set_focusable(true);
            widget.set_can_focus(true);
            widget.set_hexpand(true);
            widget.set_vexpand(true);
            widget.set_overflow(gtk::Overflow::Hidden);
            widget.install_controllers();
            widget.refresh_font();
        }

        fn dispose(&self) {
            if let Some(im) = self.im.borrow().as_ref() {
                im.set_client_widget(gtk::Widget::NONE);
            }
        }
    }

    impl WidgetImpl for TerminalView {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let metrics = self.metrics.get();
            let natural = match orientation {
                gtk::Orientation::Horizontal => metrics.width * DEFAULT_COLUMNS,
                _ => metrics.height * DEFAULT_ROWS,
            };
            (0, natural.ceil() as i32, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().publish_geometry(width, height);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let bounds =
                graphene::Rect::new(0.0, 0.0, widget.width() as f32, widget.height() as f32);
            let viewport = self.viewport.borrow();
            match viewport.as_ref() {
                Some(viewport) => widget.paint(snapshot, viewport, bounds),
                None => snapshot.append_color(&gdk::RGBA::BLACK, &bounds),
            }
        }
    }
}

const DEFAULT_COLUMNS: f32 = 80.0;
const DEFAULT_ROWS: f32 = 24.0;
const CURSOR_THICKNESS: f32 = 2.0;
const FAINT_ALPHA: f32 = 0.6;
/// Rows one wheel notch moves, matching the raw-terminal client.
const WHEEL_LINES: f32 = 3.0;
const MIDDLE_BUTTON: u32 = 2;

/// Copy requests are correlated by the daemon alone; the client only has to
/// keep two outstanding requests from sharing an id.
static COPY_REQUEST: AtomicU64 = AtomicU64::new(1);

glib::wrapper! {
    pub struct TerminalView(ObjectSubclass<imp::TerminalView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TerminalView {
    /// `chrome` receives the chrome actions a terminal surface cannot answer on
    /// its own — detaching, font size, anything window-shaped.
    pub fn new(
        engine: Arc<Engine>,
        pane: PaneId,
        appearance: TerminalAppearance,
        chrome: Rc<dyn Fn(ChromeAction)>,
    ) -> Self {
        let view: Self = glib::Object::new();
        view.imp().pane.set(pane);
        view.imp().engine.replace(Some(engine));
        view.imp().chrome.replace(Some(chrome));
        view.set_appearance(appearance);
        view
    }

    pub fn pane(&self) -> PaneId {
        self.imp().pane.get()
    }

    pub fn set_appearance(&self, appearance: TerminalAppearance) {
        if *self.imp().appearance.borrow() == appearance {
            return;
        }
        self.imp().appearance.replace(appearance);
        self.refresh_font();
        self.invalidate_all();
        self.queue_resize();
        self.queue_draw();
    }

    /// Adopt a frame, dropping only the cached rows it touched. Everything that
    /// changes the whole grid — geometry, default colors, a full frame — clears
    /// the cache instead.
    pub fn apply_frame(&self, viewport: TerminalViewport, damage: &ViewportDamage) {
        let imp = self.imp();
        let reshaped = imp.viewport.borrow().as_ref().is_none_or(|current| {
            current.columns != viewport.columns
                || current.rows != viewport.rows
                || current.foreground != viewport.foreground
                || current.background != viewport.background
        });
        let rows = viewport.rows;
        imp.viewport.replace(Some(viewport));
        match damage {
            _ if reshaped => self.invalidate_all(),
            ViewportDamage::All => self.invalidate_all(),
            ViewportDamage::Rows(indices) => {
                let mut cache = imp.rows.borrow_mut();
                cache.resize(usize::from(rows), None);
                for index in indices {
                    if let Some(slot) = cache.get_mut(usize::from(*index)) {
                        *slot = None;
                    }
                }
            }
        }
        self.queue_draw();
    }

    fn invalidate_all(&self) {
        let rows = self
            .imp()
            .viewport
            .borrow()
            .as_ref()
            .map_or(0, |viewport| usize::from(viewport.rows));
        let mut cache = self.imp().rows.borrow_mut();
        cache.clear();
        cache.resize(rows, None);
    }

    fn engine(&self) -> Option<Arc<Engine>> {
        self.imp().engine.borrow().clone()
    }

    /// Cell height is the font's ascent plus descent, not pango's line height:
    /// the latter adds the family's line gap, which a terminal cell must not
    /// carry or the grid drifts apart by a few pixels a row.
    fn refresh_font(&self) {
        let imp = self.imp();
        let appearance = imp.appearance.borrow();
        let mut font = pango::FontDescription::new();
        font.set_family(
            appearance
                .font_families
                .first()
                .map_or("Monospace", String::as_str),
        );
        font.set_weight(if appearance.font_weight >= 600 {
            pango::Weight::Bold
        } else {
            pango::Weight::Normal
        });
        font.set_size((appearance.font_size_points * pango::SCALE as f32).round() as i32);
        let metrics = self.pango_context().metrics(Some(&font), None);
        let width = metrics.approximate_char_width() as f32 / pango::SCALE as f32;
        let height = (metrics.ascent() + metrics.descent()) as f32 / pango::SCALE as f32;
        imp.metrics.set(CellMetrics {
            width: if width > 1.0 { width } else { 8.0 },
            height: if height > 1.0 { height } else { 16.0 },
        });
        drop(appearance);
        imp.font.replace(font);
    }

    fn publish_geometry(&self, width: i32, height: i32) {
        let Some(engine) = self.engine() else {
            return;
        };
        let metrics = self.imp().metrics.get();
        let columns = (width as f32 / metrics.width).floor().max(1.0) as u16;
        let rows = (height as f32 / metrics.height).floor().max(1.0) as u16;
        let scale = self.scale_factor().max(1) as f32;
        engine.resize_terminal(
            self.pane(),
            columns,
            rows,
            (metrics.width * scale).round() as u32,
            (metrics.height * scale).round() as u32,
        );
    }
}

impl TerminalView {
    fn install_controllers(&self) {
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(self));
        let commit_target = self.downgrade();
        im.connect_commit(move |_, text| {
            let Some(view) = commit_target.upgrade() else {
                return;
            };
            if view.imp().in_key_press.get() {
                view.imp().pending_commit.replace(Some(text.to_owned()));
            } else {
                view.send_text(text);
            }
        });
        let preedit_target = self.downgrade();
        im.connect_preedit_start(move |_| {
            if let Some(view) = preedit_target.upgrade() {
                view.imp().composing.set(true);
            }
        });
        let preedit_target = self.downgrade();
        im.connect_preedit_end(move |_| {
            if let Some(view) = preedit_target.upgrade() {
                view.imp().composing.set(false);
            }
        });
        self.imp().im.replace(Some(im));

        let keyboard = gtk::EventControllerKey::new();
        let pressed_target = self.downgrade();
        keyboard.connect_key_pressed(move |controller, keyval, _keycode, state| {
            let Some(view) = pressed_target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            view.on_key(controller, KeyAction::Press, keyval, state)
        });
        let released_target = self.downgrade();
        keyboard.connect_key_released(move |_, keyval, _keycode, state| {
            if let Some(view) = released_target.upgrade() {
                view.on_key_released(keyval, state);
            }
        });
        self.add_controller(keyboard);

        let focus = gtk::EventControllerFocus::new();
        let enter_target = self.downgrade();
        focus.connect_enter(move |_| {
            if let Some(view) = enter_target.upgrade() {
                view.on_focus(true);
            }
        });
        let leave_target = self.downgrade();
        focus.connect_leave(move |_| {
            if let Some(view) = leave_target.upgrade() {
                view.on_focus(false);
            }
        });
        self.add_controller(focus);

        self.install_pointer();
    }

    /// Pointer input is daemon business: selection, mouse reporting and
    /// scrollback all live behind [`TerminalViewAction`], so the client only
    /// converts pixels into the cell grid and forwards.
    fn install_pointer(&self) {
        let click = gtk::GestureClick::builder().button(0).build();
        let pressed_target = self.downgrade();
        click.connect_pressed(move |gesture, presses, x, y| {
            if let Some(view) = pressed_target.upgrade() {
                view.on_press(gesture, presses, x, y);
            }
        });
        let released_target = self.downgrade();
        click.connect_released(move |gesture, presses, x, y| {
            if let Some(view) = released_target.upgrade() {
                view.on_release(gesture, presses, x, y);
            }
        });
        self.add_controller(click);

        let motion = gtk::EventControllerMotion::new();
        let motion_target = self.downgrade();
        motion.connect_motion(move |controller, x, y| {
            if let Some(view) = motion_target.upgrade() {
                view.on_motion(controller, x, y);
            }
        });
        self.add_controller(motion);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let scroll_target = self.downgrade();
        scroll.connect_scroll(move |controller, _, delta| {
            let Some(view) = scroll_target.upgrade() else {
                return glib::Propagation::Proceed;
            };
            view.on_scroll(controller, delta)
        });
        self.add_controller(scroll);
    }

    fn on_press(&self, gesture: &gtk::GestureClick, presses: i32, x: f64, y: f64) {
        self.grab_focus();
        let Some(engine) = self.engine() else {
            return;
        };
        engine.select_pane(self.pane());
        self.imp().pointer.set((x, y));
        let modifiers = gesture.current_event_state();
        let button = gesture.current_button();
        let force = self.force_selection(modifiers);
        if button == MIDDLE_BUTTON && force {
            self.paste_from(&self.primary_clipboard());
            return;
        }
        let cell = self.cell_at(x, y, presses, modifiers);
        self.imp()
            .dragging
            .set(button == gdk::BUTTON_PRIMARY && force);
        self.imp().anchor.set(Some(cell));
        self.imp().extent.set(false);
        let input = self.mouse_input(
            TerminalMousePhase::Press,
            pointer_button(button),
            cell,
            x,
            y,
            modifiers,
            force,
        );
        self.view_action(TerminalViewAction::Mouse(input));
    }

    fn on_motion(&self, controller: &gtk::EventControllerMotion, x: f64, y: f64) {
        self.imp().pointer.set((x, y));
        let dragging = self.imp().dragging.get();
        if !dragging && !self.mouse_tracking() {
            return;
        }
        let modifiers = controller.current_event_state();
        let cell = self.cell_at(x, y, 1, modifiers);
        if dragging && self.imp().anchor.get().is_some_and(|anchor| anchor != cell) {
            self.imp().extent.set(true);
        }
        let input = self.mouse_input(
            TerminalMousePhase::Motion,
            dragging.then_some(TerminalMouseButton::Left),
            cell,
            x,
            y,
            modifiers,
            self.force_selection(modifiers),
        );
        self.view_action(TerminalViewAction::Mouse(input));
    }

    /// A drag that covered more than one cell copies to the primary selection
    /// on release, the way every X11 and Wayland terminal does.
    fn on_release(&self, gesture: &gtk::GestureClick, presses: i32, x: f64, y: f64) {
        let modifiers = gesture.current_event_state();
        let button = gesture.current_button();
        let cell = self.cell_at(x, y, presses, modifiers);
        let copied =
            self.imp().dragging.get() && self.imp().extent.get() && button == gdk::BUTTON_PRIMARY;
        let input = self.mouse_input(
            TerminalMousePhase::Release,
            pointer_button(button),
            cell,
            x,
            y,
            modifiers,
            self.force_selection(modifiers),
        );
        self.view_action(TerminalViewAction::Mouse(input));
        self.imp().dragging.set(false);
        if copied {
            self.copy_selection(ClipboardTarget::Primary);
        }
    }

    /// Fractional touchpad deltas are accumulated so a slow swipe still moves
    /// whole rows instead of rounding away to nothing.
    fn on_scroll(&self, controller: &gtk::EventControllerScroll, delta: f64) -> glib::Propagation {
        let pending = self.imp().scroll_remainder.get() + delta as f32 * WHEEL_LINES;
        let lines = pending.trunc();
        self.imp().scroll_remainder.set(pending - lines);
        if lines == 0.0 {
            return glib::Propagation::Stop;
        }
        let modifiers = controller.current_event_state();
        let (x, y) = self.imp().pointer.get();
        let button = if lines < 0.0 {
            TerminalMouseButton::ScrollUp
        } else {
            TerminalMouseButton::ScrollDown
        };
        let input = self.mouse_input(
            TerminalMousePhase::Press,
            Some(button),
            self.cell_at(x, y, 1, modifiers),
            x,
            y,
            modifiers,
            self.force_selection(modifiers),
        );
        self.view_action(TerminalViewAction::ScrollWheel {
            lines: lines as i32,
            input,
        });
        glib::Propagation::Stop
    }

    /// True when the press means "select text" rather than "tell the program":
    /// Shift always forces selection, and so does a pane nobody is tracking.
    fn force_selection(&self, modifiers: gdk::ModifierType) -> bool {
        modifiers.contains(gdk::ModifierType::SHIFT_MASK) || !self.mouse_tracking()
    }

    fn mouse_tracking(&self) -> bool {
        self.imp()
            .viewport
            .borrow()
            .as_ref()
            .is_some_and(|viewport| viewport.mouse_tracking)
    }

    fn cell_at(
        &self,
        x: f64,
        y: f64,
        presses: i32,
        modifiers: gdk::ModifierType,
    ) -> PointerCellEvent {
        let metrics = self.imp().metrics.get();
        let (columns, rows) = self
            .imp()
            .viewport
            .borrow()
            .as_ref()
            .map_or((1, 1), |v| (v.columns.max(1), v.rows.max(1)));
        let column = (x as f32 / metrics.width).floor().max(0.0) as u16;
        let row = (y as f32 / metrics.height).floor().max(0.0) as u16;
        PointerCellEvent {
            column: column.min(columns - 1),
            row: row.min(rows - 1),
            click_count: u8::try_from(presses).unwrap_or(u8::MAX),
            rectangle: modifiers.contains(gdk::ModifierType::ALT_MASK),
        }
    }

    /// Pixel fields are scaled the same way the published cell geometry is, so
    /// the daemon's pixel mouse reporting lands on the grid it was told about.
    fn mouse_input(
        &self,
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        cell: PointerCellEvent,
        x: f64,
        y: f64,
        modifiers: gdk::ModifierType,
        force_selection: bool,
    ) -> TerminalMouseInput {
        let metrics = self.imp().metrics.get();
        let scale = self.scale_factor().max(1) as f32;
        TerminalMouseInput::new(
            phase,
            button,
            cell,
            (x as f32 * scale).max(0.0) as u32,
            (y as f32 * scale).max(0.0) as u32,
            (self.width() as f32 * scale).max(0.0) as u32,
            (self.height() as f32 * scale).max(0.0) as u32,
            (metrics.width * scale).round() as u32,
            (metrics.height * scale).round() as u32,
            keys::modifiers(modifiers),
            force_selection,
        )
    }

    fn on_focus(&self, focused: bool) {
        if let Some(im) = self.imp().im.borrow().as_ref() {
            if focused {
                im.focus_in();
            } else {
                im.focus_out();
                im.reset();
            }
        }
        self.view_action(TerminalViewAction::Focus(focused));
        self.queue_draw();
    }

    /// Chrome first, input method second, the pane last. Chrome chords are
    /// resolved client-side and never reach the wire; the wire grammar cannot
    /// even spell them.
    fn on_key(
        &self,
        controller: &gtk::EventControllerKey,
        action: KeyAction,
        keyval: gdk::Key,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        if keys::is_modifier(keyval) {
            return glib::Propagation::Proceed;
        }
        let Some(engine) = self.engine() else {
            return glib::Propagation::Proceed;
        };
        let probe = keys::key_input(action, keyval, state, None);
        if let Some(chrome) = resolve_chrome(engine.chrome(), &probe) {
            self.perform(&engine, chrome);
            return glib::Propagation::Stop;
        }
        let text = match self.filter_input_method(controller) {
            ImOutcome::Composing => return glib::Propagation::Stop,
            ImOutcome::Text(text) => text,
        };
        let input = keys::key_input(action, keyval, state, text.as_deref());
        engine.send_key(self.pane(), input, false);
        glib::Propagation::Stop
    }

    fn on_key_released(&self, keyval: gdk::Key, state: gdk::ModifierType) {
        if keys::is_modifier(keyval) {
            return;
        }
        let Some(engine) = self.engine() else {
            return;
        };
        let kitty = self
            .imp()
            .viewport
            .borrow()
            .as_ref()
            .is_some_and(|viewport| viewport.kitty_keyboard);
        if !kitty {
            return;
        }
        let input = keys::key_input(KeyAction::Release, keyval, state, None);
        engine.send_key(self.pane(), input, false);
    }

    /// Runs the press through the input method by hand rather than handing the
    /// context to the controller: `GtkEventControllerKey` would let the IM
    /// swallow the event before `key-pressed` fires, and then plain letters
    /// would never reach the daemon's key tables as keys.
    fn filter_input_method(&self, controller: &gtk::EventControllerKey) -> ImOutcome {
        let imp = self.imp();
        let Some(im) = imp.im.borrow().clone() else {
            return ImOutcome::Text(None);
        };
        let Some(event) = controller.current_event() else {
            return ImOutcome::Text(None);
        };
        imp.pending_commit.replace(None);
        imp.in_key_press.set(true);
        let claimed = im.filter_keypress(&event);
        imp.in_key_press.set(false);
        let committed = imp.pending_commit.borrow_mut().take();
        match committed {
            Some(text) => ImOutcome::Text(Some(text)),
            None if claimed || imp.composing.get() => ImOutcome::Composing,
            None => ImOutcome::Text(None),
        }
    }

    fn send_text(&self, text: &str) {
        if let Some(engine) = self.engine() {
            engine.send_text(self.pane(), text.to_owned());
        }
    }

    /// Pane-scoped chrome is answered here; everything window-shaped is handed
    /// to the shell, which owns tabs, fonts and the connection.
    fn perform(&self, engine: &Arc<Engine>, action: ChromeAction) {
        match action {
            ChromeAction::ClosePane => engine.kill_pane(self.pane()),
            ChromeAction::TerminalPaste => self.paste_from(&self.clipboard()),
            ChromeAction::TerminalCopy => self.copy_selection(ClipboardTarget::Clipboard),
            ChromeAction::TerminalSelectAll => self.view_action(TerminalViewAction::SelectAll),
            ChromeAction::TerminalClearHistory => {
                self.view_action(TerminalViewAction::ClearHistory);
            }
            other => {
                let chrome = self.imp().chrome.borrow().clone();
                match chrome {
                    Some(chrome) => chrome(other),
                    None => log::debug!(
                        "zz-gtk has no handler for the {} chrome action",
                        other.name()
                    ),
                }
            }
        }
    }

    fn view_action(&self, action: TerminalViewAction) {
        if let Some(engine) = self.engine() {
            engine.send(InputMessage::TerminalView {
                pane: self.pane(),
                action,
            });
        }
    }

    fn copy_selection(&self, target: ClipboardTarget) {
        self.view_action(TerminalViewAction::CopySelection {
            request_id: COPY_REQUEST.fetch_add(1, Ordering::Relaxed),
            target,
        });
    }

    /// Pastes travel as a view action rather than typed text so the daemon
    /// applies bracketed paste and the pane's own paste limits.
    fn paste_from(&self, clipboard: &gdk::Clipboard) {
        let clipboard = clipboard.clone();
        let target = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(text) = clipboard.read_text_future().await else {
                return;
            };
            let (Some(view), Some(text)) = (target.upgrade(), text) else {
                return;
            };
            if !text.is_empty() {
                view.view_action(TerminalViewAction::Paste(text.into()));
            }
        });
    }
}

fn pointer_button(button: u32) -> Option<TerminalMouseButton> {
    match button {
        gdk::BUTTON_PRIMARY => Some(TerminalMouseButton::Left),
        MIDDLE_BUTTON => Some(TerminalMouseButton::Middle),
        gdk::BUTTON_SECONDARY => Some(TerminalMouseButton::Right),
        _ => None,
    }
}

/// The `ui` table owns chords that belong to the whole client; `terminal` owns
/// the ones only a terminal surface answers.
fn resolve_chrome(chrome: &ChromeKeymap, input: &KeyInput) -> Option<ChromeAction> {
    chrome
        .resolve(zz_client::UI_TABLE, input)
        .or_else(|| chrome.resolve(zz_client::TERMINAL_TABLE, input))
}

struct StyleRun {
    start: usize,
    end: usize,
    style: PackedStyle,
}

impl TerminalView {
    fn paint(&self, snapshot: &gtk::Snapshot, viewport: &TerminalViewport, bounds: graphene::Rect) {
        let metrics = self.imp().metrics.get();
        snapshot.append_color(&colors::rgba(viewport.background), &bounds);
        let visible = (bounds.height() / metrics.height).ceil() as usize;
        let mut cache = self.imp().rows.borrow_mut();
        cache.resize(usize::from(viewport.rows), None);
        for row in 0..usize::from(viewport.rows).min(visible.saturating_add(1)) {
            if cache[row].is_none() {
                cache[row] = self.row_node(viewport, row, metrics);
            }
            let Some(node) = cache[row].as_ref() else {
                continue;
            };
            snapshot.save();
            snapshot.translate(&graphene::Point::new(0.0, row as f32 * metrics.height));
            snapshot.append_node(node);
            snapshot.restore();
        }
        drop(cache);
        self.paint_cursor(snapshot, viewport, metrics);
    }

    /// One retained node per row: rows the damage did not name are replayed
    /// untouched, so a busy pane only re-shapes the lines that moved.
    fn row_node(
        &self,
        viewport: &TerminalViewport,
        row: usize,
        metrics: CellMetrics,
    ) -> Option<gsk::RenderNode> {
        let columns = usize::from(viewport.columns);
        let start = row.checked_mul(columns)?;
        let cells = viewport.cells.get(start..start.checked_add(columns)?)?;
        let snapshot = gtk::Snapshot::new();
        let font = self.imp().font.borrow().clone();

        for run in style_runs(viewport, cells) {
            let x = run.start as f32 * metrics.width;
            let width = (run.end - run.start) as f32 * metrics.width;
            if run.style.background() != viewport.background {
                snapshot.append_color(
                    &colors::rgba(run.style.background()),
                    &graphene::Rect::new(x, 0.0, width, metrics.height),
                );
            }
            if run.style.invisible() {
                continue;
            }
            self.paint_run(&snapshot, viewport, cells, &run, metrics, &font);
        }
        self.paint_overlays(&snapshot, viewport, row, metrics);
        snapshot.to_node()
    }

    /// A run is broken again at every spacer cell so a wide glyph's second
    /// column stays empty and the next segment restarts on the column grid.
    fn paint_run(
        &self,
        snapshot: &gtk::Snapshot,
        viewport: &TerminalViewport,
        cells: &[PackedCell],
        run: &StyleRun,
        metrics: CellMetrics,
        font: &pango::FontDescription,
    ) {
        let attributes = run_attributes(run.style);
        let mut column = run.start;
        while column < run.end {
            let mut text = String::new();
            let segment_start = column;
            while column < run.end
                && !matches!(
                    cells[column].width(),
                    CellWidth::SpacerTail | CellWidth::SpacerHead
                )
            {
                match viewport.glyph(cells[column]) {
                    Glyph::Empty => text.push(' '),
                    Glyph::Scalar(character) => text.push(character),
                    Glyph::Grapheme(grapheme) => text.push_str(grapheme),
                }
                column += 1;
            }
            while column < run.end
                && matches!(
                    cells[column].width(),
                    CellWidth::SpacerTail | CellWidth::SpacerHead
                )
            {
                column += 1;
            }
            if text.trim_end().is_empty() {
                continue;
            }
            let layout = self.create_pango_layout(Some(&text));
            layout.set_font_description(Some(font));
            layout.set_attributes(Some(&attributes));
            let foreground = if run.style.faint() {
                colors::rgba_faded(run.style.foreground(), FAINT_ALPHA)
            } else {
                colors::rgba(run.style.foreground())
            };
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                segment_start as f32 * metrics.width,
                0.0,
            ));
            snapshot.append_layout(&layout, &foreground);
            snapshot.restore();
        }
    }

    fn paint_overlays(
        &self,
        snapshot: &gtk::Snapshot,
        viewport: &TerminalViewport,
        row: usize,
        metrics: CellMetrics,
    ) {
        let appearance = self.imp().appearance.borrow();
        for overlay in viewport
            .overlays
            .iter()
            .filter(|overlay| usize::from(overlay.row) == row && overlay.end > overlay.start)
        {
            let color = match overlay.kind() {
                OverlayKind::Selection => appearance.selection_background,
                OverlayKind::SearchMatch => appearance.search_match_color,
                OverlayKind::SearchCurrent => appearance.search_current_color,
                OverlayKind::CopyCursor => appearance.copy_cursor_color,
                OverlayKind::LinkHover => continue,
            };
            snapshot.append_color(
                &colors::appearance_rgba(color),
                &graphene::Rect::new(
                    f32::from(overlay.start) * metrics.width,
                    0.0,
                    f32::from(overlay.end - overlay.start) * metrics.width,
                    metrics.height,
                ),
            );
        }
    }

    fn paint_cursor(
        &self,
        snapshot: &gtk::Snapshot,
        viewport: &TerminalViewport,
        metrics: CellMetrics,
    ) {
        let Some(cursor) = viewport.cursor.filter(|cursor| cursor.visible()) else {
            return;
        };
        let x = f32::from(cursor.column()) * metrics.width;
        let y = f32::from(cursor.row()) * metrics.height;
        let color = colors::rgba(cursor.color());
        let focused = self.has_focus();
        let style = if focused {
            cursor.style()
        } else {
            CursorStyle::BlockHollow
        };
        match style {
            CursorStyle::Bar => snapshot.append_color(
                &color,
                &graphene::Rect::new(x, y, CURSOR_THICKNESS, metrics.height),
            ),
            CursorStyle::Underline => snapshot.append_color(
                &color,
                &graphene::Rect::new(
                    x,
                    y + metrics.height - CURSOR_THICKNESS,
                    metrics.width,
                    CURSOR_THICKNESS,
                ),
            ),
            CursorStyle::BlockHollow => {
                for edge in [
                    graphene::Rect::new(x, y, metrics.width, CURSOR_THICKNESS),
                    graphene::Rect::new(
                        x,
                        y + metrics.height - CURSOR_THICKNESS,
                        metrics.width,
                        CURSOR_THICKNESS,
                    ),
                    graphene::Rect::new(x, y, CURSOR_THICKNESS, metrics.height),
                    graphene::Rect::new(
                        x + metrics.width - CURSOR_THICKNESS,
                        y,
                        CURSOR_THICKNESS,
                        metrics.height,
                    ),
                ] {
                    snapshot.append_color(&color, &edge);
                }
            }
            CursorStyle::Block => {
                snapshot.append_color(
                    &color,
                    &graphene::Rect::new(x, y, metrics.width, metrics.height),
                );
                self.paint_cursor_glyph(snapshot, viewport, cursor, x, y);
            }
        }
    }

    /// A filled block hides the glyph under it, so it is redrawn in the pane's
    /// background color — one layout per frame, not per cell.
    fn paint_cursor_glyph(
        &self,
        snapshot: &gtk::Snapshot,
        viewport: &TerminalViewport,
        cursor: Cursor,
        x: f32,
        y: f32,
    ) {
        let index = usize::from(cursor.row()) * usize::from(viewport.columns)
            + usize::from(cursor.column());
        let Some(cell) = viewport.cells.get(index) else {
            return;
        };
        let text = match viewport.glyph(*cell) {
            Glyph::Empty => return,
            Glyph::Scalar(character) => character.to_string(),
            Glyph::Grapheme(grapheme) => grapheme.to_owned(),
        };
        let layout = self.create_pango_layout(Some(&text));
        layout.set_font_description(Some(&*self.imp().font.borrow()));
        snapshot.save();
        snapshot.translate(&graphene::Point::new(x, y));
        snapshot.append_layout(&layout, &colors::rgba(viewport.background));
        snapshot.restore();
    }
}

/// Runs are keyed by the resolved style value, never by `style_id`: patch
/// streams append to the frame dictionary while full frames rebuild it, so the
/// same visible row can carry different ids.
fn style_runs(viewport: &TerminalViewport, cells: &[PackedCell]) -> Vec<StyleRun> {
    let default = PackedStyle::new(
        viewport.foreground,
        viewport.background,
        None,
        0,
        UnderlineStyle::None,
    );
    let mut runs: Vec<StyleRun> = Vec::new();
    for (column, cell) in cells.iter().enumerate() {
        let style = viewport.style(*cell).unwrap_or(default);
        match runs.last_mut() {
            Some(run) if run.style == style => run.end = column + 1,
            _ => runs.push(StyleRun {
                start: column,
                end: column + 1,
                style,
            }),
        }
    }
    runs
}

fn run_attributes(style: PackedStyle) -> pango::AttrList {
    let attributes = pango::AttrList::new();
    if style.bold() {
        attributes.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
    }
    if style.italic() {
        attributes.insert(pango::AttrInt::new_style(pango::Style::Italic));
    }
    if style.strikethrough() {
        attributes.insert(pango::AttrInt::new_strikethrough(true));
    }
    let underline = match style.underline() {
        UnderlineStyle::None => None,
        UnderlineStyle::Double => Some(pango::Underline::Double),
        UnderlineStyle::Curly | UnderlineStyle::Dotted | UnderlineStyle::Dashed => {
            Some(pango::Underline::Error)
        }
        UnderlineStyle::Single => Some(pango::Underline::Single),
    };
    if let Some(underline) = underline {
        attributes.insert(pango::AttrInt::new_underline(underline));
    }
    if let Some(color) = style.underline_color() {
        attributes.insert(pango::AttrColor::new_underline_color(
            u16::from(color.r) << 8,
            u16::from(color.g) << 8,
            u16::from(color.b) << 8,
        ));
    }
    attributes
}

#[cfg(test)]
mod tests {
    use gtk::gdk;
    use zz_client::{ChromeAction, ChromeKeymap, ChromeProfile};
    use zz_terminal::KeyAction;

    use super::resolve_chrome;
    use crate::ui::keys;

    fn press(keyval: gdk::Key, state: gdk::ModifierType) -> zz_terminal::KeyInput {
        keys::key_input(KeyAction::Press, keyval, state, None)
    }

    #[test]
    fn chrome_claims_its_own_chords() {
        let chrome = ChromeKeymap::for_profile(ChromeProfile::DESKTOP);

        let copy = press(
            gdk::Key::C,
            gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK,
        );

        assert_eq!(
            resolve_chrome(&chrome, &copy),
            Some(ChromeAction::TerminalCopy)
        );
    }

    #[test]
    fn plain_typing_never_reaches_chrome() {
        let chrome = ChromeKeymap::for_profile(ChromeProfile::DESKTOP);

        for keyval in [gdk::Key::a, gdk::Key::A, gdk::Key::Return, gdk::Key::slash] {
            assert_eq!(
                resolve_chrome(&chrome, &press(keyval, gdk::ModifierType::empty())),
                None,
                "chrome swallowed {keyval:?}"
            );
        }
        assert_eq!(
            resolve_chrome(
                &chrome,
                &press(gdk::Key::c, gdk::ModifierType::CONTROL_MASK)
            ),
            None,
            "Control-C belongs to the pane"
        );
    }
}
