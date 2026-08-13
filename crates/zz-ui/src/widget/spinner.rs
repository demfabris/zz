//! Indeterminate loading indicator.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, Hsla, IntoElement, ParentElement as _, RenderOnce,
    Transformation, Window, div, ease_in_out, percentage, prelude::FluentBuilder as _,
};

use crate::{Icon, IconName, Sizable, Size};

const ROTATION_PERIOD: Duration = Duration::from_millis(800);

#[derive(IntoElement)]
pub struct Spinner {
    size: Size,
    color: Option<Hsla>,
}

impl Spinner {
    /// A spinner at the default (medium) size, inheriting the ambient text color.
    #[must_use]
    pub fn new() -> Self {
        Self {
            size: Size::Medium,
            color: None,
        }
    }

    /// Tint the spinner. Defaults to the inherited text color.
    #[must_use]
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Sizable for Spinner {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div().child(
            Icon::new(IconName::Loader)
                .with_size(self.size)
                .when_some(self.color, gpui::Styled::text_color)
                .with_animation(
                    "spinner-rotation",
                    Animation::new(ROTATION_PERIOD)
                        .repeat()
                        .with_easing(ease_in_out),
                    |this, delta| this.transform(Transformation::rotate(percentage(delta))),
                ),
        )
    }
}
