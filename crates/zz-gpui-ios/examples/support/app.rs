use crate::{
    connection::{Connection, Event, Prompt},
    input::{key_input, wire_modifiers},
};
use gpui::{
    Bounds, ClipboardItem, Context, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, Pixels, Point, Render,
    Subscription, Task, UTF16Selection, Window, canvas, div, prelude::*, px,
};
use std::{collections::HashSet, ops::Range, path::PathBuf, sync::Arc, time::Duration};
use zz_client::{
    ChromeAction, ChromeKeymap, ChromeProfile, ClientCore, CoreEvent, Outbound, TERMINAL_TABLE,
};
use zz_daemon::{AskpassPromptKind, AskpassReply, InteractiveClient};
use zz_protocol::{InputMessage, PaneId, ProtocolMessage};
use zz_terminal::{
    ClipboardTarget, KeyAction, KeyCode, PointerCellEvent, TerminalMouseButton, TerminalMouseInput,
    TerminalMousePhase, TerminalViewAction,
};
use zz_ui::{
    ActiveTheme, Colorize,
    button::Button,
    input::{Input, InputContentType, InputEvent, InputState},
    terminal::{GridSize, PaintState, RowRenderCache, TerminalGeometry, TerminalRenderInput},
};

pub struct TerminalApp {
    pub focus: FocusHandle,
    endpoint: Entity<InputState>,
    answer: Entity<InputState>,
    prompt: Option<Prompt>,
    connection: Option<Connection>,
    client: Option<Arc<InteractiveClient>>,
    core: ClientCore,
    pane: Option<PaneId>,
    status: String,
    cache: RowRenderCache,
    images: zz_ui::terminal_images::TerminalImages,
    revision: u64,
    grid: Option<GridSize>,
    geometry: Option<TerminalGeometry>,
    forwarded: HashSet<String>,
    chrome: ChromeKeymap,
    font_delta: f32,
    select: bool,
    drag: Option<Point<Pixels>>,
    mouse_down: bool,
    scroll: f32,
    focused: bool,
    show_endpoint: bool,
    _updates: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl TerminalApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = std::env::var("ZZ_GPUI_ENDPOINT")
            .or_else(|_| std::env::var("ZZ_SOCKET"))
            .ok()
            .or_else(|| std::fs::read_to_string(endpoint_path()).ok())
            .unwrap_or_default();
        let endpoint = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("ssh://user@host or /path/to/socket")
                .default_value(saved.clone())
        });
        let answer = cx.new(|cx| InputState::new(window, cx));
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.subscribe_in(&endpoint, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.connect(window, cx);
                }
            }),
            cx.subscribe_in(&answer, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.answer_prompt(None, window, cx);
                }
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                this.send(
                    InputMessage::ClientFocus {
                        focused: window.is_window_active(),
                    },
                    cx,
                );
                this.sync_focus(window, cx);
            }),
            cx.on_focus(&focus, window, |this, window, cx| {
                this.sync_focus(window, cx)
            }),
            cx.on_blur(&focus, window, |this, window, cx| {
                this.sync_focus(window, cx)
            }),
        ];
        let updates = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut app = Self {
            focus,
            endpoint,
            answer,
            prompt: None,
            connection: None,
            client: None,
            core: ClientCore::new(),
            pane: None,
            status: "Enter a host to connect".into(),
            cache: RowRenderCache::default(),
            images: Default::default(),
            revision: 1,
            grid: None,
            geometry: None,
            forwarded: HashSet::new(),
            chrome: ChromeKeymap::for_profile(ChromeProfile::DesktopApple),
            font_delta: 0.,
            select: false,
            drag: None,
            mouse_down: false,
            scroll: 0.,
            focused: false,
            show_endpoint: saved.is_empty(),
            _updates: updates,
            _subscriptions: subscriptions,
        };
        if !saved.is_empty() {
            app.connect(window, cx);
        }
        app
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let endpoint = self.endpoint.read(cx).value().to_string();
        if endpoint.trim().is_empty() {
            return;
        }
        self.connection = None;
        self.client = None;
        self.prompt = None;
        self.core = ClientCore::new();
        self.pane = None;
        self.grid = None;
        self.geometry = None;
        self.forwarded.clear();
        self.cache = RowRenderCache::default();
        self.connection = Some(Connection::connect(
            endpoint.trim().into(),
            Some(std::env::var("ZZ_GPUI_SESSION").unwrap_or_default()),
            false,
        ));
        self.status = "Connecting…".into();
        self.show_endpoint = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for _ in 0..128 {
            let event = self
                .connection
                .as_ref()
                .and_then(|connection| connection.events.try_recv().ok());
            let Some(event) = event else {
                break;
            };
            match event {
                Event::Connected(client) => {
                    self.core.handle_message(ProtocolMessage::ServerHello(
                        client.server_hello().clone(),
                    ));
                    self.client = Some(client);
                    self.status = "Connected".into();

                    let path = endpoint_path();
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(path, self.endpoint.read(cx).value().as_bytes());
                }
                Event::Message(message) => self.core.handle_message(*message),
                Event::Prompt(prompt) => {
                    self.answer
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    self.prompt = Some(prompt);
                    self.answer.read(cx).focus_handle(cx).focus(window, cx);
                }
                Event::Failed(error) => {
                    self.status = error;
                    self.client = None;
                    self.connection = None;
                    self.prompt = None;
                    self.show_endpoint = true;
                    self.forwarded.clear();
                }
            }
            while let Some(Outbound::RequestFull(pane)) = self.core.poll_outbound() {
                if let Some(client) = &self.client
                    && let Err(error) = client.request_full(pane)
                {
                    self.status = error.to_string();
                }
            }
            while let Some(event) = self.core.poll_event() {
                self.images.apply(&event);
                match event {
                    CoreEvent::Attached { .. } => self.send(
                        InputMessage::ClientFocus {
                            focused: window.is_window_active(),
                        },
                        cx,
                    ),
                    CoreEvent::OpenUri { uri, .. }
                        if uri.starts_with("https://")
                            || uri.starts_with("http://")
                            || uri.starts_with("mailto:") =>
                    {
                        cx.open_url(&uri)
                    }
                    CoreEvent::Clipboard { text, .. } => {
                        cx.write_to_clipboard(ClipboardItem::new_string(text))
                    }
                    CoreEvent::ViewportChanged { .. } | CoreEvent::AppearanceChanged => {
                        self.revision = self.revision.wrapping_add(1)
                    }
                    CoreEvent::Detached { .. } | CoreEvent::ServerStopping => {
                        self.status = "Disconnected".into();
                        self.client = None;
                        self.connection = None;
                        self.show_endpoint = true;
                    }
                    CoreEvent::ClientMessage { text, .. } => self.status = text,
                    _ => {}
                }
            }
            let pane = self.active_pane();
            if self.pane != pane {
                self.release_keys(cx);
                self.view(TerminalViewAction::Focus(false), cx);
                self.pane = pane;
                self.grid = None;
                self.geometry = None;
                self.cache = RowRenderCache::default();
                self.revision = self.revision.wrapping_add(1);
                self.focused = self.focus.is_focused(window) && window.is_window_active();
                self.view(TerminalViewAction::Focus(self.focused), cx);
            }
            cx.notify();
        }
    }

    fn active_pane(&self) -> Option<PaneId> {
        let snapshot = self.core.snapshot();
        let session = snapshot
            .sessions
            .iter()
            .find(|session| Some(session.id) == self.core.attached_session())?;
        session
            .windows
            .iter()
            .find(|window| window.id == snapshot.focused_window_for(session))
            .map(|window| window.active_pane)
    }

    fn send(&mut self, input: InputMessage, cx: &mut Context<Self>) {
        if let Some(client) = &self.client
            && let Err(error) = client.send_input(input)
        {
            self.status = error.to_string();
            cx.notify();
        }
    }

    fn view(&mut self, action: TerminalViewAction, cx: &mut Context<Self>) {
        if let Some(pane) = self.pane {
            self.send(InputMessage::TerminalView { pane, action }, cx);
        }
    }

    fn paste_clipboard(&mut self, _: &mut Context<Self>) {
        zz_gpui_ios::request_paste();
    }

    fn answer_prompt(
        &mut self,
        reply: Option<AskpassReply>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(prompt) = self.prompt.take() {
            let reply = reply.unwrap_or_else(|| {
                if prompt.kind == AskpassPromptKind::HostKey {
                    AskpassReply::Cancel
                } else {
                    AskpassReply::answer(self.answer.read(cx).value().to_string())
                }
            });
            let _ = prompt.reply.send(reply);
        }
        self.answer
            .update(cx, |input, cx| input.set_value("", window, cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn sync_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let focused = self.focus.is_focused(window) && window.is_window_active();
        if self.focused != focused {
            self.focused = focused;
            if !focused {
                self.release_keys(cx);
            }
            self.view(TerminalViewAction::Focus(focused), cx);
            cx.notify();
        }
    }

    fn release_keys(&mut self, cx: &mut Context<Self>) {
        for key in std::mem::take(&mut self.forwarded) {
            if let Some(pane) = self.pane {
                let input = key_input(
                    &gpui::Keystroke {
                        key,
                        key_char: None,
                        modifiers: Default::default(),
                    },
                    KeyAction::Release,
                );
                self.send(
                    InputMessage::Key {
                        pane,
                        input,
                        text_follows: false,
                    },
                    cx,
                );
            }
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.pane.filter(|_| self.client.is_some()) else {
            return;
        };
        let input = key_input(
            &event.keystroke,
            if event.is_held {
                KeyAction::Repeat
            } else {
                KeyAction::Press
            },
        );
        if input.key == KeyCode::Unidentified {
            return;
        }
        if !self.core.claims_prefix_input(&input) {
            let handled = match self.chrome.resolve(TERMINAL_TABLE, &input) {
                Some(ChromeAction::TerminalCopy) => {
                    self.view(
                        TerminalViewAction::CopySelection {
                            request_id: 1,
                            target: ClipboardTarget::Clipboard,
                        },
                        cx,
                    );
                    true
                }
                Some(ChromeAction::TerminalPaste) => {
                    self.paste_clipboard(cx);
                    true
                }
                Some(ChromeAction::TerminalSelectAll) => {
                    self.view(TerminalViewAction::SelectAll, cx);
                    true
                }
                Some(ChromeAction::TerminalClearHistory) => {
                    self.view(TerminalViewAction::ClearHistory, cx);
                    true
                }
                Some(
                    action @ (ChromeAction::TerminalFontIncrease
                    | ChromeAction::TerminalFontDecrease),
                ) => {
                    self.font_delta = (self.font_delta
                        + if action == ChromeAction::TerminalFontIncrease {
                            1.
                        } else {
                            -1.
                        })
                    .clamp(-6., 24.);
                    self.revision = self.revision.wrapping_add(1);
                    cx.notify();
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
            if input.modifiers.shift()
                && !input.modifiers.control()
                && !input.modifiers.alt()
                && !input.modifiers.platform()
            {
                let action = match input.key {
                    KeyCode::PageUp => Some(TerminalViewAction::ScrollPages(-1)),
                    KeyCode::PageDown => Some(TerminalViewAction::ScrollPages(1)),
                    KeyCode::Home => Some(TerminalViewAction::ScrollTop),
                    KeyCode::End => Some(TerminalViewAction::ScrollBottom),
                    _ => None,
                };
                if let Some(action) = action {
                    self.view(action, cx);
                    cx.stop_propagation();
                    return;
                }
            }
        }
        self.forwarded.insert(event.keystroke.key.clone());
        self.send(
            InputMessage::Key {
                pane,
                input,
                text_follows: false,
            },
            cx,
        );
        cx.stop_propagation();
    }

    fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.forwarded.remove(&event.keystroke.key)
            && let Some(pane) = self.pane
        {
            self.send(
                InputMessage::Key {
                    pane,
                    input: key_input(&event.keystroke, KeyAction::Release),
                    text_follows: false,
                },
                cx,
            );
            cx.stop_propagation();
        }
    }

    fn mouse(
        &mut self,
        position: Point<Pixels>,
        phase: TerminalMousePhase,
        modifiers: gpui::Modifiers,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(geometry) = self.geometry else {
            return;
        };
        let x = (position.x - geometry.grid_bounds.left()).max(px(0.));
        let y = (position.y - geometry.grid_bounds.top()).max(px(0.));
        let grid = geometry.grid;
        let cell = PointerCellEvent {
            column: ((x / geometry.cell_width) as u16).min(grid.columns.saturating_sub(1)),
            row: ((y / geometry.line_height) as u16).min(grid.rows.saturating_sub(1)),
            click_count: clicks.min(3) as u8,
            rectangle: modifiers.alt,
        };
        let scale = grid.cell_width_px as f32 / f32::from(geometry.cell_width);
        let input = TerminalMouseInput::new(
            phase,
            Some(TerminalMouseButton::Left),
            cell,
            (f32::from(x) * scale) as u32,
            (f32::from(y) * scale) as u32,
            u32::from(grid.columns) * grid.cell_width_px,
            u32::from(grid.rows) * grid.cell_height_px,
            grid.cell_width_px,
            grid.cell_height_px,
            wire_modifiers(modifiers),
            self.select || modifiers.shift,
        );
        self.view(TerminalViewAction::Mouse(input), cx);
    }

    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PaintState> {
        for image in self.images.take_retired() {
            let _ = window.drop_image(image);
        }
        let pane = self.pane?;
        let viewport = self.core.viewport(pane)?;
        let mut appearance = self.core.appearance().cloned().unwrap_or_default();
        appearance.font_families = vec!["Lilex".into()];
        appearance.font_families_bold.clear();
        appearance.font_families_italic.clear();
        appearance.font_families_bold_italic.clear();
        appearance.font_size_points = 14. + self.font_delta;
        let rows: Vec<_> = (0..u64::from(viewport.rows))
            .map(|row| (self.revision << 16) | row)
            .collect();
        let paint = self.cache.prepaint(
            TerminalRenderInput {
                viewport,
                row_revisions: &rows,
                revision_epoch: self.revision,
                history: None,
                images: self
                    .images
                    .pane(pane)
                    .map(|images| images as &dyn zz_ui::terminal::TerminalImageSource),
                local_scroll_target: None,
                scroll_pixel_offset: px(0.0),
                command_output: false,
                appearance: &appearance,
                appearance_hash: appearance.stable_hash(),
                text_opacity: 1.,
                focused: self.focused,
                cursor_blink_visible: true,
                marked_text: None,
            },
            bounds,
            window,
            cx,
        );
        let grid = paint.geometry.grid;
        self.geometry = Some(paint.geometry);
        if self.grid != Some(grid) && grid.columns > 0 && grid.rows > 0 {
            self.grid = Some(grid);
            self.send(
                InputMessage::ResizeTerminal {
                    pane,
                    columns: grid.columns,
                    rows: grid.rows,
                    cell_width_px: grid.cell_width_px,
                    cell_height_px: grid.cell_height_px,
                },
                cx,
            );
        }
        Some(paint)
    }
}

impl Render for TerminalApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible = window.fully_visible_bounds();
        let prepare = cx.entity();
        let paint = cx.entity();
        let status = self
            .pane
            .and_then(|pane| self.core.viewport(pane))
            .map(|viewport| {
                format!(
                    "{} · {} × {}",
                    viewport.title(),
                    viewport.columns,
                    viewport.rows
                )
            })
            .filter(|_| self.client.is_some())
            .unwrap_or_else(|| self.status.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .pt(visible.top() + px(4.))
            .pb(window.viewport_size().height - visible.bottom() + px(4.))
            .pl(visible.left() + px(8.))
            .pr(window.viewport_size().width - visible.right() + px(8.))
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("connection")
                            .label("Host")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_endpoint = !this.show_endpoint;
                                cx.notify();
                            })),
                    )
                    .child(Button::new("copy").label("Copy").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.view(
                                TerminalViewAction::CopySelection {
                                    request_id: 1,
                                    target: ClipboardTarget::Clipboard,
                                },
                                cx,
                            );
                            window.focus(&this.focus, cx);
                        },
                    )))
                    .child(Button::new("paste").label("Paste").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.paste_clipboard(cx);
                            window.focus(&this.focus, cx);
                        },
                    )))
                    .child(
                        Button::new("select")
                            .label(if self.select { "Done" } else { "Select" })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select = !this.select;
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    )
                    .child(Button::new("bottom").label("End").on_click(cx.listener(
                        |this, _, window, cx| {
                            this.view(TerminalViewAction::ScrollBottom, cx);
                            window.focus(&this.focus, cx);
                        },
                    ))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().foreground.muted())
                    .child(status),
            )
            .when(self.show_endpoint, |root| {
                root.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Input::new(&self.endpoint).flex_1())
                        .child(
                            Button::new("connect").label("Connect").on_click(
                                cx.listener(|this, _, window, cx| this.connect(window, cx)),
                            ),
                        ),
                )
            })
            .when_some(self.prompt.as_ref(), |root, prompt| {
                let host_key = prompt.kind == AskpassPromptKind::HostKey;
                root.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .child(prompt.text.clone())
                        .when(!host_key, |root| {
                            root.child(Input::new(&self.answer).when(!prompt.echo, |input| {
                                input.content_type(InputContentType::Password)
                            }))
                        })
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .when(host_key, |root| {
                                    root.child(
                                        Button::new("trust-once").label("Trust once").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.answer_prompt(
                                                    Some(AskpassReply::answer("once")),
                                                    window,
                                                    cx,
                                                )
                                            }),
                                        ),
                                    )
                                    .child(
                                        Button::new("trust-save").label("Trust and save").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.answer_prompt(
                                                    Some(AskpassReply::answer("save")),
                                                    window,
                                                    cx,
                                                )
                                            }),
                                        ),
                                    )
                                })
                                .when(!host_key, |root| {
                                    root.child(Button::new("answer").label("Continue").on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.answer_prompt(None, window, cx)
                                        }),
                                    ))
                                })
                                .child(Button::new("cancel").label("Cancel").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.answer_prompt(Some(AskpassReply::Cancel), window, cx)
                                    }),
                                )),
                        ),
                )
            })
            .child(
                div()
                    .id("terminal")
                    .role(gpui::accesskit::Role::Terminal)
                    .key_context("Terminal")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::key_down))
                    .on_key_up(cx.listener(Self::key_up))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&this.focus, cx);
                            this.drag = Some(event.position);
                            this.scroll = 0.;
                            this.mouse_down = this.select
                                || this
                                    .core
                                    .viewport(this.pane.unwrap_or(PaneId(0)))
                                    .is_some_and(|v| v.mouse_tracking)
                                || event.modifiers.control
                                || event.modifiers.platform;
                            if this.mouse_down {
                                this.mouse(
                                    event.position,
                                    TerminalMousePhase::Press,
                                    event.modifiers,
                                    event.click_count,
                                    cx,
                                );
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                        if let Some(previous) = this.drag {
                            if this.mouse_down {
                                this.mouse(
                                    event.position,
                                    TerminalMousePhase::Motion,
                                    event.modifiers,
                                    1,
                                    cx,
                                );
                            } else if let Some(geometry) = this.geometry {
                                this.scroll +=
                                    (previous.y - event.position.y) / geometry.line_height;
                                let lines = this.scroll as i32;
                                this.scroll -= lines as f32;
                                if lines != 0 {
                                    this.view(TerminalViewAction::ScrollLines(lines), cx);
                                }
                            }
                            this.drag = Some(event.position);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                            if this.drag.take().is_some() && std::mem::take(&mut this.mouse_down) {
                                this.mouse(
                                    event.position,
                                    TerminalMousePhase::Release,
                                    event.modifiers,
                                    event.click_count,
                                    cx,
                                );
                            }
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                            if this.drag.take().is_some() && std::mem::take(&mut this.mouse_down) {
                                this.mouse(
                                    event.position,
                                    TerminalMousePhase::Release,
                                    event.modifiers,
                                    event.click_count,
                                    cx,
                                );
                            }
                        }),
                    )
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                prepare.update(cx, |this, cx| this.prepare(bounds, window, cx))
                            },
                            move |bounds, mut state, window, cx| {
                                paint.update(cx, |this, cx| {
                                    window.handle_input(
                                        &this.focus,
                                        ElementInputHandler::new(bounds, cx.entity()),
                                        cx,
                                    );
                                    if let Some(state) = &mut state {
                                        this.cache.paint(state, bounds, window, cx);
                                    }
                                })
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

impl EntityInputHandler for TerminalApp {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
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
        None
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane {
            self.send(
                InputMessage::Text {
                    pane,
                    text: text.into(),
                },
                cx,
            );
        }
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.geometry.and_then(|geometry| geometry.input_bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
    fn paste(&mut self, item: ClipboardItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.view(TerminalViewAction::Paste(text), cx);
        }
    }
}

fn endpoint_path() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join("Library/Application Support/zz-gpui/endpoint")
}
