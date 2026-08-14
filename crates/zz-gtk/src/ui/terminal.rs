use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use gtk::{gdk, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
use zz_client::{ChromeAction, ChromeKeymap, ViewportDamage};
use zz_protocol::{InputMessage, PaneId};
use zz_terminal::{
    CellWidth, ClipboardTarget, Cursor, CursorStyle, GRAPHEME_TABLE_BIT, Glyph, KeyAction,
    KeyInput, OverlayKind, PackedCell, PackedStyle, PointerCellEvent, ScrollbarState,
    TerminalAppearance, TerminalDictionary, TerminalMouseButton, TerminalMouseInput,
    TerminalMousePhase, TerminalViewAction, TerminalViewport, UnderlineStyle,
};

use crate::{
    engine::{Engine, HistoryRow, local_scroll_gate, max_scroll_offset},
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

    use super::{CellMetrics, DEFAULT_COLUMNS, DEFAULT_ROWS, LocalScroll};
    use crate::engine::Engine;

    #[derive(Default)]
    pub struct TerminalView {
        pub engine: RefCell<Option<Arc<Engine>>>,
        pub chrome: RefCell<Option<Rc<dyn Fn(ChromeAction)>>>,
        pub search: RefCell<Option<Rc<dyn Fn()>>>,
        pub pane: Cell<PaneId>,
        pub frozen: Cell<bool>,
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
        pub scroll: RefCell<Option<LocalScroll>>,
        /// Mirrors `scroll.is_some()` so the frame path can skip the overlay
        /// with a plain bool read instead of a `RefCell` borrow.
        pub scrolling: Cell<bool>,
        pub scroll_serial: Cell<u64>,
        pub scrollbar_dragging: Cell<bool>,
        pub hover: RefCell<Option<String>>,
        pub popup: RefCell<Option<gtk::Popover>>,
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
/// A local scroll waits this long before telling the daemon where it went, so a
/// continuous gesture costs one message per quiet stretch rather than one per
/// notch.
const SCROLL_DEBOUNCE: Duration = Duration::from_millis(120);
/// How long the overlay waits for the daemon to agree before giving up and
/// falling back to whatever the live frame says.
const SCROLL_TIMEOUT: Duration = Duration::from_secs(2);
/// What is left of the deadline once the debounce has elapsed.
const SCROLL_DEADLINE: Duration = Duration::from_millis(1_880);
/// Rows the ring shows as an unfilled band while their backfill is in flight.
const SHIMMER_ALPHA: f32 = 0.04;
/// The daemon reports a hovered link through the viewport; the popup shows at
/// most this much of it.
const HOVER_URI_CHARACTERS: usize = 96;

/// Copy requests are correlated by the daemon alone; the client only has to
/// keep two outstanding requests from sharing an id.
static COPY_REQUEST: AtomicU64 = AtomicU64::new(1);

/// A scroll the client is painting out of its own scrollback while the daemon
/// catches up. `serial` orphans the timers of a gesture that has moved on.
#[derive(Clone)]
pub struct LocalScroll {
    target: u32,
    serial: u64,
    started: Instant,
    /// The window the ring could answer, resolved once per move and replayed by
    /// every paint until the target changes.
    rows: Vec<Option<HistoryRow>>,
    nodes: Vec<Option<gsk::RenderNode>>,
}

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

    /// The daemon's frozen command-output view — `list-keys` and friends. It
    /// rides the same painter and the same key path as a pane, but its actions
    /// and its geometry go down the client-scoped command-output lane instead
    /// of a pane-scoped one.
    pub fn new_command_output(
        engine: Arc<Engine>,
        pane: PaneId,
        appearance: TerminalAppearance,
        chrome: Rc<dyn Fn(ChromeAction)>,
    ) -> Self {
        let view = Self::new(engine, pane, appearance, chrome);
        view.imp().frozen.set(true);
        view
    }

    pub fn pane(&self) -> PaneId {
        self.imp().pane.get()
    }

    pub fn engine(&self) -> Option<Arc<Engine>> {
        self.imp().engine.borrow().clone()
    }

    /// What answers the client's own search chord. The strip lives one widget
    /// up, so the surface hands the chord over rather than owning a prompt.
    pub fn set_search_handler(&self, open: Rc<dyn Fn()>) {
        self.imp().search.replace(Some(open));
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
        if imp.scrolling.get() {
            self.reconcile_local_scroll(&viewport);
        }
        if imp.hover.borrow().as_deref() != viewport.presentation.hovered_uri.as_deref() {
            self.sync_hover(viewport.presentation.hovered_uri.as_deref());
        }
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
        let cell_width_px = (metrics.width * scale).round() as u32;
        let cell_height_px = (metrics.height * scale).round() as u32;
        if self.imp().frozen.get() {
            if columns > 0 && rows > 0 {
                engine.send(InputMessage::ResizeCommandOutput {
                    columns,
                    rows,
                    cell_width_px,
                    cell_height_px,
                });
            }
            return;
        }
        engine.resize_terminal(self.pane(), columns, rows, cell_width_px, cell_height_px);
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
        let leave_target = self.downgrade();
        motion.connect_leave(move |_| {
            let Some(view) = leave_target.upgrade() else {
                return;
            };
            if view.imp().hover.borrow().is_some() {
                view.view_action(TerminalViewAction::ClearLinkHover);
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
        if !self.imp().frozen.get() {
            engine.select_pane(self.pane());
        }
        self.imp().pointer.set((x, y));
        let modifiers = gesture.current_event_state();
        let button = gesture.current_button();
        if button == gdk::BUTTON_PRIMARY && self.scrollbar_hit(x) {
            self.imp().scrollbar_dragging.set(true);
            self.scroll_to_pointer(y);
            return;
        }
        let force = self.local_selection(modifiers);
        if button == MIDDLE_BUTTON && force {
            self.paste_from(&self.primary_clipboard());
            return;
        }
        // A press is where the daemon decides whether a modified click opens the
        // link under it, so the pending local offset has to be real first.
        self.flush_local_scroll();
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
        );
        self.view_action(TerminalViewAction::Mouse(input));
    }

    fn on_motion(&self, controller: &gtk::EventControllerMotion, x: f64, y: f64) {
        self.imp().pointer.set((x, y));
        if self.imp().scrollbar_dragging.get() {
            self.scroll_to_pointer(y);
            return;
        }
        let dragging = self.imp().dragging.get();
        let modifiers = controller.current_event_state();
        // Link discovery is the daemon's, and it only recomputes from motion
        // while the link modifier is down — so plain motion never has to travel.
        if !dragging && !self.mouse_tracking() && !Self::link_modifier(modifiers) {
            return;
        }
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
        );
        self.view_action(TerminalViewAction::Mouse(input));
    }

    /// A drag that covered more than one cell copies to the primary selection
    /// on release, the way every X11 and Wayland terminal does.
    fn on_release(&self, gesture: &gtk::GestureClick, presses: i32, x: f64, y: f64) {
        let modifiers = gesture.current_event_state();
        let button = gesture.current_button();
        if self.imp().scrollbar_dragging.get() {
            self.scroll_to_pointer(y);
            self.imp().scrollbar_dragging.set(false);
            return;
        }
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
        // Toward older history the client can often paint the move itself; the
        // other direction is always the daemon's, because the ring holds
        // nothing newer than the live frame.
        if lines < 0.0 && self.scroll_locally_by(lines as i64) {
            return glib::Propagation::Stop;
        }
        self.flush_local_scroll();
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
        );
        self.view_action(TerminalViewAction::ScrollWheel {
            lines: lines as i32,
            input,
        });
        glib::Propagation::Stop
    }

    /// True when this press selects text here rather than reaching the program:
    /// Shift always does, and so does a pane nobody is tracking. Local only —
    /// the wire's `force_selection` means something narrower.
    fn local_selection(&self, modifiers: gdk::ModifierType) -> bool {
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
            forced_selection(modifiers, cell.click_count),
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
            // The daemon only recomputes a hover from pointer motion, so a
            // modifier pressed over a stationary pointer needs one made up.
            if matches!(
                keyval,
                gdk::Key::Control_L
                    | gdk::Key::Control_R
                    | gdk::Key::Super_L
                    | gdk::Key::Super_R
                    | gdk::Key::Meta_L
                    | gdk::Key::Meta_R
            ) {
                self.republish_pointer(state | gdk::ModifierType::CONTROL_MASK);
            }
            return glib::Propagation::Proceed;
        }
        let Some(engine) = self.engine() else {
            return glib::Propagation::Proceed;
        };
        self.flush_local_scroll();
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
            if self.imp().hover.borrow().is_some() {
                self.view_action(TerminalViewAction::ClearLinkHover);
            }
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
            ChromeAction::TerminalSearch => {
                if let Some(open) = self.imp().search.borrow().clone() {
                    open();
                }
            }
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

    pub fn view_action(&self, action: TerminalViewAction) {
        let Some(engine) = self.engine() else {
            return;
        };
        engine.send(if self.imp().frozen.get() {
            InputMessage::CommandOutputView { action }
        } else {
            InputMessage::TerminalView {
                pane: self.pane(),
                action,
            }
        });
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

/// The scrollbar's hit strip is wider than the thumb it paints, matching the
/// desktop: 16 logical pixels of gutter, a 6-pixel thumb inset by 2, and a
/// floor so a very long scrollback still leaves something to grab.
const GUTTER_WIDTH: f32 = 16.0;
const THUMB_WIDTH: f32 = 6.0;
const THUMB_INSET: f32 = 2.0;
const MIN_THUMB_HEIGHT: f32 = 48.0;
const THUMB_ALPHA: f32 = 0.28;

/// Local scrolling: the client owns the offset for as long as it can paint it,
/// and tells the daemon once the gesture goes quiet.
impl TerminalView {
    /// Move the local offset by `delta` rows, negative toward older history.
    /// True when the overlay took the gesture and nothing should reach the wire.
    fn scroll_locally_by(&self, delta: i64) -> bool {
        let imp = self.imp();
        if imp.frozen.get() {
            return false;
        }
        let Some(engine) = self.engine() else {
            return false;
        };
        let Some(viewport) = imp.viewport.borrow().clone() else {
            return false;
        };
        let scrollbar = viewport.scrollbar;
        let base = imp
            .scroll
            .borrow()
            .as_ref()
            .map_or(scrollbar.offset, |scroll| scroll.target);
        let target = (i64::from(base).saturating_add(delta))
            .clamp(0, i64::from(max_scroll_offset(scrollbar))) as u32;
        self.scroll_locally_to(&engine, &viewport, target)
    }

    fn scroll_locally_to(
        &self,
        engine: &Arc<Engine>,
        viewport: &TerminalViewport,
        target: u32,
    ) -> bool {
        let scrollbar = viewport.scrollbar;
        let pane = self.pane();
        // Retire whatever the pane has outgrown before asking for more: the
        // ring answers "how far back do I already reach" from its anchor, and
        // an anchor a frame has moved past would aim the request at the wrong
        // rows — or, on a ring that has never seen a frame, at nothing at all.
        let retained = engine.history_rows(pane, viewport);
        // Warm it for wherever this is heading, even when the overlay cannot
        // take this notch: the next one usually can.
        engine.request_history(pane, target.saturating_sub(scrollbar.len));
        if !local_scroll_gate(viewport, retained) {
            self.clear_local_scroll();
            return false;
        }
        let covered = scrollbar
            .offset
            .saturating_sub(u32::try_from(retained).unwrap_or(u32::MAX));
        if target >= scrollbar.offset {
            // The live frame already shows this; there is nothing to overlay.
            let had = self.clear_local_scroll();
            return had && target == scrollbar.offset;
        }
        if target < covered {
            self.clear_local_scroll();
            return false;
        }

        let imp = self.imp();
        let serial = imp.scroll_serial.get().wrapping_add(1);
        imp.scroll_serial.set(serial);
        imp.scroll.replace(Some(LocalScroll {
            target,
            serial,
            started: Instant::now(),
            rows: engine.history_window(pane, target, viewport.rows),
            nodes: vec![None; usize::from(viewport.rows)],
        }));
        imp.scrolling.set(true);
        self.queue_draw();
        self.schedule_scroll_sync(serial);
        true
    }

    /// Drop the overlay. True when one was up.
    fn clear_local_scroll(&self) -> bool {
        let imp = self.imp();
        if !imp.scrolling.get() {
            return false;
        }
        imp.scroll_serial
            .set(imp.scroll_serial.get().wrapping_add(1));
        imp.scroll.replace(None);
        imp.scrolling.set(false);
        self.queue_draw();
        true
    }

    /// Send the offset the overlay was holding before anything else reaches the
    /// wire, so the daemon lands where the user left off rather than where its
    /// own frame still is.
    fn flush_local_scroll(&self) {
        let target = self
            .imp()
            .scroll
            .borrow()
            .as_ref()
            .map(|scroll| scroll.target);
        if let Some(target) = target {
            self.clear_local_scroll();
            self.view_action(TerminalViewAction::ScrollToOffset(target));
        }
    }

    /// One message per quiet stretch, then a deadline: a daemon that never
    /// agrees must not leave a stale overlay pinned over a live pane.
    fn schedule_scroll_sync(&self, serial: u64) {
        let target = self.downgrade();
        glib::timeout_add_local_once(SCROLL_DEBOUNCE, move || {
            let Some(view) = target.upgrade() else {
                return;
            };
            let pending = {
                let held = view.imp().scroll.borrow();
                held.as_ref()
                    .filter(|scroll| scroll.serial == serial)
                    .map(|scroll| scroll.target)
            };
            let Some(pending) = pending else {
                return;
            };
            view.view_action(TerminalViewAction::ScrollToOffset(pending));

            let target = view.downgrade();
            glib::timeout_add_local_once(SCROLL_DEADLINE, move || {
                let Some(view) = target.upgrade() else {
                    return;
                };
                let expired = {
                    let held = view.imp().scroll.borrow();
                    held.as_ref().is_some_and(|scroll| {
                        scroll.serial == serial && scroll.started.elapsed() >= SCROLL_TIMEOUT
                    })
                };
                if expired {
                    view.clear_local_scroll();
                }
            });
        });
    }

    /// Retire the overlay once the daemon agrees, or once the frame it was
    /// anchored to has moved on. Only runs while an overlay is actually up.
    fn reconcile_local_scroll(&self, viewport: &TerminalViewport) {
        let imp = self.imp();
        let retire = {
            let held = imp.scroll.borrow();
            held.as_ref().is_none_or(|scroll| {
                viewport.scrollbar.offset == scroll.target
                    || scroll.started.elapsed() >= SCROLL_TIMEOUT
            })
        };
        if retire {
            self.clear_local_scroll();
        }
    }

    fn paint_scrollbar(
        &self,
        snapshot: &gtk::Snapshot,
        viewport: &TerminalViewport,
        bounds: graphene::Rect,
    ) {
        let scrollbar = self
            .imp()
            .scroll
            .borrow()
            .as_ref()
            .map_or(viewport.scrollbar, |scroll| ScrollbarState {
                offset: scroll.target,
                ..viewport.scrollbar
            });
        if scrollbar.total <= scrollbar.len || scrollbar.total == 0 {
            return;
        }
        let track = (bounds.height() - THUMB_INSET * 2.0).max(0.0);
        let ratio = scrollbar.len as f32 / scrollbar.total as f32;
        let thumb = (track * ratio).max(MIN_THUMB_HEIGHT).min(track);
        let travel = (track - thumb).max(0.0);
        let denominator = scrollbar.total.saturating_sub(scrollbar.len).max(1) as f32;
        let progress = (scrollbar.offset as f32 / denominator).clamp(0.0, 1.0);
        snapshot.append_color(
            &colors::rgba_faded(viewport.foreground, THUMB_ALPHA),
            &graphene::Rect::new(
                bounds.width() - THUMB_WIDTH - THUMB_INSET,
                THUMB_INSET + travel * progress,
                THUMB_WIDTH,
                thumb,
            ),
        );
    }

    /// True while the pointer is over the scroll gutter of a pane that has
    /// somewhere to scroll; the gutter never selects text.
    fn scrollbar_hit(&self, x: f64) -> bool {
        self.imp()
            .viewport
            .borrow()
            .as_ref()
            .is_some_and(|viewport| viewport.scrollbar.total > viewport.scrollbar.len)
            && x as f32 >= self.width() as f32 - GUTTER_WIDTH
    }

    /// Absolute positioning, the way the desktop does it: the thumb jumps so the
    /// pointer's fraction of the track becomes the viewport's fraction of the
    /// scrollback. Grabbing the middle of the thumb snaps it.
    fn scroll_to_pointer(&self, y: f64) {
        let height = self.height() as f32;
        if height <= 0.0 {
            return;
        }
        let fraction = (y as f32 / height).clamp(0.0, 1.0);
        let Some(viewport) = self.imp().viewport.borrow().clone() else {
            return;
        };
        let maximum = max_scroll_offset(viewport.scrollbar);
        let target = (f64::from(maximum) * f64::from(fraction)).round() as u32;
        if let Some(engine) = self.engine()
            && self.scroll_locally_to(&engine, &viewport, target)
        {
            return;
        }
        self.view_action(TerminalViewAction::ScrollToOffset(target));
    }
}

/// Link hover and activation. Discovery is the daemon's: it needs pointer
/// motion while the link modifier is held, answers with `hovered_uri` on the
/// viewport, and turns a modified click into an `OpenUri` event of its own.
impl TerminalView {
    /// True for the modifier the daemon treats as "I mean the link".
    fn link_modifier(modifiers: gdk::ModifierType) -> bool {
        modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            || modifiers.contains(gdk::ModifierType::SUPER_MASK)
    }

    /// Show, move or drop the hover popup. Only touched when the daemon's
    /// reported URI actually changed.
    fn sync_hover(&self, uri: Option<&str>) {
        let imp = self.imp();
        if imp.hover.borrow().as_deref() == uri {
            return;
        }
        imp.hover.replace(uri.map(str::to_owned));
        let Some(uri) = uri else {
            if let Some(popup) = imp.popup.borrow_mut().take() {
                popup.unparent();
            }
            return;
        };
        let label = gtk::Label::builder()
            .label(truncate_uri(uri))
            .ellipsize(pango::EllipsizeMode::Middle)
            .max_width_chars(64)
            .build();
        label.add_css_class("caption");
        let mut held = imp.popup.borrow_mut();
        let popup = held.get_or_insert_with(|| {
            let popup = gtk::Popover::builder()
                .autohide(false)
                .has_arrow(false)
                .position(gtk::PositionType::Top)
                .halign(gtk::Align::End)
                .valign(gtk::Align::End)
                .can_target(false)
                .build();
            popup.add_css_class("zz-link");
            popup.set_parent(self);
            popup
        });
        popup.set_child(Some(&label));
        popup.popup();
    }

    /// The daemon recomputes a hover only from pointer motion, so a modifier
    /// pressed while the pointer sits still needs one synthesized.
    fn republish_pointer(&self, modifiers: gdk::ModifierType) {
        if self.imp().dragging.get() {
            return;
        }
        let (x, y) = self.imp().pointer.get();
        let cell = self.cell_at(x, y, 1, modifiers);
        let input = self.mouse_input(TerminalMousePhase::Motion, None, cell, x, y, modifiers);
        self.view_action(TerminalViewAction::Mouse(input));
    }
}

fn truncate_uri(uri: &str) -> String {
    let mut characters = uri.chars();
    let mut presented: String = characters.by_ref().take(HOVER_URI_CHARACTERS).collect();
    if characters.next().is_some() {
        presented.push('…');
    }
    presented
}

/// The wire's `force_selection` is not "this pane is not tracking the mouse" —
/// it is the narrow "the user is overriding what the program asked for". The
/// daemon reads it to route a wheel notch past alternate-scroll and to refuse
/// to open a link, so widening it would quietly cost both.
fn forced_selection(modifiers: gdk::ModifierType, click_count: u8) -> bool {
    modifiers.contains(gdk::ModifierType::SHIFT_MASK)
        || (click_count >= 3 && TerminalView::link_modifier(modifiers))
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
        let painted = usize::from(viewport.rows).min(visible.saturating_add(1));
        if self.paint_scrollback(snapshot, viewport, metrics, painted) {
            self.paint_scrollbar(snapshot, viewport, bounds);
            return;
        }
        let mut cache = self.imp().rows.borrow_mut();
        cache.resize(usize::from(viewport.rows), None);
        for row in 0..painted {
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
        self.paint_scrollbar(snapshot, viewport, bounds);
    }

    /// Paint the window a local scroll is holding: rows the ring answers come
    /// from client memory, rows the live frame still covers come from the same
    /// per-row cache the live path uses, and anything neither has is a band.
    /// False when no local scroll is up, which is every ordinary frame.
    fn paint_scrollback(
        &self,
        snapshot: &gtk::Snapshot,
        viewport: &TerminalViewport,
        metrics: CellMetrics,
        painted: usize,
    ) -> bool {
        let imp = self.imp();
        if !imp.scrolling.get() {
            return false;
        }
        let mut held = imp.scroll.borrow_mut();
        let Some(scroll) = held.as_mut() else {
            return false;
        };
        let live_top = viewport.scrollbar.offset;
        let mut live = imp.rows.borrow_mut();
        live.resize(usize::from(viewport.rows), None);
        scroll.nodes.resize(usize::from(viewport.rows), None);
        for row in 0..painted {
            let absolute = scroll.target.saturating_add(row as u32);
            let node = if absolute >= live_top {
                let live_row = usize::try_from(absolute - live_top).unwrap_or(usize::MAX);
                match live.get_mut(live_row) {
                    Some(slot) => {
                        if slot.is_none() {
                            *slot = self.row_node(viewport, live_row, metrics);
                        }
                        slot.clone()
                    }
                    None => None,
                }
            } else {
                if scroll.nodes[row].is_none()
                    && let Some(history) = scroll.rows.get(row).and_then(Option::as_ref)
                {
                    scroll.nodes[row] =
                        self.cells_node(&history.cells, &history.dictionary, viewport, metrics);
                }
                scroll.nodes[row].clone()
            };
            match node {
                Some(node) => {
                    snapshot.save();
                    snapshot.translate(&graphene::Point::new(0.0, row as f32 * metrics.height));
                    snapshot.append_node(&node);
                    snapshot.restore();
                }
                None => snapshot.append_color(
                    &colors::rgba_faded(viewport.foreground, SHIMMER_ALPHA),
                    &graphene::Rect::new(
                        0.0,
                        row as f32 * metrics.height,
                        f32::from(viewport.columns) * metrics.width,
                        metrics.height,
                    ),
                ),
            }
        }
        true
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
        self.paint_cells(&snapshot, cells, &viewport.dictionary, viewport, metrics);
        self.paint_overlays(&snapshot, viewport, row, metrics);
        snapshot.to_node()
    }

    /// A scrollback row, resolved against the dictionary its own chunk carried
    /// rather than the live frame's — the two are unrelated tables.
    fn cells_node(
        &self,
        cells: &[PackedCell],
        dictionary: &TerminalDictionary,
        viewport: &TerminalViewport,
        metrics: CellMetrics,
    ) -> Option<gsk::RenderNode> {
        let snapshot = gtk::Snapshot::new();
        self.paint_cells(&snapshot, cells, dictionary, viewport, metrics);
        snapshot.to_node()
    }

    fn paint_cells(
        &self,
        snapshot: &gtk::Snapshot,
        cells: &[PackedCell],
        dictionary: &TerminalDictionary,
        viewport: &TerminalViewport,
        metrics: CellMetrics,
    ) {
        let font = self.imp().font.borrow().clone();
        for run in style_runs(dictionary, viewport, cells) {
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
            self.paint_run(snapshot, dictionary, cells, &run, metrics, &font);
        }
    }

    /// A run is broken again at every spacer cell so a wide glyph's second
    /// column stays empty and the next segment restarts on the column grid.
    fn paint_run(
        &self,
        snapshot: &gtk::Snapshot,
        dictionary: &TerminalDictionary,
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
                match glyph_of(dictionary, cells[column]) {
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

/// The glyph a cell names, resolved against an arbitrary dictionary.
/// [`TerminalViewport::glyph`] only ever answers for its own frame, and a
/// scrollback row carries the table of the chunk that delivered it.
fn glyph_of(dictionary: &TerminalDictionary, cell: PackedCell) -> Glyph<'_> {
    let glyph = cell.glyph();
    if glyph == 0 {
        return Glyph::Empty;
    }
    if glyph & GRAPHEME_TABLE_BIT == 0 {
        return char::from_u32(glyph).map_or(Glyph::Empty, Glyph::Scalar);
    }
    let index = (glyph & !GRAPHEME_TABLE_BIT) as usize;
    let Some((&start, &end)) = dictionary
        .grapheme_offsets
        .get(index)
        .zip(dictionary.grapheme_offsets.get(index.saturating_add(1)))
    else {
        return Glyph::Empty;
    };
    let Some(bytes) = dictionary.grapheme_bytes.get(start as usize..end as usize) else {
        return Glyph::Empty;
    };
    std::str::from_utf8(bytes).map_or(Glyph::Empty, Glyph::Grapheme)
}

/// Runs are keyed by the resolved style value, never by `style_id`: patch
/// streams append to the frame dictionary while full frames rebuild it, so the
/// same visible row can carry different ids.
fn style_runs(
    dictionary: &TerminalDictionary,
    viewport: &TerminalViewport,
    cells: &[PackedCell],
) -> Vec<StyleRun> {
    let default = PackedStyle::new(
        viewport.foreground,
        viewport.background,
        None,
        0,
        UnderlineStyle::None,
    );
    let mut runs: Vec<StyleRun> = Vec::new();
    for (column, cell) in cells.iter().enumerate() {
        let style = dictionary
            .styles
            .get(usize::from(cell.style_id()))
            .copied()
            .unwrap_or(default);
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

    use super::{forced_selection, resolve_chrome, truncate_uri};
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

    /// The daemon refuses to open a link while this bit is set, and routes a
    /// wheel notch past alternate-scroll on it — so "this pane is not tracking
    /// the mouse" must never reach the wire as a forced selection.
    #[test]
    fn only_a_real_override_forces_selection_on_the_wire() {
        const NONE: gdk::ModifierType = gdk::ModifierType::empty();
        const SHIFT: gdk::ModifierType = gdk::ModifierType::SHIFT_MASK;
        const CONTROL: gdk::ModifierType = gdk::ModifierType::CONTROL_MASK;
        const SUPER: gdk::ModifierType = gdk::ModifierType::SUPER_MASK;

        assert!(!forced_selection(NONE, 1));
        assert!(!forced_selection(CONTROL, 1), "Ctrl-click opens a link");
        assert!(!forced_selection(CONTROL, 2));
        assert!(forced_selection(SHIFT, 1));
        assert!(forced_selection(CONTROL, 3), "Ctrl triple-click selects");
        assert!(forced_selection(SUPER, 3));
    }

    #[test]
    fn a_long_link_is_shown_with_its_tail_marked() {
        let short = "https://zzmux.sh";
        assert_eq!(truncate_uri(short), short);

        let long = format!("https://zzmux.sh/{}", "z".repeat(400));
        let shown = truncate_uri(&long);

        assert!(shown.ends_with('…'));
        assert_eq!(shown.chars().count(), 97);
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
