use std::{collections::HashSet, ops::Range};

use gpui::{
    App, Bounds, ContentMask, Context, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, FontStyle, FontWeight, Hsla, KeyDownEvent, KeyUpEvent, Keystroke,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, Rgba,
    ScrollWheelEvent, ShapedLine, StrikethroughStyle, Subscription, TextAlign, TextRun,
    UTF16Selection, Window, canvas, div, fill, font, point, prelude::*, px, size,
};
use zz_client::{
    ChromeAction, ChromeKeymap, ChromeProfile, CoreEvent, TERMINAL_TABLE, ViewportDamage,
};
use zz_protocol::{InputMessage, PaneId, PopupAction};
use zz_terminal::{
    CellWidth, Color, CursorStyle, KeyAction, KeyCode, KeyInput, PackedStyle, PointerCellEvent,
    TerminalMouseButton, TerminalMouseInput, TerminalMousePhase, TerminalViewAction,
    TerminalViewport,
};
use zz_ui::{
    ActiveTheme, Sizable,
    button::Button,
    input::{Input, InputEvent, InputState},
};

use crate::connection::Connection;

struct RowPaint {
    backgrounds: Vec<(u16, u16, Hsla)>,
    text: Vec<(u16, ShapedLine)>,
}

pub struct TerminalPane {
    pane: PaneId,
    connection: Entity<Connection>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    cursor_bounds: Bounds<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    font_size: Pixels,
    font_delta: f32,
    scroll_rows: f32,
    geometry: (u16, u16),
    cache: Vec<Option<RowPaint>>,
    all_dirty: bool,
    dirty_rows: HashSet<u16>,
    forwarded: HashSet<String>,
    marked_text: Option<String>,
    surface: TerminalSurface,
    chrome: ChromeKeymap,
    apple_chrome: ChromeKeymap,
    search: Option<Entity<InputState>>,
    search_subscription: Option<Subscription>,
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
        let subscription =
            cx.subscribe(
                &connection,
                move |this, _, event: &CoreEvent, cx| match event {
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
                    CoreEvent::Attached { .. } | CoreEvent::AppearanceChanged => {
                        this.all_dirty = true;
                        this.geometry = (0, 0);
                        cx.notify();
                    }
                    CoreEvent::CommandOutputChanged
                        if this.surface == TerminalSurface::CommandOutput =>
                    {
                        this.all_dirty = true;
                        cx.notify();
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
            cell_width: px(8.),
            line_height: px(18.),
            font_size: px(13.),
            font_delta: 0.,
            scroll_rows: 0.,
            geometry: (0, 0),
            cache: Vec::new(),
            all_dirty: true,
            dirty_rows: HashSet::new(),
            forwarded: HashSet::new(),
            marked_text: None,
            surface: TerminalSurface::Pane,
            chrome: ChromeKeymap::for_profile(ChromeProfile::Desktop),
            apple_chrome: ChromeKeymap::for_profile(ChromeProfile::DesktopApple),
            search: None,
            search_subscription: None,
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
        let input = key_input(event);
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
        if self.search.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in terminal"));
            self.search_subscription =
                Some(cx.subscribe(&input, |this, input, event, cx| match event {
                    InputEvent::Change => this.view(
                        TerminalViewAction::SearchUpdate(zz_terminal::SearchQuery::literal(
                            input.read(cx).value().to_string(),
                        )),
                        cx,
                    ),
                    InputEvent::PressEnter { shift } => this.view(
                        if *shift {
                            TerminalViewAction::SearchPrevious
                        } else {
                            TerminalViewAction::SearchNext
                        },
                        cx,
                    ),
                    _ => {}
                }));
            self.search = Some(input);
            self.view(
                TerminalViewAction::SearchBegin(zz_terminal::SearchQuery::literal("")),
                cx,
            );
        }
        if let Some(input) = &self.search {
            input.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.search_subscription = None;
        self.view(TerminalViewAction::SearchClose, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
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
        let x = f32::from(position.x - self.bounds.origin.x).max(0.);
        let y = f32::from(position.y - self.bounds.origin.y).max(0.);
        let column = (x / f32::from(self.cell_width)) as u16;
        let row = (y / f32::from(self.line_height)) as u16;
        TerminalMouseInput::new(
            phase,
            button,
            PointerCellEvent {
                column: column.min(self.geometry.0.saturating_sub(1)),
                row: row.min(self.geometry.1.saturating_sub(1)),
                click_count: count.min(3) as u8,
                rectangle: modifiers.alt,
            },
            x as u32,
            y as u32,
            f32::from(self.bounds.size.width) as u32,
            f32::from(self.bounds.size.height) as u32,
            f32::from(self.cell_width).ceil() as u32,
            f32::from(self.line_height).ceil() as u32,
            wire_modifiers(modifiers),
            modifiers.shift,
        )
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if self.surface == TerminalSurface::Pane {
            self.connection.update(cx, |connection, cx| {
                connection.command("select-pane", vec!["-t".into(), self.pane.to_string()], cx);
            });
        }
        self.view(TerminalViewAction::Focus(true), cx);
        self.view(
            TerminalViewAction::Mouse(self.mouse(
                event.position,
                event.modifiers,
                TerminalMousePhase::Press,
                mouse_button(event.button),
                event.click_count,
            )),
            cx,
        );
        cx.stop_propagation();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.view(
            TerminalViewAction::Mouse(self.mouse(
                event.position,
                event.modifiers,
                TerminalMousePhase::Release,
                mouse_button(event.button),
                1,
            )),
            cx,
        );
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.view(
            TerminalViewAction::Mouse(self.mouse(
                event.position,
                event.modifiers,
                TerminalMousePhase::Motion,
                event.pressed_button.and_then(mouse_button),
                1,
            )),
            cx,
        );
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

    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds = bounds;
        let connection = self.connection.read(cx);
        let font_size = px((connection
            .core
            .appearance()
            .map_or(13., |appearance| appearance.font_size_points)
            + self.font_delta)
            .clamp(7., 48.));
        if font_size != self.font_size || self.all_dirty {
            self.font_size = font_size;
            let probe = window.text_system().shape_line(
                "m".into(),
                font_size,
                &[text_run(1, None, cx.theme().foreground, cx)],
                None,
            );
            self.cell_width = probe.width.max(px(1.));
            self.line_height = (probe.ascent + probe.descent + px(2.)).max(font_size * 1.2);
        }
        let geometry = (
            (f32::from(bounds.size.width) / f32::from(self.cell_width))
                .floor()
                .clamp(1., 500.) as u16,
            (f32::from(bounds.size.height) / f32::from(self.line_height))
                .floor()
                .clamp(1., 300.) as u16,
        );
        if let Some(viewport) = self.viewport(&connection.core) {
            if self.cache.len() != usize::from(viewport.rows) {
                self.cache.resize_with(usize::from(viewport.rows), || None);
                self.all_dirty = true;
            }
            for row in 0..viewport.rows {
                if self.all_dirty
                    || self.dirty_rows.contains(&row)
                    || self.cache[usize::from(row)].is_none()
                {
                    self.cache[usize::from(row)] = Some(shape_row(
                        viewport,
                        row,
                        self.font_size,
                        self.cell_width,
                        window,
                        cx,
                    ));
                }
            }
            if let Some(cursor) = viewport.cursor {
                self.cursor_bounds = Bounds::new(
                    bounds.origin
                        + point(
                            self.cell_width * f32::from(cursor.column()),
                            self.line_height * f32::from(cursor.row()),
                        ),
                    size(self.cell_width, self.line_height),
                );
            }
            self.all_dirty = false;
            self.dirty_rows.clear();
        }
        if self.geometry != geometry && connection.core.attached_session().is_some() {
            self.geometry = geometry;
            let cell_width_px = f32::from(self.cell_width).ceil() as u32;
            let cell_height_px = f32::from(self.line_height).ceil() as u32;
            match self.surface {
                TerminalSurface::Pane => self.send(
                    InputMessage::ResizeTerminal {
                        pane: self.pane,
                        columns: geometry.0,
                        rows: geometry.1,
                        cell_width_px,
                        cell_height_px,
                    },
                    cx,
                ),
                TerminalSurface::CommandOutput => self.send(
                    InputMessage::ResizeCommandOutput {
                        columns: geometry.0,
                        rows: geometry.1,
                        cell_width_px,
                        cell_height_px,
                    },
                    cx,
                ),
                TerminalSurface::Popup => {}
            }
        }
    }

    fn paint(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(self.bounds, cx.entity()),
            cx,
        );
        let (background, foreground, cursor, overlays, overlay_colors) = {
            let core = &self.connection.read(cx).core;
            let Some(viewport) = self.viewport(core) else {
                return;
            };
            let colors = core.appearance().map_or(
                [zz_terminal::AppearanceColor::default(); 5],
                |appearance| {
                    [
                        appearance.selection_background,
                        appearance.search_match_color,
                        appearance.search_current_color,
                        zz_terminal::AppearanceColor::opaque(appearance.link_color),
                        appearance.copy_cursor_color,
                    ]
                },
            );
            (
                viewport.background,
                viewport.foreground,
                viewport.cursor,
                std::sync::Arc::clone(&viewport.overlays),
                colors,
            )
        };
        window.with_content_mask(
            Some(ContentMask {
                bounds: self.bounds,
            }),
            |window| {
                window.paint_quad(fill(self.bounds, terminal_color(background)));
                for (row, paint) in self.cache.iter().enumerate() {
                    let Some(paint) = paint else {
                        continue;
                    };
                    for &(column, count, color) in &paint.backgrounds {
                        window.paint_quad(fill(
                            Bounds::new(
                                self.bounds.origin
                                    + point(
                                        self.cell_width * f32::from(column),
                                        self.line_height * row as f32,
                                    ),
                                size(self.cell_width * f32::from(count), self.line_height),
                            ),
                            color,
                        ));
                    }
                }
                for overlay in overlays.iter() {
                    let color = overlay_colors[overlay.kind() as usize];
                    let color = terminal_color(color.rgb()).opacity(f32::from(color.a) / 255.);
                    window.paint_quad(fill(
                        Bounds::new(
                            self.bounds.origin
                                + point(
                                    self.cell_width * f32::from(overlay.start),
                                    self.line_height * f32::from(overlay.row),
                                ),
                            size(
                                self.cell_width
                                    * f32::from(overlay.end.saturating_sub(overlay.start)),
                                self.line_height,
                            ),
                        ),
                        color,
                    ));
                }
                for (row, paint) in self.cache.iter().enumerate() {
                    if let Some(paint) = paint {
                        for (column, line) in &paint.text {
                            let _ = line.paint(
                                self.bounds.origin
                                    + point(
                                        self.cell_width * f32::from(*column),
                                        self.line_height * row as f32,
                                    ),
                                self.line_height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                    }
                }
                if let Some(cursor) = cursor.filter(|cursor| cursor.visible()) {
                    let mut bounds = self.cursor_bounds;
                    let mut color = terminal_color(cursor.color());
                    match cursor.style() {
                        CursorStyle::Bar => bounds.size.width = px(2.),
                        CursorStyle::Underline => {
                            bounds.origin.y += self.line_height - px(2.);
                            bounds.size.height = px(2.);
                        }
                        CursorStyle::Block | CursorStyle::BlockHollow => {
                            color = color.opacity(0.45);
                        }
                    }
                    window.paint_quad(fill(bounds, color));
                }
                if let Some(marked) = &self.marked_text {
                    let line = window.text_system().shape_line(
                        marked.clone().into(),
                        self.font_size,
                        &[text_run(marked.len(), None, terminal_color(foreground), cx)],
                        None,
                    );
                    window.paint_quad(fill(
                        Bounds::new(
                            self.cursor_bounds.origin,
                            size(line.width, self.line_height),
                        ),
                        terminal_color(background),
                    ));
                    let _ = line.paint(
                        self.cursor_bounds.origin,
                        self.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            },
        );
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prepare = cx.entity();
        let paint = cx.entity();
        let search = self.search.as_ref().map(|input| {
            div()
                .absolute()
                .right(px(8.))
                .top(px(8.))
                .w(px(320.))
                .flex()
                .gap(px(4.))
                .child(Input::new(input).small())
                .child(
                    Button::compact_icon("terminal-search-close", zz_ui::IconName::Xmark)
                        .tooltip("Close search")
                        .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx))),
                )
        });
        div()
            .id(("terminal", self.pane.0))
            .relative()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.search.is_some() && event.keystroke.key == "escape" {
                    this.close_search(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        prepare.update(cx, |view, cx| view.prepare(bounds, window, cx));
                    },
                    move |_, (), window, cx| paint.update(cx, |view, cx| view.paint(window, cx)),
                )
                .size_full(),
            )
            .children(search)
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
        let composed = self.marked_text.take().is_some();
        if !text.is_empty() {
            if !composed && (text.contains(['\n', '\r']) || text.chars().count() > 1) {
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

fn shape_row(
    viewport: &TerminalViewport,
    row: u16,
    font_size: Pixels,
    cell_width: Pixels,
    window: &mut Window,
    cx: &App,
) -> RowPaint {
    let mut output = RowPaint {
        backgrounds: Vec::new(),
        text: Vec::new(),
    };
    let mut column = 0;
    while column < viewport.columns {
        let Some(cell) = viewport.cell(row, column) else {
            break;
        };
        let style = viewport.style(cell);
        let background = style.map_or(viewport.background, PackedStyle::background);
        if background != viewport.background {
            if let Some((start, count, color)) = output.backgrounds.last_mut()
                && *start + *count == column
                && *color == terminal_color(background)
            {
                *count += 1;
            } else {
                output
                    .backgrounds
                    .push((column, 1, terminal_color(background)));
            }
        }
        if matches!(cell.width(), CellWidth::SpacerHead | CellWidth::SpacerTail)
            || style.is_some_and(PackedStyle::invisible)
        {
            column += 1;
            continue;
        }
        let start = column;
        let wide = cell.width() == CellWidth::Wide;
        let mut text = viewport.cell_text(cell).into_owned();
        if text.is_empty() {
            text.push(' ');
        }
        column += 1;
        if !wide && text.len() == 1 {
            while column < viewport.columns {
                let Some(next) = viewport.cell(row, column) else {
                    break;
                };
                if next.style_id() != cell.style_id()
                    || next.width() != CellWidth::Narrow
                    || next.glyph() > 127
                {
                    break;
                }
                text.push(
                    char::from_u32(next.glyph())
                        .filter(|value| *value != '\0')
                        .unwrap_or(' '),
                );
                if background != viewport.background
                    && let Some((_, count, _)) = output.backgrounds.last_mut()
                {
                    *count += 1;
                }
                column += 1;
            }
        }
        if text.trim().is_empty() {
            continue;
        }
        let color = terminal_color(style.map_or(viewport.foreground, PackedStyle::foreground));
        let run = text_run(text.len(), style, color, cx);
        let scalar = text.chars().count() == 1;
        let line = window.text_system().shape_line(
            text.into(),
            font_size,
            &[run],
            if wide || !scalar && column - start == 1 {
                None
            } else {
                Some(cell_width)
            },
        );
        output.text.push((start, line));
    }
    output
}

fn text_run(len: usize, style: Option<PackedStyle>, color: Hsla, cx: &App) -> TextRun {
    let mut font = font(cx.theme().mono_font_family.clone());
    if style.is_some_and(PackedStyle::bold) {
        font.weight = FontWeight::BOLD;
    }
    if style.is_some_and(PackedStyle::italic) {
        font.style = FontStyle::Italic;
    }
    TextRun {
        len,
        font,
        color: if style.is_some_and(PackedStyle::faint) {
            color.opacity(0.6)
        } else {
            color
        },
        background_color: None,
        underline: style
            .filter(|style| style.underline() != zz_terminal::UnderlineStyle::None)
            .map(|style| gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(style.underline_color().map_or(color, terminal_color)),
                wavy: style.underline() == zz_terminal::UnderlineStyle::Curly,
            }),
        strikethrough: style
            .filter(|style| style.strikethrough())
            .map(|_| StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
    }
}

fn terminal_color(color: Color) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: 1.,
    }
    .into()
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
