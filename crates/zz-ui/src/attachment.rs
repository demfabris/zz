//! Full-size preview for pasted images.

use std::sync::Arc;

use gpui::{App, Image, ImageSource, ObjectFit, RenderImage, Window, div, img, prelude::*, px};

use crate::WindowExt as _;

const ATTACHMENT_PREVIEW_WIDTH: f32 = 880.0;
const ATTACHMENT_PREVIEW_HEIGHT: f32 = 560.0;

/// Show `image` as large as the window allows, over a scrim.
pub fn open_attachment_preview(image: Arc<Image>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, _| {
        let viewport = window.viewport_size();
        let image = Arc::clone(&image);
        dialog
            .width(px(ATTACHMENT_PREVIEW_WIDTH).min(viewport.width * 0.9))
            .child(
                div()
                    .w_full()
                    .h(px(ATTACHMENT_PREVIEW_HEIGHT).min(viewport.height * 0.7))
                    .child(
                        img(ImageSource::Image(image))
                            .size_full()
                            .object_fit(ObjectFit::ScaleDown),
                    ),
            )
    });
}

pub fn open_render_image_preview(image: Arc<RenderImage>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, _| {
        let viewport = window.viewport_size();
        let image = Arc::clone(&image);
        dialog
            .width(px(ATTACHMENT_PREVIEW_WIDTH).min(viewport.width * 0.9))
            .child(
                div()
                    .w_full()
                    .h(px(ATTACHMENT_PREVIEW_HEIGHT).min(viewport.height * 0.7))
                    .child(
                        img(ImageSource::Render(image))
                            .size_full()
                            .object_fit(ObjectFit::ScaleDown),
                    ),
            )
    });
}
