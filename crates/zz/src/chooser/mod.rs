use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId, IntoElement,
    KeyDownEvent, LayoutId, MouseButton, Pixels, Point, Render, ScrollStrategy, Style,
    UTF16Selection, UniformListScrollHandle, Window, div, prelude::*, uniform_list,
};
use zz_ui::chooser::{
    ChooserDimensions, ChooserModal, ChooserSearch as ChooserSearchView, chooser_has_key_gutter,
};
pub(crate) use zz_ui::chooser::{ChooserHint, ChooserRowTheme};
use zz_ui::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use zz_terminal::{KeyAction, KeyInput};

use crate::{
    config::frame_content_corner_radius,
    mux::{client::MuxClient, prefix::terminal_key_input},
    terminal::view::TERMINAL_FONT,
    window::corners::WindowCorners,
};

pub(crate) mod buffer;
pub(crate) mod tree;

impl Default for buffer::BufferChooser {
    fn default() -> Self {
        Self
    }
}

pub(crate) struct ChooserSearch<'a> {
    pub(crate) query: &'a str,
    pub(crate) reverse: bool,
}

pub(crate) trait ChooserSpec: Default + 'static {
    type State: Clone;
    type Item: Clone + 'static;
    type Action;

    const OVERLAY_ID: &'static str;
    const MODAL_ID: &'static str;
    const ROWS_ID: &'static str;
    const ROW_ID: &'static str;
    const CLOSE_ID: &'static str;
    const WIDTH: f32;
    const MAX_WIDTH: f32;
    const HEIGHT: f32;
    const MIN_HEIGHT: f32;
    const MAX_HEIGHT: f32;
    const HINTS: &'static [ChooserHint];

    fn state(mux: &MuxClient) -> Option<Self::State>;
    fn selected(state: &Self::State) -> u32;
    fn items(state: &Self::State) -> &[Self::Item];
    fn item_key(item: &Self::Item) -> &str;
    fn effective_selected(&self, state: &Self::State, _: &MuxClient) -> u32 {
        Self::selected(state)
    }
    fn render_items(state: &Self::State, _: &MuxClient) -> Vec<Self::Item> {
        Self::items(state).to_vec()
    }
    fn synchronize_local(&mut self, _: &Self::State, _: &MuxClient) {}
    fn search(state: &Self::State) -> Option<ChooserSearch<'_>>;
    /// The single-key confirmation the daemon's chooser session owns. Every key
    /// still goes to the daemon while it stands, so a client that does not draw
    /// it answers a kill the viewer never saw.
    fn prompt(_: &Self::State) -> Option<&str> {
        None
    }
    fn title(state: &Self::State) -> &'static str;
    fn subtitle(state: &Self::State, count: usize) -> String;
    fn row(
        item: Self::Item,
        index: usize,
        selected: bool,
        show_key_gutter: bool,
        mux: Entity<MuxClient>,
        theme: ChooserRowTheme,
    ) -> AnyElement;
    fn row_with_chooser(
        item: Self::Item,
        index: usize,
        selected: bool,
        show_key_gutter: bool,
        _: usize,
        _: Entity<Chooser<Self>>,
        mux: Entity<MuxClient>,
        theme: ChooserRowTheme,
    ) -> AnyElement {
        Self::row(item, index, selected, show_key_gutter, mux, theme)
    }
    fn key(input: KeyInput) -> Self::Action;
    fn search_append(text: String) -> Self::Action;
    fn close() -> Self::Action;
    fn send(mux: &MuxClient, action: Self::Action);
}

pub(crate) struct Chooser<S: ChooserSpec> {
    focus_handle: FocusHandle,
    mux: Entity<MuxClient>,
    scroll_handle: UniformListScrollHandle,
    revision: u64,
    selected: Option<u32>,
    search_active: bool,
    marked_text: Option<String>,
    input_bounds: Option<Bounds<Pixels>>,
    spec: S,
}

impl<S: ChooserSpec> Chooser<S> {
    pub(crate) fn new(mux: Entity<MuxClient>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            mux,
            scroll_handle: UniformListScrollHandle::new(),
            revision: 0,
            selected: None,
            search_active: false,
            marked_text: None,
            input_bounds: None,
            spec: S::default(),
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn synchronize(&mut self, state: &S::State, revision: u64, cx: &mut Context<Self>) {
        if self.revision == revision {
            return;
        }
        self.revision = revision;
        self.spec.synchronize_local(state, self.mux.read(cx));
        self.search_active = S::search(state).is_some();
        self.synchronize_selected(state, cx);
        if S::search(state).is_none() {
            self.marked_text = None;
        }
        cx.notify();
    }

    fn send(&self, action: S::Action, cx: &Context<Self>) {
        S::send(self.mux.read(cx), action);
    }

    fn synchronize_selected(&mut self, state: &S::State, cx: &Context<Self>) -> bool {
        let selected = self.spec.effective_selected(state, self.mux.read(cx));
        if self.selected == Some(selected) {
            return false;
        }
        self.selected = Some(selected);
        self.scroll_handle.scroll_to_item(
            usize::try_from(selected).unwrap_or(usize::MAX),
            ScrollStrategy::Center,
        );
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if self.search_active
            && keystroke.key_char.is_some()
            && !modifiers.control
            && !modifiers.platform
            && !modifiers.alt
        {
            // The IME input handler owns typed search text.
            return;
        }
        let input = terminal_key_input(keystroke, KeyAction::Press);
        self.send(S::key(input), cx);
        cx.stop_propagation();
    }
}

impl<S: ChooserSpec> Focusable for Chooser<S> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<S: ChooserSpec> EntityInputHandler for Chooser<S> {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.marked_text.as_ref()?;
        let length = marked.encode_utf16().count();
        actual_range.replace(0..length);
        (range.start <= length).then(|| marked.clone())
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
        if self.marked_text.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        if !text.is_empty() && self.search_active {
            self.send(S::search_append(text.to_owned()), cx);
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
        self.marked_text = (!text.is_empty()).then(|| text.to_owned());
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
        self.input_bounds
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

impl<S: ChooserSpec> Render for Chooser<S> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = S::state(self.mux.read(cx)) else {
            return div().into_any_element();
        };
        let selected = usize::try_from(self.spec.effective_selected(&state, self.mux.read(cx)))
            .unwrap_or(usize::MAX);
        let daemon_item_count = S::items(&state).len();
        let items: Arc<[S::Item]> = S::render_items(&state, self.mux.read(cx)).into();
        let count = items.len();
        let show_key_gutter = chooser_has_key_gutter(items.iter().map(|item| S::item_key(item)));
        let row_theme = ChooserRowTheme::from_theme(cx);
        let rows_mux = self.mux.clone();
        let rows_chooser = cx.entity();
        let rows = uniform_list(
            S::ROWS_ID,
            items.len(),
            cx.processor(move |_, range: Range<usize>, _, _| {
                range
                    .filter_map(|index| {
                        items.get(index).cloned().map(|item| {
                            S::row_with_chooser(
                                item,
                                index,
                                index == selected,
                                show_key_gutter,
                                daemon_item_count,
                                rows_chooser.clone(),
                                rows_mux.clone(),
                                row_theme,
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .track_scroll(&self.scroll_handle);

        let search = S::search(&state).map(|search| {
            let prefix = if search.reverse { "?" } else { "/" };
            let marked = self.marked_text.as_deref().unwrap_or_default();
            ChooserSearchView {
                prefix: prefix.into(),
                value: format!("{}{marked}", search.query).into(),
            }
        });

        let close_mux = self.mux.clone();
        let close = Button::new(S::CLOSE_ID)
            .xsmall()
            .ghost()
            .icon(IconName::Xmark)
            .tooltip("Close")
            .on_click(move |_, _, cx| {
                S::send(close_mux.read(cx), S::close());
                cx.stop_propagation();
            });
        let modal = ChooserModal::new(
            S::MODAL_ID,
            S::title(&state),
            S::subtitle(&state, count),
            ChooserDimensions {
                width: S::WIDTH,
                max_width: S::MAX_WIDTH,
                height: S::HEIGHT,
                min_height: S::MIN_HEIGHT,
                max_height: S::MAX_HEIGHT,
            },
            rows,
            close,
            TERMINAL_FONT,
        )
        .prompt(S::prompt(&state).map(|prompt| prompt.to_owned().into()))
        .search(search)
        .hints(S::HINTS);

        let backdrop_mux = self.mux.clone();
        WindowCorners::for_window(window)
            .bottom()
            .round_div(
                div()
                    .id(S::OVERLAY_ID)
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().scrim)
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_key_up(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        S::send(backdrop_mux.read(cx), S::close());
                        cx.stop_propagation();
                    })
                    .child(ChooserInputElement { view: cx.entity() })
                    .child(modal),
                frame_content_corner_radius(cx),
            )
            .into_any_element()
    }
}

struct ChooserInputElement<S: ChooserSpec> {
    view: Entity<Chooser<S>>,
}

impl<S: ChooserSpec> IntoElement for ChooserInputElement<S> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<S: ChooserSpec> Element for ChooserInputElement<S> {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).focus().clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        self.view
            .update(cx, |view, _| view.input_bounds = Some(bounds));
    }
}
