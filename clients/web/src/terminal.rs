use std::{collections::HashSet, ops::Range, time::Duration};

use gpui::{
    AnyElement, App, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, KeyDownEvent, KeyUpEvent, Keystroke, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollWheelEvent, Subscription, Task, UTF16Selection, Window, canvas, div, prelude::*, px,
};
use zz_client::{
    ChromeAction, ChromeKeymap, ChromeProfile, CoreEvent, TERMINAL_TABLE, ViewportDamage,
};
use zz_protocol::{InputMessage, PaneId, PopupAction, TerminalUiCommand};
use zz_terminal::{
    KeyAction, KeyCode, KeyInput, PointerCellEvent, SearchCase, SearchDirection, SearchMode,
    SearchQuery, SearchStatus, SessionStatus, TerminalMode, TerminalMouseButton,
    TerminalMouseInput, TerminalMousePhase, TerminalViewAction, TerminalViewport,
};
use zz_ui::{
    ActiveTheme, Colorize as _,
    pane::{
        PaneOverlayCorner, pane_overlay_stack, terminal_link_popup, terminal_mode_indicator,
        terminal_search_prompt, terminal_status_popup,
    },
    terminal::{
        GridSize, PaintState, RowRenderCache, TerminalRenderInput, cursor_should_blink,
        selection_autoscroll_lines, terminal_font_for_style,
    },
};

use crate::connection::Connection;

pub struct TerminalPane {
    pane: PaneId,
    connection: Entity<Connection>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    cursor_bounds: Bounds<Pixels>,
    surface_bounds: Bounds<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    scale: f32,
    force_local_selection: bool,
    font_delta: f32,
    scroll_rows: f32,
    geometry: Option<GridSize>,
    cache: RowRenderCache,
    row_revisions: Vec<u64>,
    next_revision: u64,
    revision_epoch: u64,
    focused: bool,
    cursor_blink_visible: bool,
    cursor_blink_task: Option<Task<()>>,
    selection_dragging: bool,
    scrollbar_dragging: bool,
    pressed_buttons: HashSet<MouseButton>,
    pointer: Option<Point<Pixels>>,
    selection_pointer: Option<PointerCellEvent>,
    selection_autoscroll_lines: i32,
    selection_autoscroll_task: Option<Task<()>>,
    focus_subscriptions: Vec<Subscription>,
    all_dirty: bool,
    dirty_rows: HashSet<u16>,
    forwarded: HashSet<String>,
    marked_text: Option<String>,
    surface: TerminalSurface,
    chrome: ChromeKeymap,
    apple_chrome: ChromeKeymap,
    search: Option<SearchQuery>,
    search_accept_on_enter: bool,
    swallowed_search_key: Option<KeyCode>,
    _subscription: Subscription,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalSurface {
    Pane,
    Popup,
    CommandOutput,
}

impl TerminalPane {
    pub fn new(pane: PaneId, connection: Entity<Connection>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(
            &connection,
            move |this, connection, event: &CoreEvent, cx| match event {
                CoreEvent::ViewportChanged {
                    pane: changed,
                    damage,
                } if *changed == pane => {
                    match damage {
                        ViewportDamage::All => this.all_dirty = true,
                        ViewportDamage::Rows(rows) => this.dirty_rows.extend(rows),
                    }
                    cx.notify();
                }
                CoreEvent::KittyImageChunk { pane: changed, .. }
                | CoreEvent::KittyImagesRemoved { pane: changed, .. }
                    if *changed == pane =>
                {
                    cx.notify();
                }
                CoreEvent::Attached { .. } | CoreEvent::AppearanceChanged => {
                    this.all_dirty = true;
                    this.geometry = None;
                    this.cursor_blink_visible = true;
                    this.cursor_blink_task = None;
                    cx.notify();
                }
                CoreEvent::CommandOutputChanged
                    if this.surface == TerminalSurface::CommandOutput =>
                {
                    this.all_dirty = true;
                    cx.notify();
                }
                CoreEvent::TerminalUiCommand {
                    pane: changed,
                    command: TerminalUiCommand::BeginSearch { direction },
                } if *changed == pane => {
                    let output_pane = connection
                        .read(cx)
                        .core
                        .command_output()
                        .map(|(pane, _)| pane);
                    if this.surface != TerminalSurface::Pane || output_pane != Some(pane) {
                        this.begin_search(
                            SearchQuery {
                                direction: *direction,
                                ..SearchQuery::default()
                            },
                            true,
                            cx,
                        );
                    }
                }
                _ => {}
            },
        );
        Self {
            pane,
            connection,
            focus: cx.focus_handle(),
            bounds: Bounds::default(),
            cursor_bounds: Bounds::default(),
            surface_bounds: Bounds::default(),
            cell_width: px(8.),
            line_height: px(18.),
            scale: 1.0,
            force_local_selection: false,
            font_delta: 0.,
            scroll_rows: 0.,
            geometry: None,
            cache: RowRenderCache::default(),
            row_revisions: Vec::new(),
            next_revision: 1,
            revision_epoch: 0,
            focused: false,
            cursor_blink_visible: true,
            cursor_blink_task: None,
            selection_dragging: false,
            scrollbar_dragging: false,
            pressed_buttons: HashSet::new(),
            pointer: None,
            selection_pointer: None,
            selection_autoscroll_lines: 0,
            selection_autoscroll_task: None,
            focus_subscriptions: Vec::new(),
            all_dirty: true,
            dirty_rows: HashSet::new(),
            forwarded: HashSet::new(),
            marked_text: None,
            surface: TerminalSurface::Pane,
            chrome: ChromeKeymap::for_profile(ChromeProfile::Desktop),
            apple_chrome: ChromeKeymap::for_profile(ChromeProfile::DesktopApple),
            search: None,
            search_accept_on_enter: false,
            swallowed_search_key: None,
            _subscription: subscription,
        }
    }

    pub fn new_popup(pane: PaneId, connection: Entity<Connection>, cx: &mut Context<Self>) -> Self {
        let mut this = Self::new(pane, connection, cx);
        this.surface = TerminalSurface::Popup;
        this
    }

    pub fn new_command_output(
        pane: PaneId,
        connection: Entity<Connection>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::new(pane, connection, cx);
        this.surface = TerminalSurface::CommandOutput;
        this
    }

    fn viewport<'a>(&self, core: &'a zz_client::ClientCore) -> Option<&'a TerminalViewport> {
        match self.surface {
            TerminalSurface::CommandOutput => core.command_output().map(|(_, viewport)| viewport),
            _ => core.viewport(self.pane),
        }
    }

    fn send(&self, input: InputMessage, cx: &mut Context<Self>) {
        let input = if self.surface == TerminalSurface::Popup {
            match input {
                InputMessage::Key {
                    input,
                    text_follows,
                    ..
                } => InputMessage::Popup {
                    action: PopupAction::Key {
                        input,
                        text_follows,
                    },
                },
                InputMessage::Text { text, .. } => InputMessage::Popup {
                    action: PopupAction::Text(text),
                },
                other => other,
            }
        } else {
            input
        };
        self.connection
            .update(cx, |connection, cx| connection.input(self.pane, input, cx));
    }

    fn view(&self, action: TerminalViewAction, cx: &mut Context<Self>) {
        self.send(
            match self.surface {
                TerminalSurface::Pane => InputMessage::TerminalView {
                    pane: self.pane,
                    action,
                },
                TerminalSurface::Popup => InputMessage::Popup {
                    action: PopupAction::TerminalView(action),
                },
                TerminalSurface::CommandOutput => InputMessage::CommandOutputView { action },
            },
            cx,
        );
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        let input = key_input(event);
        if self.swallowed_search_key == Some(input.key) {
            cx.stop_propagation();
            return;
        }
        if let Some(query) = self.search.as_mut() {
            if input.key == KeyCode::Escape {
                self.close_search(window, cx);
                self.swallowed_search_key = Some(input.key);
            } else if input.key == KeyCode::Enter && self.search_accept_on_enter {
                self.search = None;
                self.marked_text = None;
                self.swallowed_search_key = Some(input.key);
            } else if let Some(action) = search_key_action(query, &input) {
                self.view(action, cx);
            } else if matches!(input.key, KeyCode::Character(_))
                && ((!input.modifiers.control() && !input.modifiers.platform())
                    || (matches!(input.key, KeyCode::Character('v' | 'V'))
                        && !input.modifiers.alt()))
            {
                return;
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if !self.connection.read(cx).core.prefix_armed() {
            match self
                .chrome
                .resolve(TERMINAL_TABLE, &input)
                .or_else(|| self.apple_chrome.resolve(TERMINAL_TABLE, &input))
            {
                Some(
                    action @ (ChromeAction::TerminalFontIncrease
                    | ChromeAction::TerminalFontDecrease),
                ) => {
                    let increment = action == ChromeAction::TerminalFontIncrease;
                    self.font_delta =
                        (self.font_delta + if increment { 1. } else { -1. }).clamp(-6., 24.);
                    self.all_dirty = true;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                Some(ChromeAction::TerminalCopy) => {
                    self.view(
                        TerminalViewAction::CopySelection {
                            request_id: 1,
                            target: zz_terminal::ClipboardTarget::Clipboard,
                        },
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                Some(ChromeAction::TerminalSelectAll) => {
                    self.view(TerminalViewAction::SelectAll, cx);
                    cx.stop_propagation();
                    return;
                }
                Some(ChromeAction::TerminalClearHistory) => {
                    self.view(TerminalViewAction::ClearHistory, cx);
                    cx.stop_propagation();
                    return;
                }
                Some(ChromeAction::TerminalPaste) => return,
                Some(ChromeAction::TerminalSearch) => {
                    self.open_search(window, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        let raw = self
            .viewport(&self.connection.read(cx).core)
            .is_some_and(|viewport| viewport.kitty_keyboard)
            || !matches!(input.key, KeyCode::Character(_))
            || input.modifiers.control()
            || input.modifiers.alt()
            || input.modifiers.platform();
        if raw {
            self.forwarded.insert(event.keystroke.key.clone());
            self.send(
                InputMessage::Key {
                    pane: self.pane,
                    input,
                    text_follows: false,
                },
                cx,
            );
            cx.stop_propagation();
        }
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_search(SearchQuery::default(), false, cx);
        window.focus(&self.focus, cx);
    }

    fn begin_search(&mut self, query: SearchQuery, accept_on_enter: bool, cx: &mut Context<Self>) {
        self.search = Some(query.clone());
        self.search_accept_on_enter = accept_on_enter;
        self.marked_text = None;
        self.view(TerminalViewAction::SearchBegin(query), cx);
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.marked_text = None;
        self.view(TerminalViewAction::SearchClose, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.swallowed_search_key == Some(key_code(&event.keystroke.key)) {
            self.swallowed_search_key = None;
            cx.stop_propagation();
            return;
        }
        if self.search.is_some() {
            self.forwarded.remove(&event.keystroke.key);
            cx.stop_propagation();
            return;
        }
        if self.forwarded.remove(&event.keystroke.key) {
            self.send(
                InputMessage::Key {
                    pane: self.pane,
                    input: keystroke_input(&event.keystroke, KeyAction::Release),
                    text_follows: false,
                },
                cx,
            );
            cx.stop_propagation();
        }
    }

    fn mouse(
        &self,
        position: Point<Pixels>,
        modifiers: gpui::Modifiers,
        phase: TerminalMousePhase,
        button: Option<TerminalMouseButton>,
        count: usize,
    ) -> TerminalMouseInput {
        let x = f32::from(position.x - self.bounds.origin.x)
            .clamp(0., f32::from(self.bounds.size.width));
        let y = f32::from(position.y - self.bounds.origin.y)
            .clamp(0., f32::from(self.bounds.size.height));
        let column = (x / f32::from(self.cell_width)) as u16;
        let row = (y / f32::from(self.line_height)) as u16;
        TerminalMouseInput::new(
            phase,
            button,
            PointerCellEvent {
                column: column.min(
                    self.geometry
                        .map_or(0, |grid| grid.columns.saturating_sub(1)),
                ),
                row: row.min(self.geometry.map_or(0, |grid| grid.rows.saturating_sub(1))),
                click_count: count.min(3) as u8,
                rectangle: modifiers.alt,
            },
            (x * self.scale).round() as u32,
            (y * self.scale).round() as u32,
            (f32::from(self.bounds.size.width) * self.scale).round() as u32,
            (f32::from(self.bounds.size.height) * self.scale).round() as u32,
            (f32::from(self.cell_width) * self.scale).round().max(1.) as u32,
            (f32::from(self.line_height) * self.scale).round().max(1.) as u32,
            wire_modifiers(modifiers),
            modifiers.shift || self.force_local_selection,
        )
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.sync_focus(window, cx);
        self.reset_cursor_blink(cx);
        self.pointer = Some(event.position);
        if self.surface == TerminalSurface::Pane {
            self.connection.update(cx, |connection, cx| {
                connection.command("select-pane", vec!["-t".into(), self.pane.to_string()], cx);
            });
        }
        if event.button == MouseButton::Left && self.scrollbar_hit(event.position, cx) {
            self.end_drag();
            self.scrollbar_dragging = true;
            self.view(TerminalViewAction::ClearLinkHover, cx);
            self.scroll_to_pointer(event.position, cx);
            cx.stop_propagation();
            return;
        }
        if !self.bounds.contains(&event.position) {
            return;
        }
        if event.button == MouseButton::Left {
            self.force_local_selection =
                event.click_count >= 3 && (event.modifiers.control || event.modifiers.platform);
            let tracking = self
                .viewport(&self.connection.read(cx).core)
                .is_some_and(|viewport| viewport.mouse_tracking);
            self.selection_dragging =
                !tracking || event.modifiers.shift || self.force_local_selection;
        }
        self.pressed_buttons.insert(event.button);
        let input = self.mouse(
            event.position,
            event.modifiers,
            TerminalMousePhase::Press,
            mouse_button(event.button),
            event.click_count,
        );
        self.selection_pointer = self.selection_dragging.then_some(input.cell);
        self.view(TerminalViewAction::Mouse(input), cx);
        cx.stop_propagation();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.reset_cursor_blink(cx);
        if self.scrollbar_dragging && event.button == MouseButton::Left {
            self.scroll_to_pointer(event.position, cx);
            self.end_drag();
            cx.stop_propagation();
            return;
        }
        if self.pressed_buttons.remove(&event.button) {
            let input = self.mouse(
                event.position,
                event.modifiers,
                TerminalMousePhase::Release,
                mouse_button(event.button),
                event.click_count,
            );
            self.view(TerminalViewAction::Mouse(input), cx);
            if event.button == MouseButton::Left {
                self.end_drag();
            }
            cx.stop_propagation();
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pointer = Some(event.position);
        if event.pressed_button.is_none() {
            self.end_drag();
            self.pressed_buttons.clear();
        }
        if self.scrollbar_dragging {
            self.scroll_to_pointer(event.position, cx);
            cx.stop_propagation();
            return;
        }
        if event
            .pressed_button
            .is_some_and(|button| !self.pressed_buttons.contains(&button))
        {
            return;
        }
        if !self.selection_dragging && !self.bounds.contains(&event.position) {
            self.view(TerminalViewAction::ClearLinkHover, cx);
            return;
        }
        let input = self.mouse(
            event.position,
            event.modifiers,
            TerminalMousePhase::Motion,
            event.pressed_button.and_then(mouse_button),
            1,
        );
        if self.selection_dragging {
            self.selection_pointer = Some(input.cell);
            self.selection_autoscroll_lines =
                selection_autoscroll_lines(self.bounds, self.line_height, event.position);
            self.ensure_selection_autoscroll(cx);
        }
        self.view(TerminalViewAction::Mouse(input), cx);
        if event.pressed_button.is_some() {
            cx.stop_propagation();
        }
    }

    fn on_mouse_exit(&mut self, _: &MouseExitEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pointer = None;
        self.view(TerminalViewAction::ClearLinkHover, cx);
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(position) = self
            .pointer
            .filter(|position| self.bounds.contains(position))
        {
            self.view(
                TerminalViewAction::Mouse(self.mouse(
                    position,
                    event.modifiers,
                    TerminalMousePhase::Motion,
                    None,
                    1,
                )),
                cx,
            );
        }
    }

    fn scrollbar_hit(&self, position: Point<Pixels>, cx: &App) -> bool {
        self.viewport(&self.connection.read(cx).core)
            .is_some_and(|viewport| {
                zz_ui::terminal::scrollbar_strip_hit(
                    self.surface_bounds,
                    viewport.scrollbar,
                    position,
                )
            })
    }

    fn scroll_to_pointer(&self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.view(
            TerminalViewAction::ScrollToFraction(zz_ui::terminal::scroll_fraction(
                self.surface_bounds,
                position,
            )),
            cx,
        );
    }

    fn end_drag(&mut self) {
        self.selection_dragging = false;
        self.scrollbar_dragging = false;
        self.force_local_selection = false;
        self.selection_pointer = None;
        self.selection_autoscroll_lines = 0;
        self.selection_autoscroll_task = None;
    }

    fn ensure_selection_autoscroll(&mut self, cx: &mut Context<Self>) {
        if self.selection_autoscroll_lines == 0 || !self.selection_dragging {
            self.selection_autoscroll_task = None;
            return;
        }
        if self.selection_autoscroll_task.is_some() {
            return;
        }
        self.selection_autoscroll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let Ok(keep_running) = this.update(cx, |view, cx| {
                    let Some(pointer) = view.selection_pointer else {
                        return false;
                    };
                    if !view.selection_dragging || view.selection_autoscroll_lines == 0 {
                        return false;
                    }
                    view.view(
                        TerminalViewAction::SelectionAutoscroll {
                            lines: view.selection_autoscroll_lines,
                            pointer,
                        },
                        cx,
                    );
                    true
                }) else {
                    break;
                };
                if !keep_running {
                    break;
                }
            }
        }));
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.line_height);
        let lines = accumulate_scroll(
            &mut self.scroll_rows,
            -f32::from(delta.y) / f32::from(self.line_height),
        );
        if lines != 0 {
            self.view(
                TerminalViewAction::ScrollWheel {
                    lines,
                    input: self.mouse(
                        event.position,
                        event.modifiers,
                        TerminalMousePhase::Press,
                        Some(if lines < 0 {
                            TerminalMouseButton::ScrollUp
                        } else {
                            TerminalMouseButton::ScrollDown
                        }),
                        1,
                    ),
                },
                cx,
            );
            cx.stop_propagation();
        }
    }

    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PaintState> {
        let (paint, attached) = self.connection.clone().update(cx, |connection, cx| {
            for image in connection.take_retired_terminal_images() {
                let _ = window.drop_image(image);
            }
            let viewport = self.viewport(&connection.core)?;
            let appearance = connection.core.appearance().cloned().unwrap_or_default();
            if self.row_revisions.len() != usize::from(viewport.rows) {
                self.row_revisions.resize(usize::from(viewport.rows), 0);
                self.all_dirty = true;
            }
            if self.all_dirty {
                self.revision_epoch = self.revision_epoch.wrapping_add(1);
            }
            for (row, revision) in self.row_revisions.iter_mut().enumerate() {
                if self.all_dirty || self.dirty_rows.contains(&(row as u16)) {
                    *revision = self.next_revision;
                    self.next_revision = self.next_revision.wrapping_add(1);
                }
            }
            self.all_dirty = false;
            self.dirty_rows.clear();
            let paint = self.cache.prepaint(
                TerminalRenderInput {
                    viewport,
                    row_revisions: &self.row_revisions,
                    revision_epoch: self.revision_epoch,
                    history: None,
                    images: if self.surface == TerminalSurface::Pane {
                        connection
                            .terminal_images(self.pane)
                            .map(|images| images as &dyn zz_ui::terminal::TerminalImageSource)
                    } else {
                        None
                    },
                    local_scroll_target: None,
                    command_output: self.surface == TerminalSurface::CommandOutput,
                    appearance: &appearance,
                    appearance_hash: appearance.stable_hash(),
                    text_opacity: 1.0,
                    focused: self.focused,
                    cursor_blink_visible: self.cursor_blink_visible,
                    marked_text: self
                        .search
                        .is_none()
                        .then_some(self.marked_text.as_deref())
                        .flatten(),
                },
                bounds,
                window,
                cx,
            );
            Some((paint, connection.core.attached_session().is_some()))
        })?;
        let geometry = paint.geometry;
        self.bounds = geometry.grid_bounds;
        self.surface_bounds = geometry.surface_bounds;
        self.cell_width = geometry.cell_width;
        self.line_height = geometry.line_height;
        self.scale = window.scale_factor();
        let input_bounds = paint.geometry.input_bounds.unwrap_or(geometry.grid_bounds);
        if self.search.is_none() && self.cursor_bounds != input_bounds {
            self.cursor_bounds = input_bounds;
            window.invalidate_character_coordinates();
        }
        let measured = geometry.grid;
        if self.geometry != Some(measured) && attached {
            self.geometry = Some(measured);
            match self.surface {
                TerminalSurface::Pane => self.send(
                    InputMessage::ResizeTerminal {
                        pane: self.pane,
                        columns: measured.columns,
                        rows: measured.rows,
                        cell_width_px: measured.cell_width_px,
                        cell_height_px: measured.cell_height_px,
                    },
                    cx,
                ),
                TerminalSurface::CommandOutput => self.send(
                    InputMessage::ResizeCommandOutput {
                        columns: measured.columns,
                        rows: measured.rows,
                        cell_width_px: measured.cell_width_px,
                        cell_height_px: measured.cell_height_px,
                    },
                    cx,
                ),
                TerminalSurface::Popup => {}
            }
        }
        Some(paint)
    }

    fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        paint: &mut Option<PaintState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, cx.entity()),
            cx,
        );
        if let Some(paint) = paint {
            self.cache.paint(paint, bounds, window, cx);
        }
    }

    fn sync_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let focused = self.focus.is_focused(window) && window.is_window_active();
        if self.focused != focused {
            self.focused = focused;
            self.view(TerminalViewAction::Focus(focused), cx);
            if !focused {
                self.end_drag();
                self.pressed_buttons.clear();
                self.forwarded.clear();
                self.swallowed_search_key = None;
                self.view(TerminalViewAction::ClearLinkHover, cx);
            }
            self.reset_cursor_blink(cx);
            cx.notify();
        }
        self.ensure_cursor_blink(cx);
    }

    fn should_blink(&self, cx: &App) -> bool {
        let core = &self.connection.read(cx).core;
        self.viewport(core).is_some_and(|viewport| {
            cursor_should_blink(
                viewport.cursor,
                core.appearance()
                    .map_or(zz_terminal::CursorBlinkPolicy::Terminal, |appearance| {
                        appearance.cursor_blink_policy
                    }),
                self.focused,
            )
        })
    }

    fn reset_cursor_blink(&mut self, cx: &mut Context<Self>) {
        let hidden = !self.cursor_blink_visible;
        self.cursor_blink_visible = true;
        self.cursor_blink_task = None;
        self.ensure_cursor_blink(cx);
        if hidden {
            cx.notify();
        }
    }

    fn ensure_cursor_blink(&mut self, cx: &mut Context<Self>) {
        if !self.should_blink(cx) {
            self.cursor_blink_visible = true;
            self.cursor_blink_task = None;
            return;
        }
        if self.cursor_blink_task.is_some() {
            return;
        }
        let interval = Duration::from_millis(u64::from(
            self.connection
                .read(cx)
                .core
                .appearance()
                .map_or(500, |appearance| appearance.cursor_blink_interval_ms),
        ));
        self.cursor_blink_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let Ok(keep_running) = this.update(cx, |view, cx| {
                    if !view.should_blink(cx) {
                        view.cursor_blink_visible = true;
                        return false;
                    }
                    view.cursor_blink_visible = !view.cursor_blink_visible;
                    cx.notify();
                    true
                }) else {
                    break;
                };
                if !keep_running {
                    break;
                }
            }
        }));
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_subscriptions.is_empty() {
            self.focus_subscriptions.push(cx.on_focus_in(
                &self.focus,
                window,
                |view, window, cx| {
                    view.sync_focus(window, cx);
                },
            ));
            self.focus_subscriptions.push(cx.on_focus_out(
                &self.focus,
                window,
                |view, _, window, cx| {
                    view.sync_focus(window, cx);
                },
            ));
            self.focus_subscriptions.push(cx.observe_window_activation(
                window,
                |view, window, cx| {
                    view.sync_focus(window, cx);
                },
            ));
        }
        self.sync_focus(window, cx);
        let appearance = self
            .connection
            .read(cx)
            .core
            .appearance()
            .cloned()
            .unwrap_or_default();
        let font = terminal_font_for_style(&appearance, &cx.theme().mono_font_family, false, false);
        let font_size = px((appearance.font_size_points + self.font_delta).clamp(7., 48.));
        let prepare = cx.entity();
        let paint = cx.entity();
        let mut background = cx.theme().background;
        let mut mode = None;
        let mut hovered_uri = None;
        let mut search_status = None;
        let mut bottom_right: Vec<AnyElement> = Vec::new();
        if let Some(viewport) = self.viewport(&self.connection.read(cx).core) {
            background = zz_ui::terminal::terminal_background(
                viewport.background,
                appearance.background_opacity,
            );
            hovered_uri.clone_from(&viewport.presentation.hovered_uri);
            search_status = viewport.search;
            if let Some(uri) = &hovered_uri {
                bottom_right.push(terminal_link_popup(uri.to_string(), cx).into_any_element());
            }
            if self.surface != TerminalSurface::Popup {
                mode = terminal_mode_text(viewport.mode, viewport.unseen_output);
                if let Some(status) = terminal_status_text(&viewport.status) {
                    bottom_right.push(terminal_status_popup(status, cx).into_any_element());
                }
            }
        }
        if let Some(query) = &self.search {
            let prompt = search_prompt_text(
                query,
                self.marked_text.as_deref().unwrap_or_default(),
                search_status,
            );
            let caret = "Find: ".len() + query.text.len();
            let view = cx.entity();
            bottom_right.push(
                terminal_search_prompt(
                    prompt,
                    caret,
                    move |bounds, window, cx| {
                        view.update(cx, |view, _| {
                            if view.cursor_bounds != bounds {
                                view.cursor_bounds = bounds;
                                window.invalidate_character_coordinates();
                            }
                        });
                    },
                    cx,
                )
                .into_any_element(),
            );
        }
        let mut root = div()
            .id(("terminal", self.pane.0))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(if self.surface == TerminalSurface::Pane {
                cx.theme()
                    .background
                    .opaque()
                    .blend(background)
                    .opacity(cx.theme().pane_background_opacity)
            } else {
                background
            })
            .font(font)
            .text_size(font_size)
            .when(self.surface != TerminalSurface::Popup, |root| {
                root.pl(px(appearance.padding_left))
                    .pr(px(appearance.padding_right))
                    .pt(px(appearance.padding_top))
                    .pb(px(appearance.padding_bottom))
            })
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_exit(cx.listener(Self::on_mouse_exit))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        prepare.update(cx, |view, cx| view.prepare(bounds, window, cx))
                    },
                    move |bounds, mut state, window, cx| {
                        paint.update(cx, |view, cx| view.paint(bounds, &mut state, window, cx));
                    },
                )
                .size_full(),
            );
        if hovered_uri.is_some() {
            root = root.cursor_pointer();
        }
        if let Some((label, detail)) = mode {
            root = root.child(
                terminal_mode_indicator(label, detail)
                    .absolute()
                    .right(px(8.0))
                    .top(px(8.0)),
            );
        }
        if !bottom_right.is_empty() {
            root = root.child(pane_overlay_stack(
                PaneOverlayCorner::BottomRight,
                bottom_right,
            ));
        }
        root
    }
}

impl EntityInputHandler for TerminalPane {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.marked_text.clone()?;
        *adjusted = Some(0..text.encode_utf16().count());
        Some(text)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_cursor_blink(cx);
        let composed = self.marked_text.take().is_some();
        if !text.is_empty() {
            if let Some(query) = self.search.as_mut() {
                query.text.push_str(text);
                let query = query.clone();
                self.view(TerminalViewAction::SearchUpdate(query), cx);
            } else if !composed && (text.contains(['\n', '\r']) || text.chars().count() > 1) {
                self.view(TerminalViewAction::Paste(text.into()), cx);
            } else {
                self.send(
                    InputMessage::Text {
                        pane: self.pane,
                        text: text.into(),
                    },
                    cx,
                );
            }
        }
        window.invalidate_character_coordinates();
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reset_cursor_blink(cx);
        self.marked_text = (!text.is_empty()).then(|| text.into());
        window.invalidate_character_coordinates();
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.cursor_bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

fn search_key_action(query: &mut SearchQuery, input: &KeyInput) -> Option<TerminalViewAction> {
    match input.key {
        KeyCode::Enter => Some(
            if (query.direction == SearchDirection::Backward) ^ input.modifiers.shift() {
                TerminalViewAction::SearchPrevious
            } else {
                TerminalViewAction::SearchNext
            },
        ),
        KeyCode::Backspace => {
            query.text.pop();
            Some(TerminalViewAction::SearchUpdate(query.clone()))
        }
        KeyCode::Character('r' | 'R') if input.modifiers.alt() => {
            query.mode = match query.mode {
                SearchMode::Literal => SearchMode::Regex,
                SearchMode::Regex => SearchMode::Literal,
            };
            Some(TerminalViewAction::SearchUpdate(query.clone()))
        }
        KeyCode::Character('c' | 'C') if input.modifiers.alt() => {
            query.case = match query.case {
                SearchCase::Smart => SearchCase::Sensitive,
                SearchCase::Sensitive => SearchCase::Insensitive,
                SearchCase::Insensitive => SearchCase::Smart,
            };
            Some(TerminalViewAction::SearchUpdate(query.clone()))
        }
        _ => None,
    }
}

fn search_prompt_text(query: &SearchQuery, marked: &str, status: Option<SearchStatus>) -> String {
    let result = status.map_or_else(String::new, |status| {
        if status.invalid_pattern() {
            "  invalid pattern".to_owned()
        } else if status.pending() {
            "  searching…".to_owned()
        } else if status.total == 0 {
            "  0/0".to_owned()
        } else {
            format!("  {}/{}", status.current(), status.total)
        }
    });
    let mode = match query.mode {
        SearchMode::Literal => "literal",
        SearchMode::Regex => "regex",
    };
    let case = match query.case {
        SearchCase::Smart => "smart-case",
        SearchCase::Sensitive => "case-sensitive",
        SearchCase::Insensitive => "case-insensitive",
    };
    let direction = match query.direction {
        SearchDirection::Forward => "forward",
        SearchDirection::Backward => "backward",
    };
    format!(
        "Find: {}{marked}{result}  [{direction}, {mode}, {case}]  Alt+R / Alt+C",
        query.text
    )
}

fn terminal_mode_text(
    mode: TerminalMode,
    unseen_output: u32,
) -> Option<(Option<&'static str>, String)> {
    match mode {
        TerminalMode::Live if unseen_output > 0 => Some((None, format!("+{unseen_output} output"))),
        TerminalMode::Live => None,
        TerminalMode::Copy {
            position,
            total,
            hide_position,
        } => Some((
            Some("COPY MODE"),
            match (hide_position, unseen_output) {
                (true, 0) => String::new(),
                (true, unseen) => format!("+{unseen} output"),
                (false, 0) => format!("{position}/{total}"),
                (false, unseen) => format!("{position}/{total}  ·  +{unseen} output"),
            },
        )),
        TerminalMode::View { position, total } => {
            Some((Some("VIEW MODE"), format!("{position}/{total}  ·  q close")))
        }
    }
}

fn terminal_status_text(status: &SessionStatus) -> Option<String> {
    match status {
        SessionStatus::Starting | SessionStatus::Running => None,
        SessionStatus::Exited(exit) => Some(exit.signal.as_ref().map_or_else(
            || format!("shell exited ({})", exit.code),
            |signal| format!("shell exited: {signal}"),
        )),
        SessionStatus::Failed(error) => Some(error.as_ref().clone()),
    }
}

pub fn key_input(event: &KeyDownEvent) -> KeyInput {
    keystroke_input(
        &event.keystroke,
        if event.is_held {
            KeyAction::Repeat
        } else {
            KeyAction::Press
        },
    )
}

fn keystroke_input(keystroke: &Keystroke, action: KeyAction) -> KeyInput {
    let key = key_code(&keystroke.key);
    let character = if let KeyCode::Character(value) = key {
        Some(value)
    } else {
        None
    };
    KeyInput {
        action,
        key,
        modifiers: wire_modifiers(keystroke.modifiers),
        text: keystroke
            .key_char
            .clone()
            .or_else(|| character.map(|value| value.to_string()))
            .filter(|text| !text.chars().any(char::is_control))
            .map(String::into_boxed_str),
        unshifted_codepoint: character,
    }
}

fn key_code(key: &str) -> KeyCode {
    match key {
        "space" => KeyCode::Character(' '),
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "escape" => KeyCode::Escape,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        value if value.chars().count() == 1 => KeyCode::Character(value.chars().next().unwrap()),
        value => value
            .strip_prefix('f')
            .and_then(|number| number.parse().ok())
            .map_or(KeyCode::Unidentified, KeyCode::Function),
    }
}

fn wire_modifiers(modifiers: gpui::Modifiers) -> zz_terminal::Modifiers {
    zz_terminal::Modifiers::new(
        modifiers.shift,
        modifiers.control,
        modifiers.alt,
        modifiers.platform,
    )
}

fn mouse_button(button: MouseButton) -> Option<TerminalMouseButton> {
    match button {
        MouseButton::Left => Some(TerminalMouseButton::Left),
        MouseButton::Middle => Some(TerminalMouseButton::Middle),
        MouseButton::Right => Some(TerminalMouseButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

fn accumulate_scroll(remainder: &mut f32, delta: f32) -> i32 {
    *remainder += delta;
    let lines = *remainder as i32;
    *remainder -= lines as f32;
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_search_keys_preserve_direction_and_cycle_regex_and_case() {
        let mut query = SearchQuery {
            text: "café".into(),
            direction: SearchDirection::Backward,
            ..SearchQuery::default()
        };
        let key = |value, shift, alt| {
            keystroke_input(
                &Keystroke {
                    key: value,
                    key_char: None,
                    modifiers: gpui::Modifiers {
                        shift,
                        alt,
                        ..Default::default()
                    },
                },
                KeyAction::Press,
            )
        };
        assert_eq!(
            search_key_action(&mut query, &key("enter".into(), false, false)),
            Some(TerminalViewAction::SearchPrevious)
        );
        assert_eq!(
            search_key_action(&mut query, &key("enter".into(), true, false)),
            Some(TerminalViewAction::SearchNext)
        );
        let _ = search_key_action(&mut query, &key("r".into(), false, true));
        assert_eq!(query.mode, SearchMode::Regex);
        let _ = search_key_action(&mut query, &key("r".into(), false, true));
        assert_eq!(query.mode, SearchMode::Literal);
        for expected in [
            SearchCase::Sensitive,
            SearchCase::Insensitive,
            SearchCase::Smart,
        ] {
            let _ = search_key_action(&mut query, &key("c".into(), false, true));
            assert_eq!(query.case, expected);
        }
        let _ = search_key_action(&mut query, &key("backspace".into(), false, false));
        assert_eq!(query.text, "caf");
        assert_eq!(
            search_key_action(&mut query, &key("x".into(), false, false)),
            None
        );
    }

    #[test]
    fn terminal_search_feedback_includes_composition_and_server_results() {
        let query = SearchQuery::literal("café");
        let text = search_prompt_text(&query, "編集中", Some(SearchStatus::new(2, 4)));
        assert_eq!(
            text,
            "Find: café編集中  2/4  [forward, literal, smart-case]  Alt+R / Alt+C"
        );
        assert!(search_prompt_text(&query, "", Some(SearchStatus::new(0, 0))).contains("  0/0"));
        assert!(
            search_prompt_text(&query, "", Some(SearchStatus::default().with_pending(true)))
                .contains("searching…")
        );
        assert!(
            search_prompt_text(
                &query,
                "",
                Some(SearchStatus::default().with_invalid_pattern(true))
            )
            .contains("invalid pattern")
        );
    }

    #[test]
    fn terminal_mode_feedback_honors_hidden_copy_position() {
        assert_eq!(terminal_mode_text(TerminalMode::Live, 0), None);
        assert_eq!(
            terminal_mode_text(
                TerminalMode::Copy {
                    position: 12,
                    total: 20,
                    hide_position: true,
                },
                3,
            ),
            Some((Some("COPY MODE"), "+3 output".into()))
        );
        assert_eq!(
            terminal_mode_text(
                TerminalMode::View {
                    position: 12,
                    total: 20,
                },
                0,
            ),
            Some((Some("VIEW MODE"), "12/20  ·  q close".into()))
        );
    }

    #[test]
    fn trackpad_deltas_accumulate_without_losing_direction_changes() {
        let mut remainder = 0.;
        assert_eq!(accumulate_scroll(&mut remainder, -0.25), 0);
        assert_eq!(accumulate_scroll(&mut remainder, -0.25), 0);
        assert_eq!(accumulate_scroll(&mut remainder, -0.75), -1);
        assert_eq!(accumulate_scroll(&mut remainder, 0.5), 0);
        assert_eq!(accumulate_scroll(&mut remainder, 0.75), 1);
        assert_eq!(remainder, 0.);
    }

    #[test]
    fn shifted_punctuation_and_releases_preserve_the_shared_key_contract() {
        let keystroke = Keystroke {
            key: "/".into(),
            key_char: Some("?".into()),
            modifiers: gpui::Modifiers {
                shift: true,
                ..Default::default()
            },
        };
        let input = keystroke_input(&keystroke, KeyAction::Repeat);
        assert_eq!(input.key, KeyCode::Character('/'));
        assert_eq!(input.text.as_deref(), Some("?"));
        assert_eq!(input.unshifted_codepoint, Some('/'));
        assert!(input.modifiers.shift());
        assert_eq!(input.action, KeyAction::Repeat);
        let released = keystroke_input(&keystroke, KeyAction::Release);
        assert_eq!(released.action, KeyAction::Release);
        assert_eq!(released.key, input.key);
    }
}
