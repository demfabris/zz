//! A custom-painted overlay bar that reads and drives a scroll handle.

use std::{cell::Cell, panic::Location, rc::Rc};

use gpui::{
    Anchor, App, Axis, Bounds, ContentMask, CursorStyle, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, ListState,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Position, ScrollHandle,
    ScrollWheelEvent, Size, Style, UniformListScrollHandle, Window, fill, point, px, relative,
    size, transparent_black,
};
use instant::{Duration, Instant};

use crate::ActiveTheme as _;
use crate::Colorize as _;

/// When a scrollbar makes itself visible.
pub use crate::ScrollbarShow;

/// Width of the scrollbar gutter, wider than the thumb + inset so the bar stays
/// easy to grab. Also the hit strip a hand-painted matching bar should reserve.
pub const WIDTH: Pixels = px(16.);
/// The thumb never shrinks below this, however long the content gets.
pub const MIN_THUMB_SIZE: f32 = 48.;

/// Resting thumb thickness (`Scrolling` mode).
pub const THUMB_WIDTH: Pixels = px(6.);
/// Gap between the thumb and the container edge.
pub const THUMB_INSET: Pixels = px(2.);

const THUMB_ACTIVE_WIDTH: Pixels = px(8.);
const THUMB_ACTIVE_INSET: Pixels = px(2.);

/// Thumb corner radius: the widget corner radius, capped at a pill.
#[must_use]
pub fn thumb_radius(thumb_width: Pixels, cx: &App) -> Pixels {
    cx.theme().radius.min(thumb_width / 2.)
}

const FADE_OUT_DURATION: f32 = 3.0;
const FADE_OUT_DELAY: f32 = 2.0;
const FADE_TICK: Duration = Duration::from_millis(33);
const DRAG_MIN_INTERVAL: Duration = Duration::from_millis(1000 / 120);

#[inline]
fn is_hover_mode(show: ScrollbarShow) -> bool {
    matches!(show, ScrollbarShow::Hover)
}

#[inline]
fn is_always_mode(show: ScrollbarShow) -> bool {
    matches!(show, ScrollbarShow::Always)
}

/// A scroll position source a [`Scrollbar`] can read and drive.
pub trait ScrollbarHandle: 'static {
    /// The current scroll offset. Offsets run negative as content scrolls away
    /// from the origin.
    fn offset(&self) -> Point<Pixels>;
    fn set_offset(&self, offset: Point<Pixels>);
    /// The full size of the content, including padding.
    fn content_size(&self) -> Size<Pixels>;
    fn start_drag(&self) {}
    fn end_drag(&self) {}
}

impl ScrollbarHandle for ScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        self.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        (self.max_offset() + self.bounds().size.into()).into()
    }
}

impl ScrollbarHandle for UniformListScrollHandle {
    fn offset(&self) -> Point<Pixels> {
        self.0.borrow().base_handle.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.0.borrow_mut().base_handle.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        let base_handle = &self.0.borrow().base_handle;
        (base_handle.max_offset() + base_handle.bounds().size.into()).into()
    }
}

impl ScrollbarHandle for ListState {
    fn offset(&self) -> Point<Pixels> {
        self.scroll_px_offset_for_scrollbar()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.set_offset_from_scrollbar(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        self.viewport_bounds().size + self.max_offset_for_scrollbar().into()
    }

    fn start_drag(&self) {
        self.scrollbar_drag_started();
    }

    fn end_drag(&self) {
        self.scrollbar_drag_ended();
    }
}

#[derive(Debug, Clone)]
struct ScrollbarState(Rc<Cell<ScrollbarStateInner>>);

impl ScrollbarState {
    fn get(&self) -> ScrollbarStateInner {
        self.0.get()
    }

    fn set(&self, state: ScrollbarStateInner) {
        self.0.set(state);
    }

    fn set_hovered(&self, axis: Option<Axis>) -> bool {
        let state = self.get();
        let changed = state.hovered_axis != axis;
        self.set(state.with_hovered(axis));
        changed
    }
}

#[derive(Debug, Clone, Copy)]
struct ScrollbarStateInner {
    hovered_axis: Option<Axis>,
    hovered_on_thumb: Option<Axis>,
    dragged_axis: Option<Axis>,
    drag_pos: Point<Pixels>,
    last_scroll_offset: Point<Pixels>,
    last_scroll_time: Option<Instant>,
    last_update: Instant,
    idle_timer_scheduled: bool,
}

impl Default for ScrollbarState {
    fn default() -> Self {
        Self(Rc::new(Cell::new(ScrollbarStateInner {
            hovered_axis: None,
            hovered_on_thumb: None,
            dragged_axis: None,
            drag_pos: point(px(0.), px(0.)),
            last_scroll_offset: point(px(0.), px(0.)),
            last_scroll_time: None,
            last_update: Instant::now(),
            idle_timer_scheduled: false,
        })))
    }
}

impl ScrollbarStateInner {
    fn with_drag_pos(&self, axis: Axis, pos: Point<Pixels>) -> Self {
        let mut state = *self;
        if axis == Axis::Vertical {
            state.drag_pos.y = pos.y;
        } else {
            state.drag_pos.x = pos.x;
        }

        state.dragged_axis = Some(axis);
        state
    }

    fn with_unset_drag_pos(&self) -> Self {
        let mut state = *self;
        state.dragged_axis = None;
        state
    }

    fn with_hovered(&self, axis: Option<Axis>) -> Self {
        let mut state = *self;
        state.hovered_axis = axis;
        if axis.is_some() {
            state.last_scroll_time = Some(Instant::now());
        }
        state
    }

    fn with_hovered_on_thumb(&self, axis: Option<Axis>) -> Self {
        let mut state = *self;
        state.hovered_on_thumb = axis;
        if self.is_scrollbar_visible() && axis.is_some() {
            state.last_scroll_time = Some(Instant::now());
        }
        state
    }

    fn with_last_scroll(
        &self,
        last_scroll_offset: Point<Pixels>,
        last_scroll_time: Option<Instant>,
    ) -> Self {
        let mut state = *self;
        state.last_scroll_offset = last_scroll_offset;
        state.last_scroll_time = last_scroll_time;
        state
    }

    fn with_last_scroll_time(&self, t: Option<Instant>) -> Self {
        let mut state = *self;
        state.last_scroll_time = t;
        state
    }

    fn with_last_update(&self, t: Instant) -> Self {
        let mut state = *self;
        state.last_update = t;
        state
    }

    fn with_idle_timer_scheduled(&self, scheduled: bool) -> Self {
        let mut state = *self;
        state.idle_timer_scheduled = scheduled;
        state
    }

    fn is_scrollbar_visible(&self) -> bool {
        if self.dragged_axis.is_some() {
            return true;
        }

        if let Some(last_time) = self.last_scroll_time {
            let elapsed = Instant::now().duration_since(last_time).as_secs_f32();
            elapsed < FADE_OUT_DURATION
        } else {
            false
        }
    }
}

fn schedule_fade_wake(
    state: &ScrollbarState,
    delay: Duration,
    view_id: gpui::EntityId,
    window: &Window,
    cx: &mut App,
) {
    if state.get().idle_timer_scheduled {
        return;
    }
    let state = state.clone();
    state.set(state.get().with_idle_timer_scheduled(true));
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            state.set(state.get().with_idle_timer_scheduled(false));
            cx.update(|_, cx| cx.notify(view_id)).ok();
        })
        .detach();
}

/// Which bars a [`Scrollbar`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarAxis {
    Vertical,
    Horizontal,
    Both,
}

impl From<Axis> for ScrollbarAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Vertical => Self::Vertical,
            Axis::Horizontal => Self::Horizontal,
        }
    }
}

impl ScrollbarAxis {
    /// Whether this is the vertical-only axis.
    #[inline]
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        matches!(self, Self::Vertical)
    }

    /// Whether both bars are drawn.
    #[inline]
    #[must_use]
    pub fn is_both(&self) -> bool {
        matches!(self, Self::Both)
    }

    #[inline]
    fn all(self) -> &'static [Axis] {
        match self {
            Self::Vertical => &[Axis::Vertical],
            Self::Horizontal => &[Axis::Horizontal],
            Self::Both => &[Axis::Horizontal, Axis::Vertical],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BarStyle {
    thumb_bg: Hsla,
    bar_bg: Hsla,
    thumb_width: Pixels,
    inset: Pixels,
    radius: Pixels,
}

fn resting_metrics(show: ScrollbarShow) -> (Pixels, Pixels) {
    match show {
        ScrollbarShow::Scrolling => (THUMB_WIDTH, THUMB_INSET),
        _ => (THUMB_ACTIVE_WIDTH, THUMB_ACTIVE_INSET),
    }
}

/// Scrollbar overlay for a scroll area, a uniform list, or a `ListState`.
pub struct Scrollbar {
    id: ElementId,
    axis: ScrollbarAxis,
    scrollbar_show: Option<ScrollbarShow>,
    scroll_handle: Rc<dyn ScrollbarHandle>,
}

impl Scrollbar {
    /// A scrollbar on both axes.
    #[track_caller]
    #[must_use]
    pub fn new<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::CodeLocation(*caller),
            axis: ScrollbarAxis::Both,
            scrollbar_show: None,
            scroll_handle: Rc::new(scroll_handle.clone()),
        }
    }

    #[track_caller]
    #[must_use]
    pub fn horizontal<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        Self::new(scroll_handle).axis(ScrollbarAxis::Horizontal)
    }

    #[track_caller]
    #[must_use]
    pub fn vertical<H: ScrollbarHandle + Clone>(scroll_handle: &H) -> Self {
        Self::new(scroll_handle).axis(ScrollbarAxis::Vertical)
    }

    /// Override the element id, which defaults to the construction site.
    #[must_use]
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Override the show mode. Defaults to `cx.theme().scrollbar_show`.
    #[must_use]
    pub fn scrollbar_show(mut self, scrollbar_show: ScrollbarShow) -> Self {
        self.scrollbar_show = Some(scrollbar_show);
        self
    }

    #[must_use]
    pub fn axis(mut self, axis: impl Into<ScrollbarAxis>) -> Self {
        self.axis = axis.into();
        self
    }

    fn show_mode(&self, cx: &App) -> ScrollbarShow {
        self.scrollbar_show.unwrap_or(cx.theme().scrollbar_show)
    }

    fn style_for_active_thumb(cx: &App) -> BarStyle {
        BarStyle {
            thumb_bg: cx.theme().foreground.glow().into(),
            bar_bg: cx.theme().transparent,
            thumb_width: THUMB_ACTIVE_WIDTH,
            inset: THUMB_ACTIVE_INSET,
            radius: thumb_radius(THUMB_ACTIVE_WIDTH, cx),
        }
    }

    fn style_for_hovered_bar(cx: &App) -> BarStyle {
        BarStyle {
            thumb_bg: cx.theme().foreground.wash().into(),
            bar_bg: cx.theme().transparent,
            thumb_width: THUMB_ACTIVE_WIDTH,
            inset: THUMB_ACTIVE_INSET,
            radius: thumb_radius(THUMB_ACTIVE_WIDTH, cx),
        }
    }

    fn style_for_normal(&self, cx: &App) -> BarStyle {
        let (thumb_width, inset) = resting_metrics(self.show_mode(cx));
        BarStyle {
            thumb_bg: cx.theme().foreground.wash().into(),
            bar_bg: cx.theme().transparent,
            thumb_width,
            inset,
            radius: thumb_radius(thumb_width, cx),
        }
    }

    fn style_for_idle(&self, cx: &App) -> BarStyle {
        let (thumb_width, inset) = resting_metrics(self.show_mode(cx));
        BarStyle {
            thumb_bg: transparent_black().into(),
            bar_bg: transparent_black(),
            thumb_width,
            inset,
            radius: thumb_radius(thumb_width, cx),
        }
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[doc(hidden)]
pub struct PrepaintState {
    hitbox: Hitbox,
    scrollbar_state: ScrollbarState,
    states: Vec<AxisPrepaintState>,
}

struct AxisPrepaintState {
    axis: Axis,
    bar_hitbox: Hitbox,
    bounds: Bounds<Pixels>,
    radius: Pixels,
    bg: Hsla,
    thumb_bounds: Bounds<Pixels>,
    thumb_fill_bounds: Bounds<Pixels>,
    thumb_bg: Hsla,
    scroll_size: Pixels,
    container_size: Pixels,
    thumb_size: Pixels,
    margin_end: Pixels,
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        });

        let state = window
            .use_state(cx, |_, _| ScrollbarState::default())
            .read(cx)
            .clone();

        let mut states = vec![];
        let mut has_both = self.axis.is_both();
        let scroll_size = self.scroll_handle.content_size();
        let scrollbar_show = self.show_mode(cx);

        for &axis in self.axis.all() {
            let is_vertical = axis == Axis::Vertical;
            let (scroll_area_size, container_size, scroll_position) = if is_vertical {
                (
                    scroll_size.height,
                    hitbox.size.height,
                    self.scroll_handle.offset().y,
                )
            } else {
                (
                    scroll_size.width,
                    hitbox.size.width,
                    self.scroll_handle.offset().x,
                )
            };

            let margin_end = if has_both && !is_vertical {
                WIDTH
            } else {
                px(0.)
            };

            if scroll_area_size <= container_size {
                has_both = false;
                continue;
            }

            let thumb_length =
                (container_size / scroll_area_size * container_size).max(px(MIN_THUMB_SIZE));
            let thumb_start = -(scroll_position / (scroll_area_size - container_size)
                * (container_size - margin_end - thumb_length));
            let thumb_end = (thumb_start + thumb_length).min(container_size - margin_end);

            let bounds = Bounds {
                origin: if is_vertical {
                    point(hitbox.origin.x + hitbox.size.width - WIDTH, hitbox.origin.y)
                } else {
                    point(
                        hitbox.origin.x,
                        hitbox.origin.y + hitbox.size.height - WIDTH,
                    )
                },
                size: if is_vertical {
                    size(WIDTH, hitbox.size.height)
                } else {
                    size(hitbox.size.width, WIDTH)
                },
            };

            let is_always_to_show = is_always_mode(scrollbar_show);
            let is_hover_to_show = is_hover_mode(scrollbar_show);
            let is_hovered_on_bar = state.get().hovered_axis == Some(axis);
            let is_hovered_on_thumb = state.get().hovered_on_thumb == Some(axis);
            let is_offset_changed = state.get().last_scroll_offset != self.scroll_handle.offset();

            let style = if state.get().dragged_axis == Some(axis) {
                Self::style_for_active_thumb(cx)
            } else if is_hover_to_show && (is_hovered_on_bar || is_hovered_on_thumb) {
                if is_hovered_on_thumb {
                    Self::style_for_active_thumb(cx)
                } else {
                    Self::style_for_hovered_bar(cx)
                }
            } else if is_offset_changed {
                self.style_for_normal(cx)
            } else if is_always_to_show {
                if is_hovered_on_thumb {
                    Self::style_for_active_thumb(cx)
                } else {
                    Self::style_for_hovered_bar(cx)
                }
            } else {
                let mut idle = self.style_for_idle(cx);
                if let Some(last_time) = state.get().last_scroll_time {
                    let elapsed = Instant::now().duration_since(last_time).as_secs_f32();
                    if is_hovered_on_bar {
                        state.set(state.get().with_last_scroll_time(Some(Instant::now())));
                        idle = if is_hovered_on_thumb {
                            Self::style_for_active_thumb(cx)
                        } else {
                            Self::style_for_hovered_bar(cx)
                        };
                    } else if elapsed < FADE_OUT_DELAY {
                        idle.thumb_bg = cx.theme().foreground.wash().into();

                        schedule_fade_wake(
                            &state,
                            Duration::from_secs_f32(FADE_OUT_DELAY - elapsed),
                            window.current_view(),
                            window,
                            cx,
                        );
                    } else if elapsed < FADE_OUT_DURATION {
                        let opacity = 1.0 - (elapsed - FADE_OUT_DELAY).powi(10);
                        idle.thumb_bg = cx.theme().foreground.wash().opacity(opacity);

                        schedule_fade_wake(&state, FADE_TICK, window.current_view(), window, cx);
                    }
                }

                idle
            };

            let inset = style.inset;

            let thumb_length = thumb_end - thumb_start - inset * 2;
            let thumb_bounds = if is_vertical {
                Bounds::from_anchor_and_size(
                    Anchor::TopRight,
                    bounds.top_right() + point(-inset, inset + thumb_start),
                    size(WIDTH, thumb_length),
                )
            } else {
                Bounds::from_anchor_and_size(
                    Anchor::BottomLeft,
                    bounds.bottom_left() + point(inset + thumb_start, -inset),
                    size(thumb_length, WIDTH),
                )
            };

            let thumb_fill_bounds = if is_vertical {
                Bounds::from_anchor_and_size(
                    Anchor::TopRight,
                    bounds.top_right() + point(-inset, inset + thumb_start),
                    size(style.thumb_width, thumb_length),
                )
            } else {
                Bounds::from_anchor_and_size(
                    Anchor::BottomLeft,
                    bounds.bottom_left() + point(inset + thumb_start, -inset),
                    size(thumb_length, style.thumb_width),
                )
            };

            let bar_hitbox = window.with_content_mask(Some(ContentMask { bounds }), |window| {
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            });

            states.push(AxisPrepaintState {
                axis,
                bar_hitbox,
                bounds,
                radius: style.radius,
                bg: style.bar_bg,
                thumb_bounds,
                thumb_fill_bounds,
                thumb_bg: style.thumb_bg,
                scroll_size: scroll_area_size,
                container_size,
                thumb_size: thumb_length,
                margin_end,
            });
        }

        PrepaintState {
            hitbox,
            states,
            scrollbar_state: state,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scrollbar_state = &prepaint.scrollbar_state;
        let scrollbar_show = self.show_mode(cx);
        let view_id = window.current_view();
        let hitbox_bounds = prepaint.hitbox.bounds;

        if self.scroll_handle.offset() != scrollbar_state.get().last_scroll_offset {
            scrollbar_state.set(
                scrollbar_state
                    .get()
                    .with_last_scroll(self.scroll_handle.offset(), Some(Instant::now())),
            );
            schedule_fade_wake(
                scrollbar_state,
                Duration::from_secs_f32(FADE_OUT_DELAY),
                view_id,
                window,
                cx,
            );
        }

        let is_visible =
            scrollbar_state.get().is_scrollbar_visible() || is_always_mode(scrollbar_show);
        let is_hover_to_show = is_hover_mode(scrollbar_show);

        window.with_content_mask(
            Some(ContentMask {
                bounds: hitbox_bounds,
            }),
            |window| {
                for state in &prepaint.states {
                    let axis = state.axis;
                    let radius = state.radius;
                    let bounds = state.bounds;
                    let thumb_bounds = state.thumb_bounds;
                    let scroll_area_size = state.scroll_size;
                    let container_size = state.container_size;
                    let thumb_size = state.thumb_size;
                    let margin_end = state.margin_end;
                    let is_vertical = axis == Axis::Vertical;

                    window.set_cursor_style(CursorStyle::default(), &state.bar_hitbox);

                    window.paint_layer(hitbox_bounds, |window| {
                        window.paint_quad(fill(state.bounds, state.bg));
                        window.paint_quad(
                            fill(state.thumb_fill_bounds, state.thumb_bg).corner_radii(radius),
                        );
                    });

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |event: &ScrollWheelEvent, phase, window, cx| {
                            if phase.bubble()
                                && hitbox_bounds.contains(&event.position)
                                && scroll_handle.offset() != state.get().last_scroll_offset
                            {
                                state.set(state.get().with_last_scroll(
                                    scroll_handle.offset(),
                                    Some(Instant::now()),
                                ));
                                schedule_fade_wake(
                                    &state,
                                    Duration::from_secs_f32(FADE_OUT_DELAY),
                                    view_id,
                                    window,
                                    cx,
                                );
                            }
                        }
                    });

                    let safe_range = (-scroll_area_size + container_size)..px(0.);

                    if is_hover_to_show || is_visible {
                        window.on_mouse_event({
                            let state = scrollbar_state.clone();
                            let scroll_handle = self.scroll_handle.clone();

                            move |event: &MouseDownEvent, phase, _, cx| {
                                if phase.bubble() && bounds.contains(&event.position) {
                                    cx.stop_propagation();

                                    if thumb_bounds.contains(&event.position) {
                                        let pos = event.position - thumb_bounds.origin;

                                        scroll_handle.start_drag();
                                        state.set(state.get().with_drag_pos(axis, pos));

                                        cx.notify(view_id);
                                    } else {
                                        let offset = scroll_handle.offset();
                                        let percentage = if is_vertical {
                                            (event.position.y - thumb_size / 2. - bounds.origin.y)
                                                / (bounds.size.height - thumb_size)
                                        } else {
                                            (event.position.x - thumb_size / 2. - bounds.origin.x)
                                                / (bounds.size.width - thumb_size)
                                        }
                                        .min(1.);

                                        if is_vertical {
                                            scroll_handle.set_offset(point(
                                                offset.x,
                                                (-scroll_area_size * percentage)
                                                    .clamp(safe_range.start, safe_range.end),
                                            ));
                                        } else {
                                            scroll_handle.set_offset(point(
                                                (-scroll_area_size * percentage)
                                                    .clamp(safe_range.start, safe_range.end),
                                                offset.y,
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                    }

                    window.on_mouse_event({
                        let scroll_handle = self.scroll_handle.clone();
                        let state = scrollbar_state.clone();

                        move |event: &MouseMoveEvent, _, _, cx| {
                            let mut notify = false;
                            let need_hover_to_update = is_hover_to_show || is_visible;
                            if bounds.contains(&event.position) && need_hover_to_update {
                                notify |= state.set_hovered(Some(axis));
                            } else if state.get().hovered_axis == Some(axis) {
                                notify |= state.set_hovered(None);
                            }

                            if thumb_bounds.contains(&event.position) {
                                if state.get().hovered_on_thumb != Some(axis) {
                                    state.set(state.get().with_hovered_on_thumb(Some(axis)));
                                    notify = true;
                                }
                            } else if state.get().hovered_on_thumb == Some(axis) {
                                state.set(state.get().with_hovered_on_thumb(None));
                                notify = true;
                            }

                            if state.get().dragged_axis == Some(axis) && event.dragging() {
                                cx.stop_propagation();

                                let drag_pos = state.get().drag_pos;

                                let percentage = (if is_vertical {
                                    (event.position.y - drag_pos.y - bounds.origin.y)
                                        / (bounds.size.height - thumb_size)
                                } else {
                                    (event.position.x - drag_pos.x - bounds.origin.x)
                                        / (bounds.size.width - thumb_size - margin_end)
                                })
                                .clamp(0., 1.);

                                let offset = if is_vertical {
                                    point(
                                        scroll_handle.offset().x,
                                        (-(scroll_area_size - container_size) * percentage)
                                            .clamp(safe_range.start, safe_range.end),
                                    )
                                } else {
                                    point(
                                        (-(scroll_area_size - container_size) * percentage)
                                            .clamp(safe_range.start, safe_range.end),
                                        scroll_handle.offset().y,
                                    )
                                };

                                if ((scroll_handle.offset().y - offset.y).abs() > px(1.)
                                    || (scroll_handle.offset().x - offset.x).abs() > px(1.))
                                    && state.get().last_update.elapsed() > DRAG_MIN_INTERVAL
                                {
                                    scroll_handle.set_offset(offset);
                                    state.set(state.get().with_last_update(Instant::now()));
                                    notify = true;
                                }
                            }

                            if notify {
                                cx.notify(view_id);
                            }
                        }
                    });

                    window.on_mouse_event({
                        let state = scrollbar_state.clone();
                        let scroll_handle = self.scroll_handle.clone();

                        move |_event: &MouseUpEvent, phase, _, cx| {
                            if phase.bubble() {
                                scroll_handle.end_drag();
                                state.set(state.get().with_unset_drag_pos());
                                cx.notify(view_id);
                            }
                        }
                    });
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Axis, ScrollbarState};

    #[test]
    fn hovered_axis_only_changes_on_track_transition() {
        let state = ScrollbarState::default();

        assert!(state.set_hovered(Some(Axis::Vertical)));
        assert!(!state.set_hovered(Some(Axis::Vertical)));
        assert!(state.set_hovered(Some(Axis::Horizontal)));
        assert!(state.set_hovered(None));
        assert!(!state.set_hovered(None));
    }
}
