//! Hover tooltip: a label, optionally trailed by a keybinding.

use gpui::{
    Action, AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render,
    SharedString, StyleRefinement, Styled, Window, div, prelude::FluentBuilder,
};

use crate::Colorize as _;
use crate::{ActiveTheme as _, StyledExt as _, h_flex, kbd::Kbd};

/// The box that appears on hover: a label, optionally trailed by the keybinding
/// of the action it triggers. Attach one through gpui's `.tooltip()`.
pub struct Tooltip {
    style: StyleRefinement,
    text: SharedString,
    action: Option<(Box<dyn Action>, Option<SharedString>)>,
}

impl Tooltip {
    #[must_use]
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            style: StyleRefinement::default(),
            text: text.into(),
            action: None,
        }
    }

    /// Trail the label with the keystroke bound to `action` in `context`, or
    /// globally when `None`. An unbound action renders nothing extra.
    #[must_use]
    pub fn action(mut self, action: &dyn Action, context: Option<&str>) -> Self {
        self.action = Some((action.boxed_clone(), context.map(SharedString::new)));
        self
    }

    /// Build the tooltip into the `AnyView` gpui's hover machinery expects.
    #[must_use]
    pub fn build(self, _window: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}

impl FluentBuilder for Tooltip {}

impl Styled for Tooltip {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key_binding = self.action.as_ref().and_then(|(action, context)| {
            Kbd::binding_for_action(action.as_ref(), context.as_deref(), window)
        });

        div().child(
            h_flex()
                .m_3()
                .px_2()
                .py_0p5()
                .gap_3()
                .justify_between()
                .font_family(cx.theme().font_family.clone())
                .text_xs()
                .bg(cx.theme().background.raised(1).opaque())
                .text_color(cx.theme().foreground)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .shadow_md()
                .refine_style(&self.style)
                .child(div().child(self.text.clone()))
                .when_some(key_binding, |this, kbd| {
                    this.child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().foreground.muted())
                            .child(kbd.p_0().border_0().min_w_0().bg(cx.theme().transparent)),
                    )
                }),
        )
    }
}
