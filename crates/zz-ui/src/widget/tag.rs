//! Small inline status pill.

use gpui::{
    AnyElement, App, Hsla, InteractiveElement as _, IntoElement, ParentElement, RenderOnce,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _, relative, transparent_white,
};

use crate::Colorize as _;
use crate::{ActiveTheme as _, Sizable, Size, StyledExt as _};

/// Color role of a [`Tag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TagVariant {
    Primary,
    #[default]
    Secondary,
    Success,
}

impl TagVariant {
    fn bg(self, cx: &App) -> Hsla {
        match self {
            Self::Primary => cx.theme().foreground,
            Self::Secondary => cx.theme().background.raised(2),
            Self::Success => cx.theme().success,
        }
    }

    fn border(self, cx: &App) -> Hsla {
        match self {
            Self::Primary => cx.theme().foreground,
            Self::Secondary => cx.theme().border,
            Self::Success => cx.theme().success,
        }
    }

    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant/outline pair keeps the table readable; two landing on `foreground` for unrelated reasons is a coincidence of the monochrome chrome, not a shared case"
    )]
    fn fg(self, outline: bool, cx: &App) -> Hsla {
        match (self, outline) {
            (Self::Primary, false) => cx.theme().foreground.on(),
            (Self::Primary, true) => cx.theme().foreground,
            (Self::Secondary, false) => cx.theme().foreground,
            (Self::Secondary, true) => cx.theme().foreground.muted(),
            (Self::Success, false) => cx.theme().success.on(),
            (Self::Success, true) => cx.theme().success,
        }
    }
}

/// A small status indicator. Honors [`Size::XSmall`] and [`Size::Small`] as a
/// single compact form; anything larger renders at the default size.
#[derive(IntoElement)]
pub struct Tag {
    style: StyleRefinement,
    variant: TagVariant,
    outline: bool,
    size: Size,
    children: Vec<AnyElement>,
}

impl Tag {
    /// A tag with the default ([`TagVariant::Secondary`]) variant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            variant: TagVariant::default(),
            outline: false,
            size: Size::default(),
            children: Vec::new(),
        }
    }

    #[must_use]
    pub fn primary() -> Self {
        Self::new().with_variant(TagVariant::Primary)
    }

    #[must_use]
    pub fn secondary() -> Self {
        Self::new().with_variant(TagVariant::Secondary)
    }

    #[must_use]
    pub fn success() -> Self {
        Self::new().with_variant(TagVariant::Success)
    }

    #[must_use]
    pub fn with_variant(mut self, variant: TagVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Drop the fill, keeping only the border and colored text.
    #[must_use]
    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }
}

impl Default for Tag {
    fn default() -> Self {
        Self::new()
    }
}

impl Sizable for Tag {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl ParentElement for Tag {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Tag {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tag {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let compact = matches!(self.size, Size::XSmall | Size::Small);
        let bg = if self.outline {
            transparent_white()
        } else {
            self.variant.bg(cx)
        };
        div()
            .flex()
            .items_center()
            .border_1()
            .line_height(relative(1.))
            .text_xs()
            .map(|this| {
                if compact {
                    this.px_1p5().py_0p5()
                } else {
                    this.px_2p5().py_1()
                }
            })
            .bg(bg)
            .text_color(self.variant.fg(self.outline, cx))
            .border_color(self.variant.border(cx))
            .rounded(cx.theme().radius)
            .hover(|this| this.opacity(0.9))
            .refine_style(&self.style)
            .children(self.children)
    }
}
