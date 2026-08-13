//! A floating panel anchored to a trigger element.

mod actions;

use gpui::{
    Anchor, AnyElement, App, Bounds, Context, DismissEvent, ElementId, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyBinding, MouseButton, ParentElement as _,
    Pixels, Point, Render, RenderOnce, StyleRefinement, Styled, Subscription, Window, anchored,
    canvas, deferred, div, prelude::FluentBuilder as _, px,
};

use crate::{Selectable, StyledExt as _, v_flex};
use actions::Cancel;

const CONTEXT: &str = "ZzPopover";

const WINDOW_MARGIN: Pixels = px(8.);

type ContentBuilder =
    Box<dyn Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> AnyElement>;

type TriggerBuilder = Box<dyn FnOnce(bool) -> AnyElement>;

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Cancel, Some(CONTEXT))]);
}

/// A floating panel opened by a trigger element. It renders deferred and
/// anchored, so it escapes the trigger's clip rect and stays inside the window.
/// Open state is keyed by the element id, so it survives across renders.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    appearance: bool,
    overlay_closable: bool,
    trigger: Option<TriggerBuilder>,
    content: Option<ContentBuilder>,
}

impl Popover {
    /// A closed popover, anchored top-left, with the default panel chrome.
    /// Without a [`Popover::trigger`] it renders an empty element.
    #[must_use]
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            anchor: Anchor::TopLeft,
            appearance: true,
            overlay_closable: true,
            trigger: None,
            content: None,
        }
    }

    /// Set the anchor corner of the panel, default [`Anchor::TopLeft`]. The
    /// panel hangs off that corner of the trigger and snaps inward at the
    /// window edge.
    #[must_use]
    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    /// Set the element that opens the popover on left mouse down. It renders
    /// selected while the panel is open.
    #[must_use]
    pub fn trigger<T>(mut self, trigger: T) -> Self
    where
        T: Selectable + IntoElement + 'static,
    {
        self.trigger = Some(Box::new(|is_open| {
            let selected = trigger.is_selected();
            trigger.selected(selected || is_open).into_any_element()
        }));
        self
    }

    /// Accepted for call-site compatibility, and ignored.
    #[must_use]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "signature parity with the upstream builder this replaces"
    )]
    pub fn trigger_style(self, _style: StyleRefinement) -> Self {
        self
    }

    /// Set the builder for the panel's content. It runs on every render, so
    /// create entities once and stash them in state.
    #[must_use]
    pub fn content<F, E>(mut self, content: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> E + 'static,
    {
        self.content = Some(Box::new(move |state, window, cx| {
            content(state, window, cx).into_any_element()
        }));
        self
    }

    /// Set whether the panel draws its own chrome, default `true`. With `false`
    /// it gets no background, border, shadow or padding.
    #[must_use]
    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    /// Set whether a click outside the panel dismisses it, default `true`. Turn
    /// it off when the content handles outside clicks itself.
    #[must_use]
    pub fn overlay_closable(mut self, closable: bool) -> Self {
        self.overlay_closable = closable;
        self
    }
}

impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn resolved_corner(anchor: Anchor, trigger: Bounds<Pixels>) -> Point<Pixels> {
    let above = trigger.origin.y - trigger.size.height;
    match anchor {
        Anchor::TopCenter => trigger.top_center(),
        Anchor::TopRight => trigger.top_right(),
        Anchor::BottomLeft => Point {
            x: trigger.origin.x,
            y: above,
        },
        Anchor::BottomCenter => Point {
            x: trigger.top_center().x,
            y: above,
        },
        Anchor::BottomRight => Point {
            x: trigger.top_right().x,
            y: above,
        },
        _ => trigger.origin,
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| PopoverState::new(cx));

        let (open, focus_handle, trigger_bounds, trigger_bounds_captured) = {
            let state = state.read(cx);
            (
                state.open,
                state.focus_handle.clone(),
                state.trigger_bounds,
                state.trigger_bounds_captured,
            )
        };

        let Some(trigger) = self.trigger else {
            return div().id("empty");
        };

        let parent_view_id = window.current_view();

        let el = div()
            .id(self.id)
            .child(trigger(open))
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                move |_, window, cx| {
                    cx.stop_propagation();
                    state.update(cx, |state, cx| {
                        state.open = open;
                        state.toggle_open(window, cx);
                    });
                    cx.notify(parent_view_id);
                }
            })
            .child(
                canvas(
                    {
                        let state = state.clone();
                        move |bounds, window, cx| {
                            let first_capture = state.update(cx, |state, _| {
                                let first = !state.trigger_bounds_captured;
                                state.trigger_bounds = bounds;
                                state.trigger_bounds_captured = true;
                                first
                            });
                            if first_capture {
                                window.request_animation_frame();
                            }
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            );

        if !open || !trigger_bounds_captured {
            return el;
        }

        let panel = v_flex()
            .id("content")
            .occlude()
            .tab_group()
            .when(self.appearance, |this| this.popover_style(cx).p_3())
            .map(|this| match self.anchor {
                Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight => this.bottom_1(),
                _ => this.top_1(),
            });

        let panel = panel
            .track_focus(&focus_handle)
            .key_context(CONTEXT)
            .on_action(window.listener_for(&state, PopoverState::on_action_cancel))
            .when_some(self.content, |this, content| {
                this.child(state.update(cx, |state, cx| content(state, window, cx)))
            })
            .when(self.overlay_closable, |this| {
                this.on_mouse_down_out({
                    let state = state.clone();
                    move |_, window, cx| {
                        state.update(cx, |state, cx| state.dismiss(window, cx));
                        cx.notify(parent_view_id);
                    }
                })
            })
            .refine_style(&self.style);

        el.child(
            deferred(
                anchored()
                    .snap_to_window_with_margin(WINDOW_MARGIN)
                    .anchor(self.anchor)
                    .position(resolved_corner(self.anchor, trigger_bounds))
                    .child(div().relative().child(panel)),
            )
            .with_priority(1),
        )
    }
}

/// The open state behind one [`Popover`], keyed by its element id. A
/// [`Popover::content`] builder receives it, and can close the popover with it.
pub struct PopoverState {
    focus_handle: FocusHandle,
    previous_focus_handle: Option<FocusHandle>,
    trigger_bounds: Bounds<Pixels>,
    trigger_bounds_captured: bool,
    open: bool,
    dismiss_subscription: Option<Subscription>,
}

impl PopoverState {
    fn new(cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            previous_focus_handle: None,
            trigger_bounds: Bounds::default(),
            trigger_bounds_captured: false,
            open: false,
            dismiss_subscription: None,
        }
    }

    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.toggle_open(window, cx);
        }
    }

    fn toggle_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = !self.open;

        if self.open {
            self.previous_focus_handle = window.focused(cx);
            self.focus_handle.focus(window, cx);

            let state = cx.entity();
            self.dismiss_subscription =
                Some(
                    window.subscribe(&cx.entity(), cx, move |_, _: &DismissEvent, window, cx| {
                        state.update(cx, |state, cx| state.dismiss(window, cx));
                        window.refresh();
                    }),
                );
        } else {
            self.dismiss_subscription = None;
            if let Some(prev) = self.previous_focus_handle.take()
                && self.focus_handle.contains_focused(window, cx)
            {
                prev.focus(window, cx);
            }
        }

        cx.notify();
    }

    fn on_action_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(window, cx);
    }
}

impl Focusable for PopoverState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PopoverState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl EventEmitter<DismissEvent> for PopoverState {}

#[cfg(test)]
mod tests {
    use gpui::size;

    use super::*;

    fn trigger_bounds() -> Bounds<Pixels> {
        Bounds {
            origin: Point {
                x: px(100.),
                y: px(100.),
            },
            size: size(px(200.), px(50.)),
        }
    }

    fn point(x: f32, y: f32) -> Point<Pixels> {
        Point { x: px(x), y: px(y) }
    }

    #[test]
    fn top_anchors_sit_on_the_trigger_top_edge() {
        let bounds = trigger_bounds();

        assert_eq!(resolved_corner(Anchor::TopLeft, bounds), point(100., 100.));
        assert_eq!(
            resolved_corner(Anchor::TopCenter, bounds),
            point(200., 100.)
        );
        assert_eq!(resolved_corner(Anchor::TopRight, bounds), point(300., 100.));
    }

    #[test]
    fn bottom_anchors_sit_one_trigger_height_above_it() {
        let bounds = trigger_bounds();

        assert_eq!(
            resolved_corner(Anchor::BottomLeft, bounds),
            point(100., 50.)
        );
        assert_eq!(
            resolved_corner(Anchor::BottomCenter, bounds),
            point(200., 50.)
        );
        assert_eq!(
            resolved_corner(Anchor::BottomRight, bounds),
            point(300., 50.)
        );
    }

    #[test]
    fn centered_anchors_fall_back_to_the_trigger_origin() {
        let bounds = trigger_bounds();

        assert_eq!(resolved_corner(Anchor::LeftCenter, bounds), bounds.origin);
        assert_eq!(resolved_corner(Anchor::RightCenter, bounds), bounds.origin);
    }

    #[test]
    fn a_zero_sized_trigger_anchors_at_its_origin() {
        let bounds = Bounds {
            origin: point(10., 20.),
            size: size(px(0.), px(0.)),
        };

        assert_eq!(resolved_corner(Anchor::TopCenter, bounds), bounds.origin);
        assert_eq!(resolved_corner(Anchor::BottomLeft, bounds), bounds.origin);
    }
}
