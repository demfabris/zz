//! A selectable row for list-shaped surfaces.

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, RenderOnce, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _,
};

use crate::Colorize as _;
use crate::{ActiveTheme as _, Disableable, Selectable, StyledExt as _, h_flex};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type MouseDownHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;
type MouseMoveHandler = Box<dyn Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static>;

/// One row of a list: a stateful horizontal flex box that hovers, selects and
/// clicks. [`Styled`], so a call site overrides height, padding, radius, cursor
/// and background as it would on a `div()`.
#[derive(IntoElement)]
pub struct ListItem {
    base: Stateful<Div>,
    style: StyleRefinement,
    disabled: bool,
    selected: bool,
    on_click: Option<ClickHandler>,
    on_mouse_down: Vec<(MouseButton, MouseDownHandler)>,
    on_mouse_enter: Option<MouseMoveHandler>,
    children: Vec<AnyElement>,
}

impl ListItem {
    /// A row identified by `id`, which must be unique among its siblings.
    #[must_use]
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: h_flex().id(id),
            style: StyleRefinement::default(),
            disabled: false,
            selected: false,
            on_click: None,
            on_mouse_down: Vec::new(),
            on_mouse_enter: None,
            children: Vec::new(),
        }
    }

    /// Render the row as selected. Inherent mirror of [`Selectable::selected`],
    /// so a call site does not have to import the trait.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Handle a click on the row. Ignored while the row is disabled.
    #[must_use]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Handle a press of `button`, carrying the click count that
    /// [`Self::on_click`] drops. Ignored while disabled; handlers accumulate.
    #[must_use]
    pub fn on_mouse_down(
        mut self,
        button: MouseButton,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_mouse_down.push((button, Box::new(handler)));
        self
    }

    /// Handle the pointer moving over the row, for lists that follow the mouse
    /// with their selection. Ignored while the row is disabled.
    #[must_use]
    pub fn on_mouse_enter(
        mut self,
        handler: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_mouse_enter = Some(Box::new(handler));
        self
    }
}

impl Disableable for ListItem {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Selectable for ListItem {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl Styled for ListItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for ListItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            base,
            style,
            disabled,
            selected,
            on_click,
            on_mouse_down,
            on_mouse_enter,
            children,
        } = self;

        let interactive = !disabled;
        let highlighted = interactive && selected;

        let foreground = cx.theme().foreground;
        let muted_foreground = cx.theme().foreground.muted();
        let hover_bg = cx.theme().background.hover();
        let active_bg = cx.theme().foreground.wash();
        let active_border = cx.theme().foreground;

        let mut corner_radii = style.corner_radii.clone();
        let radius = cx.theme().radius.into();
        corner_radii.top_left.get_or_insert(radius);
        corner_radii.top_right.get_or_insert(radius);
        corner_radii.bottom_left.get_or_insert(radius);
        corner_radii.bottom_right.get_or_insert(radius);
        let outline_style = StyleRefinement {
            corner_radii,
            ..StyleRefinement::default()
        };

        base.relative()
            .justify_between()
            .gap_x_1()
            .py_1()
            .px_3()
            .text_base()
            .text_color(foreground)
            .rounded(cx.theme().radius)
            .refine_style(&style)
            .when(interactive, |this| {
                this.when_some(on_click, |this, on_click| this.on_click(on_click))
                    .when_some(on_mouse_enter, |this, on_mouse_enter| {
                        this.on_mouse_move(on_mouse_enter)
                    })
                    .map(|this| {
                        on_mouse_down
                            .into_iter()
                            .fold(this, |this, (button, handler)| {
                                this.on_mouse_down(button, handler)
                            })
                    })
                    .when(!selected, |this| this.hover(|this| this.bg(hover_bg)))
            })
            .when(!interactive, |this| this.text_color(muted_foreground))
            .child(div().w_full().children(children))
            .when(highlighted, |this| {
                this.bg(active_bg).child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .border_1()
                        .border_color(active_border)
                        .refine_style(&outline_style),
                )
            })
    }
}
