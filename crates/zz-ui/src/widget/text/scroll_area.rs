//! A horizontal scroll viewport that lets vertical wheel events keep bubbling.
#![allow(clippy::pedantic, clippy::style, clippy::complexity)]

use gpui::{
    App, Bounds, ContentMask, Element, ElementId, GlobalElementId, Hitbox, InspectorElementId,
    InteractiveElement as _, IntoElement, IsZero as _, LayoutId, ParentElement as _, Pixels, Point,
    Position, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement as _, Style,
    StyleRefinement, Styled as _, Window, div, px, relative,
};

use crate::StyledExt as _;

pub(super) fn horizontal_scroll_area(
    id: impl Into<ElementId>,
    scroll_handle: &ScrollHandle,
    style: &StyleRefinement,
    child: impl IntoElement,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .relative()
        .refine_style(style)
        .overflow_hidden()
        .track_scroll(scroll_handle)
        .child(child)
        .child(HorizontalScrollMask {
            scroll_handle: scroll_handle.clone(),
        })
}

struct HorizontalScrollMask {
    scroll_handle: ScrollHandle,
}

impl IntoElement for HorizontalScrollMask {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for HorizontalScrollMask {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
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
        _: &mut App,
    ) -> Self::PrepaintState {
        let cover_bounds = Bounds {
            origin: Point {
                x: bounds.origin.x,
                y: bounds.origin.y - bounds.size.height,
            },
            size: bounds.size,
        };

        window.insert_hitbox(cover_bounds, gpui::HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let line_height = window.line_height();
        let bounds = hitbox.bounds;

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.on_mouse_event({
                let view_id = window.current_view();
                let scroll_handle = self.scroll_handle.clone();

                move |event: &ScrollWheelEvent, phase, _, cx| {
                    if !(bounds.contains(&event.position) && phase.bubble()) {
                        return;
                    }

                    let mut offset = scroll_handle.offset();
                    let mut delta = event.delta.pixel_delta(line_height);

                    if !delta.x.is_zero() && !delta.y.is_zero() {
                        if delta.x.abs() > delta.y.abs() {
                            delta.y = px(0.);
                        } else {
                            delta.x = px(0.);
                        }
                    }

                    offset.x += delta.x;

                    if offset != scroll_handle.offset() {
                        scroll_handle.set_offset(offset);
                        cx.notify(view_id);
                        cx.stop_propagation();
                    }
                }
            });
        });
    }
}
