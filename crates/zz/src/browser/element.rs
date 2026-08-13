use std::sync::Arc;

#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
use gpui::{
    App, Bounds, ContentMask, Corners, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, RenderImage, Window,
};

use crate::{browser::view::BrowserView, diagnostics, pane};

const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::browser_render";

pub(crate) struct BrowserElement {
    view: Entity<BrowserView>,
    image: Option<Arc<RenderImage>>,
    #[cfg(target_os = "macos")]
    surface: Option<CVPixelBuffer>,
    corner_radii: Corners<Pixels>,
}

impl BrowserElement {
    pub(crate) fn new(
        view: Entity<BrowserView>,
        image: Option<Arc<RenderImage>>,
        corner_radii: Corners<Pixels>,
    ) -> Self {
        Self {
            view,
            image,
            #[cfg(target_os = "macos")]
            surface: None,
            corner_radii,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn surface(mut self, surface: Option<CVPixelBuffer>) -> Self {
        self.surface = surface;
        self
    }
}

impl IntoElement for BrowserElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(crate) struct BrowserPaintState {
    image: Option<Arc<RenderImage>>,
    #[cfg(target_os = "macos")]
    surface: Option<CVPixelBuffer>,
}

impl Element for BrowserElement {
    type RequestLayoutState = ();
    type PrepaintState = BrowserPaintState;

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
        (pane::fill_parent(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        self.view.update(cx, |view, cx| {
            view.update_content_bounds(bounds, window, cx);
        });
        log::trace!(
            target: "zz::diagnostics::browser_render",
            "prepaint bounds={bounds:?} has_image={} image_strong_count={:?} scale_factor={} elapsed_us={}",
            self.image.is_some(),
            self.image.as_ref().map(Arc::strong_count),
            window.scale_factor(),
            diagnostics::elapsed_us(started),
        );
        BrowserPaintState {
            image: self.image.clone(),
            #[cfg(target_os = "macos")]
            surface: self.surface.clone(),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let had_image = state.image.is_some();
        let image_strong_count = state.image.as_ref().map(Arc::strong_count);
        let focus = self.view.read(cx).focus_handle();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            #[cfg(target_os = "macos")]
            if let Some(surface) = state.surface.take() {
                window.paint_surface(bounds, self.corner_radii, surface);
                return;
            }
            if let Some(image) = state.image.take()
                && let Err(error) =
                    window.paint_image(bounds, bounds, self.corner_radii, image, 0, false)
            {
                log::warn!("failed to paint browser frame: {error}");
            }
        });
        log::trace!(
            target: "zz::diagnostics::browser_render",
            "paint bounds={bounds:?} had_image={had_image} image_strong_count={image_strong_count:?} focused={} elapsed_us={}",
            focus.is_focused(window),
            diagnostics::elapsed_us(started),
        );
    }
}
