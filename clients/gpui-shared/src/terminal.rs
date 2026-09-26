use std::{cell::Cell, collections::HashSet, ops::Range, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    Anchor, AnyElement, App, Bounds, ClipboardEntry, ClipboardItem, Context, Corners,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Hsla, ImageSource,
    KeyDownEvent, KeyUpEvent, Keystroke, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point, Render,
    ScrollWheelEvent, Subscription, Task, UTF16Selection, Window, anchored, canvas, deferred, div,
    img, point, prelude::*, px,
};
use zz_client::{
    ChromeAction, ChromeKeymap, ChromeProfile, ClientCore, CoreEvent, TERMINAL_TABLE,
    ViewportDamage,
};
use zz_protocol::{InputMessage, PaneId, PopupAction, TerminalUiCommand};
use zz_terminal::{
    AppearanceConfigKey, AppearanceSource, KeyAction, KeyCode, KeyInput, PointerCellEvent,
    SearchCase, SearchDirection, SearchMode, SearchQuery, SearchStatus, SessionStatus,
    TerminalAppearance, TerminalMode, TerminalMouseButton, TerminalMouseInput, TerminalMousePhase,
    TerminalViewAction, TerminalViewport,
};
use zz_ui::{
    ActiveTheme, Colorize as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    StyledExt as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    pane::{
        PaneOverlayCorner, pane_overlay_stack, pane_status_badge, pane_sync_badge,
        pane_unzoom_control, terminal_link_popup, terminal_mode_indicator, terminal_status_popup,
    },
    terminal::{
        GridSize, PaintState, RowRenderCache, TerminalRenderInput, cursor_should_blink,
        selection_autoscroll_lines, terminal_font_for_style,
    },
};

use crate::connection::Connection;

#[derive(Clone, PartialEq)]
pub(crate) struct TerminalDisplayPreferences {
    pub font_family: Option<String>,
    pub font_scale: f32,
}

impl gpui::Global for TerminalDisplayPreferences {}

pub(crate) fn localized_font_appearance(
    core: &ClientCore,
    available_fonts: &[String],
    fallback: &str,
    cx: &App,
) -> TerminalAppearance {
    let mut appearance = core.appearance().cloned().unwrap_or_default();
    for (families, key) in [
        (
            &mut appearance.font_families,
            AppearanceConfigKey::FontFamily,
        ),
        (
            &mut appearance.font_families_bold,
            AppearanceConfigKey::FontFamilyBold,
        ),
        (
            &mut appearance.font_families_italic,
            AppearanceConfigKey::FontFamilyItalic,
        ),
        (
            &mut appearance.font_families_bold_italic,
            AppearanceConfigKey::FontFamilyBoldItalic,
        ),
    ] {
        localize_font_stack(
            families,
            core.appearance_provenance().source(key),
            available_fonts,
        );
    }
    if appearance.font_families.is_empty() {
        appearance.font_families.push(fallback.to_owned());
    }
    if let Some(preferences) = cx.try_global::<TerminalDisplayPreferences>() {
        if let Some(family) = preferences.font_family.as_ref().filter(|family| {
            available_fonts
                .iter()
                .any(|available| available.eq_ignore_ascii_case(family))
        }) {
            appearance.font_families = vec![family.clone()];
            appearance.font_families_bold.clear();
            appearance.font_families_italic.clear();
            appearance.font_families_bold_italic.clear();
        }
        appearance.font_size_points =
            (appearance.font_size_points * preferences.font_scale).clamp(7.0, 48.0);
    }
    appearance
}

fn localize_font_stack(families: &mut Vec<String>, source: AppearanceSource, available: &[String]) {
    if source == AppearanceSource::Default {
        families.clear();
    } else {
        families.retain(|family| {
            available
                .iter()
                .any(|name| name.eq_ignore_ascii_case(family))
        });
    }
}

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
    available_fonts: Vec<String>,
    text_opacity: f32,
    pane_status: (Option<String>, bool, bool),
    corner_radii: Corners<Pixels>,
    resize_suppressed: Rc<Cell<bool>>,
    scroll_rows: f32,
    overscroll: gpui::RubberBand,
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
    hovered_image_uri: Option<Arc<str>>,
    image_hover_ready: bool,
    image_hover_task: Option<Task<()>>,
    link_hover_bounds: Option<Bounds<Pixels>>,
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
    search_input: Option<Entity<InputState>>,
    search_focus_pending: bool,
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
                CoreEvent::Message(message)
                    if matches!(message.as_ref(),
                    zz_protocol::ProtocolMessage::PastedImageChunk { pane: changed, .. }
                    | zz_protocol::ProtocolMessage::PastedImageUnavailable { pane: changed, .. }
                    if *changed == pane) =>
                {
                    cx.notify();
                }
                CoreEvent::Attached { .. } | CoreEvent::AppearanceChanged => {
                    this.observe_image_hover(None, cx);
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
            available_fonts: cx.text_system().all_font_names(),
            text_opacity: 1.0,
            pane_status: (None, false, false),
            corner_radii: Corners::default(),
            resize_suppressed: Rc::default(),
            scroll_rows: 0.,
            overscroll: gpui::RubberBand::default(),
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
            hovered_image_uri: None,
            image_hover_ready: false,
            image_hover_task: None,
            link_hover_bounds: None,
            selection_pointer: None,
            selection_autoscroll_lines: 0,
            selection_autoscroll_task: None,
            focus_subscriptions: Vec::new(),
            all_dirty: true,
            dirty_rows: HashSet::new(),
            forwarded: HashSet::new(),
            marked_text: None,
            surface: TerminalSurface::Pane,
            chrome: ChromeKeymap::for_profile(if cfg!(target_os = "ios") {
                ChromeProfile::DesktopApple
            } else {
                ChromeProfile::Desktop
            }),
            apple_chrome: ChromeKeymap::for_profile(ChromeProfile::DesktopApple),
            search: None,
            search_input: None,
            search_focus_pending: false,
            search_accept_on_enter: false,
            swallowed_search_key: None,
            _subscription: subscription,
        }
    }

    pub(crate) fn with_resize_suppression(mut self, suppressed: Rc<Cell<bool>>) -> Self {
        self.resize_suppressed = suppressed;
        self
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

    pub(crate) fn set_text_dimmed(&mut self, dimmed: bool, opacity: f32, cx: &mut Context<Self>) {
        let opacity = if dimmed { opacity.clamp(0.0, 1.0) } else { 1.0 };
        if self.text_opacity.to_bits() != opacity.to_bits() {
            self.text_opacity = opacity;
            cx.notify();
        }
    }

    pub(crate) fn set_pane_status(
        &mut self,
        dead: Option<String>,
        synchronized: bool,
        zoomed: bool,
        cx: &mut Context<Self>,
    ) {
        let status = (dead, synchronized, zoomed);
        if self.pane_status != status {
            self.pane_status = status;
            cx.notify();
        }
    }

    pub(crate) fn set_corner_radii(&mut self, radii: Corners<Pixels>, cx: &mut Context<Self>) {
        if self.corner_radii != radii {
            self.corner_radii = radii;
            cx.notify();
        }
    }

    pub(crate) fn pane_background(&self, cx: &App) -> Hsla {
        let core = &self.connection.read(cx).core;
        let background = self
            .viewport(core)
            .map_or(cx.theme().background, |viewport| {
                zz_ui::terminal::terminal_background(
                    viewport.background,
                    core.appearance()
                        .map_or(1.0, |appearance| appearance.background_opacity),
                )
            });
        if self.surface == TerminalSurface::Popup {
            background
        } else {
            cx.theme()
                .background
                .opaque()
                .blend(background)
                .opacity(cx.theme().pane_background_opacity)
        }
    }

    fn observe_image_hover(&mut self, uri: Option<Arc<str>>, cx: &mut Context<Self>) {
        let uri = uri.filter(|uri| pasted_image_number(uri).is_some());
        if self.hovered_image_uri == uri {
            return;
        }
        self.hovered_image_uri.clone_from(&uri);
        self.image_hover_ready = false;
        self.image_hover_task = None;
        if let Some(uri) = uri {
            let number = pasted_image_number(&uri).unwrap();
            self.connection.update(cx, |connection, cx| {
                connection.fetch_pasted_image(self.pane, number, cx);
            });
            self.image_hover_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.hovered_image_uri.as_deref() == Some(uri.as_ref()) {
                        this.image_hover_ready = true;
                        cx.notify();
                    }
                });
            }));
        }
        cx.notify();
    }

    fn image_hover_popover(&self, cx: &App) -> Option<AnyElement> {
        if !self.image_hover_ready {
            return None;
        }
        let bounds = self.link_hover_bounds?;
        let number = pasted_image_number(self.hovered_image_uri.as_deref()?)?;
        let image = self.connection.read(cx).pasted_image(self.pane, number)?;
        let (anchor, position) = if bounds.origin.y >= px(308.0) {
            (Anchor::BottomLeft, bounds.origin)
        } else {
            (Anchor::TopLeft, bounds.bottom_left())
        };
        Some(
            deferred(
                anchored()
                    .anchor(anchor)
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .size(px(300.0))
                            .p_1()
                            .bg(cx.theme().background.raised(1).opaque())
                            .text_color(cx.theme().foreground)
                            .border_1()
                            .border_color(cx.theme().border())
                            .rounded(cx.theme().radius)
                            .shadow_md()
                            .child(
                                img(ImageSource::Image(image))
                                    .size_full()
                                    .object_fit(ObjectFit::ScaleDown),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
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
        if self.search.is_some() && !self.focus.is_focused(window) {
            return;
        }
        self.reset_cursor_blink(cx);
        let input = key_input(event);
        if input.key == KeyCode::Unidentified {
            return;
        }
        if self.swallowed_search_key == Some(input.key) {
            cx.stop_propagation();
            return;
        }
        if self.search.is_some() {
            if self
                .chrome
                .resolve(TERMINAL_TABLE, &input)
                .or_else(|| self.apple_chrome.resolve(TERMINAL_TABLE, &input))
                == Some(ChromeAction::TerminalSearch)
            {
                self.open_search(window, cx);
            } else if let Some(input) = &self.search_input {
                input.read(cx).focus_handle(cx).focus(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        if !self.connection.read(cx).core.claims_prefix_input(&input) {
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
                Some(ChromeAction::TerminalPaste) => {
                    #[cfg(target_os = "ios")]
                    {
                        zz_gpui_ios::request_paste();
                        cx.stop_propagation();
                    }
                    return;
                }
                Some(ChromeAction::TerminalSearch) => {
                    self.open_search(window, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        let raw = cfg!(target_os = "ios")
            || self
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
        self.sync_search_input(window, cx);
    }

    fn begin_search(&mut self, query: SearchQuery, accept_on_enter: bool, cx: &mut Context<Self>) {
        self.search = Some(query.clone());
        self.search_focus_pending = true;
        self.search_accept_on_enter = accept_on_enter;
        self.marked_text = None;
        self.view(TerminalViewAction::SearchBegin(query), cx);
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.search_focus_pending = false;
        self.marked_text = None;
        self.view(TerminalViewAction::SearchClose, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn sync_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_none() {
            return;
        }
        if self.search_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Find in this pane…")
                    .context_menu(true)
            });
            cx.subscribe_in(
                &input,
                window,
                |view, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        if let Some(query) = view.search.as_mut() {
                            query.text = input.read(cx).value().to_string();
                            let query = query.clone();
                            view.view(TerminalViewAction::SearchUpdate(query), cx);
                            cx.notify();
                        }
                    }
                    InputEvent::Focus if view.surface == TerminalSurface::Pane => {
                        view.connection.update(cx, |connection, cx| {
                            connection.command(
                                "select-pane",
                                vec!["-Z".into(), "-t".into(), view.pane.to_string()],
                                cx,
                            );
                        });
                    }
                    InputEvent::PressEnter { shift } if view.search.is_some() => {
                        if view.search_accept_on_enter {
                            view.search = None;
                            view.search_focus_pending = false;
                            view.marked_text = None;
                            view.focus.focus(window, cx);
                            view.swallowed_search_key = Some(KeyCode::Enter);
                            cx.notify();
                        } else {
                            let backward =
                                view.search.as_ref().is_some_and(|query| {
                                    query.direction == SearchDirection::Backward
                                }) ^ shift;
                            view.step_search(backward, cx);
                        }
                    }
                    _ => {}
                },
            )
            .detach();
            self.search_input = Some(input);
        }
        if self.search_focus_pending {
            self.search_focus_pending = false;
            let input = self.search_input.as_ref().unwrap();
            let query = self.search.as_ref().unwrap();
            input.update(cx, |input, cx| {
                input.set_value(query.text.clone(), window, cx);
            });
            input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    fn step_search(&self, backward: bool, cx: &mut Context<Self>) {
        self.view(
            if backward {
                TerminalViewAction::SearchPrevious
            } else {
                TerminalViewAction::SearchNext
            },
            cx,
        );
    }

    fn search_bar(&self, status: Option<SearchStatus>, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.as_ref().unwrap();
        let enabled = status.is_some_and(|status| {
            status.total > 0 && !status.pending() && !status.invalid_pattern()
        });
        let regex = Button::compact_icon("terminal-search-regex", IconName::Asterisk)
            .ghost()
            .flat()
            .selected(query.mode == SearchMode::Regex)
            .tooltip("Regular expression")
            .on_click(cx.listener(|view, _, _, cx| {
                if let Some(query) = view.search.as_mut() {
                    query.mode = match query.mode {
                        SearchMode::Literal => SearchMode::Regex,
                        SearchMode::Regex => SearchMode::Literal,
                    };
                    let query = query.clone();
                    view.view(TerminalViewAction::SearchUpdate(query), cx);
                    cx.notify();
                }
                cx.stop_propagation();
            }));
        let case = Button::compact_icon("terminal-search-case", IconName::CaseSensitive)
            .ghost()
            .flat()
            .selected(query.case == SearchCase::Sensitive)
            .tooltip(match query.case {
                SearchCase::Smart => "Smart case",
                SearchCase::Sensitive => "Match case",
                SearchCase::Insensitive => "Ignore case",
            })
            .on_click(cx.listener(|view, _, _, cx| {
                if let Some(query) = view.search.as_mut() {
                    query.case = match query.case {
                        SearchCase::Sensitive => SearchCase::Insensitive,
                        _ => SearchCase::Sensitive,
                    };
                    let query = query.clone();
                    view.view(TerminalViewAction::SearchUpdate(query), cx);
                    cx.notify();
                }
                cx.stop_propagation();
            }));
        let previous = Button::compact_icon("terminal-search-previous", IconName::ChevronUp)
            .ghost()
            .flat()
            .disabled(!enabled)
            .tooltip("Previous match")
            .on_click(cx.listener(|view, _, _, cx| {
                view.step_search(true, cx);
                cx.stop_propagation();
            }));
        let next = Button::compact_icon("terminal-search-next", IconName::ChevronDown)
            .ghost()
            .flat()
            .disabled(!enabled)
            .tooltip("Next match")
            .on_click(cx.listener(|view, _, _, cx| {
                view.step_search(false, cx);
                cx.stop_propagation();
            }));
        let close = Button::compact_icon("terminal-search-close", IconName::Xmark)
            .ghost()
            .flat()
            .tooltip("Close search")
            .on_click(cx.listener(|view, _, window, cx| {
                view.close_search(window, cx);
                cx.stop_propagation();
            }));
        div()
            .id("terminal-search")
            .debug_selector(|| "terminal-search".to_owned())
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .w(px(460.0))
            .max_w_full()
            .p(px(4.0))
            .bg(cx.theme().background.raised(1))
            .rounded(cx.theme().radius)
            .control_surface(cx)
            .occlude()
            .font_family(cx.theme().font_family.clone())
            .text_color(cx.theme().foreground)
            .text_size(zz_ui::rems_from_px(13.0))
            .line_height(px(16.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                if let Some(query) = view.search.as_mut()
                    && let Some(action) = search_key_action(query, &key_input(event))
                {
                    view.view(action, cx);
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|view, _: &zz_ui::input::Escape, window, cx| {
                view.close_search(window, cx);
                view.swallowed_search_key = Some(KeyCode::Escape);
                cx.stop_propagation();
            }))
            .on_action(|_: &zz_ui::input::Enter, _, cx| cx.stop_propagation())
            .child(
                div().flex_1().min_w(px(110.0)).child(
                    Input::new(self.search_input.as_ref().unwrap())
                        .small()
                        .appearance(false)
                        .prefix(Icon::new(IconName::Search).size(px(13.0))),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.0))
                    .ml_auto()
                    .child(
                        div()
                            .px(px(4.0))
                            .text_size(zz_ui::rems_from_px(11.0))
                            .text_color(if status.is_some_and(SearchStatus::invalid_pattern) {
                                cx.theme().danger
                            } else {
                                cx.theme().foreground.muted()
                            })
                            .child(if query.text.is_empty() {
                                String::new()
                            } else {
                                search_result_text(status)
                            }),
                    )
                    .child(regex)
                    .child(case)
                    .child(previous)
                    .child(next)
                    .child(close),
            )
            .into_any_element()
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
                connection.command(
                    "select-pane",
                    vec!["-Z".into(), "-t".into(), self.pane.to_string()],
                    cx,
                );
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
            window.request_virtual_keyboard();
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
        self.observe_image_hover(None, cx);
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

    fn overscroll(
        &mut self,
        delta: Pixels,
        phase: gpui::TouchPhase,
        window: &Window,
        cx: &App,
    ) -> Pixels {
        if window.gesture_tuning().overscroll != gpui::Overscroll::Bounce {
            return delta;
        }
        let Some(viewport) = self.viewport(&self.connection.read(cx).core) else {
            return delta;
        };
        let bar = viewport.scrollbar;
        if viewport.mouse_tracking
            || bar.total <= bar.len
            || matches!(viewport.mode, TerminalMode::Copy { .. })
        {
            return delta;
        }
        let forward = if bar.offset + bar.len < bar.total {
            px(f32::MIN)
        } else {
            Pixels::ZERO
        };
        let back = if bar.offset > 0 {
            px(f32::MAX)
        } else {
            Pixels::ZERO
        };
        self.overscroll.scroll(
            Pixels::ZERO,
            delta,
            forward,
            back,
            self.surface_bounds.size.height,
            phase,
        )
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

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.line_height);
        let stretched = self.overscroll.is_stretched();
        let delta = self.overscroll(delta.y, event.touch_phase, window, cx);
        if stretched || self.overscroll.is_stretched() {
            cx.notify();
            cx.stop_propagation();
        }
        let lines = accumulate_scroll(
            &mut self.scroll_rows,
            -f32::from(delta) / f32::from(self.line_height),
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
        let displacement = self.overscroll.displacement(bounds.size.height);
        if self.overscroll.is_animating() {
            window.request_animation_frame();
        }
        let shifted = Bounds::new(
            bounds.origin + point(Pixels::ZERO, displacement),
            bounds.size,
        );
        let (paint, attached) = self.connection.clone().update(cx, |connection, cx| {
            for image in connection.take_retired_terminal_images() {
                let _ = window.drop_image(image);
            }
            let viewport = self.viewport(&connection.core)?;
            let appearance = localized_font_appearance(
                &connection.core,
                &self.available_fonts,
                &cx.theme().mono_font_family,
                cx,
            );
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
                    scroll_pixel_offset: px(0.0),
                    command_output: self.surface == TerminalSurface::CommandOutput,
                    appearance: &appearance,
                    appearance_hash: appearance.stable_hash(),
                    text_opacity: self.text_opacity,
                    focused: self.focused,
                    cursor_blink_visible: self.cursor_blink_visible,
                    marked_text: self
                        .search
                        .is_none()
                        .then_some(self.marked_text.as_deref())
                        .flatten(),
                },
                shifted,
                window,
                cx,
            );
            Some((paint, connection.core.attached_session().is_some()))
        })?;
        let geometry = paint.geometry;
        self.link_hover_bounds = geometry.link_hover_bounds;
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
        let measurable =
            bounds.size.width >= geometry.cell_width && bounds.size.height >= geometry.line_height;
        if measurable
            && self.geometry != Some(measured)
            && attached
            && (self.surface != TerminalSurface::Pane || !self.resize_suppressed.get())
        {
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
        hitbox: &gpui::Hitbox,
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
        {
            let view = cx.entity().downgrade();
            let press_hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &gpui::LongPressEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                let _ = view.update(cx, |this, cx| {
                    let entity = cx.entity();
                    if event.phase == gpui::TouchPhase::Started {
                        if window.default_prevented()
                            || !press_hitbox.is_hovered_at(event.start_position, window)
                        {
                            return;
                        }
                        window.capture_long_press(&entity);
                        window.prevent_default();
                    } else if !window.has_long_press_capture(&entity) {
                        return;
                    }
                    this.touch_selection(event.phase, event.position, window, cx);
                    cx.stop_propagation();
                });
            });
            let view = cx.entity().downgrade();
            let drag_hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &gpui::TouchDragEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                let _ = view.update(cx, |this, cx| {
                    if event.phase == gpui::TouchPhase::Started {
                        let tracking = this
                            .viewport(&this.connection.read(cx).core)
                            .is_some_and(|viewport| viewport.mouse_tracking);
                        if window.default_prevented()
                            || !drag_hitbox.is_hovered_at(event.start_position, window)
                            || (!tracking && !this.scrollbar_hit(event.start_position, cx))
                        {
                            return;
                        }
                        this.on_mouse_down(
                            &MouseDownEvent {
                                button: MouseButton::Left,
                                position: event.start_position,
                                modifiers: gpui::Modifiers::default(),
                                click_count: 1,
                                first_mouse: false,
                            },
                            window,
                            cx,
                        );
                        window.prevent_default();
                    } else if this.scrollbar_dragging
                        || this.pressed_buttons.contains(&MouseButton::Left)
                    {
                        match event.phase {
                            gpui::TouchPhase::Moved => this.on_mouse_move(
                                &MouseMoveEvent {
                                    position: event.position,
                                    pressed_button: Some(MouseButton::Left),
                                    modifiers: gpui::Modifiers::default(),
                                },
                                window,
                                cx,
                            ),
                            gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled => this
                                .on_mouse_up(
                                    &MouseUpEvent {
                                        button: MouseButton::Left,
                                        position: event.position,
                                        modifiers: gpui::Modifiers::default(),
                                        click_count: 1,
                                    },
                                    window,
                                    cx,
                                ),
                            gpui::TouchPhase::Started => {}
                        }
                    } else {
                        return;
                    }
                    cx.stop_propagation();
                });
            });
        }
    }

    fn touch_selection(
        &mut self,
        phase: gpui::TouchPhase,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match phase {
            gpui::TouchPhase::Started => {
                self.on_mouse_down(
                    &MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: gpui::Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                        click_count: 2,
                        first_mouse: false,
                    },
                    window,
                    cx,
                );
                self.force_local_selection = true;
            }
            gpui::TouchPhase::Moved => self.on_mouse_move(
                &MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: gpui::Modifiers::default(),
                },
                window,
                cx,
            ),
            gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled => {
                self.on_mouse_up(
                    &MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: gpui::Modifiers::default(),
                        click_count: 1,
                    },
                    window,
                    cx,
                );
                #[cfg(target_os = "ios")]
                if phase == gpui::TouchPhase::Ended {
                    zz_gpui_ios::show_edit_menu(
                        f32::from(position.x) * window.zoom(),
                        f32::from(position.y) * window.zoom(),
                    );
                }
            }
        }
    }

    fn sync_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let focused = self.focus.is_focused(window) && window.is_window_active();
        if self.focused != focused {
            self.focused = focused;
            self.view(TerminalViewAction::Focus(focused), cx);
            if !focused {
                self.observe_image_hover(None, cx);
                self.end_drag();
                self.pressed_buttons.clear();
                #[cfg(target_os = "ios")]
                for key in self.forwarded.drain().collect::<Vec<_>>() {
                    self.send(
                        InputMessage::Key {
                            pane: self.pane,
                            input: keystroke_input(
                                &Keystroke {
                                    key,
                                    modifiers: gpui::Modifiers::default(),
                                    key_char: None,
                                },
                                KeyAction::Release,
                            ),
                            text_follows: false,
                        },
                        cx,
                    );
                }
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
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.search.is_some()
            && let Some(input) = &self.search_input
        {
            input.read(cx).focus_handle(cx)
        } else {
            self.focus.clone()
        }
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
        self.sync_search_input(window, cx);
        self.sync_focus(window, cx);
        let appearance = localized_font_appearance(
            &self.connection.read(cx).core,
            &self.available_fonts,
            &cx.theme().mono_font_family,
            cx,
        );
        let font = terminal_font_for_style(&appearance, &cx.theme().mono_font_family, false, false);
        let font_size = px((appearance.font_size_points + self.font_delta).clamp(7., 48.));
        let prepare = cx.entity();
        let paint = cx.entity();
        let mut mode = None;
        let mut hovered_uri = None;
        let mut search_status = None;
        let mut top_right: Vec<AnyElement> = Vec::new();
        let mut bottom_right: Vec<AnyElement> = Vec::new();
        if let Some(viewport) = self.viewport(&self.connection.read(cx).core) {
            hovered_uri.clone_from(&viewport.presentation.hovered_uri);
            search_status = viewport.search;
            if let Some(uri) = &hovered_uri
                && pasted_image_number(uri).is_none()
            {
                bottom_right.push(terminal_link_popup(presented_uri(uri), cx).into_any_element());
            }
            if self.surface != TerminalSurface::Popup {
                mode = terminal_mode_text(viewport.mode, viewport.unseen_output);
                if let Some(status) = terminal_status_text(&viewport.status) {
                    bottom_right.push(terminal_status_popup(status, cx).into_any_element());
                }
            }
        }
        self.observe_image_hover(
            if self.pointer.is_some() && self.surface == TerminalSurface::Pane {
                hovered_uri.clone()
            } else {
                None
            },
            cx,
        );
        if self.search.is_some() {
            top_right.push(self.search_bar(search_status, cx));
        }
        let mut root = div()
            .id(("terminal", self.pane.0))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(self.pane_background(cx))
            .rounded_bl(self.corner_radii.bottom_left)
            .rounded_br(self.corner_radii.bottom_right)
            .font(font)
            .text_size(font_size)
            .when(self.surface != TerminalSurface::Popup, |root| {
                root.pl(px(appearance.padding_left))
                    .pr(px(appearance.padding_right))
                    .pt(px(appearance.padding_top))
                    .pb(px(appearance.padding_bottom))
            })
            .key_context("Terminal")
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
                        let hitbox = window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal);
                        (
                            prepare.update(cx, |view, cx| view.prepare(bounds, window, cx)),
                            hitbox,
                        )
                    },
                    move |bounds, (mut state, hitbox), window, cx| {
                        paint.update(cx, |view, cx| {
                            view.paint(bounds, &mut state, &hitbox, window, cx);
                        });
                    },
                )
                .size_full(),
            );
        if hovered_uri.is_some() {
            root = root.cursor_pointer();
        }
        if let Some((label, detail)) = mode {
            top_right.push(terminal_mode_indicator(label, detail, cx).into_any_element());
        }
        if let Some(dead) = &self.pane_status.0 {
            top_right
                .push(pane_status_badge(IconName::CircleX, dead.clone(), cx).into_any_element());
        }
        if self.pane_status.1 {
            top_right.push(pane_sync_badge(cx).into_any_element());
        }
        if self.pane_status.2 {
            top_right.push(
                pane_unzoom_control()
                    .on_click(cx.listener(|view, _, _, cx| {
                        let pane = view.pane;
                        view.connection.update(cx, |connection, cx| {
                            if !connection.core.attached_read_only() {
                                connection.command(
                                    "resize-pane",
                                    vec!["-Z".into(), "-t".into(), pane.to_string()],
                                    cx,
                                );
                            }
                        });
                        cx.stop_propagation();
                    }))
                    .into_any_element(),
            );
        }
        if !top_right.is_empty() {
            root = root.child(
                pane_overlay_stack(PaneOverlayCorner::TopRight, top_right)
                    .left(px(8.0))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            );
        }
        if let Some(popover) = self.image_hover_popover(cx) {
            root = root.child(popover);
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
    fn paste(&mut self, item: ClipboardItem, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_some()
            && let Some(input) = self.search_input.clone()
        {
            input.update(cx, |input, cx| input.paste(item, window, cx));
            return;
        }
        if self.surface == TerminalSurface::Pane
            && self.search.is_none()
            && let Some(image) = item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image),
                _ => None,
            })
        {
            self.connection.update(cx, |connection, cx| {
                connection.upload_image(self.pane, image, cx);
            });
            return;
        }
        if let Some(text) = item.text() {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

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
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.is_some()
            && let Some(input) = self.search_input.clone()
        {
            input.update(cx, |input, cx| {
                input.replace_text_in_range(range, text, window, cx);
            });
            return;
        }
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

fn pasted_image_number(uri: &str) -> Option<u32> {
    uri.strip_prefix(zz_terminal::IMAGE_PLACEHOLDER_SCHEME)?
        .strip_prefix("://")?
        .parse()
        .ok()
}

fn search_key_action(query: &mut SearchQuery, input: &KeyInput) -> Option<TerminalViewAction> {
    match input.key {
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

fn search_result_text(status: Option<SearchStatus>) -> String {
    match status {
        Some(status) if status.invalid_pattern() => "Invalid pattern".to_owned(),
        Some(status) if status.pending() => "Searching…".to_owned(),
        Some(status) if status.total > 0 => format!("{} / {}", status.current(), status.total),
        _ => "0 matches".to_owned(),
    }
}

fn presented_uri(uri: &str) -> String {
    let mut characters = uri.chars();
    let mut presented = characters.by_ref().take(240).collect::<String>();
    if characters.next().is_some() {
        presented.push('…');
    }
    presented
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
            Some("Copy mode"),
            match (hide_position, unseen_output) {
                (true, 0) => String::new(),
                (true, unseen) => format!("+{unseen} output"),
                (false, 0) => format!("{position} / {total}"),
                (false, unseen) => format!("{position} / {total} · +{unseen} output"),
            },
        )),
        TerminalMode::View { position, total } => {
            Some((Some("View mode"), format!("{position} / {total}")))
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
    #[cfg(target_os = "ios")]
    {
        crate::input::key_input(keystroke, action)
    }
    #[cfg(not(target_os = "ios"))]
    {
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
            None
        );
        assert_eq!(
            search_key_action(&mut query, &key("enter".into(), true, false)),
            None
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
        assert_eq!(
            search_key_action(&mut query, &key("backspace".into(), false, false)),
            None
        );
        assert_eq!(query.text, "café");
        assert_eq!(query.direction, SearchDirection::Backward);
        assert_eq!(
            search_key_action(&mut query, &key("x".into(), false, false)),
            None
        );
    }

    #[test]
    fn terminal_search_feedback_uses_server_results() {
        assert_eq!(search_result_text(Some(SearchStatus::new(2, 4))), "2 / 4");
        assert_eq!(
            search_result_text(Some(SearchStatus::new(0, 0))),
            "0 matches"
        );
        assert_eq!(
            search_result_text(Some(SearchStatus::default().with_pending(true))),
            "Searching…"
        );
        assert_eq!(
            search_result_text(Some(SearchStatus::default().with_invalid_pattern(true))),
            "Invalid pattern"
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
            Some((Some("Copy mode"), "+3 output".into()))
        );
        assert_eq!(
            terminal_mode_text(
                TerminalMode::View {
                    position: 12,
                    total: 20,
                },
                0,
            ),
            Some((Some("View mode"), "12 / 20".into()))
        );
    }

    #[test]
    fn hovered_uri_presentation_preserves_character_boundaries() {
        let uri = format!("https://example.com/{}", "界".repeat(300));
        let presented = presented_uri(&uri);
        assert_eq!(presented.chars().count(), 241);
        assert!(presented.ends_with('…'));
        assert_eq!(presented_uri("https://example.com"), "https://example.com");
    }

    #[test]
    fn terminal_fonts_keep_available_overrides_and_drop_remote_defaults() {
        let available = vec!["Lilex".to_owned(), "Local Mono".to_owned()];
        let mut explicit = vec![
            "Missing Mono".to_owned(),
            "local mono".to_owned(),
            "Lilex".to_owned(),
        ];
        localize_font_stack(&mut explicit, AppearanceSource::Override, &available);
        assert_eq!(explicit, ["local mono", "Lilex"]);
        localize_font_stack(&mut explicit, AppearanceSource::Default, &available);
        assert!(explicit.is_empty());
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
