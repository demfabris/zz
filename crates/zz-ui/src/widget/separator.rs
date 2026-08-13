//! Thin divider rule.

use gpui::{
    App, Axis, Hsla, IntoElement, ParentElement as _, RenderOnce, StyleRefinement, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, StyledExt as _};

/// A one-pixel divider rule. The rule is an absolutely positioned child, so the
/// separator takes no space along its own axis; only its margins do.
#[derive(IntoElement)]
pub struct Separator {
    style: StyleRefinement,
    color: Option<Hsla>,
    axis: Axis,
}

impl Separator {
    /// A vertical rule, filling the height of its container.
    #[must_use]
    pub fn vertical() -> Self {
        Self {
            style: StyleRefinement::default(),
            color: None,
            axis: Axis::Vertical,
        }
    }

    /// A horizontal rule, filling the width of its container.
    #[must_use]
    pub fn horizontal() -> Self {
        Self {
            axis: Axis::Horizontal,
            ..Self::vertical()
        }
    }

    /// Override the rule color. Defaults to the theme border.
    #[must_use]
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }
}

impl Styled for Separator {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Separator {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().border);
        let vertical = self.axis == Axis::Vertical;

        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .map(|this| {
                if vertical {
                    this.h_full()
                } else {
                    this.w_full()
                }
            })
            .refine_style(&self.style)
            .child(div().absolute().bg(color).map(|this| {
                if vertical {
                    this.w(px(1.)).h_full()
                } else {
                    this.h(px(1.)).w_full()
                }
            }))
    }
}
